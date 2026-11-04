//! Receiving: bytes and a policy in, a decision out.
//!
//! [`receive`] recognises the syntax, reads the document, classifies BT-24,
//! validates under the declared and required profiles, and recommends, with
//! the semantics KoSIT's validator uses.
//!
//! | | |
//! |---|---|
//! | **Reject** | unreadable; a container, syntax or schema layer invalid; BT-24 names no profile this build ships (or names one ambiguously, or another process); or a finding is fatal — after the policy's re-levellings — under the declared or the required profile |
//! | **Undefined** | the document is lawful as what it declares and is not an EN 16931 invoice: ZUGFeRD MINIMUM and BASIC WL |
//! | **Accept** | everything else |
//!
//! Everything the recommendation is derived from is in the two reports.

use en16931::profiles::{self, Declaration};
use en16931::validation::Check;
use en16931::{Invoice, Profile, Severity};

use crate::report::{self, Entry, Identity, Kind, Layer, Report, RuleSet, Semantic, Validity};
use crate::{Error, Read};

/// What the receiver requires, and how it reads the authorities' severities.
///
/// Supplied by the receiver, never inferred from the document.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    /// The profile the receiver requires — `Some(&profiles::XRECHNUNG)` for a
    /// German public buyer. `None` checks the document as declared only.
    pub require: Option<&'static Profile>,
    /// Rules the receiver reports at another severity — KoSIT's
    /// `customLevel`, as a receiver's own deviation. Recorded in both reports.
    pub relevel: Vec<(String, Severity)>,
    /// Rules the receiver does not run. Recorded in both reports as not
    /// checked; a report carrying one never yields a proof.
    pub without: Vec<String>,
    /// The bounds a hybrid PDF is extracted under.
    #[cfg(feature = "zugferd")]
    #[cfg_attr(docsrs, doc(cfg(feature = "zugferd")))]
    pub limits: crate::zugferd::Limits,
}

/// What to do with a received document — KoSIT's three answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum Recommendation {
    /// Nothing is effectively fatal: process it.
    Accept,
    /// Do not process it; [`Received::reasons`] says why.
    Reject,
    /// The document is not an EN 16931 invoice (ZUGFeRD MINIMUM, BASIC WL):
    /// the totals its terms carry were checked, and whether to process it is a
    /// business decision, not a validation result.
    Undefined,
}

impl Recommendation {
    /// `accept`, `reject` or `undefined` — the interchange word.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accept => "accept",
            Self::Reject => "reject",
            Self::Undefined => "undefined",
        }
    }
}

impl core::fmt::Display for Recommendation {
    /// `accept`, `REJECT`, `undefined` — the refusal stands out, as an invalid
    /// layer does.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Reject => f.write_str("REJECT"),
            other => f.write_str(other.as_str()),
        }
    }
}

/// A received document: what it declared, two reports, and a recommendation.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Received {
    /// The model, when the document could be read.
    pub invoice: Option<Invoice>,
    /// What BT-24 declares, classified — `None` only when nothing was read.
    pub declared: Option<Declaration>,
    /// The document under its declared profile — under the core, for
    /// information, when the declaration names none.
    pub report_declared: Report,
    /// The document under the profile the policy requires, when that is not
    /// the declared one.
    pub report_required: Option<Report>,
    /// What to do with it.
    pub recommendation: Recommendation,
    /// Why, one line per cause. Empty for `Accept`.
    pub reasons: Vec<String>,
    coincides: bool,
}

impl Received {
    /// Whether the policy required a profile and the document declares exactly
    /// that one — so one report answers both questions.
    #[must_use]
    pub fn coincides(&self) -> bool {
        self.coincides
    }

    /// The whole result in `en16931`'s interchange shape
    /// ([`en16931::report::SCHEMA`]): the report as declared, carrying the
    /// declaration, the recommendation and its reasons, with the report as
    /// required nested in [`required`](en16931::report::Report::required).
    ///
    /// What the command-line tool prints as JSON and renders as SVRL.
    #[must_use]
    pub fn to_interchange(&self) -> en16931::report::Report {
        let mut out = self.report_declared.to_interchange();
        out.declaration = self.declared.as_ref().map(ToString::to_string);
        out.recommendation = Some(self.recommendation.as_str().into());
        out.reasons.clone_from(&self.reasons);
        out.required = self
            .report_required
            .as_ref()
            .map(|r| Box::new(r.to_interchange()));
        out
    }
}

impl core::fmt::Display for Received {
    /// The one text rendering of a received document: the recommendation and
    /// why, what BT-24 declares, the report as declared and — when the
    /// receiver requires another profile — the report as required.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "recommendation: {}", self.recommendation)?;
        for why in &self.reasons {
            writeln!(f, "  because {why}")?;
        }
        match &self.declared {
            Some(d) => writeln!(f, "declared: {d}")?,
            None => writeln!(f, "declared: nothing was read")?,
        }
        write!(f, "{}", self.report_declared)?;
        if let Some(r) = &self.report_required {
            let id = r.rule_set.as_ref().map_or("", |rs| rs.profile.as_str());
            write!(f, "\n\nas required ({id}):\n{r}")?;
        }
        Ok(())
    }
}

/// Receive a document: read it, classify what it declares, validate it as
/// declared and as required, and recommend.
///
/// Never fails: an unreadable document is a [`Recommendation::Reject`] whose
/// report says which layer refused it and why.
///
/// ```
/// # #[cfg(feature = "ubl")] {
/// use en16931::profiles;
/// use en16931_formats::{Policy, Recommendation};
///
/// let policy = Policy { require: Some(&profiles::XRECHNUNG), ..Policy::default() };
/// let got = en16931_formats::receive(b"<Invoice/>", &policy);
/// assert_eq!(got.recommendation, Recommendation::Reject);
/// assert!(!got.reasons.is_empty());
/// # }
/// ```
#[must_use]
pub fn receive(bytes: &[u8], policy: &Policy) -> Received {
    #[cfg(feature = "zugferd")]
    let read = crate::read_with(bytes, &policy.limits);
    #[cfg(not(feature = "zugferd"))]
    let read = crate::read(bytes);
    assess(read, policy)
}

/// Whether an error is the container's — the PDF, not the payload.
fn container_error(e: &Error) -> bool {
    match e {
        #[cfg(feature = "zugferd")]
        Error::Pdf(_) => true,
        Error::Unsupported { found, .. } => *found == "a PDF",
        _ => false,
    }
}

/// The decision, from a read (or its failure) and a policy.
pub(crate) fn assess(read: Result<Read, Error>, policy: &Policy) -> Received {
    let read = match read {
        Ok(r) => r,
        Err(e) => return unreadable(&e),
    };

    let (container, syntax, schema) = document_entries(&read);
    let (container, syntax) = (Layer::of(container), Layer::of(syntax));
    let invoice = read.invoice;
    let identity = Identity::of(&invoice);

    let declared = profiles::classify(invoice.specification_id.as_deref());
    let as_declared = declared.profile().unwrap_or(&profiles::EN16931);
    let run = |p: &'static Profile| {
        let mut check = Check::new(p);
        for (rule, level) in &policy.relevel {
            check = check.relevel(rule.clone(), *level);
        }
        for rule in &policy.without {
            check = check.without(rule.clone());
        }
        check.run(&invoice)
    };
    let layered = |p: &'static Profile| Report {
        identity: identity.clone(),
        rule_set: Some(RuleSet::of(p)),
        container: container.clone(),
        syntax: syntax.clone(),
        schema: schema_under(p, &schema),
        semantic: report::semantic(run(p)),
    };

    let report_declared = layered(as_declared);
    let coincides = policy
        .require
        .is_some_and(|r| declared.profile().is_some_and(|p| core::ptr::eq(p, r)));
    let report_required = policy.require.filter(|_| !coincides).map(layered);

    let mut reasons = Vec::new();
    for (name, layer) in [
        ("container", &report_declared.container),
        ("syntax", &report_declared.syntax),
        ("schema", &report_declared.schema),
    ] {
        if layer.validity == Validity::Invalid {
            reasons.push(format!(
                "the {name} layer is invalid: {}",
                fatal_entries(layer)
            ));
        }
    }
    // The container and syntax layers are the document's; the schema layer can
    // differ by profile (KoSIT's raise), so the required one is read too.
    if let (Some(r), Some(report)) = (policy.require, &report_required)
        && report.schema.validity == Validity::Invalid
        && report_declared.schema.validity != Validity::Invalid
    {
        reasons.push(format!(
            "the schema layer is invalid under the required {}: {}",
            r.id,
            fatal_entries(&report.schema)
        ));
    }
    match &declared {
        Declaration::Known(_) => {}
        Declaration::Unknown(None) => reasons.push(
            "BT-24 is absent, so no profile matches; the core rules ran for information".to_owned(),
        ),
        Declaration::Unknown(Some(id)) => reasons.push(format!(
            "BT-24 {id:?} names no profile this build ships; the core rules ran for information"
        )),
        Declaration::WrongProcess(id) => reasons.push(format!(
            "BT-24 {id:?} is the identifier of another business process; the core rules ran \
             for information"
        )),
    }
    if let Declaration::Known(p) = &declared
        && let Some(why) = fatal_findings(&report_declared)
    {
        reasons.push(format!("under the declared {}: {why}", p.id));
    }
    if let (Some(r), Some(report)) = (policy.require, &report_required)
        && let Some(why) = fatal_findings(report)
    {
        reasons.push(format!("under the required {}: {why}", r.id));
    }

    let recommendation = if !reasons.is_empty() {
        Recommendation::Reject
    } else if !as_declared.is_en16931_invoice() {
        reasons.push(format!(
            "{} is not an EN 16931 invoice: only the totals rules its terms carry ran",
            as_declared.id
        ));
        Recommendation::Undefined
    } else {
        Recommendation::Accept
    };

    Received {
        invoice: Some(invoice),
        declared: Some(declared),
        report_declared,
        report_required,
        recommendation,
        reasons,
        coincides,
    }
}

/// The container, syntax and schema layers of a read document, at the levels
/// CEN publishes.
pub(crate) fn document_layers(read: &Read) -> (Layer, Layer, Layer) {
    let (container, syntax, schema) = document_entries(read);
    (Layer::of(container), Layer::of(syntax), Layer::of(schema))
}

/// The entries of the container, syntax and schema layers of a read document.
fn document_entries(read: &Read) -> (Vec<Entry>, Vec<Entry>, Vec<Entry>) {
    let (schema, syntax): (Vec<Entry>, Vec<Entry>) = read
        .entries
        .iter()
        .cloned()
        .partition(|e| matches!(e.kind, Kind::Duplicate | Kind::Repeated | Kind::Schema));
    (container_entries(read), syntax, schema)
}

/// The schema layer as `profile` reads it.
///
/// KoSIT's XRechnung configuration reports two of CEN's warning-level CII
/// single-occurrence rules at error ([`crate::cii::KOSIT_RAISED`]); under a
/// profile built on that configuration, such a repeat is a duplicate.
fn schema_under(profile: &Profile, entries: &[Entry]) -> Layer {
    let kosit = profile.artefacts.contains(&profiles::KOSIT_CONFIG);
    Layer::of(
        entries
            .iter()
            .cloned()
            .map(|mut e| {
                if kosit && e.kind == Kind::Repeated && e.rule.as_deref().is_some_and(raised) {
                    e.kind = Kind::Duplicate;
                    e.message = format!(
                        "a further occurrence of an element CEN's syntax rule allows once, which \
                         KoSIT's configuration reports as an error under {}; the first is read, \
                         this one is not",
                        profile.id
                    );
                }
                e
            })
            .collect(),
    )
}

/// Whether KoSIT raises `rule` to error under XRechnung.
#[cfg_attr(not(feature = "cii"), allow(unused_variables))]
fn raised(rule: &str) -> bool {
    #[cfg(feature = "cii")]
    return crate::cii::KOSIT_RAISED.contains(&rule);
    #[cfg(not(feature = "cii"))]
    false
}

/// `"duplicate Foo/Bar; schema Baz"` — the fatal entries of a layer.
fn fatal_entries(layer: &Layer) -> String {
    layer
        .entries
        .iter()
        .filter(|e| e.level() == Severity::Fatal)
        .map(|e| match &e.term {
            // A reason names the business term, not only the syntax path.
            Some(t) => format!("{} {} ({t})", e.kind.as_str(), e.path),
            None => format!("{} {}", e.kind.as_str(), e.path),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// `"2 fatal finding(s): BR-CO-10, BR-CO-15"`, or `None` when nothing is fatal.
fn fatal_findings(report: &Report) -> Option<String> {
    let r = report.semantic.report.as_ref()?;
    let rules: Vec<&str> = r.fatal().map(|f| f.rule.as_str()).collect();
    (!rules.is_empty()).then(|| format!("{} fatal finding(s): {}", rules.len(), rules.join(", ")))
}

/// The container's entries: for a hybrid PDF, every disagreement between its
/// metadata and its payload.
#[cfg_attr(
    not(feature = "zugferd"),
    allow(clippy::unnecessary_wraps, unused_variables)
)]
fn container_entries(read: &Read) -> Vec<Entry> {
    #[cfg(feature = "zugferd")]
    if let Some(pdf) = &read.pdf {
        return pdf
            .divergence
            .iter()
            .map(|d| {
                let kind = if matches!(d, crate::zugferd::Divergence::Profile { .. }) {
                    Kind::Ambiguous
                } else {
                    Kind::Container
                };
                Entry::new(kind, "PDF", d.to_string())
            })
            .collect();
    }
    Vec::new()
}

/// A document that could not be read: the layer that refused it is invalid,
/// every layer above it skipped.
fn unreadable(e: &Error) -> Received {
    let skipped = |why: &str| Layer::skipped(why);
    let (container, syntax) = if container_error(e) {
        (
            Layer::of(vec![Entry::new(Kind::Unreadable, "PDF", e.to_string())]),
            skipped("the container could not be read"),
        )
    } else {
        (
            Layer::of(Vec::new()),
            Layer::of(vec![Entry::new(Kind::Unreadable, "XML", e.to_string())]),
        )
    };
    Received {
        invoice: None,
        declared: None,
        report_declared: Report {
            identity: Identity::default(),
            rule_set: None,
            container,
            syntax,
            schema: skipped("nothing was read"),
            semantic: Semantic {
                validity: Validity::Skipped("nothing was read".to_owned()),
                report: None,
            },
        },
        report_required: None,
        recommendation: Recommendation::Reject,
        reasons: vec![format!("unreadable: {e}")],
        coincides: false,
    }
}
