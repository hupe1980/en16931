//! The profiles this crate ships.
//!
//! # Not "levels"
//!
//! It is tempting to model these as an ordered scale — *basic, EN 16931, Peppol,
//! XRechnung*. There is no such order. Peppol BIS Billing 3.0 and XRechnung are
//! **siblings**: both are CIUSes of EN 16931, and each forbids things the other
//! allows. XRechnung is additionally Peppol-transportable in Germany through the
//! national rule set, which is a third relationship, not a fourth level.
//!
//! ZUGFeRD's BASIC and MINIMUM are not levels of this either — they are profiles
//! *of ZUGFeRD*, and MINIMUM is explicitly **not** an EN 16931-conformant
//! invoice. If `en16931-formats` needs them, they define them.

use crate::validation::Severity;
use crate::validation::profile::{
    ArtefactRef, Profile, ProfileMarker, Restriction, Underlies, terms as t,
};
use crate::validation::rules::peppol;

/// The CEN release every profile here is verified against.
///
/// Equal to [`crate::ARTEFACT_VERSION`], and `tests/artefact_pin.rs` says so.
pub const CEN: ArtefactRef = ArtefactRef {
    authority: "CEN",
    repo: "ConnectingEurope/eInvoicing-EN16931",
    git_ref: crate::ARTEFACT_VERSION,
};

/// KoSIT's Schematron release — the source of every `BR-DE-*` and `BR-DEX-*`.
///
/// KoSIT's changelog states which XRechnung version each release is compatible
/// with; this one says **XRechnung 3.0.x**.
pub const KOSIT_SCHEMATRON: ArtefactRef = ArtefactRef {
    authority: "KoSIT",
    repo: "itplr-kosit/xrechnung-schematron",
    git_ref: "v2.5.0",
};

/// KoSIT's validator configuration release — the source of [`Profile::levels`].
///
/// A separate release from the Schematron, on its own cadence, and the one that
/// actually decides whether a finding is fatal in Germany. Its `master` branch
/// carries overrides that are not in any published release, which is why this
/// is a tag.
pub const KOSIT_CONFIG: ArtefactRef = ArtefactRef {
    authority: "KoSIT",
    repo: "itplr-kosit/validator-configuration-xrechnung",
    git_ref: "v2026-01-31",
};

/// OpenPeppol's release — the source of every `PEPPOL-EN16931-*`.
pub const PEPPOL: ArtefactRef = ArtefactRef {
    authority: "OpenPeppol",
    repo: "OpenPEPPOL/peppol-bis-invoice-3",
    git_ref: "v3.0.20",
};

// ── EN 16931 core ─────────────────────────────────────────────────────────────

/// The core invoice model, with no usage specification on top.
pub static EN16931: Profile = Profile {
    id: "EN 16931",
    edition: crate::Edition::En2017A1,
    specification_id: "urn:cen.eu:en16931:2017",
    artefacts: &[CEN],
    underlying: &[],
    restrictions: &[],
    extra_rules: &[],
    extensions: &[],
    levels: &[],
};

/// Type-level marker for [`EN16931`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct En16931;

impl ProfileMarker for En16931 {
    const PROFILE: &'static Profile = &EN16931;
}

// ── XRechnung ─────────────────────────────────────────────────────────────────

/// BT-3 under `BR-DE-17`, verbatim from KoSIT's `supportedInvAndCNTypeCodes`.
///
/// Eight of the 50 + 13 UNTDID 1001 codes `BR-CL-01` permits. Note it *includes*
/// `389` (self-billed), which Peppol's `P0100` excludes — the two CIUSes
/// genuinely disagree, which is the point of §7.3.1's restriction model and the
/// reason "levels" is the wrong metaphor.
static XR_TYPE_CODES: &[&str] = &["326", "380", "384", "389", "381", "875", "876", "877"];

/// BT-24 under `BR-DE-21` — the CIUS, the Extension, and the CVD variant.
static XR_SPEC_IDS: &[&str] = &[
    // `$XR-CIUS-ID` — the CIUS itself.
    "urn:cen.eu:en16931:2017#compliant#urn:xeinkauf.de:kosit:xrechnung_3.0",
    // `$XR-EXTENSION-ID` = `$XR-CIUS-ID` + `#conformant#…extension:xrechnung_3.0`.
    // Note it *extends* the CIUS identifier rather than replacing it, and that
    // `conformant` is §4.3's word for an Extension.
    "urn:cen.eu:en16931:2017#compliant#urn:xeinkauf.de:kosit:xrechnung_3.0\
#conformant#urn:xeinkauf.de:kosit:extension:xrechnung_3.0",
    // `$XR-CVD-ID` = `$XR-CIUS-ID` + `#compliant#…xrechnung:cvd_0.9`.
    //
    // **`compliant`, not `conformant`** — KoSIT labels the Clean Vehicles
    // variant a CIUS, even though `BR-TMP-CVD-01` widens UNTDID 7143 by one
    // value. The identifier and the behaviour disagree; see `XRECHNUNG_CVD`.
    // Version `0.9`, not `1.0`: `$CVD-MAJOR-MINOR-VERSION`.
    "urn:cen.eu:en16931:2017#compliant#urn:xeinkauf.de:kosit:xrechnung_3.0\
#compliant#urn:xeinkauf.de:kosit:xrechnung:cvd_0.9",
];

/// XRechnung 3.0.2 — the German CIUS, **complete**.
///
/// Thirteen of its `BR-DE-*` rules are pure narrowings and live here as data.
/// The rest — the conditional payment-means rules `BR-DE-23/24/25`, the
/// `BR-DE-16` identifier requirement, `BR-DE-26`'s corrected-invoice reference,
/// the `BR-DE-27/28` format checks, `BR-DE-18`'s Skonto grammar and
/// `BR-DE-22`'s filename uniqueness — need code and live in
/// [`crate::validation::rules::xrechnung`].
///
/// Every `BR-DE-*`, `BR-TMP-*` and `BR-DE-TMP-*` assertion in KoSIT's
/// `XRechnung-UBL-validation.sch` is registered. The `BR-DEX-*` family is
/// **not**: it belongs to the XRechnung *Extension*, a different specification
/// identifier with its own model (sub-invoice lines, third-party payments) that
/// the core model has no terms for. See [`XRECHNUNG_CVD`] for the variant that
/// *is* shipped.
pub static XRECHNUNG: Profile = Profile {
    id: "XRechnung 3.0",
    edition: crate::Edition::En2017A1,
    specification_id: XR_SPEC_IDS[0],
    artefacts: &[CEN, KOSIT_SCHEMATRON, KOSIT_CONFIG, PEPPOL],
    underlying: &["EN 16931", "Peppol BIS Billing 3.0"],
    restrictions: &[
        Restriction::Mandatory {
            id: "BR-DE-1",
            term: &t::PAYMENT_INSTRUCTIONS,
        },
        Restriction::Mandatory {
            id: "BR-DE-3",
            term: &t::SELLER_CITY,
        },
        Restriction::Mandatory {
            id: "BR-DE-4",
            term: &t::SELLER_POST_CODE,
        },
        Restriction::Mandatory {
            id: "BR-DE-5",
            term: &t::SELLER_CONTACT_POINT,
        },
        Restriction::Mandatory {
            id: "BR-DE-6",
            term: &t::SELLER_CONTACT_PHONE,
        },
        Restriction::Mandatory {
            id: "BR-DE-7",
            term: &t::SELLER_CONTACT_EMAIL,
        },
        Restriction::Mandatory {
            id: "BR-DE-8",
            term: &t::BUYER_CITY,
        },
        Restriction::Mandatory {
            id: "BR-DE-9",
            term: &t::BUYER_POST_CODE,
        },
        // The subtle one. CEN's BR-48 exempts category `O` from BT-119;
        // BR-DE-14 has **no category exception**, so an `O` breakdown must state
        // a rate under XRechnung and must not omit it. Suppressing BT-119 for
        // `O` on the strength of BR-O-05 — which governs BT-152, a different
        // term — fails the KoSIT validator.
        Restriction::Mandatory {
            id: "BR-DE-14",
            term: &t::VAT_RATE,
        },
        Restriction::Mandatory {
            id: "BR-DE-15",
            term: &t::BUYER_REFERENCE,
        },
        Restriction::CodeValues {
            id: "BR-DE-17",
            term: &t::TYPE_CODE,
            allowed: XR_TYPE_CODES,
        },
        Restriction::CodeValues {
            id: "BR-DE-21",
            term: &t::SPECIFICATION_ID,
            allowed: XR_SPEC_IDS,
        },
    ],
    // Its own rules and nothing else — see `XR_EXTRA` for why Peppol's are not
    // spliced in here.
    extra_rules: XR_EXTRA,
    // XRechnung has an Extension of its own, but it does not include ZUGFeRD's
    // advance-payment group — so an invoice carrying BG-X-45 loses it here.
    extensions: &[],
    levels: XR_LEVELS,
};

/// The two CEN rules KoSIT's validator reports at **warning** for every
/// XRechnung scenario — CIUS, CVD and Extension alike.
///
/// From `validator-configuration-xrechnung/scenarios.xml`, with the
/// configuration's own comments:
///
/// ```xml
/// <!-- overwrites CEN severity level "fatal" for ISO 6523 values of BT-157 … -->
/// <customLevel level="warning">BR-CL-21</customLevel>
/// <!-- overwrites CEN severity level "fatal" for codelist values of BT-130 … and BT-150 … -->
/// <customLevel level="warning">BR-CL-23</customLevel>
/// ```
///
/// Both are code-list rules whose CEN tables lag the registries they track —
/// ISO 6523 ICD and UN/ECE Rec 20/21. KoSIT will not reject a German invoice
/// over a unit code CEN has not yet imported, and neither will this crate: it
/// reported both as fatal until this was measured against the configuration, and
/// so rejected documents the German reference validator accepts.
///
/// It is also why [`XRECHNUNG`] is **not** a conformant CIUS under §4.4.2 —
/// see [`Profile::is_conformant_cius`](crate::Profile::is_conformant_cius).
static XR_LEVELS: &[(&str, Severity)] = &[
    ("BR-CL-21", Severity::Warning),
    ("BR-CL-23", Severity::Warning),
];

/// XRechnung's own rules **plus the 31 Peppol rules its build merges in**.
///
/// # The released Schematron is not the one in source control
///
/// KoSIT's repository holds a `XRechnung-UBL-validation.sch` containing only
/// `BR-DE-*`, `BR-DEX-*` and `BR-TMP-*`. That file is an **input**. The build
/// runs `peppol-into-xr.xsl` over it, splicing in every Peppol assert named in
/// `rule-list.xml`, and *that* is what ships and what the KoSIT validator loads.
///
/// So the validator configuration naming two Schematrons — CEN's and
/// XRechnung's — is not evidence that Peppol's rules are absent. They are inside
/// the second one. See [`peppol::MERGED_INTO_XRECHNUNG`] for the 31, and the
/// fifteen deliberately left out.
///
/// Two of the merged rules are **rewritten** on the way in, so XRechnung gets
/// its own instances: [`peppol::XR_R120`] is downgraded to a warning, and both
/// it and [`peppol::XR_R040`] use a slack of `0.5` for HUF where Peppol always
/// uses `0.02`.
static XR_EXTRA: &[&crate::validation::Rule] = &const {
    let x = crate::validation::rules::xrechnung::ALL;
    let p = peppol::FOR_XRECHNUNG;
    let mut out = [x[0]; 43];
    let mut i = 0;
    while i < x.len() {
        out[i] = x[i];
        i += 1;
    }
    let mut j = 0;
    while j < p.len() {
        out[x.len() + j] = p[j];
        j += 1;
    }
    assert!(x.len() + p.len() == 43);
    out
};

/// Type-level marker for [`XRECHNUNG`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XRechnung;

impl ProfileMarker for XRechnung {
    const PROFILE: &'static Profile = &XRECHNUNG;
}

// **No `Underlies<XRechnung> for En16931`, and this was a soundness bug.**
//
// §4.4.4's guarantee — that a CIUS-valid instance is core-valid — holds for a
// CIUS that only restricts. XRechnung's *specification* does only restrict. Its
// *reference validator* does not: `XR_LEVELS` reports `BR-CL-21` and `BR-CL-23`
// at warning, so a document with a unit code outside CEN's Rec 20 table is a
// valid XRechnung and is not a valid core invoice.
//
// With the impl, `Validated::<XRechnung>::new(inv)?.widen::<En16931>()` handed
// back a proof of core-validity for exactly that document — the same hole
// `XRechnungCvd` had and for the same reason, one layer up. It is not a
// hypothetical: KoSIT downgraded these two precisely because real German
// invoices carry codes CEN has not imported.
//
// `Validated::<En16931>::new(invoice)` re-runs the core rule set and is one
// line. A proof that has to be earned is the point of the type.
//
// `exactly_three_profiles_are_not_conformant_ciuses` and
// `an_xrechnung_invoice_can_be_core_invalid` in `tests/profiles.rs` are the
// evidence.

// ── Peppol BIS Billing 3.0 ────────────────────────────────────────────────────

/// BT-24 under `PEPPOL-EN16931-R004`.
pub(crate) const PEPPOL_SPEC_ID: &str =
    "urn:cen.eu:en16931:2017#compliant#urn:fdc:peppol.eu:2017:poacc:billing:3.0";

/// Peppol BIS Billing 3.0.
///
/// Pure-data restrictions plus Peppol's own arithmetic — `R120`
/// (`BT-131 = BT-129 × (BT-146 ÷ BT-149) + BG-28 − BG-27`, ±0.02), `R040`,
/// `R041`, `R042`, `R046` (**exact**), `R061`, `R121` and `R130`. Those are
/// conditional computations rather than narrowings, so they live in
/// `extra_rules` — §7.3.2's one axis that genuinely needs code.
pub static PEPPOL_BIS_3: Profile = Profile {
    id: "Peppol BIS Billing 3.0",
    edition: crate::Edition::En2017A1,
    specification_id: PEPPOL_SPEC_ID,
    artefacts: &[CEN, PEPPOL],
    underlying: &["EN 16931"],
    // `R003` and `R004` used to live here as `Restriction::Mandatory` and both
    // were wrong for the same reason: a restriction can say *"this term is
    // present"* and neither rule says that.
    //
    // `R003` is a **disjunction** — BT-10 *or* BT-13 — so requiring BT-10
    // rejected eight of CEN's own published test invoices, every one of which
    // supplies an order reference instead. `R004` constrains BT-24's **value**
    // with `starts-with`, so requiring mere presence accepted any string at all.
    //
    // Both are now rules. See `peppol::R003` and `peppol::R004`.
    restrictions: &[],
    // Peppol's own arithmetic — the ±0.02 regime. `R120` has no CEN
    // counterpart at all, which is exactly why it lives here rather than in the
    // core rule set.
    extra_rules: crate::validation::rules::peppol::ALL,
    extensions: &[],
    // Peppol re-levels nothing: its Schematron ships the flags it means, and
    // there is no separate report configuration to override them. So Peppol BIS
    // Billing 3.0 *is* a conformant CIUS, and XRechnung is not — a difference
    // between the two that only shows up once the configurations are read.
    levels: &[],
};

/// Type-level marker for [`PEPPOL_BIS_3`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeppolBis3;

impl ProfileMarker for PeppolBis3 {
    const PROFILE: &'static Profile = &PEPPOL_BIS_3;
}

impl Underlies<PeppolBis3> for En16931 {}

// ── Lookup ────────────────────────────────────────────────────────────────────

/// Every profile this crate ships.
/// XRechnung's rules plus the CVD variant's seven, spliced at compile time.
static XR_CVD_EXTRA: &[&crate::validation::Rule] = &const {
    let x = XR_EXTRA;
    let c = crate::validation::rules::xrechnung::cvd::ALL;
    let mut out = [x[0]; 51];
    let mut i = 0;
    while i < x.len() {
        out[i] = x[i];
        i += 1;
    }
    let mut j = 0;
    while j < c.len() {
        out[x.len() + j] = c[j];
        j += 1;
    }
    assert!(x.len() + c.len() == 51);
    out
};

/// XRechnung 3.0 **CVD** — the EU Clean Vehicles Directive variant.
///
/// A CIUS on a CIUS. Directive (EU) 2019/1161 sets minimum procurement targets
/// for clean road vehicles, and a public body can only count a vehicle it can
/// identify — so the invoice must name the contract (BT-12), the tender
/// (BT-17), and for at least one line the vehicle category and its clean-vehicle
/// attribute.
///
/// It has its **own** specification identifier, which is what selects it:
///
/// ```
/// use en16931::profiles;
///
/// let p = profiles::for_specification_id(profiles::XRECHNUNG_CVD.specification_id);
/// assert_eq!(p.map(|p| p.id), Some("XRechnung 3.0 CVD"));
/// ```
///
/// # KoSIT calls it a CIUS; it behaves like an Extension
///
/// Its identifier ends `#compliant#urn:xeinkauf.de:kosit:xrechnung:cvd_0.9`.
/// **`compliant`** is §4.3's word for a CIUS — so KoSIT classifies this as a
/// restriction of XRechnung.
///
/// The rules do not agree. `BR-TMP-CVD-01` checks BT-158's scheme against
/// `concat($CVD-CODE, $UNTDID-7143-CODES)` — UNTDID 7143 **plus `CVD`** — and
/// `CVD` is not in UNTDID 7143. A CVD invoice therefore violates core
/// `BR-CL-13`, which a CIUS is not permitted to cause: §4.4.4 guarantees that
/// CIUS-valid implies core-valid.
///
/// This crate follows the behaviour rather than the label, because the behaviour
/// is what a validator has to reproduce: `BR-CL-13` is **re-levelled to
/// `Information`** — the level KoSIT's own `scenarios.xml` gives it — and
/// `BR-TMP-CVD-01` runs in its place. The alternative, reporting `BR-CL-13` as
/// fatal on every conforming CVD invoice, would be a false positive on a
/// document KoSIT accepts.
///
/// Everything else XRechnung requires, this requires too. That is *not* stated
/// at the type level: relaxing `BR-CL-13` is precisely what stops this being a
/// conformant CIUS, so there is no widening out of it — see below.
pub static XRECHNUNG_CVD: Profile = Profile {
    id: "XRechnung 3.0 CVD",
    edition: crate::Edition::En2017A1,
    specification_id: XR_SPEC_IDS[2],
    artefacts: &[CEN, KOSIT_SCHEMATRON, KOSIT_CONFIG, PEPPOL],
    underlying: &["EN 16931", "XRechnung 3.0"],
    restrictions: XRECHNUNG.restrictions,
    extra_rules: XR_CVD_EXTRA,
    extensions: &[],
    // `<customLevel level="information">BR-CL-13</customLevel>`, on top of
    // XRechnung's own two. CVD widens UNTDID 7143 by one value, so KoSIT keeps
    // reporting the core rule and stops rejecting on it — `BR-TMP-CVD-01` is
    // what actually decides. Dropping `BR-CL-13` instead, as this crate did,
    // lost the line that explains the `CVD` scheme to a reader.
    levels: &[
        ("BR-CL-21", Severity::Warning),
        ("BR-CL-23", Severity::Warning),
        ("BR-CL-13", Severity::Info),
    ],
};

/// Type-level marker for [`XRECHNUNG_CVD`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XRechnungCvd;

impl ProfileMarker for XRechnungCvd {
    const PROFILE: &'static Profile = &XRECHNUNG_CVD;
}

// **No `Underlies` impl, deliberately — and this was a soundness bug.**
//
// It used to read:
//
// ```rust,ignore
// // A CVD-valid invoice is XRechnung-valid, which is core-valid.
// impl Underlies<XRechnungCvd> for XRechnung {}
// impl Underlies<XRechnungCvd> for En16931 {}
// ```
//
// The comment is false. CVD **relaxes `BR-CL-13` to `Information`**, because it
// marks vehicle lines with a `BT-158` scheme that is not in UNTDID 7143 — so a
// conforming CVD invoice violates the core model, exactly as `XRECHNUNG_CVD`'s
// own documentation says.
//
// With those impls, `Validated<XRechnungCvd>::widen::<En16931>()` compiled and
// handed back a `Validated<En16931>`: a **proof of core-validity for a document
// that is not core-valid**. A serialiser accepting the core proof — which is the
// whole point of `Validated<P>` — would then have been handed an invoice no
// core-only receiver can process.
//
// `is_conformant_cius()` is the runtime witness of the same fact, and
// `a_cvd_invoice_can_be_core_invalid` in `tests/profiles.rs` is the evidence.
// Widening out of CVD requires re-validating, which is what `Validated::new`
// is for.

/// XRechnung's rules plus the Extension's fourteen.
static XR_EXT_EXTRA: &[&crate::validation::Rule] = &const {
    let x = XR_EXTRA;
    let e = crate::validation::rules::xrechnung::extension::ALL;
    let mut out = [x[0]; 57];
    let mut i = 0;
    while i < x.len() {
        out[i] = x[i];
        i += 1;
    }
    let mut j = 0;
    while j < e.len() {
        out[x.len() + j] = e[j];
        j += 1;
    }
    assert!(x.len() + e.len() == 57);
    out
};

/// XRechnung 3.0 **Extension** — §4.3's second mechanism, in the wild.
///
/// Where [`XRECHNUNG`] restricts, this **adds**: sub-invoice lines
/// ([`crate::extensions::SubInvoiceLine`], `BG-DEX-01`) for invoices whose
/// positions decompose, and third-party payments
/// ([`crate::extensions::ThirdPartyPayment`], `BG-DEX-09`) for the German
/// digital-health case where a statutory insurer settles part of an invoice
/// addressed to the insured.
///
/// # Three widenings, and what each costs
///
/// | | Core / CIUS | Here |
/// |---|---|---|
/// | BT-125 mime code | six codes | + `application/xml` |
/// | scheme identifiers | ISO 6523 ICD / CEF EAS | + `XR01`, `XR02`, `XR03` (DiGA) |
/// | BT-115 | `BR-CO-16` | **`BR-DEX-09`** — third-party payments added back |
///
/// Each widening is why this is not a CIUS. §4.4.4's guarantee runs one way
/// only: an Extension-valid invoice **need not be core-valid**, so there is
/// deliberately no `Underlies<XRechnungExtension> for En16931` here — the
/// direction [`XRECHNUNG`] and [`XRECHNUNG_CVD`] both have.
///
/// `BR-CO-16` is re-levelled to *information* by [`Profile::levels`] rather than
/// layered over, because with a third-party payment present the two equations
/// disagree by exactly that sum — so reporting `BR-CO-16` as fatal beside
/// `BR-DEX-09` would be reporting the same fact twice, once wrongly.
///
/// [`Profile::levels`]: crate::validation::profile::Profile::levels
pub static XRECHNUNG_EXTENSION: Profile = Profile {
    id: "XRechnung 3.0 Extension",
    edition: crate::Edition::En2017A1,
    specification_id: XR_SPEC_IDS[1],
    artefacts: &[CEN, KOSIT_SCHEMATRON, KOSIT_CONFIG, PEPPOL],
    underlying: &["XRechnung 3.0"],
    restrictions: XRECHNUNG.restrictions,
    extra_rules: XR_EXT_EXTRA,
    extensions: &[
        crate::extensions::SUB_INVOICE_LINES,
        crate::extensions::THIRD_PARTY_PAYMENTS,
    ],
    // Transcribed from the Extension scenario of
    // `validator-configuration-xrechnung/scenarios.xml`, in its order, with one
    // documented divergence below.
    //
    // The previous list was reconstructed from "which core rule does each
    // `BR-DEX-*` widen?" and got two of them wrong. It named
    // `PEPPOL-EN16931-CL001` for the mime code — a rule XRechnung's build does
    // not merge in ([`peppol::MERGED_INTO_XRECHNUNG`]), so it never ran and
    // withdrawing it did nothing, while CEN's `BR-CL-24` went on rejecting
    // exactly the `application/xml` attachment `BR-DEX-01` exists to permit. And
    // it omitted `BR-CL-21` entirely.
    //
    // [`peppol::MERGED_INTO_XRECHNUNG`]: crate::validation::rules::peppol::MERGED_INTO_XRECHNUNG
    levels: &[
        ("BR-CL-21", Severity::Info),    // BR-DEX-06 widens BT-157's schemes
        ("BR-CL-23", Severity::Warning), // as for every XRechnung scenario
        ("BR-CL-24", Severity::Info),    // BR-DEX-01 adds application/xml
        ("BR-CL-10", Severity::Info),    // BR-DEX-04 adds the DiGA schemes
        ("BR-CL-11", Severity::Info),    // BR-DEX-05
        ("BR-CL-25", Severity::Info),    // BR-DEX-07
        ("BR-CL-26", Severity::Info),    // BR-DEX-08
        // The one divergence, and it is KoSIT's rather than ours: the **UBL**
        // Extension scenario carries this and the **CII** one does not, though
        // `BR-DEX-09` replaces `BR-CO-16` in both. A syntax-independent model
        // cannot hold two answers, and reporting a third-party payment as a
        // fatal `BR-CO-16` would contradict the rule that exists to permit it —
        // so the UBL configuration is followed.
        ("BR-CO-16", Severity::Info),
    ],
};

/// Type-level marker for [`XRECHNUNG_EXTENSION`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XRechnungExtension;

impl ProfileMarker for XRechnungExtension {
    const PROFILE: &'static Profile = &XRECHNUNG_EXTENSION;
}

/// Every profile this crate ships, in increasing specificity.
pub static ALL: &[&Profile] = &[
    &EN16931,
    &XRECHNUNG,
    &XRECHNUNG_CVD,
    &XRECHNUNG_EXTENSION,
    &PEPPOL_BIS_3,
];

/// Find the profile a document declares in BT-24.
///
/// §7.6 exists for exactly this: *"The invoice instance document itself should
/// carry the assigned identifier in the business term Specification
/// identification. This will allow the receiver of the invoice instance document
/// to apply processing of the document in accordance with the rules under which
/// it was generated."*
///
/// Validating an invoice that declares XRechnung against core-only rules is the
/// most common way to ship a document a validator then rejects.
#[must_use]
pub fn for_specification_id(id: &str) -> Option<&'static Profile> {
    ALL.iter()
        .copied()
        .find(|p| p.specification_id == id)
        .or_else(|| {
            // Both KoSIT variants embed the CIUS identifier and extend it, so a
            // document declaring one is at minimum an XRechnung document.
            id.starts_with(XR_SPEC_IDS[0]).then_some(&XRECHNUNG)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_are_siblings_not_a_scale() {
        // The clearest proof: XRechnung permits BT-3 = 389 (self-billed) and
        // Peppol does not. Neither is "more restrictive" overall.
        let peppol_invoice_codes = |c: &str| {
            let inv = crate::Invoice {
                business_process: Some("urn:fdc:peppol.eu:2017:poacc:billing:01:1.0".to_owned()),
                type_code: Some(crate::invoice::Code::new(c)),
                ..Default::default()
            };
            PEPPOL_BIS_3.validate(&inv).has("PEPPOL-EN16931-P0100")
        };
        assert!(XR_TYPE_CODES.contains(&"389"));
        assert!(peppol_invoice_codes("389"), "Peppol rejects 389");
        // …and Peppol permits codes XRechnung does not.
        assert!(!peppol_invoice_codes("386"), "Peppol accepts 386");
        assert!(!XR_TYPE_CODES.contains(&"386"));
    }

    #[test]
    fn a_document_selects_its_own_profile_through_bt_24() {
        assert_eq!(
            for_specification_id(XRECHNUNG.specification_id).map(|p| p.id),
            Some("XRechnung 3.0")
        );
        // Each KoSIT variant has its own identifier and selects its own rules.
        assert_eq!(
            for_specification_id(XR_SPEC_IDS[1]).map(|p| p.id),
            Some("XRechnung 3.0 Extension")
        );
        assert_eq!(
            for_specification_id(XR_SPEC_IDS[2]).map(|p| p.id),
            Some("XRechnung 3.0 CVD")
        );
        // An unknown KoSIT variant still resolves to the CIUS it embeds.
        assert_eq!(
            for_specification_id(&format!("{}#conformant#urn:nope", XR_SPEC_IDS[0])).map(|p| p.id),
            Some("XRechnung 3.0")
        );
        assert_eq!(
            for_specification_id(EN16931.specification_id).map(|p| p.id),
            Some("EN 16931")
        );
        assert!(for_specification_id("urn:nonsense").is_none());
    }

    #[test]
    fn every_profile_states_what_4_4_2_requires() {
        for p in ALL {
            assert!(!p.id.is_empty());
            assert!(!p.specification_id.is_empty());
            // A CIUS shall state its underlying specifications; only the core
            // model itself has none.
            assert_eq!(p.underlying.is_empty(), p.id == "EN 16931", "{}", p.id);
            // Conformance is a *property*, computed from the level overrides.
            // Relaxing a core rule is exactly what §4.4.2 forbids, so the two
            // must agree in both directions.
            let relaxes = p.levels.iter().any(|(id, level)| {
                crate::validation::rules::explain(id).is_some_and(|r| *level > r.severity)
            });
            assert_eq!(
                p.is_conformant_cius(),
                !relaxes,
                "{} — conformance must follow the level overrides",
                p.id
            );
        }
    }

    /// The profiles that are **not** conformant CIUSes, named.
    ///
    /// Asserted exactly, so neither direction can drift silently: relaxing a
    /// core rule in a CIUS fails here, and so does removing an override without
    /// revisiting whether widening out of it is now sound.
    ///
    /// **XRechnung 3.0 itself is on this list**, which is the finding that came
    /// out of reading KoSIT's validator configuration rather than its
    /// Schematron: it reports `BR-CL-21` and `BR-CL-23` at warning, so a
    /// document it accepts can violate the core model.
    #[test]
    fn exactly_three_profiles_are_not_conformant_ciuses() {
        let not: Vec<&str> = ALL
            .iter()
            .filter(|p| !p.is_conformant_cius())
            .map(|p| p.id)
            .collect();
        assert_eq!(
            not,
            [
                "XRechnung 3.0",
                "XRechnung 3.0 CVD",
                "XRechnung 3.0 Extension"
            ]
        );
    }

    /// Every level override must name a rule the profile actually runs.
    ///
    /// The invariant that would have caught `PEPPOL-EN16931-CL001`: it named a
    /// Peppol rule XRechnung's build does not merge in, so it applied to nothing
    /// while the core rule it was meant to relax went on firing.
    #[test]
    fn every_level_override_names_a_rule_that_runs() {
        for p in ALL {
            let ids: Vec<&str> = p.check_ids().collect();
            for (id, _) in p.levels {
                assert!(
                    ids.contains(id),
                    "{} re-levels {id}, which it does not run",
                    p.id
                );
            }
        }
    }

    /// A finding does not vanish when a profile relaxes it — it is reported at
    /// the authority's level, which is what KoSIT's `<customLevel>` does.
    #[test]
    fn a_relaxed_rule_is_reported_rather_than_dropped() {
        use crate::validation::Severity;
        use crate::{Invoice, InvoiceAmount, InvoiceLine, Percentage, Quantity};

        let mut inv = Invoice::default();
        inv.lines.push(InvoiceLine::new(
            "1",
            "Widget",
            Quantity::ONE,
            "NOT-A-UNIT", // BR-CL-23
            InvoiceAmount::ZERO,
            "S",
            Some(Percentage::new(rust_decimal::Decimal::from(19))),
        ));

        let core = EN16931.validate(&inv);
        assert!(
            core.fatal().any(|f| f.rule == "BR-CL-23"),
            "fatal in the core model"
        );

        let xr = XRECHNUNG.validate(&inv);
        let f = xr
            .findings()
            .iter()
            .find(|f| f.rule == "BR-CL-23")
            .expect("still reported");
        assert_eq!(f.severity, Severity::Warning, "KoSIT's customLevel");
    }
}
