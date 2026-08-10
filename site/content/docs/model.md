+++
title = "The semantic model"
weight = 2
description = "EN 16931's business terms and groups as Rust types: fixed-point amounts, calendar dates, per-cent percentages, signed quantities, and eighteen generated code lists."
+++

EN 16931-1 defines an invoice as **164 business terms** (BT-1 … BT-165) arranged
into **business groups** (BG-1 … BG-25). That is the whole standard: the two XML
syntaxes are bindings of it, and the business rules are constraints on it.

This crate makes the model the primary artefact. `Invoice` is a plain Rust
struct whose fields are business terms, and everything else — validation,
reconciliation, both syntaxes, the PDF container — is built on top of it.

## The design invariants

These five decisions explain most of the API, so they are worth stating up front.

**No `f64`.** Amounts are fixed-point; rates and quantities are `Decimal`. A
binary float cannot represent `0.10`, and an invoice is a legal statement about
money.

**Rounding is never implicit.** An amount that does not fit two decimals is an
error, not a rounding opportunity. Whoever produced the third decimal knows what
should happen to it; the type does not.

**Mandatory means non-`Option`.** The type system carries every cardinality it
can. Rules exist for the ones it cannot express, not as a substitute for it.

**Types enforce representability; rules enforce validity.** An *invalid* invoice
must still be representable — otherwise a parser cannot load one in order to
explain what is wrong with it. This is the line that decides, for any given
constraint, whether it belongs in a type or in a rule.

**No I/O, no async, no `unsafe`.** `#![forbid(unsafe_code)]`, and `wasm32` is
tested in CI.

## The ten semantic data types

§6.5 of the standard defines ten data types, and each is a distinct Rust type
here rather than a `String` or a `Decimal` with a comment.

| §6.5 | Rust | Why it is its own type |
|---|---|---|
| Amount | `InvoiceAmount` | two decimals, hard |
| Unit Price Amount | `UnitPriceAmount` | *not* capped — see below |
| Quantity | `Quantity` | may be negative |
| Percentage | `Percentage` | per cent, not a fraction |
| Date | `Date` | a calendar day, with no time of day |
| Identifier | `Identifier` | value plus optional scheme |
| Document reference | `DocumentReference` | an identifier with no scheme |
| Code | `Code` | a value from a named list |
| Text | `String` | |
| Binary object | `Attachment` | bytes plus MIME type plus filename |

### `InvoiceAmount` — where 21 rules go to die

§6.5.2 does not merely *restrict* amounts to two decimals; it defines the
semantic type that way:

> EN 16931_ Amount. Type is floating up to two fraction digits.

Table 26 lists every term it applies to, and the CEN artefacts render that table
as 21 Schematron assertions — `BR-DEC-01`, `-02`, `-05`, `-06`, `-09`…`-20`,
`-23`…`-25`, `-27`, `-28`. A type that cannot hold a third decimal retires all 21
at compile time.

```rust
use en16931::InvoiceAmount;

let net = InvoiceAmount::parse("1000.00")?;
let vat = InvoiceAmount::parse("190.00")?;
assert_eq!(net.checked_add(vat)?.to_string(), "1190.00");

// Refused, not rounded — neither 0.01 nor 0.00.
assert!(InvoiceAmount::parse("0.005").is_err());
```

Every operation is `checked_`: an invoice total that overflows is a data error
worth surfacing, never a number worth guessing.

**And where it must *not* be used.** `Unit Price Amount` (§6.5.3) is a
*different* semantic type — based on Amount, but with no cap. The standard's own
example is `10000.1234`.

```rust
use en16931::{InvoiceAmount, UnitPriceAmount};
use rust_decimal::dec;

let price = UnitPriceAmount::new(dec!(0.28901));   // EUR/kWh
assert_eq!(price.to_string(), "0.28901");

// The same value as an Amount would be refused outright.
assert!(InvoiceAmount::from_decimal_exact(dec!(0.28901)).is_err());
```

Metering, freight and commodity pricing all live in the fourth and fifth decimal.
An implementation that caps BT-146 at two silently overcharges or undercharges
every kilowatt-hour, and it is one of the most common bugs in this space.

### `Date` — a calendar day, not an instant

§6.5.9 is unusually explicit, and both halves matter:

> Dates shall be in accordance to the "Calendar date complete representation" as
> specified by ISO 8601. **Calendar dates do not include a specification for the
> time of the day.**

```rust
use en16931::Date;

let from = Date::parse("2026-06-01")?;
let to   = Date::parse("2026-06-30")?;
assert!(to >= from);                                   // BR-29

assert!(Date::parse("2026-02-30").is_err());           // not a real day
assert!(Date::parse("2026-06-01T00:00:00").is_err());  // §6.5.9: no time of day
```

Three integers, no timezone. There is nothing to offset, and shifting BT-2 by a
zone changes the VAT period it falls in. Enable the `chrono` or `time` feature
for conversions; the default build carries neither.

### `Percentage` — per cent, never a fraction

> Percentages are given as fractions of a hundred (per cent) e.g. the value
> 34,78 % in percentage terms is given as 34,78. — §6.5.5

```rust
use en16931::Percentage;
use rust_decimal::dec;

let vat = Percentage::new(dec!(19));       // nineteen per cent, NOT 0.19
assert_eq!(vat.to_string(), "19");
assert_eq!(vat.as_fraction(), dec!(0.19)); // what you multiply by
```

This is the most common transcription bug when bridging a calculation engine: a
tax engine stores `0.19` because that is what you multiply by, and the standard
stores what you print. Convert once, at the boundary.

Trailing zeros need no special handling — `rust_decimal` compares by value, so
`19` and `19.00` are one VAT breakdown group in `Eq`, `Ord` **and `Hash`**. A
`Hash` disagreeing with `Eq` here would be a silent, data-dependent grouping bug,
so a test pins it.

### `Quantity` — and why it may be negative

Annex A.1.6 (*Example 5 — Negative Invoice line*) invoices 25 cases of pens and
credits 10 returned ones **on the same ordinary invoice**:

| BT-126 | BT-129 | BT-146 | BT-131 |
|---|---|---|---|
| 1 | `25` | `8,50` | `212,50` |
| 2 | **`−10`** | `8,50` | **`−85,00`** |

The sign lives on the **quantity**, never on the price — BR-27 forbids a negative
item net price.

## Code lists — 4 887 values, generated

Eighteen code lists are generated from the CEN artefacts and re-verified in CI:
ISO 3166-1 countries, ISO 4217 currencies, UNTDID 1001 document types, UNTDID
5305 VAT categories, UNTDID 4451 note subjects, UN/ECE Rec 20 and Rec 21 unit
codes, the EAS scheme list, ISO 6523 ICD, VATEX exemption reasons, and the rest.
None is written by hand; `cargo xtask check` re-derives all of them and fails if
a committed table differs.

The guarded constructors check membership **at the map**, where you still know
what you were mapping:

`contains` answers yes or no. `guard` answers **what to do**:

```rust
use en16931::codes::guard;

// `9958` — the scheme every German integrator reaches for. Withdrawn 2023-07-31.
let err = guard::eas("9958").unwrap_err();
assert!(err.to_string().contains("use 0204 instead"));

// The single most common unit-code bug.
assert!(guard::unit("kwh").unwrap_err().to_string().contains("did you mean \"KWH\""));
```

Twelve EAS schemes have left the CEF list since CEN artefact `validation-1.2.0`,
nine of them in 2023 alone, and the table names the successor of each. That the
withdrawn codes really are gone from the current list, and that every named
successor really is in it, is asserted against the pinned artefacts — so a code
CEN reinstates fails the build rather than producing a wrong hint.

A code list that only speaks up in the final report tells you at the end of a
pipeline what it could have told you at the start, and by then the context that
would let you fix it is three layers away. This is a convenience and never a
second source of truth: `guard` checks the same generated list the corresponding
rule checks, and skipping the layer loses nothing but the earlier message.

## Formatting never shortens a value

Every `Display` in the crate used `Formatter::pad`, the standard helper — which,
because that is what precision means for a *string*, truncates to N
**characters**:

```text
format!("{:.2}",    amount)   →  "11"           ← 1190.00, as eleven euros
format!("{:>12.4}", amount)   →  "        1190"
format!("{:.4}",    date)     →  "2026"
```

A caller asking for two decimal places got a hundredth of the amount, right
where a person reads it. The crate refuses to round an amount at a boundary
because a plausible wrong number is worse than an error, and this was the same
failure arriving through the formatter.

Nothing truncates now, and **precision on the numeric types is a minimum number
of fraction digits** — `{:.4}` on `1190.00` is `1190.0000`, `{:.0}` is still
`1190.00`. Padding is lossless; rounding is not. Width, fill and alignment are
unchanged, and `en16931::fmt` exposes the two helpers so a downstream `Display`
can make the same promise.

## Editions are values, not crate versions

`XRechnung 3.0` and a future `XRechnung 4.0` are *values* of the same type, not
two versions of this crate. That is what makes *"valid today, and still valid
under the next edition?"* one call instead of two pipelines and a diff.

## What is next

- **[Validation](@/docs/validation.md)** — what the rules do with all of this.
- **[Profiles](@/docs/profiles.md)** — restricting the model for XRechnung or Peppol.
