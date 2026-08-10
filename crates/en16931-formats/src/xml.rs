//! A **document tree that serialises itself into schema order**.
//!
//! Shared by both bindings, because both need exactly the same thing and for
//! exactly the same reason.
//!
//! # Why a tree and not a string
//!
//! UBL and CII content models are XSD `sequence`s: a document carrying the
//! right elements in the wrong order is invalid, and **no Schematron rule says
//! so** — ordering is the schema's job, and this crate ships no schema.
//!
//! A hand-sequenced writer therefore has to get the order right at every call
//! site, in two syntaxes, one of which (UBL) has two document elements that
//! disagree about where `cbc:TaxPointDate` goes. That was tried, and it was
//! wrong. So the writer emits in whatever order reads best and this module
//! sorts by the derived tables ([`crate::ubl::order`], [`crate::cii::order`]).
//! Misordering became unrepresentable rather than tested-for.
//!
//! # It also enforces the prohibitions
//!
//! 1 218 of the 1 339 syntax rules say some element "shall not be used". The
//! serialiser checks each path against the extracted tables, so "the writer
//! cannot emit a forbidden element" is a property of *this* module rather than
//! a habit spread across two hundred call sites.
//!
//! # Nothing is dropped silently
//!
//! Anything the target sequence has no place for is removed **and reported**.
//! UBL's `<CreditNote>` has no `cbc:DueDate`; dropping BT-9 is correct, and
//! dropping it quietly would mean a payment due date vanishing between two
//! systems with nothing in any log.
//!
//! Three mechanisms notice, and they all write to the same list, because a
//! caller asking *"what did I lose?"* should not have to know which one it was:
//!
//! | | |
//! |---|---|
//! | the sequence tables | an element the target document has no place for |
//! | the prohibitions | an element CEN fences out of the EN 16931 subset |
//! | [`Xml::dropped`] | what only the writer knows — CII nesting BT-147 inside the gross-price aggregate, a credit note whose BT-3 says otherwise, a group that would serialise empty |

use core::fmt::Write as _;

/// How a syntax answers the serialiser's two questions.
pub(crate) struct Rules {
    /// The child order for a parent element, by local name.
    pub order: fn(&str) -> Option<&'static [&'static str]>,
    /// The rule forbidding this element path, if any.
    pub forbidden_path: fn(&str) -> Option<&'static str>,
    /// The rule forbidding this attribute anywhere, if any.
    pub forbidden_attribute: fn(&str) -> Option<&'static str>,
}

#[derive(Clone)]
pub(crate) struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    text: Option<String>,
    children: Vec<Node>,
}

/// A document under construction.
pub(crate) struct Xml {
    stack: Vec<Node>,
    rules: &'static Rules,
    /// Losses the *writer* knows about, as opposed to the ones the sequence
    /// tables and the prohibitions discover during rendering.
    ///
    /// Some terms have nowhere to go in a syntax for reasons no table can see:
    /// CII nests BT-147 inside the gross-price aggregate, so a discount stated
    /// without a gross price cannot be written at all. The writer is the only
    /// thing that knows, so it says so here rather than dropping it in silence.
    notes: Vec<String>,
    /// Prohibition ids this document is permitted to violate.
    ///
    /// # Why a writer may ever be allowed to
    ///
    /// The prohibitions are **CEN core's** subset definition, and an Extension
    /// is §4.3's mechanism for going outside it. `UBL-CR-646` forbids
    /// `cac:SubInvoiceLine` and `UBL-CR-470` forbids `cac:PrepaidPayment`;
    /// KoSIT's XRechnung Extension scenario reports both at `information`
    /// precisely so `BG-DEX-01` and `BG-DEX-09` can be carried.
    ///
    /// So a write *for a profile that declares the extension group* waives the
    /// matching prohibition, and a write for anything else does not — the
    /// element is dropped and reported, as before. The capability comes from
    /// [`en16931::Profile::extensions`], the same field `EN-EXT-01` reads, so
    /// the warning and the writer cannot disagree about what a profile can hold.
    waived: &'static [&'static str],
}

impl Xml {
    pub fn new(root: &str, attrs: Vec<(String, String)>, rules: &'static Rules) -> Self {
        Self {
            stack: vec![Node {
                name: root.to_owned(),
                attrs,
                text: None,
                children: Vec::new(),
            }],
            rules,
            notes: Vec::new(),
            waived: &[],
        }
    }

    /// Permit the prohibitions in `ids` for this document — see [`Xml::waived`].
    ///
    /// UBL only, and that is not an oversight in the `cfg`: the CII binding does
    /// not *write* `BG-DEX-01` or `BG-DEX-09` at all — it reports them in
    /// `dropped` — so there is no prohibition for it to waive. A waiver it never
    /// calls would be a method advertising a capability the binding does not
    /// have, and under `--no-default-features --features cii` the compiler says
    /// so out loud.
    ///
    /// When the CII writer learns those groups, this loses the attribute and
    /// gains a second caller.
    #[cfg(feature = "ubl")]
    pub fn waiving(mut self, ids: &'static [&'static str]) -> Self {
        self.waived = ids;
        self
    }

    /// Record a term this syntax has no place for, with the reason.
    ///
    /// Goes into the same list the sequence tables and prohibitions write to,
    /// because a caller asking "what did I lose?" should not have to know which
    /// of three mechanisms noticed.
    pub fn dropped(&mut self, what: impl Into<String>) {
        self.notes.push(what.into());
    }

    fn push(&mut self, node: Node) {
        self.stack
            .last_mut()
            .expect("the root is never popped")
            .children
            .push(node);
    }

    /// `<name attrs>text</name>`.
    pub fn leaf(&mut self, name: &str, attrs: &[(&str, &str)], text: &str) {
        self.push(Node {
            name: name.to_owned(),
            attrs: attrs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            text: Some(text.to_owned()),
            children: Vec::new(),
        });
    }

    /// Run `f` inside `name`, emitting nothing at all if `f` writes nothing.
    ///
    /// Both syntaxes treat an empty aggregate as present-but-blank and several
    /// rules count occurrences, so a wrapper around no children is not merely
    /// untidy — it changes what the document asserts.
    pub fn group(&mut self, name: &str, f: impl FnOnce(&mut Self)) {
        self.stack.push(Node {
            name: name.to_owned(),
            attrs: Vec::new(),
            text: None,
            children: Vec::new(),
        });
        f(self);
        let node = self.stack.pop().expect("group pushed a node");
        if !node.children.is_empty() {
            self.push(node);
        }
    }

    /// Like [`Xml::group`], but emitted even when empty.
    ///
    /// CII's `rsm:ExchangedDocumentContext` and `ram:ApplicableHeaderTradeDelivery`
    /// are **mandatory in the D16B sequence** and may legitimately have no
    /// children — a minimal invoice delivers nothing and says nothing about a
    /// process. Pruning them produces a document that fails schema validation
    /// while looking tidier, so the two cases are distinguished at the call
    /// site rather than guessed at here.
    ///
    /// CII-only: UBL has no aggregate that is both mandatory and legitimately
    /// empty, so gating this keeps the `ubl`-only build free of dead code
    /// rather than merely free of a warning about it.
    #[cfg(feature = "cii")]
    pub fn group_required(&mut self, name: &str, f: impl FnOnce(&mut Self)) {
        self.stack.push(Node {
            name: name.to_owned(),
            attrs: Vec::new(),
            text: None,
            children: Vec::new(),
        });
        f(self);
        let node = self.stack.pop().expect("group pushed a node");
        self.push(node);
    }

    /// Serialise, ordering every level and enforcing the prohibitions.
    ///
    /// Returns the document and everything the syntax could not carry.
    pub fn finish(mut self) -> (String, Vec<String>) {
        let root = self.stack.pop().expect("the root");
        debug_assert!(self.stack.is_empty(), "unbalanced group()");
        let mut out = String::with_capacity(4096);
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        let mut dropped = std::mem::take(&mut self.notes);
        let name = root.name.clone();
        render(
            &root,
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
/// or a bare name matches at any depth. That distinction is load-bearing:
/// `/ubl:Invoice` + `not(cbc:UUID)` forbids a `cbc:UUID` **child of the
/// document element**, and treating it as floating would forbid the element
/// wherever it appears.
///
/// The document element is compared by local name, because a writer using a
/// default namespace emits `Invoice` where the Schematron writes `ubl:Invoice`.
/// Everything below it is compared qualified, as both sides spell it.
pub(crate) fn path_matches(path: &str, context: &str, relative: &str) -> bool {
    let floating = context.starts_with("//") || !context.starts_with('/');
    let ctx = context.trim_start_matches('/');
    if floating {
        let needle = format!("{ctx}/{relative}");
        return path == needle || path.ends_with(&format!("/{needle}"));
    }
    // Anchored: the whole path must be the context element followed by the
    // relative path, and only the first segment is matched loosely.
    let Some((head, rest)) = path.split_once('/') else {
        return false;
    };
    local(head) == local(ctx) && rest == relative
}

/// The local name, as the order tables key them — `cac:Party` → `Party`.
fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// Sort `children` into sequence order, reporting any the sequence cannot place.
///
/// Repeated elements keep the order they were written in: `sort_by_key` is
/// stable, so two invoice lines stay in line order.
fn order_children(parent: &str, children: &mut Vec<Node>, dropped: &mut Vec<String>, r: &Rules) {
    let Some(seq) = (r.order)(local(parent)) else {
        return; // no evidence for this parent; leave the writer's order alone
    };
    children.retain(|c| {
        let known = seq.contains(&local(&c.name));
        if !known {
            // The sequences were derived from the authorities' own instances.
            // An element absent from one is an element no published document
            // places there — `cac:ProjectReference` under `<CreditNote>`,
            // because UBL's credit note has no such element. Emitting it anyway
            // produces a document the counterparty's schema rejects, which is
            // worse than dropping it and saying so.
            dropped.push(format!("{}/{}", local(parent), c.name));
        }
        known
    });
    children.sort_by_key(|c| {
        seq.iter()
            .position(|e| *e == local(&c.name))
            .unwrap_or(usize::MAX)
    });
}

fn render(
    node: &Node,
    path: &str,
    depth: usize,
    out: &mut String,
    dropped: &mut Vec<String>,
    r: &Rules,
    waived: &[&str],
) {
    for _ in 0..depth {
        out.push_str("  ");
    }
    let _ = write!(out, "<{}", node.name);
    for (k, v) in &node.attrs {
        if let Some(rule) = (r.forbidden_attribute)(k) {
            dropped.push(format!("{path}/@{k} ({rule})"));
            continue;
        }
        let _ = write!(out, " {k}=\"");
        escape(v, out);
        out.push('"');
    }
    if node.children.is_empty() {
        // A required-but-empty aggregate serialises as `<x/>` rather than
        // `<x></x>`: both are the same infoset, and the short form is what
        // every producer in this field emits.
        if node.text.is_none() {
            let _ = writeln!(out, "/>");
            return;
        }
        out.push('>');
        escape(node.text.as_deref().unwrap_or_default(), out);
        let _ = writeln!(out, "</{}>", node.name);
        return;
    }
    let _ = writeln!(out, ">");
    let mut kids = node.children.clone();
    order_children(&node.name, &mut kids, dropped, r);
    for c in &kids {
        let child_path = if path.is_empty() {
            c.name.clone()
        } else {
            format!("{path}/{}", c.name)
        };
        // Enforcing here rather than trusting every call site is what makes
        // "the writer cannot emit a forbidden element" a property instead of a
        // habit — `cbc:CompanyLegalForm` is BT-33, the *seller's*, and
        // `UBL-CR-244` forbids it on the customer, which a hand-written writer
        // got wrong.
        if let Some(rule) = (r.forbidden_path)(&child_path)
            && !waived.contains(&rule)
        {
            dropped.push(format!("{child_path} ({rule})"));
            continue;
        }
        render(c, &child_path, depth + 1, out, dropped, r, waived);
    }
    for _ in 0..depth {
        out.push_str("  ");
    }
    let _ = writeln!(out, "</{}>", node.name);
}

/// Escape the five metacharacters, and drop control characters XML 1.0 cannot
/// represent at all.
pub(crate) fn escape(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
}

/// Base64, RFC 4648 §4, no line breaks.
///
/// Fifteen lines rather than a dependency, and both syntaxes need it: BT-125
/// carries an attachment's bytes inline.
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

/// Decode base64, RFC 4648 §4, ignoring whitespace.
///
/// Handing the *encoded* text back as an attachment's content is a bug that
/// survives every schema check and every rule: the document is valid, the
/// attachment is present, and the bytes are wrong. Only a round-trip finds it.
pub(crate) fn decode_base64(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => continue, // whitespace and line breaks are legal in XML text
        };
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
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    static NO_RULES: Rules = Rules {
        order: |_| None,
        forbidden_path: |_| None,
        forbidden_attribute: |_| None,
    };

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
            assert_eq!(decode_base64(encoded), plain, "round trip {encoded}");
        }
    }

    #[test]
    fn decoding_ignores_the_whitespace_xml_permits() {
        assert_eq!(decode_base64("Zm9v\n  YmFy"), b"foobar");
    }

    #[test]
    fn text_is_escaped() {
        let mut s = String::new();
        escape("x<y & \"z\" 'q'\u{7}", &mut s);
        assert_eq!(s, "x&lt;y &amp; &quot;z&quot; &apos;q&apos;");
    }

    #[test]
    fn empty_groups_vanish() {
        let mut x = Xml::new("Root", vec![], &NO_RULES);
        x.group("a:Empty", |_| {});
        let (xml, dropped) = x.finish();
        assert!(!xml.contains("Empty"), "{xml}");
        assert!(dropped.is_empty());
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
        assert!(dropped.is_empty());
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

    /// With no order table for a parent, the writer's own order is preserved
    /// rather than mangled — the fallback must be inert, not lossy.
    #[test]
    fn an_unknown_parent_keeps_the_writers_order() {
        let mut x = Xml::new("Root", vec![], &NO_RULES);
        x.leaf("a:Second", &[], "2");
        x.leaf("a:First", &[], "1");
        let (xml, dropped) = x.finish();
        assert!(xml.find("Second").unwrap() < xml.find("First").unwrap());
        assert!(dropped.is_empty());
    }
}
