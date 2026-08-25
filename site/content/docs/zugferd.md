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

## What else the read checks

Getting the invoice out is the easy half. The hard half is that a hybrid PDF can
be **structurally wrong in ways nothing complains about**: it opens, it renders,
every viewer shows the attachment — and the counterparty's intake never sees an
e-invoice. Those defects come back as a rejected invoice weeks later, with no
error message to search for.

Every extraction therefore reports how the payload is wired in, as `Divergence`
values:

| | What breaks |
|---|---|
| `NotAssociated` — absent from the catalogue's `/AF` | a PDF/A-3 receiver asking what is associated with this document is told nothing. The commonest defect: every PDF library can *attach* a file, fewer can *associate* one |
| `NotInEmbeddedFiles` — absent from `/Names/EmbeddedFiles` | readers without PDF/A-3 support never find it |
| `NoRelationship` — no `/AFRelationship` | nothing says whether the XML *is* the invoice or accompanies one |
| `NotPdfA3` — `pdfaid:part` is not `3` | parts 1 and 2 of ISO 19005 forbid embedding a file of arbitrary type at all, so the file contradicts itself and veraPDF will say so |
| `Relationship`, `Profile`, `Filename`, `NoXmp` | the metadata and the payload disagree |

None of these stops extraction: the payload still comes back verbatim, which is
what you diagnose with. They are what a **sender** wants to know before the file
leaves, and `en16931 inspect invoice.pdf` prints them.

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

## Writing PDFs: out of scope, and not for want of effort

There is no `embed(pdf_bytes, &invoice) -> Vec<u8>`.

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
profile names, attachment filenames and XMP structure here are corroborated
against the Factur-X *reference implementation* rather than against the
normative text, and the crate marks them ⚠. That is a weaker claim than the
specification and a much stronger one than recollection. The two artefacts, for
anyone repeating it:
[`facturx.py`](https://github.com/akretion/factur-x/blob/master/src/facturx/facturx.py)
and its [XMP extension schema](https://github.com/akretion/factur-x/blob/master/src/facturx/xmp/Factur-X_extension_schema.xmp).

One property is easy to misread: **`fx:Version` is the version of the Factur-X
XMP schema, not of ZUGFeRD.** It has been the constant `1.0` since Factur-X 1.0,
and a ZUGFeRD 2.3 file still carries `1.0` — so a check comparing it against
`2.3` rejects every conforming file.

## Two things a writer gets wrong first

Recorded here because neither is in this crate.

**The XMP namespace URI** is
`urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#`. The mixed case and the
trailing `#` are both load-bearing, and the sample PDFs in the Factur-X 1.0 info
package spell it in lowercase, which is wrong.

**The Factur-X extension-schema block cannot be pasted into an arbitrary XMP
packet as its own `rdf:Description`.** XMP allows each property once per packet
and `pdfaExtension:schemas` is a property — so a PDF generator that already
writes extension schemas of its own already carries that bag, and a second one
makes Adobe-lineage parsers and veraPDF reject the whole packet, silently
ending the file's PDF/A conformance. Merge the fx schema's `rdf:li` into the
existing bag; a standalone description is only correct when the generator writes
no extension schemas at all. The defect is invisible to every XML parser and to
`en16931 validate`, which is why a writer is checkable only against veraPDF.

## What is next

- **[The CLI](@/docs/cli.md)** — `en16931 inspect rechnung.pdf`.
