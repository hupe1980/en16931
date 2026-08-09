//! Extracting the syntax rules' prohibitions — **with their contexts**.
//!
//! # The context is half the rule
//!
//! `CII-DT-076` is `not(ram:ID)`. It does **not** say "no document may contain
//! `ram:ID`"; it says "the element this rule's `context` selects may not have an
//! `ram:ID` child". An earlier version of this extractor kept the test and threw
//! the context away, turning a narrow prohibition into a blanket one — and the
//! serialiser duly discarded every `ram:ID` in every document it wrote.
//!
//! So the source is the **preprocessed** Schematron, where `<rule context="…">`
//! carries a fully resolved XPath instead of a `$Variable` reference, and each
//! entry is `(rule, context, path relative to that context)`.
//!
//! # What is deliberately not extracted
//!
//! A context can be conditional — a predicate, `ends-with(name(), 'Amount')`, a
//! wildcard. Selecting the elements it matches needs an XPath engine this crate
//! does not have and will not grow one for. Those are **counted** and the count
//! is emitted as `UNEXTRACTED`, so the test that uses these tables reports
//! "1 045 of 1 268 checked" rather than implying a clean sweep.

use std::path::{Path, PathBuf};

use crate::order::Syntax;
use crate::{Generated, Set, escape, header};

/// Where each syntax's preprocessed artefact lives, relative to `spec/`.
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

/// Is this a syntax-rule id — `UBL-CR-123`, `CII-DT-07`, `UBL-SR-4`?
///
/// Business rules (`BR-*`) live in the same file and are `en16931`'s job, so
/// they are skipped rather than mistaken for prohibitions.
fn is_syntax_rule(id: &str) -> bool {
    let mut parts = id.split('-');
    let (Some(prefix), Some(kind), Some(num), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    !prefix.is_empty()
        && prefix.chars().all(|c| c.is_ascii_uppercase())
        && matches!(kind, "CR" | "SR" | "DT")
        && !num.is_empty()
        && num.chars().all(|c| c.is_ascii_digit())
}

/// A chain of qualified element names — `cac:Party/cbc:Name`.
///
/// Anything else (a predicate, a function call, a wildcard) is a condition this
/// crate cannot evaluate by comparing paths.
fn is_element_chain(s: &str) -> bool {
    !s.is_empty()
        && s.split('/').all(|seg| {
            let mut halves = seg.split(':');
            let (Some(prefix), Some(local), None) = (halves.next(), halves.next(), halves.next())
            else {
                return false;
            };
            !prefix.is_empty()
                && prefix.chars().next().is_some_and(char::is_alphabetic)
                && prefix
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '-')
                && !local.is_empty()
                && local
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '-')
        })
}

/// `not( … )`, with the contents trimmed — or `None` if it is not that shape.
fn strip_not(test: &str) -> Option<&str> {
    test.strip_prefix("not(")?.strip_suffix(')').map(str::trim)
}

/// Expand a forbidden path that alternates, into one plain chain per branch.
///
/// # The shape this exists for
///
/// UBL has two document elements, so a rule about a line is written once and
/// selects both:
///
/// ```xpath
/// not((cac:InvoiceLine|cac:CreditNoteLine)/cac:SubInvoiceLine)
/// ```
///
/// That is not an element chain, so it used to be counted as unextractable —
/// and it is **131 of UBL's 163**, four fifths of everything the table was
/// missing, all of it the same purely notational `Invoice`/`CreditNote` split.
/// `cac:SubInvoiceLine` was among them, which is how the writer came to emit an
/// element CEN forbids with nothing to notice.
///
/// Two forms are handled and nothing else:
///
/// * `(a|b)/tail` — a leading alternation with a common tail;
/// * `a|b` — a bare alternation.
///
/// A predicate, a function call or a wildcard still needs an XPath engine, and
/// those stay counted rather than guessed at.
fn expand_alternation(inner: &str) -> Option<Vec<String>> {
    if is_element_chain(inner) {
        return Some(vec![inner.to_owned()]);
    }
    // `(a|b)/tail`
    if let Some(rest) = inner.strip_prefix('(')
        && let Some((head, tail)) = rest.split_once(')')
        && !head.contains('(')
    {
        let tail = tail.trim();
        // Either nothing after the group, or `/` and a plain chain.
        let suffix = match tail.strip_prefix('/') {
            Some(t) if is_element_chain(t) => Some(t),
            None if tail.is_empty() => None,
            _ => return None,
        };
        let mut out = Vec::new();
        for branch in head.split('|').map(str::trim) {
            if !is_element_chain(branch) {
                return None;
            }
            out.push(match suffix {
                Some(t) => format!("{branch}/{t}"),
                None => branch.to_owned(),
            });
        }
        return (!out.is_empty()).then_some(out);
    }
    // `a|b`
    if inner.contains('|') {
        let branches: Vec<&str> = inner.split('|').map(str::trim).collect();
        if branches.iter().all(|b| is_element_chain(b)) {
            return Some(branches.into_iter().map(str::to_owned).collect());
        }
    }
    None
}

/// Extract the prohibition tables for `syntax`.
///
/// # Errors
///
/// Fails if the artefact is missing or unparsable, or if it yields no
/// prohibitions at all — which would mean the layout changed and the extractor
/// is silently producing an empty table.
pub fn extract(syntax: Syntax, spec: &Path, root: &Path) -> Result<Generated, String> {
    let path = spec.join(artefact(syntax));
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let doc = roxmltree::Document::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;

    // Sets, so the same prohibition appearing under several contexts is counted
    // once per (context, path) pair and the output is byte-stable.
    let mut paths: Set = Set::new();
    let mut attrs: Set = Set::new();
    let mut unextracted = 0usize;
    // Counted per **assertion**, not per emitted row. Expanding
    // `(cac:InvoiceLine|cac:CreditNoteLine)/x` yields two rows for one rule, and
    // a total that grew with the expansion would report the artefact getting
    // bigger every time the extractor got better at reading it.
    let mut assertions = 0usize;

    for rule in doc.descendants().filter(|n| n.has_tag_name("rule")) {
        let context = rule.attribute("context").unwrap_or("").trim();
        for assertion in rule.children().filter(|n| n.has_tag_name("assert")) {
            let Some(id) = assertion.attribute("id") else {
                continue;
            };
            if !is_syntax_rule(id) {
                continue;
            }
            let Some(test) = assertion.attribute("test").map(str::trim) else {
                continue;
            };
            let Some(inner) = strip_not(test) else {
                continue; // a count(…) or string-length(…) rule, not a prohibition
            };

            // `//@languageID` — forbidden anywhere, no context needed.
            assertions += 1;
            if let Some(attr) = inner.strip_prefix("//@")
                && !attr.is_empty()
                && attr.chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                attrs.insert(format!("(\"{}\", \"{}\"),", escape(id), escape(attr)));
                continue;
            }
            let Some(forbidden) = expand_alternation(inner) else {
                unextracted += 1;
                continue;
            };

            // A context may be an alternation: `/ubl:Invoice | /cn:CreditNote`
            // is where most of UBL's prohibitions live, because they forbid an
            // element on the document root and UBL has two roots. Every branch
            // must be usable, or the rule is only half-applied — which is worse
            // than not applying it.
            let branches: Vec<&str> = context.split('|').map(str::trim).collect();
            let usable = branches
                .iter()
                .all(|b| is_element_chain(b.trim_start_matches('/')) && !b.is_empty());
            if !usable || branches.is_empty() {
                unextracted += 1;
                continue;
            }
            for b in branches {
                for rel in &forbidden {
                    // The leading slashes are kept: `/ubl:Invoice` anchors at
                    // the document element, `//cac:PostalAddress` matches at any
                    // depth, and conflating them forbids root elements
                    // everywhere.
                    paths.insert(format!(
                        "(\"{}\", \"{}\", \"{}\"),",
                        escape(id),
                        escape(b),
                        escape(rel)
                    ));
                }
            }
        }
    }

    if paths.is_empty() && attrs.is_empty() {
        return Err(format!(
            "{} yielded no prohibitions — has the artefact layout changed?",
            path.display()
        ));
    }

    Ok(render(
        syntax,
        &paths,
        &attrs,
        unextracted,
        assertions,
        root,
    ))
}

fn render(
    syntax: Syntax,
    paths: &Set,
    attrs: &Set,
    unextracted: usize,
    assertions: usize,
    root: &Path,
) -> Generated {
    let (label, module) = match syntax {
        Syntax::Ubl => ("UBL", "ubl"),
        Syntax::Cii => ("CII", "cii"),
    };
    let total = assertions;
    let checked = assertions - unextracted;
    let rows = paths.len() + attrs.len();

    // `Set` sorts by the rendered line, which starts with the rule id — stable
    // across runs and readable in a diff.
    let path_body: String = paths.iter().map(|l| format!("    {l}\n")).collect();
    let attr_body: String = attrs.iter().map(|l| format!("    {l}\n")).collect();

    let doc = header(
        &format!("{label} prohibitions, extracted from CEN's preprocessed Schematron."),
        &format!(
            "//! # Each prohibition is context-relative, and the context is the point\n\
             //!\n\
             //! `{label}-…` rules of the form `not(x)` do **not** say \"no document may\n\
             //! contain `x`\". They say \"the element this rule's context selects may not\n\
             //! have an `x` child\". An earlier version of this table dropped the context\n\
             //! and turned narrow prohibitions into blanket ones — which made the writer\n\
             //! discard `ram:ID` everywhere. So each entry carries\n\
             //! `(rule, context, relative path)`, and a match requires both halves.\n\
             //!\n\
             //! Source is the **preprocessed** artefact, where `<rule context=\"…\">` is a\n\
             //! fully resolved XPath rather than a `$Variable` reference.\n\
             //!\n\
             //! {checked} of the artefact's {total} `not(…)` assertions are represented,\n\
             //! as {p} element and {a} attribute rows — more rows than assertions,\n\
             //! because an alternation like\n\
             //! `(cac:InvoiceLine|cac:CreditNoteLine)/x` is one rule and two paths.\n\
             //!\n\
             //! [`UNEXTRACTED`] = {unextracted} are not represented: their test is\n\
             //! conditional (a predicate, `ends-with(…)`, a wildcard) and selecting what\n\
             //! it matches needs an XPath engine this crate does not have. The number is\n\
             //! public so a test reports \"{checked} of {total} checked\" rather than\n\
             //! implying a clean sweep.\n",
            p = paths.len(),
            a = attrs.len(),
        ),
    );

    let contents = format!(
        "{doc}\n\
         /// `(rule id, context, forbidden path relative to that context)`.\n\
         ///\n\
         /// A context beginning `/` anchors at the document element; `//` or a bare\n\
         /// name matches at any depth.\n\
         pub static FORBIDDEN_PATHS: &[(&str, &str, &str)] = &[\n\
         {path_body}];\n\
         \n\
         /// `(rule id, attribute name)` — prohibited anywhere in the document.\n\
         pub static FORBIDDEN_ATTRIBUTES: &[(&str, &str)] = &[\n\
         {attr_body}];\n\
         \n\
         /// Prohibitions whose test this crate cannot evaluate, and which\n\
         /// [`FORBIDDEN_PATHS`] therefore does **not** cover.\n\
         pub const UNEXTRACTED: usize = {unextracted};\n\
         \n\
         /// Every `{label}-*` `not(…)` assertion in the artefact, checkable or not.\n\
         ///\n\
         /// One per **assertion**. The tables above hold {rows} rows, which is more,\n\
         /// because one alternating rule expands to one row per branch.\n\
         pub const TOTAL_PARAMS: usize = {total};\n"
    );

    Generated {
        path: root
            .join("src")
            .join(module)
            .join("prohibitions_generated.rs"),
        contents,
    }
}

/// Keeps the import list honest — `PathBuf` is used by [`Generated`].
#[allow(dead_code)]
type _Unused = PathBuf;

#[cfg(test)]
mod tests {
    use super::*;

    /// The expansion is a string transformation over an XPath subset, so its
    /// edges are worth pinning: the difference between reading a rule and
    /// mis-reading one is a forbidden element the writer is then free to emit.
    #[test]
    fn alternations_expand_into_one_chain_per_branch() {
        let expand = |s: &str| expand_alternation(s);

        // A plain chain passes through.
        assert_eq!(expand("cbc:UUID"), Some(vec!["cbc:UUID".to_owned()]));
        assert_eq!(
            expand("cac:Party/cbc:Name"),
            Some(vec!["cac:Party/cbc:Name".to_owned()])
        );

        // The shape 131 of UBL's rules use, and the one that matters.
        assert_eq!(
            expand("(cac:InvoiceLine|cac:CreditNoteLine)/cac:SubInvoiceLine"),
            Some(vec![
                "cac:InvoiceLine/cac:SubInvoiceLine".to_owned(),
                "cac:CreditNoteLine/cac:SubInvoiceLine".to_owned(),
            ])
        );
        // Whitespace around the branches is the artefact's, not ours.
        assert_eq!(
            expand("( cac:A | cac:B )/cbc:C"),
            Some(vec!["cac:A/cbc:C".to_owned(), "cac:B/cbc:C".to_owned()])
        );
        // A group with no tail, and a bare alternation.
        assert_eq!(
            expand("(cac:A|cac:B)"),
            Some(vec!["cac:A".to_owned(), "cac:B".to_owned()])
        );
        assert_eq!(
            expand("cac:A|cac:B"),
            Some(vec!["cac:A".to_owned(), "cac:B".to_owned()])
        );
        // Three branches, and a multi-segment tail.
        assert_eq!(
            expand("(cac:A|cac:B|cac:C)/cac:D/cbc:E").map(|v| v.len()),
            Some(3)
        );
    }

    /// Anything needing an XPath engine must stay **unexpanded**, because a
    /// half-understood context is worse than an admitted gap: it forbids
    /// elements the rule never mentioned.
    #[test]
    fn conditional_tests_are_refused_rather_than_guessed_at() {
        for hard in [
            "cac:Party[cbc:Name]",              // a predicate
            "//*[ends-with(name(), 'Amount')]", // a function
            "cac:*",                            // a wildcard
            "@languageID",                      // an attribute
            "(cac:A|cac:B[x])/cbc:C",           // a predicate inside a branch
            "(cac:A|cac:B)[1]",                 // a predicate after the group
            "(cac:A|(cac:B|cac:C))/cbc:D",      // nested groups
            "",
        ] {
            assert_eq!(expand_alternation(hard), None, "{hard} must not expand");
        }
    }

    #[test]
    fn only_syntax_rule_ids_are_treated_as_prohibitions() {
        for yes in ["UBL-CR-1", "CII-SR-452", "UBL-DT-13"] {
            assert!(is_syntax_rule(yes), "{yes}");
        }
        // Business rules live in the same file and belong to `en16931`.
        for no in [
            "BR-01",
            "BR-CO-14",
            "PEPPOL-EN16931-R120",
            "UBL-XX-1",
            "UBL-CR-x",
        ] {
            assert!(!is_syntax_rule(no), "{no}");
        }
    }
}
