//! The UBL 2.1 binding, both directions.
//!
//! UBL has **two document elements** where CII has one: `Invoice` and
//! `CreditNote`, in different namespaces, with different names for the type
//! code and the line. [`en16931::DocumentKind`] is what selects between them,
//! and it exists in the model precisely because CEN's own credit-note fixtures
//! carry no BT-3 to infer it from.
//!
//! ```
//! use en16931::Invoice;
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let xml = en16931_formats::ubl::to_string(&Invoice::default());
//! assert!(xml.starts_with("<?xml"));
//!
//! let back = en16931_formats::ubl::from_str(&xml)?;
//! assert_eq!(back.invoice.kind, Invoice::default().kind);
//! # Ok(()) }
//! ```

pub mod order;
pub mod prohibitions;
pub mod read;
pub mod write;

#[doc(hidden)]
pub mod prohibitions_generated;

use en16931::Invoice;

pub use read::Reader;
pub use write::{Written, write};

/// Write a **validated** invoice, stamping BT-24 from the profile it proved.
///
/// This is the path worth using, and the reason `en16931` has a typed proof at
/// all. Two things become impossible rather than merely discouraged:
///
/// * An unvalidated invoice cannot be serialised — there is no way to construct
///   the argument without having run the rules.
/// * BT-24 cannot disagree with the rules that were actually applied. A
///   document claiming `XRechnung` 3.0 because someone typed the string, having
///   been checked against the bare core model, is the single most common way an
///   invoice passes local validation and is rejected on receipt.
///
/// ```
/// use en16931::profiles::XRechnung;
/// use en16931::validation::profile::Validated;
///
/// # fn demo(inv: en16931::Invoice) {
/// let Ok(proof) = Validated::<XRechnung>::new(inv) else { return };
/// let out = en16931_formats::ubl::write_validated(&proof);
/// assert!(out.xml.contains("xrechnung_3.0"));
/// # }
/// ```
#[must_use]
pub fn write_validated<P>(validated: &en16931::validation::profile::Validated<P>) -> Written
where
    P: en16931::validation::profile::ProfileMarker,
{
    let mut inv = validated.invoice().clone();
    inv.specification_id = Some(P::PROFILE.specification_id.to_owned());
    write(&inv)
}

/// Write an invoice as UBL, discarding the report of anything the syntax could
/// not carry.
///
/// A convenience for the common case where the model is known to fit — which is
/// every ordinary invoice. Use [`fn@write`] when it might not: BT-11 on a credit
/// note has nowhere to go, and finding that out from the counterparty is worse
/// than finding it out here.
///
/// # Why this returns `String` and not `Result<String, _>`
///
/// **Serialisation cannot fail, and that is a property of the model rather than
/// an omission.** Every field of [`Invoice`] already holds a value UBL can
/// carry: [`en16931::InvoiceAmount`] cannot hold a third decimal,
/// [`en16931::Date`] cannot hold something that is not a calendar day, a code is
/// a string. There is no state a writer could be handed that it would have to
/// refuse. Writing into a `String` does no I/O, so there is nothing else to go
/// wrong either.
///
/// **Validity is a separate question, and it is the caller's.** An invoice with
/// no seller serialises perfectly into a document no counterparty will accept.
/// Run [`en16931::validate`] first — or use [`to_string_for`], which will not
/// hand you a document until you have.
#[must_use]
pub fn to_string(invoice: &Invoice) -> String {
    write(invoice).xml
}

/// Write an invoice as UBL **for a profile**, or refuse and say why.
///
/// The call that turns "we forgot to validate before submitting" from a
/// rejection letter into an `Err` on the line that would have shipped it. It
/// validates against `profile`, stamps BT-24 from it, and only then writes.
///
/// ```
/// use en16931::profiles::XRECHNUNG;
///
/// # fn demo(invoice: &en16931::Invoice) {
/// match en16931_formats::ubl::to_string_for(invoice, &XRECHNUNG) {
///     Ok(xml) => submit(&xml),
///     // Neither a panic nor a silent fallback: the report says exactly which
///     // BR-DE rules the document still owes.
///     Err(e) => eprintln!("{e}\n{}", e.report()),
/// }
/// # }
/// # fn submit(_: &str) {}
/// ```
///
/// # This and [`write_validated`] are the same guarantee, twice
///
/// [`write_validated`] takes a [`Validated<P>`] and is *unconditional* — the
/// proof was produced elsewhere and the type carries it. Reach for that when
/// the proof travels: across a function boundary, into a queue, through a trait.
///
/// This one validates on the spot and returns a `Result`. Reach for it when the
/// profile is a runtime choice, which it is whenever a counterparty's preferred
/// CIUS comes out of a database rather than out of the source.
///
/// Neither can produce a document whose BT-24 disagrees with the rules that
/// were actually run.
///
/// # Errors
/// [`NotValid`](crate::NotValid), carrying the full report, when any fatal
/// finding was raised.
///
/// [`Validated<P>`]: en16931::validation::profile::Validated
/// [`write_validated`]: fn@write_validated
pub fn to_string_for(
    invoice: &Invoice,
    profile: &'static en16931::validation::profile::Profile,
) -> Result<String, crate::NotValid> {
    write_for(invoice, profile).map(|w| w.xml)
}

/// As [`to_string_for`], keeping the report of anything the syntax could not
/// carry.
///
/// # Errors
/// [`NotValid`](crate::NotValid), carrying the full report, when any fatal
/// finding was raised.
pub fn write_for(
    invoice: &Invoice,
    profile: &'static en16931::validation::profile::Profile,
) -> Result<Written, crate::NotValid> {
    crate::prepare_for(invoice, profile).map(|inv| write(&inv))
}

/// The UBL namespaces, for a caller that needs to recognise a document before
/// handing it over.
pub mod ns {
    /// `oasis:…:Invoice-2`.
    pub const INVOICE: &str = "urn:oasis:names:specification:ubl:schema:xsd:Invoice-2";
    /// `oasis:…:CreditNote-2`.
    pub const CREDIT_NOTE: &str = "urn:oasis:names:specification:ubl:schema:xsd:CreditNote-2";
}

/// What reading a UBL document produced.
///
/// The unmapped and malformed lists are not diagnostics to be ignored: a reader
/// that returns an `Invoice` and says nothing about the six elements it skipped
/// is how a validation run comes back green having checked nothing.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Read {
    /// The model.
    pub invoice: Invoice,
    /// Element paths encountered and not mapped, as `Parent/Child`.
    ///
    /// Non-empty means the document carried something outside the EN 16931
    /// subset — an extension, another CIUS's addition, or a syntax rule
    /// violation. It never means the reader gave up quietly.
    pub unmapped: Vec<String>,
    /// Values present in the document but not representable in the model:
    /// `<cbc:IssueDate>123</cbc:IssueDate>`, `<cbc:Amount>.00</cbc:Amount>`.
    ///
    /// `en16931`'s types refuse these at the boundary, so the field is left
    /// absent and the fact is recorded here. Without this list a `BR-03`
    /// finding on such a document would be unexplainable.
    pub malformed: Vec<String>,
}

/// Anything that stopped a document being read at all.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The bytes are not well-formed XML.
    #[error("not well-formed XML: {0}")]
    Xml(#[from] roxmltree::Error),
    /// Well-formed, but the document element is not a UBL invoice or credit
    /// note. Reported rather than guessed at, because a CII document handed to
    /// the UBL reader should say so and not return an empty invoice.
    #[error("expected a UBL Invoice or CreditNote, found <{0}>")]
    NotUbl(String),
}

/// Read a UBL `Invoice` or `CreditNote`.
///
/// # Errors
///
/// [`Error::Xml`] if the input is not well-formed, [`Error::NotUbl`] if the
/// document element is something else.
pub fn from_str(xml: &str) -> Result<Read, Error> {
    let doc = roxmltree::Document::parse(xml)?;
    let root = doc.root_element();
    let name = root.tag_name().name();
    if !matches!(name, "Invoice" | "CreditNote") {
        return Err(Error::NotUbl(name.to_owned()));
    }
    let mut r = Reader::default();
    let invoice = r.read(root);
    Ok(Read {
        invoice,
        unmapped: r.unmapped.into_iter().collect(),
        malformed: r.malformed,
    })
}
