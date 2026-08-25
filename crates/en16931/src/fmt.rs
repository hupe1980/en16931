//! Formatting that cannot print a value that is not the value.
//!
//! # `Formatter::pad` truncates, and every type here used it
//!
//! [`core::fmt::Formatter::pad`] is the standard helper for a `Display` that
//! produces a string: it honours width, fill and alignment, and — because that
//! is what precision means for a *string* — it also truncates to
//! `precision` **characters**. Every `Display` in this crate called it.
//!
//! ```text
//! format!("{:.2}", InvoiceAmount::parse("1190.00")?)   →  "11"
//! format!("{:>12.4}", InvoiceAmount::parse("1190.00")?)   →  "        1190"
//! format!("{:.4}", Date::parse("2026-07-31")?)         →  "2026"
//! ```
//!
//! A caller writing `{:.2}` to get two decimal places got eleven euros. Not a
//! layout defect — a **wrong number**, right-aligned in a column, in the one
//! place a person reads it. This crate refuses to round an amount at a
//! boundary; printing one at a hundredth of its value is the same failure with
//! a worse blast radius, and it was reachable from ordinary formatting syntax.
//!
//! So nothing here truncates. Two helpers:
//!
//! * [`padded`] — width, fill and alignment, and precision **ignored**. For
//!   values where precision has no meaning: a date, a code, a rule id, a path.
//!   A truncated `BG-25[2]/BT-151` is not a shorter path, it is a different one.
//! * [`number`] — the same, with precision read as a **minimum** number of
//!   fraction digits.
//!
//! # Why precision is a minimum and not an exact count
//!
//! For an `f64`, `{:.2}` rounds, and that is unremarkable because an `f64` was
//! never exact. These types are exact on purpose, and the crate's whole argument
//! is that an amount which does not fit two decimals is an error rather than a
//! rounding opportunity ([`crate::InvoiceAmount::parse`]). A `Display` that
//! quietly rounded would put a number on an invoice that differs from the number
//! the invoice states — which is the failure the type system here exists to
//! prevent, arriving through the formatter instead.
//!
//! So `{:.4}` on `1190.00` is `1190.0000`, and `{:.0}` is still `1190.00`.
//! Padding is lossless and truncation is not, so only one of them happens.
//! Callers who genuinely want a rounded presentation have
//! [`rust_decimal::Decimal::round_dp`] and the exact value to apply it to.

use core::fmt;

/// Write `s` honouring width, fill and alignment — and **never** precision.
///
/// [`Formatter::pad`](core::fmt::Formatter::pad) with its one dangerous
/// behaviour removed. See the [module documentation](self).
pub fn padded(f: &mut fmt::Formatter<'_>, s: &str) -> fmt::Result {
    let Some(width) = f.width() else {
        return f.write_str(s);
    };
    let len = s.chars().count();
    if len >= width {
        return f.write_str(s);
    }
    let pad = width - len;
    // `Default` matches `Formatter::pad`'s: a value rendered as a string aligns
    // left unless asked otherwise. Preserved rather than improved, because
    // changing the default alignment of every type in the crate is a separate
    // decision from not corrupting the value.
    let (before, after) = match f.align() {
        Some(fmt::Alignment::Right) => (pad, 0),
        Some(fmt::Alignment::Center) => (pad / 2, pad - pad / 2),
        Some(fmt::Alignment::Left) | None => (0, pad),
    };
    let fill = f.fill();
    for _ in 0..before {
        f.write_str(fill.encode_utf8(&mut [0u8; 4]))?;
    }
    f.write_str(s)?;
    for _ in 0..after {
        f.write_str(fill.encode_utf8(&mut [0u8; 4]))?;
    }
    Ok(())
}

/// As [`padded`], reading precision as a **minimum** number of fraction digits.
///
/// `value` must already be the canonical rendering — `"1190.00"`, `"19"`,
/// `"0.28901"`. Zeros are appended to reach the requested scale and nothing is
/// ever removed; see the [module documentation](self) for why.
pub fn number(f: &mut fmt::Formatter<'_>, value: &str) -> fmt::Result {
    let Some(want) = f.precision() else {
        return padded(f, value);
    };
    let have = value.split_once('.').map_or(0, |(_, frac)| frac.len());
    if have >= want {
        return padded(f, value);
    }
    let mut out = String::with_capacity(value.len() + (want - have) + 1);
    out.push_str(value);
    if have == 0 {
        out.push('.');
    }
    for _ in have..want {
        out.push('0');
    }
    padded(f, &out)
}

#[cfg(test)]
mod tests {
    use crate::{Date, InvoiceAmount, Percentage, Quantity, UnitPriceAmount};
    use rust_decimal::dec;

    /// `{:.n}` is a minimum width for these types, never a truncation:
    /// `Formatter::pad` would cut `1190.00` to `11`, which is a different
    /// amount.
    #[test]
    fn precision_never_truncates_a_value() {
        let a = InvoiceAmount::parse("1190.00").expect("amount");
        assert_eq!(format!("{a:.2}"), "1190.00");
        assert_eq!(format!("{a:.3}"), "1190.000");
        assert_eq!(format!("{a:.0}"), "1190.00", "precision is a minimum");
        assert_eq!(format!("{a:>12.4}"), "   1190.0000");

        let d = Date::parse("2026-07-31").expect("date");
        assert_eq!(format!("{d:.4}"), "2026-07-31", "was \"2026\"");
        assert_eq!(format!("{d:.2}"), "2026-07-31");
    }

    /// Padding reaches the requested scale on every numeric type.
    #[test]
    fn precision_pads_to_the_requested_scale() {
        assert_eq!(format!("{:.4}", Percentage::new(dec!(19))), "19.0000");
        assert_eq!(format!("{:.2}", Quantity::new(dec!(10000))), "10000.00");
        assert_eq!(
            format!("{:.7}", UnitPriceAmount::new(dec!(0.28901))),
            "0.2890100"
        );
        // …and never reduces one that is already longer.
        assert_eq!(
            format!("{:.2}", UnitPriceAmount::new(dec!(0.28901))),
            "0.28901"
        );
    }

    /// Width, fill and alignment are unchanged — they were never the problem.
    #[test]
    fn width_fill_and_alignment_still_behave() {
        let a = InvoiceAmount::parse("1.00").expect("amount");
        assert_eq!(format!("{a:>10}"), "      1.00");
        assert_eq!(format!("{a:<10}"), "1.00      ");
        assert_eq!(format!("{a:^10}"), "   1.00   ");
        assert_eq!(format!("{a:*>10}"), "******1.00");
        assert_eq!(format!("{a}"), "1.00", "no width, no padding");
        // A width narrower than the value never clips it.
        assert_eq!(format!("{a:>2}"), "1.00");
        // A multi-byte fill is one character wide, not one byte.
        assert_eq!(format!("{a:—>8}"), "————1.00");
    }
}
