//! [`Percentage`] and [`Quantity`] — EN 16931 §6.5.5 and §6.5.4.
//!
//! Both are unbounded decimals, and both are easy to get subtly wrong in
//! opposite directions: a percentage that is secretly a fraction, and a quantity
//! that is forbidden from going negative.

use core::fmt;
use core::str::FromStr;

use rust_decimal::Decimal;

/// A percentage — EN 16931-1 §6.5.5 `Percentage. Type`.
///
/// > Percentages are given as fractions of a hundred (per cent) e.g. the value
/// > 34,78 % in percentage terms is given as 34,78.
///
/// So `Percentage::new(dec!(19))` is nineteen per cent. **Not `0.19`.**
///
/// This is the single most common transcription bug when bridging a calculation
/// engine to EN 16931: `billing` stores VAT rates as fractions (`0.19`) because
/// that is what you multiply by, while the standard stores what you print. The
/// conversion happens once, at the adapter boundary, and the model holds what
/// the standard holds — storing the fraction and multiplying by 100 on the way
/// out invites exactly one off-by-100 per format crate.
///
/// BR-CO-17 is written in these terms: *"VAT category tax amount (BT-117) = VAT
/// category taxable amount (BT-116) x (VAT category rate (BT-119) / 100)"*.
///
/// # Trailing zeros do not split a VAT group
///
/// Peppol is explicit that *"for the VAT rate, only significant decimals should
/// be considered, i.e. any difference in trailing zeros should not result in
/// different VAT breakdowns"* — otherwise `19` and `19.00` produce two BG-23
/// groups for one rate and BR-S-08 fails on an arithmetically correct invoice.
///
/// **This needs no special handling here.** `rust_decimal` compares by
/// mathematical value, not by representation, so `Eq`, `Ord` **and `Hash`** all
/// treat `19` and `19.00` as one rate. A `Percentage` is therefore safe as a
/// `HashMap` or `BTreeMap` key for `(category, rate)` grouping, with no
/// normalisation step to forget. That is worth stating because it is *not*
/// universal — several decimal libraries in other ecosystems make scale part of
/// identity, and a `Hash` that disagreed with `Eq` here would be a silent,
/// data-dependent grouping bug. [`Percentage::normalized`] exists for canonical
/// *rendering*, not for comparison, and the property is pinned by a test.
///
/// ```
/// use en16931::Percentage;
/// use rust_decimal::dec;
///
/// let vat = Percentage::new(dec!(19));
/// assert_eq!(vat.to_string(), "19");
/// assert_eq!(vat.as_fraction(), dec!(0.19));   // what you multiply by
///
/// // A reduced rate with a fractional per cent is ordinary, not exotic.
/// assert_eq!(Percentage::new(dec!(7.5)).to_string(), "7.5");
/// ```
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Percentage(Decimal);

impl Percentage {
    /// Zero per cent — the rate every zero-tax category states.
    pub const ZERO: Self = Self(Decimal::ZERO);

    /// Wrap a value already expressed as a per-cent figure (`19`, not `0.19`).
    #[must_use]
    pub const fn new(percent: Decimal) -> Self {
        Self(percent)
    }

    /// Build from a fraction (`0.19` → 19 %), for bridging engines that store
    /// rates the way you multiply by them.
    ///
    /// Returns `None` on overflow.
    #[must_use]
    pub fn from_fraction(fraction: Decimal) -> Option<Self> {
        fraction
            .checked_mul(Decimal::ONE_HUNDRED)
            .map(|d| Self(d.normalize()))
    }

    /// The per-cent figure, as written on the invoice.
    #[must_use]
    pub const fn into_decimal(self) -> Decimal {
        self.0
    }

    /// The multiplier: `19 %` → `0.19`.
    ///
    /// # Panics
    /// Cannot panic — division by the constant 100 is always defined for a
    /// `Decimal`, and the quotient's scale is bounded by the dividend's.
    #[must_use]
    pub fn as_fraction(self) -> Decimal {
        self.0 / Decimal::ONE_HUNDRED
    }

    /// Whether the rate is exactly zero.
    ///
    /// The zero-tax categories (`Z`, `E`, `AE`, `K`, `G`) require this;
    /// `S` requires the opposite (BR-S-05).
    #[must_use]
    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }

    /// Whether the rate is above zero — BR-S-05, BR-AF-05, BR-AG-05.
    #[must_use]
    pub fn is_positive(self) -> bool {
        self.0 > Decimal::ZERO
    }

    /// Whether the rate is below zero, which no VAT category permits.
    #[must_use]
    pub fn is_negative(self) -> bool {
        self.0 < Decimal::ZERO
    }

    /// The value with trailing zeros stripped — `19.00` → `19`.
    ///
    /// For *rendering*, not for comparison: see the type documentation on why
    /// [`Eq`] already ignores scale.
    #[must_use]
    pub fn normalized(self) -> Self {
        Self(self.0.normalize())
    }
}

impl fmt::Display for Percentage {
    /// Trailing zeros stripped: `19`, `7.5`, `0`. No `%` sign — the business
    /// term is the number, and the unit is implied by BT-119 being a rate.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&self.0.normalize().to_string())
    }
}

impl From<Decimal> for Percentage {
    /// Treats the value as a per-cent figure. Use [`Percentage::from_fraction`]
    /// if you hold `0.19`.
    fn from(d: Decimal) -> Self {
        Self(d)
    }
}

impl FromStr for Percentage {
    type Err = rust_decimal::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Decimal::from_str_exact(s).map(Self)
    }
}

/// A counted amount of something — EN 16931-1 §6.5.4 `Quantity. Type`.
///
/// Unbounded decimal, and **it may be negative**. That is not an edge case the
/// standard tolerates; it is the mechanism it prescribes. Annex A.1.6
/// (*Example 5 — Negative Invoice line*) invoices 25 cases of pens and credits
/// 10 returned ones on the same document:
///
/// | BT-126 | BT-129 | BT-146 | BT-131 |
/// |---|---|---|---|
/// | 1 | `25` | `8,50` | `212,50` |
/// | 2 | **`−10`** | `8,50` | **`−85,00`** |
///
/// The sign lives on the **quantity**, never on the price: BR-27 forbids a
/// negative item net price. An engine that models returns as `Sign::Credit`
/// with a non-negative quantity has to flip the convention at this boundary.
///
/// The unit of measure is a separate business term (BT-130), not part of this
/// type — §6.5.4: *"The code for the Unit of Measure is defined as a separate
/// business term."*
///
/// ```
/// use en16931::Quantity;
/// use rust_decimal::dec;
///
/// let returned = Quantity::new(dec!(-10));
/// assert!(returned.is_negative());
/// assert_eq!(returned.to_string(), "-10");
/// ```
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Quantity(Decimal);

impl Quantity {
    /// Zero.
    pub const ZERO: Self = Self(Decimal::ZERO);
    /// One — the quantity a flat charge states, so that BR-22 is satisfied and
    /// `1 × amount` reproduces the amount exactly.
    pub const ONE: Self = Self(Decimal::ONE);

    /// Wrap a decimal.
    #[must_use]
    pub const fn new(value: Decimal) -> Self {
        Self(value)
    }

    /// The value.
    #[must_use]
    pub const fn into_decimal(self) -> Decimal {
        self.0
    }

    /// Whether the quantity is below zero — a return line (Annex A.1.6).
    #[must_use]
    pub fn is_negative(self) -> bool {
        self.0 < Decimal::ZERO
    }

    /// Whether the quantity is above zero.
    #[must_use]
    pub fn is_positive(self) -> bool {
        self.0 > Decimal::ZERO
    }

    /// Whether the quantity is exactly zero.
    #[must_use]
    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }

    /// `-self`, for converting a sign-carrying credit line into the negative
    /// quantity EN 16931 expects.
    ///
    /// Returns `None` only on `Decimal` overflow, which requires a value at the
    /// 28-digit ceiling.
    #[must_use]
    pub fn checked_neg(self) -> Option<Self> {
        Decimal::ZERO.checked_sub(self.0).map(Self)
    }
}

impl fmt::Display for Quantity {
    /// Trailing zeros stripped. Honours width, fill and alignment.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&self.0.normalize().to_string())
    }
}

impl From<Decimal> for Quantity {
    fn from(d: Decimal) -> Self {
        Self(d)
    }
}

impl FromStr for Quantity {
    type Err = rust_decimal::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Decimal::from_str_exact(s).map(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::dec;

    #[test]
    fn percentage_is_per_cent_not_a_fraction() {
        // §6.5.5: "34,78 % in percentage terms is given as 34,78".
        let p = Percentage::new(dec!(34.78));
        assert_eq!(p.to_string(), "34.78");
        assert_eq!(p.as_fraction(), dec!(0.3478));
        assert_eq!(
            Percentage::from_fraction(dec!(0.19)).unwrap(),
            Percentage::new(dec!(19))
        );
    }

    #[test]
    fn percentage_display_strips_trailing_zeros() {
        assert_eq!(Percentage::new(dec!(19.00)).to_string(), "19");
        assert_eq!(Percentage::new(dec!(7.50)).to_string(), "7.5");
        assert_eq!(Percentage::ZERO.to_string(), "0");
    }

    /// Pins the property VAT grouping depends on: a rate's *scale* is not part
    /// of its identity, in `Eq`, `Ord` or `Hash`.
    ///
    /// If a future `rust_decimal` made `Hash` scale-sensitive while `Eq` stayed
    /// value-based, `(category, rate)` grouping would break for some documents
    /// and not others — the worst kind of bug. This test is the tripwire.
    #[test]
    fn a_rates_scale_is_not_part_of_its_identity() {
        use std::collections::HashMap;
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let a = Percentage::new(dec!(19));
        let b = Percentage::new(dec!(19.00));

        assert_eq!(a, b, "Peppol: trailing zeros are not significant");
        assert_eq!(a.cmp(&b), core::cmp::Ordering::Equal);

        let hash = |p: &Percentage| {
            let mut h = DefaultHasher::new();
            p.hash(&mut h);
            h.finish()
        };
        assert_eq!(
            hash(&a),
            hash(&b),
            "Hash must agree with Eq or grouping breaks"
        );

        // The property that actually matters: one BG-23 group, not two.
        let mut groups: HashMap<(super::super::VatCategory, Percentage), u32> = HashMap::new();
        *groups
            .entry((super::super::VatCategory::Standard, a))
            .or_default() += 1;
        *groups
            .entry((super::super::VatCategory::Standard, b))
            .or_default() += 1;
        assert_eq!(groups.len(), 1, "19 and 19.00 are one VAT breakdown group");

        // Rendering is still canonical.
        assert_eq!(b.normalized().to_string(), "19");
    }

    #[test]
    fn rate_predicates_match_the_category_rules() {
        assert!(Percentage::new(dec!(19)).is_positive()); // BR-S-05
        assert!(Percentage::ZERO.is_zero()); // BR-Z-05, BR-E-05, BR-AE-05
        assert!(!Percentage::ZERO.is_positive());
        assert!(Percentage::new(dec!(-1)).is_negative()); // no category allows it
    }

    #[test]
    fn quantity_may_be_negative() {
        // Annex A.1.6 line 2: -10 cases returned.
        let q = Quantity::new(dec!(-10));
        assert!(q.is_negative());
        assert_eq!(q.to_string(), "-10");
        assert_eq!(Quantity::new(dec!(10)).checked_neg().unwrap(), q);
    }

    #[test]
    fn quantity_one_is_exact_for_a_flat_charge() {
        // A flat charge states quantity 1 so BR-22 holds and 1 x amount is the
        // amount — nothing is rounded that was not rounded before.
        assert_eq!(Quantity::ONE.into_decimal(), Decimal::ONE);
        assert_eq!(Quantity::ONE.into_decimal() * dec!(8.50), dec!(8.50));
    }

    #[test]
    fn quantity_keeps_metering_precision() {
        let q = Quantity::new(dec!(1234.567));
        assert_eq!(q.to_string(), "1234.567");
    }
}
