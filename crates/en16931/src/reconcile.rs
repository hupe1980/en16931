//! Deriving BG-23 and BG-22 from the lines — the arithmetic every hand-mapper
//! otherwise re-implements.
//!
//! # Why this is in the model crate and not in `billing`
//!
//! `billing` computes an invoice *forwards*: quantities and prices in, positions
//! and tax layers out. An ERP that already has its own engine does not want a
//! second one — it has the numbers. What it does not have is EN 16931's
//! **presentation** of them: which rows form one BG-23 group, whether a category
//! states a rate at all, which of BT-107 and BT-108 must be absent rather than
//! zero, and where the rounding goes.
//!
//! That is not arithmetic, it is the standard's own bookkeeping, and every
//! hand-mapper re-derives it from the rule texts. `BR-CO-10` … `BR-CO-16`, the
//! `-08` and `-09` rows of all nine category families, and `BR-CO-18` are one
//! function.
//!
//! ```
//! use en16931::{Invoice, InvoiceAmount, validate};
//! use en16931::reconcile::Reconciler;
//!
//! # fn demo(mut inv: Invoice) -> Result<(), Box<dyn std::error::Error>> {
//! // The lines, the parties and the allowances are yours. BG-23 and BG-22 are
//! // not: they are a function of the lines, and this computes that function.
//! Reconciler::new().apply(&mut inv)?;
//!
//! assert!(validate(&inv).fatal().all(|f| !f.rule.starts_with("BR-CO-1")));
//! # Ok(()) }
//! ```
//!
//! # Where it stops, on purpose
//!
//! It computes the amounts. It does **not** invent a BT-120 exemption reason
//! (`BR-E-10` and its four siblings require one, and only the seller knows what
//! it is — see [`Reconciler::exemption`]), a due date, or a VAT identifier.
//! Reconciling and validating are different jobs; run [`crate::validate`]
//! afterwards, because a reconciled invoice is *arithmetically* consistent and
//! nothing more.
//!
//! # It does not disagree with the validator
//!
//! Grouping comes from [`crate::validation::rules::category::profile`] — the
//! same table the `-08` rows check against — rather than from a second reading
//! of the standard. `every_category_reconciles_to_a_valid_breakdown` builds one
//! invoice per category and asserts the result carries no arithmetic finding,
//! so the two cannot drift apart silently.

use rust_decimal::{Decimal, RoundingStrategy};

use crate::bt::{Group, Path};
use crate::invoice::{Code, DocumentTotals, Invoice, VatBreakdown};
use crate::validation::rules::category::{TaxRule, profile};
use crate::{InvoiceAmount, Percentage, VatCategory};

// ── Errors ────────────────────────────────────────────────────────────────────

/// Why an invoice could not be reconciled.
///
/// **Not** a validation finding. These say "the arithmetic cannot be performed
/// at all"; a [`crate::ValidationReport`] says "the arithmetic was performed and
/// disagrees". Every variant is something [`crate::validate`] would also report,
/// which is deliberate — a caller who ignores these and validates anyway still
/// learns about them.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ReconcileError {
    /// A line, allowance or charge carries a VAT category outside UNCL 5305.
    ///
    /// Grouping and the tax rule both come from the category's semantics, so an
    /// unknown code leaves nothing to compute. `BR-CL-17` / `BR-CL-18` report
    /// the same thing at validation time; this is the earlier, louder version.
    #[error(
        "{at} carries VAT category {code:?}, which is not in UNCL 5305 — \
         BG-23 grouping and the tax rule both depend on the category (BR-CL-18)"
    )]
    UnknownCategory {
        /// Which element.
        at: Path,
        /// The code found.
        code: String,
    },

    /// A taxed category with no rate to multiply by.
    ///
    /// `S`, `L`, `M` and `B` derive BT-117 from BT-119. Defaulting an absent
    /// rate to zero would produce a breakdown that balances and charges no tax,
    /// which is the one wrong answer nobody notices.
    #[error(
        "{at} is VAT category {category} at no rate; a taxed category derives its tax \
         amount from the rate, and defaulting it to zero would silently under-declare VAT"
    )]
    MissingRate {
        /// Which element.
        at: Path,
        /// The category that needs one.
        category: VatCategory,
    },

    /// A sum did not fit in [`InvoiceAmount`], or an intermediate overflowed.
    #[error("{term} overflowed while reconciling; the amounts involved are not representable")]
    Overflow {
        /// Which business term was being computed.
        term: &'static str,
    },
}

// ── The result ────────────────────────────────────────────────────────────────

/// What reconciliation produced, before it is put back on an invoice.
///
/// Returned by [`Reconciler::compute`] for a caller who wants to inspect or
/// diff the numbers rather than overwrite theirs — which is the useful shape
/// when the question is *"does my ERP agree with EN 16931?"* rather than
/// *"give me the numbers"*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reconciled {
    /// BG-23, one entry per group, ordered by category then rate so two runs
    /// over equal input produce equal output.
    pub vat_breakdown: Vec<VatBreakdown>,
    /// BG-22.
    pub totals: DocumentTotals,
}

// ── The reconciler ────────────────────────────────────────────────────────────

/// An exemption reason to attach to a category's BG-23 group.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Exemption {
    category: String,
    text: Option<String>,
    code: Option<String>,
}

/// Computes BG-23 and BG-22 from an invoice's lines, allowances and charges.
///
/// Defaults are the common case: no prepayment, no rounding amount, and any
/// exemption reasons already present on the invoice's own BG-23 preserved.
#[derive(Debug, Clone, Default)]
pub struct Reconciler {
    exemptions: Vec<Exemption>,
    paid: Option<InvoiceAmount>,
    rounding: Option<InvoiceAmount>,
    vat_total_accounting: Option<InvoiceAmount>,
}

impl Reconciler {
    /// A reconciler with nothing configured.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// BT-120 / BT-121 for one VAT category — what `BR-E-10` and its four
    /// siblings require and no amount of arithmetic can supply.
    ///
    /// Five categories *must* carry a reason (`E`, `AE`, `K`, `G`, `O`) and four
    /// *must not* (`S`, `Z`, `L`, `M`). This sets it for the categories that
    /// need it; a reason given for a category that forbids one is **dropped**
    /// rather than written, because writing it would turn a caller's mistake
    /// into a `BR-S-10` finding on a document that is otherwise correct.
    ///
    /// Pass `None` for either half — the rules accept the text or the code.
    ///
    /// ```
    /// use en16931::reconcile::Reconciler;
    ///
    /// let r = Reconciler::new()
    ///     .exemption("AE", None, Some("VATEX-EU-AE"))      // reverse charge
    ///     .exemption("K", Some("Innergemeinschaftliche Lieferung"), None);
    /// # let _ = r;
    /// ```
    #[must_use]
    pub fn exemption(
        mut self,
        category: impl Into<String>,
        text: Option<&str>,
        code: Option<&str>,
    ) -> Self {
        self.exemptions.push(Exemption {
            category: category.into(),
            text: text.map(str::to_owned),
            code: code.map(str::to_owned),
        });
        self
    }

    /// BT-113 — an amount already paid, subtracted from BT-115 by `BR-CO-16`.
    ///
    /// Absent is not zero: BT-113 is omitted when nothing was prepaid, and
    /// `BR-CO-16` branches on its presence.
    #[must_use]
    pub fn paid(mut self, amount: InvoiceAmount) -> Self {
        self.paid = Some(amount);
        self
    }

    /// BT-114 — a rounding amount, added to BT-115 by `BR-CO-16`.
    #[must_use]
    pub fn rounding(mut self, amount: InvoiceAmount) -> Self {
        self.rounding = Some(amount);
        self
    }

    /// BT-111 — the total VAT in the accounting currency, required by `BR-53`
    /// whenever BT-6 is present.
    ///
    /// Not derived: it is BT-110 converted at a rate this crate does not have
    /// and must not invent.
    #[must_use]
    pub fn vat_total_accounting(mut self, amount: InvoiceAmount) -> Self {
        self.vat_total_accounting = Some(amount);
        self
    }

    /// Compute BG-23 and BG-22 without touching the invoice.
    ///
    /// # Errors
    /// [`ReconcileError`] when the arithmetic cannot be performed — an unknown
    /// VAT category, a taxed category with no rate, or an overflow.
    pub fn compute(&self, inv: &Invoice) -> Result<Reconciled, ReconcileError> {
        let vat_breakdown = self.breakdown(inv)?;
        let totals = self.totals(inv, &vat_breakdown)?;
        Ok(Reconciled {
            vat_breakdown,
            totals,
        })
    }

    /// Compute and write both groups back onto the invoice.
    ///
    /// Everything else is left alone — this replaces exactly
    /// [`Invoice::vat_breakdown`] and [`Invoice::totals`].
    ///
    /// # Errors
    /// As [`compute`](Self::compute). The invoice is **unchanged** on error:
    /// both groups are computed before either is written.
    pub fn apply(&self, inv: &mut Invoice) -> Result<(), ReconcileError> {
        let r = self.compute(inv)?;
        inv.vat_breakdown = r.vat_breakdown;
        inv.totals = r.totals;
        Ok(())
    }

    // ── BG-23 ────────────────────────────────────────────────────────────────

    /// One entry per group, grouped exactly as the `-08` rows check.
    fn breakdown(&self, inv: &Invoice) -> Result<Vec<VatBreakdown>, ReconcileError> {
        // Collect the keys first, in document order, then sort — so the output
        // is a function of the input and not of iteration order.
        let mut keys: Vec<(VatCategory, Option<Percentage>)> = Vec::new();
        for (cat, rate, at) in content(inv) {
            let semantics = VatCategory::from_code(cat.as_str()).ok_or_else(|| {
                ReconcileError::UnknownCategory {
                    at,
                    code: cat.as_str().to_owned(),
                }
            })?;
            if semantics.carries_tax() && rate.is_none() {
                return Err(ReconcileError::MissingRate {
                    at,
                    category: semantics,
                });
            }
            let key = (semantics, group_rate(semantics, rate));
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        keys.sort_by(|a, b| a.0.code().cmp(b.0.code()).then(a.1.cmp(&b.1)));

        keys.into_iter()
            .map(|(category, rate)| self.group(inv, category, rate))
            .collect()
    }

    /// One BG-23 entry.
    fn group(
        &self,
        inv: &Invoice,
        category: VatCategory,
        rate: Option<Percentage>,
    ) -> Result<VatBreakdown, ReconcileError> {
        let p = profile(category);
        let belongs = |c: &Code, r: Option<Percentage>| {
            VatCategory::from_code(c.as_str()) == Some(category)
                && (!p.grouped_by_rate() || group_rate(category, r) == rate)
        };

        // BT-116 = Σ line net + Σ document charges − Σ document allowances, over
        // the rows in this group. Every term is already two decimals, so the sum
        // is exact in minor units and `BR-*-08` holds with no tolerance used.
        let positive = inv
            .lines
            .iter()
            .filter(|l| belongs(&l.vat.category, l.vat.rate))
            .map(|l| l.net_amount)
            .chain(
                inv.charges
                    .iter()
                    .filter(|c| belongs(&c.vat.category, c.vat.rate))
                    .map(|c| c.amount),
            );
        let negative = inv
            .allowances
            .iter()
            .filter(|a| belongs(&a.vat.category, a.vat.rate))
            .map(|a| a.amount);

        let taxable_amount = InvoiceAmount::checked_sum(positive)
            .and_then(|pos| {
                InvoiceAmount::checked_sum(negative).and_then(|neg| pos.checked_sub(neg))
            })
            .map_err(|_| ReconcileError::Overflow { term: "BT-116" })?;

        let tax_amount = match p.tax {
            // `Z`, `E`, `AE`, `K`, `G`, `O` — "shall equal 0 (zero)", exactly.
            TaxRule::Zero => InvoiceAmount::ZERO,
            TaxRule::Derived => tax_on(taxable_amount, rate)?,
        };

        let (exemption_reason, exemption_reason_code) = self.reason_for(inv, category);
        Ok(VatBreakdown {
            taxable_amount,
            tax_amount,
            category: Code::new(category.code()),
            rate: breakdown_rate(rate),
            exemption_reason,
            exemption_reason_code,
        })
    }

    /// BT-120 / BT-121 for a category: what the caller configured, else what the
    /// invoice's existing BG-23 already carried, else nothing.
    ///
    /// Preserving the invoice's own reasons is what makes `apply` safe to call
    /// on an invoice a caller has already filled in by hand — the numbers are
    /// recomputed and the prose survives.
    fn reason_for(&self, inv: &Invoice, category: VatCategory) -> (Option<String>, Option<Code>) {
        if category.forbids_exemption_reason() {
            return (None, None);
        }
        if let Some(e) = self
            .exemptions
            .iter()
            .find(|e| e.category == category.code())
        {
            return (e.text.clone(), e.code.as_deref().map(Code::new));
        }
        inv.vat_breakdown
            .iter()
            .find(|e| e.semantics() == Some(category) && e.has_exemption_reason())
            .map_or((None, None), |e| {
                (e.exemption_reason.clone(), e.exemption_reason_code.clone())
            })
    }

    // ── BG-22 ────────────────────────────────────────────────────────────────

    /// The whole `BR-CO-10` … `BR-CO-16` chain, in the order it is defined.
    fn totals(
        &self,
        inv: &Invoice,
        breakdown: &[VatBreakdown],
    ) -> Result<DocumentTotals, ReconcileError> {
        let sum = |it: &mut dyn Iterator<Item = InvoiceAmount>, term| {
            InvoiceAmount::checked_sum(it).map_err(|_| ReconcileError::Overflow { term })
        };

        // BT-106.
        let line_total = sum(&mut inv.lines.iter().map(|l| l.net_amount), "BT-106")?;

        // BT-107 / BT-108 — **absent is not zero**. `BR-CO-11` and `-12` accept
        // an absent total only when the corresponding group is empty, and
        // `BR-CO-13` branches on presence.
        let allowance_total = if inv.allowances.is_empty() {
            None
        } else {
            Some(sum(&mut inv.allowances.iter().map(|a| a.amount), "BT-107")?)
        };
        let charge_total = if inv.charges.is_empty() {
            None
        } else {
            Some(sum(&mut inv.charges.iter().map(|c| c.amount), "BT-108")?)
        };

        // BT-109 = BT-106 − BT-107 + BT-108.
        let taxable_total = line_total
            .checked_sub(allowance_total.unwrap_or(InvoiceAmount::ZERO))
            .and_then(|v| v.checked_add(charge_total.unwrap_or(InvoiceAmount::ZERO)))
            .map_err(|_| ReconcileError::Overflow { term: "BT-109" })?;

        // BT-110 = Σ BT-117. Stated whenever there is a breakdown at all, even
        // when it sums to zero: `BR-CO-14` permits absence there, and every
        // deployed profile expects the element.
        let vat_total = if breakdown.is_empty() {
            None
        } else {
            Some(sum(&mut breakdown.iter().map(|e| e.tax_amount), "BT-110")?)
        };

        // BT-112 = BT-109 + BT-110.
        let gross_total = taxable_total
            .checked_add(vat_total.unwrap_or(InvoiceAmount::ZERO))
            .map_err(|_| ReconcileError::Overflow { term: "BT-112" })?;

        // BT-115 = BT-112 − BT-113 + BT-114.
        let due = gross_total
            .checked_sub(self.paid.unwrap_or(InvoiceAmount::ZERO))
            .and_then(|v| v.checked_add(self.rounding.unwrap_or(InvoiceAmount::ZERO)))
            .map_err(|_| ReconcileError::Overflow { term: "BT-115" })?;

        Ok(DocumentTotals {
            line_total,
            allowance_total,
            charge_total,
            taxable_total,
            vat_total,
            vat_total_accounting: self.vat_total_accounting,
            gross_total,
            paid: self.paid,
            rounding: self.rounding,
            due,
        })
    }
}

// ── Free functions ────────────────────────────────────────────────────────────

/// Reconcile with every default — the one-liner for the common case.
///
/// # Errors
/// As [`Reconciler::compute`].
pub fn reconcile(inv: &mut Invoice) -> Result<(), ReconcileError> {
    Reconciler::new().apply(inv)
}

/// Every VAT-bearing row in the document, with where it is.
fn content(inv: &Invoice) -> impl Iterator<Item = (&Code, Option<Percentage>, Path)> {
    let lines = inv
        .lines
        .iter()
        .enumerate()
        .map(|(i, l)| (&l.vat.category, l.vat.rate, Path::at(Group::Line, i)));
    let allowances = inv.allowances.iter().enumerate().map(|(i, a)| {
        (
            &a.vat.category,
            a.vat.rate,
            Path::at(Group::DocumentAllowance, i),
        )
    });
    let charges = inv.charges.iter().enumerate().map(|(i, c)| {
        (
            &c.vat.category,
            c.vat.rate,
            Path::at(Group::DocumentCharge, i),
        )
    });
    lines.chain(allowances).chain(charges)
}

/// The rate a group is keyed on.
///
/// For the six categories whose `-01` row says *"exactly one"*, every row in the
/// category belongs to one group whatever rate it states — so the key must not
/// depend on the rate, or a document mixing `Some(0)` and `None` on category `E`
/// would produce two groups and fail `BR-E-01`.
fn group_rate(category: VatCategory, rate: Option<Percentage>) -> Option<Percentage> {
    if profile(category).grouped_by_rate() {
        rate
    } else {
        None
    }
}

/// BT-119 for a group — which is **not** BT-152.
///
/// Every group states it, category `O` included, which is why this takes no
/// category argument and is a named function anyway: the temptation is to reuse
/// [`VatCategory::states_rate`] here, and that would be wrong.
///
/// `BR-O-05` suppresses the *line* rate (BT-152) for `O`. BT-119 has no such
/// rule: `BR-48` merely **permits** its absence there, and XRechnung's
/// `BR-DE-14` requires it unconditionally with no category exception. Stating
/// `0` satisfies all three; omitting it fails the KoSIT validator.
fn breakdown_rate(rate: Option<Percentage>) -> Option<Percentage> {
    rate.or(Some(Percentage::ZERO))
}

/// BT-117 for a taxed group: BT-116 × BT-119 / 100, to two decimals.
///
/// **Commercial rounding**, half away from zero, so `−0.125` goes to `−0.13` as
/// it does on the credit note a German auditor reads. `BR-CO-17` and the `-09`
/// rows allow a full currency unit of slack, so the strategy is not what makes
/// this pass — it is what makes the number the one a person would have written.
fn tax_on(base: InvoiceAmount, rate: Option<Percentage>) -> Result<InvoiceAmount, ReconcileError> {
    let rate = rate.map_or(Decimal::ZERO, Percentage::into_decimal);
    let exact = base
        .into_decimal()
        .checked_mul(rate)
        .map(|v| v / Decimal::ONE_HUNDRED)
        .ok_or(ReconcileError::Overflow { term: "BT-117" })?;
    InvoiceAmount::from_decimal_exact(
        exact.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero),
    )
    .map_err(|_| ReconcileError::Overflow { term: "BT-117" })
}

#[cfg(test)]
mod tests {
    use rust_decimal::dec;

    use super::*;
    use crate::invoice::{DocumentAllowanceCharge, Item, LineVat, PriceDetails};
    use crate::{Date, InvoiceLine, Quantity, validate};

    fn line(id: &str, net: &str, category: &str, rate: Option<Decimal>) -> InvoiceLine {
        InvoiceLine {
            id: id.to_owned(),
            note: None,
            order_line_reference: None,
            accounting_reference: None,
            object_identifier: None,
            quantity: Quantity::new(Decimal::ONE),
            unit_code: Code::new("C62"),
            net_amount: InvoiceAmount::parse(net).expect("amount"),
            period: None,
            allowances: Vec::new(),
            charges: Vec::new(),
            price: PriceDetails::default(),
            vat: LineVat {
                category: Code::new(category),
                rate: rate.map(Percentage::new),
            },
            item: Item {
                name: Some(format!("item {id}")),
                ..Default::default()
            },
        }
    }

    fn invoice(lines: Vec<InvoiceLine>) -> Invoice {
        let mut inv = Invoice::builder(
            "urn:cen.eu:en16931:2017",
            "INV-1",
            Date::parse("2026-07-31").expect("date"),
            "380",
            "EUR",
        )
        .build();
        inv.lines = lines;
        inv
    }

    /// The feedback's own example: two lines, two VAT rates.
    #[test]
    fn two_lines_two_rates_produce_two_groups_and_a_balancing_total() {
        let mut inv = invoice(vec![
            line("1", "1000.00", "S", Some(dec!(19))),
            line("2", "500.00", "S", Some(dec!(7))),
        ]);
        reconcile(&mut inv).expect("reconciles");

        assert_eq!(inv.vat_breakdown.len(), 2, "one group per rate");
        let by_rate = |r: Decimal| {
            inv.vat_breakdown
                .iter()
                .find(|e| e.rate == Some(Percentage::new(r)))
                .expect("group")
        };
        assert_eq!(by_rate(dec!(19)).taxable_amount.to_string(), "1000.00");
        assert_eq!(by_rate(dec!(19)).tax_amount.to_string(), "190.00");
        assert_eq!(by_rate(dec!(7)).tax_amount.to_string(), "35.00");

        let t = &inv.totals;
        assert_eq!(t.line_total.to_string(), "1500.00");
        assert_eq!(t.taxable_total.to_string(), "1500.00");
        assert_eq!(t.vat_total.expect("BT-110").to_string(), "225.00");
        assert_eq!(t.gross_total.to_string(), "1725.00");
        assert_eq!(t.due.to_string(), "1725.00");
        assert_eq!(t.allowance_total, None, "absent is not zero");
        assert_eq!(t.charge_total, None);
    }

    /// The rounding case the feedback flagged as the one to be careful with:
    /// three 2-dp lines whose exact VAT sum is not the sum of per-line VAT.
    #[test]
    fn rounding_happens_once_on_the_group_not_per_line() {
        let mut inv = invoice(vec![
            line("1", "0.05", "S", Some(dec!(19))),
            line("2", "0.05", "S", Some(dec!(19))),
            line("3", "0.05", "S", Some(dec!(19))),
        ]);
        reconcile(&mut inv).expect("reconciles");

        // Per line: 0.0095 → 0.01 each → 0.03. On the group: 0.15 × 19 % =
        // 0.0285 → 0.03. Same here, and the group figure is the one BR-S-09
        // checks — computing it per line is how the two diverge on real data.
        assert_eq!(inv.vat_breakdown[0].taxable_amount.to_string(), "0.15");
        assert_eq!(inv.vat_breakdown[0].tax_amount.to_string(), "0.03");
        assert!(
            validate(&inv)
                .findings()
                .iter()
                .all(|f| f.rule != "BR-S-09")
        );
    }

    /// Half away from zero, not half to even — what a person would have written.
    #[test]
    fn the_midpoint_rounds_away_from_zero() {
        // 26.30 × 19 % = 4.9970 — no midpoint. 2.50 × 5 % = 0.125, exactly one.
        assert_eq!(
            tax_on(
                InvoiceAmount::parse("2.50").unwrap(),
                Some(Percentage::new(dec!(5)))
            )
            .unwrap()
            .to_string(),
            "0.13",
            "banker's rounding would give 0.12"
        );
        assert_eq!(
            tax_on(
                InvoiceAmount::parse("-2.50").unwrap(),
                Some(Percentage::new(dec!(5)))
            )
            .unwrap()
            .to_string(),
            "-0.13",
            "and symmetrically on a credit note"
        );
    }

    /// Document-level allowances and charges land in the right group and the
    /// right sign — BT-107 is subtracted, BT-108 added, both stated positive.
    #[test]
    fn allowances_and_charges_move_the_base_of_their_own_group() {
        let mut inv = invoice(vec![line("1", "1000.00", "S", Some(dec!(19)))]);
        inv.allowances.push(DocumentAllowanceCharge {
            amount: InvoiceAmount::parse("100.00").unwrap(),
            base_amount: None,
            percentage: None,
            vat: LineVat {
                category: Code::new("S"),
                rate: Some(Percentage::new(dec!(19))),
            },
            reason: Some("Skonto".into()),
            reason_code: None,
        });
        inv.charges.push(DocumentAllowanceCharge {
            amount: InvoiceAmount::parse("50.00").unwrap(),
            base_amount: None,
            percentage: None,
            vat: LineVat {
                category: Code::new("S"),
                rate: Some(Percentage::new(dec!(19))),
            },
            reason: Some("Versand".into()),
            reason_code: None,
        });
        reconcile(&mut inv).expect("reconciles");

        assert_eq!(inv.vat_breakdown[0].taxable_amount.to_string(), "950.00");
        assert_eq!(inv.vat_breakdown[0].tax_amount.to_string(), "180.50");
        let t = &inv.totals;
        assert_eq!(t.allowance_total.expect("BT-107").to_string(), "100.00");
        assert_eq!(t.charge_total.expect("BT-108").to_string(), "50.00");
        assert_eq!(t.taxable_total.to_string(), "950.00");
        assert_eq!(t.gross_total.to_string(), "1130.50");
    }

    /// BT-113 and BT-114 flow into BT-115, and stay absent when unset.
    #[test]
    fn prepayment_and_rounding_reach_the_amount_due() {
        let inv = invoice(vec![line("1", "1000.00", "S", Some(dec!(19)))]);
        let r = Reconciler::new()
            .paid(InvoiceAmount::parse("190.00").unwrap())
            .rounding(InvoiceAmount::parse("-0.01").unwrap())
            .compute(&inv)
            .expect("reconciles");
        assert_eq!(r.totals.gross_total.to_string(), "1190.00");
        assert_eq!(r.totals.due.to_string(), "999.99");
    }

    /// The keystone claim: whatever the category, the result passes the rules
    /// that check the arithmetic. This is what stops the reconciler and the
    /// validator drifting apart.
    #[test]
    fn every_category_reconciles_to_a_valid_breakdown() {
        for cat in VatCategory::ALL {
            let rate = if cat.states_rate() && cat.carries_tax() {
                Some(dec!(19))
            } else if cat.states_rate() {
                Some(Decimal::ZERO)
            } else {
                None
            };
            let mut inv = invoice(vec![line("1", "1000.00", cat.code(), rate)]);
            // The `-02`/`-03`/`-04` rows want tax identifiers, which are the
            // caller's data. Supplying them keeps the assertion below over the
            // *whole* family rather than a hand-picked subset — except for `O`,
            // whose row is the one phrased as a prohibition.
            if cat != VatCategory::OutOfScope {
                inv.seller.vat_identifier = Some("DE123456789".into());
                inv.buyer.vat_identifier = Some("DE987654321".into());
            }
            // `BR-B-01`: split payment is Italy's, and only domestically.
            if cat == VatCategory::SplitPayment {
                inv.seller.address.country = Some(Code::new("IT"));
                inv.buyer.address.country = Some(Code::new("IT"));
            }
            // `BR-IC-11` / `-12` want a delivery date and country. Also caller
            // data, and also not something to exclude from the assertion.
            inv.delivery = Some(crate::invoice::Delivery {
                party_name: None,
                location: None,
                date: Some(Date::parse("2026-07-15").expect("date")),
                address: Some(crate::invoice::PostalAddress {
                    country: Some(Code::new("FR")),
                    ..Default::default()
                }),
            });
            // The reasons the arithmetic cannot supply.
            Reconciler::new()
                .exemption(cat.code(), Some("Steuerbefreiung"), None)
                .apply(&mut inv)
                .unwrap_or_else(|e| panic!("{cat}: {e}"));

            // The rules a reconciler is answerable for: the totals chain, the
            // VAT derivation, and every row of the nine category families.
            // Deliberately not `BR-08`/`BR-09`/`BR-10` — those want a postal
            // address, which is the caller's data, not the arithmetic's.
            const FAMILIES: [&str; 10] = [
                "BR-S-", "BR-Z-", "BR-E-", "BR-AE-", "BR-IC-", "BR-G-", "BR-O-", "BR-AF-",
                "BR-AG-", "BR-B-",
            ];
            let arithmetic = |r: &str| {
                r.starts_with("BR-CO-1")
                    || r == "BR-48"
                    || FAMILIES.iter().any(|p| r.starts_with(p))
            };
            let left: Vec<_> = validate(&inv)
                .findings()
                .iter()
                .filter(|f| arithmetic(&f.rule))
                .map(|f| f.to_string())
                .collect();
            assert!(left.is_empty(), "{cat} left {left:#?}");
        }
    }

    /// `O` is the trap: no rate on the line, a rate on the breakdown.
    #[test]
    fn out_of_scope_states_bt_119_but_not_bt_152() {
        let mut inv = invoice(vec![line("1", "1000.00", "O", None)]);
        Reconciler::new()
            .exemption("O", Some("Nicht steuerbar"), None)
            .apply(&mut inv)
            .expect("reconciles");
        assert_eq!(inv.lines[0].vat.rate, None, "BR-O-05");
        assert_eq!(
            inv.vat_breakdown[0].rate,
            Some(Percentage::ZERO),
            "BT-119 is a different term; BR-DE-14 wants it unconditionally"
        );
        assert_eq!(inv.vat_breakdown[0].tax_amount, InvoiceAmount::ZERO);
    }

    /// A zero-tax category with rows at `Some(0)` and `None` is still one group.
    ///
    /// The `None` row is itself invalid — `BR-AE-05` says the *line* rate "shall
    /// be 0", and absent is not zero — which is precisely why this matters. A
    /// parser must be able to load such a document, and the grouping must not
    /// split on it, or the report would carry a spurious `BR-AE-01` about a
    /// breakdown that has two groups instead of the `BR-AE-05` about the line
    /// that caused it.
    #[test]
    fn exactly_one_group_categories_do_not_split_on_the_rate() {
        let mut inv = invoice(vec![
            line("1", "100.00", "AE", Some(Decimal::ZERO)),
            line("2", "100.00", "AE", None),
        ]);
        Reconciler::new()
            .exemption("AE", None, Some("VATEX-EU-AE"))
            .apply(&mut inv)
            .expect("reconciles");
        assert_eq!(inv.vat_breakdown.len(), 1, "BR-AE-01 says exactly one");
        assert_eq!(inv.vat_breakdown[0].taxable_amount.to_string(), "200.00");
    }

    #[test]
    fn a_category_outside_uncl_5305_is_refused_by_name() {
        let inv = invoice(vec![line("1", "100.00", "Q", Some(dec!(19)))]);
        let err = reconcile(&mut inv.clone()).expect_err("Q is not a category");
        assert!(matches!(err, ReconcileError::UnknownCategory { .. }));
        assert!(err.to_string().contains("BR-CL-18"), "{err}");
    }

    #[test]
    fn a_taxed_category_with_no_rate_is_refused_rather_than_zeroed() {
        let inv = invoice(vec![line("1", "100.00", "S", None)]);
        let err = reconcile(&mut inv.clone()).expect_err("S needs a rate");
        assert!(matches!(err, ReconcileError::MissingRate { .. }));
        assert!(err.to_string().contains("under-declare"), "{err}");
    }

    /// Reasons already on the invoice survive a recompute of the numbers.
    #[test]
    fn existing_exemption_reasons_are_preserved() {
        let mut inv = invoice(vec![line("1", "100.00", "E", Some(Decimal::ZERO))]);
        inv.vat_breakdown.push(VatBreakdown {
            taxable_amount: InvoiceAmount::ZERO, // deliberately wrong
            tax_amount: InvoiceAmount::ZERO,
            category: Code::new("E"),
            rate: Some(Percentage::ZERO),
            exemption_reason: Some("§ 4 Nr. 21 UStG".into()),
            exemption_reason_code: None,
        });
        reconcile(&mut inv).expect("reconciles");
        assert_eq!(inv.vat_breakdown[0].taxable_amount.to_string(), "100.00");
        assert_eq!(
            inv.vat_breakdown[0].exemption_reason.as_deref(),
            Some("§ 4 Nr. 21 UStG"),
            "the numbers are recomputed; the prose is not invented and not lost"
        );
    }

    /// A reason offered for a category that forbids one is dropped, not written.
    #[test]
    fn a_reason_is_never_written_where_a_rule_forbids_it() {
        let mut inv = invoice(vec![line("1", "100.00", "S", Some(dec!(19)))]);
        Reconciler::new()
            .exemption("S", Some("not allowed here"), None)
            .apply(&mut inv)
            .expect("reconciles");
        assert!(!inv.vat_breakdown[0].has_exemption_reason(), "BR-S-10");
    }

    /// Reconciling twice changes nothing — the operation is idempotent, which is
    /// what lets it sit in a pipeline that may run it more than once.
    #[test]
    fn reconciliation_is_idempotent() {
        let mut inv = invoice(vec![
            line("1", "33.33", "S", Some(dec!(19))),
            line("2", "66.67", "S", Some(dec!(7))),
        ]);
        reconcile(&mut inv).expect("first");
        let once = inv.clone();
        reconcile(&mut inv).expect("second");
        assert_eq!(inv, once);
    }
}
