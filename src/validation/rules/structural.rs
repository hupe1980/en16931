//! Presence, conditionality and code-list rules that are not VAT-category
//! specific.
//!
//! # What is *not* here, and why
//!
//! Several rules the artefacts ship are satisfied by the model's own types and
//! have nothing left to check at runtime:
//!
//! | Rule | Why it cannot fire |
//! |---|---|
//! | `BR-22`, `BR-24` | BT-129 and BT-131 are non-`Option` on [`crate::InvoiceLine`] |
//! | `BR-26` | BT-146 is non-`Option` on `PriceDetails` |
//! | `BR-45`, `BR-46` | BT-116 and BT-117 are non-`Option` on `VatBreakdown` |
//! | `BR-12` … `BR-15` | BT-106, BT-109, BT-112 and BT-115 are non-`Option` on `DocumentTotals` |
//! | `BR-31`, `BR-36`, `BR-41`, `BR-43` | the amount is non-`Option` on every allowance and charge |
//! | `BR-DEC-*` (21 rules) | a third decimal is not representable in [`crate::InvoiceAmount`] |
//!
//! That is thirty-odd rules retired by the type system rather than by a
//! predicate — the crate's central claim, made concrete. They are registered in
//! [`STRUCTURAL_BY_TYPE`] with a constant-pass evaluation so `explain` still
//! works and a report can state they were checked.

use super::{Findings, Rule, RuleId, Severity, Source};
use crate::Identifier;
use crate::bt::{BtId, Group, Path};
use crate::codes::generated as lists;
use crate::invoice::{Invoice, terms as bt};

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

/// A rule the type system already guarantees. Registered so it is listable and
/// explainable; its evaluation is a constant pass.
macro_rules! by_type {
    // `expr` not `literal`: the `BR-DEC-*` texts are built with `concat!`.
    ($konst:ident, $id:literal, $text:expr) => {
        #[doc = $text]
        #[doc = ""]
        #[doc = "Satisfied by the model's types — see the module documentation."]
        pub static $konst: Rule = Rule {
            id: RuleId::new($id),
            severity: Severity::Fatal,
            text: $text,
            terms: &[],
            source: Source::Both,
            eval: |_, _| {},
        };
    };
}

// ── Addresses and parties ─────────────────────────────────────────────────────

rule!(BR_08, "BR-08", Fatal, Both, terms: [],
"An Invoice shall contain the Seller postal address.",
|inv, f| {
    let a = &inv.seller.address;
    if a.country.is_none() && a.city.is_none() && a.line1.is_none() && a.post_code.is_none() {
        f.at(Path::group(Group::Seller));
    }
});

rule!(BR_10, "BR-10", Fatal, Both, terms: [],
"An Invoice shall contain the Buyer postal address (BG-8).",
|inv, f| {
    let a = &inv.buyer.address;
    if a.country.is_none() && a.city.is_none() && a.line1.is_none() && a.post_code.is_none() {
        f.at(Path::group(Group::Buyer));
    }
});

rule!(BR_62, "BR-62", Fatal, Both, terms: [bt::SELLER_ELECTRONIC_ADDRESS],
"The Seller electronic address (BT-34) shall have a Scheme identifier.",
|inv, f| {
    if inv.seller.electronic_address.as_ref().is_some_and(|e| e.scheme().is_none()) {
        f.at(Path::group_term(Group::Seller, bt::SELLER_ELECTRONIC_ADDRESS));
    }
});

rule!(BR_63, "BR-63", Fatal, Both, terms: [bt::BUYER_ELECTRONIC_ADDRESS],
"The Buyer electronic address (BT-49) shall have a Scheme identifier.",
|inv, f| {
    if inv.buyer.electronic_address.as_ref().is_some_and(|e| e.scheme().is_none()) {
        f.at(Path::group_term(Group::Buyer, bt::BUYER_ELECTRONIC_ADDRESS));
    }
});

rule!(BR_CO_09, "BR-CO-09", Fatal, Both,
terms: [bt::SELLER_VAT_ID, bt::BUYER_VAT_ID, BtId(63)],
"The Seller VAT identifier (BT-31), the Seller tax representative VAT identifier (BT-63) and \
 the Buyer VAT identifier (BT-48) shall have a prefix in accordance with ISO code ISO 3166-1 \
 alpha-2 by which the country of issue may be identified. Nevertheless, Greece may use the \
 prefix 'EL'.",
|inv, f| {
    // `EL` is Greece's own prefix and is deliberately not ISO 3166-1 — the
    // rule names the exception explicitly, so the check must too.
    let ok = |id: &str| {
        let p: String = id.chars().take(2).collect::<String>().to_ascii_uppercase();
        p == "EL" || crate::codes::contains(lists::COUNTRY_CODES, &p)
    };
    for (vat, path) in [
        (
            inv.seller.vat_identifier.as_deref(),
            Path::group_term(Group::Seller, bt::SELLER_VAT_ID),
        ),
        (
            inv.buyer.vat_identifier.as_deref(),
            Path::group_term(Group::Buyer, bt::BUYER_VAT_ID),
        ),
        // BT-63. The rule text names it and it is easy to miss, because the tax
        // representative is not a `Party` — CEN's own conformance suite has a
        // case for exactly this.
        (
            inv.tax_representative
                .as_ref()
                .and_then(|t| t.vat_identifier.as_deref()),
            Path::term(BtId(63)),
        ),
    ] {
        if vat.is_some_and(|id| !ok(id)) {
            f.at(path);
        }
    }
});

rule!(BR_CO_26, "BR-CO-26", Fatal, Both,
terms: [bt::SELLER_VAT_ID],
"In order for the buyer to automatically identify a supplier, the Seller identifier (BT-29), \
 the Seller legal registration identifier (BT-30) and/or the Seller VAT identifier (BT-31) \
 shall be present.",
|inv, f| {
    let s = &inv.seller;
    if s.identifiers.is_empty()
        && s.legal_registration.is_none()
        && s.vat_identifier.is_none()
    {
        f.at(Path::group(Group::Seller));
    }
});

// ── Header conditionality ─────────────────────────────────────────────────────

rule!(BR_CO_03, "BR-CO-03", Fatal, Both, terms: [bt::VAT_POINT_DATE, bt::VAT_POINT_DATE_CODE],
"Value added tax point date (BT-7) and Value added tax point date code (BT-8) are mutually \
 exclusive.",
|inv, f| {
    if inv.vat_point_date.is_some() && inv.vat_point_date_code.is_some() {
        f.at(Path::term(bt::VAT_POINT_DATE));
    }
});

rule!(BR_53, "BR-53", Fatal, Both, terms: [bt::VAT_ACCOUNTING_CURRENCY, bt::VAT_TOTAL_ACCOUNTING],
"If the VAT accounting currency code (BT-6) is present, then the Invoice total VAT amount in \
 accounting currency (BT-111) shall be provided.",
|inv, f| {
    if inv.vat_accounting_currency.is_some() && inv.totals.vat_total_accounting.is_none() {
        f.at(Path::group_term(Group::Totals, bt::VAT_TOTAL_ACCOUNTING));
    }
});

rule!(BR_55, "BR-55", Fatal, Both, terms: [bt::PRECEDING_INVOICE],
"Each Preceding Invoice reference (BG-3) shall contain a Preceding Invoice reference (BT-25).",
|inv, f| {
    for p in &inv.preceding_invoices {
        if p.reference.is_blank() {
            f.at(Path::term(bt::PRECEDING_INVOICE));
        }
    }
});

rule!(BR_52, "BR-52", Fatal, Both, terms: [bt::SUPPORTING_DOCUMENT],
"Each Additional supporting document (BG-24) shall contain a Supporting document reference \
 (BT-122).",
|inv, f| {
    for d in &inv.attachments {
        if d.reference.is_blank() {
            f.at(Path::term(bt::SUPPORTING_DOCUMENT));
        }
    }
});

rule!(BR_57, "BR-57", Fatal, Both, terms: [bt::DELIVER_TO_COUNTRY],
"Each Deliver to address (BG-15) shall contain a Deliver to country code (BT-80).",
|inv, f| {
    if inv
        .delivery
        .as_ref()
        .and_then(|d| d.address.as_ref())
        .is_some_and(|a| a.country.is_none())
    {
        f.at(Path::group_term(Group::Delivery, bt::DELIVER_TO_COUNTRY));
    }
});

// ── Payment ───────────────────────────────────────────────────────────────────

rule!(BR_49, "BR-49", Fatal, Both, terms: [bt::PAYMENT_MEANS_CODE],
"A Payment instruction (BG-16) shall specify the Payment means type code (BT-81).",
|inv, f| {
    if inv.payment.as_ref().is_some_and(|p| {
        p.means_code.as_ref().is_none_or(crate::invoice::Code::is_blank)
    }) {
        f.at(Path::group_term(Group::Payment, bt::PAYMENT_MEANS_CODE));
    }
});

rule!(BR_61, "BR-61", Fatal, Both, terms: [bt::PAYMENT_ACCOUNT],
"If the Payment means type code (BT-81) means SEPA credit transfer, Local credit transfer or \
 Non-SEPA international credit transfer, the Payment account identifier (BT-84) shall be \
 present.",
|inv, f| {
    // UNTDID 4461: 30 credit transfer, 58 SEPA credit transfer.
    const CREDIT_TRANSFER: &[&str] = &["30", "58"];
    if let Some(p) = &inv.payment
        && p.means_code
            .as_ref()
            .is_some_and(|c| CREDIT_TRANSFER.contains(&c.as_str()))
        && p.account_identifier().is_none_or(str::is_empty)
    {
        f.at(Path::group_term(Group::Payment, bt::PAYMENT_ACCOUNT));
    }
});

rule!(BR_CL_16, "BR-CL-16", Fatal, ArtefactOnly, terms: [bt::PAYMENT_MEANS_CODE],
"Payment means in an invoice MUST be coded using UNCL4461 code list.",
|inv, f| {
    if let Some(p) = &inv.payment
        && let Some(c) = &p.means_code
        && !c.is_blank()
        && !c.is_in(lists::PAYMENT_MEANS_CODES)
    {
        f.at(Path::group_term(Group::Payment, bt::PAYMENT_MEANS_CODE));
    }
});

rule!(BR_51, "BR-51", Warning, Both, terms: [BtId(87)],
"In accordance with card payments security standards an invoice should never include a full \
 card primary account number (BT-87). At the moment PCI Security Standards Council has \
 defined that the first 6 digits and last 4 digits are the maximum number of digits to be \
 shown.",
|inv, f| {
    // The **only** warning in the 201-rule CEN abstract model, and the only
    // rule there about a security property rather than a semantic one.
    //
    // PCI DSS permits at most the first six and the last four digits, so a
    // PAN showing more than ten is not masked. Counting digits rather than
    // characters is deliberate: `4111 11** **** 1111` is masked despite
    // being nineteen characters long.
    if let Some(crate::invoice::PaymentMeans::Card(card)) =
        inv.payment.as_ref().and_then(|p| p.means.as_ref())
        && let Some(pan) = card.primary_account_number.as_deref()
        && pan.chars().filter(char::is_ascii_digit).count() > 10
    {
        f.at(Path::group_term(Group::Payment, BtId(87)));
    }
});

// ── Allowances and charges ────────────────────────────────────────────────────

rule!(BR_CO_21, "BR-CO-21", Fatal, Both, terms: [bt::ALLOWANCE_REASON, bt::ALLOWANCE_REASON_CODE],
"Each Document level allowance (BG-20) shall contain a Document level allowance reason \
 (BT-97) or a Document level allowance reason code (BT-98), or both.",
|inv, f| {
    for (i, a) in inv.allowances.iter().enumerate() {
        if a.reason.as_deref().is_none_or(str::is_empty) && a.reason_code.is_none() {
            f.at(Path::at_term(Group::DocumentAllowance, i, bt::ALLOWANCE_REASON));
        }
    }
});

rule!(BR_CO_22, "BR-CO-22", Fatal, Both, terms: [bt::CHARGE_REASON, bt::CHARGE_REASON_CODE],
"Each Document level charge (BG-21) shall contain a Document level charge reason (BT-104) or \
 a Document level charge reason code (BT-105), or both.",
|inv, f| {
    for (i, c) in inv.charges.iter().enumerate() {
        if c.reason.as_deref().is_none_or(str::is_empty) && c.reason_code.is_none() {
            f.at(Path::at_term(Group::DocumentCharge, i, bt::CHARGE_REASON));
        }
    }
});

rule!(BR_CO_23, "BR-CO-23", Fatal, Both,
terms: [bt::LINE_ALLOWANCE_REASON, bt::LINE_ALLOWANCE_REASON_CODE],
"Each Invoice line allowance (BG-27) shall contain an Invoice line allowance reason (BT-139) \
 or an Invoice line allowance reason code (BT-140), or both.",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        for a in &line.allowances {
            if a.reason.as_deref().is_none_or(str::is_empty) && a.reason_code.is_none() {
                f.at(Path::at_term(Group::Line, i, bt::LINE_ALLOWANCE_REASON));
            }
        }
    }
});

rule!(BR_CO_24, "BR-CO-24", Fatal, Both,
terms: [bt::LINE_CHARGE_REASON, bt::LINE_CHARGE_REASON_CODE],
"Each Invoice line charge (BG-28) shall contain an Invoice line charge reason (BT-144) or an \
 Invoice line charge reason code (BT-145), or both.",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        for c in &line.charges {
            if c.reason.as_deref().is_none_or(str::is_empty) && c.reason_code.is_none() {
                f.at(Path::at_term(Group::Line, i, bt::LINE_CHARGE_REASON));
            }
        }
    }
});

rule!(BR_CL_19, "BR-CL-19", Fatal, ArtefactOnly, terms: [bt::ALLOWANCE_REASON_CODE],
"Coded allowance reasons MUST belong to the UNCL 5189 code list.",
|inv, f| {
    for (i, a) in inv.allowances.iter().enumerate() {
        if a.reason_code.as_ref().is_some_and(|c| !c.is_in(lists::ALLOWANCE_REASON_CODES)) {
            f.at(Path::at_term(Group::DocumentAllowance, i, bt::ALLOWANCE_REASON_CODE));
        }
    }
});

rule!(BR_CL_20, "BR-CL-20", Fatal, ArtefactOnly, terms: [bt::CHARGE_REASON_CODE],
"Coded charge reasons MUST belong to the UNCL 7161 code list.",
|inv, f| {
    for (i, c) in inv.charges.iter().enumerate() {
        if c.reason_code.as_ref().is_some_and(|c| !c.is_in(lists::CHARGE_REASON_CODES)) {
            f.at(Path::at_term(Group::DocumentCharge, i, bt::CHARGE_REASON_CODE));
        }
    }
});

// ── Items and remaining code lists ────────────────────────────────────────────

rule!(BR_64, "BR-64", Fatal, Both, terms: [bt::ITEM_STANDARD_ID],
"The Item standard identifier (BT-157) shall have a Scheme identifier.",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        if line.item.standard_identifier.as_ref().is_some_and(|id| id.scheme().is_none()) {
            f.at(Path::at_term(Group::Line, i, bt::ITEM_STANDARD_ID));
        }
    }
});

rule!(BR_65, "BR-65", Fatal, Both, terms: [bt::ITEM_CLASSIFICATION_ID],
"The Item classification identifier (BT-158) shall have a Scheme identifier.",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        if line.item.classification_identifiers.iter().any(|id| id.scheme().is_none()) {
            f.at(Path::at_term(Group::Line, i, bt::ITEM_CLASSIFICATION_ID));
        }
    }
});

rule!(BR_CL_14, "BR-CL-14", Fatal, ArtefactOnly, terms: [bt::SELLER_COUNTRY, bt::BUYER_COUNTRY],
"Country codes in an invoice MUST be coded using ISO code list 3166-1.",
|inv, f| {
    for (addr, path) in [
        (&inv.seller.address, Path::group_term(Group::Seller, bt::SELLER_COUNTRY)),
        (&inv.buyer.address, Path::group_term(Group::Buyer, bt::BUYER_COUNTRY)),
    ] {
        if addr.country.as_ref().is_some_and(|c| !c.is_blank() && !c.is_in(lists::COUNTRY_CODES))
        {
            f.at(path);
        }
    }
});

rule!(BR_CL_06, "BR-CL-06", Fatal, ArtefactOnly, terms: [bt::VAT_POINT_DATE_CODE],
"Value added tax point date code MUST be coded using a restriction of UNTDID 2005.",
|inv, f| {
    if inv
        .vat_point_date_code
        .as_ref()
        .is_some_and(|c| !c.is_blank() && !c.is_in(lists::VAT_POINT_DATE_CODES))
    {
        f.at(Path::term(bt::VAT_POINT_DATE_CODE));
    }
});

rule!(BR_CL_22, "BR-CL-22", Fatal, ArtefactOnly, terms: [bt::EXEMPTION_REASON_CODE],
"Tax exemption reason code identifier scheme identifier MUST belong to the CEF VATEX code \
 list.",
|inv, f| {
    // **Case-insensitive**, and this is the one code-list rule that is.
    //
    // The codelist *source* (`EN16931-UBL-codes.sch`) compares verbatim; the
    // **released** artefact wraps the value:
    //
    // ```xpath
    // contains(' VATEX-EU-… ', concat(' ', normalize-space(upper-case(.)), ' '))
    // ```
    //
    // So `vatex-eu-132-1g` is valid, and CEN's own `Invoice-Max_content.xml`
    // reference file relies on it. Generating this rule's semantics from the
    // source file — as this crate did — made it reject two of CEN's own
    // examples. Same lesson as XRechnung's Peppol merge: the file in source
    // control is not the artefact that ships.
    //
    // It also brings this rule into line with Peppol's `P0104`…`P0111`, which
    // already fold BT-121's case.
    //
    // §6.5.8 says *"Codes shall be entered exactly as shown in the selected code
    // list"*, so the standard and the artefact disagree here. This crate follows
    // the artefact, because that is what every deployed validator runs.
    for (i, e) in inv.vat_breakdown.iter().enumerate() {
        if e.exemption_reason_code.as_ref().is_some_and(|c| {
            !c.is_blank() && !crate::codes::contains(lists::VATEX_CODES, &c.as_str().to_uppercase())
        }) {
            f.at(Path::at_term(Group::VatBreakdown, i, bt::EXEMPTION_REASON_CODE));
        }
    }
});

// ── Retired by the type system ────────────────────────────────────────────────

by_type!(
    BR_12,
    "BR-12",
    "An Invoice shall have the Sum of Invoice line net amount (BT-106)."
);
by_type!(
    BR_13,
    "BR-13",
    "An Invoice shall have the Invoice total amount without VAT (BT-109)."
);
by_type!(
    BR_14,
    "BR-14",
    "An Invoice shall have the Invoice total amount with VAT (BT-112)."
);
by_type!(
    BR_15,
    "BR-15",
    "An Invoice shall have the Amount due for payment (BT-115)."
);
by_type!(
    BR_22,
    "BR-22",
    "Each Invoice line (BG-25) shall have an Invoiced quantity (BT-129)."
);
by_type!(
    BR_24,
    "BR-24",
    "Each Invoice line (BG-25) shall have an Invoice line net amount (BT-131)."
);
by_type!(
    BR_26,
    "BR-26",
    "Each Invoice line (BG-25) shall contain the Item net price (BT-146)."
);
by_type!(
    BR_31,
    "BR-31",
    "Each Document level allowance (BG-20) shall have a Document level allowance amount (BT-92)."
);
by_type!(
    BR_32,
    "BR-32",
    "Each Document level allowance (BG-20) shall have a Document level allowance VAT category code (BT-95)."
);
by_type!(
    BR_36,
    "BR-36",
    "Each Document level charge (BG-21) shall have a Document level charge amount (BT-99)."
);
by_type!(
    BR_37,
    "BR-37",
    "Each Document level charge (BG-21) shall have a Document level charge VAT category code (BT-102)."
);
by_type!(
    BR_41,
    "BR-41",
    "Each Invoice line allowance (BG-27) shall have an Invoice line allowance amount (BT-136)."
);
by_type!(
    BR_43,
    "BR-43",
    "Each Invoice line charge (BG-28) shall have an Invoice line charge amount (BT-141)."
);
by_type!(
    BR_45,
    "BR-45",
    "Each VAT breakdown (BG-23) shall have a VAT category taxable amount (BT-116)."
);
by_type!(
    BR_46,
    "BR-46",
    "Each VAT breakdown (BG-23) shall have a VAT category tax amount (BT-117)."
);

/// The `BR-DEC-*` family: *"The allowed maximum number of decimals for BT-x
/// is 2."*
///
/// All twenty-one are retired by [`crate::InvoiceAmount`], which is `i64` minor
/// units and physically cannot hold a third decimal. They are registered so
/// `explain("BR-DEC-12")` works and a report can state they were checked.
macro_rules! dec_rules {
    ($($konst:ident, $id:literal, $bt:literal;)*) => {
        $(by_type!($konst, $id,
            concat!("The allowed maximum number of decimals for ", $bt, " is 2."));)*
        /// Every `BR-DEC-*` rule.
        pub static DECIMALS: &[&Rule] = &[$(&$konst),*];
    };
}

dec_rules! {
    BR_DEC_01, "BR-DEC-01", "the Document level allowance amount (BT-92)";
    BR_DEC_02, "BR-DEC-02", "the Document level allowance base amount (BT-93)";
    BR_DEC_05, "BR-DEC-05", "the Document level charge amount (BT-99)";
    BR_DEC_06, "BR-DEC-06", "the Document level charge base amount (BT-100)";
    BR_DEC_09, "BR-DEC-09", "the Sum of Invoice line net amount (BT-106)";
    BR_DEC_10, "BR-DEC-10", "the Sum of allowances on document level (BT-107)";
    BR_DEC_11, "BR-DEC-11", "the Sum of charges on document level (BT-108)";
    BR_DEC_12, "BR-DEC-12", "the Invoice total amount without VAT (BT-109)";
    BR_DEC_13, "BR-DEC-13", "the Invoice total VAT amount (BT-110)";
    BR_DEC_14, "BR-DEC-14", "the Invoice total amount with VAT (BT-112)";
    BR_DEC_15, "BR-DEC-15", "the Invoice total VAT amount in accounting currency (BT-111)";
    BR_DEC_16, "BR-DEC-16", "the Paid amount (BT-113)";
    BR_DEC_17, "BR-DEC-17", "the Rounding amount (BT-114)";
    BR_DEC_18, "BR-DEC-18", "the Amount due for payment (BT-115)";
    BR_DEC_19, "BR-DEC-19", "the VAT category taxable amount (BT-116)";
    BR_DEC_20, "BR-DEC-20", "the VAT category tax amount (BT-117)";
    BR_DEC_23, "BR-DEC-23", "the Invoice line net amount (BT-131)";
    BR_DEC_24, "BR-DEC-24", "the Invoice line allowance amount (BT-136)";
    BR_DEC_25, "BR-DEC-25", "the Invoice line allowance base amount (BT-137)";
    BR_DEC_27, "BR-DEC-27", "the Invoice line charge amount (BT-141)";
    BR_DEC_28, "BR-DEC-28", "the Invoice line charge base amount (BT-142)";
}

/// Rules satisfied by the model's types rather than by a predicate.
pub static STRUCTURAL_BY_TYPE: &[&Rule] = &[
    // Every amount in the model is implicitly in BT-5's currency, so there is
    // no per-amount `@currencyID` that could disagree with it.
    &BR_CL_03, &BR_12, &BR_13, &BR_14, &BR_15, &BR_22, &BR_24, &BR_26, &BR_31, &BR_32, &BR_36,
    &BR_37, &BR_41, &BR_43, &BR_45, &BR_46,
];

/// Everything this module defines.
pub static ALL: &[&Rule] = &[
    &BR_CL_05, &BR_CL_07, &BR_CL_08, &BR_CO_05, &BR_CO_06, &BR_CO_07, &BR_CO_08, &BR_17, &BR_18,
    &BR_19, &BR_20, &BR_33, &BR_38, &BR_42, &BR_44, &BR_50, &BR_54, &BR_56, &BR_CL_10, &BR_CL_11,
    &BR_CL_13, &BR_CL_15, &BR_CL_21, &BR_CL_24, &BR_CL_25, &BR_CL_26, &BR_CO_20, &BR_IC_11,
    &BR_IC_12, &BR_08, &BR_10, &BR_49, &BR_51, &BR_52, &BR_53, &BR_55, &BR_57, &BR_61, &BR_62,
    &BR_63, &BR_64, &BR_65, &BR_CL_06, &BR_CL_14, &BR_CL_16, &BR_CL_19, &BR_CL_20, &BR_CL_22,
    &BR_CO_03, &BR_CO_09, &BR_CO_21, &BR_CO_22, &BR_CO_23, &BR_CO_24, &BR_CO_26,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_type_system_retires_thirty_seven_rules() {
        assert_eq!(STRUCTURAL_BY_TYPE.len(), 16);
        assert_eq!(DECIMALS.len(), 21, "Table 26 has 21 rows in the artefacts");
        // They pass unconditionally — that is the point.
        for r in STRUCTURAL_BY_TYPE.iter().chain(DECIMALS.iter()) {
            let mut out = Vec::new();
            let mut sink = crate::validation::Findings::for_test(&mut out, r);
            (r.eval)(&Invoice::default(), &mut sink);
            assert!(out.is_empty(), "{} fired on an empty invoice", r.id);
        }
    }

    /// The four rules CEN itself binds to `true()` must also never fire —
    /// registering them is a disposition, not a check.
    #[test]
    fn the_undecidable_four_never_fire() {
        for r in [&BR_CO_05, &BR_CO_06, &BR_CO_07, &BR_CO_08] {
            let mut out = Vec::new();
            let mut sink = crate::validation::Findings::for_test(&mut out, r);
            (r.eval)(&Invoice::default(), &mut sink);
            assert!(out.is_empty(), "{} fired", r.id);
        }
    }

    #[test]
    fn greece_may_use_el_which_is_not_iso_3166_1() {
        // BR-CO-09 names the exception explicitly, so the check must too.
        assert!(!crate::codes::contains(lists::COUNTRY_CODES, "EL"));
        assert!(crate::codes::contains(lists::COUNTRY_CODES, "GR"));
    }
}

// ── BG-10 payee, BG-11 tax representative ─────────────────────────────────────

rule!(BR_17, "BR-17", Fatal, Both, terms: [BtId(59)],
"The Payee name (BT-59) shall be provided in the Invoice, if the Payee (BG-10) is different \
 from the Seller (BG-4).",
|inv, f| {
    // "different from the Seller" is the condition the standard names, so a
    // payee whose name simply repeats the seller's is not a finding.
    if let Some(p) = &inv.payee
        && p.name.as_deref().is_none_or(str::is_empty)
    {
        f.at(Path::term(BtId(59)));
    }
});

rule!(BR_18, "BR-18", Fatal, Both, terms: [BtId(62)],
"The Seller tax representative name (BT-62) shall be provided in the Invoice, if the Seller \
 (BG-4) has a Seller tax representative party (BG-11).",
|inv, f| {
    if let Some(r) = &inv.tax_representative
        && r.name.as_deref().is_none_or(str::is_empty)
    {
        f.at(Path::term(BtId(62)));
    }
});

rule!(BR_19, "BR-19", Fatal, Both, terms: [],
"The Seller tax representative postal address (BG-12) shall be provided in the Invoice, if \
 the Seller (BG-4) has a Seller tax representative party (BG-11).",
|inv, f| {
    if let Some(r) = &inv.tax_representative {
        let a = &r.address;
        if a.country.is_none() && a.city.is_none() && a.line1.is_none() && a.post_code.is_none()
        {
            f.at(Path::term(BtId(62)));
        }
    }
});

rule!(BR_20, "BR-20", Fatal, Both, terms: [BtId(69)],
"The Seller tax representative postal address (BG-12) shall contain a Tax representative \
 country code (BT-69), if the Seller (BG-4) has a Seller tax representative party (BG-11).",
|inv, f| {
    if inv
        .tax_representative
        .as_ref()
        .is_some_and(|r| r.address.country.is_none())
    {
        f.at(Path::term(BtId(69)));
    }
});

rule!(BR_56, "BR-56", Fatal, Both, terms: [BtId(63)],
"Each Seller tax representative party (BG-11) shall have a Seller tax representative VAT \
 identifier (BT-63).",
|inv, f| {
    if inv
        .tax_representative
        .as_ref()
        .is_some_and(|r| r.vat_identifier.as_deref().is_none_or(str::is_empty))
    {
        f.at(Path::term(BtId(63)));
    }
});

// ── Allowance and charge reasons, under their own ids ─────────────────────────
//
// `BR-33` / `-38` / `-42` / `-44` say the same thing as `BR-CO-21` … `-24`. The
// artefacts ship both, so both are registered: a report that named only one
// would send half its readers to a rule index entry that does not exist.

rule!(BR_33, "BR-33", Fatal, Both, terms: [bt::ALLOWANCE_REASON, bt::ALLOWANCE_REASON_CODE],
"Each Document level allowance (BG-20) shall have a Document level allowance reason (BT-97) \
 or a Document level allowance reason code (BT-98).",
|inv, f| {
    for (i, a) in inv.allowances.iter().enumerate() {
        if a.reason.as_deref().is_none_or(str::is_empty) && a.reason_code.is_none() {
            f.at(Path::at_term(Group::DocumentAllowance, i, bt::ALLOWANCE_REASON));
        }
    }
});

rule!(BR_38, "BR-38", Fatal, Both, terms: [bt::CHARGE_REASON, bt::CHARGE_REASON_CODE],
"Each Document level charge (BG-21) shall have a Document level charge reason (BT-104) or a \
 Document level charge reason code (BT-105).",
|inv, f| {
    for (i, c) in inv.charges.iter().enumerate() {
        if c.reason.as_deref().is_none_or(str::is_empty) && c.reason_code.is_none() {
            f.at(Path::at_term(Group::DocumentCharge, i, bt::CHARGE_REASON));
        }
    }
});

rule!(BR_42, "BR-42", Fatal, Both, terms: [bt::LINE_ALLOWANCE_REASON],
"Each Invoice line allowance (BG-27) shall have an Invoice line allowance reason (BT-139) or \
 an Invoice line allowance reason code (BT-140).",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        for a in &line.allowances {
            if a.reason.as_deref().is_none_or(str::is_empty) && a.reason_code.is_none() {
                f.at(Path::at_term(Group::Line, i, bt::LINE_ALLOWANCE_REASON));
            }
        }
    }
});

rule!(BR_44, "BR-44", Fatal, Both, terms: [bt::LINE_CHARGE_REASON],
"Each Invoice line charge shall have an Invoice line charge reason or an invoice line \
 allowance reason code.",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        for c in &line.charges {
            if c.reason.as_deref().is_none_or(str::is_empty) && c.reason_code.is_none() {
                f.at(Path::at_term(Group::Line, i, bt::LINE_CHARGE_REASON));
            }
        }
    }
});

// ── Remaining presence and conditionality ─────────────────────────────────────

rule!(BR_50, "BR-50", Fatal, Both, terms: [bt::PAYMENT_ACCOUNT],
"A Payment account identifier (BT-84) shall be present if Credit transfer (BG-17) information \
 is provided in the Invoice.",
|inv, f| {
    if let Some(crate::invoice::PaymentMeans::CreditTransfer(ts)) =
        inv.payment.as_ref().and_then(|p| p.means.as_ref())
        && ts
            .iter()
            .any(|t| t.account_identifier.as_deref().is_none_or(str::is_empty))
    {
        f.at(Path::group_term(Group::Payment, bt::PAYMENT_ACCOUNT));
    }
});

rule!(BR_54, "BR-54", Fatal, Both, terms: [BtId(160), BtId(161)],
"Each Item attribute (BG-32) shall contain an Item attribute name (BT-160) and an Item \
 attribute value (BT-161).",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        for a in &line.item.attributes {
            if a.name.as_deref().is_none_or(str::is_empty)
                || a.value.as_deref().is_none_or(str::is_empty)
            {
                f.at(Path::at_term(Group::Line, i, BtId(160)));
            }
        }
    }
});

rule!(BR_CO_20, "BR-CO-20", Fatal, Both, terms: [bt::LINE_PERIOD_START, bt::LINE_PERIOD_END],
"If Invoice line period (BG-26) is used, the Invoice line period start date (BT-134) or the \
 Invoice line period end date (BT-135) shall be filled, or both.",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        if let Some(p) = &line.period
            && p.start.is_none()
            && p.end.is_none()
        {
            f.at(Path::at_term(Group::Line, i, bt::LINE_PERIOD_START));
        }
    }
});

rule!(BR_IC_11, "BR-IC-11", Fatal, Both, terms: [bt::DELIVERY_DATE, bt::PERIOD_START],
"In an Invoice with a VAT breakdown (BG-23) where the VAT category code (BT-118) is \
 \"Intra-community supply\" the Actual delivery date (BT-72) or the Invoicing period (BG-14) \
 shall not be blank.",
|inv, f| {
    let intra = inv
        .vat_breakdown
        .iter()
        .any(|e| e.semantics() == Some(crate::VatCategory::IntraCommunity));
    let has_date = inv.delivery.as_ref().is_some_and(|d| d.date.is_some());
    let has_period = inv
        .invoicing_period
        .as_ref()
        .is_some_and(|p| p.start.is_some() || p.end.is_some());
    if intra && !has_date && !has_period {
        f.at(Path::group_term(Group::Delivery, bt::DELIVERY_DATE));
    }
});

rule!(BR_IC_12, "BR-IC-12", Fatal, Both, terms: [bt::DELIVER_TO_COUNTRY],
"In an Invoice with a VAT breakdown (BG-23) where the VAT category code (BT-118) is \
 \"Intra-community supply\" the Deliver to country code (BT-80) shall not be blank.",
|inv, f| {
    let intra = inv
        .vat_breakdown
        .iter()
        .any(|e| e.semantics() == Some(crate::VatCategory::IntraCommunity));
    let has_country = inv
        .delivery
        .as_ref()
        .and_then(|d| d.address.as_ref())
        .is_some_and(|a| a.country.is_some());
    if intra && !has_country {
        f.at(Path::group_term(Group::Delivery, bt::DELIVER_TO_COUNTRY));
    }
});

// ── Remaining code lists ──────────────────────────────────────────────────────

rule!(BR_CL_15, "BR-CL-15", Fatal, ArtefactOnly, terms: [BtId(159)],
"Country codes in an invoice MUST be coded using ISO code list 3166-1.",
|inv, f| {
    // `cac:OriginCountry/cbc:IdentificationCode` — **BT-159**, the item's
    // country of origin. Not BT-80: that is `cac:Country`, which is
    // `BR-CL-14`'s context. The two rules share a message and check different
    // terms, and reading the message rather than the context is how this rule
    // spent several revisions duplicating its neighbour.
    for (i, line) in inv.lines.iter().enumerate() {
        if line
            .item
            .origin_country
            .as_ref()
            .is_some_and(|c| !c.is_blank() && !c.is_in(lists::COUNTRY_CODES))
        {
            f.at(Path::at_term(Group::Line, i, BtId(159)));
        }
    }
});

rule!(BR_CL_13, "BR-CL-13", Fatal, ArtefactOnly, terms: [bt::ITEM_CLASSIFICATION_ID],
"Item classification identifier identification scheme identifier MUST be coded using one of \
 the UNTDID 7143 list.",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        for id in &line.item.classification_identifiers {
            if id
                .scheme()
                .is_some_and(|s| !crate::codes::contains(lists::ITEM_CLASSIFICATION_SCHEMES, s))
            {
                f.at(Path::at_term(Group::Line, i, bt::ITEM_CLASSIFICATION_ID));
            }
        }
    }
});

rule!(BR_CL_21, "BR-CL-21", Fatal, ArtefactOnly, terms: [bt::ITEM_STANDARD_ID],
"Item standard identifier scheme identifier MUST belong to the ISO 6523 ICD code list.",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        if line
            .item
            .standard_identifier
            .as_ref()
            .and_then(crate::Identifier::scheme)
            .is_some_and(|s| !crate::codes::contains(lists::ICD_SCHEMES, s))
        {
            f.at(Path::at_term(Group::Line, i, bt::ITEM_STANDARD_ID));
        }
    }
});

rule!(BR_CL_25, "BR-CL-25", Fatal, ArtefactOnly,
terms: [bt::SELLER_ELECTRONIC_ADDRESS, bt::BUYER_ELECTRONIC_ADDRESS],
"Endpoint identifier scheme identifier MUST belong to the CEF EAS code list.",
|inv, f| {
    for (party, path, _) in [
        (&inv.seller, Path::group_term(Group::Seller, bt::SELLER_ELECTRONIC_ADDRESS), 0),
        (&inv.buyer, Path::group_term(Group::Buyer, bt::BUYER_ELECTRONIC_ADDRESS), 1),
    ] {
        if party
            .electronic_address
            .as_ref()
            .and_then(crate::Identifier::scheme)
            .is_some_and(|s| !crate::codes::contains(lists::EAS_SCHEMES, s))
        {
            f.at(path);
        }
    }
});

rule!(BR_CL_26, "BR-CL-26", Fatal, ArtefactOnly, terms: [],
"Delivery location identifier scheme identifier MUST belong to the ISO 6523 ICD code list.",
|inv, f| {
    if inv
        .delivery
        .as_ref()
        .and_then(|d| d.location.as_ref())
        .and_then(crate::Identifier::scheme)
        .is_some_and(|s| !crate::codes::contains(lists::ICD_SCHEMES, s))
    {
        f.at(Path::group(Group::Delivery));
    }
});

rule!(BR_CL_10, "BR-CL-10", Fatal, ArtefactOnly, terms: [],
"Any identifier identification scheme identifier MUST be coded using one of the ISO 6523 ICD \
 list.",
|inv, f| {
    // `BR-CL-10`'s own test also admits the literal `SEPA`, but only on a
    // party identification under the supplier or the payee — a contextual
    // extension the flat ICD table deliberately does not carry.
    const SEPA: &str = "SEPA";
    for (party, group) in [(&inv.seller, Group::Seller), (&inv.buyer, Group::Buyer)] {
        for id in &party.identifiers {
            if id.scheme().is_some_and(|s| {
                s != SEPA && !crate::codes::contains(lists::ICD_SCHEMES, s)
            }) {
                f.at(Path::group(group));
            }
        }
    }
});

rule!(BR_CL_11, "BR-CL-11", Fatal, ArtefactOnly, terms: [],
"Any registration identifier identification scheme identifier MUST be coded using one of the \
 ISO 6523 ICD list.",
|inv, f| {
    for (party, group) in [(&inv.seller, Group::Seller), (&inv.buyer, Group::Buyer)] {
        if party
            .legal_registration
            .as_ref()
            .and_then(crate::Identifier::scheme)
            .is_some_and(|s| !crate::codes::contains(lists::ICD_SCHEMES, s))
        {
            f.at(Path::group(group));
        }
    }
});

rule!(BR_CL_24, "BR-CL-24", Fatal, ArtefactOnly, terms: [],
"For Mime code in attribute use MIMEMediaType.",
|inv, f| {
    // The artefact checks membership of a short list; §6.5.11's normative
    // receiver obligation names the same six types.
    for d in &inv.attachments {
        if let Some(a) = &d.attachment
            && !crate::Attachment::RECEIVER_MUST_ACCEPT.contains(&a.mime_code())
        {
            f.at(Path::term(bt::SUPPORTING_DOCUMENT));
        }
    }
});

// ── The last four code lists ──────────────────────────────────────────────────

rule!(BR_CL_05, "BR-CL-05", Fatal, ArtefactOnly, terms: [bt::VAT_ACCOUNTING_CURRENCY],
"Tax currency code MUST be coded using ISO code list 4217 alpha-3.",
|inv, f| {
    if inv
        .vat_accounting_currency
        .as_ref()
        .is_some_and(|c| !c.is_blank() && !c.is_in(lists::CURRENCY_CODES))
    {
        f.at(Path::term(bt::VAT_ACCOUNTING_CURRENCY));
    }
});

rule!(BR_CL_07, "BR-CL-07", Fatal, ArtefactOnly, terms: [BtId(18), BtId(128)],
"Object identifier identification scheme identifier MUST be coded using a restriction of UNTDID \
 1153.",
|inv, f| {
    // The artefact context is a **union**: `cac:AdditionalDocumentReference`
    // with document type 130 (BT-18, document level) *and* the line's
    // `cac:DocumentReference` with the same code (BT-128).
    let bad = |id: &Identifier| {
        id.scheme()
            .is_some_and(|s| !crate::codes::contains(lists::REFERENCE_QUALIFIERS, s))
    };
    if inv.object_identifier.as_ref().is_some_and(bad) {
        f.at(Path::term(BtId(18)));
    }
    for (i, line) in inv.lines.iter().enumerate() {
        if line.object_identifier.as_ref().is_some_and(bad) {
            f.at(Path::at_term(Group::Line, i, BtId(128)));
        }
    }
});

rule!(BR_CL_08, "BR-CL-08", Fatal, ArtefactOnly, terms: [BtId(21)],
"Invoiced note subject code shall be coded using UNCL4451.",
|inv, f| {
    for n in &inv.notes {
        if n.subject_code
            .as_ref()
            .is_some_and(|c| !c.is_blank() && !c.is_in(lists::NOTE_SUBJECT_CODES))
        {
            f.at(Path::term(BtId(21)));
        }
    }
});

// ── Rules no validator can decide — CEN's own included ────────────────────────

/// Register a rule that **CEN's own Schematron binds to a constant `true()`**.
///
/// `BR-CO-05` … `BR-CO-08` all say some variant of *"the reason code and the
/// reason shall indicate the same type"* — they ask whether a code and a piece
/// of free text mean the same thing. No validator can answer that, and CEN's
/// UBL binding says so out loud:
///
/// ```text
/// <let name="BR-CO-05" value="true()"/>
/// ```
///
/// So these are not gaps in this crate relative to the artefacts. Registering
/// them, with the binding quoted, matches CEN exactly — and, unlike silence,
/// tells a reader that the rule was read and dispositioned rather than missed.
macro_rules! undecidable {
    ($konst:ident, $id:literal, $text:literal) => {
        #[doc = $text]
        #[doc = ""]
        #[doc = "**Not mechanically decidable.** CEN's own UBL binding is"]
        #[doc = "`value=\"true()\"` — the artefacts register this rule and never"]
        #[doc = "evaluate it. Neither does this crate."]
        pub static $konst: Rule = Rule {
            id: RuleId::new($id),
            severity: Severity::Fatal,
            text: $text,
            terms: &[],
            source: Source::Both,
            eval: |_, _| {},
        };
    };
}

undecidable!(
    BR_CO_05,
    "BR-CO-05",
    "Document level allowance reason code (BT-98) and Document level allowance reason (BT-97) \
     shall indicate the same type of allowance."
);
undecidable!(
    BR_CO_06,
    "BR-CO-06",
    "Document level charge reason code (BT-105) and Document level charge reason (BT-104) shall \
     indicate the same type of charge."
);
undecidable!(
    BR_CO_07,
    "BR-CO-07",
    "Invoice line allowance reason code (BT-140) and Invoice line allowance reason (BT-139) shall \
     indicate the same type of allowance reason."
);
undecidable!(
    BR_CO_08,
    "BR-CO-08",
    "Invoice line charge reason code (BT-145) and Invoice line charge reason (BT-144) shall \
     indicate the same type of charge reason."
);

by_type!(
    BR_CL_03,
    "BR-CL-03",
    "currencyID MUST be coded using ISO code list 4217 alpha-3."
);
