//! The validation engine — rules as data, findings as a report.
//!
//! [`ValidationReport`] carries every finding, in a stable order so two runs
//! diff cleanly; `report.into_result()` is the short path. A [`Rule`] carries
//! its id, severity, terms, provenance and text beside the predicate, so the
//! registry can be listed and queried (`rules::explain("BR-CO-14")`).

pub mod profile;
pub mod rules;

use core::fmt;

use crate::bt::{BtId, Path};
use crate::invoice::Invoice;

// ── RuleId ────────────────────────────────────────────────────────────────────

/// A business rule identifier, as the artefacts spell it.
///
/// [`matches`](Self::matches) is for user input and forgives only case and
/// zero-padding (EN 16931-1 writes `BR-CO-3`, the artefacts `BR-CO-03`). No
/// rule family is mapped to another (`BR-AF-*`, CEN's spelling before 1.3.17, is not `BR-IG-*`).
///
/// ```
/// use en16931::validation::RuleId;
///
/// let r = RuleId::new("BR-CO-14");
/// assert!(r.matches("BR-CO-14"));
/// assert!(r.matches("BR-CO-4") == false);
/// assert!(RuleId::new("BR-CO-03").matches("BR-CO-3"));   // padding
/// assert!(!RuleId::new("BR-IG-01").matches("BR-AF-1"));  // no family mapping
/// assert!(RuleId::new("BR-CO-14").matches("br-co-14"));  // case
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuleId(&'static str);

impl RuleId {
    /// Wrap a canonical id, as the artefacts spell it.
    #[must_use]
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    /// The canonical spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }

    /// Whether `query` names this rule, in any of its spellings.
    #[must_use]
    pub fn matches(self, query: &str) -> bool {
        canonical_eq(self.0, query)
    }
}

/// Compare two rule ids case-insensitively, with a trailing number compared
/// numerically (`BR-1` == `BR-01`). Does not allocate: it runs on the hot path.
fn canonical_eq(a: &str, b: &str) -> bool {
    let (head_a, num_a) = split_trailing_number(a.trim());
    let (head_b, num_b) = split_trailing_number(b.trim());
    if num_a != num_b {
        return false;
    }
    head_a.eq_ignore_ascii_case(head_b)
}

/// Split `BR-CO-14` into (`BR-CO`, `Some(14)`); leave `BR-DE-23-a` whole.
fn split_trailing_number(id: &str) -> (&str, Option<u32>) {
    match id.rsplit_once('-') {
        Some((head, tail))
            if !tail.is_empty() && tail.len() <= 9 && tail.bytes().all(|b| b.is_ascii_digit()) =>
        {
            (head, tail.parse().ok())
        }
        _ => (id, None),
    }
}

impl fmt::Display for RuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        crate::fmt::padded(f, self.0)
    }
}

// ── Severity and provenance ───────────────────────────────────────────────────

/// How badly a finding matters.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// The document is not valid. Every CEN rule except `BR-51`.
    Fatal,
    /// Advisory. In the CEN model, only `BR-51` (card-PAN masking).
    Warning,
    /// Informational, weaker than a warning — XRechnung's `flag="information"`.
    ///
    /// `Ord` orders most severe first (`Fatal < Warning < Info`), so
    /// `severity >= Warning` means advisory and a profile relaxes a rule by
    /// moving it to a greater value.
    Info,
}

impl core::fmt::Display for Severity {
    /// The SVRL spelling — `fatal`, `warning`, `information`.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        crate::fmt::padded(
            f,
            match self {
                Self::Fatal => "fatal",
                Self::Warning => "warning",
                Self::Info => "information",
            },
        )
    }
}

/// Parse a severity as an authority or a person writes it: `fatal` (or KoSIT's
/// `error`), `warning`, `information` (or `info`), case-insensitively.
///
/// ```
/// use en16931::Severity;
///
/// assert_eq!("error".parse::<Severity>()?, Severity::Fatal);
/// assert_eq!("Information".parse::<Severity>()?, Severity::Info);
/// let err = "fatale".parse::<Severity>().unwrap_err();
/// assert!(err.to_string().contains("fatal, warning or information"), "{err}");
/// # Ok::<(), en16931::validation::UnknownSeverity>(())
/// ```
impl core::str::FromStr for Severity {
    type Err = UnknownSeverity;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "fatal" | "error" => Ok(Self::Fatal),
            "warning" => Ok(Self::Warning),
            "information" | "info" => Ok(Self::Info),
            _ => Err(UnknownSeverity {
                name: s.trim().to_owned(),
            }),
        }
    }
}

/// A word that names no [`Severity`]. Its `Display` lists the accepted ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownSeverity {
    /// The word as given.
    pub name: String,
}

impl core::fmt::Display for UnknownSeverity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{:?} is not a severity; use fatal, warning or information (error and info are accepted too)",
            self.name
        )
    }
}

impl core::error::Error for UnknownSeverity {}

/// Where a rule comes from: the standard and the CEN artefacts are not the same
/// rule set (`BR-CO-25` is standard-only; `BR-CL-*`, `BR-DEC-*` and others are
/// artefact-only).
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    /// In EN 16931-1's rule tables and in the CEN artefacts.
    Both,
    /// Normative in EN 16931-1, **not shipped** as an artefact assertion.
    StandardOnly,
    /// Added by the artefacts, rendering normative prose as an assertion.
    ArtefactOnly,
    /// Added by this crate, outside the standard. Always namespaced `EN-*`.
    Crate,
}

// ── Finding ───────────────────────────────────────────────────────────────────

/// The arithmetic behind an arithmetic finding.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detail {
    /// What the rule computed.
    pub expected: String,
    /// What the document states.
    pub actual: String,
}

/// One thing wrong with the invoice.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Which rule.
    pub rule: String,
    /// How badly.
    pub severity: Severity,
    /// Where — `BG-25[2]/BT-151`, never an XPath.
    pub path: Path,
    /// The rule's own wording, verbatim.
    pub message: String,
    /// Expected versus actual, for the arithmetic rules.
    pub detail: Option<Detail>,
    /// Advice beyond the rule text, e.g. a withdrawn code's successor.
    ///
    /// Kept apart from `message` so the authority's wording stays verbatim.
    /// `None` for most findings. See [`crate::codes::guard`].
    pub hint: Option<String>,
}

impl fmt::Display for Finding {
    /// `fatal        [BR-CO-14] BT-110 — …`, severity padded so ids align.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:<12} [{}] {} — {}",
            self.severity, self.rule, self.path, self.message
        )?;
        if let Some(d) = &self.detail {
            write!(f, " (expected {}, found {})", d.expected, d.actual)?;
        }
        if let Some(h) = &self.hint {
            write!(f, " [hint: {h}]")?;
        }
        Ok(())
    }
}

// ── Report ────────────────────────────────────────────────────────────────────

/// Everything wrong with an invoice, in one pass.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ValidationReport {
    findings: Vec<Finding>,
    checked: usize,
    /// Which profile produced this; `None` for a bare [`validate`].
    profile: Option<String>,
    /// Which edition of EN 16931-1 those rules belong to.
    edition: crate::Edition,
    /// Rules the caller asked to skip — see [`Check::without`].
    suppressed: Vec<String>,
    /// Rules the caller re-levelled — see [`Check::relevel`].
    relevelled: Vec<(String, Severity)>,
    /// From [`profile::Profile::artefacts`]. Not serialised (`&'static`);
    /// [`crate::Report`] carries the same provenance as owned strings.
    #[cfg_attr(feature = "serde", serde(skip))]
    artefacts: &'static [profile::ArtefactRef],
}

impl ValidationReport {
    /// Whether no **fatal** finding was raised. Warnings do not invalidate.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.findings.iter().any(|f| f.severity == Severity::Fatal)
    }

    /// Every finding, most severe first, then by path.
    #[must_use]
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// The authority releases the rules that ran were verified against.
    ///
    /// Empty only for a report built by hand.
    #[must_use]
    pub fn artefacts(&self) -> &'static [profile::ArtefactRef] {
        self.artefacts
    }

    /// Only the fatal ones.
    pub fn fatal(&self) -> impl Iterator<Item = &Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Fatal)
    }

    /// Only the warnings.
    pub fn warnings(&self) -> impl Iterator<Item = &Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Warning)
    }

    /// Only the informational findings.
    pub fn info(&self) -> impl Iterator<Item = &Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Info)
    }

    /// Everything that is not fatal: warnings and information.
    pub fn advisory(&self) -> impl Iterator<Item = &Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity != Severity::Fatal)
    }

    /// How many rules were evaluated.
    #[must_use]
    pub fn rules_checked(&self) -> usize {
        self.checked
    }

    /// The profile this report was produced against; `None` for a bare
    /// [`validate`].
    #[must_use]
    pub fn profile(&self) -> Option<&str> {
        self.profile.as_deref()
    }

    /// The edition of EN 16931-1 the rules belong to.
    #[must_use]
    pub fn edition(&self) -> crate::Edition {
        self.edition
    }

    /// Rules the caller asked to skip, which were therefore not checked.
    #[must_use]
    pub fn suppressed(&self) -> &[String] {
        &self.suppressed
    }

    /// Rules the caller re-levelled, with the severity each was reported at —
    /// see [`Check::relevel`].
    #[must_use]
    pub fn relevelled(&self) -> &[(String, Severity)] {
        &self.relevelled
    }

    /// Record which profile produced this report.
    pub(crate) fn attribute_to(
        &mut self,
        id: &'static str,
        edition: crate::Edition,
        artefacts: &'static [profile::ArtefactRef],
    ) {
        self.profile = Some(id.to_owned());
        self.edition = edition;
        self.artefacts = artefacts;
    }

    /// Whether a given rule raised anything.
    #[must_use]
    pub fn has(&self, rule: &str) -> bool {
        self.findings.iter().any(|f| canonical_eq(&f.rule, rule))
    }

    /// Apply a profile's severity overrides after every rule has run (as
    /// KoSIT's `<customLevel>` does), then re-sort.
    pub(crate) fn relevel(&mut self, levels: &[(&'static str, Severity)]) {
        if levels.is_empty() {
            return;
        }
        for f in &mut self.findings {
            if let Some((_, level)) = levels.iter().find(|(id, _)| canonical_eq(&f.rule, id)) {
                f.severity = *level;
            }
        }
        sort_findings(&mut self.findings);
    }

    /// Merge additional findings in, keeping the stable order.
    pub(crate) fn absorb(&mut self, extra: Vec<Finding>, checked: usize) {
        self.findings.extend(extra);
        self.checked += checked;
        sort_findings(&mut self.findings);
    }

    /// ` (1 fatal, 2 information)`, or empty when fewer than two severities
    /// are present. Zero counts are left out.
    fn breakdown(&self) -> String {
        let count = |s: Severity| self.findings.iter().filter(|f| f.severity == s).count();
        let counts = [
            (count(Severity::Fatal), "fatal"),
            (count(Severity::Warning), "warning"),
            (count(Severity::Info), "information"),
        ];
        let present: Vec<String> = counts
            .iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, label)| format!("{n} {label}"))
            .collect();
        if present.len() < 2 {
            String::new()
        } else {
            format!(" ({})", present.join(", "))
        }
    }

    /// The ergonomic path: `report.into_result()?`. The report is an
    /// [`Error`](core::error::Error), so `?` converts it into `Box<dyn Error>`.
    ///
    /// ```
    /// use en16931::{Invoice, validate};
    ///
    /// fn check(invoice: &Invoice) -> Result<(), Box<dyn std::error::Error>> {
    ///     validate(invoice).into_result()?;
    ///     Ok(())
    /// }
    ///
    /// assert!(check(&Invoice::default()).is_err());
    /// ```
    ///
    /// # Errors
    /// The report itself, when any fatal finding was raised.
    // Both arms are the report; boxing one would make them different types.
    #[allow(clippy::result_large_err)]
    pub fn into_result(self) -> Result<Self, Self> {
        if self.is_valid() { Ok(self) } else { Err(self) }
    }
}

impl fmt::Display for ValidationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{} validation ({}) — {} rule(s) checked, {} finding(s){}{}",
            self.profile.as_deref().unwrap_or("EN 16931"),
            self.edition,
            self.checked,
            self.findings.len(),
            self.breakdown(),
            if self.is_valid() {
                ", valid"
            } else {
                ", INVALID"
            }
        )?;
        if !self.suppressed.is_empty() {
            writeln!(
                f,
                "  ⚠ {} rule(s) suppressed and NOT checked: {}",
                self.suppressed.len(),
                self.suppressed.join(", ")
            )?;
        }
        if !self.relevelled.is_empty() {
            let list: Vec<String> = self
                .relevelled
                .iter()
                .map(|(rule, level)| format!("{rule} → {level}"))
                .collect();
            writeln!(
                f,
                "  ⚠ {} rule(s) re-levelled by the caller: {}",
                self.relevelled.len(),
                list.join(", ")
            )?;
        }
        for finding in &self.findings {
            writeln!(f, "  {finding}")?;
        }
        if !self.artefacts.is_empty() {
            let refs: Vec<String> = self.artefacts.iter().map(ToString::to_string).collect();
            writeln!(f, "  verified against: {}", refs.join(", "))?;
        }
        write!(f, "  ({})", crate::ATTRIBUTION)
    }
}

impl core::error::Error for ValidationReport {}

// ── Rule ──────────────────────────────────────────────────────────────────────

/// Where a rule writes its findings.
///
/// Rules push through this rather than building [`Finding`] values, so the id,
/// severity and text come from the registry entry and cannot drift from it.
pub struct Findings<'a> {
    out: &'a mut Vec<Finding>,
    rule: &'static Rule,
}

impl<'a> Findings<'a> {
    /// Build a sink directly, for testing a rule in isolation.
    #[cfg(test)]
    pub(crate) fn for_test(out: &'a mut Vec<Finding>, rule: &'static Rule) -> Self {
        Self { out, rule }
    }
}

impl Findings<'_> {
    /// Report a failure at `path`.
    pub fn at(&mut self, path: Path) {
        self.push(path, None, None);
    }

    /// Report a failure at `path`, with advice the rule text cannot give.
    pub fn at_with_hint(&mut self, path: Path, hint: impl Into<String>) {
        self.push(path, None, Some(hint.into()));
    }

    /// As [`at_with_hint`](Self::at_with_hint), but the hint may be absent —
    /// which is what [`crate::codes::guard::CodeList::advice`] returns.
    pub fn at_maybe_hinted(&mut self, path: Path, hint: Option<String>) {
        self.push(path, None, hint);
    }

    /// Report a failure at `path`, showing the arithmetic.
    pub fn arithmetic(
        &mut self,
        path: Path,
        expected: impl fmt::Display,
        actual: impl fmt::Display,
    ) {
        self.push(
            path,
            Some(Detail {
                expected: expected.to_string(),
                actual: actual.to_string(),
            }),
            None,
        );
    }

    /// The one place a [`Finding`] is built.
    fn push(&mut self, path: Path, detail: Option<Detail>, hint: Option<String>) {
        self.out.push(Finding {
            rule: self.rule.id.as_str().to_owned(),
            severity: self.rule.severity,
            path,
            message: self.rule.text.to_owned(),
            detail,
            hint,
        });
    }
}

/// One business rule.
pub struct Rule {
    /// Canonical id, as the artefacts spell it.
    pub id: RuleId,
    /// Fatal or advisory.
    pub severity: Severity,
    /// The rule's own wording.
    pub text: &'static str,
    /// The business terms it touches — powers "which rules concern BT-117".
    pub terms: &'static [BtId],
    /// Standard, artefact, both, or ours.
    pub source: Source,
    /// The predicate.
    pub eval: fn(&Invoice, &mut Findings<'_>),
}

impl fmt::Debug for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rule")
            .field("id", &self.id)
            .field("severity", &self.severity)
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

/// Run `rules` over `invoice`. Findings are ordered by severity, path, rule id.
#[must_use]
pub fn validate_with(invoice: &Invoice, rules: &[&'static Rule]) -> ValidationReport {
    let mut findings = Vec::new();
    for rule in rules {
        let mut sink = Findings {
            out: &mut findings,
            rule,
        };
        (rule.eval)(invoice, &mut sink);
    }
    sort_findings(&mut findings);
    ValidationReport {
        findings,
        checked: rules.len(),
        profile: None,
        edition: crate::DEFAULT_EDITION,
        suppressed: Vec::new(),
        relevelled: Vec::new(),
        // `Profile::validate` replaces this with the profile's own list.
        artefacts: crate::profiles::EN16931.artefacts,
    }
}

/// Run a filtered core sequence and `extra` as one pass, without collecting
/// them into a `Vec` first.
pub(crate) fn validate_with_all<I>(
    invoice: &Invoice,
    core: I,
    extra: &[&'static Rule],
) -> ValidationReport
where
    I: Iterator<Item = &'static Rule>,
{
    let mut findings = Vec::new();
    let mut checked = 0usize;
    for rule in core.chain(extra.iter().copied()) {
        checked += 1;
        let mut sink = Findings {
            out: &mut findings,
            rule,
        };
        (rule.eval)(invoice, &mut sink);
    }
    sort_findings(&mut findings);
    ValidationReport {
        findings,
        checked,
        profile: None,
        edition: crate::DEFAULT_EDITION,
        suppressed: Vec::new(),
        relevelled: Vec::new(),
        artefacts: crate::profiles::EN16931.artefacts,
    }
}

/// Severity, then path, then rule id — so two runs are byte-identical.
fn sort_findings(findings: &mut [Finding]) {
    findings.sort_by(|a, b| {
        a.severity
            .cmp(&b.severity)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.rule.cmp(&b.rule))
    });
}

/// Run the full EN 16931 core rule set; `rules_checked()` is
/// [`rules::CORE`]`.len()`.
#[must_use]
pub fn validate(invoice: &Invoice) -> ValidationReport {
    validate_with(invoice, &rules::CORE)
}

/// A validation run with deviations (suppressed or re-levelled rules).
///
/// Deviations are recorded on the [`ValidationReport`] and printed by its
/// `Display`, and [`Check::prove`] refuses to produce a
/// [`profile::Validated<P>`] for such a run.
///
/// ```
/// use en16931::{Invoice, profiles};
/// use en16931::validation::Check;
///
/// let report = Check::new(&profiles::XRECHNUNG)
///     .without("BR-DE-15")            // the buyer will not send BT-10
///     .run(&Invoice::default());
///
/// assert_eq!(report.suppressed(), ["BR-DE-15"]);
/// assert!(report.to_string().contains("suppressed and NOT checked"));
/// ```
#[derive(Debug, Clone)]
pub struct Check {
    profile: &'static profile::Profile,
    suppressed: Vec<String>,
    relevelled: Vec<(String, Severity)>,
}

impl Check {
    /// Start a run against a profile chosen at runtime. Use [`Check::of`]
    /// when the run ends in [`prove`](Self::prove).
    #[must_use]
    pub fn new(profile: &'static profile::Profile) -> Self {
        Self {
            profile,
            suppressed: Vec::new(),
            relevelled: Vec::new(),
        }
    }

    /// Start a run against the profile `P` names, so [`prove`](Self::prove)
    /// cannot claim a different one.
    ///
    /// ```
    /// use en16931::profiles::XRechnung;
    /// use en16931::validation::Check;
    ///
    /// let check = Check::of::<XRechnung>();
    /// assert_eq!(check.profile().id, "XRechnung 3.0");
    /// ```
    #[must_use]
    pub fn of<P: profile::ProfileMarker>() -> Self {
        Self::new(P::PROFILE)
    }

    /// The profile this run checks against.
    #[must_use]
    pub fn profile(&self) -> &'static profile::Profile {
        self.profile
    }

    /// Skip a rule, by any of its spellings.
    ///
    /// An id naming no rule this profile runs is recorded on the report but
    /// does not reduce [`ValidationReport::rules_checked`].
    #[must_use]
    pub fn without(mut self, rule: impl Into<String>) -> Self {
        self.suppressed.push(rule.into());
        self
    }

    /// Report `rule` at `to` instead of the severity the profile gives it, as
    /// KoSIT's `customLevel` does. The rule still runs.
    ///
    /// Recorded on the report ([`ValidationReport::relevelled`]); any
    /// re-levelling, raising included, makes [`Check::prove`] refuse.
    ///
    /// ```
    /// use en16931::{Invoice, Severity, profiles};
    /// use en16931::validation::Check;
    ///
    /// let report = Check::new(&profiles::XRECHNUNG)
    ///     .relevel("BR-CL-23", Severity::Warning)
    ///     .run(&Invoice::default());
    /// assert_eq!(report.relevelled(), [("BR-CL-23".to_owned(), Severity::Warning)]);
    /// assert!(report.to_string().contains("re-levelled"));
    /// ```
    #[must_use]
    pub fn relevel(mut self, rule: impl Into<String>, to: Severity) -> Self {
        self.relevelled.push((rule.into(), to));
        self
    }

    /// The re-levellings this run applies.
    #[must_use]
    pub fn relevellings(&self) -> &[(String, Severity)] {
        &self.relevelled
    }

    /// How many of this profile's checks the suppressions remove (not
    /// `suppressed.len()`, which counts requests).
    fn skipped(&self) -> usize {
        self.profile
            .check_ids()
            .filter(|id| self.suppressed.iter().any(|s| canonical_eq(id, s)))
            .count()
    }

    /// The ids this run will skip.
    #[must_use]
    pub fn suppressions(&self) -> &[String] {
        &self.suppressed
    }

    /// Validate, skipping the suppressed rules and recording that it did.
    #[must_use]
    pub fn run(&self, invoice: &Invoice) -> ValidationReport {
        let mut report = self.profile.validate(invoice);
        if !self.suppressed.is_empty() {
            report
                .findings
                .retain(|f| !self.suppressed.iter().any(|s| canonical_eq(&f.rule, s)));
            report.checked = report.checked.saturating_sub(self.skipped());
            report.suppressed.clone_from(&self.suppressed);
        }
        if !self.relevelled.is_empty() {
            for f in &mut report.findings {
                if let Some((_, level)) = self
                    .relevelled
                    .iter()
                    .rev()
                    .find(|(id, _)| canonical_eq(&f.rule, id))
                {
                    f.severity = *level;
                }
            }
            sort_findings(&mut report.findings);
            report.relevelled.clone_from(&self.relevelled);
        }
        report
    }

    /// Validate and hand back a typed proof — only with no deviations, and
    /// only when `P` is the profile this `Check` runs.
    ///
    /// # Errors
    /// [`ProveError::WrongProfile`] when `P` does not name this `Check`'s
    /// profile, [`ProveError::Suppressed`] or [`ProveError::Relevelled`] when
    /// the run has deviations (use [`Check::run`] instead), and
    /// [`ProveError::Rejected`] when the invoice does not pass.
    pub fn prove<P>(&self, invoice: Invoice) -> Result<profile::Validated<P>, ProveError>
    where
        P: profile::ProfileMarker,
    {
        // Pointer identity: two profiles sharing a name are still two rule sets.
        if !core::ptr::eq(self.profile, P::PROFILE) {
            return Err(ProveError::WrongProfile {
                checked: self.profile.id,
                claimed: P::PROFILE.id,
            });
        }
        if !self.suppressed.is_empty() {
            return Err(ProveError::Suppressed(self.suppressed.clone()));
        }
        if !self.relevelled.is_empty() {
            return Err(ProveError::Relevelled(self.relevelled.clone()));
        }
        profile::Validated::new(invoice).map_err(ProveError::Rejected)
    }
}

/// Why [`Check::prove`] did not produce a proof.
#[derive(Debug)]
#[non_exhaustive]
pub enum ProveError {
    /// The type parameter named a different profile than the one being checked.
    WrongProfile {
        /// The profile this [`Check`] runs.
        checked: &'static str,
        /// The profile the requested proof would have claimed.
        claimed: &'static str,
    },
    /// Rules were suppressed, so no proof can honestly be made.
    Suppressed(Vec<String>),
    /// Rules were re-levelled, so the rule set that ran is not the profile's.
    Relevelled(Vec<(String, Severity)>),
    /// The invoice did not pass; the report says why.
    Rejected(profile::Rejected),
}

impl fmt::Display for ProveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongProfile { checked, claimed } => write!(
                f,
                "cannot prove validity: this check runs {checked} and the proof \
                 would claim {claimed}. Use `Check::of::<{claimed}>()`, or ask for \
                 a proof of {checked}."
            ),
            Self::Suppressed(ids) => write!(
                f,
                "cannot prove validity: {} rule(s) were suppressed ({}). A proof \
                 means the whole rule set passed; use `Check::run` for a report.",
                ids.len(),
                ids.join(", ")
            ),
            Self::Relevelled(ids) => write!(
                f,
                "cannot prove validity: {} rule(s) were re-levelled ({}). A proof \
                 means the profile passed at its published severities; use \
                 `Check::run` for a report.",
                ids.len(),
                ids.iter()
                    .map(|(r, l)| format!("{r} → {l}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Rejected(r) => write!(f, "invoice is not valid:\n{r}"),
        }
    }
}

impl core::error::Error for ProveError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_ids_normalise_padding_and_case_and_nothing_else() {
        assert!(RuleId::new("BR-CO-03").matches("BR-CO-3"));
        assert!(RuleId::new("BR-CO-03").matches("br-co-3"));
        assert!(RuleId::new("BR-01").matches("BR-1"));
        assert!(RuleId::new("BR-S-01").matches("BR-S-1"));
        // No family is mapped to another.
        assert!(!RuleId::new("BR-IG-01").matches("BR-AF-1"));
        assert!(!RuleId::new("BR-IP-10").matches("BR-AG-10"));
        // …but distinct rules stay distinct.
        assert!(!RuleId::new("BR-CO-13").matches("BR-CO-14"));
        assert!(!RuleId::new("BR-CO-01").matches("BR-CO-10"));
    }

    #[test]
    fn suffixed_ids_are_left_alone() {
        // XRechnung has BR-DE-23-a / -b; padding must not mangle them.
        assert!(RuleId::new("BR-DE-23-a").matches("br-de-23-a"));
        assert!(!RuleId::new("BR-DE-23-a").matches("BR-DE-23-b"));
    }

    /// A proof names the rule set that actually ran, or it is not produced.
    #[test]
    fn a_proof_cannot_name_a_profile_the_check_did_not_run() {
        use crate::profiles::{En16931, XRechnung};
        use crate::validation::profile::ProfileMarker as _;
        use crate::{Invoice, profiles};

        let err = Check::new(&profiles::XRECHNUNG)
            .prove::<En16931>(Invoice::default())
            .expect_err("a core proof out of an XRechnung check");
        assert!(
            matches!(
                err,
                ProveError::WrongProfile {
                    checked: "XRechnung 3.0",
                    claimed: "EN 16931"
                }
            ),
            "{err}"
        );

        let err = Check::new(&profiles::EN16931)
            .prove::<XRechnung>(Invoice::default())
            .expect_err("an XRechnung proof out of a core check");
        assert!(matches!(err, ProveError::WrongProfile { .. }), "{err}");

        // `Check::of` cannot be mismatched: the marker is the profile.
        assert!(core::ptr::eq(
            Check::of::<XRechnung>().profile(),
            XRechnung::PROFILE
        ));
    }

    /// The suppression refusal still applies once the profiles agree.
    #[test]
    fn a_matching_profile_still_cannot_prove_a_suppressed_run() {
        use crate::Invoice;
        use crate::profiles::En16931;

        let err = Check::of::<En16931>()
            .without("BR-02")
            .prove::<En16931>(Invoice::default())
            .expect_err("suppressed");
        assert!(matches!(err, ProveError::Suppressed(_)), "{err}");
    }

    /// A re-levelled rule changes severity, is recorded, and blocks the proof.
    #[test]
    fn a_relevelled_check_reports_its_deviation_and_proves_nothing() {
        use crate::profiles::En16931;
        use crate::{Invoice, profiles};

        let plain = Check::new(&profiles::EN16931).run(&Invoice::default());
        assert!(
            plain
                .findings()
                .iter()
                .any(|f| f.rule == "BR-01" && f.severity == Severity::Fatal)
        );
        let report = Check::new(&profiles::EN16931)
            .relevel("br-1", Severity::Warning)
            .run(&Invoice::default());
        let br01 = report
            .findings()
            .iter()
            .find(|f| f.rule == "BR-01")
            .expect("fires");
        assert_eq!(br01.severity, Severity::Warning);
        assert_eq!(
            report.relevelled(),
            [("br-1".to_owned(), Severity::Warning)]
        );
        assert!(
            report
                .to_string()
                .contains("re-levelled by the caller: br-1 → warning")
        );

        for level in [Severity::Warning, Severity::Fatal] {
            let err = Check::of::<En16931>()
                .relevel("BR-01", level)
                .prove::<En16931>(Invoice::default())
                .expect_err("re-levelled");
            assert!(matches!(err, ProveError::Relevelled(_)), "{err}");
        }
    }

    /// The bare core path and `profiles::EN16931` agree on findings and count.
    #[test]
    fn the_bare_core_path_agrees_with_the_core_profile() {
        use crate::{Invoice, profiles};

        let inv = Invoice::default();
        let bare = validate(&inv);
        let via_profile = profiles::EN16931.validate(&inv);
        assert_eq!(bare.rules_checked(), via_profile.rules_checked());
        assert_eq!(bare.findings(), via_profile.findings());
    }
}
