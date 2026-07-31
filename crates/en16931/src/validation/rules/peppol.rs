//! Peppol BIS Billing 3.0's own rules — the **third and fourth** tolerance
//! regimes.
//!
//! # ±0.02, and why it is not the other three
//!
//! | Regime | Rules | Tolerance | Whose |
//! |---|---|---|---|
//! | Totals chain | `BR-CO-10` … `BR-CO-16` | **exact** | CEN |
//! | VAT derivation | `BR-CO-17`, `BR-*-08/09` | **±1.00**, on absolute values | CEN artefacts |
//! | **Line & allowance derivation** | **`R120`, `R040`** | **±0.02** | **Peppol** |
//! | **…the same, in HUF** | **[`XR_R120`], [`XR_R040`]** | **±0.5** | **XRechnung** |
//!
//! Peppol implements its slack with a helper in its own Schematron:
//!
//! ```xslt
//! <function name="u:slack" as="xs:boolean">
//!   <param name="exp"/><param name="val"/><param name="slack"/>
//!   <sequence select="($exp + $slack) >= $val and ($exp - $slack) <= $val"/>
//! </function>
//! ```
//!
//! Exactly two rules use it, both at `0.02` — and XRechnung, when it merges
//! them, replaces that constant with `if($documentCurrencyCode = 'HUF') then
//! 0.5 else 0.02`. Everything else in this module is **exact**, including
//! `R046`, which is the trap: it looks like `R040`'s sibling and carries no
//! slack at all, so a caller who computes the net price themselves and lands a
//! cent out fails it.
//!
//! # `R120` is not a CEN rule
//!
//! EN 16931 has **no** rule tying BT-131 to quantity × price. Under
//! [`crate::profiles::EN16931`] a line whose amount does not follow from its
//! price is perfectly valid. That is why these live in
//! [`crate::Profile::extra_rules`] rather than in the core set — and why
//! tolerance is a property of the *rule*, never a crate-wide constant.

use rust_decimal::Decimal;

use super::{Findings, Rule, RuleId, Severity, Source};
use crate::bt::BtId;
use crate::bt::{Group, Path};
use crate::codes::generated as lists;
use crate::invoice::{Invoice, LineAllowanceCharge, terms as bt};
use crate::{InvoiceAmount, Percentage};

/// Peppol's `u:slack`, verbatim.
pub const SLACK: Decimal = Decimal::from_parts(2, 0, 0, false, 2); // 0.02

/// XRechnung's slack for Hungarian forint — **a fourth tolerance regime**.
///
/// When XRechnung merges Peppol's rules it rewrites the constant:
///
/// ```xslt
/// <let name="slackValue" value="if($documentCurrencyCode = 'HUF') then 0.5 else 0.02"/>
/// ```
///
/// HUF has no minor unit in practice — prices are whole forint — so 0.02 is
/// tighter than the currency can express. **Peppol does not do this**: its own
/// Schematron hardcodes `0.02` for every currency, so the same HUF invoice can
/// pass XRechnung and fail Peppol. That is why the two profiles get their own
/// instances of `R040` and `R120` rather than sharing one.
pub const HUF_SLACK: Decimal = Decimal::from_parts(5, 0, 0, false, 1); // 0.5

/// Peppol's policy: 0.02, whatever the currency.
fn fixed_slack(_: &Invoice) -> Decimal {
    SLACK
}

/// XRechnung's policy: 0.5 for HUF, 0.02 otherwise.
pub(crate) fn currency_slack(inv: &Invoice) -> Decimal {
    if inv.currency.as_ref().is_some_and(|c| c.as_str() == "HUF") {
        HUF_SLACK
    } else {
        SLACK
    }
}

/// `|expected − actual| ≤ slack`, matching `u:slack`'s inclusive comparison.
fn within(expected: Decimal, actual: Decimal, slack: Decimal) -> bool {
    (expected - actual).abs() <= slack
}

macro_rules! rule {
    (
        $konst:ident, $id:literal,
        terms: [$($t:expr),* $(,)?],
        $text:literal,
        |$inv:ident, $f:ident| $body:block
    ) => {
        #[doc = $text]
        pub static $konst: Rule = Rule {
            id: RuleId::new($id),
            severity: Severity::Fatal,
            text: $text,
            terms: &[$($t),*],
            // Peppol's own additions: neither CEN's standard nor its artefacts.
            source: Source::ArtefactOnly,
            eval: |$inv: &Invoice, $f: &mut Findings<'_>| $body,
        };
    };
}

// ── R120 — the line amount, ±0.02 ─────────────────────────────────────────────

/// `R120`'s check, parameterised by the profile's slack policy.
pub(crate) fn check_line_amount(
    inv: &Invoice,
    f: &mut Findings<'_>,
    slack: fn(&Invoice) -> Decimal,
) {
    let tolerance = slack(inv);
    for (i, line) in inv.lines.iter().enumerate() {
        // `None` means 1, matching R120's own `$baseQuantity` default.
        let base = line
            .price
            .base_quantity
            .map_or(Decimal::ONE, |q| q.into_decimal());
        if base.is_zero() {
            continue; // R121 reports this; dividing would panic.
        }
        let sum = |v: &[LineAllowanceCharge]| {
            InvoiceAmount::checked_sum(v.iter().map(|a| a.amount)).map(InvoiceAmount::into_decimal)
        };
        let (Ok(charges), Ok(allowances)) = (sum(&line.charges), sum(&line.allowances)) else {
            continue;
        };
        let Some(product) = line
            .quantity
            .into_decimal()
            .checked_mul(line.price.net_price.into_decimal())
            .and_then(|p| p.checked_div(base))
        else {
            continue;
        };
        let expected = product + charges - allowances;
        if !within(expected, line.net_amount.into_decimal(), tolerance) {
            f.arithmetic(
                Path::at_term(Group::Line, i, bt::LINE_NET_AMOUNT),
                expected.round_dp(2),
                line.net_amount,
            );
        }
    }
}

rule!(R120, "PEPPOL-EN16931-R120",
terms: [bt::LINE_NET_AMOUNT, bt::LINE_QUANTITY, bt::ITEM_NET_PRICE, bt::PRICE_BASE_QUANTITY],
"Invoice line net amount MUST equal (Invoiced quantity * (Item net price/item price base \
 quantity) + Sum of invoice line charge amount - Sum of invoice line allowance amount).",
|inv, f| {
    check_line_amount(inv, f, fixed_slack);
});

// ── R040 / R041 / R042 — the allowance basis ──────────────────────────────────

/// Check one allowance or charge's `amount = base × percentage / 100`.
fn check_basis(
    amount: InvoiceAmount,
    base: Option<InvoiceAmount>,
    pct: Option<Percentage>,
    path: Path,
    f: &mut Findings<'_>,
    tolerance: Decimal,
) {
    let (Some(base), Some(pct)) = (base, pct) else {
        return;
    };
    let Some(expected) = base
        .into_decimal()
        .checked_mul(pct.into_decimal())
        .map(|v| v / Decimal::ONE_HUNDRED)
    else {
        return;
    };
    if !within(expected, amount.into_decimal(), tolerance) {
        f.arithmetic(path, expected.round_dp(2), amount);
    }
}

/// `R040`'s check, parameterised by the profile's slack policy.
pub(crate) fn check_allowance_basis(
    inv: &Invoice,
    f: &mut Findings<'_>,
    slack: fn(&Invoice) -> Decimal,
) {
    let tolerance = slack(inv);
    for (i, a) in inv.allowances.iter().enumerate() {
        check_basis(
            a.amount,
            a.base_amount,
            a.percentage,
            Path::at_term(Group::DocumentAllowance, i, bt::ALLOWANCE_AMOUNT),
            f,
            tolerance,
        );
    }
    for (i, c) in inv.charges.iter().enumerate() {
        check_basis(
            c.amount,
            c.base_amount,
            c.percentage,
            Path::at_term(Group::DocumentCharge, i, bt::CHARGE_AMOUNT),
            f,
            tolerance,
        );
    }
    // The Schematron's context lists the line-level element alongside the
    // document-level one, so the same rule governs BG-27 and BG-28.
    for (i, line) in inv.lines.iter().enumerate() {
        for a in line.allowances.iter().chain(&line.charges) {
            check_basis(
                a.amount,
                a.base_amount,
                a.percentage,
                Path::at_term(Group::Line, i, bt::LINE_ALLOWANCE_AMOUNT),
                f,
                tolerance,
            );
        }
    }
}

rule!(R040, "PEPPOL-EN16931-R040",
terms: [bt::ALLOWANCE_AMOUNT, bt::ALLOWANCE_BASE, bt::ALLOWANCE_PERCENTAGE],
"Allowance/charge amount must equal base amount * percentage/100 if base amount and \
 percentage exists.",
|inv, f| {
    check_allowance_basis(inv, f, fixed_slack);
});

rule!(R041, "PEPPOL-EN16931-R041",
terms: [bt::ALLOWANCE_BASE, bt::ALLOWANCE_PERCENTAGE],
"Allowance/charge base amount MUST be provided when allowance/charge percentage is provided.",
|inv, f| {
    let half = |base: Option<InvoiceAmount>, pct: Option<Percentage>| {
        pct.is_some() && base.is_none()
    };
    for (i, a) in inv.allowances.iter().enumerate() {
        if half(a.base_amount, a.percentage) {
            f.at(Path::at_term(Group::DocumentAllowance, i, bt::ALLOWANCE_BASE));
        }
    }
    for (i, c) in inv.charges.iter().enumerate() {
        if half(c.base_amount, c.percentage) {
            f.at(Path::at_term(Group::DocumentCharge, i, bt::CHARGE_BASE));
        }
    }
    for (i, line) in inv.lines.iter().enumerate() {
        for a in line.allowances.iter().chain(&line.charges) {
            if half(a.base_amount, a.percentage) {
                f.at(Path::at_term(Group::Line, i, bt::LINE_ALLOWANCE_AMOUNT));
            }
        }
    }
});

rule!(R042, "PEPPOL-EN16931-R042",
terms: [bt::ALLOWANCE_BASE, bt::ALLOWANCE_PERCENTAGE],
"Allowance/charge percentage MUST be provided when allowance/charge base amount is provided.",
|inv, f| {
    let half = |base: Option<InvoiceAmount>, pct: Option<Percentage>| {
        base.is_some() && pct.is_none()
    };
    for (i, a) in inv.allowances.iter().enumerate() {
        if half(a.base_amount, a.percentage) {
            f.at(Path::at_term(Group::DocumentAllowance, i, bt::ALLOWANCE_PERCENTAGE));
        }
    }
    for (i, c) in inv.charges.iter().enumerate() {
        if half(c.base_amount, c.percentage) {
            f.at(Path::at_term(Group::DocumentCharge, i, bt::CHARGE_PERCENTAGE));
        }
    }
    for (i, line) in inv.lines.iter().enumerate() {
        for a in line.allowances.iter().chain(&line.charges) {
            if half(a.base_amount, a.percentage) {
                f.at(Path::at_term(Group::Line, i, bt::LINE_ALLOWANCE_AMOUNT));
            }
        }
    }
});

// ── BG-29 — R046, R121, R130 ──────────────────────────────────────────────────

rule!(R046, "PEPPOL-EN16931-R046",
terms: [bt::ITEM_NET_PRICE, bt::ITEM_GROSS_PRICE, bt::ITEM_PRICE_DISCOUNT],
"Item net price MUST equal (Gross price - Allowance amount) when gross price is provided.",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        let Some(gross) = line.price.gross_price else { continue };
        let discount = line
            .price
            .price_discount
            .map_or(Decimal::ZERO, |d| d.into_decimal());
        let expected = gross.into_decimal() - discount;
        // **Exact.** Unlike R040 this carries no `u:slack`, which is why a
        // producer should derive BT-146 rather than compute and state it.
        if expected != line.price.net_price.into_decimal() {
            f.arithmetic(
                Path::at_term(Group::Line, i, bt::ITEM_NET_PRICE),
                expected,
                line.price.net_price,
            );
        }
    }
});

rule!(R121, "PEPPOL-EN16931-R121",
terms: [bt::PRICE_BASE_QUANTITY],
"Base quantity MUST be a positive number above zero.",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        // It is a divisor in R120, so zero is not merely odd but undefined.
        if line.price.base_quantity.is_some_and(|q| !q.is_positive()) {
            f.at(Path::at_term(Group::Line, i, bt::PRICE_BASE_QUANTITY));
        }
    }
});

rule!(R130, "PEPPOL-EN16931-R130",
terms: [bt::PRICE_BASE_QUANTITY_CODE, bt::LINE_UNIT_CODE],
"Unit code of price base quantity MUST be same as invoiced quantity.",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        // A cross-field rule: BT-130 lives on the quantity and BT-150 on the
        // price, so only the line can check it. Only when both are stated —
        // supplying one is not a contradiction.
        if line
            .price
            .base_quantity_code
            .as_ref()
            .is_some_and(|c| c != &line.unit_code)
        {
            f.arithmetic(
                Path::at_term(Group::Line, i, bt::PRICE_BASE_QUANTITY_CODE),
                &line.unit_code,
                line.price.base_quantity_code.as_ref().unwrap(),
            );
        }
    }
});

// ── Peppol's own presence rules ───────────────────────────────────────────────

rule!(R061, "PEPPOL-EN16931-R061",
terms: [],
"Mandate reference MUST be provided for direct debit.",
|inv, f| {
    // UNTDID 4461: 59 is SEPA direct debit.
    if let Some(p) = &inv.payment
        && p.means_code.as_ref().is_some_and(|c| c.as_str() == "59")
        && p.mandate_reference().is_none_or(str::is_empty)
    {
        f.at(Path::group(Group::Payment));
    }
});

/// Every rule this module defines.
///
/// Attached to [`crate::profiles::PEPPOL_BIS_3`] through
/// [`crate::Profile::extra_rules`], **not** to the core set — `R120` in
/// particular has no CEN counterpart.
pub static ALL: &[&Rule] = &[
    &R040, &R041, &R042, &R046, &R061, &R120, &R121, &R130, //
    &R001, &R002, &R003, &R004, &R005, &R007, &R010, &R020, &R055, &R110, &R111, //
    &P0104, &P0105, &P0106, &P0107, &P0108, &P0109, &P0110, &P0111, &P0112, //
    &CL001, &CL002, &CL003, &CL006, &CL008, &P0100, &P0101, //
    &F001, &R008, &R043, &R044, &R051, &CL007, &R053, &R054, &R080, &R100, &R101,
];

/// The Peppol rules the model retires — registered, never fired.
pub static BY_TYPE: &[&Rule] = &[
    &F001, &R008, &R043, &R044, &R051, &CL007, &R053, &R054, &R080, &R100, &R101,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slack_is_exactly_two_hundredths() {
        assert_eq!(SLACK, Decimal::from_str_exact("0.02").unwrap());
        // Inclusive, matching `u:slack`'s `>=` / `<=`.
        let d = |s: &str| Decimal::from_str_exact(s).unwrap();
        assert!(within(d("10.00"), d("10.02"), SLACK));
        assert!(within(d("10.00"), d("9.98"), SLACK));
        assert!(!within(d("10.00"), d("10.03"), SLACK));
    }

    #[test]
    fn these_are_peppols_rules_not_cens() {
        // R120 has no CEN counterpart at all: EN 16931 never ties BT-131 to
        // quantity x price. Registering it in the core set would reject invoices
        // the standard permits.
        for r in ALL {
            assert!(
                r.id.as_str().starts_with("PEPPOL-"),
                "{} is not namespaced",
                r.id
            );
            // Against `CORE` directly. This used to call `explain`, which
            // searched `CORE` and so happened to mean the same thing — until
            // `explain` was fixed to resolve profile rules too, at which point
            // the proxy stopped matching the intent. Assert the thing meant.
            assert!(
                !super::super::CORE.iter().any(|c| c.id == r.id),
                "{} leaked into CORE",
                r.id
            );
        }
    }
}

// ── The VATEX ⇒ category rules, P0104 … P0111 ─────────────────────────────────

/// Peppol pins eight VATEX codes to the category they may appear under.
///
/// EN 16931 has nothing like this: `BR-CL-22` checks that BT-121 is *in* the
/// VATEX list and stops. Peppol goes further, because the CEF list encodes the
/// legal basis and the basis determines the category — `VATEX-EU-IC` is an
/// intra-community supply, so the category can only be `K`. A document that
/// says `K`-exempt-because-`VATEX-EU-AE` is internally contradictory in a way
/// core EN 16931 cannot see.
///
/// Note `VATEX-EU-D`, `-F`, `-I` and `-J` all pin to `E`: four different Article
/// bases for the same category.
const VATEX_CATEGORY: &[(&str, &str, &str)] = &[
    ("PEPPOL-EN16931-P0104", "VATEX-EU-G", "G"),
    ("PEPPOL-EN16931-P0105", "VATEX-EU-O", "O"),
    ("PEPPOL-EN16931-P0106", "VATEX-EU-IC", "K"),
    ("PEPPOL-EN16931-P0107", "VATEX-EU-AE", "AE"),
    ("PEPPOL-EN16931-P0108", "VATEX-EU-D", "E"),
    ("PEPPOL-EN16931-P0109", "VATEX-EU-F", "E"),
    ("PEPPOL-EN16931-P0110", "VATEX-EU-I", "E"),
    ("PEPPOL-EN16931-P0111", "VATEX-EU-J", "E"),
];

macro_rules! vatex_rule {
    ($konst:ident, $idx:expr) => {
        #[doc = "Tax category MUST match the exemption reason code — see the VATEX category table."]
        pub static $konst: Rule = Rule {
            id: RuleId::new(VATEX_CATEGORY[$idx].0),
            severity: Severity::Fatal,
            text: "Tax category MUST be the one the VATEX exemption reason code implies.",
            terms: &[bt::EXEMPTION_REASON_CODE, bt::VAT_CATEGORY],
            source: Source::ArtefactOnly,
            eval: |inv: &Invoice, f: &mut Findings<'_>| {
                let (_, vatex, want) = VATEX_CATEGORY[$idx];
                for (i, g) in inv.vat_breakdown.iter().enumerate() {
                    // Peppol upper-cases the code before comparing; the category
                    // it compares verbatim.
                    let hit = g
                        .exemption_reason_code
                        .as_ref()
                        .is_some_and(|c| c.as_str().eq_ignore_ascii_case(vatex));
                    if hit && g.category.as_str() != want {
                        f.at(Path::at_term(Group::VatBreakdown, i, bt::VAT_CATEGORY));
                    }
                }
            },
        };
    };
}

vatex_rule!(P0104, 0);
vatex_rule!(P0105, 1);
vatex_rule!(P0106, 2);
vatex_rule!(P0107, 3);
vatex_rule!(P0108, 4);
vatex_rule!(P0109, 5);
vatex_rule!(P0110, 6);
vatex_rule!(P0111, 7);

// ── Document-level Peppol rules ───────────────────────────────────────────────

/// Whether both parties are German — the condition `R002` and `P0112` share.
fn both_german(inv: &Invoice) -> bool {
    let de = |p: &crate::invoice::Party| {
        p.address
            .country
            .as_ref()
            .is_some_and(|c| c.as_str() == "DE")
    };
    de(&inv.seller) && de(&inv.buyer)
}

rule!(R001, "PEPPOL-EN16931-R001", terms: [bt::BUSINESS_PROCESS],
"Business process MUST be provided.",
|inv, f| {
    if inv.business_process.as_deref().is_none_or(str::is_empty) {
        f.at(Path::term(bt::BUSINESS_PROCESS));
    }
});

rule!(R003, "PEPPOL-EN16931-R003", terms: [bt::BUYER_REFERENCE, BtId(13)],
"A buyer reference or purchase order reference MUST be provided.",
|inv, f| {
    // **A disjunction, not a mandatory term.** `cbc:BuyerReference or
    // cac:OrderReference/cbc:ID` — eight of CEN's published Peppol test
    // invoices carry only the order reference, and a flat "BT-10 is mandatory"
    // restriction rejects every one of them.
    let buyer_ref = inv.buyer_reference.as_deref().is_some_and(|r| !r.trim().is_empty());
    let order_ref = inv
        .purchase_order_reference
        .as_ref()
        .is_some_and(|r| !r.as_str().trim().is_empty());
    if !buyer_ref && !order_ref {
        f.at(Path::term(bt::BUYER_REFERENCE));
    }
});

rule!(R004, "PEPPOL-EN16931-R004", terms: [bt::SPECIFICATION_ID],
"Specification identifier MUST have the value \
 'urn:cen.eu:en16931:2017#compliant#urn:fdc:peppol.eu:2017:poacc:billing:3.0'.",
|inv, f| {
    // A **value** constraint, and a prefix one at that — the artefact uses
    // `starts-with`, so a CIUS of Peppol may append its own suffix. Modelling
    // this as "BT-24 is present" accepted any string whatsoever.
    if inv
        .specification_id
        .as_deref()
        .is_none_or(|s| !s.trim().starts_with(crate::profiles::PEPPOL_SPEC_ID))
    {
        f.at(Path::term(bt::SPECIFICATION_ID));
    }
});

rule!(R002, "PEPPOL-EN16931-R002", terms: [BtId(22)],
"No more than one note is allowed on document level, unless both the buyer and seller are German \
 organizations.",
|inv, f| {
    // The German exemption is real and in Peppol's own test: XRechnung needs
    // several document-level notes, so Peppol carves DE-DE out rather than
    // forcing German invoices into a single blob.
    if inv.notes.len() > 1 && !both_german(inv) {
        f.at(Path::term(BtId(22)));
    }
});

rule!(R005, "PEPPOL-EN16931-R005",
terms: [bt::VAT_ACCOUNTING_CURRENCY, bt::CURRENCY],
"VAT accounting currency code MUST be different from invoice currency code when provided.",
|inv, f| {
    if let (Some(tax), Some(doc)) = (&inv.vat_accounting_currency, &inv.currency)
        && tax == doc
    {
        // BT-6 exists to report VAT in a *second* currency. Equal to BT-5 it
        // says nothing and duplicates BT-110 into BT-111.
        f.at(Path::term(bt::VAT_ACCOUNTING_CURRENCY));
    }
});

rule!(R007, "PEPPOL-EN16931-R007", terms: [bt::BUSINESS_PROCESS],
"Business process MUST be in the format 'urn:fdc:peppol.eu:2017:poacc:billing:NN:1.0' where NN \
 indicates the process number.",
|inv, f| {
    if let Some(p) = inv.business_process.as_deref()
        && !p.is_empty()
        && peppol_process(p).is_none()
    {
        f.at(Path::term(bt::BUSINESS_PROCESS));
    }
});

/// The `NN` of `urn:fdc:peppol.eu:2017:poacc:billing:NN:1.0`, if it parses.
///
/// Peppol derives `$profile` from BT-23 and `P0100` / `P0101` then branch on it,
/// so this is shared rather than re-parsed per rule.
fn peppol_process(business_process: &str) -> Option<&str> {
    let rest = business_process.strip_prefix("urn:fdc:peppol.eu:2017:poacc:billing:")?;
    let (nn, tail) = rest.split_once(':')?;
    (tail == "1.0" && !nn.is_empty() && nn.bytes().all(|b| b.is_ascii_digit())).then_some(nn)
}

rule!(R010, "PEPPOL-EN16931-R010", terms: [bt::BUYER_ELECTRONIC_ADDRESS],
"Buyer electronic address MUST be provided.",
|inv, f| {
    if inv.buyer.electronic_address.is_none() {
        f.at(Path::term(bt::BUYER_ELECTRONIC_ADDRESS));
    }
});

rule!(R020, "PEPPOL-EN16931-R020", terms: [bt::SELLER_ELECTRONIC_ADDRESS],
"Seller electronic address MUST be provided.",
|inv, f| {
    if inv.seller.electronic_address.is_none() {
        f.at(Path::term(bt::SELLER_ELECTRONIC_ADDRESS));
    }
});

rule!(R055, "PEPPOL-EN16931-R055", terms: [bt::VAT_TOTAL, BtId(111)],
"Invoice total VAT amount and Invoice total VAT amount in accounting currency MUST have the same \
 operational sign.",
|inv, f| {
    if let (Some(a), Some(b)) = (inv.totals.vat_total, inv.totals.vat_total_accounting) {
        let sign = |v: crate::InvoiceAmount| v.into_decimal() > Decimal::ZERO;
        if !a.is_zero() && !b.is_zero() && sign(a) != sign(b) {
            f.at(Path::term(BtId(111)));
        }
    }
});

rule!(P0112, "PEPPOL-EN16931-P0112", terms: [bt::TYPE_CODE],
"Invoice type code 326 or 384 are only allowed when both buyer and seller are German organizations.",
|inv, f| {
    if inv
        .type_code
        .as_ref()
        .is_some_and(|c| matches!(c.as_str(), "326" | "384"))
        && !both_german(inv)
    {
        f.at(Path::term(bt::TYPE_CODE));
    }
});

// ── Line-level Peppol rules ───────────────────────────────────────────────────

rule!(R110, "PEPPOL-EN16931-R110", terms: [BtId(134), BtId(73)],
"Start date of line period MUST be within invoice period.",
|inv, f| {
    let Some(doc_start) = inv.invoicing_period.as_ref().and_then(|p| p.start) else {
        return;
    };
    for (i, line) in inv.lines.iter().enumerate() {
        if line
            .period
            .as_ref()
            .and_then(|p| p.start)
            .is_some_and(|s| s < doc_start)
        {
            f.at(Path::at_term(Group::Line, i, BtId(134)));
        }
    }
});

rule!(R111, "PEPPOL-EN16931-R111", terms: [BtId(135), BtId(74)],
"End date of line period MUST be within invoice period.",
|inv, f| {
    let Some(doc_end) = inv.invoicing_period.as_ref().and_then(|p| p.end) else {
        return;
    };
    for (i, line) in inv.lines.iter().enumerate() {
        if line
            .period
            .as_ref()
            .and_then(|p| p.end)
            .is_some_and(|e| e > doc_end)
        {
            f.at(Path::at_term(Group::Line, i, BtId(135)));
        }
    }
});

// ── Peppol's own code lists ───────────────────────────────────────────────────

rule!(CL001, "PEPPOL-EN16931-CL001", terms: [BtId(125)],
"Mime code must be according to subset of IANA code list.",
|inv, f| {
    for (i, doc) in inv.attachments.iter().enumerate() {
        if let Some(a) = &doc.attachment
            && !crate::codes::contains(lists::PEPPOL_MIME_CODES, a.mime_code())
        {
            f.at(Path::at_term(Group::Attachment, i, BtId(125)));
        }
    }
});

rule!(CL008, "PEPPOL-EN16931-CL008", terms: [bt::SELLER_ELECTRONIC_ADDRESS, bt::BUYER_ELECTRONIC_ADDRESS],
"Electronic address identifier scheme must be from the codelist \"Electronic Address Identifier \
 Scheme\".",
|inv, f| {
    // Strictly narrower than `BR-CL-25`: 94 of CEN's 104. Scheme `0219` passes
    // the core rule and fails this one, which is exactly what a CIUS is for.
    for (group, party) in [(Group::Seller, &inv.seller), (Group::Buyer, &inv.buyer)] {
        if party
            .electronic_address
            .as_ref()
            .and_then(crate::Identifier::scheme)
            .is_some_and(|s| !crate::codes::contains(lists::PEPPOL_EAS_SCHEMES, s))
        {
            f.at(Path::group(group));
        }
    }
});

// ── Peppol rules the model retires ────────────────────────────────────────────

/// A Peppol rule the model makes unrepresentable — same idea as
/// [`crate::validation::rules::structural`]'s `by_type!`, with Peppol's ids.
macro_rules! by_type {
    ($konst:ident, $id:literal, $text:literal, $why:literal) => {
        #[doc = $text]
        #[doc = ""]
        #[doc = $why]
        pub static $konst: Rule = Rule {
            id: RuleId::new($id),
            severity: Severity::Fatal,
            text: $text,
            terms: &[],
            source: Source::ArtefactOnly,
            eval: |_, _| {},
        };
    };
}

by_type!(
    F001,
    "PEPPOL-EN16931-F001",
    "A date MUST be formatted YYYY-MM-DD.",
    "`Date` is `{ year, month, day }` and parses only `YYYY-MM-DD`; a timestamp is rejected at \
     the boundary, not reported as a finding."
);
by_type!(
    R008,
    "PEPPOL-EN16931-R008",
    "Document MUST not contain empty elements.",
    "An element with no content is a syntax artefact. `Option::None` and an empty element are \
     the same value here."
);
by_type!(
    R043,
    "PEPPOL-EN16931-R043",
    "Allowance/charge ChargeIndicator value MUST equal 'true' or 'false'.",
    "Allowances and charges are separate `Vec`s, not one list discriminated by a string."
);
by_type!(
    R044,
    "PEPPOL-EN16931-R044",
    "Charge on price level is NOT allowed. Only value 'false' allowed.",
    "BG-29 carries `price_discount` (BT-147) and nothing else — a price-level *charge* has no \
     field to live in."
);
by_type!(
    R051,
    "PEPPOL-EN16931-R051",
    "All currencyID attributes must have the same value as the invoice currency code (BT-5), \
     except for the tax amount in accounting currency.",
    "Every amount is implicitly in BT-5; only BT-111 is in BT-6, and it has its own field. Same \
     disposition as `BR-CL-03`."
);
by_type!(
    CL007,
    "PEPPOL-EN16931-CL007",
    "Currency code must be according to ISO 4217:2005.",
    "The per-amount `@currencyID` does not exist in the model. BT-5 and BT-6 are checked by \
     `BR-CL-04` and `BR-CL-05`."
);
by_type!(
    R053,
    "PEPPOL-EN16931-R053",
    "Only one tax total with tax subtotals MUST be provided.",
    "`vat_breakdown` is one `Vec` of BG-23 groups; there is no second TaxTotal to provide."
);
by_type!(
    R054,
    "PEPPOL-EN16931-R054",
    "Only one tax total without tax subtotals MUST be provided when tax currency code is provided.",
    "BT-111 is one `Option` field, present or not — the UBL shape it constrains has no analogue."
);
by_type!(
    R080,
    "PEPPOL-EN16931-R080",
    "Only one project reference is allowed on document level.",
    "BT-11 is `Option<DocumentReference>`, so a second one cannot be expressed."
);
by_type!(
    R100,
    "PEPPOL-EN16931-R100",
    "Only one invoiced object is allowed pr line.",
    "BT-128 is `Option<Identifier>` on the line."
);
by_type!(
    R101,
    "PEPPOL-EN16931-R101",
    "Element Document reference can only be used for Invoice line object.",
    "The line's only document reference *is* BT-128; there is no untyped reference to misuse."
);

// ── The document type codes, per business process ─────────────────────────────

/// `P0100` — BT-3 on an **invoice**, under billing process `01`.
///
/// 26 of `BR-CL-01`'s 50, and **`389` is not among them**: self-billing is a
/// separate Peppol profile with its own customization identifier.
const P0100_CODES: &[&str] = &[
    "102", "218", "219", "326", "331", "380", "382", "383", "384", "386", "388", "393", "395",
    "553", "575", "623", "71", "780", "80", "817", "82", "84", "870", "875", "876", "877",
];

/// `P0101` — BT-3 on a **credit note**, under billing process `01`.
const P0101_CODES: &[&str] = &["381", "396", "532", "81", "83"];

rule!(P0100, "PEPPOL-EN16931-P0100", terms: [bt::TYPE_CODE],
"Invoice type code MUST be set according to the profile.",
|inv, f| {
    if is_billing_01(inv)
        && let Some(code) = inv.type_code.as_ref()
        // Which list applies is decided the way `BR-CL-01` decides it: by
        // whether the code is a credit-note code. UBL splits this into two
        // elements; a syntax-independent model has one term and one document.
        && !is_credit_note(code)
        && !crate::codes::contains(P0100_CODES, code.as_str())
    {
        f.at(Path::term(bt::TYPE_CODE));
    }
});

rule!(P0101, "PEPPOL-EN16931-P0101", terms: [bt::TYPE_CODE],
"Credit note type code MUST be set according to the profile.",
|inv, f| {
    if is_billing_01(inv)
        && let Some(code) = inv.type_code.as_ref()
        && is_credit_note(code)
        && !crate::codes::contains(P0101_CODES, code.as_str())
    {
        f.at(Path::term(bt::TYPE_CODE));
    }
});

/// Whether BT-23 names Peppol billing process `01`, which is what `P0100` and
/// `P0101` are conditional on. A document on another process is not constrained
/// by either.
fn is_billing_01(inv: &Invoice) -> bool {
    inv.business_process
        .as_deref()
        .and_then(peppol_process)
        .is_some_and(|nn| nn.trim_start_matches('0') == "1")
}

/// Whether BT-3 is a credit-note code, per `BR-CL-01`'s own split.
///
/// `81` is in **both** CEN lists, and in Peppol's credit-note list only — so it
/// is read as a credit note here, which is what `P0101` expects.
fn is_credit_note(code: &crate::invoice::Code) -> bool {
    code.is_in(lists::CREDIT_NOTE_TYPE_CODES)
}

// ── The three rules that mirror a CEN rule exactly ────────────────────────────

/// Peppol code-list rules whose list the generator **asserts** is identical to a
/// CEN table.
///
/// They are registered as rules of their own rather than folded into the CEN
/// ones, because a Peppol validator reports Peppol ids: a user fixing a document
/// against phive's output searches for `PEPPOL-EN16931-CL003`, and finding
/// nothing is worse than seeing it beside `BR-CL-20`.
macro_rules! mirror_rule {
    ($konst:ident, $id:literal, $mirrors:literal, $list:ident, $text:literal,
     |$inv:ident, $f:ident| $body:block) => {
        #[doc = $text]
        #[doc = ""]
        #[doc = concat!("Checks the same list as `", $mirrors, "`; `cargo xtask codegen` fails")]
        #[doc = "if the two ever diverge, so this doc cannot rot."]
        pub static $konst: Rule = Rule {
            id: RuleId::new($id),
            severity: Severity::Fatal,
            text: $text,
            terms: &[],
            source: Source::ArtefactOnly,
            eval: |$inv: &Invoice, $f: &mut Findings<'_>| $body,
        };
    };
}

mirror_rule!(
    CL002,
    "PEPPOL-EN16931-CL002",
    "BR-CL-19",
    ALLOWANCE_REASON_CODES,
    "Reason code MUST be according to subset of UNCL 5189 D.16B.",
    |inv, f| {
        for (i, a) in inv.allowances.iter().enumerate() {
            if a.reason_code
                .as_ref()
                .is_some_and(|c| !c.is_blank() && !c.is_in(lists::ALLOWANCE_REASON_CODES))
            {
                f.at(Path::at_term(Group::DocumentAllowance, i, BtId(98)));
            }
        }
        for (i, line) in inv.lines.iter().enumerate() {
            for a in &line.allowances {
                if a.reason_code
                    .as_ref()
                    .is_some_and(|c| !c.is_blank() && !c.is_in(lists::ALLOWANCE_REASON_CODES))
                {
                    f.at(Path::at_term(Group::Line, i, BtId(140)));
                }
            }
        }
    }
);

mirror_rule!(
    CL003,
    "PEPPOL-EN16931-CL003",
    "BR-CL-20",
    CHARGE_REASON_CODES,
    "Reason code MUST be according to UNCL 7161 D.16B.",
    |inv, f| {
        for (i, c) in inv.charges.iter().enumerate() {
            if c.reason_code
                .as_ref()
                .is_some_and(|c| !c.is_blank() && !c.is_in(lists::CHARGE_REASON_CODES))
            {
                f.at(Path::at_term(Group::DocumentCharge, i, BtId(105)));
            }
        }
        for (i, line) in inv.lines.iter().enumerate() {
            for c in &line.charges {
                if c.reason_code
                    .as_ref()
                    .is_some_and(|c| !c.is_blank() && !c.is_in(lists::CHARGE_REASON_CODES))
                {
                    f.at(Path::at_term(Group::Line, i, BtId(145)));
                }
            }
        }
    }
);

mirror_rule!(
    CL006,
    "PEPPOL-EN16931-CL006",
    "BR-CL-06",
    VAT_POINT_DATE_CODES,
    "Invoice period description code must be according to UNCL 2005 D.16B.",
    |inv, f| {
        if inv
            .vat_point_date_code
            .as_ref()
            .is_some_and(|c| !c.is_blank() && !c.is_in(lists::VAT_POINT_DATE_CODES))
        {
            f.at(Path::term(bt::VAT_POINT_DATE_CODE));
        }
    }
);

// ── What XRechnung actually merges ────────────────────────────────────────────

/// The **21** Peppol rules XRechnung's released Schematron merges in.
///
/// XRechnung 3.0.2 does not ship the file KoSIT keeps in source control. Its
/// build runs `peppol-into-xr.xsl` over `PEPPOL-EN16931-UBL.sch`, copying in
/// every assert whose id appears in `rule-list.xml` — 21 of Peppol's 46 — and
/// rewriting two of them on the way (see [`XR_R040`] and [`XR_R120`]).
///
/// # `rule-list.xml` is a whitelist with holes
///
/// Eleven entries are **commented out in the XML**, several with a reason:
///
/// ```xml
/// <!--
/// <r:rule>PEPPOL-EN16931-R002</r:rule>
/// <r:rule>PEPPOL-EN16931-R003</r:rule>
/// <r:rule>PEPPOL-EN16931-R004</r:rule>
/// -->
/// …
/// <!-- R051 will be included when Issue is solved, see …/pull/140 -->
/// ```
///
/// `R004` is the one that proves it: it requires BT-24 to start with Peppol's
/// customization identifier, and XRechnung's BT-24 is KoSIT's own. Were it
/// merged, **every XRechnung invoice would fail it**, including all of KoSIT's
/// own test instances.
///
/// A regex over this file matches inside the comments and reports 32. That is
/// how this list read 31 for a while, and it is the fourth time in this crate's
/// history that pattern-matching structured text has produced a confident wrong
/// answer. It is parsed as XML now, in `tests/codelists.rs`.
///
/// The **twenty-five it leaves out** include `CL001`…`CL008` (Peppol's own
/// narrower code lists), `P0100`/`P0101` (type codes — XRechnung has `BR-DE-17`),
/// `P0104`…`P0112`, and the reference rules `R002`/`R003`/`R004`.
///
/// This is the reverse of what a previous version of this crate concluded from
/// KoSIT's validator configuration. The configuration names two Schematrons,
/// CEN's and XRechnung's — and XRechnung's *already contains* Peppol's, because
/// the merge happens at build time. Reading the shipped configuration was right;
/// reading the source Schematron and stopping there was not.
pub const MERGED_INTO_XRECHNUNG: &[&str] = &[
    "PEPPOL-EN16931-R001",
    "PEPPOL-EN16931-R005",
    "PEPPOL-EN16931-R008",
    "PEPPOL-EN16931-R010",
    "PEPPOL-EN16931-R020",
    "PEPPOL-EN16931-R040",
    "PEPPOL-EN16931-R041",
    "PEPPOL-EN16931-R042",
    "PEPPOL-EN16931-R043",
    "PEPPOL-EN16931-R044",
    "PEPPOL-EN16931-R046",
    "PEPPOL-EN16931-R053",
    "PEPPOL-EN16931-R054",
    "PEPPOL-EN16931-R055",
    "PEPPOL-EN16931-R061",
    "PEPPOL-EN16931-R101",
    "PEPPOL-EN16931-R110",
    "PEPPOL-EN16931-R111",
    "PEPPOL-EN16931-R120",
    "PEPPOL-EN16931-R121",
    "PEPPOL-EN16931-R130",
];

/// `R040` as XRechnung merges it: the same rule, with the HUF slack.
pub static XR_R040: Rule = Rule {
    id: RuleId::new("PEPPOL-EN16931-R040"),
    severity: Severity::Fatal,
    text: R040.text,
    terms: R040.terms,
    source: Source::ArtefactOnly,
    eval: |inv, f| check_allowance_basis(inv, f, currency_slack),
};

/// `R120` as XRechnung merges it: **a warning**, and with the HUF slack.
///
/// `peppol-into-xr.xsl` rewrites the flag outright:
///
/// ```xslt
/// <xsl:when test="@id='PEPPOL-EN16931-R120'">
///   <xsl:attribute name="flag">warning</xsl:attribute>
/// ```
///
/// So a line whose net amount does not follow from its price **rejects** a
/// Peppol invoice and merely annotates an XRechnung one. Same id, same text,
/// different consequence — which is exactly why severity belongs to the rule
/// instance a profile holds and not to a global registry.
pub static XR_R120: Rule = Rule {
    id: RuleId::new("PEPPOL-EN16931-R120"),
    severity: Severity::Warning,
    text: R120.text,
    terms: R120.terms,
    source: Source::ArtefactOnly,
    eval: |inv, f| check_line_amount(inv, f, currency_slack),
};

/// The 31 merged rules, as XRechnung runs them.
///
/// [`XR_R040`] and [`XR_R120`] are XRechnung's rewritten instances; the other 29
/// are Peppol's own, unchanged. Listed explicitly rather than filtered, so it is
/// a `const` — and `the_merged_set_matches_the_rule_list` asserts it agrees with
/// [`MERGED_INTO_XRECHNUNG`], which is transcribed from KoSIT's `rule-list.xml`.
pub static FOR_XRECHNUNG: &[&Rule] = &[
    &R001, &R005, &R008, &R010, &R020, &XR_R040, &R041, &R042, &R043, &R044, //
    &R046, &R053, &R054, &R055, &R061, &R101, &R110, &R111, &XR_R120, &R121, &R130,
];

#[cfg(test)]
mod merge_tests {
    use super::*;

    /// [`FOR_XRECHNUNG`] and [`MERGED_INTO_XRECHNUNG`] must name the same rules.
    ///
    /// One is a list of `&Rule`s, the other a list of ids transcribed from
    /// KoSIT's `rule-list.xml`. Keeping them in step by hand is exactly the sort
    /// of thing that silently rots.
    #[test]
    fn the_merged_set_matches_the_rule_list() {
        let mut got: Vec<&str> = FOR_XRECHNUNG.iter().map(|r| r.id.as_str()).collect();
        got.sort_unstable();
        let mut want: Vec<&str> = MERGED_INTO_XRECHNUNG.to_vec();
        want.sort_unstable();
        assert_eq!(got, want);
    }

    /// The two rewritten instances differ from Peppol's in the ways the
    /// stylesheet says, and in no others.
    #[test]
    fn xrechnung_rewrites_exactly_two_rules() {
        assert_eq!(R120.severity, Severity::Fatal);
        assert_eq!(XR_R120.severity, Severity::Warning, "peppol-into-xr.xsl");
        assert_eq!(XR_R120.text, R120.text, "only the flag changes");
        assert_eq!(XR_R040.severity, R040.severity, "R040 keeps its flag");

        // HUF is the whole reason the second instance exists.
        let huf = Invoice {
            currency: Some(crate::invoice::Code::new("HUF")),
            ..Default::default()
        };
        assert_eq!(currency_slack(&huf), HUF_SLACK);
        assert_eq!(fixed_slack(&huf), SLACK, "Peppol does not widen for HUF");
        assert_eq!(currency_slack(&Invoice::default()), SLACK);
    }
}
