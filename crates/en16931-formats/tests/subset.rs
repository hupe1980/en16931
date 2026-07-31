#![cfg(any(feature = "ubl", feature = "cii"))]

//! The writer cannot emit an element EN 16931 forbids — in **either** syntax.
//!
//! 1 220 of the 1 339 syntax rules say some element or attribute "shall not be
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
                    "{}: {n} element paths written; {checked} of {} prohibitions checked, \
                     {} have a conditional context and are NOT checked",
                    stringify!($module),
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
