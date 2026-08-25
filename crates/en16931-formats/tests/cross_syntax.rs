#![cfg(all(feature = "ubl", feature = "cii"))]

//! **UBL and CII are two spellings of one model** — over the authorities' own
//! documents, in both directions.
//!
//! `tests/roundtrip.rs` checks `Invoice → syntax → Invoice` within each syntax,
//! and `tests/fidelity.rs` does the same over the corpus. Both can pass while the
//! two bindings disagree with *each other*: a term the UBL side reads and the CII
//! side does not is invisible to a test that never crosses.
//!
//! So this reads each published document in its own syntax, writes it in the
//! **other** one, reads it back, and requires the two models to agree — or the
//! writer to have said what it could not carry.
//!
//! It found four, none of which any same-syntax test could have:
//!
//! | | |
//! |---|---|
//! | BT-20 was **trimmed** by the CII reader | `BR-DE-18` needs the Skonto block to end with a newline, and XRechnung is carried in CII too. 36 documents. |
//! | credit notes were detected from `381` alone | `396`, `532`, `83` read back as *invoices*, and `BR-CL-01` then reported a violation that is not one |
//! | BT-111 vanished when BT-6 = BT-5 | one element is then both totals, which the UBL reader already knew and the CII reader did not |
//! | `kind` and the Extension groups | genuinely unrepresentable in CII — now **reported** rather than lost |
//!
//! # Why the comparison is on the model and not on its JSON
//!
//! `Decimal` compares by value and serialises by representation, so a quantity
//! written `1.0000` and read back `1` is the same quantity and two different
//! strings. Diffing the JSON reports that as data loss, which is how a test like
//! this gets weakened until it passes. The JSON is used only to *describe* a
//! difference once the model has said there is one.

mod common;

use std::path::{Path, PathBuf};

use en16931::Invoice;
use en16931_formats::{Syntax, cii, ubl};

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

fn documents(syntax: Syntax) -> Vec<(PathBuf, String)> {
    let Some(root) = common::require("cross-syntax equivalence") else {
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

/// CII splits a party's BT-29 by whether it carries a scheme — `ram:GlobalID`
/// when it does, `ram:ID` when it does not — and the sequence fixes their order.
/// A party carrying both kinds therefore comes back reordered. EN 16931 gives
/// the order of repeated BT-29 occurrences no meaning, so the comparison sorts
/// rather than the binding pretending it can control it.
fn canonical(inv: &Invoice) -> Invoice {
    let mut inv = inv.clone();
    for party in [&mut inv.seller, &mut inv.buyer] {
        party
            .identifiers
            .sort_by_key(|i| (i.content().to_owned(), i.scheme().map(str::to_owned)));
    }
    inv
}

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
        _ => out.push(format!("{at}: source {a}, other syntax {b}")),
    }
}

/// `read in one syntax → write in the other → read back` preserves the model,
/// or the writer named what it could not carry.
fn cross(from: Syntax, min: usize) {
    let docs = documents(from);
    if docs.is_empty() {
        println!("SKIPPED: no artefacts — run `cargo xtask fetch`");
        return;
    }
    let (mut checked, mut reported) = (0usize, 0usize);
    let mut silent: Vec<String> = Vec::new();

    for (path, text) in &docs {
        let source = match from {
            Syntax::Ubl => ubl::from_str(text).ok().map(|r| r.invoice),
            Syntax::Cii => cii::from_str(text).ok().map(|r| r.invoice),
            _ => None,
        };
        // A document this reader refuses is `tests/corpus.rs`' business.
        let Some(source) = source else { continue };
        checked += 1;

        // Write in the *other* syntax, and read it back with that syntax's own
        // reader — so a shared misunderstanding between one binding's writer and
        // its own reader cannot hide the disagreement.
        let (xml, dropped, back) = match from {
            Syntax::Ubl => {
                let w = cii::write(&source);
                let back = cii::from_str(&w.xml).map(|r| r.invoice);
                (w.xml, w.dropped, back.map_err(|e| e.to_string()))
            }
            _ => {
                let w = ubl::write(&source);
                let back = ubl::from_str(&w.xml).map(|r| r.invoice);
                (w.xml, w.dropped, back.map_err(|e| e.to_string()))
            }
        };
        let back = back.unwrap_or_else(|e| {
            panic!("{}: unreadable after crossing: {e}\n{xml}", path.display())
        });

        let (before, after) = (canonical(&source), canonical(&back));
        if before == after {
            continue;
        }
        let mut diffs = Vec::new();
        differences(
            &serde_json::to_value(&before).expect("serialises"),
            &serde_json::to_value(&after).expect("serialises"),
            "",
            &mut diffs,
        );
        if dropped.is_empty() {
            silent.push(format!("{}\n    {}", path.display(), diffs.join("\n    ")));
        } else {
            reported += 1;
        }
    }

    assert!(
        checked > min,
        "only {checked} documents — is the corpus complete?"
    );
    assert!(
        silent.is_empty(),
        "{} document(s) changed meaning crossing syntaxes with nothing in `dropped` \
         to say so:\n\n{}",
        silent.len(),
        silent.join("\n\n")
    );
    println!(
        "{from:?} → other syntax → back: {checked} documents, {reported} differ and every \
         difference is named in `dropped`"
    );
}

#[test]
fn ubl_documents_survive_a_trip_through_cii() {
    cross(Syntax::Ubl, 250);
}

#[test]
fn cii_documents_survive_a_trip_through_ubl() {
    cross(Syntax::Cii, 120);
}

// ── The four bugs this file was written to find ──────────────────────────────

/// BT-90 survives being written as UBL.
///
/// UBL has no element for it inside BG-19: it rides on the **seller** as a
/// `cac:PartyIdentification` with `schemeID="SEPA"`, the one place `BR-CL-10`
/// admits a scheme outside ISO 6523. The reader had always hopped it into BG-19,
/// where `BR-DE-30` can see it. The writer never hopped it back, so every
/// direct-debit invoice written as UBL lost BT-90 **in silence** — and CII keeps
/// it in BG-19, so nothing but a crossing could show it.
#[test]
fn bt_90_survives_the_crossing_in_both_directions() {
    use en16931::invoice::{DirectDebit, PaymentInstructions, PaymentMeans};

    let mut inv = common::maximal();
    inv.payment = Some(PaymentInstructions {
        means_code: Some(en16931::invoice::Code::new("59")),
        means: Some(PaymentMeans::DirectDebit(DirectDebit {
            mandate_reference: Some("MANDATE-1".to_owned()),
            creditor_identifier: Some("DE98ZZZ09999999999".to_owned()),
            debited_account: Some("DE89370400440532013000".to_owned()),
        })),
        ..Default::default()
    });

    let creditor = |i: &en16931::Invoice| match i.payment.as_ref().and_then(|p| p.means.as_ref()) {
        Some(PaymentMeans::DirectDebit(d)) => d.creditor_identifier.clone(),
        _ => None,
    };

    for (name, xml) in [("ubl", ubl::write(&inv).xml), ("cii", cii::write(&inv).xml)] {
        let back = if name == "ubl" {
            ubl::from_str(&xml).expect("readable").invoice
        } else {
            cii::from_str(&xml).expect("readable").invoice
        };
        assert_eq!(
            creditor(&back).as_deref(),
            Some("DE98ZZZ09999999999"),
            "{name} lost BT-90:\n{xml}"
        );
        // `BR-DE-30` is the rule that would otherwise fail at the counterparty.
        assert!(
            !en16931::profiles::XRECHNUNG.validate(&back).has("BR-DE-30"),
            "{name}"
        );
        // And it is *moved*, not copied: a SEPA party identification is the UBL
        // binding's home for BT-90, not a BT-29 with an unusual scheme. Copying
        // it made a document grow a seller identifier on every crossing.
        assert!(
            !back
                .seller
                .identifiers
                .iter()
                .any(|i| i.scheme() == Some("SEPA")),
            "{name} left BT-90 in BT-29 as well"
        );
    }
}

/// A CII credit note is detected from the **whole** UNTDID 1001 credit-note
/// list, not from `381` alone.
///
/// CII has one document element, so BT-3 is the only signal — and checking
/// `381` alone is wrong in a way the corpus cannot show, because every
/// published CII credit note uses `381`. `396`, `532` and `83` would read back
/// as *invoices*, whereupon `BR-CL-01` compares them against the 50 invoice
/// codes and reports a violation that is not one.
#[test]
fn cii_reads_every_credit_note_code_as_a_credit_note() {
    use en16931::DocumentKind;

    for code in ["381", "396", "532", "83"] {
        let mut inv = common::maximal_credit_note();
        inv.type_code = Some(en16931::invoice::Code::new(code));
        let xml = cii::write(&inv).xml;
        let back = cii::from_str(&xml).expect("readable").invoice;

        assert_eq!(back.kind, DocumentKind::CreditNote, "BT-3 = {code}");
        assert!(
            !en16931::validate(&back).has("BR-CL-01"),
            "BT-3 = {code} is a credit-note code:\n{}",
            en16931::validate(&back)
        );
    }

    // An invoice code stays an invoice — the widening must not swallow BT-3's
    // actual meaning.
    let mut inv = common::maximal();
    inv.type_code = Some(en16931::invoice::Code::new("380"));
    let back = cii::from_str(&cii::write(&inv).xml)
        .expect("readable")
        .invoice;
    assert_eq!(back.kind, DocumentKind::Invoice);
}

/// BT-111 survives when BT-6 equals BT-5.
///
/// `BR-53`'s binding is satisfied by the document-currency total whenever the
/// two currencies coincide, so one element is then **both** BT-110 and BT-111.
/// The UBL reader knew that; the CII reader keyed BT-111 off "a currency other
/// than the document's" and therefore never found it.
#[test]
fn bt_111_survives_when_the_two_currencies_are_the_same() {
    use en16931::invoice::Code;

    let mut inv = common::maximal();
    inv.vat_accounting_currency = Some(Code::new("EUR")); // == BT-5
    inv.totals.vat_total_accounting = inv.totals.vat_total;

    for (name, xml) in [("ubl", ubl::write(&inv).xml), ("cii", cii::write(&inv).xml)] {
        let back = if name == "ubl" {
            ubl::from_str(&xml).expect("readable").invoice
        } else {
            cii::from_str(&xml).expect("readable").invoice
        };
        assert_eq!(
            back.totals.vat_total_accounting, inv.totals.vat_total_accounting,
            "{name} lost BT-111:\n{xml}"
        );
        assert!(!en16931::validate(&back).has("BR-53"), "{name}");
    }
}

/// An empty element is an **absent** term, in both readers.
///
/// `<ram:Description/>` is not a description whose value is the empty string.
/// The UBL reader always said so — `roxmltree` gives an empty element no text
/// node — and the CII reader mapped the same element to `Some("")`. Two readers
/// of one model disagreeing about that is a real difference: nine published CII
/// instances differed from their UBL crossing on it alone.
#[test]
fn an_empty_element_is_an_absent_term_in_both_readers() {
    let ubl_doc = r#"<Invoice xmlns="urn:oasis:names:specification:ubl:schema:xsd:Invoice-2"
        xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2">
        <cbc:BuyerReference/></Invoice>"#;
    assert_eq!(
        ubl::from_str(ubl_doc)
            .expect("readable")
            .invoice
            .buyer_reference,
        None
    );

    let cii_doc = r#"<rsm:CrossIndustryInvoice
        xmlns:rsm="urn:un:unece:uncefact:data:standard:CrossIndustryInvoice:100"
        xmlns:ram="urn:un:unece:uncefact:data:standard:ReusableAggregateBusinessInformationEntity:100">
        <rsm:SupplyChainTradeTransaction><ram:ApplicableHeaderTradeAgreement>
        <ram:BuyerReference/></ram:ApplicableHeaderTradeAgreement>
        </rsm:SupplyChainTradeTransaction></rsm:CrossIndustryInvoice>"#;
    assert_eq!(
        cii::from_str(cii_doc)
            .expect("readable")
            .invoice
            .buyer_reference,
        None,
        "an empty element is not a value"
    );
}
