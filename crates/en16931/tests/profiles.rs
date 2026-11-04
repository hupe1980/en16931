//! Profiles end to end: the CIUS restriction model and the typed proof.

use en16931::DocumentReference;
use en16931::invoice::*;
use en16931::profiles::{self, En16931, PeppolBis3, XRechnung};
use en16931::validation::profile::{ProfileMarker, Validated};
use en16931::*;
use rust_decimal::dec;

fn amount(s: &str) -> InvoiceAmount {
    InvoiceAmount::parse(s).unwrap()
}

fn pct(v: i64) -> Percentage {
    Percentage::new(rust_decimal::Decimal::from(v))
}

/// A core-valid invoice, with nothing a CIUS additionally demands.
fn core_valid() -> Invoice {
    let party = |name: &str, country: &str| Party {
        name: Some(name.to_owned()),
        address: PostalAddress {
            country: Some(Code::new(country)),
            ..Default::default()
        },
        // A GLN with a valid check digit: Peppol's `PEPPOL-COMMON-R040` checks it.
        electronic_address: Some(Identifier::schemed("4006381333931", "0088")),
        // BR-CO-26 / BR-CO-09; a Dutch one in Dutch shape (`PEPPOL-COMMON-R056-2`).
        vat_identifier: Some(if country == "NL" {
            "NL123456789B01".to_owned()
        } else {
            format!("{country}123456789")
        }),
        ..Default::default()
    };
    let line = InvoiceLine {
        id: "1".to_owned(),
        note: None,
        order_line_reference: None,
        accounting_reference: None,
        object_identifier: None,
        quantity: Quantity::new(dec!(1)),
        unit_code: Code::new("C62"),
        net_amount: amount("100.00"),
        period: None,
        allowances: vec![],
        charges: vec![],
        price: PriceDetails {
            net_price: UnitPriceAmount::new(dec!(100)),
            price_discount: None,
            gross_price: None,
            base_quantity: None,
            base_quantity_code: None,
        },
        vat: LineVat {
            category: Code::new("S"),
            rate: Some(pct(19)),
        },
        item: Item {
            name: Some("Widget".to_owned()),
            ..Default::default()
        },
    };
    Invoice::builder(
        profiles::EN16931.specification_id,
        "INV-1",
        Date::parse("2026-06-30").unwrap(),
        Code::new("380"),
        Code::new("EUR"),
    )
    .seller(party("Seller GmbH", "DE"))
    .buyer(party("Buyer BV", "NL"))
    .due_date(Date::parse("2026-07-30").unwrap())
    .line(line)
    .vat_breakdown(VatBreakdown {
        taxable_amount: amount("100.00"),
        tax_amount: amount("19.00"),
        category: Code::new("S"),
        rate: Some(pct(19)),
        exemption_reason: None,
        exemption_reason_code: None,
    })
    .totals(DocumentTotals {
        line_total: amount("100.00"),
        allowance_total: None,
        charge_total: None,
        taxable_total: amount("100.00"),
        vat_total: Some(amount("19.00")),
        vat_total_accounting: None,
        gross_total: amount("119.00"),
        paid: None,
        rounding: None,
        due: amount("119.00"),
    })
    .build()
}

/// Fill in everything XRechnung additionally requires.
fn xrechnung_valid() -> Invoice {
    let mut inv = core_valid();
    inv.specification_id = Some(profiles::XRECHNUNG.specification_id.to_owned());
    inv.buyer_reference = Some("04011000-12345-34".to_owned()); // BR-DE-15
    inv.seller.address.city = Some("Berlin".to_owned()); // BR-DE-3
    inv.seller.address.post_code = Some("10115".to_owned()); // BR-DE-4
    inv.delivery = Some(Delivery {
        date: Some(Date::parse("2026-06-30").unwrap()), // BR-DE-TMP-32
        ..Default::default()
    });
    // Merged from Peppol: `R001` requires BT-23, `R007` fixes its shape.
    inv.business_process = Some("urn:fdc:peppol.eu:2017:poacc:billing:01:1.0".to_owned());
    inv.seller.contact = Contact {
        name: Some("Frau Muster".to_owned()),         // BR-DE-5
        phone: Some("+49 30 123456".to_owned()),      // BR-DE-6
        email: Some("rechnung@seller.de".to_owned()), // BR-DE-7
    };
    inv.buyer.address.city = Some("Amsterdam".to_owned()); // BR-DE-8
    inv.buyer.address.post_code = Some("1011".to_owned()); // BR-DE-9
    inv.payment = Some(PaymentInstructions {
        means_code: Some(Code::new("58")), // SEPA credit transfer
        // BG-17 (BR-DE-23-a); a mod-97-valid IBAN for BR-DE-19.
        means: Some(PaymentMeans::CreditTransfer(vec![CreditTransfer {
            account_identifier: Some("DE89370400440532013000".to_owned()),
            ..Default::default()
        }])),
        ..Default::default()
    }); // BR-DE-1
    inv
}

/// A Peppol BIS Billing 3.0-valid document: the XRechnung one with Peppol's BT-24.
fn peppol_valid() -> Invoice {
    let mut inv = xrechnung_valid();
    inv.specification_id = Some(profiles::PEPPOL_BIS_3.specification_id.to_owned());
    inv
}

/// A CIUS restricts: CIUS-valid implies core-valid, not the reverse (§4.4.4).
#[test]
fn a_cius_restricts_and_the_direction_matters() {
    let core = core_valid();
    assert!(
        profiles::EN16931.validate(&core).is_valid(),
        "{}",
        profiles::EN16931.validate(&core)
    );

    // The same document under XRechnung: every added restriction fires.
    let report = profiles::XRECHNUNG.validate(&core);
    assert!(!report.is_valid());
    for id in [
        "BR-DE-1", "BR-DE-3", "BR-DE-4", "BR-DE-5", "BR-DE-6", "BR-DE-7", "BR-DE-8", "BR-DE-9",
        "BR-DE-15", "BR-DE-21",
    ] {
        assert!(report.has(id), "{id} did not fire:\n{report}");
    }

    // Fill them in, and it is valid under both.
    let xr = xrechnung_valid();
    assert!(
        profiles::XRECHNUNG.validate(&xr).is_valid(),
        "{}",
        profiles::XRECHNUNG.validate(&xr)
    );
    assert!(profiles::EN16931.validate(&xr).is_valid());
}

/// Restrictions carry the profile's real rule ids, so findings match KoSIT's index.
#[test]
fn findings_carry_the_real_br_de_ids_and_business_term_paths() {
    let report = profiles::XRECHNUNG.validate(&core_valid());
    let f = report
        .fatal()
        .find(|f| f.rule == "BR-DE-3")
        .expect("BR-DE-3");
    assert_eq!(f.path.to_string(), "BG-4/BT-37");
    assert!(f.message.contains("Seller city"), "{}", f.message);
}

/// CEN's `BR-48` exempts category `O` from BT-119; XRechnung's `BR-DE-14` does not.
#[test]
fn br_de_14_requires_bt_119_where_br_48_exempts_it() {
    let mut inv = xrechnung_valid();
    inv.lines[0].vat = LineVat {
        category: Code::new("O"),
        rate: None, // BR-O-05: the LINE rate must be absent
    };
    inv.vat_breakdown = vec![VatBreakdown {
        taxable_amount: amount("100.00"),
        tax_amount: amount("0.00"),
        category: Code::new("O"),
        rate: None, // BR-48 permits this; BR-DE-14 does not
        exemption_reason: Some("Not subject to VAT".to_owned()),
        exemption_reason_code: None,
    }];
    inv.totals.vat_total = Some(amount("0.00"));
    inv.totals.gross_total = amount("100.00");
    inv.totals.due = amount("100.00");

    // Core: fine. BR-48's own exception covers it.
    assert!(
        !profiles::EN16931.validate(&inv).has("BR-48"),
        "BR-48 exempts category O"
    );
    // XRechnung: not fine.
    assert!(
        profiles::XRECHNUNG.validate(&inv).has("BR-DE-14"),
        "BR-DE-14 has no category exception"
    );
}

/// XRechnung and Peppol are siblings: each permits a BT-3 the other forbids.
#[test]
fn xrechnung_and_peppol_genuinely_disagree() {
    let mut inv = xrechnung_valid();
    inv.type_code = Some(Code::new("389")); // self-billed invoice
    // `P0100` only applies when BT-23 names Peppol billing `01`.
    inv.business_process = Some("urn:fdc:peppol.eu:2017:poacc:billing:01:1.0".to_owned());

    // XRechnung's BR-DE-17 allows 389.
    assert!(!profiles::XRECHNUNG.validate(&inv).has("BR-DE-17"));
    // Peppol's P0100 does not — self-billing is a separate Peppol profile.
    assert!(
        profiles::PEPPOL_BIS_3
            .validate(&inv)
            .has("PEPPOL-EN16931-P0100")
    );

    // And the other way: 386 (prepayment invoice) is Peppol-legal, not XRechnung-legal.
    inv.type_code = Some(Code::new("386"));
    assert!(profiles::XRECHNUNG.validate(&inv).has("BR-DE-17"));
    assert!(
        !profiles::PEPPOL_BIS_3
            .validate(&inv)
            .has("PEPPOL-EN16931-P0100")
    );
}

/// A document declares its own profile, so the rule set is selected from it.
#[test]
fn a_document_selects_its_own_rule_set() {
    let inv = xrechnung_valid();
    let declared = profiles::classify(inv.specification_id.as_deref());
    let profile = declared.profile().expect("known profile");
    assert_eq!(profile.id, "XRechnung 3.0");
    assert!(profile.validate(&inv).is_valid());
}

/// The typed proof, and widening out of a conformant CIUS (§4.4.4).
#[test]
fn a_proof_survives_the_call_boundary_and_widens_for_free() {
    fn serialise_xrechnung(v: &Validated<XRechnung>) -> String {
        v.invoice().number.clone().unwrap_or_default()
    }
    fn accepts_core(_: Validated<En16931>) {}

    let proof: Validated<XRechnung> = Validated::new(xrechnung_valid()).unwrap();
    assert_eq!(serialise_xrechnung(&proof), "INV-1");

    // Peppol is a conformant CIUS, so `widen` is infallible. XRechnung relaxes
    // core code-list rules, so leaving it means re-validating.
    let peppol: Validated<PeppolBis3> = Validated::new(peppol_valid()).unwrap();
    accepts_core(peppol.widen());

    let core: Validated<En16931> =
        Validated::new(proof.into_inner()).expect("this particular document is core-valid too");
    accepts_core(core);

    // The failure branch hands the invoice back, so a caller can fix and retry.
    let rejected = Validated::<XRechnung>::new(core_valid()).unwrap_err();
    let (returned, report) = rejected.into_parts();
    assert_eq!(returned.number.as_deref(), Some("INV-1"));
    assert!(report.has("BR-DE-15"));
}

/// KoSIT reports `BR-CL-23` at warning, so an XRechnung-valid invoice can be
/// core-invalid; hence no `Underlies<XRechnung> for En16931`.
#[test]
fn an_xrechnung_invoice_can_be_core_invalid() {
    let mut inv = xrechnung_valid();
    inv.lines[0].unit_code = en16931::invoice::Code::new("NOT-A-UNIT");

    let xr = profiles::XRECHNUNG.validate(&inv);
    assert!(xr.is_valid(), "KoSIT accepts it:\n{xr}");
    assert!(
        xr.findings().iter().any(|f| f.rule == "BR-CL-23"),
        "…and still says so, at warning"
    );

    let core = en16931::validate(&inv);
    assert!(!core.is_valid(), "core EN 16931 does not");

    assert!(Validated::<En16931>::new(inv).is_err());
}

/// §4.4.2: a profile that reports a core rule at a lower severity is not a
/// conformant CIUS. Only `EN 16931` and Peppol qualify; the XRechnung family
/// relaxes core code-list rules per KoSIT's configuration.
#[test]
fn conformance_is_reported_honestly() {
    let conformant: Vec<&str> = profiles::ALL
        .iter()
        .filter(|p| p.is_conformant_cius())
        .map(|p| p.id)
        .collect();
    assert_eq!(
        conformant,
        ["EN 16931", "Peppol BIS Billing 3.0"],
        "a profile that relaxes a core rule is not a conformant CIUS"
    );
    // The property is derived from the level overrides.
    for p in profiles::ALL {
        let relaxes = p.levels.iter().any(|(id, level)| {
            en16931::validation::rules::explain(id).is_some_and(|r| *level > r.severity)
        });
        assert_eq!(
            p.is_conformant_cius(),
            !relaxes,
            "{} — conformance and the level overrides must agree",
            p.id
        );
    }
}

/// §4.4.4 behaviourally: whatever a conformant CIUS accepts, core accepts, which
/// is what makes [`Validated::widen`] infallible.
#[test]
fn a_conformant_cius_never_accepts_what_core_rejects() {
    let docs = [core_valid(), xrechnung_valid()];
    for p in profiles::ALL.iter().filter(|p| p.is_conformant_cius()) {
        for doc in &docs {
            let mut doc = doc.clone();
            doc.specification_id = Some(p.specification_id.to_owned());
            if p.validate(&doc).is_valid() {
                let core = en16931::validate(&doc);
                assert!(
                    core.is_valid(),
                    "{} accepted a document core EN 16931 rejects, so §4.4.4's \
                     widening guarantee does not hold for it:\n{core}",
                    p.id
                );
            }
        }
    }
}

/// A profile report counts the restrictions it checked alongside the core rules,
/// so coverage is never overstated.
#[test]
fn profile_reports_state_their_own_coverage() {
    let core_only = profiles::EN16931.validate(&core_valid());
    let with_cius = profiles::XRECHNUNG.validate(&core_valid());
    assert!(
        with_cius.rules_checked() > core_only.rules_checked(),
        "{} vs {}",
        with_cius.rules_checked(),
        core_only.rules_checked()
    );
}

/// Markers and profiles agree — a marker cannot point at the wrong profile.
#[test]
fn markers_match_their_profiles() {
    assert_eq!(En16931::PROFILE.id, profiles::EN16931.id);
    assert_eq!(XRechnung::PROFILE.id, profiles::XRECHNUNG.id);
    assert_eq!(PeppolBis3::PROFILE.id, profiles::PEPPOL_BIS_3.id);
}

/// Peppol's ±0.02 line tolerance, distinct from CEN's ±1.00 and the exact totals chain.
#[test]
fn peppols_line_arithmetic_is_a_third_tolerance_regime() {
    let build = |net: &str| {
        let mut inv = core_valid();
        inv.specification_id = Some(profiles::PEPPOL_BIS_3.specification_id.to_owned());
        inv.buyer_reference = Some("REF".to_owned()); // PEPPOL-EN16931-R003
        inv.lines[0].quantity = Quantity::new(dec!(1));
        inv.lines[0].price.net_price = UnitPriceAmount::new(dec!(100));
        inv.lines[0].net_amount = amount(net);
        // Keep the totals chain consistent so only R120 can fire.
        inv.totals.line_total = amount(net);
        inv.totals.taxable_total = amount(net);
        inv.vat_breakdown[0].taxable_amount = amount(net);
        inv
    };

    // 1 × 100 = 100.00. Two cents out is inside Peppol's slack.
    let inv = build("100.02");
    assert!(
        !profiles::PEPPOL_BIS_3
            .validate(&inv)
            .has("PEPPOL-EN16931-R120")
    );
    // Three cents is not.
    let inv = build("100.03");
    assert!(
        profiles::PEPPOL_BIS_3
            .validate(&inv)
            .has("PEPPOL-EN16931-R120")
    );

    // Core EN 16931 has no such rule.
    let inv = build("999.00");
    assert!(!profiles::EN16931.validate(&inv).has("PEPPOL-EN16931-R120"));
}

/// `R046` is exact (no `u:slack`), unlike `R040`.
#[test]
fn r046_is_exact_where_r040_is_tolerant() {
    let mut inv = core_valid();
    inv.specification_id = Some(profiles::PEPPOL_BIS_3.specification_id.to_owned());
    inv.buyer_reference = Some("REF".to_owned());
    inv.lines[0].price.gross_price = Some(UnitPriceAmount::new(dec!(101)));
    inv.lines[0].price.price_discount = Some(UnitPriceAmount::new(dec!(1)));
    // 101 − 1 = 100, exactly what BT-146 already is.
    assert!(
        !profiles::PEPPOL_BIS_3
            .validate(&inv)
            .has("PEPPOL-EN16931-R046")
    );

    // One cent out — tolerated by R040, fatal under R046.
    inv.lines[0].price.price_discount = Some(UnitPriceAmount::new(dec!(0.99)));
    assert!(
        profiles::PEPPOL_BIS_3
            .validate(&inv)
            .has("PEPPOL-EN16931-R046")
    );
}

/// `R130` compares BT-130 (quantity) with BT-150 (price), so the line checks it.
#[test]
fn r130_compares_two_terms_neither_type_owns() {
    let mut inv = core_valid();
    inv.specification_id = Some(profiles::PEPPOL_BIS_3.specification_id.to_owned());
    inv.buyer_reference = Some("REF".to_owned());
    inv.lines[0].price.base_quantity = Some(Quantity::new(dec!(1)));
    inv.lines[0].price.base_quantity_code = Some(Code::new("H87")); // line is C62

    let report = profiles::PEPPOL_BIS_3.validate(&inv);
    assert!(report.has("PEPPOL-EN16931-R130"), "{report}");

    inv.lines[0].price.base_quantity_code = Some(Code::new("C62"));
    assert!(
        !profiles::PEPPOL_BIS_3
            .validate(&inv)
            .has("PEPPOL-EN16931-R130")
    );
}

/// `R121`: a zero base quantity (R120's divisor) is reported, never divided by.
#[test]
fn a_zero_base_quantity_is_reported_and_never_divided_by() {
    let mut inv = core_valid();
    inv.specification_id = Some(profiles::PEPPOL_BIS_3.specification_id.to_owned());
    inv.buyer_reference = Some("REF".to_owned());
    inv.lines[0].price.base_quantity = Some(Quantity::ZERO);

    let report = profiles::PEPPOL_BIS_3.validate(&inv); // must not panic
    assert!(report.has("PEPPOL-EN16931-R121"), "{report}");
    assert!(
        !report.has("PEPPOL-EN16931-R120"),
        "R120 must skip, not divide"
    );
}

/// `extra_rules` carries conditional rules that restrictions cannot express.
#[test]
fn profiles_carry_conditional_rules_that_restrictions_cannot_express() {
    assert!(
        profiles::EN16931.extra_rules.is_empty(),
        "core adds nothing"
    );
    assert!(!profiles::PEPPOL_BIS_3.extra_rules.is_empty());
    // XRechnung inherits Peppol's arithmetic via the German national rule set.
    assert!(!profiles::XRECHNUNG.extra_rules.is_empty());
}

/// `PaymentMeans` is an enum, so the combinations `BR-DE-23-b`, `-24-b` and
/// `-25-b` forbid cannot be built; the `-a` rules still tie the variant to BT-81.
#[test]
fn the_payment_groups_are_mutually_exclusive_by_construction() {
    let mut inv = xrechnung_valid();

    // BT-81 = 58 with BG-17 present: correct.
    assert!(!profiles::XRECHNUNG.validate(&inv).has("BR-DE-23-a"));

    // Card code with a credit-transfer group: `-a` fires.
    inv.payment.as_mut().unwrap().means_code = Some(Code::new("48"));
    let report = profiles::XRECHNUNG.validate(&inv);
    assert!(report.has("BR-DE-24-a"), "{report}");

    inv.payment.as_mut().unwrap().means = Some(PaymentMeans::Card(PaymentCard {
        primary_account_number: Some("############1234".to_owned()),
        holder_name: Some("A. Muster".to_owned()),
    }));
    assert!(!profiles::XRECHNUNG.validate(&inv).has("BR-DE-24-a"));
}

/// Direct debit requires its own terms, and the IBAN checksum is verified.
#[test]
fn direct_debit_requires_its_own_terms_and_a_real_iban() {
    let mut inv = xrechnung_valid();
    inv.payment = Some(PaymentInstructions {
        means_code: Some(Code::new("59")), // SEPA direct debit
        means: Some(PaymentMeans::DirectDebit(DirectDebit::default())),
        ..Default::default()
    });

    let report = profiles::XRECHNUNG.validate(&inv);
    assert!(
        report.has("BR-DE-30"),
        "BT-90 creditor identifier: {report}"
    );
    assert!(report.has("BR-DE-31"), "BT-91 debited account: {report}");

    // BT-89 via `PEPPOL-EN16931-R061`, which KoSIT merges into XRechnung in
    // place of the withdrawn `BR-DE-29`.
    assert!(
        report.has("PEPPOL-EN16931-R061"),
        "R061 replaced BR-DE-29 and is merged into XRechnung:\n{report}"
    );

    // Fill them, but with a mistyped IBAN.
    inv.payment.as_mut().unwrap().means = Some(PaymentMeans::DirectDebit(DirectDebit {
        mandate_reference: Some("MANDATE-1".to_owned()),
        creditor_identifier: Some("DE98ZZZ09999999999".to_owned()),
        debited_account: Some("DE89370400440532013001".to_owned()), // last digit wrong
    }));
    let report = profiles::XRECHNUNG.validate(&inv);
    assert!(!report.has("BR-DE-30"));
    assert!(!report.has("BR-DE-31"));
    assert!(report.has("BR-DE-20"), "mod-97 catches the typo: {report}");
    // Advisory only: a checksum is not a registry lookup.
    assert!(report.is_valid(), "{report}");

    // Correct the checksum and the warning goes.
    inv.payment.as_mut().unwrap().means = Some(PaymentMeans::DirectDebit(DirectDebit {
        mandate_reference: Some("MANDATE-1".to_owned()),
        creditor_identifier: Some("DE98ZZZ09999999999".to_owned()),
        debited_account: Some("DE89370400440532013000".to_owned()),
    }));
    assert!(!profiles::XRECHNUNG.validate(&inv).has("BR-DE-20"));
}

/// `BR-DE-26` — a corrected invoice must say what it corrects.
#[test]
fn a_corrected_invoice_must_reference_the_original() {
    let mut inv = xrechnung_valid();
    inv.type_code = Some(Code::new("384")); // corrected invoice

    assert!(profiles::XRECHNUNG.validate(&inv).has("BR-DE-26"));

    inv.preceding_invoices = vec![PrecedingInvoice {
        reference: DocumentReference::new("INV-2026-000"),
        issue_date: Some(Date::parse("2026-05-31").unwrap()),
    }];
    assert!(!profiles::XRECHNUNG.validate(&inv).has("BR-DE-26"));
}

/// `BR-DE-27` / `BR-DE-28`: contact shape checks, not RFC parsers.
#[test]
fn contact_formats_are_shape_checked() {
    let mut inv = xrechnung_valid();

    inv.seller.contact.phone = Some("ext.".to_owned()); // fewer than three digits
    assert!(profiles::XRECHNUNG.validate(&inv).has("BR-DE-27"));
    inv.seller.contact.phone = Some("+49 30 123456".to_owned());
    assert!(!profiles::XRECHNUNG.validate(&inv).has("BR-DE-27"));

    inv.seller.contact.email = Some("not-an-address".to_owned());
    assert!(profiles::XRECHNUNG.validate(&inv).has("BR-DE-28"));
    inv.seller.contact.email = Some("rechnung@seller.de".to_owned());
    assert!(!profiles::XRECHNUNG.validate(&inv).has("BR-DE-28"));
}

/// `BR-DE-16`: a seller charging VAT needs BT-31 or BT-32.
#[test]
fn a_vat_charging_seller_needs_a_tax_identifier() {
    let mut inv = xrechnung_valid();
    inv.seller.vat_identifier = None;
    inv.seller.tax_registration = None;

    let report = profiles::XRECHNUNG.validate(&inv);
    assert!(report.has("BR-DE-16"), "{report}");

    inv.seller.tax_registration = Some("DE 199/123/45678".to_owned());
    assert!(!profiles::XRECHNUNG.validate(&inv).has("BR-DE-16"));
}

/// XRechnung merges Peppol's R120 with a lower severity and a wider HUF slack.
#[test]
fn xrechnung_rewrites_r120_on_the_way_in() {
    // 1. Severity: fatal under Peppol, warning under XRechnung.
    let mut inv = xrechnung_valid();
    inv.lines[0].price.net_price = UnitPriceAmount::new(dec!(1));

    let de = profiles::XRECHNUNG.validate(&inv);
    assert!(de.has("PEPPOL-EN16931-R120"), "R120 still runs:\n{de}");
    assert!(
        de.is_valid(),
        "…but only as a warning, so the document stands:\n{de}"
    );
    assert!(
        de.warnings().any(|f| f.rule.contains("R120")),
        "and it is reported as one:\n{de}"
    );

    let mut peppol_doc = inv.clone();
    peppol_doc.specification_id = Some(profiles::PEPPOL_BIS_3.specification_id.to_owned());
    let pe = profiles::PEPPOL_BIS_3.validate(&peppol_doc);
    assert!(!pe.is_valid(), "Peppol keeps it fatal:\n{pe}");

    // 2. Slack: XRechnung allows 0.5 for HUF; Peppol always 0.02.
    let mut huf = xrechnung_valid();
    huf.currency = Some(Code::new("HUF"));
    huf.lines[0].net_amount = InvoiceAmount::parse("100.30").unwrap();
    assert!(
        !profiles::XRECHNUNG
            .validate(&huf)
            .has("PEPPOL-EN16931-R120"),
        "0.30 is within XRechnung's HUF slack"
    );

    let mut huf_peppol = huf.clone();
    huf_peppol.specification_id = Some(profiles::PEPPOL_BIS_3.specification_id.to_owned());
    assert!(
        profiles::PEPPOL_BIS_3
            .validate(&huf_peppol)
            .has("PEPPOL-EN16931-R120"),
        "…and outside Peppol's, which is 0.02 for every currency"
    );
}

/// Peppol's code-list rules (`CL001`…`CL008`) and VATEX pinning (`P0104`…) are
/// not merged into XRechnung.
#[test]
fn the_unmerged_peppol_rules_stay_out_of_xrechnung() {
    let ids: Vec<&str> = profiles::XRECHNUNG
        .extra_rules
        .iter()
        .map(|r| r.id.as_str())
        .collect();
    for unmerged in [
        "PEPPOL-EN16931-CL001",
        "PEPPOL-EN16931-CL008",
        "PEPPOL-EN16931-P0104",
        "PEPPOL-EN16931-P0112",
    ] {
        assert!(!ids.contains(&unmerged), "{unmerged} must not be merged");
    }
    // …and the merged ones are there.
    assert!(ids.contains(&"PEPPOL-EN16931-R061"));
    assert!(ids.contains(&"PEPPOL-EN16931-R120"));
}

/// A CVD invoice's BT-158 scheme `CVD` is not in UNTDID 7143: core `BR-CL-13`
/// rejects it, so CVD has no `Underlies` impl for core.
#[test]
fn a_cvd_invoice_can_be_core_invalid() {
    let mut inv = xrechnung_valid();
    inv.specification_id = Some(profiles::XRECHNUNG_CVD.specification_id.to_owned());
    inv.contract_reference = Some(en16931::DocumentReference::new("V-2026-88"));
    inv.tender_reference = Some(en16931::DocumentReference::new("LOS-3"));
    inv.lines[0].item.classification_identifiers = vec![Identifier::schemed("N1", "CVD")];
    inv.lines[0].item.attributes = vec![ItemAttribute {
        name: Some("cva".to_owned()),
        value: Some("zero-emission".to_owned()),
    }];

    let cvd = profiles::XRECHNUNG_CVD.validate(&inv);
    assert!(cvd.is_valid(), "a conforming CVD invoice:\n{cvd}");

    let core = en16931::validate(&inv);
    assert!(
        core.has("BR-CL-13"),
        "…which core EN 16931 rejects, because `CVD` is not in UNTDID 7143:\n{core}"
    );
    assert!(
        !profiles::XRECHNUNG_CVD.is_conformant_cius(),
        "so CVD is not a conformant CIUS, and nothing may widen out of it"
    );
}

// ── Suppressions ──────────────────────────────────────────────────────────────

/// A suppressed rule is skipped, and the report says so.
#[test]
fn suppression_is_loud() {
    use en16931::validation::Check;

    let mut inv = xrechnung_valid();
    inv.buyer_reference = None; // BR-DE-15 and PEPPOL-EN16931-R003

    let plain = profiles::XRECHNUNG.validate(&inv);
    assert!(plain.has("BR-DE-15"), "the rule fires normally:\n{plain}");
    assert!(plain.suppressed().is_empty());

    let deviated = Check::new(&profiles::XRECHNUNG)
        .without("BR-DE-15")
        .run(&inv);
    assert!(!deviated.has("BR-DE-15"), "suppressed:\n{deviated}");
    assert_eq!(deviated.suppressed(), ["BR-DE-15"]);
    assert!(
        deviated.to_string().contains("suppressed and NOT checked"),
        "a stored report must not misrepresent what ran:\n{deviated}"
    );
    assert!(deviated.rules_checked() < plain.rules_checked());
}

/// Suppression accepts any spelling of the id, like every other lookup here.
#[test]
fn suppression_matches_ids_canonically() {
    use en16931::validation::Check;
    let inv = core_valid();
    let report = Check::new(&profiles::EN16931).without("br-co-3").run(&inv);
    assert_eq!(report.suppressed(), ["br-co-3"]);
}

/// A run with suppressions cannot produce a `Validated<P>` proof.
#[test]
fn a_deviated_run_refuses_to_prove() {
    use en16931::validation::{Check, ProveError};

    let inv = xrechnung_valid();
    let proof = Check::new(&profiles::XRECHNUNG).prove::<XRechnung>(inv.clone());
    assert!(proof.is_ok(), "a clean run proves");

    let refused = Check::new(&profiles::XRECHNUNG)
        .without("BR-DE-15")
        .prove::<XRechnung>(inv);
    match refused {
        Err(ProveError::Suppressed(ids)) => assert_eq!(ids, ["BR-DE-15"]),
        Err(e) => panic!("wrong error: {e}"),
        Ok(_) => panic!("a suppressed run must not yield a proof"),
    }
}

/// A report names the profile it came from, and the edition.
#[test]
fn a_report_says_what_it_checked_against() {
    let report = profiles::XRECHNUNG.validate(&Invoice::default());
    assert_eq!(report.profile(), Some("XRechnung 3.0"));
    assert_eq!(report.edition(), en16931::Edition::En2017A1);
    let shown = report.to_string();
    assert!(shown.starts_with("XRechnung 3.0 validation"), "{shown}");

    let core = en16931::validate(&Invoice::default());
    assert_eq!(core.profile(), None);
    assert!(core.to_string().starts_with("EN 16931 validation"));
}

/// A report prints each finding's severity and a per-severity summary.
#[test]
fn a_report_says_how_badly_each_finding_matters() {
    let report = profiles::XRECHNUNG.validate(&Invoice::default());
    let shown = report.to_string();

    let fatal = report.fatal().count();
    let advisory = report.advisory().count();
    assert!(
        fatal > 0 && advisory > 0,
        "the fixture must exercise both: {shown}"
    );

    for finding in report.findings() {
        let line = finding.to_string();
        assert!(
            line.starts_with(&finding.severity.to_string()),
            "a finding must lead with its severity: {line}"
        );
    }

    let header = shown.lines().next().expect("a header");
    assert!(
        header.contains(&format!("{fatal} fatal")),
        "the header breaks the count down by severity: {header}"
    );
}

/// The core path and the `EN 16931` profile report the same count and findings.
#[test]
fn the_core_path_and_the_en16931_profile_agree() {
    for invoice in [en16931::Invoice::default(), corpus_invoice()] {
        let core = en16931::validate(&invoice);
        let profile = en16931::profiles::EN16931.validate(&invoice);

        assert_eq!(
            core.rules_checked(),
            profile.rules_checked(),
            "the two paths disagree about how many rules ran"
        );
        let core_rules: Vec<&str> = core.findings().iter().map(|f| f.rule.as_str()).collect();
        let profile_rules: Vec<&str> = profile.findings().iter().map(|f| f.rule.as_str()).collect();
        assert_eq!(
            core_rules, profile_rules,
            "the two paths disagree on findings"
        );
    }
}

/// An invoice carrying an extension, so `EN-EXT-01` is genuinely applicable.
fn corpus_invoice() -> en16931::Invoice {
    let mut inv = en16931::Invoice::default();
    inv.extensions
        .third_party_payments
        .push(en16931::ThirdPartyPayment {
            payment_type: Some("BG-DEX-01".into()),
            amount: None,
            description: None,
        });
    inv
}

// ── Edge cases ────────────────────────────────────────────────────────────────

/// The XRechnung Extension accepts an `application/xml` attachment (`BR-DEX-01`);
/// KoSIT relaxes `BR-CL-24` to information there.
#[test]
fn the_extension_accepts_the_xml_attachment_br_dex_01_permits() {
    let mut inv = xrechnung_valid();
    inv.specification_id = Some(profiles::XRECHNUNG_EXTENSION.specification_id.to_owned());
    inv.attachments.push(SupportingDocument {
        reference: DocumentReference::new("ANLAGE-1"),
        description: None,
        uri: None,
        attachment: Some(
            Attachment::new(b"<x/>".to_vec(), "application/xml", "anlage.xml").expect("attachment"),
        ),
    });

    let report = profiles::XRECHNUNG_EXTENSION.validate(&inv);
    assert!(
        !report.fatal().any(|f| f.rule == "BR-CL-24"),
        "BR-DEX-01 permits application/xml:\n{report}"
    );
    assert!(
        !report.has("BR-DEX-01"),
        "…and the Extension's own rule agrees:\n{report}"
    );
    assert!(
        report
            .info()
            .any(|f| f.rule == "BR-CL-24" && f.severity == Severity::Info),
        "reported at KoSIT's level:\n{report}"
    );
    // The plain CIUS still rejects it.
    let mut cius = inv.clone();
    cius.specification_id = Some(profiles::XRECHNUNG.specification_id.to_owned());
    assert!(
        profiles::XRECHNUNG
            .validate(&cius)
            .fatal()
            .any(|f| f.rule == "BR-CL-24")
    );
}

/// `BR-CL-10` / `BR-CL-11` cover every party identification; the `SEPA` scheme
/// is admitted only for the seller and the payee.
#[test]
fn the_party_identifier_scheme_rules_cover_every_party() {
    let bad = || Identifier::schemed("X", "NOT-AN-ICD");

    let mut inv = core_valid();
    inv.payee = Some(Payee {
        name: Some("Factoring GmbH".to_owned()),
        identifier: Some(bad()),
        legal_registration: Some(bad()),
    });
    let report = en16931::validate(&inv);
    assert!(report.has("BR-CL-10"), "BT-60:\n{report}");
    assert!(report.has("BR-CL-11"), "BT-61:\n{report}");

    let mut ok = core_valid();
    ok.seller.identifiers = vec![Identifier::schemed("DE98ZZZ09999999999", "SEPA")];
    ok.payee = Some(Payee {
        name: Some("Factoring GmbH".to_owned()),
        identifier: Some(Identifier::schemed("DE98ZZZ09999999999", "SEPA")),
        legal_registration: None,
    });
    assert!(!en16931::validate(&ok).has("BR-CL-10"));

    let mut buyer = core_valid();
    buyer.buyer.identifiers = vec![Identifier::schemed("DE98ZZZ09999999999", "SEPA")];
    assert!(
        en16931::validate(&buyer).has("BR-CL-10"),
        "SEPA is not admissible on the buyer"
    );
}

/// Suppressing a rule the profile does not run leaves the count unchanged.
#[test]
fn an_unknown_suppression_does_not_shrink_the_count() {
    use en16931::validation::Check;

    let plain = profiles::EN16931.validate(&core_valid());
    let bogus = Check::new(&profiles::EN16931)
        .without("BR-DE-15") // a real rule, but not one this profile runs
        .without("NOT-A-RULE-AT-ALL")
        .run(&core_valid());

    assert_eq!(
        bogus.rules_checked(),
        plain.rules_checked(),
        "nothing was skipped, so nothing may be deducted"
    );
    // The request is still recorded.
    assert_eq!(bogus.suppressed().len(), 2);

    // A suppression that *does* bite deducts exactly one.
    let real = Check::new(&profiles::EN16931)
        .without("BR-CO-26")
        .run(&core_valid());
    assert_eq!(real.rules_checked(), plain.rules_checked() - 1);
}

/// Sub-lines keyed (by BG-25 index) to a line that does not exist are reported,
/// since no writer or rule would ever reach them.
#[test]
fn stranded_sub_invoice_lines_are_a_finding() {
    let mut inv = xrechnung_valid();
    inv.specification_id = Some(profiles::XRECHNUNG_EXTENSION.specification_id.to_owned());
    let sub = en16931::SubInvoiceLine {
        line: inv.lines[0].clone(),
        vat: Some(inv.lines[0].vat.clone()),
        quantity_stated: true,
        price_stated: true,
        children: vec![],
    };
    inv.extensions
        .sub_invoice_lines
        .push((0, vec![sub.clone()]));
    assert!(
        !profiles::XRECHNUNG_EXTENSION
            .validate(&inv)
            .has("EN-EXT-02"),
        "index 0 names the line that is there"
    );

    inv.extensions.sub_invoice_lines = vec![(7, vec![sub])];
    let report = profiles::XRECHNUNG_EXTENSION.validate(&inv);
    assert!(report.has("EN-EXT-02"), "index 7 names nothing:\n{report}");
    assert!(
        !report.is_valid(),
        "and it is fatal — the data would vanish"
    );
}

/// Every profile's check count, as the READMEs, `lib.rs` and the site quote it.
#[test]
fn the_documented_profile_check_counts_are_measured() {
    let documented = [
        (&profiles::EN16931, 227),
        (&profiles::XRECHNUNG, 282),
        (&profiles::XRECHNUNG_CVD, 290),
        (&profiles::XRECHNUNG_EXTENSION, 296),
        (&profiles::PEPPOL_BIS_3, 290),
    ];
    let mut wrong = Vec::new();
    for (profile, want) in documented {
        let got = profile.check_ids().count();
        if got != want {
            wrong.push(format!("  {} — {got}, documented as {want}", profile.id));
        }
    }
    assert!(
        wrong.is_empty(),
        "a documented profile check count is wrong.\n{}\n\
         Either the rule should not be there, or every file quoting the old \
         number needs updating:\n  \
         rg -n '<old>' README.md crates/*/README.md crates/en16931/src/lib.rs site/content",
        wrong.join("\n")
    );
}

/// A report's `rules_checked` equals the profile's registered count on any document.
#[test]
fn a_report_says_it_checked_what_the_profile_registers() {
    use en16931::extensions::SubInvoiceLine;

    let mut with_extension = xrechnung_valid();
    with_extension.extensions.sub_invoice_lines = vec![(
        0,
        vec![SubInvoiceLine {
            line: with_extension.lines[0].clone(),
            vat: Some(with_extension.lines[0].vat.clone()),
            quantity_stated: true,
            price_stated: true,
            children: vec![],
        }],
    )];

    for (label, inv) in [
        ("empty", Invoice::default()),
        ("with extension data", with_extension),
    ] {
        for p in profiles::ALL {
            assert_eq!(
                p.validate(&inv).rules_checked(),
                p.check_ids().count(),
                "{} on an {label} invoice reports a count it cannot justify",
                p.id
            );
        }
        assert_eq!(
            en16931::validate(&inv).rules_checked(),
            profiles::EN16931.check_ids().count(),
            "the bare core path on an {label} invoice"
        );
    }
}

// ── Valid starting points and profile names ───────────────────────────────────

/// Every shipped profile has a valid example declaring it; ZUGFeRD profiles have none.
#[test]
fn every_shipped_profile_has_a_valid_example() {
    for p in en16931::profiles::ALL {
        let invoice = en16931::examples::for_profile(p).expect("a shipped profile");
        assert_eq!(
            invoice.specification_id.as_deref(),
            Some(p.specification_id)
        );
        let report = p.validate(&invoice);
        assert_eq!(report.fatal().count(), 0, "{}: {report}", p.id);
        assert_eq!(report.warnings().count(), 0, "{}: {report}", p.id);
    }
    for p in en16931::profiles::ZUGFERD {
        assert!(en16931::examples::for_profile(p).is_none(), "{}", p.id);
    }
}

/// Change one field of a valid example and the report names exactly that.
#[test]
fn an_example_without_bt_1_has_exactly_one_fatal_finding() {
    for p in en16931::profiles::ALL {
        let mut invoice = en16931::examples::for_profile(p).expect("a shipped profile");
        invoice.number = None;
        let report = p.validate(&invoice);
        let fatal: Vec<&str> = report.fatal().map(|f| f.rule.as_str()).collect();
        assert_eq!(fatal, ["BR-02"], "{}: {report}", p.id);
    }
}

/// Profiles parse from slug, name, alias or BT-24; an unknown name lists the slugs.
#[test]
fn a_profile_parses_from_slug_name_alias_or_bt24() {
    use en16931::Profile;
    use en16931::profiles::{EN16931, PEPPOL_BIS_3, XRECHNUNG};

    for (spelling, want) in [
        ("core", &EN16931),
        ("CORE", &EN16931),
        ("bis3", &PEPPOL_BIS_3),
        ("peppol-bis-3", &PEPPOL_BIS_3),
        ("XRechnung 3.0", &XRECHNUNG),
        ("xrechnung", &XRECHNUNG),
        (XRECHNUNG.specification_id, &XRECHNUNG),
    ] {
        let got: &Profile = spelling.parse().expect(spelling);
        assert!(std::ptr::eq(got, want), "{spelling}");
    }
    let err = "xrechung".parse::<&Profile>().unwrap_err();
    assert_eq!(err.name, "xrechung");
    let text = err.to_string();
    for p in en16931::profiles::ALL {
        assert!(text.contains(p.slug), "{text}");
    }
}
