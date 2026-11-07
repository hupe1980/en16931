//! Invoice generators, shared by `tests/robustness.rs` and the `roundtrip` fuzz
//! target (which includes this file by path).
//!
//! Output is structurally valid but semantically absurd: `i64`-boundary amounts,
//! `Decimal`-boundary quantities, prices and rates (the largest, the smallest
//! step, 28 places), multi-byte and control characters, empty codes.

#![allow(dead_code)]

use en16931::invoice::*;
use en16931::{InvoiceAmount, Percentage, Quantity, UnitPriceAmount};
use proptest::prelude::*;
use rust_decimal::Decimal;

/// Amounts clustered at the `i64` boundaries.
pub fn any_amount() -> impl Strategy<Value = InvoiceAmount> {
    prop_oneof![
        Just(InvoiceAmount::from_minor_units(i64::MAX)),
        Just(InvoiceAmount::from_minor_units(i64::MIN)),
        Just(InvoiceAmount::ZERO),
        any::<i64>().prop_map(InvoiceAmount::from_minor_units),
        (-1_000_000i64..1_000_000).prop_map(InvoiceAmount::from_minor_units),
    ]
}

pub fn any_code() -> impl Strategy<Value = Code> {
    prop_oneof![
        Just(Code::new("")),
        Just(Code::new("S")),
        Just(Code::new("Z")),
        Just(Code::new("O")),
        Just(Code::new("EUR")),
        "[A-Za-z0-9 ]{0,8}".prop_map(Code::new),
        any_text().prop_map(|t| Code::new(&t)),
    ]
}

/// Text as European invoices carry it, plus hostile input: multi-byte
/// characters of every width, combining marks, a right-to-left mark, an
/// astral-plane character, control characters and the empty string.
pub fn any_text() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        Just("Müller".to_owned()),
        Just("Straße".to_owned()),
        Just("BR-Ié".to_owned()),
        // Combining acute: two chars, three bytes, one grapheme.
        Just("e\u{301}".to_owned()),
        Just("\u{200F}".to_owned()),  // right-to-left mark
        Just("\u{1F9FE}".to_owned()), // astral plane — four bytes
        Just("\u{0}".to_owned()),
        Just("\u{7}".to_owned()),
        ".{0,24}",
        // Long, for code that indexes rather than iterates.
        "\\PC{200,240}",
    ]
    .prop_map(|s: String| s)
}

/// A `Decimal` at its edges: the range, the smallest step, and a value with
/// all 28 places, which no product or sum can keep exactly. Normalised, as the
/// writers spell it, so a roundtrip compares like with like.
fn any_decimal(small: std::ops::Range<i64>) -> impl Strategy<Value = Decimal> {
    prop_oneof![
        Just(Decimal::ZERO),
        Just(Decimal::MAX),
        Just(Decimal::MIN),
        Just(Decimal::new(1, 28)),
        Just(Decimal::from_i128_with_scale(
            3_333_333_333_333_333_333_333_333_333,
            28
        )),
        small.prop_map(Decimal::from),
        (any::<i64>(), 0u32..=28).prop_map(|(m, s)| Decimal::new(m, s).normalize()),
    ]
}

pub fn any_percentage() -> impl Strategy<Value = Percentage> {
    any_decimal(-100..1000).prop_map(Percentage::new)
}

pub fn any_quantity() -> impl Strategy<Value = Quantity> {
    any_decimal(-1000..1000).prop_map(Quantity::new)
}

pub fn any_unit_price() -> impl Strategy<Value = UnitPriceAmount> {
    any_decimal(-1000..1000).prop_map(UnitPriceAmount::new)
}

/// A line allowance or charge, with or without the base and percentage
/// `R040` multiplies.
pub fn any_line_allowance_charge() -> impl Strategy<Value = LineAllowanceCharge> {
    (
        any_amount(),
        proptest::option::of(any_amount()),
        proptest::option::of(any_percentage()),
    )
        .prop_map(|(amount, base_amount, percentage)| LineAllowanceCharge {
            amount,
            base_amount,
            percentage,
            reason: None,
            reason_code: None,
        })
}

/// A document-level allowance or charge, as [`any_line_allowance_charge`].
pub fn any_document_allowance_charge() -> impl Strategy<Value = DocumentAllowanceCharge> {
    (any_line_allowance_charge(), any_code(), any_percentage()).prop_map(|(ac, cat, rate)| {
        DocumentAllowanceCharge {
            amount: ac.amount,
            base_amount: ac.base_amount,
            percentage: ac.percentage,
            vat: LineVat {
                category: cat,
                rate: Some(rate),
            },
            reason: None,
            reason_code: None,
        }
    })
}

/// The price terms `R120` and `R046` combine, each independent: a base
/// quantity that may be zero, a gross price and a discount.
pub fn any_price() -> impl Strategy<Value = PriceDetails> {
    (
        any_unit_price(),
        proptest::option::of(any_unit_price()),
        proptest::option::of(any_unit_price()),
        proptest::option::of(any_quantity()),
    )
        .prop_map(
            |(net_price, gross_price, price_discount, base_quantity)| PriceDetails {
                net_price,
                gross_price,
                price_discount,
                base_quantity,
                ..Default::default()
            },
        )
}

pub fn any_line() -> impl Strategy<Value = InvoiceLine> {
    (
        any_amount(),
        any_quantity(),
        any_code(),
        any_code(),
        any_percentage(),
        any::<bool>(),
        any_text(),
        any_price(),
        prop::collection::vec(any_line_allowance_charge(), 0..3),
        prop::collection::vec(any_line_allowance_charge(), 0..3),
    )
        .prop_map(
            |(net, qty, unit, cat, rate, has_rate, text, price, allowances, charges)| InvoiceLine {
                id: text.clone(),
                note: Some(text.clone()),
                order_line_reference: None,
                accounting_reference: None,
                object_identifier: None,
                quantity: qty,
                unit_code: unit,
                net_amount: net,
                period: None,
                allowances,
                charges,
                price,
                vat: LineVat {
                    category: cat,
                    rate: has_rate.then_some(rate),
                },
                item: Item {
                    name: Some(text.clone()),
                    description: Some(text),
                    ..Item::default()
                },
            },
        )
}

pub fn any_breakdown() -> impl Strategy<Value = VatBreakdown> {
    (any_amount(), any_amount(), any_code(), any_percentage()).prop_map(
        |(taxable, tax, cat, rate)| VatBreakdown {
            taxable_amount: taxable,
            tax_amount: tax,
            category: cat,
            rate: Some(rate),
            exemption_reason: None,
            exemption_reason_code: None,
        },
    )
}

prop_compose! {
    /// A document that is structurally valid and semantically arbitrary.
    pub fn any_invoice()(
        lines in prop::collection::vec(any_line(), 0..12),
        breakdown in prop::collection::vec(any_breakdown(), 0..6),
        allowances in prop::collection::vec(any_document_allowance_charge(), 0..3),
        charges in prop::collection::vec(any_document_allowance_charge(), 0..3),
        allowance_total in proptest::option::of(any_amount()),
        charge_total in proptest::option::of(any_amount()),
        paid in proptest::option::of(any_amount()),
        rounding in proptest::option::of(any_amount()),
        line_total in any_amount(),
        taxable in any_amount(),
        vat in proptest::option::of(any_amount()),
        gross in any_amount(),
        due in any_amount(),
        currency in proptest::option::of(any_code()),
        type_code in proptest::option::of(any_code()),
        credit_note in any::<bool>(),
    ) -> Invoice {
        let mut inv = Invoice::default();
        inv.kind = if credit_note { DocumentKind::CreditNote } else { DocumentKind::Invoice };
        inv.currency = currency;
        inv.type_code = type_code;
        inv.lines = lines;
        inv.vat_breakdown = breakdown;
        inv.allowances = allowances;
        inv.charges = charges;
        inv.totals = DocumentTotals {
            line_total,
            allowance_total,
            charge_total,
            paid,
            rounding,
            taxable_total: taxable,
            vat_total: vat,
            gross_total: gross,
            due,
            ..Default::default()
        };
        inv
    }
}
