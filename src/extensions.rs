//! Data that has **no core business term** — EN 16931's second extension
//! mechanism.
//!
//! # CIUS restricts; an Extension adds
//!
//! §4.3 draws the line, and the two are constantly confused:
//!
//! > There are however circumstances where the trading partners may wish to:
//! > Either **1. restrict** the information elements to be used in an e-invoice
//! > or **2. provide additional** information elements. The first requirement is
//! > satisfied using a **Core Invoice Usage Specification (CIUS)**. The second
//! > requirement is satisfied using an **extension** specified in an Extension
//! > Specification.
//!
//! [`crate::validation::profile::Restriction`] is mechanism 1. This module is
//! mechanism 2, governed by CEN/TR 16931-5 and bound by §4.3's constraint:
//!
//! > Any such extension shall not infringe or contradict the semantic
//! > definitions in the core invoice model.
//!
//! Enforced literally: nothing here may shadow a core business term. Everything
//! in this module carries data the core model has no term for.
//!
//! # Why this is a correctness feature, not a convenience
//!
//! A final invoice that deducts advance payments must, in Germany, state *"die
//! auf sie entfallenden Steuerbeträge"* — the tax contained in each advance
//! (§14 Abs. 5 Satz 2 UStG). Omit it and, per UStAE 14.8 Abs. 10, the issuer
//! owes the tax shown **plus** the advance-related portion again under §14c
//! Abs. 1: the same tax, billed twice.
//!
//! Core EN 16931 has **nowhere to put it**. BT-113 is a single flat figure. The
//! only standardised home is ZUGFeRD / Factur-X EXTENDED's
//! `SpecifiedAdvancePayment` (BG-X-45).
//!
//! So an adapter that quietly maps itemised advances to BT-113 and drops the
//! rest produces a document that validates perfectly and is a tax liability.
//! This module carries the data, and [`crate::validation::rules`]' `EN-EXT-01`
//! **warns** whenever a target profile cannot represent it.

use crate::invoice::{InvoiceLine, LineVat, VatBreakdown};
use crate::{Date, DocumentReference, InvoiceAmount};

/// One previously invoiced and received advance payment, with its tax.
///
/// Mirrors ZUGFeRD / Factur-X EXTENDED `BG-X-45`, the one standardised place
/// this data has a home.
///
/// | Here | ZUGFeRD EXTENDED | Meaning |
/// |---|---|---|
/// | [`gross`](Self::gross) | `BT-X-291` | amount received |
/// | [`received_on`](Self::received_on) | `BT-X-292` | date of receipt |
/// | [`tax`](Self::tax) | `BG-X-46` | tax contained, per category and rate |
/// | [`reference`](Self::reference) | `BT-X-558` | the advance invoice's number |
/// | [`reference_date`](Self::reference_date) | `BT-X-560` | its issue date |
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvancePayment {
    /// `BT-X-291` — the gross amount received, `Σ (base + tax)`.
    pub gross: InvoiceAmount,
    /// `BT-X-292` — when it was received.
    pub received_on: Option<Date>,
    /// `BG-X-46` — the tax it contains, per `(category, rate)`.
    ///
    /// This is the figure §14 Abs. 5 Satz 2 UStG requires a final invoice to
    /// state, and the reason this whole module exists.
    pub tax: Vec<VatBreakdown>,
    /// `BT-X-558` — the advance invoice's number.
    ///
    /// Worth setting: §14 Abs. 5 Satz 2 only obliges deduction where invoices
    /// *were issued*, so the reference is what evidences the obligation.
    pub reference: Option<DocumentReference>,
    /// `BT-X-560` — the advance invoice's issue date.
    pub reference_date: Option<Date>,
}

impl AdvancePayment {
    /// The tax contained, summed over the breakdown.
    ///
    /// # Errors
    /// [`crate::AmountError::Overflow`].
    pub fn tax_total(&self) -> Result<InvoiceAmount, crate::AmountError> {
        InvoiceAmount::checked_sum(self.tax.iter().map(|e| e.tax_amount))
    }
}

/// Data with no core business term, carried alongside the invoice.
///
/// A format crate reads what its own profile can represent and ignores the rest.
/// `zugferd` consumes [`advance_payments`](Self::advance_payments) when emitting
/// EXTENDED; `xrechnung` cannot, and `EN-EXT-01` says so rather than letting it
/// vanish.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Extensions {
    /// ZUGFeRD / Factur-X EXTENDED `BG-X-45`.
    ///
    /// Non-empty makes this a **final invoice**: the totals and the VAT
    /// breakdown still describe the whole supply, and the advances plus their
    /// tax are deducted to reach BT-115.
    pub advance_payments: Vec<AdvancePayment>,
    /// XRechnung Extension `BG-DEX-01`, indexed by the BG-25 line they hang
    /// beneath — the line's zero-based position in [`crate::Invoice::lines`].
    ///
    /// Kept out of [`crate::InvoiceLine`] on purpose: a core line has no child,
    /// and putting one there would make every consumer of the core model carry
    /// a field only one Extension can populate.
    pub sub_invoice_lines: Vec<(usize, Vec<SubInvoiceLine>)>,
    /// XRechnung Extension `BG-DEX-09`.
    ///
    /// Non-empty changes the totals equation: `BR-DEX-09` replaces `BR-CO-16`,
    /// adding these amounts back into BT-115.
    pub third_party_payments: Vec<ThirdPartyPayment>,
}

impl Extensions {
    /// The extension group names this value actually populates.
    ///
    /// Compared against [`crate::Profile::extensions`] by `EN-EXT-01`.
    #[must_use]
    pub fn populated(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if !self.advance_payments.is_empty() {
            v.push(ADVANCE_PAYMENTS);
        }
        if !self.sub_invoice_lines.is_empty() {
            v.push(SUB_INVOICE_LINES);
        }
        if !self.third_party_payments.is_empty() {
            v.push(THIRD_PARTY_PAYMENTS);
        }
        v
    }

    /// The sub-lines hanging beneath BG-25 line `index`, if any.
    #[must_use]
    pub fn sub_lines(&self, index: usize) -> &[SubInvoiceLine] {
        self.sub_invoice_lines
            .iter()
            .find(|(i, _)| *i == index)
            .map_or(&[], |(_, v)| v.as_slice())
    }

    /// `Σ BT-DEX-002`, as `BR-DEX-09` needs it.
    ///
    /// # Errors
    /// [`crate::AmountError::Overflow`].
    pub fn third_party_total(&self) -> Result<InvoiceAmount, crate::AmountError> {
        InvoiceAmount::checked_sum(self.third_party_payments.iter().filter_map(|p| p.amount))
    }

    /// Whether anything is carried at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.advance_payments.is_empty()
            && self.sub_invoice_lines.is_empty()
            && self.third_party_payments.is_empty()
    }
}

/// The `BG-X-45` extension group, as a profile capability name.
pub const ADVANCE_PAYMENTS: &str = "BG-X-45";

/// The XRechnung Extension's `BG-DEX-01`, as a profile capability name.
pub const SUB_INVOICE_LINES: &str = "BG-DEX-01";

/// The XRechnung Extension's `BG-DEX-09`, as a profile capability name.
pub const THIRD_PARTY_PAYMENTS: &str = "BG-DEX-09";

/// `BG-DEX-01` SUB INVOICE LINE — a line beneath a line.
///
/// The XRechnung Extension's answer to invoices whose lines have internal
/// structure: a telecoms bill where one subscription line decomposes into calls,
/// or a construction invoice where one position decomposes into trades. Core
/// EN 16931 is deliberately flat — BG-25 has no child — so this cannot be a
/// CIUS and is not one.
///
/// Sub-lines nest: `BR-DEX-02` sums a line's BT-131 over its immediate
/// children, and a child may itself have children.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubInvoiceLine {
    /// The line itself, with the same business terms as BG-25.
    pub line: InvoiceLine,
    /// `BG-DEX-06` — its VAT information. Exactly one (`BR-DEX-03`).
    pub vat: Option<LineVat>,
    /// Its own sub-lines, if it decomposes further.
    pub children: Vec<SubInvoiceLine>,
}

impl SubInvoiceLine {
    /// The sum of this sub-tree's leaf amounts, as `BR-DEX-02` computes it.
    ///
    /// # Errors
    /// [`crate::AmountError::Overflow`].
    pub fn total(&self) -> Result<InvoiceAmount, crate::AmountError> {
        if self.children.is_empty() {
            return Ok(self.line.net_amount);
        }
        InvoiceAmount::checked_sum(
            self.children
                .iter()
                .map(Self::total)
                .collect::<Result<Vec<_>, _>>()?,
        )
    }
}

/// `BG-DEX-09` THIRD PARTY PAYMENT — an amount settled by someone who is not
/// the buyer.
///
/// Introduced for German digital health applications (DiGA), where a statutory
/// health insurer settles part of an invoice addressed to the insured. All three
/// terms are mandatory when the group is present (`BR-DEX-10`, `-11`, `-12`).
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ThirdPartyPayment {
    /// `BT-DEX-001` — the payment type.
    pub payment_type: Option<String>,
    /// `BT-DEX-002` — the amount, in BT-5's currency (`BR-DEX-14`).
    pub amount: Option<InvoiceAmount>,
    /// `BT-DEX-003` — a description of what was settled.
    pub description: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Percentage;
    use crate::invoice::Code;

    fn amount(s: &str) -> InvoiceAmount {
        InvoiceAmount::parse(s).unwrap()
    }

    #[test]
    fn an_advance_states_the_tax_it_contains() {
        let a = AdvancePayment {
            gross: amount("446.25"),
            received_on: Some(Date::parse("2026-03-31").unwrap()),
            tax: vec![VatBreakdown {
                taxable_amount: amount("375.00"),
                tax_amount: amount("71.25"),
                category: Code::new("S"),
                rate: Some(Percentage::new(rust_decimal::dec!(19))),
                exemption_reason: None,
                exemption_reason_code: None,
            }],
            reference: Some(DocumentReference::new("AB-1")),
            reference_date: None,
        };
        assert_eq!(a.tax_total().unwrap(), amount("71.25"));
        // gross = net + tax, which is what makes BT-113 derivable from it.
        assert_eq!(
            a.tax[0]
                .taxable_amount
                .checked_add(a.tax[0].tax_amount)
                .unwrap(),
            a.gross
        );
    }

    #[test]
    fn populated_names_only_what_is_there() {
        assert!(Extensions::default().is_empty());
        assert!(Extensions::default().populated().is_empty());
    }
}
