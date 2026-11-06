//! Constructor invariants hold across `serde`: types re-validate on
//! deserialisation (`#[serde(try_from = ...)]`), and invoices and reports round-trip.

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

/// A whole invoice round-trips, and validates identically under every profile.
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

/// A report round-trips.
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

/// `19` and `19.00` deserialise to equal, equally hashed `Percentage`s.
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

/// A stored invoice carries the version of the shape it was stored in, and a
/// reader of another version refuses it — naming both — rather than reading a
/// partial model.
#[test]
fn an_invoice_is_stored_in_a_versioned_envelope() {
    let inv = sample();
    let json: serde_json::Value = serde_json::to_value(&inv).expect("serialise");
    assert_eq!(json["en16931"], en16931::invoice::JSON_VERSION);
    assert_eq!(en16931::invoice::JSON_VERSION, "0.10");
    assert!(json["invoice"].is_object(), "{json}");
    assert_eq!(json.as_object().map(serde_json::Map::len), Some(2));

    let mut other = json.clone();
    other["en16931"] = "0.7".into();
    let err = serde_json::from_value::<Invoice>(other).expect_err("another version");
    let msg = err.to_string();
    assert!(msg.contains("\"0.7\"") && msg.contains("\"0.10\""), "{msg}");

    // An unenveloped body is refused too.
    let bare = json["invoice"].clone();
    let err = serde_json::from_value::<Invoice>(bare).expect_err("no envelope");
    assert!(err.to_string().contains("en16931"), "{err}");
}

/// Every list field may be left out of stored JSON and reads as empty.
#[test]
fn list_fields_may_be_omitted() {
    let json = serde_json::json!({
        "en16931": en16931::invoice::JSON_VERSION,
        "invoice": {
            "kind": "Invoice",
            "seller": { "address": {}, "contact": {} },
            "buyer": { "address": {}, "contact": {} },
            "totals": {
                "line_total": "0.00", "taxable_total": "0.00",
                "gross_total": "0.00", "due": "0.00"
            }
        }
    });
    let inv: Invoice = serde_json::from_value(json).expect("lists default to empty");
    assert!(inv.lines.is_empty() && inv.notes.is_empty() && inv.seller.identifiers.is_empty());
}
