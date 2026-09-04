# en16931-formats

**European e-invoicing formats**, on top of the
[`en16931`](../en16931) semantic model — UBL 2.1,
UN/CEFACT CII, XRechnung, and ZUGFeRD / Factur-X.

```text
   ┌──────────────┐   ┌─────────────┐
   │   billing    │   │   your ERP  │
   │ calculations │   │             │
   └──────┬───────┘   └──────┬──────┘
          │ adapter (feature)│
          └─────────┬────────┘
                    ▼
        ┌─────────────────────┐
        │      en16931        │   the semantic model. Complete on its own:
        │  semantic model     │   build an Invoice, get a verdict.
        │  validation engine  │   10 deps, no XML, no I/O, wasm32.
        │  proof of validity  │
        └─────────┬───────────┘
                  │  Validated<P> — the typed proof
                  ▼
        ┌─────────────────────┐
        │   en16931-formats   │   ← this crate. Only if you exchange
        │  UBL · CII · PDF/A  │     documents: parses inbound, writes
        │  1 339 syntax rules │     outbound, +1 339 syntax rules.
        └─────────┬───────────┘
                  │ ▲
                  ▼ │
   ┌──────────────────────────┐
   │   documents in / out     │
   │  UBL · CII · ZUGFeRD     │
   └──────────────────────────┘
```

`en16931` decides whether an invoice is **correct**. This crate decides what it
looks like **on the wire**, and re-implements not one of the 317 rules.

---

## 🧩 Why one crate, not one per format

XRechnung is carried in UBL *and* CII; every ZUGFeRD payload is CII. A crate per
format would need the CII binding twice, and two bindings drift. Cargo features
already express which syntax a consumer wants, so a crate boundary here would be
solving with a package what `--no-default-features` solves for free.

What *is* a separate crate is `en16931`, and that boundary is
load-bearing: this crate depends on it, so rustc forbids the reverse. "The
semantic rules do not depend on a syntax" is enforced rather than asked for, and
`en16931` keeps a **10-crate** graph that builds for `wasm32`.

---

## 📦 Features, and what each one costs

| Feature | Default | Graph | What |
|---|---|---|---|
| `ubl` | ✅ | 13 crates | UBL 2.1 `Invoice` / `CreditNote`, both directions |
| `cii` | — | 13 crates | UN/CEFACT CII D16B, both directions |
| `zugferd` | — | **57 crates** | ZUGFeRD / Factur-X hybrid PDFs — **reading only** |
| `serde` | — | + `serde` | `Serialize` / `Deserialize` on this crate's own types |

`zugferd` is off by default and that matters: `lopdf` brings AES, ChaCha20,
SHA-2, `getrandom` and `libc`, and the result does not build for
`wasm32-unknown-unknown`. Nobody reading a UBL invoice should pay for that.

The writers have **no dependency at all** — writing XML is escaping and
ordering. Only reading pulls in a parser.

---

## 🎯 The 91 % that costs a writer nothing

CEN's artefacts carry **1 339** syntax rules. **1 218 of them (91 %)** say some
element "shall not be used" — they fence off the parts of UBL 2.1 and CII D16B
that EN 16931 does not use. That inverts the usual expectation:

| | Rules that apply | Why |
|---|---|---|
| **Writer** | ~119 | It has no way to express `cbc:UUID` — the model has no term for it. The prohibitions are *unreachable*, not cheaply satisfied. |
| **Reader** | all 1 339 | The document came from somewhere else. |

Unreachability is a claim, so the serialiser enforces it against prohibitions
extracted from CEN's own Schematron, and `tests/subset.rs` asserts the writer
never needs that safety net.

**1 111 of the 1 218 are represented** — 664 of UBL's 696 and 447 of CII's 522.
The rest have a test an XPath engine is needed to evaluate (a predicate, a
wildcard, `ends-with(name(), 'Amount')`), and they are *counted* rather than
quietly omitted, so a test can report "1 111 of 1 218 checked" instead of
implying a clean sweep.

It was 1 045 until the extractor learned one shape:

```xpath
not((cac:InvoiceLine|cac:CreditNoteLine)/cac:SubInvoiceLine)
```

That is one rule about the two names UBL gives the same thing, and **131 of the
163 UBL prohibitions the table was missing looked exactly like it**. Among them
was `UBL-CR-646` — which is why the writer could emit `cac:SubInvoiceLine` into
a core document with nothing to notice. More rows than assertions is the
expected consequence: an alternation is one rule and two paths.

---

## 📐 The binding is data, not code

Two tables are **generated from the authorities' artefacts**, never transcribed:

| Table | Source | Size |
|---|---|---|
| UBL element order | **319** published UBL instances — CEN unit tests, KoSIT mutation instances, OpenPeppol examples | 36 parents |
| CII element order | **167** published CII instances | 38 parents |
| UBL prohibitions | preprocessed `EN16931-UBL-validation` | 1 548 paths + 21 attributes |
| CII prohibitions | preprocessed `EN16931-CII-validation` | 447 paths |

Both syntaxes' content models are XSD `sequence`s, so a document with exactly
the right elements in the wrong order is **invalid** — and no Schematron rule
says so, because ordering is the schema's job. The order is derived by topologically
sorting the pairwise precedences observed across all 319 documents, taking the
majority direction where they disagree (much of that corpus is deliberately
invalid). Derivation reports **no unresolved conflicts**, and the generator
exits non-zero if it ever does.

The writers hand-sequence nothing: they emit in whatever order reads best and
the serialiser sorts by the table. That made a class of bug structurally
impossible — UBL's two document elements disagree about where `cbc:TaxPointDate`
goes, and a hand-sequenced writer got it wrong.

**The prohibitions are context-relative, and that is half the rule.**
`CII-DT-076` is `not(ram:ID)`, and it does *not* mean "no document may contain
`ram:ID`" — it means the element that rule's context selects may not have one.
An earlier table dropped the context, and the writer duly discarded every
`ram:ID` in the document. Each entry now carries `(rule, context, relative
path)`, taken from the **preprocessed** artefacts where contexts are fully
resolved rather than `$Variable` references.

Regenerate all four with `cargo xtask codegen`; `cargo xtask check` re-derives
them and fails if a committed one differs. Each generator exits non-zero rather
than emitting a table it could not derive cleanly.

---

## 🔒 Nothing is dropped silently

```rust
let out = en16931_formats::ubl::write(&invoice);
assert!(out.dropped.is_empty());    // e.g. BT-9 on a credit note

let read = en16931_formats::ubl::from_str(&xml)?;
assert!(read.unmapped.is_empty());  // elements outside the EN 16931 subset
assert!(read.malformed.is_empty()); // present, but not representable
```

UBL's `<CreditNote>` has no `cbc:DueDate` and no `cac:ProjectReference`. Dropping
them is correct; dropping them *quietly* means a payment due date vanishing
between two systems with nothing in any log.

---

## 🛡️ Reading a document somebody else wrote

Which is the only kind worth reading. Three ways an inbound document can attack
the reader rather than inform it:

| | |
|---|---|
| **entity expansion** (billion laughs) | needs a DTD; the parser rejects every document carrying one |
| **external entities** (XXE, file disclosure) | the same DTD refusal, for the same reason |
| **nesting** | refused past `MAX_DEPTH` **before** parsing |

The third is the one that had teeth. `roxmltree` recurses once per level of
nesting and overflows the stack a few hundred levels in — and a stack overflow is
**not a panic**. Rust cannot unwind it and cannot catch it, so the process
aborts: two lines of XML took down the caller, with no report, no log line and
nothing for `?` to catch.

It cannot be handled afterwards, so it is refused before. One linear scan of the
bytes, then `Error::TooDeep`:

```text
nested 50001 elements deep, and the limit is 64. A document this deep is not a
UBL invoice; it is a denial of service, and the XML parser would abort the
process rather than fail.
```

The limit is **measured, not guessed**: the deepest of the 487 published
instances in the artefact tree is **9**, and `tests/corpus.rs` fails if a future
release ships anything within three times the limit. The headroom runs the other
way too — the overflow was measured on the main thread's 8 MB, and a worker
thread gets 2 MB, so a limit that fits `main` would still abort inside a server.

What this crate does **not** do is bound input size or time. A caller reading
from a socket owns that decision, and a library that quietly capped it would be
wrong for the batch job and useless for the endpoint.

### The schema is wider than the model, and the reader reads the schema

An inbound document is valid against **UBL's XSD**, not against this crate's
idea of a value. `cbc:IssueDate` is `xs:date`, whose lexical space ends with an
optional time zone:

```xml
<cbc:IssueDate>2026-07-31+02:00</cbc:IssueDate>
```

That is schema-valid, no Schematron rule objects to it, and Java's
`XMLGregorianCalendar` — which a great many UBL producers are built on — writes
the offset by default. `Date` holds a calendar day and nothing else, because
EN 16931-1 §6.5.9 has no term for a time zone; so the zone is **dropped, not
applied** (`2026-07-31+02:00` *is* the day 2026-07-31) and BT-2 comes through.

Refusing it instead — which this reader did — cost the whole business term:
BT-2 came back absent and `BR-03` fired on an invoice that states its issue date
perfectly well. A date that is merely wrong still fails, and is still reported
in `malformed`.

`cbc:ChargeIndicator` is the other one, and its failure is worse than a lost
field. It is `xs:boolean`, whose lexical space is `{true, false, 1, 0}` — and
it is the element that decides whether an amount is **added to or subtracted
from** the invoice. A reader that knows only the two words turns a schema-valid
`<cbc:ChargeIndicator>1</cbc:ChargeIndicator>` into an allowance: the same
money, on the other side of the total. Both readers take all four forms now,
and an indicator that is not a boolean at all is reported in `malformed` rather
than folded into "allowance" — there is no safe default, so the answer is to
say the document could not be read.

---

## ✅ Writing a proof, not a hope

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

### When the profile is a runtime choice

`Validated<P>` is a *compile-time* answer, and it is the right one when the proof
travels — across a function boundary, into a queue, through a trait. It is the
wrong shape when the counterparty's preferred CIUS comes out of a database, which
is most of the time. So the same guarantee is available as a `Result`:

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
`cii::to_string_for` is the same in the other syntax, and `write_for` on either
keeps the `dropped` report.

### Why `to_string` returns `String` and not `Result`

**Serialisation cannot fail, and that is a property of the model rather than an
omission.** Every field of `Invoice` already holds a value the syntax can carry:
`InvoiceAmount` cannot hold a third decimal, `Date` cannot hold something that is
not a calendar day — which is exactly what `udt:DateTimeString format="102"`
accepts, and nothing more. There is no state a writer could be handed that it
would have to refuse, and writing into a `String` does no I/O.

**Validity is a separate question, and it is the caller's.** An invoice with no
seller serialises perfectly into a document no counterparty will accept. Run
`en16931::validate` first — or use `to_string_for`, which will not hand you a
document until you have.

---

## 📄 ZUGFeRD / Factur-X

```rust
let got = en16931_formats::zugferd::extract(&pdf_bytes)?;

got.xml;               // the payload, byte-identical to what was sent
got.profile;           // what the payload's BT-24 claims
got.xmp;               // what the PDF's own metadata declares
got.divergence;        // where those two disagree
```

Extraction is the common direction — receiving is more common than sending — and
the one with no PDF/A risk.

**The XMP is not decoration.** It is how a receiver discovers an invoice is there
and which profile it claims, *before parsing anything*. A PDF whose metadata says
BASIC while the payload says EN 16931 validates, opens, and is wrong in a way no
schema notices: a receiver routing on the XMP and one routing on BT-24 process
the same file differently, and both behave correctly. So both are read and
compared, and `Divergence::NoXmp` says when a counterparty scanning metadata
first will not see an e-invoice at all.

### 🔌 …and how the payload is wired in

Getting the invoice out is the easy half. A hybrid PDF can be **structurally
wrong in ways nothing complains about**: it opens, it renders, every viewer
shows the attachment — and the counterparty's intake never sees an e-invoice.
That comes back as a rejected invoice weeks later, with no error to search for.

| | What breaks |
|---|---|
| `NotAssociated` — absent from the catalogue's `/AF` | a PDF/A-3 receiver asking what is associated with this document is told nothing. The commonest defect: every PDF library can *attach* a file, fewer can *associate* one |
| `NotInEmbeddedFiles` — absent from `/Names/EmbeddedFiles` | readers without PDF/A-3 support never find it |
| `NoRelationship` — no `/AFRelationship` | nothing says whether the XML *is* the invoice or accompanies one |
| `NotPdfA3` — `pdfaid:part` is not `3` | parts 1 and 2 of ISO 19005 forbid embedding a file of arbitrary type, so the file contradicts itself and veraPDF says so |

None stops extraction — the payload still comes back verbatim, which is what you
diagnose with. They are what a **sender** wants to know before the file leaves.

### 🪤 The trap in the profile matrix

**Not every ZUGFeRD profile is an EN 16931 invoice.** MINIMUM and BASIC WL carry
no lines, so they cannot satisfy **BR-16**.

```rust
match got.profile.is_en16931_invoice() {
    IsInvoice::Yes     => { /* the 317 rules apply */ }
    IsInvoice::No(why) => println!("not an invoice: {why}"),  // names BR-16
    IsInvoice::Unknown => { /* unrecognised — do not guess */ }
}
```

A type system that says MINIMUM is an EN 16931 invoice is worse than no type
system. An unrecognised profile is `Unknown`, never quietly the core model.

### ⚠️ Provenance

Everything else here is derived from artefacts fetched into `spec/` and verified
there. **The ZUGFeRD and Factur-X specifications are not among them**, so the
⚠-marked claims — profile names, attachment filenames, the XMP structure — are
corroborated against the Factur-X reference implementation rather than against
the normative text. The module documentation names the two files to check
against.

### Writing PDFs: out of scope, and not for want of effort

There is **no `embed(pdf_bytes, &invoice) -> Vec<u8>`**.

A ZUGFeRD file is not "a PDF with an attachment". It is a **PDF/A-3** document,
and the conformance is normative: a file that is no longer valid PDF/A is no
longer a valid ZUGFeRD invoice. Embedding correctly means all of

- rewriting the cross-reference table and trailer without disturbing the
  original's object numbering;
- an `/AF` associated-files array on the catalogue **and** an `/AFRelationship`
  on the file specification — PDF/A-3's own requirement, the part most
  implementations omit, and the one value this crate will not guess (below);
- an XMP packet carrying the ZUGFeRD extension schema whose `DocumentFileName`,
  `Version` and `ConformanceLevel` agree with the payload's BT-24 — the
  divergence this crate already *detects* on the way in, and would have to be
  incapable of *creating* on the way out;
- preserving whatever conformance the input had, `/OutputIntent`, embedded fonts
  and metadata included.

Most of that is checkable only against **veraPDF**, not against a Rust test. A
writer producing files that open happily in a viewer and fail a recipient's
conformance check would be worse than no writer: the failure arrives at the
counterparty, months later, on documents already sent.

And one field this crate refuses to invent. `/AFRelationship` decides whether the
XML **is** the invoice or merely accompanies one — legally load-bearing, and the
published guidance disagrees:

| Profile | Guidance |
|---|---|
| MINIMUM, BASIC WL | `Data` — no lines; the pages are the invoice |
| BASIC, EN 16931, EXTENDED | German sources say `Alternative`; PDFlib documents `Source` for Factur-X to non-German recipients |

So the reader reports the value on `Extracted::relationship` and raises
`Divergence::Relationship` for the one case every source agrees is wrong —
`Data` on a profile that carries lines. Where they disagree it takes no
position.

**What composes today**, and it is most of the way there: render the PDF/A-3
with a toolchain that already guarantees conformance, take the payload from
`cii::to_string_for` — which will not hand you XML until it has validated the
model against the profile you name — and have that toolchain embed it. The half
this crate can guarantee is the half it does.

---

## 🧪 What is tested

| Suite | What it establishes |
|---|---|
| `roundtrip` | `Invoice → syntax → Invoice` is the identity **in both syntaxes**, reported per field — plus a test that UBL and CII agree with *each other* |
| `fidelity` | the same property over the **486 published documents** that parse, where a difference is a failure *unless the writer named it in `dropped`* |
| `cross_syntax` | the stronger claim: read in one syntax, write in the **other**, read back — over the same 486, both directions |
| `order` | Every element either writer emits is in schema sequence |
| `subset` | Neither writer emits a forbidden element or attribute |
| `corpus` | All **319** UBL and **167** CII published instances read; every unmapped element named |
| `zugferd` | Extraction, XMP, `/AFRelationship`, divergence, and the payload as a model — against PDFs built in the test |
| `profile_scoped` | `to_string_for` refuses an invalid invoice in **both** syntaxes, and stamps BT-24 *before* the rules run rather than after |

The corpus suite skips when `spec/` is absent, and CI sets
`EN16931_REQUIRE_SPEC=1` so a skip *there* fails the build: a corpus test that
silently passes on an empty corpus is worse than none. Run `cargo xtask fetch`,
or `just test-artefacts` to hold yourself to CI's standard.

ZUGFeRD's PDFs are built in the tests rather than checked in: a binary fixture is
opaque, and pins one producer's output rather than the structure the
specification describes.

### Why three round-trip suites and not one

Each catches a class the one before it cannot.

- **`roundtrip`** works on a fixture this crate wrote, so it can only find a bug
  in a term someone thought to put in the fixture.
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

486 and 487 are both measured and count different things: the tree holds 320
UBL and 167 CII files, and one UBL file is rejected as not well-formed — a
deliberately malformed instance, correctly refused. The properties run over the
486 that parse; the depth guard above scans all 487 files.

- **`fidelity`** runs the same property over 486 documents this crate did not
  write, which is where terms nobody remembered turn up.
- **`cross_syntax`** changes syntax in between — and that is the one that finds a
  binding which is *consistently* wrong, because a writer and its own reader
  agreeing on a mistake round-trip perfectly. `BT-90` written to CII and never
  to UBL, a credit note detected from `381` alone, BT-111 vanishing when
  BT-6 = BT-5: all invisible until the document has to change syntax.

Every defect of that class produces a schema-valid document that the reader
reads without complaint, which is why the property is asserted over a corpus
rather than argued from the code.

---

## 🚀 Examples

```sh
cargo run --example read_and_validate                       # inbound UBL → Invoice → verdict
cargo run --example write_both_syntaxes --features cii      # one proof, two syntaxes
cargo run --example zugferd_extract --features zugferd      # pull the invoice out of a PDF
```

…or without writing any Rust at all, since
[`en16931-cli`](../en16931-cli) is this crate with a command around it:

```sh
en16931 inspect  rechnung.pdf            # what is this file?
en16931 extract  rechnung.pdf            # the payload, verbatim
en16931 convert  rechnung.xml --to cii   # through the model, not element by element
```

---

## 🧰 Development

[`just`](https://just.systems) is the task runner; `just` alone lists everything.

All commands run from the **workspace root**, one level up:

```sh
cargo xtask fetch     # download the artefacts and published instances into ./spec/
cargo xtask codegen   # re-derive every table in the workspace from them
cargo xtask check     # fail if any committed table no longer matches
just ci               # everything CI runs, locally
just test-all         # every crate, every feature
```

There are no shell scripts. Fetching and generating are both `cargo xtask`
subcommands, so they are compiled, type-checked and linted like the rest of the
workspace.

`en16931` is a **path dependency** in the same workspace, so a breaking change
to the model and its use here land in one commit and one PR. It carries a
version as well, so `cargo publish` resolves it from crates.io — there is no
`[patch.crates-io]` block to remember to remove, and no publish-ordering hazard
that spans two repositories.

`spec/` is **not committed** — the CEN artefacts are EUPL-1.2, a reciprocal
licence. The fetch pulls only what the generators and the suites read, pinned by
**fully-qualified ref**, once for both crates: `eInvoicing-EN16931` publishes
`validation-1.3.16` as
both a tag *and* a branch pointing at different commits, and `git clone
--branch` prefers the branch — so two clones of the same "pin" produced
different trees and different tables.

`cargo xtask check` runs in CI and re-derives every table from the artefacts,
failing if the committed result differs. A table cannot drift away from the
documents it was derived from, and the generators exit non-zero rather than
emitting something they could not derive cleanly — an ordering cycle or a tied
pair fails the build instead of becoming a guess written to disk.

---

### Minimum supported Rust version

**1.88** — the rule code uses `let`-chains. Measured, not declared: 1.87 fails,
and CI reads the number from `Cargo.toml` rather than repeating it.

## ⚖️ Licence

MIT OR Apache-2.0.

The bindings are derived from CEN's EUPL-1.2 validation artefacts and the element
order from the authorities' published instances — facts about element placement
rather than copied expressions. The CEN attribution notice is a licence
condition, is re-exported from `en16931` rather than restated, and has a test.

---

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
