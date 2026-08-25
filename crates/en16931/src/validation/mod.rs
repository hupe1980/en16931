//! The validation engine — rules as data, findings as a report.
//!
//! # A report, not a `Result<()>`
//!
//! A rejection from a clearing platform lists every problem with a document. A
//! validator that reports one is a validator you run in a loop, fixing a field
//! at a time. [`ValidationReport`] carries them all, ordered stably so a CI diff
//! is meaningful.
//!
//! `report.into_result()` is there for the ergonomic path, but the report is the
//! product.
//!
//! # Rules are data
//!
//! A [`Rule`] carries its id, severity, the business terms it touches, its
//! provenance and its text alongside the predicate. That makes the registry
//! listable, filterable and explainable — `rules::explain("BR-CO-14")` works,
//! and so does "which rules touch BT-117".

pub mod profile;
pub mod rules;

use core::fmt;

use crate::bt::{BtId, Path};
use crate::invoice::Invoice;

// ── RuleId ────────────────────────────────────────────────────────────────────

/// A business rule identifier, compared **canonically**.
///
/// Two purely notational differences between the standard and the CEN artefacts
/// break naive string lookup, and both are handled here:
///
/// **Zero-padding.** EN 16931-1 writes `BR-1`, `BR-S-1`, `BR-CO-3`. The
/// artefacts write `BR-01`, `BR-S-01`, `BR-CO-03`. A user reading the PDF and
/// asking for `BR-CO-3` must not get "no such rule".
///
/// **Family renames.** The standard's IGIC and IPSI families are `BR-IG-*` and
/// `BR-IP-*`; the artefacts call the same rules `BR-AF-*` and `BR-AG-*`. Same
/// text, same meaning, different spelling.
///
/// ```
/// use en16931::validation::RuleId;
///
/// let r = RuleId::new("BR-CO-14");
/// assert!(r.matches("BR-CO-14"));
/// assert!(r.matches("BR-CO-4") == false);
/// assert!(RuleId::new("BR-CO-03").matches("BR-CO-3"));   // padding
/// assert!(RuleId::new("BR-AF-01").matches("BR-IG-1"));   // family alias
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

/// Compare two rule ids canonically, **without allocating**.
///
/// Canonical means: case-insensitive, the renamed families aliased
/// (`BR-IG-*` → `BR-AF-*`, `BR-IP-*` → `BR-AG-*`), and a trailing number
/// compared numerically so `BR-1`, `BR-01` and `BR-001` agree.
///
/// # Why not build the canonical string
///
/// It costs 24× on the hot path. `Profile::validate` asks `matches` once per
/// rule per document — around 300 calls — and building up to three `String`s
/// each time turns 1.5 µs of core validation into 35 µs of profile validation,
/// for a profile that adds no rules at all. Comparing two ids should not touch
/// the allocator.
fn canonical_eq(a: &str, b: &str) -> bool {
    let (head_a, num_a) = split_trailing_number(a.trim());
    let (head_b, num_b) = split_trailing_number(b.trim());
    // A numeric tail is compared as a number, so `-1` == `-01`. Ids without one
    // (`BR-DE-23-a`) compare whole.
    if num_a != num_b {
        return false;
    }
    let (prefix_a, rest_a) = alias_family(head_a);
    let (prefix_b, rest_b) = alias_family(head_b);
    eq_ignore_case_concat(prefix_a, rest_a, prefix_b, rest_b)
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

/// Rewrite a renamed family prefix, returning it split so nothing is allocated.
///
/// The standard's IGIC and IPSI families are `BR-IG-*` and `BR-IP-*`; the
/// artefacts call the same rules `BR-AF-*` and `BR-AG-*`.
fn alias_family(head: &str) -> (&'static str, &str) {
    const ALIASES: [(&str, &str); 2] = [("BR-IG", "BR-AF"), ("BR-IP", "BR-AG")];
    for (from, to) in ALIASES {
        if head.len() >= from.len() && head[..from.len()].eq_ignore_ascii_case(from) {
            return (to, &head[from.len()..]);
        }
    }
    ("", head)
}

/// `a1 + a2 == b1 + b2`, case-insensitively, without building either.
fn eq_ignore_case_concat(a1: &str, a2: &str, b1: &str, b2: &str) -> bool {
    let a = a1.bytes().chain(a2.bytes()).map(|b| b.to_ascii_uppercase());
    let b = b1.bytes().chain(b2.bytes()).map(|b| b.to_ascii_uppercase());
    a.eq(b)
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
    /// The document is not valid. 200 of the 201 CEN abstract-model rules.
    Fatal,
    /// Advisory. In the CEN model, exactly one rule — `BR-51`, card-PAN masking.
    Warning,
    /// Informational. XRechnung's `flag="information"`, weaker than a warning:
    /// `BR-DE-TMP-32` suggests stating a delivery date and does not object if
    /// you do not.
    ///
    /// # The ordering, stated because it reads backwards
    ///
    /// `Ord` is derived, so the variants order **most severe first**:
    /// `Fatal < Warning < Info`. Read as a number that is the opposite of
    /// "severity", which is why the useful comparison is `>=`:
    /// `severity >= Warning` means *"advisory"* and `severity == Fatal` means
    /// *"invalid"*. [`ValidationReport::advisory`] is the same test with a name.
    ///
    /// This is also what makes [`crate::Profile::is_conformant_cius`] a `<=`:
    /// a profile **relaxes** a rule when it moves it to a greater value.
    Info,
}

impl core::fmt::Display for Severity {
    /// The SVRL spelling — `fatal`, `warning`, `information`.
    ///
    /// Not `Debug`'s `Fatal`: a severity printed in a report is read by people
    /// and by other tools, and every Schematron implementation in this field
    /// writes these three words.
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

/// Where a rule comes from, because the standard and the artefacts are not the
/// same rule set.
///
/// Diffing them yields exactly one rule in the standard that the artefacts do
/// not ship (`BR-CO-25`) and 47 the artefacts add — the `BR-CL-*` and `BR-DEC-*`
/// families, `BR-B-*`, and `BR-AF-10`.
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
    /// The rule's own wording.
    ///
    /// Owned rather than `&'static str` so a report survives deserialisation —
    /// a report is an artefact you store, ship and diff, not only one you print.
    pub message: String,
    /// Expected versus actual, for the arithmetic rules.
    pub detail: Option<Detail>,
    /// What to do about it, when this crate can say something the rule text
    /// cannot.
    ///
    /// # Why this is separate from `message`
    ///
    /// `message` is the **authority's** wording, verbatim, and it is what makes
    /// a finding look up in KoSIT's or CEN's index. Appending advice to it would
    /// quietly make this crate's reports disagree with every other tool's on the
    /// text of a rule.
    ///
    /// So the advice goes here. `BR-CL-25` says only *"MUST belong to the CEF
    /// EAS code list"*; the hint says *"9958 was the Peppol Leitweg-ID scheme
    /// and has been withdrawn; use 0204"*, which is the sentence that actually
    /// ends the investigation. See [`crate::codes::guard`], which derives these
    /// and offers the same advice at mapping time.
    ///
    /// `None` for the great majority of findings. A hint is only present where
    /// the crate genuinely knows more than the rule text, never as filler.
    pub hint: Option<String>,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {} — {}", self.rule, self.path, self.message)?;
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
    /// Which profile produced this, when one did.
    ///
    /// `None` for a bare [`validate`] against the core rule set. A stored report
    /// that cannot say what it was checked against is close to useless six
    /// months later — "this invoice was valid" means nothing without "under
    /// which rule set", and the two profiles that are not conformant CIUSes make
    /// the difference load-bearing rather than pedantic.
    ///
    /// Owned rather than `&'static str`: a report is an artefact you store and
    /// read back, and a borrowed field cannot be deserialised from a buffer. One
    /// allocation per *report* — not per rule — which is nothing beside the
    /// findings it already carries.
    profile: Option<String>,
    /// Which edition of EN 16931-1 those rules belong to.
    edition: crate::Edition,
    /// Rules the caller asked to skip — see [`crate::validation::Check`].
    ///
    /// A report that quietly omits what it did not check is worse than no
    /// report, so these are carried, printed by `Display`, and block the typed
    /// proof.
    suppressed: Vec<String>,
    /// The authority releases the rules that ran were verified against.
    ///
    /// Taken from [`profile::Profile::artefacts`]. `&'static` rather than
    /// owned, because every value is a compile-time constant — which is also
    /// why it is **not** serialised here: a `&'static str` cannot be
    /// deserialised from a buffer, and this type's serde shape is the crate's
    /// internals rather than its wire format. [`crate::Report`] is the stored
    /// shape, and it carries the same provenance in owned `String`s.
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
    /// Empty only for a report built by hand. A stored report saying
    /// `BR-DE-15` beside `CEN validation-1.3.16` names the wrong authority —
    /// `BR-DE-*` is KoSIT's, on its own release cadence — so every profile
    /// lists all of the releases its rules came from.
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
    ///
    /// XRechnung's `flag="information"` — weaker than a warning. `BR-DE-TMP-32`
    /// suggests stating a delivery date and does not object if you do not.
    pub fn info(&self) -> impl Iterator<Item = &Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Info)
    }

    /// Everything that is not fatal: warnings **and** information.
    ///
    /// The useful split in practice is "must fix" versus "should look at", and
    /// that is this, not [`warnings`](Self::warnings) alone — which silently
    /// omits `Severity::Info`.
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

    /// The profile this report was produced against, if any.
    ///
    /// `None` means the bare core rule set — [`validate`] rather than
    /// [`profile::Profile::validate`].
    #[must_use]
    pub fn profile(&self) -> Option<&str> {
        self.profile.as_deref()
    }

    /// The edition of EN 16931-1 the rules belong to.
    #[must_use]
    pub fn edition(&self) -> crate::Edition {
        self.edition
    }

    /// Rules the caller asked to skip, and which therefore were **not checked**.
    ///
    /// Empty for an ordinary run. Non-empty means this report is a weaker claim
    /// than it looks, which is why it is on the report rather than only in the
    /// caller's head.
    #[must_use]
    pub fn suppressed(&self) -> &[String] {
        &self.suppressed
    }

    /// Record which profile produced this report.
    ///
    /// Takes the fields rather than the `Profile`, because a `&Profile` method
    /// receiver is not `&'static` even when every profile is a static.
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

    /// Apply a profile's severity overrides, then re-sort.
    ///
    /// Runs after every rule and restriction, because the authorities' own
    /// configurations do exactly that: KoSIT's `<customLevel>` sits in
    /// `<createReport>`, not in the Schematron. The rule fired; what changes is
    /// what the reader is expected to do about it.
    pub(crate) fn relevel(&mut self, levels: &[(&'static str, Severity)]) {
        if levels.is_empty() {
            return;
        }
        // `canonical_eq` rather than `==`, unlike the rule-set filtering: a
        // profile's overrides are transcribed from an authority's own
        // configuration, and an authority is entitled to spell a family the way
        // the standard does. The list is single digits long and runs once per
        // *finding*, not once per rule, so the cost is nothing.
        for f in &mut self.findings {
            if let Some((_, level)) = levels.iter().find(|(id, _)| canonical_eq(&f.rule, id)) {
                f.severity = *level;
            }
        }
        sort_findings(&mut self.findings);
    }

    /// Merge additional findings in, keeping the stable order.
    ///
    /// Used by [`profile::Profile::validate`] to fold restriction findings into
    /// the core report, so a caller sees one report rather than two.
    pub(crate) fn absorb(&mut self, extra: Vec<Finding>, checked: usize) {
        self.findings.extend(extra);
        self.checked += checked;
        self.findings.sort_by(|a, b| {
            a.severity
                .cmp(&b.severity)
                .then_with(|| a.path.cmp(&b.path))
                .then_with(|| a.rule.cmp(&b.rule))
        });
    }

    /// The ergonomic path: `report.into_result()?`.
    ///
    /// # Errors
    /// The report itself, when any fatal finding was raised.
    pub fn into_result(self) -> Result<Self, Self> {
        if self.is_valid() { Ok(self) } else { Err(self) }
    }
}

impl fmt::Display for ValidationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Name the rule set. A report listing `BR-DE-*` findings and a count of
        // 280 leaves the reader to infer which profile ran — and with two
        // shipped profiles that are not conformant CIUSes, that inference is
        // exactly the thing not to leave to the reader.
        writeln!(
            f,
            "{} validation ({}) — {} rule(s) checked, {} finding(s){}",
            self.profile.as_deref().unwrap_or("EN 16931"),
            self.edition,
            self.checked,
            self.findings.len(),
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
        for finding in &self.findings {
            writeln!(f, "  {finding}")?;
        }
        // Which releases these verdicts were checked against. Printed last and
        // once, because it is provenance rather than a finding — but printed,
        // because "BR-DE-15 fired" is only actionable if the reader knows which
        // XRechnung release said so.
        if !self.artefacts.is_empty() {
            let refs: Vec<String> = self.artefacts.iter().map(ToString::to_string).collect();
            writeln!(f, "  verified against: {}", refs.join(", "))?;
        }
        write!(f, "  ({})", crate::ATTRIBUTION)
    }
}

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
    ///
    /// Use this only where the crate knows something specific — a withdrawn
    /// code's successor, the term the value actually belongs in. A hint that
    /// restates the rule is noise in every report that carries it.
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

    /// The one place a [`Finding`] is built, so id, severity and text always
    /// come from the registry entry and cannot drift from it.
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

/// Run `rules` over `invoice`.
///
/// Findings come back ordered by severity then path, so two runs over equal
/// input produce byte-identical reports and a CI diff means something.
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
        // A bare core run is verified against CEN's release and nothing else.
        // `Profile::validate` replaces this with the profile's own list.
        artefacts: crate::profiles::EN16931.artefacts,
    }
}

/// Run two rule sequences as one pass, without joining them first.
///
/// A profile is *core plus its own*, and building a `Vec` of every rule to
/// express that costs more than evaluating several of them. This is the shape
/// [`profile::Profile::validate`] wants: a filtered core, then `extra_rules`,
/// one report.
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
        // A bare core run is verified against CEN's release and nothing else.
        // `Profile::validate` replaces this with the profile's own list.
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

/// Run the full EN 16931 core rule set.
///
/// Every rule in the registry, unconditionally — so `rules_checked()` is
/// [`rules::CORE`]`.len()`, which is the number the documentation quotes.
///
/// `EN-EXT-01` included: a rule with nothing to say has still been checked, and
/// filtering it out would make the count one lower than every published figure
/// on any invoice without extension data.
/// [`profile::Profile::validate`] withdraws the *finding* where a profile can
/// represent the data, which is the only place the distinction is load-bearing.
#[must_use]
pub fn validate(invoice: &Invoice) -> ValidationReport {
    validate_with(invoice, &rules::CORE)
}

/// A validation run with **deviations**, recorded rather than hidden.
///
/// # Why this exists, and why it cannot produce a proof
///
/// Real counterparties demand deviations. A buyer who will not send BT-10 does
/// not care that `BR-DE-15` requires it, and a supplier who must ship anyway
/// needs a way to say "check everything except that". Refusing outright pushes
/// them to fork the rule set or ignore the validator, which is worse.
///
/// So suppression is offered — and it is **loud**:
///
/// * the ids are recorded on the [`ValidationReport`] and printed by its
///   `Display`, so a stored report cannot misrepresent what was checked;
/// * [`Check::prove`] **refuses** to hand back a [`profile::Validated<P>`].
///
/// That second point is the important one, and it is the same lesson
/// `XRECHNUNG_CVD` taught at compile time: a profile that skips a core rule may
/// accept documents the core model rejects, so a proof derived from it is not a
/// proof. `Validated<P>` means *passed the whole rule set*. If it could also
/// mean *passed most of it*, no consumer could rely on it, and the type would be
/// decoration.
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
}

impl Check {
    /// Start a run against `profile`, with nothing suppressed.
    ///
    /// Takes the profile at *runtime*, which is what a CLI or a service needs:
    /// the profile comes out of BT-24 or a flag, not out of the source. Use
    /// [`Check::of`] when the profile is known at compile time and you intend to
    /// call [`prove`](Self::prove).
    #[must_use]
    pub fn new(profile: &'static profile::Profile) -> Self {
        Self {
            profile,
            suppressed: Vec::new(),
        }
    }

    /// Start a run against the profile `P` names, with nothing suppressed.
    ///
    /// The constructor to reach for when the run ends in [`prove`](Self::prove):
    /// the profile that is checked and the profile the proof claims are then the
    /// same by construction, rather than by two call sites agreeing.
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
    /// Accepts ids that resolve to nothing: a counterparty's deviation list is
    /// not this crate's to validate, and silently dropping an unknown id would
    /// make the report claim a suppression it never applied. Such an id is
    /// recorded on the report and changes nothing else — in particular it does
    /// **not** reduce [`ValidationReport::rules_checked`], which counts checks
    /// that did not run rather than requests to skip.
    #[must_use]
    pub fn without(mut self, rule: impl Into<String>) -> Self {
        self.suppressed.push(rule.into());
        self
    }

    /// Which of this profile's checks the suppressions actually remove.
    ///
    /// Not `suppressed.len()`, which counts *requests*: suppressing
    /// `BR-DE-15` against the bare core profile, or a name that resolves to no
    /// rule at all, would reduce `rules_checked` for a check that was never
    /// going to run. A count that can be wrong in the safe-looking direction is
    /// worse than no count.
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
        if self.suppressed.is_empty() {
            return report;
        }
        report
            .findings
            .retain(|f| !self.suppressed.iter().any(|s| canonical_eq(&f.rule, s)));
        report.checked = report.checked.saturating_sub(self.skipped());
        report.suppressed.clone_from(&self.suppressed);
        report
    }

    /// Validate and hand back a typed proof — **only** with nothing suppressed,
    /// and only for the profile this `Check` was built for.
    ///
    /// # The proof is of `P`, so `P` has to be the profile that ran
    ///
    /// `P` decides which rule set [`profile::Validated::new`] evaluates. If it
    /// were the only thing that did,
    /// `Check::new(&profiles::XRECHNUNG).prove::<En16931>(inv)` would announce
    /// an XRechnung run, check the bare core set and hand back a proof — in the
    /// one method whose entire job is to say *which* rule set a document
    /// passed. So the two are compared.
    ///
    /// [`Check::of`] makes the mismatch unrepresentable; this catches it for
    /// callers who came in through [`Check::new`].
    ///
    /// # Errors
    /// [`ProveError::WrongProfile`] when `P` does not name this `Check`'s
    /// profile. [`ProveError::Suppressed`] when any rule was suppressed, because
    /// the resulting document has not passed the rule set the proof would claim —
    /// use [`Check::run`] and read the report instead. [`ProveError::Rejected`]
    /// when the invoice simply does not pass.
    pub fn prove<P>(&self, invoice: Invoice) -> Result<profile::Validated<P>, ProveError>
    where
        P: profile::ProfileMarker,
    {
        // Pointer identity, not `id` equality: every profile is a distinct
        // `static`, and two profiles that happened to share a name would still be
        // two rule sets.
        if !core::ptr::eq(self.profile, P::PROFILE) {
            return Err(ProveError::WrongProfile {
                checked: self.profile.id,
                claimed: P::PROFILE.id,
            });
        }
        if !self.suppressed.is_empty() {
            return Err(ProveError::Suppressed(self.suppressed.clone()));
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
            Self::Rejected(r) => write!(f, "invoice is not valid:\n{}", r.1),
        }
    }
}

impl core::error::Error for ProveError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_ids_normalise_padding_case_and_family_aliases() {
        assert!(RuleId::new("BR-CO-03").matches("BR-CO-3"));
        assert!(RuleId::new("BR-CO-03").matches("br-co-3"));
        assert!(RuleId::new("BR-01").matches("BR-1"));
        assert!(RuleId::new("BR-S-01").matches("BR-S-1"));
        // The standard's IGIC/IPSI families are the artefacts' AF/AG.
        assert!(RuleId::new("BR-AF-01").matches("BR-IG-1"));
        assert!(RuleId::new("BR-AG-10").matches("BR-IP-10"));
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

        // …and the other direction, which silently ran the *stricter* set.
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

    /// The bare core path and `profiles::EN16931` agree on both halves of a
    /// report, including how many rules they say they ran.
    ///
    /// The `EN-EXT-01` skip exists for exactly this parity, and it changed shape
    /// when the per-call `Vec` came out of it.
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
