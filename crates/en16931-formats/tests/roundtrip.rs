#![cfg(any(feature = "ubl", feature = "cii"))]

//! `Invoice → syntax → Invoice` is the identity, in **both** syntaxes.
//!
//! This is the test a hand-written serialiser and a hand-written parser make
//! impossible to trust and a shared binding makes routine. It is also the only
//! check that catches the asymmetric bug — a term written into one element and
//! read back out of another — which no amount of schema validation notices,
//! because both documents are perfectly valid.
//!
//! UBL and CII are independent mappings of the same model, so running one test
//! body against both is what catches a bug present in one and not the other.

mod common;

use en16931::Invoice;

/// The shared body: check what was written, read back, and report the
/// difference **per field**.
///
/// Comparing two `Invoice`s with `assert_eq!` prints two 200-line `Debug` dumps
/// and leaves the reader to spot the difference by eye. Naming the field that
/// moved is the difference between a five-second fix and an afternoon.
fn check(
    inv: &Invoice,
    xml: &str,
    dropped: &[String],
    back: &Invoice,
    unmapped: &[String],
    malformed: &[String],
) {
    assert!(dropped.is_empty(), "the writer dropped {dropped:?}");
    assert!(
        malformed.is_empty(),
        "the writer emitted values its own reader rejects: {malformed:?}\n{xml}"
    );
    assert!(
        unmapped.is_empty(),
        "the writer emitted elements its own reader ignores: {unmapped:?}"
    );

    let a = canonical(inv);
    let b = canonical(back);
    let (a, b) = (
        a.as_object().expect("object"),
        b.as_object().expect("object"),
    );
    let diffs: Vec<String> = a
        .keys()
        .filter(|k| a.get(*k) != b.get(*k))
        .map(|k| {
            format!(
                "{k}:\n  wrote {}\n  read  {}",
                a[k],
                b.get(k)
                    .map_or_else(|| "<missing>".to_owned(), ToString::to_string)
            )
        })
        .collect();
    assert!(
        diffs.is_empty(),
        "the round-trip is not the identity — {} field(s) differ:\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}

/// An invoice as JSON, with the one ordering the standard does not fix put into
/// a canonical order.
///
/// BT-29 (and its buyer counterpart) is a **repeatable** term whose occurrence
/// order EN 16931 gives no meaning to. CII splits it across `ram:GlobalID` and
/// `ram:ID` by whether it carries a scheme, and the sequence fixes *those* in a
/// fixed order — so a party with both kinds gets them back in a different order.
/// The set is preserved exactly; only the order is not, and only where the
/// standard never promised one.
///
/// Sorting here rather than comparing as sets keeps every other field strictly
/// positional, so a genuine ordering bug elsewhere still fails.
fn canonical(inv: &Invoice) -> serde_json::Value {
    let mut v = serde_json::to_value(inv).expect("serialise");
    for party in ["seller", "buyer"] {
        if let Some(ids) = v
            .get_mut(party)
            .and_then(|p| p.get_mut("identifiers"))
            .and_then(serde_json::Value::as_array_mut)
        {
            ids.sort_by_key(ToString::to_string);
        }
    }
    v
}

/// Generate the same tests for every syntax compiled in.
macro_rules! syntaxes {
    ($($feature:literal => $module:ident),* $(,)?) => {$(
        #[cfg(feature = $feature)]
        mod $module {
            use super::common;
            use en16931::Invoice;
            use en16931_formats::$module as syntax;

            fn roundtrip(inv: &Invoice) {
                let out = syntax::write(inv);
                let read = syntax::from_str(&out.xml).expect("the writer emits readable output");
                super::check(
                    inv,
                    &out.xml,
                    &out.dropped,
                    &read.invoice,
                    &read.unmapped,
                    &read.malformed,
                );
            }

            #[test]
            fn a_maximal_invoice_survives_the_round_trip() {
                roundtrip(&common::maximal());
            }

            #[test]
            fn a_maximal_credit_note_survives_the_round_trip() {
                roundtrip(&common::maximal_credit_note());
            }

            #[test]
            fn an_empty_invoice_survives_the_round_trip() {
                roundtrip(&Invoice::default());
            }

            /// Input that is not this syntax must be refused, not silently read
            /// as an empty invoice.
            #[test]
            fn foreign_input_is_rejected() {
                assert!(syntax::from_str("not xml").is_err());
                assert!(syntax::from_str("<html><body/></html>").is_err());
            }
        }
    )*};
}

syntaxes! {
    "ubl" => ubl,
    "cii" => cii,
}

/// The serialiser drops what the syntax cannot carry, and **says so**. This
/// pins exactly which drops are expected.
///
/// Without it the safety net would quietly absorb writer bugs: an element put
/// in the wrong place would be dropped as "forbidden here" and the subset test
/// would still pass. The writer is required not to need the net.
#[cfg(feature = "ubl")]
#[test]
fn ubl_drops_nothing_it_should_not() {
    use en16931_formats::ubl;

    let out = ubl::write(&common::maximal());
    assert!(
        out.dropped.is_empty(),
        "an invoice should lose nothing: {:?}",
        out.dropped
    );

    // BT-33 is *Seller* additional legal information. The model gives every
    // party the field; UBL-CR-244 forbids it on the customer.
    let mut with_buyer_legal = common::maximal();
    with_buyer_legal.buyer.additional_legal_information = Some("not a seller term".into());
    let out = ubl::write(&with_buyer_legal);
    assert_eq!(out.dropped.len(), 1, "{:?}", out.dropped);
    assert!(
        out.dropped[0].contains("CompanyLegalForm"),
        "{:?}",
        out.dropped
    );
    assert!(out.dropped[0].contains("UBL-CR-244"), "{:?}", out.dropped);
}

/// A UBL credit note that carries BT-9 or BT-11 loses them — and is told.
///
/// UBL's `<CreditNote>` has no `cbc:DueDate` and no `cac:ProjectReference`.
/// Dropping them is correct; dropping them *quietly* would mean a payment due
/// date vanishing between two systems with nothing in any log.
#[cfg(feature = "ubl")]
#[test]
fn a_ubl_credit_note_reports_what_it_cannot_carry() {
    use en16931_formats::ubl;

    let mut cn = common::maximal_credit_note();
    cn.due_date = Some(en16931::Date::parse("2026-02-15").expect("date"));
    cn.project_reference = Some(en16931::DocumentReference::new("PRJ-1"));

    let out = ubl::write(&cn);
    assert_eq!(out.dropped.len(), 2, "{:?}", out.dropped);
    assert!(
        out.dropped.iter().any(|d| d.contains("DueDate")),
        "{:?}",
        out.dropped
    );
    assert!(
        out.dropped.iter().any(|d| d.contains("ProjectReference")),
        "{:?}",
        out.dropped
    );
}

/// CII carries both terms UBL's credit note cannot — one document element
/// serves both kinds. Exactly the sort of asymmetry that running one test body
/// against both syntaxes exposes.
#[cfg(feature = "cii")]
#[test]
fn a_cii_credit_note_keeps_what_ubl_would_drop() {
    use en16931_formats::cii;

    let mut cn = common::maximal_credit_note();
    cn.due_date = Some(en16931::Date::parse("2026-02-15").expect("date"));
    cn.project_reference = Some(en16931::DocumentReference::new("PRJ-1"));

    let out = cii::write(&cn);
    assert!(out.dropped.is_empty(), "{:?}", out.dropped);
    let back = cii::from_str(&out.xml).expect("readable").invoice;
    assert_eq!(back.due_date, cn.due_date);
    assert_eq!(back.project_reference, cn.project_reference);
}

/// Each reader refuses the *other* syntax rather than returning an empty
/// invoice that looks like a valid document with nothing in it.
#[cfg(all(feature = "ubl", feature = "cii"))]
#[test]
fn each_reader_refuses_the_other_syntax() {
    use en16931_formats::{cii, ubl};

    let as_ubl = ubl::to_string(&common::maximal());
    let as_cii = cii::to_string(&common::maximal());

    assert!(matches!(cii::from_str(&as_ubl), Err(cii::Error::NotCii(_))));
    assert!(matches!(ubl::from_str(&as_cii), Err(ubl::Error::NotUbl(_))));
}

/// The two syntaxes agree about what the invoice *is*.
///
/// Writing the same model both ways and reading both back must give the same
/// invoice. This catches a term mapped correctly in one binding and to the
/// wrong element in the other — where each round-trips perfectly on its own and
/// the two nevertheless disagree.
#[cfg(all(feature = "ubl", feature = "cii"))]
#[test]
fn the_two_syntaxes_agree() {
    use en16931_formats::{cii, ubl};

    let inv = common::maximal();
    let via_ubl = ubl::from_str(&ubl::to_string(&inv)).expect("ubl").invoice;
    let via_cii = cii::from_str(&cii::to_string(&inv)).expect("cii").invoice;

    let a = canonical(&via_ubl);
    let b = canonical(&via_cii);
    let (a, b) = (
        a.as_object().expect("object"),
        b.as_object().expect("object"),
    );
    let diffs: Vec<String> = a
        .keys()
        .filter(|k| a.get(*k) != b.get(*k))
        .map(|k| {
            format!(
                "{k}:\n  ubl {}\n  cii {}",
                a[k],
                b.get(k)
                    .map_or_else(|| "<missing>".to_owned(), ToString::to_string)
            )
        })
        .collect();
    assert!(
        diffs.is_empty(),
        "UBL and CII disagree about {} field(s):\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}
