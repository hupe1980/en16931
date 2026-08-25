+++
title = "Validation"
weight = 3
description = "The 317 EN 16931 business rules as Rust: the nine VAT category families, four tolerance regimes, severity from the authority, and reports in JSON or SVRL."
+++

`validate(&invoice)` runs the core EN 16931 rule set and returns a report.
`profiles::XRECHNUNG.validate(&invoice)` runs core plus XRechnung. Neither ever
sees a document, and both point their findings at business terms.

## What is in the registry

**317 rules.** Of those, **53 are retired by the type system** — no state of the
model can make them fire — **4 are undecidable** at the semantic layer, and the
remaining **260 are each exercised by their own failing fixture**.

That last number is the one that matters. A rule nobody has seen fire may be
inverted, unreachable, or checking the wrong field, and a suite of valid
documents would be green either way. So the coverage gate fails if a rule is
uncovered and undeclared, if a declared rule has since become covered, **and if
anything is declared for a reason other than being type-retired**. The excuse
list has no room in it.

```text
conformance corpus
  registered:            317
  retired by the types:   53  (no state can make them fire)
  undecidable:             4  (CEN binds them to true() too)
  checkable:             260
  exercised by a case:   260  (100% of checkable)
  declared uncovered:      0
```

The composition, by family:

| Family | Count | What |
|---|---:|---|
| `BR-*` and the nine VAT families | 156 | cardinality, presence, the category tables |
| `PEPPOL-EN16931-*` | 46 | Peppol BIS Billing 3.0 |
| `BR-DE-*` | 27 | XRechnung |
| `BR-CO-*` | 24 | the totals and derivation chain |
| `BR-CL-*` | 23 | code-list membership |
| `BR-DEC-*` | 21 | decimal places — all 21 retired by `InvoiceAmount` |
| `BR-DEX-*` | 14 | the XRechnung Extension — of KoSIT's 15; `BR-DEX-15` checks a CII element |
| `BR-TMP-*` | 2 | temporary CEN rules |
| `EN-*` | 4 | this crate's own, namespaced so they cannot be mistaken for CEN's |

Out of scope and named rather than quietly dropped: the **1 339 syntax rules**
(`UBL-*`, `CII-*`) belong to [`en16931-formats`](@/docs/syntaxes.md), and
Peppol's ~90 national rules (`DK-R-*`, `SE-R-*`, …) are country registry-format
and check-digit checks.

### This crate's own four

Namespaced `EN-*` because inventing a `BR-` id would be indistinguishable from
CEN's:

- **`EN-CURRENCY-01`** — BT-5 is `XXX`, ISO 4217 for *no currency*. `BR-CL-04`
  accepts it because it is a real code, so an unconfigured document validates
  cleanly as an invoice denominated in nothing.
- **`EN-EXT-01`** — the §14c Abs. 1 UStG hazard: the profile in hand cannot
  represent extension data the invoice carries, so writing it out would lose it.
- **`EN-EXT-02`** — a sub-line group keyed to a BG-25 line that does not exist,
  which every consumer skips and no writer emits.
- **`EN-SEPA-01`** — BT-90 does not look like a SEPA Creditor Identifier (EPC
  AT-02). A warning, and only checked when the optional `sepa` feature is on.

## The nine VAT category families

EN 16931-1 §6.4.3 writes these as **nine parallel tables** with the same ten row
headings. `BR-S-08` and `BR-Z-08` are the same sentence with a different category
and a different answer to *"may this category appear at several rates?"*.

So the logic lives in one checker per row, parameterised by a table, and the rule
entries are emitted per category by a macro — each keeping **its own real id**,
because a report saying `BR-CATEGORY-08` would be useless to look up.

| Row | Taxed (`S`, `L`, `M`) | Zero-tax (`Z`, `E`, `AE`, `K`, `G`, `O`) |
|---|---|---|
| `-01` groups | *at least one* — may repeat per rate | **exactly one** |
| `-05/06/07` rate | `S` > 0; `L`/`M` ≥ 0 | `= 0`, except `O` which must be **absent** |
| `-08` base | grouped by **(category, rate)** | grouped by category alone |
| `-09` tax | `= base × rate`, ±1.00 | `= 0`, exact |
| `-10` reason | **forbidden** | **required** |

Every column differs, which is exactly why the standard writes nine tables rather
than one parameterised rule.

`BR-*-08` is the keystone: the only rule tying invoice lines to the VAT
breakdown, and therefore the only thing that turns a mis-attributed line into a
*reported* error rather than a silently wrong invoice.

## The four tolerance regimes

Impossible to get right from the standard's prose, and where a hand-written
engine most easily diverges from the Schematron everyone else runs:

| Regime | Rules | Tolerance | Whose | Where |
|---|---|---|---|---|
| Totals chain | `BR-CO-10` … `BR-CO-16` | **exact** | CEN | core |
| VAT derivation | `BR-CO-17`, `BR-*-08/09` | **±1.00**, on absolute values | CEN artefacts | core |
| Line & allowance derivation | `R120`, `R040` | **±0.02** | Peppol | profile |
| …the same rules, in HUF | `R120`, `R040` | **±0.5** | XRechnung | profile |

**None of it is in the standard.** §6.4.2 states `BR-CO-17` as a plain equation
with no slack; the ±1.00 is an artefact decision, the ±0.02 a Peppol one, and the
±0.5 XRechnung's — it rewrites Peppol's constant to `if($documentCurrencyCode =
'HUF') then 0.5 else 0.02` when it merges the rules in, because HUF has no minor
unit in practice. Peppol never widens it.

So the same forint invoice can be a valid XRechnung and an invalid Peppol
document. That is why tolerance is a property of the **rule instance a profile
holds**, never a crate-wide constant.

```text
// Exact — one cent out is fatal.
BR-CO-14:  BT-110 = 19.01 against Σ BT-117 = 19.00   → fires

// ±1.00 on absolute values, which is what lets credit notes pass.
BR-CO-17:  BT-117 = 18.50 against 100.00 × 19%       → passes
BR-CO-17:  BT-117 = 17.50                            → fires

// ±0.02, Peppol only.
R120:      BT-131 = 100.02 against 1 × 100.00        → passes
R120:      BT-131 = 100.03                           → fires
R120 under the core profile                          → not a rule at all
```

`R120` has **no CEN counterpart**: EN 16931 never ties BT-131 to quantity × price,
so under core a line whose amount does not follow from its price is perfectly
valid. And `R046` is the trap — it looks like `R040`'s sibling and carries **no
slack at all**.

### …and a fifth thing that is not what it looks like: `round`

The artefacts do not say "round to two decimals". They say it in XPath:

```text
round(abs(TaxableAmount) * (Percent div 100) * 10 * 10) div 100
```

and pick the zero-rate branch of `BR-CO-17` on `round(Percent) = 0`. Both are
**XPath's** `fn:round`, which is *"the one closest to +∞"* — and no
`rust_decimal::RoundingStrategy` reproduces it:

| | `round(0.5)` | `round(2.5)` | `round(-0.5)` |
|---|---|---|---|
| XPath `fn:round` | `1` | `3` | `0` |
| `Decimal::round` — banker's | `0` | `2` | `0` |
| half away from zero | `1` | `3` | `-1` |

Banker's and half-away-from-zero each get one of the two midpoint columns
wrong, so the rules use `floor(x + 0.5)` — the definition rather than an
approximation of it. It is not academic: a VAT rate of exactly **0.5 %**
(Spain's *recargo de equivalencia* on reduced-rate goods) rounds to `1` for the
artefact and to `0` for banker's, which sent `BR-CO-17` down its zero-rate
branch and rejected a correct invoice every deployed validator accepts.

## Severity is the authority's, not ours

A rule's *consequence* is not a property of the rule. It is a property of the
rule **in a profile**, and the authorities publish it separately from their
Schematron — which is why reading only the Schematron gets it wrong.

**Two files publish severity, and they cover different rules.** Reading either
one alone is how a validator comes to reject documents Germany accepts.

**1. The validator configuration** re-levels *CEN's* rules, once per scenario:

```xml
<!-- overwrites CEN severity level "fatal" for codelist values of BT-130 … -->
<customLevel level="warning">BR-CL-23</customLevel>
<!-- overwrites CEN severity level "fatal" to enable use of mime codes per BR-DEX-01 -->
<customLevel level="information">BR-CL-24</customLevel>
```

Nine CEN rules are re-levelled across the three XRechnung scenarios, and the
profiles carry all nine.

**2. The Schematron's own `flag`** carries the severity of *KoSIT's* rules, and
five of the fifty-five are not fatal:

```xml
<assert test="matches(normalize-space(cbc:Telephone), $XR-TELEPHONE-REGEX)"
  flag="warning" id="BR-DE-27">…</assert>
```

| | Why not fatal |
|---|---|
| `BR-DE-26` | *"soll … übermittelt werden"* — a corrected invoice **should** cite the original |
| `BR-DE-27`, `BR-DE-28` | a telephone number with two digits; an address that is not quite one |
| `BR-DE-17`, `BR-DE-21` | scoping, not malformation: a lawful EN 16931 type code, or a BT-24 naming another CIUS |

A test reads `scenarios.xml` for the first and both Schematrons for the second,
comparing all 121 severities the three XRechnung profiles run — measured rather
than transcribed.

**Getting either file wrong rejects invoices Germany accepts.** `BR-CL-21` and
`BR-CL-23` are code-list rules whose CEN tables lag the registries they track
(ISO 6523 ICD, UN/ECE Rec 20/21) and KoSIT reports both at *warning*,
deliberately; five of KoSIT's own rules are warnings in the Schematron. Reading
either as fatal fails an invoice the German reference validator passes — the
worst direction for a validator to be wrong in, because it stops a document
nobody else would stop.

**A finding is re-levelled, never dropped.** No authority removes a rule, and
suppression costs the report the one line explaining why an unusual value is
present and unobjected to.

## Every finding can be explained

Rules are data — id, severity, provenance, the business terms they touch, and the
standard's own wording — so the registry is listable, filterable and explainable.

```rust
use en16931::validation::rules::{explain, explain_restriction};

assert_eq!(explain("BR-CO-3").map(|r| r.id.as_str()), Some("BR-CO-03"));   // padding
assert_eq!(explain("BR-IG-8").map(|r| r.id.as_str()), Some("BR-AF-08"));   // family alias
assert!(explain("PEPPOL-EN16931-R120").is_some());
assert!(explain_restriction("br-de-3").is_some());                          // a restriction
```

`touching(BtId(117))` answers *which rules constrain BT-117*, which is the
question you actually have when a field is in doubt.

### Hints

`BR-CL-25` says only *"MUST belong to the CEF EAS code list"*. That is the
authority's wording, it is what makes the finding look up in CEN's index, and
this crate does not touch it. The advice goes in its own field:

```text
[BR-CL-25] BG-7/BT-49 — Endpoint identifier scheme identifier MUST belong to the
CEF EAS code list. [hint: 9958 was DE:LID — the Peppol Leitweg-ID scheme, and has
been withdrawn; use 0204 instead]
```

Present on a small minority of findings — only where the crate genuinely knows
more than the rule text, never as filler.

## Deviations are allowed — and loud

Real counterparties demand them. A buyer who will not send BT-10 does not care
that `BR-DE-15` requires it, and refusing outright pushes people to fork the rule
set or ignore the validator, which is worse. So `Check` offers suppression, and
makes it impossible to hide.

```rust
use en16931::profiles::XRechnung;
use en16931::{Invoice, validation::Check};

let report = Check::of::<XRechnung>()
    .without("BR-DE-15")             // the buyer will not send BT-10
    .run(&Invoice::default());

assert_eq!(report.suppressed(), ["BR-DE-15"]);
assert!(report.to_string().contains("suppressed and NOT checked"));
```

The suppressed ids are on the report, printed by `Display`, carried in the JSON,
and `rules_checked` drops by the number of checks that were **actually** removed
— not by the number of requests, which would let a name resolving to nothing
deduct from a count of checks that were never going to run.

### A proof has to be earned twice over

`Check::prove` hands back a [`Validated<P>`](@/docs/profiles.md) only when two
things hold, and refuses on either.

| Refusal | Why |
|---|---|
| `ProveError::Suppressed` | a rule set with a hole may accept documents the full set rejects, so a proof derived from it claims something untrue |
| `ProveError::WrongProfile` | `P` names a profile this `Check` does not run |

The second was a real hole rather than a hypothetical one. `prove::<P>()` read
only its type parameter, so `Check::new(&profiles::XRECHNUNG).prove::<En16931>(inv)`
announced an XRechnung run, evaluated the bare core rule set, and returned a
proof — and the mirror image silently ran the *stricter* set. The profile named
and the profile that ran were unrelated choices, in the one method whose whole
job is to say which rule set a document passed.

`Check::of::<P>()` is the constructor that makes the mismatch unrepresentable:
the marker **is** the profile. `Check::new` stays for the runtime case a CLI
resolving `--profile`, or a service reading BT-24, actually has.

## Reports you can store, diff and ship

Two shapes: a versioned JSON one, and — behind `features = ["svrl"]` — **SVRL**,
which every Schematron tool in this field already speaks.

```rust
use en16931::{Report, Invoice, profiles};

let report = profiles::XRECHNUNG.validate(&Invoice::default());
let out = Report::of(&report);
assert_eq!(out.schema, "en16931-report/3");
assert_eq!(out.profile.as_deref(), Some("XRechnung 3.0"));
```

```json
{
  "schema": "en16931-report/3",
  "valid": false,
  "profile": "XRechnung 3.0",
  "edition": "EN 16931-1:2017+A1:2019",
  "rulesChecked": 282,
  "artefacts": [
    { "authority": "CEN",        "repo": "ConnectingEurope/eInvoicing-EN16931",
      "gitRef": "validation-1.3.16" },
    { "authority": "KoSIT",      "repo": "itplr-kosit/xrechnung-schematron",
      "gitRef": "v2.5.0" },
    { "authority": "KoSIT",      "repo": "itplr-kosit/validator-configuration-xrechnung",
      "gitRef": "v2026-01-31" },
    { "authority": "OpenPeppol", "repo": "OpenPEPPOL/peppol-bis-invoice-3",
      "gitRef": "v3.0.20" }
  ],
  "findings": [
    { "rule": "BR-02", "severity": "fatal", "source": "standard+artefact",
      "location": "BT-1", "text": "An Invoice shall have an Invoice number (BT-1)." }
  ]
}
```

`ValidationReport` derives `Serialize`, but that shape is the crate's
*internals*. `Report` is a separate, versioned shape: the internal layout may
change freely, this one only by bumping `schema`.

### What travels with a report that SVRL has no room for

The **provenance** of each rule — CEN's, a profile's, or this crate's — which
**profile and edition** produced the report, and the **authority releases** those
rules were verified against.

That last one is per profile, and the reason is `BR-DE-15`. It is KoSIT's rule,
it moves on KoSIT's release cadence, and a report stamped only
`validation-1.3.16` beside it names CEN for something CEN never published. An
XRechnung report cites four releases because its rules come from four: CEN's
model, KoSIT's Schematron, KoSIT's *validator configuration* — a separate
release, and the one that decides the severities — and OpenPeppol's, 31 of whose
rules XRechnung merges in.

A stored report that cannot say what it was checked against is close to useless
six months later. One that says the wrong thing is worse.

The SVRL feature **adds no dependencies**. SVRL is a report format, not an
invoice syntax: writing it needs escaping, not a parser. `svrl:text` stays
byte-identical to the authority's wording, and this crate's advice goes in
Schematron's supplementary-text element, so a consumer that does not know about
it ignores it and loses nothing. `location` carries a business-term path rather
than an XPath, and the output says so in a comment — there is no source document
to point into.

## It does not panic

A clearing platform does not choose its inputs. A property suite generates
structurally valid, semantically absurd documents — `i64::MAX` amounts that
overflow when summed, zero base quantities that `R120` would divide by, empty
codes, thousands of lines — and asserts that validation terminates, never panics,
is **deterministic and stably ordered**, and never cites a rule id that
`explain()` cannot resolve.

`proptest` rather than `cargo-fuzz`, on purpose: it runs in the ordinary suite on
every commit. A property that only runs when someone remembers is a property that
regresses.

### The one failure that is not a panic

Those properties generate *models*, and a model cannot be nested. A **document**
can, and `en16931-formats` had the one crash a property suite over the model was
never going to find: `roxmltree` recurses once per level of XML nesting, and at
a few hundred levels it overflows the stack. That is not a panic — Rust cannot
unwind a stack overflow and cannot catch it, so the process **aborts**.
`en16931 validate theirs.xml` exited `134`, and a service embedding the reader
simply died.

It cannot be handled afterwards, so it is refused before: both readers measure
nesting in one linear scan and return `TooDeep` past
[`MAX_DEPTH`](https://docs.rs/en16931-formats/latest/en16931_formats/ubl/constant.MAX_DEPTH.html).
The limit is 64 and the deepest of the 487 published instances is **9**, which
the corpus suite measures rather than assumes.

Two neighbouring attacks were already closed, and by the same principle of
refusing rather than coping: `roxmltree` rejects any document carrying a DTD, so
billion-laughs entity expansion and XXE file disclosure are both unreachable.

## What is next

- **[Profiles](@/docs/profiles.md)** — XRechnung, Peppol, and the typed proof.
- **[Conformance](@/docs/conformance.md)** — how all of the above is measured.
