//! **European e-invoicing formats**, on top of the EN 16931 semantic model.
//!
//! ```text
//!         ┌──────────────┐                       ┌──────────────────────┐
//!         │   billing    │                       │  inbound documents   │
//!         │ calculations │                       │  UBL / CII / ZUGFeRD │
//!         └──────┬───────┘                       └──────────┬───────────┘
//!                │  adapter (optional feature)              │
//!                └──────────────┬───────────────────────────┘
//!                               ▼
//!                     ┌─────────────────────┐
//!                     │      en16931        │  semantic model, 317 rules,
//!                     │  proof of validity  │  no XML, no PDF, no I/O
//!                     └──────────┬──────────┘
//!                                ▼
//!                     ┌─────────────────────┐
//!                     │  en16931-formats    │  ← you are here
//!                     │   UBL · CII · PDF   │
//!                     └─────────────────────┘
//! ```
//!
//! [`en16931`] decides whether an invoice is **correct**. This crate decides
//! what it looks like **on the wire**, and re-implements not one of the 317
//! rules.
//!
//! # Why one crate, not one per format
//!
//! XRechnung is carried in UBL *and* CII; every ZUGFeRD payload is CII. A crate
//! per format would need the CII binding twice, and two bindings drift. Cargo
//! features already express which syntax a consumer wants, so a crate boundary
//! here would be solving with a package what `--no-default-features` solves for
//! free.
//!
//! What *is* a separate crate is [`en16931`], and that boundary is
//! load-bearing: this crate depends on it, so rustc forbids the reverse. "The
//! semantic rules do not depend on a syntax" is enforced rather than asked for,
//! and `en16931`'s dependency graph stays at ten crates and builds for
//! `wasm32`.
//!
//! # Features, and what each one costs
//!
//! | Feature | Default | Graph | What |
//! |---|---|---|---|
//! | [`ubl`] | ✅ | 13 crates | UBL 2.1, both directions |
//! | [`cii`] | — | 13 crates | UN/CEFACT CII D16B, both directions |
//! | [`zugferd`] | — | **57 crates** | ZUGFeRD / Factur-X hybrid PDFs — **reading only**, see [`zugferd`] |
//!
//! `zugferd` is off by default and that matters: `lopdf` brings AES, ChaCha20,
//! SHA-2, `getrandom` and `libc`, and the result does not build for
//! `wasm32-unknown-unknown`. Nobody reading a UBL invoice should pay for that.
//!
//! # The 91 % that costs a writer nothing
//!
//! CEN's artefacts carry **1 339** syntax rules, and **1 218 of them (91 %)**
//! say some element "shall not be used" — they fence off the parts of UBL 2.1
//! and CII D16B that EN 16931 does not use. That inverts the usual expectation:
//!
//! * A **writer** driven from the semantic model cannot violate them. It has no
//!   way to express `cbc:UUID`, because the model has no term for it. They are
//!   *unreachable*, not cheaply satisfied — the same shape as `InvoiceAmount`
//!   making `BR-DEC-*` unrepresentable. So the writer answers to roughly **119**
//!   real rules.
//! * A **reader** must cope with all 1 339, because the document came from
//!   somewhere else.
//!
//! Unreachability is a claim, so the serialiser enforces it against the
//! prohibitions extracted from CEN's own Schematron, and `tests/subset.rs`
//! asserts the writer never needs that safety net. See [`ubl::prohibitions`].
//!
//! # Quick start
//!
//! ```
//! # #[cfg(feature = "ubl")] {
//! use en16931::Invoice;
//!
//! let xml = en16931_formats::ubl::to_string(&Invoice::default());
//! let read = en16931_formats::ubl::from_str(&xml).expect("readable");
//! assert!(read.unmapped.is_empty(), "nothing was silently dropped");
//! # }
//! ```
//!
//! # Reading a document somebody else wrote
//!
//! Which is the only kind worth reading. Three ways an inbound document can
//! attack the reader rather than inform it, and what happens to each:
//!
//! | | |
//! |---|---|
//! | **entity expansion** (billion laughs) | needs a DTD; the parser rejects every document carrying one |
//! | **external entities** (XXE, file disclosure) | the same DTD refusal, for the same reason |
//! | **nesting** | refused past [`ubl::MAX_DEPTH`] before parsing — see below |
//!
//! The third is the one that had teeth. `roxmltree` recurses once per level of
//! nesting and overflows the stack a few hundred levels in, and a stack overflow
//! is **not a panic**: Rust cannot unwind it and cannot catch it, so the process
//! aborts. Two lines of XML took down the caller, with no report and nothing for
//! `?` to catch.
//!
//! It cannot be handled afterwards, so it is refused before: one linear scan of
//! the bytes, then [`ubl::Error::TooDeep`] or its CII twin. The limit is 64 and
//! the deepest of the 487 published instances in the artefact tree is **nine**,
//! which `tests/corpus.rs` measures rather than assumes.
//!
//! What this crate does **not** do is bound input size or time — a caller who
//! reads from a socket owns that decision, and a library that quietly capped it
//! would be wrong for the batch job and useless for the endpoint.
//!
//! [`ubl::MAX_DEPTH`]: crate::ubl::MAX_DEPTH
//! [`ubl::Error::TooDeep`]: crate::ubl::Error::TooDeep
//!
//! # Attribution
//!
//! The bindings are derived from CEN's EUPL-1.2 validation artefacts and the
//! element order from the authorities' published instances. The notice is a
//! licence condition, not decoration, and it has a test.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs, clippy::pedantic)]
#![allow(
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    // The writer is one function per aggregate and `write` is a straight walk
    // of the document element's sequence. Splitting it to satisfy a line count
    // would scatter the order it exists to express.
    clippy::too_many_lines,
    // `en16931::invoice::*` is the model. Naming forty types individually in
    // three files is noise, not clarity.
    clippy::wildcard_imports
)]

#[cfg(feature = "cii")]
#[cfg_attr(docsrs, doc(cfg(feature = "cii")))]
pub mod cii;
#[cfg(feature = "ubl")]
#[cfg_attr(docsrs, doc(cfg(feature = "ubl")))]
pub mod ubl;
pub mod xrechnung;

#[cfg(any(feature = "ubl", feature = "cii"))]
mod xml;
#[cfg(feature = "zugferd")]
#[cfg_attr(docsrs, doc(cfg(feature = "zugferd")))]
pub mod zugferd;

pub use xrechnung::{Flavour, detect};

/// A document was **not** written, because it did not pass the profile it was
/// asked to be written for.
///
/// Returned by [`ubl::to_string_for`] / [`ubl::write_for`] and their CII twins.
///
/// # Why an error type rather than the report itself
///
/// [`en16931::ValidationReport`] is a *product* — you store it, diff it, show
/// it to an operator — and it deliberately does not implement
/// [`std::error::Error`]. `report.is_valid()` being false is an ordinary
/// outcome of validating, not a failure of validation.
///
/// It *is* an error when the thing you asked for was a shippable document. This
/// type is that framing, so `?` works and the message says what happened rather
/// than spilling forty findings into a log line. The report is right there in
/// [`report`](Self::report) when you want it.
///
/// [`ubl::to_string_for`]: crate::ubl::to_string_for
/// [`ubl::write_for`]: crate::ubl::write_for
#[derive(Debug)]
pub struct NotValid {
    profile: &'static str,
    // Boxed: a report carries a `Vec<Finding>` and several `String`s, and the
    // success arm of these `Result`s is one `String`. Unboxed, the error would
    // widen every one of them.
    report: Box<en16931::ValidationReport>,
}

impl NotValid {
    /// The full report — every finding, in the order the validator produced it.
    #[must_use]
    pub fn report(&self) -> &en16931::ValidationReport {
        &self.report
    }

    /// Take the report.
    #[must_use]
    pub fn into_report(self) -> en16931::ValidationReport {
        *self.report
    }

    /// The profile the invoice was checked against — `"XRechnung 3.0"`.
    #[must_use]
    pub fn profile(&self) -> &'static str {
        self.profile
    }
}

impl std::fmt::Display for NotValid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "not written: the invoice does not satisfy {} ({} fatal finding(s)); \
             see `NotValid::report` for all of them",
            self.profile,
            self.report.fatal().count()
        )
    }
}

impl std::error::Error for NotValid {}

/// Validate against `profile`, stamping BT-24 from it, or say why not.
///
/// Shared by both syntaxes: the check is about the *model*, so doing it twice
/// would be two places for it to differ.
///
/// The stamp happens **before** validation, not after. `BR-01` requires BT-24
/// and XRechnung's `BR-DE-21` constrains its value, so a document validated
/// carrying the caller's BT-24 and shipped carrying the profile's would have
/// been checked as something other than what it claims to be.
#[cfg(any(feature = "ubl", feature = "cii"))]
fn prepare_for(
    invoice: &en16931::Invoice,
    profile: &'static en16931::validation::profile::Profile,
) -> Result<en16931::Invoice, NotValid> {
    let mut inv = invoice.clone();
    inv.specification_id = Some(profile.specification_id.to_owned());
    let report = profile.validate(&inv);
    if report.is_valid() {
        Ok(inv)
    } else {
        Err(NotValid {
            profile: profile.id,
            report: Box::new(report),
        })
    }
}

/// The CEN attribution notice, as [`en16931`] carries it.
///
/// Re-exported rather than restated, so the two cannot drift.
pub const ATTRIBUTION: &str = en16931::ATTRIBUTION;

/// Which syntax a document is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Syntax {
    /// OASIS UBL 2.1 — `Invoice` or `CreditNote`.
    Ubl,
    /// UN/CEFACT Cross Industry Invoice D16B.
    Cii,
}

/// Guess the syntax from the document element, without parsing.
///
/// Returns `None` rather than guessing when the root is neither — "not an
/// e-invoice at all" and "an e-invoice in the other syntax" need different
/// messages, and a caller that cannot tell them apart writes a bad one.
///
/// # The prologue is skipped properly, not by shape
///
/// Splitting on `<` and taking the first piece that begins with neither `?` nor
/// `!` reads a comment's *contents* as markup, so a document opening with
///
/// ```xml
/// <!-- exported from <ERP> 4.2 -->
/// <Invoice …>
/// ```
///
/// would sniff as `ERP` and come back `None`, for a perfectly ordinary UBL
/// invoice. A comment, a processing instruction and a doctype each have their own
/// terminator, so each is skipped by its own.
#[must_use]
pub fn sniff(xml: &str) -> Option<Syntax> {
    let mut rest = xml;
    let name = loop {
        rest = &rest[rest.find('<')? + 1..];
        if let Some(body) = rest.strip_prefix("!--") {
            // A comment may contain anything at all, `<` included.
            rest = &body[body.find("-->")? + 3..];
        } else if rest.starts_with('?') {
            rest = &rest[rest.find("?>")? + 2..];
        } else if rest.starts_with('!') {
            // `<!DOCTYPE …>`. An internal subset re-enters this loop on its own
            // `<!ENTITY …>` declarations and falls out at the real root.
            rest = &rest[rest.find('>')? + 1..];
        } else {
            break rest
                .split([' ', '>', '\t', '\n', '\r', '/'])
                .next()?
                .rsplit(':')
                .next()?;
        }
    };
    match name {
        "Invoice" | "CreditNote" => Some(Syntax::Ubl),
        "CrossIndustryInvoice" => Some(Syntax::Cii),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniff_distinguishes_the_syntaxes() {
        assert_eq!(
            sniff("<?xml version=\"1.0\"?><Invoice xmlns=\"x\">"),
            Some(Syntax::Ubl)
        );
        assert_eq!(sniff("<CreditNote>"), Some(Syntax::Ubl));
        assert_eq!(sniff("<rsm:CrossIndustryInvoice>"), Some(Syntax::Cii));
        assert_eq!(sniff("<html>"), None);
        assert_eq!(sniff(""), None);
    }

    /// A prologue or comment before the root must not be mistaken for it.
    #[test]
    fn sniff_skips_the_prologue() {
        assert_eq!(
            sniff("<?xml?>\n<!-- a note -->\n<Invoice>"),
            Some(Syntax::Ubl)
        );
    }

    /// A comment's *contents* are not markup, however much they look like it.
    ///
    /// Real exporters write their own name in a banner comment, and one of them
    /// writing it in angle brackets made a perfectly ordinary UBL invoice sniff
    /// as "not an e-invoice".
    #[test]
    fn sniff_is_not_fooled_by_markup_inside_a_comment() {
        assert_eq!(
            sniff("<!-- exported from <ERP> 4.2 -->\n<Invoice/>"),
            Some(Syntax::Ubl)
        );
        assert_eq!(
            sniff("<!--<CrossIndustryInvoice>--><Invoice/>"),
            Some(Syntax::Ubl),
            "the comment names the other syntax; the document is still UBL"
        );
        assert_eq!(
            sniff("<?xml version=\"1.0\"?><!DOCTYPE Invoice><rsm:CrossIndustryInvoice>"),
            Some(Syntax::Cii)
        );
        // An unterminated comment has no document element to find.
        assert_eq!(sniff("<!-- never closed <Invoice>"), None);
    }

    /// The notice travels with the crate, and is not a second copy that can rot.
    #[test]
    fn the_attribution_is_en16931s() {
        assert_eq!(ATTRIBUTION, en16931::ATTRIBUTION);
        assert!(ATTRIBUTION.contains("CEN"));
    }
}
