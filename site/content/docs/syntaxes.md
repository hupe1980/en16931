+++
title = "Syntaxes: UBL and CII"
weight = 5
description = "Read, write and convert UBL 2.1 and UN/CEFACT CII D16B through the EN 16931 semantic model — one call per task, generated element order, 1 218 prohibitions, and nothing dropped silently."
+++

CEN/TS 16931-2 names exactly two mandatory syntaxes, and a receiver must accept
both:

- **UBL 2.1** — OASIS, `<Invoice>` and `<CreditNote>` as two different document
  elements.
- **UN/CEFACT CII D16B** — `<rsm:CrossIndustryInvoice>`, one document element for
  both, with the kind in BT-3.

`en16931-formats` binds the semantic model to both, in both directions, and
re-implements not one of the 334 business rules. `en16931` decides whether an
invoice is *correct*; this crate decides what it looks like *on the wire*.

## Features, and what each one costs

| Feature | Default | Graph | What |
|---|---|---|---|
| `ubl` | ✅ | 13 crates | UBL 2.1 `Invoice` / `CreditNote`, both directions |
| `cii` | — | 13 crates | UN/CEFACT CII D16B, both directions |
| `zugferd` | — | **57 crates** | [ZUGFeRD / Factur-X](@/docs/zugferd.md) hybrid PDFs — reading, and embedding into PDF/A-3 |
| `serde` | — | + `serde` | `Serialize` / `Deserialize` on this crate's own types |

`zugferd` is off by default and that matters: the PDF layer brings AES,
ChaCha20, SHA-2, `getrandom` and `libc`, and the result does not build for
`wasm32-unknown-unknown`. Nobody reading a UBL invoice should pay for that.

The writers have **no dependency at all** — writing XML is escaping and ordering.
Only reading pulls in a parser.

## Reading

One call, from bytes, whatever the document is:

```rust
let read = en16931_formats::read(&bytes)?;

read.invoice;          // the semantic model
read.syntax;           // Syntax::Ubl or Syntax::Cii — the document's own
read.entries;          // everything the reader had to say, typed
read.pdf;              // for a ZUGFeRD / Factur-X PDF: profile, XMP, divergences
```

The syntax comes from the bytes — a PDF signature, else the document element
(by name *and* namespace) — so there is nothing to choose and nothing to get
wrong. The bytes may be UTF-8, UTF-8 with a byte-order mark or UTF-16 with one,
the encodings XML 1.0 obliges a reader to accept; any other, declared or
detected, is refused by name rather than guessed.

Every entry has a kind, the syntax path it is about, the business term where the
binding knows it, the value, the artefact rule it cites and, where the remedy is
known, a hint:

| Kind | Means |
|---|---|
| `Unmapped` | an element outside the EN 16931 subset — another CIUS's addition, an extension |
| `Malformed` | present, and not representable — a third decimal, a timestamp where a date belongs, `abc` as a quantity, an amount in another currency than BT-5 |
| `Missing` | a term the model requires that the document does not carry — no `ram:CalculatedAmount` in a VAT breakdown; the field holds a placeholder |
| `Duplicate` | a second occurrence of an element the schema (or a CEN syntax rule) allows once; the first is read |
| `Repeated` | the same, where CEN publishes the rule as a warning |
| `Schema` | a spelling the schema does not admit, read anyway; writing, a required element the model lacks |
| `Unreadable` | the bytes are not an invoice at all — not well-formed, an encoding refused, nested too deep, a PDF refused |
| `Container` | a hybrid PDF's metadata disagrees with its payload without changing what it declares |
| `Ambiguous` | the PDF's XMP profile and the payload's BT-24 disagree |
| `Dropped` | writing: a term the target document has no place for |
| `Prohibited` | writing: an element CEN's syntax rules fence out of the EN 16931 subset |
| `Derived` | writing: a value the syntax requires and the model lacked, derived |

`Unmapped`, `Malformed` and `Missing` are not errors on their own, and none is silent.

**A reader never invents a value.** A required term the document lacks gets a
placeholder and a `Missing` entry (`BG-23[1]/BT-117`), so `receive` rejects it;
a number that does not parse is `Malformed`, never zero. What is required
follows the declared profile: ZUGFeRD MINIMUM lawfully has no BT-106.

`Display` on an entry is the one text rendering:

```text
malformed    …/ChargeIndicator/Indicator (BG-20) = "yes" — not an xs:boolean [hint: the value must be `true` or `false`]
```

To validate as well, and get a decision, read with
[`receive`](@/docs/validation.md) instead.

### The document is hostile until it parses

| | |
|---|---|
| **entity expansion** (billion laughs) | needs a DTD; the parser rejects every document carrying one |
| **external entities** (XXE, file disclosure) | the same DTD refusal, for the same reason |
| **nesting** | refused past `MAX_DEPTH` **before** parsing |

The XML parser recurses once per level, and a stack overflow is not a panic:
Rust can neither unwind nor catch it, so the process would abort. Nesting is
therefore measured in one linear scan of the bytes first, and a document past the
limit is `Error::TooDeep`. The limit is 64; the deepest of the 501 published
instances is **9**, and the corpus suite fails if an artefact release ships
anything within three times the limit.

Input size and time are deliberately *not* bounded here. A caller reading from a
socket owns that decision, and a library that quietly capped it would be wrong
for the batch job and useless for the endpoint.

### The schema is wider than the model, and the reader reads the schema

An inbound document is valid against **UBL's XSD**, not against this crate's
idea of a value. `cbc:IssueDate` is `xs:date`, whose lexical space ends with an
optional time zone:

```xml
<cbc:IssueDate>2026-07-31+02:00</cbc:IssueDate>
```

Schema-valid, objected to by no Schematron rule, and written by default by
Java's `XMLGregorianCalendar`. `Date` holds a calendar day and nothing else,
because EN 16931-1 §6.5.9 has no term for a time zone, so the zone is **dropped,
not applied**: `2026-07-31+02:00` *is* the day 2026-07-31.

`cbc:ChargeIndicator` is `xs:boolean`, whose lexical space is
`{true, false, 1, 0}`, and it decides whether an amount is **added to or
subtracted from** the invoice. Both readers take all four forms; an indicator
that is not a boolean is reported as `Malformed`, never read as an allowance.

## One business term, two code lists: BT-8

The VAT point date code (BT-8) has three meanings in EN 16931 — invoice issue
date, actual delivery date, paid to date — and CEN's two syntax bindings spell
them from different code lists:

| Meaning | Model and UBL (UNTDID 2005) | CII `ram:DueDateTypeCode` (UNTDID 2475) |
|---|---|---|
| invoice issue date | `3` | `5` |
| actual delivery date | `35` | `29` |
| paid to date | `432` | `72` |

The model always holds the UNTDID 2005 code, so `BR-CL-06` is one rule whichever
syntax a document came from. The CII reader translates on the way in and the
CII writer on the way out; a CII value outside `5 29 72` is reported as
`Malformed` and left out of the model — a `3` copied across would pass
`BR-CL-06` in a document CEN's CII rules reject — and a model value with no CII
spelling is not written and is reported as `Dropped`. CII repeats BT-8 in every
VAT breakdown group, and CEN's `CII-SR-462` requires the repetitions to agree: a
group that disagrees is reported as a `Duplicate` citing that rule, and the first
is read.

## Writing

```rust
use en16931::profiles::XRechnung;
use en16931::validation::profile::Validated;
use en16931_formats::Syntax;

let proof: Validated<XRechnung> = Validated::new(invoice)?;
let out = en16931_formats::write(&proof, Syntax::Ubl);

out.xml;
out.entries;          // e.g. a `Dropped` BT-9 on a credit note
```

`write` takes a **proof**, and stamps BT-24 from the profile that was actually
proved. Two things become impossible rather than discouraged: serialising an
unvalidated invoice, and a document claiming XRechnung 3.0 that was only checked
against the bare core model — the most common way an invoice passes local
validation and is rejected on receipt. Every written document comes back with
its entries: UBL's `<CreditNote>` has no `cbc:DueDate` and no
`cac:ProjectReference`, and dropping them is correct; dropping them *quietly*
means a payment due date vanishing between two systems with nothing in any log.

**A model nobody validated** — a document you did not author, on its way to
another syntax — goes through `write_unchecked`:

```rust
match en16931_formats::write_unchecked(&invoice, Syntax::Cii) {
    Ok(out) => submit(&out.xml),
    // The target schema has no form for this model — no BT-1, say. Nothing
    // is emitted that the schema would reject.
    Err(e) => eprintln!("{e}"),
}
```

It is the one way to write without a proof, and it refuses a model the target
syntax's schema cannot express, listing the `Schema` entries — so no public
function emits a document its schema rejects, and none returns one without its
report.

## The 91 % that costs a writer nothing

CEN's artefacts carry **1 346** syntax rules. **1 218 of them (91 %)** say some
element "shall not be used" — they fence off the parts of UBL 2.1 and CII D16B
that EN 16931 does not use. That inverts the usual expectation:

| | Rules that apply | Why |
|---|---|---|
| **Writer** | ~119 | It has no way to express `cbc:UUID` — the model has no term for it. The prohibitions are *unreachable*, not cheaply satisfied. |
| **Reader** | all 1 346 | The document came from somewhere else. |

Unreachability is a claim, so the serialiser enforces it against prohibitions
extracted from CEN's own Schematron, and a test asserts the writer never needs
that safety net.

**1 111 of the 1 218 are represented** — 664 of UBL's 696 and 447 of CII's 522.
The rest have a test an XPath engine is needed to evaluate (a predicate, a
wildcard, `ends-with(name(), 'Amount')`), and they are *counted* rather than
quietly omitted, so a test can report "1 111 of 1 218 checked" instead of
implying a clean sweep.

**The prohibitions are context-relative.** `CII-DT-076` is `not(ram:ID)`, and
it means the element that rule's context selects may not have one — not that no
document may contain `ram:ID`. Each entry carries `(rule, context, relative
path)`, taken from the **preprocessed** artefacts, where contexts are fully
resolved rather than left as `$Variable` references.

## The binding is data, not code

Six tables are **generated from the authorities' artefacts**, never transcribed:

| Table | Source | Size |
|---|---|---|
| UBL element order | **325** published UBL instances | 36 parents |
| CII element order | CEN's D16B subset schema, over the parents and children of **175** published CII instances | 38 parents |
| UBL prohibitions | preprocessed `EN16931-UBL-validation` | 1 548 paths + 21 attributes |
| CII prohibitions | preprocessed `EN16931-CII-validation` | 447 paths |
| UBL single occurrence | the OASIS UBL 2.1 schemas and CEN's `count(…) <= 1` rules | 253 rows |
| CII single occurrence | CEN's D16B subset schema and CEN's `count(…) <= 1` rules | 186 rows |

Both syntaxes' content models are XSD `sequence`s, so a document with exactly the
right elements **in the wrong order** is invalid — and no Schematron rule says
so, because ordering is the schema's job. CII's order is read from the D16B
schema CEN validates against, which the reference corpus carries. UBL's is
derived by topologically sorting the pairwise precedences observed across the
corpus, taking the majority direction where instances disagree (much of that
corpus is deliberately invalid). Derivation reports no unresolved conflicts,
and the generator exits non-zero if it ever does. Every document the test
suites write is then validated with `xmllint`: CII against the D16B schema, UBL
against the OASIS UBL 2.1 schemas.

The writers hand-sequence nothing: they emit in whatever order reads best, and
the serialiser sorts by the table — which also handles UBL's two document
elements disagreeing about where `cbc:TaxPointDate` goes.

## Converting between the two

Conversion goes *through the model*, never element by element — one call:

```rust
use en16931_formats::{Loss, Syntax};

let converted = en16931_formats::convert(&bytes, Syntax::Cii, Loss::Refuse)?;
converted.document;   // the CII
converted.report;     // the source, received as it declares itself
converted.written;    // what the writer said — a `Derived` BT-110, say
converted.lost;       // empty under Loss::Refuse
```

Anything that would not reach the target — an element the reader did not map, a
value it could not hold, a duplicate it did not read, a term the target cannot or
may not carry — refuses the conversion with `ConvertError::Lossy`, naming each
item, unless the caller passes `Loss::Allow`. Invalidity under the business
rules is not a refusal: the report travels with the result and says so.

```sh
en16931 convert rechnung.xml --to cii                # refuses a lossy conversion
en16931 convert rechnung.xml --to cii --allow-loss   # converts, and lists the loss
```

An element-by-element transform has to know 164 × 2 bindings and cannot say what
it lost. Going through the model means the same reader and writer everything else
uses, and the same typed entries.

## What crossing the syntaxes proves

A same-syntax round trip cannot see a binding that is *consistently* wrong;
crossing the syntaxes catches:

| | |
|---|---|
| **a term written to one syntax and not the other** | UBL carries BT-90 on the **seller** as `schemeID="SEPA"`, the one place `BR-CL-10` admits a non-ISO-6523 scheme, and CII does not. A reader that hops it into BG-19 and a writer that never hops it back lose it on every direct-debit invoice, and the counterparty reports `BR-DE-30`. |
| **a value one binding normalises and the other does not** | `BR-DE-18` needs BT-20's Skonto block to end with a newline, and XRechnung is carried in CII as well as UBL — a reader that trims the field breaks 36 documents. |
| **a signal read from too narrow a set** | detecting a CII credit note from `381` alone reads `396`, `532` and `83` back as *invoices*, and `BR-CL-01` then reports a violation that is not one. Every published CII credit note uses `381`, so no corpus shows it. |
| **an element that is two terms at once** | BT-111 and BT-110 are one element when BT-6 = BT-5 — which one reader can know and the other not. |

Both readers read an **empty element** (`<cbc:BuyerReference/>`) as an absent
term.

## See also

- **[ZUGFeRD / Factur-X](@/docs/zugferd.md)** — the hybrid PDF container.
- **[Conformance](@/docs/conformance.md)** — the corpora all of this runs against.
