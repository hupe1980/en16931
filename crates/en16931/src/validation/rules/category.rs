//! The nine VAT category families — `BR-S-*`, `BR-Z-*`, `BR-E-*`, `BR-AE-*`,
//! `BR-IC-*`, `BR-G-*`, `BR-O-*`, `BR-AF-*`, `BR-AG-*` — plus `BR-B-*`.
//!
//! # Why these are table-driven and the rest are not
//!
//! EN 16931-1 §6.4.3 writes these as **nine parallel tables**, one per category,
//! with the same ten row headings. `BR-S-08` and `BR-Z-08` are the same sentence
//! with a different category name and a different answer to "may this category
//! appear at several rates". Hand-writing ninety near-identical predicates would
//! be ninety chances to get one wrong, and the artefacts themselves are generated
//! this way.
//!
//! So the *logic* lives in one checker per row, parameterised by a
//! [`CategoryProfile`], and the [`crate::validation::Rule`] entries are emitted
//! per category by a macro — each with **its own real id**, because a report that
//! said `BR-CATEGORY-08` would be useless to anyone looking the rule up.
//!
//! # The asymmetries the table encodes
//!
//! | Row | Taxed (`S`, `L`, `M`) | Zero-tax (`Z`, `E`, `AE`, `K`, `G`, `O`) |
//! |---|---|---|
//! | `-01` groups | *at least one* — may repeat per rate | **exactly one** |
//! | `-05/06/07` rate | `S` > 0; `L`/`M` ≥ 0 | `= 0`, except `O` which must be **absent** |
//! | `-08` base | grouped by **(category, rate)** | grouped by category alone |
//! | `-09` tax | `= base × rate`, ±1.00 | `= 0`, exact |
//! | `-10` reason | **forbidden** | **required** |
//!
//! Every one of those columns differs, which is exactly why the standard writes
//! nine tables rather than one rule with a category parameter.

use rust_decimal::Decimal;

use crate::bt::{Group, Path};
use crate::invoice::{Invoice, VatBreakdown, terms as bt};
use crate::validation::{Findings, Rule, RuleId, Severity, Source};
use crate::{InvoiceAmount, Percentage, VatCategory};

/// The ±1.00 the artefacts allow on the `-08` and `-09` rows.
const TOLERANCE: Decimal = Decimal::ONE;

/// How many BG-23 groups a category may occupy — the `-01` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Groups {
    /// *"shall contain … at least one"* — the category may appear at several rates.
    AtLeastOne,
    /// *"shall contain … exactly one"* — the category has a single rate, zero.
    ExactlyOne,
}

/// What the `-05` / `-06` / `-07` rows require of a rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateRule {
    /// `BR-S-05`: *"shall be greater than zero"*.
    Positive,
    /// `BR-Z-05` and friends: *"shall be 0 (zero)"*.
    Zero,
    /// `BR-AF-05` / `BR-AG-05`: *"shall be 0 (zero) or greater than zero"*.
    ZeroOrPositive,
    /// `BR-O-05`: *"shall not contain"* a rate at all — absent, not zero.
    Absent,
}

/// What the `-09` row requires of the tax amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaxRule {
    /// *"shall equal 0 (zero)"*, exactly.
    Zero,
    /// *"shall equal the taxable amount multiplied by the rate"*, ±1.00.
    Derived,
}

/// One row of §6.4.3 — everything that differs between the nine tables.
#[derive(Debug, Clone, Copy)]
pub struct CategoryProfile {
    /// Which category.
    pub category: VatCategory,
    /// `-01`.
    pub groups: Groups,
    /// `-05` / `-06` / `-07`.
    pub rate: RateRule,
    /// `-09`.
    pub tax: TaxRule,
}

impl CategoryProfile {
    /// Whether `-08` groups by `(category, rate)` rather than by category alone.
    ///
    /// Only the categories that may appear at several rates do — which is
    /// exactly the categories whose `-01` says *"at least one"*.
    ///
    /// Public because [`crate::reconcile`](mod@crate::reconcile) must group BG-23 **the same way**
    /// `-08` checks it. Two independent readings of "which rows are one group"
    /// is exactly how a reconciler comes to produce a breakdown its own
    /// validator rejects.
    #[must_use]
    pub const fn grouped_by_rate(self) -> bool {
        matches!(self.groups, Groups::AtLeastOne)
    }
}

/// The nine tables, as data.
///
/// Public for the same reason as [`CategoryProfile::grouped_by_rate`]: the
/// reconciler derives a breakdown from this table rather than restating it.
#[must_use]
pub const fn profile(category: VatCategory) -> CategoryProfile {
    use RateRule::{Absent, Positive, Zero as RZero, ZeroOrPositive};
    use TaxRule::{Derived, Zero as TZero};
    use VatCategory::*;
    let (groups, rate, tax) = match category {
        Standard => (Groups::AtLeastOne, Positive, Derived),
        CanaryIslands | CeutaMelilla => (Groups::AtLeastOne, ZeroOrPositive, Derived),
        ZeroRated | Exempt | ReverseCharge | IntraCommunity | Export => {
            (Groups::ExactlyOne, RZero, TZero)
        }
        OutOfScope => (Groups::ExactlyOne, Absent, TZero),
        // `B` has only -01 and -02 in the artefacts, and neither is a rate or
        // tax rule. The values here are never consulted: no `-05`/`-08`/`-09`
        // rule is emitted for it.
        SplitPayment => (Groups::AtLeastOne, ZeroOrPositive, Derived),
    };
    CategoryProfile {
        category,
        groups,
        rate,
        tax,
    }
}

// ── Shared checkers, one per row ──────────────────────────────────────────────

/// Every `(category, rate)` a line, allowance or charge uses.
fn used_in_content(inv: &Invoice, cat: VatCategory) -> Vec<Option<Percentage>> {
    let mut v: Vec<_> = inv
        .lines
        .iter()
        .map(|l| &l.vat)
        .chain(inv.allowances.iter().map(|a| &a.vat))
        .chain(inv.charges.iter().map(|c| &c.vat))
        .filter(|v| v.semantics() == Some(cat))
        .map(|v| v.rate)
        .collect();
    v.sort();
    v.dedup();
    v
}

/// `-01` — the breakdown must contain the right number of groups for a category
/// that appears in the content.
pub fn check_groups(inv: &Invoice, p: CategoryProfile, f: &mut Findings<'_>) {
    if used_in_content(inv, p.category).is_empty() {
        return;
    }
    let groups = inv
        .vat_breakdown
        .iter()
        .filter(|e| e.semantics() == Some(p.category))
        .count();
    let ok = match p.groups {
        Groups::AtLeastOne => groups >= 1,
        Groups::ExactlyOne => groups == 1,
    };
    if !ok {
        f.arithmetic(
            Path::group(Group::VatBreakdown),
            match p.groups {
                Groups::AtLeastOne => "at least one group",
                Groups::ExactlyOne => "exactly one group",
            },
            groups,
        );
    }
}

/// Which of a line, an allowance or a charge a rate rule governs.
///
/// The artefacts number these separately — `-05` for the line (BT-152), `-06`
/// for the allowance (BT-96), `-07` for the charge (BT-103) — so a finding must
/// say which. Reporting all three under `-05` would send a reader to the wrong
/// row of the standard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateContext {
    /// `-05` — BT-152 on an invoice line.
    Line,
    /// `-06` — BT-96 on a document level allowance.
    Allowance,
    /// `-07` — BT-103 on a document level charge.
    Charge,
}

/// `-05` / `-06` / `-07` — the rate, in one of the three contexts.
pub fn check_rate(inv: &Invoice, p: CategoryProfile, ctx: RateContext, f: &mut Findings<'_>) {
    let ok = |rate: Option<Percentage>| match p.rate {
        RateRule::Positive => rate.is_some_and(Percentage::is_positive),
        RateRule::Zero => rate.is_some_and(Percentage::is_zero),
        RateRule::ZeroOrPositive => rate.is_some_and(|r| !r.is_negative()),
        // "shall not contain" — absent, which is not the same as zero.
        RateRule::Absent => rate.is_none(),
    };
    match ctx {
        RateContext::Line => {
            for (i, line) in inv.lines.iter().enumerate() {
                if line.vat.semantics() == Some(p.category) && !ok(line.vat.rate) {
                    f.at(Path::at_term(Group::Line, i, bt::LINE_VAT_RATE));
                }
            }
        }
        RateContext::Allowance => {
            for (i, a) in inv.allowances.iter().enumerate() {
                if a.vat.semantics() == Some(p.category) && !ok(a.vat.rate) {
                    f.at(Path::at_term(
                        Group::DocumentAllowance,
                        i,
                        bt::ALLOWANCE_VAT_RATE,
                    ));
                }
            }
        }
        RateContext::Charge => {
            for (i, c) in inv.charges.iter().enumerate() {
                if c.vat.semantics() == Some(p.category) && !ok(c.vat.rate) {
                    f.at(Path::at_term(Group::DocumentCharge, i, bt::CHARGE_VAT_RATE));
                }
            }
        }
    }
}

/// What the `-02` / `-03` / `-04` rows require of the seller's and buyer's tax
/// identifiers.
///
/// Every family has these three rows and they differ in four ways — which is
/// why they are a table rather than a shared predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentifierRule {
    /// `S`, `Z`, `E`, `AF`, `AG`: BT-31, BT-32 **or** BT-63.
    SellerAny,
    /// `G`: BT-31 **or** BT-63 — the tax registration identifier is not enough
    /// for an export.
    SellerVatOnly,
    /// `AE`: the seller set **and** (BT-48 or BT-47) — the buyer accounts for
    /// the tax, so the buyer must be identified too.
    SellerAnyAndBuyer,
    /// `IC`: (BT-31 or BT-63) **and** BT-48.
    SellerVatAndBuyerVat,
    /// `O`: BT-31, BT-63 and BT-48 shall **not** be present at all.
    NoneAllowed,
}

/// `-02` / `-03` / `-04` — the identifier requirement, in one of the three
/// contexts.
pub fn check_identifiers(
    inv: &Invoice,
    p: CategoryProfile,
    ctx: RateContext,
    f: &mut Findings<'_>,
) {
    let present = match ctx {
        RateContext::Line => inv
            .lines
            .iter()
            .any(|l| l.vat.semantics() == Some(p.category)),
        RateContext::Allowance => inv
            .allowances
            .iter()
            .any(|a| a.vat.semantics() == Some(p.category)),
        RateContext::Charge => inv
            .charges
            .iter()
            .any(|c| c.vat.semantics() == Some(p.category)),
    };
    if !present {
        return;
    }

    let seller_vat = inv.seller.vat_identifier.is_some();
    let seller_tax = inv.seller.tax_registration.is_some();
    let rep_vat = inv
        .tax_representative
        .as_ref()
        .is_some_and(|r| r.vat_identifier.is_some());
    let buyer_vat = inv.buyer.vat_identifier.is_some();
    let buyer_legal = inv.buyer.legal_registration.is_some();

    let ok = match identifier_rule(p.category) {
        IdentifierRule::SellerAny => seller_vat || seller_tax || rep_vat,
        IdentifierRule::SellerVatOnly => seller_vat || rep_vat,
        IdentifierRule::SellerAnyAndBuyer => {
            (seller_vat || seller_tax || rep_vat) && (buyer_vat || buyer_legal)
        }
        IdentifierRule::SellerVatAndBuyerVat => (seller_vat || rep_vat) && buyer_vat,
        // The only row phrased as a prohibition.
        IdentifierRule::NoneAllowed => !seller_vat && !rep_vat && !buyer_vat,
    };
    if !ok {
        f.at(Path::group_term(Group::Seller, bt::SELLER_VAT_ID));
    }
}

/// The `-02` / `-03` / `-04` requirement for a category.
const fn identifier_rule(category: VatCategory) -> IdentifierRule {
    match category {
        VatCategory::Export => IdentifierRule::SellerVatOnly,
        VatCategory::ReverseCharge => IdentifierRule::SellerAnyAndBuyer,
        VatCategory::IntraCommunity => IdentifierRule::SellerVatAndBuyerVat,
        VatCategory::OutOfScope => IdentifierRule::NoneAllowed,
        _ => IdentifierRule::SellerAny,
    }
}

/// `-08` — BT-116 equals Σ line net + Σ charges − Σ allowances for the category
/// (and, for the categories that may repeat, the rate).
///
/// This is the keystone: it is the only rule tying the invoice lines to the VAT
/// breakdown, and it is what makes a mis-attributed line a *reported* error
/// rather than a silently wrong invoice.
pub fn check_taxable_amount(inv: &Invoice, p: CategoryProfile, f: &mut Findings<'_>) {
    for (i, entry) in inv.vat_breakdown.iter().enumerate() {
        if entry.semantics() != Some(p.category) {
            continue;
        }
        let matches = |v: &crate::invoice::LineVat| {
            v.semantics() == Some(p.category) && (!p.grouped_by_rate() || v.rate == entry.rate)
        };
        let lines = inv
            .lines
            .iter()
            .filter(|l| matches(&l.vat))
            .map(|l| l.net_amount);
        let charges = inv
            .charges
            .iter()
            .filter(|c| matches(&c.vat))
            .map(|c| c.amount);
        let allowances = inv
            .allowances
            .iter()
            .filter(|a| matches(&a.vat))
            .map(|a| a.amount);

        let (Ok(pos), Ok(neg)) = (
            InvoiceAmount::checked_sum(lines.chain(charges)),
            InvoiceAmount::checked_sum(allowances),
        ) else {
            return;
        };
        let Ok(expected) = pos.checked_sub(neg) else {
            return;
        };

        // ±1.00, as the artefacts write it — and on absolute values, so a credit
        // note satisfies it.
        let diff = (expected.into_decimal() - entry.taxable_amount.into_decimal()).abs();
        if diff >= TOLERANCE {
            f.arithmetic(
                Path::at_term(Group::VatBreakdown, i, bt::VAT_TAXABLE_AMOUNT),
                expected,
                entry.taxable_amount,
            );
        }
    }
}

/// `-09` — the tax amount, either exactly zero or derived from the base.
pub fn check_tax_amount(inv: &Invoice, p: CategoryProfile, f: &mut Findings<'_>) {
    for (i, entry) in inv.vat_breakdown.iter().enumerate() {
        if entry.semantics() != Some(p.category) {
            continue;
        }
        let path = Path::at_term(Group::VatBreakdown, i, bt::VAT_TAX_AMOUNT);
        match p.tax {
            TaxRule::Zero => {
                if !entry.tax_amount.is_zero() {
                    f.arithmetic(path, "0.00", entry.tax_amount);
                }
            }
            TaxRule::Derived => {
                let rate = entry.rate.map_or(Decimal::ZERO, Percentage::into_decimal);
                let base = entry.taxable_amount.into_decimal().abs();
                let Some(exact) = base.checked_mul(rate).map(|v| v / Decimal::ONE_HUNDRED) else {
                    return;
                };
                let expected = exact.round_dp(2);
                let stated = entry.tax_amount.into_decimal().abs();
                if (stated - expected).abs() >= TOLERANCE {
                    f.arithmetic(path, expected, entry.tax_amount);
                }
            }
        }
    }
}

/// `-10` — whether an exemption reason is required or forbidden.
pub fn check_exemption_reason(inv: &Invoice, p: CategoryProfile, f: &mut Findings<'_>) {
    for (i, entry) in inv.vat_breakdown.iter().enumerate() {
        if entry.semantics() != Some(p.category) {
            continue;
        }
        let has = entry.has_exemption_reason();
        let bad = (p.category.requires_exemption_reason() && !has)
            || (p.category.forbids_exemption_reason() && has);
        if bad {
            f.at(Path::at_term(Group::VatBreakdown, i, bt::EXEMPTION_REASON));
        }
    }
}

// ── Rule emission ─────────────────────────────────────────────────────────────

/// Emit one [`Rule`] per (family, row), each with its real id.
macro_rules! family {
    // `expr` rather than `literal`: the ids and texts are built with `concat!`,
    // which is an expression at macro-expansion time even though it evaluates to
    // a `&'static str` literal — exactly what `RuleId::new` and `#[doc]` want.
    ($konst:ident, $id:expr, $cat:ident, $checker:ident, $text:expr) => {
        #[doc = $text]
        pub static $konst: Rule = Rule {
            id: RuleId::new($id),
            severity: Severity::Fatal,
            text: $text,
            terms: &[],
            source: Source::Both,
            eval: |inv, f| $checker(inv, profile(VatCategory::$cat), f),
        };
    };
    // The three-context rows: `-02/-03/-04` and `-05/-06/-07`.
    ($konst:ident, $id:expr, $cat:ident, $checker:ident, $ctx:ident, $text:expr) => {
        #[doc = $text]
        pub static $konst: Rule = Rule {
            id: RuleId::new($id),
            severity: Severity::Fatal,
            text: $text,
            terms: &[],
            source: Source::Both,
            eval: |inv, f| $checker(inv, profile(VatCategory::$cat), RateContext::$ctx, f),
        };
    };
}

/// Emit all ten rows of one category's table.
macro_rules! category_family {
    ($cat:ident, $prefix:literal, $konsts:ident) => {
        /// One VAT category's ten rows. Public so a caller can name a single
        /// rule — `category::s::R08` is `BR-S-08`.
        pub mod $konsts {
            use super::*;

            family!(R01, concat!($prefix, "-01"), $cat, check_groups, concat!(
                "An Invoice that contains an Invoice line (BG-25), a Document level allowance \
                 (BG-20) or a Document level charge (BG-21) with this VAT category shall contain \
                 the required number of VAT breakdown (BG-23) groups for it. [", $prefix, "-01]"));

            family!(R02, concat!($prefix, "-02"), $cat, check_identifiers, Line, concat!(
                "An Invoice that contains an Invoice line (BG-25) in this VAT category shall \
                 contain the tax identifiers the category requires. [", $prefix, "-02]"));
            family!(R03, concat!($prefix, "-03"), $cat, check_identifiers, Allowance, concat!(
                "An Invoice that contains a Document level allowance (BG-20) in this VAT category \
                 shall contain the tax identifiers the category requires. [", $prefix, "-03]"));
            family!(R04, concat!($prefix, "-04"), $cat, check_identifiers, Charge, concat!(
                "An Invoice that contains a Document level charge (BG-21) in this VAT category \
                 shall contain the tax identifiers the category requires. [", $prefix, "-04]"));

            family!(R05, concat!($prefix, "-05"), $cat, check_rate, Line, concat!(
                "In an Invoice line (BG-25) in this VAT category the Invoiced item VAT rate \
                 (BT-152) shall satisfy the category's rate rule. [", $prefix, "-05]"));
            family!(R06, concat!($prefix, "-06"), $cat, check_rate, Allowance, concat!(
                "In a Document level allowance (BG-20) in this VAT category the VAT rate (BT-96) \
                 shall satisfy the category's rate rule. [", $prefix, "-06]"));
            family!(R07, concat!($prefix, "-07"), $cat, check_rate, Charge, concat!(
                "In a Document level charge (BG-21) in this VAT category the VAT rate (BT-103) \
                 shall satisfy the category's rate rule. [", $prefix, "-07]"));

            family!(R08, concat!($prefix, "-08"), $cat, check_taxable_amount, concat!(
                "The VAT category taxable amount (BT-116) shall equal the sum of Invoice line net \
                 amounts (BT-131) plus document level charges (BT-99) minus document level \
                 allowances (BT-92) in this VAT category. [", $prefix, "-08]"));
            family!(R09, concat!($prefix, "-09"), $cat, check_tax_amount, concat!(
                "The VAT category tax amount (BT-117) in a VAT breakdown (BG-23) with this VAT \
                 category shall satisfy the category's tax rule. [", $prefix, "-09]"));
            family!(R10, concat!($prefix, "-10"), $cat, check_exemption_reason, concat!(
                "A VAT breakdown (BG-23) with this VAT category shall have, or shall not have, a \
                 VAT exemption reason code (BT-121) or text (BT-120) as the category requires. [",
                $prefix, "-10]"));

            /// All ten rows of this family.
            pub static ALL: &[&Rule] =
                &[&R01, &R02, &R03, &R04, &R05, &R06, &R07, &R08, &R09, &R10];
        }
    };
}

category_family!(Standard, "BR-S", s);
category_family!(ZeroRated, "BR-Z", z);
category_family!(Exempt, "BR-E", e);
category_family!(ReverseCharge, "BR-AE", ae);
category_family!(IntraCommunity, "BR-IC", ic);
category_family!(Export, "BR-G", g);
category_family!(OutOfScope, "BR-O", o);
category_family!(CanaryIslands, "BR-AF", af);
category_family!(CeutaMelilla, "BR-AG", ag);

// ── `B` — split payment, which has only two rules of its own ─────────────────

/// `BR-B-01` — split payment is domestic Italian only.
pub static BR_B_01: Rule = Rule {
    id: RuleId::new("BR-B-01"),
    severity: Severity::Fatal,
    text: "An Invoice where the VAT category code (BT-151, BT-95 or BT-102) is \"Split payment\" \
           shall be a domestic Italian invoice.",
    terms: &[bt::SELLER_COUNTRY, bt::BUYER_COUNTRY],
    source: Source::ArtefactOnly,
    eval: |inv, f| {
        if used_in_content(inv, VatCategory::SplitPayment).is_empty() {
            return;
        }
        let italian = |c: Option<&crate::invoice::Code>| c.is_some_and(|c| c.as_str() == "IT");
        if !italian(inv.seller.address.country.as_ref())
            || !italian(inv.buyer.address.country.as_ref())
        {
            f.at(Path::term(bt::TYPE_CODE));
        }
    },
};

/// `BR-B-02` — split payment and standard rated cannot coexist.
pub static BR_B_02: Rule = Rule {
    id: RuleId::new("BR-B-02"),
    severity: Severity::Fatal,
    text: "An Invoice that contains a line, allowance or charge where the VAT category code is \
           \"Split payment\" shall not contain one where it is \"Standard rated\".",
    terms: &[bt::LINE_VAT_CATEGORY],
    source: Source::ArtefactOnly,
    eval: |inv, f| {
        let used = inv.categories_used();
        if used.contains(&VatCategory::SplitPayment) && used.contains(&VatCategory::Standard) {
            f.at(Path::group(Group::VatBreakdown));
        }
    },
};

/// `BR-O-11` … `BR-O-14` — an `O` breakdown excludes everything else.
pub static BR_O_11: Rule = Rule {
    id: RuleId::new("BR-O-11"),
    severity: Severity::Fatal,
    text: "An Invoice that contains a VAT breakdown group (BG-23) with a VAT category code \
           (BT-118) \"Not subject to VAT\" shall not contain other VAT breakdown groups (BG-23).",
    terms: &[bt::VAT_CATEGORY],
    source: Source::Both,
    eval: |inv, f| {
        let has_o = |e: &VatBreakdown| e.semantics() == Some(VatCategory::OutOfScope);
        if inv.vat_breakdown.iter().any(has_o) && inv.vat_breakdown.len() > 1 {
            for (i, e) in inv.vat_breakdown.iter().enumerate() {
                if !has_o(e) {
                    f.at(Path::at_term(Group::VatBreakdown, i, bt::VAT_CATEGORY));
                }
            }
        }
    },
};

/// `BR-O-12` … `BR-O-14` — nor content in another category.
pub static BR_O_12: Rule = Rule {
    id: RuleId::new("BR-O-12"),
    severity: Severity::Fatal,
    text: "An Invoice that contains a VAT breakdown group (BG-23) with a VAT category code \
           (BT-118) \"Not subject to VAT\" shall not contain an Invoice line (BG-25), a Document \
           level allowance (BG-20) or a Document level charge (BG-21) in another category.",
    terms: &[bt::LINE_VAT_CATEGORY],
    source: Source::Both,
    eval: |inv, f| {
        if !inv
            .vat_breakdown
            .iter()
            .any(|e| e.semantics() == Some(VatCategory::OutOfScope))
        {
            return;
        }
        for (i, line) in inv.lines.iter().enumerate() {
            if line.vat.semantics() != Some(VatCategory::OutOfScope) {
                f.at(Path::at_term(Group::Line, i, bt::LINE_VAT_CATEGORY));
            }
        }
    },
};

/// `BR-O-13` — nor an allowance in another category.
pub static BR_O_13: Rule = Rule {
    id: RuleId::new("BR-O-13"),
    severity: Severity::Fatal,
    text: "An Invoice that contains a VAT breakdown group (BG-23) with a VAT category code \
           (BT-118) \"Not subject to VAT\" shall not contain Document level allowances (BG-20) \
           where the VAT category code (BT-95) is not \"Not subject to VAT\".",
    terms: &[bt::ALLOWANCE_VAT_CATEGORY],
    source: Source::Both,
    eval: |inv, f| {
        if !has_out_of_scope_group(inv) {
            return;
        }
        for (i, a) in inv.allowances.iter().enumerate() {
            if a.vat.semantics() != Some(VatCategory::OutOfScope) {
                f.at(Path::at_term(
                    Group::DocumentAllowance,
                    i,
                    bt::ALLOWANCE_VAT_CATEGORY,
                ));
            }
        }
    },
};

/// `BR-O-14` — nor a charge in another category.
pub static BR_O_14: Rule = Rule {
    id: RuleId::new("BR-O-14"),
    severity: Severity::Fatal,
    text: "An Invoice that contains a VAT breakdown group (BG-23) with a VAT category code \
           (BT-118) \"Not subject to VAT\" shall not contain Document level charges (BG-21) \
           where the VAT category code (BT-102) is not \"Not subject to VAT\".",
    terms: &[bt::CHARGE_VAT_CATEGORY],
    source: Source::Both,
    eval: |inv, f| {
        if !has_out_of_scope_group(inv) {
            return;
        }
        for (i, c) in inv.charges.iter().enumerate() {
            if c.vat.semantics() != Some(VatCategory::OutOfScope) {
                f.at(Path::at_term(
                    Group::DocumentCharge,
                    i,
                    bt::CHARGE_VAT_CATEGORY,
                ));
            }
        }
    },
};

/// Whether the breakdown contains an `O` group, which BR-O-11..14 all turn on.
fn has_out_of_scope_group(inv: &Invoice) -> bool {
    inv.vat_breakdown
        .iter()
        .any(|e| e.semantics() == Some(VatCategory::OutOfScope))
}

/// Every rule this module defines — ninety family rows plus the extras.
pub static ALL: std::sync::LazyLock<Vec<&'static Rule>> = std::sync::LazyLock::new(|| {
    let families: [&[&'static Rule]; 9] = [
        s::ALL,
        z::ALL,
        e::ALL,
        ae::ALL,
        ic::ALL,
        g::ALL,
        o::ALL,
        af::ALL,
        ag::ALL,
    ];
    let extras: [&'static Rule; 6] = [&BR_B_01, &BR_B_02, &BR_O_11, &BR_O_12, &BR_O_13, &BR_O_14];
    families
        .into_iter()
        .flatten()
        .copied()
        .chain(extras)
        .collect()
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_matches_the_standards_nine_tables() {
        // Taxed categories may repeat per rate; zero-tax ones may not.
        for c in [
            VatCategory::Standard,
            VatCategory::CanaryIslands,
            VatCategory::CeutaMelilla,
        ] {
            assert_eq!(profile(c).groups, Groups::AtLeastOne, "{c}");
            assert_eq!(profile(c).tax, TaxRule::Derived, "{c}");
            assert!(profile(c).grouped_by_rate(), "{c}");
        }
        for c in [
            VatCategory::ZeroRated,
            VatCategory::Exempt,
            VatCategory::ReverseCharge,
            VatCategory::IntraCommunity,
            VatCategory::Export,
            VatCategory::OutOfScope,
        ] {
            assert_eq!(profile(c).groups, Groups::ExactlyOne, "{c}");
            assert_eq!(profile(c).tax, TaxRule::Zero, "{c}");
            assert!(!profile(c).grouped_by_rate(), "{c}");
        }
    }

    #[test]
    fn only_out_of_scope_forbids_the_rate_outright() {
        for c in VatCategory::ALL {
            if c == VatCategory::SplitPayment {
                continue; // no rate rule in the artefacts
            }
            assert_eq!(
                profile(c).rate == RateRule::Absent,
                c == VatCategory::OutOfScope,
                "{c}"
            );
        }
        assert_eq!(profile(VatCategory::Standard).rate, RateRule::Positive);
        assert_eq!(profile(VatCategory::ZeroRated).rate, RateRule::Zero);
        assert_eq!(
            profile(VatCategory::CanaryIslands).rate,
            RateRule::ZeroOrPositive
        );
    }

    #[test]
    fn every_family_emits_its_own_real_id() {
        // A report saying `BR-CATEGORY-08` would be useless to look up.
        let ids: Vec<_> = ALL.iter().map(|r| r.id.as_str()).collect();
        for expect in [
            "BR-S-02", "BR-S-06", "BR-S-07", "BR-S-08", "BR-Z-09", "BR-E-10", "BR-AE-01",
            "BR-AE-03", "BR-IC-04", "BR-IC-05", "BR-G-08", "BR-O-05", "BR-O-14", "BR-AF-10",
            "BR-AG-09", "BR-B-01",
        ] {
            assert!(ids.contains(&expect), "{expect} missing from {ids:?}");
        }
        // Nine families x ten rows, plus BR-B-01/02 and BR-O-11..14.
        assert_eq!(ids.len(), 9 * 10 + 6);
    }

    #[test]
    fn the_standards_ig_ip_spellings_resolve_to_the_artefacts_af_ag() {
        // EN 16931-1 calls these families BR-IG-* and BR-IP-*; the artefacts
        // call them BR-AF-* and BR-AG-*. Both must reach the same rule.
        let by = |q: &str| ALL.iter().find(|r| r.id.matches(q)).map(|r| r.id.as_str());
        assert_eq!(by("BR-IG-1"), Some("BR-AF-01"));
        assert_eq!(by("BR-IP-10"), Some("BR-AG-10"));
    }
}
