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
            LineItem::flat_fee("Grundpreis", Amount::parse("8.50000").unwrap())
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
            LineItem::flat_fee("Service", Amount::parse("100.00000").unwrap())
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
            LineItem::flat_fee("Jahresverbrauch", Amount::parse("1000.00000").unwrap())
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
