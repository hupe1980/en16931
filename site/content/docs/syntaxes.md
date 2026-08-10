+++
title = "Syntaxes: UBL and CII"
weight = 5
description = "Read and write UBL 2.1 and UN/CEFACT CII D16B from the EN 16931 semantic model — generated element order, 1 218 prohibitions, and nothing dropped silently."
+++

CEN/TS 16931-2 names exactly two mandatory syntaxes, and a receiver must accept
both:

- **UBL 2.1** — OASIS, `<Invoice>` and `<CreditNote>` as two different document
  elements.
- **UN/CEFACT CII D16B** — `<rsm:CrossIndustryInvoice>`, one document element for
  both, with the kind in BT-3.

`en16931-formats` binds the semantic model to both, in both directions, and
re-implements not one of the 317 business rules. `en16931` decides whether an
invoice is *correct*; this crate decides what it looks like *on the wire*.

## Features, and what each one costs

| Feature | Default | Graph | What |
|---|---|---|---|
| `ubl` | ✅ | 13 crates | UBL 2.1 `Invoice` / `CreditNote`, both directions |
| `cii` | — | 13 crates | UN/CEFACT CII D16B, both directions |
| `zugferd` | — | **57 crates** | [ZUGFeRD / Factur-X](@/docs/zugferd.md) hybrid PDFs — **reading only** |
| `serde` | — | + `serde` | `Serialize` / `Deserialize` on this crate's own types |

`zugferd` is off by default and that matters: the PDF layer brings AES,
ChaCha20, SHA-2, `getrandom` and `libc`, and the result does not build for
`wasm32-unknown-unknown`. Nobody reading a UBL invoice should pay for that.

The writers have **no dependency at all** — writing XML is escaping and ordering.
Only reading pulls in a parser.

## Reading

```rust
let read = en16931_formats::ubl::from_str(&xml)?;

read.invoice;          // the semantic model
read.unmapped;         // elements outside the EN 16931 subset
read.malformed;        // present, but not representable
```

Or let the file say what it is:

```rust
match en16931_formats::sniff(&xml) {
    Some(Syntax::Ubl) => { /* … */ }
    Some(Syntax::Cii) => { /* … */ }
    None => { /* not an e-invoice this crate knows */ }
}
```

Both lists matter. `unmapped` is how you find out a counterparty is sending
something the standard has no term for; `malformed` is how you find out they are
sending something the model refuses at the boundary — a third decimal, a
timestamp where a calendar date belongs. Neither is an error on its own, and
neither is silent.

### The document is hostile until it parses

| | |
|---|---|
| **entity expansion** (billion laughs) | needs a DTD; the parser rejects every document carrying one |
| **external entities** (XXE, file disclosure) | the same DTD refusal, for the same reason |
| **nesting** | refused past `MAX_DEPTH` **before** parsing |

The third is the one that had teeth. `roxmltree` recurses once per level of
nesting and overflows the stack a few hundred levels in — and a stack overflow is
**not a panic**: Rust can neither unwind it nor catch it, so the process aborts.
Two lines of XML took down the caller, with nothing for `?` to catch.

It cannot be handled afterwards, so it is refused before: one linear scan of the
bytes, then `Error::TooDeep`. The limit is 64, the deepest of the 487 published
instances is **9**, and the corpus suite fails if a future artefact release ships
anything within three times the limit.

Input size and time are deliberately *not* bounded here. A caller reading from a
socket owns that decision, and a library that quietly capped it would be wrong
for the batch job and useless for the endpoint.

## Writing

```rust
let out = en16931_formats::ubl::write(&invoice);

out.xml;
out.dropped;          // e.g. BT-9 on a credit note
```

UBL's `<CreditNote>` has no `cbc:DueDate` and no `cac:ProjectReference`. Dropping
them is correct; dropping them *quietly* means a payment due date vanishing
between two systems with nothing in any log.

### Why `to_string` returns `String` and not `Result`

**Serialisation cannot fail, and that is a property of the model rather than an
omission.** Every field of `Invoice` already holds a value the syntax can carry:
`InvoiceAmount` cannot hold a third decimal, `Date` cannot hold something that is
not a calendar day — which is exactly what `udt:DateTimeString format="102"`
accepts, and nothing more. There is no state a writer could be handed that it
would have to refuse, and writing into a `String` does no I/O.

**Validity is a separate question, and it is the caller's.** An invoice with no
seller serialises perfectly into a document no counterparty will accept.

## Writing a proof, not a hope

```rust
use en16931::profiles::XRechnung;
use en16931::validation::profile::Validated;

let proof: Validated<XRechnung> = Validated::new(invoice)?;
let out = en16931_formats::ubl::write_validated(&proof);
```

`write_validated` stamps BT-24 from the profile that was actually proved. Two
things become impossible rather than discouraged: serialising an unvalidated
invoice, and a document claiming XRechnung 3.0 that was only checked against the
bare core model — the most common way an invoice passes local validation and is
rejected on receipt.

**When the profile is a runtime choice**, which is most of the time, the same
guarantee is available as a `Result`:

```rust
use en16931::profiles::XRECHNUNG;

match en16931_formats::ubl::to_string_for(&invoice, &XRECHNUNG) {
    Ok(xml) => submit(&xml),
    // Neither a panic nor a silent fallback: the report says exactly which
    // BR-DE rules the document still owes.
    Err(e)  => eprintln!("{e}\n{}", e.report()),
}
```

It validates, stamps BT-24 **from the profile**, and only then writes — in that
order, because a document validated carrying the caller's BT-24 and shipped
carrying the profile's was checked as something other than what it claims to be.

## The 91 % that costs a writer nothing

CEN's artefacts carry **1 339** syntax rules. **1 218 of them (91 %)** say some
element "shall not be used" — they fence off the parts of UBL 2.1 and CII D16B
that EN 16931 does not use. That inverts the usual expectation:

| | Rules that apply | Why |
|---|---|---|
| **Writer** | ~119 | It has no way to express `cbc:UUID` — the model has no term for it. The prohibitions are *unreachable*, not cheaply satisfied. |
| **Reader** | all 1 339 | The document came from somewhere else. |

Unreachability is a claim, so the serialiser enforces it against prohibitions
extracted from CEN's own Schematron, and a test asserts the writer never needs
that safety net.

**1 111 of the 1 218 are represented** — 664 of UBL's 696 and 447 of CII's 522.
The rest have a test an XPath engine is needed to evaluate (a predicate, a
wildcard, `ends-with(name(), 'Amount')`), and they are *counted* rather than
quietly omitted, so a test can report "1 111 of 1 218 checked" instead of
implying a clean sweep.

**The prohibitions are context-relative, and that is half the rule.**
`CII-DT-076` is `not(ram:ID)`, and it does *not* mean "no document may contain
`ram:ID`" — it means the element that rule's context selects may not have one. An
earlier table dropped the context, and the writer duly discarded every `ram:ID`
in the document. Each entry now carries `(rule, context, relative path)`, taken
from the **preprocessed** artefacts where contexts are fully resolved rather than
left as `$Variable` references.

## The binding is data, not code

Four tables are **generated from the authorities' artefacts**, never transcribed:

| Table | Source | Size |
|---|---|---|
| UBL element order | **319** published UBL instances | 36 parents |
| CII element order | **167** published CII instances | 38 parents |
| UBL prohibitions | preprocessed `EN16931-UBL-validation` | 1 548 paths + 21 attributes |
| CII prohibitions | preprocessed `EN16931-CII-validation` | 447 paths |

Both syntaxes' content models are XSD `sequence`s, so a document with exactly the
right elements **in the wrong order** is invalid — and no Schematron rule says
so, because ordering is the schema's job. The order is derived by topologically
sorting the pairwise precedences observed across the whole corpus, taking the
majority direction where instances disagree (much of that corpus is deliberately
invalid). Derivation reports no unresolved conflicts, and the generator exits
non-zero if it ever does.

The writers hand-sequence nothing: they emit in whatever order reads best, and
the serialiser sorts by the table. That made a class of bug structurally
impossible — UBL's two document elements disagree about where `cbc:TaxPointDate`
goes, and a hand-sequenced writer got it wrong.

## Converting between the two

Conversion goes *through the model*, never element by element:

```sh
en16931 convert rechnung.xml --to cii
```

An element-by-element transform has to know 164 × 2 bindings and cannot say what
it lost. Going through the model means the same reader and writer everything else
uses, and the same `dropped` report.

## What crossing the syntaxes proved

A same-syntax round trip cannot see a binding that is *consistently* wrong — a
writer and its own reader agreeing on a mistake round-trip perfectly. Crossing
breaks the agreement, and it found four bugs no other test could:

| | |
|---|---|
| **BT-90 was never written to UBL** | UBL carries the creditor identifier on the **seller** as `schemeID="SEPA"`, the one place `BR-CL-10` admits a non-ISO-6523 scheme. The reader hopped it into BG-19; the writer never hopped it back, so every direct-debit invoice written as UBL lost it and failed `BR-DE-30` at the counterparty. |
| **BT-20 was trimmed by the CII reader** | `BR-DE-18` needs the Skonto block to end with a newline, and XRechnung is carried in CII too — 36 documents. |
| **CII detected credit notes from `381` alone** | `396`, `532` and `83` read back as *invoices*, and `BR-CL-01` then reported a violation that is not one. Every published CII credit note uses `381`, so no corpus could have shown it. |
| **BT-111 vanished when BT-6 = BT-5** | One element is then both totals — which the UBL reader knew and the CII reader did not. |

Plus one the two readers simply disagreed about: an **empty element**.
`<cbc:BuyerReference/>` is an absent term, not a term whose value is the empty
string. Both readers said `None` through their `text()` helper and `Some("")`
through the dozen call sites that read an element's own text — inconsistent with
themselves, and with each other on nine documents.

## What is next

- **[ZUGFeRD / Factur-X](@/docs/zugferd.md)** — the hybrid PDF container.
- **[Conformance](@/docs/conformance.md)** — the corpora all of this runs against.
