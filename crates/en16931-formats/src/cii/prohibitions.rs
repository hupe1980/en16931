//! CII prohibitions, extracted from CEN's Schematron as data.
//!
//! The CII half of the story [`crate::ubl::prohibitions`] tells, and the same
//! caveat applies with more force: **the context is half the rule.**
//!
//! `CII-DT-076` is `not(ram:ID)`, and it does *not* mean "no document may
//! contain `ram:ID`" — it means the element that rule's context selects may not
//! have one. An earlier version of this table dropped the context, and the
//! writer duly discarded every `ram:ID` in the document. So each entry carries
//! its context and [`forbidden_path`] matches both halves.
//!
//! CII has **no global attribute prohibitions** — its extension points are
//! elements. The table is present and empty rather than absent, so both
//! syntaxes offer the shared serialiser the same interface.

use super::prohibitions_generated as generated;
pub use generated::{FORBIDDEN_ATTRIBUTES, FORBIDDEN_PATHS, TOTAL_PARAMS, UNEXTRACTED};

/// Does this element path violate a prohibition?
///
/// `path` is a `/`-joined chain of qualified element names starting at the
/// document element. Returns the rule id.
#[must_use]
pub fn forbidden_path(path: &str) -> Option<&'static str> {
    FORBIDDEN_PATHS
        .iter()
        .find(|(_, ctx, rel)| crate::xml::path_matches(path, ctx, rel))
        .map(|(id, _, _)| *id)
}

/// Is this attribute forbidden anywhere in a CII document?
#[must_use]
pub fn forbidden_attribute(name: &str) -> Option<&'static str> {
    FORBIDDEN_ATTRIBUTES
        .iter()
        .find(|(_, a)| *a == name)
        .map(|(id, _)| *id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_populated() {
        assert!(FORBIDDEN_PATHS.len() > 300, "{}", FORBIDDEN_PATHS.len());
        const { assert!(TOTAL_PARAMS > 400) };
    }

    /// The regression this module's context exists to prevent: `ram:ID` is a
    /// legitimate element almost everywhere, and a context-free table forbade
    /// it outright.
    #[test]
    fn ram_id_is_not_forbidden_outright() {
        for path in [
            "rsm:CrossIndustryInvoice/rsm:ExchangedDocument/ram:ID",
            "rsm:CrossIndustryInvoice/rsm:SupplyChainTradeTransaction/\
             ram:ApplicableHeaderTradeAgreement/ram:SellerTradeParty/ram:ID",
        ] {
            assert_eq!(forbidden_path(path), None, "{path} must be allowed");
        }
    }

    #[test]
    fn legitimate_elements_are_not_forbidden() {
        for path in [
            "rsm:CrossIndustryInvoice/rsm:ExchangedDocument/ram:TypeCode",
            "rsm:CrossIndustryInvoice/rsm:SupplyChainTradeTransaction/\
             ram:ApplicableHeaderTradeSettlement/\
             ram:SpecifiedTradeSettlementHeaderMonetarySummation/ram:DuePayableAmount",
        ] {
            assert_eq!(forbidden_path(path), None, "{path} must be allowed");
        }
    }

    /// A prohibition in the table must still fire when it genuinely applies,
    /// or the whole thing passes by checking nothing.
    #[test]
    fn a_real_prohibition_still_fires() {
        let (rule, ctx, rel) = FORBIDDEN_PATHS[0];
        let stem = ctx.trim_start_matches('/');
        assert_eq!(forbidden_path(&format!("{stem}/{rel}")), Some(rule));
    }
}
