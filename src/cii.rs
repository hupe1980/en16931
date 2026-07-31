//! The UN/CEFACT Cross Industry Invoice D16B binding.
//!
//! **Not yet implemented.** This module exists so the feature, the layering and
//! the roadmap are visible rather than implied, and it deliberately exports
//! nothing that would let a caller believe otherwise.
//!
//! # Why UBL came first
//!
//! Not preference — evidence. `en16931`'s conformance suites run against CEN's
//! and KoSIT's *UBL* instances, so a UBL reader could be checked against 1 131
//! rule assertions the day it was written. The CII reader will be checked the
//! same way, against CEN's CII instances, and shipping it before that harness
//! exists would mean shipping something whose correctness is an opinion.
//!
//! # The known obstacle
//!
//! CEN's CII Schematron writes its rule contexts through variables —
//! `$Specified_Trade_Settlement_PaymentMeans` rather than a literal path — so
//! the binding cannot be read off the artefacts the way UBL's element order
//! was ([`crate::ubl::order`]). The variables have to be resolved first. That
//! is a solved problem, not a hard one, but it is the reason this module is a
//! placeholder rather than a half-written reader.

/// The CII namespaces.
pub mod ns {
    /// The document element's namespace.
    pub const RSM: &str =
        "urn:un:unece:uncefact:data:standard:CrossIndustryInvoice:100";
    /// Reusable aggregate business information entities.
    pub const RAM: &str =
        "urn:un:unece:uncefact:data:standard:ReusableAggregateBusinessInformationEntity:100";
    /// Unqualified data types.
    pub const UDT: &str =
        "urn:un:unece:uncefact:data:standard:UnqualifiedDataType:100";
}
