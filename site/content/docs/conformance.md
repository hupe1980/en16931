+++
title = "Conformance and performance"
weight = 9
description = "What is measured rather than claimed: CEN's unit tests, KoSIT's mutation suite, 59 published invoices, 500 documents round-tripped and crossed, and the benchmarks."
+++

The rule logic here is hand-written, so it is checked against the authorities'
own test suites, at pinned releases, on every change. Every suite summary on
this page is pasted from the suite's output and checked against it, and every
count in the prose is asserted by the suite that measures it; the benchmark
timings are measured, not asserted.

## CEN's unit tests

277 files and 1 131 assertions, each a minimal UBL fragment with an explicit
expectation. 1 027 CEN unit-test assertions run against the rules; 66 more test
rules the model's types retire, and are judged by what the reader reports
instead; the rest are syntax rules or values the model refuses:

```xml
<test>
  <assert><error>BR-01</error></assert>
  <Invoice> … no CustomizationID … </Invoice>
</test>
```

```text
CEN conformance suite
  files:                  277
  assertions:            1131
  run:                   1027
  agreed:                1016  (98.9% of run)
  disagreed:                0
  diverged, declared:      11
     6 × an empty <cac:PostalAddress/> is an absent address here
     2 × a <cac:BillingReference/> with no child is an absent BG-3 here
     1 × a <cac:InvoicePeriod/> with no dates is an absent BG-14 here
     2 × two cac:TaxTotal elements cannot both be BT-110
  type-retired, by the reader:   66  (the term reported missing exactly when the suite expects the rule)
  skipped, unevaluated:     0  (undecidable)
  skipped, syntax rules:   32  (UBL-*/CII-*, the formats crate's job)
  skipped, malformed:       6  (a value the model refuses)
  skipped, unreadable:      0
  run, no indicator:        8  (absent cbc:ChargeIndicator, read as an allowance)
  core rules with no CEN expectation: 35 of 227
```

Every expectation a test states is checked against the report of the **full**
rule set — a test may state several (`BR-29` and `BR-30` must both stay silent).
CEN's files say nothing about 35 of the 227 core rules, most of them the
`BR-DEC-*` decimal checks the model's amount types retire. CEN's fragments are
minimal by design, so what *other* rules do on them is not an expectation.

**Type-retired rules are checked through the reader.** Sixteen rules (`BR-12`
… `BR-46`, `BR-CL-03`) require a term the model cannot lack, so no invoice built
in code breaks them; a document can. CEN's 66 cases for them are judged by
whether the reader reports the term `missing` (or a currency `malformed`), and
all agree.

**Zero disagreements.** Each divergence is declared by file and rule with a
reason, and the list is asserted exactly: a divergence that starts agreeing
fails the build as a new one would. They have two causes:

- UBL can carry a group that is present and empty — `<cac:PostalAddress/>`,
  `<cac:BillingReference/>`, `<cac:InvoicePeriod/>` — and this model cannot. An
  address with no fields *is* an absent address; there is no third state.
- UBL permits two `cac:TaxTotal` elements in the same currency with different
  amounts. BT-110 is one field, so the contradiction cannot be written down —
  and `BR-CO-15` is then satisfied by whichever value was read. Peppol's `R053`
  is the rule that catches this, and it is a syntax rule about element counts.

## KoSIT's mutation suite

226 *complete, valid* XRechnung invoices with mutations embedded as processing
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
  instances:              226
  runnable mutations:     471
  run:                    381
  agreed:                 381  (100.0% of run)
  disagreed:                0
  skipped:                 90  (type-retired, not in the profile, or a value the model refuses)
```

## Published example invoices

The 59 complete invoices CEN and OpenPeppol ship as examples of correct usage. No
per-rule expectation, just a blunt one: *the authority publishes this as valid*,
so **nothing fatal may fire**.

```text
published example invoices
  checked:                 59
  valid:                   59
  skipped:                  2  (each named with its reason)
```

The skipped ones are named with their reason: one is a bug-report reproduction
rather than an example, and one fails CEN's own `BR-06`.

One excuse is narrower than a skip: CEN's 2017 Swedish Peppol samples carry
placeholder organisation numbers and GLNs whose check digits do not hold, so
Peppol's own `PEPPOL-COMMON-R049` and `R040` reject them, here as in Peppol's
validator. Those two findings are excused for those files by name; everything
else in them is still checked.

## The syntax corpora

The formats crate runs three properties over **500 published documents** — every
UBL and CII instance in the CEN, KoSIT and OpenPeppol trees.

The tree holds 326 UBL and 175 CII files; one UBL file is deliberately not
well-formed and is correctly refused. The properties run over the 500 that parse;
the [depth guard](@/docs/syntaxes.md#the-document-is-hostile-until-it-parses)
scans all 501 files.

| Suite | What it establishes |
|---|---|
| `roundtrip` | `Invoice → syntax → Invoice` is the identity in both syntaxes, reported per field |
| `fidelity` | the same property over all 500, where a difference is a failure *unless the writer reported it as an entry* |
| `cross_syntax` | read in one syntax, write in the **other**, read back — over the same 500, both directions |
| `order` | every element either writer emits is in schema sequence |
| `subset` | neither writer emits a forbidden element or attribute |
| `corpus` | every published UBL and CII instance read; every unmapped element named |
| `schema_gate` | every document the suites write — fixtures, profile-scoped writes, every published document rewritten or converted — validates with `xmllint`: CII against CEN's D16B schema, UBL against the OASIS UBL 2.1 `Invoice` or `CreditNote` schema; skipped with a printed line without it, a failure in CI |
| `zugferd_embed` | every PDF `zugferd::embed` writes — both profiles, into blank PDF/A-3b files of four shapes and into the ZUGFeRD corpus's PDF/A-3 samples with their own invoice removed — passes veraPDF PDF/A-3b and reads back with no divergence; skipped with a printed line without Docker, a failure in CI |

Each finds a class the one before it cannot: `roundtrip` works on a fixture this
crate wrote, `fidelity` on documents it did not, and `cross_syntax` on a binding
that is *consistently* wrong — see [what crossing the syntaxes
proves](@/docs/syntaxes.md#what-crossing-the-syntaxes-proves). Every defect of
that class produces a schema-valid document the reader reads without complaint,
which is why the property is asserted over a corpus rather than argued from the
code.

`schema_gate` closes the gap those leave: the round trips read back by local
name, so a permitted element with the wrong child type or namespace survives
all of them. Written UBL is validated against the OASIS UBL 2.1 schemas, pinned
by the SHA-256 of OASIS's archive.

## The readers, over documents somebody else's tooling mangled

The 500 published documents are all *well-formed by construction* — the
authorities publish invalid **documents**, not broken **files**. A truncated
stream, a duplicated element, a value no producer would emit: none of that is in
any corpus, and all of it arrives from a real counterparty.

`reader_robustness` mutates this crate's own complete fixture in both syntaxes
and presses on through everything a caller does next — validate, write, read
back:

| | Covers |
|---|---|
| random single and triple mutations | truncation, dropped characters, duplicated elements, renamed tags, deep nesting, an injected doctype — **structural** corruption |
| a dense deterministic sweep | **every** value replaced with each of 16 adversarial strings |

The dense sweep does the work. A complete invoice has around a hundred value
nodes, so random targeting reaches any given converter a few per cent of the
time; replacing every value guarantees each converter meets each input.

It asserts that the mutations **reach** the reader. That the reader *reports*
what it refuses is checked one element at a time by `reader_absence`: every
element of the complete invoice and credit note, in both syntaxes, gets an
unholdable value, an invalid and a foreign `currencyID`, and is deleted. Each
case must keep the value, invent none, or be reported:

```text
reader sweep
  UBL invoice:            680
  CII invoice:            650
  UBL credit note:        675
  CII credit note:        641
  cases:                 2646  (value, currency and absence of every element)
  excused:                 18  (each with its reason)
  silent:                   0
```

Each exception names why it is no term (for example `ram:Name` beside BT-11);
a stale one fails the suite.

Coverage-guided fuzzing runs beside it in CI: 4 `cargo-fuzz` targets, 60
seconds each on every change — any bytes through `read`; a PDF through
`extract` under small limits, asserting what comes back stays within them; a
generated invoice written as UBL and as CII and read back, which must be
**unchanged unless the writer or the reader reported something**; and a fuzzed
PDF/A through `embed`. Every defect they find becomes a named regression test.

## The ZUGFeRD corpus

The 151 PDFs of the public ZUGFeRD corpus are extracted one by one, and the
outcome is asserted **per file**: 148 come back with an invoice, and each one
that does not is named with its reason — a PDF with nothing attached,
Mustang's deliberately misnamed `factur-y.xml`, and Mustang's sample declaring
the encoding `UTF-8-W/O-BOM`, which is refused by name rather than decoded on a
guess. A change that loses one file, or makes a refused one extract, fails naming it.

## Every source is pinned to a release

| | Pinned at |
|---|---|
| `ConnectingEurope/eInvoicing-EN16931` | `validation-1.3.17` |
| `itplr-kosit/xrechnung-schematron` | `v2.6.0` — its changelog says *"compatible with XRechnung 3.0"* |
| `itplr-kosit/validator-configuration-xrechnung` | `v2026-08-31` |
| `OpenPEPPOL/peppol-bis-invoice-3` | BIS Billing 3.0.21-hotfix, commit `5df0eff7c8efafd7c79c0dacce55082d175db759` (tag `3.0.21`) |
| `ZUGFeRD/corpus` | commit `82e886fb21369e3e83c1e97170a4ff406deceee7` (Apache-2.0) |
| OASIS UBL 2.1 (`docs.oasis-open.org`) | archive SHA-256 `60b80d76394a8a2add90723ecb8e0e2e9d826775de9749df37a72d60703f86ed` — the schemas the UBL schema gate validates against |

A release is an authority's normative publication, tagged or not: the tag where
there is one, the commit otherwise. Peppol re-pointed its `3.0.21` tag to the
3.0.21-hotfix, so its pin is the commit the tag names, and the weekly watch
checks the tag still names it. OASIS publishes UBL 2.1 as a download, so its pin is the archive's
SHA-256. Never a branch: **an authority's `master` is its next release**, and
this project reports rules at the severities the authorities *publish*. The
fetch fails unless a commit-pinned checkout is that commit. A
KoSIT release that moves to XRechnung 4.0 is a new profile, not a newer pin.

Each profile declares which of these its rules were checked against, and that
list travels in every report — including the JSON one you store. A test asserts
every declared ref is one the fetch actually clones, so a profile cannot cite a
release the suites never ran on.

## Why the artefacts are not committed

The CEN artefacts are EUPL-1.2, a reciprocal licence, and keeping them out of the
tree is what keeps every crate here `MIT OR Apache-2.0`. The suites that need
them skip without them, and CI sets `EN16931_REQUIRE_SPEC=1` so a skip *there* is
a **failure**.

## The documentation is tested too

A test walks every README, every source file and every page of this site, finds
each measured figure in its sentence, and compares it against the value the code
produces. See [contributing](@/docs/contributing.md#the-documentation-is-tested).

## Measured, not asserted

```text
validate/core/5                      1.50 µs      ← the 5-line invoice
validate/core/1000                 137.4  µs      ← linear in line count
profile/EN 16931/5                   1.57 µs
profile/XRechnung 3.0/5              6.30 µs
profile/XRechnung 3.0 Extension/5    5.05 µs
```

`cargo bench`, on the maintainer's machine — the order of magnitude matters, not
the digits.

The Extension is faster than the CIUS it extends because its documents trip
fewer rules, and at this scale findings cost more than predicates. `en16931`
builds for `wasm32-unknown-unknown`, so validation can run in the browser.

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
the order of magnitude matters, not the digits. Reading is linear in line count.
The prohibition tables are indexed by the last segment of each path, and a test
compares the index against a linear scan over every path the tables describe.

## The generated tables

The workspace has 7 generated files, none written by hand: the code lists for
`en16931`, and the element-order, prohibition and single-occurrence tables for
each syntax in `en16931-formats`. `cargo xtask check` re-derives all 7 and
fails if a committed one differs, so a table cannot drift from its artefact.

The generators refuse to guess. `BR-CL-01`'s test is a *disjunction* carrying two
different lists, so the table declares which branch it wants and the generator
fails if the shape changes. `BR-CL-08`'s UNCL 4451 is bound differently by each of
CEN's three syntaxes — EDIFACT 381 codes ⊂ UBL 383 ⊂ CII 401, three frozen UNTDID
directory revisions — so the generator checks that they still form a chain, takes
the union, and stops outright if one binding ever gains a code another dropped.

## See also

- **[Contributing](@/docs/contributing.md)** — running all of this locally.
