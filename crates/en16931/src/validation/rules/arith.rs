//! The arithmetic the VAT-derivation rules share, written the way the
//! artefacts write it:
//!
//! ```xpath
//! round(abs(TaxableAmount) * (Percent div 100) * 10 * 10) div 100
//! ```
//!
//! guarded by `round(Percent) = 0` for the zero-rate branch. XPath's `round`
//! sends ties towards +∞, which no `rust_decimal::RoundingStrategy` matches:
//!
//! | | `round(0.5)` | `round(2.5)` | `round(-0.5)` |
//! |---|---|---|---|
//! | XPath `fn:round` | `1` | `3` | `0` |
//! | `Decimal::round` — banker's | `0` | `2` | `0` |
//! | half away from zero | `1` | `3` | `-1` |
//!
//! A 0.5 % VAT rate is the case that tells them apart.

use rust_decimal::Decimal;

use crate::InvoiceAmount;

/// The ±1.00 the artefacts allow on `BR-CO-17` and the category `-08`/`-09`
/// rows. Not in EN 16931-1 §6.4.2, which states a plain equation.
pub(crate) const VAT_TOLERANCE: Decimal = Decimal::ONE;

/// `0.5`, as the constant `floor(x + 0.5)` needs.
const HALF: Decimal = Decimal::from_parts(5, 0, 0, false, 1);

/// XPath's `fn:round` — `floor(x + 0.5)`, ties towards +∞. Returns `x`
/// unchanged if `x + 0.5` overflows.
pub(crate) fn xpath_round(x: Decimal) -> Decimal {
    x.checked_add(HALF).map_or(x, |shifted| shifted.floor())
}

/// The artefacts' derived VAT amount: `round(|base| × rate ÷ 100 × 100) ÷ 100`.
///
/// Computed as the equivalent `round(|base| × rate) ÷ 100`. `None` when the
/// product is not exact ([`exact_mul`]); callers report that group.
pub(crate) fn derived_vat(base: Decimal, rate: Decimal) -> Option<Decimal> {
    exact_mul(base.abs(), rate).map(|product| xpath_round(product) / Decimal::ONE_HUNDRED)
}

/// `a × b` when a `Decimal` holds it exactly. `None` when it overflows, or when
/// it needs more than 28 places, which `rust_decimal` would round without
/// saying so. Trailing zeros of the factors do not count.
pub(crate) fn exact_mul(a: Decimal, b: Decimal) -> Option<Decimal> {
    // A zero factor gives an exact zero, whatever scale `rust_decimal` keeps.
    if a.is_zero() || b.is_zero() {
        return Some(Decimal::ZERO);
    }
    let (a, b) = (a.normalize(), b.normalize());
    a.checked_mul(b)
        .filter(|product| product.scale() == a.scale() + b.scale())
}

/// `a − b` when a `Decimal` holds it exactly, as [`exact_mul`].
pub(crate) fn exact_sub(a: Decimal, b: Decimal) -> Option<Decimal> {
    if a == b {
        return Some(Decimal::ZERO);
    }
    let (a, b) = (a.normalize(), b.normalize());
    a.checked_sub(b)
        .filter(|difference| difference.scale() == a.scale().max(b.scale()))
}

/// Advice for a finding whose expected amount does not fit a `Decimal`.
pub(crate) const BEYOND_RANGE: &str = "The expected amount exceeds the range or the 28 places \
     of a decimal, so it cannot be checked exactly.";

/// The exact sum of `amounts`. Each fits an `i64` of cents, so a `Decimal`
/// holds the sum of more amounts than a document can carry, where a sum of
/// [`InvoiceAmount`]s overflows and would leave its rule unchecked.
pub(crate) fn exact_sum(amounts: impl IntoIterator<Item = InvoiceAmount>) -> Decimal {
    amounts.into_iter().fold(Decimal::new(0, 2), |sum, amount| {
        sum + amount.into_decimal()
    })
}

/// Whether a stated VAT amount is within the artefacts' ±1.00 of `expected`.
///
/// Strict, as the artefacts write it: a difference of exactly 1.00 fails.
pub(crate) fn within_vat_tolerance(stated: Decimal, expected: Decimal) -> bool {
    (stated - expected).abs() < VAT_TOLERANCE
}

#[cfg(test)]
mod tests {
    use rust_decimal::dec;

    use super::*;

    #[test]
    fn ties_go_towards_positive_infinity() {
        assert_eq!(xpath_round(dec!(0.5)), dec!(1));
        assert_eq!(xpath_round(dec!(2.5)), dec!(3));
        assert_eq!(xpath_round(dec!(-0.5)), dec!(0));
        assert_eq!(xpath_round(dec!(-1.5)), dec!(-1));
        assert_eq!(xpath_round(dec!(0.4)), dec!(0));
        assert_eq!(xpath_round(dec!(0.6)), dec!(1));
        assert_eq!(xpath_round(dec!(-0.6)), dec!(-1));
        assert_eq!(xpath_round(dec!(19)), dec!(19));
    }

    /// 1.01 × 1.4851485148514851485148514851 is just below 1.5; rounded to
    /// `Decimal`'s 28 places it would be 1.5 and round up to 0.02.
    #[test]
    fn a_product_beyond_decimal_precision_is_not_rounded_silently() {
        assert_eq!(
            derived_vat(dec!(1.01), dec!(1.4851485148514851485148514851)),
            None
        );
    }

    /// A zero factor or two equal values give an exact zero (CEN's
    /// `BIS_Billing_30-Telefoni.xml` has 693.8 × 0).
    #[test]
    fn zero_is_exact() {
        assert_eq!(exact_mul(dec!(693.8), dec!(0)), Some(Decimal::ZERO));
        assert_eq!(exact_sub(dec!(1.50), dec!(1.5)), Some(Decimal::ZERO));
    }

    #[test]
    fn a_rate_of_half_a_per_cent_is_not_a_zero_rate() {
        assert_ne!(xpath_round(dec!(0.5)), Decimal::ZERO);
        assert_eq!(derived_vat(dec!(1000.00), dec!(0.5)), Some(dec!(5)));
    }

    #[test]
    fn the_derivation_is_taken_on_absolute_values() {
        assert_eq!(
            derived_vat(dec!(-1000.00), dec!(19)),
            derived_vat(dec!(1000.00), dec!(19))
        );
    }

    #[test]
    fn a_full_currency_unit_of_slack_excludes_its_own_boundary() {
        assert!(within_vat_tolerance(dec!(190.99), dec!(190.00)));
        assert!(!within_vat_tolerance(dec!(191.00), dec!(190.00)));
        assert!(!within_vat_tolerance(dec!(189.00), dec!(190.00)));
    }
}
