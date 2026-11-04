//! Code lists — EN 16931-1 §6.5.8 `Code. Type`. Codes are case-sensitive.
//!
//! Eighteen lists, **4 896 values**, all generated from the pinned CEN
//! validation artefacts by `cargo xtask codegen` and re-verified against them by
//! `tests/codelists.rs`. See [`generated`].
//!
//! Lists are sorted `&[&str]` checked with [`contains`]; [`VatCategory`] is an
//! enum because rules branch on its meaning. [`guard`] checks a value up front
//! and returns a [`CodeError`](guard::CodeError) naming the rule and, where
//! known, a fix (successor code, case, whitespace).
//!
//! ```
//! use en16931::codes::guard;
//!
//! assert!(guard::eas("9958").is_err());   // withdrawn 2023-07-31; the hint says `0204`
//! assert!(guard::eas("0204").is_ok());
//! ```

pub mod generated;
pub mod guard;

use core::fmt;

/// Whether `code` appears in a generated list.
///
/// A binary search over the generator-sorted list.
///
/// ```
/// use en16931::codes::{contains, generated::UNIT_CODES};
///
/// assert!(contains(UNIT_CODES, "KWH"));   // kilowatt hour
/// assert!(contains(UNIT_CODES, "C62"));   // one
/// assert!(!contains(UNIT_CODES, "kwh"));  // codes are case-sensitive
/// ```
#[must_use]
pub fn contains(list: &[&str], code: &str) -> bool {
    list.binary_search(&code).is_ok()
}

/// EN 16931 BT-118 / BT-151 / BT-95 / BT-102 — the UNCL 5305 VAT category.
///
/// Ten codes (`BR-CL-17`, `BR-CL-18`). Not `#[non_exhaustive]`, so callers can
/// match exhaustively; a new code would be a breaking change.
///
/// | | `carries_tax` | `requires_exemption_reason` | `forbids_exemption_reason` | `states_rate` |
/// |---|---|---|---|---|
/// | `S` Standard | ✓ | | ✓ | ✓ |
/// | `Z` Zero rated | | | ✓ | ✓ |
/// | `E` Exempt | | ✓ | | ✓ |
/// | `AE` Reverse charge | | ✓ | | ✓ |
/// | `K` Intra-community | | ✓ | | ✓ |
/// | `G` Export | | ✓ | | ✓ |
/// | `O` Outside scope | | ✓ | | **✗** |
/// | `L` IGIC | ✓ | | ✓ | ✓ |
/// | `M` IPSI | ✓ | | ✓ | ✓ |
/// | `B` Split payment | ✓ | | | ✓ |
///
/// `Z` forbids an exemption reason while `E` requires one (BR-Z-10, BR-E-10);
/// `B` has no reason rule either way. `states_rate` concerns BT-152, not BT-119.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VatCategory {
    /// `S` — standard rate.
    Standard,
    /// `Z` — zero-rated goods. Taxable at 0 %; input tax remains deductible.
    ZeroRated,
    /// `E` — exempt from VAT. Input tax generally not deductible. Needs a reason.
    Exempt,
    /// `AE` — VAT reverse charge: the recipient accounts for the tax.
    ReverseCharge,
    /// `K` — VAT-exempt intra-Community supply of goods.
    IntraCommunity,
    /// `G` — free export item, VAT not charged.
    Export,
    /// `O` — services outside the scope of VAT. Exclusive: BR-O-11..14.
    OutOfScope,
    /// `L` — Canary Islands general indirect tax (IGIC).
    CanaryIslands,
    /// `M` — tax for production, services and importation in Ceuta and Melilla (IPSI).
    CeutaMelilla,
    /// `B` — split payment (Italy, *scissione dei pagamenti*). Taxed; the buyer remits.
    SplitPayment,
}

/// Why a set of VAT categories cannot appear in one document.
///
/// Returned by [`VatCategory::can_share_document`], with the governing rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryConflict {
    /// The exclusive category — `O` or `B`.
    pub category: VatCategory,
    /// The first category found that it cannot share a document with.
    pub conflicts_with: VatCategory,
    /// The rules governing this exclusivity; which of them fires depends on
    /// where the other category appears.
    pub rules: &'static [&'static str],
    /// What to do instead.
    pub hint: &'static str,
}

impl fmt::Display for CategoryConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "VAT categories {} and {} cannot share one document ({}) — {}",
            self.category.code(),
            self.conflicts_with.code(),
            self.rules.join(", "),
            self.hint
        )
    }
}

impl core::error::Error for CategoryConflict {}

impl VatCategory {
    /// All ten codes, in the order `BR-CL-17` lists them.
    pub const ALL: [Self; 10] = [
        Self::ReverseCharge,
        Self::CanaryIslands,
        Self::CeutaMelilla,
        Self::Exempt,
        Self::Standard,
        Self::ZeroRated,
        Self::Export,
        Self::OutOfScope,
        Self::IntraCommunity,
        Self::SplitPayment,
    ];

    /// The UNCL 5305 code as written in an invoice.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Standard => "S",
            Self::ZeroRated => "Z",
            Self::Exempt => "E",
            Self::ReverseCharge => "AE",
            Self::IntraCommunity => "K",
            Self::Export => "G",
            Self::OutOfScope => "O",
            Self::CanaryIslands => "L",
            Self::CeutaMelilla => "M",
            Self::SplitPayment => "B",
        }
    }

    /// Parse a UNCL 5305 code.
    ///
    /// Case-sensitive (§6.5.8, `BR-CL-17`).
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Some(match code {
            "S" => Self::Standard,
            "Z" => Self::ZeroRated,
            "E" => Self::Exempt,
            "AE" => Self::ReverseCharge,
            "K" => Self::IntraCommunity,
            "G" => Self::Export,
            "O" => Self::OutOfScope,
            "L" => Self::CanaryIslands,
            "M" => Self::CeutaMelilla,
            "B" => Self::SplitPayment,
            _ => return None,
        })
    }

    /// Whether these categories can share one document, checked without one:
    /// `O` excludes every other category (`BR-O-11` … `BR-O-14`) and `B`
    /// excludes `S` (`BR-B-02`).
    ///
    /// ```
    /// use en16931::VatCategory;
    /// use en16931::VatCategory::{OutOfScope, SplitPayment, Standard, ZeroRated};
    ///
    /// // Ordinary: a zero-rated line beside a standard-rated one.
    /// assert!(VatCategory::can_share_document(&[Standard, ZeroRated]).is_ok());
    ///
    /// // The municipal invoice, refused with its reason and its rules.
    /// let err = VatCategory::can_share_document(&[Standard, OutOfScope]).unwrap_err();
    /// assert_eq!(err.rules, ["BR-O-11", "BR-O-12", "BR-O-13", "BR-O-14"]);
    /// assert!(err.to_string().contains("cannot share"));
    ///
    /// // Italy's split payment excludes the standard rate, and nothing else.
    /// assert!(VatCategory::can_share_document(&[SplitPayment, ZeroRated]).is_ok());
    /// assert!(VatCategory::can_share_document(&[SplitPayment, Standard]).is_err());
    /// ```
    ///
    /// An empty set, or one category on its own, is always fine.
    ///
    /// # Errors
    /// [`CategoryConflict`], naming both categories and the rules that would
    /// report them.
    pub fn can_share_document(categories: &[Self]) -> Result<(), CategoryConflict> {
        let has = |c: Self| categories.contains(&c);

        // Exclusive categories first: they are the broader conflict.
        if let Some(exclusive) = categories.iter().copied().find(|c| c.is_exclusive())
            && let Some(other) = categories.iter().copied().find(|c| *c != exclusive)
        {
            debug_assert_eq!(exclusive, Self::OutOfScope, "the rule list below is O's");
            return Err(CategoryConflict {
                category: exclusive,
                conflicts_with: other,
                rules: &["BR-O-11", "BR-O-12", "BR-O-13", "BR-O-14"],
                hint: "\"Not subject to VAT\" is exclusive: an invoice carrying it may carry \
                       nothing else. Bill the out-of-scope items as their own document",
            });
        }

        if has(Self::SplitPayment) && has(Self::Standard) {
            return Err(CategoryConflict {
                category: Self::SplitPayment,
                conflicts_with: Self::Standard,
                rules: &["BR-B-02"],
                hint: "split payment and the standard rate cannot appear on one document; \
                       BR-B-01 also requires a split-payment invoice to be domestic Italian",
            });
        }

        Ok(())
    }

    /// Whether this category actually levies tax.
    ///
    /// `S`, `L`, `M` and `B` do. For the rest, BR-Z-09, BR-E-09, BR-AE-09,
    /// BR-IC-09, BR-G-09 and BR-O-09 each require BT-117 to be exactly zero.
    #[must_use]
    pub const fn carries_tax(self) -> bool {
        matches!(
            self,
            Self::Standard | Self::CanaryIslands | Self::CeutaMelilla | Self::SplitPayment
        )
    }

    /// Whether a VAT breakdown in this category must state BT-120 or BT-121
    /// (BR-E-10, BR-AE-10, BR-IC-10, BR-G-10, BR-O-10).
    #[must_use]
    pub const fn requires_exemption_reason(self) -> bool {
        matches!(
            self,
            Self::Exempt
                | Self::ReverseCharge
                | Self::IntraCommunity
                | Self::Export
                | Self::OutOfScope
        )
    }

    /// Whether a VAT breakdown in this category must not state BT-120 or BT-121
    /// (BR-S-10, BR-Z-10, BR-IG-10, BR-IP-10).
    #[must_use]
    pub const fn forbids_exemption_reason(self) -> bool {
        matches!(
            self,
            Self::Standard | Self::ZeroRated | Self::CanaryIslands | Self::CeutaMelilla
        )
    }

    /// Whether a line, allowance or charge in this category states a VAT rate
    /// (BT-152 / BT-96 / BT-103). Only `O` does not (BR-O-05 … BR-O-07), so a
    /// writer omits the element rather than writing `0`.
    ///
    /// Not about BT-119: XRechnung's BR-DE-14 requires that even for `O`.
    #[must_use]
    pub const fn states_rate(self) -> bool {
        !matches!(self, Self::OutOfScope)
    }

    /// The category's name, as EN 16931 and UNCL 5305 write it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Standard => "standard rate",
            Self::ZeroRated => "zero rated goods",
            Self::Exempt => "exempt from VAT",
            Self::ReverseCharge => "VAT reverse charge",
            Self::IntraCommunity => "VAT-exempt intra-Community supply",
            Self::Export => "free export item, VAT not charged",
            Self::OutOfScope => "services outside the scope of VAT",
            Self::CanaryIslands => "Canary Islands general indirect tax (IGIC)",
            Self::CeutaMelilla => "tax for Ceuta and Melilla (IPSI)",
            Self::SplitPayment => "split payment",
        }
    }

    /// Whether this category may appear alongside any other in one document.
    ///
    /// Only `O` may not: BR-O-11 forbids a second breakdown group, and
    /// BR-O-12/13/14 forbid any line, allowance or charge in another category.
    #[must_use]
    pub const fn is_exclusive(self) -> bool {
        matches!(self, Self::OutOfScope)
    }
}

impl fmt::Display for VatCategory {
    /// The code. Honours width, fill and alignment.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        crate::fmt::padded(f, self.code())
    }
}

#[cfg(test)]
mod tests {
    use super::generated::*;
    use super::*;

    /// Every generated list, by name; checked against [`generated::TABLES`].
    const LISTS: &[(&str, &[&str])] = &[
        ("ALLOWANCE_REASON_CODES", ALLOWANCE_REASON_CODES),
        ("CHARGE_REASON_CODES", CHARGE_REASON_CODES),
        ("COUNTRY_CODES", COUNTRY_CODES),
        ("CREDIT_NOTE_TYPE_CODES", CREDIT_NOTE_TYPE_CODES),
        ("CURRENCY_CODES", CURRENCY_CODES),
        ("EAS_SCHEMES", EAS_SCHEMES),
        ("ICD_SCHEMES", ICD_SCHEMES),
        ("INVOICE_TYPE_CODES", INVOICE_TYPE_CODES),
        ("ITEM_CLASSIFICATION_SCHEMES", ITEM_CLASSIFICATION_SCHEMES),
        ("NOTE_SUBJECT_CODES", NOTE_SUBJECT_CODES),
        ("PAYMENT_MEANS_CODES", PAYMENT_MEANS_CODES),
        ("PEPPOL_EAS_SCHEMES", PEPPOL_EAS_SCHEMES),
        ("PEPPOL_MIME_CODES", PEPPOL_MIME_CODES),
        ("REFERENCE_QUALIFIERS", REFERENCE_QUALIFIERS),
        ("UNIT_CODES", UNIT_CODES),
        ("VATEX_CODES", VATEX_CODES),
        ("VAT_CATEGORY_CODES", VAT_CATEGORY_CODES),
        ("VAT_POINT_DATE_CODES", VAT_POINT_DATE_CODES),
    ];

    #[test]
    fn every_generated_list_is_sorted_and_unique() {
        // `contains` binary-searches.
        for (name, list) in LISTS {
            assert!(!list.is_empty(), "{name} is empty");
            for w in list.windows(2) {
                assert!(w[0] < w[1], "{name} is not sorted/unique at {w:?}");
            }
        }
    }

    /// [`LISTS`] names exactly the tables in [`generated::TABLES`].
    #[test]
    fn no_generated_list_escapes_the_sortedness_check() {
        for (name, _) in generated::TABLES {
            assert!(
                LISTS.iter().any(|(n, _)| n == name),
                "{name} is generated and not in LISTS, so nothing asserts it is \
                 sorted — and `contains` binary-searches it"
            );
        }
        assert_eq!(
            LISTS.len(),
            generated::TABLES.len(),
            "LISTS names a table the generator does not emit"
        );
    }

    #[test]
    fn vat_category_round_trips_and_is_case_sensitive() {
        for c in VatCategory::ALL {
            assert_eq!(VatCategory::from_code(c.code()), Some(c));
            assert!(
                contains(VAT_CATEGORY_CODES, c.code()),
                "{c} missing from BR-CL-17"
            );
        }
        assert_eq!(VAT_CATEGORY_CODES.len(), VatCategory::ALL.len());
        // §6.5.8: "Codes shall be entered exactly as shown."
        assert_eq!(VatCategory::from_code("ae"), None);
        assert_eq!(VatCategory::from_code("Q"), None);
    }

    #[test]
    fn zero_rated_and_exempt_differ_on_the_reason() {
        // Both carry zero tax; Z forbids a reason and E requires one.
        assert!(!VatCategory::ZeroRated.carries_tax());
        assert!(!VatCategory::Exempt.carries_tax());
        assert!(VatCategory::ZeroRated.forbids_exemption_reason());
        assert!(VatCategory::Exempt.requires_exemption_reason());
    }

    #[test]
    fn split_payment_is_the_only_category_with_neither_reason_rule() {
        for c in VatCategory::ALL {
            let neither = !c.requires_exemption_reason() && !c.forbids_exemption_reason();
            assert_eq!(
                neither,
                c == VatCategory::SplitPayment,
                "{c} should{} be the neither-case",
                if c == VatCategory::SplitPayment {
                    ""
                } else {
                    " not"
                }
            );
        }
        assert!(
            VatCategory::SplitPayment.carries_tax(),
            "B is taxed, unlike AE"
        );
    }

    #[test]
    fn only_out_of_scope_suppresses_the_line_rate_and_is_exclusive() {
        for c in VatCategory::ALL {
            assert_eq!(c.states_rate(), c != VatCategory::OutOfScope);
            assert_eq!(c.is_exclusive(), c == VatCategory::OutOfScope);
        }
    }

    #[test]
    fn the_two_bt_3_lists_are_not_disjoint_but_split_380_from_381() {
        // The two BR-CL-01 lists share exactly one code, `81`.
        assert!(contains(INVOICE_TYPE_CODES, "380"));
        assert!(!contains(INVOICE_TYPE_CODES, "381"));
        assert!(contains(CREDIT_NOTE_TYPE_CODES, "381"));
        assert!(!contains(CREDIT_NOTE_TYPE_CODES, "380"));

        let shared: Vec<_> = INVOICE_TYPE_CODES
            .iter()
            .filter(|c| contains(CREDIT_NOTE_TYPE_CODES, c))
            .collect();
        assert_eq!(shared, [&"81"], "exactly one overlap: 81");
        assert_eq!(
            (INVOICE_TYPE_CODES.len(), CREDIT_NOTE_TYPE_CODES.len()),
            (50, 13)
        );
    }

    #[test]
    fn spot_checks_against_the_standard() {
        assert!(contains(UNIT_CODES, "KWH")); // Rec 20 kilowatt hour
        assert!(contains(UNIT_CODES, "H87")); // piece
        assert!(contains(UNIT_CODES, "C62")); // one — what a flat charge states
        assert!(contains(CURRENCY_CODES, "EUR"));
        assert!(
            contains(CURRENCY_CODES, "XXX"),
            "in ISO 4217, rejected by us separately"
        );
        assert!(contains(COUNTRY_CODES, "DE"));
        assert!(contains(PAYMENT_MEANS_CODES, "58")); // SEPA credit transfer
        assert!(contains(VATEX_CODES, "VATEX-EU-AE"));
        assert_eq!(VAT_POINT_DATE_CODES, ["3", "35", "432"]);
    }
}
