//! [`Percentage`] and [`Quantity`] — EN 16931 §6.5.5 and §6.5.4.

use core::fmt;
use core::str::FromStr;

use rust_decimal::Decimal;

use crate::error::ParseDecimalError;

/// The parts of an `xs:decimal`, `(\+|-)?([0-9]+(\.[0-9]*)?|\.[0-9]+)`:
/// whether it is negative, its integer digits and its fraction digits.
/// `None` for anything else — `1e3`, `1_000`, ` 1`, `.`.
pub(crate) fn xs_decimal_parts(s: &str) -> Option<(bool, &str, &str)> {
    let (negative, unsigned) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let digits = |part: &str| part.bytes().all(|b| b.is_ascii_digit());
    ((!whole.is_empty() || !fraction.is_empty()) && digits(whole) && digits(fraction))
        .then_some((negative, whole, fraction))
}

/// `s` as a [`Decimal`], when it is an `xs:decimal` a decimal holds exactly.
///
/// The one reading of a number in this workspace: [`Percentage`] and
/// [`Quantity`] parse with it, [`crate::InvoiceAmount::parse`] shares its
/// spelling, and the syntax readers call it. `Decimal`'s own parser also
/// accepts `1e3` and `1_000` and rounds past its precision; this does neither.
///
/// ```
/// use en16931::numeric::parse_decimal;
/// use rust_decimal::dec;
///
/// assert_eq!(parse_decimal(".5"), Ok(dec!(0.5)));
/// assert!(parse_decimal("1_000").is_err());
/// assert!(parse_decimal("1.00000000000000000000000000001").is_err());
/// ```
///
/// # Errors
/// [`ParseDecimalError`]: malformed, out of range, or more digits than a
/// decimal holds.
pub fn parse_decimal(s: &str) -> Result<Decimal, ParseDecimalError> {
    let Some((_, _, fraction)) = xs_decimal_parts(s) else {
        return Err(ParseDecimalError::Malformed {
            input: s.to_owned(),
        });
    };
    let parsed: Decimal = s.parse().map_err(|_| ParseDecimalError::OutOfRange {
        input: s.to_owned(),
    })?;
    // `Decimal` rounds what does not fit its 96 bits; a stated digit lost is
    // a scale below the places the text carries.
    let places = fraction.trim_end_matches('0').len();
    if places > parsed.scale() as usize {
        return Err(ParseDecimalError::TooPrecise {
            input: s.to_owned(),
        });
    }
    Ok(parsed)
}

/// A percentage — EN 16931-1 §6.5.5 `Percentage. Type`.
///
/// Per cent, not a fraction: `Percentage::new(dec!(19))` is 19 %, not `0.19`
/// (use [`Percentage::from_fraction`] for `0.19`).
///
/// `Eq`, `Ord` and `Hash` compare by value, so `19` and `19.00` are one rate
/// and one BG-23 group, as Peppol requires. [`Percentage::normalized`] is for
/// rendering only.
///
/// ```
/// use en16931::Percentage;
/// use rust_decimal::dec;
///
/// let vat = Percentage::new(dec!(19));
/// assert_eq!(vat.to_string(), "19");
/// assert_eq!(vat.as_fraction(), dec!(0.19));   // what you multiply by
///
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

    /// Build from a fraction (`0.19` → 19 %). `None` on overflow.
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
    #[must_use]
    pub fn as_fraction(self) -> Decimal {
        self.0 / Decimal::ONE_HUNDRED
    }

    /// Whether the rate is exactly zero, as categories `Z`, `E`, `AE`, `K`, `G`
    /// require.
    #[must_use]
    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }

    /// Whether the rate is above zero — BR-S-05, BR-IG-05, BR-IP-05.
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
    /// For rendering; [`Eq`] already ignores scale.
    #[must_use]
    pub fn normalized(self) -> Self {
        Self(self.0.normalize())
    }
}

impl fmt::Display for Percentage {
    /// Trailing zeros stripped: `19`, `7.5`, `0`. No `%` sign.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        crate::fmt::number(f, &self.0.normalize().to_string())
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
    type Err = ParseDecimalError;
    /// An `xs:decimal`, exactly ([`parse_decimal`]).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_decimal(s).map(Self)
    }
}

/// A counted amount of something — EN 16931-1 §6.5.4 `Quantity. Type`.
///
/// Unbounded decimal that may be negative: a return line carries a negative
/// quantity, never a negative price (BR-27; Annex A.1.6). The unit is BT-130.
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
    /// One — the quantity of a flat charge.
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

    /// `-self`. `None` on `Decimal` overflow.
    #[must_use]
    pub fn checked_neg(self) -> Option<Self> {
        Decimal::ZERO.checked_sub(self.0).map(Self)
    }
}

impl fmt::Display for Quantity {
    /// Trailing zeros stripped. Honours width, fill and alignment.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        crate::fmt::number(f, &self.0.normalize().to_string())
    }
}

impl From<Decimal> for Quantity {
    fn from(d: Decimal) -> Self {
        Self(d)
    }
}

impl FromStr for Quantity {
    type Err = ParseDecimalError;
    /// An `xs:decimal`, exactly ([`parse_decimal`]).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_decimal(s).map(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::dec;

    /// One spelling for every number: a percentage, a quantity, an amount and
    /// `parse_decimal` accept and refuse the same text (amounts within their
    /// two decimals).
    #[test]
    fn every_number_reads_by_one_rule() {
        for (text, ok) in [
            ("12", true),
            ("+12", true),
            ("-12", true),
            (".5", true),
            ("5.", true),
            ("-.5", true),
            ("0.50", true),
            ("1e3", false),
            ("1E3", false),
            ("1_000", false),
            ("1,5", false),
            ("1 2", false),
            ("1.2.3", false),
            (".", false),
            ("+", false),
            ("-.", false),
            ("", false),
            ("0x10", false),
            ("NaN", false),
        ] {
            assert_eq!(parse_decimal(text).is_ok(), ok, "parse_decimal({text:?})");
            assert_eq!(
                text.parse::<Percentage>().is_ok(),
                ok,
                "Percentage {text:?}"
            );
            assert_eq!(text.parse::<Quantity>().is_ok(), ok, "Quantity {text:?}");
            assert_eq!(
                crate::InvoiceAmount::parse(text).is_ok(),
                ok,
                "InvoiceAmount {text:?}"
            );
        }
    }

    /// Exact or refused, and the error says which.
    #[test]
    fn a_number_is_never_rounded() {
        assert_eq!(
            parse_decimal("0.0000000000000000000000000001"),
            Ok(Decimal::from_i128_with_scale(1, 28))
        );
        assert_eq!(
            parse_decimal("1.0000000000000000000000000000000"),
            Ok(dec!(1))
        );
        assert!(matches!(
            parse_decimal("1.00000000000000000000000000001"),
            Err(ParseDecimalError::TooPrecise { .. })
        ));
        assert!(matches!(
            parse_decimal("9.9999999999999999999999999999"),
            Err(ParseDecimalError::TooPrecise { .. })
        ));
        assert!(matches!(
            parse_decimal("79228162514264337593543950336"),
            Err(ParseDecimalError::OutOfRange { .. })
        ));
        assert_eq!(
            parse_decimal("79228162514264337593543950335"),
            Ok(Decimal::MAX)
        );
        assert!(matches!(
            parse_decimal("1_000"),
            Err(ParseDecimalError::Malformed { .. })
        ));
    }

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

    /// A rate's scale is not part of its identity in `Eq`, `Ord` or `Hash`.
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
        assert_eq!(Quantity::ONE.into_decimal(), Decimal::ONE);
        assert_eq!(Quantity::ONE.into_decimal() * dec!(8.50), dec!(8.50));
    }

    #[test]
    fn quantity_keeps_metering_precision() {
        let q = Quantity::new(dec!(1234.567));
        assert_eq!(q.to_string(), "1234.567");
    }
}
