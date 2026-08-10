//! The UN/CEFACT Cross Industry Invoice **D16B** binding, both directions.
//!
//! CII has **one** document element where UBL has two — `rsm:CrossIndustryInvoice`
//! carries invoices and credit notes alike, distinguished only by BT-3. So the
//! writer never branches on [`en16931::DocumentKind`], and the reader infers it
//! from the type code.
//!
//! # Three things CII does differently
//!
//! **Dates are wrapped and formatted.** Where UBL writes
//! `<cbc:IssueDate>2026-01-15</cbc:IssueDate>`, CII writes
//! `<ram:IssueDateTime><udt:DateTimeString format="102">20260115</udt:DateTimeString></ram:IssueDateTime>`.
//! Format `102` is `CCYYMMDD`; it is the only format EN 16931 permits, and
//! [`read`] rejects anything else rather than guessing.
//!
//! **Allowances and charges share an element.** `ram:SpecifiedTradeAllowanceCharge`
//! carries both, told apart by `ram:ChargeIndicator/udt:Indicator`. UBL does the
//! same with `cbc:ChargeIndicator`, so the model's split into `allowances` and
//! `charges` costs one boolean in each binding.
//!
//! **Party identifiers split by whether they have a scheme.** A scheme-qualified
//! BT-29 is `ram:GlobalID`; an unqualified one is `ram:ID`. Two elements for one
//! repeatable business term, and the sequence fixes their order — so a party
//! carrying both kinds gets them back in a **different order** than they went
//! in. That is not lossy: EN 16931 gives the order of repeated BT-29 occurrences
//! no meaning, and the set is preserved exactly. It is stated here because a
//! round-trip test that compared them positionally would fail for a reason that
//! looks like a bug and is not.
//!
//! **The document is three-part.** `ExchangedDocumentContext` (BT-23, BT-24),
//! `ExchangedDocument` (BT-1, BT-2, BT-3, notes), and
//! `SupplyChainTradeTransaction` (everything else, split across *agreement*,
//! *delivery* and *settlement*). Which of those three a term lives in is not
//! guessable, which is why the element order here was derived from 170
//! published instances rather than recalled — see [`order`].
//!
//! ```
//! use en16931::Invoice;
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let xml = en16931_formats::cii::to_string(&Invoice::default());
//! assert!(xml.contains("CrossIndustryInvoice"));
//!
//! let back = en16931_formats::cii::from_str(&xml)?;
//! assert!(back.unmapped.is_empty());
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

/// The CII namespaces. Fixed by UN/CEFACT; not configurable.
pub mod ns {
    /// The document element's namespace.
    pub const RSM: &str = "urn:un:unece:uncefact:data:standard:CrossIndustryInvoice:100";
    /// Reusable aggregate business information entities.
    pub const RAM: &str =
        "urn:un:unece:uncefact:data:standard:ReusableAggregateBusinessInformationEntity:100";
    /// Unqualified data types — where `udt:DateTimeString` and `udt:Indicator` live.
    pub const UDT: &str = "urn:un:unece:uncefact:data:standard:UnqualifiedDataType:100";
    /// Qualified data types.
    pub const QDT: &str = "urn:un:unece:uncefact:data:standard:QualifiedDataType:100";
}

/// Write an invoice as CII, discarding the report of anything the syntax could
/// not carry.
///
/// Use [`fn@write`] when that might matter.
///
/// # Why this returns `String` and not `Result<String, _>`
///
/// **Serialisation cannot fail, and that is a property of the model rather than
/// an omission.** Every field of [`Invoice`] already holds a value CII can
/// carry: [`en16931::InvoiceAmount`] cannot hold a third decimal, and
/// [`en16931::Date`] cannot hold something that is not a calendar day — which
/// is exactly what `udt:DateTimeString format="102"` accepts, and nothing more.
/// There is no state a writer could be handed that it would have to refuse, and
/// writing into a `String` does no I/O.
///
/// **Validity is a separate question, and it is the caller's.** An invoice with
/// no seller serialises perfectly into a document no counterparty will accept.
/// Run [`en16931::validate`] first — or use [`to_string_for`], which will not
/// hand you a document until you have.
#[must_use]
pub fn to_string(invoice: &Invoice) -> String {
    write(invoice).xml
}

/// Write an invoice as CII **for a profile**, or refuse and say why.
///
/// The CII twin of [`crate::ubl::to_string_for`], and the one that matters for
/// ZUGFeRD: every ZUGFeRD payload is CII, and a hybrid PDF carrying an invalid
/// one is a document that looks right to a human and is rejected by a machine.
///
/// ```
/// use en16931::profiles::XRECHNUNG;
///
/// # fn demo(invoice: &en16931::Invoice) {
/// match en16931_formats::cii::to_string_for(invoice, &XRECHNUNG) {
///     Ok(xml) => embed_in_pdf(&xml),
///     Err(e) => eprintln!("{e}\n{}", e.report()),
/// }
/// # }
/// # fn embed_in_pdf(_: &str) {}
/// ```
///
/// # Errors
/// [`NotValid`](crate::NotValid), carrying the full report, when any fatal
/// finding was raised.
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

/// Write a **validated** invoice, stamping BT-24 from the profile it proved.
///
/// The same guarantee [`crate::ubl::write_validated`] gives, in the other
/// syntax: an unvalidated invoice cannot be serialised, and BT-24 cannot
/// disagree with the rules that were actually applied.
#[must_use]
pub fn write_validated<P>(validated: &en16931::validation::profile::Validated<P>) -> Written
where
    P: en16931::validation::profile::ProfileMarker,
{
    let mut inv = validated.invoice().clone();
    inv.specification_id = Some(P::PROFILE.specification_id.to_owned());
    write(&inv)
}

/// What reading a CII document produced.
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
    /// Values present but not representable in the model.
    ///
    /// `en16931`'s types refuse these at the boundary, so the field is left
    /// absent and the fact recorded here. Without this list a `BR-03` finding
    /// on such a document would be unexplainable.
    pub malformed: Vec<String>,
}

/// Anything that stopped a document being read at all.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The bytes are not well-formed XML.
    #[error("not well-formed XML: {0}")]
    Xml(#[from] roxmltree::Error),
    /// Well-formed, but the document element is not a CII invoice.
    ///
    /// Reported rather than guessed at, because a UBL document handed to the
    /// CII reader should say so and not return an empty invoice.
    #[error("expected a CII CrossIndustryInvoice, found <{0}>")]
    NotCii(String),
    /// Nested deeper than [`MAX_DEPTH`], and refused **before** being parsed.
    ///
    /// Not a style objection. The XML parser recurses once per level and aborts
    /// the process on a stack overflow, which Rust cannot catch — so a document
    /// nested a few hundred deep took the whole program down. CII's content
    /// model is about a dozen levels deep at its worst, so nothing lawful is
    /// anywhere near this.
    #[error(
        "nested {depth} elements deep, and the limit is {limit}. A document this \
         deep is not a CII invoice; it is a denial of service, and the XML parser \
         would abort the process rather than fail."
    )]
    TooDeep {
        /// How deep the document actually goes.
        depth: usize,
        /// [`MAX_DEPTH`].
        limit: usize,
    },
}

/// The deepest element nesting [`from_str`] will accept — see [`Error::TooDeep`].
pub const MAX_DEPTH: usize = crate::xml::MAX_DEPTH;

/// Read a CII `rsm:CrossIndustryInvoice`.
///
/// # Errors
///
/// [`Error::TooDeep`] if the input is nested past [`MAX_DEPTH`],
/// [`Error::Xml`] if it is not well-formed, [`Error::NotCii`] if the
/// document element is something else.
pub fn from_str(xml: &str) -> Result<Read, Error> {
    let depth = crate::xml::max_depth(xml);
    if depth > MAX_DEPTH {
        return Err(Error::TooDeep {
            depth,
            limit: MAX_DEPTH,
        });
    }
    let doc = roxmltree::Document::parse(xml)?;
    let root = doc.root_element();
    let name = root.tag_name().name();
    if name != "CrossIndustryInvoice" {
        return Err(Error::NotCii(name.to_owned()));
    }
    let mut r = Reader::default();
    let invoice = r.read(root);
    Ok(Read {
        invoice,
        unmapped: r.unmapped.into_iter().collect(),
        malformed: r.malformed,
    })
}
