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
//! # Writing: not implemented, and the reason is not effort
//!
//! There is **no `embed(pdf_bytes, &invoice) -> Vec<u8>`**, and asking for one
//! is entirely reasonable — so here is what stands in the way, because "not
//! yet" without a reason is the least useful thing a crate can say.
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
//! There was a `render` feature declared for the visible half of the same job.
//! It enabled nothing, gated nothing and shipped in the feature table for two
//! releases; a feature that does not exist is worse documentation than an
//! absence, so it is gone. This section is the answer it was standing in for.
//!
//! # ⚠ Provenance, and what has since been corroborated
//!
//! [`en16931`]'s design was written against artefacts fetched into `spec/` and
//! verified there. **The ZUGFeRD and Factur-X specifications are not among
//! them.** Claims marked ⚠ — profile names, attachment filenames, the XMP
//! structure — are stated from knowledge rather than a fetched specification,
//! and are the first thing to check before relying on them.
//!
//! That warning is not boilerplate. This project already had one incident where
//! two plausible specification identifiers were invented and an argument built
//! on them; the fix was to check every transcribed value against its source.
//!
//! **The warning worked.** A downstream user building a writer needed exactly
//! these values, went and checked them against the reference implementation, and
//! reported back: every ⚠ value in this module is correct —
//! [`Profile::as_str`]'s five Factur-X level names including the space in
//! `EN 16931`, [`FILENAMES`] and their preference order, the four `fx:`
//! properties and their spellings, and the observation behind
//! [`Divergence::Relationship`] that the published sources genuinely disagree
//! between `Alternative` and `Source` so this crate is right not to pick.
//!
//! One was **wrong**, and is fixed: `fx:Version` is the version of the Factur-X
//! *XMP schema* — constant `1.0` — and was documented here as the ZUGFeRD
//! version. See [`Xmp::version`].
//!
//! The two artefacts they checked against, for anyone repeating it:
//!
//! | | |
//! |---|---|
//! | [`facturx.py`](https://raw.githubusercontent.com/akretion/factur-x/master/src/facturx/facturx.py) | `FACTURX_LEVEL2xmp`, `XML_AFRelationship`, the filenames, and the hardcoded `"version": "1.0"` |
//! | [`Factur-X_extension_schema.xmp`](https://raw.githubusercontent.com/akretion/factur-x/master/src/facturx/xmp/Factur-X_extension_schema.xmp) | the PDF/A extension schema, authored by PDFlib for the Factur-X 1.0 info package |
//!
//! Neither is the specification, so **the ⚠ stays**: these are the artefacts
//! every implementation is built against, which is a different claim from the
//! normative text and a weaker one. But it is a great deal better than one
//! person's recollection, and the marks now mean "corroborated against the
//! reference implementation, not against CEN" rather than "unchecked".
//!
//! One value a writer needs that is **not** in this crate, recorded here because
//! it is the first thing to get wrong: the XMP namespace URI is
//! `urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#`. The mixed case in
//! `CrossIndustryDocument` and the trailing `#` are both load-bearing, and the
//! PDFlib file notes that the sample PDFs in the Factur-X 1.0 info package use
//! an all-lowercase spelling that is *not* correct.
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
