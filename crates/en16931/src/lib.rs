//! # en16931 — the European e-invoice, as Rust types
//!
//! `billing` proves an invoice is *arithmetically* correct. This crate proves it
//! is *legally meaningful*: it holds the EN 16931 semantic data model, decides
//! what the standard and its national usage specifications demand, and hands a
//! proof of that decision to the syntax layer.
//!
//! **It never emits a byte of XML.** UBL, CII and PDF/A-3 belong to
//! `en16931-formats`, the sibling crate in this workspace; the 1 339
//! syntax-binding rules belong with them. This crate owns the 223 that are
//! syntax-independent, which are the only ones it can meaningfully check.
//!
//! ## Validate the model, not the document
//!
//! Every other implementation in this space is *XML in, Schematron out*, so the
//! loop is build → serialise → validate → parse the error → guess which field it
//! meant. Here a finding points at `lines[3]` BT-151, and you can validate an
//! invoice you are still assembling.
//!
//! ## Design invariants
//!
//! - **No `f64`.** Amounts are fixed-point; rates and quantities are `Decimal`.
//! - **No I/O, no async, no `unsafe`** — so `wasm32` works, and an invoice never
//!   has to leave the client.
//! - **Rounding is never implicit.** An amount that does not fit two decimals is
//!   an error, not a rounding opportunity. Reduce precision at the source.
//! - **Mandatory means non-`Option`.** Where EN 16931 says 1..1, the type says
//!   so. Rules exist for the cardinalities the type system cannot express, not
//!   as a substitute for it.
//! - **Invariants survive deserialisation**, via `#[serde(try_from = ...)]` —
//!   asserted in `tests/serde_invariants.rs`, not merely intended.
//!
//! ## The ten semantic data types
//!
//! EN 16931-1 §6.5 defines exactly ten, and every one of the 164 business terms
//! has one. Mirroring them one-for-one is the crate's organising principle, so
//! "which Rust type does BT-*n* get?" is a lookup in Table 2 rather than a
//! judgement call.
//!
//! | §6.5 | Semantic type | Here |
//! |---|---|---|
//! | 6.5.2 | Amount | [`InvoiceAmount`] — `i64` minor units, no third decimal |
//! | 6.5.3 | Unit Price Amount | [`UnitPriceAmount`] — `Decimal`, **no** cap |
//! | 6.5.4 | Quantity | [`Quantity`] — `Decimal`, may be negative |
//! | 6.5.5 | Percentage | [`Percentage`] — per cent (`19`), not a fraction |
//! | 6.5.6 | Identifier | [`Identifier`] — content + scheme + scheme **version** |
//! | 6.5.7 | Document Reference | [`DocumentReference`] — deliberately no scheme |
//! | 6.5.8 | Code | [`codes`] — 4 887 values, generated and re-verified |
//! | 6.5.9 | Date | [`Date`] — a calendar day, no time of day |
//! | 6.5.10 | Text | `String` — 62 of the 164 terms |
//! | 6.5.11 | Binary Object | [`Attachment`] — mime and filename mandatory |
//!
//! ## Status
//!
//! The ten semantic data types, all eighteen code lists, the [`invoice`] model,
//! and a [`validation`] engine that registers **all 223 syntax-independent
//! rules** of the pinned CEN artefacts — every `BR-*`, `BR-CO-*`, `BR-CL-*` and
//! all nine VAT category families. `tests/codelists.rs` asserts that 223 against
//! the artefacts on any machine that has them, so it is measured rather than
//! claimed.
//!
//! Of the **317** rules registered across every shipped profile:
//!
//! | | | |
//! |---|---:|---|
//! | retired by the types | 53 | no state can make them fire — `BT-112` is not an `Option` |
//! | undecidable | 4 | `BR-CO-05`…`-08`; **CEN's own binding is `value="true()"`** |
//! | checkable | 260 | **every one exercised by its own failing fixture** |
//!
//! And the profile rule sets are complete against *their* authorities too,
//! asserted the same way:
//!
//! | [`profiles`] | Rules run | Artefact coverage |
//! |---|---:|---|
//! | EN 16931 core | 227 | 223 / 223 CEN syntax-independent |
//! | XRechnung 3.0 | 282 | **55 / 55** KoSIT UBL asserts + **21 / 21** merged Peppol |
//! | XRechnung 3.0 CVD | 290 | + all 8 Clean Vehicles Directive rules |
//! | XRechnung 3.0 Extension | 296 | + 14 of the 15 `BR-DEX-*`; `BR-DEX-15` is a CII element check |
//! | Peppol BIS Billing 3.0 | 273 | **46 / 46** `PEPPOL-EN16931-*` |
//!
//! …and at the severities those authorities publish, which are not the
//! severities the rules carry and are not written down in one place.
//!
//! | Where a severity is published | What it re-levels |
//! |---|---|
//! | `validator-configuration-xrechnung/scenarios.xml` `<customLevel>` | nine of **CEN's** rules across KoSIT's three scenarios — `BR-CL-23` to *warning* even for the plain CIUS, because CEN's unit-code table lags UN/ECE's |
//! | the XRechnung Schematron's own `flag` | five of **KoSIT's** rules: `BR-DE-17`, `-21`, `-26`, `-27`, `-28` are `warning`, and `BR-DE-TMP-32` is `information` |
//!
//! Both are read. `tests/codelists.rs` compares the first against
//! `scenarios.xml` and the second against all 121 severities the two
//! Schematrons publish, because reporting any of them as fatal — which this
//! crate did — rejects invoices the German reference validator accepts.
//!
//! [`Profile::levels`]: validation::profile::Profile::levels
//!
//! And the rules agree with the authorities' **own conformance suites**, not
//! only with their rule lists: **100 %** agreement on every assertion run —
//! 1 013 of CEN's unit tests (11 divergences declared and explained), 381
//! runnable KoSIT mutations, and all 58 published example invoices.
//!
//! Those totals move: two of the three suites come from repositories pinned to
//! a moving branch, because neither publishes releases on a cadence worth
//! pinning to. So `tests/conformance.rs` asserts 100 % agreement *exactly* and
//! coverage as a **floor** — upstream growing the suite must not fail the
//! build, and upstream losing it must.
//!
//! Each comes with the typed [`Validated`] proof, and there is a `billing`
//! adapter. Checked against the standard's own Annex A worked examples.
//!
//! Three things sit on top of the verdict, each deriving from the *same* tables
//! the rules use rather than from a second reading of the standard:
//!
//! | | |
//! |---|---|
//! | [`reconcile`](mod@reconcile) | BG-23 and BG-22 as a function of the lines — `BR-CO-10`…`-16`, every category family's `-08` and `-09` |
//! | [`codes::guard`] | a withdrawn EAS scheme rejected at the map, with its successor named, rather than at the report |
//! | [`Profile::missing_terms`] | which fields a profile will ask for, answerable *before* the data is fetched |
//!
//! [`Profile::missing_terms`]: validation::profile::Profile::missing_terms
//!
//! A five-line invoice validates in about **1.5 µs** through the core rules and
//! under 7 µs through XRechnung's 282; `proptest` properties assert validation
//! never panics, is deterministic, and never cites an unresolvable rule id.
//!
//! Out of scope, deliberately and named rather than quietly dropped: the 1 339
//! *syntax* rules (`UBL-*`, `CII-*`) belong to `en16931-formats`, and Peppol's
//! ~90 national rules (`DK-R-*`, `SE-R-*`, …) are country registry-format and
//! check-digit checks.
//!
//! # A whole invoice, end to end
//!
//! Two lines at two VAT rates, built, reconciled and validated. Nothing is
//! elided — this is the complete program, and it is a doctest, so it compiles
//! and passes on every commit.
//!
//! ```
//! use en16931::invoice::{Party, PostalAddress};
//! use en16931::{Date, Identifier, InvoiceAmount, Percentage, Quantity, prelude::*};
//! use rust_decimal::dec;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let seller = Party {
//!     name: Some("Stadtwerke Musterstadt GmbH".into()),
//!     vat_identifier: Some("DE123456789".into()),
//!     // BT-34's scheme is an EAS code. `Identifier::eas` rejects a withdrawn
//!     // one here rather than at validation time — 9958 is the classic.
//!     electronic_address: Some(Identifier::eas("4012345000009", "0088")?),   // GLN
//!     address: PostalAddress {
//!         city: Some("Musterstadt".into()),
//!         post_code: Some("12345".into()),
//!         country: Some(en16931::codes::guard::country("DE")?),   // BR-09
//!         ..Default::default()
//!     },
//!     ..Default::default()
//! };
//! let buyer = Party {
//!     name: Some("Beispiel AG".into()),
//!     electronic_address: Some(Identifier::schemed("991-01234-56", "0204")),
//!     address: PostalAddress {
//!         city: Some("Beispielstadt".into()),
//!         post_code: Some("54321".into()),
//!         country: Some(en16931::codes::guard::country("DE")?),   // BR-11
//!         ..Default::default()
//!     },
//!     ..Default::default()
//! };
//!
//! let invoice = Invoice::builder(
//!         "urn:cen.eu:en16931:2017",       // BT-24
//!         "R-2026-0001",                   // BT-1
//!         Date::parse("2026-07-31")?,      // BT-2
//!         "380",                           // BT-3 — commercial invoice
//!         "EUR",                           // BT-5
//!     )
//!     .seller(seller)
//!     .buyer(buyer)
//!     .due_in_days(14)                     // BT-9 — satisfies BR-CO-25
//!     .line(InvoiceLine::new(
//!         "1", "Netznutzung Arbeitspreis",
//!         Quantity::new(dec!(10000)), "KWH",
//!         InvoiceAmount::parse("2890.00")?,
//!         "S", Some(Percentage::new(dec!(19))),
//!     ))
//!     .line(InvoiceLine::new(
//!         "2", "Messstellenbetrieb",
//!         Quantity::new(dec!(12)), "MON",
//!         InvoiceAmount::parse("120.00")?,
//!         "S", Some(Percentage::new(dec!(7))),
//!     ))
//!     // BG-23 and BG-22 are a *function* of the lines. This computes it —
//!     // grouping, rounding and the absent-is-not-zero rules included.
//!     .build_reconciled()?;
//!
//! assert_eq!(invoice.vat_breakdown.len(), 2);          // one group per rate
//! assert_eq!(invoice.totals.vat_total.unwrap().to_string(), "557.50");
//! assert_eq!(invoice.totals.gross_total.to_string(), "3567.50");
//!
//! let report = validate(&invoice);
//! assert!(report.is_valid(), "{report}");
//! # Ok(()) }
//! ```
//!
//! And the other direction — an invoice with nothing in it, so every finding
//! points somewhere:
//!
//! ```
//! use en16931::{validate, prelude::*};
//!
//! let invoice = Invoice::default();          // nothing filled in
//! let report = validate(&invoice);
//!
//! assert!(!report.is_valid());
//! assert!(report.has("BR-02"));              // no invoice number
//! assert!(report.has("BR-16"));              // no invoice line
//! // Findings point at business terms, never at an XPath.
//! assert_eq!(report.fatal().next().unwrap().path.to_string(), "BT-1");
//! ```
//!
//! ## Attribution
//!
//! This crate is an implementation of the semantic data model of EN 16931-1 and
//! of the two mandatory syntaxes listed in CEN/TS 16931-2. EN 16931-1 and
//! CEN/TS 16931-2 are made available free of charge by CEN and the European
//! Commission under their 2018 licence agreement, which permits derivative use
//! on condition that derivative applications carry a statement to this effect.
//! Copyright in the standard remains with CEN.
//!
//! ## README
//!
//! The crate README is included below, so **every Rust example in it is compiled
//! and run as a doctest**. Documentation that drifts out of compiling is the
//! most expensive kind, and this makes that class of rot impossible.
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![warn(missing_docs, unreachable_pub, rust_2018_idioms, clippy::all)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod amount;
pub mod attachment;
#[cfg(feature = "billing")]
#[cfg_attr(docsrs, doc(cfg(feature = "billing")))]
pub mod billing_adapter;
pub mod bt;
pub mod codes;
pub mod date;
pub mod edition;
pub mod error;
pub mod extensions;
pub mod fmt;
pub mod identifier;
pub mod invoice;
pub mod numeric;
pub mod profiles;
pub mod reconcile;
pub mod report;
#[cfg(feature = "svrl")]
#[cfg_attr(docsrs, doc(cfg(feature = "svrl")))]
pub mod svrl;
pub mod validation;

pub use amount::{InvoiceAmount, UnitPriceAmount};
pub use attachment::{Attachment, AttachmentError};
pub use bt::{BtId, Group, Path};
pub use codes::VatCategory;
pub use date::Date;
pub use edition::Edition;
pub use error::{AmountError, ParseAmountError, ParseDateError};
pub use extensions::{AdvancePayment, Extensions, SubInvoiceLine, ThirdPartyPayment};
pub use identifier::{DocumentReference, Identifier};
pub use invoice::{DocumentKind, Invoice, InvoiceLine, InvoiceNote};
pub use numeric::{Percentage, Quantity};
// All five markers, not three. `XRechnungCvd` and `XRechnungExtension` were
// reachable only as `profiles::XRechnungCvd` while their three siblings were at
// the root, which reads as "these two are second-class" — they are not; they are
// the two profiles a German integrator is most likely to need after the CIUS.
pub use profiles::{En16931, PeppolBis3, XRechnung, XRechnungCvd, XRechnungExtension};
pub use reconcile::{ReconcileError, Reconciler, reconcile};
pub use report::Report;
pub use validation::profile::{Profile, Validated};
pub use validation::{Check, Finding, ProveError, Severity, ValidationReport, validate};

/// The notice the CEN–EC licence agreement **requires** this crate to carry.
///
/// # Not decoration — a licence condition
///
/// EN 16931-1 and CEN/TS 16931-2 are free of charge under the 2018 agreement
/// between CEN and the European Commission, which permits derivative use *on
/// condition* that a derivative carries a statement, visible to users, that it
/// is an implementation of the semantic data model. Everything this crate does
/// rests on that permission, so the notice appears in three places: the crate
/// documentation, `README.md`, and the header of every
/// [`ValidationReport`]'s `Display`.
///
/// It is a `const` rather than three string literals because three copies drift,
/// and `tests/attribution.rs` asserts all three still agree. Losing this by
/// reformatting would forfeit the licence the whole crate depends on, and
/// nothing else would notice.
///
/// **Written on one line on purpose**, and the reason is narrower than it once
/// said here.
///
/// A multi-line literal **without** the `\` continuation puts its own source
/// indentation into the value: that is what put a run of 32 spaces inside the
/// notice this crate emits, and every report carried it that way. The test did
/// not catch it because it normalised whitespace on *both* sides — and
/// `"a;\n     b"` and `"a; b"` are equal once normalised, which is exactly the
/// comparison that cannot see this bug.
///
/// `rustfmt` is **not** the culprit, though this comment used to say so. It
/// preserves a `\`-continued literal untouched, and Rust strips the newline and
/// the following indentation from one, so the continued form is correct. One
/// line is kept anyway: this is a licence condition, the constant is compared
/// byte-for-byte against `README.md` and the report header, and a form with no
/// whitespace decision in it is the form with nothing to get wrong.
///
/// `the_notice_is_canonical_not_merely_present` asserts the canonical value,
/// not merely its presence.
pub const ATTRIBUTION: &str = "implementation of the EN 16931-1 semantic data model; © CEN, used under the 2018 CEN–EC licence agreement";

/// The CEN validation-artefacts release this crate's rule metadata and code
/// lists are generated from.
///
/// Exposed so a bug report can say which rule text was in force. The artefacts
/// and the standard do not always agree — so knowing
/// which artefact revision produced a finding is part of reproducing it.
pub const ARTEFACT_VERSION: &str = "validation-1.3.16";

/// The edition of EN 16931-1 this crate's core rule set targets by default.
///
/// EN 16931-1:2026 is published and the 2017 edition formally withdrawn, but
/// every deployed validator — XRechnung 3.0.2, Peppol BIS Billing 3.0,
/// ZUGFeRD 2.x — is a usage specification of 2017+A1:2019. Leading with :2026
/// would produce a crate that fails all of them. See [`Edition`].
pub const DEFAULT_EDITION: Edition = Edition::En2017A1;

/// Convenience glob import.
pub mod prelude {
    pub use crate::{
        AmountError, Attachment, BtId, Date, DocumentKind, DocumentReference, En16931, Finding,
        Group, Identifier, Invoice, InvoiceAmount, InvoiceLine, InvoiceNote, ParseAmountError,
        ParseDateError, Path, PeppolBis3, Percentage, Profile, Quantity, Severity, UnitPriceAmount,
        Validated, ValidationReport, VatCategory, XRechnung, XRechnungCvd, XRechnungExtension,
        validate,
    };
}
