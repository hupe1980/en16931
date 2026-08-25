+++
title = "Bridging a billing engine"
weight = 8
description = "Turn a calculation engine's output into an EN 16931 invoice without the four traps that make it look like a field-for-field copy: levies, rates, signs and the §14c hole."
+++

Behind `features = ["billing"]`, `en16931` maps a
[`billing`](https://crates.io/crates/billing) document into a semantic invoice.
The division of labour is worth stating even if you use a different engine,
because the same seams appear whatever produced the arithmetic:

- **The engine owns the arithmetic.** What the numbers are.
- **`en16931` owns what the arithmetic means.** Which business term each number is.
- **The caller owns the parties.** A tariff engine has no business knowing a
  buyer's postal address.

```rust
let invoice = FromBilling::new(&billing_document)
    .specification_id(profiles::XRECHNUNG.specification_id)
    .seller(seller)
    .buyer(buyer)
    .build()?;

validate(&invoice).into_result()?;
```

There is deliberately **no `TryFrom`**. A billing document has no seller, no
buyer, no addresses, no country codes and no item names — a line's description is
display text, not BT-153. A `TryFrom` would fail on every realistic input, which
makes it a trait impl whose only behaviour is `Err` while implying that
conversion is total.

## The levy trap

This is the reason the adapter is not a field-for-field copy, and it catches
nearly everyone.

A per-unit excise — Stromsteuer, a CO₂ levy — is produced by a tax layer, so it
lands in the engine's tax total. But EN 16931 counts it **inside the taxable
base**: it is a BG-21 document-level charge, not tax.

Map the tax total to BT-110, the obvious thing to do, and `BR-CO-14`
(`BT-110 = Σ BT-117`) fails on **every levy-bearing invoice**.

```text
BT-106  Σ line net amounts        ← net positions
BT-107  Σ allowances              ← discount positions (stated positive)
BT-108  Σ charges                 ← the levy
BT-109  = BT-106 − BT-107 + BT-108
BT-110  = Σ BT-117                ← VAT only
BT-112  = BT-109 + BT-110         ← equals the engine's gross total
```

Note what is *not* there: a "net total". It would be `BT-106 − BT-107`, and
EN 16931 has no term for it. An engine that exposes one is exposing something the
standard cannot carry, and mapping it anywhere is a mistake.

## The §14c hole

A final invoice deducting advance payments must, in Germany, state *"die auf sie
entfallenden Steuerbeträge"* — the tax contained in each advance (§14 Abs. 5
Satz 2 UStG). Omit it and the issuer owes that tax **a second time** under §14c
Abs. 1.

Core EN 16931 has nowhere to put it: BT-113 is one flat figure. So an adapter
that maps itemised advances to BT-113 and drops the rest produces a document that
validates perfectly and is a tax liability.

`Extensions` carries it — mirroring ZUGFeRD EXTENDED's `BG-X-45`, the only
standardised home — and `EN-EXT-01` **warns** when the target profile cannot
represent it:

```text
EN 16931 validation — 227 rule(s) checked, 1 finding(s), valid
  [EN-EXT-01] BT-113 — This invoice carries extension data that the target
  profile cannot represent. […] In Germany that is a §14c Abs. 1 UStG liability.
```

Not fatal: the invoice *is* lawful. But not silent either.

## Four conversions that are not copies

| | |
|---|---|
| **Rates** | A calculation engine stores `0.19` because that is what you multiply by; EN 16931 stores `19`, what you print. Converted once, at the seam. |
| **Signs** | The engine models a return as a credit with a *non-negative* quantity. EN 16931 puts the sign on **BT-129** and forbids a negative BT-146 (`BR-27`) — Annex A.1.6. A negative unit price is flipped onto the quantity rather than dropped. |
| **Precision** | Refused, never rounded — and the error names the fix rather than the symptom. |
| **The document kind** | Taken from the engine's own credit-note predicate, not from BT-3. `81` is on *both* UNTDID 1001 lists, and the syntax layer picks the UBL document element from the kind — so deriving it from the code would put a credit note inside `<ubl:Invoice>`. |

That last one was a real bug here, found by the upstream crate adding the
predicate: the adapter left the document kind at its default, so a credit note
became an invoice carrying BT-3 = `381`. It fails `BR-CL-01`, wrongly runs
`BR-CO-25`, and would have gone out as `<ubl:Invoice>` with a credit note inside
it. Schema-valid, and wrong.

## What crosses

BT-1, BT-2, BT-3, BT-5, BT-6, BT-9, BT-20, BT-21, BT-22, BT-25, BT-26, BT-29,
BT-46, BT-111, BG-1, BG-3, BG-14, BG-20, BG-21, BG-22, BG-23, BG-25 and
ZUGFeRD's `BG-X-45` — plus the document kind, which is not a business term and
decides the root element.

Three are worth naming:

- **BT-6 and BT-111 cross together.** `BR-53` makes the second mandatory whenever
  the first is present, so mapping only the currency would manufacture a finding
  out of a complete document.
- **BT-29 and BT-46 are merged, not overwritten.** The caller's party carries
  master data; the document carries the party code the billing run was keyed on —
  an MP-ID in the energy market, a GLN in retail. EN 16931 makes both repeatable
  precisely because a party has more than one identity. The scheme is compared
  alongside the value, because the same digits under `0088` and under `0293` are
  two registries saying two different things.
- **BG-3 arrives filled in.** A credit note that does not say what it credits is
  an unexplained payment, and `billing`'s `reverse` populates BT-25 and BT-26
  from the document it reverses. `BR-55` is satisfied by the *type*: BT-25 is
  not an `Option` upstream and its constructor refuses a blank string, so a BG-3
  without a reference is not constructible.

Display text and arbitrary key/value labels do not cross: they have no business
term at all. Neither does a **late-payment penalty**, for a sharper reason —
`BR-DE-18` gives a Skonto a micro-syntax inside BT-20 and gives a penalty none,
so a field with no representation in the syntax would vanish on the way out,
silently. Default interest is generally outside the scope of VAT anyway
(art. 63 of the VAT Directive; CJEU C-222/81 *BAZ Bausystem*), so a penalty
billed later is its own document.

Units are resolved from the quantity's own code first, falling back to a small
resolver table. An unresolvable label is an **error**: guessing produces an
invoice that validates and describes the wrong thing, and unlike a wrong amount
nobody notices.

## The BT-20 newline

The smallest trap at this seam, and a good illustration of why the semantic layer
has to know the CIUS rules rather than trusting a renderer.

Germany's Skonto micro-syntax goes in BT-20:

```text
Zahlbar innerhalb 30 Tagen ohne Abzug.
#SKONTO#TAGE=10#PROZENT=2.00#
```

`BR-DE-18` has **two** halves, and the second hides inside the same assertion as
the first:

```text
every $line in …tokenize(., '(\r?\n)')[starts-with(normalize-space(.), '#')]
  satisfies matches(normalize-space($line), $XR-SKONTO-REGEX)
        and matches(…tokenize(., '#.+#')[last()], '^\s*\n')
```

Everything after the **last** `#…#` must begin with a newline. A rendering that
ends at the `#` makes `tokenize(…)[last()]` the empty string, and every German
invoice carrying a Skonto fails.

This one moved upstream — the micro-syntax has no core EN 16931 form, so a
rendering that omits the terminator is valid nowhere at all, and the renderer is
the right place to fix it. What is left at the seam is an idempotent *guard*,
paired with a test that asserts the **upstream** behaviour rather than the
adapter's output. Asserting the output alone would pass just as well against an
upstream regression plus an adapter quietly papering over it.

## What is next

- **[Validation](@/docs/validation.md)** — what happens to the invoice you built.
- **[Conformance](@/docs/conformance.md)** — how the mapping is held to the artefacts.
