//! Error types.
//!
//! These are **not** validation findings. A `ValidationReport` says "this
//! invoice does not satisfy BR-CO-14"; the errors here say "the value you handed
//! me is not a value". Conflating the two makes both harder to act on: one is a
//! document problem for an accountant, the other is a bug for a programmer.

use thiserror::Error;

/// Parsing a monetary amount from a string failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ParseAmountError {
    /// The input was empty or whitespace only.
    #[error("amount must not be empty")]
    Empty,

    /// The input was not a plain decimal number.
    ///
    /// Deliberately strict: no thousands separators, no comma decimal mark, no
    /// exponent. An invoice amount arrives from a wire format that specifies
    /// exactly one spelling, and accepting more is how `1,50` becomes `150`.
    #[error("amount {input:?} is not a plain decimal number")]
    Malformed {
        /// The rejected input.
        input: String,
    },

    /// More decimals than EN 16931 `Amount. Type` can carry.
    ///
    /// §6.5.2: *"EN 16931_ Amount. Type is floating up to two fraction digits."*
    /// Refused rather than rounded — see [`crate::InvoiceAmount`].
    #[error("amount {input:?} has more than {max} decimals (EN 16931-1 §6.5.2)")]
    TooManyDecimals {
        /// The rejected input.
        input: String,
        /// The maximum number of decimals, always 2.
        max: u8,
    },

    /// The value does not fit the representable range.
    #[error("amount {input:?} is outside the representable range")]
    OutOfRange {
        /// The rejected input.
        input: String,
    },
}

/// Arithmetic or conversion on a monetary amount failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum AmountError {
    /// The result does not fit the representable range.
    ///
    /// Every arithmetic method returns `Result` rather than wrapping or
    /// saturating: an invoice total that overflows is a data error worth
    /// surfacing, never a number worth guessing.
    #[error("monetary overflow")]
    Overflow,

    /// Converting from a wider decimal would have lost precision.
    ///
    /// Raised instead of rounding. Rounding here is the mistake that breaks the
    /// totals identities: BR-CO-10 and BR-CO-15 are exact equalities over sums,
    /// so rounding leaves and aggregates independently makes them disagree.
    /// Reduce precision at the *source* — `billing`'s `AmountScale::EN16931`
    /// rounds every leaf before any total is computed — and convert losslessly
    /// here.
    #[error("value {value} needs more than {max} decimals; reduce precision at the source")]
    PrecisionLoss {
        /// The value that did not fit.
        value: String,
        /// The maximum number of decimals, always 2.
        max: u8,
    },
}

/// Parsing a calendar date failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ParseDateError {
    /// Not `YYYY-MM-DD`.
    ///
    /// §6.5.9 requires the ISO 8601 *"Calendar date complete representation"*,
    /// and says explicitly that calendar dates **do not include a specification
    /// for the time of day** — so a timestamp is rejected rather than truncated.
    #[error("date {input:?} is not an ISO 8601 calendar date (YYYY-MM-DD)")]
    Malformed {
        /// The rejected input.
        input: String,
    },

    /// Well-formed but not a real day, such as `2026-02-30`.
    #[error("date {year:04}-{month:02}-{day:02} does not exist")]
    NotACalendarDay {
        /// Proleptic Gregorian year.
        year: i32,
        /// Month, 1–12.
        month: u8,
        /// Day of month.
        day: u8,
    },
}
