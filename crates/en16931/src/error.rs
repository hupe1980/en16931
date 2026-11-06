//! Errors for values that cannot be constructed. These are not validation
//! findings, which a validation report carries.

use thiserror::Error;

/// Parsing a monetary amount from a string failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ParseAmountError {
    /// The input was empty or whitespace only.
    #[error("amount must not be empty")]
    Empty,

    /// The input was not a plain decimal number (no thousands separators,
    /// comma decimal mark or exponent).
    #[error("amount {input:?} is not a plain decimal number")]
    Malformed {
        /// The rejected input.
        input: String,
    },

    /// More than the two decimals EN 16931-1 §6.5.2 allows; never rounded.
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

/// A number is not an `xs:decimal` the model holds exactly: the spelling the
/// UBL and CII schemas give every quantity, price, rate and amount.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ParseDecimalError {
    /// Not digits with at most one point and an optional sign: an exponent
    /// (`1e3`), a separator (`1_000`, `1,5`), whitespace or no digit at all.
    #[error("{input:?} is not an xs:decimal")]
    Malformed {
        /// The rejected input.
        input: String,
    },

    /// Beyond the largest magnitude a decimal holds.
    #[error("{input:?} is outside the range a decimal holds")]
    OutOfRange {
        /// The rejected input.
        input: String,
    },

    /// More significant digits than a decimal holds; never rounded.
    #[error("{input:?} has more digits than a decimal holds exactly")]
    TooPrecise {
        /// The rejected input.
        input: String,
    },
}

/// Arithmetic or conversion on a monetary amount failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum AmountError {
    /// The result does not fit the representable range.
    #[error("monetary overflow")]
    Overflow,

    /// Converting from a wider decimal would have lost precision. Round at the
    /// source, before totals are computed.
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
    /// Not `YYYY-MM-DD` (§6.5.9); a timestamp is rejected, not truncated.
    #[error("date {input:?} is not an ISO 8601 calendar date (YYYY-MM-DD)")]
    Malformed {
        /// The rejected input.
        input: String,
    },

    /// Not a real day, such as `2026-02-30`, or a year outside `0000..=9999`.
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
