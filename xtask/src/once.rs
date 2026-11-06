//! Deriving the single-occurrence set: which elements a document may carry at
//! most once. Readers read the first occurrence and report every further one.
//!
//! Two sources:
//!
//! * The schemas: for each parent element the authorities' instances show,
//!   every child its type declares with `maxOccurs="1"` (CII: CEN's uncoupled
//!   D16B subset schema; UBL: OASIS UBL 2.1), whether or not an instance
//!   carries it. The row's rule cell is empty.
//! * CEN's syntax rules from the preprocessed Schematron that count (e.g.
//!   `UBL-SR-09`; see [`clauses`] for the shapes understood). A path is
//!   a chain of child steps, optionally led by `//`, each with at most one
//!   predicate comparing a child's value (optionally upper-cased or
//!   whitespace-normalised) to a literal. Anything else goes to `UNCHECKED` with
//!   the reason.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::order::Syntax;
use crate::{Generated, escape, header};

const XS: &str = "http://www.w3.org/2001/XMLSchema";

/// The schema files whose types the readers walk, relative to the reference corpus.
fn schemas(syntax: Syntax, spec: &Path) -> Result<Vec<PathBuf>, String> {
    match syntax {
        Syntax::Ubl => Ok([
            "oasis-ubl-2.1/xsd/maindoc/UBL-Invoice-2.1.xsd",
            "oasis-ubl-2.1/xsd/maindoc/UBL-CreditNote-2.1.xsd",
            "oasis-ubl-2.1/xsd/common/UBL-CommonAggregateComponents-2.1.xsd",
        ]
        .iter()
        .map(|p| spec.join(p))
        .collect()),
        Syntax::Cii => {
            let dir = spec.join(
                "eInvoicing-EN16931/cii/schema/D16B SCRDM (Subset)/uncoupled clm/CII/uncefact/data/standard",
            );
            let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
                .map_err(|e| format!("{}: {e}", dir.display()))?
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "xsd"))
                .collect();
            out.sort();
            Ok(out)
        }
    }
}

fn artefact(syntax: Syntax) -> &'static str {
    match syntax {
        Syntax::Ubl => {
            "eInvoicing-EN16931/ubl/schematron/preprocessed/EN16931-UBL-validation-preprocessed.sch"
        }
        Syntax::Cii => {
            "eInvoicing-EN16931/cii/schematron/preprocessed/EN16931-CII-validation-preprocessed.sch"
        }
    }
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// Element name → the types it is declared with; type → its children in
/// sequence order, each with whether it is allowed once.
#[derive(Default)]
struct Model {
    element_types: BTreeMap<String, BTreeSet<String>>,
    types: BTreeMap<String, Vec<(String, bool)>>,
}

impl Model {
    fn read(files: &[PathBuf]) -> Result<Self, String> {
        let mut m = Self::default();
        for path in files {
            let text =
                std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let doc = roxmltree::Document::parse(&text)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            for n in doc
                .root_element()
                .children()
                .filter(roxmltree::Node::is_element)
            {
                let is = |want: &str| {
                    n.tag_name().namespace() == Some(XS) && n.tag_name().name() == want
                };
                if is("element")
                    && let (Some(name), Some(ty)) = (n.attribute("name"), n.attribute("type"))
                {
                    m.element_types
                        .entry(name.to_owned())
                        .or_default()
                        .insert(local(ty).to_owned());
                } else if is("complexType")
                    && let Some(ty) = n.attribute("name")
                {
                    m.complex_type(ty, n);
                }
            }
        }
        Ok(m)
    }

    fn complex_type(&mut self, ty: &str, node: roxmltree::Node<'_, '_>) {
        let mut children: Vec<(String, bool)> = Vec::new();
        for d in node
            .descendants()
            .filter(|d| d.tag_name().namespace() == Some(XS) && d.tag_name().name() == "element")
        {
            let Some(child) = d
                .attribute("name")
                .or_else(|| d.attribute("ref").map(local))
            else {
                continue;
            };
            if let (Some(name), Some(t)) = (d.attribute("name"), d.attribute("type")) {
                self.element_types
                    .entry(name.to_owned())
                    .or_default()
                    .insert(local(t).to_owned());
            }
            // Once only if the element and every particle between it and the
            // type say once.
            let repeated =
                |x: roxmltree::Node<'_, '_>| x.attribute("maxOccurs").unwrap_or("1") != "1";
            let once = !repeated(d)
                && !d
                    .ancestors()
                    .skip(1)
                    .take_while(|a| a.id() != node.id())
                    .any(repeated);
            // The same name twice in one content model is a repetition.
            match children.iter_mut().find(|(c, _)| c == child) {
                Some((_, o)) => *o = false,
                None => children.push((child.to_owned(), once)),
            }
        }
        self.types.insert(ty.to_owned(), children);
    }

    /// Whether `child` is allowed once under `parent`: every type `parent` is
    /// declared with that has `child` says so, and at least one has it.
    fn once(&self, parent: &str, child: &str) -> bool {
        let Some(types) = self.element_types.get(parent) else {
            return false;
        };
        let verdicts: Vec<bool> = types
            .iter()
            .filter_map(|t| {
                self.types
                    .get(t)
                    .and_then(|c| c.iter().find(|(n, _)| n == child))
                    .map(|(_, o)| *o)
            })
            .collect();
        !verdicts.is_empty() && verdicts.iter().all(|o| *o)
    }

    /// Every child any of `parent`'s types declares.
    fn children(&self, parent: &str) -> BTreeSet<&str> {
        self.element_types
            .get(parent)
            .into_iter()
            .flatten()
            .filter_map(|t| self.types.get(t))
            .flatten()
            .map(|(c, _)| c.as_str())
            .collect()
    }
}

/// Element name → the child sequence of its type, for every element declared
/// with a complex type that has children.
///
/// # Errors
///
/// A schema is unreadable, or one element name is declared with two types
/// whose sequences differ.
pub fn sequences(syntax: Syntax, spec: &Path) -> Result<BTreeMap<String, Vec<String>>, String> {
    let model = Model::read(&schemas(syntax, spec)?)?;
    let mut out = BTreeMap::new();
    for (element, types) in &model.element_types {
        let mut seqs = types
            .iter()
            .filter_map(|t| model.types.get(t))
            .filter(|c| !c.is_empty())
            .map(|c| c.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>());
        let Some(first) = seqs.next() else { continue };
        if seqs.any(|s| s != first) {
            return Err(format!(
                "<{element}> is declared with two types whose sequences differ"
            ));
        }
        out.insert(element.clone(), first);
    }
    Ok(out)
}

/// Every `(parent, child)` pair the authorities' instances use.
fn observed(syntax: Syntax, files: &[PathBuf]) -> BTreeSet<(String, String)> {
    let roots: &[&str] = match syntax {
        Syntax::Ubl => &["Invoice", "CreditNote"],
        Syntax::Cii => &["CrossIndustryInvoice"],
    };
    let mut out = BTreeSet::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(doc) = roxmltree::Document::parse(&text) else {
            continue;
        };
        if !roots.contains(&doc.root_element().tag_name().name()) {
            continue;
        }
        for n in doc.descendants().filter(roxmltree::Node::is_element) {
            for c in n.children().filter(roxmltree::Node::is_element) {
                out.insert((
                    n.tag_name().name().to_owned(),
                    c.tag_name().name().to_owned(),
                ));
            }
        }
    }
    out
}

fn is_chain(s: &str) -> bool {
    !s.is_empty()
        && s.split('/').all(|seg| {
            let mut halves = seg.split(':');
            matches!((halves.next(), halves.next(), halves.next()),
                (Some(p), Some(l), None) if !p.is_empty() && !l.is_empty()
                    && p.chars().all(|c| c.is_alphanumeric())
                    && l.chars().all(|c| c.is_alphanumeric() || c == '_'))
        })
}

/// Split on `/` outside brackets.
fn steps(path: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0usize, 0usize);
    for (i, c) in path.char_indices() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
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

/// A predicate body in the canonical spelling the reader compiles:
/// `upper:a/b=LIT`, `norm:a=LIT`, `a!=LIT`.
fn predicate(body: &str) -> Result<String, &'static str> {
    if body.contains("preceding::") || body.starts_with("not(") {
        return Err("a distinct-value count, which is not a count of occurrences");
    }
    if body.contains('@') {
        return Err("an attribute predicate");
    }
    let (lhs, op, rhs) = match body.split_once("!=") {
        Some((l, r)) => (l, "!=", r),
        None => {
            let (l, r) = body
                .split_once('=')
                .ok_or("a predicate that is not a comparison")?;
            (l, "=", r)
        }
    };
    let lit = rhs
        .trim()
        .strip_prefix('\'')
        .and_then(|r| r.strip_suffix('\''))
        .ok_or("a comparison with something other than a literal")?;
    let lhs = lhs.trim();
    let (prefix, chain) = if let Some(inner) = lhs
        .strip_prefix("normalize-space(")
        .and_then(|r| r.strip_suffix(')'))
    {
        ("norm:", inner.trim().to_owned())
    } else if let Some((head, tail)) = lhs.rsplit_once("upper-case(") {
        let inner = tail.strip_suffix(')').ok_or("an unbalanced upper-case()")?;
        ("upper:", format!("{head}{inner}"))
    } else {
        ("", lhs.to_owned())
    };
    if !is_chain(&chain) {
        return Err("a predicate on something other than an element path");
    }
    Ok(format!("{prefix}{chain}{op}{lit}"))
}

/// The canonical relative path, or why it cannot be expressed.
fn relative(path: &str) -> Result<String, &'static str> {
    let (lead, body) = match path.strip_prefix("//") {
        Some(rest) => ("//", rest),
        None => ("", path),
    };
    let mut out = Vec::new();
    for step in steps(body) {
        let (name, pred) = match step.split_once('[') {
            Some((n, p)) => (
                n,
                Some(p.strip_suffix(']').ok_or("an unbalanced predicate")?),
            ),
            None => (step, None),
        };
        if name.starts_with('@') || name.contains('.') {
            return Err("an attribute or value count, not an element count");
        }
        if !is_chain(name) || name.contains('/') {
            return Err("a step that is not an element name");
        }
        out.push(match pred {
            Some(p) => format!("{name}[{}]", predicate(p.trim())?),
            None => name.to_owned(),
        });
    }
    Ok(format!("{lead}{}", out.join("/")))
}

/// Split on ` and ` / ` or ` outside brackets.
fn top_level<'a>(test: &'a str, op: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0usize, 0usize);
    let b = test.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth = depth.saturating_sub(1),
            _ if depth == 0 && test[i..].starts_with(op) => {
                out.push(test[start..i].trim());
                i += op.len();
                start = i;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(test[start..].trim());
    out
}

/// `(context predicate, counted path)` per clause of a syntax rule's test.
type Clauses = Vec<(Option<String>, String)>;

/// What a syntax rule's test says about occurrences: `(context predicate,
/// counted path)` per clause, `None` for a test that counts nothing, or why
/// it cannot be expressed. Three shapes are understood:
///
/// * `count(p) <= 1` or `count(p) = 1`, alone or as a conjunction of them;
/// * `not(g) or count(g/…) = 1` (or `<= 1`): at most once, whether `g` is
///   there or not — the "at least once" half is not this table's;
/// * `normalize-space(x) != 'L' or count(p) <= 1`: the same rule under the
///   context predicate `[normalize-space(x) = 'L']`.
fn clauses(test: &str) -> Option<Result<Clauses, &'static str>> {
    if !test.contains("count(") {
        return None;
    }
    let t = test.trim();
    let t = t
        .strip_prefix('(')
        .and_then(|r| r.strip_suffix(')'))
        .filter(|r| !r.contains(") or (") && !r.contains(") and ("))
        .map_or(t, str::trim);
    if t.contains("distinct-values(") {
        return Some(Err(
            "a distinct-value count, which is not a count of occurrences",
        ));
    }
    let ands = top_level(t, " and ");
    if ands.len() > 1 {
        let paths: Option<Vec<_>> = ands
            .iter()
            .map(|c| counted(c).or_else(|| exactly_one(c)))
            .collect();
        return Some(
            paths
                .map(|ps| ps.into_iter().map(|p| (None, p.to_owned())).collect())
                .ok_or("a conjunction with a clause that is not `count(…) <= 1`"),
        );
    }
    let ors = top_level(t, " or ");
    if let [guard, count] = ors.as_slice() {
        let path = counted(count).or_else(|| exactly_one(count));
        let Some(path) = path else {
            return Some(Err("a count inside a condition this table cannot express"));
        };
        if let Some(g) = guard.strip_prefix("not(").and_then(|r| r.strip_suffix(')')) {
            let g = g.trim();
            return Some(if path == g || path.starts_with(&format!("{g}/")) {
                Ok(vec![(None, path.to_owned())])
            } else {
                Err("a count guarded by an unrelated absence")
            });
        }
        if let Some((lhs, rhs)) = guard.split_once("!=") {
            return Some(Ok(vec![(
                Some(format!("{} = {}", lhs.trim(), rhs.trim())),
                path.to_owned(),
            )]));
        }
        return Some(Err("a count inside a condition this table cannot express"));
    }
    // `count(p) = 1` is "exactly once"; its "at most once" half is this table's.
    match counted(t).or_else(|| exactly_one(t)) {
        Some(p) => Some(Ok(vec![(None, p.to_owned())])),
        None => Some(Err("a count this table cannot express")),
    }
}

/// `count(x) = 1` — `x`.
fn exactly_one(test: &str) -> Option<&str> {
    let (lhs, rhs) = test.trim().rsplit_once('=')?;
    if rhs.trim() != "1" || lhs.ends_with('<') || lhs.ends_with('!') {
        return None;
    }
    lhs.trim().strip_prefix("count(")?.strip_suffix(')')
}

/// `count(x) <= 1`, with or without parentheses and spaces — `x`.
fn counted(test: &str) -> Option<&str> {
    let t = test.trim();
    let t = t
        .strip_prefix('(')
        .and_then(|r| r.strip_suffix(')'))
        .map_or(t, str::trim);
    let (lhs, rhs) = t.rsplit_once("<=")?;
    if rhs.trim() != "1" {
        return None;
    }
    lhs.trim().strip_prefix("count(")?.strip_suffix(')')
}

type Row = (String, String, String, String);

/// A rule context's branches in the canonical spelling: element chains, the
/// last step optionally carrying one predicate.
fn contexts(context: &str) -> Result<Vec<String>, &'static str> {
    context
        .split('|')
        .map(str::trim)
        .map(|b| {
            let lead = if b.starts_with("//") {
                "//"
            } else if b.starts_with('/') {
                "/"
            } else {
                ""
            };
            let body = b.trim_start_matches('/');
            let parts = steps(body);
            let (last, init) = parts.split_last().ok_or("an empty context")?;
            if !init.iter().all(|s| is_chain(s)) {
                return Err("a conditional rule context");
            }
            let last = match last.split_once('[') {
                Some((name, pred)) if is_chain(name) => format!(
                    "{name}[{}]",
                    predicate(
                        pred.strip_suffix(']')
                            .ok_or("an unbalanced predicate")?
                            .trim()
                    )?
                ),
                None if is_chain(last) => (*last).to_owned(),
                _ => return Err("a conditional rule context"),
            };
            let mut chain: Vec<String> = init.iter().map(|s| (*s).to_owned()).collect();
            chain.push(last);
            Ok(format!("{lead}{}", chain.join("/")))
        })
        .collect()
}

/// `(rule id, why it is not expressible)` for assertions outside the subset.
type Unchecked = Vec<(String, String)>;

fn rules(syntax: Syntax, spec: &Path) -> Result<(Vec<Row>, Unchecked), String> {
    let path = spec.join(artefact(syntax));
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let doc = roxmltree::Document::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut rows = Vec::new();
    let mut unchecked = Vec::new();
    for rule in doc.descendants().filter(|n| n.has_tag_name("rule")) {
        let context = rule.attribute("context").unwrap_or("").trim();
        for a in rule.children().filter(|n| n.has_tag_name("assert")) {
            let (Some(id), Some(test)) = (a.attribute("id"), a.attribute("test")) else {
                continue;
            };
            if id.split('-').nth(1) != Some("SR") {
                continue;
            }
            // Every syntax rule that counts is either expressed or listed with
            // its reason — never skipped.
            let Some(parsed) = clauses(test) else {
                continue;
            };
            let level = a.attribute("flag").unwrap_or("fatal");
            let expressed = parsed.and_then(|cs| {
                let mut out = Vec::new();
                for (guard, path) in cs {
                    let rel = relative(&path)?;
                    let guard = guard.map(|g| predicate(&g)).transpose()?;
                    for b in contexts(context).map_err(|_| "a conditional rule context")? {
                        let b = match &guard {
                            None => b,
                            Some(_) if b.ends_with(']') => {
                                return Err("a guard on a context that already has a predicate");
                            }
                            Some(g) => format!("{b}[{g}]"),
                        };
                        out.push((id.to_owned(), level.to_owned(), b, rel.clone()));
                    }
                }
                Ok(out)
            });
            match expressed {
                Ok(r) => rows.extend(r),
                Err(why) => unchecked.push((id.to_owned(), why.to_owned())),
            }
        }
    }
    if rows.is_empty() {
        return Err(format!(
            "{} yielded no `count(…) <= 1` syntax rules — has the artefact layout changed?",
            path.display()
        ));
    }
    Ok((rows, unchecked))
}

/// Derive `src/{ubl,cii}/once.rs`.
///
/// # Errors
///
/// A schema or the Schematron is missing or unreadable, or either source
/// yields nothing.
pub fn derive(
    syntax: Syntax,
    files: &[PathBuf],
    spec: &Path,
    root: &Path,
) -> Result<Generated, String> {
    let model = Model::read(&schemas(syntax, spec)?)?;
    let mut rows: BTreeSet<Row> = BTreeSet::new();
    // Every child the type declares, not only those an instance carries: a
    // second occurrence the corpus never shows is still a second occurrence.
    let parents: BTreeSet<String> = observed(syntax, files)
        .into_iter()
        .map(|(parent, _)| parent)
        .collect();
    for parent in &parents {
        for child in model.children(parent) {
            if model.once(parent, child) {
                rows.insert((
                    String::new(),
                    "fatal".to_owned(),
                    parent.clone(),
                    child.to_owned(),
                ));
            }
        }
    }
    let schema_rows = rows.len();
    if schema_rows == 0 {
        return Err(format!(
            "no single-occurrence pair derived for {} — are the schemas fetched?",
            match syntax {
                Syntax::Ubl => "UBL",
                Syntax::Cii => "CII",
            }
        ));
    }
    let (rule_rows, mut unchecked) = rules(syntax, spec)?;
    let rule_count = rule_rows.len();
    rows.extend(rule_rows);
    unchecked.sort();
    unchecked.dedup();
    Ok(render(
        syntax,
        &rows,
        schema_rows,
        rule_count,
        &unchecked,
        parents.len(),
        root,
    ))
}

fn render(
    syntax: Syntax,
    rows: &BTreeSet<Row>,
    schema_rows: usize,
    rule_rows: usize,
    unchecked: &[(String, String)],
    parents: usize,
    root: &Path,
) -> Generated {
    let (label, module, schema) = match syntax {
        Syntax::Ubl => ("UBL", "ubl", "the OASIS UBL 2.1 schemas"),
        Syntax::Cii => ("CII", "cii", "CEN's uncoupled D16B subset schema"),
    };
    let body: String = rows
        .iter()
        .map(|(rule, level, ctx, rel)| {
            format!(
                "    (\"{}\", \"{}\", \"{}\", \"{}\"),\n",
                escape(rule),
                escape(level),
                escape(ctx),
                escape(rel)
            )
        })
        .collect();
    let unchecked_body: String = unchecked
        .iter()
        .map(|(id, why)| format!("    (\"{}\", \"{}\"),\n", escape(id), escape(why)))
        .collect();
    let doc = header(
        &format!("The {label} single-occurrence set, from {schema} and CEN's syntax rules."),
        &format!(
            "//! A reader reads the **first** occurrence of each element below and reports\n\
             //! every further one; nothing of a second occurrence reaches the model.\n\
             //!\n\
             //! * {schema_rows} rows come from the schema: for each of the {parents} parent\n\
             //!   elements the authorities' instances show, every child {schema}\n\
             //!   declare `maxOccurs=\"1\"` in its type. Their rule cell is empty.\n\
             //! * {rule_rows} rows come from CEN's `count(…) <= 1` syntax rules, with the\n\
             //!   level CEN publishes for each — and, where KoSIT replaces a CEN rule whose\n\
             //!   context is wrong, from KoSIT's replacement.\n\
             //! * [`UNCHECKED`] lists the {n} such rules this table cannot express, and why.\n\
             //!\n\
             //! A context beginning `/` anchors at the document element; `//` or a bare\n\
             //! name matches at any depth; a context's last step may carry a predicate.\n\
             //! A predicate is written\n\
             //! `[path=literal]`, `[path!=literal]`, with `upper:` or `norm:` when CEN\n\
             //! compares the value upper-cased or with its whitespace normalised.\n",
            n = unchecked.len(),
        ),
    );
    let contents = format!(
        "{doc}\n\
         /// `(rule, level, context, relative path)` — an empty rule is the schema's.\n\
         pub static ONCE: &[(&str, &str, &str, &str)] = &[\n\
         {body}];\n\
         \n\
         /// `count(…) <= 1` syntax rules this table cannot express, with the reason.\n\
         pub static UNCHECKED: &[(&str, &str)] = &[\n\
         {unchecked_body}];\n"
    );
    Generated {
        path: root.join("src").join(module).join("once.rs"),
        contents,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_recognised_with_or_without_parentheses() {
        assert_eq!(counted("(count(cac:A/cbc:B) <= 1)"), Some("cac:A/cbc:B"));
        assert_eq!(
            counted("count(ram:X) &lt;= 1".replace("&lt;", "<").as_str()),
            Some("ram:X")
        );
        assert_eq!(counted("( count(ram:A/ram:B)  <= 1)"), Some("ram:A/ram:B"));
        assert_eq!(counted("count(x) <= 2"), None);
        assert_eq!(counted("not(cbc:X)"), None);
    }

    #[test]
    fn conjunctions_guards_and_exactly_one_are_understood() {
        let ok = |t: &str| clauses(t).unwrap().unwrap();
        assert_eq!(
            ok("count(a:P/b:X) <= 1 and count(a:Q/b:X) <= 1"),
            [(None, "a:P/b:X".to_owned()), (None, "a:Q/b:X".to_owned())]
        );
        assert_eq!(
            ok("not(a:D) or count(a:D/b:N) = 1"),
            [(None, "a:D/b:N".to_owned())]
        );
        assert_eq!(
            ok("normalize-space(ram:TypeCode) != '916' or count(ram:Name) <= 1"),
            [(
                Some("normalize-space(ram:TypeCode) = '916'".to_owned()),
                "ram:Name".to_owned()
            )]
        );
        assert!(
            clauses("count(distinct-values(//cbc:PaymentID)) <= 1")
                .unwrap()
                .is_err()
        );
        assert_eq!(ok("not(a:L) or count(a:L) = 1"), [(None, "a:L".to_owned())]);
        assert!(clauses("not(a:D) or count(b:N) = 1").unwrap().is_err());
        assert!(clauses("exists(cbc:ID)").is_none());
    }

    #[test]
    fn predicates_are_canonicalised_or_refused() {
        assert_eq!(
            relative("cac:AdditionalDocumentReference[cbc:DocumentTypeCode='130']/cbc:ID"),
            Ok("cac:AdditionalDocumentReference[cbc:DocumentTypeCode=130]/cbc:ID".to_owned())
        );
        assert_eq!(
            relative("cac:P/cac:T[cac:TaxScheme/upper-case(cbc:ID)!='VAT']/cbc:C"),
            Ok("cac:P/cac:T[upper:cac:TaxScheme/cbc:ID!=VAT]/cbc:C".to_owned())
        );
        assert_eq!(
            relative("ram:A[normalize-space(ram:TypeCode) = '916']/ram:Name"),
            Ok("ram:A[norm:ram:TypeCode=916]/ram:Name".to_owned())
        );
        assert_eq!(
            relative("//ram:S/ram:PaymentReference"),
            Ok("//ram:S/ram:PaymentReference".to_owned())
        );
        assert!(relative("cac:PaymentMeans/cbc:PaymentMeansCode/@name").is_err());
        assert!(relative("//cbc:PaymentID[not(preceding::cbc:PaymentID/. = .)]").is_err());
        assert!(relative("//cac:P/cbc:ID[upper-case(@schemeID) = 'SEPA']").is_err());
    }
}
