#![cfg(any(feature = "ubl", feature = "cii"))]

//! The writer cannot emit an element EN 16931 forbids — in **either** syntax.
//!
//! 1 218 of the 1 339 syntax rules say some element or attribute "shall not be
//! used". The claim is that a writer driven from the semantic model cannot
//! violate them: it has no way to express `cbc:UUID`, because the model has no
//! term for it.
//!
//! That is a claim, and claims with no test are how this project's sibling
//! accumulated seven classes of the same bug. So: walk everything each writer
//! emits, against the prohibitions extracted from CEN's own artefacts.
//!
//! **These tests report their own coverage.** Some prohibitions have a
//! conditional context — a predicate, `ends-with(…)`, a wildcard — and
//! selecting what they match needs an XPath engine this crate does not have.
//! Those are counted and printed, so the result reads as "1 045 of 1 268
//! checked" rather than as a clean sweep it has not earned.

mod common;

/// The prefix CEN's artefacts use for a namespace URI.
///
/// Recovered from the namespace rather than the source text, so a document
/// using different prefixes for the same namespaces is still checked.
fn prefix_for(ns: &str) -> &'static str {
    match ns {
        u if u.ends_with("CommonAggregateComponents-2") => "cac",
        u if u.ends_with("CommonBasicComponents-2") => "cbc",
        u if u.ends_with("CrossIndustryInvoice:100") => "rsm",
        u if u.ends_with("ReusableAggregateBusinessInformationEntity:100") => "ram",
        u if u.ends_with("UnqualifiedDataType:100") => "udt",
        u if u.ends_with("QualifiedDataType:100") => "qdt",
        _ => "",
    }
}

/// Every element path in a document, as a chain from the document element, each
/// with its attribute names.
fn paths(xml: &str) -> Vec<(String, Vec<String>)> {
    fn rec(n: roxmltree::Node<'_, '_>, prefix: &str, out: &mut Vec<(String, Vec<String>)>) {
        for c in n.children().filter(roxmltree::Node::is_element) {
            let q = match c.tag_name().namespace() {
                Some(ns) => format!("{}:{}", prefix_for(ns), c.tag_name().name()),
                None => c.tag_name().name().to_owned(),
            };
            let path = if prefix.is_empty() {
                q.clone()
            } else {
                format!("{prefix}/{q}")
            };
            let attrs = c.attributes().map(|a| a.name().to_owned()).collect();
            out.push((path.clone(), attrs));
            rec(c, &path, out);
        }
    }
    let doc = roxmltree::Document::parse(xml).expect("the writer emits well-formed XML");
    let root = doc.root_element();
    let mut out = vec![(root.tag_name().name().to_owned(), Vec::new())];
    rec(root, root.tag_name().name(), &mut out);
    out
}

macro_rules! syntaxes {
    ($($feature:literal => $module:ident),* $(,)?) => {$(
        #[cfg(feature = $feature)]
        mod $module {
            use super::{common, paths};
            use en16931_formats::$module::{self as syntax, prohibitions};

            fn check(inv: &en16931::Invoice) -> usize {
                let all = paths(&syntax::to_string(inv));
                for (path, attrs) in &all {
                    assert_eq!(
                        prohibitions::forbidden_path(path),
                        None,
                        "the writer emitted {path}"
                    );
                    for a in attrs {
                        assert_eq!(
                            prohibitions::forbidden_attribute(a),
                            None,
                            "the writer emitted @{a} on {path}"
                        );
                    }
                }
                all.len()
            }

            #[test]
            fn the_writer_stays_inside_the_en16931_subset() {
                let n = check(&common::maximal()) + check(&common::maximal_credit_note());
                let checked = prohibitions::FORBIDDEN_PATHS.len()
                    + prohibitions::FORBIDDEN_ATTRIBUTES.len();
                println!(
                    "{}: {n} element paths written; {} of {} \"shall not be used\" \
                     assertions represented, as {checked} rows; {} have a conditional \
                     test and are NOT checked",
                    stringify!($module),
                    prohibitions::TOTAL_PARAMS - prohibitions::UNEXTRACTED,
                    prohibitions::TOTAL_PARAMS,
                    prohibitions::UNEXTRACTED,
                );
                assert!(
                    n > 100,
                    "only {n} paths — the fixture is too thin to prove anything"
                );
            }

            /// The tables must contain something, or the test above passes by
            /// checking nothing at all.
            #[test]
            fn the_prohibition_tables_are_populated() {
                assert!(
                    prohibitions::FORBIDDEN_PATHS.len() > 300,
                    "{}",
                    prohibitions::FORBIDDEN_PATHS.len()
                );
                assert!(prohibitions::TOTAL_PARAMS > 400);
                assert!(prohibitions::UNEXTRACTED < prohibitions::TOTAL_PARAMS / 5);
            }

            /// A path the artefacts forbid must be caught when it appears.
            ///
            /// Without this, a bug in `forbidden_path` would make the suite
            /// green for the worst possible reason.
            #[test]
            fn the_checker_actually_catches_a_violation() {
                let (rule, ctx, rel) = prohibitions::FORBIDDEN_PATHS[0];
                let stem = ctx.trim_start_matches('/');
                assert_eq!(
                    prohibitions::forbidden_path(&format!("{stem}/{rel}")),
                    Some(rule),
                    "context {ctx} + {rel}"
                );
            }
        }
    )*};
}

syntaxes! {
    "ubl" => ubl,
    "cii" => cii,
}

// ── The numbers the documentation quotes ─────────────────────────────────────

/// *"1 218 of the 1 339 syntax rules say some element shall not be used"* is
/// repeated in seven files. This is where it is true.
///
/// Both figures come from the generated tables, which `cargo xtask check`
/// re-derives from the artefacts on every CI run — so pinning them here pins
/// them to the artefacts without this test needing `spec/`.
///
/// The coverage figure is the one that moves. It was 1 045 of 1 208 until the
/// extractor learned to read `(cac:InvoiceLine|cac:CreditNoteLine)/x`, which is
/// one rule about two spellings of the same thing and was four fifths of
/// everything the tables were missing.
#[cfg(all(feature = "ubl", feature = "cii"))]
#[test]
fn the_prohibition_counts_are_the_ones_the_docs_quote() {
    use en16931_formats::{cii, ubl};

    let total = ubl::prohibitions::TOTAL_PARAMS + cii::prohibitions::TOTAL_PARAMS;
    let unchecked = ubl::prohibitions::UNEXTRACTED + cii::prohibitions::UNEXTRACTED;
    let checked = total - unchecked;

    assert_eq!(total, 1_218, "the documented \"shall not be used\" count");
    assert_eq!(
        checked, 1_111,
        "the documented coverage of those prohibitions"
    );
    // 91 % of the 1 339 syntax rules are prohibitions, and 91 % of those are
    // represented. The two 91 %s are a coincidence and both are quoted, so both
    // are checked.
    assert_eq!(total * 100 / 1_339, 90, "…of 1 339 syntax rules");
    assert_eq!(
        checked * 100 / total,
        91,
        "…of which this many are represented"
    );
}
