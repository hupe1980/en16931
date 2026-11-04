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

/// XPath's `fn:round` — `floor(x + 0.5)`, ties towards +∞. Taken as
/// `floor(x)`, plus one when the fraction is at least a half: `x + 0.5`
/// itself would round when `x` has 28 places. Returns `x` unchanged if the
/// result overflows.
pub(crate) fn xpath_round(x: Decimal) -> Decimal {
    let floor = x.floor();
    if x - floor >= HALF {
        floor.checked_add(Decimal::ONE).unwrap_or(x)
    } else {
        floor
    }
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
/// saying so.
pub(crate) fn exact_mul(a: Decimal, b: Decimal) -> Option<Decimal> {
    // A zero factor gives an exact zero, whatever scale `rust_decimal` keeps.
    if a.is_zero() || b.is_zero() {
        return Some(Decimal::ZERO);
    }
    let product = a.checked_mul(b)?;
    // The exact product has the scale `a.scale() + b.scale()`. To hold it,
    // `rust_decimal` drops its lowest digits; the product is exact when each
    // of them is zero, that is when the mantissas have as many factors of 2
    // and of 5 between them.
    let dropped = (a.scale() + b.scale()).saturating_sub(product.scale());
    let (ma, mb) = (a.mantissa().unsigned_abs(), b.mantissa().unsigned_abs());
    let holds = |prime: u128| factors(ma, prime) + factors(mb, prime) >= dropped;
    (holds(2) && holds(5)).then_some(product)
}

/// How often `prime` divides `m`, which is not zero.
fn factors(mut m: u128, prime: u128) -> u32 {
    let mut count = 0;
    while m.is_multiple_of(prime) {
        m /= prime;
        count += 1;
    }
    count
}

/// `a − b` when a `Decimal` holds it exactly, as [`exact_mul`].
pub(crate) fn exact_sub(a: Decimal, b: Decimal) -> Option<Decimal> {
    let difference = a.checked_sub(b)?;
    // The exact difference has the larger scale of the two; the digits
    // `rust_decimal` drops below the result's scale must all be zero.
    let scale = a.scale().max(b.scale());
    let dropped = scale.saturating_sub(difference.scale());
    let modulus = 10_i128.pow(dropped);
    // A value at `scale`, modulo `modulus`: its mantissa shifted to that scale.
    let rest = |d: Decimal| {
        let shift = scale - d.scale();
        if shift >= dropped {
            0
        } else {
            d.mantissa().rem_euclid(10_i128.pow(dropped - shift)) * 10_i128.pow(shift)
        }
    };
    ((rest(a) - rest(b)).rem_euclid(modulus) == 0).then_some(difference)
}

/// `a + b` when a `Decimal` holds it exactly, as [`exact_mul`].
pub(crate) fn exact_add(a: Decimal, b: Decimal) -> Option<Decimal> {
    exact_sub(a, -b)
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

    /// The exact product needs scale 30; `rust_decimal` drops its two trailing
    /// zeros, and nothing else.
    #[test]
    fn a_product_that_drops_only_zeros_is_exact() {
        assert_eq!(
            exact_mul(dec!(0.20), dec!(5.0000000000000000000000000005)),
            Some(dec!(1.0000000000000000000000000001))
        );
        assert_eq!(
            derived_vat(dec!(0.20), dec!(5.0000000000000000000000000005)),
            Some(dec!(0.01))
        );
    }

    /// The exact difference needs 29 digits at scale 28; `rust_decimal` drops
    /// its trailing zero, or rounds a digit that is not one.
    #[test]
    fn a_difference_that_drops_only_zeros_is_exact() {
        let gross = dec!(4.0000000000000000000000000005);
        assert_eq!(
            exact_sub(gross, -gross),
            Some(dec!(8.000000000000000000000000001))
        );
        assert_eq!(
            exact_sub(gross, dec!(-4.0000000000000000000000000004)),
            None
        );
    }

    /// `x + 0.5` would round 7.4999…9 (28 nines after the point) to 8 before
    /// `floor`; XPath's `round` gives 7.
    #[test]
    fn rounding_half_up_rounds_once() {
        assert_eq!(xpath_round(dec!(7.4999999999999999999999999999)), dec!(7));
        assert_eq!(xpath_round(dec!(-7.5000000000000000000000000001)), dec!(-8));
        assert_eq!(
            derived_vat(dec!(1.00), dec!(7.4999999999999999999999999999)),
            Some(dec!(0.07))
        );
    }
}
