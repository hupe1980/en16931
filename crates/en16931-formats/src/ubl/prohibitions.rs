//! UBL prohibitions, extracted from CEN's Schematron as data.
//!
//! # Why this module is small
//!
//! There are 1 339 syntax rules across CEN's artefacts, and **1 218 of them say
//! some element "shall not be used"**. They exist because UBL 2.1 and CII D16B
//! are far larger than EN 16931, and the standard has to fence off the rest.
//! They are not business logic; they are a subset definition.
//!
//! A *writer* built from the semantic model cannot violate them — it has no way
//! to express `cbc:UUID`, because the model has no term for it. They are
//! unreachable rather than cheaply satisfied, the same shape as
//! [`en16931`]'s `InvoiceAmount` making `BR-DEC-*` unrepresentable.
//!
//! A *reader* is a different matter, because the document came from elsewhere.
//! [`Read::unmapped`](super::Read::unmapped) is where it reports having seen
//! something outside the subset.
//!
//! # The context is half the rule
//!
//! `not(cbc:UUID)` does not mean "no document may contain `cbc:UUID`". It means
//! "the element this rule's *context* selects may not have one". An earlier
//! version of this table dropped the context and turned narrow prohibitions
//! into blanket ones, which made the writer discard `ram:ID` everywhere.
//!
//! So [`forbidden_path`] takes a full path from the document element and
//! matches both halves.

use super::prohibitions_generated as generated;
pub use generated::{FORBIDDEN_ATTRIBUTES, FORBIDDEN_PATHS, TOTAL_PARAMS, UNEXTRACTED};

/// Does this element path violate a prohibition?
///
/// `path` is a `/`-joined chain of qualified element names starting at the
/// document element — `Invoice/cac:AccountingCustomerParty/cac:Party/cac:Language`.
/// Returns the rule id.
#[must_use]
pub fn forbidden_path(path: &str) -> Option<&'static str> {
    static INDEX: std::sync::OnceLock<crate::xml::ProhibitionIndex> = std::sync::OnceLock::new();
    INDEX
        .get_or_init(|| crate::xml::ProhibitionIndex::build(FORBIDDEN_PATHS))
        .find(FORBIDDEN_PATHS, path)
}

/// Is this attribute forbidden anywhere in a UBL document?
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

    /// **The index answers exactly what a full scan answers.**
    ///
    /// `forbidden_path` used to scan all of `FORBIDDEN_PATHS`; it now looks up
    /// a bucket keyed by the path's last segment, which made writing a document
    /// twenty to sixty times faster (D44). That is only sound if the two agree
    /// on **every** input, including the ones where two prohibitions match and
    /// the reported id has to be the same one.
    ///
    /// So the scan is kept here as the reference implementation and the two are
    /// compared over every path the tables themselves describe — both the
    /// matching case, built from each entry's own context and relative path,
    /// and near misses built by nesting it one level deeper.
    #[test]
    fn the_index_agrees_with_a_full_scan() {
        fn by_scan(path: &str) -> Option<&'static str> {
            FORBIDDEN_PATHS
                .iter()
                .find(|(_, ctx, rel)| crate::xml::path_matches(path, ctx, rel))
                .map(|(id, _, _)| *id)
        }

        let mut checked = 0usize;
        for (_, ctx, rel) in FORBIDDEN_PATHS {
            let root = ctx.trim_start_matches('/');
            // Two variants rather than four: the path this entry is about,
            // and the same nested one level deeper — which an anchored context
            // must refuse and a floating one must still match. That is the
            // distinction the index could plausibly get wrong, and the sweep is
            // quadratic in the table, so the other variants cost seconds and
            // test nothing new.
            for path in [
                format!("{root}/{rel}"),
                format!("Invoice/cac:Somewhere/{root}/{rel}"),
            ] {
                assert_eq!(
                    forbidden_path(&path),
                    by_scan(&path),
                    "index and scan disagree on {path:?}"
                );
                checked += 1;
            }
        }

        // Paths that are in no table at all, so both must answer `None`.
        for path in ["", "/", "Invoice", "Invoice/cbc:ID", "a/b/c/d/e"] {
            assert_eq!(forbidden_path(path), by_scan(path), "{path:?}");
            checked += 1;
        }
        assert!(checked > 1_000, "the sweep should be large: {checked}");
    }

    /// The tables must contain something, or every test that consults them
    /// passes for the wrong reason.
    ///
    /// `TOTAL_PARAMS` counts **assertions**, not rows. Counting rows and
    /// unextracted assertions together would make the total grow whenever the
    /// extractor got better at reading the artefact, which is not a total.
    #[test]
    fn the_tables_are_populated() {
        assert!(FORBIDDEN_PATHS.len() > 1_000, "{}", FORBIDDEN_PATHS.len());
        assert!(FORBIDDEN_ATTRIBUTES.len() > 10);
        const { assert!(TOTAL_PARAMS > 600) };
        // Four fifths of what was once unreadable is now read; the rest needs an
        // XPath engine. A regression here means the extractor lost ground.
        const { assert!(UNEXTRACTED < 40) };
    }

    /// A prohibition anchored at the document element fires there and **only**
    /// there. This is the regression the context was added for.
    #[test]
    fn a_root_anchored_prohibition_does_not_fire_at_depth() {
        let Some((rule, ctx, rel)) = FORBIDDEN_PATHS
            .iter()
            .find(|(_, c, _)| c.starts_with("/ubl:"))
        else {
            panic!("expected a root-anchored prohibition");
        };
        let root = ctx
            .trim_start_matches('/')
            .split(':')
            .next_back()
            .expect("root");
        assert_eq!(forbidden_path(&format!("{root}/{rel}")), Some(*rule));
        // The same relative path nested one level deeper is a different
        // element, and this rule says nothing about it.
        assert_eq!(forbidden_path(&format!("{root}/cac:Elsewhere/{rel}")), None);
    }

    /// Terms EN 16931 *does* use must never be reported.
    #[test]
    fn legitimate_elements_are_not_forbidden() {
        for path in [
            "Invoice/cbc:ID",
            "Invoice/cbc:IssueDate",
            "Invoice/cac:AccountingSupplierParty/cac:Party/cac:PartyName/cbc:Name",
            "Invoice/cac:LegalMonetaryTotal/cbc:PayableAmount",
            "Invoice/cac:InvoiceLine/cac:Item/cbc:Name",
        ] {
            assert_eq!(forbidden_path(path), None, "{path} must be allowed");
        }
    }

    #[test]
    fn language_id_is_forbidden_everywhere() {
        assert!(forbidden_attribute("languageID").is_some());
        assert_eq!(forbidden_attribute("currencyID"), None);
    }
}
