//! UBL 2.1, UN/CEFACT CII D16B and ZUGFeRD / Factur-X for the EN 16931 e-invoice.
//!
//! [`en16931`] decides whether an invoice is correct; this crate reads and
//! writes it on the wire and re-implements none of the 334 business rules.
//!
//! | Feature | Default | Graph | What |
//! |---|---|---|---|
//! | [`ubl`] | ✅ | 13 crates | UBL 2.1, both directions |
//! | [`cii`] | — | 13 crates | UN/CEFACT CII D16B, both directions |
//! | [`zugferd`] | — | 57 crates | ZUGFeRD / Factur-X: extract from and embed into PDF/A-3; does not build for `wasm32` |
//!
//! | Task | Call |
//! |---|---|
//! | read UBL, CII or a ZUGFeRD PDF from bytes | [`read`] |
//! | receive bytes and decide — accept, reject, undefined | [`receive`] |
//! | write a validated invoice | [`write()`] |
//! | write a model nobody validated | [`write_unchecked`] |
//! | convert between syntaxes | [`convert`] |
//! | embed a validated invoice into a PDF/A-3 file | [`zugferd::embed`] (feature `zugferd`) |
//!
//! ```
//! # #[cfg(feature = "ubl")] {
//! use en16931::Invoice;
//! use en16931_formats::{Policy, Recommendation, Syntax};
//!
//! // A model UBL's schema has no form for is refused, naming what it lacks.
//! let refused = en16931_formats::write_unchecked(&Invoice::default(), Syntax::Ubl);
//! assert!(refused.unwrap_err().to_string().contains("BT-1"));
//!
//! let received = en16931_formats::receive(b"<Invoice/>", &Policy::default());
//! assert_eq!(received.recommendation, Recommendation::Reject);
//! # }
//! ```
//!
//! Reading never drops anything silently: what the reader could not map or
//! hold, and every further occurrence of an element the schema allows once, is
//! a typed [`report::Entry`]; the first occurrence is the one read. Input is
//! UTF-8 or UTF-16 with a byte-order mark; any other encoding is refused
//! ([`Error::Encoding`]). A DTD is refused, which rules out entity expansion
//! and external entities, and nesting deeper than [`MAX_DEPTH`] is refused
//! before parsing ([`Error::TooDeep`]). Input size and time are the caller's
//! to bound.
//!
//! Writers driven from the model cannot produce the elements CEN's syntax
//! rules prohibit; the prohibitions extracted from CEN's Schematron are
//! enforced anyway (see [`ubl::prohibitions`]).
//!
//! The bindings are derived from CEN's EUPL-1.2 validation artefacts; the
//! attribution notice is [`ATTRIBUTION`].

#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs, clippy::pedantic)]
#![allow(
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    // A writer function walks one element's schema sequence in order.
    clippy::too_many_lines,
    // `en16931::invoice::*` is the model.
    clippy::wildcard_imports
)]

#[cfg(feature = "cii")]
#[cfg_attr(docsrs, doc(cfg(feature = "cii")))]
pub mod cii;
#[cfg(any(feature = "ubl", feature = "cii"))]
mod receive;
pub mod report;
#[cfg(any(feature = "ubl", feature = "cii"))]
mod required;
#[cfg(feature = "ubl")]
#[cfg_attr(docsrs, doc(cfg(feature = "ubl")))]
pub mod ubl;
#[cfg(any(feature = "ubl", feature = "cii"))]
mod xml;
#[cfg(feature = "zugferd")]
#[cfg_attr(docsrs, doc(cfg(feature = "zugferd")))]
pub mod zugferd;

use en16931::Invoice;
#[cfg(any(feature = "ubl", feature = "cii"))]
use en16931::validation::profile::{ProfileMarker, Validated};

#[cfg(any(feature = "ubl", feature = "cii"))]
pub use receive::{Policy, Received, Recommendation, receive};
use report::{Entry, Kind};
#[cfg(any(feature = "ubl", feature = "cii"))]
pub use xml::MAX_DEPTH;

/// The CEN attribution notice, as [`en16931`] carries it.
pub const ATTRIBUTION: &str = en16931::ATTRIBUTION;

/// Which syntax a document is written in.
///
/// A variant exists only when its feature is on, so a build that cannot write
/// CII cannot be asked to; a `match` outside this crate needs a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Syntax {
    /// OASIS UBL 2.1 — `Invoice` or `CreditNote`.
    #[cfg(feature = "ubl")]
    Ubl,
    /// UN/CEFACT Cross Industry Invoice D16B.
    #[cfg(feature = "cii")]
    Cii,
}

impl core::fmt::Display for Syntax {
    #[cfg_attr(not(any(feature = "ubl", feature = "cii")), allow(unused_variables))]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            #[cfg(feature = "ubl")]
            Self::Ubl => f.write_str("UBL 2.1"),
            #[cfg(feature = "cii")]
            Self::Cii => f.write_str("UN/CEFACT CII D16B"),
        }
    }
}

/// A document, read.
#[derive(Debug, Clone)]
pub struct Read {
    /// The model.
    pub invoice: Invoice,
    /// The syntax the invoice was written in — CII for a hybrid PDF.
    pub syntax: Syntax,
    /// Everything reading had to say: duplicates (in document order), then the
    /// reader's malformed values and schema notes, then each unmapped path once.
    pub entries: Vec<Entry>,
    /// For a hybrid PDF, the container: the payload's bytes, its filename, the
    /// ZUGFeRD profile, the XMP and every disagreement between them.
    #[cfg(feature = "zugferd")]
    #[cfg_attr(docsrs, doc(cfg(feature = "zugferd")))]
    pub pdf: Option<zugferd::Extracted>,
}

impl Read {
    /// The entries of one kind.
    pub fn entries_of(&self, kind: Kind) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(move |e| e.kind == kind)
    }

    /// What reading found, as a layered [`report::Report`] with **no rules
    /// run**: the container, syntax and schema layers, the document's
    /// identity, and a semantic layer skipped. [`receive`] is the call that
    /// validates.
    #[cfg(any(feature = "ubl", feature = "cii"))]
    #[must_use]
    pub fn report(&self) -> report::Report {
        let (container, syntax, schema) = receive::document_layers(self);
        report::Report {
            identity: report::Identity::of(&self.invoice),
            rule_set: None,
            container,
            syntax,
            schema,
            semantic: report::Semantic {
                validity: report::Validity::Skipped("not validated".to_owned()),
                report: None,
            },
        }
    }
}

/// A document, written.
///
/// One type for both syntaxes, and never a document without its report: what
/// the target could not carry is in [`entries`](Self::entries).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// The document.
    pub xml: String,
    /// What the target syntax could not carry ([`Kind::Dropped`]), what CEN's
    /// syntax rules forbid it to ([`Kind::Prohibited`]), and what the writer
    /// had to derive ([`Kind::Derived`]). Empty for an ordinary invoice.
    pub entries: Vec<Entry>,
}

/// A model the target syntax's schema has no form for, so nothing was written.
///
/// UBL 2.1 requires `cbc:ID`, `cbc:IssueDate`, both parties and a line; D16B
/// requires `ram:ID` and `ram:IssueDateTime`. A model without them would be a
/// document every receiver's schema check rejects, and the writer will not
/// emit one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "not written: the target schema has no form for this model ({})",
    self.entries
        .iter()
        .filter(|e| e.kind == Kind::Schema)
        .map(|e| e.term.clone().unwrap_or_else(|| e.path.clone()))
        .collect::<Vec<_>>()
        .join(", ")
)]
pub struct Unwritable {
    /// Every entry the writer produced, the [`Kind::Schema`] ones among them.
    pub entries: Vec<Entry>,
}

/// Whether [`convert`] may lose content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loss {
    /// Refuse a conversion that would lose anything, naming what.
    Refuse,
    /// Convert, and list what was lost.
    Allow,
}

/// A conversion.
#[derive(Debug, Clone)]
pub struct Converted {
    /// The document in the target syntax.
    pub document: String,
    /// The source document as received under its declared profile — so a
    /// conversion of an invalid invoice says so.
    pub report: report::Report,
    /// Everything the writer said, [`Kind::Derived`] included.
    pub written: Vec<Entry>,
    /// What did not reach the target: source content the reader could not map
    /// or hold, and terms the target could not or may not carry.
    pub lost: Vec<Entry>,
}

/// Why a conversion produced nothing.
#[derive(Debug, thiserror::Error)]
pub enum ConvertError {
    /// The source could not be read.
    #[error(transparent)]
    Read(#[from] Error),
    /// The conversion would lose content, and [`Loss::Refuse`] was asked for.
    #[error(
        "not converted: {} item(s) would be lost — {}",
        lost.len(),
        lost.iter().map(|e| e.path.as_str()).collect::<Vec<_>>().join(", ")
    )]
    Lossy {
        /// What would be lost.
        lost: Vec<Entry>,
    },
    /// The target syntax's schema has no form for the model.
    #[error(transparent)]
    Unwritable(#[from] Unwritable),
}

/// Anything that stopped a document being read at all.
///
/// Which variants exist depends on the enabled features.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// An encoding an XML reader need not accept, declared or detected — named.
    #[error(
        "the document is encoded as {found}; EN 16931 XML must be UTF-8, or UTF-16 with a \
         byte-order mark"
    )]
    Encoding {
        /// The declared encoding, or what the bytes failed to be.
        found: String,
    },
    /// The text is not well-formed XML.
    #[error("not well-formed XML at line {line}, column {column}: {message}")]
    Xml {
        /// The parser's message.
        message: String,
        /// 1-based line.
        line: u32,
        /// 1-based column.
        column: u32,
    },
    /// Nested deeper than [`MAX_DEPTH`], refused before parsing: the XML
    /// parser recurses per level, and a stack overflow aborts the process.
    #[error(
        "nested {depth} elements deep, and the limit is {limit}. A document this deep is \
         not an invoice; it is a denial of service, and the XML parser would abort the \
         process rather than fail."
    )]
    TooDeep {
        /// How deep the document actually goes.
        depth: usize,
        /// The limit.
        limit: usize,
    },
    /// Well-formed, and not an invoice: the document element is neither UBL's
    /// nor CII's.
    #[error(
        "expected a UBL <Invoice> or <CreditNote>, a CII <CrossIndustryInvoice>, or a PDF; \
         found <{found}>"
    )]
    NotAnInvoice {
        /// The document element found.
        found: String,
    },
    /// A document this build cannot read: the feature that would is off.
    #[error("{found}, and this build has no `{feature}` feature")]
    Unsupported {
        /// What the bytes are — `"a PDF"`, `"a CII document"`.
        found: &'static str,
        /// The feature that reads it.
        feature: &'static str,
    },
    /// A hybrid PDF that could not be read.
    #[cfg(feature = "zugferd")]
    #[cfg_attr(docsrs, doc(cfg(feature = "zugferd")))]
    #[error(transparent)]
    Pdf(#[from] zugferd::Error),
}

/// Read a UBL or CII document, or a ZUGFeRD / Factur-X PDF, from its bytes.
///
/// The syntax is the document's own: a PDF signature, else the document
/// element. Duplicated single-occurrence elements are cut before the model is
/// read and reported as [`Kind::Duplicate`] entries; everything else the
/// reader could not map or hold is an entry too.
///
/// A PDF is read under [`zugferd::Limits::default`]; [`read_with`] reads it
/// under the caller's limits, [`receive`] under its [`Policy`]'s.
///
/// ```
/// # #[cfg(feature = "ubl")] {
/// let xml = br#"<Invoice xmlns="urn:oasis:names:specification:ubl:schema:xsd:Invoice-2"
///   xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2">
///   <cbc:ID>INV-1</cbc:ID><cbc:ID>FORGED</cbc:ID></Invoice>"#;
/// let read = en16931_formats::read(xml).expect("readable");
/// assert_eq!(read.invoice.number.as_deref(), Some("INV-1"));
/// let dup = read.entries_of(en16931_formats::report::Kind::Duplicate).next().expect("reported");
/// assert_eq!(dup.value.as_deref(), Some("FORGED"));
/// # }
/// ```
///
/// # Errors
///
/// [`Error`]: the bytes are not an invoice this build reads — the encoding,
/// the XML, the depth, the document element, or the PDF says why.
#[cfg(any(feature = "ubl", feature = "cii"))]
pub fn read(bytes: &[u8]) -> Result<Read, Error> {
    #[cfg(feature = "zugferd")]
    {
        read_with(bytes, &zugferd::Limits::default())
    }
    #[cfg(not(feature = "zugferd"))]
    {
        if bytes.starts_with(b"%PDF") {
            return Err(Error::Unsupported {
                found: "a PDF",
                feature: "zugferd",
            });
        }
        read_xml(bytes)
    }
}

/// [`read`], with the limits a PDF is extracted under. An XML document is read
/// as [`read`] reads it.
///
/// ```no_run
/// use en16931_formats::zugferd::Limits;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let pdf = std::fs::read("invoice.pdf")?;
/// let read = en16931_formats::read_with(&pdf, &Limits::new().with_max_stream_bytes(20 << 20))?;
/// # Ok(()) }
/// ```
///
/// # Errors
///
/// As [`read`]; a PDF beyond `limits` is [`Error::Pdf`] with
/// [`zugferd::Error::LimitExceeded`] or [`zugferd::Error::EmbeddedFilesTree`].
#[cfg(feature = "zugferd")]
#[cfg_attr(docsrs, doc(cfg(feature = "zugferd")))]
pub fn read_with(bytes: &[u8], limits: &zugferd::Limits) -> Result<Read, Error> {
    if bytes.starts_with(b"%PDF") {
        let got = zugferd::extract(bytes, limits)?;
        let mut read = read_xml(&got.xml)?;
        read.pdf = Some(got);
        return Ok(read);
    }
    read_xml(bytes)
}

/// Read an XML document: decode, parse, find and cut duplicates, read.
#[cfg(any(feature = "ubl", feature = "cii"))]
fn read_xml(bytes: &[u8]) -> Result<Read, Error> {
    let text = xml::decode(bytes)?;
    let doc = xml::parse(&text)?;
    let root = doc.root_element();
    // By local name **and** namespace: an `<Invoice>` in some other vocabulary
    // is not a UBL invoice, and reading it as one yields a silently empty model.
    let expected_ns = match xml::name(root) {
        "Invoice" => Some("urn:oasis:names:specification:ubl:schema:xsd:Invoice-2"),
        "CreditNote" => Some("urn:oasis:names:specification:ubl:schema:xsd:CreditNote-2"),
        "CrossIndustryInvoice" => {
            Some("urn:un:unece:uncefact:data:standard:CrossIndustryInvoice:100")
        }
        _ => None,
    };
    if let Some(ns) = expected_ns
        && root.tag_name().namespace() != Some(ns)
    {
        return Err(Error::NotAnInvoice {
            found: format!(
                "{} in namespace {:?}",
                xml::name(root),
                root.tag_name().namespace().unwrap_or("")
            ),
        });
    }
    let syntax = match xml::name(root) {
        #[cfg(feature = "ubl")]
        "Invoice" | "CreditNote" => Syntax::Ubl,
        #[cfg(not(feature = "ubl"))]
        "Invoice" | "CreditNote" => {
            return Err(Error::Unsupported {
                found: "a UBL document",
                feature: "ubl",
            });
        }
        #[cfg(feature = "cii")]
        "CrossIndustryInvoice" => Syntax::Cii,
        #[cfg(not(feature = "cii"))]
        "CrossIndustryInvoice" => {
            return Err(Error::Unsupported {
                found: "a CII document",
                feature: "cii",
            });
        }
        other => {
            return Err(Error::NotAnInvoice {
                found: other.to_owned(),
            });
        }
    };
    let (once, schema) = match syntax {
        #[cfg(feature = "ubl")]
        Syntax::Ubl => (ubl::once_table(), "the UBL 2.1 schema"),
        #[cfg(feature = "cii")]
        Syntax::Cii => (cii::once_table(), "the D16B schema"),
    };
    let duplicates = xml::duplicates(&doc, once);
    let mut entries: Vec<Entry> = duplicates
        .iter()
        .map(|d| xml::duplicate_entry(d, schema))
        .collect();
    let cleaned;
    let reparsed;
    let root = if duplicates.is_empty() {
        doc.root_element()
    } else {
        cleaned = xml::without(&text, &duplicates);
        reparsed = xml::parse(&cleaned)?;
        reparsed.root_element()
    };
    let invoice = match syntax {
        #[cfg(feature = "ubl")]
        Syntax::Ubl => {
            let mut r = ubl::read::Reader::default();
            let invoice = r.read(root);
            entries.extend(r.finish());
            invoice
        }
        #[cfg(feature = "cii")]
        Syntax::Cii => {
            let mut r = cii::read::Reader::default();
            let invoice = r.read(root);
            entries.extend(r.finish());
            invoice
        }
    };
    Ok(Read {
        invoice,
        syntax,
        entries,
        #[cfg(feature = "zugferd")]
        pdf: None,
    })
}

/// Write a **validated** invoice, stamping BT-24 from the profile it proved.
///
/// The proof makes an unvalidated invoice unwritable here, and BT-24 always
/// names the profile whose rules ran.
///
/// A profile that declares extension groups (the XRechnung Extension) has them
/// written; for any other, they are reported as [`Kind::Prohibited`].
///
/// ```
/// use en16931::profiles::XRechnung;
/// use en16931::validation::profile::Validated;
///
/// # fn demo(inv: en16931::Invoice) {
/// let Ok(proof) = Validated::<XRechnung>::new(inv) else { return };
/// let out = en16931_formats::write(&proof, en16931_formats::Syntax::Ubl);
/// assert!(out.xml.contains("xrechnung_3.0"));
/// # }
/// ```
#[cfg(any(feature = "ubl", feature = "cii"))]
#[must_use]
pub fn write<P: ProfileMarker>(validated: &Validated<P>, syntax: Syntax) -> Written {
    let mut invoice = validated.invoice().clone();
    invoice.specification_id = Some(P::PROFILE.specification_id.to_owned());
    let written = write_model(&invoice, syntax, P::PROFILE);
    debug_assert!(
        !written.entries.iter().any(|e| e.kind == Kind::Schema),
        "a proof carries every element the schemas require: {:?}",
        written.entries
    );
    written
}

/// Write a model nobody validated — a document you did not author, on its way
/// to another syntax.
///
/// BT-24 is written as the model carries it, and extension groups are written
/// when the profile it declares defines them. Everything the target could not
/// carry is in [`Written::entries`].
///
/// # Errors
///
/// [`Unwritable`] when the target syntax's schema has no form for the model —
/// no BT-1, no BT-2, and for UBL no seller, buyer or line. No public function
/// emits a document its schema rejects.
#[cfg(any(feature = "ubl", feature = "cii"))]
pub fn write_unchecked(invoice: &Invoice, syntax: Syntax) -> Result<Written, Unwritable> {
    let profile = en16931::profiles::classify(invoice.specification_id.as_deref())
        .profile()
        .unwrap_or(&en16931::profiles::EN16931);
    let written = write_model(invoice, syntax, profile);
    if written.entries.iter().any(|e| e.kind == Kind::Schema) {
        return Err(Unwritable {
            entries: written.entries,
        });
    }
    Ok(written)
}

#[cfg(any(feature = "ubl", feature = "cii"))]
#[cfg_attr(not(feature = "ubl"), allow(unused_variables))]
fn write_model(invoice: &Invoice, syntax: Syntax, profile: &'static en16931::Profile) -> Written {
    match syntax {
        #[cfg(feature = "ubl")]
        Syntax::Ubl => ubl::write::write(invoice, ubl::waivers(profile)),
        #[cfg(feature = "cii")]
        Syntax::Cii => cii::write::write(invoice),
    }
}

/// Convert a document to another syntax, in one call.
///
/// Read, write in `to`, and report: the result carries the document, the
/// source's report as declared, and **what was lost** — source content the
/// reader could not map or hold, and terms the target cannot or may not carry.
/// With [`Loss::Refuse`], any loss refuses the conversion and names it.
///
/// Validity is not a refusal: an invoice that breaks its profile's rules is
/// converted, and [`Converted::report`] says what it breaks.
///
/// # Errors
///
/// [`ConvertError::Read`] when the source cannot be read;
/// [`ConvertError::Lossy`] when something would be lost and `loss` is
/// [`Loss::Refuse`]; [`ConvertError::Unwritable`] when the target schema has
/// no form for the model.
#[cfg(any(feature = "ubl", feature = "cii"))]
pub fn convert(bytes: &[u8], to: Syntax, loss: Loss) -> Result<Converted, ConvertError> {
    let read = read(bytes)?;
    let written = write_unchecked(&read.invoice, to)?;
    let lost_entries: Vec<Entry> = read
        .entries
        .iter()
        .chain(&written.entries)
        .filter(|e| e.is_loss())
        .cloned()
        .collect();
    if loss == Loss::Refuse && !lost_entries.is_empty() {
        return Err(ConvertError::Lossy { lost: lost_entries });
    }
    let report = receive::assess(Ok(read), &Policy::default()).report_declared;
    Ok(Converted {
        document: written.xml,
        report,
        written: written.entries,
        lost: lost_entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_attribution_is_en16931s() {
        assert_eq!(ATTRIBUTION, en16931::ATTRIBUTION);
        assert!(ATTRIBUTION.contains("CEN"));
    }
}
