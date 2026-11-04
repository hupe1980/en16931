#![cfg(any(feature = "ubl", feature = "cii"))]

//! Reading the reference corpus: CEN's, KoSIT's and OpenPeppol's documents,
//! including several hundred deliberately invalid ones.
//!
//! The corpus is not committed (`cargo xtask fetch`). Absent, the suites skip
//! with a message; CI sets `EN16931_REQUIRE_SPEC=1` to make that a failure.

mod common;

use std::path::{Path, PathBuf};

/// The workspace's artefacts, or a loud skip.
///
/// One tree at the workspace root serves both crates (see `tests/common/mod.rs`).
fn spec_root() -> Option<PathBuf> {
    common::require("corpus")
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "xml") {
            out.push(p);
        }
    }
}

/// Every document in the corpus written in `syntax`, with its text.
fn documents(syntax: en16931_formats::Syntax) -> Vec<(PathBuf, String)> {
    let Some(root) = spec_root() else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for source in common::AUTHORITY_SOURCES {
        collect(&root.join(source), &mut paths);
    }
    paths
        .into_iter()
        .filter_map(|p| {
            let text = std::fs::read_to_string(&p).ok()?;
            (common::sniff(&text) == Some(syntax)).then_some((p, text))
        })
        .collect()
}

macro_rules! syntaxes {
    ($($feature:literal => ($module:ident, $syntax:ident, $min:literal, $allow:expr, $per_file:expr)),* $(,)?) => {$(
        #[cfg(feature = $feature)]
        mod $module {
            use super::{documents, spec_root};
            use en16931_formats::{Error, Syntax, report::Kind};

            #[test]
            fn the_reader_survives_every_published_instance() {
                let docs = documents(Syntax::$syntax);
                if docs.is_empty() {
                    println!("SKIPPED: no artefacts — run `cargo xtask fetch`");
                    assert!(spec_root().is_none(), "artefacts present but no documents found");
                    return;
                }
                let (mut read, mut rejected, mut malformed) = (0usize, 0usize, 0usize);
                for (path, text) in &docs {
                    match en16931_formats::read(text.as_bytes()) {
                        Ok(r) => {
                            assert_eq!(r.syntax, Syntax::$syntax, "{}", path.display());
                            read += 1;
                            malformed += r.entries_of(Kind::Malformed).count();
                        }
                        // Some instances are deliberately not well-formed;
                        // refusing them is correct.
                        Err(Error::Xml { .. }) => rejected += 1,
                        // Sniffing said this syntax and the reader disagreed.
                        Err(e) => panic!("{}: {e}", path.display()),
                    }
                }
                println!(
                    "{}: {read} documents read, {rejected} correctly rejected as not \
                     well-formed; {malformed} values present but not representable \
                     (deliberate — the corpus is partly invalid by design)",
                    stringify!($module)
                );
                assert!(read > $min, "only {read} documents — is the corpus complete?");
                assert!(
                    rejected < read / 10,
                    "{rejected} of {} unreadable — that is a reader bug, not a corpus of \
                     negative tests",
                    read + rejected
                );
            }

            /// Every element in the corpus is either mapped or named in an
            /// explicit allowlist: by path, or by (file, path) where only some
            /// documents may carry it. Both lists are exact: an entry nothing
            /// needs any more fails.
            #[test]
            fn nothing_is_ignored_without_being_named() {
                let docs = documents(Syntax::$syntax);
                let Some(root) = spec_root().filter(|_| !docs.is_empty()) else {
                    println!("SKIPPED: no artefacts — run `cargo xtask fetch`");
                    return;
                };
                let allowed: &[&str] = $allow;
                let per_file: &[(&str, &str)] = $per_file;
                // Unmapped path → the first file carrying it.
                let mut unmapped = std::collections::BTreeMap::new();
                let mut excused = std::collections::BTreeSet::new();
                for (path, text) in &docs {
                    let file = path
                        .strip_prefix(&root)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    let Ok(r) = en16931_formats::read(text.as_bytes()) else {
                        continue;
                    };
                    for e in r.entries_of(Kind::Unmapped) {
                        match per_file.iter().find(|(f, p)| *f == file && *p == e.path) {
                            Some(key) => {
                                excused.insert(*key);
                            }
                            None => {
                                unmapped.entry(e.path.clone()).or_insert_with(|| file.clone());
                            }
                        }
                    }
                }
                let unexpected: Vec<String> = unmapped
                    .iter()
                    .filter(|(p, _)| !allowed.contains(&p.as_str()))
                    .map(|(p, f)| format!("{p} (in {f})"))
                    .collect();
                assert!(
                    unexpected.is_empty(),
                    "{} ignored {} element path(s) nobody has justified:\n  {}",
                    stringify!($module),
                    unexpected.len(),
                    unexpected.join("\n  ")
                );
                let stale: Vec<&&str> = allowed
                    .iter()
                    .filter(|p| !unmapped.contains_key(**p))
                    .collect();
                assert!(stale.is_empty(), "allowed paths nothing carries any more: {stale:?}");
                assert_eq!(
                    excused.into_iter().collect::<Vec<_>>(),
                    per_file,
                    "the excused (file, path) pairs and the table differ"
                );
                println!(
                    "{}: {} unmapped element paths and {} (file, path) pairs, all accounted for",
                    stringify!($module),
                    unmapped.len(),
                    per_file.len()
                );
            }
        }
    )*};
}

syntaxes! {
    // `UBLExtensions` is forbidden by UBL-CR-001 and appears only in negative
    // instances; `CreditAccount` is a UBL payment element EN 16931 does not use.
    //
    // `PayeeFinancialAccount` and `PaymentMandate` come from KoSIT's
    // `BR-DE-23/24/25` instances: BG-17 with BG-18 or BG-19, which the
    // `PaymentMeans` enum cannot hold. The first kind read wins.
    //
    // `Invoice/PaymentMeans`: Peppol's stylesheet test document carries three
    // kinds of payment means; the first is read.
    //
    // By (file, path), `TaxTotal/TaxAmount`: a second VAT total in a currency
    // already read, in the negative instances for `BR-CO-15` (no multiple BT-110)
    // and Peppol's `R053` / `R054`; the first is read. Anywhere else it would be
    // a reader bug.
    //
    // `AdditionalDocumentReference/DocumentTypeCode`: a code other than 130 on
    // a supporting document — CEN's Max_content credit note carries 50, the
    // older credit-note spelling of BT-17, which both bindings now put in
    // `cac:OriginatorDocumentReference`. `InvoiceLine/DocumentReference/
    // DocumentTypeCode`: a line reference other than BT-128 (Peppol `R101`).
    "ubl" => (ubl, Ubl, 200, &[
        "AdditionalDocumentReference/DocumentTypeCode",
        "InvoiceLine/DocumentReference/DocumentTypeCode",
        "Invoice/UBLExtensions",
        "Invoice/PaymentMeans",
        "PaymentMeans/CreditAccount",
        "PaymentMeans/PayeeFinancialAccount",
        "PaymentMeans/PaymentMandate",
    ], &[
        ("validator-configuration-xrechnung/src/test/cen-unit-test/ubl-br-co-15-no-multiple-bt-110-allowed-test.xml", "Invoice/TaxTotal/TaxAmount"),
        ("xrechnung-schematron/test/instances/ubl-cn/ubl-cn-peppol-en16931-r053-2.xml", "CreditNote/TaxTotal/TaxAmount"),
        ("xrechnung-schematron/test/instances/ubl-cn/ubl-cn-peppol-en16931-r054-2.xml", "CreditNote/TaxTotal/TaxAmount"),
        ("xrechnung-schematron/test/instances/ubl-inv/ubl-inv-peppol-en16931-r053-2.xml", "Invoice/TaxTotal/TaxAmount"),
        ("xrechnung-schematron/test/instances/ubl-inv/ubl-inv-peppol-en16931-r054-2.xml", "Invoice/TaxTotal/TaxAmount"),
    ]),
    // The debtor's bank: EN 16931 has no term for the payer's BIC.
    // `PayeePartyCreditorFinancialAccount`: the KoSIT instances named above.
    "cii" => (cii, Cii, 100, &[
        "SpecifiedTradeSettlementPaymentMeans/PayerSpecifiedDebtorFinancialInstitution",
        "SpecifiedTradeSettlementPaymentMeans/PayeePartyCreditorFinancialAccount",
    ], &[]),
}

/// The depth limit leaves headroom over every published UBL and CII instance.
///
/// `MAX_DEPTH` guards `roxmltree`'s per-level recursion; the asserted floor is
/// that no corpus document is deeper than a third of it.
#[test]
fn the_depth_limit_clears_every_published_document() {
    // One limit, shared by both readers.
    let limit = en16931_formats::MAX_DEPTH;
    common::assert_page_states("cli.md", &[("nested more than <N> elements", limit)]);
    let syntaxes = [
        #[cfg(feature = "ubl")]
        en16931_formats::Syntax::Ubl,
        #[cfg(feature = "cii")]
        en16931_formats::Syntax::Cii,
    ];

    let mut deepest = 0usize;
    let mut worst = String::new();
    let mut seen = 0usize;
    for syntax in syntaxes {
        for (path, text) in documents(syntax) {
            seen += 1;
            // Counted independently of the crate's own scanner.
            let d = nesting(&text);
            assert!(
                d <= limit,
                "{} is {d} levels deep and the reader will refuse it",
                path.display()
            );
            if d > deepest {
                deepest = d;
                worst = path.display().to_string();
            }
        }
    }
    if seen == 0 {
        return; // no artefacts; `common::require` has already said so
    }
    println!("deepest of {seen} published documents: {deepest} ({worst})");
    assert!(
        deepest * 3 <= limit,
        "the deepest published document is {deepest} levels ({worst}) and the \
         limit is {limit}. That is less than three times the headroom — raise the \
         limit deliberately, or find out why a document grew."
    );
    common::assert_page_states(
        "syntaxes.md",
        &[
            ("The limit is <N>;", limit),
            ("the deepest of the <N> published", seen),
            ("instances is **<N>**", deepest),
        ],
    );
}

/// The corpus's own depth counter, independent of the crate's.
fn nesting(xml: &str) -> usize {
    let doc = match roxmltree::Document::parse(xml) {
        Ok(d) => d,
        Err(_) => return 0, // the negative instances include unparseable ones
    };
    doc.descendants()
        .filter(roxmltree::Node::is_element)
        .map(|n| n.ancestors().filter(roxmltree::Node::is_element).count())
        .max()
        .unwrap_or(0)
}

/// The syntaxes page states the single-occurrence tables' sizes.
#[cfg(all(feature = "ubl", feature = "cii"))]
#[test]
fn the_single_occurrence_tables_have_the_documented_size() {
    common::assert_page_states(
        "syntaxes.md",
        &[
            (
                "UBL 2.1 schemas and CEN's `count(…) <= 1` rules | <N> rows",
                en16931_formats::ubl::once::ONCE.len(),
            ),
            (
                "subset schema and CEN's `count(…) <= 1` rules | <N> rows",
                en16931_formats::cii::once::ONCE.len(),
            ),
        ],
    );
}
