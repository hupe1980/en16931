# 🇪🇺 en16931

**The EN 16931 semantic data model and its business rules, as Rust types.**

`billing` proves an invoice is *arithmetically* correct. This crate proves it is
*legally meaningful* — and hands a typed proof of that to the syntax layer.

```text
   ┌──────────────┐   ┌─────────────┐
   │   billing    │   │   your ERP  │
   │ calculations │   │             │
   └──────┬───────┘   └──────┬──────┘
          │ adapter (feature)│
          └─────────┬────────┘
                    ▼
        ┌─────────────────────┐
        │      en16931        │   ← this crate. Complete on its own:
        │  semantic model     │     build an Invoice, get a verdict.
        │  validation engine  │     10 deps, no XML, no I/O, wasm32.
        │  proof of validity  │
        └─────────┬───────────┘
                  │  Validated<P> — the typed proof
                  ▼
   ╭ ─ only if you exchange documents ─ ─ ─ ─ ─ ─ ─ ╮
        ┌─────────────────────┐
   │    │   en16931-formats   │  parses inbound UBL/CII,   │
        │  UBL · CII · PDF/A  │  writes outbound, +1 339
   │    └─────────────────────┘  syntax rules                │
    ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─
```

**This crate never sees a document, and most users need nothing else.** If your
system already has the invoice as data — from `billing`, from an ERP, from your
own types — `en16931` gives you the verdict and the typed proof, and that is the
whole job.

Parsing an inbound UBL or CII file, or producing one, is
[`en16931-formats`](../en16931-formats)'s job. It is a
separate crate — in the same workspace — because it depends on this one, so
rustc forbids the reverse,
and "the semantic rules do not depend on a syntax" is enforced rather than
promised. The practical payoff is the dependency line above: adding an XML
parser would end that, and adding a PDF parser takes the graph to 57 crates and
breaks `wasm32` outright.

> **Status: complete against every rule set it claims.** Not "supports
> XRechnung" — **41 of 41** KoSIT rules, asserted against KoSIT's own Schematron
> by a test.
>
> | Profile | Rules run | Coverage of its authority's artefact |
> |---|---:|---|
> | EN 16931 core | 227 | **223 / 223** CEN syntax-independent |
> | XRechnung 3.0 | 282 | **55 / 55** KoSIT asserts + **21 / 21** merged Peppol |
> | XRechnung 3.0 CVD | 290 | **+ 8 / 8** Clean Vehicles Directive |
> | XRechnung 3.0 Extension | 296 | **+ 14 / 14** `BR-DEX-*` |
> | Peppol BIS Billing 3.0 | 273 | **46 / 46** `PEPPOL-EN16931-*` |
>
> …and at the **severities those authorities publish**, which is a separate
> claim and one this crate got wrong until it was measured. KoSIT's validator
> configuration re-levels nine CEN rules across its three scenarios — see
> [severity is the authority's, not ours](#️-severity-is-the-authoritys-not-ours).
>
> And — the part that matters — **the rules agree with the authorities' own
> conformance suites**, not just with their rule lists:
>
> | Suite | Assertions | Agreement |
> |---|---:|---|
> | CEN `Invoice`/`CreditNote` unit tests | 1 024 run | **100 %** (11 declared divergences) |
> | KoSIT XRechnung mutation suite | 381 run | **100 %** |
> | Published example invoices | 58 documents | **100 % valid** |
>
> The totals move — two of those three suites are pinned to a moving upstream
> branch — so the agreement is asserted exactly and the coverage as a floor.
>
> 317 rules registered; 53 retired by the type system, 4 undecidable (CEN binds
> them to `true()` too), and **every one of the remaining 260 exercised by its
> own failing fixture**. Plus the ten semantic data types, eighteen generated
> code lists, the typed `Validated<P>` proof, the standard's own Annex A worked
> examples — **and the `billing` adapter**.
>
> On top of the verdict, three things that use the same tables rather than a
> second reading of them: a [**reconciler**](#-reconciling--the-arithmetic-every-hand-mapper-re-implements)
> that derives BG-23 and BG-22 from the lines, [**guarded code
> lists**](#️-guarded-code-lists--catching-9958-at-the-map-not-at-the-report)
> that reject a withdrawn EAS scheme at the map and name its successor, and a
> [**pre-flight**](#-pre-flight--which-fields-will-this-profile-ask-me-for) that
> says which fields a profile will ask for before the data is fetched.
>
> **Four** of those 317 are this crate's own, namespaced `EN-*` so they can never
> be mistaken for CEN's:
>
> | | |
> |---|---|
> | `EN-CURRENCY-01` | BT-5 is `XXX`, ISO 4217 for *no currency*. `BR-CL-04` accepts it because it is a real code, so an unconfigured document validates as an invoice denominated in nothing. |
> | `EN-EXT-01` | the target profile cannot represent extension data the invoice carries — §14c Abs. 1 UStG, [below](#the-14c-hole). |
> | `EN-EXT-02` | a sub-line group keyed to a BG-25 line that does not exist, which every consumer skips and no writer emits. |
> | `EN-SEPA-01` | BT-90 does not look like a SEPA Creditor Identifier. `BR-DE-30` requires it to be *present* and no rule anywhere checks that it is well formed. |
>
> Out of scope and named rather than dropped: the 1 339 *syntax* rules
> (`UBL-*`, `CII-*`) belong to
> [`en16931-formats`](../en16931-formats), and Peppol's
> ~90 national rules (`DK-R-*`, `SE-R-*`, …) are country registry-format and
> check-digit checks.

---

## ✅ Checked against the authorities' own conformance suites

Every other test here was written by the same person as the code it checks, from
the same reading of the same documents. These two were not.

**CEN's unit tests** — 277 files, ~1 130 assertions, each a minimal UBL fragment
with an explicit expectation:

```xml
<test>
  <assert><error>BR-01</error></assert>
  <Invoice> … no CustomizationID … </Invoice>
</test>
```

**Published example invoices** — the 58 complete invoices CEN and OpenPeppol
ship as examples of correct usage. No per-rule expectation, just a blunt one:
*the authority publishes this as valid*, so **nothing fatal may fire**. That is
the assertion a corpus of deliberately-broken documents structurally cannot make
— it is the only thing that catches a rule which is merely **too eager**.

**KoSIT's mutation suite** — 224 *complete, valid* XRechnung invoices with
mutations embedded as processing instructions:

```xml
<?xmute mutator="remove" schematron-invalid="xrubl:BR-DE-15" ?>
<cbc:BuyerReference>90000000-03083-12</cbc:BuyerReference>
```

Remove BT-10 and `BR-DE-15` must fire. `identity` asserts the *unmutated* invoice
is clean — which catches rules that are too eager, the failure mode a corpus of
deliberately-broken documents cannot see.

Running them needs a UBL reader, which lives in [`tests/ubl.rs`](tests/ubl.rs) —
test-only, so it is not in this crate's API, dependency graph or wasm build. It
records every element it does not map, and a test asserts that set is empty.

**Five real bugs came out of the first run**, none of which any other test in
this repository could have found:

| | |
|---|---|
| `BR-CL-15` | checked BT-80; its artefact context is `cac:OriginCountry` — **BT-159**. It had been duplicating `BR-CL-14` and leaving BT-159 unchecked. |
| `BR-CO-09` | checked seller and buyer, **not BT-63** — which the rule text names. |
| `BR-CO-25` | fired on credit notes. CEN has six cases titled *"Verify that rule does not fire on Credit Notes"*. |
| `BR-CL-07` | covered BT-128 and not BT-18 — its context is a union of the two. |
| `BR-DE-18` | missed the second half of the rule: the Skonto block must **end with a newline**. |
| `PEPPOL-EN16931-R003` | modelled as "BT-10 is mandatory". It is a **disjunction** — BT-10 *or* BT-13 — and rejected eight of CEN's own published invoices. |
| `PEPPOL-EN16931-R004` | modelled as "BT-24 is present". It constrains BT-24's **value** with `starts-with`, so any string passed. |
| `BR-CL-22` | compared BT-121 case-sensitively. The **released** artefact wraps it in `upper-case()`; only the source file does not. |

`BR-CO-25` forced a model change. CEN's credit-note cases carry **no BT-3 at
all**, so inferring the document kind from the type code cannot answer the
question they ask. [`DocumentKind`](src/invoice.rs) is now an explicit field —
which is also what both syntaxes carry, and it makes `BR-CL-01` exact rather
than permissive.

### The 11 divergences are declared, not ignored

Two causes, both the same shape — UBL can write down a state this model does not
have.

**Nine cases**: a group that is present and empty. `<cac:PostalAddress/>`,
`<cac:BillingReference/>`, `<cac:InvoicePeriod/>`. An address with no fields *is*
an absent address here; there is no third state, and adding one would put a
syntax artefact in every consumer's way to satisfy nine test cases.

**Two cases**: two `cac:TaxTotal` elements in the same currency with different
amounts. BT-110 is one field, so the contradiction cannot be written down, and
`BR-CO-15` is then satisfied by whichever value was read. Peppol's `R053` catches
it, and it is a syntax rule about element counts.

The table is asserted exactly — by file and by rule — so it can only shrink, and
a divergence that starts agreeing fails the build as loudly as a new one.

---

## Why another e-invoicing library

Every existing implementation — [phive], [Mustangproject], the KoSIT validator —
is **XML in, Schematron out**. You cannot ask them anything until you have
serialised a document, so the loop is *build → serialise → validate → parse the
error → guess which field it meant*.

This crate validates the **model**. A finding points at `BG-25[2]/BT-151`, not at
an XPath, and you can check an invoice you are still assembling.

That buys four things nothing XML-first can offer:

1. **Whole rule families become unrepresentable.** All 21 `BR-DEC-*` rules die
   to a two-decimal type, and presence and cardinality rules die to non-`Option`
   fields and enums — **53 rules retired by the type system**, not by a
   predicate. They stay in the registry so `explain` works and a report can say
   they were checked.
2. **A proof that survives the call boundary.** `Validated<XRechnung>` means a
   serialiser physically cannot be handed an unchecked invoice.
3. **Cross-edition answers.** *"Valid today, and still valid under XRechnung
   4.0?"* is one call, not two pipelines.
4. **µs, not ms, and no JVM** — plus `wasm32`, so the invoice never has to leave
   the client.

What we do **not** claim to beat: a Schematron-driven tool is, by construction,
exactly as correct as the artefact. Our rule logic is hand-written, so it can be
wrong in ways theirs cannot. That is why the conformance corpus gates every
release — and why the crate reports its own coverage rather than implying it:

```text
conformance corpus
  registered:            317
  retired by the types:   53  (no state can make them fire)
  undecidable:             4  (CEN binds them to true() too)
  checkable:             260
  exercised by a case:   260  (100% of checkable)
  declared uncovered:      0
```

Those five figures are not typed into this file. A test reads them back out of
it — and out of every other README, `lib.rs` and documentation page — and
compares each against the value the code produces. Three of them had been wrong
here for several releases, which is what the test is for.

**A rule nobody has seen fire may be inverted, unreachable, or checking the
wrong field — and the suite would be green either way.** So every registered rule
either has a fixture that makes it fire, or is a rule the *type system* retires
and no document can trigger. There is no third category: the gate fails if a rule
is uncovered and undeclared, if a declared rule has since been covered, **and if
anything is declared for a reason other than being type-retired**. The excuse
list has no room in it.

[phive]: https://github.com/phax/phive
[Mustangproject]: https://github.com/ZUGFeRD/mustangproject

---

## Design invariants

- **No `f64`.** Amounts are fixed-point; rates and quantities are `Decimal`.
- **No I/O, no async, no `unsafe`.** `#![forbid(unsafe_code)]`, `wasm32` tested.
- **Rounding is never implicit.** An amount that does not fit two decimals is an
  error, not a rounding opportunity.
- **Mandatory means non-`Option`.** Rules exist for the cardinalities the type
  system cannot express, not as a substitute for it.
- **Types enforce representability; rules enforce validity.** An *invalid*
  document must still be representable, or a parser cannot load it in order to
  explain what is wrong.
- **Two dependencies** by default: `rust_decimal` and `thiserror`.

---

## 🏗️ A whole invoice, end to end

Two lines at two VAT rates, built, reconciled and validated. Nothing is elided —
this is the complete program, and it is a doctest, so it compiles and passes on
every commit.

```rust
use en16931::invoice::{Party, PostalAddress};
use en16931::{Date, Identifier, InvoiceAmount, Percentage, Quantity, prelude::*};
use rust_decimal::dec;

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let seller = Party {
    name: Some("Stadtwerke Musterstadt GmbH".into()),
    vat_identifier: Some("DE123456789".into()),
    // BT-34's scheme is an EAS code, checked here rather than at validation
    // time — `9958` is the one every German integrator reaches for, and it was
    // withdrawn on 2023-07-31.
    electronic_address: Some(Identifier::eas("4012345000009", "0088")?),   // GLN
    address: PostalAddress {
        city: Some("Musterstadt".into()),
        post_code: Some("12345".into()),
        country: Some(en16931::codes::guard::country("DE")?),             // BR-09
        ..Default::default()
    },
    ..Default::default()
};
let buyer = Party {
    name: Some("Beispiel AG".into()),
    electronic_address: Some(Identifier::eas("991-01234-56", "0204")?),
    address: PostalAddress {
        city: Some("Beispielstadt".into()),
        post_code: Some("54321".into()),
        country: Some(en16931::codes::guard::country("DE")?),             // BR-11
        ..Default::default()
    },
    ..Default::default()
};

let invoice = Invoice::builder(
        "urn:cen.eu:en16931:2017",       // BT-24
        "R-2026-0001",                   // BT-1
        Date::parse("2026-07-31")?,      // BT-2
        "380",                           // BT-3 — commercial invoice
        "EUR",                           // BT-5
    )
    .seller(seller)
    .buyer(buyer)
    .due_in_days(14)                     // BT-9 — satisfies BR-CO-25
    .line(InvoiceLine::new(
        "1", "Netznutzung Arbeitspreis",
        Quantity::new(dec!(10000)), "KWH",
        InvoiceAmount::parse("2890.00")?,
        "S", Some(Percentage::new(dec!(19))),
    ))
    .line(InvoiceLine::new(
        "2", "Messstellenbetrieb",
        Quantity::new(dec!(12)), "MON",
        InvoiceAmount::parse("120.00")?,
        "S", Some(Percentage::new(dec!(7))),
    ))
    // BG-23 and BG-22 are a *function* of the lines. This computes it.
    .build_reconciled()?;

assert_eq!(invoice.vat_breakdown.len(), 2);          // one group per rate
assert_eq!(invoice.totals.gross_total.to_string(), "3567.50");
assert!(validate(&invoice).is_valid());
# Ok(()) }
```

---

## 🧮 Reconciling — the arithmetic every hand-mapper re-implements

`BR-CO-10` … `BR-CO-16`, the `-08` and `-09` rows of all nine VAT category
families, and `BR-CO-18` are **one function of the lines**. If your engine
already produced the positions, it should not also have to know which rows form
one BG-23 group, that BT-107 is *absent* rather than zero when there are no
allowances, or that BT-119 is stated for category `O` even though BT-152 is not.

```rust
use en16931::reconcile::Reconciler;

# fn demo(mut inv: en16931::Invoice) -> Result<(), Box<dyn std::error::Error>> {
Reconciler::new()
    .exemption("AE", None, Some("VATEX-EU-AE"))    // BR-AE-10 needs a reason
    .paid(en16931::InvoiceAmount::parse("190.00")?)   // BT-113
    .apply(&mut inv)?;
# Ok(()) }
```

Grouping comes from the **same table** the `-08` rows are checked against, not
from a second reading of the standard, and a test builds one invoice per
category and asserts the result carries no arithmetic finding. The two cannot
drift apart silently.

Three things it deliberately does not do: invent a BT-120 exemption reason
(only the seller knows it), round per line rather than per group (that is how
three `0.05` lines come out a cent wrong), or default an absent rate on a taxed
category to zero — which balances perfectly and under-declares VAT.

---

## 🏷️ Guarded code lists — catching `9958` at the map, not at the report

`contains` answers yes or no. `guard` answers **what to do**:

```rust
use en16931::codes::guard;

// Withdrawn on 2023-07-31. The hint names its successor.
let err = guard::eas("9958").unwrap_err();
assert!(err.to_string().contains("use 0204 instead"));

// The single most common unit-code bug.
assert!(guard::unit("kwh").unwrap_err().to_string().contains("did you mean \"KWH\""));

assert!(guard::eas("0204").is_ok());
assert!(guard::unit("KWH").is_ok());
```

Twelve EAS schemes have left the CEF list since CEN artefact `validation-1.2.0`
— nine in 2023 alone — and `guard::WITHDRAWN` names the successor of each. The
table's central claim, that every entry really is gone from the current list and
every named successor really is in it, is asserted against the pinned artefacts,
so a code CEN reinstates fails the build rather than producing a wrong hint.

This is a convenience, never a second source of truth: each function checks the
same generated list the corresponding rule checks, and skipping the layer loses
nothing but the earlier message.

### 💡 Hints — the sentence that ends the investigation

`BR-CL-25` says only *"MUST belong to the CEF EAS code list"*. That is the
authority's wording, it is what makes the finding look up in CEN's index, and
this crate does not touch it. The advice goes in its own field:

```text
[BR-CL-25] BG-7/BT-49 — Endpoint identifier scheme identifier MUST belong to the
CEF EAS code list. [hint: 9958 was DE:LID — the Peppol Leitweg-ID scheme, and has
been withdrawn; use 0204 instead (the Leitweg-ID itself belongs in BT-10 …)]
```

Present on a small minority of findings — only where the crate genuinely knows
more than the rule text, never as filler. It travels in the JSON shape and in
SVRL's own `svrl:diagnostic-reference`, so `svrl:text` stays byte-identical to
the authority's.

---

## ↩️ Stornorechnung — the credit note that cancels an invoice

`Invoice` fields are public precisely so a stored invoice can be reshaped
without re-billing. The one reshape with rules attached gets a method:

```rust
# use en16931::{Date, Invoice};
# use en16931::invoice::DocumentKind;
# fn demo(original: Invoice) -> Result<(), Box<dyn std::error::Error>> {
let storno = original.to_credit_note("STORNO-2026-0007", Date::parse("2026-08-15")?);

assert_eq!(storno.kind, DocumentKind::CreditNote);   // the UBL root element
assert_eq!(storno.type_code.unwrap().as_str(), "381");
assert_eq!(storno.preceding_invoices.len(), 1);      // BG-3 → BT-25, per BR-55
# Ok(()) }
```

**It does not negate the amounts**, and that is the point. Under EN 16931 a
credit note states what is credited as a *positive* figure — the document type
carries the direction. Negating would state it twice and fail `BR-S-08` against
the document's own lines.

---

## 🛫 Pre-flight — which fields will this profile ask me for?

`validate` answers *"is this document acceptable?"*. On a half-built invoice
that is a hundred findings, most of them about lines and totals that are not
there yet. A different question, answerable **before** the data is fetched:

```rust
use en16931::invoice::{Party, PartyRole};
use en16931::profiles::XRECHNUNG;

let gaps = Party::default().missing_for(&XRECHNUNG, PartyRole::Buyer);
let terms: Vec<_> = gaps.iter().map(|m| m.term.0).collect();
assert!(terms.contains(&52));   // BT-52 Buyer city     — BR-DE-8
assert!(terms.contains(&53));   // BT-53 Buyer post code — BR-DE-9
```

So a seller whose master data lives in a contract service fetches what XRechnung
needs in one round trip, instead of a build-validate-fetch loop.
`XRECHNUNG.missing_terms(&invoice)` does the same for a whole document.

Restrictions only, and deliberately: they are the §7.3.2 axis that is pure data,
so the answer is exact. The conditional rules cannot be answered before the
document exists — `BR-DE-23-a` asks for BT-84 only if BT-81 names a credit
transfer — and `validate` remains the complete check.

---

## 💶 `InvoiceAmount` — where 21 rules go to die

EN 16931-1 §6.5.2 does not merely *restrict* amounts to two decimals; it defines
the semantic type that way:

> EN 16931_ Amount. Type is floating up to two fraction digits.

Table 26 then lists every term it applies to, and the CEN artefacts render that
table as 21 assertions — `BR-DEC-01`, `-02`, `-05`, `-06`, `-09`..`-20`,
`-23`..`-25`, `-27`, `-28`. A type that cannot hold a third decimal retires all
of them at compile time.

```rust
use en16931::InvoiceAmount;

let net = InvoiceAmount::parse("1000.00")?;
let vat = InvoiceAmount::parse("190.00")?;
assert_eq!(net.checked_add(vat)?.to_string(), "1190.00");

// Refused, not rounded — neither 0.01 nor 0.00.
assert!(InvoiceAmount::parse("0.005").is_err());
# Ok::<(), Box<dyn std::error::Error>>(())
```

Every operation is `checked_`: an invoice total that overflows is a data error
worth surfacing, never a number worth guessing.

### …and where it must *not* be used

`Unit Price Amount` (§6.5.3) is a **different** semantic type — based on Amount,
but with no cap. Its own example in the standard is `10000.1234`.

```rust
use en16931::{InvoiceAmount, UnitPriceAmount};
use rust_decimal::dec;

let price = UnitPriceAmount::new(dec!(0.28901));   // EUR/kWh
assert_eq!(price.to_string(), "0.28901");

// The same value as an Amount would be refused outright.
assert!(InvoiceAmount::from_decimal_exact(dec!(0.28901)).is_err());
```

---

## ✅ Validating

```rust
use en16931::{validate, prelude::*};

let invoice = Invoice::default();          // nothing filled in
let report = validate(&invoice);

assert!(!report.is_valid());
assert!(report.has("BR-02"));              // no invoice number
assert!(report.has("BR-16"));              // no invoice line

// Findings point at business terms, never at an XPath.
assert_eq!(report.fatal().next().unwrap().path.to_string(), "BT-1");
```

A [`ValidationReport`] carries **every** finding, ordered stably so a CI diff
means something. `report.into_result()?` is there for the ergonomic path, but the
report is the product — a rejection from a clearing platform lists every problem,
and a validator that reports one is a validator you run in a loop.

Rule ids normalise, because the standard and the artefacts spell them
differently:

```rust
use en16931::validation::rules;

// EN 16931-1 writes `BR-CO-4`; the CEN artefacts write `BR-CO-04`.
assert_eq!(rules::explain("BR-CO-4").map(|r| r.id.as_str()), Some("BR-CO-04"));
assert_eq!(rules::explain("br-1").map(|r| r.id.as_str()), Some("BR-01"));
```

### The nine VAT category families

EN 16931-1 §6.4.3 writes these as **nine parallel tables** with the same ten row
headings. `BR-S-08` and `BR-Z-08` are the same sentence with a different category
and a different answer to *"may this category appear at several rates"*.

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

`BR-*-08` is the keystone — the only rule tying invoice lines to the VAT
breakdown, and therefore the only thing that turns a mis-attributed line into a
*reported* error rather than a silently wrong invoice.

### The four tolerance regimes

Impossible to get right from the standard's prose, and where a hand-written
engine most easily diverges from the Schematron everyone else runs:

| Regime | Rules | Tolerance | Whose | Where |
|---|---|---|---|---|
| Totals chain | `BR-CO-10` … `BR-CO-16` | **exact** | CEN | core |
| VAT derivation | `BR-CO-17`, `BR-*-08/09` | **±1.00**, on absolute values | CEN artefacts | core |
| Line & allowance derivation | `R120`, `R040` | **±0.02** | Peppol | `Profile::extra_rules` |
| …the same rules, in HUF | `R120`, `R040` | **±0.5** | XRechnung | `Profile::extra_rules` |

**None of it is in the standard.** EN 16931-1 §6.4.2 states `BR-CO-17` as a plain
equation with no slack; the ±1.00 is an artefact decision, the ±0.02 a Peppol
one, and the ±0.5 XRechnung's — it rewrites Peppol's constant to
`if($documentCurrencyCode = 'HUF') then 0.5 else 0.02` when it merges the rules
in, because HUF has no minor unit in practice. Peppol never widens it.

So the same forint invoice can be a valid XRechnung and an invalid Peppol
document. That is why tolerance is a property of the **rule instance a profile
holds**, never a crate-wide constant — and why each rule records its `Source`.

All four are implemented and pinned in both directions:

```text
// Exact — one cent out is fatal.
BR-CO-14:  BT-110 = 19.01 against Σ BT-117 = 19.00   → fires

// ±1.00 on absolute values, which is what lets credit notes pass.
BR-CO-17:  BT-117 = 18.50 against 100.00 × 19%       → passes
BR-CO-17:  BT-117 = 17.50                            → fires

// ±0.02, Peppol only.
R120:      BT-131 = 100.02 against 1 × 100.00        → passes
R120:      BT-131 = 100.03                           → fires
R120 under Profile::core()                           → not a rule at all
```

`R120` has **no CEN counterpart**: EN 16931 never ties BT-131 to quantity × price,
so under the core profile a line whose amount does not follow from its price is
perfectly valid. And `R046` is the trap — it looks like `R040`'s sibling and
carries **no slack at all**.

[`ValidationReport`]: https://docs.rs/en16931/latest/en16931/validation/struct.ValidationReport.html
[`Source`]: https://docs.rs/en16931/latest/en16931/validation/enum.Source.html

---

## 🔌 From `billing`

`billing` owns the arithmetic. This crate owns what the arithmetic *means*. The
caller owns the parties — a tariff engine has no business knowing a buyer's
postal address.

```rust,ignore
let invoice = FromBilling::new(&billing_document)
    .specification_id(profiles::XRECHNUNG.specification_id)
    .seller(seller)
    .buyer(buyer)
    .build()?;

validate(&invoice).into_result()?;
```

There is deliberately **no `TryFrom`**. A `BillingDocument` has no seller, no
buyer, no addresses, no country codes and no item names — `LineItem::description`
is display text, not BT-153. A `TryFrom` would fail on every realistic input,
which makes it a trait impl whose only behaviour is `Err` while implying that
conversion is total.

### The levy trap

The reason the adapter is not a field-for-field copy. A per-unit excise —
Stromsteuer, a CO₂ levy — is produced by a `TaxLayer`, so it lands in
`tax_total`. But EN 16931 counts it **inside the taxable base**: it is a BG-21
document level charge, not tax.

Map `tax_total → BT-110`, the obvious thing to do, and `BR-CO-14`
(`BT-110 = Σ BT-117`) fails on **every levy-bearing invoice**.

```text
BT-106  Σ line net amounts        ← net positions
BT-107  Σ allowances              ← discount positions (stated positive)
BT-108  Σ charges                 ← the levy
BT-109  = BT-106 − BT-107 + BT-108
BT-110  = Σ BT-117                ← VAT only
BT-112  = BT-109 + BT-110         ← equals billing's gross_total
```

`net_total` appears nowhere: it is `BT-106 − BT-107`, which EN 16931 has no term
for. That is the single most valuable thing the upstream work clarified.

### The §14c hole

A final invoice deducting advance payments must, in Germany, state *"die auf sie
entfallenden Steuerbeträge"* — the tax in each advance (§14 Abs. 5 Satz 2 UStG).
Omit it and the issuer owes that tax **a second time** under §14c Abs. 1.

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

### The BT-20 newline — a trap that moved upstream

The newest of the seam's traps, and the smallest: one character.

`billing` 0.12 gained structured payment terms, and rendered BT-20 including
Germany's Skonto micro-syntax — without a terminator:

```text
Zahlbar innerhalb 30 Tagen ohne Abzug.
#SKONTO#TAGE=10#PROZENT=2.00#
```

`BR-DE-18` has **two** halves, and the second hides inside the same assertion as
the first:

```xpath
every $line in …tokenize(., '(\r?\n)')[starts-with(normalize-space(.), '#')]
  satisfies matches(normalize-space($line), $XR-SKONTO-REGEX)
        and matches(…tokenize(., '#.+#')[last()], '^\s*\n')
```

Everything after the **last** `#…#` must begin with a newline. The rendering
above ends at the `#`, so `tokenize(…)[last()]` is the empty string and every
German invoice carrying a Skonto fails.

The adapter appended the newline for one release. **`billing` 0.13 does it
upstream**, which is the right place: the `#SKONTO#…#` syntax has no core
EN 16931 form, so a rendering that omits the terminator is valid nowhere at all.

What is left here is a *guard*, not a fix — idempotent, and paired with
`billing_renders_bt_20_with_the_terminator_br_de_18_needs`, which asserts the
**upstream** behaviour rather than the adapter's output. Asserting the output
alone would pass just as well against an upstream regression and an adapter
quietly papering over it, which is exactly the state these two crates were in a
release ago.

### Four conversions that are not copies

| | |
|---|---|
| **Rates** | `billing` stores `0.19` because that is what you multiply by; EN 16931 stores `19`, what you print. Converted once, here. |
| **Signs** | `billing` models a return as `Sign::Credit` with a *non-negative* quantity. EN 16931 puts the sign on **BT-129** and forbids a negative BT-146 (BR-27) — Annex A.1.6. A negative unit price gets flipped onto the quantity rather than dropped. |
| **Precision** | Refused, never rounded — and the error names the fix (`.amount_scale(AmountScale::EN16931)`) rather than the symptom. |
| **The document kind** | `DocumentKind::is_credit_note()`, not BT-3. `81` is on *both* UNTDID 1001 lists, and `en16931-formats` picks the UBL document element from the kind — so deriving it from the code would put a credit note inside `<ubl:Invoice>`. |

### What crosses

BT-1, BT-2, BT-3, BT-5, BT-6, BT-9, BT-20, BT-21, BT-22, BT-29, BT-46, BT-111,
BG-1, BG-14, BG-20, BG-21, BG-22, BG-23, BG-25 and ZUGFeRD's `BG-X-45` — plus the
document kind, which is not a business term and decides the root element.

Two of those pairs are worth naming:

* **BT-6 and BT-111 cross together.** `BR-53` makes the second mandatory whenever
  the first is present, so mapping only the currency would manufacture a finding
  out of a complete document.
* **BT-29 and BT-46 are merged, not overwritten.** The caller's `Party` carries
  master data; the document carries the party code the billing run was keyed on —
  an MP-ID in the energy market, a GLN in retail. EN 16931 makes both repeatable
  precisely because a party has more than one identity. The scheme is compared
  alongside the value, because the same digits under `0088` and under `0293` are
  two registries saying two different things.

`meta.period_label` and `meta.labels` do not cross: display text and arbitrary
key/value pairs, with no business term at all.

Units are resolved from `Quantity::code` first, falling back to a small
`UnitResolver` table. An unresolvable label is an **error**: guessing produces an
invoice that validates and describes the wrong thing, and unlike a wrong amount
nobody notices.

---

## 🇩🇪 Profiles — a CIUS is a set of *restrictions*

Every Schematron-based tool models a CIUS as "core rules plus extra rules",
because Schematron has no other vocabulary. **That is not how EN 16931 defines
one.**

§7.3.2 is a normative table of the thirteen kinds of change a CIUS may make
across six axes. **Only one of those axes is "add a rule."** The other five are
*restrictions on the model* — so they are data here, and the rules are derived:

```rust
use en16931::profiles;
use en16931::validation::profile::Restriction;

// Eleven of XRechnung's BR-DE rules are pure `Mandatory` restrictions, and two
// are `CodeValues`. All thirteen are data, not code; the other 28 need code.
let ids: Vec<_> = profiles::XRECHNUNG.restrictions.iter().map(Restriction::id).collect();
assert!(ids.contains(&"BR-DE-3"));    // Seller city (BT-37) shall be present
assert!(ids.contains(&"BR-DE-17"));   // BT-3 restricted to eight codes
```

Each keeps **its real published id**, so a finding is lookup-able in KoSIT's
index — and the path still names the business term:

```text
[BR-DE-3] BG-4/BT-37 — Seller city (BT-37) shall be present
```

### Two properties this buys

**A CIUS can be checked for conformance.** §4.4.2 requires that *"the resulting
invoice document instance shall be fully compliant to the core invoice model"*.
Every `Restriction` variant is by construction a narrowing, so a profile that
tried to *loosen* something cannot be expressed. Loosening is an **Extension**
(§4.3, CEN/TR 16931-5) — a different mechanism.

**Validation widens for free — from a *conformant* CIUS.** §4.4.4 says an
instance complying with one *"can still be received and processed by a party who
is not supporting the CIUS"*. So the proof converts, infallibly:

```rust,ignore
let proof: Validated<PeppolBis3> = Validated::new(invoice)?;
serialise_peppol(&proof);           // demands the CIUS proof
accepts_core(proof.widen());        // §4.4.4 — free, no re-validation
```

Peppol BIS Billing 3.0 is the one shipped CIUS this holds for, and the reason it
is not XRechnung is the next section.

### Enums retire rules too

`BR-DE-23-b`, `-24-b` and `-25-b` each forbid the two payment groups BT-81 did
not name. Because `PaymentMeans` is an **enum** over BG-17 / BG-18 / BG-19, that
combination cannot be written down — so all three have nothing left to check.

The `-a` halves stay real: they tie the *variant* to BT-81's **value**, which no
type can see.

```text
pub enum PaymentMeans {
    CreditTransfer(Vec<CreditTransfer>),  // BG-17
    Card(PaymentCard),                    // BG-18
    DirectDebit(DirectDebit),             // BG-19
}
```

### Offline checks that are worth doing

`BR-DE-19` and `-20` want a *correct IBAN*. This crate implements ISO 7064
mod-97-10 — no registry, no network, so it still runs on `wasm32`. It cannot tell
you the account exists, only that the string is not a typo, which catches the
overwhelming majority of real errors.

Both are **warnings**, matching KoSIT's `soll`: a suspicion, not a rejection.

### Not levels — siblings

It is tempting to model these as an ordered scale. There is no such order, and
the crate's tests pin it: **XRechnung permits `BT-3 = 389`** (self-billed) and
Peppol does not; **Peppol permits `386`** (prepayment invoice) and XRechnung does
not. Neither is "more restrictive".

The sharpest case is BT-119. CEN's `BR-48` exempts category `O` from stating a
VAT breakdown rate; XRechnung's `BR-DE-14` requires it **unconditionally**.
Suppressing BT-119 for `O` — on the strength of `BR-O-05`, which governs BT-152,
a *different* term — is the natural mistake, and it fails the KoSIT validator.

### XRechnung merges 31 of Peppol's rules — and rewrites two

The Schematron in KoSIT's repository contains only `BR-DE-*`, `BR-DEX-*` and
`BR-TMP-*`. **That file is an input, not the artefact.** The build runs
`peppol-into-xr.xsl` over it, splicing in every Peppol assert named in
`rule-list.xml`, and *that* is what ships:

```xml
<target name="merge-peppol-rules-with-xr-rules">
  <xslt in="…/XRechnung-UBL-validation.sch" style="…/peppol-into-xr.xsl" …/>
```

31 of Peppol's 46. The fifteen left out are `CL001`…`CL008` (Peppol's own
narrower code lists) and `P0104`…`P0112` (the VATEX-to-category pinning, plus
the German-parties type-code rule that would be circular inside a German CIUS).

Two are **rewritten on the way in**, and both differences are observable:

| | Peppol | XRechnung |
|---|---|---|
| `R120` severity | `fatal` | **`warning`** |
| `R040` / `R120` slack | `0.02` always | **`0.5` for HUF**, 0.02 otherwise |

HUF has no minor unit in practice, so 0.02 is tighter than the currency can
express — and Peppol never widens it. The same forint invoice can be a valid
XRechnung and an invalid Peppol document, so the two profiles hold **separate
instances** of those rules rather than sharing one.

This reverses what this crate concluded one revision earlier, from reading
KoSIT's validator configuration:

```xml
<resource>…/EN16931-UBL-validation.xsl</resource>   <!-- CEN's -->
<resource>…/xsl/XRechnung-UBL-validation.xsl</resource>   <!-- its own -->
```

Two Schematrons, no Peppol — which is true, and does not mean what it looks
like. The second one *already contains* Peppol's rules by the time the validator
loads it.

### A CIUS restricts; an Extension adds — and KoSIT ships one of each

`profiles::XRECHNUNG_EXTENSION` is §4.3's second mechanism in the wild. Where the
CIUS narrows, it **widens**:

| | Core / CIUS | Extension |
|---|---|---|
| BT-125 mime code | six codes | + `application/xml` |
| scheme identifiers | ISO 6523 ICD / CEF EAS | + `XR01`–`XR03` (DiGA) |
| BT-115 | `BR-CO-16` | **`BR-DEX-09`** — third-party payments added back |

and it adds two groups the core model has no term for: `BG-DEX-01` sub-invoice
lines, for positions that decompose, and `BG-DEX-09` third-party payments, for
the German digital-health case where a statutory insurer settles part of an
invoice addressed to the insured. Both live in `en16931::extensions`, not on
`InvoiceLine` — a core line has no child, and putting one there would make every
consumer carry a field only one Extension populates.

Because it widens, §4.4.4's guarantee does not run: an Extension-valid invoice
need **not** be core-valid, so there is deliberately no
`Underlies<XRechnungExtension> for En16931`.

**The CVD variant is the awkward case.** Its identifier says `#compliant#` —
§4.3's word for a *CIUS* — but `BR-TMP-CVD-01` checks BT-158's scheme against
UNTDID 7143 **plus `CVD`**, and `CVD` is not in UNTDID 7143. So a conforming CVD
invoice violates core `BR-CL-13`, which a CIUS may not cause. This crate follows
the behaviour rather than the label. Reporting `BR-CL-13` as fatal on every CVD
invoice would be a false positive on a document KoSIT accepts.

---

## ⚠️ Severity is the authority's, not ours

A rule's *consequence* is not a property of the rule. It is a property of the
rule **in a profile**, and the authorities publish it separately from their
Schematron — which is why reading only the Schematron gets it wrong.

KoSIT's validator configuration says so in as many words, once per scenario:

```xml
<!-- overwrites CEN severity level "fatal" for codelist values of BT-130 … -->
<customLevel level="warning">BR-CL-23</customLevel>
<!-- overwrites CEN severity level "fatal" to enable use of mime codes per BR-DEX-01 -->
<customLevel level="information">BR-CL-24</customLevel>
```

Nine CEN rules are re-levelled across the three XRechnung scenarios, and
`Profile::levels` carries all nine. `tests/codelists.rs` reads
`scenarios.xml` out of `spec/` and asserts the mapping, so it is measured rather
than transcribed from memory. Two consequences are worth stating outright.

**This crate used to reject invoices Germany accepts.** `BR-CL-21` and
`BR-CL-23` are code-list rules whose CEN tables lag the registries they track —
ISO 6523 ICD and UN/ECE Rec 20/21. KoSIT reports both at *warning*, deliberately;
this crate reported them as fatal, so a perfectly ordinary German invoice with a
unit code CEN has not yet imported failed here and passed there. That is the
worst direction for a validator to be wrong in, because it stops a document
nobody else would have stopped.

**A finding is re-levelled, never dropped.** The mechanism used to be
`suppressed: &[&str]`, a list of rules to remove — which no authority does, and
which cost the report the one line explaining why an unusual value is present and
unobjected to. It also encouraged reconstructing the list from *"which rule does
each `BR-DEX-*` widen?"*, and that reconstruction was wrong twice: it named
`PEPPOL-EN16931-CL001`, which XRechnung's build does not merge in, so it removed
nothing while CEN's `BR-CL-24` went on rejecting exactly the `application/xml`
attachment `BR-DEX-01` exists to permit.

**And XRechnung 3.0 is therefore not a conformant CIUS.** §4.4.2 forbids a CIUS
to accept what the core model rejects, and relaxing `BR-CL-23` does exactly that.
`Profile::is_conformant_cius()` computes this from the data rather than asserting
it, and answers `false` for three of the five shipped profiles:

| Profile | Conformant CIUS? | Because |
|---|---|---|
| EN 16931 | n/a | it *is* the core model |
| XRechnung 3.0 | no | `BR-CL-21`, `BR-CL-23` → warning |
| XRechnung 3.0 CVD | no | + `BR-CL-13` → information |
| XRechnung 3.0 Extension | no | + six more, `BR-CO-16` among them |
| Peppol BIS Billing 3.0 | **yes** | ships the flags it means, and no override file |

So `Validated<XRechnung>` does **not** widen to `Validated<En16931>`, for the
same reason `Validated<XRechnungCvd>` does not. Re-validate instead —
`Validated::<En16931>::new(invoice)` is one line, and it is a line that can
honestly fail.

---

## 📅 `Date` — a calendar day, not an instant

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
# Ok::<(), Box<dyn std::error::Error>>(())
```

Three integers, no timezone — there is nothing to offset, and shifting BT-2 by a
zone changes the VAT period it falls in. Enable `chrono` or `time` for
conversions; the default build carries neither.

---

## 📊 `Percentage` — per cent, never a fraction

> Percentages are given as fractions of a hundred (per cent) e.g. the value
> 34,78 % in percentage terms is given as 34,78. — §6.5.5

```rust
use en16931::Percentage;
use rust_decimal::dec;

let vat = Percentage::new(dec!(19));      // nineteen per cent, NOT 0.19
assert_eq!(vat.to_string(), "19");
assert_eq!(vat.as_fraction(), dec!(0.19)); // what you multiply by
```

This is the most common transcription bug when bridging a calculation engine:
`billing` stores `0.19` because that is what you multiply by; the standard stores
what you print. Convert once, at the boundary.

Trailing zeros need no special handling — `rust_decimal` compares by value, so
`19` and `19.00` are one VAT breakdown group in `Eq`, `Ord` **and `Hash`**. The
crate pins that with a test, because a `Hash` disagreeing with `Eq` here would be
a silent, data-dependent grouping bug.

---

## 🔢 `Quantity` — and why it may be negative

Annex A.1.6 (*Example 5 — Negative Invoice line*) invoices 25 cases of pens and
credits 10 returned ones **on the same ordinary invoice**:

| BT-126 | BT-129 | BT-146 | BT-131 |
|---|---|---|---|
| 1 | `25` | `8,50` | `212,50` |
| 2 | **`−10`** | `8,50` | **`−85,00`** |

The sign lives on the **quantity**, never on the price — BR-27 forbids a negative
item net price.

```rust
use en16931::Quantity;
use rust_decimal::dec;

let returned = Quantity::new(dec!(-10));
assert!(returned.is_negative());
```

### The proof has to be earned

```rust,ignore
let proof: Validated<PeppolBis3> = Validated::new(invoice)?;
let core: Validated<En16931>     = proof.widen();   // infallible — §4.4.4
```

Widening is free *only* from a conformant CIUS, and `Profile::is_conformant_cius()`
is the runtime witness — computed from `Profile::levels`, not declared. It
answers `false` for three shipped profiles, and each `false` closed a hole.

`impl Underlies<XRechnungCvd> for En16931` existed once, so
`Validated<XRechnungCvd>::widen::<En16931>()` compiled and produced a **proof of
core-validity for an invoice violating `BR-CL-13`**. A serialiser trusting the
core proof — the entire purpose of `Validated<P>` — would have been handed a
document no core-only receiver can process.

`impl Underlies<XRechnung> for En16931` was the same hole one layer up, and it
survived the first fix because the argument stopped at the CVD variant. KoSIT
relaxes `BR-CL-23` for *every* XRechnung scenario, so a unit code outside CEN's
Rec 20 table leaves an invoice valid as an XRechnung and invalid as a core
invoice — and the widening turned that into a proof of the opposite.
`an_xrechnung_invoice_can_be_core_invalid` is the witness.

All three impls are gone, and `tests/robustness.rs` asserts the surviving
guarantee over arbitrary generated documents rather than one fixture: *if a
conformant CIUS accepts it, core accepts it.*

---

## ⚡ Measured, not asserted

```text
validate/core/5                      1.94 µs      ← the 5-line invoice
validate/core/1000                 130.2  µs      ← linear in line count
profile/EN 16931/5                   1.76 µs
profile/XRechnung 3.0/5              6.30 µs
profile/XRechnung 3.0 Extension/5    5.09 µs
```

`cargo bench`. The target was *"well under 100 µs for a typical 5-line invoice
through the full core rule set"*; it is about 2 µs, and profile validation — 280
rules for XRechnung — is a handful.

The Extension being *faster* than the CIUS it extends is not a mistake: it runs
fourteen more rules and its documents trip fewer of them, and at this scale the
findings are a larger cost than the predicates.

Writing the benchmark immediately found a defect it existed to catch:
`Profile::validate` was **35 µs** on the same document the core rules took 1.5 µs
on, for a profile that adds *no rules at all*. Comparing two rule ids built up to
three `String`s, and profile validation does that once per rule per document.
Making the comparison allocation-free and skipping the intermediate `Vec` took it
to 2.3 µs — **15×**, and 35× for the Extension profile.

A performance claim in a README is worth exactly as much as the benchmark behind
it. This one had none until it had a bug.

---

## ⚖️ Deviations are allowed — and loud

Real counterparties demand them. A buyer who will not send BT-10 does not care
that `BR-DE-15` requires it, and refusing outright just pushes people to fork the
rule set or ignore the validator.

```rust
use en16931::{Invoice, profiles, validation::Check};

let report = Check::new(&profiles::XRECHNUNG)
    .without("BR-DE-15")             // the buyer will not send BT-10
    .run(&Invoice::default());

assert_eq!(report.suppressed(), ["BR-DE-15"]);
```

```text
XRechnung 3.0 validation (EN 16931-1:2017+A1:2019) — 280 rule(s) checked, 27 finding(s), INVALID
  ⚠ 1 rule(s) suppressed and NOT checked: BR-DE-15
```

The suppressed ids are on the report, printed by `Display`, carried in the JSON,
and `rules_checked` drops from 282 to 281 — so a stored report cannot overstate
what ran.

It drops by **one**, because one check was actually removed. It used to drop by
`suppressed.len()`, which counted *requests*: asking to skip `BR-DE-15` against
the bare core profile, or naming a rule that resolves to nothing, deducted from a
count of checks that were never going to run. A number that can be wrong in the
reassuring direction is worse than no number.

**And a deviated run cannot produce a proof.** `Check::prove` refuses:

```text
Check::new(&profiles::XRECHNUNG).without("BR-DE-15").prove::<XRechnung>(inv)
// Err(ProveError::Suppressed(["BR-DE-15"]))
```

That is the `XRECHNUNG_CVD` lesson at runtime. A rule set with a hole may accept
documents the full set rejects, so a `Validated<P>` derived from it would claim
something untrue. `Validated<P>` means *the whole rule set passed* — if it could
also mean *most of it*, no consumer could rely on it and the type would be
decoration.

---

## 🧭 The other crates

[`en16931-formats`](../en16931-formats) carries the
syntax layer — the UBL 2.1 and CII bindings in both directions, the 1 339 syntax
rules, the XRechnung CIUS, and ZUGFeRD / Factur-X hybrid PDFs — behind features,
so a consumer that wants UBL does not compile a PDF parser. It is what parses an
inbound document into the `Invoice` these rules run against, and what turns a
`Validated<P>` back into bytes.

**One crate there, not three.** XRechnung is carried in UBL *and* CII, and every
ZUGFeRD payload is CII: a crate per format would need the CII binding twice, and
two bindings drift. Cargo features already express which syntax a consumer wants.

**But it is a separate crate from this one, and that boundary is load-bearing.**
It depends on `en16931`, so rustc forbids the reverse: "the semantic rules do not
depend on a syntax" is enforced rather than promised. The payoff is measurable —
this crate's graph is **10 crates** and builds for `wasm32`; adding an XML parser
would end the first claim and a PDF parser takes it to **57 crates** and breaks
the target outright.

The two meet at exactly two places. `Validated<P>` is one: `ubl::write_validated`
demands the proof, so an unchecked invoice cannot be serialised. The other is
`ubl::to_string_for(&invoice, &XRECHNUNG)`, which validates on the spot and hands
back `Result<String, NotValid>` — for when the profile is a runtime choice, as it
is whenever a counterparty's preferred CIUS comes out of a database. Neither can
produce a document whose BT-24 disagrees with the rules that were run.

**And [`en16931-cli`](../en16931-cli)**, if the question is about a file rather
than about a type:

```console
$ en16931 validate rechnung.xml
$ en16931 explain BR-CO-14
```

One binary, everything above turned on, and exit `0` / `1` / `2` for *valid* /
*invalid* / *unreadable*. It may enable every feature precisely because nothing
depends on it: the graph discipline in this README exists to protect a
consumer's dependency tree, and a binary is in nobody's.

---

## 📤 A report you can store, diff and ship

Two shapes: a versioned JSON one, and — behind `features = ["svrl"]` — **SVRL**,
which every Schematron tool in this field already speaks.

```xml
<svrl:schematron-output title="EN 16931 — XRechnung 3.0" schemaVersion="EN 16931-1:2017+A1:2019">
  <svrl:failed-assert id="BR-02" flag="fatal" location="BT-1" test="en16931:BR-02">
    <svrl:text>An Invoice shall have an Invoice number (BT-1).</svrl:text>
  </svrl:failed-assert>
  <svrl:failed-assert id="BR-CL-25" flag="fatal" location="BG-7/BT-49" test="en16931:BR-CL-25">
    <svrl:diagnostic-reference diagnostic="en16931-hint">9958 was DE:LID — the Peppol
      Leitweg-ID scheme, and has been withdrawn; use 0204 instead</svrl:diagnostic-reference>
    <svrl:text>Endpoint identifier scheme identifier MUST belong to the CEF EAS code list.</svrl:text>
  </svrl:failed-assert>
</svrl:schematron-output>
```

`svrl:text` stays **byte-identical to the authority's wording** — that is what
makes a finding look up in CEN's or KoSIT's index. This crate's own advice goes
in Schematron's supplementary-text element instead, so a consumer that does not
know about it ignores it and loses nothing. See
[Hints](#-hints--the-sentence-that-ends-the-investigation).

That feature **adds no dependencies**. SVRL is a report format, not an invoice
syntax: writing it needs escaping, not a parser, and no UBL or CII element names.
The "no XML" rule is about never learning a syntax binding — this crate still
could not parse an invoice if it wanted to.

`location` carries a business-term path rather than an XPath, and the output says
so in a comment: there is no source document to point into. Everything reading
SVRL for *which rules failed and why* works unchanged.


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
  "attribution": "implementation of the EN 16931-1 semantic data model; …",
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
      "location": "BT-1", "text": "An Invoice shall have an Invoice number (BT-1)." },
    { "rule": "BR-CL-25", "severity": "fatal", "source": "artefact",
      "location": "BG-7/BT-49",
      "text": "Endpoint identifier scheme identifier MUST belong to the CEF EAS code list.",
      "hint": "9958 was DE:LID — the Peppol Leitweg-ID scheme, and has been withdrawn; use 0204 instead" }
  ]
}
```

`ValidationReport` derives `Serialize`, but that shape is the crate's *internals*
and changes when they do. `Report` is a separate, versioned shape — the internal
layout may change freely, this one only by bumping `schema`.

**It is designed against SVRL rather than invented.** Every Schematron tool in
the field emits `<svrl:failed-assert id flag location>` with a `<svrl:text>`, and
each maps one-to-one, so an `en16931-svrl` crate is a rename rather than a
translation. One field deliberately differs: SVRL's `location` is an XPath into a
serialised document, and this crate's is a **business-term path**
(`BG-25[2]/BT-151`). A crate holding the XML can map BT → XPath; the reverse is
lossy, so the semantic form is the one worth storing.

Three things travel with it that SVRL has no room for: the **provenance** of each
rule — CEN's, a profile's, or this crate's — which **profile and edition**
produced the report, and the **authority releases** those rules were verified
against.

That last one is per profile rather than per crate, and the reason is
`BR-DE-15`. It is KoSIT's rule, it moves on KoSIT's release cadence, and a
report stamped only `validation-1.3.16` beside it is naming CEN for something
CEN never published. An XRechnung report cites four releases because its rules
come from four: CEN's model, KoSIT's Schematron, KoSIT's *validator
configuration* — a separate release that decides the severities — and
OpenPeppol's, 31 of whose rules XRechnung merges in.

A stored report that cannot say what it was checked against is close to useless
six months later; one that says the wrong thing is worse.

---

## 🔎 Every finding can be explained

```rust
use en16931::validation::rules::{explain, explain_restriction};

assert_eq!(explain("BR-CO-3").map(|r| r.id.as_str()), Some("BR-CO-03"));   // padding
assert_eq!(explain("BR-IG-8").map(|r| r.id.as_str()), Some("BR-AF-08"));   // family alias
assert!(explain("PEPPOL-EN16931-R120").is_some());
assert!(explain_restriction("br-de-3").is_some());                          // a restriction
```

Rules are data — id, severity, provenance, the business terms they touch, and
the standard's own wording — so the registry is listable, filterable and
explainable. `touching(BtId(117))` answers "which rules constrain BT-117".

`explain` used to search the **core set only**, so an ordinary XRechnung report
citing `BR-DE-16` or `PEPPOL-EN16931-R120` resolved to `None` for every id in it.
A registry that cannot explain its own findings is not a registry. It now
searches every rule the crate ships, and `explain_restriction` covers the
profile restrictions, which are data rather than predicates and so have no
`Rule` to hand back.

---

## 🔒 Invariants survive `serde`

```text
serde_json::from_str::<InvoiceAmount>(r#""1.234""#).is_err()   // three decimals
serde_json::from_str::<Date>(r#""2026-06-30T12:00:00Z""#).is_err()  // an instant
```

A derived `Deserialize` rebuilds private fields **without calling the
constructor**, which would make "types enforce representability" true everywhere
except the one boundary where untrusted data arrives. The types with invariants
use `#[serde(try_from = …)]` so deserialisation re-runs the check.

That was advertised in `Cargo.toml` and untested. Writing the test found that
`Attachment` enforced nothing at all: the docs said "mime code and filename
mandatory" per §6.5.11 and `Attachment::new` happily accepted `""` for both.
Nothing else caught it either — the only rule requiring a filename is
`UBL-DT-07`, a **syntax** rule this crate deliberately does not implement. It is
a `Result` now.

---

## 🛡️ It does not panic

```text
proptest! {
    #[test]
    fn validate_never_panics(inv in any_invoice()) { … }
}
```

A clearing platform does not choose its inputs. `tests/robustness.rs` generates
structurally valid, semantically absurd documents — `i64::MAX` amounts that
overflow when summed, zero base quantities that `R120` would divide by, empty
codes, thousands of lines — and asserts that validation terminates, never
panics, is **deterministic and stably ordered**, and never cites a rule id that
`explain()` cannot resolve.

`proptest` rather than `cargo-fuzz` on purpose: it runs in the ordinary suite on
every commit. A property that only runs when someone remembers is a property that
regresses.

---

## 📅 Editions are values, not crate versions

```rust
use en16931::{profiles, Edition};

assert_eq!(profiles::XRECHNUNG.edition, Edition::En2017A1);
assert_eq!(Edition::En2017A1.designation(), "EN 16931-1:2017+A1:2019");
```

EN 16931-1:2026 is published and 2017 formally withdrawn — but every deployed
validator (XRechnung 3.0.2, Peppol BIS 3.0, ZUGFeRD 2.x) is a usage
specification of **2017+A1:2019**. Leading with :2026 would produce a crate that
fails all of them.

So the edition is a property of the *profile*, and a document declares its
profile in BT-24 — which means `for_specification_id` recovers the edition from
the document itself. When XRechnung 4.0 arrives it is a new `Profile`, a *minor*
release, not a new crate.

`Edition::En2026` exists as a classification and **no profile declares it**: a
test fails the build if one does without a rule set to go with it. The
term-level half — which business terms :2026 introduces, and the
`EN-EDITION-01` rule that forbids populating them under a 2017 profile — waits
on the normative text. Writing that map from memory is how you ship a validator
that is confidently wrong.

---

## 🔗 Optional: stronger payment identifiers via [`sepa`]

`BR-DE-19` and `BR-DE-20` say BT-84 and BT-91 *"should contain a valid IBAN"*.
By default that check is ISO 7064 mod-97-10 — correct, and **blind to length**:
a 21-character German IBAN with consistent check digits passes, and no German
bank will take it.

```toml
en16931 = { version = "0.1", features = ["sepa"] }
```

turns it into the full ISO 13616 registry — 89 countries, each with its own
length and BBAN structure — and adds `EN-SEPA-01`, this crate's own warning that
BT-90 is a well-formed EPC AT-02 creditor identifier. **No rule anywhere checks
that**, yet a direct debit quoting a malformed creditor identifier is rejected by
the bank long after the invoice was accepted.

Off by default, because the default build is `rust_decimal` + `thiserror` and
nothing else, and [`sepa`] brings `quick-xml` — which this crate otherwise goes
to some lengths not to have.

[`sepa`]: https://crates.io/crates/sepa

---

## 🏷️ Code lists — 4 887 values, generated and re-verified

Eighteen lists, from UNCL 5305's ten VAT categories to UN/ECE Rec 20's 2 162
unit codes. All generated from the pinned CEN artefacts by `cargo xtask codegen`, all
re-checked against them by `tests/codelists.rs`, and all re-derived in CI by
`cargo xtask check` so they cannot drift from the artefacts they came from.

The generator refuses to guess. `BR-CL-01`'s test is a *disjunction* carrying two
different lists, so the table declares which branch it wants and the generator
fails if the shape changes. `BR-CL-08`'s UNCL 4451 is bound differently by each
of CEN's three syntaxes — EDIFACT 381 codes ⊂ UBL 383 ⊂ CII 401, three frozen
UNTDID directory revisions — so the generator checks that they still form a
chain, takes the union, and stops outright if one binding ever gains a code
another dropped.

```rust
use en16931::codes::{contains, generated::UNIT_CODES};
use en16931::VatCategory;

assert!(contains(UNIT_CODES, "KWH"));
assert!(!contains(UNIT_CODES, "kwh"));   // §6.5.8: codes are entered exactly

assert_eq!(VatCategory::from_code("AE"), Some(VatCategory::ReverseCharge));
assert_eq!(VatCategory::from_code("ae"), None);
```

`VatCategory` carries the semantics the rules branch on:

```rust
use en16931::VatCategory;

// Both carry zero tax, but Z FORBIDS an exemption reason and E REQUIRES one.
assert!(VatCategory::ZeroRated.forbids_exemption_reason());   // BR-Z-10
assert!(VatCategory::Exempt.requires_exemption_reason());     // BR-E-10

// B is the only category with neither rule — and unlike AE, it is taxed.
assert!(VatCategory::SplitPayment.carries_tax());

// Only O suppresses the LINE rate (BT-152). BT-119 is a different term.
assert!(!VatCategory::OutOfScope.states_rate());
```

### Why the generator is paranoid

A Schematron `test` is a **program**, not a data structure. Three of the eighteen
tables cannot be read off it directly, for three different reasons:

- **`BR-CL-01`** is a disjunction over `self::` — **50** codes for
  `cbc:InvoiceTypeCode`, **13** for `cbc:CreditNoteTypeCode`. They overlap in
  exactly one code (`81`) and are disjoint on `380`/`381`.
- **`BR-CL-10`** is the ISO 6523 list **plus** a *contextual* literal: `SEPA` is
  admissible only on a party identification under `cac:AccountingSupplierParty`
  or `cac:PayeeParty`. It therefore belongs in the rule, not in a flat table.
- **`BR-CL-08`** is not in the code-list file at all — UBL embeds BT-21 in the
  note text (`#AAI#the text`), so the list lives in the preprocessed binding. And
  the three syntaxes disagree: EDIFACT 381 ⊂ UBL 383 ⊂ CII 401, three frozen
  UNTDID directory revisions. A syntax-independent crate cannot know which
  syntax an invoice will be written to, so it takes the union — rejecting a code
  a CEN binding accepts would be a false positive on a lawful invoice. The
  generator verifies the chain is still nested and stops if it ever breaks.

An extractor that reads the first `contains(…)` and stops is confidently,
precisely wrong on the first two. That mistake was made during this crate's
design and produced an incorrect bug report against an upstream project — so the
defence is a test, not a promise to be careful. The UNCL 4451 list was very
nearly written from memory during the *last* pass of this crate's development;
it would have been wrong in both directions.

---

## 🚀 Examples

```sh
cargo run --example validate_an_invoice                    # build one, validate, read the report
cargo run --example build_and_reconcile                    # lines in, BG-23 and BG-22 derived
cargo run --example profiles_and_proofs                    # every profile, and the typed proof
cargo run --example report_formats --features serde,svrl   # JSON and SVRL output
```

---

## 🧰 Development

[`just`](https://just.systems) is the task runner; `just` on its own lists every
recipe. There are **no shell scripts** — fetching and generating are `cargo
xtask` subcommands, so they are compiled, type-checked and linted like the rest
of the crate and behave the same on every platform CI uses.

```sh
cargo xtask fetch        # → spec/ (gitignored, ~136 MB); the suites become live
cargo xtask codegen      # regenerate src/codes/generated.rs; review the diff
cargo xtask check        # fail if the committed file no longer matches
just ci                  # everything CI runs, locally
```

### Minimum supported Rust version

**1.88** — the rule code uses `let`-chains. Measured, not declared: 1.87 fails,
and CI reads the number from `Cargo.toml` rather than repeating it.

### The artefacts

`spec/` is **not committed**: the CEN artefacts are EUPL-1.2, a reciprocal
licence, and keeping them out is what keeps this crate `MIT OR Apache-2.0` —
which is also why `deny.toml`'s allow-list does not mention EUPL.

The fetch pulls four repositories and nothing else: the CEN validation
artefacts, Peppol BIS Billing 3.0, and KoSIT's XRechnung Schematron and
validator configuration. It does **not** fetch specification PDFs — including
EN 16931-1 itself, whose full English text ÚNMS SR publishes openly. Reading the
standard is a research task, not a build step; `spec/README.md` lists the routes.

| | Pinned at |
|---|---|
| `ConnectingEurope/eInvoicing-EN16931` | `validation-1.3.16` |
| `itplr-kosit/xrechnung-schematron` | `v2.5.0` — its changelog says *"compatible with XRechnung 3.0.x"* |
| `itplr-kosit/validator-configuration-xrechnung` | `v2026-01-31` |
| `OpenPEPPOL/peppol-bis-invoice-3` | `v3.0.20` |

**All four are release tags, and three of them used not to be.** Tracking
`master` is not merely irreproducible; an authority's `master` is its *next*
release. When this was fixed, KoSIT's validator-configuration branch carried two
`customLevel` overrides — `CII-SR-465`, `CII-SR-466` — that appear in no
published release. A crate whose central claim is that it reports rules at the
severities the authorities *publish* was reading severities nobody had published.

Each profile declares which of these its rules were checked against, and that
list travels in every report — see [above](#-a-report-you-can-store-diff-and-ship).
`tests/artefact_pin.rs` asserts every declared ref is one `xtask` actually
fetches, so a profile cannot cite a release the suites never ran on.

Pins are **fully-qualified refs**, and that is not pedantry: `eInvoicing-EN16931`
publishes `validation-1.3.16` as both a tag *and* a branch pointing at different
commits, and `git clone --branch` prefers the branch — so two clones of the same
"pin" produced different trees and different code lists.

### The code lists are generated, not written

`src/codes/generated.rs` holds **4 887 values across 18 tables**. The generator
**fails rather than guesses**: a Schematron `test` is a program, not a data
structure, and `BR-CL-01` alone carries two different lists in one disjunctive
expression — 50 invoice type codes and 13 credit-note ones. An extractor that
reads the first and stops is confidently wrong. So every table declares how its
list is selected, and a rule that changes shape stops the build.

It also *proves* claims made in comments elsewhere: that Peppol's `UNCL5189`
really is identical to the CEN table, and that the three CEN bindings' UNCL 4451
lists (381 / 383 / 401 codes) really are nested — so taking their union is
directory drift and not a divergence being papered over. `cargo xtask check`
runs in CI, so none of it can quietly rot.

---

## ⚖️ Licence

MIT OR Apache-2.0, at your option.

### Attribution

> implementation of the EN 16931-1 semantic data model; © CEN, used under the 2018 CEN–EC licence agreement

This crate is an implementation of the semantic data model of EN 16931-1 and of
the two mandatory syntaxes listed in CEN/TS 16931-2. EN 16931-1 and
CEN/TS 16931-2 are made available free of charge by CEN and the European
Commission under their 2018 licence agreement, which permits derivative use on
condition that derivative applications carry a statement to this effect.
Copyright in the standard remains with CEN.

The notice above is `en16931::ATTRIBUTION`, and every `ValidationReport` carries
it verbatim. `tests/attribution.rs` asserts all three copies still agree —
because losing a licence condition by reformatting is the kind of mistake
nothing else in a build would catch.
