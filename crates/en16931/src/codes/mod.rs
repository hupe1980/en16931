//! Code lists — EN 16931-1 §6.5.8 `Code. Type`.
//!
//! > Codes are used to specify allowed values in elements as well as for lists
//! > of options. Code is different from Identifier in that allowed values have
//! > standardized meanings that can be known by the recipient. […] Codes shall
//! > be entered exactly as shown in the selected code list.
//!
//! Eighteen lists, **4 887 values**, all generated from the pinned CEN
//! validation artefacts by `cargo xtask codegen` and re-verified against them by
//! `tests/codelists.rs`. See [`generated`].
//!
//! # Two shapes, for two kinds of list
//!
//! - **Small, closed, semantically load-bearing** lists become enums, because
//!   the code's *meaning* drives rules. [`VatCategory`] is the case that
//!   matters: ten codes, each with different rules about rate, tax amount and
//!   exemption reason.
//! - **Large, open, referentially checked** lists stay as sorted `&[&str]` with
//!   [`contains`] doing a binary search. 2 162 unit codes do not want to be an
//!   enum, and nothing in the standard branches on *which* unit it is.
//!
//! # Catching a bad code at the map, not at the report
//!
//! [`contains`] answers yes or no. [`guard`] answers *what to do*: it returns a
//! [`CodeError`](guard::CodeError) naming the rule that would have reported the
//! value, and — where the crate can tell — the successor of a withdrawn code,
//! the case-folded spelling, or the whitespace that made it fail.
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
/// The lists are sorted at generation time, so this is a binary search with no
/// allocation and no runtime setup.
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

/// EN 16931 **BT-118 / BT-151 / BT-95 / BT-102** — the UNCL 5305 VAT category.
///
/// Ten codes, fixed by `BR-CL-17` and `BR-CL-18`. This enum is deliberately
/// **not** `#[non_exhaustive]`: it mirrors a closed, externally governed list,
/// and a caller mapping to an output format legitimately needs exhaustive
/// matching. If CEN adds a code, that is a breaking change and should look like
/// one.
///
/// # The three predicates, and why they are not the same question
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
/// Three traps this table encodes, all of which implementations get wrong:
///
/// 1. **`Z` and `E` both carry zero tax, but `Z` *forbids* an exemption reason
///    and `E` *requires* one.** Zero-rating and exemption are legally distinct —
///    input tax stays deductible under `Z`. (BR-Z-10 vs BR-E-10.)
/// 2. **`B` is the only category where both reason predicates are false.** The
///    artefacts contain no `BR-B-09` forcing the tax to zero, no `BR-B-05`
///    constraining the rate and no `BR-B-10` requiring a reason. Split payment
///    charges VAT normally; the *buyer* remits it.
/// 3. **`states_rate` is about BT-152, not BT-119.** See its own documentation —
///    this is the subtlest distinction in the whole enum.
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
/// Returned by [`VatCategory::can_share_document`]. It carries the rules that
/// *would* report the conflict, so a caller can cite them to whoever asked for
/// the invoice — which is the difference between "we cannot bill this" and "we
/// cannot bill this, and here is the clause".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryConflict {
    /// The exclusive category — `O` or `B`.
    pub category: VatCategory,
    /// One category it cannot share a document with. There may be others; this
    /// is the first found, and naming one is enough to explain the refusal.
    pub conflicts_with: VatCategory,
    /// The rules that **govern** this exclusivity, in the artefacts' spelling.
    ///
    /// Not "the rules that will fire": `BR-O-11` covers a second breakdown
    /// group, `BR-O-12` a line, `BR-O-13` an allowance and `BR-O-14` a charge,
    /// and which of them reports depends on *where* the other category appears
    /// — which a set of category codes cannot say. The family is the clause a
    /// caller cites; at least one of its members will report the document.
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
    /// **Case-sensitive**, because §6.5.8 says *"Codes shall be entered exactly
    /// as shown in the selected code list"* and `BR-CL-17` compares literally.
    /// Accepting `"ae"` here would make this crate disagree with every validator
    /// on a document it then declared valid.
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

    /// **Can these categories share one document?** — answerable before the
    /// document exists.
    ///
    /// # Why this is not simply `validate`
    ///
    /// Two of the ten categories are *exclusive*, and they are the only rules
    /// in EN 16931 that a set of category codes decides on its own — no
    /// amounts, no lines, no parties. Everything else needs a document.
    ///
    /// That makes this the same kind of question as
    /// [`Profile::missing_terms`](crate::Profile::missing_terms): the answer is
    /// **exact**, and it is available at the moment a caller is choosing what to
    /// put on an invoice rather than after assembling one it then has to throw
    /// away.
    ///
    /// The case that forced it is German municipal billing. A *hoheitliche
    /// Abwassergebühr* is not subject to VAT (`O`), drinking water is taxable
    /// (`S`), and `BR-O-11` … `BR-O-14` forbid `O` from sharing a document with
    /// anything else — so the combined invoice that over 90 % of municipalities
    /// issue **has no valid EN 16931 rendering at all**. That is the standard's
    /// decision and no implementation can fix it. What an implementation owes is
    /// a refusal that names the reason *before* the work, instead of a wall of
    /// findings after it.
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

        // The exclusive categories first: one of them excludes *everything*, so
        // it is the broader answer and the more useful one to report when both
        // conflicts are present. Which categories those are is
        // [`Self::is_exclusive`]'s to know, not this function's — the two
        // disagreeing about `O` is exactly the drift worth designing out.
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

    /// Whether a VAT breakdown in this category **must** state BT-120 or BT-121.
    ///
    /// BR-E-10, BR-AE-10, BR-IC-10, BR-G-10 and BR-O-10. Each accepts *either*
    /// the reason text **or** the reason code — modelling only the text forces a
    /// caller holding a VATEX code to invent prose.
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

    /// Whether a VAT breakdown in this category **must not** state BT-120 or BT-121.
    ///
    /// BR-S-10, BR-Z-10, BR-AF-10 and BR-AG-10. A taxed or zero-rated supply is
    /// not an exemption and needs no justification.
    #[must_use]
    pub const fn forbids_exemption_reason(self) -> bool {
        matches!(
            self,
            Self::Standard | Self::ZeroRated | Self::CanaryIslands | Self::CeutaMelilla
        )
    }

    /// Whether a **line, allowance or charge** in this category states a VAT rate.
    ///
    /// `O` is the only category that does not. BR-O-05, BR-O-06 and BR-O-07 say
    /// the element *"shall not contain"* BT-152 / BT-96 / BT-103, where every
    /// other zero-tax category says the rate *"shall be 0"*. A serialiser must
    /// therefore **suppress** the element for `O` rather than emit `0`.
    ///
    /// # This does not apply to BT-119
    ///
    /// The VAT **breakdown** rate is a different business term. BR-48 makes it
    /// optional only *"if the Invoice is not subject to VAT"*, and XRechnung's
    /// **BR-DE-14** then requires it **unconditionally** — *"Das Element 'VAT
    /// category rate' (BT-119) muss übermittelt werden"*, fatal, with no
    /// category exception. Suppressing BT-119 for `O` on the strength of
    /// BR-O-05 is the natural mistake and it fails the KoSIT validator.
    #[must_use]
    pub const fn states_rate(self) -> bool {
        !matches!(self, Self::OutOfScope)
    }

    /// The category's name, as EN 16931 and UNCL 5305 write it.
    ///
    /// Short enough for a table and unambiguous enough to catch the mistake the
    /// code alone hides: `O` is *not* "other", and a caller who read it that way
    /// has produced an invoice that can carry nothing else (BR-O-11 … BR-O-14).
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

    /// Every generated list, by name — the single place this module enumerates
    /// them.
    ///
    /// A hand-written list is what let three tables go unchecked: this array
    /// said `15` while [`generated`] had grown to eighteen, and
    /// `NOTE_SUBJECT_CODES`, `PEPPOL_EAS_SCHEMES` and `PEPPOL_MIME_CODES` were
    /// never asserted to be sorted at all. They are binary-searched like the
    /// rest, so an unsorted one would have reported a valid code as invalid and
    /// no test would have said so.
    ///
    /// `no_generated_list_escapes_the_sortedness_check` now reads the generated
    /// source and fails if this array does not name every `pub static` in it, so
    /// the next table cannot be forgotten the same way.
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
        // `contains` binary-searches, so an unsorted table would silently return
        // false negatives — a valid code reported as invalid.
        for (name, list) in LISTS {
            assert!(!list.is_empty(), "{name} is empty");
            for w in list.windows(2) {
                assert!(w[0] < w[1], "{name} is not sorted/unique at {w:?}");
            }
        }
    }

    /// The check above is only as good as [`LISTS`], so [`LISTS`] is checked.
    ///
    /// Against [`generated::TABLES`], which the generator emits alongside the
    /// tables themselves and therefore cannot omit one of — rather than by
    /// counting `pub static` lines in the generated source, which breaks on a
    /// change to the syntax rather than to the subject.
    ///
    /// [`LISTS`] stays hand-written: it is the list this module promises to
    /// binary-search, and deriving it from the same source it is checking
    /// against would make the check vacuous.
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
        // BR-CL-01's test is a `self::` disjunction over two lists. They share
        // exactly one code, and that one is not 380 or 381.
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
