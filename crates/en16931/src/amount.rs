//! [`InvoiceAmount`] — EN 16931 `Amount. Type`, and the reason 21 rules cannot fire.
//!
//! # Why two decimals live in the type
//!
//! EN 16931-1 §6.5.2 defines the semantic data type directly:
//!
//! > This EN 16931_ Amount. Type is based on the Amount. Type as defined in
//! > ISO 15000-5:2014, Annex B. **EN 16931_ Amount. Type is floating up to two
//! > fraction digits.**
//!
//! Table 26 (§6.5.12) then lists every business term the cap applies to, and the
//! CEN validation artefacts render that table as 21 separate assertions —
//! `BR-DEC-01`, `-02`, `-05`, `-06`, `-09`..`-20`, `-23`..`-25`, `-27`, `-28`.
//!
//! A type that physically cannot hold a third decimal retires all 21 at compile
//! time. They stay in the rule registry so `explain("BR-DEC-12")` works and a
//! report can state they were checked, but their evaluation is a constant pass.
//!
//! This is the crate's thesis in one type: encode the normative *table* as a
//! *type*, not as 21 runtime checks.
//!
//! # What is deliberately not this type
//!
//! `Unit Price Amount. Type` (§6.5.3) is a **different** semantic type, based on
//! Amount but with no decimal cap — its own example in the standard is
//! `10000.1234`. BT-146, BT-147 and BT-148 are unit prices. Using
//! `InvoiceAmount` for a price would silently truncate `0.28901 EUR/kWh` to
//! `0.29`, which is not a rounding error but a different price.

use core::fmt;
use core::iter::Sum;
use core::ops::Neg;
use core::str::FromStr;

use rust_decimal::Decimal;

use crate::error::{AmountError, ParseAmountError};

/// The scale every EN 16931 monetary amount carries.
const SCALE: u32 = 2;
/// `10^SCALE`, as the multiplier between the stored integer and the value.
const UNITS: i64 = 100;

/// A monetary amount as EN 16931 carries it — EN 16931-1 §6.5.2 `Amount. Type`.
///
/// Stored as `i64` minor units, so the representable range is roughly
/// ±92 quadrillion major units and a third decimal place does not exist.
///
/// The currency is **not** part of this type: §6.5.2 says *"The currency of the
/// amount is defined as a separate business term"* (BT-5), and it is stated once
/// per document rather than per amount. Mixing currencies is therefore a
/// document-level rule, not something this type can prevent — and pretending
/// otherwise would put a `Currency` on all 21 amounts of an invoice that all
/// necessarily share one.
///
/// ```
/// use en16931::InvoiceAmount;
///
/// let a = InvoiceAmount::parse("1000.00")?;
/// let b = InvoiceAmount::parse("190.00")?;
/// assert_eq!(a.checked_add(b)?.to_string(), "1190.00");
///
/// // A third decimal is not representable, so it is refused rather than rounded.
/// assert!(InvoiceAmount::parse("0.005").is_err());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "String", into = "String"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct InvoiceAmount(i64);

impl InvoiceAmount {
    /// Zero.
    pub const ZERO: Self = Self(0);
    /// The largest representable amount.
    pub const MAX: Self = Self(i64::MAX);
    /// The smallest representable amount.
    pub const MIN: Self = Self(i64::MIN);

    /// Parse an exact decimal string: `"1190.00"`, `"-85.00"`, `"12"`, `"12.5"`.
    ///
    /// **Refuses excess precision rather than rounding it.** `"0.005"` is an
    /// error, not `0.01` and not `0.00` — a parser that rounds turns a caller's
    /// mistake into a plausible wrong number, which is the failure mode this
    /// whole crate exists to prevent.
    ///
    /// # Errors
    /// [`ParseAmountError`] for malformed input, more than two decimals, or a
    /// value outside the representable range.
    pub fn parse(s: &str) -> Result<Self, ParseAmountError> {
        let t = s.trim();
        if t.is_empty() {
            return Err(ParseAmountError::Empty);
        }
        let (neg, digits) = match t.as_bytes()[0] {
            b'-' => (true, &t[1..]),
            b'+' => (false, &t[1..]),
            _ => (false, t),
        };
        if digits.is_empty() {
            return Err(ParseAmountError::Malformed {
                input: s.to_owned(),
            });
        }
        let (int_part, frac_part) = match digits.split_once('.') {
            Some((i, f)) => (i, f),
            None => (digits, ""),
        };
        // An empty integer part (".5") is rejected: EN 16931 amounts appear in
        // documents read by humans, and a leading dot is a typo far more often
        // than it is an intent.
        if int_part.is_empty()
            || !int_part.bytes().all(|b| b.is_ascii_digit())
            || !frac_part.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(ParseAmountError::Malformed {
                input: s.to_owned(),
            });
        }
        if frac_part.len() > SCALE as usize {
            return Err(ParseAmountError::TooManyDecimals {
                input: s.to_owned(),
                max: SCALE as u8,
            });
        }

        let overflow = || ParseAmountError::OutOfRange {
            input: s.to_owned(),
        };
        let mut units: i64 = 0;
        for b in int_part.bytes() {
            units = units
                .checked_mul(10)
                .and_then(|u| u.checked_add(i64::from(b - b'0')))
                .ok_or_else(overflow)?;
        }
        units = units.checked_mul(UNITS).ok_or_else(overflow)?;
        // Pad `"5"` to `"50"` so `"1.5"` is 150 minor units, not 105.
        let mut frac: i64 = 0;
        for i in 0..SCALE as usize {
            let d = frac_part
                .as_bytes()
                .get(i)
                .map_or(0, |b| i64::from(b - b'0'));
            frac = frac * 10 + d;
        }
        units = units.checked_add(frac).ok_or_else(overflow)?;
        Ok(Self(if neg { -units } else { units }))
    }

    /// Convert from a [`Decimal`] **exactly**, or fail.
    ///
    /// This is the boundary an upstream calculation crosses. `billing`'s
    /// `Amount::exact_to::<2>()` is the mirror of it: both refuse rather than
    /// round, because rounding at an interchange boundary breaks the totals
    /// identities the format also checks (BR-CO-10 and BR-CO-15 in particular —
    /// three leaves of `0.005` round to `0.03` while their exact sum rounds to
    /// `0.02`).
    ///
    /// # Errors
    /// [`AmountError::PrecisionLoss`] if `d` needs more than two decimals;
    /// [`AmountError::Overflow`] if it does not fit.
    pub fn from_decimal_exact(d: Decimal) -> Result<Self, AmountError> {
        let scaled = d
            .checked_mul(Decimal::from(UNITS))
            .ok_or(AmountError::Overflow)?;
        if scaled.fract() != Decimal::ZERO {
            return Err(AmountError::PrecisionLoss {
                value: d.to_string(),
                max: SCALE as u8,
            });
        }
        let units = i64::try_from(scaled.trunc()).map_err(|_| AmountError::Overflow)?;
        Ok(Self(units))
    }

    /// The value as an exact [`Decimal`] with scale 2.
    #[must_use]
    pub fn into_decimal(self) -> Decimal {
        Decimal::new(self.0, SCALE)
    }

    /// The raw count of minor units. `12.34` → `1234`.
    ///
    /// For interop with systems that store money as integers; prefer
    /// [`InvoiceAmount::into_decimal`] for arithmetic.
    #[must_use]
    pub fn to_minor_units(self) -> i64 {
        self.0
    }

    /// Construct from a raw count of minor units. `1234` → `12.34`.
    #[must_use]
    pub fn from_minor_units(units: i64) -> Self {
        Self(units)
    }

    /// `self + rhs`.
    ///
    /// # Errors
    /// [`AmountError::Overflow`] — never wraps, never saturates. An invoice that
    /// overflows `i64` minor units is a data error worth surfacing, not a number
    /// worth guessing.
    pub fn checked_add(self, rhs: Self) -> Result<Self, AmountError> {
        self.0
            .checked_add(rhs.0)
            .map(Self)
            .ok_or(AmountError::Overflow)
    }

    /// `self - rhs`.
    ///
    /// # Errors
    /// [`AmountError::Overflow`].
    pub fn checked_sub(self, rhs: Self) -> Result<Self, AmountError> {
        self.0
            .checked_sub(rhs.0)
            .map(Self)
            .ok_or(AmountError::Overflow)
    }

    /// `-self`.
    ///
    /// # Errors
    /// [`AmountError::Overflow`] for [`InvoiceAmount::MIN`], which has no
    /// positive counterpart.
    pub fn checked_neg(self) -> Result<Self, AmountError> {
        self.0.checked_neg().map(Self).ok_or(AmountError::Overflow)
    }

    /// `|self|`.
    ///
    /// Needed by the VAT-derivation rules, which compare **absolute** values —
    /// `BR-CO-17` and `BR-S-09` apply `abs()` to both operands before rounding,
    /// which is what lets a credit note satisfy them.
    ///
    /// # Errors
    /// [`AmountError::Overflow`] for [`InvoiceAmount::MIN`].
    pub fn checked_abs(self) -> Result<Self, AmountError> {
        self.0.checked_abs().map(Self).ok_or(AmountError::Overflow)
    }

    /// Sum without panicking on overflow.
    ///
    /// The totals chain — `BR-CO-10` through `BR-CO-16` — is a chain of sums
    /// compared for **exact** equality, so this is the operation every one of
    /// those rules is built on.
    ///
    /// # Errors
    /// [`AmountError::Overflow`].
    pub fn checked_sum<I: IntoIterator<Item = Self>>(iter: I) -> Result<Self, AmountError> {
        let mut acc = Self::ZERO;
        for a in iter {
            acc = acc.checked_add(a)?;
        }
        Ok(acc)
    }

    /// Whether this amount is greater than zero.
    #[must_use]
    pub fn is_positive(self) -> bool {
        self.0 > 0
    }

    /// Whether this amount is less than zero.
    #[must_use]
    pub fn is_negative(self) -> bool {
        self.0 < 0
    }

    /// Whether this amount is exactly zero.
    #[must_use]
    pub fn is_zero(self) -> bool {
        self.0 == 0
    }
}

impl fmt::Display for InvoiceAmount {
    /// Always two decimals, always `.` — the interchange spelling, not a locale.
    ///
    /// Honours width, fill and alignment via [`fmt::Formatter::pad`].
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        // `unsigned_abs` so `i64::MIN` formats rather than panicking.
        let abs = self.0.unsigned_abs();
        let s = format!("{sign}{}.{:02}", abs / UNITS as u64, abs % UNITS as u64);
        crate::fmt::number(f, &s)
    }
}

impl FromStr for InvoiceAmount {
    type Err = ParseAmountError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Neg for InvoiceAmount {
    type Output = Self;
    /// # Panics
    /// Panics for [`InvoiceAmount::MIN`]; use [`InvoiceAmount::checked_neg`].
    fn neg(self) -> Self {
        self.checked_neg().expect("negation of InvoiceAmount::MIN")
    }
}

impl Sum for InvoiceAmount {
    /// # Panics
    /// Panics on overflow; use [`InvoiceAmount::checked_sum`] for untrusted input.
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        Self::checked_sum(iter).expect("overflow summing InvoiceAmount")
    }
}

impl TryFrom<Decimal> for InvoiceAmount {
    type Error = AmountError;
    fn try_from(d: Decimal) -> Result<Self, Self::Error> {
        Self::from_decimal_exact(d)
    }
}

impl From<InvoiceAmount> for Decimal {
    fn from(a: InvoiceAmount) -> Self {
        a.into_decimal()
    }
}

// Serde goes through the decimal *string*, never a float and never the raw
// integer: a JSON number would lose exactness in any consumer using f64, and a
// bare `1234` is indistinguishable from twelve hundred and thirty-four euros.
#[cfg(feature = "serde")]
impl TryFrom<String> for InvoiceAmount {
    type Error = ParseAmountError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::parse(&s)
    }
}

#[cfg(feature = "serde")]
impl From<InvoiceAmount> for String {
    fn from(a: InvoiceAmount) -> Self {
        a.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::dec;

    #[test]
    fn parses_and_renders_the_interchange_spelling() {
        for (input, rendered) in [
            ("1190.00", "1190.00"),
            ("0", "0.00"),
            ("12.5", "12.50"),
            ("-85.00", "-85.00"),
            ("+7.25", "7.25"),
            ("  31.88  ", "31.88"),
            ("0.07", "0.07"),
        ] {
            let a = InvoiceAmount::parse(input).unwrap_or_else(|e| panic!("{input}: {e}"));
            assert_eq!(a.to_string(), rendered, "input {input}");
        }
    }

    #[test]
    fn a_third_decimal_is_refused_not_rounded() {
        // The whole point of the type. Neither 0.01 nor 0.00 — an error.
        assert!(matches!(
            InvoiceAmount::parse("0.005"),
            Err(ParseAmountError::TooManyDecimals { .. })
        ));
        assert!(matches!(
            InvoiceAmount::from_decimal_exact(dec!(0.005)),
            Err(AmountError::PrecisionLoss { .. })
        ));
    }

    #[test]
    fn rejects_malformed_input() {
        for bad in ["", "   ", "abc", "1.2.3", "1,50", ".5", "-", "1e3", "1 000"] {
            assert!(
                InvoiceAmount::parse(bad).is_err(),
                "{bad:?} should not parse"
            );
        }
    }

    #[test]
    fn fraction_padding_is_positional() {
        // "1.5" is 150 minor units, not 105 — the classic off-by-a-digit.
        assert_eq!(InvoiceAmount::parse("1.5").unwrap().to_minor_units(), 150);
        assert_eq!(InvoiceAmount::parse("1.05").unwrap().to_minor_units(), 105);
    }

    #[test]
    fn arithmetic_is_checked_never_wrapping() {
        assert!(
            InvoiceAmount::MAX
                .checked_add(InvoiceAmount::parse("0.01").unwrap())
                .is_err()
        );
        assert!(InvoiceAmount::MIN.checked_neg().is_err());
        assert!(InvoiceAmount::MIN.checked_abs().is_err());
        assert!(
            InvoiceAmount::MIN
                .checked_sub(InvoiceAmount::parse("0.01").unwrap())
                .is_err()
        );
    }

    #[test]
    fn decimal_round_trips_exactly() {
        for s in ["1190.00", "-85.00", "0.07", "0.00"] {
            let a = InvoiceAmount::parse(s).unwrap();
            assert_eq!(
                InvoiceAmount::from_decimal_exact(a.into_decimal()).unwrap(),
                a
            );
        }
    }

    /// Annex A.1.6 — *Example 5, Negative Invoice line*: 25 cases of pens at
    /// 8,50 with 10 returned. Checks that the model can hold a mixed-sign
    /// invoice and that the totals chain closes exactly at two decimals.
    #[test]
    fn annex_a_1_6_negative_invoice_line() {
        let line1 = InvoiceAmount::parse("212.50").unwrap(); // BT-131, 25 × 8.50
        let line2 = InvoiceAmount::parse("-85.00").unwrap(); // BT-131, −10 × 8.50

        let bt_106 = InvoiceAmount::checked_sum([line1, line2]).unwrap();
        assert_eq!(bt_106, InvoiceAmount::parse("127.50").unwrap());

        // BR-CO-17: BT-117 = BT-116 × (BT-119 / 100), rounded to two decimals.
        let bt_117 = InvoiceAmount::parse("31.88").unwrap();
        let exact = bt_106.into_decimal() * dec!(25) / dec!(100); // 31.875
        assert_eq!(exact, dec!(31.875));
        // 31.875 is not representable, which is exactly why BR-CO-17 says
        // "rounded to two decimals" and why the artefact then allows ±1.00.
        assert!(InvoiceAmount::from_decimal_exact(exact).is_err());
        assert!((bt_117.into_decimal() - exact).abs() < dec!(1));

        // BR-CO-15 / BR-CO-16 close exactly.
        let bt_112 = bt_106.checked_add(bt_117).unwrap();
        assert_eq!(bt_112, InvoiceAmount::parse("159.38").unwrap());
        assert_eq!(bt_112.checked_sub(InvoiceAmount::ZERO).unwrap(), bt_112);
    }

    /// Annex A.1.7 — *Example 6, Prepayment and negative Amount due*.
    ///
    /// Also pins the erratum: the annex's remark column says BT-115 is
    /// "Invoice total **VAT** amount − Paid amount", but BR-CO-16 and the
    /// example's own arithmetic use BT-112.
    #[test]
    fn annex_a_1_7_negative_amount_due() {
        let bt_112 = InvoiceAmount::parse("137.50").unwrap();
        let bt_113 = InvoiceAmount::parse("250.00").unwrap();
        let bt_115 = bt_112.checked_sub(bt_113).unwrap(); // + BT-114, absent
        assert_eq!(bt_115, InvoiceAmount::parse("-112.50").unwrap());
        assert!(bt_115.is_negative(), "a refund is a lawful invoice");
    }

    #[test]
    fn ordering_and_display_padding() {
        let a = InvoiceAmount::parse("-1.00").unwrap();
        let b = InvoiceAmount::parse("1.00").unwrap();
        assert!(a < b);
        assert_eq!(format!("{b:>10}"), "      1.00");
        assert_eq!(format!("{a:<8}"), "-1.00   ");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_uses_a_decimal_string() {
        let a = InvoiceAmount::parse("1190.00").unwrap();
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(json, r#""1190.00""#, "never a float, never a raw integer");
        assert_eq!(serde_json::from_str::<InvoiceAmount>(&json).unwrap(), a);
        // Invariants survive deserialisation.
        assert!(serde_json::from_str::<InvoiceAmount>(r#""0.005""#).is_err());
    }
}

// ── UnitPriceAmount ───────────────────────────────────────────────────────────

/// A unit price — EN 16931-1 §6.5.3 `Unit Price Amount. Type`.
///
/// > A unit price amount states a numerical monetary amount value for data
/// > elements that contain item prices that may be multiplied by item
/// > quantities.
///
/// Based on `Amount. Type` but **without its two-decimal cap** — the standard's
/// own example for this type is `10000.1234`. That is the whole reason it is a
/// separate type, and it is why [`InvoiceAmount`] must never be used for BT-146,
/// BT-147 or BT-148: `0.28901 EUR/kWh` truncated to `0.29` is not a rounding
/// error, it is a different price.
///
/// # Why negative values are representable
///
/// BR-27 (*"The Item net price (BT-146) shall NOT be negative"*) and BR-28 (the
/// same for BT-148) are **rules**, not type invariants. The distinction this
/// crate draws throughout:
///
/// - a **type** enforces what is *representable* — §6.5.2's two decimals, a
///   calendar day that exists, the components a semantic type has;
/// - a **rule** enforces what is *valid* — and an invalid document must still be
///   representable, or a parser cannot load it in order to report why it fails.
///
/// A negative price is perfectly representable and simply violates BR-27. It
/// also arises legitimately upstream: spot markets have negative prices
/// (EPEX negative-price hours), and the conversion into EN 16931 flips the sign
/// onto the quantity rather than dropping the line.
///
/// ```
/// use en16931::UnitPriceAmount;
/// use rust_decimal::dec;
///
/// let p = UnitPriceAmount::new(dec!(0.28901));
/// assert_eq!(p.to_string(), "0.28901");   // five decimals survive
/// assert!(!p.is_negative());              // BR-27 is satisfied
/// ```
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct UnitPriceAmount(Decimal);

impl UnitPriceAmount {
    /// Zero.
    pub const ZERO: Self = Self(Decimal::ZERO);

    /// Wrap a decimal price.
    #[must_use]
    pub const fn new(value: Decimal) -> Self {
        Self(value)
    }

    /// The price.
    #[must_use]
    pub const fn into_decimal(self) -> Decimal {
        self.0
    }

    /// Whether the price is below zero — violates BR-27 for BT-146 and BR-28
    /// for BT-148.
    #[must_use]
    pub fn is_negative(self) -> bool {
        self.0 < Decimal::ZERO
    }

    /// `self - rhs`, for deriving BT-146 from BT-148 − BT-147.
    ///
    /// `PEPPOL-EN16931-R046` is an **exact** equality — unlike `R040` it carries
    /// no `u:slack` — so the net price is derived rather than accepted, and a
    /// caller cannot compute it and be a cent out.
    #[must_use]
    pub fn checked_sub(self, rhs: Self) -> Option<Self> {
        self.0.checked_sub(rhs.0).map(Self)
    }
}

impl fmt::Display for UnitPriceAmount {
    /// The exact value, trailing zeros stripped.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        crate::fmt::number(f, &self.0.normalize().to_string())
    }
}

impl From<Decimal> for UnitPriceAmount {
    fn from(d: Decimal) -> Self {
        Self(d)
    }
}

#[cfg(test)]
mod unit_price_tests {
    use super::*;
    use rust_decimal::dec;

    #[test]
    fn a_unit_price_is_not_capped_at_two_decimals() {
        // §6.5.3's own example is 10000.1234.
        let p = UnitPriceAmount::new(dec!(10000.1234));
        assert_eq!(p.to_string(), "10000.1234");
        // The same value as an Amount would be refused outright.
        assert!(InvoiceAmount::from_decimal_exact(dec!(10000.1234)).is_err());
    }

    #[test]
    fn metering_precision_survives() {
        assert_eq!(UnitPriceAmount::new(dec!(0.28901)).to_string(), "0.28901");
    }

    #[test]
    fn r046_derives_the_net_price_exactly() {
        // Annex A.1.6: gross 9,50 less discount 1,00 gives net 8,50.
        let gross = UnitPriceAmount::new(dec!(9.50));
        let discount = UnitPriceAmount::new(dec!(1.00));
        assert_eq!(
            gross.checked_sub(discount).unwrap(),
            UnitPriceAmount::new(dec!(8.50))
        );
    }

    #[test]
    fn negative_is_representable_because_br_27_is_a_rule() {
        let p = UnitPriceAmount::new(dec!(-0.005));
        assert!(
            p.is_negative(),
            "reportable as a BR-27 finding, not a parse error"
        );
    }
}
