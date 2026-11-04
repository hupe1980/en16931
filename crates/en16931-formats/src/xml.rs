//! The XML both bindings share: reading (bytes to a parsed tree, one set of
//! helpers, duplicates found and cut) and writing (a document tree that
//! serialises itself into schema order).
//!
//! The writer emits in any order; [`Xml::finish`] sorts each level by the
//! generated tables ([`crate::ubl::order`], [`crate::cii::order`]) and checks
//! every element path against the prohibitions. Anything removed is reported,
//! all into one list:
//!
//! | | |
//! |---|---|
//! | the sequence tables | an element the target document has no place for |
//! | the prohibitions | an element CEN fences out of the EN 16931 subset |
//! | [`Xml::note`] | what only the writer knows, e.g. CII's BT-147 without a gross price |

use core::fmt::Write as _;
use std::borrow::Cow;

use crate::report::{Entry, Kind};

// ── Decoding ──────────────────────────────────────────────────────────────────

/// The text of an XML document, from its bytes.
///
/// XML 1.0 (§4.3.3) obliges a processor to read **UTF-8 and UTF-16**, and
/// requires UTF-16 to begin with a byte-order mark. So:
///
/// | Bytes begin with | Read as |
/// |---|---|
/// | `EF BB BF` | UTF-8, the mark dropped |
/// | `FF FE` / `FE FF` | UTF-16 LE / BE, the mark dropped |
/// | anything else | UTF-8 |
///
/// The XML declaration's `encoding`, when present, must name the family that
/// was found; a contradicting mark is refused. Every other encoding
/// (`ISO-8859-1`, `windows-1252`) is refused by name.
pub(crate) fn decode(bytes: &[u8]) -> Result<Cow<'_, str>, crate::Error> {
    let refuse = |found: &str| crate::Error::Encoding {
        found: found.to_owned(),
    };
    let (text, family): (Cow<'_, str>, &str) =
        if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
            (
                Cow::Borrowed(core::str::from_utf8(rest).map_err(|_| refuse("invalid UTF-8"))?),
                "UTF-8",
            )
        } else if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE") {
            (Cow::Owned(utf16(rest, u16::from_le_bytes)?), "UTF-16")
        } else if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF") {
            (Cow::Owned(utf16(rest, u16::from_be_bytes)?), "UTF-16")
        } else if bytes.starts_with(b"<\0") || bytes.starts_with(b"\0<") {
            return Err(refuse("UTF-16 without a byte-order mark"));
        } else {
            (
                Cow::Borrowed(core::str::from_utf8(bytes).map_err(|_| refuse("invalid UTF-8"))?),
                "UTF-8",
            )
        };
    if let Some(declared) = declared_encoding(&text)
        && !declared.eq_ignore_ascii_case(family)
    {
        return Err(refuse(declared));
    }
    Ok(text)
}

/// UTF-16 code units to a `String`, refusing an unpaired surrogate or an odd
/// byte count rather than replacing it.
fn utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> Result<String, crate::Error> {
    if !bytes.len().is_multiple_of(2) {
        return Err(crate::Error::Encoding {
            found: "invalid UTF-16".to_owned(),
        });
    }
    char::decode_utf16(bytes.as_chunks::<2>().0.iter().map(|c| unit(*c)))
        .collect::<Result<String, _>>()
        .map_err(|_| crate::Error::Encoding {
            found: "invalid UTF-16".to_owned(),
        })
}

/// The `encoding` pseudo-attribute of the XML declaration, if there is one.
fn declared_encoding(text: &str) -> Option<&str> {
    let decl = text.strip_prefix("<?xml")?;
    let decl = &decl[..decl.find("?>")?];
    let at = decl.find("encoding")?;
    let rest = decl[at + "encoding".len()..]
        .trim_start()
        .strip_prefix('=')?;
    let rest = rest.trim_start();
    let quote = rest.chars().next().filter(|c| matches!(c, '"' | '\''))?;
    let rest = &rest[1..];
    Some(&rest[..rest.find(quote)?])
}

// ── Parsing ───────────────────────────────────────────────────────────────────

/// Parse, refusing nesting past [`MAX_DEPTH`] before the parser recurses.
pub(crate) fn parse(text: &str) -> Result<roxmltree::Document<'_>, crate::Error> {
    let depth = max_depth(text);
    if depth > MAX_DEPTH {
        return Err(crate::Error::TooDeep {
            depth,
            limit: MAX_DEPTH,
        });
    }
    roxmltree::Document::parse(text).map_err(|e| {
        let pos = e.pos();
        // The parser's message ends in its own ` at ROW:COL`; the position is
        // carried as fields, so it is said once.
        let message = e.to_string();
        let suffix = format!(" at {}:{}", pos.row, pos.col);
        crate::Error::Xml {
            message: message
                .strip_suffix(&suffix)
                .map_or_else(|| message.clone(), str::to_owned),
            line: pos.row,
            column: pos.col,
        }
    })
}

// ── Reading helpers — one behaviour for both syntaxes ─────────────────────────

/// A parsed element.
pub(crate) type Node<'a, 'i> = roxmltree::Node<'a, 'i>;

/// Local name, namespace-independent.
pub(crate) fn name<'i>(n: Node<'_, 'i>) -> &'i str {
    n.tag_name().name()
}

/// Direct element children.
pub(crate) fn kids<'a, 'i>(n: Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> {
    n.children().filter(Node::is_element)
}

/// The first direct child with this local name.
pub(crate) fn kid<'a, 'i>(n: Node<'a, 'i>, want: &str) -> Option<Node<'a, 'i>> {
    kids(n).find(|c| name(*c) == want)
}

/// An element's text **as written**: every direct text child, concatenated.
///
/// `12<!-- note -->34` is `1234`, the `XPath` string value; CDATA is text.
/// Not trimmed: BT-20's trailing newline matters (`BR-DE-18`).
pub(crate) fn raw_text(n: Node<'_, '_>) -> Option<String> {
    let mut out = String::new();
    for c in n.children().filter(Node::is_text) {
        out.push_str(c.text().unwrap_or_default());
    }
    (!out.is_empty()).then_some(out)
}

/// An element's text, trimmed, and `None` when that leaves nothing.
///
/// An empty or whitespace-only element is an absent term, in both syntaxes.
pub(crate) fn text(n: Node<'_, '_>) -> Option<String> {
    raw_text(n)
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
}

/// [`text`] of the first child named `want`.
pub(crate) fn child_text(n: Node<'_, '_>, want: &str) -> Option<String> {
    kid(n, want).and_then(text)
}

/// `text` as a [`rust_decimal::Decimal`] when it is an `xs:decimal`: an
/// optional sign, then digits with at most one point, at least one digit.
/// `rust_decimal` alone also reads `1e3` and `1_000`, which the schemas
/// refuse, and rounds a fraction beyond its 28 places without saying so.
pub(crate) fn xs_decimal(text: &str) -> Result<rust_decimal::Decimal, String> {
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let digits = |part: &str| part.bytes().all(|b| b.is_ascii_digit());
    if (whole.is_empty() && fraction.is_empty()) || !digits(whole) || !digits(fraction) {
        return Err("not an xs:decimal".to_owned());
    }
    let parsed: rust_decimal::Decimal = text.parse().map_err(|e: rust_decimal::Error| {
        format!("not a decimal number the model can hold: {e}")
    })?;
    let places = fraction.trim_end_matches('0').len();
    if u32::try_from(places).map_or(true, |places| places > parsed.scale()) {
        return Err(format!(
            "{places} decimal places, more than a decimal holds beside its integer digits"
        ));
    }
    Ok(parsed)
}

/// A business-term path, `BG-23[1]` + `BT-117`, formatted only when an entry
/// is recorded (eager formatting at every field read cost ~10 % read time).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Term<'a> {
    group: &'a str,
    term: &'a str,
}

impl<'a> Term<'a> {
    /// `term` within `group`; an empty group is the term alone.
    pub(crate) const fn new(group: &'a str, term: &'a str) -> Self {
        Self { group, term }
    }

    /// The last part, `BT-117`.
    pub(crate) const fn term(self) -> &'a str {
        self.term
    }
}

impl std::fmt::Display for Term<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.group.is_empty() {
            f.write_str(self.term)
        } else {
            write!(f, "{}/{}", self.group, self.term)
        }
    }
}

/// The chain of local names from the document element down to `n`.
pub(crate) fn path_of(n: Node<'_, '_>) -> String {
    let mut names: Vec<&str> = n
        .ancestors()
        .filter(Node::is_element)
        .map(|a| name(a))
        .collect();
    names.reverse();
    names.join("/")
}

// ── Duplicates ────────────────────────────────────────────────────────────────

/// One row of a generated single-occurrence table, compiled.
struct Row {
    rule: &'static str,
    warning: bool,
    anchored: bool,
    context: Vec<&'static str>,
    /// A predicate the context element itself must satisfy.
    when: Option<Predicate>,
    steps: Vec<Step>,
}

/// One step of a relative path: a local name, whether it is reached through
/// `//`, and at most one predicate.
struct Step {
    name: &'static str,
    descend: bool,
    predicate: Option<Predicate>,
}

/// `[path = 'literal']`, `[path != 'literal']`, optionally comparing the value
/// upper-cased or with its whitespace normalised.
struct Predicate {
    path: Vec<&'static str>,
    negated: bool,
    upper: bool,
    normalise: bool,
    literal: &'static str,
}

fn local_of(qualified: &'static str) -> &'static str {
    qualified.rsplit(':').next().unwrap_or(qualified)
}

/// Split on `/` outside brackets.
fn split_steps(path: &'static str) -> Vec<&'static str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0usize, 0usize);
    for (i, c) in path.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            '/' if depth == 0 => {
                out.push(&path[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&path[start..]);
    out
}

/// Compile the canonical row spelling the generator writes:
/// `cac:A[upper:cac:B/cbc:C=VAT]/cbc:D`, a leading `//` for a descendant step.
fn compile(rows: &'static [(&'static str, &'static str, &'static str, &'static str)]) -> Vec<Row> {
    rows.iter()
        .map(|(rule, level, context, relative)| {
            let anchored = context.starts_with('/') && !context.starts_with("//");
            let mut when = None;
            let context = split_steps(context.trim_start_matches('/'))
                .into_iter()
                .map(|step| match step.split_once('[') {
                    Some((head, pred)) => {
                        when = Some(predicate(pred.trim_end_matches(']')));
                        local_of(head)
                    }
                    None => local_of(step),
                })
                .collect();
            let mut steps = Vec::new();
            let mut descend = false;
            for raw in split_steps(relative) {
                if raw.is_empty() {
                    // `//x` splits into an empty step and `x`.
                    descend = true;
                    continue;
                }
                let (head, predicate) = match raw.split_once('[') {
                    Some((head, pred)) => (head, Some(predicate(pred.trim_end_matches(']')))),
                    None => (raw, None),
                };
                steps.push(Step {
                    name: local_of(head),
                    descend,
                    predicate,
                });
                descend = false;
            }
            Row {
                rule,
                warning: *level == "warning",
                anchored,
                context,
                when,
                steps,
            }
        })
        .collect()
}

fn predicate(body: &'static str) -> Predicate {
    let (upper, body) = match body.strip_prefix("upper:") {
        Some(rest) => (true, rest),
        None => (false, body),
    };
    let (normalise, body) = match body.strip_prefix("norm:") {
        Some(rest) => (true, rest),
        None => (false, body),
    };
    let (path, negated, literal) = if let Some((p, l)) = body.split_once("!=") {
        (p, true, l)
    } else {
        let (p, l) = body.split_once('=').unwrap_or((body, ""));
        (p, false, l)
    };
    Predicate {
        path: path.split('/').map(local_of).collect(),
        negated,
        upper,
        normalise,
        literal,
    }
}

impl Predicate {
    fn holds(&self, n: Node<'_, '_>) -> bool {
        let mut at = Some(n);
        for step in &self.path {
            at = at.and_then(|x| kid(x, step));
        }
        let mut value = at.and_then(raw_text).unwrap_or_default();
        if self.normalise {
            value = value.split_whitespace().collect::<Vec<_>>().join(" ");
        }
        if self.upper {
            value = value.to_uppercase();
        }
        (value == self.literal) != self.negated
    }
}

/// A compiled single-occurrence table, indexed by the last name of each row's
/// context.
pub(crate) struct Once {
    rows: Vec<Row>,
    by_context: std::collections::HashMap<&'static str, Vec<usize>>,
}

impl Once {
    /// Compile a generated table.
    pub(crate) fn new(
        rows: &'static [(&'static str, &'static str, &'static str, &'static str)],
    ) -> Self {
        let rows = compile(rows);
        let mut by_context: std::collections::HashMap<&'static str, Vec<usize>> =
            std::collections::HashMap::new();
        for (i, r) in rows.iter().enumerate() {
            if let Some(last) = r.context.last() {
                by_context.entry(last).or_default().push(i);
            }
        }
        Self { rows, by_context }
    }
}

/// A second or later occurrence of a single-occurrence element.
pub(crate) struct Duplicate<'a, 'i> {
    pub node: Node<'a, 'i>,
    pub rule: &'static str,
    /// The rule that forbids it is published as a warning.
    pub warning: bool,
}

/// Every second and later occurrence the table forbids, outermost only, in
/// document order.
pub(crate) fn duplicates<'a, 'i>(
    doc: &'a roxmltree::Document<'i>,
    once: &Once,
) -> Vec<Duplicate<'a, 'i>> {
    let mut found: Vec<Duplicate<'a, 'i>> = Vec::new();
    let mut stack: Vec<&str> = Vec::new();
    walk(doc.root_element(), once, &mut stack, &mut found);
    // A node two rows report keeps the stricter reading: fatal before warning.
    found.sort_by_key(|d| (d.node.range().start, d.warning));
    found.dedup_by_key(|d| d.node.id());
    // Outermost only: a duplicated group's own duplicates go with it.
    let mut out: Vec<Duplicate<'a, 'i>> = Vec::new();
    for d in found {
        if !out
            .iter()
            .any(|o| d.node.ancestors().any(|a| a.id() == o.node.id()))
        {
            out.push(d);
        }
    }
    out
}

fn walk<'a, 'i>(
    n: Node<'a, 'i>,
    once: &Once,
    stack: &mut Vec<&'i str>,
    found: &mut Vec<Duplicate<'a, 'i>>,
) {
    stack.push(name(n));
    if let Some(rows) = once.by_context.get(name(n)) {
        for &i in rows {
            let row = &once.rows[i];
            let matches = if row.anchored {
                stack.len() == row.context.len() && stack.iter().eq(row.context.iter())
            } else {
                stack.ends_with(&row.context)
            };
            if !matches || row.when.as_ref().is_some_and(|p| !p.holds(n)) {
                continue;
            }
            let mut nodes = vec![n];
            for step in &row.steps {
                let mut next = Vec::new();
                for x in nodes {
                    let candidates: Box<dyn Iterator<Item = Node<'a, 'i>>> = if step.descend {
                        Box::new(x.descendants().skip(1).filter(Node::is_element))
                    } else {
                        Box::new(kids(x))
                    };
                    next.extend(candidates.filter(|c| {
                        name(*c) == step.name && step.predicate.as_ref().is_none_or(|p| p.holds(*c))
                    }));
                }
                nodes = next;
            }
            nodes.sort_by_key(|n| n.range().start);
            for extra in nodes.into_iter().skip(1) {
                found.push(Duplicate {
                    node: extra,
                    rule: row.rule,
                    warning: row.warning,
                });
            }
        }
    }
    for c in kids(n) {
        walk(c, once, stack, found);
    }
    stack.pop();
}

/// `text` without the byte ranges of `cut` — the document as it would be had
/// the duplicates not been there.
pub(crate) fn without(text: &str, cut: &[Duplicate<'_, '_>]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for d in cut {
        let r = d.node.range();
        if r.start >= at {
            out.push_str(&text[at..r.start]);
            at = r.end;
        }
    }
    out.push_str(&text[at..]);
    out
}

/// The entry for a duplicate, before it is cut.
pub(crate) fn duplicate_entry(d: &Duplicate<'_, '_>, schema: &str) -> Entry {
    let leaf = kids(d.node).next().is_none();
    let mut e = Entry::new(
        if d.warning {
            Kind::Repeated
        } else {
            Kind::Duplicate
        },
        path_of(d.node),
        if d.rule.is_empty() {
            format!(
                "a second occurrence of an element {schema} allows once; the first is read, \
                 this one is not"
            )
        } else if d.warning {
            "a further occurrence of an element CEN's syntax rule (a warning) allows once; \
             the first is read, this one is not"
                .to_owned()
        } else {
            "a further occurrence of an element CEN's syntax rule allows once; the first is \
             read, this one is not"
                .to_owned()
        },
    );
    if leaf && let Some(v) = text(d.node) {
        e = e.value(v);
    }
    if !d.rule.is_empty() {
        e = e.rule(d.rule);
    }
    e
}

// ── Depth guard ───────────────────────────────────────────────────────────────

/// The deepest element nesting either reader will accept.
///
/// A security limit: the parser recurses per level and a stack overflow aborts
/// the process, so deeper documents are refused by a linear byte scan before
/// parsing. Real UBL and CII instances nest about a dozen deep
/// (`tests/corpus.rs` checks the reference corpus); a debug build uses about
/// 16 KB of stack per level, so 64 fits a 2 MB worker-thread stack.
pub const MAX_DEPTH: usize = 64;

/// The bounds on [`MAX_DEPTH`], checked at compile time.
const _: () = {
    assert!(
        MAX_DEPTH >= 32,
        "UBL and CII nest about a dozen deep; a limit this low would reject real invoices"
    );
    assert!(
        MAX_DEPTH <= 100,
        "the parser overflows a 2 MB worker-thread stack around 125 levels"
    );
};

/// The deepest element nesting in `xml`, without parsing it.
///
/// A scanner, so it answers before anything recurses. It must never
/// over-report: comments, CDATA, processing instructions and quoted attribute
/// values are skipped.
pub(crate) fn max_depth(xml: &str) -> usize {
    let b = xml.as_bytes();
    let (mut i, mut depth, mut max) = (0usize, 0usize, 0usize);
    let after = |from: usize, needle: &[u8]| -> usize {
        b[from..]
            .windows(needle.len())
            .position(|w| w == needle)
            .map_or(b.len(), |p| from + p + needle.len())
    };
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &b[i + 1..];
        if rest.starts_with(b"!--") {
            i = after(i + 4, b"-->");
        } else if rest.starts_with(b"![CDATA[") {
            i = after(i + 9, b"]]>");
        } else if rest.starts_with(b"?") {
            i = after(i + 2, b"?>");
        } else if rest.starts_with(b"!") {
            // `<!DOCTYPE …>`: `roxmltree` refuses it later (no XXE, no
            // billion laughs); skipped here so the count stays right.
            i = after(i + 2, b">");
        } else if rest.starts_with(b"/") {
            depth = depth.saturating_sub(1);
            i = after(i + 2, b">");
        } else {
            // A start tag. `>` inside a quoted attribute value is legal, so the
            // end of the tag is found with the quoting respected.
            let (end, self_closing) = end_of_tag(b, i + 1);
            if !self_closing {
                depth += 1;
                max = max.max(depth);
            }
            i = end;
        }
    }
    max
}

/// `(index just past `>`, whether the tag closed itself)`.
fn end_of_tag(b: &[u8], mut i: usize) -> (usize, bool) {
    let mut quote = None::<u8>;
    let mut last = b'<';
    while i < b.len() {
        let c = b[i];
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == b'"' || c == b'\'' {
            quote = Some(c);
        } else if c == b'>' {
            return (i + 1, last == b'/');
        }
        last = c;
        i += 1;
    }
    (b.len(), false)
}

/// How a syntax answers the serialiser's two questions.
pub(crate) struct Rules {
    /// The child order for a parent element, by local name.
    pub order: fn(&str) -> Option<&'static [&'static str]>,
    /// The rule forbidding this element path, if any.
    pub forbidden_path: fn(&str) -> Option<&'static str>,
    /// The rule forbidding this attribute anywhere, if any.
    pub forbidden_attribute: fn(&str) -> Option<&'static str>,
}

/// An element under construction.
#[derive(Clone)]
pub(crate) struct XNode {
    name: String,
    attrs: Vec<(String, String)>,
    text: Option<String>,
    children: Vec<XNode>,
    /// The business term the writer emitted this element for, when it said —
    /// so a drop the sequence tables or the prohibitions discover can name it.
    term: Option<&'static str>,
}

/// A document under construction.
pub(crate) struct Xml {
    stack: Vec<XNode>,
    rules: &'static Rules,
    /// Losses the *writer* knows about, as opposed to the ones the sequence
    /// tables and the prohibitions discover during rendering.
    ///
    /// E.g. CII nests BT-147 inside the gross-price aggregate, so a discount
    /// without a gross price cannot be written.
    notes: Vec<Entry>,
    /// Prohibition ids this document is permitted to violate.
    ///
    /// A profile declaring an extension group (§4.3) waives the matching core
    /// prohibition: `UBL-CR-646` (`cac:SubInvoiceLine`, BG-DEX-01) and
    /// `UBL-CR-470` (`cac:PrepaidPayment`, BG-DEX-09). Decided from
    /// [`en16931::Profile::extensions`].
    waived: &'static [&'static str],
}

impl Xml {
    pub fn new(root: &str, attrs: Vec<(String, String)>, rules: &'static Rules) -> Self {
        Self {
            stack: vec![XNode {
                name: root.to_owned(),
                attrs,
                text: None,
                children: Vec::new(),
                term: None,
            }],
            rules,
            notes: Vec::new(),
            waived: &[],
        }
    }

    /// Permit the prohibitions in `ids` for this document — see [`Xml::waived`].
    ///
    /// UBL only: the CII writer reports `BG-DEX-01` and `BG-DEX-09` as dropped.
    #[cfg(feature = "ubl")]
    pub fn waiving(mut self, ids: &'static [&'static str]) -> Self {
        self.waived = ids;
        self
    }

    /// Record what only the writer knows: a term this syntax has no place for,
    /// a required element the model lacks, a value derived because the syntax
    /// demands one.
    pub fn note(&mut self, entry: Entry) {
        self.notes.push(entry);
    }

    /// Run `f`, tagging every element it emits at this level with `term`.
    ///
    /// So a `Dropped` entry names the term (UBL's `<CreditNote>` has no
    /// `cbc:DueDate`: BT-9).
    #[cfg(feature = "ubl")]
    pub fn term(&mut self, term: &'static str, f: impl FnOnce(&mut Self)) {
        let before = self.stack.last().map_or(0, |n| n.children.len());
        f(self);
        if let Some(top) = self.stack.last_mut() {
            for c in top.children.iter_mut().skip(before) {
                c.term = Some(term);
            }
        }
    }

    fn push(&mut self, node: XNode) {
        self.stack
            .last_mut()
            .expect("the root is never popped")
            .children
            .push(node);
    }

    /// `<name attrs>text</name>`.
    pub fn leaf(&mut self, name: &str, attrs: &[(&str, &str)], text: &str) {
        self.push(XNode {
            name: name.to_owned(),
            attrs: attrs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            text: Some(text.to_owned()),
            children: Vec::new(),
            term: None,
        });
    }

    /// Run `f` inside `name`, emitting nothing at all if `f` writes nothing.
    ///
    /// An empty aggregate is present-but-blank and rules count occurrences.
    pub fn group(&mut self, name: &str, f: impl FnOnce(&mut Self)) {
        self.stack.push(XNode {
            name: name.to_owned(),
            attrs: Vec::new(),
            text: None,
            children: Vec::new(),
            term: None,
        });
        f(self);
        let node = self.stack.pop().expect("group pushed a node");
        if !node.children.is_empty() {
            self.push(node);
        }
    }

    /// [`Xml::group`], saying whether the group was emitted (had children).
    #[cfg(feature = "ubl")]
    pub fn group_emitted(&mut self, name: &str, f: impl FnOnce(&mut Self)) -> bool {
        let before = self.stack.last().map_or(0, |n| n.children.len());
        self.group(name, f);
        self.stack.last().map_or(0, |n| n.children.len()) > before
    }

    /// Like [`Xml::group`], but emitted even when empty.
    ///
    /// For CII's `rsm:ExchangedDocumentContext` and
    /// `ram:ApplicableHeaderTradeDelivery`, mandatory in D16B and possibly
    /// empty. CII only.
    #[cfg(feature = "cii")]
    pub fn group_required(&mut self, name: &str, f: impl FnOnce(&mut Self)) {
        self.stack.push(XNode {
            name: name.to_owned(),
            attrs: Vec::new(),
            text: None,
            children: Vec::new(),
            term: None,
        });
        f(self);
        let node = self.stack.pop().expect("group pushed a node");
        self.push(node);
    }

    /// Serialise, ordering every level and enforcing the prohibitions.
    ///
    /// Returns the document and everything the syntax could not carry.
    pub fn finish(mut self) -> (String, Vec<Entry>) {
        let mut root = self.stack.pop().expect("the root");
        debug_assert!(self.stack.is_empty(), "unbalanced group()");
        let mut out = String::with_capacity(4096);
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        let mut dropped = std::mem::take(&mut self.notes);
        let name = root.name.clone();
        render(
            &mut root,
            &name,
            0,
            &mut out,
            &mut dropped,
            self.rules,
            self.waived,
        );
        (out, dropped)
    }
}

/// Does a document path fall under `context` with `relative` beneath it?
///
/// A context beginning with a single `/` anchors at the document element; `//`
/// or a bare name matches at any depth.
///
/// The document element is compared by local name (`Invoice` vs
/// `ubl:Invoice`); everything below it qualified.
pub(crate) fn path_matches(path: &str, context: &str, relative: &str) -> bool {
    let floating = context.starts_with("//") || !context.starts_with('/');
    let ctx = context.trim_start_matches('/');
    if floating {
        // `path == "{ctx}/{relative}"`, or `path` ends with `"/{ctx}/{relative}"`,
        // compared without allocating.
        return ends_with_segment_pair(path, ctx, relative);
    }
    // Anchored: the whole path must be the context element followed by the
    // relative path, and only the first segment is matched loosely.
    let Some((head, rest)) = path.split_once('/') else {
        return false;
    };
    local(head) == local(ctx) && rest == relative
}

/// A prohibition table indexed by the last segment of its relative path, since
/// a prohibition can only match a path ending in that name. `forbidden_path`
/// runs once per element emitted.
pub(crate) struct ProhibitionIndex {
    by_tail: std::collections::HashMap<&'static str, Vec<usize>>,
}

impl ProhibitionIndex {
    /// Index `table` by the last segment of each relative path.
    pub(crate) fn build(table: &'static [(&'static str, &'static str, &'static str)]) -> Self {
        let mut by_tail: std::collections::HashMap<&'static str, Vec<usize>> =
            std::collections::HashMap::new();
        for (i, (_, _, rel)) in table.iter().enumerate() {
            by_tail.entry(tail_of(rel)).or_default().push(i);
        }
        Self { by_tail }
    }

    /// The first entry in table order that `path` violates, so the reported
    /// rule id is stable when two match.
    pub(crate) fn find(
        &self,
        table: &'static [(&'static str, &'static str, &'static str)],
        path: &str,
    ) -> Option<&'static str> {
        let candidates = self.by_tail.get(tail_of(path))?;
        candidates
            .iter()
            .map(|&i| &table[i])
            .find(|(_, ctx, rel)| path_matches(path, ctx, rel))
            .map(|(id, _, _)| *id)
    }
}

/// The last `/`-separated segment.
fn tail_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Whether `path` ends with `a/b`, on a segment boundary — without building it.
///
/// Slices through `get`, so a non-ASCII name cannot panic.
fn ends_with_segment_pair(path: &str, a: &str, b: &str) -> bool {
    let needle_len = a.len() + 1 + b.len();
    let Some(start) = path.len().checked_sub(needle_len) else {
        return false;
    };
    // Either the whole path, or preceded by a separator — never mid-segment.
    if start > 0 && path.as_bytes().get(start - 1) != Some(&b'/') {
        return false;
    }
    let Some(tail) = path.get(start..) else {
        return false;
    };
    tail.get(..a.len()) == Some(a)
        && tail.as_bytes().get(a.len()) == Some(&b'/')
        && tail.get(a.len() + 1..) == Some(b)
}

/// The local name, as the order tables key them — `cac:Party` → `Party`.
fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// Sort `children` into sequence order, reporting any the sequence cannot place.
///
/// Stable: repeated elements keep the order they were written in.
fn order_children(parent: &str, children: &mut Vec<XNode>, dropped: &mut Vec<Entry>, r: &Rules) {
    let Some(seq) = (r.order)(local(parent)) else {
        return; // no evidence for this parent; leave the writer's order alone
    };
    children.retain(|c| {
        let known = seq.contains(&local(&c.name));
        if !known {
            // Not in the parent's sequence (e.g. `cac:ProjectReference` under
            // `<CreditNote>`): emitting it would fail the schema.
            let mut e = Entry::new(
                Kind::Dropped,
                format!("{}/{}", local(parent), c.name),
                "the target document's element sequence has no place for it",
            );
            if let Some(t) = c.term {
                e = e.term(t);
            }
            dropped.push(e);
        }
        known
    });
    children.sort_by_key(|c| {
        seq.iter()
            .position(|e| *e == local(&c.name))
            .unwrap_or(usize::MAX)
    });
}

/// Render `node`, sorting each level **in place**.
///
/// `&mut` so [`order_children`] sorts in place rather than cloning subtrees.
fn render(
    node: &mut XNode,
    path: &str,
    depth: usize,
    out: &mut String,
    dropped: &mut Vec<Entry>,
    r: &Rules,
    waived: &[&str],
) {
    for _ in 0..depth {
        out.push_str("  ");
    }
    let _ = write!(out, "<{}", node.name);
    for (k, v) in &node.attrs {
        if let Some(rule) = (r.forbidden_attribute)(k) {
            dropped.push(
                Entry::new(
                    Kind::Prohibited,
                    format!("{path}/@{k}"),
                    "CEN's syntax rules fence it out of the EN 16931 subset",
                )
                .rule(rule),
            );
            continue;
        }
        let _ = write!(out, " {k}=\"");
        if escape_attribute(v, out) {
            dropped.push(unwritable_characters(format!("{path}/@{k}"), node.term));
        }
        out.push('"');
    }
    if node.children.is_empty() {
        // A required-but-empty aggregate serialises as `<x/>`.
        if node.text.is_none() {
            let _ = writeln!(out, "/>");
            return;
        }
        out.push('>');
        if escape(node.text.as_deref().unwrap_or_default(), out) {
            dropped.push(unwritable_characters(path.to_owned(), node.term));
        }
        let _ = writeln!(out, "</{}>", node.name);
        return;
    }
    let _ = writeln!(out, ">");
    // Disjoint field borrows: the sequence table is keyed by the parent's name
    // and the sort mutates its children, and the two are different fields.
    let XNode { name, children, .. } = node;
    order_children(name, children, dropped, r);
    for c in children.iter_mut() {
        let child_path = if path.is_empty() {
            c.name.clone()
        } else {
            format!("{path}/{}", c.name)
        };
        // Enforced here for every element (e.g. `UBL-CR-244` forbids
        // `cbc:CompanyLegalForm` on the customer).
        if let Some(rule) = (r.forbidden_path)(&child_path)
            && !waived.contains(&rule)
        {
            let mut e = Entry::new(
                Kind::Prohibited,
                child_path,
                "CEN's syntax rules fence it out of the EN 16931 subset",
            )
            .rule(rule);
            if let Some(t) = c.term {
                e = e.term(t);
            }
            dropped.push(e);
            continue;
        }
        render(c, &child_path, depth + 1, out, dropped, r, waived);
    }
    for _ in 0..depth {
        out.push_str("  ");
    }
    let _ = writeln!(out, "</{name}>");
}

/// Escape the five metacharacters and a carriage return, and drop the
/// characters XML 1.0 cannot represent at all (C0 controls, U+FFFE, U+FFFF) —
/// returning whether any were dropped, so the caller reports it.
///
/// A literal carriage return would reach the reader as a line feed (XML's
/// end-of-line normalisation), so it is written as `&#13;`, which survives.
pub(crate) fn escape(s: &str, out: &mut String) -> bool {
    let mut dropped = false;
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\r' => out.push_str("&#13;"),
            // Outside XML 1.0's `Char`: the C0 controls but tab and line feed
            // (carriage return is referenced above), and U+FFFE / U+FFFF.
            c if ((c as u32) < 0x20 && !matches!(c, '\t' | '\n'))
                || matches!(c, '\u{FFFE}' | '\u{FFFF}') =>
            {
                dropped = true;
            }
            c => out.push(c),
        }
    }
    dropped
}

/// [`escape`] for an attribute value, where a parser also turns a literal tab
/// or line feed into a space (XML 1.0 §3.3.3): both are written as character
/// references, which survive.
pub(crate) fn escape_attribute(s: &str, out: &mut String) -> bool {
    let mut dropped = false;
    for part in s.split_inclusive(['\t', '\n']) {
        let (body, end) = match part.strip_suffix(['\t', '\n']) {
            Some(body) => (body, &part[body.len()..]),
            None => (part, ""),
        };
        dropped |= escape(body, out);
        match end {
            "\t" => out.push_str("&#9;"),
            "\n" => out.push_str("&#10;"),
            _ => {}
        }
    }
    dropped
}

/// The entry for a value that lost control characters XML cannot carry.
fn unwritable_characters(path: String, term: Option<&'static str>) -> Entry {
    let e = Entry::new(
        Kind::Dropped,
        path,
        "characters XML 1.0 cannot represent were left out of the value",
    );
    match term {
        Some(t) => e.term(t),
        None => e,
    }
}

/// Base64, RFC 4648 §4, no line breaks.
///
/// For BT-125, an attachment's bytes inline.
pub(crate) fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        out.push(A[(n >> 18 & 63) as usize] as char);
        out.push(A[(n >> 12 & 63) as usize] as char);
        out.push(if c.len() > 1 {
            A[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            A[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Decode `xs:base64Binary` (RFC 4648 §4), ignoring XML whitespace. Any other
/// byte outside the alphabet, misplaced padding or a length not a multiple of
/// four is refused, never skipped into plausible bytes.
pub(crate) fn decode_base64(s: &str) -> Result<Vec<u8>, &'static str> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    let mut count = 0usize;
    let mut padding = 0usize;
    for c in s.bytes() {
        let v = match c {
            b' ' | b'\t' | b'\n' | b'\r' => continue,
            b'=' => {
                padding += 1;
                count += 1;
                continue;
            }
            _ if padding > 0 => return Err("base64 content continues after its padding"),
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err("not base64: a character outside the base64 alphabet"),
        };
        count += 1;
        acc = acc << 6 | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            // Truncation is the decode: six-bit groups are reassembled into
            // bytes, and the high bits shifted past are the previous byte's.
            #[allow(clippy::cast_possible_truncation)]
            out.push((acc >> bits) as u8);
        }
    }
    if padding > 2 || !count.is_multiple_of(4) {
        return Err("not base64: the length is not a multiple of four");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    static NO_RULES: Rules = Rules {
        order: |_| None,
        forbidden_path: |_| None,
        forbidden_attribute: |_| None,
    };

    /// One `text()` for both syntaxes: every text child, trimmed, and an empty
    /// or whitespace-only element is absent.
    #[test]
    fn text_is_the_whole_value_and_blank_is_absent() {
        let doc = roxmltree::Document::parse(
            "<r><a> 12<!-- note -->34 </a><b>   </b><c/><d><![CDATA[x<y]]></d><e>\n7\n</e></r>",
        )
        .expect("well-formed");
        let r = doc.root_element();
        assert_eq!(child_text(r, "a").as_deref(), Some("1234"));
        assert_eq!(child_text(r, "b"), None, "whitespace-only is absent");
        assert_eq!(child_text(r, "c"), None, "empty is absent");
        assert_eq!(child_text(r, "d").as_deref(), Some("x<y"));
        assert_eq!(child_text(r, "missing"), None);
        // Untrimmed for the one term whose whitespace is load-bearing.
        assert_eq!(kid(r, "e").and_then(raw_text).as_deref(), Some("\n7\n"));
    }

    #[test]
    fn utf8_and_utf16_with_a_mark_decode_alike() {
        let xml = "<?xml version=\"1.0\"?><Invoice>Müller</Invoice>";
        let mut bom8 = b"\xEF\xBB\xBF".to_vec();
        bom8.extend_from_slice(xml.as_bytes());
        let le: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain(xml.encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        let be: Vec<u8> = [0xFE, 0xFF]
            .into_iter()
            .chain(xml.encode_utf16().flat_map(u16::to_be_bytes))
            .collect();
        for bytes in [xml.as_bytes().to_vec(), bom8, le, be] {
            assert_eq!(decode(&bytes).expect("decodes"), xml);
        }
    }

    #[test]
    fn every_other_encoding_is_refused_by_name() {
        let found = |b: &[u8]| match decode(b) {
            Err(crate::Error::Encoding { found }) => found,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            found(b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><a/>"),
            "ISO-8859-1"
        );
        assert_eq!(found(b"<a>\xFC</a>"), "invalid UTF-8");
        // A mark contradicting the declaration: refused, not one of them believed.
        let mut lying = b"\xEF\xBB\xBF".to_vec();
        lying.extend_from_slice(b"<?xml version='1.0' encoding='UTF-16'?><a/>");
        assert_eq!(found(&lying), "UTF-16");
        assert_eq!(found(b"<\0a\0/\0>\0"), "UTF-16 without a byte-order mark");
        // The declared family that was found is fine, in any case.
        assert!(decode(b"<?xml version='1.0' encoding='utf-8'?><a/>").is_ok());
    }

    /// The depth scanner never over-reports: each case is a `<` or `>` that is
    /// not a tag.
    #[test]
    fn depth_counts_elements_and_nothing_that_merely_looks_like_one() {
        assert_eq!(max_depth(""), 0);
        assert_eq!(max_depth("<a/>"), 0, "a self-closing element opens nothing");
        assert_eq!(max_depth("<a></a>"), 1);
        assert_eq!(max_depth("<a><b><c/></b></a>"), 2);
        // Siblings are not depth.
        assert_eq!(max_depth("<a><b/><b/><b/></a>"), 1);
        // Markup inside a comment or CDATA is text.
        assert_eq!(max_depth("<a><!-- <b><c><d> --></a>"), 1);
        assert_eq!(max_depth("<a><![CDATA[<b><c><d>]]></a>"), 1);
        assert_eq!(max_depth("<?xml version=\"1.0\"?><a></a>"), 1);
        assert_eq!(max_depth("<!DOCTYPE a><a></a>"), 1);
        // `>` inside a quoted attribute value is legal.
        assert_eq!(max_depth("<a x=\"1>2\"><b/></a>"), 1);
        assert_eq!(max_depth("<a x='a>b' y=\"c>d\"/>"), 0);
        // An unterminated construct ends the scan. A truncated start tag may
        // over-report by one; the document is not well-formed anyway.
        assert_eq!(max_depth("<a><!-- never closed"), 1);
        assert_eq!(max_depth("<a"), 1);
    }

    #[test]
    fn base64_matches_rfc_4648_vectors() {
        for (plain, encoded) in [
            (&b""[..], ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg=="),
            (b"fooba", "Zm9vYmE="),
            (b"foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(plain), encoded);
            assert_eq!(
                decode_base64(encoded).unwrap(),
                plain,
                "round trip {encoded}"
            );
        }
    }

    #[test]
    fn decoding_ignores_the_whitespace_xml_permits() {
        assert_eq!(decode_base64("Zm9v\n  YmFy").unwrap(), b"foobar");
    }

    /// Corrupt content is refused, never decoded into plausible bytes.
    #[test]
    fn decoding_refuses_what_is_not_base64() {
        for bad in [
            "abc!",
            "abc",
            "Zg=",
            "Zg===",
            "Zg==Zg==",
            "Zm9v YmF",
            "Zm9-",
            "\u{e9}AAA",
        ] {
            assert!(decode_base64(bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn text_is_escaped() {
        let mut s = String::new();
        assert!(escape("x<y & \"z\" 'q'\u{7}\r\u{FFFF}", &mut s));
        assert_eq!(s, "x&lt;y &amp; &quot;z&quot; &apos;q&apos;&#13;");
        assert!(!escape("tab\tand\nline", &mut String::new()));
        let mut a = String::new();
        assert!(!escape_attribute("a\tb\nc\rd", &mut a));
        assert_eq!(a, "a&#9;b&#10;c&#13;d");
    }

    #[test]
    fn empty_groups_vanish() {
        let mut x = Xml::new("Root", vec![], &NO_RULES);
        x.group("a:Empty", |_| {});
        let (xml, dropped) = x.finish();
        assert!(!xml.contains("Empty"), "{xml}");
        assert!(dropped.is_empty(), "{dropped:?}");
    }

    #[cfg(feature = "cii")]
    #[test]
    fn a_required_group_survives_being_empty() {
        let mut x = Xml::new("Root", vec![], &NO_RULES);
        x.group_required("a:Mandatory", |_| {});
        x.group("a:Optional", |_| {});
        let (xml, dropped) = x.finish();
        assert!(xml.contains("<a:Mandatory/>"), "{xml}");
        assert!(!xml.contains("Optional"), "{xml}");
        assert!(dropped.is_empty(), "{dropped:?}");
    }

    #[test]
    fn an_anchored_context_matches_only_at_the_root() {
        assert!(path_matches("Invoice/cbc:UUID", "/ubl:Invoice", "cbc:UUID"));
        // A default-namespace writer emits `Invoice`; the Schematron writes
        // `ubl:Invoice`. Only the document element is matched loosely.
        assert!(path_matches(
            "ubl:Invoice/cbc:UUID",
            "/ubl:Invoice",
            "cbc:UUID"
        ));
        assert!(!path_matches(
            "Invoice/cac:Party/cbc:UUID",
            "/ubl:Invoice",
            "cbc:UUID"
        ));
        assert!(!path_matches(
            "CreditNote/cbc:UUID",
            "/ubl:Invoice",
            "cbc:UUID"
        ));
    }

    #[test]
    fn a_floating_context_matches_at_any_depth() {
        assert!(path_matches(
            "Invoice/cac:Party/cbc:X",
            "//cac:Party",
            "cbc:X"
        ));
        assert!(path_matches("cac:Party/cbc:X", "//cac:Party", "cbc:X"));
        assert!(path_matches("A/B/cac:Party/cbc:X", "cac:Party", "cbc:X"));
        // Element boundaries are respected — no partial-name match.
        assert!(!path_matches(
            "Invoice/cac:MyParty/cbc:X",
            "//cac:Party",
            "cbc:X"
        ));
    }

    /// With no order table for a parent, the writer's own order is preserved.
    #[test]
    fn an_unknown_parent_keeps_the_writers_order() {
        let mut x = Xml::new("Root", vec![], &NO_RULES);
        x.leaf("a:Second", &[], "2");
        x.leaf("a:First", &[], "1");
        let (xml, dropped) = x.finish();
        assert!(xml.find("Second").unwrap() < xml.find("First").unwrap());
        assert!(dropped.is_empty(), "{dropped:?}");
    }
}
