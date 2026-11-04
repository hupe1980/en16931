//! Profiles and CIUSes — restrictions as data, rules derived from them.
//!
//! EN 16931-1 §7.3.2 lists what a CIUS may change; most of it is narrowing the
//! model, not adding rules. A [`Restriction`] expresses such a narrowing as
//! data, and its finding is derived; conditional rules go in
//! [`Profile::extra_rules`]. XRechnung, for example, has ten `Mandatory` and two
//! `CodeValues` restrictions.
//!
//! A restriction cannot loosen the core model (§4.4.2), but [`Profile::levels`]
//! can report a core rule at a lower severity, and such a profile is not a
//! conformant CIUS ([`Profile::is_conformant_cius`]). [`Validated::widen`]
//! exists only where the profile is conformant (§4.4.4).

use core::fmt;
use core::marker::PhantomData;

use crate::bt::{BtId, Path};
use crate::invoice::Invoice;
use crate::validation::{Finding, Rule, Severity, ValidationReport, validate_with_all};

/// What a [`TermAccessor`] returns: one `(where, value)` per occurrence.
///
/// `None` means the term is absent at that place.
pub type Occurrences = Vec<(Path, Option<String>)>;

/// What [`Validated::new`] hands back when the invoice does not pass: the
/// invoice and its report. Displays as the report, so `?` into
/// `Box<dyn Error>` prints the findings.
///
/// ```
/// use en16931::{En16931, Invoice, Validated};
///
/// fn prove(invoice: Invoice) -> Result<Validated<En16931>, Box<dyn std::error::Error>> {
///     Ok(Validated::new(invoice)?)
/// }
///
/// let err = prove(Invoice::default()).unwrap_err();
/// assert!(err.to_string().contains("INVALID"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    // Boxed: the pair is large and travels in the `Err` arm.
    parts: Box<(Invoice, ValidationReport)>,
}

impl Rejected {
    /// The invoice that did not pass.
    #[must_use]
    pub fn invoice(&self) -> &Invoice {
        &self.parts.0
    }

    /// Why it did not pass.
    #[must_use]
    pub fn report(&self) -> &ValidationReport {
        &self.parts.1
    }

    /// The invoice and the report.
    #[must_use]
    pub fn into_parts(self) -> (Invoice, ValidationReport) {
        *self.parts
    }
}

impl fmt::Display for Rejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.report(), f)
    }
}

impl core::error::Error for Rejected {}

// ── Term accessors ────────────────────────────────────────────────────────────

/// Reads one business term out of an invoice, wherever it occurs.
///
/// Returns one entry per place the term can appear (one per line for a
/// line-level term); the [`Path`] of a derived finding comes from here.
pub struct TermAccessor {
    /// Which term, or [`BtId(0)`](BtId) for a whole group.
    pub term: BtId,
    /// Its name in the standard, for messages.
    pub name: &'static str,
    /// `(where, value)` for every occurrence. `None` means absent.
    pub read: fn(&Invoice) -> Occurrences,
}

impl TermAccessor {
    /// How this reads in a finding — `"Seller city (BT-37)"`. A group accessor
    /// (`BtId(0)`) shows its name only; the group id is already in it.
    #[must_use]
    pub fn label(&self) -> String {
        if self.term == BtId(0) {
            self.name.to_owned()
        } else {
            format!("{} ({})", self.name, self.term)
        }
    }
}

impl fmt::Debug for TermAccessor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.term, self.name)
    }
}

/// Accessors for the terms the shipped profiles restrict (not every term).
pub mod terms {
    use super::TermAccessor;
    use crate::bt::{BtId, Group, Path};
    use crate::invoice::terms as bt;

    /// One document-level term, read through a closure.
    macro_rules! doc_term {
        ($konst:ident, $bt:expr, $name:literal, $group:expr, |$inv:ident| $get:expr) => {
            #[doc = concat!($name, ".")]
            pub static $konst: TermAccessor = TermAccessor {
                term: $bt,
                name: $name,
                read: |$inv: &crate::invoice::Invoice| vec![(Path::group_term($group, $bt), $get)],
            };
        };
    }

    doc_term!(
        BUYER_REFERENCE,
        bt::BUYER_REFERENCE,
        "Buyer reference",
        Group::Document,
        |inv| inv.buyer_reference.clone()
    );
    doc_term!(SELLER_CITY, BtId(37), "Seller city", Group::Seller, |inv| {
        inv.seller.address.city.clone()
    });
    doc_term!(
        SELLER_POST_CODE,
        BtId(38),
        "Seller post code",
        Group::Seller,
        |inv| inv.seller.address.post_code.clone()
    );
    doc_term!(
        SELLER_CONTACT_POINT,
        BtId(41),
        "Seller contact point",
        Group::Seller,
        |inv| inv.seller.contact.name.clone()
    );
    doc_term!(
        SELLER_CONTACT_PHONE,
        BtId(42),
        "Seller contact telephone number",
        Group::Seller,
        |inv| inv.seller.contact.phone.clone()
    );
    doc_term!(
        SELLER_CONTACT_EMAIL,
        BtId(43),
        "Seller contact email address",
        Group::Seller,
        |inv| inv.seller.contact.email.clone()
    );
    doc_term!(BUYER_CITY, BtId(52), "Buyer city", Group::Buyer, |inv| inv
        .buyer
        .address
        .city
        .clone());
    doc_term!(
        BUYER_POST_CODE,
        BtId(53),
        "Buyer post code",
        Group::Buyer,
        |inv| inv.buyer.address.post_code.clone()
    );
    doc_term!(
        TYPE_CODE,
        bt::TYPE_CODE,
        "Invoice type code",
        Group::Document,
        |inv| inv.type_code.as_ref().map(|c| c.as_str().to_owned())
    );
    doc_term!(
        SPECIFICATION_ID,
        bt::SPECIFICATION_ID,
        "Specification identifier",
        Group::Document,
        |inv| inv.specification_id.clone()
    );
    doc_term!(
        PAYMENT_MEANS_CODE,
        bt::PAYMENT_MEANS_CODE,
        "Payment means type code",
        Group::Payment,
        |inv| inv
            .payment
            .as_ref()
            .and_then(|p| p.means_code.as_ref())
            .map(|c| c.as_str().to_owned())
    );

    /// BT-119, once per VAT breakdown group. `BR-DE-14` requires it on every
    /// BG-23, including the `O` group that `BR-48` exempts.
    pub static VAT_RATE: TermAccessor = TermAccessor {
        term: bt::VAT_RATE,
        name: "VAT category rate",
        read: |inv| {
            inv.vat_breakdown
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    (
                        Path::at_term(Group::VatBreakdown, i, bt::VAT_RATE),
                        e.rate.map(|r| r.to_string()),
                    )
                })
                .collect()
        },
    };

    /// BG-16 as a whole, for the profiles that require payment instructions.
    pub static PAYMENT_INSTRUCTIONS: TermAccessor = TermAccessor {
        term: BtId(0),
        name: "PAYMENT INSTRUCTIONS (BG-16)",
        read: |inv| {
            vec![(
                Path::group(Group::Payment),
                inv.payment.as_ref().map(|_| "present".to_owned()),
            )]
        },
    };
}

// ── Restriction ───────────────────────────────────────────────────────────────

/// One of §7.3.2's allowed narrowings: it can only reject documents the core
/// model accepts, never the reverse. Failing one is fatal unless
/// [`Profile::levels`] says otherwise.
#[derive(Debug)]
pub enum Restriction {
    /// Cardinality `0..x → 1..x` — a conditional element becomes mandatory
    /// (e.g. `BR-DE-1`, `BR-DE-3` … `BR-DE-9`, `BR-DE-14`, `BR-DE-15`).
    Mandatory {
        /// The rule id the profile publishes for it.
        id: &'static str,
        /// Which term.
        term: &'static TermAccessor,
    },
    /// Cardinality `0..x → 0..0` — a conditional element must not be used.
    NotUsed {
        /// The rule id the profile publishes for it.
        id: &'static str,
        /// Which term.
        term: &'static TermAccessor,
    },
    /// *"Mark defined values as not allowed"* — restrict a code list (e.g.
    /// `BR-DE-17` on BT-3). An absent or blank value fails it, as the
    /// authorities' general comparisons do.
    CodeValues {
        /// The rule id the profile publishes for it.
        id: &'static str,
        /// Which term.
        term: &'static TermAccessor,
        /// The values that remain admissible.
        allowed: &'static [&'static str],
    },
}

impl Restriction {
    /// The rule id this restriction is published under.
    #[must_use]
    pub const fn id(&self) -> &'static str {
        match self {
            Self::Mandatory { id, .. } | Self::NotUsed { id, .. } | Self::CodeValues { id, .. } => {
                id
            }
        }
    }

    /// The term it narrows.
    #[must_use]
    pub const fn term(&self) -> &'static TermAccessor {
        match self {
            Self::Mandatory { term, .. }
            | Self::NotUsed { term, .. }
            | Self::CodeValues { term, .. } => term,
        }
    }

    /// Evaluate it, producing findings with paths taken from the accessor.
    fn check(&self, inv: &Invoice, out: &mut Vec<Finding>) {
        let acc = self.term();
        for (path, value) in (acc.read)(inv) {
            let (ok, why, hint) = match self {
                Self::Mandatory { .. } => (
                    value.as_deref().is_some_and(|v| !v.trim().is_empty()),
                    format!("{} shall be present", acc.label()),
                    None,
                ),
                Self::NotUsed { .. } => (
                    // Blank counts as absent, as in `Mandatory`.
                    value.as_deref().is_none_or(|v| v.trim().is_empty()),
                    format!("{} shall not be used", acc.label()),
                    None,
                ),
                Self::CodeValues { allowed, .. } => {
                    // KoSIT's asserts are general comparisons, false on an
                    // empty sequence, so `BR-DE-21` fires on a missing BT-24.
                    let v = value.as_deref().filter(|v| !v.trim().is_empty());
                    (
                        v.is_some_and(|v| allowed.contains(&v)),
                        format!("{} shall be one of: {}", acc.label(), allowed.join(", ")),
                        v.and_then(|v| case_or_scope_hint(v, allowed)),
                    )
                }
            };
            if !ok {
                out.push(Finding {
                    rule: self.id().to_owned(),
                    severity: Severity::Fatal,
                    path,
                    message: why,
                    detail: None,
                    hint,
                });
            }
        }
    }
}

/// Why a value failed a [`Restriction::CodeValues`], when the crate can tell:
/// a case typo (`"s"` for `"S"`), or a lawful core code this profile does not
/// admit (`BT-3 = 386` under XRechnung).
fn case_or_scope_hint(value: &str, allowed: &[&'static str]) -> Option<String> {
    if let Some(c) = allowed.iter().find(|c| c.eq_ignore_ascii_case(value)) {
        return Some(format!(
            "did you mean {c:?}? EN 16931-1 §6.5.8 requires codes \"entered exactly as shown\""
        ));
    }
    let core = crate::codes::guard::ALL
        .iter()
        .find(|l| l.accepts(value) && l.values.len() > allowed.len())?;
    Some(format!(
        "{value:?} is a valid {} value but this profile does not admit it — \
         a CIUS may narrow a code list (§7.3.2), so this is a scope question, not a typo",
        core.name
    ))
}

// ── Profile ───────────────────────────────────────────────────────────────────

/// One authority release a [`Profile`] was verified against
/// (`git clone <repo> --branch <git_ref>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ArtefactRef {
    /// The authority, as it calls itself — `"CEN"`, `"KoSIT"`, `"OpenPeppol"`.
    pub authority: &'static str,
    /// The repository the artefact lives in, without the host.
    pub repo: &'static str,
    /// The release tag fetched, verbatim.
    pub git_ref: &'static str,
}

impl fmt::Display for ArtefactRef {
    /// Authority and tag, without the repository.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.authority, self.git_ref)
    }
}

/// Which of the core rules a [`Profile`] runs.
///
/// Every EN 16931 profile runs all of them. ZUGFeRD MINIMUM and BASIC WL are
/// not EN 16931 invoices (no lines), so they name the rules that apply and
/// [`Profile::left_out`] names the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Core {
    /// Every core rule — every EN 16931 profile.
    All,
    /// Only these core rules, by id.
    Only(&'static [&'static str]),
}

impl Core {
    /// Whether a core rule with this id runs.
    /// Readers ask it before reporting a term missing: ZUGFeRD MINIMUM does
    /// not run `BR-12`, so its lawful lack of BT-106 is not reported.
    #[must_use]
    pub fn runs(self, id: &str) -> bool {
        match self {
            Self::All => true,
            Self::Only(ids) => ids.contains(&id),
        }
    }
}

/// A core invoice usage specification, carrying what §4.4.2 requires a CIUS to
/// state: its identifier, its BT-24 value and its underlying specifications.
#[derive(Debug)]
pub struct Profile {
    /// Short name, for reports.
    pub id: &'static str,
    /// The name for a command line or config file (`--profile xrechnung`).
    ///
    /// Lower-case ASCII, hyphen-separated, unique across
    /// [`crate::profiles::ALL`], and stable.
    pub slug: &'static str,
    /// Other accepted spellings — `core` for EN 16931, `bis3` for Peppol BIS
    /// Billing 3.0. Parsed case-insensitively like the slug, and unique.
    pub aliases: &'static [&'static str],
    /// The BT-24 value a document declaring this profile carries (§7.6); used
    /// by [`crate::profiles::classify`].
    pub specification_id: &'static str,
    /// Which edition of EN 16931-1 this profile is a usage specification of.
    pub edition: crate::Edition,
    /// §4.4.2: *"shall state its underlying specifications"*.
    pub underlying: &'static [&'static str],
    /// The authority releases this profile's hand-written rules were verified
    /// against (CEN for the core rules, KoSIT or OpenPeppol for their own).
    pub artefacts: &'static [ArtefactRef],
    /// §7.3.2's narrowings, as data.
    pub restrictions: &'static [Restriction],
    /// Conditional rules beyond the core set.
    pub extra_rules: &'static [&'static Rule],
    /// Which core rules run: [`Core::All`] for every EN 16931 profile.
    pub core: Core,
    /// Extension groups this profile can represent. `EN-EXT-01` warns about
    /// extension data outside this list.
    pub extensions: &'static [&'static str],
    /// Rules this profile reports at a severity other than the rule's own,
    /// transcribed from the authority's configuration (KoSIT's `customLevel`)
    /// or, for a [`Restriction`], its Schematron `flag`.
    ///
    /// The rule still runs; only its severity changes. A downward re-level
    /// makes the profile not a conformant CIUS — see
    /// [`is_conformant_cius`](Self::is_conformant_cius). Every id must name a
    /// rule the profile runs.
    pub levels: &'static [(&'static str, Severity)],
}

impl Profile {
    /// Validate `invoice` against the core rules plus this profile;
    /// `rules_checked()` is [`check_ids`](Self::check_ids)`().count()`.
    #[must_use]
    pub fn validate(&self, invoice: &Invoice) -> ValidationReport {
        let core = self.core;
        let mut report = validate_with_all(
            invoice,
            super::rules::CORE
                .iter()
                .copied()
                .filter(move |r| core.runs(r.id.as_str())),
            self.extra_rules,
        );

        // `EN-EXT-01` depends on the target: withdraw its finding (not the
        // rule, so the count stays fixed) when this profile can hold every
        // extension group the invoice carries.
        if invoice
            .extensions
            .populated()
            .iter()
            .all(|g| self.extensions.contains(g))
        {
            report.findings.retain(|f| f.rule != "EN-EXT-01");
        }

        let mut findings = Vec::new();
        for restriction in self.restrictions {
            restriction.check(invoice, &mut findings);
        }
        report.absorb(findings, self.restrictions.len());
        report.relevel(self.levels);
        report.attribute_to(self.id, self.edition, self.artefacts);
        report
    }

    /// The severity this profile reports `rule` (or restriction) at, after
    /// [`levels`](Self::levels); `None` if the profile runs no such check.
    ///
    /// Looks in this profile's own [`extra_rules`](Self::extra_rules) before
    /// the core set, because one id can name two instances with different
    /// severities (`PEPPOL-EN16931-R120`).
    ///
    /// ```
    /// use en16931::{Severity, profiles};
    ///
    /// let id = "PEPPOL-EN16931-R120";
    /// assert_eq!(profiles::PEPPOL_BIS_3.severity_of(id), Some(Severity::Fatal));
    /// assert_eq!(profiles::XRECHNUNG.severity_of(id), Some(Severity::Warning));
    /// assert_eq!(profiles::EN16931.severity_of(id), None);   // core runs neither
    /// ```
    #[must_use]
    pub fn severity_of(&self, rule: &str) -> Option<Severity> {
        let id = crate::validation::RuleId::new;
        let declared = || {
            self.extra_rules
                .iter()
                .find(|r| r.id.matches(rule))
                .or_else(|| super::rules::CORE.iter().find(|r| r.id.matches(rule)))
                .map(|r| r.severity)
                // A restriction is fatal unless `levels` says otherwise.
                .or_else(|| {
                    self.restrictions
                        .iter()
                        .any(|r| id(r.id()).matches(rule))
                        .then_some(Severity::Fatal)
                })
        };
        // Overrides apply only to checks this profile runs.
        let declared = declared()?;
        Some(
            self.levels
                .iter()
                .find(|(over, _)| id(over).matches(rule))
                .map_or(declared, |(_, level)| *level),
        )
    }

    /// The id of every check this profile evaluates — core rules,
    /// [`extra_rules`](Self::extra_rules) and
    /// [`restrictions`](Self::restrictions) — in evaluation order, as the
    /// authority spells them. Re-levelled rules are included.
    pub fn check_ids(&self) -> impl Iterator<Item = &'static str> {
        let core = self.core;
        super::rules::CORE
            .iter()
            .map(|r| r.id.as_str())
            .filter(move |id| core.runs(id))
            .chain(self.extra_rules.iter().map(|r| r.id.as_str()))
            .chain(self.restrictions.iter().map(Restriction::id))
    }

    /// Whether a document declaring this profile is an EN 16931 invoice.
    /// False exactly for a [`Core::Only`] selection (ZUGFeRD MINIMUM, BASIC WL).
    #[must_use]
    pub fn is_en16931_invoice(&self) -> bool {
        matches!(self.core, Core::All)
    }

    /// The core rules this profile does not run, in registry order. Empty for
    /// every EN 16931 profile.
    #[must_use]
    pub fn left_out(&self) -> Vec<&'static str> {
        super::rules::CORE
            .iter()
            .map(|r| r.id.as_str())
            .filter(|id| !self.core.runs(id))
            .collect()
    }

    /// Whether this profile is a conformant CIUS under §4.4.2: true unless
    /// [`levels`](Self::levels) relaxes a core rule to a lower severity.
    ///
    /// False for the three XRechnung profiles: all relax `BR-CL-21` and
    /// `BR-CL-23`, CVD also `BR-CL-13`, and the Extension also `BR-CO-16` and
    /// further `BR-CL-*` rules. Matches the [`Underlies`] impls.
    #[must_use]
    pub fn is_conformant_cius(&self) -> bool {
        self.levels.iter().all(|(id, level)| {
            // Most-severe-first ordering: "not relaxed" is `<=`.
            super::rules::explain(id).is_none_or(|r| *level <= r.severity)
        })
    }

    /// The terms this profile's `Mandatory` restrictions require that
    /// `invoice` does not yet carry — a pre-flight for a document still being
    /// assembled.
    ///
    /// Conditional [`extra_rules`](Self::extra_rules) are not covered;
    /// [`validate`](Self::validate) remains the complete check.
    ///
    /// ```
    /// use en16931::{Invoice, profiles};
    ///
    /// let missing = profiles::XRECHNUNG.missing_terms(&Invoice::default());
    /// let ids: Vec<_> = missing.iter().map(|m| m.rule).collect();
    /// assert!(ids.contains(&"BR-DE-3"), "Seller city (BT-37)");
    /// assert!(ids.contains(&"BR-DE-15"), "Buyer reference (BT-10)");
    /// ```
    #[must_use]
    pub fn missing_terms(&self, invoice: &Invoice) -> Vec<Missing> {
        let mut out = Vec::new();
        for r in self.restrictions {
            let Restriction::Mandatory { id, term } = r else {
                continue;
            };
            for (path, value) in (term.read)(invoice) {
                if value.as_deref().is_none_or(|v| v.trim().is_empty()) {
                    out.push(Missing {
                        term: term.term,
                        name: term.name,
                        rule: id,
                        path,
                    });
                }
            }
        }
        out
    }
}

/// A term a profile requires and a document does not yet carry.
///
/// Produced by [`Profile::missing_terms`] and [`crate::invoice::Party::missing_for`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Missing {
    /// Which business term. [`BtId(0)`](BtId) for a whole group.
    pub term: BtId,
    /// Its name in the standard, ready to put in front of a user.
    pub name: &'static str,
    /// The rule the profile publishes it under — `"BR-DE-3"`.
    pub rule: &'static str,
    /// Where it belongs — `BG-4/BT-37`.
    pub path: Path,
}

impl fmt::Display for Missing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {} — {}", self.rule, self.path, self.name)
    }
}

// ── The proof ─────────────────────────────────────────────────────────────────

/// A profile, at the type level.
pub trait ProfileMarker {
    /// The profile this marker stands for.
    const PROFILE: &'static Profile;
}

/// An invoice that has passed validation against `P`, so an API can demand
/// e.g. `Validated<XRechnung>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validated<P: ProfileMarker> {
    invoice: Invoice,
    _profile: PhantomData<P>,
}

impl<P: ProfileMarker> Validated<P> {
    /// Validate, or hand the invoice back with the report.
    ///
    /// # Errors
    /// The invoice and its report, when any fatal finding was raised.
    pub fn new(invoice: Invoice) -> Result<Self, Rejected> {
        let report = P::PROFILE.validate(&invoice);
        if report.is_valid() {
            Ok(Self {
                invoice,
                _profile: PhantomData,
            })
        } else {
            Err(Rejected {
                parts: Box::new((invoice, report)),
            })
        }
    }

    /// The invoice.
    #[must_use]
    pub fn invoice(&self) -> &Invoice {
        &self.invoice
    }

    /// Give the invoice back.
    #[must_use]
    pub fn into_inner(self) -> Invoice {
        self.invoice
    }

    /// Re-badge as validated against a less restrictive profile, without
    /// re-validating (§4.4.4). Only available where [`Underlies`] holds.
    #[must_use]
    pub fn widen<Q>(self) -> Validated<Q>
    where
        Q: Underlies<P>,
    {
        Validated {
            invoice: self.invoice,
            _profile: PhantomData,
        }
    }
}

/// `Q: Underlies<P>` means every document valid under `P` is valid under `Q`.
/// Implemented only where that holds.
pub trait Underlies<P: ProfileMarker>: ProfileMarker {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::{En16931, PeppolBis3, XRechnung};

    #[test]
    fn a_cius_states_what_4_4_2_requires() {
        let p = XRechnung::PROFILE;
        assert!(!p.specification_id.is_empty(), "§7.6: BT-24 identifier");
        assert!(
            !p.underlying.is_empty(),
            "§4.4.2: underlying specifications"
        );
    }

    #[test]
    fn restrictions_are_data_not_code() {
        let p = XRechnung::PROFILE;
        assert!(
            p.restrictions.len() >= 10,
            "XRechnung has ten Mandatory and two CodeValues restrictions"
        );
        let ids: Vec<_> = p.restrictions.iter().map(Restriction::id).collect();
        for expect in ["BR-DE-3", "BR-DE-15", "BR-DE-14", "BR-DE-17"] {
            assert!(ids.contains(&expect), "{expect} missing from {ids:?}");
        }
    }

    /// §4.4.4 — a conformant CIUS proof widens to core (Peppol BIS 3; not
    /// XRechnung, which relaxes core rules).
    #[test]
    fn core_is_reachable_from_a_conformant_cius_proof() {
        fn takes_core(_: Validated<En16931>) {}
        fn round_trip(v: Validated<PeppolBis3>) {
            takes_core(v.widen());
        }
        let _ = round_trip; // compile-time assertion
    }
}

#[cfg(test)]
mod restriction_tests {
    use super::*;
    use crate::invoice::Code;

    /// A profile exercising every [`Restriction`] variant (no shipped profile
    /// uses `NotUsed`).
    static EVERY_VARIANT: Profile = Profile {
        id: "test",
        slug: "test",
        aliases: &[],
        edition: crate::Edition::En2017A1,
        specification_id: "urn:test",
        artefacts: &[],
        underlying: &["EN 16931"],
        restrictions: &[
            Restriction::Mandatory {
                id: "T-MANDATORY",
                term: &terms::BUYER_REFERENCE,
            },
            Restriction::NotUsed {
                id: "T-NOTUSED",
                term: &terms::SELLER_CITY,
            },
            Restriction::CodeValues {
                id: "T-CODES",
                term: &terms::TYPE_CODE,
                allowed: &["380"],
            },
        ],
        extra_rules: &[],
        core: Core::All,
        extensions: &[],
        levels: &[],
    };

    fn subject() -> Invoice {
        Invoice {
            type_code: Some(Code::new("380")),
            buyer_reference: Some("REF-1".to_owned()),
            ..Default::default()
        }
    }

    #[test]
    fn mandatory_fires_only_when_the_term_is_absent_or_blank() {
        let mut inv = subject();
        assert!(!EVERY_VARIANT.validate(&inv).has("T-MANDATORY"));

        inv.buyer_reference = None;
        assert!(EVERY_VARIANT.validate(&inv).has("T-MANDATORY"));

        inv.buyer_reference = Some("   ".to_owned());
        assert!(
            EVERY_VARIANT.validate(&inv).has("T-MANDATORY"),
            "whitespace is not a value"
        );
    }

    #[test]
    fn not_used_fires_only_when_the_term_carries_something() {
        let mut inv = subject();
        assert!(!EVERY_VARIANT.validate(&inv).has("T-NOTUSED"));

        inv.seller.address.city = Some("Berlin".to_owned());
        assert!(EVERY_VARIANT.validate(&inv).has("T-NOTUSED"));

        // Blank reads as absent here too, as in `Mandatory`.
        inv.seller.address.city = Some("  ".to_owned());
        assert!(
            !EVERY_VARIANT.validate(&inv).has("T-NOTUSED"),
            "blank is not 'used'"
        );
    }

    #[test]
    fn code_values_fires_only_on_a_value_outside_the_list() {
        let mut inv = subject();
        assert!(!EVERY_VARIANT.validate(&inv).has("T-CODES"));

        inv.type_code = Some(Code::new("381"));
        assert!(EVERY_VARIANT.validate(&inv).has("T-CODES"));

        // Absent or blank is not one of the allowed values.
        inv.type_code = None;
        assert!(EVERY_VARIANT.validate(&inv).has("T-CODES"));
        inv.type_code = Some(Code::new("  "));
        assert!(EVERY_VARIANT.validate(&inv).has("T-CODES"));
    }

    /// A whole-group accessor does not report `BT-0`.
    #[test]
    fn a_group_restriction_does_not_report_bt_0() {
        let label = terms::PAYMENT_INSTRUCTIONS.label();
        assert_eq!(label, "PAYMENT INSTRUCTIONS (BG-16)");
        assert!(!label.contains("BT-0"));
        // …and a real term still carries its id.
        assert_eq!(terms::SELLER_CITY.label(), "Seller city (BT-37)");

        let report = crate::profiles::XRECHNUNG.validate(&Invoice::default());
        let f = report
            .findings()
            .iter()
            .find(|f| f.rule == "BR-DE-1")
            .expect("BG-16 is absent from a default invoice");
        assert!(!f.message.contains("BT-0"), "{}", f.message);
    }

    /// Findings from restrictions carry the profile's own id and a term path.
    #[test]
    fn a_derived_finding_is_indistinguishable_from_a_hand_written_one() {
        let mut inv = subject();
        inv.buyer_reference = None;
        let report = EVERY_VARIANT.validate(&inv);
        let f = report
            .fatal()
            .find(|f| f.rule == "T-MANDATORY")
            .expect("finding");
        assert_eq!(f.path.to_string(), "BT-10");
        assert!(f.message.contains("Buyer reference"), "{}", f.message);
    }
}
