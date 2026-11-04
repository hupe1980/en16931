//! The UN/CEFACT Cross Industry Invoice D16B binding.
//!
//! One document element, `rsm:CrossIndustryInvoice`, carries invoices and
//! credit notes alike; the reader infers [`en16931::DocumentKind`] from BT-3.
//!
//! - Dates use `udt:DateTimeString` format `102` (`CCYYMMDD`), the only format
//!   EN 16931 permits; the reader rejects any other.
//! - Allowances and charges share `ram:SpecifiedTradeAllowanceCharge`, told
//!   apart by `ram:ChargeIndicator`.
//! - A scheme-qualified BT-29 is `ram:GlobalID`, an unqualified one `ram:ID`;
//!   the schema sequence fixes their order, so a party with both kinds may
//!   round-trip in a different order (the set is preserved; EN 16931 gives the
//!   order no meaning).
//! - BT-8 is UNTDID 2005 in the model (`3`, `35`, `432`) and UNTDID 2475 in CII
//!   (`5`, `29`, `72`). The reader and writer translate; a value outside the
//!   list is reported (malformed on read, dropped on write) and left out.
//!
//! Reading and writing go through the crate's façade with [`crate::Syntax::Cii`].
//! This module holds the element [`order`] (generated from the D16B schema), the
//! [`prohibitions`], the single-occurrence set ([`once`]) and the namespaces ([`ns`]).

pub mod once;
pub mod order;
pub mod prohibitions;
pub(crate) mod read;
pub(crate) mod write;

/// The single-occurrence table, compiled once.
pub(crate) fn once_table() -> &'static crate::xml::Once {
    static TABLE: std::sync::OnceLock<crate::xml::Once> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| crate::xml::Once::new(once::ONCE))
}

#[doc(hidden)]
pub mod prohibitions_generated;

/// CII single-occurrence rules ([`once`]) that KoSIT's XRechnung configuration
/// raises from warning to error: a second `ram:SpecifiedTradePaymentTerms`
/// (`CII-SR-452`), a second description in one (`CII-SR-453`), or a second
/// `ram:ApplicableTradeTax` on a line (`CII-SR-454`, a `count(…) = 1` rule).
///
/// Applied only under a profile whose artefacts include
/// [`en16931::profiles::KOSIT_CONFIG`].
pub const KOSIT_RAISED: &[&str] = &["CII-SR-452", "CII-SR-453", "CII-SR-454"];

/// BT-8, model (UNTDID 2005, as EN 16931 and UBL spell it) against CII
/// (UNTDID 2475, CEN's CII code list for `ram:DueDateTypeCode`), by meaning:
/// invoice issue date, actual delivery date, paid to date.
const VAT_POINT_DATE_CODES: [(&str, &str); 3] = [("3", "5"), ("35", "29"), ("432", "72")];

/// The model's BT-8 for a CII `ram:DueDateTypeCode`, or `None` outside CII's list.
fn vat_point_date_code_from_cii(cii: &str) -> Option<&'static str> {
    VAT_POINT_DATE_CODES
        .iter()
        .find(|(_, c)| *c == cii)
        .map(|(m, _)| *m)
}

/// The CII `ram:DueDateTypeCode` for the model's BT-8, or `None` outside the
/// model's list.
fn vat_point_date_code_to_cii(model: &str) -> Option<&'static str> {
    VAT_POINT_DATE_CODES
        .iter()
        .find(|(m, _)| *m == model)
        .map(|(_, c)| *c)
}

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
