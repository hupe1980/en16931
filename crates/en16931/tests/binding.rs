//! `BR-CL-06` is syntax-dependent, and this pins the consequence.
//!
//! # The defect this file exists for
//!
//! EN 16931's `BR-CL-*` rules restrict code lists, and every one of them
//! restricts the *same* list whichever XML syntax carries the document — which
//! is why this crate validates the abstract model and has no notion of syntax at
//! all. `BR-CL-06` is the exception. BT-8 is restricted to a slice of a UNTDID
//! directory, and the two artefacts CEN publishes name **different** directories:
//!
//! * UBL (`EN16931-UBL-codes.sch`): UNTDID **2005** — `3`, `35`, `432`;
//! * CII (`EN16931-CII-codes.sch`): UNTDID **2475** — `5`, `29`, `72`.
//!
//! Neither list contains the other, so one flat check cannot serve both. The
//! crate used to check only the UBL list, and consequently **rejected a lawful
//! CII document** that carried `5`: KoSIT's XRechnung Schematron and CEN's own
//! CII Schematron both accept it, and this crate did not.
//!
//! The fix threads a [`Binding`] through validation. These tests hold both ends:
//! the CII list admits what it should, and the UBL list still rejects what it
//! should — a fix that simply widened the list would pass the first and fail the
//! second.

use en16931::validation::Binding;
use en16931::validation::Check;
use en16931::{Invoice, validate, validate_as};

use en16931::profiles;

/// An invoice whose only interesting term is BT-8.
fn with_vat_point_date_code(code: &str) -> Invoice {
    let mut inv = Invoice::default();
    inv.vat_point_date_code = Some(en16931::invoice::Code::new(code));
    inv
}

#[test]
fn the_cii_corpus_value_passes_under_the_cii_binding() {
    // `5` (deposit) appears in the KoSIT corpus' comprehensive CII document and
    // is in the CII list but not the UBL one.
    let inv = with_vat_point_date_code("5");
    assert!(
        !validate_as(&inv, Binding::Cii).has("BR-CL-06"),
        "5 is a lawful CII BT-8 code and BR-CL-06 must not fire under Binding::Cii"
    );
}

#[test]
fn the_cii_value_still_fires_under_ubl_because_the_lists_are_disjoint() {
    // The other half of the fix: `5` is *not* admitted under UBL, so a run that
    // takes the UBL default (or is asked for it explicitly) still rejects it.
    // A fix that widened the UBL list would make this pass — which is exactly
    // what it must not do.
    let inv = with_vat_point_date_code("5");
    assert!(
        validate(&inv).has("BR-CL-06"),
        "5 is not a UBL BT-8 code; the UBL default must still reject it"
    );
    assert!(validate_as(&inv, Binding::Ubl).has("BR-CL-06"));
}

#[test]
fn the_ubl_codes_pass_under_ubl_and_fire_under_cii() {
    // Symmetry: the lists are disjoint in both directions, so `3` is lawful UBL
    // and unlawful CII. This proves the binding selects a genuinely different
    // list rather than one list plus an exception.
    for code in ["3", "35", "432"] {
        let inv = with_vat_point_date_code(code);
        assert!(
            !validate_as(&inv, Binding::Ubl).has("BR-CL-06"),
            "{code} is a lawful UBL BT-8 code"
        );
        assert!(
            validate_as(&inv, Binding::Cii).has("BR-CL-06"),
            "{code} is not in the CII BT-8 list, so BR-CL-06 must fire under Cii"
        );
    }
}

#[test]
fn a_code_in_neither_list_fires_under_both_bindings() {
    let inv = with_vat_point_date_code("99");
    assert!(validate_as(&inv, Binding::Ubl).has("BR-CL-06"));
    assert!(validate_as(&inv, Binding::Cii).has("BR-CL-06"));
}

#[test]
fn the_default_binding_is_ubl_so_the_public_api_is_unchanged() {
    // `validate` and `validate_with` keep their exact previous behaviour; a
    // caller who does not name a binding gets the UBL list. Only `validate_as`
    // and `Check::binding` change the verdict.
    let inv = with_vat_point_date_code("5");
    assert_eq!(validate(&inv).binding(), Binding::Ubl);
    assert!(validate(&inv).has("BR-CL-06"));
}

#[test]
fn the_report_records_which_binding_it_used() {
    let inv = with_vat_point_date_code("5");
    let report = validate_as(&inv, Binding::Cii);
    assert_eq!(report.binding(), Binding::Cii);
}

#[test]
fn a_profile_run_can_name_the_binding_too() {
    // Profiles carry rules beyond the core set, and `validate_as` is the way a
    // reader that knows the syntax — `en16931-formats` does — threads it in.
    let inv = with_vat_point_date_code("5");
    assert!(
        !profiles::EN16931
            .validate_as(&inv, Binding::Cii)
            .has("BR-CL-06"),
        "a CII document is valid under the core profile's CII binding"
    );
    assert!(
        profiles::EN16931.validate(&inv).has("BR-CL-06"),
        "and still invalid under the UBL default"
    );
}

#[test]
fn check_carries_the_binding_through_suppression() {
    let inv = with_vat_point_date_code("5");
    let report = Check::new(&profiles::EN16931)
        .binding(Binding::Cii)
        .run(&inv);
    assert_eq!(report.binding(), Binding::Cii);
    assert!(!report.has("BR-CL-06"));
}

#[test]
fn a_proof_is_produced_under_the_same_binding_the_run_used() {
    // `Validated::new_as` and `Check::prove` follow the binding, so a proof and
    // the report it came from cannot disagree about `BR-CL-06`. The invoice here
    // is not otherwise valid, so both calls fail — but the *rejection* differs
    // by binding, which is the property under test.
    use en16931::profiles::En16931;
    use en16931::validation::Check;
    use en16931::validation::profile::Validated;

    let inv = with_vat_point_date_code("5");

    let ubl = Validated::<En16931>::new(inv.clone()).unwrap_err();
    assert!(
        ubl.1.has("BR-CL-06"),
        "the default binding is UBL; `5` fails `BR-CL-06` there"
    );

    let cii = Validated::<En16931>::new_as(inv.clone(), Binding::Cii).unwrap_err();
    assert!(
        !cii.1.has("BR-CL-06"),
        "`5` is lawful CII, so the CII rejection must not name `BR-CL-06`"
    );

    // `Check::prove` routes through `Validated::new_as` with the check's own
    // binding, so the same distinction holds.
    let ubl = Check::of::<En16931>()
        .prove::<En16931>(inv.clone())
        .unwrap_err();
    assert!(matches!(
        ubl,
        en16931::validation::ProveError::Rejected(ref r) if r.1.has("BR-CL-06")
    ));
    let cii = Check::of::<En16931>()
        .binding(Binding::Cii)
        .prove::<En16931>(inv)
        .unwrap_err();
    assert!(matches!(
        cii,
        en16931::validation::ProveError::Rejected(ref r) if !r.1.has("BR-CL-06")
    ));
}
