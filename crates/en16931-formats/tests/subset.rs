#![cfg(any(feature = "ubl", feature = "cii"))]

//! Neither writer emits an element EN 16931 forbids: everything each writer
//! emits is walked against the prohibitions extracted from CEN's artefacts.
//!
//! Prohibitions with a conditional context (a predicate, `ends-with(…)`, a
//! wildcard) need an XPath engine; they are counted and printed as unchecked.

mod common;

/// The prefix CEN's artefacts use for a namespace URI.
///
/// Recovered from the namespace, so other prefixes are still checked.
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
    ($($feature:literal => $module:ident, $syntax:ident),* $(,)?) => {$(
        #[cfg(feature = $feature)]
        mod $module {
            use super::{common, paths};
            use en16931_formats::$module::prohibitions;
            use en16931_formats::Syntax;

            fn check(inv: &en16931::Invoice) -> usize {
                let written = en16931_formats::write_unchecked(inv, Syntax::$syntax)
                    .expect("the fixture carries everything the schema requires");
                let all = paths(&written.xml);
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

            /// The tables are not empty.
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
    "ubl" => ubl, Ubl,
    "cii" => cii, Cii,
}

// ── The documented figures ───────────────────────────────────────────────────

/// The prohibition and coverage figures the documentation quotes. Both come
/// from the generated tables, which `cargo xtask check` re-derives from the
/// artefacts, so no corpus is needed here.
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
    // Both quoted 91 % figures are checked.
    assert_eq!(total * 100 / 1_339, 90, "…of 1 339 syntax rules");
    assert_eq!(
        checked * 100 / total,
        91,
        "…of which this many are represented"
    );
    let (ubl_checked, ubl_total) = (
        ubl::prohibitions::TOTAL_PARAMS - ubl::prohibitions::UNEXTRACTED,
        ubl::prohibitions::TOTAL_PARAMS,
    );
    let (cii_checked, cii_total) = (
        cii::prohibitions::TOTAL_PARAMS - cii::prohibitions::UNEXTRACTED,
        cii::prohibitions::TOTAL_PARAMS,
    );
    common::assert_page_states(
        "syntaxes.md",
        &[
            ("**<N> of them (91 %)**", total),
            ("**<N> of the 1 218 are represented**", checked),
            ("of the <N> are represented", total),
            ("<N> of UBL's", ubl_checked),
            ("of UBL's <N>", ubl_total),
            ("<N> of CII's", cii_checked),
            ("of CII's <N>", cii_total),
            ("\"<N> of 1 218 checked\"", checked),
            ("\"1 111 of <N> checked\"", total),
        ],
    );
}

/// Sub invoice lines are refused in CII because XRechnung refuses them.
///
/// `BR-DEX-15` forbids `//ram:ParentLineID` ("XRechnung does not support
/// this"). The model cannot hold that rule (it is listed as syntax-only in
/// `en16931`'s `KOSIT_SYNTAX_ONLY_RULES` test table), so the CII binding honours
/// it, and its note points at UBL, where `cac:SubInvoiceLine` is written.
#[test]
#[cfg(all(feature = "cii", feature = "ubl"))]
fn cii_omits_sub_invoice_lines_and_says_which_rule_forbids_them() {
    let mut invoice = common::maximal();
    let child = invoice.lines[0].clone();
    invoice.extensions.sub_invoice_lines = vec![(
        0,
        vec![en16931::SubInvoiceLine {
            line: child,
            vat: Some(invoice.lines[0].vat.clone()),
            quantity_stated: true,
            price_stated: true,
            children: vec![],
        }],
    )];

    use en16931_formats::{Syntax, report::Kind, write_unchecked};

    // The fixture declares XRechnung (core), which defines no extension groups.
    let out = write_unchecked(&invoice, Syntax::Cii).expect("CII has a form for the fixture");
    assert!(
        !out.xml.contains("ParentLineID"),
        "BR-DEX-15 forbids ram:ParentLineID in a CII XRechnung"
    );
    let note = out
        .entries
        .iter()
        .find(|e| e.term.as_deref() == Some("BG-DEX-01"))
        .unwrap_or_else(|| {
            panic!(
                "dropping a group the caller supplied is never silent: {:?}",
                out.entries
            )
        });
    assert_eq!(note.kind, Kind::Dropped, "{note}");
    assert!(
        note.rule.as_deref() == Some("BR-DEX-15") || note.message.contains("BR-DEX-15"),
        "the note must cite the rule, or omitting the group reads as a missing \
         feature rather than as conformance: {note}"
    );

    // …and UBL carries them, but only under the Extension profile: for core,
    // `UBL-CR-646` forbids `cac:SubInvoiceLine` too.
    let core = write_unchecked(&invoice, Syntax::Ubl).expect("UBL has a form for the fixture");
    assert!(
        !core.xml.contains("cac:SubInvoiceLine"),
        "UBL-CR-646 forbids it in a core document"
    );
    assert!(
        core.entries
            .iter()
            .any(|e| e.kind == Kind::Prohibited && e.rule.as_deref() == Some("UBL-CR-646")),
        "removing it is reported, citing the rule: {:?}",
        core.entries
    );
    // `write_unchecked` waives the prohibitions for the extension groups the
    // declared BT-24 profile defines; no validation proof is needed.
    let mut declared = invoice.clone();
    declared.specification_id = Some(
        en16931::profiles::XRECHNUNG_EXTENSION
            .specification_id
            .to_owned(),
    );
    let extension =
        write_unchecked(&declared, Syntax::Ubl).expect("UBL has a form for the fixture");
    assert!(
        extension.xml.contains("cac:SubInvoiceLine"),
        "the Extension profile is where UBL carries BG-DEX-01"
    );
    assert!(
        !extension
            .entries
            .iter()
            .any(|e| e.rule.as_deref() == Some("UBL-CR-646")),
        "the Extension must not forbid the group it defines: {:?}",
        extension.entries
    );
}
