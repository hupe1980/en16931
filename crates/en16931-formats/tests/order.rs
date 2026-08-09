#![cfg(any(feature = "ubl", feature = "cii"))]

//! The writer emits elements in UBL's schema order.
//!
//! UBL content models are XSD `sequence`s. A document carrying exactly the right
//! elements in the wrong order is **invalid**, and no Schematron rule reports it
//! — ordering is the schema's job, and this crate ships no schema. Without this
//! test the failure surfaces at a counterparty's validator, weeks later, as
//! "your invoice is malformed" with no rule id attached.
//!
//! The expected order is [`en16931_formats::ubl::order`], derived from 319 published
//! instances rather than transcribed.

#![cfg(feature = "ubl")]

mod common;

macro_rules! syntaxes {
    ($($feature:literal => $module:ident),* $(,)?) => {$(
        #[cfg(feature = $feature)]
        mod $module {
            use super::common;
            use en16931_formats::$module::{self as syntax, order};

            /// Every `(parent, children)` adjacency in a document.
            fn walk(doc: &roxmltree::Document<'_>) -> Vec<(String, Vec<String>)> {
                fn rec(n: roxmltree::Node<'_, '_>, out: &mut Vec<(String, Vec<String>)>) {
                    let kids: Vec<String> = n
                        .children()
                        .filter(roxmltree::Node::is_element)
                        .map(|c| c.tag_name().name().to_owned())
                        .collect();
                    if !kids.is_empty() {
                        out.push((n.tag_name().name().to_owned(), kids));
                    }
                    for c in n.children().filter(roxmltree::Node::is_element) {
                        rec(c, out);
                    }
                }
                let mut out = Vec::new();
                rec(doc.root_element(), &mut out);
                out
            }

            fn check(inv: &en16931::Invoice) {
                let xml = syntax::to_string(inv);
                let doc = roxmltree::Document::parse(&xml)
                    .expect("the writer emits well-formed XML");
                let mut checked = 0usize;
                for (parent, kids) in walk(&doc) {
                    let Some(expected) = order::children_of(&parent) else {
                        continue;
                    };
                    let mut last: Option<(usize, &str)> = None;
                    for k in &kids {
                        let Some(i) = expected.iter().position(|e| e == k) else {
                            panic!(
                                "<{parent}> has child <{k}>, which no authority instance carries"
                            );
                        };
                        if let Some((j, prev)) = last {
                            assert!(
                                i >= j,
                                "<{parent}>: <{k}> must come before <{prev}>\n\
                                 expected: {expected:?}\ngot: {kids:?}"
                            );
                        }
                        last = Some((i, k));
                        checked += 1;
                    }
                }
                assert!(
                    checked > 80,
                    "only {checked} elements checked — the fixture is too thin"
                );
            }

            #[test]
            fn an_invoice_is_written_in_schema_order() {
                check(&common::maximal());
            }

            #[test]
            fn a_credit_note_is_written_in_schema_order() {
                check(&common::maximal_credit_note());
            }

            /// The table must stay sorted, or `children_of`'s binary search
            /// silently returns `None` and this whole suite checks nothing.
            #[test]
            fn the_table_is_sorted_and_findable() {
                let mut sorted = order::ORDER.to_vec();
                sorted.sort_by_key(|(p, _)| *p);
                assert_eq!(sorted, order::ORDER, "ORDER must be sorted");
                assert_eq!(order::children_of("NoSuchElement"), None);
                assert!(!order::ORDER.is_empty());
            }
        }
    )*};
}

syntaxes! {
    "ubl" => ubl,
    "cii" => cii,
}
