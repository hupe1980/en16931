//! Build an invoice from **lines alone** and let the crate derive the rest.
//!
//! ```sh
//! cargo run --example build_and_reconcile
//! ```
//!
//! The counterpart to `validate_an_invoice`, which states every total by hand.
//! Here the engine has produced positions — an amount, a VAT category, a rate —
//! and nothing else. BG-23 and BG-22 are a *function* of those, and this shows
//! what computing that function does and does not include.
//!
//! Three things worth watching for:
//!
//! * **The grouping.** Two rates in category `S` become two BG-23 entries; the
//!   reverse-charge line becomes a third, and the two `AE` positions at
//!   `Some(0)` and `None` stay **one** group because `BR-AE-01` says exactly one.
//! * **The rounding.** VAT is computed once on the group's taxable amount, not
//!   per line and summed — which is where three `0.05` lines come out a cent
//!   wrong and `BR-S-09` notices.
//! * **What it refuses to invent.** `BR-AE-10` needs an exemption reason, and no
//!   arithmetic can produce one. It is configured, not guessed.

use en16931::codes::guard;
use en16931::invoice::{Party, PartyRole, PostalAddress};
use en16931::reconcile::Reconciler;
use en16931::{
    Date, Identifier, Invoice, InvoiceAmount, InvoiceLine, Percentage, Quantity, profiles, validate,
};
use rust_decimal::dec;

fn amount(s: &str) -> InvoiceAmount {
    InvoiceAmount::parse(s).expect("a two-decimal amount")
}

fn party(name: &str, eas: &str, address: &str, city: &str, post_code: &str) -> Party {
    Party {
        name: Some(name.into()),
        // The scheme is checked here, at the map, rather than reported later
        // against an assembled document.
        electronic_address: Some(
            Identifier::eas(address, eas).unwrap_or_else(|e| panic!("{name}: {e}")),
        ),
        address: PostalAddress {
            city: Some(city.into()),
            post_code: Some(post_code.into()),
            country: Some(guard::country("DE").expect("DE is a country")),
            ..PostalAddress::default()
        },
        ..Party::default()
    }
}

fn main() {
    // ── What a profile will ask for, before anything is fetched ──────────────
    //
    // A pre-flight, not a verdict: this is answerable on an empty party, so the
    // fields can be fetched from a contract service in one round trip instead
    // of a build-validate-fetch loop.
    println!("XRechnung wants these of a buyer, before we build anything:");
    for gap in Party::default().missing_for(&profiles::XRECHNUNG, PartyRole::Buyer) {
        println!("  {gap}");
    }

    // ── The lines, which is all the engine produced ──────────────────────────
    let mut seller = party(
        "Stadtwerke Musterstadt GmbH",
        "0088", // GLN. `9958` — the Leitweg scheme — was withdrawn in 2023.
        "4012345000009",
        "Musterstadt",
        "12345",
    );
    seller.vat_identifier = Some("DE123456789".into());
    let mut buyer = party(
        "Beispiel AG",
        "0204", // DE:LWID, the successor to 9958
        "991-01234-56",
        "Beispielstadt",
        "54321",
    );
    buyer.vat_identifier = Some("DE987654321".into());

    let invoice = Invoice::builder(
        "urn:cen.eu:en16931:2017",
        "RE-2026-0042",
        Date::new(2026, 7, 31).expect("a real date"),
        "380",
        "EUR",
    )
    .seller(seller)
    .buyer(buyer)
    // BR-CO-25 wants a due date **or** payment terms once something is owed.
    // Both are offered; neither is computed from the amount.
    .due_in_days(14)
    .payment_terms("Zahlbar innerhalb 14 Tagen ohne Abzug")
    .line(InvoiceLine::new(
        "1",
        "Netznutzung Arbeitspreis",
        Quantity::new(dec!(10000)),
        "KWH",
        amount("2890.00"),
        "S",
        Some(Percentage::new(dec!(19))),
    ))
    .line(InvoiceLine::new(
        "2",
        "Messstellenbetrieb",
        Quantity::new(dec!(12)),
        "MON",
        amount("120.00"),
        "S",
        Some(Percentage::new(dec!(7))), // a second rate → a second BG-23 group
    ))
    .line(InvoiceLine::new(
        "3",
        "Bauleistung (§ 13b UStG)",
        Quantity::new(dec!(1)),
        "C62",
        amount("500.00"),
        "AE",
        Some(Percentage::ZERO),
    ))
    .line(InvoiceLine::new(
        "4",
        "Bauleistung, zweite Position",
        Quantity::new(dec!(1)),
        "C62",
        amount("250.00"),
        "AE",
        // `BR-AE-05` says the line rate "shall be 0" — absent is NOT zero here,
        // and a rate of `None` would be reported. Both `AE` lines still land in
        // ONE BG-23 group, because `BR-AE-01` says exactly one and the grouping
        // therefore ignores the rate for this category. That matters most on a
        // *parsed* document: one carrying `None` gets a `BR-AE-05` finding about
        // the line, and not a spurious `BR-AE-01` about a group that split.
        Some(Percentage::ZERO),
    ))
    .build_reconciled_with(
        // The one thing the arithmetic cannot supply.
        &Reconciler::new().exemption("AE", None, Some("VATEX-EU-AE")),
    )
    .expect("the lines reconcile");

    // ── What came out ────────────────────────────────────────────────────────
    println!("\nBG-23, derived from the lines:");
    for e in &invoice.vat_breakdown {
        println!(
            "  {:<3} @ {:>5}  base {:>9}  tax {:>8}{}",
            e.category.as_str(),
            e.rate.map_or_else(|| "—".into(), |r| format!("{r} %")),
            e.taxable_amount,
            e.tax_amount,
            e.exemption_reason_code
                .as_ref()
                .map_or(String::new(), |c| format!("   ({c})")),
        );
    }

    let t = &invoice.totals;
    println!("\nBG-22:");
    println!("  BT-106 line total        {:>10}", t.line_total);
    println!("  BT-109 total without VAT {:>10}", t.taxable_total);
    println!(
        "  BT-110 total VAT         {:>10}",
        t.vat_total.expect("stated whenever there is a breakdown")
    );
    println!("  BT-112 total with VAT    {:>10}", t.gross_total);
    println!("  BT-115 amount due        {:>10}", t.due);
    println!(
        "  BT-107 / BT-108          {} / {}   (absent is not zero)",
        t.allowance_total.map_or("absent".into(), |v| v.to_string()),
        t.charge_total.map_or("absent".into(), |v| v.to_string()),
    );

    // ── And the verdict, which is still a separate question ──────────────────
    let report = validate(&invoice);
    println!("\n{report}");
    assert!(report.is_valid(), "a reconciled invoice still gets checked");

    // The profile is a stricter question, and its findings are the ones a
    // pre-flight predicted.
    let xr = profiles::XRECHNUNG.validate(&invoice);
    println!(
        "\nAgainst XRechnung 3.0: {} rule(s) checked, {} still owed",
        xr.rules_checked(),
        xr.fatal().count()
    );
    for f in xr.fatal().take(5) {
        println!("  {f}");
    }
}
