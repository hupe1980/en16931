//! Data with no core business term — an Extension in the sense of EN 16931-1
//! §4.3 (a CIUS restricts, an Extension adds). Nothing here shadows a core term.
//!
//! `EN-EXT-01` warns when the target profile cannot carry what is populated,
//! e.g. itemised advance payments with their tax (§14 Abs. 5 Satz 2 UStG),
//! which core BT-113 cannot express.

use crate::invoice::{InvoiceLine, LineVat, VatBreakdown};
use crate::{Date, DocumentReference, InvoiceAmount};

/// One previously invoiced and received advance payment, with its tax.
///
/// Mirrors ZUGFeRD / Factur-X EXTENDED `BG-X-45`.
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
    /// `BG-X-46` — the tax it contains, per `(category, rate)`, as §14 Abs. 5
    /// Satz 2 UStG requires a final invoice to state.
    #[cfg_attr(feature = "serde", serde(default))]
    pub tax: Vec<VatBreakdown>,
    /// `BT-X-558` — the advance invoice's number.
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
/// A format writes what its profile can represent; `EN-EXT-01` warns about the
/// rest. Only the XRechnung Extension profile declares any (`BG-DEX-01`,
/// `BG-DEX-09`).
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Extensions {
    /// ZUGFeRD / Factur-X EXTENDED `BG-X-45`. Non-empty makes this a final
    /// invoice: totals and VAT breakdown describe the whole supply.
    #[cfg_attr(feature = "serde", serde(default))]
    pub advance_payments: Vec<AdvancePayment>,
    /// XRechnung Extension `BG-DEX-01`, keyed by the zero-based index of the
    /// parent line in [`crate::Invoice::lines`].
    #[cfg_attr(feature = "serde", serde(default))]
    pub sub_invoice_lines: Vec<(usize, Vec<SubInvoiceLine>)>,
    /// XRechnung Extension `BG-DEX-09`. `BR-DEX-09` replaces `BR-CO-16`,
    /// adding these amounts into BT-115.
    #[cfg_attr(feature = "serde", serde(default))]
    pub third_party_payments: Vec<ThirdPartyPayment>,
}

impl Extensions {
    /// The extension group names populated here; `EN-EXT-01` compares them
    /// with [`crate::Profile::extensions`].
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
/// Sub-lines nest; `BR-DEX-02` sums a line's BT-131 over its children.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubInvoiceLine {
    /// The line itself, with the same business terms as BG-25.
    ///
    /// Any sub-line, grouping or childless, may leave `line.quantity` and
    /// `line.price` at their default; see
    /// [`quantity_stated`](Self::quantity_stated) and
    /// [`price_stated`](Self::price_stated).
    pub line: InvoiceLine,
    /// `BG-DEX-06` — its VAT information. Exactly one (`BR-DEX-03`), on every
    /// sub-line, grouping or not.
    pub vat: Option<LineVat>,
    /// Whether `BT-129` (and its unit, BT-130) is present *and valid*.
    ///
    /// `BR-22`'s context is `cac:InvoiceLine | cac:CreditNoteLine`
    /// (CEN's EN 16931 UBL Schematron), never `cac:SubInvoiceLine`: it does
    /// not apply to a sub-line at all, grouping or childless, confirmed
    /// against the KoSIT validator. `false` with `line.quantity` at its
    /// `Quantity::default()` (zero) means `cbc:InvoicedQuantity` /
    /// `cbc:CreditedQuantity` is simply absent — a quantity is never actually
    /// zero and absent. A *present but empty* element is different: the UBL
    /// schema requires non-empty `xs:decimal` content wherever the element
    /// appears at all (KoSIT 406, `cvc-datatype-valid.1.2.1`), so a reader
    /// rejects that (`Kind::Malformed`) rather than reading `quantity_stated`
    /// as `false` for a stated zero.
    #[cfg_attr(feature = "serde", serde(default = "stated_by_default"))]
    pub quantity_stated: bool,
    /// Whether `BG-29` (`BT-146`, the item net price) is present *and
    /// valid*, on the same terms as
    /// [`quantity_stated`](Self::quantity_stated) but for `BR-26`: never
    /// required on a sub-line, but a stated `cac:Price` must have a valid
    /// `cbc:PriceAmount` (missing or empty is rejected, KoSIT 406,
    /// `cvc-complex-type.2.4.b`). `false` means `line.price` is
    /// `PriceDetails::default()`.
    #[cfg_attr(feature = "serde", serde(default = "stated_by_default"))]
    pub price_stated: bool,
    /// Its own sub-lines, if it decomposes further.
    #[cfg_attr(feature = "serde", serde(default))]
    pub children: Vec<SubInvoiceLine>,
}

/// Serde default for [`SubInvoiceLine::quantity_stated`] and
/// [`SubInvoiceLine::price_stated`]: data from before these fields existed
/// carried both, so an absent field reads as `true`.
#[cfg(feature = "serde")]
const fn stated_by_default() -> bool {
    true
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
/// Used for German digital health applications (DiGA). All three terms are
/// mandatory when the group is present (`BR-DEX-10`, `-11`, `-12`).
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
        // gross = net + tax.
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
