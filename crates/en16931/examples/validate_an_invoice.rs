//! Build an invoice, validate it, and read the report.
//!
//! ```sh
//! cargo run --example validate_an_invoice
//! ```
//!
//! The point this example is making: a finding names a **business term**, not a
//! location in a file. `BT-151 on line 2` is actionable by whoever entered the
//! data; `/ubl:Invoice/cac:InvoiceLine[2]/cac:Item/cac:ClassifiedTaxCategory/cbc:ID`
//! is not.

use en16931::invoice::*;
use en16931::{Date, Invoice, InvoiceAmount, Percentage, Quantity, UnitPriceAmount, validate};
use rust_decimal::dec;

fn amount(s: &str) -> InvoiceAmount {
    InvoiceAmount::parse(s).expect("a two-decimal amount")
}

fn main() {
    let invoice = Invoice::builder(
        "urn:cen.eu:en16931:2017",
        "RE-2026-0042",
        Date::new(2026, 1, 15).expect("a real date"),
        Code::new("380"), // commercial invoice
        Code::new("EUR"),
    )
    .seller(Party {
        name: Some("Seller GmbH".into()),
        vat_identifier: Some("DE123456789".into()),
        address: PostalAddress {
            line1: Some("Hauptstraße 1".into()),
            city: Some("München".into()),
            post_code: Some("80331".into()),
            country: Some(Code::new("DE")),
            ..PostalAddress::default()
        },
        ..Party::default()
    })
    .buyer(Party {
        name: Some("Buyer AG".into()),
        address: PostalAddress {
            city: Some("Hamburg".into()),
            country: Some(Code::new("DE")),
            ..PostalAddress::default()
        },
        ..Party::default()
    })
    .line(InvoiceLine {
        id: "1".into(),
        quantity: Quantity::new(dec!(2)),
        unit_code: Code::new("C62"), // "one" / piece
        net_amount: amount("200.00"),
        price: PriceDetails {
            net_price: UnitPriceAmount::new(dec!(100)),
            ..PriceDetails::default()
        },
        vat: LineVat {
            category: Code::new("S"),
            rate: Some(Percentage::from_fraction(dec!(0.19)).expect("19%")),
        },
        item: Item {
            name: Some("Widget".into()),
            ..Item::default()
        },
        note: None,
        order_line_reference: None,
        accounting_reference: None,
        object_identifier: None,
        period: None,
        allowances: vec![],
        charges: vec![],
    })
    .vat_breakdown(VatBreakdown {
        taxable_amount: amount("200.00"),
        tax_amount: amount("38.00"),
        category: Code::new("S"),
        rate: Some(Percentage::from_fraction(dec!(0.19)).expect("19%")),
        exemption_reason: None,
        exemption_reason_code: None,
    })
    .totals(DocumentTotals {
        line_total: amount("200.00"),
        taxable_total: amount("200.00"),
        vat_total: Some(amount("38.00")),
        gross_total: amount("238.00"),
        due: amount("238.00"),
        ..DocumentTotals::default()
    })
    .build();

    report("as built", &invoice);

    // BR-CO-25: a positive amount due needs either a due date or payment terms.
    // Neither was set, so the invoice is not valid — and the finding says which
    // term is missing, not where in a file to look.
    let mut fixed = invoice;
    fixed.due_date = Some(Date::new(2026, 2, 15).expect("a real date"));
    report("with BT-9 supplied", &fixed);
}

fn report(label: &str, invoice: &Invoice) {
    let report = validate(invoice);

    println!("── {label} ──");
    println!("valid:         {}", report.is_valid());
    println!("rules checked: {}", report.rules_checked());
    println!("findings:      {}\n", report.findings().len());

    for f in report.findings() {
        // `f.path` is a business-term path — `BG-25[1]/BT-151` — because this
        // crate validates the model rather than a serialised document.
        println!("  [{}] {} — {}", f.severity, f.rule, f.path);
        println!("      {}", f.message);
        if let Some(d) = &f.detail {
            println!("      expected {}, found {}", d.expected, d.actual);
        }
    }

    if report.is_valid() {
        println!("  (nothing to report)");
    }
    println!();
}
