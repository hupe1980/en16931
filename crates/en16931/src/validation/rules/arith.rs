//! The arithmetic the VAT-derivation rules share, written the way the
//! artefacts write it.
//!
//! # Why rounding needs its own module
//!
//! `BR-CO-17` and the nine `-09` rows are one calculation stated ten times, and
//! the calculation is not "multiply and round to two decimals". The CEN
//! artefacts spell it out in XPath:
//!
//! ```xpath
//! round(abs(TaxableAmount) * (Percent div 100) * 10 * 10) div 100
//! ```
//!
//! and guard it with `round(Percent) = 0` to pick the zero-rate branch. Both
//! `round`s are **XPath's**, and XPath's is not Rust's and not
//! `rust_decimal`'s:
//!
//! | | `round(0.5)` | `round(2.5)` | `round(-0.5)` |
//! |---|---|---|---|
//! | XPath `fn:round` — *"the one closest to +∞"* | `1` | `3` | `0` |
//! | `Decimal::round` — banker's | `0` | `2` | `0` |
//! | half away from zero | `1` | `3` | `-1` |
//!
//! No `RoundingStrategy` matches: banker's and half-away-from-zero each get one
//! of the two midpoint columns wrong. So [`xpath_round`] is `floor(x + 0.5)`,
//! which is the definition rather than an approximation of it.
//!
//! It is not academic. A VAT rate of exactly **0.5 %** — Spain's *recargo de
//! equivalencia* on reduced-rate goods is one — rounds to `1` for the artefact
//! and to `0` for banker's rounding, which sent `BR-CO-17` down its zero-rate
//! branch and made it reject a correct invoice. Every deployed validator
//! accepted the document this crate refused.

use rust_decimal::Decimal;

/// The ±1.00 the artefacts allow on the VAT derivation family.
///
/// **Not in the standard.** EN 16931-1 §6.4.2 states `BR-CO-17` as a plain
/// equation with no slack; the tolerance is the artefacts' decision, and it is
/// what every deployed validator runs. Shared by `BR-CO-17` and by the `-08`
/// and `-09` rows of all nine category tables, which is nine tables' worth of
/// reasons for it to exist exactly once.
pub(crate) const VAT_TOLERANCE: Decimal = Decimal::ONE;

/// `0.5`, as the constant `floor(x + 0.5)` needs.
const HALF: Decimal = Decimal::from_parts(5, 0, 0, false, 1);

/// XPath's `fn:round` — the closest integer, ties going towards **+∞**.
///
/// `floor(x + 0.5)` is the specification's own definition. See the module
/// header for why no `rust_decimal::RoundingStrategy` is equivalent.
///
/// Saturates rather than failing: `x + 0.5` can only overflow within half a
/// unit of `Decimal`'s range, where every amount in an invoice is already
/// nonsense, and returning the input keeps the rule evaluating.
pub(crate) fn xpath_round(x: Decimal) -> Decimal {
    x.checked_add(HALF).map_or(x, |shifted| shifted.floor())
}

/// The artefacts' derived VAT amount: `round(|base| × rate ÷ 100 × 100) ÷ 100`.
///
/// Written as `round(|base| × rate) ÷ 100`, which is the same number with one
/// fewer division: the artefact divides by 100 and multiplies by 100 again, and
/// both operands have few enough decimals that neither step can lose a digit.
///
/// `abs` on the base is the artefacts' too, and it is what lets a credit note
/// satisfy the rule at all.
///
/// Returns `None` only when the product overflows, which a caller should treat
/// as "cannot say" rather than as a failure — one unrepresentable group must
/// not silence the rule for the groups after it.
pub(crate) fn derived_vat(base: Decimal, rate: Decimal) -> Option<Decimal> {
    base.abs()
        .checked_mul(rate)
        .map(|product| xpath_round(product) / Decimal::ONE_HUNDRED)
}

/// Whether a stated VAT amount is within the artefacts' ±1.00 of `expected`.
///
/// The artefacts write it as two strict inequalities — `stated - 1 < expected`
/// and `stated + 1 > expected` — so the boundary belongs to the *failing* side:
/// a difference of exactly 1.00 is a finding.
pub(crate) fn within_vat_tolerance(stated: Decimal, expected: Decimal) -> bool {
    (stated - expected).abs() < VAT_TOLERANCE
}

#[cfg(test)]
mod tests {
    use rust_decimal::dec;

    use super::*;

    /// The three columns of the module header's table, as assertions.
    #[test]
    fn ties_go_towards_positive_infinity() {
        assert_eq!(xpath_round(dec!(0.5)), dec!(1));
        assert_eq!(xpath_round(dec!(2.5)), dec!(3));
        assert_eq!(xpath_round(dec!(-0.5)), dec!(0));
        assert_eq!(xpath_round(dec!(-1.5)), dec!(-1));
        // …and everything that is not a tie is simply the nearest.
        assert_eq!(xpath_round(dec!(0.4)), dec!(0));
        assert_eq!(xpath_round(dec!(0.6)), dec!(1));
        assert_eq!(xpath_round(dec!(-0.6)), dec!(-1));
        assert_eq!(xpath_round(dec!(19)), dec!(19));
    }

    /// The rate that made `BR-CO-17` reject a correct invoice.
    #[test]
    fn a_rate_of_half_a_per_cent_is_not_a_zero_rate() {
        assert_ne!(xpath_round(dec!(0.5)), Decimal::ZERO);
        assert_eq!(derived_vat(dec!(1000.00), dec!(0.5)), Some(dec!(5)));
    }

    #[test]
    fn the_derivation_is_taken_on_absolute_values() {
        // A credit note's negative base derives the same positive tax.
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
