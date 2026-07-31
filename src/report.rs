//! A **stable interchange shape** for a [`ValidationReport`].
//!
//! # Why a second representation
//!
//! `ValidationReport` derives `Serialize`, so it already has *a* JSON form. That
//! form is the crate's internal layout, and it changes whenever the internals
//! do — `profile` and `edition` were added to it in the same breath as this
//! module. Anything that persists reports, diffs them across releases, or feeds
//! them to a non-Rust consumer needs a shape that is allowed to be boring.
//!
//! So [`Report`] is a separate type with a [`SCHEMA`] version string, and the
//! rule is simple: **the internal layout may change freely; this one may not,
//! except by bumping `SCHEMA`.**
//!
//! # Designed against SVRL, not invented
//!
//! Every Schematron tool in this field — phive, Mustangproject, the KoSIT
//! validator — emits SVRL, whose `failed-assert` carries exactly four things:
//!
//! ```xml
//! <svrl:failed-assert id="BR-52" flag="fatal" location="/ubl:Invoice/cac:…">
//!   <svrl:text>[BR-52]-Each Additional supporting document …</svrl:text>
//! </svrl:failed-assert>
//! ```
//!
//! `id`, `flag`, `location`, `text`. [`Finding`] maps one-to-one onto those, so
//! an `en16931-svrl` crate is a rename rather than a translation — which is the
//! point of [`crate::validation`] doing the deciding and something else owning
//! the XML.
//!
//! **One field deliberately differs.** SVRL's `location` is an XPath into a
//! serialised document. This crate has no document, and its `location` is a
//! business-term path — `BG-25[2]/BT-151`. That is the whole reason the crate
//! exists ([§1](https://docs.rs/en16931)), and a format crate that *does* hold
//! the XML can map a BT path to an XPath. The reverse is lossy, so the semantic
//! form is the one worth storing.
//!
//! # Why the string fields are `Cow`
//!
//! They were `&'static str`, which made the type **impossible to deserialise**:
//! `serde` would have needed JSON that outlives the program. A shape whose
//! stated purpose is "persist reports and read them back" that cannot be read
//! back is not a shape, and `derive(Deserialize)` compiled anyway because the
//! impl is only unusable at the call site.
//!
//! `Cow<'static, str>` costs nothing on the way out — every value comes from a
//! constant, so it borrows — and deserialises into owned data on the way in.
//!
//! # The notice travels with it
//!
//! [`Report::attribution`] carries the CEN notice, because the licence condition
//! is that it is visible to users — and a report handed to another system is
//! exactly a place where the crate's own `Display` is not what anyone reads.

use std::borrow::Cow;

use crate::validation::{Finding, Severity, Source, ValidationReport};

/// The interchange schema this module emits.
///
/// Bump it — do not reshape [`Report`] silently — when a field changes meaning
/// or disappears. Consumers are expected to reject a version they do not know.
///
/// **`/1` is not frozen until the crate reaches 1.0.** Saying "stable" of a
/// shape published in an unreleased crate would be overstating it; the promise
/// is that it changes *deliberately and visibly*, not that it has stopped
/// changing. After 1.0 the version is a contract.
pub const SCHEMA: &str = "en16931-report/1";

/// A validation report in a shape safe to store, diff and send elsewhere.
///
/// See the [module documentation](self) for why this is not simply
/// `serde_json::to_string(&report)`.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Always [`SCHEMA`]. Reject what you do not recognise.
    pub schema: Cow<'static, str>,
    /// Whether anything fatal fired.
    pub valid: bool,
    /// The profile it was checked against, or `null` for the bare core rules.
    pub profile: Option<String>,
    /// Which edition of EN 16931-1 those rules belong to.
    pub edition: Cow<'static, str>,
    /// How many rules ran — SVRL's `fired-rule` count, in effect.
    pub rules_checked: usize,
    /// The CEN attribution notice. A licence condition, not metadata.
    pub attribution: Cow<'static, str>,
    /// Rules the caller asked to skip, and which were therefore **not checked**.
    ///
    /// Empty on an ordinary run. Non-empty means `valid` is a weaker claim than
    /// it looks, and a consumer that ignores this field is reading the report
    /// wrong — which is why it is not optional and not omitted when empty.
    pub suppressed: Vec<String>,
    /// One entry per finding, most severe first.
    pub findings: Vec<Entry>,
}

/// One finding, shaped like SVRL's `failed-assert`.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The rule id, as the authority publishes it — SVRL's `@id`.
    pub rule: String,
    /// SVRL's `@flag`: `fatal`, `warning` or `information`.
    pub severity: Cow<'static, str>,
    /// Where the rule came from — this crate's addition, and the thing that
    /// tells a reader whether an id is CEN's, a profile's, or ours.
    pub source: Cow<'static, str>,
    /// A **business-term** path, `BG-25[2]/BT-151`, where SVRL puts an XPath.
    pub location: String,
    /// The rule's own wording — SVRL's `svrl:text`.
    pub text: String,
    /// For arithmetic rules: what the rule computed.
    pub expected: Option<String>,
    /// For arithmetic rules: what the document states.
    pub actual: Option<String>,
}

const fn severity_name(s: Severity) -> &'static str {
    match s {
        Severity::Fatal => "fatal",
        Severity::Warning => "warning",
        Severity::Info => "information",
    }
}

const fn source_name(s: Source) -> &'static str {
    match s {
        Source::Both => "standard+artefact",
        Source::StandardOnly => "standard",
        Source::ArtefactOnly => "artefact",
        Source::Crate => "en16931",
    }
}

impl Report {
    /// Convert a [`ValidationReport`] into the interchange shape.
    #[must_use]
    pub fn of(report: &ValidationReport) -> Self {
        Self {
            schema: Cow::Borrowed(SCHEMA),
            valid: report.is_valid(),
            profile: report.profile().map(str::to_owned),
            edition: Cow::Borrowed(report.edition().designation()),
            rules_checked: report.rules_checked(),
            attribution: Cow::Borrowed(crate::ATTRIBUTION),
            suppressed: report.suppressed().to_vec(),
            findings: report.findings().iter().map(Entry::of).collect(),
        }
    }
}

impl Entry {
    /// Convert one finding.
    #[must_use]
    pub fn of(f: &Finding) -> Self {
        // The rule's provenance is not on the `Finding` — it is on the rule — so
        // it is looked up. An id that resolves to nothing is a restriction,
        // which is always a profile's own.
        // A rule id that resolves to nothing is a profile restriction, which is
        // always the profile's own.
        let source =
            crate::validation::rules::explain(&f.rule).map_or("profile", |r| source_name(r.source));
        Self {
            rule: f.rule.clone(),
            severity: Cow::Borrowed(severity_name(f.severity)),
            source: Cow::Borrowed(source),
            location: f.path.to_string(),
            text: f.message.clone(),
            expected: f.detail.as_ref().map(|d| d.expected.clone()),
            actual: f.detail.as_ref().map(|d| d.actual.clone()),
        }
    }
}

impl From<&ValidationReport> for Report {
    fn from(r: &ValidationReport) -> Self {
        Self::of(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Invoice, profiles, validate};

    #[test]
    fn the_shape_carries_what_svrl_carries() {
        let report = profiles::XRECHNUNG.validate(&Invoice::default());
        let out = Report::of(&report);

        assert_eq!(out.schema, SCHEMA);
        assert!(!out.valid);
        assert_eq!(out.profile.as_deref(), Some("XRechnung 3.0"));
        assert_eq!(out.edition, "EN 16931-1:2017+A1:2019");
        assert!(out.rules_checked > 0);
        assert_eq!(out.attribution, crate::ATTRIBUTION);

        let e = out.findings.first().expect("findings");
        assert!(!e.rule.is_empty());
        assert!(matches!(&*e.severity, "fatal" | "warning" | "information"));
        assert!(!e.location.is_empty(), "SVRL's `location`, semantically");
        assert!(!e.text.is_empty(), "SVRL's `svrl:text`");
    }

    /// The core path reports no profile rather than guessing one.
    #[test]
    fn a_core_report_names_no_profile() {
        let out = Report::of(&validate(&Invoice::default()));
        assert_eq!(out.profile, None);
        assert_eq!(out.edition, "EN 16931-1:2017+A1:2019");
    }

    /// A deviated run says so in the interchange shape too.
    ///
    /// The whole point of recording suppressions is that they survive being
    /// written down and read back somewhere else.
    #[test]
    fn suppressions_travel_with_the_report() {
        let report = crate::validation::Check::new(&profiles::EN16931)
            .without("BR-CO-26")
            .run(&Invoice::default());
        let out = Report::of(&report);
        assert_eq!(out.suppressed, ["BR-CO-26"]);

        let clean = Report::of(&validate(&Invoice::default()));
        assert!(clean.suppressed.is_empty());
    }

    /// The shape can be **read back**, which is the whole reason it exists.
    ///
    /// The fields were `&'static str`, so `derive(Deserialize)` compiled and
    /// produced an impl no caller could use: deserialising needed JSON that
    /// outlived the program. Nothing caught it because no test ever read one
    /// back — the type was only ever serialised.
    #[cfg(feature = "serde")]
    #[test]
    fn a_report_survives_a_round_trip_through_json() {
        let report = profiles::XRECHNUNG.validate(&Invoice::default());
        let out = Report::of(&report);

        let json = serde_json::to_string(&out).expect("serialise");
        let back: Report = serde_json::from_str(&json).expect("deserialise");

        assert_eq!(back, out);
        assert_eq!(back.schema, SCHEMA);
        assert_eq!(back.findings.len(), report.findings().len());
    }

    /// A consumer must be able to reject a schema version it does not know.
    #[cfg(feature = "serde")]
    #[test]
    fn an_unknown_schema_version_is_visible_to_a_reader() {
        let json = serde_json::to_string(&Report::of(&validate(&Invoice::default())))
            .expect("serialise")
            .replace(SCHEMA, "en16931-report/99");
        let back: Report = serde_json::from_str(&json).expect("deserialise");
        assert_ne!(back.schema, SCHEMA, "a reader can compare and refuse");
    }

    /// Provenance survives, so a reader can tell CEN's rules from ours.
    #[test]
    fn provenance_is_carried_per_finding() {
        let out = Report::of(&validate(&Invoice::default()));
        let br = out
            .findings
            .iter()
            .find(|e| e.rule.starts_with("BR-"))
            .expect("a CEN rule fired");
        assert!(
            matches!(&*br.source, "standard+artefact" | "standard" | "artefact"),
            "{}",
            br.source
        );
    }
}
