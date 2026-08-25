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

/// Letters and digits only, lower-cased.
///
/// `EN 16931`, `EN16931` and `en_16931` are one profile written three ways, and
/// all three occur: the XMP spells profiles the way the specification prints
/// them and a URN cannot carry a space.
fn fold(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
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
    /// A type system that says MINIMUM is an EN 16931 invoice is worse than no
    /// type system.
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

    /// Classify a profile identifier from a document's BT-24 **or** from the
    /// PDF's `fx:ConformanceLevel`.
    ///
    /// Those two spell the same profile differently and both reach this
    /// function: a URN cannot contain a space, so BT-24 writes
    /// `urn:cen.eu:en16931:2017`, while the XMP writes the profile bare and
    /// with the spaces the specification prints — `EN 16931`, `BASIC WL`.
    /// Comparison is therefore on letters and digits only, case-folded, which
    /// makes all four spellings of a profile one profile.
    ///
    /// ```
    /// use en16931_formats::zugferd::Profile;
    ///
    /// // What the XMP carries …
    /// assert_eq!(Profile::parse("EN 16931"), Profile::En16931);
    /// assert_eq!(Profile::parse("BASIC WL"), Profile::BasicWl);
    /// // … and what BT-24 carries, for the same two documents.
    /// assert_eq!(Profile::parse("urn:cen.eu:en16931:2017"), Profile::En16931);
    /// assert_eq!(Profile::parse("urn:factur-x.eu:1p0:basicwl"), Profile::BasicWl);
    /// ```
    ///
    /// Returns [`Profile::Unknown`] rather than guessing: a document whose
    /// profile is unrecognised must not be silently treated as the core model,
    /// which is the permissive answer and therefore the wrong default.
    #[must_use]
    pub fn parse(id: &str) -> Self {
        // The profile name is normally the identifier's **last** segment, and
        // every ZUGFeRD URN also embeds `en16931` in its prefix — so the tail
        // is asked first and the whole identifier only as a fallback. Reversing
        // the two makes `…#conformant#urn:factur-x.eu:1p0:extended` the EN 16931
        // profile it merely conforms to.
        let tail = id.rsplit(':').next().unwrap_or(id);
        Self::from_fold(&fold(tail))
            .or_else(|| Self::from_fold(&fold(id)))
            .unwrap_or(Self::Unknown)
    }

    /// Match a folded identifier against the profile names.
    fn from_fold(s: &str) -> Option<Self> {
        // Order matters: `basicwl` before `basic`, and both `extended` and
        // `xrechnung` before `en16931`, or a substring match claims the wrong
        // profile.
        if s.contains("minimum") {
            Some(Self::Minimum)
        } else if s.contains("basicwl") {
            Some(Self::BasicWl)
        } else if s.contains("basic") {
            Some(Self::Basic)
        } else if s.contains("extended") {
            Some(Self::Extended)
        } else if s.contains("xrechnung") {
            Some(Self::XRechnung)
        // `COMFORT` is ZUGFeRD 1.0's name for the profile 2.x renamed
        // `EN 16931`. A 1.0 file is not an EN 16931 invoice by BT-24 — it has
        // none — but its XMP is the only thing that says which profile it is,
        // and a reader that answers `Unknown` there has thrown that away.
        } else if s.contains("en16931") || s.contains("comfort") {
            Some(Self::En16931)
        } else {
            None
        }
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Not `Formatter::pad`: it truncates to `precision` characters, so
        // `{:.2}` on `EN 16931` prints `EN` — a profile name that is not a
        // profile. `en16931::fmt::padded` is the same helper the model crate's
        // own types use, shared rather than copied.
        en16931::fmt::padded(f, self.as_str())
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

    /// Every profile's own name parses back to it.
    ///
    /// The property that was missing, and the bug it let through: `as_str`
    /// returns exactly what a PDF's `fx:ConformanceLevel` carries, and
    /// `parse("EN 16931")` answered `Unknown` because the URN spelling has no
    /// space in it. `extract`'s profile-mismatch check compares the two through
    /// `parse`, so it was silently dead for the most common ZUGFeRD profile —
    /// a PDF whose metadata claimed EN 16931 over a MINIMUM payload was
    /// reported as agreeing with itself.
    #[test]
    fn every_profile_name_parses_back_to_its_profile() {
        for p in [
            Profile::Minimum,
            Profile::BasicWl,
            Profile::Basic,
            Profile::En16931,
            Profile::Extended,
            Profile::XRechnung,
        ] {
            assert_eq!(Profile::parse(p.as_str()), p, "{p} did not round-trip");
        }
        // `UNKNOWN` is not a profile name, so it does not round-trip and must
        // not be made to: parsing it as a profile would invent one.
        assert_eq!(Profile::parse(Profile::Unknown.as_str()), Profile::Unknown);
    }

    /// ZUGFeRD 1.0's name for what 2.x calls `EN 16931`.
    #[test]
    fn comfort_is_the_older_name_for_en_16931() {
        assert_eq!(Profile::parse("COMFORT"), Profile::En16931);
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

    /// Case, spacing and separators are notation, not identity.
    #[test]
    fn case_and_whitespace_do_not_change_the_profile() {
        assert_eq!(
            Profile::parse("  URN:FACTUR-X.EU:1P0:BASIC  "),
            Profile::Basic
        );
        // The same profile, as the XMP writes it and as a URN must.
        for spelling in ["EN 16931", "EN16931", "en_16931", "en-16931"] {
            assert_eq!(Profile::parse(spelling), Profile::En16931, "{spelling}");
        }
        for spelling in ["BASIC WL", "BASICWL", "basic-wl"] {
            assert_eq!(Profile::parse(spelling), Profile::BasicWl, "{spelling}");
        }
    }
}
