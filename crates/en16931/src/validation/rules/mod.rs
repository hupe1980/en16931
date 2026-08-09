//! The EN 16931 core rule set.
//!
//! # The three tolerance regimes
//!
//! This is the detail that is impossible to get right from the standard's prose
//! alone, and where a hand-written engine most easily diverges from the
//! Schematron everyone else runs:
//!
//! | Regime | Rules | Tolerance | Whose |
//! |---|---|---|---|
//! | Totals chain | `BR-CO-10` … `BR-CO-16` | **exact** | CEN |
//! | VAT derivation | `BR-CO-17`, `BR-S-08/09`, `BR-AF/AG-08/09` | **±1.00**, on absolute values | CEN artefacts |
//! | Line & allowance derivation | `PEPPOL-EN16931-R120`, `-R040` | **±0.02** | Peppol |
//!
//! They are not interchangeable. Checking the VAT derivation exactly rejects
//! invoices every real validator accepts; applying its tolerance to `BR-CO-14`
//! accepts invoices every real validator rejects.
//!
//! **None of the tolerance is in the standard.** EN 16931-1 §6.4.2 states
//! `BR-CO-17` as a plain equation — *"= BT-116 x (BT-119 / 100), rounded to two
//! decimals"* — with no slack at all. The ±1.00 is an artefact decision and the
//! ±0.02 a Peppol one, which is why [`Source`] is recorded per rule.

use rust_decimal::Decimal;

pub mod category;
pub mod peppol;
pub mod structural;
pub mod xrechnung;

use super::{Findings, Rule, RuleId, Severity, Source};
use crate::bt::{BtId, Group, Path};
use crate::codes::generated as lists;
use crate::invoice::{Invoice, terms as bt};
use crate::{InvoiceAmount, VatCategory};

/// The ±1.00 the artefacts allow on the VAT derivation family.
const VAT_TOLERANCE: Decimal = Decimal::ONE;

/// Declare a rule with its metadata beside its predicate.
macro_rules! rule {
    (
        $konst:ident, $id:literal, $sev:ident, $src:ident,
        terms: [$($t:expr),* $(,)?],
        $text:literal,
        |$inv:ident, $f:ident| $body:block
    ) => {
        #[doc = $text]
        pub static $konst: Rule = Rule {
            id: RuleId::new($id),
            severity: Severity::$sev,
            text: $text,
            terms: &[$($t),*],
            source: Source::$src,
            eval: |$inv: &Invoice, $f: &mut Findings<'_>| $body,
        };
    };
}

// ── Presence: BR-01 … BR-16 ───────────────────────────────────────────────────

rule!(BR_01, "BR-01", Fatal, Both, terms: [bt::SPECIFICATION_ID],
"An Invoice shall have a Specification identifier (BT-24).",
|inv, f| {
    if inv.specification_id.as_deref().is_none_or(str::is_empty) {
        f.at(Path::term(bt::SPECIFICATION_ID));
    }
});

rule!(BR_02, "BR-02", Fatal, Both, terms: [bt::NUMBER],
"An Invoice shall have an Invoice number (BT-1).",
|inv, f| {
    if inv.number.as_deref().is_none_or(str::is_empty) {
        f.at(Path::term(bt::NUMBER));
    }
});

rule!(BR_03, "BR-03", Fatal, Both, terms: [bt::ISSUE_DATE],
"An Invoice shall have an Invoice issue date (BT-2).",
|inv, f| {
    if inv.issue_date.is_none() {
        f.at(Path::term(bt::ISSUE_DATE));
    }
});

rule!(BR_04, "BR-04", Fatal, Both, terms: [bt::TYPE_CODE],
"An Invoice shall have an Invoice type code (BT-3).",
|inv, f| {
    if inv.type_code.as_ref().is_none_or(crate::invoice::Code::is_blank) {
        f.at(Path::term(bt::TYPE_CODE));
    }
});

rule!(BR_05, "BR-05", Fatal, Both, terms: [bt::CURRENCY],
"An Invoice shall have an Invoice currency code (BT-5).",
|inv, f| {
    if inv.currency.as_ref().is_none_or(crate::invoice::Code::is_blank) {
        f.at(Path::term(bt::CURRENCY));
    }
});

rule!(BR_06, "BR-06", Fatal, Both, terms: [bt::SELLER_NAME],
"An Invoice shall contain the Seller name (BT-27).",
|inv, f| {
    if inv.seller.name.as_deref().is_none_or(str::is_empty) {
        f.at(Path::group_term(Group::Seller, bt::SELLER_NAME));
    }
});

rule!(BR_07, "BR-07", Fatal, Both, terms: [bt::BUYER_NAME],
"An Invoice shall contain the Buyer name (BT-44).",
|inv, f| {
    if inv.buyer.name.as_deref().is_none_or(str::is_empty) {
        f.at(Path::group_term(Group::Buyer, bt::BUYER_NAME));
    }
});

rule!(BR_09, "BR-09", Fatal, Both, terms: [bt::SELLER_COUNTRY],
"The Seller postal address (BG-5) shall contain a Seller country code (BT-40).",
|inv, f| {
    if inv.seller.address.country.as_ref().is_none_or(crate::invoice::Code::is_blank) {
        f.at(Path::group_term(Group::Seller, bt::SELLER_COUNTRY));
    }
});

rule!(BR_11, "BR-11", Fatal, Both, terms: [bt::BUYER_COUNTRY],
"The Buyer postal address shall contain a Buyer country code (BT-55).",
|inv, f| {
    if inv.buyer.address.country.as_ref().is_none_or(crate::invoice::Code::is_blank) {
        f.at(Path::group_term(Group::Buyer, bt::BUYER_COUNTRY));
    }
});

rule!(BR_16, "BR-16", Fatal, Both, terms: [],
"An Invoice shall have at least one Invoice line (BG-25).",
|inv, f| {
    if inv.lines.is_empty() {
        f.at(Path::group(Group::Line));
    }
});

// ── Invoice line: BR-21 … BR-27, BR-CO-04 ─────────────────────────────────────

rule!(BR_21, "BR-21", Fatal, Both, terms: [bt::LINE_ID],
"Each Invoice line (BG-25) shall have an Invoice line identifier (BT-126).",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        if line.id.trim().is_empty() {
            f.at(Path::at_term(Group::Line, i, bt::LINE_ID));
        }
    }
});

rule!(BR_23, "BR-23", Fatal, Both, terms: [bt::LINE_UNIT_CODE],
"An Invoice line (BG-25) shall have an Invoiced quantity unit of measure code (BT-130).",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        if line.unit_code.is_blank() {
            f.at(Path::at_term(Group::Line, i, bt::LINE_UNIT_CODE));
        }
    }
});

rule!(BR_25, "BR-25", Fatal, Both, terms: [bt::ITEM_NAME],
"Each Invoice line (BG-25) shall contain the Item name (BT-153).",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        if line.item.name.as_deref().is_none_or(str::is_empty) {
            f.at(Path::at_term(Group::Line, i, bt::ITEM_NAME));
        }
    }
});

rule!(BR_27, "BR-27", Fatal, Both, terms: [bt::ITEM_NET_PRICE],
"The Item net price (BT-146) shall NOT be negative.",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        if line.price.net_price.is_negative() {
            f.at(Path::at_term(Group::Line, i, bt::ITEM_NET_PRICE));
        }
    }
});

rule!(BR_28, "BR-28", Fatal, Both, terms: [bt::ITEM_GROSS_PRICE],
"The Item gross price (BT-148) shall NOT be negative.",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        if line.price.gross_price.is_some_and(|p| p.is_negative()) {
            f.at(Path::at_term(Group::Line, i, bt::ITEM_GROSS_PRICE));
        }
    }
});

rule!(BR_CO_04, "BR-CO-04", Fatal, Both, terms: [bt::LINE_VAT_CATEGORY],
"Each Invoice line (BG-25) shall be categorized with an Invoiced item VAT category code (BT-151).",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        if line.vat.category.is_blank() {
            f.at(Path::at_term(Group::Line, i, bt::LINE_VAT_CATEGORY));
        }
    }
});

// ── Periods: BR-29, BR-30, BR-CO-19, BR-CO-20 ─────────────────────────────────

rule!(BR_29, "BR-29", Fatal, Both, terms: [bt::PERIOD_START, bt::PERIOD_END],
"If both Invoicing period start date (BT-73) and Invoicing period end date (BT-74) are given \
 then the Invoicing period end date (BT-74) shall be later or equal to the Invoicing period \
 start date (BT-73).",
|inv, f| {
    if inv.invoicing_period.as_ref().and_then(crate::invoice::Period::is_ordered) == Some(false) {
        f.at(Path::term(bt::PERIOD_END));
    }
});

rule!(BR_30, "BR-30", Fatal, Both, terms: [bt::LINE_PERIOD_START, bt::LINE_PERIOD_END],
"If both Invoice line period start date (BT-134) and Invoice line period end date (BT-135) \
 are given then the Invoice line period end date (BT-135) shall be later or equal to the \
 Invoice line period start date (BT-134).",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        if line.period.as_ref().and_then(crate::invoice::Period::is_ordered) == Some(false) {
            f.at(Path::at_term(Group::Line, i, bt::LINE_PERIOD_END));
        }
    }
});

rule!(BR_CO_19, "BR-CO-19", Fatal, Both, terms: [bt::PERIOD_START, bt::PERIOD_END],
"If Invoicing period (BG-14) is used, the Invoicing period start date (BT-73) or the \
 Invoicing period end date (BT-74) shall be filled, or both.",
|inv, f| {
    if let Some(p) = &inv.invoicing_period
        && p.start.is_none()
        && p.end.is_none()
    {
        f.at(Path::term(bt::PERIOD_START));
    }
});

// ── VAT breakdown presence: BR-45 … BR-48, BR-CO-18 ───────────────────────────

rule!(BR_47, "BR-47", Fatal, Both, terms: [bt::VAT_CATEGORY],
"Each VAT breakdown (BG-23) shall be defined through a VAT category code (BT-118).",
|inv, f| {
    for (i, e) in inv.vat_breakdown.iter().enumerate() {
        if e.category.is_blank() {
            f.at(Path::at_term(Group::VatBreakdown, i, bt::VAT_CATEGORY));
        }
    }
});

rule!(BR_48, "BR-48", Fatal, Both, terms: [bt::VAT_RATE],
"Each VAT breakdown (BG-23) shall have a VAT category rate (BT-119), except if the Invoice \
 is not subject to VAT.",
|inv, f| {
    for (i, e) in inv.vat_breakdown.iter().enumerate() {
        // "not subject to VAT" is category O — and note this is BT-119, a
        // different term from BT-152. XRechnung's BR-DE-14 requires BT-119
        // unconditionally, which is a profile rule, not this one.
        let exempt_from_the_requirement = e.semantics() == Some(VatCategory::OutOfScope);
        if e.rate.is_none() && !exempt_from_the_requirement {
            f.at(Path::at_term(Group::VatBreakdown, i, bt::VAT_RATE));
        }
    }
});

rule!(BR_CO_18, "BR-CO-18", Fatal, Both, terms: [],
"An Invoice shall at least have one VAT breakdown group (BG-23).",
|inv, f| {
    if inv.vat_breakdown.is_empty() {
        f.at(Path::group(Group::VatBreakdown));
    }
});

// ── The totals chain — EXACT ──────────────────────────────────────────────────

/// Sum, reporting overflow as a finding rather than panicking.
fn sum(amounts: impl IntoIterator<Item = InvoiceAmount>) -> Option<InvoiceAmount> {
    InvoiceAmount::checked_sum(amounts).ok()
}

rule!(BR_CO_10, "BR-CO-10", Fatal, Both, terms: [bt::LINE_TOTAL, bt::LINE_NET_AMOUNT],
"Sum of Invoice line net amount (BT-106) = Σ Invoice line net amount (BT-131).",
|inv, f| {
    let Some(expected) = sum(inv.lines.iter().map(|l| l.net_amount)) else { return };
    if expected != inv.totals.line_total {
        f.arithmetic(
            Path::group_term(Group::Totals, bt::LINE_TOTAL),
            expected,
            inv.totals.line_total,
        );
    }
});

rule!(BR_CO_11, "BR-CO-11", Fatal, Both, terms: [bt::ALLOWANCE_TOTAL, bt::ALLOWANCE_AMOUNT],
"Sum of allowances on document level (BT-107) = Σ Document level allowance amount (BT-92).",
|inv, f| {
    let path = Path::group_term(Group::Totals, bt::ALLOWANCE_TOTAL);
    let Some(expected) = sum(inv.allowances.iter().map(|a| a.amount)) else { return };
    match inv.totals.allowance_total {
        Some(stated) if stated != expected => f.arithmetic(path, expected, stated),
        // Absent is not zero: BT-107 may be omitted only when there are no
        // allowances at all.
        None if !inv.allowances.is_empty() => f.arithmetic(path, expected, "absent"),
        _ => {}
    }
});

rule!(BR_CO_12, "BR-CO-12", Fatal, Both, terms: [bt::CHARGE_TOTAL, bt::CHARGE_AMOUNT],
"Sum of charges on document level (BT-108) = Σ Document level charge amount (BT-99).",
|inv, f| {
    let path = Path::group_term(Group::Totals, bt::CHARGE_TOTAL);
    let Some(expected) = sum(inv.charges.iter().map(|c| c.amount)) else { return };
    match inv.totals.charge_total {
        Some(stated) if stated != expected => f.arithmetic(path, expected, stated),
        None if !inv.charges.is_empty() => f.arithmetic(path, expected, "absent"),
        _ => {}
    }
});

rule!(BR_CO_13, "BR-CO-13", Fatal, Both,
terms: [bt::TAXABLE_TOTAL, bt::LINE_TOTAL, bt::ALLOWANCE_TOTAL, bt::CHARGE_TOTAL],
"Invoice total amount without VAT (BT-109) = Σ Invoice line net amount (BT-131) - Sum of \
 allowances on document level (BT-107) + Sum of charges on document level (BT-108).",
|inv, f| {
    let t = &inv.totals;
    let allowances = t.allowance_total.unwrap_or(InvoiceAmount::ZERO);
    let charges = t.charge_total.unwrap_or(InvoiceAmount::ZERO);
    let Ok(expected) = t
        .line_total
        .checked_sub(allowances)
        .and_then(|v| v.checked_add(charges))
    else { return };
    if expected != t.taxable_total {
        f.arithmetic(
            Path::group_term(Group::Totals, bt::TAXABLE_TOTAL),
            expected,
            t.taxable_total,
        );
    }
});

rule!(BR_CO_14, "BR-CO-14", Fatal, Both, terms: [bt::VAT_TOTAL, bt::VAT_TAX_AMOUNT],
"Invoice total VAT amount (BT-110) = Σ VAT category tax amount (BT-117).",
|inv, f| {
    let path = Path::group_term(Group::Totals, bt::VAT_TOTAL);
    let Some(expected) = sum(inv.vat_breakdown.iter().map(|e| e.tax_amount)) else { return };
    match inv.totals.vat_total {
        // EXACT. Applying the VAT-derivation tolerance here would accept
        // invoices every validator rejects.
        Some(stated) if stated != expected => f.arithmetic(path, expected, stated),
        None if !expected.is_zero() => f.arithmetic(path, expected, "absent"),
        _ => {}
    }
});

rule!(BR_CO_15, "BR-CO-15", Fatal, Both, terms: [bt::GROSS_TOTAL, bt::TAXABLE_TOTAL, bt::VAT_TOTAL],
"Invoice total amount with VAT (BT-112) = Invoice total amount without VAT (BT-109) + \
 Invoice total VAT amount (BT-110).",
|inv, f| {
    let t = &inv.totals;
    let vat = t.vat_total.unwrap_or(InvoiceAmount::ZERO);
    let Ok(expected) = t.taxable_total.checked_add(vat) else { return };
    if expected != t.gross_total {
        f.arithmetic(
            Path::group_term(Group::Totals, bt::GROSS_TOTAL),
            expected,
            t.gross_total,
        );
    }
});

rule!(BR_CO_16, "BR-CO-16", Fatal, Both, terms: [bt::DUE, bt::GROSS_TOTAL, bt::PAID, bt::ROUNDING],
"Amount due for payment (BT-115) = Invoice total amount with VAT (BT-112) -Paid amount \
 (BT-113) +Rounding amount (BT-114).",
|inv, f| {
    let t = &inv.totals;
    let paid = t.paid.unwrap_or(InvoiceAmount::ZERO);
    let rounding = t.rounding.unwrap_or(InvoiceAmount::ZERO);
    let Ok(expected) = t
        .gross_total
        .checked_sub(paid)
        .and_then(|v| v.checked_add(rounding))
    else { return };
    if expected != t.due {
        f.arithmetic(Path::group_term(Group::Totals, bt::DUE), expected, t.due);
    }
});

// ── The VAT derivation — ±1.00 ON ABSOLUTE VALUES ─────────────────────────────

rule!(BR_CO_17, "BR-CO-17", Fatal, Both, terms: [bt::VAT_TAX_AMOUNT, bt::VAT_TAXABLE_AMOUNT, bt::VAT_RATE],
"VAT category tax amount (BT-117) = VAT category taxable amount (BT-116) x (VAT category \
 rate (BT-119) / 100), rounded to two decimals.",
|inv, f| {
    for (i, e) in inv.vat_breakdown.iter().enumerate() {
        let path = Path::at_term(Group::VatBreakdown, i, bt::VAT_TAX_AMOUNT);
        let rate = e.rate.map_or(Decimal::ZERO, |r| r.into_decimal());

        // The artefact's zero-rate branch: if the rate rounds to zero, the
        // tax must round to zero — checked WITHOUT tolerance.
        if rate.round() == Decimal::ZERO {
            if e.tax_amount.into_decimal().round() != Decimal::ZERO {
                f.arithmetic(path, "0", e.tax_amount);
            }
            continue;
        }

        // `abs()` on BOTH sides, which is what lets a credit note pass, then
        // a full currency unit of slack. Not in the standard — see the
        // module docs — but it is what every validator runs.
        let base = e.taxable_amount.into_decimal().abs();
        // `continue`, not `return`: one group whose product overflows must not
        // silence the rule for every group after it.
        let Some(exact) = base.checked_mul(rate).map(|v| v / Decimal::ONE_HUNDRED) else {
            continue;
        };
        let expected = exact.round_dp(2);
        let stated = e.tax_amount.into_decimal().abs();
        if (stated - expected).abs() >= VAT_TOLERANCE {
            f.arithmetic(path, expected, e.tax_amount);
        }
    }
});

// ── Category rules ────────────────────────────────────────────────────────────

// ── Code lists ────────────────────────────────────────────────────────────────

rule!(BR_CL_01, "BR-CL-01", Fatal, ArtefactOnly, terms: [bt::TYPE_CODE],
"The document type code MUST be coded by the invoice and credit note related code lists of \
 UNTDID 1001.",
|inv, f| {
    // Two lists, selected by the document's kind: 50 codes for an invoice, 13
    // for a credit note, overlapping only in `81`. The artefact expresses this
    // as a `self::` disjunction over the two UBL elements; `DocumentKind` is
    // the same distinction without the syntax, so the check is **exact** — an
    // invoice carrying `381` is invalid here, as it is in UBL.
    let list = match inv.kind {
        crate::invoice::DocumentKind::Invoice => lists::INVOICE_TYPE_CODES,
        crate::invoice::DocumentKind::CreditNote => lists::CREDIT_NOTE_TYPE_CODES,
    };
    if let Some(code) = &inv.type_code
        && !code.is_blank()
        && !code.is_in(list)
    {
        f.at(Path::term(bt::TYPE_CODE));
    }
});

rule!(BR_CL_04, "BR-CL-04", Fatal, ArtefactOnly, terms: [bt::CURRENCY],
"Invoice currency code MUST be coded using ISO code list 4217 alpha-3.",
|inv, f| {
    if let Some(c) = &inv.currency
        && !c.is_blank()
        && !c.is_in(lists::CURRENCY_CODES)
    {
        f.at(Path::term(bt::CURRENCY));
    }
});

rule!(BR_CL_17, "BR-CL-17", Fatal, ArtefactOnly, terms: [bt::VAT_CATEGORY],
"Invoice tax categories MUST be coded using UNCL5305 code list.",
|inv, f| {
    for (i, e) in inv.vat_breakdown.iter().enumerate() {
        if !e.category.is_blank() && !e.category.is_in(lists::VAT_CATEGORY_CODES) {
            f.at(Path::at_term(Group::VatBreakdown, i, bt::VAT_CATEGORY));
        }
    }
});

rule!(BR_CL_18, "BR-CL-18", Fatal, ArtefactOnly, terms: [bt::LINE_VAT_CATEGORY],
"Invoice tax categories MUST be coded using UNCL5305 code list.",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        if !line.vat.category.is_blank() && !line.vat.category.is_in(lists::VAT_CATEGORY_CODES) {
            f.at(Path::at_term(Group::Line, i, bt::LINE_VAT_CATEGORY));
        }
    }
});

rule!(BR_CL_23, "BR-CL-23", Fatal, ArtefactOnly, terms: [bt::LINE_UNIT_CODE],
"Unit code MUST be coded according to the UN/ECE Recommendation 20 with Rec 21 extension.",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        if !line.unit_code.is_blank() && !line.unit_code.is_in(lists::UNIT_CODES) {
            // `"kwh"` for `KWH` is the single most common unit-code bug, and a
            // list of 2 162 values is not something a reader greps by eye.
            f.at_maybe_hinted(
                Path::at_term(Group::Line, i, bt::LINE_UNIT_CODE),
                crate::codes::guard::UNIT.advice(line.unit_code.as_str()),
            );
        }
    }
});

// ── Normative in the standard, absent from the artefacts ──────────────────────

rule!(BR_CO_25, "BR-CO-25", Fatal, StandardOnly, terms: [bt::DUE, bt::DUE_DATE, bt::PAYMENT_TERMS],
"In case the Amount due for payment (BT-115) is positive, either the Payment due date (BT-9) \
 or the Payment terms (BT-20) shall be present.",
|inv, f| {
    // **Invoices only.** The artefact binds this to `ubl-invoice:Invoice`, and
    // CEN's conformance suite spells it out with six cases titled *"Verify that
    // rule does not fire on Credit Notes"* — including one with a positive
    // payable amount. A credit note states what is owed *back*, so a due date
    // for paying it is not the sender's to give.
    if is_credit_note(inv) {
        return;
    }
    if inv.totals.due.is_positive()
        && inv.due_date.is_none()
        && inv.payment_terms.as_deref().is_none_or(str::is_empty)
    {
        // The rule names both terms; the hint names the two *methods*, because
        // an engine that computes an amount and leaves terms to an ERP reads
        // "shall be present" and has no idea which of the two it is expected to
        // fill in, nor that BT-20 is free text it may simply state.
        f.at_with_hint(
            Path::term(bt::DUE_DATE),
            "set BT-9 with `InvoiceBuilder::due_date`, or state BT-20 as free text with \
             `InvoiceBuilder::payment_terms(\"Zahlbar innerhalb 14 Tagen ohne Abzug\")` — \
             either one satisfies this, and neither is computed from the amount",
        );
    }
});

/// Whether this document is a credit note.
///
/// Reads [`crate::invoice::DocumentKind`], not BT-3: CEN's conformance suite
/// has credit-note cases with no BT-3 at all, and `BR-CO-25` must not fire on
/// them.
pub(crate) fn is_credit_note(inv: &Invoice) -> bool {
    inv.kind == crate::invoice::DocumentKind::CreditNote
}

// ── Ours, namespaced so it can never be mistaken for CEN's ────────────────────

rule!(EN_CURRENCY_01, "EN-CURRENCY-01", Fatal, Crate, terms: [bt::CURRENCY],
"The Invoice currency code (BT-5) shall not be XXX. ISO 4217 XXX means \"no currency \
 involved\", and BR-CL-04 accepts it because it is a real ISO code — so a document that was \
 never configured validates as an invoice denominated in nothing.",
|inv, f| {
    if inv.currency.as_ref().is_some_and(|c| c.as_str() == "XXX") {
        f.at(Path::term(bt::CURRENCY));
    }
});

rule!(EN_EXT_01, "EN-EXT-01", Warning, Crate, terms: [bt::PAID],
"This invoice carries extension data that the target profile cannot represent. Core \
 EN 16931 has nowhere to put the tax contained in an advance payment (BT-113 is a single \
 flat figure), so emitting it against a profile without ZUGFeRD EXTENDED's BG-X-46 silently \
 drops it. In Germany that is a §14c Abs. 1 UStG liability: the advance-related tax becomes \
 payable a second time. Either target a profile that carries it, or bill the residual \
 instead and list no advances.",
|inv, f| {
    // Core has no extension support at all, so any populated group warns.
    // A profile-aware run narrows this — see `Profile::validate`.
    if !inv.extensions.is_empty() {
        f.at(Path::term(bt::PAID));
    }
});

rule!(EN_EXT_02, "EN-EXT-02", Fatal, Crate, terms: [bt::LINE_ID],
"A SUB INVOICE LINE group (BG-DEX-01) is attached to an Invoice line (BG-25) that does not \
 exist. Sub-lines are keyed by the zero-based position of the line they hang beneath, so an \
 index past the end of BG-25 names nothing: the group is invisible to BR-DEX-02 and BR-DEX-03, \
 and no writer will emit it. Re-key it, or remove it.",
|inv, f| {
    // **This crate's own**, because the hazard is this crate's own modelling
    // choice: `Extensions::sub_invoice_lines` is keyed by line *index*, so a
    // caller that removes or reorders a line leaves the key pointing at the
    // wrong one — or at nothing.
    //
    // Nothing else notices. `BR-DEX-02` and `BR-DEX-03` iterate the lines and
    // ask for each one's sub-lines, so an out-of-range group is never visited;
    // the writer does the same, so it is never emitted. Data that validates
    // clean and does not survive being written down is the failure mode this
    // whole crate is built against, so it is a finding.
    for (index, subs) in &inv.extensions.sub_invoice_lines {
        if !subs.is_empty() && *index >= inv.lines.len() {
            f.arithmetic(
                Path::group(Group::Line),
                format!("an index below {}", inv.lines.len()),
                index,
            );
        }
    }
});

// ── The registry ──────────────────────────────────────────────────────────────

/// Rules that are not part of a VAT category family.
static GENERAL: &[&Rule] = &[
    &BR_01,
    &BR_02,
    &BR_03,
    &BR_04,
    &BR_05,
    &BR_06,
    &BR_07,
    &BR_09,
    &BR_11,
    &BR_16,
    &BR_21,
    &BR_23,
    &BR_25,
    &BR_27,
    &BR_28,
    &BR_29,
    &BR_30,
    &BR_47,
    &BR_48,
    &BR_CL_01,
    &BR_CL_04,
    &BR_CL_17,
    &BR_CL_18,
    &BR_CL_23,
    &BR_CO_04,
    &BR_CO_10,
    &BR_CO_11,
    &BR_CO_12,
    &BR_CO_13,
    &BR_CO_14,
    &BR_CO_15,
    &BR_CO_16,
    &BR_CO_17,
    &BR_CO_18,
    &BR_CO_19,
    &BR_CO_25,
    &EN_CURRENCY_01,
    &EN_EXT_01,
    &EN_EXT_02,
];

/// The EN 16931 core rule set — **all 223 syntax-independent ids** of the
/// pinned CEN artefacts, plus this crate's own `EN-*` rules.
///
/// Complete, and measured rather than claimed:
/// `the_registry_covers_the_syntax_independent_artefacts` in `tests/codelists.rs`
/// diffs this list against the artefacts on any machine that has them, and CI
/// runs it with `EN16931_REQUIRE_SPEC=1` so it cannot skip.
///
/// "Complete" does not mean every rule has a predicate: 53 are retired by the
/// model's types and four are undecidable — CEN's own binding for them is
/// `value="true()"`. Both kinds are registered so [`explain`] resolves them and
/// a report can state they were checked. See
/// [`structural`] for the list and the reasoning.
///
/// [`crate::validation::ValidationReport::rules_checked`] reports what a given
/// run covered, which for a profile is this set plus its own.
pub static CORE: std::sync::LazyLock<Vec<&'static Rule>> = std::sync::LazyLock::new(|| {
    GENERAL
        .iter()
        .copied()
        .chain(category::ALL.iter().copied())
        .chain(structural::ALL.iter().copied())
        .chain(structural::STRUCTURAL_BY_TYPE.iter().copied())
        .chain(structural::DECIMALS.iter().copied())
        .collect()
});

/// Every rule this crate can report, core **and** profile.
///
/// `CORE` alone is not that set: `PEPPOL-EN16931-R120`, `BR-DE-16` and
/// `BR-DEX-09` all live in a profile's `extra_rules` and all appear in reports.
pub fn all() -> impl Iterator<Item = &'static Rule> {
    CORE.iter().copied().chain(
        crate::profiles::ALL
            .iter()
            .flat_map(|p| p.extra_rules.iter().copied()),
    )
}

/// Look a rule up by any of its spellings — `BR-CO-3`, `BR-CO-03`, `br-co-3`.
///
/// Searches **every** rule the crate ships, not just the core set. It used to
/// search `CORE` only, which meant a user holding a perfectly ordinary
/// XRechnung report — `BR-DE-16`, `PEPPOL-EN16931-R120`, `BR-DEX-09` — got
/// `None` for every id in it. A registry that cannot explain its own findings is
/// not a registry.
///
/// Restrictions are not rules and are not found here; see
/// [`explain_restriction`].
#[must_use]
pub fn explain(query: &str) -> Option<&'static Rule> {
    all().find(|r| r.id.matches(query))
}

/// Look up a profile **restriction** by its published id — `BR-DE-3`.
///
/// The companion to [`explain`]. Restrictions are data rather than predicates
/// (see [`crate::validation::profile::Restriction`]), so they have no `Rule` to
/// return — but they do appear in reports under their real ids, and a user
/// holding one deserves an answer.
#[must_use]
pub fn explain_restriction(
    query: &str,
) -> Option<(
    &'static crate::Profile,
    &'static crate::validation::profile::Restriction,
)> {
    crate::profiles::ALL.iter().find_map(|p| {
        p.restrictions
            .iter()
            .find(|r| crate::validation::RuleId::new(r.id()).matches(query))
            .map(|r| (*p, r))
    })
}

/// Every rule that touches `term`, across the core set and every profile.
pub fn touching(term: BtId) -> impl Iterator<Item = &'static Rule> {
    all().filter(move |r| r.terms.contains(&term))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_has_no_duplicate_ids() {
        let mut ids: Vec<_> = CORE.iter().map(|r| r.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate rule id in CORE");
    }

    #[test]
    fn explain_accepts_every_spelling() {
        assert!(explain("BR-CO-14").is_some());
        assert!(explain("br-co-14").is_some());

        assert_eq!(explain("BR-CO-04").map(|r| r.id.as_str()), Some("BR-CO-04"));
        assert!(explain("nonsense").is_none());
    }

    #[test]
    fn touching_finds_the_arithmetic_rules_for_a_term() {
        let rules: Vec<_> = touching(bt::VAT_TAX_AMOUNT)
            .map(|r| r.id.as_str())
            .collect();
        assert!(rules.contains(&"BR-CO-14"), "{rules:?}");
        assert!(rules.contains(&"BR-CO-17"), "{rules:?}");
    }

    #[test]
    fn our_own_rules_are_namespaced_and_marked() {
        for r in CORE.iter() {
            if r.source == Source::Crate {
                assert!(
                    r.id.as_str().starts_with("EN-"),
                    "{} is ours but not namespaced",
                    r.id
                );
            } else {
                assert!(r.id.as_str().starts_with("BR-"), "{}", r.id);
            }
        }
    }

    #[test]
    fn exactly_one_rule_is_standard_only() {
        let only: Vec<_> = CORE
            .iter()
            .filter(|r| r.source == Source::StandardOnly)
            .map(|r| r.id.as_str())
            .collect();
        assert_eq!(
            only,
            ["BR-CO-25"],
            "BR-CO-27 does not exist in this edition"
        );
    }
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    /// The registry's actual size and shape.
    ///
    /// The *authoritative* completeness check is
    /// `the_registry_covers_the_syntax_independent_artefacts` in
    /// `tests/codelists.rs`, which diffs this list against CEN's artefacts. This
    /// one is its offline stand-in: it runs without `spec/`, so a contributor
    /// who deletes a family notices before CI does.
    ///
    /// Both counts are **exact**. They were floors — `>= 45` and `>= 85` against
    /// an actual 96 and 225 — which is a test that cannot fail for any change
    /// short of deleting a third of the registry.
    #[test]
    fn coverage_is_what_we_say_it_is() {
        let ids: Vec<&str> = CORE.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids.len(), CORE.len());

        // Nine categories × the five rows whose ids end in these, plus BR-B-01,
        // BR-O-11 and BR-O-14, and the `-01`/`-05` of nothing else.
        let family_rows = ids
            .iter()
            .filter(|id| {
                ["-01", "-05", "-08", "-09", "-10"]
                    .iter()
                    .any(|s| id.ends_with(s))
            })
            .count();
        assert_eq!(family_rows, 65, "nine families of five rows, plus the rest");

        assert_eq!(CORE.len(), 227, "the core registry changed size");
    }

    /// Every source classification is represented, and ours are namespaced.
    #[test]
    fn provenance_is_recorded_for_every_rule() {
        let mut standard_only = 0;
        let mut crate_only = 0;
        for r in CORE.iter() {
            match r.source {
                Source::StandardOnly => standard_only += 1,
                Source::Crate => {
                    crate_only += 1;
                    assert!(r.id.as_str().starts_with("EN-"), "{}", r.id);
                }
                _ => assert!(r.id.as_str().starts_with("BR-"), "{}", r.id),
            }
        }
        // Diffing EN 16931-1 against the artefacts yields exactly one rule the
        // standard states and the artefacts do not ship.
        assert_eq!(standard_only, 1, "BR-CO-25 is the only one");
        assert!(crate_only >= 1);
    }

    /// The nine category families are all present, by their artefact spelling.
    #[test]
    fn all_nine_category_families_are_registered() {
        for prefix in [
            "BR-S", "BR-Z", "BR-E", "BR-AE", "BR-IC", "BR-G", "BR-O", "BR-AF", "BR-AG",
        ] {
            for row in ["01", "05", "08", "09", "10"] {
                let id = format!("{prefix}-{row}");
                assert!(explain(&id).is_some(), "{id} missing");
            }
        }
        // …and the standard's own spelling for the last two reaches them.
        assert_eq!(explain("BR-IG-8").map(|r| r.id.as_str()), Some("BR-AF-08"));
        assert_eq!(explain("BR-IP-9").map(|r| r.id.as_str()), Some("BR-AG-09"));
    }
}

#[cfg(test)]
mod provenance_tests {
    use super::*;

    /// The `EN-` prefix is the whole reason a crate rule can never be mistaken
    /// for CEN's, so it must agree with [`Source::Crate`] in both directions.
    ///
    /// Asserted rather than assumed: `EN-EDITION-01` has already been misread as
    /// an authority's package-revision number, which is exactly the confusion the
    /// namespace exists to prevent. A prefix that drifts out
    /// of step with the provenance it advertises is worse than no prefix.
    #[test]
    fn the_en_namespace_means_exactly_source_crate() {
        for p in crate::profiles::ALL {
            for r in CORE.iter().chain(p.extra_rules.iter()) {
                assert_eq!(
                    r.id.as_str().starts_with("EN-"),
                    matches!(r.source, Source::Crate),
                    "{} is {:?} — the `EN-` prefix and `Source::Crate` must agree",
                    r.id,
                    r.source
                );
            }
        }
    }

    /// Exactly one core rule is in the standard and absent from the artefacts.
    ///
    /// If a second ever appears, the standard-versus-artefact diff has moved and
    /// the reader of a report deserves to know before the rule set does.
    #[test]
    fn br_co_25_is_the_only_standard_only_rule() {
        let ids: Vec<_> = CORE
            .iter()
            .filter(|r| matches!(r.source, Source::StandardOnly))
            .map(|r| r.id.as_str())
            .collect();
        assert_eq!(ids, ["BR-CO-25"]);
    }
}
