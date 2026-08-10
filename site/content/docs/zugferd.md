+++
title = "ZUGFeRD and Factur-X"
weight = 6
description = "Extract the CII payload from a ZUGFeRD or Factur-X hybrid PDF, compare it against the PDF's own XMP metadata, and know which profiles are not EN 16931 invoices at all."
+++

ZUGFeRD (Germany) and Factur-X (France) are the same thing under two names: a
**PDF/A-3** document with a CII payload embedded as an attachment. The pages are
for a human, the XML is for a machine, and they are supposed to say the same
thing.

Behind `features = ["zugferd"]`, because the PDF layer takes the dependency graph
from 13 crates to 57 and breaks `wasm32`.

## Extracting

```rust
let got = en16931_formats::zugferd::extract(&pdf_bytes)?;

got.xml;               // the payload, byte-identical to what was sent
got.profile;           // what the payload's BT-24 claims
got.xmp;               // what the PDF's own metadata declares
got.divergence;        // where those two disagree
got.relationship;      // the /AFRelationship value found
```

Extraction is the common direction — receiving is more common than sending — and
the one with no PDF/A risk.

## The XMP is not decoration

It is how a receiver discovers an invoice is there and which profile it claims,
*before parsing anything*. A PDF whose metadata says BASIC while the payload says
EN 16931 validates, opens, and is wrong in a way no schema notices: a receiver
routing on the XMP and one routing on BT-24 process the same file differently,
and both behave correctly.

So both are read and compared. `Divergence::NoXmp` says when a counterparty
scanning metadata first will not see an e-invoice at all.

## Not every ZUGFeRD profile is an EN 16931 invoice

This is the trap in the profile matrix. **MINIMUM and BASIC WL carry no lines**,
so they cannot satisfy `BR-16`. They are accounting aids, not invoices.

```rust
match got.profile.is_en16931_invoice() {
    IsInvoice::Yes     => { /* the 317 rules apply */ }
    IsInvoice::No(why) => println!("not an invoice: {why}"),  // names BR-16
    IsInvoice::Unknown => { /* unrecognised — do not guess */ }
}
```

An unrecognised profile is `Unknown`, never quietly the core model. A type system
that says MINIMUM is an EN 16931 invoice is worse than no type system.

## Writing PDFs: not implemented, and the reason is not effort

There is no `embed(pdf_bytes, &invoice) -> Vec<u8>`, and asking for one is
entirely reasonable — so here is what stands in the way, because "not yet"
without a reason is the least useful thing a crate can say.

A ZUGFeRD file is not "a PDF with an attachment". It is a **PDF/A-3** document,
and the conformance is normative: a file that is no longer valid PDF/A is no
longer a valid ZUGFeRD invoice. Embedding correctly means all of

- rewriting the cross-reference table and trailer without disturbing the
  original's object numbering;
- an `/AF` associated-files array on the catalogue **and** an `/AFRelationship`
  on the file specification — PDF/A-3's own requirement, the part most
  implementations omit;
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

### And one field this crate refuses to invent

`/AFRelationship` decides whether the XML **is** the invoice or merely accompanies
one. That is legally load-bearing, and the published guidance disagrees:

| Profile | Guidance |
|---|---|
| MINIMUM, BASIC WL | `Data` — no lines; the pages are the invoice |
| BASIC, EN 16931, EXTENDED | German sources say `Alternative`; PDFlib documents `Source` for Factur-X to non-German recipients |

So the reader reports the value it found and raises `Divergence::Relationship`
for the one case every source agrees is wrong — `Data` on a profile that carries
lines. Where the sources disagree it takes no position.

### What composes today

Most of the way there, and the half this crate can guarantee is the half it does:

1. Render the PDF/A-3 with a toolchain that already guarantees conformance.
2. Take the payload from `cii::to_string_for`, which will not hand you XML until
   it has validated the model against the profile you name.
3. Have that toolchain embed it.

## A note on provenance

The rest of this project is written against artefacts fetched and verified
locally. **The ZUGFeRD and Factur-X specifications are not among them** — the
profile names, attachment filenames and XMP structure here are stated from
knowledge rather than from a fetched specification, and the crate marks them ⚠.

**The marks worked.** A downstream team building a PDF/A-3 writer needed exactly
these values, checked them against the Factur-X reference implementation, and
reported back. Everything matched — the five level names including the space in
`EN 16931`, the filenames and their preference order, the four `fx:` XMP
properties, and the finding that published guidance genuinely disagrees on
`/AFRelationship`, so declining to pick a default is right rather than evasive.

One did not, and is fixed: **`fx:Version` is the version of the Factur-X XMP
schema, not of ZUGFeRD.** It has been the constant `1.0` since Factur-X 1.0, and
a ZUGFeRD 2.3 file still carries `1.0`. It was documented as "the ZUGFeRD /
Factur-X version", which invites comparing it against `2.3` — a check that
rejects every conforming file.

So the ⚠ stays, and now means *"corroborated against the reference
implementation, not against CEN"*. That is a weaker claim than the normative
text and a much stronger one than recollection. The two artefacts, for anyone
repeating it: [`facturx.py`](https://github.com/akretion/factur-x/blob/master/src/facturx/facturx.py)
and its [XMP extension schema](https://github.com/akretion/factur-x/blob/master/src/facturx/xmp/Factur-X_extension_schema.xmp).

One value a writer needs and this crate does not hold: the XMP namespace URI is
`urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#`. The mixed case and the
trailing `#` are both load-bearing, and the sample PDFs in the Factur-X 1.0 info
package spell it in lowercase, which is wrong.

And a second writer pitfall the same team hit, found only by veraPDF: **the
Factur-X extension-schema block cannot be pasted into an arbitrary XMP packet as
its own `rdf:Description`.** XMP allows each property once per packet, and
`pdfaExtension:schemas` is a property — a PDF generator that already writes
extension schemas of its own already carries that bag, and a second one makes
Adobe-lineage parsers and veraPDF reject the whole packet, so the file silently
stops being PDF/A. Merge the fx schema's `rdf:li` into the existing
`pdfaExtension:schemas` bag; a standalone description is only correct when the
generator writes no extension schemas at all. The defect is invisible to every
XML parser and to `en16931 validate` — it is not an XML defect — which is
exactly why a writer is checkable only against veraPDF.

## What is next

- **[The CLI](@/docs/cli.md)** — `en16931 inspect rechnung.pdf`.
