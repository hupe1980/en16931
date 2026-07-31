//! Invariants must survive the `serde` boundary.
//!
//! The crate's central claim is that **types enforce representability**: an
//! `InvoiceAmount` cannot hold three decimals, a `Date` cannot hold a timestamp,
//! an `Attachment` cannot lack a filename. Every one of those is enforced in a
//! constructor — and a derived `Deserialize` rebuilds private fields *without
//! calling the constructor*, which would make the guarantee hold everywhere
//! except across the one boundary where untrusted data actually arrives.
//!
//! `Cargo.toml` advertises that this is handled:
//!
//! > Types carrying invariants re-run their validation on deserialisation via
//! > `#[serde(try_from = ...)]` rather than trusting reconstructed fields.
//!
//! That claim had no test. This file is the test, and it is written to fail if
//! anyone removes a `try_from` — the failure mode is silent otherwise, because
//! the happy path round-trips identically either way.

#![cfg(feature = "serde")]

use en16931::invoice::*;
use en16931::{Attachment, Date, Invoice, InvoiceAmount, Percentage, profiles, validate};

/// Every type whose constructor rejects something must reject it via `serde` too.
#[test]
fn deserialisation_re_runs_the_constructor_checks() {
    // `InvoiceAmount` — two decimals, and `parse` refuses a third.
    assert!(InvoiceAmount::parse("1.234").is_err());
    assert!(
        serde_json::from_str::<InvoiceAmount>(r#""1.234""#).is_err(),
        "a third decimal must not survive deserialisation — the whole reason \
         the BR-DEC-* family is unrepresentable is that this type cannot hold it"
    );
    assert!(serde_json::from_str::<InvoiceAmount>(r#""12.34""#).is_ok());

    // `Date` — a calendar day, never an instant.
    assert!(Date::parse("2026-06-30T12:00:00Z").is_err());
    assert!(
        serde_json::from_str::<Date>(r#""2026-06-30T12:00:00Z""#).is_err(),
        "a timestamp must not survive deserialisation"
    );
    assert!(serde_json::from_str::<Date>(r#""2026-06-30""#).is_ok());
    assert!(
        serde_json::from_str::<Date>(r#""2026-02-30""#).is_err(),
        "30 February is not a day"
    );

    // `Attachment` — §6.5.11 makes the mime code and filename mandatory.
    assert!(Attachment::new(vec![], "", "x.pdf").is_err());
    assert!(
        serde_json::from_str::<Attachment>(r#"{"content":[],"mime_code":"","filename":"x.pdf"}"#)
            .is_err(),
        "an attachment with no mime code must not survive deserialisation"
    );
    assert!(
        serde_json::from_str::<Attachment>(
            r#"{"content":[],"mime_code":"application/pdf","filename":"  "}"#
        )
        .is_err(),
        "…nor one with a blank filename"
    );
    assert!(
        serde_json::from_str::<Attachment>(
            r#"{"content":[],"mime_code":"application/pdf","filename":"a.pdf"}"#
        )
        .is_ok()
    );
}

/// A whole invoice round-trips, and validates identically on the far side.
///
/// The stronger property: serialisation is not merely lossless in the fields it
/// happens to test, but lossless *as far as the rule set can tell*. Two
/// documents that produce the same report are equivalent for this crate's
/// purpose, and that is what a format crate on the far side depends on.
#[test]
fn an_invoice_round_trips_and_validates_the_same() {
    let inv = sample();
    let json = serde_json::to_string(&inv).expect("serialise");
    let back: Invoice = serde_json::from_str(&json).expect("deserialise");

    assert_eq!(inv, back, "round-trip must be lossless");

    let before = validate(&inv);
    let after = validate(&back);
    assert_eq!(before.findings(), after.findings());
    assert_eq!(before.rules_checked(), after.rules_checked());

    for p in profiles::ALL {
        assert_eq!(
            p.validate(&inv).findings(),
            p.validate(&back).findings(),
            "{} disagrees across the serde boundary",
            p.id
        );
    }
}

/// A report is an artefact you store and diff, so it round-trips too.
#[test]
fn a_report_round_trips() {
    let report = validate(&Invoice::default());
    assert!(!report.is_valid(), "an empty invoice has findings to carry");
    let json = serde_json::to_string(&report).expect("serialise");
    let back: en16931::ValidationReport = serde_json::from_str(&json).expect("deserialise");
    assert_eq!(report.findings(), back.findings());
    assert_eq!(report.rules_checked(), back.rules_checked());
    assert_eq!(report.is_valid(), back.is_valid());
}

/// `Percentage` keeps its scale-independence across the boundary.
///
/// `19` and `19.00` are the same rate, and VAT grouping depends on `Eq` and
/// `Hash` agreeing with that — including after a round-trip through a
/// representation that writes one and reads the other.
#[test]
fn percentages_stay_scale_independent() {
    let a: Percentage = serde_json::from_str("19").expect("parse");
    let b: Percentage = serde_json::from_str("19.00").expect("parse");
    assert_eq!(a, b);
    let mut set = std::collections::HashSet::new();
    set.insert(a);
    assert!(set.contains(&b), "19 and 19.00 must group together");
}

fn sample() -> Invoice {
    let party = |name: &str, country: &str| Party {
        name: Some(name.to_owned()),
        address: PostalAddress {
            city: Some("Berlin".to_owned()),
            post_code: Some("10115".to_owned()),
            country: Some(Code::new(country)),
            ..Default::default()
        },
        ..Default::default()
    };
    let amount = |s: &str| InvoiceAmount::parse(s).expect("amount");
    let mut inv = Invoice::builder(
        profiles::EN16931.specification_id,
        "INV-1",
        Date::parse("2026-06-30").expect("date"),
        Code::new("380"),
        Code::new("EUR"),
    )
    .seller(party("Seller GmbH", "DE"))
    .buyer(party("Buyer BV", "NL"))
    .note("A note")
    .totals(DocumentTotals {
        line_total: amount("100.00"),
        taxable_total: amount("100.00"),
        vat_total: Some(amount("19.00")),
        gross_total: amount("119.00"),
        due: amount("119.00"),
        ..Default::default()
    })
    .build();
    inv.attachments = vec![SupportingDocument {
        reference: en16931::DocumentReference::new("DOC-1"),
        description: Some("Terms".to_owned()),
        uri: None,
        attachment: Some(
            Attachment::new(b"%PDF-1.7".to_vec(), "application/pdf", "terms.pdf").expect("valid"),
        ),
    }];
    inv
}
