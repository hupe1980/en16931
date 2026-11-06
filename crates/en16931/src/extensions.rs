//! Data with no core business term — an Extension in the sense of EN 16931-1
//! §4.3 (a CIUS restricts, an Extension adds). Nothing here shadows a core term.
//!
//! `EN-EXT-01` warns when the target profile cannot carry what is populated,
//! e.g. itemised advance payments with their tax (§14 Abs. 5 Satz 2 UStG),
//! which core BT-113 cannot express.

use crate::invoice::{
    Code, InvoiceLine, Item, LineAllowanceCharge, LineVat, Period, PriceDetails, VatBreakdown,
};
use crate::{Date, DocumentReference, Identifier, InvoiceAmount, Quantity};

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
/// Its terms follow the XRechnung 3.0.2 specification, section 16. Sub-lines
/// nest; `BR-DEX-02` sums a line's BT-131 over its children.
///
/// The specification's table gives BT-129, BT-130 and BG-DEX-07 cardinality 1
/// on a sub-line, but no KoSIT rule checks it, `BR-22`, `BR-23` and `BR-26`
/// hold on `cac:InvoiceLine | cac:CreditNoteLine` only, and KoSIT's own
/// `ubl-inv-br-dex-03-test.xml` leaves them out. So [`quantity`](Self::quantity)
/// and [`price`](Self::price) are `None` when a sub-line does not state them.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubInvoiceLine {
    /// BT-126 — the sub-line's identifier.
    pub id: String,
    /// BT-127 — a note.
    pub note: Option<String>,
    /// BT-128 — the invoiced object identifier.
    pub object_identifier: Option<Identifier>,
    /// BT-129 with its unit, BT-130; `None` when the sub-line states no
    /// quantity.
    pub quantity: Option<SubLineQuantity>,
    /// BT-131 — the net amount.
    pub net_amount: InvoiceAmount,
    /// BT-132 — the referenced purchase order line.
    pub order_line_reference: Option<DocumentReference>,
    /// BT-133 — the buyer accounting reference.
    pub accounting_reference: Option<String>,
    /// `BG-DEX-05` — the sub-line's period.
    pub period: Option<Period>,
    /// `BG-DEX-03` — its allowances.
    #[cfg_attr(feature = "serde", serde(default))]
    pub allowances: Vec<LineAllowanceCharge>,
    /// `BG-DEX-04` — its charges.
    #[cfg_attr(feature = "serde", serde(default))]
    pub charges: Vec<LineAllowanceCharge>,
    /// `BG-DEX-02` — the item.
    pub item: Item,
    /// `BG-DEX-06` — its VAT information. Exactly one (`BR-DEX-03`), on every
    /// sub-line, grouping or not; `None` when the document carries none or
    /// several, so the rule fires.
    pub vat: Option<LineVat>,
    /// `BG-DEX-07` — its price; `None` when the sub-line states none.
    pub price: Option<PriceDetails>,
    /// Its own sub-lines, if it decomposes further.
    #[cfg_attr(feature = "serde", serde(default))]
    pub children: Vec<SubInvoiceLine>,
}

/// A sub-line's BT-129 with its unit, BT-130, which a sub-line may leave out
/// even when it states the quantity.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubLineQuantity {
    /// BT-129 — the quantity.
    pub value: Quantity,
    /// BT-130 — its unit of measure.
    pub unit_code: Option<Code>,
}

impl From<InvoiceLine> for SubInvoiceLine {
    /// The sub-line stating every term `line` carries, its quantity, unit,
    /// price and VAT information included, and no children.
    fn from(line: InvoiceLine) -> Self {
        Self {
            id: line.id,
            note: line.note,
            object_identifier: line.object_identifier,
            quantity: Some(SubLineQuantity {
                value: line.quantity,
                // An empty code is no unit: a sub-line's BT-130 is optional.
                unit_code: Some(line.unit_code).filter(|u| !u.as_str().is_empty()),
            }),
            net_amount: line.net_amount,
            order_line_reference: line.order_line_reference,
            accounting_reference: line.accounting_reference,
            period: line.period,
            allowances: line.allowances,
            charges: line.charges,
            item: line.item,
            vat: Some(line.vat),
            price: Some(line.price),
            children: vec![],
        }
    }
}

impl SubInvoiceLine {
    /// The sum of this sub-tree's leaf amounts, as `BR-DEX-02` computes it.
    ///
    /// # Errors
    /// [`crate::AmountError::Overflow`].
    pub fn total(&self) -> Result<InvoiceAmount, crate::AmountError> {
        if self.children.is_empty() {
            return Ok(self.net_amount);
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
