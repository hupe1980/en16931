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
| `zugferd` | — | **57 crates** | ZUGFeRD / Factur-X hybrid PDFs |
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

`en16931` shipped and fixed exactly that bug once: an `Underlies` impl that let
an invoice validated against one profile be widened into a proof for another it
had never been checked against. A type system that says MINIMUM is an EN 16931
invoice is worse than no type system. An unrecognised profile is `Unknown`, never
quietly the core model.

### ⚠️ Provenance

`en16931`'s design was written against artefacts fetched into `spec/` and
verified there. **The ZUGFeRD and Factur-X specifications are not among them.**
Everything ⚠-marked — profile names, attachment filenames, the XMP structure — is
stated from knowledge, not a fetched specification. **Milestone 0.0 is: fetch the
specification.**

### Writing PDFs: not implemented, and the reason is not effort

There is **no `embed(pdf_bytes, &invoice) -> Vec<u8>`**, and asking for one is
entirely reasonable — so here is what stands in the way, because "not yet"
without a reason is the least useful thing a crate can say.

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

There was a `render` feature declared for the visible half. It enabled nothing
and gated nothing, and shipped in the table above for two releases; a feature
that does not exist is worse documentation than an absence, so it is gone. This
section is the answer it was standing in for.

---

## 🧪 What is tested

**114 tests.**

| Suite | What it establishes |
|---|---|
| `roundtrip` | `Invoice → syntax → Invoice` is the identity **in both syntaxes**, reported per field — plus a test that UBL and CII agree with *each other* |
| `fidelity` | the same property over the **486 published documents**, where a difference is a failure *unless the writer named it in `dropped`* |
| `cross_syntax` | the stronger claim: read in one syntax, write in the **other**, read back — over the same 486, both directions |
| `order` | Every element either writer emits is in schema sequence |
| `subset` | Neither writer emits a forbidden element or attribute |
| `corpus` | All **319** UBL and **167** CII published instances read; every unmapped element named |
| `zugferd` | Extraction, XMP, `/AFRelationship`, divergence, and the payload as a model — against PDFs built in the test |
| `profile_scoped` | `to_string_for` refuses an invalid invoice in **both** syntaxes, and stamps BT-24 *before* the rules run rather than after |

The corpus suite skips when `spec/` is absent, and CI sets
`EN16931_REQUIRE_SPEC=1` so that a skip *there* fails the build. A corpus test
that silently passes on an empty corpus is worse than none — and a `println!`
in a green run is not the warning it looks like. Run `cargo xtask fetch`, or
`just test-artefacts` to hold yourself to CI's standard.

ZUGFeRD's PDFs are built in the tests rather than checked in: a binary fixture is
opaque, and pins one producer's output rather than the structure the
specification describes.

**Fourteen real bugs came out of writing these rather than assuming.** Four from the
early suites: BT-158's scheme is `@listID`, not `@schemeID` (well-formed,
schema-valid, silently wrong); the reader handed back base64 text instead of
decoded attachment bytes; BT-9 on a credit note was dropped without saying so;
and `UBL-CR-244` forbids BT-33 on the customer.

Five more the moment `fidelity` ran the same property over documents this crate
did not write. The `maximal()` fixture is *this crate's* idea of a complete
invoice, so it can only catch a bug in a term someone thought to put in it:

| | |
|---|---|
| `cbc:BaseQuantity unitCode=""` | BT-150 came back as `Some("")`, and **`PEPPOL-EN16931-R130` then fired** — a fatal finding manufactured by writing a document out and reading it back |
| BT-147 required BT-148 | a price discount with no gross price was dropped, on seven instances |
| `cac:SubInvoiceLine` | BG-DEX-01 was read and never written |
| `cac:PrepaidPayment` | BG-DEX-09 likewise — the data `EN-EXT-01` exists to warn about losing |
| empty `cac:InvoicePeriod` | dropped without a word, so `BR-CO-20` stopped firing on the rewrite |

Every one of them produced a schema-valid document that the reader read without
complaint. That is the class of bug a round trip catches and nothing else does.

**Four more when `cross_syntax` made the documents change syntax.** A same-syntax
round trip cannot see a binding that is *consistently* wrong — a writer and its
own reader agreeing on a mistake round-trip perfectly. Crossing breaks the
agreement:

| | |
|---|---|
| **BT-90 was never written to UBL** | UBL carries the creditor identifier on the **seller** as `schemeID="SEPA"`, the one place `BR-CL-10` admits a non-ISO-6523 scheme. The reader hopped it into BG-19; the writer never hopped it back, so every direct-debit invoice written as UBL lost it and failed `BR-DE-30` at the counterparty |
| **BT-20 was trimmed by the CII reader** | `BR-DE-18` needs the Skonto block to end with a newline, and XRechnung is carried in CII too — 36 documents |
| **CII detected credit notes from `381` alone** | `396`, `532` and `83` read back as *invoices*, and `BR-CL-01` then reported a violation that is not one. Every published CII credit note uses `381`, so no corpus could show it |
| **BT-111 vanished when BT-6 = BT-5** | one element is then both totals — which the UBL reader knew and the CII reader did not |

Plus one the two readers simply disagreed about: an **empty element**.
`<cbc:BuyerReference/>` is an absent term, not a term whose value is the empty
string. Both readers said `None` through their `text()` helper and `Some("")`
through the dozen call sites that read an element's own text — inconsistent with
themselves, and with each other on nine documents.

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
