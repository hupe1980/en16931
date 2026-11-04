//! Rule branches the corpus fixtures do not reach, pinned so a mutant of the
//! rule (a dropped arm, `||` for `&&`, a lost `!`) fails.

use en16931::invoice::{Code, VatBreakdown};
use en16931::{Invoice, InvoiceAmount, Percentage, profiles, validate};

fn base() -> Invoice {
    en16931::examples::for_profile(&profiles::EN16931).expect("a shipped example")
}

fn paths(inv: &Invoice, rule: &str) -> Vec<String> {
    validate(inv)
        .findings()
        .iter()
        .filter(|f| f.rule.as_str() == rule)
        .map(|f| f.path.to_string())
        .collect()
}

fn categorise(inv: &mut Invoice, category: &str) {
    for l in &mut inv.lines {
        l.vat.category = Code::new(category);
        l.vat.rate = Some(Percentage::ZERO);
    }
}

/// Export (`G`) needs the seller's **VAT** identifier (`BR-G-02`): a tax
/// registration (BT-32) is enough for the other categories, not for this one.
#[test]
fn export_needs_the_sellers_vat_identifier_not_a_tax_registration() {
    let mut inv = base();
    categorise(&mut inv, "G");
    inv.seller.vat_identifier = None;
    inv.seller.tax_registration = Some("201/113/40209".to_owned());
    inv.tax_representative = None;
    assert!(!paths(&inv, "BR-G-02").is_empty());
}

/// Split payment is a domestic Italian invoice: **either** party abroad is
/// enough to break `BR-B-01`.
#[test]
fn split_payment_with_one_party_abroad_breaks_br_b_01() {
    let mut inv = base();
    categorise(&mut inv, "B");
    inv.seller.address.country = Some(Code::new("IT"));
    inv.buyer.address.country = Some(Code::new("FR"));
    assert!(!paths(&inv, "BR-B-01").is_empty());
}

/// `BR-O-11` reports the breakdowns that are **not** `O`, at their index.
#[test]
fn br_o_11_points_at_the_other_breakdown() {
    let mut inv = base();
    let other = inv.vat_breakdown[0].clone();
    inv.vat_breakdown = vec![
        VatBreakdown {
            taxable_amount: InvoiceAmount::ZERO,
            tax_amount: InvoiceAmount::ZERO,
            category: Code::new("O"),
            rate: None,
            exemption_reason: Some("Not subject to VAT".to_owned()),
            exemption_reason_code: None,
        },
        other,
    ];
    let at = paths(&inv, "BR-O-11");
    assert_eq!(at.len(), 1, "{at:?}");
    assert!(at[0].starts_with("BG-23[1]"), "{at:?}");
}

/// Two exempt lines whose sum exceeds an `InvoiceAmount`, with every total at
/// zero: the sums are exact, so `BR-CO-10` and `BR-E-08` fail instead of
/// skipping on overflow.
#[test]
fn a_sum_beyond_an_invoice_amount_fails_its_total_rules() {
    let mut inv = base();
    categorise(&mut inv, "E");
    let half = InvoiceAmount::parse("50000000000000000.00").expect("fits an amount");
    inv.lines.truncate(1);
    inv.lines[0].net_amount = half;
    let mut second = inv.lines[0].clone();
    second.id = "2".to_owned();
    inv.lines.push(second);
    inv.allowances.clear();
    inv.charges.clear();
    inv.vat_breakdown = vec![VatBreakdown {
        taxable_amount: InvoiceAmount::ZERO,
        tax_amount: InvoiceAmount::ZERO,
        category: Code::new("E"),
        rate: Some(Percentage::ZERO),
        exemption_reason: Some("Exempt".to_owned()),
        exemption_reason_code: None,
    }];
    inv.totals = en16931::invoice::DocumentTotals::default();
    assert_eq!(
        (paths(&inv, "BR-CO-10").len(), paths(&inv, "BR-E-08").len()),
        (1, 1)
    );
}

/// A rate whose tax exceeds `Decimal`'s range matches no stated tax:
/// `BR-CO-17` and `BR-S-09` fail instead of skipping the breakdown.
#[test]
fn a_tax_beyond_the_decimal_range_fails_its_rules() {
    let mut inv = base();
    let entry = inv
        .vat_breakdown
        .iter_mut()
        .find(|e| e.category.as_str() == "S")
        .expect("the example has a standard rate");
    entry.rate = Some(Percentage::new(rust_decimal::Decimal::MAX));
    assert_eq!(
        (paths(&inv, "BR-CO-17").len(), paths(&inv, "BR-S-09").len()),
        (1, 1)
    );
}
