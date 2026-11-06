#![doc = include_str!("../README.md")]
//!
//! ## API map
//!
//! | Item | What it is |
//! |---|---|
//! | [`Invoice`], [`InvoiceLine`], [`invoice`] | the model; [`Invoice::builder`] takes BT-24, BT-1, BT-2, BT-3 and BT-5, which stay `Option` on the struct so an incomplete document can be represented and reported on |
//! | [`InvoiceAmount`], [`UnitPriceAmount`], [`Quantity`], [`Percentage`], [`Date`], [`Identifier`], [`Attachment`], [`codes`] | the semantic data types of EN 16931-1 §6.5 |
//! | [`reconcile`](mod@reconcile), [`Reconciler`] | BG-23 and BG-22 derived from the lines |
//! | [`validate`], [`Check`], [`ValidationReport`], [`validation::rules`] | the rule engine and its registry |
//! | [`profiles`], [`Profile`], [`Validated`] | the shipped profiles and the typed proof of validity |
//! | [`Report`] | the versioned JSON report; `svrl` (feature `svrl`) renders it as SVRL |
//! | [`codes::guard`] | code checks at the point of mapping, naming the fix |
//! | `billing_adapter` (feature `billing`) | a `billing` document and a header to an [`Invoice`] |
//!
//! Amounts are fixed-point and never rounded implicitly; there is no `f64`, no
//! I/O and no `unsafe`. With `serde`, types with invariants re-check them on
//! deserialisation.
//!
//! ## Rule coverage
//!
//! Of the **334** rules registered across every shipped profile:
//!
//! | | | |
//! |---|---:|---|
//! | retired by the types | 53 | no state can make them fire — `BT-112` is not an `Option` |
//! | undecidable | 4 | `BR-CO-05`…`-08`; CEN's own binding is `value="true()"` |
//! | checkable | 277 | each exercised by its own failing fixture |
//!
//! The UBL and CII syntax rules belong to `en16931-formats`. Peppol's national
//! rules are listed on the
//! [profiles page](https://hupe1980.github.io/en16931/docs/profiles/#peppol-s-national-rules).
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
pub mod examples;
pub mod extensions;
pub mod fmt;
pub mod identifier;
pub mod invoice;
pub mod numeric;
pub mod payment_terms;
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
pub use extensions::{
    AdvancePayment, Extensions, SubInvoiceLine, SubLineQuantity, ThirdPartyPayment,
};
pub use identifier::{DocumentReference, Identifier};
pub use invoice::{DocumentKind, Invoice, InvoiceLine, InvoiceNote};
pub use numeric::{Percentage, Quantity};
pub use payment_terms::{EarlyPaymentDiscount, PaymentTerms, PaymentTermsError};
pub use profiles::{En16931, PeppolBis3, XRechnung, XRechnungCvd, XRechnungExtension};
pub use reconcile::{ReconcileError, Reconciler, reconcile};
pub use report::Report;
pub use validation::profile::{Profile, Validated};
pub use validation::{Check, Finding, ProveError, Severity, ValidationReport, validate};

/// The notice the CEN–EC licence agreement requires this crate to carry.
///
/// EN 16931-1 and CEN/TS 16931-2 may be used in derivative works on condition
/// that the work states it implements the semantic data model. The notice
/// appears in the crate documentation, `README.md` and the header of every
/// [`ValidationReport`]'s `Display`; it is one line so the byte-for-byte
/// comparison against those places has no whitespace to disagree on.
pub const ATTRIBUTION: &str = "implementation of the EN 16931-1 semantic data model; © CEN, used under the 2018 CEN–EC licence agreement";

/// The CEN validation-artefacts release this crate's rule metadata and code
/// lists are generated from.
///
/// Exposed so a bug report can say which rule text was in force.
pub const ARTEFACT_VERSION: &str = "validation-1.3.17";

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
