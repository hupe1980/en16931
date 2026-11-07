//! Peppol BIS Billing 3.0's own rules, attached through
//! [`crate::Profile::extra_rules`] rather than the core set (EN 16931 has no
//! rule tying BT-131 to quantity × price).
//!
//! Only `R120` and `R040` have slack: Peppol's `u:slack` at ±0.02, which
//! XRechnung's merged instances ([`XR_R120`], [`XR_R040`]) widen to ±0.5 for
//! HUF. Everything else here is exact, including `R046`.

use rust_decimal::Decimal;

use super::arith::{BEYOND_RANGE, exact_mul, exact_sub, exact_sum};
use super::{Findings, Rule, RuleId, Severity, Source};
use crate::bt::BtId;
use crate::bt::{Group, Path};
use crate::codes::generated as lists;
use crate::invoice::{Invoice, LineAllowanceCharge, terms as bt};
use crate::{InvoiceAmount, Percentage};

/// Peppol's `u:slack`, verbatim.
pub const SLACK: Decimal = Decimal::from_parts(2, 0, 0, false, 2); // 0.02

/// XRechnung's slack for Hungarian forint, as its merge rewrites `u:slack`
/// (`if($documentCurrencyCode = 'HUF') then 0.5 else 0.02`). Peppol itself
/// uses 0.02 for every currency.
pub const HUF_SLACK: Decimal = Decimal::from_parts(5, 0, 0, false, 1); // 0.5

/// Peppol's policy: 0.02, whatever the currency.
fn fixed_slack(_: &Invoice) -> Decimal {
    SLACK
}

/// XRechnung's policy: 0.5 for HUF, 0.02 otherwise.
pub(crate) fn currency_slack(inv: &Invoice) -> Decimal {
    if inv.currency.as_ref().is_some_and(|c| c.as_str() == "HUF") {
        HUF_SLACK
    } else {
        SLACK
    }
}

/// `|expected − actual| ≤ slack`, matching `u:slack`'s inclusive comparison.
/// A difference beyond `Decimal`'s range is never within the slack.
fn within(expected: Decimal, actual: Decimal, slack: Decimal) -> bool {
    expected
        .checked_sub(actual)
        .is_some_and(|difference| difference.abs() <= slack)
}

macro_rules! rule {
    (
        $konst:ident, $id:literal,
        terms: [$($t:expr),* $(,)?],
        $text:literal,
        |$inv:ident, $f:ident| $body:block
    ) => {
        #[doc = $text]
        pub static $konst: Rule = Rule {
            id: RuleId::new($id),
            severity: Severity::Fatal,
            text: $text,
            terms: &[$($t),*],
            source: Source::ArtefactOnly,
            eval: |$inv: &Invoice, $f: &mut Findings<'_>| $body,
        };
    };
}

// ── R120 — the line amount, ±0.02 ─────────────────────────────────────────────

/// `R120`'s check, parameterised by the profile's slack policy.
pub(crate) fn check_line_amount(
    inv: &Invoice,
    f: &mut Findings<'_>,
    slack: fn(&Invoice) -> Decimal,
) {
    let tolerance = slack(inv);
    for (i, line) in inv.lines.iter().enumerate() {
        // `None` means 1, matching R120's own `$baseQuantity` default.
        let base = line
            .price
            .base_quantity
            .map_or(Decimal::ONE, |q| q.into_decimal());
        if base.is_zero() {
            continue; // R121 reports this; dividing would panic.
        }
        let sum = |v: &[LineAllowanceCharge]| exact_sum(v.iter().map(|a| a.amount));
        let (charges, allowances) = (sum(&line.charges), sum(&line.allowances));
        let path = Path::at_term(Group::Line, i, bt::LINE_NET_AMOUNT);
        let (quantity, price) = (
            line.quantity.into_decimal(),
            line.price.net_price.into_decimal(),
        );
        // R120 compares within a slack of 0.02, so rounding at 28 places,
        // as XPath's decimal arithmetic does, never changes its outcome; only
        // an overflow is reported. The exact product comes first, then the
        // larger factor divided first, which keeps its digits where the
        // product alone overflows or a tiny base quantity would magnify the
        // rounding; only then the rounded product.
        let (large, small) = if quantity.abs() >= price.abs() {
            (quantity, price)
        } else {
            (price, quantity)
        };
        let divided_first = |multiply: fn(Decimal, Decimal) -> Option<Decimal>| {
            large
                .checked_div(base)
                .and_then(|share| multiply(share, small))
        };
        let exact = |a: Decimal, b: Decimal| exact_mul(a, b).ok();
        let expected = exact(quantity, price)
            .and_then(|p| p.checked_div(base))
            .or_else(|| divided_first(exact))
            .or_else(|| {
                quantity
                    .checked_mul(price)
                    .and_then(|p| p.checked_div(base))
            })
            .or_else(|| divided_first(Decimal::checked_mul))
            .and_then(|p| p.checked_add(charges))
            .and_then(|p| p.checked_sub(allowances));
        match expected {
            None => f.at_with_hint(path, BEYOND_RANGE),
            Some(expected) if !within(expected, line.net_amount.into_decimal(), tolerance) => {
                f.arithmetic(path, expected.round_dp(2), line.net_amount);
            }
            Some(_) => {}
        }
    }
}

rule!(R120, "PEPPOL-EN16931-R120",
terms: [bt::LINE_NET_AMOUNT, bt::LINE_QUANTITY, bt::ITEM_NET_PRICE, bt::PRICE_BASE_QUANTITY],
"Invoice line net amount MUST equal (Invoiced quantity * (Item net price/item price base \
 quantity) + Sum of invoice line charge amount - Sum of invoice line allowance amount).",
|inv, f| {
    check_line_amount(inv, f, fixed_slack);
});

// ── R040 / R041 / R042 — the allowance basis ──────────────────────────────────

/// Check one allowance or charge's `amount = base × percentage / 100`.
fn check_basis(
    amount: InvoiceAmount,
    base: Option<InvoiceAmount>,
    pct: Option<Percentage>,
    path: Path,
    f: &mut Findings<'_>,
    tolerance: Decimal,
) {
    let (Some(base), Some(pct)) = (base, pct) else {
        return;
    };
    // A stated amount fits an `i64` of cents, far below `Decimal::MAX / 100`,
    // so none matches a product beyond `Decimal`'s range. R040 compares within
    // a slack, so the product's rounding at 28 places is accepted.
    let Some(product) = base.into_decimal().checked_mul(pct.into_decimal()) else {
        f.at_with_hint(path, BEYOND_RANGE);
        return;
    };
    // Compared a hundredfold, so the division by 100 rounds nothing away.
    let hundredfold = amount.into_decimal() * Decimal::ONE_HUNDRED;
    if !within(product, hundredfold, tolerance * Decimal::ONE_HUNDRED) {
        f.arithmetic(path, (product / Decimal::ONE_HUNDRED).round_dp(2), amount);
    }
}

/// `R040`'s check, parameterised by the profile's slack policy.
pub(crate) fn check_allowance_basis(
    inv: &Invoice,
    f: &mut Findings<'_>,
    slack: fn(&Invoice) -> Decimal,
) {
    let tolerance = slack(inv);
    for (i, a) in inv.allowances.iter().enumerate() {
        check_basis(
            a.amount,
            a.base_amount,
            a.percentage,
            Path::at_term(Group::DocumentAllowance, i, bt::ALLOWANCE_AMOUNT),
            f,
            tolerance,
        );
    }
    for (i, c) in inv.charges.iter().enumerate() {
        check_basis(
            c.amount,
            c.base_amount,
            c.percentage,
            Path::at_term(Group::DocumentCharge, i, bt::CHARGE_AMOUNT),
            f,
            tolerance,
        );
    }
    // The Schematron context includes line-level BG-27 and BG-28 too.
    for (i, line) in inv.lines.iter().enumerate() {
        for a in line.allowances.iter().chain(&line.charges) {
            check_basis(
                a.amount,
                a.base_amount,
                a.percentage,
                Path::at_term(Group::Line, i, bt::LINE_ALLOWANCE_AMOUNT),
                f,
                tolerance,
            );
        }
    }
}

rule!(R040, "PEPPOL-EN16931-R040",
terms: [bt::ALLOWANCE_AMOUNT, bt::ALLOWANCE_BASE, bt::ALLOWANCE_PERCENTAGE],
"Allowance/charge amount must equal base amount * percentage/100 if base amount and \
 percentage exists.",
|inv, f| {
    check_allowance_basis(inv, f, fixed_slack);
});

rule!(R041, "PEPPOL-EN16931-R041",
terms: [bt::ALLOWANCE_BASE, bt::ALLOWANCE_PERCENTAGE],
"Allowance/charge base amount MUST be provided when allowance/charge percentage is provided.",
|inv, f| {
    let half = |base: Option<InvoiceAmount>, pct: Option<Percentage>| {
        pct.is_some() && base.is_none()
    };
    for (i, a) in inv.allowances.iter().enumerate() {
        if half(a.base_amount, a.percentage) {
            f.at(Path::at_term(Group::DocumentAllowance, i, bt::ALLOWANCE_BASE));
        }
    }
    for (i, c) in inv.charges.iter().enumerate() {
        if half(c.base_amount, c.percentage) {
            f.at(Path::at_term(Group::DocumentCharge, i, bt::CHARGE_BASE));
        }
    }
    for (i, line) in inv.lines.iter().enumerate() {
        for a in line.allowances.iter().chain(&line.charges) {
            if half(a.base_amount, a.percentage) {
                f.at(Path::at_term(Group::Line, i, bt::LINE_ALLOWANCE_AMOUNT));
            }
        }
    }
});

rule!(R042, "PEPPOL-EN16931-R042",
terms: [bt::ALLOWANCE_BASE, bt::ALLOWANCE_PERCENTAGE],
"Allowance/charge percentage MUST be provided when allowance/charge base amount is provided.",
|inv, f| {
    let half = |base: Option<InvoiceAmount>, pct: Option<Percentage>| {
        base.is_some() && pct.is_none()
    };
    for (i, a) in inv.allowances.iter().enumerate() {
        if half(a.base_amount, a.percentage) {
            f.at(Path::at_term(Group::DocumentAllowance, i, bt::ALLOWANCE_PERCENTAGE));
        }
    }
    for (i, c) in inv.charges.iter().enumerate() {
        if half(c.base_amount, c.percentage) {
            f.at(Path::at_term(Group::DocumentCharge, i, bt::CHARGE_PERCENTAGE));
        }
    }
    for (i, line) in inv.lines.iter().enumerate() {
        for a in line.allowances.iter().chain(&line.charges) {
            if half(a.base_amount, a.percentage) {
                f.at(Path::at_term(Group::Line, i, bt::LINE_ALLOWANCE_AMOUNT));
            }
        }
    }
});

// ── BG-29 — R046, R121, R130 ──────────────────────────────────────────────────

rule!(R046, "PEPPOL-EN16931-R046",
terms: [bt::ITEM_NET_PRICE, bt::ITEM_GROSS_PRICE, bt::ITEM_PRICE_DISCOUNT],
"Item net price MUST equal (Gross price - Allowance amount) when gross price is provided.",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        let Some(gross) = line.price.gross_price else { continue };
        let discount = line
            .price
            .price_discount
            .map_or(Decimal::ZERO, |d| d.into_decimal());
        let path = Path::at_term(Group::Line, i, bt::ITEM_NET_PRICE);
        let expected = match exact_sub(gross.into_decimal(), discount) {
            Ok(expected) => expected,
            Err(reason) => {
                f.at_with_hint(path, reason.hint());
                continue;
            }
        };
        // Exact: no `u:slack`, unlike R040.
        if expected != line.price.net_price.into_decimal() {
            f.arithmetic(path, expected, line.price.net_price);
        }
    }
});

rule!(R121, "PEPPOL-EN16931-R121",
terms: [bt::PRICE_BASE_QUANTITY],
"Base quantity MUST be a positive number above zero.",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        // A divisor in R120.
        if line.price.base_quantity.is_some_and(|q| !q.is_positive()) {
            f.at(Path::at_term(Group::Line, i, bt::PRICE_BASE_QUANTITY));
        }
    }
});

rule!(R130, "PEPPOL-EN16931-R130",
terms: [bt::PRICE_BASE_QUANTITY_CODE, bt::LINE_UNIT_CODE],
"Unit code of price base quantity MUST be same as invoiced quantity.",
|inv, f| {
    for (i, line) in inv.lines.iter().enumerate() {
        // BT-150 against BT-130, only when BT-150 is stated.
        if line
            .price
            .base_quantity_code
            .as_ref()
            .is_some_and(|c| c != &line.unit_code)
        {
            f.arithmetic(
                Path::at_term(Group::Line, i, bt::PRICE_BASE_QUANTITY_CODE),
                &line.unit_code,
                line.price.base_quantity_code.as_ref().unwrap(),
            );
        }
    }
});

// ── Peppol's own presence rules ───────────────────────────────────────────────

rule!(R061, "PEPPOL-EN16931-R061",
terms: [],
"Mandate reference MUST be provided for direct debit.",
|inv, f| {
    // UNTDID 4461: 59 is SEPA direct debit.
    if let Some(p) = &inv.payment
        && p.means_code.as_ref().is_some_and(|c| c.as_str() == "59")
        && p.mandate_reference().is_none_or(str::is_empty)
    {
        f.at(Path::group(Group::Payment));
    }
});

// ── PEPPOL-COMMON-* — identifier checks, by scheme ────────────────────────────

/// Every schemed party identifier `PEPPOL-COMMON-*` reaches, with where it sits.
///
/// Peppol's UBL contexts are `cbc:EndpointID[@schemeID]` (BT-34, BT-49),
/// `cac:PartyIdentification/cbc:ID[@schemeID]` (BT-29, BT-46, BT-60) and
/// `cbc:CompanyID[@schemeID]` (BT-30, BT-47, BT-61). `endpoints_only` is
/// `PEPPOL-COMMON-R046`'s narrower context.
fn party_identifiers(inv: &Invoice, endpoints_only: bool) -> Vec<(&crate::Identifier, Path)> {
    let mut out = Vec::new();
    for (party, group, [ident, legal, endpoint]) in [
        (&inv.seller, Group::Seller, [29, 30, 34]),
        (&inv.buyer, Group::Buyer, [46, 47, 49]),
    ] {
        if let Some(e) = &party.electronic_address {
            out.push((e, Path::group_term(group, BtId(endpoint))));
        }
        if endpoints_only {
            continue;
        }
        for i in &party.identifiers {
            out.push((i, Path::group_term(group, BtId(ident))));
        }
        if let Some(r) = &party.legal_registration {
            out.push((r, Path::group_term(group, BtId(legal))));
        }
    }
    if let Some(payee) = inv.payee.as_ref().filter(|_| !endpoints_only) {
        if let Some(i) = &payee.identifier {
            out.push((i, Path::group_term(Group::Payee, BtId(60))));
        }
        if let Some(r) = &payee.legal_registration {
            out.push((r, Path::group_term(Group::Payee, BtId(61))));
        }
    }
    out
}

/// Report every identifier in `scheme` whose content `ok` refuses.
fn check_scheme(
    inv: &Invoice,
    f: &mut Findings<'_>,
    scheme: &str,
    endpoints_only: bool,
    ok: fn(&str) -> bool,
) {
    for (id, path) in party_identifiers(inv, endpoints_only) {
        if id.scheme() == Some(scheme) && !ok(id.content()) {
            f.at(path);
        }
    }
}

/// XPath's `normalize-space()`: trimmed, inner whitespace runs collapsed.
fn normalize_space(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every byte an ASCII digit, and at least one.
fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// `s castable as xs:integer`: optional sign, then digits, surrounding
/// whitespace allowed.
fn castable_integer(s: &str) -> bool {
    let t = s.trim();
    all_digits(t.strip_prefix(['+', '-']).unwrap_or(t))
}

/// The value of each ASCII digit, in order.
fn digit_values(s: &str) -> impl DoubleEndedIterator<Item = u32> + '_ {
    s.bytes().map(|b| u32::from(b - b'0'))
}

/// Peppol's `u:gln` — the GS1 mod-10 check digit. Caller ensures digits only.
fn gln(v: &str) -> bool {
    let Some((body, check)) = v.len().checked_sub(1).and_then(|n| v.split_at_checked(n)) else {
        return false;
    };
    let sum: u32 = digit_values(body)
        .rev()
        .enumerate()
        .map(|(i, d)| d * if i % 2 == 0 { 3 } else { 1 })
        .sum();
    digit_values(check).next() == Some((10 - sum % 10) % 10)
}

/// Peppol's `u:mod11` — weights 2…7 from the right; a value of zero fails.
fn mod11(v: &str) -> bool {
    let Some((body, check)) = v.len().checked_sub(1).and_then(|n| v.split_at_checked(n)) else {
        return false;
    };
    let sum: u32 = digit_values(body)
        .rev()
        .enumerate()
        .map(|(i, d)| d * (u32::try_from(i % 6).unwrap_or(0) + 2))
        .sum();
    v.bytes().any(|b| b != b'0') && digit_values(check).next() == Some((11 - sum % 11) % 11)
}

/// Peppol's `u:mod97-0208` — the Belgian enterprise number's check pair.
fn mod97_0208(v: &str) -> bool {
    let (Some(body), Some(check)) = (v.get(..8), v.get(8..10)) else {
        return false;
    };
    let (Ok(body), Ok(check)) = (body.parse::<u64>(), check.parse::<u64>()) else {
        return false;
    };
    97 - body % 97 == check
}

/// Peppol's `u:checkCodiceIPA` — six ASCII letters or digits.
fn codice_ipa(v: &str) -> bool {
    v.len() == 6 && v.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Peppol's `u:checkCF` — an Italian *codice fiscale*: 16 characters in the
/// personal shape, or 11 castable as an integer.
fn codice_fiscale(v: &str) -> bool {
    match v.chars().count() {
        16 => {
            let c: Vec<char> = v.chars().collect();
            let letters = |r: std::ops::Range<usize>| c[r].iter().all(char::is_ascii_alphabetic);
            let int =
                |r: std::ops::Range<usize>| castable_integer(&c[r].iter().collect::<String>());
            letters(0..6)
                && int(6..8)
                && letters(8..9)
                && int(9..11)
                && int(14..15)
                && letters(15..16)
        }
        11 => castable_integer(v),
        _ => false,
    }
}

/// Peppol's `u:checkPIVAseIT` — an `IT` *partita IVA* is 11 digits with a Luhn
/// check digit; any other prefix passes.
fn partita_iva(v: &str) -> bool {
    match v.get(..2) {
        Some("IT" | "it") => {
            let code = &v[2..];
            code.len() == 11 && all_digits(code) && {
                let sum: u32 = digit_values(code)
                    .enumerate()
                    .map(|(i, d)| {
                        if i % 2 == 1 {
                            [0, 2, 4, 6, 8, 1, 3, 5, 7, 9][d as usize]
                        } else {
                            d
                        }
                    })
                    .sum();
                sum.is_multiple_of(10)
            }
        }
        _ => true,
    }
}

/// Peppol's `u:checkSEOrgnr` — ten digits, the last a Luhn check over the nine
/// before it.
fn se_orgnr(v: &str) -> bool {
    if v.len() != 10 || !all_digits(v) {
        return false;
    }
    let sum: u32 = digit_values(&v[..9])
        .rev()
        .enumerate()
        .map(|(i, d)| {
            if i % 2 == 0 {
                (d * 2) % 10 + (d * 2) / 10
            } else {
                d
            }
        })
        .sum();
    digit_values(&v[9..]).next() == Some((10 - sum % 10) % 10)
}

/// Peppol's `u:abn` — the Australian Business Number's weighted mod-89 sum.
fn abn(v: &str) -> bool {
    const WEIGHTS: [i64; 11] = [10, 1, 3, 5, 7, 9, 11, 13, 15, 17, 19];
    v.len() == 11
        && all_digits(v)
        && digit_values(v)
            .zip(WEIGHTS)
            .enumerate()
            // The first digit is reduced by one before weighting, so a leading
            // zero contributes -10 — and XPath's `mod` keeps the dividend's sign.
            .map(|(i, (d, w))| (i64::from(d) - i64::from(i == 0)) * w)
            .sum::<i64>()
            % 89
            == 0
}

/// `^[0-9]{n}$` over `normalize-space()`.
fn n_digits(v: &str, n: usize) -> bool {
    let v = normalize_space(v);
    v.len() == n && all_digits(&v)
}

/// `DK` followed by eight digits, over `string()`.
fn dk_and_eight(v: &str) -> bool {
    v.len() == 10 && v.starts_with("DK") && all_digits(&v[2..])
}

/// `^NL[0-9]{9}B[0-9]{2}$` over `normalize-space()`.
fn dutch_vat(v: &str) -> bool {
    let v = normalize_space(v);
    let b = v.as_bytes();
    // On bytes, so a multi-byte character cannot put a slice mid-character.
    b.len() == 14
        && b.starts_with(b"NL")
        && b[2..11].iter().all(u8::is_ascii_digit)
        && b[11] == b'B'
        && b[12..].iter().all(u8::is_ascii_digit)
}

/// A `PEPPOL-COMMON-*` rule: one scheme, one predicate, its published flag.
macro_rules! common_rule {
    ($konst:ident, $id:literal, $severity:ident, $scheme:literal, $endpoints_only:literal,
     $text:literal, $ok:expr) => {
        #[doc = $text]
        #[doc = ""]
        #[doc = concat!("Identifiers with scheme `", $scheme, "`.")]
        pub static $konst: Rule = Rule {
            id: RuleId::new($id),
            severity: Severity::$severity,
            text: $text,
            terms: &[
                BtId(29),
                BtId(30),
                BtId(34),
                BtId(46),
                BtId(47),
                BtId(49),
                BtId(60),
                BtId(61),
            ],
            source: Source::ArtefactOnly,
            eval: |inv, f| check_scheme(inv, f, $scheme, $endpoints_only, $ok),
        };
    };
}

common_rule!(
    COMMON_R040,
    "PEPPOL-COMMON-R040",
    Fatal,
    "0088",
    false,
    "GLN must have a valid format according to GS1 rules.",
    |v| {
        let v = normalize_space(v);
        all_digits(&v) && gln(&v)
    }
);
common_rule!(
    COMMON_R041,
    "PEPPOL-COMMON-R041",
    Fatal,
    "0192",
    false,
    "Norwegian organization number MUST be stated in the correct format.",
    |v| n_digits(v, 9) && mod11(&normalize_space(v))
);
common_rule!(
    COMMON_R042,
    "PEPPOL-COMMON-R042",
    Fatal,
    "0184",
    false,
    "Danish organization number (CVR) MUST be stated in the correct format.",
    // UBL's test, `(DK + 8 digits) or (8 characters and 8 digits)`. CII's drops
    // the second alternative; this profile follows the UBL file.
    |v| dk_and_eight(v) || (v.len() == 8 && all_digits(v))
);
common_rule!(
    COMMON_R043,
    "PEPPOL-COMMON-R043",
    Fatal,
    "0208",
    false,
    "Belgian enterprise number MUST be stated in the correct format.",
    |v| n_digits(v, 10) && mod97_0208(&normalize_space(v))
);
common_rule!(
    COMMON_R044,
    "PEPPOL-COMMON-R044",
    Warning,
    "0201",
    false,
    "IPA Code (Codice Univoco Unità Organizzativa) must be stated in the correct format",
    |v| codice_ipa(&normalize_space(v))
);
common_rule!(
    COMMON_R045,
    "PEPPOL-COMMON-R045",
    Warning,
    "0210",
    false,
    "Tax Code (Codice Fiscale) must be stated in the correct format",
    |v| codice_fiscale(&normalize_space(v))
);
common_rule!(
    COMMON_R046,
    "PEPPOL-COMMON-R046",
    Warning,
    "9907",
    true,
    "Tax Code (Codice Fiscale) must be stated in the correct format",
    |v| codice_fiscale(&normalize_space(v))
);
common_rule!(
    COMMON_R047,
    "PEPPOL-COMMON-R047",
    Warning,
    "0211",
    false,
    "Italian VAT Code (Partita Iva) must be stated in the correct format",
    |v| partita_iva(&normalize_space(v))
);
common_rule!(
    COMMON_R049,
    "PEPPOL-COMMON-R049",
    Fatal,
    "0007",
    false,
    "Swedish organization number MUST be stated in the correct format.",
    |v| se_orgnr(&normalize_space(v))
);
common_rule!(
    COMMON_R050,
    "PEPPOL-COMMON-R050",
    Fatal,
    "0151",
    false,
    "Australian Business Number (ABN) MUST be stated in the correct format.",
    // UBL adds `^[0-9]{11}$` before the checksum; CII does not.
    |v| abn(&normalize_space(v))
);
common_rule!(
    COMMON_R052,
    "PEPPOL-COMMON-R052",
    Fatal,
    "0096",
    false,
    "Danish chamber of commerce number (P) MUST be stated in the correct format.",
    |v| v.len() == 10 && all_digits(v)
);
common_rule!(
    COMMON_R053,
    "PEPPOL-COMMON-R053",
    Fatal,
    "0198",
    false,
    "Danish ERSTORG number (SE) MUST be stated in the correct format.",
    dk_and_eight
);
common_rule!(
    COMMON_R054,
    "PEPPOL-COMMON-R054",
    Warning,
    "0106",
    false,
    "Dutch Chamber of Commerce (KVK) numbers (0106) MUST be stated in the correct format \
     (12345678).",
    |v| n_digits(v, 8)
);
common_rule!(
    COMMON_R055,
    "PEPPOL-COMMON-R055",
    Warning,
    "0190",
    false,
    "Dutch organization identification numbers (0190) MUST be stated in the correct format \
     (12345678901234567890).",
    |v| n_digits(v, 20)
);
common_rule!(
    COMMON_R056_1,
    "PEPPOL-COMMON-R056-1",
    Warning,
    "9944",
    false,
    "Dutch VAT numbers (9944) MUST be stated in the correct format (NL123456789B12).",
    dutch_vat
);
common_rule!(
    COMMON_R057,
    "PEPPOL-COMMON-R057",
    Warning,
    "0217",
    false,
    "Dutch Chamber of Commerce Establishment numbers (0217) MUST be stated in the correct \
     format (123456789012).",
    |v| n_digits(v, 12)
);

/// `PEPPOL-COMMON-R056-2` — a Dutch **VAT identifier** (BT-31, BT-48, BT-63), not
/// a schemed identifier: `PartyTaxScheme[TaxScheme = 'VAT']/CompanyID` starting
/// `NL`.
pub static COMMON_R056_2: Rule = Rule {
    id: RuleId::new("PEPPOL-COMMON-R056-2"),
    severity: Severity::Warning,
    text: "Dutch VAT numbers MUST have the format (NL123456789B12).",
    terms: &[BtId(31), BtId(48), BtId(63)],
    source: Source::ArtefactOnly,
    eval: |inv, f| {
        let tax_rep = inv
            .tax_representative
            .as_ref()
            .and_then(|t| t.vat_identifier.as_deref());
        for (vat, path) in [
            (
                inv.seller.vat_identifier.as_deref(),
                Path::group_term(Group::Seller, BtId(31)),
            ),
            (
                inv.buyer.vat_identifier.as_deref(),
                Path::group_term(Group::Buyer, BtId(48)),
            ),
            (
                tax_rep,
                Path::group_term(Group::TaxRepresentative, BtId(63)),
            ),
        ] {
            if let Some(v) = vat
                && normalize_space(v).starts_with("NL")
                && !dutch_vat(v)
            {
                f.at(path);
            }
        }
    },
};

/// Every rule this module defines; [`crate::profiles::PEPPOL_BIS_3`]'s
/// `extra_rules`.
pub static ALL: &[&Rule] = &[
    &R040,
    &R041,
    &R042,
    &R046,
    &R061,
    &R120,
    &R121,
    &R130, //
    &R001,
    &R002,
    &R003,
    &R004,
    &R005,
    &R007,
    &R010,
    &R020,
    &R055,
    &R110,
    &R111, //
    &P0104,
    &P0105,
    &P0106,
    &P0107,
    &P0108,
    &P0109,
    &P0110,
    &P0111,
    &P0112, //
    &CL001,
    &CL002,
    &CL003,
    &CL006,
    &CL008,
    &P0100,
    &P0101, //
    &F001,
    &R008,
    &R043,
    &R044,
    &R051,
    &CL007,
    &R053,
    &R054,
    &R080,
    &R100,
    &R101, //
    &COMMON_R040,
    &COMMON_R041,
    &COMMON_R042,
    &COMMON_R043,
    &COMMON_R044,
    &COMMON_R045,
    &COMMON_R046,
    &COMMON_R047,
    &COMMON_R049,
    &COMMON_R050,
    &COMMON_R052,
    &COMMON_R053,
    &COMMON_R054,
    &COMMON_R055,
    &COMMON_R056_1,
    &COMMON_R056_2,
    &COMMON_R057,
];

/// The Peppol rules the model retires — registered, never fired.
pub static BY_TYPE: &[&Rule] = &[
    &F001, &R008, &R043, &R044, &R051, &CL007, &R053, &R054, &R080, &R100, &R101,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `rule` alone on `inv` and returns its findings' paths.
    fn fired(rule: &'static Rule, inv: &Invoice) -> Vec<String> {
        let mut out = Vec::new();
        let mut sink = crate::validation::Findings::for_test(&mut out, rule);
        (rule.eval)(inv, &mut sink);
        out.iter().map(|f| f.path.to_string()).collect()
    }

    /// Each segment of a codice fiscale, partita IVA and Swedish number fails
    /// on its own.
    #[test]
    fn each_identifier_segment_is_checked_on_its_own() {
        for bad in [
            "1SSMRA85T10A562S", // letters 0..6
            "RSSMRAX5T10A562S", // digits 6..8
            "RSSMRA85110A562S", // letter 8
            "RSSMRA85TX0A562S", // digits 9..11
            "RSSMRA85T10A56XS", // digit 14
            "RSSMRA85T10A5621", // letter 15
        ] {
            assert!(!codice_fiscale(bad), "{bad}");
        }
        assert!(partita_iva("IT37783533740"));
        assert!(se_orgnr("8889964378"));
        assert!(!se_orgnr("556036079A"));
    }

    /// `R002` and `P0112` exempt a document only when **both** parties are
    /// German.
    #[test]
    fn the_german_exemption_needs_both_parties() {
        let mut inv = Invoice {
            notes: vec!["one".into(), "two".into()],
            type_code: Some(crate::invoice::Code::new("326")),
            ..Default::default()
        };
        inv.seller.address.country = Some(crate::invoice::Code::new("DE"));
        inv.buyer.address.country = Some(crate::invoice::Code::new("DE"));
        assert!(fired(&R002, &inv).is_empty());
        assert!(fired(&P0112, &inv).is_empty());
        inv.buyer.address.country = Some(crate::invoice::Code::new("FR"));
        assert_eq!(fired(&R002, &inv).len(), 1);
        assert_eq!(fired(&P0112, &inv).len(), 1);
    }

    /// 3.0.21-hotfix (RFC-607): 389 is German-only like 326, and 384 is also
    /// allowed between two Slovak parties — by Peppol's `$supplierCountry`,
    /// the VAT identifier's prefix before the address.
    #[test]
    fn p0112_follows_the_3_0_21_hotfix() {
        let with = |code: &str, seller: &str, buyer: &str| {
            let mut inv = Invoice {
                type_code: Some(crate::invoice::Code::new(code)),
                ..Default::default()
            };
            inv.seller.address.country = Some(crate::invoice::Code::new(seller));
            inv.buyer.address.country = Some(crate::invoice::Code::new(buyer));
            fired(&P0112, &inv).len()
        };
        assert_eq!(with("389", "DE", "DE"), 0);
        assert_eq!(with("389", "DE", "FR"), 1);
        assert_eq!(with("384", "SK", "SK"), 0);
        assert_eq!(with("384", "SK", "DE"), 1);
        assert_eq!(with("326", "SK", "SK"), 1);
        // The VAT identifier's prefix outranks the address.
        let mut inv = Invoice {
            type_code: Some(crate::invoice::Code::new("384")),
            ..Default::default()
        };
        inv.seller.vat_identifier = Some("SK2020000000".into());
        inv.buyer.vat_identifier = Some("sk2020000001".into());
        inv.seller.address.country = Some(crate::invoice::Code::new("AT"));
        assert!(fired(&P0112, &inv).is_empty());
    }

    /// `P0100` constrains BT-3 only on a Peppol billing process.
    #[test]
    fn the_type_code_list_applies_to_billing_processes_only() {
        let mut inv = Invoice {
            type_code: Some(crate::invoice::Code::new("999")),
            business_process: Some("urn:example:another-process".to_owned()),
            ..Default::default()
        };
        assert!(fired(&P0100, &inv).is_empty());
        inv.business_process = Some("urn:fdc:peppol.eu:2017:poacc:billing:01:1.0".to_owned());
        assert_eq!(fired(&P0100, &inv).len(), 1);
    }

    #[test]
    fn slack_is_exactly_two_hundredths() {
        assert_eq!(SLACK, Decimal::from_str_exact("0.02").unwrap());
        // Inclusive, matching `u:slack`'s `>=` / `<=`.
        let d = |s: &str| Decimal::from_str_exact(s).unwrap();
        assert!(within(d("10.00"), d("10.02"), SLACK));
        assert!(within(d("10.00"), d("9.98"), SLACK));
        assert!(!within(d("10.00"), d("10.03"), SLACK));
    }

    /// One line, no line allowances or charges, from the shipped Peppol example.
    fn one_line(quantity: Decimal, price: Decimal, net: &str) -> Invoice {
        let mut inv = crate::examples::for_profile(&crate::profiles::PEPPOL_BIS_3).unwrap();
        inv.lines.truncate(1);
        let line = &mut inv.lines[0];
        line.allowances.clear();
        line.charges.clear();
        line.price.base_quantity = None;
        line.quantity = crate::Quantity::new(quantity);
        line.price.net_price = crate::UnitPriceAmount::new(price);
        line.net_amount = InvoiceAmount::parse(net).unwrap();
        inv
    }

    /// A line amount beyond `Decimal`'s range matches no BT-131, so `R120`
    /// fires rather than skipping the line or panicking.
    #[test]
    fn r120_fires_when_the_expected_line_amount_overflows() {
        let product_overflows = one_line(Decimal::MAX, Decimal::TWO, "10.00");
        assert_eq!(fired(&R120, &product_overflows).len(), 1);
        let difference_overflows = one_line(Decimal::MAX, Decimal::ONE, "-1.00");
        assert_eq!(fired(&R120, &difference_overflows).len(), 1);
    }

    /// Quantity × price may overflow while quantity × (price / base quantity)
    /// fits; that line is still checked, and here it is right.
    #[test]
    fn r120_divides_first_when_the_product_alone_overflows() {
        let mut inv = one_line(
            Decimal::from_str_exact("100000000000000000000").unwrap(),
            Decimal::from(1_000_000_000),
            "10000000000000000.00",
        );
        inv.lines[0].price.base_quantity =
            Some(crate::Quantity::new(Decimal::from(10_000_000_000_000_i64)));
        assert!(fired(&R120, &inv).is_empty());
    }

    /// Quantity and base quantity both at `Decimal::MAX`: dividing the price
    /// first would round it to zero; dividing the quantity keeps the 2.00.
    #[test]
    fn r120_divides_the_larger_factor_when_the_product_overflows() {
        let mut inv = one_line(Decimal::MAX, Decimal::TWO, "0.00");
        inv.lines[0].price.base_quantity = Some(crate::Quantity::new(Decimal::MAX));
        assert_eq!(fired(&R120, &inv).len(), 1);
        inv.lines[0].net_amount = InvoiceAmount::parse("2.00").unwrap();
        assert!(fired(&R120, &inv).is_empty());
    }

    /// Gross price minus discount beyond `Decimal`'s range matches no net
    /// price: `R046` fires rather than panicking.
    #[test]
    fn r046_fires_when_gross_minus_discount_overflows() {
        let mut inv = one_line(Decimal::ONE, Decimal::ONE, "1.00");
        inv.lines[0].price.gross_price = Some(crate::UnitPriceAmount::new(Decimal::MAX));
        inv.lines[0].price.price_discount =
            Some(crate::UnitPriceAmount::new(Decimal::NEGATIVE_ONE));
        assert_eq!(fired(&R046, &inv).len(), 1);
    }

    /// Quantity × price below `Decimal`'s 28 places is not rounded to zero:
    /// 0.00000000000001 × 0.000000000000004 / 10^-28 is 0.40.
    #[test]
    fn r120_keeps_a_product_below_decimal_precision() {
        let d = |s: &str| Decimal::from_str_exact(s).unwrap();
        let mut inv = one_line(d("0.00000000000001"), d("0.000000000000004"), "0.40");
        inv.lines[0].price.base_quantity =
            Some(crate::Quantity::new(d("0.0000000000000000000000000001")));
        assert!(fired(&R120, &inv).is_empty());
        inv.lines[0].net_amount = InvoiceAmount::parse("0.00").unwrap();
        assert_eq!(fired(&R120, &inv).len(), 1);
    }

    fn line_charge(amount: &str) -> LineAllowanceCharge {
        LineAllowanceCharge {
            amount: InvoiceAmount::parse(amount).unwrap(),
            base_amount: None,
            percentage: None,
            reason: None,
            reason_code: None,
        }
    }

    /// A price per 3 with a line charge: the share 0.333… (28 places) plus
    /// 10.00 needs more digits than a `Decimal` holds. R120 compares within
    /// 0.02, so the rounding at 28 places is accepted, not reported.
    #[test]
    fn r120_accepts_a_base_quantity_with_a_line_charge() {
        let d = |s: &str| Decimal::from_str_exact(s).unwrap();
        let mut inv = one_line(d("1"), d("1.00"), "10.33");
        inv.lines[0].price.base_quantity = Some(crate::Quantity::new(d("3")));
        inv.lines[0].charges.push(line_charge("10.00"));
        assert!(fired(&R120, &inv).is_empty());

        let mut inv = one_line(d("1"), d("1.00"), "-9.67");
        inv.lines[0].price.base_quantity = Some(crate::Quantity::new(d("3")));
        inv.lines[0].allowances.push(line_charge("10.00"));
        assert!(fired(&R120, &inv).is_empty());
    }

    /// 2 000 lines from a fixed pseudo-random sequence: quantities with up to
    /// three places, unit prices with up to six, base quantities up to 1 000
    /// with or without cents, up to two line charges and two allowances. A
    /// BT-131 rounded to the cent passes R120; 0.03 off fires it.
    #[test]
    fn r120_holds_on_random_everyday_lines() {
        let mut state: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = |below: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % below
        };
        let d = |mantissa: u64, scale: u32| Decimal::new(i64::try_from(mantissa).unwrap(), scale);
        for _ in 0..2000 {
            let quantity = d(next(10_000_000) + 1, 3);
            let price = d(next(10_000_000_000) + 1, 6);
            let base = match next(3) {
                0 => None,
                1 => Some(d(next(1000) + 1, 0)),
                _ => Some(d(next(100_000) + 1, 2)),
            };
            let charges: Vec<Decimal> = (0..next(3)).map(|_| d(next(100_000), 2)).collect();
            let allowances: Vec<Decimal> = (0..next(3)).map(|_| d(next(100_000), 2)).collect();
            let expected = quantity * price / base.unwrap_or(Decimal::ONE)
                + charges.iter().sum::<Decimal>()
                - allowances.iter().sum::<Decimal>();
            let stated = expected
                .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
            let off = Decimal::from_str_exact("0.03").unwrap();
            for (net, fires) in [(stated, 0), (stated + off, 1)] {
                let mut inv = one_line(quantity, price, &net.to_string());
                inv.lines[0].price.base_quantity = base.map(crate::Quantity::new);
                inv.lines[0].charges = charges
                    .iter()
                    .map(|c| line_charge(&c.to_string()))
                    .collect();
                inv.lines[0].allowances = allowances
                    .iter()
                    .map(|a| line_charge(&a.to_string()))
                    .collect();
                assert_eq!(
                    fired(&R120, &inv).len(),
                    fires,
                    "{quantity} × {price} / {base:?} + {charges:?} − {allowances:?} = {net}"
                );
            }
        }
    }

    /// Quantity × price with 31 places is rounded at 28, not reported.
    #[test]
    fn r120_accepts_a_product_with_more_than_28_places() {
        let d = |s: &str| Decimal::from_str_exact(s).unwrap();
        let inv = one_line(d("1.123456789012345"), d("1.1234567890123456"), "1.26");
        assert!(fired(&R120, &inv).is_empty());
    }

    /// Everyday lines: base quantities, prices with up to four places,
    /// quantities with up to three, line charges and allowances. A BT-131
    /// rounded to the cent passes R120; 0.03 off fires it.
    #[test]
    fn r120_accepts_every_everyday_line_and_fires_beyond_the_slack() {
        let d = |s: &str| Decimal::from_str_exact(s).unwrap();
        let bases = [
            None,
            Some("1"),
            Some("2"),
            Some("3"),
            Some("6"),
            Some("7"),
            Some("12"),
            Some("100"),
        ];
        let prices = [
            "0.01", "1.00", "1.23", "9.99", "19.95", "1.2345", "0.3333", "1234.56",
        ];
        let quantities = ["1", "2", "3", "0.5", "1.5", "7", "12.345", "0.333"];
        let extras: [(&[&str], &[&str]); 4] = [
            (&[], &[]),
            (&["10.00"], &[]),
            (&[], &["10.00"]),
            (&["8.50", "0.99"], &["1.25"]),
        ];
        let mut checked = 0;
        for base in bases {
            for price in prices {
                for quantity in quantities {
                    for (charges, allowances) in extras {
                        let divisor = base.map_or(Decimal::ONE, d);
                        let expected = d(quantity) * d(price) / divisor
                            + charges.iter().map(|c| d(c)).sum::<Decimal>()
                            - allowances.iter().map(|a| d(a)).sum::<Decimal>();
                        let stated = expected.round_dp_with_strategy(
                            2,
                            rust_decimal::RoundingStrategy::MidpointAwayFromZero,
                        );
                        for (net, fires) in [(stated, 0), (stated + d("0.03"), 1)] {
                            let mut inv = one_line(d(quantity), d(price), &net.to_string());
                            inv.lines[0].price.base_quantity =
                                base.map(|b| crate::Quantity::new(d(b)));
                            inv.lines[0].charges = charges.iter().map(|c| line_charge(c)).collect();
                            inv.lines[0].allowances =
                                allowances.iter().map(|a| line_charge(a)).collect();
                            assert_eq!(
                                fired(&R120, &inv).len(),
                                fires,
                                "{quantity} × {price} / {base:?} + {charges:?} − {allowances:?} = {net}"
                            );
                            checked += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(checked, 8 * 8 * 8 * 4 * 2);
    }

    /// Gross 4 + 5·10^-28 minus its negative is 8 + 10^-27: exact, though
    /// `rust_decimal` drops a trailing zero to hold it.
    #[test]
    fn r046_accepts_a_difference_that_drops_only_zeros() {
        let d = |s: &str| Decimal::from_str_exact(s).unwrap();
        let gross = d("4.0000000000000000000000000005");
        let mut inv = one_line(Decimal::ONE, d("8.000000000000000000000000001"), "8.00");
        inv.lines[0].price.gross_price = Some(crate::UnitPriceAmount::new(gross));
        inv.lines[0].price.price_discount = Some(crate::UnitPriceAmount::new(-gross));
        assert!(fired(&R046, &inv).is_empty());
    }

    /// 10^28 minus a discount of 0.01 needs more places than `Decimal` has;
    /// rounded, it would equal a net price of 10^28.
    #[test]
    fn r046_does_not_round_away_a_discount() {
        let d = |s: &str| Decimal::from_str_exact(s).unwrap();
        let big = d("10000000000000000000000000000");
        let mut inv = one_line(Decimal::ONE, big, "1.00");
        inv.lines[0].price.gross_price = Some(crate::UnitPriceAmount::new(big));
        inv.lines[0].price.price_discount = Some(crate::UnitPriceAmount::new(d("0.01")));
        assert_eq!(fired(&R046, &inv).len(), 1);
    }

    /// 0.01 × 200.00000000000000000000000001 / 100 is just above the slack
    /// of 0.02 from a stated 0.00; divided first it rounds onto the slack.
    #[test]
    fn r040_compares_without_rounding_the_expected_amount() {
        let mut inv = one_line(Decimal::ONE, Decimal::ONE, "1.00");
        inv.allowances = vec![crate::invoice::DocumentAllowanceCharge {
            amount: InvoiceAmount::parse("0.00").unwrap(),
            base_amount: Some(InvoiceAmount::parse("0.01").unwrap()),
            percentage: Some(Percentage::new(
                Decimal::from_str_exact("200.00000000000000000000000001").unwrap(),
            )),
            vat: crate::invoice::LineVat::default(),
            reason: None,
            reason_code: None,
        }];
        assert_eq!(fired(&R040, &inv).len(), 1);
    }

    fn document_allowance(
        amount: &str,
        base: &str,
        percent: &str,
    ) -> crate::invoice::DocumentAllowanceCharge {
        crate::invoice::DocumentAllowanceCharge {
            amount: InvoiceAmount::parse(amount).unwrap(),
            base_amount: Some(InvoiceAmount::parse(base).unwrap()),
            percentage: Some(Percentage::new(Decimal::from_str_exact(percent).unwrap())),
            vat: crate::invoice::LineVat::default(),
            reason: None,
            reason_code: None,
        }
    }

    /// 1.01 × 33.333333333333333333333333333 % needs 29 places; R040
    /// compares within 0.02, so the rounding at 28 places is accepted.
    #[test]
    fn r040_accepts_a_product_with_more_than_28_places() {
        let mut inv = one_line(Decimal::ONE, Decimal::ONE, "1.00");
        inv.allowances = vec![document_allowance(
            "0.34",
            "1.01",
            "33.333333333333333333333333333",
        )];
        assert!(fired(&R040, &inv).is_empty());
    }

    /// Everyday allowances: an amount rounded to the cent passes R040, 0.03
    /// off fires it.
    #[test]
    fn r040_accepts_every_everyday_allowance_and_fires_beyond_the_slack() {
        let d = |s: &str| Decimal::from_str_exact(s).unwrap();
        let mut checked = 0;
        for base in ["0.01", "1.00", "99.99", "1234.56", "1000000.00"] {
            for percent in [
                "0.5",
                "2",
                "2.75",
                "3",
                "7",
                "12.5",
                "19",
                "33.33",
                "33.333333333333333333333333333",
            ] {
                let stated = (d(base) * d(percent) / Decimal::ONE_HUNDRED).round_dp_with_strategy(
                    2,
                    rust_decimal::RoundingStrategy::MidpointAwayFromZero,
                );
                for (amount, fires) in [(stated, 0), (stated + d("0.03"), 1)] {
                    let mut inv = one_line(Decimal::ONE, Decimal::ONE, "1.00");
                    inv.allowances = vec![document_allowance(&amount.to_string(), base, percent)];
                    assert_eq!(
                        fired(&R040, &inv).len(),
                        fires,
                        "{base} × {percent} % = {amount}"
                    );
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 5 * 9 * 2);
    }

    /// A base amount × percentage beyond `Decimal`'s range matches no amount.
    #[test]
    fn r040_fires_when_the_expected_amount_overflows() {
        let mut inv = one_line(Decimal::ONE, Decimal::ONE, "1.00");
        inv.allowances = vec![crate::invoice::DocumentAllowanceCharge {
            amount: InvoiceAmount::parse("10.00").unwrap(),
            base_amount: Some(InvoiceAmount::parse("1000.00").unwrap()),
            percentage: Some(Percentage::new(Decimal::MAX)),
            vat: crate::invoice::LineVat::default(),
            reason: None,
            reason_code: None,
        }];
        assert_eq!(fired(&R040, &inv).len(), 1);
    }

    /// The ported `u:*` helpers accept published real identifiers and reject
    /// the same with the check digit changed.
    #[test]
    fn the_identifier_checks_agree_with_real_identifiers() {
        for (check, good, bad) in [
            (gln as fn(&str) -> bool, "4006381333931", "4006381333932"), // GS1's EAN-13 example
            (mod11, "923609016", "923609017"),                           // a Norwegian org. number
            (mod97_0208, "0403170701", "0403170702"), // a Belgian enterprise number
            (partita_iva, "IT00743110157", "IT00743110158"),
            (se_orgnr, "5560360793", "5560360794"), // a Swedish org. number
            (abn, "51824753556", "51824753557"),    // the ATO's example ABN
            (codice_fiscale, "RSSMRA85T10A562S", "RSSMRA85T10A562"),
            (codice_ipa, "UFY9MH", "UFY9M-"),
            (dutch_vat, "NL123456789B12", "NL123456789C12"),
        ] {
            assert!(check(good), "{good}");
            assert!(!check(bad), "{bad}");
        }
        // `u:checkPIVAseIT` only checks Italian numbers.
        assert!(partita_iva("DE123"));
        // `u:checkCF`'s eleven-character form is "castable as xs:integer".
        assert!(codice_fiscale("12345678901"));
        assert!(!codice_fiscale("1234567890A"));
        // `u:mod11` refuses an all-zero number whose check digit happens to fit.
        assert!(!mod11("000000000"));
        // Empty or multi-byte input is refused, not a panic.
        assert!(!gln("") && !mod11(""));
        assert!(!dutch_vat("NL1234567äB12") && !partita_iva("IT0074311015ä"));
    }

    /// Each `PEPPOL-COMMON-*` rule reaches every schemed party identifier, and
    /// only those with its scheme.
    #[test]
    fn a_common_rule_reads_every_party_identifier_with_its_scheme() {
        use crate::Identifier;
        let bad = Identifier::schemed("124", "0088"); // "123" happens to be a valid GLN
        let mut inv = Invoice::default();
        inv.seller.identifiers = vec![Identifier::schemed("123", "0089"), bad.clone()];
        inv.buyer.legal_registration = Some(bad.clone());
        inv.payee = Some(crate::invoice::Payee {
            identifier: Some(bad),
            ..Default::default()
        });
        let mut out = Vec::new();
        let mut sink = crate::validation::Findings::for_test(&mut out, &COMMON_R040);
        (COMMON_R040.eval)(&inv, &mut sink);
        let at: Vec<String> = out.iter().map(|f| f.path.to_string()).collect();
        assert_eq!(at.len(), 3, "{at:?}");
    }

    #[test]
    fn these_are_peppols_rules_not_cens() {
        for r in ALL {
            assert!(
                r.id.as_str().starts_with("PEPPOL-"),
                "{} is not namespaced",
                r.id
            );
            // `CORE` directly: `explain` resolves profile rules too.
            assert!(
                !super::super::CORE.iter().any(|c| c.id == r.id),
                "{} leaked into CORE",
                r.id
            );
        }
    }
}

// ── The VATEX ⇒ category rules, P0104 … P0111 ─────────────────────────────────

/// Peppol pins eight VATEX codes to the category they may appear under (e.g.
/// `VATEX-EU-IC` only with `K`); core `BR-CL-22` checks list membership only.
const VATEX_CATEGORY: &[(&str, &str, &str)] = &[
    ("PEPPOL-EN16931-P0104", "VATEX-EU-G", "G"),
    ("PEPPOL-EN16931-P0105", "VATEX-EU-O", "O"),
    ("PEPPOL-EN16931-P0106", "VATEX-EU-IC", "K"),
    ("PEPPOL-EN16931-P0107", "VATEX-EU-AE", "AE"),
    ("PEPPOL-EN16931-P0108", "VATEX-EU-D", "E"),
    ("PEPPOL-EN16931-P0109", "VATEX-EU-F", "E"),
    ("PEPPOL-EN16931-P0110", "VATEX-EU-I", "E"),
    ("PEPPOL-EN16931-P0111", "VATEX-EU-J", "E"),
];

macro_rules! vatex_rule {
    ($konst:ident, $idx:expr) => {
        #[doc = "Tax category MUST match the exemption reason code — see the VATEX category table."]
        pub static $konst: Rule = Rule {
            id: RuleId::new(VATEX_CATEGORY[$idx].0),
            severity: Severity::Fatal,
            text: "Tax category MUST be the one the VATEX exemption reason code implies.",
            terms: &[bt::EXEMPTION_REASON_CODE, bt::VAT_CATEGORY],
            source: Source::ArtefactOnly,
            eval: |inv: &Invoice, f: &mut Findings<'_>| {
                let (_, vatex, want) = VATEX_CATEGORY[$idx];
                for (i, g) in inv.vat_breakdown.iter().enumerate() {
                    // Peppol upper-cases the code before comparing; the category
                    // it compares verbatim.
                    let hit = g
                        .exemption_reason_code
                        .as_ref()
                        .is_some_and(|c| c.as_str().eq_ignore_ascii_case(vatex));
                    if hit && g.category.as_str() != want {
                        f.at(Path::at_term(Group::VatBreakdown, i, bt::VAT_CATEGORY));
                    }
                }
            },
        };
    };
}

vatex_rule!(P0104, 0);
vatex_rule!(P0105, 1);
vatex_rule!(P0106, 2);
vatex_rule!(P0107, 3);
vatex_rule!(P0108, 4);
vatex_rule!(P0109, 5);
vatex_rule!(P0110, 6);
vatex_rule!(P0111, 7);

// ── Document-level Peppol rules ───────────────────────────────────────────────

/// Whether both parties are German — the condition `R002` and `P0112` share.
fn both_german(inv: &Invoice) -> bool {
    let de = |p: &crate::invoice::Party| {
        p.address
            .country
            .as_ref()
            .is_some_and(|c| c.as_str() == "DE")
    };
    de(&inv.seller) && de(&inv.buyer)
}

rule!(R001, "PEPPOL-EN16931-R001", terms: [bt::BUSINESS_PROCESS],
"Business process MUST be provided.",
|inv, f| {
    if inv.business_process.as_deref().is_none_or(str::is_empty) {
        f.at(Path::term(bt::BUSINESS_PROCESS));
    }
});

rule!(R003, "PEPPOL-EN16931-R003", terms: [bt::BUYER_REFERENCE, BtId(13)],
"A buyer reference or purchase order reference MUST be provided.",
|inv, f| {
    // BT-10 or BT-13, not BT-10 alone.
    let buyer_ref = inv.buyer_reference.as_deref().is_some_and(|r| !r.trim().is_empty());
    let order_ref = inv
        .purchase_order_reference
        .as_ref()
        .is_some_and(|r| !r.as_str().trim().is_empty());
    if !buyer_ref && !order_ref {
        f.at(Path::term(bt::BUYER_REFERENCE));
    }
});

rule!(R004, "PEPPOL-EN16931-R004", terms: [bt::SPECIFICATION_ID],
"Specification identifier MUST begin with the value \
 'urn:cen.eu:en16931:2017#compliant#urn:fdc:peppol.eu:2017:poacc:billing:3.0' and follow the \
 format rules for the identifier.",
|inv, f| {
    // A prefix (`starts-with`), so a CIUS of Peppol may append a suffix; the
    // suffix may not contain `::`.
    if inv.specification_id.as_deref().is_none_or(|s| {
        let s = s.trim();
        !s.starts_with(crate::profiles::PEPPOL_SPEC_ID) || s.contains("::")
    }) {
        f.at(Path::term(bt::SPECIFICATION_ID));
    }
});

rule!(R002, "PEPPOL-EN16931-R002", terms: [BtId(22)],
"No more than one note is allowed on document level, unless both the buyer and seller are German \
 organizations.",
|inv, f| {
    if inv.notes.len() > 1 && !both_german(inv) {
        f.at(Path::term(BtId(22)));
    }
});

rule!(R005, "PEPPOL-EN16931-R005",
terms: [bt::VAT_ACCOUNTING_CURRENCY, bt::CURRENCY],
"VAT accounting currency code MUST be different from invoice currency code when provided.",
|inv, f| {
    if let (Some(tax), Some(doc)) = (&inv.vat_accounting_currency, &inv.currency)
        && tax == doc
    {
        f.at(Path::term(bt::VAT_ACCOUNTING_CURRENCY));
    }
});

rule!(R007, "PEPPOL-EN16931-R007", terms: [bt::BUSINESS_PROCESS],
"Business process MUST have an approved identifier.",
|inv, f| {
    // An absent BT-23 is `R001`'s finding.
    if let Some(p) = inv.business_process.as_deref()
        && !p.is_empty()
        && peppol_process(p).is_none()
    {
        f.at(Path::term(bt::BUSINESS_PROCESS));
    }
});

/// The Peppol billing process BT-23 names — the Schematron's `$profile`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Process {
    /// `01`, billing.
    Billing,
    /// `02`, billing with response.
    BillingWithResponse,
}

/// BT-23 → [`Process`], as Peppol 3.0.21 derives `$profile`: four listed
/// identifiers, nothing else.
fn peppol_process(business_process: &str) -> Option<Process> {
    match business_process.trim() {
        "urn:fdc:peppol.eu:2017:poacc:billing:01:1.0"
        | "urn:peppol:france:billing:regulated"
        | "urn:peppol:france:billing:non-regulated" => Some(Process::Billing),
        "urn:peppol:bis:billing_with_response" => Some(Process::BillingWithResponse),
        _ => None,
    }
}

rule!(R010, "PEPPOL-EN16931-R010", terms: [bt::BUYER_ELECTRONIC_ADDRESS],
"Buyer electronic address MUST be provided.",
|inv, f| {
    if inv.buyer.electronic_address.is_none() {
        f.at(Path::term(bt::BUYER_ELECTRONIC_ADDRESS));
    }
});

rule!(R020, "PEPPOL-EN16931-R020", terms: [bt::SELLER_ELECTRONIC_ADDRESS],
"Seller electronic address MUST be provided.",
|inv, f| {
    if inv.seller.electronic_address.is_none() {
        f.at(Path::term(bt::SELLER_ELECTRONIC_ADDRESS));
    }
});

rule!(R055, "PEPPOL-EN16931-R055", terms: [bt::VAT_TOTAL, BtId(111)],
"Invoice total VAT amount and Invoice total VAT amount in accounting currency MUST have the same \
 operational sign.",
|inv, f| {
    if let (Some(a), Some(b)) = (inv.totals.vat_total, inv.totals.vat_total_accounting) {
        let sign = |v: crate::InvoiceAmount| v.into_decimal() > Decimal::ZERO;
        if !a.is_zero() && !b.is_zero() && sign(a) != sign(b) {
            f.at(Path::term(BtId(111)));
        }
    }
});

/// Peppol's `$supplierCountry` / `$customerCountry`: the VAT identifier's
/// prefix, then (supplier only) the tax representative's, then the address
/// country, else `XX` — upper-cased and trimmed.
fn peppol_country(vat: &[Option<&str>], address: Option<&crate::invoice::Code>) -> String {
    vat.iter()
        .flatten()
        .map(|v| v.trim())
        .find(|v| v.chars().count() >= 2)
        .map(|v| v.chars().take(2).collect::<String>().to_ascii_uppercase())
        .or_else(|| address.map(|c| c.as_str().trim().to_ascii_uppercase()))
        .unwrap_or_else(|| "XX".to_owned())
}

rule!(P0112, "PEPPOL-EN16931-P0112", terms: [bt::TYPE_CODE],
"Invoice type codes 326 and 389 are only allowed when both buyer and seller are German \
 organizations. Invoice type code 384 is only allowed when both buyer and seller are German \
 organizations or both are Slovak organizations.",
|inv, f| {
    let Some(code) = inv.type_code.as_ref().map(|c| c.as_str().trim()) else {
        return;
    };
    if !matches!(code, "326" | "384" | "389") || both_german(inv) {
        return;
    }
    let supplier = peppol_country(
        &[
            inv.seller.vat_identifier.as_deref(),
            inv.tax_representative.as_ref().and_then(|t| t.vat_identifier.as_deref()),
        ],
        inv.seller.address.country.as_ref(),
    );
    let customer = peppol_country(
        &[inv.buyer.vat_identifier.as_deref()],
        inv.buyer.address.country.as_ref(),
    );
    if !(code == "384" && supplier == "SK" && customer == "SK") {
        f.at(Path::term(bt::TYPE_CODE));
    }
});

// ── Line-level Peppol rules ───────────────────────────────────────────────────

rule!(R110, "PEPPOL-EN16931-R110", terms: [BtId(134), BtId(73)],
"Start date of line period MUST be within invoice period.",
|inv, f| {
    let Some(doc_start) = inv.invoicing_period.as_ref().and_then(|p| p.start) else {
        return;
    };
    for (i, line) in inv.lines.iter().enumerate() {
        if line
            .period
            .as_ref()
            .and_then(|p| p.start)
            .is_some_and(|s| s < doc_start)
        {
            f.at(Path::at_term(Group::Line, i, BtId(134)));
        }
    }
});

rule!(R111, "PEPPOL-EN16931-R111", terms: [BtId(135), BtId(74)],
"End date of line period MUST be within invoice period.",
|inv, f| {
    let Some(doc_end) = inv.invoicing_period.as_ref().and_then(|p| p.end) else {
        return;
    };
    for (i, line) in inv.lines.iter().enumerate() {
        if line
            .period
            .as_ref()
            .and_then(|p| p.end)
            .is_some_and(|e| e > doc_end)
        {
            f.at(Path::at_term(Group::Line, i, BtId(135)));
        }
    }
});

// ── Peppol's own code lists ───────────────────────────────────────────────────

rule!(CL001, "PEPPOL-EN16931-CL001", terms: [BtId(125)],
"Mime code must be according to subset of IANA code list.",
|inv, f| {
    for (i, doc) in inv.attachments.iter().enumerate() {
        if let Some(a) = &doc.attachment
            && !crate::codes::contains(lists::PEPPOL_MIME_CODES, a.mime_code())
        {
            f.at(Path::at_term(Group::Attachment, i, BtId(125)));
        }
    }
});

rule!(CL008, "PEPPOL-EN16931-CL008", terms: [bt::SELLER_ELECTRONIC_ADDRESS, bt::BUYER_ELECTRONIC_ADDRESS],
"Electronic address identifier scheme must be from the codelist \"Electronic Address Identifier \
 Scheme\".",
|inv, f| {
    // Narrower than `BR-CL-25`.
    for (group, party) in [(Group::Seller, &inv.seller), (Group::Buyer, &inv.buyer)] {
        if party
            .electronic_address
            .as_ref()
            .and_then(crate::Identifier::scheme)
            .is_some_and(|s| !crate::codes::contains(lists::PEPPOL_EAS_SCHEMES, s))
        {
            f.at(Path::group(group));
        }
    }
});

// ── Peppol rules the model retires ────────────────────────────────────────────

/// A Peppol rule the model makes unrepresentable — same idea as
/// [`crate::validation::rules::structural`]'s `by_type!`, with Peppol's ids.
macro_rules! by_type {
    ($konst:ident, $id:literal, $text:literal, $why:literal) => {
        #[doc = $text]
        #[doc = ""]
        #[doc = $why]
        pub static $konst: Rule = Rule {
            id: RuleId::new($id),
            severity: Severity::Fatal,
            text: $text,
            terms: &[],
            source: Source::ArtefactOnly,
            eval: |_, _| {},
        };
    };
}

by_type!(
    F001,
    "PEPPOL-EN16931-F001",
    "A date MUST be formatted YYYY-MM-DD.",
    "`Date` is `{ year, month, day }` and parses only `YYYY-MM-DD`; a timestamp is rejected at \
     the boundary, not reported as a finding."
);
by_type!(
    R008,
    "PEPPOL-EN16931-R008",
    "Document MUST not contain empty elements.",
    "An element with no content is a syntax artefact. `Option::None` and an empty element are \
     the same value here."
);
by_type!(
    R043,
    "PEPPOL-EN16931-R043",
    "Allowance/charge ChargeIndicator value MUST equal 'true' or 'false'.",
    "Allowances and charges are separate `Vec`s, not one list discriminated by a string."
);
by_type!(
    R044,
    "PEPPOL-EN16931-R044",
    "Charge on price level is NOT allowed. Only value 'false' allowed.",
    "BG-29 carries `price_discount` (BT-147) and nothing else — a price-level *charge* has no \
     field to live in."
);
by_type!(
    R051,
    "PEPPOL-EN16931-R051",
    "All currencyID attributes must have the same value as the invoice currency code (BT-5), \
     except for the tax amount in accounting currency.",
    "Every amount is implicitly in BT-5; only BT-111 is in BT-6, and it has its own field. Same \
     disposition as `BR-CL-03`."
);
by_type!(
    CL007,
    "PEPPOL-EN16931-CL007",
    "Currency code must be according to ISO 4217:2005.",
    "The per-amount `@currencyID` does not exist in the model. BT-5 and BT-6 are checked by \
     `BR-CL-04` and `BR-CL-05`."
);
by_type!(
    R053,
    "PEPPOL-EN16931-R053",
    "Only one tax total with tax subtotals MUST be provided.",
    "`vat_breakdown` is one `Vec` of BG-23 groups; there is no second TaxTotal to provide."
);
by_type!(
    R054,
    "PEPPOL-EN16931-R054",
    "Only one tax total without tax subtotals MUST be provided when tax currency code is provided.",
    "BT-111 is one `Option` field, present or not — the UBL shape it constrains has no analogue."
);
by_type!(
    R080,
    "PEPPOL-EN16931-R080",
    "Only one project reference is allowed on document level.",
    "BT-11 is `Option<DocumentReference>`, so a second one cannot be expressed."
);
by_type!(
    R100,
    "PEPPOL-EN16931-R100",
    "Only one invoiced object is allowed pr line.",
    "BT-128 is `Option<Identifier>` on the line."
);
by_type!(
    R101,
    "PEPPOL-EN16931-R101",
    "Element Document reference can only be used for Invoice line object.",
    "The line's only document reference *is* BT-128; there is no untyped reference to misuse."
);

// ── The document type codes, per business process ─────────────────────────────

/// `P0100` — BT-3 on an invoice, under billing process `01` or `02`. Excludes
/// `389` (self-billing is a separate Peppol profile).
const P0100_CODES: &[&str] = &[
    "102", "218", "219", "326", "331", "380", "382", "383", "384", "386", "388", "393", "395",
    "553", "575", "623", "71", "780", "80", "817", "82", "84", "870", "875", "876", "877",
];

/// `P0101` — BT-3 on a credit note, under billing process `01` or `02`.
const P0101_CODES: &[&str] = &["381", "396", "532", "81", "83"];

rule!(P0100, "PEPPOL-EN16931-P0100", terms: [bt::TYPE_CODE],
"Invoice type code MUST be set according to the profile.",
|inv, f| {
    if is_billing_process(inv)
        && let Some(code) = inv.type_code.as_ref()
        // Invoice or credit-note list, by the code itself.
        && !is_credit_note(code)
        && !crate::codes::contains(P0100_CODES, code.as_str())
    {
        f.at(Path::term(bt::TYPE_CODE));
    }
});

rule!(P0101, "PEPPOL-EN16931-P0101", terms: [bt::TYPE_CODE],
"Credit note type code MUST be set according to the profile.",
|inv, f| {
    if is_billing_process(inv)
        && let Some(code) = inv.type_code.as_ref()
        && is_credit_note(code)
        && !crate::codes::contains(P0101_CODES, code.as_str())
    {
        f.at(Path::term(bt::TYPE_CODE));
    }
});

/// Whether BT-23 names a Peppol billing process — the condition of `P0100`
/// and `P0101` (`$profile = ('01','02')`).
fn is_billing_process(inv: &Invoice) -> bool {
    inv.business_process
        .as_deref()
        .and_then(peppol_process)
        .is_some()
}

/// Whether BT-3 is a credit-note code, per `BR-CL-01`'s split (`81`, in both
/// CEN lists, counts as a credit note, as `P0101` expects).
fn is_credit_note(code: &crate::invoice::Code) -> bool {
    code.is_in(lists::CREDIT_NOTE_TYPE_CODES)
}

// ── The three rules that mirror a CEN rule exactly ────────────────────────────

/// Peppol code-list rules whose list the generator asserts is identical to a
/// CEN table, registered under their own Peppol ids.
macro_rules! mirror_rule {
    ($konst:ident, $id:literal, $mirrors:literal, $list:ident, $text:literal,
     |$inv:ident, $f:ident| $body:block) => {
        #[doc = $text]
        #[doc = ""]
        #[doc = concat!("Checks the same list as `", $mirrors, "`; `cargo xtask codegen` fails")]
        #[doc = "if the two ever diverge, so this doc cannot rot."]
        pub static $konst: Rule = Rule {
            id: RuleId::new($id),
            severity: Severity::Fatal,
            text: $text,
            terms: &[],
            source: Source::ArtefactOnly,
            eval: |$inv: &Invoice, $f: &mut Findings<'_>| $body,
        };
    };
}

mirror_rule!(
    CL002,
    "PEPPOL-EN16931-CL002",
    "BR-CL-19",
    ALLOWANCE_REASON_CODES,
    "Reason code MUST be according to subset of UNCL 5189 D.16B.",
    |inv, f| {
        for (i, a) in inv.allowances.iter().enumerate() {
            if a.reason_code
                .as_ref()
                .is_some_and(|c| !c.is_blank() && !c.is_in(lists::ALLOWANCE_REASON_CODES))
            {
                f.at(Path::at_term(Group::DocumentAllowance, i, BtId(98)));
            }
        }
        for (i, line) in inv.lines.iter().enumerate() {
            for a in &line.allowances {
                if a.reason_code
                    .as_ref()
                    .is_some_and(|c| !c.is_blank() && !c.is_in(lists::ALLOWANCE_REASON_CODES))
                {
                    f.at(Path::at_term(Group::Line, i, BtId(140)));
                }
            }
        }
    }
);

mirror_rule!(
    CL003,
    "PEPPOL-EN16931-CL003",
    "BR-CL-20",
    CHARGE_REASON_CODES,
    "Reason code MUST be according to UNCL 7161 D.16B.",
    |inv, f| {
        for (i, c) in inv.charges.iter().enumerate() {
            if c.reason_code
                .as_ref()
                .is_some_and(|c| !c.is_blank() && !c.is_in(lists::CHARGE_REASON_CODES))
            {
                f.at(Path::at_term(Group::DocumentCharge, i, BtId(105)));
            }
        }
        for (i, line) in inv.lines.iter().enumerate() {
            for c in &line.charges {
                if c.reason_code
                    .as_ref()
                    .is_some_and(|c| !c.is_blank() && !c.is_in(lists::CHARGE_REASON_CODES))
                {
                    f.at(Path::at_term(Group::Line, i, BtId(145)));
                }
            }
        }
    }
);

mirror_rule!(
    CL006,
    "PEPPOL-EN16931-CL006",
    "BR-CL-06",
    VAT_POINT_DATE_CODES,
    "Invoice period description code must be according to UNCL 2005 D.16B.",
    |inv, f| {
        if inv
            .vat_point_date_code
            .as_ref()
            .is_some_and(|c| !c.is_blank() && !c.is_in(lists::VAT_POINT_DATE_CODES))
        {
            f.at(Path::term(bt::VAT_POINT_DATE_CODE));
        }
    }
);

// ── What XRechnung actually merges ────────────────────────────────────────────

/// The 21 Peppol rules XRechnung's released Schematron merges in.
///
/// KoSIT's build runs `peppol-into-xr.xsl` over Peppol's Schematron, copying
/// every assert listed (not commented out) in `rule-list.xml` and rewriting
/// two (see [`XR_R040`] and [`XR_R120`]). Left out are, among others,
/// `CL001`…`CL008`, `P0100`/`P0101`, `P0104`…`P0112` and `R002`/`R003`/`R004`
/// (`R004` would reject every XRechnung BT-24).
pub const MERGED_INTO_XRECHNUNG: &[&str] = &[
    "PEPPOL-EN16931-R001",
    "PEPPOL-EN16931-R005",
    "PEPPOL-EN16931-R008",
    "PEPPOL-EN16931-R010",
    "PEPPOL-EN16931-R020",
    "PEPPOL-EN16931-R040",
    "PEPPOL-EN16931-R041",
    "PEPPOL-EN16931-R042",
    "PEPPOL-EN16931-R043",
    "PEPPOL-EN16931-R044",
    "PEPPOL-EN16931-R046",
    "PEPPOL-EN16931-R053",
    "PEPPOL-EN16931-R054",
    "PEPPOL-EN16931-R055",
    "PEPPOL-EN16931-R061",
    "PEPPOL-EN16931-R101",
    "PEPPOL-EN16931-R110",
    "PEPPOL-EN16931-R111",
    "PEPPOL-EN16931-R120",
    "PEPPOL-EN16931-R121",
    "PEPPOL-EN16931-R130",
];

/// `R040` as XRechnung merges it: the same rule, with the HUF slack.
pub static XR_R040: Rule = Rule {
    id: RuleId::new("PEPPOL-EN16931-R040"),
    severity: Severity::Fatal,
    text: R040.text,
    terms: R040.terms,
    source: Source::ArtefactOnly,
    eval: |inv, f| check_allowance_basis(inv, f, currency_slack),
};

/// `R120` as XRechnung merges it: a warning (`peppol-into-xr.xsl` rewrites
/// the flag), with the HUF slack.
pub static XR_R120: Rule = Rule {
    id: RuleId::new("PEPPOL-EN16931-R120"),
    severity: Severity::Warning,
    text: R120.text,
    terms: R120.terms,
    source: Source::ArtefactOnly,
    eval: |inv, f| check_line_amount(inv, f, currency_slack),
};

/// The 21 merged rules, as XRechnung runs them: [`XR_R040`], [`XR_R120`] and
/// 19 of Peppol's own. Must agree with [`MERGED_INTO_XRECHNUNG`].
pub static FOR_XRECHNUNG: &[&Rule] = &[
    &R001, &R005, &R008, &R010, &R020, &XR_R040, &R041, &R042, &R043, &R044, //
    &R046, &R053, &R054, &R055, &R061, &R101, &R110, &R111, &XR_R120, &R121, &R130,
];

#[cfg(test)]
mod merge_tests {
    use super::*;

    /// [`FOR_XRECHNUNG`] and [`MERGED_INTO_XRECHNUNG`] name the same rules.
    #[test]
    fn the_merged_set_matches_the_rule_list() {
        let mut got: Vec<&str> = FOR_XRECHNUNG.iter().map(|r| r.id.as_str()).collect();
        got.sort_unstable();
        let mut want: Vec<&str> = MERGED_INTO_XRECHNUNG.to_vec();
        want.sort_unstable();
        assert_eq!(got, want);
    }

    /// The two rewritten instances differ from Peppol's in the ways the
    /// stylesheet says, and in no others.
    #[test]
    fn xrechnung_rewrites_exactly_two_rules() {
        assert_eq!(R120.severity, Severity::Fatal);
        assert_eq!(XR_R120.severity, Severity::Warning, "peppol-into-xr.xsl");
        assert_eq!(XR_R120.text, R120.text, "only the flag changes");
        assert_eq!(XR_R040.severity, R040.severity, "R040 keeps its flag");

        let huf = Invoice {
            currency: Some(crate::invoice::Code::new("HUF")),
            ..Default::default()
        };
        assert_eq!(currency_slack(&huf), HUF_SLACK);
        assert_eq!(fixed_slack(&huf), SLACK, "Peppol does not widen for HUF");
        assert_eq!(currency_slack(&Invoice::default()), SLACK);
    }
}
