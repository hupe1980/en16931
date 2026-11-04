//! What happened to a document, typed and in layers.
//!
//! * An [`Entry`] is one thing a reader, the container or a writer reports
//!   about the document: an unmapped element, a malformed value, a duplicate,
//!   a dropped term. It carries a syntax path and may cite an artefact rule
//!   (`UBL-CR-646`, `CII-SR-475`); unlike a [`Finding`](en16931::Finding) it is
//!   never counted as a failed business rule.
//! * A [`Report`] has four layers (container, syntax, schema, semantic), each
//!   [`Valid`](Validity::Valid), [`Invalid`](Validity::Invalid) or
//!   [`Skipped`](Validity::Skipped), under a header identifying the document
//!   and the rule set.
//!
//! `Display` on [`Entry`] and [`Report`] is the text rendering the command-line
//! tool prints:
//!
//! ```text
//! malformed    CrossIndustryInvoice/…/ChargeIndicator/Indicator (BG-20) = "yes" — not an xs:boolean [hint: the value must be `true` or `false`]
//! ```

use core::fmt;

use en16931::{Severity, ValidationReport};

/// What an [`Entry`] is about.
///
/// The first nine come from reading, the last three from writing. Each kind has
/// one [`level`](Kind::level).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum Kind {
    /// An element the reader does not map: outside the EN 16931 subset, another
    /// CIUS's addition, or an extension the declared profile does not define.
    Unmapped,
    /// A value present and not representable in the model — `123` as a date,
    /// `yes` as a boolean, a code outside the binding's list. The term is left
    /// absent.
    Malformed,
    /// A term the model requires that the document does not carry: the
    /// element is absent or empty. The model's field cannot be absent, so it
    /// holds a placeholder (its neutral value — zero, an empty code); this
    /// entry says the placeholder is not the document's value.
    Missing,
    /// A second or later occurrence of an element the syntax allows once. The
    /// first occurrence is read; this one is not.
    Duplicate,
    /// As [`Duplicate`](Self::Duplicate), where the rule allowing it once is one
    /// CEN publishes as a warning (`CII-SR-475`: a second `ram:Name` in one
    /// BG-24). The first occurrence is read; the layer stays valid.
    Repeated,
    /// A schema-level fact that is not a duplicate: a spelling the schema does
    /// not admit and the reader accepted anyway, or (writing) a required
    /// element the model lacks.
    Schema,
    /// The bytes could not be read as an invoice at all — not well-formed, an
    /// encoding refused, nested too deep, a PDF refused.
    Unreadable,
    /// A hybrid PDF whose metadata disagrees with its payload in a way that does
    /// not change what the document declares (filename, relationship, XMP).
    Container,
    /// The document declares two different things: the PDF's XMP profile and the
    /// payload's BT-24 disagree.
    Ambiguous,
    /// A term the target document has no place for.
    Dropped,
    /// An element CEN's syntax rules fence out of the EN 16931 subset.
    Prohibited,
    /// A value the writer derived because the syntax requires one and the model
    /// had none.
    Derived,
}

impl Kind {
    /// How much an entry of this kind matters.
    ///
    /// `Fatal` kinds make their layer [`Invalid`](Validity::Invalid).
    /// `Unmapped` is information — a document may carry what this crate does not
    /// map — and never invalidates.
    #[must_use]
    pub const fn level(self) -> Severity {
        match self {
            Self::Unmapped | Self::Derived => Severity::Info,
            Self::Repeated | Self::Container | Self::Dropped | Self::Prohibited => {
                Severity::Warning
            }
            Self::Malformed
            | Self::Missing
            | Self::Duplicate
            | Self::Schema
            | Self::Unreadable
            | Self::Ambiguous => Severity::Fatal,
        }
    }

    /// The word the text rendering leads with.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unmapped => "unmapped",
            Self::Malformed => "malformed",
            Self::Missing => "missing",
            Self::Duplicate => "duplicate",
            Self::Repeated => "repeated",
            Self::Schema => "schema",
            Self::Unreadable => "unreadable",
            Self::Container => "container",
            Self::Ambiguous => "ambiguous",
            Self::Dropped => "dropped",
            Self::Prohibited => "prohibited",
            Self::Derived => "derived",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        en16931::fmt::padded(f, self.as_str())
    }
}

/// One thing a reader, the container or a writer has to say about a document.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Entry {
    /// What kind of thing it is.
    pub kind: Kind,
    /// Where, as a syntax path: `Invoice/cac:Delivery`, `TradeParty/Foo`.
    /// A reader's path is the chain of local names it was walking.
    pub path: String,
    /// The business term or group, when the binding knows it — `BT-8`,
    /// `BG-25[2]/BT-147`.
    pub term: Option<String>,
    /// The value as the document carries it, when there is one.
    pub value: Option<String>,
    /// The artefact rule this entry cites, when one exists — `UBL-CR-646`,
    /// `CII-SR-475`. Never a registry rule id: an entry is not a finding.
    pub rule: Option<String>,
    /// The binding's statement of what happened and why.
    pub message: String,
    /// What to do about it, where this crate knows. Advice; it never changes a
    /// verdict.
    pub hint: Option<String>,
}

impl Entry {
    /// An entry with only a kind, a path and a statement.
    #[must_use]
    pub fn new(kind: Kind, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind,
            path: path.into(),
            term: None,
            value: None,
            rule: None,
            message: message.into(),
            hint: None,
        }
    }

    /// With the business term.
    #[must_use]
    pub fn term(mut self, term: impl Into<String>) -> Self {
        self.term = Some(term.into());
        self
    }

    /// With the value as the document carries it.
    #[must_use]
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// With the artefact rule it cites.
    #[must_use]
    pub fn rule(mut self, rule: impl Into<String>) -> Self {
        self.rule = Some(rule.into());
        self
    }

    /// With a hint.
    #[must_use]
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// How much it matters — [`Kind::level`].
    #[must_use]
    pub fn level(&self) -> Severity {
        self.kind.level()
    }

    /// Whether this entry means document content did not reach the other side
    /// of a conversion: an element not mapped, a value not held, a required
    /// term the source lacks (its placeholder would be written as if it were
    /// the document's), a duplicate not read, a term the target cannot carry
    /// or may not carry.
    #[must_use]
    pub fn is_loss(&self) -> bool {
        matches!(
            self.kind,
            Kind::Unmapped
                | Kind::Malformed
                | Kind::Missing
                | Kind::Duplicate
                | Kind::Repeated
                | Kind::Dropped
                | Kind::Prohibited
        )
    }
}

impl fmt::Display for Entry {
    /// `kind         path (term) = "value" — message [rule] [hint: …]`
    ///
    /// The kind is padded to a column; optional parts appear only when present.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:<12} {}", self.kind, self.path)?;
        if let Some(t) = &self.term {
            write!(f, " ({t})")?;
        }
        if let Some(v) = &self.value {
            write!(f, " = {v:?}")?;
        }
        write!(f, " — {}", self.message)?;
        if let Some(r) = &self.rule {
            write!(f, " [{r}]")?;
        }
        if let Some(h) = &self.hint {
            write!(f, " [hint: {h}]")?;
        }
        Ok(())
    }
}

/// Whether a layer passed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum Validity {
    /// Nothing in the layer is fatal.
    Valid,
    /// At least one entry or finding in the layer is fatal.
    Invalid,
    /// The layer did not run, and why — a layer below it failed, or there is
    /// nothing for it to check.
    Skipped(String),
}

impl Validity {
    /// `Invalid` when any of `levels` is fatal, `Valid` otherwise.
    fn of(mut levels: impl Iterator<Item = Severity>) -> Self {
        if levels.any(|l| l == Severity::Fatal) {
            Self::Invalid
        } else {
            Self::Valid
        }
    }
}

impl fmt::Display for Validity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Valid => f.write_str("valid"),
            Self::Invalid => f.write_str("INVALID"),
            Self::Skipped(why) => write!(f, "skipped ({why})"),
        }
    }
}

/// One of the container, syntax and schema layers.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Layer {
    /// Whether it passed.
    pub validity: Validity,
    /// What it has to say, in document order.
    pub entries: Vec<Entry>,
}

impl Layer {
    /// A layer whose validity follows from its entries.
    #[must_use]
    pub fn of(entries: Vec<Entry>) -> Self {
        Self {
            validity: Validity::of(entries.iter().map(Entry::level)),
            entries,
        }
    }

    /// A layer that did not run.
    #[must_use]
    pub fn skipped(why: impl Into<String>) -> Self {
        Self {
            validity: Validity::Skipped(why.into()),
            entries: Vec::new(),
        }
    }
}

/// The semantic layer: the business rules of a profile.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Semantic {
    /// Whether it passed.
    pub validity: Validity,
    /// The validation report, when the rules ran.
    pub report: Option<ValidationReport>,
}

/// Which document a report is about — what a person looks for first.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Identity {
    /// BT-27, the seller's name.
    pub seller: Option<String>,
    /// BT-1, the invoice number.
    pub number: Option<String>,
    /// BT-2, the issue date, `YYYY-MM-DD`.
    pub issue_date: Option<String>,
}

impl Identity {
    /// The three terms, read off a model.
    #[must_use]
    pub fn of(invoice: &en16931::Invoice) -> Self {
        Self {
            seller: invoice.seller.name.clone(),
            number: invoice.number.clone(),
            issue_date: invoice.issue_date.map(|d| d.to_string()),
        }
    }
}

/// The rule set a report was checked against.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct RuleSet {
    /// The profile's id — `XRechnung 3.0`.
    pub profile: String,
    /// Its BT-24 identifier.
    pub specification_id: String,
    /// The authority releases its rules were verified against —
    /// `CEN validation-1.3.17`.
    pub artefacts: Vec<String>,
    /// Core rules the profile does not run: non-empty only for a totals-only
    /// profile (ZUGFeRD MINIMUM, BASIC WL), so "no fatal finding" is never read
    /// as "every rule passed".
    pub left_out: Vec<String>,
}

impl RuleSet {
    /// The rule set of a profile.
    #[must_use]
    pub fn of(p: &en16931::Profile) -> Self {
        Self {
            profile: p.id.to_owned(),
            specification_id: p.specification_id.to_owned(),
            artefacts: p.artefacts.iter().map(ToString::to_string).collect(),
            left_out: p.left_out().into_iter().map(str::to_owned).collect(),
        }
    }
}

/// A received document, in four layers.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Report {
    /// Which document — seller, number, date.
    pub identity: Identity,
    /// Which rule set the semantic layer ran, when one did.
    pub rule_set: Option<RuleSet>,
    /// The PDF around the payload, for a hybrid document.
    pub container: Layer,
    /// Reading the syntax: unreadable bytes, unmapped elements, malformed values.
    pub syntax: Layer,
    /// What this crate checks of the schema: single-occurrence elements and
    /// spellings the schema does not admit. Not full XSD validation.
    pub schema: Layer,
    /// The business rules.
    pub semantic: Semantic,
}

impl Report {
    /// The most severe level of anything in the report — entries and findings —
    /// or `None` for a report with nothing in it.
    #[must_use]
    pub fn most_severe(&self) -> Option<Severity> {
        let entries = self.entries().map(Entry::level);
        let findings = self
            .semantic
            .report
            .iter()
            .flat_map(|r| r.findings().iter().map(|f| f.severity));
        // `Severity` orders most severe first.
        entries.chain(findings).min()
    }

    /// Whether no layer is invalid.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        [
            &self.container.validity,
            &self.syntax.validity,
            &self.schema.validity,
            &self.semantic.validity,
        ]
        .iter()
        .all(|v| **v != Validity::Invalid)
    }

    /// Every entry of the three document layers, container first.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.container
            .entries
            .iter()
            .chain(&self.syntax.entries)
            .chain(&self.schema.entries)
    }
}

impl fmt::Display for Report {
    /// The header — document, rule set, most severe level — then each layer
    /// with its validity and what it holds, then the findings.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let or_absent = |v: &Option<String>| v.clone().unwrap_or_else(|| "(absent)".to_owned());
        writeln!(
            f,
            "document: {} — invoice {} of {}",
            or_absent(&self.identity.seller),
            or_absent(&self.identity.number),
            or_absent(&self.identity.issue_date)
        )?;
        match &self.rule_set {
            Some(rs) => {
                write!(f, "rule set: {} ({})", rs.profile, rs.artefacts.join(", "))?;
                if !rs.left_out.is_empty() {
                    write!(
                        f,
                        " — totals only; {} core rule(s) not run: {}",
                        rs.left_out.len(),
                        rs.left_out.join(", ")
                    )?;
                }
                writeln!(f)?;
            }
            None => writeln!(f, "rule set: none ran")?,
        }
        match self.most_severe() {
            Some(level) => writeln!(f, "most severe: {level}")?,
            None => writeln!(f, "most severe: nothing to report")?,
        }
        for (name, layer) in [
            ("container", &self.container),
            ("syntax", &self.syntax),
            ("schema", &self.schema),
        ] {
            writeln!(f, "{name:<10} {}", layer.validity)?;
            for e in &layer.entries {
                writeln!(f, "  {e}")?;
            }
        }
        write!(f, "{:<10} {}", "semantic", self.semantic.validity)?;
        if let Some(r) = &self.semantic.report {
            for line in r.to_string().lines() {
                write!(f, "\n  {line}")?;
            }
        }
        Ok(())
    }
}

// ── The interchange shape ────────────────────────────────────────────────────

impl Entry {
    /// This entry in `en16931`'s syntax-neutral interchange shape.
    #[must_use]
    pub fn to_interchange(&self) -> en16931::report::DocumentEntry {
        en16931::report::DocumentEntry {
            kind: self.kind.as_str().to_owned(),
            level: self.level_name().into(),
            path: self.path.clone(),
            term: self.term.clone(),
            value: self.value.clone(),
            rule: self.rule.clone(),
            message: self.message.clone(),
            hint: self.hint.clone(),
        }
    }

    const fn level_name(&self) -> &'static str {
        en16931::report::severity_name(self.kind.level())
    }
}

impl Validity {
    /// `valid`, `invalid` or `skipped`, and the reason for `skipped`.
    fn to_interchange(&self) -> (&'static str, Option<String>) {
        match self {
            Self::Valid => ("valid", None),
            Self::Invalid => ("invalid", None),
            Self::Skipped(why) => ("skipped", Some(why.clone())),
        }
    }
}

/// One named layer in the interchange shape.
pub(crate) fn interchange_layer(
    name: &'static str,
    validity: &Validity,
    entries: &[Entry],
) -> en16931::report::Layer {
    let (word, reason) = validity.to_interchange();
    en16931::report::Layer {
        name: name.into(),
        validity: word.into(),
        reason,
        entries: entries.iter().map(Entry::to_interchange).collect(),
    }
}

impl Report {
    /// This report in `en16931`'s interchange shape
    /// ([`en16931::report::SCHEMA`]): the findings as
    /// [`en16931::report::Report::of`] gives them, and beside them the four
    /// layers, the document's identity and the rules left out.
    #[must_use]
    pub fn to_interchange(&self) -> en16931::report::Report {
        let mut out = self.semantic.report.as_ref().map_or_else(
            en16931::report::Report::unchecked,
            en16931::report::Report::of,
        );
        out.valid = self.is_valid();
        out.identity = Some(en16931::report::Identity {
            seller: self.identity.seller.clone(),
            number: self.identity.number.clone(),
            issue_date: self.identity.issue_date.clone(),
        });
        if let Some(rs) = &self.rule_set {
            out.profile.get_or_insert_with(|| rs.profile.clone());
            out.left_out.clone_from(&rs.left_out);
        }
        out.layers = vec![
            interchange_layer(
                "container",
                &self.container.validity,
                &self.container.entries,
            ),
            interchange_layer("syntax", &self.syntax.validity, &self.syntax.entries),
            interchange_layer("schema", &self.schema.validity, &self.schema.entries),
            interchange_layer("semantic", &self.semantic.validity, &[]),
        ];
        out
    }
}

/// Build a [`Semantic`] layer from a validation report.
#[cfg(any(feature = "ubl", feature = "cii"))]
pub(crate) fn semantic(report: ValidationReport) -> Semantic {
    Semantic {
        validity: if report.is_valid() {
            Validity::Valid
        } else {
            Validity::Invalid
        },
        report: Some(report),
    }
}
