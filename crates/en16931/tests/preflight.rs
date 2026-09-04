//! **Questions answerable before the document exists.**
//!
//! `validate` answers *"is this document acceptable?"*, which on a half-built
//! invoice is a hundred findings about lines and totals that are not there yet.
//! Two questions have exact answers *earlier* than that, and both are here:
//!
//! | | |
//! |---|---|
//! | `Profile::missing_terms` | which extra fields will this profile ask me for? |
//! | `VatCategory::can_share_document` | may these categories appear on one invoice? |
//!
//! # The property that makes a pre-flight worth having
//!
//! **It has to agree with the validator.** A pre-flight that refuses what
//! `validate` accepts costs its caller invoices they could have sent; one that
//! accepts what `validate` refuses is a pre-flight nobody can act on, which is
//! worse than none at all. So the agreement is asserted over **every subset of
//! the ten VAT categories** rather than over the handful anybody thought to
//! write down.

use en16931::invoice::{Code, PriceDetails, VatBreakdown};
use en16931::{Invoice, InvoiceAmount, InvoiceLine, Percentage, Quantity, VatCategory, validate};
use rust_decimal::Decimal;

/// The rules `can_share_document` claims to stand for — and no others.
///
/// If a future artefact release adds an exclusivity rule, this list is what has
/// to grow with it, and the sweep below is what notices.
const EXCLUSIVITY_RULES: [&str; 5] = ["BR-O-11", "BR-O-12", "BR-O-13", "BR-O-14", "BR-B-02"];

/// A document carrying exactly `categories`, and nothing else worth reporting.
///
/// One line and one breakdown group per category, with a rate each category
/// will accept — the point is to make the exclusivity rules fire or not fire,
/// so every other finding is noise this test ignores.
fn invoice_using(categories: &[VatCategory]) -> Invoice {
    let mut inv = Invoice::default();
    for (i, cat) in categories.iter().enumerate() {
        let rate = if cat.carries_tax() {
            Percentage::new(Decimal::from(19))
        } else {
            Percentage::ZERO
        };
        let mut line = InvoiceLine::new(
            (i + 1).to_string(),
            "item",
            Quantity::new(Decimal::ONE),
            "C62",
            InvoiceAmount::from_minor_units(10_000),
            cat.code(),
            Some(rate),
        );
        line.price = PriceDetails::default();
        inv.lines.push(line);
        inv.vat_breakdown.push(VatBreakdown {
            taxable_amount: InvoiceAmount::from_minor_units(10_000),
            tax_amount: InvoiceAmount::ZERO,
            category: Code::new(cat.code()),
            rate: Some(rate),
            exemption_reason: Some("reason".into()),
            exemption_reason_code: None,
        });
    }
    inv
}

/// **Every subset of the ten categories**: the pre-flight and the rules agree.
///
/// 1 024 subsets, which is small enough to be exhaustive and therefore not to
/// need a generator or an argument about coverage.
#[test]
fn the_preflight_refuses_exactly_what_the_rules_report() {
    let all = VatCategory::ALL;
    let mut checked = 0usize;
    let mut refused = 0usize;

    for mask in 0u32..(1 << all.len()) {
        let categories: Vec<VatCategory> = all
            .iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << i) != 0)
            .map(|(_, c)| *c)
            .collect();

        let preflight = VatCategory::can_share_document(&categories);
        let report = validate(&invoice_using(&categories));
        let rules_fire = EXCLUSIVITY_RULES.iter().any(|r| report.has(r));

        assert_eq!(
            preflight.is_err(),
            rules_fire,
            "can_share_document and the rule set disagree about {:?}: \
             pre-flight said {}, the validator reported {}",
            categories.iter().map(|c| c.code()).collect::<Vec<_>>(),
            if preflight.is_err() { "no" } else { "yes" },
            if rules_fire { "a conflict" } else { "none" },
        );

        // A refusal cites the family that governs the exclusivity, and at
        // least one member must have reported *this* document — else the caller
        // is handed a clause they cannot find in their own report. Not *every*
        // member: `BR-O-11` covers a second breakdown group, `-12` a line,
        // `-13` an allowance and `-14` a charge, so a document with no
        // allowances cannot trip `-13`.
        if let Err(conflict) = &preflight {
            assert!(
                conflict.rules.iter().any(|r| report.has(r)),
                "{conflict} cites {:?}, none of which fired on the document",
                conflict.rules
            );
            assert!(
                conflict.rules.iter().all(|r| EXCLUSIVITY_RULES.contains(r)),
                "{conflict} cites a rule outside the declared family"
            );
            refused += 1;
        }
        checked += 1;
    }

    println!("category pre-flight\n  subsets checked: {checked}\n  refused:         {refused}");
    assert_eq!(checked, 1024, "every subset of the ten categories");
    assert!(refused > 0 && refused < checked, "both answers must occur");
}

/// One category on its own is always fine, and so is none.
///
/// The degenerate cases, named because an exclusivity check that refuses `[O]`
/// alone would refuse every out-of-scope invoice — including the ones that are
/// perfectly valid, which is the whole point of the category existing.
#[test]
fn a_single_category_never_conflicts_with_itself() {
    assert!(VatCategory::can_share_document(&[]).is_ok());
    for cat in VatCategory::ALL {
        assert!(
            VatCategory::can_share_document(&[cat]).is_ok(),
            "{} alone must be permitted",
            cat.code()
        );
        // …and a duplicate is still one category.
        assert!(VatCategory::can_share_document(&[cat, cat]).is_ok());
    }
}

/// The German municipal case, by name.
///
/// A *hoheitliche Abwassergebühr* is `O`; drinking water is `S`. Over 90 % of
/// German municipalities bill them together, and the combined document has no
/// valid EN 16931 rendering. The refusal must name the reason and the rules —
/// that is the difference between "we cannot bill this" and a wall of findings.
#[test]
fn the_municipal_invoice_is_refused_with_its_reason() {
    let err = VatCategory::can_share_document(&[VatCategory::Standard, VatCategory::OutOfScope])
        .expect_err("O and S cannot share a document");

    assert_eq!(err.category, VatCategory::OutOfScope);
    assert_eq!(err.rules, ["BR-O-11", "BR-O-12", "BR-O-13", "BR-O-14"]);

    let shown = err.to_string();
    assert!(shown.contains("O"), "{shown}");
    assert!(shown.contains("BR-O-11"), "{shown}");
    assert!(shown.contains("own document"), "the way out: {shown}");
}

/// `can_share_document` composes with what an invoice already carries.
#[test]
fn it_answers_for_a_document_already_built() {
    let inv = invoice_using(&[VatCategory::Standard, VatCategory::OutOfScope]);
    assert!(VatCategory::can_share_document(&inv.categories_used()).is_err());

    let ok = invoice_using(&[VatCategory::Standard, VatCategory::ZeroRated]);
    assert!(VatCategory::can_share_document(&ok.categories_used()).is_ok());
}
