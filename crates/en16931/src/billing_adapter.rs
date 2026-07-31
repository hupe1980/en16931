//! Conversion from [`billing::BillingDocument`] — behind the `billing` feature.
//!
//! # Why there is no `TryFrom`
//!
//! A `BillingDocument` has no seller (BR-06), no buyer (BR-07), no postal
//! addresses (BR-08/BR-10), no country codes (BR-09/BR-11), no electronic
//! addresses (BR-62/BR-63) and no item names — `LineItem::description` is not
//! BT-153, it is display text. A `TryFrom` would fail on every input that is not
//! pathologically pre-annotated, which makes it a trait impl whose only
//! behaviour is `Err` while inviting the reader to believe conversion is total.
//!
//! So: a builder that takes the document for its *arithmetic* and the caller for
//! everything the standard needs and a calculation engine has no business
//! knowing.
//!
//! ```no_run
//! # use en16931::billing_adapter::FromBilling;
//! # fn demo(doc: &billing::BillingDocument, seller: en16931::invoice::Party,
//! #         buyer: en16931::invoice::Party) -> Result<(), Box<dyn std::error::Error>> {
//! let invoice = FromBilling::new(doc)
//!     .specification_id("urn:cen.eu:en16931:2017")
//!     .seller(seller)
//!     .buyer(buyer)
//!     .build()?;
//! # Ok(()) }
//! ```
//!
//! # The three traps, and where they went
//!
//! Each of these was a genuine problem in `billing` 0.8 that the adapter had to
//! work around with heuristics. All three were fixed upstream, in the only place
//! they *could* be fixed — the engine knows which layer produced which position
//! and which predicate selected its base; an adapter looking at a finished
//! document never can.
//!
//! | Trap | Where it went |
//! |---|---|
//! | `tax_total` is **not** BT-110 — a levy is a BG-21 charge inside the taxable base, so mapping it to BT-110 breaks `BR-CO-14` on every levy-bearing invoice | `vat_total()` / `charge_total()` |
//! | `net_total` is neither BT-106 nor BT-109 — it is `BT-106 − BT-107`, which EN 16931 has no term for | `line_total()` / `taxable_total()` |
//! | Per-line VAT attribution was unrecoverable after assembly | `LineItem::vat`, derived from `TaxLayer::covers` |
//!
//! # What the adapter still owes
//!
//! Two obligations `billing` deliberately leaves here:
//!
//! 1. **Call `verify_vat_attribution()`.** It is not part of `validate()`,
//!    because `AllocationRule` splits positions and breakdown with independent
//!    penny corrections and cannot preserve it. It is `BR-S-08`, and nothing
//!    downstream will catch a failure.
//! 2. **Reject `LineItem::vat == None` by name.** Lawful in `billing`, fatal in
//!    EN 16931 (`BR-CO-04`). Never default it.

use billing::{BillingDocument, LineItem, Sign};
use rust_decimal::Decimal;

use crate::extensions::{AdvancePayment, Extensions};
use crate::invoice::{
    Code, DocumentAllowanceCharge, DocumentTotals, Invoice, Item, LineAllowanceCharge, LineVat,
    Party, Period, PriceDetails, VatBreakdown,
};
use crate::{Date, InvoiceAmount, InvoiceLine, Percentage, Quantity, UnitPriceAmount};

// ── Errors ────────────────────────────────────────────────────────────────────

/// Conversion failed.
///
/// **Not** a validation finding. These say "the document you handed me cannot be
/// expressed as an EN 16931 invoice at all"; a [`crate::ValidationReport`] says
/// "this invoice does not satisfy BR-CO-14". Conflating them makes both harder
/// to act on.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ConversionError {
    /// An amount carries more than two decimals.
    ///
    /// Rounding here is the mistake `AmountScale` exists to prevent: it breaks
    /// `BR-CO-10` and `BR-CO-15`, which are exact equalities over sums. Fix it
    /// at the source with `.amount_scale(AmountScale::EN16931)` on the builder.
    #[error(
        "{what} = {value} needs more than two decimals; rebuild the document with \
         `.amount_scale(AmountScale::EN16931)` rather than rounding here"
    )]
    PrecisionLoss {
        /// Which field.
        what: String,
        /// Its value.
        value: String,
    },

    /// A position carries no VAT attribution.
    ///
    /// Lawful in `billing`; fatal in EN 16931 under `BR-CO-04`. Set it with
    /// `LineItemBuilder::vat`, or ensure a `TaxLayer` covers the position.
    #[error("position {index} ({description:?}) has no VAT attribution; BR-CO-04 requires BT-151")]
    NoVatAttribution {
        /// Which position.
        index: usize,
        /// Its description, for finding it.
        description: String,
    },

    /// A quantity unit could not be resolved to a UN/ECE Rec 20 code.
    ///
    /// Guessing produces an invoice that validates and describes the wrong
    /// thing — and unlike a wrong amount, nobody notices.
    #[error(
        "unit label {label:?} on position {index} has no BT-130 code; set `Quantity::code` \
         or extend the `UnitResolver`"
    )]
    UnresolvedUnit {
        /// Which position.
        index: usize,
        /// The unresolved label.
        label: String,
    },

    /// A date string is not an ISO 8601 calendar date.
    #[error("{field} = {value:?} is not an ISO 8601 calendar date")]
    UnparsableDate {
        /// Which field.
        field: &'static str,
        /// Its value.
        value: String,
    },

    /// The document has no currency, or still carries ISO 4217 `XXX`.
    #[error(
        "the document's currency is {0}; XXX means \"no currency involved\" and a document \
         still carrying it was never configured"
    )]
    NoCurrency(String),

    /// `billing` reported an arithmetic problem.
    #[error("billing: {0}")]
    Billing(String),
}

// ── Unit resolution ───────────────────────────────────────────────────────────

/// Maps a display unit label to a UN/ECE Rec 20 code, for documents that predate
/// `Quantity::code` or whose caller only set the label.
///
/// `billing::Quantity::unit` is display text and is load-bearing for
/// `PerUnitLevy` base matching; BT-130 is a code list. Different namespaces, and
/// the mapping is not mechanical — `"Stk"`, `"Stück"`, `"pcs"` and `"pieces"`
/// are all `H87`.
#[derive(Debug, Clone, Default)]
pub struct UnitResolver {
    extra: Vec<(String, String)>,
}

/// The mappings that are unambiguous enough to build in.
///
/// Deliberately short. A resolver that guessed widely would produce invoices
/// that validate and describe the wrong thing.
const BUILT_IN: &[(&str, &str)] = &[
    ("kWh", "KWH"),
    ("MWh", "MWH"),
    ("Wh", "WHR"),
    ("kW", "KWT"),
    ("m³", "MTQ"),
    ("m3", "MTQ"),
    ("m²", "MTK"),
    ("m", "MTR"),
    ("km", "KMT"),
    ("kg", "KGM"),
    ("g", "GRM"),
    ("t", "TNE"),
    ("l", "LTR"),
    ("h", "HUR"),
    ("d", "DAY"),
    ("Monat", "MON"),
    ("month", "MON"),
    ("Stk", "H87"),
    ("Stück", "H87"),
    ("pcs", "H87"),
    ("piece", "H87"),
    ("Stunde", "HUR"),
    ("%", "P1"),
    ("Pauschale", "C62"),
    ("one", "C62"),
];

impl UnitResolver {
    /// A resolver with only the built-in table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or override a mapping.
    #[must_use]
    pub fn with(mut self, label: impl Into<String>, code: impl Into<String>) -> Self {
        self.extra.push((label.into(), code.into()));
        self
    }

    /// Resolve a label, caller-supplied mappings first.
    #[must_use]
    pub fn resolve(&self, label: &str) -> Option<&str> {
        self.extra
            .iter()
            .find(|(l, _)| l == label)
            .map(|(_, c)| c.as_str())
            .or_else(|| BUILT_IN.iter().find(|(l, _)| *l == label).map(|(_, c)| *c))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Narrow a `billing` amount to two decimals **without rounding**.
fn amount(a: billing::Amount<5>, what: &str) -> Result<InvoiceAmount, ConversionError> {
    a.exact_to::<2>()
        .map_err(|_| ConversionError::PrecisionLoss {
            what: what.to_owned(),
            value: a.to_string(),
        })
        .map(|v: billing::Amount<2>| InvoiceAmount::from_minor_units(v.to_raw()))
}

/// `billing` stores VAT rates as fractions (`0.19`) because that is what you
/// multiply by; EN 16931 stores what you print (`19`). Convert once, here.
fn rate(fraction: Decimal) -> Percentage {
    Percentage::from_fraction(fraction).unwrap_or_else(|| Percentage::new(fraction))
}

fn date(s: Option<&str>, field: &'static str) -> Result<Option<Date>, ConversionError> {
    s.map(|v| {
        Date::parse(v).map_err(|_| ConversionError::UnparsableDate {
            field,
            value: v.to_owned(),
        })
    })
    .transpose()
}

fn period(
    p: Option<&billing::Period>,
    what: &'static str,
) -> Result<Option<Period>, ConversionError> {
    p.map(|p| {
        Ok(Period {
            start: date(Some(&p.from), what)?,
            end: date(Some(&p.to), what)?,
        })
    })
    .transpose()
}

fn line_vat(v: Option<&billing::vat::LineVat>) -> Option<LineVat> {
    v.map(|v| LineVat {
        category: Code::new(v.category.code()),
        // Category `O` states no rate at all — BR-O-05/06/07 say the element
        // "shall not contain" it, where every other zero-tax category says it
        // "shall be 0". `billing` stores a plain `Decimal`, so an `O` position
        // holds `0`; `states_rate` is what tells us to drop it.
        rate: crate::VatCategory::from_code(v.category.code())
            .is_none_or(crate::VatCategory::states_rate)
            .then(|| rate(v.rate)),
    })
}

fn allowance_charge(item: &LineItem) -> (Option<InvoiceAmount>, Option<Percentage>, Option<Code>) {
    match &item.allowance_charge {
        Some(ac) => (
            ac.base_amount
                .and_then(|b| b.exact_to::<2>().ok())
                .map(|v: billing::Amount<2>| InvoiceAmount::from_minor_units(v.to_raw())),
            ac.percentage.map(Percentage::new),
            ac.reason_code.as_deref().map(Code::new),
        ),
        None => (None, None, None),
    }
}

// ── The builder ───────────────────────────────────────────────────────────────

/// Builds an [`Invoice`] from a [`BillingDocument`] plus the terms the standard
/// requires and a calculation engine does not hold.
pub struct FromBilling<'a> {
    doc: &'a BillingDocument,
    specification_id: Option<String>,
    seller: Party,
    buyer: Party,
    units: UnitResolver,
    verify_attribution: bool,
}

impl<'a> FromBilling<'a> {
    /// Start from a document. Borrowed, not consumed — callers routinely keep the
    /// billing document for archival.
    #[must_use]
    pub fn new(doc: &'a BillingDocument) -> Self {
        Self {
            doc,
            specification_id: None,
            seller: Party::default(),
            buyer: Party::default(),
            units: UnitResolver::new(),
            verify_attribution: true,
        }
    }

    /// BT-24 — the profile this invoice declares.
    #[must_use]
    pub fn specification_id(mut self, id: impl Into<String>) -> Self {
        self.specification_id = Some(id.into());
        self
    }

    /// BG-4 — the seller.
    #[must_use]
    pub fn seller(mut self, seller: Party) -> Self {
        self.seller = seller;
        self
    }

    /// BG-7 — the buyer.
    #[must_use]
    pub fn buyer(mut self, buyer: Party) -> Self {
        self.buyer = buyer;
        self
    }

    /// Supply unit-code mappings for documents whose quantities carry only a
    /// display label.
    #[must_use]
    pub fn units(mut self, units: UnitResolver) -> Self {
        self.units = units;
        self
    }

    /// Skip `verify_vat_attribution`.
    ///
    /// Only correct for a document produced by `AllocationRule`, which splits
    /// positions and breakdown with independent penny corrections and therefore
    /// cannot preserve BR-S-08 exactly. For anything else, leaving this on is
    /// what catches a mis-tagged tax layer before the counterparty does.
    #[must_use]
    pub fn allow_unverified_attribution(mut self) -> Self {
        self.verify_attribution = false;
        self
    }

    /// Convert.
    ///
    /// # Errors
    /// [`ConversionError`] when the document cannot be expressed as an
    /// EN 16931 invoice at all. Whether the *result* is valid is a separate
    /// question — run [`crate::validate`] on it.
    pub fn build(self) -> Result<Invoice, ConversionError> {
        let doc = self.doc;

        // Order matters, and it is cheapest-and-most-unambiguous first. A
        // document that was never given a currency should be told so, not told
        // about its VAT attribution: `verify_vat_attribution` is a deep
        // arithmetic check and its message, while correct, is the least
        // actionable thing a caller with a configuration problem can be handed.
        let currency = doc.currency();
        if currency.is_unset() {
            return Err(ConversionError::NoCurrency(currency.code().to_owned()));
        }

        if self.verify_attribution {
            doc.verify_vat_attribution()
                .map_err(|e| ConversionError::Billing(e.to_string()))?;
        }

        // Totals first: `self` is consumed by the party moves below, and the
        // borrow checker is right that reading it afterwards is a mistake
        // waiting to happen.
        let totals = self.totals()?;

        // Struct-update rather than default-then-assign: `Invoice` is
        // `#[non_exhaustive]`, but this crate is inside its own boundary, so the
        // functional-update form is available here and states every mapped term
        // in one place.
        let mut inv = Invoice {
            specification_id: self.specification_id.clone(),
            number: Some(doc.meta.invoice_number.clone()).filter(|s| !s.is_empty()),
            issue_date: date(doc.meta.issue_date.as_deref(), "issue_date")?,
            due_date: date(doc.meta.due_date.as_deref(), "due_date")?,
            type_code: Some(Code::new(doc.meta.kind.code().to_string())),
            currency: Some(Code::new(currency.code())),
            invoicing_period: period(doc.meta.period.as_ref(), "period")?,
            // billing has no BT-21 analogue, so the notes arrive uncoded.
            notes: doc
                .meta
                .notes
                .iter()
                .map(crate::invoice::InvoiceNote::new)
                .collect(),
            seller: self.seller.clone(),
            buyer: self.buyer.clone(),
            ..Default::default()
        };

        // BG-25 — the net positions become invoice lines.
        for (i, item) in doc.net_positions().iter().enumerate() {
            inv.lines.push(self.line(i, item)?);
        }

        // BG-20 — discounts. Allowances are stated **positive**; `billing`
        // carries them as credits.
        for (i, item) in doc.discount_positions().iter().enumerate() {
            let (base, pct, reason_code) = allowance_charge(item);
            inv.allowances.push(DocumentAllowanceCharge {
                amount: amount(
                    item.net_amount
                        .checked_neg()
                        .map_err(|e| ConversionError::Billing(e.to_string()))?,
                    &format!("discount[{i}] BT-92"),
                )?,
                base_amount: base,
                percentage: pct,
                vat: line_vat(item.vat.as_ref()).ok_or_else(|| {
                    ConversionError::NoVatAttribution {
                        index: i,
                        description: item.description.clone(),
                    }
                })?,
                // BT-97 falls back to the position's description, which is what
                // a human reads on the rendered invoice anyway. BR-33 accepts
                // either the text or the code.
                reason: Some(item.description.clone()),
                reason_code,
            });
        }

        // BG-21 — the tax positions that are NOT VAT. A per-unit levy or a
        // commission is part of the taxable base, so EN 16931 calls it a
        // document level charge, not tax.
        for (i, item) in doc.charge_positions().enumerate() {
            let (base, pct, reason_code) = allowance_charge(item);
            inv.charges.push(DocumentAllowanceCharge {
                amount: amount(item.net_amount, &format!("charge[{i}] BT-99"))?,
                base_amount: base,
                percentage: pct,
                vat: line_vat(item.vat.as_ref()).ok_or_else(|| {
                    ConversionError::NoVatAttribution {
                        index: i,
                        description: item.description.clone(),
                    }
                })?,
                reason: Some(item.description.clone()),
                reason_code,
            });
        }

        // BG-23.
        for (i, e) in doc.tax_breakdown().iter().enumerate() {
            let category = Code::new(e.category.code());
            let states_rate = crate::VatCategory::from_code(e.category.code()).is_none_or(|_| true); // BT-119 is not BT-152; BR-48 governs it.
            inv.vat_breakdown.push(VatBreakdown {
                taxable_amount: amount(e.taxable_base, &format!("BG-23[{i}] BT-116"))?,
                tax_amount: amount(e.tax_amount, &format!("BG-23[{i}] BT-117"))?,
                category,
                rate: states_rate.then(|| rate(e.rate)),
                exemption_reason: e.exemption_reason.clone(),
                exemption_reason_code: e.exemption_reason_code.as_deref().map(Code::new),
            });
        }

        // The per-advance tax has no core business term. Carrying it into
        // `Extensions` rather than dropping it is the difference between a
        // lawful final invoice and a §14c Abs. 1 UStG liability — see
        // `crate::extensions`. `EN-EXT-01` then warns if the target profile
        // cannot represent it.
        inv.extensions = self.advances()?;

        inv.totals = totals;
        Ok(inv)
    }

    /// One BG-25 line.
    fn line(&self, i: usize, item: &LineItem) -> Result<InvoiceLine, ConversionError> {
        let quantity = item.quantity.as_ref();

        // BT-130. `Quantity::code` first; the resolver only as a fallback.
        let unit_code = match quantity {
            Some(q) => match q.code.as_deref() {
                Some(c) => c.to_owned(),
                None => self
                    .units
                    .resolve(&q.unit)
                    .ok_or_else(|| ConversionError::UnresolvedUnit {
                        index: i,
                        label: q.unit.clone(),
                    })?
                    .to_owned(),
            },
            // A flat charge with no quantity: `1` of `C62` ("one"), so BR-22 and
            // BR-23 hold and `1 × amount` reproduces the amount exactly.
            None => "C62".to_owned(),
        };

        let net = amount(item.net_amount, &format!("line[{i}] BT-131"))?;

        // The sign convention flips here. `billing` models a return as
        // `Sign::Credit` with a NON-negative quantity; EN 16931 puts the sign on
        // BT-129 and forbids a negative BT-146 (BR-27). Annex A.1.6 shows it:
        // 25 cases invoiced, −10 returned, one ordinary invoice.
        let (bt_129, bt_146) = match (quantity, item.unit_price.as_ref()) {
            (Some(q), Some(p)) => {
                let mut qty = q.value;
                let mut price = p.value;
                if item.sign == Sign::Credit {
                    qty = -qty;
                }
                // A negative unit price — lawful in `billing` for spot markets —
                // violates BR-27. Flip it onto the quantity instead of dropping
                // the line: `1000 kWh × −0.005` becomes `−1000 kWh × 0.005`.
                if price < Decimal::ZERO {
                    price = -price;
                    qty = -qty;
                }
                (Quantity::new(qty), UnitPriceAmount::new(price))
            }
            // Fixed amount: quantity 1 (or −1 for a credit) at the full amount.
            _ => {
                let one = if item.sign == Sign::Credit {
                    Decimal::NEGATIVE_ONE
                } else {
                    Decimal::ONE
                };
                let abs = net.into_decimal().abs();
                (Quantity::new(one), UnitPriceAmount::new(abs))
            }
        };

        Ok(InvoiceLine {
            id: (i + 1).to_string(),
            note: None,
            // BT-132 / BT-133 have no `billing` analogue: it is a financial
            // document and these are the buyer's procurement and bookkeeping
            // handles. A caller who has them sets them after conversion.
            order_line_reference: None,
            accounting_reference: None,
            object_identifier: None,
            quantity: bt_129,
            unit_code: Code::new(unit_code),
            net_amount: net,
            period: period(item.period.as_ref(), "line period")?,
            allowances: self.line_allowances(item, billing::AllowanceKind::Allowance)?,
            charges: self.line_allowances(item, billing::AllowanceKind::Charge)?,
            price: PriceDetails {
                net_price: bt_146,
                price_discount: item
                    .unit_price
                    .as_ref()
                    .and_then(|p| p.price_discount)
                    .map(UnitPriceAmount::new),
                gross_price: item
                    .unit_price
                    .as_ref()
                    .and_then(|p| p.gross_price)
                    .map(UnitPriceAmount::new),
                base_quantity: item
                    .unit_price
                    .as_ref()
                    .and_then(|p| p.base_quantity)
                    .map(Quantity::new),
                base_quantity_code: item
                    .unit_price
                    .as_ref()
                    .and_then(|p| p.base_quantity_code.clone())
                    .map(Code::new),
            },
            vat: line_vat(item.vat.as_ref()).ok_or_else(|| ConversionError::NoVatAttribution {
                index: i,
                description: item.description.clone(),
            })?,
            // BT-153. `description` is the only thing `billing` has, and an
            // unlabelled position is already rejected there, so this is always
            // non-empty — but BR-25 is checked by the engine regardless.
            item: Item {
                name: Some(item.description.clone()),
                ..Default::default()
            },
        })
    }

    /// BG-27 or BG-28 for one line.
    fn line_allowances(
        &self,
        item: &LineItem,
        kind: billing::AllowanceKind,
    ) -> Result<Vec<LineAllowanceCharge>, ConversionError> {
        item.line_allowances
            .iter()
            .filter(|a| a.kind == kind)
            .map(|a| {
                Ok(LineAllowanceCharge {
                    amount: amount(a.amount, "line allowance/charge")?,
                    base_amount: a
                        .base_amount
                        .map(|b| amount(b, "line allowance/charge base"))
                        .transpose()?,
                    percentage: a.percentage.map(Percentage::new),
                    reason: a.reason.clone(),
                    reason_code: a.reason_code.as_deref().map(Code::new),
                })
            })
            .collect()
    }

    /// ZUGFeRD EXTENDED `BG-X-45`, from `billing`'s itemised advances.
    ///
    /// Empty for an ordinary invoice, and empty for a *residual* invoice, which
    /// bills only the remainder and deliberately lists no advances. Non-empty
    /// makes this a **final invoice**.
    fn advances(&self) -> Result<Extensions, ConversionError> {
        let billing_err = |e: billing::BillingError| ConversionError::Billing(e.to_string());
        let mut out = Vec::new();
        for a in self.doc.advances() {
            out.push(AdvancePayment {
                gross: amount(a.checked_gross().map_err(billing_err)?, "BT-X-291")?,
                received_on: date(a.received_on(), "advance received_on")?,
                tax: a
                    .tax()
                    .iter()
                    .map(|e| {
                        Ok(VatBreakdown {
                            taxable_amount: amount(e.taxable_base, "BG-X-46 base")?,
                            tax_amount: amount(e.tax_amount, "BG-X-46 tax")?,
                            category: Code::new(e.category.code()),
                            rate: Some(rate(e.rate)),
                            exemption_reason: e.exemption_reason.clone(),
                            exemption_reason_code: e
                                .exemption_reason_code
                                .as_deref()
                                .map(Code::new),
                        })
                    })
                    .collect::<Result<Vec<_>, ConversionError>>()?,
                reference: a.reference().map(crate::DocumentReference::new),
                reference_date: date(a.reference_date(), "advance reference_date")?,
            });
        }
        Ok(Extensions {
            // `billing` models neither sub-lines nor third-party settlement.
            sub_invoice_lines: Vec::new(),
            third_party_payments: Vec::new(),
            advance_payments: out,
        })
    }

    /// BG-22, taking each term from the accessor that actually means it.
    fn totals(&self) -> Result<DocumentTotals, ConversionError> {
        let doc = self.doc;
        let billing_err = |e: billing::BillingError| ConversionError::Billing(e.to_string());

        let line_total = amount(doc.line_total().map_err(billing_err)?, "BT-106")?;
        let allowances = doc.discount_total();
        let charges = doc.charge_total().map_err(billing_err)?;
        let vat = doc.vat_total().map_err(billing_err)?;

        Ok(DocumentTotals {
            line_total,
            // Absent is not zero: BT-107 may be omitted only when there are no
            // allowances at all, and BR-CO-13 branches on its presence.
            allowance_total: (!doc.discount_positions().is_empty())
                .then(|| amount(allowances.checked_neg().map_err(billing_err)?, "BT-107"))
                .transpose()?,
            charge_total: (doc.charge_positions().next().is_some())
                .then(|| amount(charges, "BT-108"))
                .transpose()?,
            taxable_total: amount(doc.taxable_total().map_err(billing_err)?, "BT-109")?,
            vat_total: (!doc.tax_breakdown().is_empty())
                .then(|| amount(vat, "BT-110"))
                .transpose()?,
            vat_total_accounting: None,
            gross_total: amount(doc.gross_total(), "BT-112")?,
            paid: (!doc.prepaid().is_zero())
                .then(|| amount(doc.prepaid(), "BT-113"))
                .transpose()?,
            rounding: (!doc.rounding().is_zero())
                .then(|| amount(doc.rounding(), "BT-114"))
                .transpose()?,
            due: amount(doc.amount_due().map_err(billing_err)?, "BT-115")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_resolver_prefers_caller_mappings_and_refuses_to_guess() {
        let r = UnitResolver::new().with("kWh", "XXX").with("Kiste", "BX");
        assert_eq!(r.resolve("kWh"), Some("XXX"), "caller overrides built-in");
        assert_eq!(r.resolve("Kiste"), Some("BX"));
        assert_eq!(r.resolve("Stk"), Some("H87"), "built-in still reachable");
        assert_eq!(r.resolve("Furlong"), None, "never guesses");
    }

    #[test]
    fn every_built_in_unit_code_is_real() {
        for (label, code) in BUILT_IN {
            assert!(
                crate::codes::contains(crate::codes::generated::UNIT_CODES, code),
                "{label} maps to {code}, which is not in BR-CL-23's list"
            );
        }
    }

    #[test]
    fn rates_convert_from_fraction_to_per_cent() {
        // The most common transcription bug when bridging the two crates.
        assert_eq!(
            rate(rust_decimal::dec!(0.19)),
            Percentage::new(rust_decimal::dec!(19))
        );
        assert_eq!(
            rate(rust_decimal::dec!(0.075)),
            Percentage::new(rust_decimal::dec!(7.5))
        );
        assert_eq!(rate(Decimal::ZERO), Percentage::ZERO);
    }
}
