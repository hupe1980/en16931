//! **XRechnung** — which specification a document claims, and writing one that claims it.
//!
//! # Claiming and being are different questions
//!
//! BT-24 is a *claim*. A document saying it is an `XRechnung` 3.0 may be nothing
//! of the sort, and the gap between the two is a real diagnostic rather than a
//! nuisance: "you sent us an `XRechnung` that is not one" and "you sent us
//! something we do not recognise" need different replies.
//!
//! So [`detect`] reports the claim and nothing else. Deciding whether the claim
//! holds is [`en16931`]'s job, and the two are kept apart deliberately.

use en16931::profiles;
use en16931::validation::profile::Profile;

/// The specification a document's BT-24 claims.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Flavour {
    /// A specification identifier this crate's registry knows, with the profile
    /// it maps to.
    Known {
        /// The profile's own id — `"XRechnung 3.0"`.
        profile: &'static str,
        /// BT-24, verbatim.
        specification_id: String,
    },
    /// A well-formed EN 16931 CIUS identifier the registry does not know.
    ///
    /// Distinct from [`Flavour::Unknown`] because it is the common case for a
    /// foreign CIUS — an Italian or Dutch one — and those documents are still
    /// EN 16931 invoices this crate can read.
    ForeignCius(String),
    /// BT-24 is absent, empty, or not an EN 16931 identifier at all.
    Unknown(Option<String>),
}

impl Flavour {
    /// The profile to validate against, when the claim is one we recognise.
    ///
    /// `None` for a foreign CIUS: validating it against the core model would
    /// silently under-check, and against `XRechnung` would over-check. Neither is
    /// a defensible default, so the caller has to choose.
    #[must_use]
    pub fn profile(&self) -> Option<&'static Profile> {
        match self {
            Self::Known {
                specification_id, ..
            } => profiles::for_specification_id(specification_id),
            _ => None,
        }
    }

    /// Whether the document claims to be an `XRechnung` of any kind — the CIUS,
    /// the Extension, or the Clean Vehicles Directive variant.
    #[must_use]
    pub fn is_xrechnung(&self) -> bool {
        matches!(self, Self::Known { profile, .. } if profile.starts_with("XRechnung"))
    }
}

/// The EN 16931 identifier prefix every CIUS builds on.
///
/// §4.4.2: a CIUS identifier is `urn:cen.eu:en16931:2017#compliant#…`, and an
/// Extension appends `#conformant#…`. That structure is what makes a foreign
/// CIUS recognisable as one without a registry entry for it.
const EN16931_PREFIX: &str = "urn:cen.eu:en16931:2017";

/// Classify a BT-24 value.
///
/// ```
/// use en16931_formats::{Flavour, detect};
///
/// let f = detect(Some("urn:cen.eu:en16931:2017#compliant#urn:xeinkauf.de:kosit:xrechnung_3.0"));
/// assert!(f.is_xrechnung());
/// assert_eq!(detect(None), Flavour::Unknown(None));
/// ```
#[must_use]
pub fn detect(specification_id: Option<&str>) -> Flavour {
    let Some(raw) = specification_id else {
        return Flavour::Unknown(None);
    };
    let id = raw.trim();
    if id.is_empty() {
        return Flavour::Unknown(Some(raw.to_owned()));
    }
    if let Some(p) = profiles::for_specification_id(id) {
        return Flavour::Known {
            profile: p.id,
            specification_id: id.to_owned(),
        };
    }
    if id.starts_with(EN16931_PREFIX) {
        return Flavour::ForeignCius(id.to_owned());
    }
    Flavour::Unknown(Some(raw.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_ids_are_all_recognised() {
        // Every profile `en16931` ships must round-trip through `detect`, or
        // the two registries have drifted — which is exactly the failure this
        // crate would otherwise discover in production.
        for p in profiles::ALL {
            let f = detect(Some(p.specification_id));
            assert_eq!(
                f.profile().map(|q| q.id),
                Some(p.id),
                "{} did not detect back to itself",
                p.id
            );
        }
    }

    #[test]
    fn a_foreign_cius_is_not_unknown() {
        let f = detect(Some(
            "urn:cen.eu:en16931:2017#compliant#urn:fdc:nen.nl:nlcius:v1.0",
        ));
        assert!(matches!(f, Flavour::ForeignCius(_)));
        assert!(f.profile().is_none(), "no defensible default profile");
        assert!(!f.is_xrechnung());
    }

    #[test]
    fn nonsense_is_unknown_and_keeps_the_original() {
        assert_eq!(
            detect(Some("hello")),
            Flavour::Unknown(Some("hello".into()))
        );
        assert_eq!(detect(Some("  ")), Flavour::Unknown(Some("  ".into())));
        assert_eq!(detect(None), Flavour::Unknown(None));
    }

    /// Surrounding whitespace is a transport artefact, not a different claim.
    #[test]
    fn whitespace_does_not_change_the_claim() {
        let padded = detect(Some(
            "\n  urn:cen.eu:en16931:2017#compliant#urn:xeinkauf.de:kosit:xrechnung_3.0  ",
        ));
        assert!(padded.is_xrechnung());
    }
}
