//! Profiles and CIUSes — restrictions as data, rules derived from them.
//!
//! # A CIUS is not "core rules plus extra rules"
//!
//! That is how every Schematron-based tool models it, because Schematron has no
//! other vocabulary. It is **not** how EN 16931 defines one.
//!
//! §7.3.2, *"Allowed specifications in a CIUS"*, is a normative table of the
//! thirteen kinds of change a CIUS may make, across six axes — business terms,
//! cardinality, semantic data type, codes and identifiers, business rules, and
//! value domain. **Only one of those six axes is "add a rule."** The other five
//! are *restrictions on the model*.
//!
//! Modelling them as hand-written rules means writing, by hand, the id, the
//! message, the path and the predicate for something that is pure data. So they
//! are data here, and the rules are **derived**.
//!
//! # Two properties this buys
//!
//! **1. A CIUS can be checked for conformance.** §4.4.2 requires that *"the
//! resulting invoice document instance shall be fully compliant to the core
//! invoice model"*. Every [`Restriction`] variant is by construction a
//! *narrowing*, so that property holds by construction rather than by hope — and
//! a profile that tried to *loosen* something cannot be expressed. Loosening is
//! an **Extension** (§4.3, CEN/TR 16931-5), a different mechanism.
//!
//! **2. Validation widens for free.** §4.4.4 states it outright: an instance
//! complying with a conformant CIUS *"can still be received and processed by a
//! party who is not supporting the CIUS because it still complies to the rules
//! of the core invoice model."* So [`Validated<P>`] converts to
//! `Validated<En16931>` infallibly — see [`Validated::widen`].
//!
//! # How real profiles look
//!
//! XRechnung is the useful case. Of its ~30 `BR-DE-*` rules, **eleven are pure
//! `Mandatory` restrictions** — BT-37, BT-38, BT-41, BT-42, BT-43, BT-52, BT-53,
//! BT-119, BT-10 and two groups — and two are `CodeValues`. Those thirteen are
//! data. The rest — the conditional payment-means rules `BR-DE-23/24/25`, the
//! `BR-DE-16` identifier requirement, `BR-DE-26` — genuinely need code, and go
//! in `extra_rules`. A restriction model that pretended to cover them would be
//! worse than one honest about where it stops.

use core::fmt;
use core::marker::PhantomData;

use crate::bt::{BtId, Path};
use crate::invoice::Invoice;
use crate::validation::{Finding, Rule, Severity, ValidationReport, validate_with_all};

/// What a [`TermAccessor`] returns: one `(where, value)` per occurrence.
///
/// `None` means the term is absent at that place.
pub type Occurrences = Vec<(Path, Option<String>)>;

/// What [`Validated::new`] hands back when the invoice does not pass.
///
/// Boxed because the pair is large and the failure path is the uncommon one:
/// an unboxed `Err` would widen every `Result` in the call chain.
pub type Rejected = Box<(Invoice, ValidationReport)>;

// ── Term accessors ────────────────────────────────────────────────────────────

/// Reads one business term out of an invoice, wherever it occurs.
///
/// Returns one entry per *place* the term can appear, so a document-level term
/// yields one and a per-line term yields one per line. That makes
/// [`Restriction`] uniform over both, and — crucially — makes the [`Path`] in a
/// derived finding come from the accessor rather than from each rule.
pub struct TermAccessor {
    /// Which term.
    pub term: BtId,
    /// Its name in the standard, for messages.
    pub name: &'static str,
    /// `(where, value)` for every occurrence. `None` means absent.
    pub read: fn(&Invoice) -> Occurrences,
}

impl fmt::Debug for TermAccessor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.term, self.name)
    }
}

/// Accessors for the terms profiles actually restrict.
///
/// Deliberately **not** all 164: an accessor is only needed where a CIUS
/// narrows something, and inventing 164 of them speculatively would be a second
/// model to keep in sync with the first. New ones are added as profiles need
/// them, and [`super::rules`] reaches into the model directly.
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

    /// BT-119, which occurs once per VAT breakdown group.
    ///
    /// The reason a per-occurrence accessor is necessary rather than a nicety:
    /// XRechnung's `BR-DE-14` requires BT-119 on **every** BG-23, including the
    /// `O` group that CEN's `BR-48` explicitly exempts.
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

/// One of §7.3.2's allowed narrowings.
///
/// Every variant is a *restriction*: it can only reject documents the core model
/// accepts, never accept ones it rejects. That is what makes §4.4.2's
/// conformance criterion structural rather than aspirational.
#[derive(Debug)]
pub enum Restriction {
    /// Cardinality `0..x → 1..x` — a conditional element becomes mandatory.
    ///
    /// `BR-DE-3` … `BR-DE-9`, `BR-DE-14`, `BR-DE-15` are all this.
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
    /// *"Mark defined values as not allowed"* — restrict a code list.
    ///
    /// `BR-DE-17` restricts BT-3 to eight of the 50+13 UNTDID 1001 codes;
    /// `BR-DE-21` restricts BT-24 to XRechnung's own identifiers.
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
                    format!("{} ({}) shall be present", acc.name, acc.term),
                    None,
                ),
                Self::NotUsed { .. } => (
                    // Blank counts as absent, symmetrically with `Mandatory`.
                    //
                    // It used to be `value.is_none()`, which made the two
                    // variants disagree about what "present" means: a term set
                    // to `"  "` failed `Mandatory` *and* `NotUsed`, so no
                    // document could satisfy either reading of it. Whitespace is
                    // not a value in any of these terms.
                    value.as_deref().is_none_or(|v| v.trim().is_empty()),
                    format!("{} ({}) shall not be used", acc.name, acc.term),
                    None,
                ),
                Self::CodeValues { allowed, .. } => {
                    let v = value.as_deref();
                    (
                        v.is_none_or(|v| allowed.contains(&v)),
                        format!(
                            "{} ({}) shall be one of: {}",
                            acc.name,
                            acc.term,
                            allowed.join(", ")
                        ),
                        // A profile's code list is narrower than the core one,
                        // so a rejected value is usually a *lawful* EN 16931
                        // code this CIUS does not admit. Saying which of the two
                        // it is turns "why is 326 rejected?" into one sentence.
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

/// Why a value failed a [`Restriction::CodeValues`], when the crate can tell.
///
/// Two questions a reader has, in the order they have them:
///
/// 1. **Is it a typo?** `"s"` for `"S"`. Answerable, and unambiguous — no code
///    list this crate carries has two values differing only in case.
/// 2. **Or is it a lawful code this CIUS excludes?** `BT-3 = 386` is valid
///    EN 16931 and valid Peppol, and XRechnung does not permit it. That is a
///    scoping decision, not a mistake, and telling the two apart is the
///    difference between "fix your mapping" and "this profile is the wrong one".
fn case_or_scope_hint(value: &str, allowed: &[&'static str]) -> Option<String> {
    if let Some(c) = allowed.iter().find(|c| c.eq_ignore_ascii_case(value)) {
        return Some(format!(
            "did you mean {c:?}? EN 16931-1 §6.5.8 requires codes \"entered exactly as shown\""
        ));
    }
    // Membership in *any* core list means the value is lawful EN 16931 and this
    // profile has narrowed it away — the one thing the rule text cannot say.
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

/// A core invoice usage specification.
///
/// Carries what §4.4.2 requires a conformant CIUS to state: its own identifier,
/// the specification identifier it puts in BT-24, and the specifications it
/// builds on.
#[derive(Debug)]
pub struct Profile {
    /// Short name, for reports.
    pub id: &'static str,
    /// The BT-24 value a document declaring this profile carries.
    ///
    /// §7.6: *"The invoice instance document itself should carry the assigned
    /// identifier in the business term Specification identification."* That is
    /// what makes [`crate::profiles::for_specification_id`] possible.
    pub specification_id: &'static str,
    /// Which edition of EN 16931-1 this profile is a usage specification of.
    ///
    /// Every deployed CIUS pins exactly one, and a document declares its profile
    /// in BT-24 — so [`crate::profiles::for_specification_id`] recovers the edition from
    /// the document itself. See [`crate::Edition`].
    pub edition: crate::Edition,
    /// §4.4.2: *"shall state its underlying specifications"*.
    pub underlying: &'static [&'static str],
    /// §7.3.2's narrowings, as data.
    pub restrictions: &'static [Restriction],
    /// The one axis that genuinely needs code — conditional rules.
    pub extra_rules: &'static [&'static Rule],
    /// Extension groups this profile can actually represent.
    ///
    /// Core EN 16931 can represent none: BT-113 is a single flat figure with
    /// nowhere to put per-advance tax. ZUGFeRD EXTENDED can. `EN-EXT-01` warns
    /// when an invoice carries data the target cannot express — which is the
    /// difference between a lawful final invoice and a §14c Abs. 1 UStG
    /// liability.
    pub extensions: &'static [&'static str],
    /// Core rules this profile withdraws, because another layer covers them.
    ///
    /// Real: XRechnung 3.0 removed `BR-DE-29` because Peppol's
    /// `PEPPOL-EN16931-R061` covers direct-debit mandates. A model of "each
    /// profile owns a flat list" cannot express that.
    pub suppressed: &'static [&'static str],
}

impl Profile {
    /// Validate `invoice` against the core rules plus this profile.
    #[must_use]
    pub fn validate(&self, invoice: &Invoice) -> ValidationReport {
        // `EN-EXT-01` is about *this* profile's capabilities, so it is skipped
        // when the profile can represent everything the invoice carries. The
        // core rule set assumes the worst, because core EN 16931 can represent
        // nothing.
        let extensions_covered = invoice
            .extensions
            .populated()
            .iter()
            .all(|g| self.extensions.contains(g));

        // Both sides of these comparisons are ids this crate writes, already in
        // the artefacts' canonical spelling — so a plain `==` is right and
        // `matches`' aliasing would be wasted work on the hot path. A user's
        // spelling never reaches here; `suppressed` is profile data.
        let skip = |r: &Rule| {
            (extensions_covered && r.id.as_str() == "EN-EXT-01")
                || self.suppressed.contains(&r.id.as_str())
        };

        let mut report = if self.suppressed.is_empty() && !extensions_covered {
            // The common case: nothing is filtered out, so the core set is
            // passed through as-is and no intermediate `Vec` is built at all.
            validate_with_all(
                invoice,
                super::rules::CORE.iter().copied(),
                self.extra_rules,
            )
        } else {
            let core: Vec<&'static Rule> = super::rules::CORE
                .iter()
                .copied()
                .filter(|r| !skip(r))
                .collect();
            validate_with_all(invoice, core.into_iter(), self.extra_rules)
        };
        let mut extra = Vec::new();
        for restriction in self.restrictions {
            restriction.check(invoice, &mut extra);
        }
        report.absorb(extra, self.restrictions.len());
        report.attribute_to(self.id, self.edition);
        report
    }

    /// Whether this profile is a **conformant CIUS** under §4.4.2 — that is,
    /// whether everything it accepts is also core-valid.
    ///
    /// # It is not always `true`, and it used to say it was
    ///
    /// [`Restriction`] has no loosening variant, so the *restrictions* are
    /// narrowings by construction. That was once the whole story and the method
    /// returned a constant.
    ///
    /// It is not the whole story. [`suppressed`](Self::suppressed) withdraws a
    /// **core** rule, which is precisely the thing §4.4.2 forbids: a document
    /// the profile accepts may then violate the core model. Two shipped profiles
    /// do exactly that, for reasons documented on each:
    ///
    /// * [`XRECHNUNG_CVD`](crate::profiles::XRECHNUNG_CVD) suppresses
    ///   `BR-CL-13`, because CVD marks vehicle lines with a `BT-158` scheme that
    ///   is not in UNTDID 7143.
    /// * [`XRECHNUNG_EXTENSION`](crate::profiles::XRECHNUNG_EXTENSION)
    ///   suppresses `BR-CO-16` and four code-list rules, because an Extension
    ///   **adds**, and §4.3 says that is a different mechanism entirely.
    ///
    /// Both are correct behaviour and neither is a conformant CIUS. Reporting
    /// `true` for them made the method a tautology and the test asserting it
    /// worthless.
    ///
    /// This is exactly the split [`Underlies`] encodes at the type level: there
    /// is no `Underlies<XRechnungExtension> for En16931`, and this method is the
    /// runtime witness of the same fact.
    #[must_use]
    pub const fn is_conformant_cius(&self) -> bool {
        self.suppressed.is_empty()
    }

    /// The terms this profile makes mandatory that `invoice` does not yet
    /// carry — a **pre-flight**, meant for a document still being assembled.
    ///
    /// # Why this is not just `validate`
    ///
    /// [`validate`](Self::validate) answers *"is this document acceptable?"*,
    /// and on a half-built invoice the answer is a hundred findings, most of
    /// them noise about lines and totals that are not there yet. This answers a
    /// different question: **"which extra fields will this profile ask me
    /// for?"** — so it can be called *before* the data is fetched.
    ///
    /// The concrete case: a seller whose master data lives in a contract
    /// service needs to know that XRechnung wants BT-37, BT-38, BT-41, BT-42,
    /// BT-43, BT-52 and BT-53 *before* building the invoice, not from a
    /// post-hoc report. One round trip instead of a build-validate-fetch loop.
    ///
    /// Restrictions only, and deliberately: they are the axis of §7.3.2 that is
    /// pure data, so the answer is exact. The conditional rules in
    /// [`extra_rules`](Self::extra_rules) depend on the document — `BR-DE-23-a`
    /// asks for BT-84 only if BT-81 names a credit transfer — and cannot be
    /// answered before the document exists. [`validate`](Self::validate)
    /// remains the complete check.
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

/// An invoice that has been validated against `P`.
///
/// The point of putting a validation crate between the calculation and the
/// syntax: a serialiser can demand `Validated<XRechnung>` and then physically
/// cannot be handed an unchecked invoice, or one checked against a different
/// profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validated<P: ProfileMarker> {
    invoice: Invoice,
    _profile: PhantomData<P>,
}

impl<P: ProfileMarker> Validated<P> {
    /// Validate, or hand the invoice back with the reason.
    ///
    /// The failure branch returns the invoice so a caller can fix and retry
    /// without cloning.
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
            Err(Box::new((invoice, report)))
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

    /// Re-badge as validated against a *less* restrictive profile.
    ///
    /// Sound by the standard, not by our inference. §4.4.4:
    ///
    /// > If an invoice instance document supports requirements that can be
    /// > considered as a use of a CIUS, the invoice instance document is still
    /// > compliant to the core invoice model. These invoice instance documents
    /// > can still be received and processed by a party who is not supporting
    /// > the CIUS because it still complies to the rules of the core invoice
    /// > model.
    ///
    /// So a `zugferd` crate accepting `Validated<En16931>` takes an
    /// XRechnung-validated invoice with **no re-validation pass**. Narrowing, of
    /// course, is not free and needs [`Validated::new`] again.
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
///
/// Implemented only where the CIUS relationship actually holds, so
/// [`Validated::widen`] cannot be used to launder a weaker proof into a stronger
/// one.
pub trait Underlies<P: ProfileMarker>: ProfileMarker {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::{En16931, XRechnung};

    #[test]
    fn a_cius_states_what_4_4_2_requires() {
        let p = XRechnung::PROFILE;
        assert!(!p.specification_id.is_empty(), "§7.6: BT-24 identifier");
        assert!(
            !p.underlying.is_empty(),
            "§4.4.2: underlying specifications"
        );
        assert!(p.is_conformant_cius());
    }

    #[test]
    fn restrictions_are_data_not_code() {
        let p = XRechnung::PROFILE;
        assert!(
            p.restrictions.len() >= 10,
            "XRechnung has eleven pure Mandatory rules and two CodeValues"
        );
        // Each publishes the real BR-DE id, so a report is lookup-able.
        let ids: Vec<_> = p.restrictions.iter().map(Restriction::id).collect();
        for expect in ["BR-DE-3", "BR-DE-15", "BR-DE-14", "BR-DE-17"] {
            assert!(ids.contains(&expect), "{expect} missing from {ids:?}");
        }
    }

    #[test]
    fn core_is_reachable_from_a_cius_proof() {
        // §4.4.4 — a CIUS-conformant instance is core-conformant, so the type
        // conversion exists and is infallible.
        fn takes_core(_: Validated<En16931>) {}
        fn round_trip(v: Validated<XRechnung>) {
            takes_core(v.widen());
        }
        let _ = round_trip; // compile-time assertion
    }
}

#[cfg(test)]
mod restriction_tests {
    use super::*;
    use crate::invoice::Code;

    /// A profile exercising **every** [`Restriction`] variant.
    ///
    /// No shipped profile uses `NotUsed` — a CIUS that forbids a term outright
    /// is rare, and the two German and Peppol ones express their prohibitions as
    /// conditional rules instead. The variant is still part of §7.3.2's thirteen
    /// permitted changes and part of this crate's public API, so a user defining
    /// their own profile can reach it, which means it needs a test.
    static EVERY_VARIANT: Profile = Profile {
        id: "test",
        edition: crate::Edition::En2017A1,
        specification_id: "urn:test",
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
        extensions: &[],
        suppressed: &[],
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

        // The symmetry that was wrong: blank must read as absent here too, or a
        // whitespace value fails `Mandatory` *and* `NotUsed` at once.
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

        // Absent is not "outside the list" — a `CodeValues` restriction narrows
        // the domain, it does not make the term mandatory. That is what
        // `Mandatory` is for, and conflating them would silently add a
        // requirement the profile never stated.
        inv.type_code = None;
        assert!(!EVERY_VARIANT.validate(&inv).has("T-CODES"));
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
