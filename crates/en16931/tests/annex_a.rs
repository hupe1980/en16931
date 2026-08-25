//! EN 16931-1 Annex A, as fixtures.
//!
//! The standard ships eight fully worked invoices with **every intermediate
//! value stated**. They are small, normative-adjacent, and cover precisely the
//! cases implementers get wrong.
//!
//! They test something the per-rule corpus cannot: whether the model can
//! *represent* what the standard describes, before any rule fires. That
//! distinction is what found the BG-29 gap in the upstream calculation crate —
//! no rule failed; the fixture simply could not be written.
//!
//! # A caution about using Annex A as an oracle
//!
//! A.1.5, A.1.6 and A.1.7 all print the BT-115 remark as *"Invoice total **VAT**
//! amount − Paid amount"*. BR-CO-16 says BT-112, and the examples' own
//! arithmetic uses BT-112 — `137,50 − 250,00 = −112,50`. The informative prose
//! disagrees with the normative rule; the rule wins. A crate that encoded Annex
//! A mechanically would inherit the error, so [`a_1_7`] asserts against BR-CO-16.

use en16931::invoice::*;
use en16931::validation::validate;
use en16931::*;
use rust_decimal::dec;

fn amount(s: &str) -> InvoiceAmount {
    InvoiceAmount::parse(s).unwrap_or_else(|e| panic!("{s}: {e}"))
}

fn pct(v: i64) -> Percentage {
    Percentage::new(rust_decimal::Decimal::from(v))
}

/// A party complete enough to satisfy BR-06/07/08/09/10/11.
fn party(name: &str, country: &str) -> Party {
    Party {
        name: Some(name.to_owned()),
        address: PostalAddress {
            country: Some(Code::new(country)),
            ..Default::default()
        },
        electronic_address: Some(Identifier::schemed(name, "0088")),
        // BR-CO-26: the buyer must be able to identify the supplier
        // automatically, so at least one of BT-29 / BT-30 / BT-31 is required.
        // BR-CO-09 then constrains the prefix to ISO 3166-1 (plus `EL`).
        vat_identifier: Some(format!("{country}123456789")),
        ..Default::default()
    }
}

/// A line, stating every mandatory BG-25 term.
fn line(
    id: &str,
    qty: rust_decimal::Decimal,
    price: &str,
    net: &str,
    cat: &str,
    rate: Option<Percentage>,
) -> InvoiceLine {
    InvoiceLine {
        id: id.to_owned(),
        note: None,
        order_line_reference: None,
        accounting_reference: None,
        object_identifier: None,
        quantity: Quantity::new(qty),
        unit_code: Code::new("C62"),
        net_amount: amount(net),
        period: None,
        allowances: vec![],
        charges: vec![],
        price: PriceDetails {
            net_price: UnitPriceAmount::new(price.parse().unwrap()),
            price_discount: None,
            gross_price: None,
            base_quantity: None,
            base_quantity_code: None,
        },
        vat: LineVat {
            category: Code::new(cat),
            rate,
        },
        item: Item {
            name: Some(format!("Item {id}")),
            ..Default::default()
        },
    }
}

/// A shell with everything mandatory filled, for a test to override.
///
/// Uses the builder, which is the supported way in from outside the crate —
/// [`Invoice`] is `#[non_exhaustive]` so that the terms EN 16931-1:2026 adds are
/// additive rather than breaking.
fn shell(lines: Vec<InvoiceLine>, breakdown: Vec<VatBreakdown>, totals: DocumentTotals) -> Invoice {
    let mut b = Invoice::builder(
        "urn:cen.eu:en16931:2017",
        "TEST-1",
        Date::parse("2026-06-30").unwrap(),
        Code::new("380"),
        Code::new("EUR"),
    )
    .seller(party("Seller GmbH", "DE"))
    .buyer(party("Buyer BV", "NL"))
    .buyer_reference("REF-1")
    .due_date(Date::parse("2026-07-30").unwrap());
    for l in lines {
        b = b.line(l);
    }
    for e in breakdown {
        b = b.vat_breakdown(e);
    }
    b.totals(totals).build()
}

/// **Annex A.1.6 — Example 5, Negative Invoice line.**
///
/// 25 cases of pens ordered at 8,50; 10 returned from an earlier wrong delivery
/// are credited. Both lines are standard rated at 25 %, on **one ordinary
/// invoice** — not a credit note.
///
/// The sign lives on the quantity (BT-129 = −10), never on the price: BR-27
/// forbids a negative BT-146.
#[test]
fn a_1_6_negative_invoice_line() {
    let inv = shell(
        vec![
            line("1", dec!(25), "8.50", "212.50", "S", Some(pct(25))),
            line("2", dec!(-10), "8.50", "-85.00", "S", Some(pct(25))),
        ],
        vec![VatBreakdown {
            taxable_amount: amount("127.50"),
            tax_amount: amount("31.88"),
            category: Code::new("S"),
            rate: Some(pct(25)),
            exemption_reason: None,
            exemption_reason_code: None,
        }],
        DocumentTotals {
            line_total: amount("127.50"),
            allowance_total: None,
            charge_total: None,
            taxable_total: amount("127.50"),
            vat_total: Some(amount("31.88")),
            vat_total_accounting: None,
            gross_total: amount("159.38"),
            paid: None,
            rounding: None,
            due: amount("159.38"),
        },
    );

    // The model holds it, and every mandatory term is stated.
    assert!(inv.lines[1].quantity.is_negative());
    assert!(!inv.lines[1].price.net_price.is_negative(), "BR-27");

    let report = validate(&inv);
    assert!(report.is_valid(), "{report}");

    // BT-117 is 31,88 while the exact product is 31,875 — which is why BR-CO-17
    // says "rounded to two decimals" and the artefact then allows ±1.00.
    assert!(!report.has("BR-CO-17"));
}

/// **Annex A.1.7 — Example 6, Prepayment and negative Amount due for payment.**
///
/// The last rate of a car rental, 110,00 at 25 %, against a 250,00 deposit. The
/// amount due is **−112,50**: a refund is a lawful invoice.
#[test]
fn a_1_7_prepayment_and_negative_amount_due() {
    let inv = shell(
        vec![line("1", dec!(1), "110.00", "110.00", "S", Some(pct(25)))],
        vec![VatBreakdown {
            taxable_amount: amount("110.00"),
            tax_amount: amount("27.50"),
            category: Code::new("S"),
            rate: Some(pct(25)),
            exemption_reason: None,
            exemption_reason_code: None,
        }],
        DocumentTotals {
            line_total: amount("110.00"),
            allowance_total: None,
            charge_total: None,
            taxable_total: amount("110.00"),
            vat_total: Some(amount("27.50")),
            vat_total_accounting: None,
            gross_total: amount("137.50"),
            paid: Some(amount("250.00")),
            rounding: None,
            due: amount("-112.50"),
        },
    );

    let report = validate(&inv);
    assert!(report.is_valid(), "{report}");
    assert!(inv.totals.due.is_negative());

    // BR-CO-16 uses BT-112, not BT-110. The annex's remark column says "VAT
    // amount"; if this crate had followed the prose, the expected value would be
    // 27.50 − 250.00 = −222.50 and this fixture would fail.
    let by_the_prose = amount("27.50").checked_sub(amount("250.00")).unwrap();
    assert_ne!(
        inv.totals.due, by_the_prose,
        "the annex remark is an erratum"
    );
}

/// **Annex A.1.8 — Example 7, Standard VAT including VAT exempted lines.**
///
/// Two standard rates (10 % and 25 %) alongside exempt lines carrying a reason.
/// Three BG-23 groups on one invoice.
#[test]
fn a_1_8_mixed_rates_with_exempt_lines() {
    let inv = shell(
        vec![
            line("1", dec!(1), "125.00", "125.00", "S", Some(pct(25))),
            line("2", dec!(1), "24.00", "24.00", "S", Some(pct(10))),
            line("3", dec!(1), "136.00", "136.00", "S", Some(pct(25))),
            line("4", dec!(1), "95.00", "95.00", "E", Some(Percentage::ZERO)),
            line("5", dec!(1), "53.00", "53.00", "E", Some(Percentage::ZERO)),
        ],
        vec![
            VatBreakdown {
                taxable_amount: amount("261.00"), // 125 + 136
                tax_amount: amount("65.25"),
                category: Code::new("S"),
                rate: Some(pct(25)),
                exemption_reason: None,
                exemption_reason_code: None,
            },
            VatBreakdown {
                taxable_amount: amount("24.00"),
                tax_amount: amount("2.40"),
                category: Code::new("S"),
                rate: Some(pct(10)),
                exemption_reason: None,
                exemption_reason_code: None,
            },
            VatBreakdown {
                taxable_amount: amount("148.00"), // 95 + 53
                tax_amount: amount("0.00"),
                category: Code::new("E"),
                rate: Some(Percentage::ZERO),
                // BR-E-10: exempt REQUIRES a reason. "Reason A" in the annex.
                exemption_reason: Some("Reason A".to_owned()),
                exemption_reason_code: None,
            },
        ],
        DocumentTotals {
            line_total: amount("433.00"),
            allowance_total: None,
            charge_total: None,
            taxable_total: amount("433.00"),
            vat_total: Some(amount("67.65")),
            vat_total_accounting: None,
            gross_total: amount("500.65"),
            paid: None,
            rounding: None,
            due: amount("500.65"),
        },
    );

    let report = validate(&inv);
    assert!(report.is_valid(), "{report}");
    assert_eq!(inv.vat_breakdown.len(), 3);
}

/// Dropping the exempt group's reason must fire BR-E-10 — and nothing else.
#[test]
fn an_exempt_group_without_a_reason_is_reported_precisely() {
    let mut inv = shell(
        vec![line(
            "1",
            dec!(1),
            "95.00",
            "95.00",
            "E",
            Some(Percentage::ZERO),
        )],
        vec![VatBreakdown {
            taxable_amount: amount("95.00"),
            tax_amount: amount("0.00"),
            category: Code::new("E"),
            rate: Some(Percentage::ZERO),
            exemption_reason: None,
            exemption_reason_code: None,
        }],
        DocumentTotals {
            line_total: amount("95.00"),
            allowance_total: None,
            charge_total: None,
            taxable_total: amount("95.00"),
            vat_total: Some(amount("0.00")),
            vat_total_accounting: None,
            gross_total: amount("95.00"),
            paid: None,
            rounding: None,
            due: amount("95.00"),
        },
    );

    let report = validate(&inv);
    assert!(report.has("BR-E-10"), "{report}");
    assert_eq!(
        report.fatal().count(),
        1,
        "one problem, one finding: {report}"
    );
    assert_eq!(
        report.fatal().next().unwrap().path.to_string(),
        "BG-23[0]/BT-120",
        "the path names the group and the term, not an XPath"
    );

    // Either form satisfies it — the code, not only prose.
    inv.vat_breakdown[0].exemption_reason_code = Some(Code::new("VATEX-EU-132-1A"));
    assert!(validate(&inv).is_valid());
}

/// The totals chain is **exact**; the VAT derivation carries ±1.00. Mixing them
/// up in either direction is the single easiest way to disagree with every real
/// validator, so both directions are pinned.
#[test]
fn the_two_cen_tolerance_regimes_are_not_interchangeable() {
    let totals = |vat: &str, gross: &str| DocumentTotals {
        line_total: amount("100.00"),
        allowance_total: None,
        charge_total: None,
        taxable_total: amount("100.00"),
        vat_total: Some(amount(vat)),
        vat_total_accounting: None,
        gross_total: amount(gross),
        paid: None,
        rounding: None,
        due: amount(gross),
    };
    let breakdown = |tax: &str| {
        vec![VatBreakdown {
            taxable_amount: amount("100.00"),
            tax_amount: amount(tax),
            category: Code::new("S"),
            rate: Some(pct(19)),
            exemption_reason: None,
            exemption_reason_code: None,
        }]
    };
    let one_line = || vec![line("1", dec!(1), "100.00", "100.00", "S", Some(pct(19)))];

    // Exact: BT-110 = Σ BT-117. One cent out is fatal.
    let inv = shell(one_line(), breakdown("19.00"), totals("19.01", "119.01"));
    assert!(validate(&inv).has("BR-CO-14"), "BR-CO-14 must be exact");

    // Tolerant: BT-117 vs BT-116 × rate. 0.50 out is inside ±1.00 and passes,
    // which is what every validator does.
    let inv = shell(one_line(), breakdown("18.50"), totals("18.50", "118.50"));
    let report = validate(&inv);
    assert!(!report.has("BR-CO-17"), "±1.00 tolerance: {report}");
    assert!(report.is_valid(), "{report}");

    // …but 1.50 out is beyond it.
    let inv = shell(one_line(), breakdown("17.50"), totals("17.50", "117.50"));
    assert!(validate(&inv).has("BR-CO-17"));
}

/// A VAT rate of exactly half a per cent is a rate, not a zero rate.
///
/// The artefact picks its zero-rate branch on `round(Percent) = 0`, and that
/// `round` is **XPath's**: ties go towards +∞, so `round(0.5)` is `1`. This
/// crate used `Decimal::round`, which is banker's, so `round(0.5)` was `0` — it
/// took the zero-rate branch, demanded a tax amount of nothing, and rejected an
/// invoice every deployed validator accepts. Spain's *recargo de equivalencia*
/// on reduced-rate goods is 0.5 %, so the rate is ordinary rather than exotic.
#[test]
fn br_co_17_accepts_a_rate_of_half_a_per_cent() {
    let breakdown = |taxable: &str, tax: &str, rate: Percentage| {
        vec![VatBreakdown {
            taxable_amount: amount(taxable),
            tax_amount: amount(tax),
            category: Code::new("S"),
            rate: Some(rate),
            exemption_reason: None,
            exemption_reason_code: None,
        }]
    };
    let half = Percentage::new(dec!(0.5));
    let inv = shell(
        vec![line("1", dec!(1), "1000.00", "1000.00", "S", Some(half))],
        breakdown("1000.00", "5.00", half),
        DocumentTotals {
            line_total: amount("1000.00"),
            allowance_total: None,
            charge_total: None,
            taxable_total: amount("1000.00"),
            vat_total: Some(amount("5.00")),
            vat_total_accounting: None,
            gross_total: amount("1005.00"),
            paid: None,
            rounding: None,
            due: amount("1005.00"),
        },
    );
    let report = validate(&inv);
    assert!(!report.has("BR-CO-17"), "{report}");
    assert!(!report.has("BR-S-09"), "{report}");
    assert!(report.is_valid(), "{report}");

    // …and a rate that really does round to zero still takes the zero branch.
    let quarter = Percentage::new(dec!(0.4));
    let inv = shell(
        vec![line("1", dec!(1), "1000.00", "1000.00", "S", Some(quarter))],
        breakdown("1000.00", "4.00", quarter),
        DocumentTotals {
            line_total: amount("1000.00"),
            allowance_total: None,
            charge_total: None,
            taxable_total: amount("1000.00"),
            vat_total: Some(amount("4.00")),
            vat_total_accounting: None,
            gross_total: amount("1004.00"),
            paid: None,
            rounding: None,
            due: amount("1004.00"),
        },
    );
    assert!(validate(&inv).has("BR-CO-17"), "round(0.4) is 0");
}

/// `abs()` on both sides of BR-CO-17 is what lets a credit note pass. Without
/// it, every negative breakdown would be reported.
#[test]
fn br_co_17_compares_absolute_values_so_credits_pass() {
    let inv = shell(
        vec![line("1", dec!(-1), "100.00", "-100.00", "S", Some(pct(19)))],
        vec![VatBreakdown {
            taxable_amount: amount("-100.00"),
            tax_amount: amount("-19.00"),
            category: Code::new("S"),
            rate: Some(pct(19)),
            exemption_reason: None,
            exemption_reason_code: None,
        }],
        DocumentTotals {
            line_total: amount("-100.00"),
            allowance_total: None,
            charge_total: None,
            taxable_total: amount("-100.00"),
            vat_total: Some(amount("-19.00")),
            vat_total_accounting: None,
            gross_total: amount("-119.00"),
            paid: None,
            rounding: None,
            due: amount("-119.00"),
        },
    );
    let report = validate(&inv);
    assert!(!report.has("BR-CO-17"), "{report}");
}

/// A document that was never configured must not validate just because `XXX` is
/// a real ISO 4217 code. `BR-CL-04` accepts it; `EN-CURRENCY-01` does not.
#[test]
fn an_unconfigured_currency_is_caught_even_though_iso_allows_it() {
    let mut inv = shell(
        vec![line("1", dec!(1), "100.00", "100.00", "S", Some(pct(19)))],
        vec![VatBreakdown {
            taxable_amount: amount("100.00"),
            tax_amount: amount("19.00"),
            category: Code::new("S"),
            rate: Some(pct(19)),
            exemption_reason: None,
            exemption_reason_code: None,
        }],
        DocumentTotals {
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
        },
    );
    inv.currency = Some(Code::new("XXX"));

    let report = validate(&inv);
    assert!(
        !report.has("BR-CL-04"),
        "XXX is in ISO 4217, so CEN accepts it"
    );
    assert!(report.has("EN-CURRENCY-01"), "but we do not: {report}");
}

/// Findings are ordered stably, so a report is diffable in CI.
#[test]
fn reports_are_deterministic() {
    let inv = shell(
        vec![],
        vec![],
        DocumentTotals {
            line_total: amount("0.00"),
            allowance_total: None,
            charge_total: None,
            taxable_total: amount("0.00"),
            vat_total: None,
            vat_total_accounting: None,
            gross_total: amount("0.00"),
            paid: None,
            rounding: None,
            due: amount("0.00"),
        },
    );
    let a = validate(&inv);
    let b = validate(&inv);
    assert_eq!(a, b);
    assert!(a.has("BR-16"), "no lines");
    assert!(a.has("BR-CO-18"), "no VAT breakdown");
    assert!(!a.is_valid());
}

/// **`BR-S-08` — the keystone.** The only rule tying the invoice lines to the
/// VAT breakdown, and therefore the only thing that makes a mis-attributed line
/// a *reported* error rather than a silently wrong invoice.
#[test]
fn br_s_08_ties_the_lines_to_the_breakdown() {
    // Two rates: 125 + 136 at 25 %, 24 at 10 %.
    let lines = || {
        vec![
            line("1", dec!(1), "125.00", "125.00", "S", Some(pct(25))),
            line("2", dec!(1), "24.00", "24.00", "S", Some(pct(10))),
            line("3", dec!(1), "136.00", "136.00", "S", Some(pct(25))),
        ]
    };
    let breakdown = |base_25: &str| {
        vec![
            VatBreakdown {
                taxable_amount: amount(base_25),
                tax_amount: amount("65.25"),
                category: Code::new("S"),
                rate: Some(pct(25)),
                exemption_reason: None,
                exemption_reason_code: None,
            },
            VatBreakdown {
                taxable_amount: amount("24.00"),
                tax_amount: amount("2.40"),
                category: Code::new("S"),
                rate: Some(pct(10)),
                exemption_reason: None,
                exemption_reason_code: None,
            },
        ]
    };
    let totals = DocumentTotals {
        line_total: amount("285.00"),
        allowance_total: None,
        charge_total: None,
        taxable_total: amount("285.00"),
        vat_total: Some(amount("67.65")),
        vat_total_accounting: None,
        gross_total: amount("352.65"),
        paid: None,
        rounding: None,
        due: amount("352.65"),
    };

    // Correct: the 25 % group's base is 125 + 136, NOT all three lines.
    let ok = shell(lines(), breakdown("261.00"), totals.clone());
    let report = validate(&ok);
    assert!(report.is_valid(), "{report}");

    // Wrong: attributing all three lines to the 25 % group.
    let bad = shell(lines(), breakdown("285.00"), totals);
    let report = validate(&bad);
    assert!(report.has("BR-S-08"), "{report}");
    assert_eq!(
        report.fatal().next().unwrap().path.to_string(),
        "BG-23[0]/BT-116"
    );
}

/// `-08` groups by `(category, rate)` for taxed categories and by category alone
/// for zero-tax ones. Getting that backwards is the classic error.
#[test]
fn zero_tax_categories_group_by_category_alone() {
    // Two exempt lines, one BG-23 group — BR-E-01 says *exactly* one.
    let inv = shell(
        vec![
            line("1", dec!(1), "95.00", "95.00", "E", Some(Percentage::ZERO)),
            line("2", dec!(1), "53.00", "53.00", "E", Some(Percentage::ZERO)),
        ],
        vec![VatBreakdown {
            taxable_amount: amount("148.00"),
            tax_amount: amount("0.00"),
            category: Code::new("E"),
            rate: Some(Percentage::ZERO),
            exemption_reason: Some("Reason A".to_owned()),
            exemption_reason_code: None,
        }],
        DocumentTotals {
            line_total: amount("148.00"),
            allowance_total: None,
            charge_total: None,
            taxable_total: amount("148.00"),
            vat_total: Some(amount("0.00")),
            vat_total_accounting: None,
            gross_total: amount("148.00"),
            paid: None,
            rounding: None,
            due: amount("148.00"),
        },
    );
    let report = validate(&inv);
    assert!(report.is_valid(), "{report}");
    assert!(!report.has("BR-E-01"));
    assert!(!report.has("BR-E-08"));
}

/// `BR-O-05` says the rate *"shall not contain"* — absent, not zero. Every other
/// zero-tax category says *"shall be 0"*. Confusing the two is why
/// `VatCategory::states_rate` exists.
#[test]
fn out_of_scope_forbids_the_rate_rather_than_zeroing_it() {
    let build = |rate: Option<Percentage>| {
        let mut inv = shell(
            vec![line("1", dec!(1), "100.00", "100.00", "O", rate)],
            vec![VatBreakdown {
                taxable_amount: amount("100.00"),
                tax_amount: amount("0.00"),
                category: Code::new("O"),
                // BT-119 is a DIFFERENT term from BT-152: BR-48 makes it
                // optional for O, and this crate does not require it.
                rate: None,
                exemption_reason: Some("Not subject to VAT".to_owned()),
                exemption_reason_code: None,
            }],
            DocumentTotals {
                line_total: amount("100.00"),
                allowance_total: None,
                charge_total: None,
                taxable_total: amount("100.00"),
                vat_total: Some(amount("0.00")),
                vat_total_accounting: None,
                gross_total: amount("100.00"),
                paid: None,
                rounding: None,
                due: amount("100.00"),
            },
        );
        // `O` forbids BT-31 (BR-O-02) while BR-CO-26 requires one of
        // BT-29 / BT-30 / BT-31 — so an out-of-scope invoice must identify its
        // seller by registration rather than by VAT number.
        inv.seller.vat_identifier = None;
        inv.buyer.vat_identifier = None;
        inv.seller.legal_registration = Some(Identifier::schemed("HRB 12345", "0198"));
        inv
    };

    // Absent — correct.
    assert!(
        validate(&build(None)).is_valid(),
        "{}",
        validate(&build(None))
    );
    // Zero — a finding, even though zero satisfies every other zero-tax category.
    assert!(validate(&build(Some(Percentage::ZERO))).has("BR-O-05"));
}

/// `BR-B-01`: split payment is domestic Italian only. A rule that needs both
/// parties' countries, so it could not live on the category enum.
#[test]
fn split_payment_is_italy_only_and_never_beside_standard() {
    let inv = shell(
        vec![line("1", dec!(1), "100.00", "100.00", "B", Some(pct(22)))],
        vec![VatBreakdown {
            taxable_amount: amount("100.00"),
            tax_amount: amount("22.00"),
            category: Code::new("B"),
            rate: Some(pct(22)),
            exemption_reason: None,
            exemption_reason_code: None,
        }],
        DocumentTotals {
            line_total: amount("100.00"),
            allowance_total: None,
            charge_total: None,
            taxable_total: amount("100.00"),
            vat_total: Some(amount("22.00")),
            vat_total_accounting: None,
            gross_total: amount("122.00"),
            paid: None,
            rounding: None,
            due: amount("122.00"),
        },
    );
    // The shell's parties are DE and NL.
    assert!(validate(&inv).has("BR-B-01"));

    // Unlike AE, B carries tax — so a zero tax amount would be the error.
    assert!(VatCategory::SplitPayment.carries_tax());
}

/// The engine reports how much it actually checked, so nobody can mistake
/// partial coverage for conformance.
#[test]
fn the_report_states_its_own_coverage() {
    let report = validate(&Invoice::default());
    assert!(
        report.rules_checked() >= 85,
        "only {} rules",
        report.rules_checked()
    );
}
