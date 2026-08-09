#![cfg(any(feature = "ubl", feature = "cii"))]

//! **Read → write → read is the identity, over the authorities' own documents.**
//!
//! `tests/roundtrip.rs` runs the same property against this crate's `maximal()`
//! fixture. That fixture is the crate's *idea* of a complete invoice, so it can
//! only catch a bug in a term someone remembered to put in it. These 486
//! documents are CEN's, KoSIT's and OpenPeppol's, and they carry combinations
//! nobody here would have thought to write down.
//!
//! It found five, all in the writers and all invisible to every other test in
//! the repository, because a lossy writer still emits a schema-valid document
//! that the reader reads without complaint:
//!
//! | | |
//! |---|---|
//! | `cbc:BaseQuantity unitCode=""` | BT-150 came back as `Some("")` and **`PEPPOL-EN16931-R130` then fired** — a fatal finding created by writing a document out and reading it back in |
//! | BT-147 required BT-148 | a price discount stated without a gross price was dropped, on seven instances |
//! | `cac:SubInvoiceLine` | BG-DEX-01 was read and never written |
//! | `cac:PrepaidPayment` | BG-DEX-09 likewise — the very data `EN-EXT-01` exists to warn about losing |
//! | an empty `cac:InvoicePeriod` | dropped without a word, so `BR-CO-20` stopped firing on the rewrite |
//!
//! # What "the identity" means when a syntax genuinely cannot carry something
//!
//! Not everything survives, and pretending otherwise would mean weakening the
//! test until it passed. UBL's `<CreditNote>` has no `cbc:DueDate`; CII nests
//! BT-147 inside the gross-price aggregate; core EN 16931 forbids the two
//! Extension groups. So the assertion is the crate's actual promise:
//!
//! > every difference between the two readings is **named in
//! > [`Written::dropped`]**.
//!
//! A silent loss fails. A reported one does not.
//!
//! [`Written::dropped`]: en16931_formats::ubl::Written::dropped

mod common;

use std::path::{Path, PathBuf};

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

fn documents(syntax: en16931_formats::Syntax) -> Vec<(PathBuf, String)> {
    let Some(root) = common::require("round-trip fidelity") else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    collect(&root, &mut paths);
    paths.sort();
    paths
        .into_iter()
        .filter_map(|p| {
            let text = std::fs::read_to_string(&p).ok()?;
            (en16931_formats::sniff(&text) == Some(syntax)).then_some((p, text))
        })
        .collect()
}

/// The model, in a shape two readings can be compared in.
///
/// One normalisation, and it is documented behaviour rather than a concession:
/// CII splits a party's BT-29 by whether it carries a scheme — `ram:GlobalID`
/// when it does, `ram:ID` when it does not — and the sequence puts those two
/// elements in a fixed order. A party carrying both kinds therefore comes back
/// with them **reordered**. EN 16931 gives the order of repeated BT-29
/// occurrences no meaning and the set is preserved exactly, so the comparison
/// sorts them rather than the writer pretending it can control it.
fn canonical(inv: &en16931::Invoice) -> en16931::Invoice {
    let mut inv = inv.clone();
    for party in [&mut inv.seller, &mut inv.buyer] {
        party
            .identifiers
            .sort_by_key(|i| (i.content().to_owned(), i.scheme().map(str::to_owned)));
    }
    inv
}

/// The same, as JSON, for describing a difference once one is found.
///
/// Comparison is on the **model**, not on this: `Decimal` compares by value and
/// serialises by representation, so a quantity read as `1.0000` and written back
/// as `1` is the same quantity and two different strings. Diffing the JSON would
/// report scale changes as data loss, which is how a test like this ends up
/// weakened until it passes.
fn as_json(inv: &en16931::Invoice) -> serde_json::Value {
    serde_json::to_value(inv).expect("the model serialises")
}

/// Field-by-field differences, so a failure names the term rather than dumping
/// two documents and leaving the reader to spot it.
fn differences(a: &serde_json::Value, b: &serde_json::Value, at: &str, out: &mut Vec<String>) {
    if a == b {
        return;
    }
    match (a, b) {
        (serde_json::Value::Object(x), serde_json::Value::Object(y)) => {
            let keys: std::collections::BTreeSet<_> = x.keys().chain(y.keys()).collect();
            for k in keys {
                let null = serde_json::Value::Null;
                differences(
                    x.get(k).unwrap_or(&null),
                    y.get(k).unwrap_or(&null),
                    &format!("{at}.{k}"),
                    out,
                );
            }
        }
        (serde_json::Value::Array(x), serde_json::Value::Array(y)) if x.len() == y.len() => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                differences(p, q, &format!("{at}[{i}]"), out);
            }
        }
        _ => out.push(format!("{at}: wrote {a}, read back {b}")),
    }
}

macro_rules! syntaxes {
    ($($feature:literal => ($module:ident, $syntax:ident, $min:literal)),* $(,)?) => {$(
        #[cfg(feature = $feature)]
        #[test]
        fn $module() {
            use en16931_formats::{$module as syntax, Syntax};

            let docs = documents(Syntax::$syntax);
            if docs.is_empty() {
                println!("SKIPPED: no artefacts — run `cargo xtask fetch`");
                return;
            }
            let (mut checked, mut reported) = (0usize, 0usize);
            let mut silent: Vec<String> = Vec::new();

            for (path, text) in &docs {
                // A document the reader refuses is `tests/corpus.rs`' business.
                let Ok(first) = syntax::from_str(text) else { continue };
                let out = syntax::write(&first.invoice);
                let second = syntax::from_str(&out.xml).unwrap_or_else(|e| {
                    panic!("{}: the writer emitted unreadable output: {e}", path.display())
                });
                checked += 1;

                let (before, after) = (canonical(&first.invoice), canonical(&second.invoice));
                if before == after {
                    continue;
                }
                let mut diffs = Vec::new();
                differences(&as_json(&before), &as_json(&after), "", &mut diffs);
                // Something moved. That is allowed **only** if the writer said so.
                if out.dropped.is_empty() {
                    silent.push(format!(
                        "{}\n    {}",
                        path.display(),
                        diffs.join("\n    ")
                    ));
                } else {
                    reported += 1;
                }
            }

            assert!(
                checked > $min,
                "only {checked} documents — is the corpus complete?"
            );
            assert!(
                silent.is_empty(),
                "{} document(s) lost data with nothing in `dropped` to say so:\n\n{}",
                silent.len(),
                silent.join("\n\n")
            );
            println!(
                "{}: {checked} documents survive read→write→read; {reported} differ and \
                 every difference is named in `dropped`",
                stringify!($module)
            );
        }
    )*};
}

syntaxes! {
    "ubl" => (ubl, Ubl, 250),
    "cii" => (cii, Cii, 120),
}

// ── The Extension groups, which only a profile that declares them may carry ──

/// `write_for(&inv, &XRECHNUNG_EXTENSION)` carries BG-DEX-01 and BG-DEX-09;
/// core `write` drops them **and says so**.
///
/// `UBL-CR-646` and `UBL-CR-470` fence `cac:SubInvoiceLine` and
/// `cac:PrepaidPayment` out of core EN 16931, and KoSIT's Extension scenario
/// reports both at `information` precisely so the two groups can be carried. So
/// whether the writer may emit them is a property of the *target profile*, and
/// the writer reads it from `Profile::extensions` — the same field `EN-EXT-01`
/// reads, so the warning and the writer cannot disagree.
///
/// Before this, the writer emitted neither and reported neither: the data went
/// missing in silence. For BG-DEX-09 that is the §14c Abs. 1 UStG case the crate
/// documents at length — the advance-related tax becoming payable a second time.
#[cfg(feature = "ubl")]
#[test]
fn the_extension_groups_travel_only_where_the_profile_can_hold_them() {
    use en16931::profiles::{XRECHNUNG, XRECHNUNG_EXTENSION};
    use en16931_formats::ubl;

    let mut inv = common::maximal();
    // The round-trip fixture carries BT-7 *and* BT-8 on purpose, to exercise
    // both. `BR-CO-03` makes them exclusive, so one goes before a profile will
    // accept the document. (BT-24 needs no help: `write_for` stamps it.)
    inv.vat_point_date_code = None;
    inv.extensions
        .third_party_payments
        .push(en16931::ThirdPartyPayment {
            payment_type: Some("MobilesBezahlen".to_owned()),
            amount: Some(en16931::InvoiceAmount::parse("19.96").expect("amount")),
            description: Some("Fremdleistung".to_owned()),
        });
    // `BR-DEX-09` replaces `BR-CO-16` and adds the third-party sum to BT-115,
    // which is the whole reason the Extension exists — so the fixture has to
    // balance under the new equation, not the old one.
    inv.totals.due = inv
        .totals
        .due
        .checked_add(en16931::InvoiceAmount::parse("19.96").expect("amount"))
        .expect("no overflow");

    // Core: dropped, and named.
    let core = ubl::write(&inv);
    assert!(
        !core.xml.contains("PrepaidPayment"),
        "core EN 16931 forbids it (UBL-CR-470)"
    );
    assert!(
        core.dropped.iter().any(|d| d.contains("UBL-CR-470")),
        "…and the caller is told: {:?}",
        core.dropped
    );

    // The Extension declares the group, so the prohibition is waived.
    inv.specification_id = Some(XRECHNUNG_EXTENSION.specification_id.to_owned());
    let ext = ubl::write_for(&inv, &XRECHNUNG_EXTENSION)
        .unwrap_or_else(|e| panic!("the fixture should satisfy it:\n{}", e.report()));
    assert!(
        ext.xml.contains("cac:PrepaidPayment"),
        "BG-DEX-09 belongs in an Extension document:\n{}",
        ext.xml
    );
    assert!(
        !ext.dropped.iter().any(|d| d.contains("UBL-CR-470")),
        "…and is not reported as a loss: {:?}",
        ext.dropped
    );

    // The plain CIUS does not declare it, so it is dropped there too.
    let mut cius = inv.clone();
    cius.specification_id = Some(XRECHNUNG.specification_id.to_owned());
    if let Ok(w) = ubl::write_for(&cius, &XRECHNUNG) {
        assert!(!w.xml.contains("PrepaidPayment"));
        assert!(w.dropped.iter().any(|d| d.contains("UBL-CR-470")));
    }
}
