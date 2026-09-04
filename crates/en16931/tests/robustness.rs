//! The invariant a validator must never break: **it does not panic.**
//!
//! A clearing platform validates millions of documents a day, and it does not
//! choose them. An invoice arrives from a counterparty, gets parsed into the
//! model, and is handed to `validate`. If a hostile or merely strange document
//! can panic the validator, that is a denial of service in a system whose whole
//! job is to keep running.
//!
//! The property is narrow and strong, and it is worth stating precisely:
//!
//! > For **any** `Invoice` that can be constructed, `validate` terminates,
//! > returns a report, and does not panic — including on values at the
//! > arithmetic boundaries, empty collections, and collections large enough to
//! > overflow a sum.
//!
//! # Why `proptest` and not `cargo-fuzz`
//!
//! `cargo-fuzz` needs nightly and a separate crate that never builds in CI on
//! stable. `proptest` runs in the ordinary test suite on every commit, which
//! means the property is checked continuously rather than during an occasional
//! campaign. Coverage-guided fuzzing finds *deeper* inputs, so it is worth
//! adding later — but a property that only runs when someone remembers is a
//! property that regresses.
//!
//! # What is deliberately generated
//!
//! Not "random bytes": the model is typed, so garbage bytes cannot reach it.
//! The interesting inputs are **structurally valid but semantically absurd**
//! documents — `i64::MAX` amounts that overflow when summed, thousands of lines,
//! negative quantities against negative prices, empty strings where codes go.
//! Those are what the arithmetic rules have to survive.

use en16931::invoice::*;
use en16931::{Date, InvoiceAmount, Percentage, Quantity, UnitPriceAmount, profiles, validate};
use proptest::prelude::*;
use rust_decimal::Decimal;

/// Amounts clustered at the boundaries, where the overflow lives.
fn any_amount() -> impl Strategy<Value = InvoiceAmount> {
    prop_oneof![
        // The extremes, which is where `checked_add` earns its keep.
        Just(InvoiceAmount::from_minor_units(i64::MAX)),
        Just(InvoiceAmount::from_minor_units(i64::MIN)),
        Just(InvoiceAmount::ZERO),
        any::<i64>().prop_map(InvoiceAmount::from_minor_units),
        (-1_000_000i64..1_000_000).prop_map(InvoiceAmount::from_minor_units),
    ]
}

fn any_code() -> impl Strategy<Value = Code> {
    prop_oneof![
        Just(Code::new("")),
        Just(Code::new("S")),
        Just(Code::new("Z")),
        Just(Code::new("O")),
        Just(Code::new("EUR")),
        "[A-Za-z0-9 ]{0,8}".prop_map(Code::new),
        // A code arriving from a document is whatever the document said.
        any_text().prop_map(|t| Code::new(&t)),
    ]
}

/// Text as a **European** invoice actually carries it, plus what an attacker
/// sends.
///
/// # Why ASCII-only generators made this suite weaker than it read
///
/// Every string in this file used to be `[A-Za-z0-9 ]`, so "validate never
/// panics" was really "validate never panics on ASCII" — on a crate whose
/// business is invoices between German, French and Spanish companies, where
/// `Müller`, `Straße` and `Ø` are the *ordinary* case rather than the hostile
/// one.
///
/// The gap was not hypothetical. Rule-id matching sliced a `&str` at a byte
/// index and aborted the process on `BR-Ié` — reachable from
/// `en16931 explain` and from `--without` — and no property here could see it,
/// because nothing in the suite ever produced a character wider than a byte.
///
/// So: multi-byte characters at every width, combining marks that make
/// character and byte counts disagree, a right-to-left mark, an astral-plane
/// character, control characters, and the empty string.
fn any_text() -> impl Strategy<Value = String> {
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
        // The general case, unrestricted: proptest shrinks toward the simple.
        ".{0,24}",
        // Long enough to exercise anything that indexes rather than iterates.
        "\\PC{200,240}",
    ]
    .prop_map(|s: String| s)
}

fn any_percentage() -> impl Strategy<Value = Percentage> {
    prop_oneof![
        Just(Percentage::ZERO),
        Just(Percentage::new(Decimal::MAX)),
        Just(Percentage::new(Decimal::MIN)),
        (-100i64..1000).prop_map(|v| Percentage::new(Decimal::from(v))),
    ]
}

fn any_quantity() -> impl Strategy<Value = Quantity> {
    prop_oneof![
        Just(Quantity::new(Decimal::ZERO)),
        Just(Quantity::new(Decimal::MAX)),
        Just(Quantity::new(Decimal::MIN)),
        (-1000i64..1000).prop_map(|v| Quantity::new(Decimal::from(v))),
    ]
}

fn any_line() -> impl Strategy<Value = InvoiceLine> {
    (
        any_amount(),
        any_quantity(),
        any_code(),
        any_code(),
        any_percentage(),
        any::<bool>(),
        any_text(),
    )
        .prop_map(|(net, qty, unit, cat, rate, has_rate, text)| InvoiceLine {
            id: text.clone(),
            note: Some(text.clone()),
            order_line_reference: None,
            accounting_reference: None,
            object_identifier: None,
            quantity: qty,
            unit_code: unit,
            net_amount: net,
            period: None,
            allowances: vec![],
            charges: vec![],
            price: PriceDetails {
                // A zero base quantity is the division-by-zero `R120` must not
                // take; generating it deliberately.
                base_quantity: Some(qty),
                net_price: UnitPriceAmount::new(Decimal::from(2)),
                ..Default::default()
            },
            vat: LineVat {
                category: cat,
                rate: has_rate.then_some(rate),
            },
            item: Item {
                name: Some(text.clone()),
                description: Some(text),
                ..Item::default()
            },
        })
}

fn any_breakdown() -> impl Strategy<Value = VatBreakdown> {
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
    fn any_invoice()(
        lines in prop::collection::vec(any_line(), 0..12),
        breakdown in prop::collection::vec(any_breakdown(), 0..6),
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
        inv.totals = DocumentTotals {
            line_total,
            taxable_total: taxable,
            vat_total: vat,
            gross_total: gross,
            due,
            ..Default::default()
        };
        inv
    }
}

proptest! {
    /// `validate` never panics, whatever the document.
    #[test]
    fn validate_never_panics(inv in any_invoice()) {
        let report = validate(&inv);
        // Touch every accessor: a panic hiding in one of them is still a panic
        // in the caller's process.
        let _ = report.is_valid();
        let _ = report.rules_checked();
        let _ = report.fatal().count();
        let _ = report.warnings().count();
        let _ = report.info().count();
        let _ = report.advisory().count();
        let _ = report.to_string();
    }

    /// Nor does any shipped profile, which runs strictly more rules.
    #[test]
    fn no_profile_panics(inv in any_invoice()) {
        for p in profiles::ALL {
            let report = p.validate(&inv);
            let _ = report.is_valid();
            let _ = report.to_string();
        }
    }

    /// **A rule name is user input, and no user input may abort the process.**
    ///
    /// This is the surface the invoice generators above cannot reach, and the
    /// one that actually broke. Every entry point below takes a string a person
    /// typed — `en16931 explain 'BR-Ié'`, `--without` with a stray accent, a
    /// `--profile` name pasted from a spreadsheet — and rule-id matching used
    /// to slice it at a byte index. The result was a **stack-unwinding panic in
    /// a library that promises not to have one**, on an input whose correct
    /// answer is "no rule by that name".
    ///
    /// Asserting the answer would be asserting a lookup; what matters here is
    /// only that each returns rather than aborting.
    #[test]
    fn a_rule_query_is_user_input_and_never_panics(q in any_text()) {
        use en16931::validation::rules::{explain, explain_restriction, touching};
        use en16931::validation::Check;

        let _ = explain(&q);
        let _ = explain_restriction(&q);
        let _ = en16931::profiles::lookup(&q);

        // The matcher itself, from both sides: a query compared against a rule
        // id, and a rule id compared against a query.
        let report = validate(&Invoice::default());
        let _ = report.has(&q);

        // Suppression resolves the name against the whole registry, and then
        // the report prints it back out.
        let deviated = Check::of::<profiles::XRechnung>().without(&q).run(&Invoice::default());
        let _ = deviated.to_string();
        let _ = deviated.suppressed().len();

        // …and the numeric surface beside it, for symmetry.
        let _ = touching(en16931::BtId(0));
    }

    /// Validation is **deterministic and stably ordered**.
    ///
    /// A report is an artefact you store, diff and gate CI on. Two runs over the
    /// same document that differ in order make every one of those uses a source
    /// of noise.
    #[test]
    fn reports_are_deterministic(inv in any_invoice()) {
        let a = validate(&inv);
        let b = validate(&inv);
        prop_assert_eq!(a.findings().len(), b.findings().len());
        for (x, y) in a.findings().iter().zip(b.findings()) {
            prop_assert_eq!(&x.rule, &y.rule);
            prop_assert_eq!(x.path.to_string(), y.path.to_string());
        }
    }

    /// Every finding names something the crate can explain — **under every
    /// profile**, not just the core set.
    ///
    /// A report citing an id nobody can look up is worse than silence: it sends
    /// the reader to KoSIT's index for a rule that is not there.
    ///
    /// The profile half is the half that mattered. `explain` searched `CORE`
    /// only, so an ordinary XRechnung report — `BR-DE-16`,
    /// `PEPPOL-EN16931-R120`, `BR-DEX-09` — resolved to `None` for every id in
    /// it, and this property passed anyway because it only looked at
    /// `validate()`.
    #[test]
    fn every_finding_can_be_explained(inv in any_invoice()) {
        use en16931::validation::rules::{explain, explain_restriction};
        for p in profiles::ALL {
            for f in p.validate(&inv).findings() {
                prop_assert!(
                    explain(&f.rule).is_some() || explain_restriction(&f.rule).is_some(),
                    "{} reports {} and nothing can explain it", p.id, f.rule
                );
            }
        }
    }

    /// **§4.4.4, as a property.** A conformant CIUS never accepts a document
    /// core EN 16931 rejects.
    ///
    /// This is what makes `Validated::widen` infallible, and it is the kind of
    /// claim that deserves arbitrary input rather than a fixture: the structural
    /// argument ("every restriction is a narrowing") is only as good as the
    /// `Restriction` variants, and `extra_rules` is not a restriction at all.
    ///
    /// Profiles that suppress a core rule are excluded — they are not CIUSes,
    /// and `is_conformant_cius()` is the thing that says so.
    #[test]
    fn conformant_profiles_never_accept_what_core_rejects(inv in any_invoice()) {
        for p in profiles::ALL.iter().filter(|p| p.is_conformant_cius()) {
            let mut doc = inv.clone();
            doc.specification_id = Some(p.specification_id.to_owned());
            if p.validate(&doc).is_valid() {
                prop_assert!(
                    validate(&doc).is_valid(),
                    "{} accepted a document core EN 16931 rejects", p.id
                );
            }
        }
    }

    /// `is_valid()` agrees with the absence of fatal findings, always.
    #[test]
    fn is_valid_means_no_fatal_findings(inv in any_invoice()) {
        let report = validate(&inv);
        prop_assert_eq!(report.is_valid(), report.fatal().count() == 0);
    }

    /// **A finding never points somewhere that cannot exist.**
    ///
    /// [`Group::repeats`] says which groups may occur more than once, and an
    /// occurrence index only means something in one of those: `BG-4[3]` claims a
    /// fourth seller, and BG-4 is `1..1`. `Path::at(Group::Seller, 3)` builds
    /// that happily, so nothing but this stops a rule emitting it.
    ///
    /// The converse is deliberately **not** asserted. `BR-16` reports "there is
    /// no invoice line" at `BG-25` with no index, which is right: the finding is
    /// about the group's absence, not about one of its occurrences.
    ///
    /// Until this existed, `repeats()` was public API whose only caller was its
    /// own unit test — a documented invariant with nothing enforcing it, which is
    /// how it came to disagree with the paths four rules were already emitting.
    #[test]
    fn every_finding_points_somewhere_that_can_exist(inv in any_invoice()) {
        for p in profiles::ALL {
            for f in p.validate(&inv).findings() {
                prop_assert!(
                    f.path.index.is_none() || f.path.group.repeats(),
                    "{} reports {} at {}, and {:?} occurs at most once",
                    p.id, f.rule, f.path, f.path.group
                );
            }
        }
    }
}

/// A minimal line carrying `net`. `InvoiceLine` has no `Default` on purpose —
/// BT-129, BT-130, BT-131 and BG-29 are all mandatory — so tests spell it out.
fn line(net: InvoiceAmount) -> InvoiceLine {
    InvoiceLine {
        id: String::new(),
        note: None,
        order_line_reference: None,
        accounting_reference: None,
        object_identifier: None,
        quantity: Quantity::new(Decimal::ONE),
        unit_code: Code::new("C62"),
        net_amount: net,
        period: None,
        allowances: vec![],
        charges: vec![],
        price: PriceDetails::default(),
        vat: LineVat::default(),
        item: Item::default(),
    }
}

/// The boundary cases worth naming, rather than hoping the generator finds them.
#[test]
fn the_extremes_are_survivable() {
    // A sum that overflows `i64` twice over.
    let mut inv = Invoice::default();
    inv.totals.line_total = InvoiceAmount::from_minor_units(i64::MAX);
    inv.lines = (0..3)
        .map(|_| line(InvoiceAmount::from_minor_units(i64::MAX)))
        .collect();
    let _ = validate(&inv);

    // A zero base quantity: `R120` divides by it if nothing stops it.
    let mut inv = Invoice::default();
    let mut l = line(InvoiceAmount::ZERO);
    l.price.base_quantity = Some(Quantity::new(Decimal::ZERO));
    inv.lines = vec![l];
    for p in profiles::ALL {
        let _ = p.validate(&inv);
    }

    // A date at the calendar's edge.
    assert!(Date::parse("9999-12-31").is_ok());
    assert!(Date::parse("0000-01-01").is_ok());
}
