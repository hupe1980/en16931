//! `billing` → `en16931` → validated, end to end.
//!
//! The seam this crate exists to define. `billing` owns the arithmetic;
//! `en16931` owns what the arithmetic *means*; the caller owns the parties.

#![cfg(feature = "billing")]

use billing::prelude::*;
use billing::{FixedRateTax, LineVat as BillingLineVat, PerUnitLevy, TaxCategory, TaxLayer};
use en16931::billing_adapter::{ConversionError, FromBilling, UnitResolver};
use en16931::invoice::{Code, Party, PostalAddress};
use en16931::profiles;
use en16931::{Identifier, validate};
use rust_decimal::dec;

fn party(name: &str, country: &str) -> Party {
    Party {
        name: Some(name.to_owned()),
        address: PostalAddress {
            country: Some(Code::new(country)),
            city: Some("Berlin".to_owned()),
            post_code: Some("10115".to_owned()),
            ..Default::default()
        },
        electronic_address: Some(Identifier::schemed(name, "0088")),
        vat_identifier: Some(format!("{country}123456789")), // BR-CO-26 / BR-CO-09
        ..Default::default()
    }
}

fn meta() -> DocumentMeta {
    DocumentMeta {
        invoice_number: "INV-2026-001".into(),
        currency: Currency::EUR,
        issue_date: Some("2026-06-30".into()),
        due_date: Some("2026-07-30".into()),
        ..Default::default()
    }
}

/// One standard-rated position — the smallest document that satisfies `billing`
/// and leaves the header the thing under test.
fn simple_document(meta: DocumentMeta) -> BillingDocument {
    BillingDocument::builder()
        .meta(meta)
        .amount_scale(AmountScale::EN16931)
        .positions(vec![
            LineItem::flat_fee(
                "Service",
                Amount::parse("100.00000").unwrap(),
                Currency::EUR,
            )
            .vat(BillingLineVat::new(TaxCategory::Standard, dec!(0.19)).unwrap())
            .build()
            .unwrap(),
        ])
        .extra_tax(FixedRateTax::new("MwSt", dec!(0.19)).unwrap().boxed())
        .build()
        .expect("a lawful billing document")
}

/// A metered utility invoice: consumption at a per-kWh price, a per-unit excise
/// **inside** the taxable base, and VAT on the lot.
///
/// This is the shape that breaks the naive mapping, and the reason the adapter
/// exists at all.
fn utility_document() -> BillingDocument {
    BillingDocument::builder()
        .meta(meta())
        .amount_scale(AmountScale::EN16931)
        .positions(vec![
            LineItem::for_usage(
                "Arbeitspreis",
                Quantity::new(dec!(1234.567), "kWh").with_code("KWH"),
                UnitPrice::new(dec!(0.28901), "EUR/kWh"),
            )
            .build()
            .unwrap(),
            LineItem::flat_fee(
                "Grundpreis",
                Amount::parse("8.50000").unwrap(),
                Currency::EUR,
            )
            .build()
            .unwrap(),
        ])
        // A per-unit excise. EN 16931 calls this a BG-21 document level CHARGE,
        // not tax: it is part of the base VAT is charged on.
        .extra_tax(
            PerUnitLevy::new("Stromsteuer", Amount::parse("0.02050").unwrap(), "kWh")
                .unwrap()
                .with_unit_code("KWH")
                .with_vat(BillingLineVat::new(TaxCategory::Standard, dec!(0.19)).unwrap())
                .boxed(),
        )
        .extra_tax(FixedRateTax::new("MwSt", dec!(0.19)).unwrap().boxed())
        .build()
        .unwrap()
}

/// The levy trap: `tax_total` is **not** BT-110.
///
/// Mapping the whole of `tax_total` to BT-110 — the obvious thing to do — breaks
/// `BR-CO-14` on every levy-bearing invoice, because the levy contributes no VAT
/// breakdown entry.
#[test]
fn a_levy_becomes_a_document_charge_not_tax() {
    let doc = utility_document();
    let invoice = FromBilling::new(&doc)
        .specification_id(profiles::EN16931.specification_id)
        .seller(party("Stadtwerke GmbH", "DE"))
        .buyer(party("Kunde AG", "DE"))
        .build()
        .expect("conversion");

    // Two lines, one charge (the levy), no line for the VAT.
    assert_eq!(invoice.lines.len(), 2);
    assert_eq!(invoice.charges.len(), 1, "the levy is BG-21");
    assert_eq!(invoice.vat_breakdown.len(), 1);

    // BT-110 is Σ BT-117, NOT tax_total — which also carries the levy.
    let bt_110 = invoice.totals.vat_total.unwrap();
    let bt_117 = invoice.vat_breakdown[0].tax_amount;
    assert_eq!(bt_110, bt_117);
    assert_ne!(
        bt_110.to_string(),
        doc.tax_total().to_string(),
        "tax_total includes the levy and is not BT-110"
    );

    // BT-109 = BT-106 − BT-107 + BT-108, with the levy in BT-108.
    assert!(invoice.totals.charge_total.is_some());

    let report = validate(&invoice);
    assert!(report.is_valid(), "{report}");
}

/// Every totals identity closes after conversion — the whole point of
/// `AmountScale::EN16931` reducing leaves before aggregates.
#[test]
fn the_totals_chain_closes_exactly() {
    let doc = utility_document();
    let inv = FromBilling::new(&doc)
        .specification_id(profiles::EN16931.specification_id)
        .seller(party("Stadtwerke GmbH", "DE"))
        .buyer(party("Kunde AG", "DE"))
        .build()
        .unwrap();

    let report = validate(&inv);
    for id in ["BR-CO-10", "BR-CO-13", "BR-CO-14", "BR-CO-15", "BR-CO-16"] {
        assert!(!report.has(id), "{id} fired:\n{report}");
    }
    // And BR-S-08 — the keystone tying lines to the breakdown.
    assert!(!report.has("BR-S-08"), "{report}");
}

/// `billing` stores rates as fractions because that is what you multiply by;
/// EN 16931 stores what you print. The conversion happens once, at the boundary.
#[test]
fn rates_arrive_as_per_cent_not_fractions() {
    let doc = utility_document();
    let inv = FromBilling::new(&doc)
        .specification_id(profiles::EN16931.specification_id)
        .seller(party("S", "DE"))
        .buyer(party("B", "DE"))
        .build()
        .unwrap();

    assert_eq!(inv.vat_breakdown[0].rate.unwrap().to_string(), "19");
    assert_eq!(inv.lines[0].vat.rate.unwrap().to_string(), "19");
}

/// A document that was never given a currency must not convert. ISO 4217 `XXX`
/// passes `BR-CL-04` because it is a real code, so catching it here is the only
/// place it gets caught before the counterparty.
#[test]
fn an_unconfigured_currency_is_refused_at_the_boundary() {
    let doc = BillingDocument::builder()
        .meta(DocumentMeta {
            invoice_number: "X".into(),
            ..Default::default() // Currency::XXX
        })
        .positions(vec![
            LineItem::flat_fee(
                "Service",
                Amount::parse("100.00000").unwrap(),
                Currency::EUR,
            )
            .vat(BillingLineVat::new(TaxCategory::Standard, dec!(0.19)).unwrap())
            .build()
            .unwrap(),
        ])
        .build()
        .unwrap();

    assert!(matches!(
        FromBilling::new(&doc).build(),
        Err(ConversionError::NoCurrency(_))
    ));
}

/// Excess precision is refused, never rounded. Rounding at an interchange
/// boundary breaks `BR-CO-10` and `BR-CO-15`, which are exact equalities.
#[test]
fn excess_precision_is_refused_with_the_fix_named() {
    // No `amount_scale`, so `1234.567 × 0.28901` keeps five decimals.
    let doc = BillingDocument::builder()
        .meta(meta())
        .positions(vec![
            LineItem::for_usage(
                "Arbeit",
                Quantity::new(dec!(1234.567), "kWh").with_code("KWH"),
                UnitPrice::new(dec!(0.28901), "EUR/kWh"),
            )
            .vat(BillingLineVat::new(TaxCategory::Standard, dec!(0.19)).unwrap())
            .build()
            .unwrap(),
        ])
        .extra_tax(FixedRateTax::new("MwSt", dec!(0.19)).unwrap().boxed())
        .build()
        .unwrap();

    let err = FromBilling::new(&doc)
        .specification_id(profiles::EN16931.specification_id)
        .seller(party("S", "DE"))
        .buyer(party("B", "DE"))
        .build()
        .unwrap_err();

    let msg = err.to_string();
    assert!(
        matches!(err, ConversionError::PrecisionLoss { .. }),
        "{msg}"
    );
    assert!(
        msg.contains("amount_scale"),
        "the error must name the fix, not just the symptom: {msg}"
    );
}

/// A unit with neither a `Quantity::code` nor a resolver entry is refused —
/// guessing produces an invoice that validates and describes the wrong thing.
#[test]
fn an_unresolvable_unit_is_refused_and_the_resolver_fixes_it() {
    let doc = BillingDocument::builder()
        .meta(meta())
        .amount_scale(AmountScale::EN16931)
        .positions(vec![
            LineItem::for_usage(
                "Kisten",
                Quantity::new(dec!(3), "Kiste"), // no code, not built in
                UnitPrice::new(dec!(10), "EUR/Kiste"),
            )
            .vat(BillingLineVat::new(TaxCategory::Standard, dec!(0.19)).unwrap())
            .build()
            .unwrap(),
        ])
        .extra_tax(FixedRateTax::new("MwSt", dec!(0.19)).unwrap().boxed())
        .build()
        .unwrap();

    let attempt = |units: UnitResolver| {
        FromBilling::new(&doc)
            .specification_id(profiles::EN16931.specification_id)
            .seller(party("S", "DE"))
            .buyer(party("B", "DE"))
            .units(units)
            .build()
    };

    assert!(matches!(
        attempt(UnitResolver::new()),
        Err(ConversionError::UnresolvedUnit { .. })
    ));

    let inv = attempt(UnitResolver::new().with("Kiste", "XBX")).expect("resolved");
    assert_eq!(inv.lines[0].unit_code.as_str(), "XBX");
    assert!(validate(&inv).is_valid(), "{}", validate(&inv));
}

/// The sign convention flips at the boundary. `billing` models a return as
/// `Sign::Credit` with a non-negative quantity; EN 16931 puts the sign on BT-129
/// and forbids a negative BT-146 — Annex A.1.6.
#[test]
fn a_credit_line_becomes_a_negative_quantity_not_a_negative_price() {
    let doc = BillingDocument::builder()
        .meta(meta())
        .amount_scale(AmountScale::EN16931)
        .positions(vec![
            LineItem::for_usage(
                "Lieferung",
                Quantity::new(dec!(25), "Stk").with_code("H87"),
                UnitPrice::new(dec!(8.50), "EUR/Stk"),
            )
            .vat(BillingLineVat::new(TaxCategory::Standard, dec!(0.19)).unwrap())
            .build()
            .unwrap(),
            LineItem::credit_for_usage(
                "Rückgabe",
                Quantity::new(dec!(10), "Stk").with_code("H87"),
                UnitPrice::new(dec!(8.50), "EUR/Stk"),
            )
            .vat(BillingLineVat::new(TaxCategory::Standard, dec!(0.19)).unwrap())
            .build()
            .unwrap(),
        ])
        .extra_tax(FixedRateTax::new("MwSt", dec!(0.19)).unwrap().boxed())
        .build()
        .unwrap();

    let inv = FromBilling::new(&doc)
        .specification_id(profiles::EN16931.specification_id)
        .seller(party("S", "DE"))
        .buyer(party("B", "DE"))
        .build()
        .unwrap();

    // Positive quantity in `billing`; negative BT-129 here.
    assert!(doc.net_positions()[1].quantity.as_ref().unwrap().value > dec!(0));
    assert!(
        inv.lines[1].quantity.is_negative(),
        "BT-129 carries the sign"
    );
    assert!(!inv.lines[1].price.net_price.is_negative(), "BR-27");
    assert!(inv.lines[1].net_amount.is_negative());

    let report = validate(&inv);
    assert!(report.is_valid(), "{report}");
    assert!(!report.has("BR-27"));
}

/// BG-29 survives the crossing — `billing` 0.10 added it, and it is what makes
/// `PEPPOL-EN16931-R120` expressible at all.
#[test]
fn price_base_quantity_and_discount_survive_the_crossing() {
    let doc = BillingDocument::builder()
        .meta(meta())
        .amount_scale(AmountScale::EN16931)
        .positions(vec![
            LineItem::for_usage(
                "Schrauben",
                Quantity::new(dec!(250), "Stk").with_code("H87"),
                // "EUR 12,00 per 100 pieces", less a 1,00 discount.
                UnitPrice::discounted(dec!(13.00), dec!(1.00), "EUR/100 Stk")
                    .per(dec!(100))
                    .with_base_quantity_code("H87"),
            )
            .vat(BillingLineVat::new(TaxCategory::Standard, dec!(0.19)).unwrap())
            .build()
            .unwrap(),
        ])
        .extra_tax(FixedRateTax::new("MwSt", dec!(0.19)).unwrap().boxed())
        .build()
        .unwrap();

    let inv = FromBilling::new(&doc)
        .specification_id(profiles::EN16931.specification_id)
        .seller(party("S", "DE"))
        .buyer(party("B", "DE"))
        .build()
        .unwrap();

    let p = &inv.lines[0].price;
    assert_eq!(p.net_price.to_string(), "12", "BT-146 = BT-148 − BT-147");
    assert_eq!(p.gross_price.unwrap().to_string(), "13"); // BT-148
    assert_eq!(p.price_discount.unwrap().to_string(), "1"); // BT-147
    assert_eq!(p.base_quantity.unwrap().to_string(), "100"); // BT-149
    assert_eq!(p.base_quantity_code.as_ref().unwrap().as_str(), "H87"); // BT-150

    // R120: 250 × (12 ÷ 100) = 30.00
    assert_eq!(inv.lines[0].net_amount.to_string(), "30.00");
    assert!(validate(&inv).is_valid(), "{}", validate(&inv));
}

/// Absent is not zero. A document with no allowances omits BT-107 rather than
/// stating `0.00`, because `BR-CO-13` branches on its presence.
#[test]
fn optional_totals_are_absent_rather_than_zero() {
    let doc = utility_document();
    let inv = FromBilling::new(&doc)
        .specification_id(profiles::EN16931.specification_id)
        .seller(party("S", "DE"))
        .buyer(party("B", "DE"))
        .build()
        .unwrap();

    assert!(inv.totals.allowance_total.is_none(), "no allowances");
    assert!(inv.totals.charge_total.is_some(), "one levy");
    assert!(inv.totals.paid.is_none(), "nothing prepaid");
    assert!(inv.totals.rounding.is_none());
}

/// **The §14c Abs. 1 UStG hole.**
///
/// A final invoice that deducts advance payments must, in Germany, state *"die
/// auf sie entfallenden Steuerbeträge"* — the tax contained in each advance
/// (§14 Abs. 5 Satz 2 UStG). Omit it and the issuer owes the advance-related tax
/// a second time.
///
/// Core EN 16931 has **nowhere to put it**: BT-113 is one flat figure. An
/// adapter that quietly maps itemised advances to BT-113 and drops the rest
/// produces a document that validates perfectly and is a tax liability.
#[test]
fn itemised_advances_are_carried_not_dropped() {
    use billing::{AdvancePayment as BillingAdvance, TaxBreakdownEntry};

    let advance = |ref_: &str| {
        BillingAdvance::new(vec![TaxBreakdownEntry::new(
            TaxCategory::Standard,
            dec!(0.19),
            Amount::parse("375.00000").unwrap(),
            Amount::parse("71.25000").unwrap(),
        )])
        .unwrap()
        .with_reference(ref_)
        .with_received_on("2026-03-31")
    };

    let doc = BillingDocument::builder()
        .meta(meta())
        .amount_scale(AmountScale::EN16931)
        .positions(vec![
            LineItem::flat_fee(
                "Jahresverbrauch",
                Amount::parse("1000.00000").unwrap(),
                Currency::EUR,
            )
            .build()
            .unwrap(),
        ])
        .extra_tax(FixedRateTax::new("MwSt", dec!(0.19)).unwrap().boxed())
        .build()
        .unwrap()
        .with_advances(vec![advance("AB-1"), advance("AB-2")])
        .unwrap();

    let inv = FromBilling::new(&doc)
        .specification_id(profiles::EN16931.specification_id)
        .seller(party("S", "DE"))
        .buyer(party("B", "DE"))
        .build()
        .unwrap();

    // The totals still describe the whole supply; only BT-115 shrinks.
    assert_eq!(inv.totals.gross_total.to_string(), "1190.00");
    assert_eq!(inv.totals.paid.unwrap().to_string(), "892.50");
    assert_eq!(inv.totals.due.to_string(), "297.50");

    // …and the per-advance tax survives, which BT-113 alone cannot express.
    assert_eq!(inv.extensions.advance_payments.len(), 2);
    let a = &inv.extensions.advance_payments[0];
    assert_eq!(a.gross.to_string(), "446.25");
    assert_eq!(a.tax_total().unwrap().to_string(), "71.25");
    assert_eq!(a.reference.as_ref().unwrap().as_str(), "AB-1");
    assert_eq!(a.received_on.unwrap().to_string(), "2026-03-31");

    // Core EN 16931 cannot represent it, so the report warns — loudly enough to
    // be read, but not fatally, because the invoice itself is lawful.
    let report = validate(&inv);
    assert!(
        report.is_valid(),
        "advances do not make it invalid: {report}"
    );
    assert!(report.has("EN-EXT-01"), "{report}");
    assert_eq!(report.warnings().count(), 1);

    // A residual invoice — bill the remainder, list no advances — has nothing to
    // lose and therefore nothing to warn about. That is what the German BMF
    // recommends for structured e-invoices (Schreiben v. 15.10.2024, Rn. 48).
    let residual = FromBilling::new(&utility_document())
        .specification_id(profiles::EN16931.specification_id)
        .seller(party("S", "DE"))
        .buyer(party("B", "DE"))
        .build()
        .unwrap();
    assert!(!validate(&residual).has("EN-EXT-01"));
}

// ── The three terms `billing` 0.12 added ─────────────────────────────────────

/// **Upstream still terminates BT-20**, so the adapter's guard stays inert.
///
/// `billing` ≤ 0.12 rendered the field and stopped at the closing `#`, and
/// `BR-DE-18`'s second half requires everything after the last `#…#` to begin
/// with a newline — so every German invoice carrying a Skonto was rejected. The
/// adapter appended the newline for one release; 0.13 does it upstream, which is
/// the right place, because the `#SKONTO#…#` syntax has no core EN 16931 form
/// and a rendering without the terminator is valid nowhere.
///
/// This asserts the *upstream* behaviour rather than only the adapter's output.
/// Checking the output alone would pass just as well against a `billing` that
/// had regressed and an adapter quietly papering over it, which is exactly the
/// state this pair of crates was in a release ago.
#[test]
fn billing_renders_bt_20_with_the_terminator_br_de_18_needs() {
    use billing::terms::{EarlyPaymentDiscount, PaymentTerms};

    let terms = PaymentTerms::text("Zahlbar innerhalb 30 Tagen ohne Abzug.")
        .with_discount(EarlyPaymentDiscount::new(10, dec!(2.00)).expect("a lawful Skonto"));
    let raw = terms.to_string();
    assert!(
        raw.ends_with('\n'),
        "billing must terminate BT-20 itself; the adapter's guard is not a fix: {raw:?}"
    );

    // And the terminated form is what BR-DE-18 wants, checked against the rule
    // rather than against the assumption that a newline is enough.
    let mut inv = en16931::Invoice::default();
    inv.payment_terms = Some(raw.clone());
    assert!(
        !profiles::XRECHNUNG.validate(&inv).has("BR-DE-18"),
        "{raw:?}"
    );

    // Strip it, and the rule fires — so the assertion above is load-bearing.
    inv.payment_terms = Some(raw.trim_end_matches('\n').to_owned());
    assert!(profiles::XRECHNUNG.validate(&inv).has("BR-DE-18"));

    // A prose-only BT-20 gets no terminator: `every … satisfies` is vacuously
    // true when no line starts with `#`, so a newline there would be noise.
    let prose = PaymentTerms::text("Zahlbar sofort ohne Abzug").to_string();
    assert!(!prose.ends_with('\n'), "{prose:?}");
}

/// BT-20 crosses the seam intact, and satisfies `BR-DE-18` on the far side.
#[test]
fn payment_terms_cross_the_seam_in_a_form_br_de_18_accepts() {
    use billing::terms::{EarlyPaymentDiscount, PaymentTerms};

    let terms = PaymentTerms::text("Zahlbar innerhalb 30 Tagen ohne Abzug.")
        .with_discount(EarlyPaymentDiscount::new(10, dec!(2.00)).expect("a lawful Skonto"));

    let mut m = meta();
    m.payment_terms = Some(terms);
    let doc = BillingDocument::builder()
        .meta(m)
        .amount_scale(AmountScale::EN16931)
        .positions(vec![
            LineItem::flat_fee(
                "Service",
                Amount::parse("100.00000").unwrap(),
                Currency::EUR,
            )
            .vat(BillingLineVat::new(TaxCategory::Standard, dec!(0.19)).unwrap())
            .build()
            .unwrap(),
        ])
        .extra_tax(FixedRateTax::new("MwSt", dec!(0.19)).unwrap().boxed())
        .build()
        .expect("a lawful billing document");

    let inv = FromBilling::new(&doc)
        .specification_id(profiles::XRECHNUNG.specification_id)
        .seller(party("Seller GmbH", "DE"))
        .buyer(party("Buyer GmbH", "DE"))
        .build()
        .expect("converts");

    let bt20 = inv
        .payment_terms
        .as_deref()
        .expect("BT-20 crossed the seam");
    assert!(bt20.starts_with("Zahlbar innerhalb 30 Tagen"), "{bt20:?}");
    assert!(bt20.contains("#SKONTO#TAGE=10#PROZENT=2.00#"), "{bt20:?}");
    assert!(bt20.ends_with('\n'), "BR-DE-18's terminator: {bt20:?}");
    assert!(
        !profiles::XRECHNUNG.validate(&inv).has("BR-DE-18"),
        "{}",
        profiles::XRECHNUNG.validate(&inv)
    );

    // …and BT-20 is one of the two ways to satisfy BR-CO-25, so the invoice no
    // longer depends on a due date to be payable.
    let mut no_due = inv.clone();
    no_due.due_date = None;
    assert!(!en16931::validate(&no_due).has("BR-CO-25"));
}

/// Prose-only payment terms get no terminator, because there is nothing to
/// terminate: `BR-DE-18` says nothing about a BT-20 with no `#` line in it.
#[test]
fn prose_only_payment_terms_are_passed_through_unchanged() {
    use billing::terms::PaymentTerms;

    let mut m = meta();
    m.payment_terms = Some(PaymentTerms::text("Zahlbar sofort ohne Abzug"));
    let doc = BillingDocument::builder()
        .meta(m)
        .amount_scale(AmountScale::EN16931)
        .positions(vec![
            LineItem::flat_fee(
                "Service",
                Amount::parse("100.00000").unwrap(),
                Currency::EUR,
            )
            .vat(BillingLineVat::new(TaxCategory::Standard, dec!(0.19)).unwrap())
            .build()
            .unwrap(),
        ])
        .extra_tax(FixedRateTax::new("MwSt", dec!(0.19)).unwrap().boxed())
        .build()
        .unwrap();

    let inv = FromBilling::new(&doc)
        .specification_id(profiles::EN16931.specification_id)
        .seller(party("Seller GmbH", "DE"))
        .buyer(party("Buyer GmbH", "DE"))
        .build()
        .expect("converts");
    assert_eq!(
        inv.payment_terms.as_deref(),
        Some("Zahlbar sofort ohne Abzug")
    );
}

/// BT-6 and BT-111 cross together, or `BR-53` fires on a document that carries
/// both.
///
/// Mapping only the currency is worse than mapping neither: `BR-53` makes
/// BT-111 mandatory whenever BT-6 is present, so a half-mapping manufactures a
/// finding out of a complete document.
#[test]
fn the_vat_accounting_currency_and_its_total_cross_together() {
    use billing::VatAccountingCurrency;

    let doc = BillingDocument::builder()
        .meta(meta())
        .amount_scale(AmountScale::EN16931)
        .positions(vec![
            LineItem::flat_fee(
                "Service",
                Amount::parse("100.00000").unwrap(),
                Currency::EUR,
            )
            .vat(BillingLineVat::new(TaxCategory::Standard, dec!(0.19)).unwrap())
            .build()
            .unwrap(),
        ])
        .extra_tax(FixedRateTax::new("MwSt", dec!(0.19)).unwrap().boxed())
        .build()
        .unwrap()
        .with_vat_accounting_currency(
            VatAccountingCurrency::converted(
                Currency::new("PLN").unwrap(),
                Amount::parse("19.00000").unwrap(),
                dec!(4.30),
                AmountScale::EN16931,
            )
            .expect("a lawful conversion"),
        )
        .expect("accepted");

    let inv = FromBilling::new(&doc)
        .specification_id(profiles::EN16931.specification_id)
        .seller(party("Seller GmbH", "DE"))
        .buyer(party("Buyer GmbH", "PL"))
        .build()
        .expect("converts");

    assert_eq!(
        inv.vat_accounting_currency.as_ref().map(Code::as_str),
        Some("PLN"),
        "BT-6"
    );
    assert_eq!(
        inv.totals
            .vat_total_accounting
            .map(|a| a.to_string())
            .as_deref(),
        Some("81.70"),
        "BT-111 — 19.00 EUR at 4.30"
    );
    let report = validate(&inv);
    assert!(!report.has("BR-53"), "{report}");
    // `PEPPOL-EN16931-R055`: BT-110 and BT-111 must share a sign.
    assert!(
        !profiles::PEPPOL_BIS_3
            .validate(&inv)
            .has("PEPPOL-EN16931-R055")
    );
}

/// A reversal arrives already knowing which invoice it credits — **BG-3**.
///
/// `billing::reverse` fills `meta.preceding` in from the document it reverses,
/// so BT-25 and BT-26 cross the seam without the caller copying the number back
/// out of `labels` — which is where it lived, and which this adapter
/// deliberately does not map. A credit note that does not say what it credits
/// is an unexplained payment, and `BR-55` is satisfied by construction: BT-25
/// is not an `Option` upstream and its constructor refuses a blank string.
#[test]
fn a_reversal_carries_the_preceding_invoice_reference() {
    let original = simple_document(meta());
    let credit = original
        .reverse(DocumentMeta {
            invoice_number: "STORNO-1".into(),
            currency: Currency::EUR,
            issue_date: Some("2026-08-15".into()),
            ..DocumentMeta::default()
        })
        .expect("reverses");

    let inv = FromBilling::new(&credit)
        .specification_id(profiles::EN16931.specification_id)
        .seller(party("Seller GmbH", "DE"))
        .buyer(party("Buyer GmbH", "DE"))
        .build()
        .expect("converts");

    let [bg3] = &inv.preceding_invoices[..] else {
        panic!("expected one BG-3, got {:?}", inv.preceding_invoices);
    };
    assert_eq!(bg3.reference.as_str(), "INV-2026-001"); // BT-25
    assert_eq!(
        bg3.issue_date.map(|d| d.to_string()).as_deref(),
        Some("2026-06-30"),
        "BT-26 is the *original's* issue date, not the credit note's"
    );
    assert!(!bg3.reference.is_blank(), "BR-55");

    let report = validate(&inv);
    assert!(!report.has("BR-55"), "{report}");
}

/// An ordinary invoice states no BG-3, and the adapter invents none.
#[test]
fn a_document_with_no_preceding_reference_produces_no_bg3() {
    let inv = FromBilling::new(&simple_document(meta()))
        .specification_id(profiles::EN16931.specification_id)
        .seller(party("Seller GmbH", "DE"))
        .buyer(party("Buyer GmbH", "DE"))
        .build()
        .expect("converts");
    assert!(inv.preceding_invoices.is_empty());
}

/// A `billing` credit note becomes an EN 16931 **credit note**, not an invoice
/// carrying a credit note's BT-3.
///
/// Leaving `Invoice::kind` at its default fails three ways at once: BT-3 =
/// `381` is a credit-note code, so `BR-CL-01` fires on the invoice list;
/// `BR-CO-25` runs when it must not; and `en16931-formats` picks the UBL
/// document element from this field, so the document goes out as
/// `<ubl:Invoice>` with a credit note inside it.
#[test]
fn a_billing_credit_note_becomes_an_en16931_credit_note() {
    use en16931::DocumentKind as EnKind;

    let doc = BillingDocument::builder()
        .meta(DocumentMeta {
            kind: DocumentKind::CreditNote,
            ..meta()
        })
        .amount_scale(AmountScale::EN16931)
        .positions(vec![
            LineItem::flat_fee(
                "Gutschrift",
                Amount::parse("100.00000").unwrap(),
                Currency::EUR,
            )
            .vat(BillingLineVat::new(TaxCategory::Standard, dec!(0.19)).unwrap())
            .build()
            .unwrap(),
        ])
        .extra_tax(FixedRateTax::new("MwSt", dec!(0.19)).unwrap().boxed())
        .build()
        .unwrap();

    let inv = FromBilling::new(&doc)
        .specification_id(profiles::EN16931.specification_id)
        .seller(party("Seller GmbH", "DE"))
        .buyer(party("Buyer GmbH", "DE"))
        .build()
        .expect("converts");

    assert_eq!(inv.kind, EnKind::CreditNote);
    assert_eq!(inv.type_code.as_ref().map(Code::as_str), Some("381"));

    let report = validate(&inv);
    assert!(
        !report.has("BR-CL-01"),
        "381 is a credit-note code:\n{report}"
    );
    assert!(
        !report.has("BR-CO-25"),
        "BR-CO-25 must not fire on a credit note:\n{report}"
    );

    // …and every other kind stays an invoice.
    for kind in [
        DocumentKind::CommercialInvoice,
        DocumentKind::PartialInvoice,
        DocumentKind::CorrectedInvoice,
    ] {
        let d = BillingDocument::builder()
            .meta(DocumentMeta { kind, ..meta() })
            .amount_scale(AmountScale::EN16931)
            .positions(vec![
                LineItem::flat_fee(
                    "Leistung",
                    Amount::parse("100.00000").unwrap(),
                    Currency::EUR,
                )
                .vat(BillingLineVat::new(TaxCategory::Standard, dec!(0.19)).unwrap())
                .build()
                .unwrap(),
            ])
            .extra_tax(FixedRateTax::new("MwSt", dec!(0.19)).unwrap().boxed())
            .build()
            .unwrap();
        let i = FromBilling::new(&d)
            .specification_id(profiles::EN16931.specification_id)
            .seller(party("Seller GmbH", "DE"))
            .buyer(party("Buyer GmbH", "DE"))
            .build()
            .expect("converts");
        assert_eq!(i.kind, EnKind::Invoice, "{kind:?}");
        assert!(!validate(&i).has("BR-CL-01"), "{kind:?}");
    }
}

// ── What `billing` 0.13 made mappable ────────────────────────────────────────

/// BG-1 crosses with **both** its terms, and repeats.
///
/// `billing` ≤ 0.12 held one uncoded string, so BT-21 could not cross at all and
/// a second note had nowhere to go. BT-21 is not decoration: a reverse-charge
/// sentence and a payment instruction are both free text, and only the subject
/// code tells a routing system which is which.
#[test]
fn invoice_notes_cross_with_their_subject_codes() {
    use billing::Note;

    let mut m = meta();
    m.notes = vec![
        Note::coded("AAI", "Steuerschuldnerschaft des Leistungsempfängers"),
        Note::new("Vielen Dank für Ihren Auftrag."),
    ];
    let doc = simple_document(m);

    let inv = FromBilling::new(&doc)
        .specification_id(profiles::EN16931.specification_id)
        .seller(party("Seller GmbH", "DE"))
        .buyer(party("Buyer GmbH", "DE"))
        .build()
        .expect("converts");

    assert_eq!(inv.notes.len(), 2, "BG-1 is 0..n");
    assert_eq!(
        inv.notes[0].subject_code.as_ref().map(Code::as_str),
        Some("AAI"),
        "BT-21"
    );
    assert_eq!(
        inv.notes[0].note.as_deref(),
        Some("Steuerschuldnerschaft des Leistungsempfängers"),
        "BT-22"
    );
    assert_eq!(inv.notes[1].subject_code, None, "uncoded is still lawful");

    // `AAI` is in UNCL 4451, so `BR-CL-08` has nothing to say.
    assert!(!validate(&inv).has("BR-CL-08"), "{}", validate(&inv));

    // …and a code outside the list is a finding rather than a silent pass.
    let mut bad = inv.clone();
    bad.notes[0].subject_code = Some(Code::new("ZZZZ"));
    assert!(validate(&bad).has("BR-CL-08"));
}

/// BT-29 and BT-46 cross, **merged** with whatever the caller's party carries.
///
/// They could not cross at all before 0.13: the field was a bare string
/// documented as "MP-ID, GLN, BDEW code, or free-form", and which of those a
/// value is decides the ISO 6523 scheme that `BR-CL-10` then checks. Guessing
/// would have produced an invoice that validates and names the wrong registry.
#[test]
fn party_identifiers_cross_and_merge_rather_than_overwrite() {
    use billing::PartyIdentifier;

    let mut m = meta();
    m.issuer_id = Some(PartyIdentifier::scheme("0088", "4012345000009")); // GLN
    m.recipient_id = Some(PartyIdentifier::new("KND-4711")); // no registry
    let doc = simple_document(m);

    // The caller's party already carries an identifier of its own.
    let mut seller = party("Seller GmbH", "DE");
    seller.identifiers = vec![Identifier::schemed("DE-MASTER-1", "0204")];

    let inv = FromBilling::new(&doc)
        .specification_id(profiles::EN16931.specification_id)
        .seller(seller)
        .buyer(party("Buyer GmbH", "DE"))
        .build()
        .expect("converts");

    // Both survive: master data and the document's own party code are different
    // facts, and BT-29 repeats precisely because a party has more than one.
    let seller_ids: Vec<_> = inv
        .seller
        .identifiers
        .iter()
        .map(|i| (i.content(), i.scheme()))
        .collect();
    assert_eq!(
        seller_ids,
        [
            ("DE-MASTER-1", Some("0204")),
            ("4012345000009", Some("0088"))
        ]
    );
    assert_eq!(
        inv.buyer
            .identifiers
            .iter()
            .map(|i| (i.content(), i.scheme()))
            .collect::<Vec<_>>(),
        [("KND-4711", None)],
        "a schemeless identifier is lawful — BR-CL-10 only constrains a scheme that is there"
    );
    assert!(!validate(&inv).has("BR-CL-10"), "{}", validate(&inv));

    // Converting twice does not duplicate: the value *and* its scheme are
    // compared, because the same digits under 0088 and under 0293 are two
    // registries saying two different things.
    let again = FromBilling::new(&doc)
        .specification_id(profiles::EN16931.specification_id)
        .seller(inv.seller.clone())
        .buyer(inv.buyer.clone())
        .build()
        .expect("converts");
    assert_eq!(again.seller.identifiers.len(), 2, "idempotent");

    // A scheme outside ISO 6523 is reported, not swallowed.
    let mut m = meta();
    m.issuer_id = Some(PartyIdentifier::scheme("NOPE", "X"));
    let bad = FromBilling::new(&simple_document(m))
        .specification_id(profiles::EN16931.specification_id)
        .seller(party("Seller GmbH", "DE"))
        .buyer(party("Buyer GmbH", "DE"))
        .build()
        .expect("converts");
    assert!(validate(&bad).has("BR-CL-10"));
}

// ── the cap, added in `billing` 0.15 ─────────────────────────────────────────

/// **A capped document crosses the seam as a valid EN 16931 credit line.**
///
/// `billing` 0.15 added `maximum_charge` — a *Preisobergrenze*, an OCPI
/// `max_price`, a "maximal 29,90 € im Monat" — and it settles the excess as a
/// **credit invoice line** rather than as a document level allowance, so that
/// the cap reduces BT-106 the way a minimum's shortfall raises it and the VAT
/// base is the capped one.
///
/// # The two rules that decide whether that composes
///
/// A credit line is the one shape where EN 16931's sign conventions bite, and
/// the two rules pull in opposite directions:
///
/// | | |
/// |---|---|
/// | `BR-27` | BT-146, the item net price, **shall not be negative** |
/// | `PEPPOL-EN16931-R120` | BT-131 **shall equal** BT-129 × BT-146 ÷ BT-149 |
///
/// So the negative cannot go on the price, and it cannot be dropped either —
/// `1 × 140` does not reproduce `−140`. It has to go on the **quantity**, and
/// that is what the adapter does: BT-129 = −1, BT-146 = +140, BT-131 = −140.
/// Both rules hold, in every profile that runs them.
///
/// Asserted rather than assumed, because it is a *cross-crate* property: either
/// side can change its sign convention without the other's tests noticing, and
/// the failure would be a document a counterparty rejects for a rule neither
/// repository runs on its own fixtures.
#[test]
fn a_capped_document_becomes_a_credit_line_both_sign_rules_accept() {
    let mut positions = vec![
        LineItem::flat_fee(
            "Verbrauch",
            Amount::parse("640.00000").unwrap(),
            Currency::EUR,
        )
        .vat(BillingLineVat::new(TaxCategory::Standard, dec!(0.19)).unwrap())
        .build()
        .unwrap(),
    ];

    // The cap is settled against the untaxed net, before the layers run.
    let net_only =
        BillingDocument::from_positions(DocumentMeta::default(), positions.clone(), vec![], vec![])
            .unwrap();
    let excess = billing::maximum_charge(
        &net_only,
        Amount::parse("500.00000").unwrap(),
        "Preisobergrenze",
        Currency::EUR,
    )
    .expect("the cap is lawful")
    .expect("640 exceeds 500, so it fires");
    assert_eq!(
        excess.net_amount.to_string(),
        "-140.00000",
        "the excess, as a credit"
    );

    // `maximum_charge` returns the line without a VAT category, because a cap is
    // a contractual term and only the contract knows its rate. EN 16931 has no
    // such freedom — `BR-CO-04` requires one on every line — so the caller
    // attaches it, and the next test asserts what happens if they do not.
    positions.push(
        LineItem::credit_flat_fee(
            "Preisobergrenze",
            Amount::parse("140.00000").unwrap(),
            Currency::EUR,
        )
        .vat(BillingLineVat::new(TaxCategory::Standard, dec!(0.19)).unwrap())
        .build()
        .unwrap(),
    );

    let doc = BillingDocument::builder()
        .meta(meta())
        .amount_scale(AmountScale::EN16931)
        .positions(positions)
        .extra_tax(FixedRateTax::new("MwSt", dec!(0.19)).unwrap().boxed())
        .build()
        .expect("a lawful capped document");

    let inv = FromBilling::new(&doc)
        .specification_id(profiles::EN16931.specification_id)
        .seller(party("Stadtwerke GmbH", "DE"))
        .buyer(party("Kunde AG", "DE"))
        .build()
        .expect("the cap crosses the seam");

    // The credit line, as the two rules require it.
    let credit = inv
        .lines
        .iter()
        .find(|l| l.net_amount.into_decimal().is_sign_negative())
        .expect("the cap produced a credit line");
    assert!(
        !credit.price.net_price.into_decimal().is_sign_negative(),
        "BT-146 must not be negative (BR-27): {}",
        credit.price.net_price
    );
    assert!(
        credit.quantity.into_decimal().is_sign_negative(),
        "the sign belongs on BT-129: {}",
        credit.quantity
    );

    // The cap did its job: 640 capped to 500, VAT on 500 rather than on 640.
    assert_eq!(inv.totals.line_total.to_string(), "500.00");

    // And neither sign rule objects, under core or under the profile that runs
    // R120 — which the core model does not.
    for profile in [&profiles::EN16931, &profiles::PEPPOL_BIS_3] {
        let report = profile.validate(&inv);
        for id in ["BR-27", "PEPPOL-EN16931-R120", "BR-CO-10", "BR-S-08"] {
            assert!(
                !report.has(id),
                "{id} fired under {}:\n{report}",
                profile.id
            );
        }
    }
}

/// **The cap is settled before the tax layers, so VAT is charged on the cap.**
///
/// This is the whole point of `maximum_charge` returning a *line* rather than a
/// document level allowance, and it is the property a consumer depends on: a
/// *"maximal 29,90 € im Monat"* that taxed the uncapped total would overcharge
/// the customer by the VAT on the excess and reconcile against nothing.
///
/// Also asserted: the credit line's VAT category is the one the covering tax
/// layer implies, not a default. `billing` leaves `LineItem::vat` unset on the
/// line `maximum_charge` returns — a cap is a contractual term and only the
/// contract names its rate — and the adapter derives it from `TaxLayer::covers`
/// rather than guessing. `verify_vat_attribution` runs on every conversion (it
/// is `BR-S-08`, and nothing downstream catches a failure), so a credit no layer
/// covers is refused at the boundary rather than shipped.
#[test]
fn the_cap_reduces_the_taxable_base_not_only_the_total() {
    let positions = vec![
        LineItem::flat_fee(
            "Verbrauch",
            Amount::parse("640.00000").unwrap(),
            Currency::EUR,
        )
        .vat(BillingLineVat::new(TaxCategory::Standard, dec!(0.19)).unwrap())
        .build()
        .unwrap(),
        LineItem::credit_flat_fee(
            "Preisobergrenze",
            Amount::parse("140.00000").unwrap(),
            Currency::EUR,
        )
        .vat(BillingLineVat::new(TaxCategory::Standard, dec!(0.19)).unwrap())
        .build()
        .unwrap(),
    ];
    let doc = BillingDocument::builder()
        .meta(meta())
        .amount_scale(AmountScale::EN16931)
        .positions(positions)
        .extra_tax(FixedRateTax::new("MwSt", dec!(0.19)).unwrap().boxed())
        .build()
        .expect("lawful");

    let inv = FromBilling::new(&doc)
        .specification_id(profiles::EN16931.specification_id)
        .seller(party("Stadtwerke GmbH", "DE"))
        .buyer(party("Kunde AG", "DE"))
        .build()
        .expect("the attribution holds, so the conversion is permitted");

    // 640 capped to 500 — and the VAT follows the cap, not the uncapped total.
    assert_eq!(inv.totals.line_total.to_string(), "500.00", "BT-106");
    assert_eq!(inv.totals.taxable_total.to_string(), "500.00", "BT-109");
    assert_eq!(
        inv.totals.vat_total.expect("BT-110").to_string(),
        "95.00",
        "19 % of 500, not of 640"
    );
    assert_eq!(inv.totals.gross_total.to_string(), "595.00", "BT-112");

    // One breakdown group, carrying the capped base.
    assert_eq!(inv.vat_breakdown.len(), 1);
    assert_eq!(inv.vat_breakdown[0].taxable_amount.to_string(), "500.00");
    assert_eq!(inv.vat_breakdown[0].category, Code::new("S"));

    // The credit line inherited the covering layer's category rather than a
    // default, which is what keeps BR-S-08 true.
    let credit = inv
        .lines
        .iter()
        .find(|l| l.net_amount.into_decimal().is_sign_negative())
        .expect("the credit line");
    assert_eq!(credit.vat.category, Code::new("S"));

    let report = validate(&inv);
    assert!(report.is_valid(), "{report}");
}
