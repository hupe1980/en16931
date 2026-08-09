//! Getting an [`Invoice`] out of whatever the user pointed at.
//!
//! Three container formats and two syntaxes reach this crate, and a user does
//! not want to tell it which: a file is a UBL invoice, a CII invoice, or a
//! ZUGFeRD PDF with a CII invoice inside it, and the bytes say which. So the
//! command takes a path and works it out.
//!
//! # Nothing is read silently
//!
//! Both readers report what they could not map ([`ubl::Read::unmapped`]) and
//! what they could not represent (`malformed`). Those lists are the difference
//! between "this document validated" and "the parts of this document I
//! understood validated", so they travel with the invoice in [`Loaded::notes`]
//! and every output format prints them.

use std::path::{Path, PathBuf};

use en16931::Invoice;
use en16931_formats::{Syntax, cii, ubl, zugferd};

/// What a loaded document turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    /// XML on its own.
    Xml(Syntax),
    /// A hybrid PDF with the invoice embedded.
    Pdf {
        /// The syntax of the payload. Always CII in practice; read, not assumed.
        payload: Syntax,
    },
}

impl std::fmt::Display for Container {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Xml(Syntax::Ubl) => f.write_str("UBL 2.1"),
            Self::Xml(Syntax::Cii) => f.write_str("UN/CEFACT CII D16B"),
            Self::Pdf { .. } => f.write_str("ZUGFeRD / Factur-X (PDF/A-3 + CII)"),
            // `Syntax` is `#[non_exhaustive]`, so a new one must not be a panic.
            Self::Xml(other) => write!(f, "{other:?}"),
        }
    }
}

/// A document, read.
pub struct Loaded {
    /// Where it came from, for messages. `-` for standard input.
    pub source: PathBuf,
    /// What it turned out to be.
    pub container: Container,
    /// The semantic model.
    pub invoice: Invoice,
    /// Everything the reader could not map or could not represent.
    pub notes: Vec<String>,
    /// The ZUGFeRD profile, for a hybrid PDF.
    pub zugferd_profile: Option<zugferd::Profile>,
}

/// Why a document could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{0}: not UTF-8 — an EN 16931 XML document must be")]
    NotUtf8(PathBuf),
    #[error("{path}: {source}")]
    Ubl {
        path: PathBuf,
        #[source]
        source: ubl::Error,
    },
    #[error("{path}: {source}")]
    Cii {
        path: PathBuf,
        #[source]
        source: cii::Error,
    },
    #[error("{path}: {source}")]
    Zugferd {
        path: PathBuf,
        #[source]
        source: zugferd::Error,
    },
    /// Well-formed enough to read and not an e-invoice.
    #[error(
        "{0}: not an e-invoice. Expected a UBL <Invoice>/<CreditNote>, a CII \
         <CrossIndustryInvoice>, or a PDF with one embedded"
    )]
    Unrecognised(PathBuf),
}

/// Read the file at `path`, or standard input for `-`.
///
/// # Errors
/// [`Error`], naming the path — a batch run over a directory must say *which*
/// file failed.
pub fn load(path: &Path) -> Result<Loaded, Error> {
    let bytes = read_bytes(path)?;
    // The PDF signature, and the only sniff done on bytes rather than text: a
    // PDF is not UTF-8 and must not be turned into a lossy string first.
    if bytes.starts_with(b"%PDF") {
        return load_pdf(path, &bytes);
    }
    let xml = String::from_utf8(bytes).map_err(|_| Error::NotUtf8(path.to_owned()))?;
    load_xml(path, xml)
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, Error> {
    if path == Path::new("-") {
        use std::io::Read as _;
        let mut buf = Vec::new();
        std::io::stdin()
            .read_to_end(&mut buf)
            .map_err(|source| Error::Io {
                path: path.to_owned(),
                source,
            })?;
        return Ok(buf);
    }
    std::fs::read(path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })
}

fn load_xml(path: &Path, xml: String) -> Result<Loaded, Error> {
    let syntax =
        en16931_formats::sniff(&xml).ok_or_else(|| Error::Unrecognised(path.to_owned()))?;
    let (invoice, notes) = match syntax {
        Syntax::Ubl => {
            let r = ubl::from_str(&xml).map_err(|source| Error::Ubl {
                path: path.to_owned(),
                source,
            })?;
            (r.invoice, join(&r.unmapped, &r.malformed))
        }
        Syntax::Cii => {
            let r = cii::from_str(&xml).map_err(|source| Error::Cii {
                path: path.to_owned(),
                source,
            })?;
            (r.invoice, join(&r.unmapped, &r.malformed))
        }
        _ => return Err(Error::Unrecognised(path.to_owned())),
    };
    Ok(Loaded {
        source: path.to_owned(),
        container: Container::Xml(syntax),
        invoice,
        notes,
        zugferd_profile: None,
    })
}

fn load_pdf(path: &Path, bytes: &[u8]) -> Result<Loaded, Error> {
    let got = zugferd::extract(bytes).map_err(|source| Error::Zugferd {
        path: path.to_owned(),
        source,
    })?;
    let mut notes = got.syntax_findings.clone();
    notes.extend(got.divergence.iter().map(ToString::to_string));
    // `extract` hands back the payload whether or not it parsed as CII, because
    // the bytes are what you diagnose with. If it did not parse, say so here
    // rather than validating an empty invoice and reporting a hundred findings
    // about a document that was never read.
    let invoice = got.invoice.clone().ok_or_else(|| Error::Cii {
        path: path.to_owned(),
        source: cii::Error::NotCii(format!(
            "embedded {} could not be read as CII",
            got.filename
        )),
    })?;
    Ok(Loaded {
        source: path.to_owned(),
        container: Container::Pdf {
            payload: Syntax::Cii,
        },
        invoice,
        notes,
        zugferd_profile: Some(got.profile),
    })
}

/// The reader's two lists, labelled so a reader of the output can tell them
/// apart — "not mapped" and "not representable" mean different repairs.
fn join(unmapped: &[String], malformed: &[String]) -> Vec<String> {
    unmapped
        .iter()
        .map(|s| format!("unmapped element: {s}"))
        .chain(malformed.iter().map(|s| format!("unreadable value: {s}")))
        .collect()
}
