//! Which edition of EN 16931-1 a profile is a usage specification of.
//!
//! # Why this is a value and not a type parameter
//!
//! The obvious alternatives both fail. Two crates — `en16931-2017` and
//! `en16931-2026` — give you two incompatible `Invoice` types, two copies of the
//! validation engine, and a downstream explosion: `xrechnung` would have to
//! depend on both and expose both. `Invoice<E: Edition>` infects every signature
//! in the ecosystem for a difference that is, in the model, a handful of
//! `Option` fields.
//!
//! What makes one model safe is that **the 2026 revision is additive**. CEN
//! cannot delete a business term without invalidating every deployed CIUS, so
//! what :2026 does is add terms and *tighten cardinalities* on terms that
//! already exist. Tightening a cardinality is a rule change, not a model change:
//! the field is already `Option`, and the rule says it must be `Some`.
//!
//! So the model is the superset, and the edition is a property of the
//! **profile** — because a profile is what a document declares in BT-24, and
//! every deployed usage specification pins exactly one edition.
//!
//! # What an [`Edition`] does not carry
//!
//! **No term assignments.** Knowing that a business term was introduced in a
//! later edition than the target profile's would need a map from term to
//! edition, and that map can only be built from the EN 16931-1:2026 normative
//! text. `spec/` holds the 2017+A1 English text and nothing else, so any such
//! map would be written from plausibility rather than from a source — which is
//! the one thing this project does not do with transcribed values.
//!
//! So [`Edition::En2026`] is a classification a profile can carry, and
//! [`Edition::is_implemented`] answers `false` for it.

use core::fmt;

/// An edition of EN 16931-1.
///
/// `#[non_exhaustive]`: CEN will publish more, and a downstream `match` should
/// not break when they do.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[non_exhaustive]
pub enum Edition {
    /// EN 16931-1:2017 + A1:2019 (+ AC:2020).
    ///
    /// The default, and **every profile this crate ships**. It is formally
    /// withdrawn and it is what every deployed validator implements — XRechnung
    /// 3.0.2, Peppol BIS Billing 3.0, ZUGFeRD 2.x are all usage specifications
    /// of it. Leading with :2026 would produce a crate that fails all of them.
    #[default]
    En2017A1,
    /// EN 16931-1:2026 — the ViDA / B2B revision.
    ///
    /// Published, and no profile here targets it. See the module docs for why
    /// it carries no term assignments.
    En2026,
}

impl Edition {
    /// The edition's designation, as CEN writes it.
    #[must_use]
    pub const fn designation(self) -> &'static str {
        match self {
            Self::En2017A1 => "EN 16931-1:2017+A1:2019",
            Self::En2026 => "EN 16931-1:2026",
        }
    }

    /// Whether this crate can validate against the edition today.
    ///
    /// `false` for [`En2026`](Self::En2026): the rule set is the 2017 one, and
    /// pretending otherwise would be the whole point of the exercise thrown
    /// away. A profile that declared :2026 would be validated against 2017's
    /// rules, so nothing here declares it.
    #[must_use]
    pub const fn is_implemented(self) -> bool {
        matches!(self, Self::En2017A1)
    }
}

impl fmt::Display for Edition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        crate::fmt::padded(f, self.designation())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_the_deployed_edition() {
        assert_eq!(Edition::default(), Edition::En2017A1);
        assert!(Edition::En2017A1.is_implemented());
    }

    /// The 2026 rule set is not implemented, and says so rather than pretending.
    ///
    /// This test exists to fail loudly on the day someone adds `En2026` to a
    /// profile without adding the rules — at which point `is_implemented` has to
    /// change too, and that is a deliberate act rather than an oversight.
    #[test]
    fn en2026_is_classified_but_not_implemented() {
        assert!(!Edition::En2026.is_implemented());
        assert_eq!(Edition::En2026.designation(), "EN 16931-1:2026");
        for p in crate::profiles::ALL {
            assert!(
                p.edition.is_implemented(),
                "{} declares {} — but this crate has no rule set for it",
                p.id,
                p.edition
            );
        }
    }

    #[test]
    fn editions_order_oldest_first() {
        assert!(Edition::En2017A1 < Edition::En2026);
    }
}
