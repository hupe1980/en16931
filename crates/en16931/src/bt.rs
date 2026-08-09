//! [`BtId`], [`BgId`] and [`Path`] — how a finding says *where*.
//!
//! Every other implementation in this space validates a serialised document, so
//! its findings are located by XPath. That is precise for a machine and useless
//! for a person: `/ubl:Invoice/cac:TaxTotal/cac:TaxSubtotal[2]/cbc:TaxAmount`
//! requires you to know the syntax binding before you can tell which *business*
//! field is wrong.
//!
//! A [`Path`] says `BG-23[1]/BT-117` instead, which is the language the standard,
//! the rules and the reader all already speak.

use core::fmt;

/// A business term identifier — `BT-117`.
///
/// A newtype over `u16` rather than a string, so a typo is a compile error and a
/// finding can be filtered by term without parsing.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BtId(pub u16);

impl fmt::Display for BtId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!("BT-{}", self.0))
    }
}

/// A business group identifier — `BG-23`.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BgId(pub u16);

impl fmt::Display for BgId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!("BG-{}", self.0))
    }
}

/// The business group a finding sits in.
///
/// Only the groups that can repeat, or that a rule needs to distinguish, are
/// enumerated — a finding on BT-1 is simply [`Group::Document`].
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Group {
    /// The invoice header — BT-1 … BT-24, and anything not in a group below.
    Document,
    /// BG-4 SELLER (and BG-5 / BG-6 beneath it).
    Seller,
    /// BG-7 BUYER (and BG-8 / BG-9 beneath it).
    Buyer,
    /// BG-10 PAYEE.
    Payee,
    /// BG-11 SELLER TAX REPRESENTATIVE PARTY (and BG-12 beneath it).
    TaxRepresentative,
    /// BG-13 DELIVERY INFORMATION.
    Delivery,
    /// BG-16 PAYMENT INSTRUCTIONS.
    Payment,
    /// BG-20 DOCUMENT LEVEL ALLOWANCES.
    DocumentAllowance,
    /// BG-21 DOCUMENT LEVEL CHARGES.
    DocumentCharge,
    /// BG-22 DOCUMENT TOTALS.
    Totals,
    /// BG-23 VAT BREAKDOWN.
    VatBreakdown,
    /// BG-24 ADDITIONAL SUPPORTING DOCUMENTS.
    Attachment,
    /// BG-25 INVOICE LINE (and BG-26 … BG-32 beneath it).
    Line,
}

impl Group {
    /// Every group, in BG order.
    ///
    /// Exists so a property of "all groups" can be *tested over all groups*
    /// rather than over a list someone remembered to extend — which is how
    /// [`Group::Attachment`] came to disagree with [`repeats`](Self::repeats)
    /// unnoticed. `every_group_is_in_all` keeps this in step with the enum.
    pub const ALL: &'static [Self] = &[
        Self::Document,
        Self::Seller,
        Self::Buyer,
        Self::Payee,
        Self::TaxRepresentative,
        Self::Delivery,
        Self::Payment,
        Self::DocumentAllowance,
        Self::DocumentCharge,
        Self::Totals,
        Self::VatBreakdown,
        Self::Attachment,
        Self::Line,
    ];

    /// The BG number, where the group has one.
    #[must_use]
    pub const fn bg(self) -> Option<BgId> {
        Some(match self {
            Self::Document => return None,
            Self::Seller => BgId(4),
            Self::Buyer => BgId(7),
            Self::Payee => BgId(10),
            Self::TaxRepresentative => BgId(11),
            Self::Delivery => BgId(13),
            Self::Payment => BgId(16),
            Self::DocumentAllowance => BgId(20),
            Self::DocumentCharge => BgId(21),
            Self::Totals => BgId(22),
            Self::VatBreakdown => BgId(23),
            Self::Attachment => BgId(24),
            Self::Line => BgId(25),
        })
    }

    /// Whether this group may occur more than once, so a [`Path`] into it needs
    /// an index to be unambiguous.
    ///
    /// [`Group::Attachment`] is here because BG-24 is `0..n` and four rules —
    /// `BR-DE-22`, `BR-DEX-01`, `PEPPOL-EN16931-CL001` and `BR-TMP-2` — already
    /// emit `BG-24[i]`. It used to answer `false` while those paths were being
    /// written, and the test below did not list it in either direction, so
    /// neither half noticed the other.
    #[must_use]
    pub const fn repeats(self) -> bool {
        matches!(
            self,
            Self::DocumentAllowance
                | Self::DocumentCharge
                | Self::VatBreakdown
                | Self::Attachment
                | Self::Line
        )
    }
}

/// Where in the invoice a finding is.
///
/// Renders as `BG-25[2]/BT-151` — group, occurrence, term. Any part may be
/// absent: a rule about a missing group has no term, and a rule about the header
/// has no group or index.
///
/// ```
/// use en16931::bt::{BtId, Group, Path};
///
/// assert_eq!(Path::term(BtId(1)).to_string(), "BT-1");
/// assert_eq!(Path::at(Group::Line, 2).to_string(), "BG-25[2]");
/// assert_eq!(Path::at_term(Group::Line, 2, BtId(151)).to_string(), "BG-25[2]/BT-151");
/// assert_eq!(Path::group(Group::Totals).to_string(), "BG-22");
/// ```
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Path {
    /// Which group.
    pub group: Group,
    /// Which occurrence, zero-based, for a repeating group.
    pub index: Option<usize>,
    /// Which business term, when the finding is about one.
    pub term: Option<BtId>,
}

impl Path {
    /// A header term — `BT-1`.
    #[must_use]
    pub const fn term(term: BtId) -> Self {
        Self {
            group: Group::Document,
            index: None,
            term: Some(term),
        }
    }

    /// A whole group — `BG-22`.
    #[must_use]
    pub const fn group(group: Group) -> Self {
        Self {
            group,
            index: None,
            term: None,
        }
    }

    /// A term within a non-repeating group — `BG-22/BT-109`.
    #[must_use]
    pub const fn group_term(group: Group, term: BtId) -> Self {
        Self {
            group,
            index: None,
            term: Some(term),
        }
    }

    /// One occurrence of a repeating group — `BG-25[2]`.
    #[must_use]
    pub const fn at(group: Group, index: usize) -> Self {
        Self {
            group,
            index: Some(index),
            term: None,
        }
    }

    /// A term in one occurrence — `BG-25[2]/BT-151`.
    #[must_use]
    pub const fn at_term(group: Group, index: usize, term: BtId) -> Self {
        Self {
            group,
            index: Some(index),
            term: Some(term),
        }
    }

    /// The whole document, for rules that are not about any one place.
    #[must_use]
    pub const fn document() -> Self {
        Self {
            group: Group::Document,
            index: None,
            term: None,
        }
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = String::new();
        if let Some(bg) = self.group.bg() {
            s.push_str(&bg.to_string());
            if let Some(i) = self.index {
                s.push_str(&format!("[{i}]"));
            }
        }
        if let Some(t) = self.term {
            if !s.is_empty() {
                s.push('/');
            }
            s.push_str(&t.to_string());
        }
        if s.is_empty() {
            s.push_str("Invoice");
        }
        f.pad(&s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_the_language_the_standard_speaks() {
        assert_eq!(Path::document().to_string(), "Invoice");
        assert_eq!(Path::term(BtId(1)).to_string(), "BT-1");
        assert_eq!(Path::group(Group::Totals).to_string(), "BG-22");
        assert_eq!(
            Path::group_term(Group::Totals, BtId(109)).to_string(),
            "BG-22/BT-109"
        );
        assert_eq!(Path::at(Group::VatBreakdown, 0).to_string(), "BG-23[0]");
        assert_eq!(
            Path::at_term(Group::Line, 2, BtId(151)).to_string(),
            "BG-25[2]/BT-151"
        );
    }

    /// `Group::ALL` must list every variant, or every property asserted over it
    /// is asserted over a subset.
    ///
    /// The `match` is what does the work: adding a variant without adding it to
    /// `ALL` is then a compile error, not a test that keeps passing.
    #[test]
    fn every_group_is_in_all() {
        for g in Group::ALL {
            match g {
                Group::Document
                | Group::Seller
                | Group::Buyer
                | Group::Payee
                | Group::TaxRepresentative
                | Group::Delivery
                | Group::Payment
                | Group::DocumentAllowance
                | Group::DocumentCharge
                | Group::Totals
                | Group::VatBreakdown
                | Group::Attachment
                | Group::Line => (),
            }
        }
        assert_eq!(Group::ALL.len(), 13);
        // BG numbers ascend, which is what makes the derived `Ord` on `Path`
        // sort a report the way the standard reads.
        let bgs: Vec<_> = Group::ALL.iter().filter_map(|g| g.bg()).collect();
        assert!(bgs.windows(2).all(|w| w[0] < w[1]), "{bgs:?}");
    }

    #[test]
    fn repeating_groups_are_exactly_the_ones_that_need_an_index() {
        const REPEATING: &[Group] = &[
            Group::DocumentAllowance,
            Group::DocumentCharge,
            Group::VatBreakdown,
            Group::Attachment,
            Group::Line,
        ];
        // Over *every* group, so a new one has to be classified rather than
        // silently defaulting to "does not repeat".
        for g in Group::ALL {
            assert_eq!(g.repeats(), REPEATING.contains(g), "{g:?}");
        }
    }

    #[test]
    fn group_numbers_match_the_standard() {
        assert_eq!(Group::Seller.bg(), Some(BgId(4)));
        assert_eq!(Group::Buyer.bg(), Some(BgId(7)));
        assert_eq!(Group::VatBreakdown.bg(), Some(BgId(23)));
        assert_eq!(Group::Line.bg(), Some(BgId(25)));
        assert_eq!(Group::Document.bg(), None);
    }

    #[test]
    fn paths_sort_stably_for_diffable_reports() {
        let mut v = [
            Path::at_term(Group::Line, 2, BtId(151)),
            Path::at_term(Group::Line, 0, BtId(151)),
            Path::term(BtId(1)),
        ];
        v.sort();
        assert_eq!(v[0], Path::term(BtId(1)));
        assert_eq!(v[1], Path::at_term(Group::Line, 0, BtId(151)));
    }
}
