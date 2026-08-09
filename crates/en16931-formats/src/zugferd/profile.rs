//! The ZUGFeRD / Factur-X profile matrix — and the trap in it.
//!
//! The critical fact, and the whole reason this is a module rather than a
//! string: **not every profile is an EN 16931 invoice.**

use core::fmt;

/// A ZUGFeRD 2.x / Factur-X profile. ⚠ Names unverified — see the crate docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Profile {
    /// Booking aid: totals only, no lines. **Not an EN 16931 invoice.**
    Minimum,
    /// "Basic without lines". **Not an EN 16931 invoice.**
    BasicWl,
    /// A CIUS of EN 16931.
    Basic,
    /// The core model, unrestricted. Also called COMFORT.
    En16931,
    /// An Extension — §4.3's other mechanism. Adds terms the core does not have.
    Extended,
    /// The German CIUS, carried in CII.
    XRechnung,
    /// A profile identifier this crate does not know.
    Unknown,
}

/// Whether a profile is an invoice under EN 16931 at all.
///
/// This exists as a type rather than a `bool` because the answer changes what a
/// caller may legitimately do, and a bare `false` invites being ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsInvoice {
    /// The document is an EN 16931 invoice; the 317 rules apply.
    Yes,
    /// The document is **not** an EN 16931 invoice, with the reason.
    No(&'static str),
    /// The profile is unrecognised, so the question cannot be answered.
    Unknown,
}

impl Profile {
    /// Whether documents in this profile are EN 16931 invoices.
    ///
    /// MINIMUM and BASIC WL are not, and this is the trap the whole module
    /// exists for: it is tempting to model all six uniformly because they
    /// appear in one table in the specification. Two of them cannot satisfy
    /// `BR-16` — an invoice shall have at least one line — so validating them
    /// against EN 16931 produces a wall of findings that are not defects, and
    /// a *typed proof* for them would be a false proof.
    ///
    /// `en16931` shipped and fixed exactly that bug once: an `Underlies` impl
    /// that let an invoice validated against one profile be widened into a
    /// proof for another it had never been checked against. A type system that
    /// says MINIMUM is an EN 16931 invoice is worse than no type system.
    ///
    /// ```
    /// use en16931_formats::zugferd::{IsInvoice, Profile};
    ///
    /// assert_eq!(Profile::En16931.is_en16931_invoice(), IsInvoice::Yes);
    /// assert!(matches!(Profile::Minimum.is_en16931_invoice(), IsInvoice::No(_)));
    /// ```
    #[must_use]
    pub const fn is_en16931_invoice(self) -> IsInvoice {
        match self {
            Self::Basic | Self::En16931 | Self::Extended | Self::XRechnung => IsInvoice::Yes,
            Self::Minimum => {
                IsInvoice::No("MINIMUM carries totals only and no lines: it cannot satisfy BR-16")
            }
            Self::BasicWl => IsInvoice::No("BASIC WL is 'without lines': it cannot satisfy BR-16"),
            Self::Unknown => IsInvoice::Unknown,
        }
    }

    /// The name as it appears in the XMP and the specification. ⚠
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Minimum => "MINIMUM",
            Self::BasicWl => "BASIC WL",
            Self::Basic => "BASIC",
            Self::En16931 => "EN 16931",
            Self::Extended => "EXTENDED",
            Self::XRechnung => "XRECHNUNG",
            Self::Unknown => "UNKNOWN",
        }
    }

    /// Classify a profile identifier from a document's BT-24 or XMP. ⚠
    ///
    /// Matching is on the identifier's **suffix** and case-insensitive, because
    /// the full URNs differ between ZUGFeRD 2.x and Factur-X for the same
    /// profile. Returns [`Profile::Unknown`] rather than guessing: a document
    /// whose profile is unrecognised must not be silently treated as the core
    /// model, which is the permissive answer and therefore the wrong default.
    #[must_use]
    pub fn parse(id: &str) -> Self {
        let id = id.trim().to_ascii_lowercase();
        let tail = id.rsplit(':').next().unwrap_or(&id);
        // Order matters: `basicwl` must be tested before `basic`, and
        // `extended` before `en16931`, or a prefix match claims the wrong one.
        if tail.contains("minimum") {
            Self::Minimum
        } else if tail.contains("basicwl") || id.contains("basic wl") {
            Self::BasicWl
        } else if tail.contains("basic") {
            Self::Basic
        } else if tail.contains("extended") {
            Self::Extended
        } else if id.contains("xrechnung") {
            Self::XRechnung
        } else if id.contains("en16931") {
            Self::En16931
        } else {
            Self::Unknown
        }
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_that_are_not_invoices_say_so() {
        assert!(matches!(
            Profile::Minimum.is_en16931_invoice(),
            IsInvoice::No(_)
        ));
        assert!(matches!(
            Profile::BasicWl.is_en16931_invoice(),
            IsInvoice::No(_)
        ));
        for p in [
            Profile::Basic,
            Profile::En16931,
            Profile::Extended,
            Profile::XRechnung,
        ] {
            assert_eq!(p.is_en16931_invoice(), IsInvoice::Yes, "{p}");
        }
        assert_eq!(Profile::Unknown.is_en16931_invoice(), IsInvoice::Unknown);
    }

    /// The reason a profile is not an invoice must be usable in a message.
    #[test]
    fn the_refusal_carries_a_reason() {
        let IsInvoice::No(why) = Profile::Minimum.is_en16931_invoice() else {
            panic!("MINIMUM is not an invoice");
        };
        assert!(why.contains("BR-16"), "{why}");
    }

    /// `basicwl` before `basic`, `extended` before `en16931`.
    #[test]
    fn parsing_is_not_fooled_by_prefixes() {
        assert_eq!(
            Profile::parse("urn:factur-x.eu:1p0:basicwl"),
            Profile::BasicWl
        );
        assert_eq!(Profile::parse("urn:factur-x.eu:1p0:basic"), Profile::Basic);
        assert_eq!(
            Profile::parse("urn:cen.eu:en16931:2017#conformant#urn:factur-x.eu:1p0:extended"),
            Profile::Extended
        );
        assert_eq!(Profile::parse("urn:cen.eu:en16931:2017"), Profile::En16931);
        assert_eq!(
            Profile::parse("urn:factur-x.eu:1p0:minimum"),
            Profile::Minimum
        );
    }

    #[test]
    fn an_unrecognised_profile_is_not_quietly_the_core_model() {
        assert_eq!(Profile::parse("urn:something:else"), Profile::Unknown);
        assert_eq!(Profile::parse(""), Profile::Unknown);
        // The permissive answer would be `En16931`, and it is wrong: it would
        // validate an unknown document against rules nobody claimed applied.
        assert_ne!(Profile::parse("nonsense"), Profile::En16931);
    }

    #[test]
    fn case_and_whitespace_do_not_change_the_profile() {
        assert_eq!(
            Profile::parse("  URN:FACTUR-X.EU:1P0:BASIC  "),
            Profile::Basic
        );
    }
}
