//! **ZUGFeRD and Factur-X** — invoices that are a PDF *and* machine-readable data.
//!
//! A ZUGFeRD invoice is a **PDF that is also data**. The data half is CII and
//! belongs to [`crate::cii`]; the PDF half is PDF/A-3 with an embedded file and
//! specific XMP metadata, and that is the only part this module owns. Every
//! business rule is delegated — this module adds no rule coverage and needs no
//! rule tests.
//!
//! # Reading, which works today
//!
//! [`extract`] is the common direction — receiving is more common than sending
//! — and the one with no PDF/A risk: reading means walking the catalogue to
//! `/Names/EmbeddedFiles` and inflating one stream. No rendering, no fonts, no
//! text layout.
//!
//! ```no_run
//! use en16931_formats::zugferd::{self, IsInvoice};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let pdf = std::fs::read("invoice.pdf")?;
//! let got = zugferd::extract(&pdf)?;
//!
//! // What the document *claims*, from its BT-24. Not every profile is an
//! // EN 16931 invoice: MINIMUM and BASIC WL carry no lines and cannot
//! // satisfy `BR-16`, so validating them would report a failure that is not one.
//! println!("{:?} in {}", got.profile, got.filename);
//! if got.profile.is_en16931_invoice() == IsInvoice::Yes {
//!     let read = en16931_formats::cii::from_str(&got.xml)?;
//!     println!("{}", en16931::validate(&read.invoice));
//! }
//! // `got.xml` is the payload **verbatim**: whoever diagnoses a rejected
//! // invoice needs the bytes the counterparty sent, not a reconstruction.
//! # Ok(()) }
//! ```
//!
//! `examples/zugferd_extract.rs` is a runnable version that builds its own PDF
//! when given no argument.
//!
//! # What else the read checks, and why it is worth checking
//!
//! Getting the invoice out is the easy half. The hard half is that a hybrid PDF
//! can be **structurally wrong in ways nothing complains about**: it opens, it
//! renders, every viewer shows the attachment — and the counterparty's intake
//! never sees an e-invoice. Those defects arrive back as a rejected invoice
//! weeks later, with no error to search for.
//!
//! So every extraction also reports how the payload is wired in, as
//! [`Divergence`] values on [`Extracted::divergence`]:
//!
//! | | What breaks |
//! |---|---|
//! | [`NotAssociated`] — absent from the catalogue's `/AF` | a PDF/A-3 receiver asking what is associated with this document is told nothing. The commonest defect: every PDF library can *attach* a file and fewer can *associate* one |
//! | [`NotInEmbeddedFiles`] — absent from `/Names/EmbeddedFiles` | readers without PDF/A-3 support never find it |
//! | [`NoRelationship`] — no `/AFRelationship` | nothing says whether the XML *is* the invoice or accompanies one |
//! | [`NotPdfA3`] — `pdfaid:part` is not `3` | parts 1 and 2 of ISO 19005 forbid embedding a file of arbitrary type at all, so the file is self-contradictory and veraPDF will say so |
//! | [`Relationship`], [`Profile`], [`Filename`], [`NoXmp`] | the metadata and the payload disagree |
//!
//! None of these stops extraction: the payload still comes back verbatim, which
//! is what you diagnose with. They are what a sender wants to know **before**
//! the file leaves, and `en16931 inspect` prints them.
//!
//! [`Divergence`]: crate::zugferd::Divergence
//! [`Extracted::divergence`]: crate::zugferd::Extracted::divergence
//! [`NotAssociated`]: crate::zugferd::Divergence::NotAssociated
//! [`NotInEmbeddedFiles`]: crate::zugferd::Divergence::NotInEmbeddedFiles
//! [`NoRelationship`]: crate::zugferd::Divergence::NoRelationship
//! [`NotPdfA3`]: crate::zugferd::Divergence::NotPdfA3
//! [`Relationship`]: crate::zugferd::Divergence::Relationship
//! [`Profile`]: crate::zugferd::Divergence::Profile
//! [`Filename`]: crate::zugferd::Divergence::Filename
//! [`NoXmp`]: crate::zugferd::Divergence::NoXmp
//!
//! # Writing: out of scope, and not for want of effort
//!
//! There is **no `embed(pdf_bytes, &invoice) -> Vec<u8>`**.
//!
//! A ZUGFeRD file is not "a PDF with an attachment". It is a **PDF/A-3**
//! document, and the conformance is normative: ZUGFeRD 2.x and Factur-X both
//! require it, and a file that is no longer valid PDF/A is no longer a valid
//! ZUGFeRD invoice. Embedding correctly means all of:
//!
//! * rewriting the cross-reference table and trailer without disturbing the
//!   original's object numbering;
//! * an `/AF` associated-files array on the catalogue **and** an
//!   `/AFRelationship` on the file specification — PDF/A-3's own requirement,
//!   the part most implementations omit, and **the one this crate will not
//!   guess**: see below;
//! * an XMP packet carrying the ZUGFeRD extension schema, whose
//!   `DocumentFileName`, `Version` and `ConformanceLevel` agree with the
//!   payload's BT-24 — a divergence this module already *detects* on the way in
//!   ([`Divergence`]) and would have to be incapable of *creating* on the way
//!   out;
//! * preserving whatever conformance the input had, `/OutputIntent`, embedded
//!   fonts and metadata included.
//!
//! Most of that is checkable only against **veraPDF**, not against a Rust test.
//! A writer producing files that open happily in a viewer and fail a
//! recipient's conformance check would be worse than no writer: the failure
//! arrives at the counterparty, months later, on documents already sent.
//!
//! ## And one value this crate refuses to invent
//!
//! `/AFRelationship` decides whether the XML **is** the invoice or merely
//! accompanies one, which makes it legally load-bearing rather than
//! descriptive. Published guidance does not agree on it:
//!
//! | Profile | Guidance |
//! |---|---|
//! | MINIMUM, BASIC WL | `Data` — no lines; the pages are the invoice |
//! | BASIC, EN 16931, EXTENDED | German sources say `Alternative`; PDFlib documents `Source` for Factur-X to non-German recipients |
//!
//! Writing the wrong one yields a file that opens, passes PDF/A validation,
//! and extracts correctly with [`extract`] — and may not be a valid invoice
//! where it lands. That is the failure mode this whole crate is built against,
//! and the ⚠ below is not decoration: the specification is not among the
//! fetched artefacts, so there is nothing here to resolve the disagreement.
//!
//! What this module does instead is **read** it, report it on
//! [`Extracted::relationship`], and raise [`Divergence::Relationship`] for the
//! one case every source agrees is wrong — `Data` on a profile that carries
//! lines. Where the sources disagree it takes no position.
//!
//! **What composes today**, and it is most of the way there: render the PDF/A-3
//! with a toolchain that already guarantees conformance, take the payload from
//! [`crate::cii::to_string_for`] — which will not hand you XML until it has
//! validated the model against the profile you name — and have that toolchain
//! embed it. The half this crate can guarantee is the half it does.
//!
//! # ⚠ Provenance
//!
//! Everything else in this crate is derived from artefacts fetched into
//! `spec/` and verified there. **The ZUGFeRD and Factur-X specifications are
//! not among them**, so the claims this module marks ⚠ — profile names,
//! attachment filenames, the XMP structure — are corroborated against the
//! *reference implementation* rather than against the normative text. That is
//! a weaker claim, and the mark says so.
//!
//! The two artefacts to check against:
//!
//! | | |
//! |---|---|
//! | [`facturx.py`](https://raw.githubusercontent.com/akretion/factur-x/master/src/facturx/facturx.py) | `FACTURX_LEVEL2xmp`, `XML_AFRelationship`, the filenames, and the hardcoded `"version": "1.0"` |
//! | [`Factur-X_extension_schema.xmp`](https://raw.githubusercontent.com/akretion/factur-x/master/src/facturx/xmp/Factur-X_extension_schema.xmp) | the PDF/A extension schema, authored by PDFlib for the Factur-X 1.0 info package |
//!
//! # Two things a writer gets wrong first
//!
//! Recorded here because neither is in this crate and both cost a working
//! afternoon.
//!
//! **The XMP namespace URI** is
//! `urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#`. The mixed case in
//! `CrossIndustryDocument` and the trailing `#` are both load-bearing, and the
//! sample PDFs in the Factur-X 1.0 info package use an all-lowercase spelling
//! that is *not* correct.
//!
//! **The Factur-X extension-schema block is not a self-contained fragment.**
//! XMP (ISO 16684-1) allows each property at most once per packet, and
//! `pdfaExtension:schemas` *is* a property — so a generator that already writes
//! extension schemas of its own (Typst/krilla does) already carries the bag,
//! and adding the Factur-X description as a second `rdf:Description` duplicates
//! it. Every XML parser accepts the result; Adobe-lineage XMP parsers and
//! veraPDF reject the whole packet (clause 6.6.2.1, then reporting the PDF/A
//! identification of 6.6.4 missing), and the file silently stops being PDF/A.
//! The fx schema's `rdf:li` must be **merged into whatever
//! `pdfaExtension:schemas` bag is already there**. Neither this crate's reader
//! nor `en16931 validate` can see the defect — it is not an XML defect, and the
//! payload is reached through the embedded-files tree rather than the XMP —
//! which is why a writer is checkable only against veraPDF.
//!
//! [`Profile::as_str`]: crate::zugferd::Profile::as_str
//! [`Xmp::version`]: crate::zugferd::Xmp::version
//! [`Divergence::Relationship`]: crate::zugferd::Divergence::Relationship

mod extract;
mod profile;

pub use extract::{Divergence, Extracted, FILENAMES, Xmp, embedded_files, extract};
pub use profile::{IsInvoice, Profile};

/// Anything that stopped a hybrid PDF being read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The bytes are not a PDF this crate can parse.
    #[error("not a readable PDF: {0}")]
    Pdf(#[from] lopdf::Error),
    /// A PDF with no embedded file that looks like an invoice.
    ///
    /// Distinct from a parse failure on purpose: "this is a plain PDF, not a
    /// ZUGFeRD one" is a different message than "this file is broken", and a
    /// caller that cannot tell them apart writes a bad one.
    #[error("no embedded invoice found (looked for {looked_for:?}, found {found:?})")]
    NoInvoice {
        /// The filenames that would have been accepted.
        looked_for: &'static [&'static str],
        /// The embedded filenames actually present, for diagnosis.
        found: Vec<String>,
    },
    /// The embedded file is not valid UTF-8.
    #[error("the embedded invoice is not valid UTF-8: {0}")]
    Encoding(#[from] std::string::FromUtf8Error),
}
