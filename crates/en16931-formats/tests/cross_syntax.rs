#![cfg(all(feature = "ubl", feature = "cii"))]

//! UBL and CII are two spellings of one model: each corpus document is read
//! in its syntax, written in the other, and read back. The models must agree,
//! or the writer must name what it could not carry.
//!
//! Comparison is on the model, not its JSON: `Decimal` compares by value but
//! serialises by representation.

mod common;

use std::path::PathBuf;

use en16931::Invoice;
use en16931_formats::{Syntax, Written};

/// Read a document, whatever its syntax, and require it to be `syntax`.
fn read(xml: &str, syntax: Syntax) -> Result<Invoice, String> {
    let r = en16931_formats::read(xml.as_bytes()).map_err(|e| e.to_string())?;
    assert_eq!(r.syntax, syntax, "read as the wrong syntax");
    Ok(r.invoice)
}

/// Write the fixture, which carries everything either schema requires.
fn write(inv: &Invoice, syntax: Syntax) -> Written {
    en16931_formats::write_unchecked(inv, syntax).expect("the fixture is writable")
}

fn documents(syntax: Syntax) -> Vec<(PathBuf, String)> {
    common::authority_documents(syntax, "cross-syntax equivalence")
}

/// The model with each party's BT-29 sorted: CII's `ram:GlobalID` / `ram:ID`
/// split fixes an order EN 16931 gives no meaning.
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
fn cross(from: Syntax, min: usize, expect_refused: usize) {
    let docs = documents(from);
    if docs.is_empty() {
        println!("SKIPPED: no artefacts — run `cargo xtask fetch`");
        return;
    }
    let to = match from {
        Syntax::Ubl => Syntax::Cii,
        Syntax::Cii => Syntax::Ubl,
        other => unreachable!("{other} has no other syntax to cross to"),
    };
    let (mut checked, mut reported, mut refused) = (0usize, 0usize, 0usize);
    let mut silent: Vec<String> = Vec::new();

    for (path, text) in &docs {
        // A document this reader refuses is `tests/corpus.rs`' business.
        let Ok(source) = read(text, from) else {
            continue;
        };

        // Write in the other syntax and read back with that syntax's reader. A
        // model the target schema has no form for is refused; counted.
        let w = match en16931_formats::write_unchecked(&source, to) {
            Ok(w) => w,
            Err(e) => {
                println!("{}: refused — {e}", path.display());
                refused += 1;
                continue;
            }
        };
        checked += 1;
        if to == Syntax::Cii {
            common::assert_cii_bt8_spelled_for_cii(&path.display().to_string(), &w.xml);
        }
        let (xml, entries) = (w.xml, w.entries);
        let back = read(&xml, to).unwrap_or_else(|e| {
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
        if entries.is_empty() {
            silent.push(format!("{}\n    {}", path.display(), diffs.join("\n    ")));
        } else {
            reported += 1;
        }
    }

    assert!(
        checked > min,
        "only {checked} documents — is the corpus complete?"
    );
    // Deliberately invalid corpus documents with no form in the target schema
    // (`processing-valid/ubl005.xml` lacks BT-2). Pinned.
    assert_eq!(refused, expect_refused, "documents refused as unwritable");
    assert!(
        silent.is_empty(),
        "{} document(s) changed meaning crossing syntaxes with nothing in `entries` \
         to say so:\n\n{}",
        silent.len(),
        silent.join("\n\n")
    );
    println!(
        "{from:?} → other syntax → back: {checked} documents, {reported} differ and every \
         difference is named in `entries`; {refused} refused as unwritable"
    );
}

#[test]
fn ubl_documents_survive_a_trip_through_cii() {
    cross(Syntax::Ubl, 250, 1);
}

#[test]
fn cii_documents_survive_a_trip_through_ubl() {
    cross(Syntax::Cii, 120, 0);
}

// ── Targeted crossings ───────────────────────────────────────────────────────

/// BT-90 survives being written as UBL.
///
/// UBL carries it on the seller as a `cac:PartyIdentification` with
/// `schemeID="SEPA"` (the one non-ISO 6523 scheme `BR-CL-10` admits); the
/// reader moves it into BG-19 and the writer back.
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

    for (name, syntax) in [("ubl", Syntax::Ubl), ("cii", Syntax::Cii)] {
        let xml = write(&inv, syntax).xml;
        let back = read(&xml, syntax).expect("readable");
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
        // Moved, not copied: no extra BT-29 appears on the seller.
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

/// BT-89 survives CII when the invoice has neither BT-20 nor BT-9.
///
/// CII carries the mandate reference inside `ram:SpecifiedTradePaymentTerms`,
/// which the writer used to emit only for payment terms or a due date.
#[test]
fn bt_89_survives_cii_without_payment_terms_or_due_date() {
    use en16931::invoice::{DirectDebit, PaymentInstructions, PaymentMeans};

    let mut inv = common::maximal();
    inv.payment_terms = None;
    inv.due_date = None;
    inv.payment = Some(PaymentInstructions {
        means_code: Some(en16931::invoice::Code::new("59")),
        means: Some(PaymentMeans::DirectDebit(DirectDebit {
            mandate_reference: Some("MANDATE-1".to_owned()),
            creditor_identifier: Some("DE98ZZZ09999999999".to_owned()),
            debited_account: Some("DE89370400440532013000".to_owned()),
        })),
        ..Default::default()
    });

    let xml = write(&inv, Syntax::Cii).xml;
    let back = read(&xml, Syntax::Cii).expect("readable");
    let mandate = match back.payment.as_ref().and_then(|p| p.means.as_ref()) {
        Some(PaymentMeans::DirectDebit(d)) => d.mandate_reference.clone(),
        _ => None,
    };
    assert_eq!(mandate.as_deref(), Some("MANDATE-1"), "{xml}");
}

/// A CII credit note is detected from the whole UNTDID 1001 credit-note list
/// (`396`, `532`, `83`, …), not from `381` alone: BT-3 is the only signal.
#[test]
fn cii_reads_every_credit_note_code_as_a_credit_note() {
    use en16931::DocumentKind;

    for code in ["381", "396", "532", "83"] {
        let mut inv = common::maximal_credit_note();
        inv.type_code = Some(en16931::invoice::Code::new(code));
        let xml = write(&inv, Syntax::Cii).xml;
        let back = read(&xml, Syntax::Cii).expect("readable");

        assert_eq!(back.kind, DocumentKind::CreditNote, "BT-3 = {code}");
        assert!(
            !en16931::validate(&back).has("BR-CL-01"),
            "BT-3 = {code} is a credit-note code:\n{}",
            en16931::validate(&back)
        );
    }

    // An invoice code stays an invoice.
    let mut inv = common::maximal();
    inv.type_code = Some(en16931::invoice::Code::new("380"));
    let back = read(&write(&inv, Syntax::Cii).xml, Syntax::Cii).expect("readable");
    assert_eq!(back.kind, DocumentKind::Invoice);
}

/// BT-111 survives when BT-6 equals BT-5.
///
/// When the currencies coincide, one element is both BT-110 and BT-111 (`BR-53`).
#[test]
fn bt_111_survives_when_the_two_currencies_are_the_same() {
    use en16931::invoice::Code;

    let mut inv = common::maximal();
    inv.vat_accounting_currency = Some(Code::new("EUR")); // == BT-5
    inv.totals.vat_total_accounting = inv.totals.vat_total;

    for (name, syntax) in [("ubl", Syntax::Ubl), ("cii", Syntax::Cii)] {
        let xml = write(&inv, syntax).xml;
        let back = read(&xml, syntax).expect("readable");
        assert_eq!(
            back.totals.vat_total_accounting, inv.totals.vat_total_accounting,
            "{name} lost BT-111:\n{xml}"
        );
        assert!(!en16931::validate(&back).has("BR-53"), "{name}");
    }
}

/// An empty element is an **absent** term, in both readers.
///
/// `<ram:Description/>` is not `Some("")`.
#[test]
fn an_empty_element_is_an_absent_term_in_both_readers() {
    let ubl_doc = r#"<Invoice xmlns="urn:oasis:names:specification:ubl:schema:xsd:Invoice-2"
        xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2">
        <cbc:BuyerReference/></Invoice>"#;
    assert_eq!(
        read(ubl_doc, Syntax::Ubl)
            .expect("readable")
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
        read(cii_doc, Syntax::Cii)
            .expect("readable")
            .buyer_reference,
        None,
        "an empty element is not a value"
    );
}
