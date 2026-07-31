//! Write one invoice as UBL **and** as CII, from a typed proof.
//!
//! ```sh
//! cargo run --example write_both_syntaxes --features cii
//! ```
//!
//! Three things worth watching:
//!
//! * **`write_validated` demands a `Validated<P>`.** An unvalidated invoice
//!   cannot be serialised, and BT-24 is stamped from the profile that was
//!   actually proved — so a document claiming XRechnung 3.0 that was only
//!   checked against the core model is unrepresentable.
//! * **Nothing is dropped silently.** `Written::dropped` names every term the
//!   target syntax had no place for. UBL's `<CreditNote>` has no `cbc:DueDate`;
//!   CII's one document element does.
//! * **Element order is not the writer's problem.** It emits in whatever order
//!   reads best and the serialiser sorts by tables derived from 490 published
//!   instances.

use en16931::invoice::*;
use en16931::profiles::En16931;
use en16931::validation::profile::Validated;
use en16931::{Date, Invoice, InvoiceAmount, Percentage, Quantity, UnitPriceAmount};
use en16931_formats::ubl;
use rust_decimal::dec;

fn amount(s: &str) -> InvoiceAmount {
    InvoiceAmount::parse(s).expect("a two-decimal amount")
}

fn build() -> Invoice {
    Invoice::builder(
        "urn:cen.eu:en16931:2017",
        "RE-2026-0042",
        Date::new(2026, 1, 15).expect("a real date"),
        Code::new("380"),
        Code::new("EUR"),
    )
    .due_date(Date::new(2026, 2, 15).expect("a real date"))
    .seller(party("Seller GmbH", Some("DE123456789")))
    .buyer(party("Buyer AG", None))
    .line(InvoiceLine {
        id: "1".into(),
        quantity: Quantity::new(dec!(2)),
        unit_code: Code::new("C62"),
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
    .build()
}

fn party(name: &str, vat: Option<&str>) -> Party {
    Party {
        name: Some(name.to_owned()),
        vat_identifier: vat.map(str::to_owned),
        address: PostalAddress {
            city: Some("München".into()),
            country: Some(Code::new("DE")),
            ..PostalAddress::default()
        },
        ..Party::default()
    }
}

fn main() {
    let invoice = build();

    // No proof, no document. This is a compile-time guarantee, not a habit.
    let proof: Validated<En16931> = match Validated::new(invoice.clone()) {
        Ok(p) => p,
        Err(rejected) => {
            let (_, report) = *rejected;
            eprintln!("not valid, so nothing is written:");
            for f in report.findings() {
                eprintln!("  {} — {}", f.rule, f.path);
            }
            return;
        }
    };

    let out = ubl::write_validated(&proof);
    println!("── UBL ({} bytes) ──", out.xml.len());
    for line in out.xml.lines().take(12) {
        println!("{line}");
    }
    println!("  …\ndropped: {:?}\n", out.dropped);

    #[cfg(feature = "cii")]
    {
        use en16931_formats::cii;
        let out = cii::write_validated(&proof);
        println!("── CII ({} bytes) ──", out.xml.len());
        for line in out.xml.lines().take(12) {
            println!("{line}");
        }
        println!("  …\ndropped: {:?}\n", out.dropped);

        // The two bindings are independent mappings of one model, so a term
        // mapped correctly in one and wrongly in the other round-trips fine on
        // its own and only shows up here.
        let via_ubl = ubl::from_str(&ubl::to_string(&invoice))
            .expect("ubl")
            .invoice;
        let via_cii = cii::from_str(&cii::to_string(&invoice))
            .expect("cii")
            .invoice;
        println!(
            "UBL and CII agree about the invoice: {}",
            via_ubl == via_cii
        );
    }

    // A credit note is where the two syntaxes visibly differ.
    let mut credit_note = invoice;
    credit_note.kind = en16931::DocumentKind::CreditNote;
    credit_note.type_code = Some(Code::new("381"));
    let out = ubl::write(&credit_note);
    println!("\ncredit note as UBL — dropped: {:?}", out.dropped);
    println!("  (UBL's <CreditNote> has no cbc:DueDate, so BT-9 has nowhere to go —");
    println!("   reported rather than discarded quietly)");
}
