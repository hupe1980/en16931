#![cfg(all(feature = "ubl", feature = "cii"))]

//! `to_string_for` — the path a caller cannot forget to validate on.
//!
//! The guarantee under test is narrow and load-bearing: **no document comes out
//! unless the rules that BT-24 will claim have actually been run.** A test that
//! only checked the happy path would pass against an implementation that
//! validated nothing, so both directions are asserted, and so is the stamping
//! order that makes the claim honest.

mod common;

use en16931::profiles::{EN16931, XRECHNUNG};
use en16931::{Invoice, validate};

/// The maximal fixture, made **valid**.
///
/// `common::maximal` populates every business term the model carries, which
/// necessarily includes both halves of a mutually exclusive pair: `BR-CO-03`
/// forbids BT-7 and BT-8 together. That is correct for the coverage suites,
/// which inspect what the writer emits, and wrong here — these tests are about
/// the verdict, so the one deliberate conflict is removed.
fn valid_fixture() -> Invoice {
    let mut inv = common::maximal();
    inv.vat_point_date_code = None; // BR-CO-03: BT-7 xor BT-8
    assert!(
        EN16931.validate(&inv).is_valid(),
        "{}",
        EN16931.validate(&inv)
    );
    inv
}

/// The refusal, which is the half that matters.
#[test]
fn an_invalid_invoice_produces_no_document_in_either_syntax() {
    let empty = Invoice::default();
    assert!(!validate(&empty).is_valid(), "the premise");

    let err = en16931_formats::ubl::to_string_for(&empty, &XRECHNUNG)
        .expect_err("an empty invoice is not an XRechnung");
    assert_eq!(err.profile(), "XRechnung 3.0");
    assert!(err.report().has("BR-02"), "the report is the whole report");
    assert!(err.report().has("BR-DE-15"), "profile rules ran too");
    assert!(
        err.to_string().contains("XRechnung 3.0"),
        "the message names the profile: {err}"
    );

    let err = en16931_formats::cii::to_string_for(&empty, &XRECHNUNG)
        .expect_err("same model, same verdict");
    assert!(err.report().has("BR-02"));
}

/// And the acceptance, with BT-24 stamped from the profile that was run.
#[test]
fn a_valid_invoice_is_written_and_carries_the_profiles_bt_24() {
    let mut inv = valid_fixture();
    // The fixture is maximal, not necessarily XRechnung-shaped. The core
    // profile is the one this fixture is built to satisfy.
    inv.specification_id = Some("something the caller typed".into());

    let xml = en16931_formats::ubl::to_string_for(&inv, &EN16931).expect("valid");
    assert!(
        xml.contains(EN16931.specification_id),
        "BT-24 comes from the profile, not from the caller"
    );
    assert!(
        !xml.contains("something the caller typed"),
        "the caller's claim is replaced, not appended"
    );

    let xml = en16931_formats::cii::to_string_for(&inv, &EN16931).expect("valid");
    assert!(xml.contains(EN16931.specification_id));
}

/// BT-24 is stamped **before** validation, not after.
///
/// If it were stamped after, an invoice carrying a BT-24 the profile forbids
/// would validate as whatever the caller typed and ship as the profile — the
/// exact mismatch this function exists to prevent. `BR-DE-21` restricts BT-24
/// to XRechnung's own identifiers, so a document that passes for `XRECHNUNG`
/// having started with a foreign one proves the order.
#[test]
fn bt_24_is_stamped_before_the_rules_run_not_after() {
    let mut inv = valid_fixture();
    inv.specification_id = Some("urn:cen.eu:en16931:2017".into());

    // Whatever the verdict for XRechnung, it must not be a BR-DE-21 finding
    // about the *core* identifier — that value never reached the validator.
    match en16931_formats::ubl::to_string_for(&inv, &XRECHNUNG) {
        Ok(xml) => assert!(xml.contains(XRECHNUNG.specification_id)),
        Err(e) => assert!(
            !e.report().has("BR-DE-21"),
            "BT-24 was validated as the caller's, not the profile's:\n{}",
            e.report()
        ),
    }
}

/// `write_for` keeps what `to_string_for` throws away.
#[test]
fn write_for_reports_what_the_syntax_could_not_carry() {
    let inv = valid_fixture();
    let written = en16931_formats::ubl::write_for(&inv, &EN16931).expect("valid");
    assert!(written.xml.starts_with("<?xml"));
    // The fixture is an ordinary invoice, so nothing is dropped — the point is
    // that the field is reachable at all on this path.
    assert!(written.dropped.is_empty(), "{:?}", written.dropped);
}

/// The error is a real `std::error::Error`, so `?` works in ordinary code.
#[test]
fn the_error_composes_with_the_question_mark_operator() {
    fn submit(inv: &Invoice) -> Result<String, Box<dyn std::error::Error>> {
        Ok(en16931_formats::ubl::to_string_for(inv, &XRECHNUNG)?)
    }
    assert!(submit(&Invoice::default()).is_err());
}
