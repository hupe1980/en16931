+++
title = "Conformance and performance"
weight = 9
description = "What is measured rather than claimed: CEN's unit tests, KoSIT's mutation suite, 58 published invoices, 486 documents round-tripped and crossed, and the benchmarks."
+++

The rule logic here is hand-written, so — unlike a Schematron-driven tool, which
is by construction exactly as correct as the artefact — it can be wrong in ways
theirs cannot. That is why the conformance corpora gate every release, and why
this page exists at all.

Every other test in this project was written by the same person as the code it
checks, from the same reading of the same documents. The ones below were not.

## CEN's unit tests

277 files, roughly 1 129 assertions, each a minimal UBL fragment with an explicit
expectation:

```xml
<test>
  <assert><error>BR-01</error></assert>
  <Invoice> … no CustomizationID … </Invoice>
</test>
```

```text
CEN conformance suite
  assertions:            1129
  run:                   1024
  agreed:                1013  (98.9% of run)
  disagreed:                0
  diverged, declared:      11
  skipped, unevaluated:    66  (type-retired or undecidable)
  skipped, syntax rules:   32  (UBL-*/CII-*, the formats crate's job)
  skipped, malformed:       7  (a value the model refuses at the boundary)
```

**Zero disagreements.** The eleven divergences are declared by file and rule with
a written reason, and the list is asserted *exactly* — so it can only shrink, and
a divergence that starts agreeing fails the build just as loudly as a new one.

They come from two causes, both of them the same kind of thing:

- **Nine cases**: UBL can carry a group that is present and empty —
  `<cac:PostalAddress/>`, `<cac:BillingReference/>`, `<cac:InvoicePeriod/>` — and
  this model cannot. An address with no fields *is* an absent address; there is
  no third state. Adding one would put a syntax artefact in every consumer's way
  to satisfy nine test cases.
- **Two cases**: UBL permits two `cac:TaxTotal` elements in the same currency
  with different amounts. BT-110 is one field, so the contradiction cannot be
  written down — and `BR-CO-15` is then satisfied by whichever value was read.
  Peppol's `R053` is the rule that catches this, and it is a syntax rule about
  element counts.

## KoSIT's mutation suite

224 *complete, valid* XRechnung invoices with mutations embedded as processing
instructions:

```xml
<?xmute mutator="remove" schematron-invalid="xrubl:BR-DE-15" ?>
<cbc:BuyerReference>90000000-03083-12</cbc:BuyerReference>
```

Remove BT-10 and `BR-DE-15` must fire. The `identity` mutation asserts the
*unmutated* invoice is clean, which catches rules that are too **eager** — the
failure mode a corpus of deliberately-broken documents structurally cannot see.

```text
XRechnung mutation suite
  runnable mutations:    449
  run:                   381
  agreed:                381  (100.0% of run)
  disagreed:               0
```

## Published example invoices

The 58 complete invoices CEN and OpenPeppol ship as examples of correct usage. No
per-rule expectation, just a blunt one: *the authority publishes this as valid*,
so **nothing fatal may fire**.

```text
published example invoices
  checked:  58
  valid:    58
```

Two are skipped by name and reason — one is a bug-report reproduction rather than
an example, and one fails CEN's own `BR-06`.

## Eight bugs the first run found

None of which any other test in this project could have found:

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
question they ask. The document kind is an explicit field now — which is also
what both syntaxes carry, and it makes `BR-CL-01` exact rather than permissive.

## The syntax corpora

The formats crate runs three properties over **486 published documents** — every
UBL and CII instance in the CEN, KoSIT and OpenPeppol trees.

**486, and elsewhere on this site 487.** Both are measured and they count
different things: the tree holds **320 UBL and 167 CII files**, and the reader
rejects one UBL file as not well-formed — correctly, it is a deliberately
malformed instance. The properties below run over the **486 that parse**; the
[depth guard](@/docs/syntaxes.md#the-document-is-hostile-until-it-parses) scans
all **487 files**, including the one that never becomes a document.

| Suite | What it establishes |
|---|---|
| `roundtrip` | `Invoice → syntax → Invoice` is the identity in both syntaxes, reported per field |
| `fidelity` | the same property over all 486, where a difference is a failure *unless the writer named it in `dropped`* |
| `cross_syntax` | read in one syntax, write in the **other**, read back — over the same 486, both directions |
| `order` | every element either writer emits is in schema sequence |
| `subset` | neither writer emits a forbidden element or attribute |
| `corpus` | all 319 UBL and 167 CII instances read; every unmapped element named |

Each finds a class the one before it cannot: `roundtrip` works on a fixture this
crate wrote, `fidelity` on documents it did not, and `cross_syntax` on a binding
that is *consistently* wrong — see [what crossing the syntaxes
proves](@/docs/syntaxes.md#what-crossing-the-syntaxes-proves). Every defect of
that class produces a schema-valid document the reader reads without complaint,
which is why the property is asserted over a corpus rather than argued from the
code.

## The readers, over documents somebody else's tooling mangled

The 486 published documents are all *well-formed by construction* — the
authorities publish invalid **documents**, not broken **files**. A truncated
stream, a duplicated element, a value no producer would emit: none of that is in
any corpus, and all of it arrives from a real counterparty.

`reader_robustness` mutates this crate's own complete fixture in both syntaxes
and presses on through everything a caller does next — validate, write, read
back:

| | Covers |
|---|---|
| random single and triple mutations | truncation, dropped characters, duplicated elements, renamed tags, deep nesting, an injected doctype — **structural** corruption |
| a dense deterministic sweep | **every** value replaced with each of sixteen adversarial strings |

The dense sweep does the work. A complete invoice has around a hundred value
nodes, so random targeting reaches any given converter a few per cent of the
time; replacing every value guarantees each converter meets each input.

Two numbers are asserted, because a fuzz suite that passes for the wrong reason
looks exactly like one that passes: how many mutations **reach** the reader, and
whether the reader **recorded** what it refused — which is what `unmapped` and
`malformed` are for.

## Every source is pinned to a release

| | Pinned at |
|---|---|
| `ConnectingEurope/eInvoicing-EN16931` | `validation-1.3.16` |
| `itplr-kosit/xrechnung-schematron` | `v2.5.0` — its changelog says *"compatible with XRechnung 3.0.x"* |
| `itplr-kosit/validator-configuration-xrechnung` | `v2026-01-31` |
| `OpenPEPPOL/peppol-bis-invoice-3` | `v3.0.20` |

Tags, not branches, for a reason sharper than reproducibility: **an authority's
`master` is its next release.** KoSIT's validator-configuration branch has
carried `customLevel` overrides — `CII-SR-465`, `CII-SR-466` — that appear in no
published release, and a project whose central claim is that it reports rules at
the severities the authorities *publish* must not read severities nobody has
published yet.

The pins name a version, not just a commit, because KoSIT states the
correspondence in its own changelog. Bumping one is a decision: a KoSIT release
that moves to XRechnung 4.0 is a new profile here, not a newer pin on the old
one.

Each profile declares which of these its rules were checked against, and that
list travels in every report — including the JSON one you store. A test asserts
every declared ref is one the fetch actually clones, so a profile cannot cite a
release the suites never ran on.

## Why the artefacts are not committed

The CEN artefacts are EUPL-1.2, a reciprocal licence, and keeping them out of the
tree is what keeps every crate here `MIT OR Apache-2.0`. The suites that need
them skip without them, and CI sets `EN16931_REQUIRE_SPEC=1` so a skip *there* is
a **failure**.

That flag matters more than it looks. A skipped conformance run and a passing one
are the same summary line, which is exactly how 486 unread documents stay green.

## The documentation is tested too

The numbers on this page are not typed in. A test walks every README, every
`lib.rs` and every page of this site, finds each figure in its sentence, and
compares it against the value the code produces — so a rule added tomorrow
fails the build in six files at once rather than making six documents quietly
wrong. See [contributing](@/docs/contributing.md#the-documentation-is-tested).

## Measured, not asserted

```text
validate/core/5                      1.50 µs      ← the 5-line invoice
validate/core/1000                 137.4  µs      ← linear in line count
profile/EN 16931/5                   1.57 µs
profile/XRechnung 3.0/5              6.30 µs
profile/XRechnung 3.0 Extension/5    5.05 µs
```

`cargo bench`, on the maintainer's machine — the useful information is the *order
of magnitude*, not the digits. Microseconds rather than the tens of milliseconds
a JVM plus an XSLT engine takes, and no process to start.

The Extension being *faster* than the CIUS it extends is not a mistake: it runs
fourteen more rules and its documents trip fewer of them, and at this scale the
findings cost more than the predicates.

Writing the benchmark immediately found a defect it existed to catch. Profile
validation was **35 µs** on the same document the core rules took 1.5 µs on, for
a profile that adds no rules at all: comparing two rule ids built up to three
`String`s, once per rule per document. Making the comparison allocation-free took
it to 2.3 µs — 15×, and 35× for the Extension profile. A performance claim is
worth exactly as much as the benchmark behind it, and this one had none until it
had a bug.

The number that matters for architecture is that validation is fast enough to run
on every keystroke of a form, and small enough to run in the browser: `en16931`
builds for `wasm32-unknown-unknown`, so an invoice never has to leave the client
to find out whether it is valid.

## Measured, not asserted — the syntax layer

```text
read/ubl/5              21.4 µs        write/ubl/5             67.5 µs
read/cii/5              26.5 µs        write/cii/5             65.6 µs
read/ubl/100           222 µs
read/cii/100           307 µs
read/ubl/1000            2.33 ms       write/ubl/1000          21.2 ms
read/cii/1000            2.92 ms       write/cii/1000          10.0 ms
convert/ubl-to-cii     158 µs          convert/cii-to-ubl     213 µs
```

`cargo bench -p en16931-formats --all-features`, on the maintainer's machine —
the useful information is the order of magnitude, not the digits. Reading is
linear in line count, which is the property that matters: a reader that goes
quadratic is fine on examples and dies on a telecoms bill with 5 000 call
records.

Writing runs the prohibition tables, which are indexed by the last segment of
each path — a linear scan of the 1 548 UBL entries, once per element emitted, is
what a document's write cost is otherwise made of. The old scan is kept as a
reference implementation and a test compares the two over every path the tables
describe: the prohibitions are what make *"the writer cannot emit a forbidden
element"* a property, so an optimisation there needs a proof rather than a
benchmark.

## The generated tables

Five files are generated from the artefacts and none is written by hand: the code
lists for `en16931`, and the element-order and prohibition tables for each syntax
in `en16931-formats`. `cargo xtask check` re-derives all five and fails if a
committed one differs, so a table cannot drift from the artefact it came from.

The generators refuse to guess. `BR-CL-01`'s test is a *disjunction* carrying two
different lists, so the table declares which branch it wants and the generator
fails if the shape changes. `BR-CL-08`'s UNCL 4451 is bound differently by each of
CEN's three syntaxes — EDIFACT 381 codes ⊂ UBL 383 ⊂ CII 401, three frozen UNTDID
directory revisions — so the generator checks that they still form a chain, takes
the union, and stops outright if one binding ever gains a code another dropped.

## What is next

- **[Contributing](@/docs/contributing.md)** — running all of this locally.
