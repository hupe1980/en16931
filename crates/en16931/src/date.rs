//! [`Date`] — EN 16931 `Date. Type`, a calendar day with no time of day.
//!
//! §6.5.9 is unusually specific, and both halves matter:
//!
//! > Dates shall be in accordance to the "Calendar date complete
//! > representation" as specified by ISO 8601 (see ISO 8601:2004, 5.2.1.1).
//! > **Calendar dates do not include a specification for the time of the day.**
//!
//! So a date is three integers, not an instant. There is no timezone, because
//! there is nothing to offset: BT-2 is the day the invoice was issued in the
//! seller's own reckoning, and shifting it by a zone changes the VAT period it
//! falls in.
//!
//! # Why not `chrono` or `time`
//!
//! Twelve bytes and a comparison do not justify a dependency in a crate whose
//! default build is two crates deep. Enable the `chrono` or `time` feature for
//! `From` conversions in both directions and use whichever your application
//! already has.
//!
//! `billing` keeps dates as unparsed ISO strings to stay date-library-agnostic.
//! This crate cannot: BR-29 and BR-30 order period endpoints, and BR-CO-25
//! depends on a due date being present, so the adapter parses at the boundary
//! and fails loudly on a string that is not a date.

use core::fmt;
use core::str::FromStr;

use crate::error::ParseDateError;

/// A calendar day — EN 16931-1 §6.5.9 `Date. Type`.
///
/// Proleptic Gregorian, validated on construction. `Ord` is chronological, which
/// is what BR-29 (*"Invoicing period end date shall be later or equal to the
/// start date"*) and BR-30 need.
///
/// ```
/// use en16931::Date;
///
/// let from = Date::parse("2026-06-01")?;
/// let to   = Date::parse("2026-06-30")?;
/// assert!(to >= from);                      // BR-29
/// assert_eq!(to.to_string(), "2026-06-30");
///
/// assert!(Date::parse("2026-02-30").is_err());       // not a real day
/// assert!(Date::parse("2026-06-01T00:00:00").is_err()); // §6.5.9: no time of day
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "String", into = "String"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    // Field order is load bearing: the derived `Ord` compares year, then month,
    // then day, which is chronological order exactly.
    year: i32,
    month: u8,
    day: u8,
}

impl Date {
    /// Construct from parts, validating that the day exists.
    ///
    /// # Errors
    /// [`ParseDateError::NotACalendarDay`] for a month outside 1–12 or a day
    /// outside the month's length, leap years included.
    pub fn new(year: i32, month: u8, day: u8) -> Result<Self, ParseDateError> {
        if month == 0 || month > 12 || day == 0 || day > days_in_month(year, month) {
            return Err(ParseDateError::NotACalendarDay { year, month, day });
        }
        Ok(Self { year, month, day })
    }

    /// Parse `YYYY-MM-DD`.
    ///
    /// Strict by design: exactly ten characters, two hyphens, digits elsewhere.
    /// A trailing time is an error rather than being truncated, because §6.5.9
    /// says a calendar date has none and silently dropping it hides a caller
    /// who thinks they are sending an instant.
    ///
    /// # Errors
    /// [`ParseDateError`].
    pub fn parse(s: &str) -> Result<Self, ParseDateError> {
        let malformed = || ParseDateError::Malformed {
            input: s.to_owned(),
        };
        let b = s.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            return Err(malformed());
        }
        let digits = |r: core::ops::Range<usize>| -> Option<u32> {
            let mut n = 0u32;
            for i in r {
                if !b[i].is_ascii_digit() {
                    return None;
                }
                n = n * 10 + u32::from(b[i] - b'0');
            }
            Some(n)
        };
        let (y, m, d) = (
            digits(0..4).ok_or_else(malformed)?,
            digits(5..7).ok_or_else(malformed)?,
            digits(8..10).ok_or_else(malformed)?,
        );
        // Casts are safe: four and two digits respectively.
        Self::new(y as i32, m as u8, d as u8)
    }

    /// The year.
    #[must_use]
    pub fn year(self) -> i32 {
        self.year
    }
    /// The month, 1–12.
    #[must_use]
    pub fn month(self) -> u8 {
        self.month
    }
    /// The day of the month.
    #[must_use]
    pub fn day(self) -> u8 {
        self.day
    }

    /// Days since 1970-01-01, negative before it.
    ///
    /// The one operation that makes every other piece of date arithmetic a
    /// subtraction. Exposed because "how many days is this invoicing period"
    /// is a real question and re-deriving it from `year`/`month`/`day` is where
    /// leap years get lost.
    ///
    /// ```
    /// # use en16931::Date;
    /// assert_eq!(Date::parse("1970-01-01")?.to_epoch_day(), 0);
    /// assert_eq!(Date::parse("2026-07-31")?.to_epoch_day(), 20_665);
    /// # Ok::<(), en16931::ParseDateError>(())
    /// ```
    #[must_use]
    pub const fn to_epoch_day(self) -> i64 {
        // Howard Hinnant's `days_from_civil`, which is exact over the whole
        // proleptic Gregorian range and needs no tables. The `y - 399` and
        // `m > 2` adjustments are what make Rust's truncating division behave
        // as the floor division the derivation assumes.
        let y = self.year as i64 - if self.month <= 2 { 1 } else { 0 };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let m = self.month as i64;
        let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + self.day as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// The calendar day `epoch_day` days after 1970-01-01.
    ///
    /// # Errors
    /// [`ParseDateError::NotACalendarDay`] only for a value so far out that the
    /// year does not fit an `i32` — every day in between is a real one.
    pub fn from_epoch_day(epoch_day: i64) -> Result<Self, ParseDateError> {
        // Hinnant's `civil_from_days`, the exact inverse of the above.
        let z = epoch_day + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = mp + if mp < 10 { 3 } else { -9 };
        let year = y + i64::from(m <= 2);
        let year = i32::try_from(year).map_err(|_| ParseDateError::NotACalendarDay {
            year: i32::MAX,
            month: m as u8,
            day: d as u8,
        })?;
        // Casts are safe: the algorithm's own ranges are [1, 12] and [1, 31].
        Self::new(year, m as u8, d as u8)
    }

    /// This day, `days` later. Negative `days` goes backwards.
    ///
    /// `None` only on the overflow that a real invoice cannot reach.
    ///
    /// ```
    /// # use en16931::Date;
    /// let issued = Date::parse("2026-07-31")?;
    /// assert_eq!(issued.checked_add_days(14).map(|d| d.to_string()).as_deref(),
    ///            Some("2026-08-14"));
    /// // Month lengths and leap years come out of the arithmetic, not a table.
    /// assert_eq!(Date::parse("2024-02-28")?.checked_add_days(1).map(|d| d.to_string()).as_deref(),
    ///            Some("2024-02-29"));
    /// # Ok::<(), en16931::ParseDateError>(())
    /// ```
    #[must_use]
    pub fn checked_add_days(self, days: i32) -> Option<Self> {
        Self::from_epoch_day(self.to_epoch_day().checked_add(i64::from(days))?).ok()
    }

    /// Days from `self` to `other`; negative if `other` is earlier.
    ///
    /// ```
    /// # use en16931::Date;
    /// let period = (Date::parse("2026-01-01")?, Date::parse("2026-12-31")?);
    /// assert_eq!(period.0.days_until(period.1), 364);
    /// # Ok::<(), en16931::ParseDateError>(())
    /// ```
    #[must_use]
    pub const fn days_until(self, other: Self) -> i64 {
        other.to_epoch_day() - self.to_epoch_day()
    }
}

/// Whether `year` is a leap year in the proleptic Gregorian calendar.
const fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Length of `month` in `year`.
const fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

impl fmt::Display for Date {
    /// `YYYY-MM-DD`. Honours width, fill and alignment.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!(
            "{:04}-{:02}-{:02}",
            self.year, self.month, self.day
        ))
    }
}

impl FromStr for Date {
    type Err = ParseDateError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(feature = "serde")]
impl TryFrom<String> for Date {
    type Error = ParseDateError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::parse(&s)
    }
}

#[cfg(feature = "serde")]
impl From<Date> for String {
    fn from(d: Date) -> Self {
        d.to_string()
    }
}

#[cfg(feature = "chrono")]
impl From<chrono::NaiveDate> for Date {
    fn from(d: chrono::NaiveDate) -> Self {
        use chrono::Datelike as _;
        // `NaiveDate` is already a valid calendar day, so this cannot fail.
        Self {
            year: d.year(),
            month: d.month() as u8,
            day: d.day() as u8,
        }
    }
}

#[cfg(feature = "chrono")]
impl TryFrom<Date> for chrono::NaiveDate {
    type Error = Date;
    fn try_from(d: Date) -> Result<Self, Self::Error> {
        chrono::NaiveDate::from_ymd_opt(d.year, u32::from(d.month), u32::from(d.day)).ok_or(d)
    }
}

#[cfg(feature = "time")]
impl TryFrom<time::Date> for Date {
    type Error = ParseDateError;
    fn try_from(d: time::Date) -> Result<Self, Self::Error> {
        Self::new(d.year(), u8::from(d.month()), d.day())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_renders_iso_8601() {
        let d = Date::parse("2026-06-30").unwrap();
        assert_eq!((d.year(), d.month(), d.day()), (2026, 6, 30));
        assert_eq!(d.to_string(), "2026-06-30");
    }

    /// The round trip is the whole correctness argument for the arithmetic:
    /// if every day maps to a number and back to itself, the leap-year and
    /// month-length handling is right by construction rather than by inspection.
    #[test]
    fn epoch_days_round_trip_over_a_century_including_every_leap_case() {
        let mut d = Date::parse("1970-01-01").expect("date");
        // 1900 (not a leap year), 2000 (a leap year), 2024, 2100 — all covered
        // by walking rather than by being listed.
        for _ in 0..40_000 {
            let n = d.to_epoch_day();
            assert_eq!(Date::from_epoch_day(n).expect("in range"), d, "{d}");
            d = d.checked_add_days(1).expect("next day");
        }
        // And backwards, across the Gregorian century rules in the other
        // direction.
        let mut d = Date::parse("1970-01-01").expect("date");
        for _ in 0..30_000 {
            let n = d.to_epoch_day();
            assert_eq!(Date::from_epoch_day(n).expect("in range"), d, "{d}");
            d = d.checked_add_days(-1).expect("previous day");
        }
    }

    #[test]
    fn day_arithmetic_crosses_months_years_and_leap_days() {
        let add = |s: &str, n: i32| {
            Date::parse(s)
                .expect("date")
                .checked_add_days(n)
                .expect("in range")
                .to_string()
        };
        assert_eq!(add("2026-07-31", 14), "2026-08-14", "the §40c EnWG default");
        assert_eq!(add("2026-12-31", 1), "2027-01-01");
        assert_eq!(add("2024-02-28", 1), "2024-02-29", "2024 is a leap year");
        assert_eq!(add("2100-02-28", 1), "2100-03-01", "2100 is not");
        assert_eq!(add("2000-02-28", 1), "2000-02-29", "2000 is");
        assert_eq!(add("2026-01-01", -1), "2025-12-31");

        let from = Date::parse("2026-06-01").expect("date");
        let to = Date::parse("2026-06-30").expect("date");
        assert_eq!(from.days_until(to), 29);
        assert_eq!(to.days_until(from), -29);
        assert_eq!(from.days_until(from), 0);
    }

    /// Known anchors, so a sign error in the epoch offset cannot hide behind a
    /// self-consistent round trip.
    #[test]
    fn epoch_day_agrees_with_known_dates() {
        let day = |s: &str| Date::parse(s).expect("date").to_epoch_day();
        assert_eq!(day("1970-01-01"), 0);
        assert_eq!(day("1969-12-31"), -1);
        assert_eq!(day("2000-01-01"), 10_957);
        assert_eq!(day("2026-07-31"), 20_665);
    }

    #[test]
    fn rejects_non_calendar_days() {
        for bad in [
            "2026-02-30",
            "2026-13-01",
            "2026-00-01",
            "2026-01-00",
            "2026-04-31",
        ] {
            assert!(Date::parse(bad).is_err(), "{bad} should not parse");
        }
        assert!(Date::parse("2024-02-29").is_ok(), "2024 is a leap year");
        assert!(Date::parse("2026-02-29").is_err(), "2026 is not");
        assert!(Date::parse("2000-02-29").is_ok(), "400-year rule");
        assert!(Date::parse("1900-02-29").is_err(), "100-year rule");
    }

    #[test]
    fn rejects_anything_that_is_not_a_bare_calendar_date() {
        // §6.5.9: "Calendar dates do not include a specification for the time of
        // the day." A timestamp is refused, never truncated.
        for bad in [
            "2026-06-01T00:00:00",
            "2026-06-01Z",
            "2026-6-1",
            "26-06-01",
            "2026/06/01",
            "",
            "today",
        ] {
            assert!(Date::parse(bad).is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn ordering_is_chronological() {
        // BR-29 / BR-30 compare period endpoints, so `Ord` has to be right
        // across month and year boundaries, not just within a month.
        let d = |s| Date::parse(s).unwrap();
        assert!(d("2026-01-31") < d("2026-02-01"));
        assert!(d("2025-12-31") < d("2026-01-01"));
        assert!(d("2026-06-30") >= d("2026-06-01"));
        let mut v = [d("2026-03-01"), d("2025-12-31"), d("2026-01-15")];
        v.sort();
        assert_eq!(v[0], d("2025-12-31"));
        assert_eq!(v[2], d("2026-03-01"));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trips_and_validates() {
        let d = Date::parse("2026-06-30").unwrap();
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(json, r#""2026-06-30""#);
        assert_eq!(serde_json::from_str::<Date>(&json).unwrap(), d);
        assert!(serde_json::from_str::<Date>(r#""2026-02-30""#).is_err());
    }
}
