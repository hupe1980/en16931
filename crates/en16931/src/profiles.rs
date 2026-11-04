//! The profiles this crate ships.
//!
//! The profiles are not an ordered scale: Peppol BIS Billing 3.0 and XRechnung
//! are sibling CIUSes of EN 16931, and each forbids things the other allows.
//!
//! The four ZUGFeRD / Factur-X profiles ([`ZUGFERD`]) exist only so [`classify`]
//! can answer for a received document; they are outside [`ALL`]. MINIMUM and
//! BASIC WL are not EN 16931 invoices and run only the totals rules their terms
//! carry; BASIC and EXTENDED run the core rules (FeRD's own rules are not shipped).

use crate::validation::Severity;
use crate::validation::profile::{
    ArtefactRef, Core, Profile, ProfileMarker, Restriction, Underlies, terms as t,
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

/// KoSIT's Schematron release (XRechnung 3.0) — the source of every `BR-DE-*` and `BR-DEX-*`.
pub const KOSIT_SCHEMATRON: ArtefactRef = ArtefactRef {
    authority: "KoSIT",
    repo: "itplr-kosit/xrechnung-schematron",
    git_ref: "v2.6.0",
};

/// KoSIT's validator configuration release — the source of [`Profile::levels`].
///
/// Released separately from the Schematron; it decides whether a finding is
/// fatal in Germany.
pub const KOSIT_CONFIG: ArtefactRef = ArtefactRef {
    authority: "KoSIT",
    repo: "itplr-kosit/validator-configuration-xrechnung",
    git_ref: "v2026-08-31",
};

/// OpenPeppol's release — the source of every `PEPPOL-EN16931-*` and
/// `PEPPOL-COMMON-*`.
///
/// BIS Billing 3.0.21-hotfix (2026-10-28), the commit tag `3.0.21` names.
pub const PEPPOL: ArtefactRef = ArtefactRef {
    authority: "OpenPeppol",
    repo: "OpenPEPPOL/peppol-bis-invoice-3",
    git_ref: "5df0eff7c8efafd7c79c0dacce55082d175db759",
};

// ── EN 16931 core ─────────────────────────────────────────────────────────────

/// The core invoice model, with no usage specification on top.
pub static EN16931: Profile = Profile {
    id: "EN 16931",
    slug: "en16931",
    aliases: &["core"],
    edition: crate::Edition::En2017A1,
    specification_id: "urn:cen.eu:en16931:2017",
    artefacts: &[CEN],
    underlying: &[],
    restrictions: &[],
    extra_rules: &[],
    core: Core::All,
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
/// Includes `389` (self-billed), which Peppol's `P0100` excludes.
static XR_TYPE_CODES: &[&str] = &["326", "380", "384", "389", "381", "875", "876", "877"];

/// BT-24 under `BR-DE-21` — the CIUS, the Extension, and the CVD variant.
static XR_SPEC_IDS: &[&str] = &[
    // `$XR-CIUS-ID` — the CIUS itself.
    "urn:cen.eu:en16931:2017#compliant#urn:xeinkauf.de:kosit:xrechnung_3.0",
    // `$XR-EXTENSION-ID` = `$XR-CIUS-ID` + `#conformant#…extension:xrechnung_3.0`.
    "urn:cen.eu:en16931:2017#compliant#urn:xeinkauf.de:kosit:xrechnung_3.0\
#conformant#urn:xeinkauf.de:kosit:extension:xrechnung_3.0",
    // `$XR-CVD-ID` = `$XR-CIUS-ID` + `#compliant#…xrechnung:cvd_0.9`
    // (`compliant` although it widens UNTDID 7143; see `XRECHNUNG_CVD`).
    "urn:cen.eu:en16931:2017#compliant#urn:xeinkauf.de:kosit:xrechnung_3.0\
#compliant#urn:xeinkauf.de:kosit:xrechnung:cvd_0.9",
];

/// XRechnung 3.0 — the German CIUS.
///
/// Its pure narrowings are listed here as restrictions; the `BR-DE-*` rules that
/// need code live in [`crate::validation::rules::xrechnung`]. The `BR-DEX-*`
/// rules belong to [`XRECHNUNG_EXTENSION`].
pub static XRECHNUNG: Profile = Profile {
    id: "XRechnung 3.0",
    slug: "xrechnung",
    aliases: &[],
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
        // Unlike CEN's BR-48, BR-DE-14 has no exception for category `O`.
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
    extra_rules: XR_EXTRA,
    core: Core::All,
    extensions: &[],
    levels: XR_LEVELS,
};

/// Checks KoSIT reports at warning that would otherwise be fatal, for every
/// XRechnung scenario.
///
/// Two sources: `scenarios.xml`'s `customLevel` relaxes CEN's `BR-CL-21` and
/// `BR-CL-23` (CEN's code tables lag ISO 6523 and UN/ECE Rec 20/21), and the
/// Schematron's `flag="warning"` covers the restrictions `BR-DE-17` and
/// `BR-DE-21`, which as a `Restriction` carry no severity of their own.
///
/// Relaxing core rules makes [`XRECHNUNG`] not a conformant CIUS under §4.4.2;
/// see [`Profile::is_conformant_cius`](crate::Profile::is_conformant_cius).
static XR_LEVELS: &[(&str, Severity)] = &[
    // scenarios.xml `customLevel`, over CEN's rules.
    ("BR-CL-21", Severity::Warning),
    ("BR-CL-23", Severity::Warning),
    // The Schematron's own `flag`, over restrictions this crate derives.
    ("BR-DE-17", Severity::Warning),
    ("BR-DE-21", Severity::Warning),
];

/// XRechnung's own rules plus the 21 Peppol rules KoSIT's build merges into
/// the released Schematron ([`peppol::MERGED_INTO_XRECHNUNG`]).
///
/// [`peppol::XR_R120`] (a warning) and [`peppol::XR_R040`] are XRechnung's
/// rewritten instances, with a HUF tolerance of `0.5` instead of `0.02`.
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

// No `Underlies<XRechnung> for En16931`: `XR_LEVELS` relaxes `BR-CL-21` and
// `BR-CL-23`, so a valid XRechnung can be core-invalid. Widen by re-validating
// with `Validated::<En16931>::new`.

// ── Peppol BIS Billing 3.0 ────────────────────────────────────────────────────

/// BT-24 under `PEPPOL-EN16931-R004`.
pub(crate) const PEPPOL_SPEC_ID: &str =
    "urn:cen.eu:en16931:2017#compliant#urn:fdc:peppol.eu:2017:poacc:billing:3.0";

/// Peppol BIS Billing 3.0.
///
/// All of Peppol's checks are rules in `extra_rules`, including its arithmetic
/// (`R120`: `BT-131 = BT-129 × (BT-146 ÷ BT-149) + BG-28 − BG-27`, ±0.02).
pub static PEPPOL_BIS_3: Profile = Profile {
    id: "Peppol BIS Billing 3.0",
    slug: "peppol",
    aliases: &["peppol-bis-3", "bis3"],
    edition: crate::Edition::En2017A1,
    specification_id: PEPPOL_SPEC_ID,
    artefacts: &[CEN, PEPPOL],
    underlying: &["EN 16931"],
    // `R003` (BT-10 or BT-13) and `R004` (BT-24 prefix) are not expressible as
    // a `Restriction`, so they are rules.
    restrictions: &[],
    extra_rules: crate::validation::rules::peppol::ALL,
    core: Core::All,
    extensions: &[],
    // Peppol re-levels nothing, so it is a conformant CIUS.
    levels: &[],
};

/// Type-level marker for [`PEPPOL_BIS_3`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeppolBis3;

impl ProfileMarker for PeppolBis3 {
    const PROFILE: &'static Profile = &PEPPOL_BIS_3;
}

impl Underlies<PeppolBis3> for En16931 {}

/// XRechnung's rules plus the CVD variant's eight, spliced at compile time.
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
/// For Directive (EU) 2019/1161: the invoice must name the contract (BT-12),
/// the tender (BT-17) and, for at least one line, the vehicle category and its
/// clean-vehicle attribute. Selected by its own specification identifier:
///
/// ```
/// use en16931::profiles;
///
/// let p = profiles::classify(Some(profiles::XRECHNUNG_CVD.specification_id));
/// assert_eq!(p.profile().map(|p| p.id), Some("XRechnung 3.0 CVD"));
/// ```
///
/// KoSIT labels it a CIUS (`#compliant#`), but `BR-TMP-CVD-01` admits the BT-158
/// scheme `CVD`, which is not in UNTDID 7143. As in KoSIT's `scenarios.xml`,
/// `BR-CL-13` is reported at `Information` and `BR-TMP-CVD-01` decides, so this
/// is not a conformant CIUS. Otherwise it requires everything XRechnung does.
pub static XRECHNUNG_CVD: Profile = Profile {
    id: "XRechnung 3.0 CVD",
    slug: "xrechnung-cvd",
    aliases: &[],
    edition: crate::Edition::En2017A1,
    specification_id: XR_SPEC_IDS[2],
    artefacts: &[CEN, KOSIT_SCHEMATRON, KOSIT_CONFIG, PEPPOL],
    underlying: &["EN 16931", "XRechnung 3.0"],
    restrictions: XRECHNUNG.restrictions,
    extra_rules: XR_CVD_EXTRA,
    core: Core::All,
    extensions: &[],
    // XRechnung's levels plus `<customLevel level="information">BR-CL-13`.
    levels: &[
        ("BR-CL-21", Severity::Warning),
        ("BR-CL-23", Severity::Warning),
        ("BR-CL-13", Severity::Info),
        ("BR-DE-17", Severity::Warning),
        ("BR-DE-21", Severity::Warning),
    ],
};

/// Type-level marker for [`XRECHNUNG_CVD`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XRechnungCvd;

impl ProfileMarker for XRechnungCvd {
    const PROFILE: &'static Profile = &XRECHNUNG_CVD;
}

// No `Underlies` impl: CVD relaxes `BR-CL-13`, so a CVD-valid invoice can be
// core-invalid. Widen by re-validating with `Validated::new`.

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

/// XRechnung 3.0 **Extension** (§4.3).
///
/// Adds sub-invoice lines ([`crate::extensions::SubInvoiceLine`], `BG-DEX-01`)
/// and third-party payments ([`crate::extensions::ThirdPartyPayment`],
/// `BG-DEX-09`), and widens the core:
///
/// | | Core / CIUS | Here |
/// |---|---|---|
/// | BT-125 mime code | six codes | + `application/xml` |
/// | scheme identifiers | ISO 6523 ICD / CEF EAS | + `XR01`, `XR02`, `XR03` (DiGA) |
/// | BT-115 | `BR-CO-16` | `BR-DEX-09` — third-party payments added back |
///
/// An Extension-valid invoice need not be core-valid, so there is no
/// `Underlies` impl. The widened core rules are reported at *information* via
/// [`Profile::levels`].
///
/// [`Profile::levels`]: crate::validation::profile::Profile::levels
pub static XRECHNUNG_EXTENSION: Profile = Profile {
    id: "XRechnung 3.0 Extension",
    slug: "xrechnung-extension",
    aliases: &[],
    edition: crate::Edition::En2017A1,
    specification_id: XR_SPEC_IDS[1],
    artefacts: &[CEN, KOSIT_SCHEMATRON, KOSIT_CONFIG, PEPPOL],
    underlying: &["XRechnung 3.0"],
    restrictions: XRECHNUNG.restrictions,
    extra_rules: XR_EXT_EXTRA,
    core: Core::All,
    extensions: &[
        crate::extensions::SUB_INVOICE_LINES,
        crate::extensions::THIRD_PARTY_PAYMENTS,
    ],
    // Transcribed from the Extension scenario of `scenarios.xml`, in its order.
    levels: &[
        ("BR-CL-21", Severity::Info),    // BR-DEX-06 widens BT-157's schemes
        ("BR-CL-23", Severity::Warning), // as for every XRechnung scenario
        ("BR-CL-24", Severity::Info),    // BR-DEX-01 adds application/xml
        ("BR-CL-10", Severity::Info),    // BR-DEX-04 adds the DiGA schemes
        ("BR-CL-11", Severity::Info),    // BR-DEX-05
        ("BR-CL-25", Severity::Info),    // BR-DEX-07
        ("BR-CL-26", Severity::Info),    // BR-DEX-08
        // Only KoSIT's UBL scenario carries this, though `BR-DEX-09` replaces
        // `BR-CO-16` in both syntaxes; the UBL configuration is followed.
        ("BR-CO-16", Severity::Info),
        // From the Schematron's `flag`; see `XR_LEVELS`.
        ("BR-DE-17", Severity::Warning),
        ("BR-DE-21", Severity::Warning),
    ],
};

/// Type-level marker for [`XRECHNUNG_EXTENSION`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XRechnungExtension;

impl ProfileMarker for XRechnungExtension {
    const PROFILE: &'static Profile = &XRECHNUNG_EXTENSION;
}

/// Every profile this crate ships for writing and proving: the core model and
/// the CIUSes and extensions built on it.
pub static ALL: &[&Profile] = &[
    &EN16931,
    &XRECHNUNG,
    &XRECHNUNG_CVD,
    &XRECHNUNG_EXTENSION,
    &PEPPOL_BIS_3,
];

// ── ZUGFeRD / Factur-X ────────────────────────────────────────────────────────

/// BT-24 of a ZUGFeRD / Factur-X **MINIMUM** document.
pub const ZUGFERD_MINIMUM_ID: &str = "urn:factur-x.eu:1p0:minimum";

/// BT-24 of a ZUGFeRD / Factur-X **BASIC WL** ("without lines") document.
pub const ZUGFERD_BASIC_WL_ID: &str = "urn:factur-x.eu:1p0:basicwl";

/// BT-24 of a ZUGFeRD 2.1+ / Factur-X **BASIC** document.
pub const ZUGFERD_BASIC_ID: &str = "urn:cen.eu:en16931:2017#compliant#urn:factur-x.eu:1p0:basic";

/// BT-24 of a ZUGFeRD 2.1+ / Factur-X **EXTENDED** document.
pub const ZUGFERD_EXTENDED_ID: &str =
    "urn:cen.eu:en16931:2017#conformant#urn:factur-x.eu:1p0:extended";

/// ZUGFeRD / Factur-X **MINIMUM** — a booking aid, not an EN 16931 invoice.
///
/// No lines, and only BT-109, BT-110, BT-112 and BT-115 in the summation, so it
/// runs only `BR-CO-15`; [`Profile::left_out`] names the rest and
/// [`Profile::is_en16931_invoice`] is false.
pub static ZUGFERD_MINIMUM: Profile = Profile {
    id: "ZUGFeRD MINIMUM (totals only)",
    slug: "zugferd-minimum",
    aliases: &[],
    edition: crate::Edition::En2017A1,
    specification_id: ZUGFERD_MINIMUM_ID,
    artefacts: &[CEN],
    underlying: &[],
    restrictions: &[],
    extra_rules: &[],
    core: Core::Only(&["BR-CO-15"]),
    extensions: &[],
    levels: &[],
};

/// ZUGFeRD / Factur-X **BASIC WL** — not an EN 16931 invoice.
///
/// No BG-25, so it runs the totals rules except those that sum over lines
/// (such as `BR-CO-10`); [`Profile::left_out`] names what does not run.
pub static ZUGFERD_BASIC_WL: Profile = Profile {
    id: "ZUGFeRD BASIC WL (totals only)",
    slug: "zugferd-basic-wl",
    aliases: &[],
    edition: crate::Edition::En2017A1,
    specification_id: ZUGFERD_BASIC_WL_ID,
    artefacts: &[CEN],
    underlying: &[],
    restrictions: &[],
    extra_rules: &[],
    core: Core::Only(&[
        "BR-CO-11", "BR-CO-12", "BR-CO-13", "BR-CO-14", "BR-CO-15", "BR-CO-16",
    ]),
    extensions: &[],
    levels: &[],
};

/// ZUGFeRD / Factur-X **BASIC** — an EN 16931 invoice (`#compliant#`), checked
/// against the core rules.
///
/// FeRD's own BASIC restrictions are not shipped. In [`ZUGFERD`], not [`ALL`].
pub static ZUGFERD_BASIC: Profile = Profile {
    id: "ZUGFeRD BASIC (EN 16931 core rules)",
    slug: "zugferd-basic",
    aliases: &[],
    edition: crate::Edition::En2017A1,
    specification_id: ZUGFERD_BASIC_ID,
    artefacts: &[CEN],
    underlying: &[],
    restrictions: &[],
    extra_rules: &[],
    core: Core::All,
    extensions: &[],
    levels: &[],
};

/// ZUGFeRD / Factur-X **EXTENDED** — an EN 16931 *extension* (`#conformant#`),
/// checked against the core rules.
///
/// Additions beyond the core are read where the model has a term (the
/// logistics service charge counts as a document-level charge, as in FeRD's
/// `BR-FXEXT-CO-12`) and reported as unmapped otherwise. FeRD's EXTENDED rules
/// are not shipped. In [`ZUGFERD`], not [`ALL`].
pub static ZUGFERD_EXTENDED: Profile = Profile {
    id: "ZUGFeRD EXTENDED (EN 16931 core rules)",
    slug: "zugferd-extended",
    aliases: &[],
    edition: crate::Edition::En2017A1,
    specification_id: ZUGFERD_EXTENDED_ID,
    artefacts: &[CEN],
    underlying: &[],
    restrictions: &[],
    extra_rules: &[],
    core: Core::All,
    extensions: &[],
    levels: &[],
};

/// The four ZUGFeRD / Factur-X profiles [`classify`] answers beyond [`ALL`]:
/// MINIMUM and BASIC WL (totals only, not invoices), BASIC and EXTENDED (the
/// core rules). The ZUGFeRD EN 16931 profile *is* [`EN16931`]: its BT-24 is
/// CEN's own.
pub static ZUGFERD: &[&Profile] = &[
    &ZUGFERD_MINIMUM,
    &ZUGFERD_BASIC_WL,
    &ZUGFERD_BASIC,
    &ZUGFERD_EXTENDED,
];

/// The other published spellings of the same four profiles.
///
/// ZUGFeRD 2.0 identifiers, and Factur-X 1.0.0's `:`-separated BASIC and
/// EXTENDED identifiers.
static ZUGFERD_ALIASES: &[(&str, &Profile)] = &[
    ("urn:zugferd.de:2p0:minimum", &ZUGFERD_MINIMUM),
    ("urn:zugferd.de:2p0:basicwl", &ZUGFERD_BASIC_WL),
    (
        "urn:cen.eu:en16931:2017#compliant#urn:zugferd.de:2p0:basic",
        &ZUGFERD_BASIC,
    ),
    (
        "urn:cen.eu:en16931:2017:compliant:factur-x.eu:1p0:basic",
        &ZUGFERD_BASIC,
    ),
    (
        "urn:cen.eu:en16931:2017#conformant#urn:zugferd.de:2p0:extended",
        &ZUGFERD_EXTENDED,
    ),
    (
        "urn:cen.eu:en16931:2017:conformant:factur-x.eu:1p0:extended",
        &ZUGFERD_EXTENDED,
    ),
];

// ── Classification ────────────────────────────────────────────────────────────

/// BT-24 values that name a known specification for another process —
/// documents this crate recognises and has no profile for, because they are not
/// the billing process its profiles describe.
static WRONG_PROCESS: &[&str] = &[
    // Peppol BIS Self-Billing 3.0.
    "urn:cen.eu:en16931:2017#compliant#urn:fdc:peppol.eu:2017:poacc:selfbilling:3.0",
];

/// What a document's BT-24 declares, classified (§7.6): the rule set the
/// sender claims, as KoSIT's scenario match.
///
/// `Known` serialises as the profile's [`slug`](Profile::slug).
#[derive(Debug, Clone)]
pub enum Declaration {
    /// BT-24 names a profile this build ships — one of [`ALL`], or one of the
    /// four [`ZUGFERD`] profiles.
    Known(&'static Profile),
    /// BT-24 is absent, or names nothing this build ships: a foreign CIUS, an
    /// obsolete identifier (an earlier XRechnung), a typo.
    ///
    /// A receiver following KoSIT rejects such a document: no profile matches.
    Unknown(Option<String>),
    /// BT-24 names a known specification for another business process —
    /// Peppol Self-Billing.
    WrongProcess(String),
}

/// Classify a BT-24 value.
///
/// Exact match after trimming whitespace, as KoSIT matches scenarios: an
/// identifier with an unknown suffix or an old spelling is
/// [`Declaration::Unknown`].
///
/// ```
/// use en16931::profiles::{self, Declaration};
///
/// let xr = profiles::classify(Some(profiles::XRECHNUNG.specification_id));
/// assert_eq!(xr, Declaration::Known(&profiles::XRECHNUNG));
///
/// let old = "urn:cen.eu:en16931:2017#compliant#urn:xoev-de:kosit:standard:xrechnung_3.0";
/// assert!(matches!(profiles::classify(Some(old)), Declaration::Unknown(Some(_))));
/// assert_eq!(profiles::classify(None), Declaration::Unknown(None));
/// ```
#[must_use]
pub fn classify(bt24: Option<&str>) -> Declaration {
    let Some(id) = bt24.map(str::trim).filter(|s| !s.is_empty()) else {
        return Declaration::Unknown(None);
    };
    if let Some(p) = ALL
        .iter()
        .chain(ZUGFERD)
        .copied()
        .find(|p| p.specification_id == id)
    {
        return Declaration::Known(p);
    }
    if let Some((_, p)) = ZUGFERD_ALIASES.iter().find(|(alias, _)| *alias == id) {
        return Declaration::Known(p);
    }
    if WRONG_PROCESS.contains(&id) {
        return Declaration::WrongProcess(id.to_owned());
    }
    Declaration::Unknown(Some(id.to_owned()))
}

/// Profiles compare by identity: every profile is a distinct `static`.
impl PartialEq for Declaration {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Known(a), Self::Known(b)) => core::ptr::eq(*a, *b),
            (Self::Unknown(a), Self::Unknown(b)) => a == b,
            (Self::WrongProcess(a), Self::WrongProcess(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for Declaration {}

impl Declaration {
    /// The profile, when one is known.
    #[must_use]
    pub fn profile(&self) -> Option<&'static Profile> {
        match self {
            Self::Known(p) => Some(p),
            _ => None,
        }
    }
}

impl core::fmt::Display for Declaration {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Known(p) => write!(f, "{}", p.id),
            Self::Unknown(None) => f.write_str("no specification identifier (BT-24)"),
            Self::Unknown(Some(id)) => write!(f, "unknown specification identifier {id:?}"),
            Self::WrongProcess(id) => write!(f, "{id:?} is the identifier of another process"),
        }
    }
}

#[cfg(feature = "serde")]
mod declaration_serde {
    use super::{ALL, Declaration, ZUGFERD};

    #[derive(serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    enum Wire {
        Known(String),
        Unknown(Option<String>),
        WrongProcess(String),
    }

    impl serde::Serialize for Declaration {
        fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            match self {
                Self::Known(p) => Wire::Known(p.slug.to_owned()),
                Self::Unknown(id) => Wire::Unknown(id.clone()),
                Self::WrongProcess(id) => Wire::WrongProcess(id.clone()),
            }
            .serialize(s)
        }
    }

    impl<'de> serde::Deserialize<'de> for Declaration {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            Ok(match Wire::deserialize(d)? {
                Wire::Known(slug) => Self::Known(
                    ALL.iter()
                        .chain(ZUGFERD)
                        .copied()
                        .find(|p| p.slug == slug)
                        .ok_or_else(|| {
                            serde::de::Error::custom(format!(
                                "profile {slug:?} is not one this build ships"
                            ))
                        })?,
                ),
                Wire::Unknown(id) => Self::Unknown(id),
                Wire::WrongProcess(id) => Self::WrongProcess(id),
            })
        }
    }
}

/// A profile name that names no profile in [`ALL`] or [`ZUGFERD`].
///
/// Its `Display` lists every slug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownProfile {
    /// The name as given.
    pub name: String,
}

impl core::fmt::Display for UnknownProfile {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "unknown profile {:?}; known: ", self.name)?;
        for (i, p) in ALL.iter().chain(ZUGFERD).enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            f.write_str(p.slug)?;
        }
        f.write_str(" (or a profile's full name, an alias, or its BT-24 identifier)")
    }
}

impl core::error::Error for UnknownProfile {}

/// Parse a profile from the name a person or a configuration file writes.
///
/// Accepts the [`slug`](Profile::slug) (`xrechnung`), the [`id`](Profile::id)
/// (`XRechnung 3.0`), an [alias](Profile::aliases) (`core`, `bis3`) and the
/// full BT-24 identifier. Slug, id and alias match case-insensitively; the
/// identifier matches exactly. Searches [`ALL`] and [`ZUGFERD`]
/// (`zugferd-basic`, …), whose other published BT-24 spellings parse too.
///
/// ```
/// use en16931::Profile;
/// use en16931::profiles;
///
/// let by_slug: &Profile = "xrechnung".parse()?;
/// let by_id: &Profile = "XRechnung 3.0".parse()?;
/// let by_bt24: &Profile = by_slug.specification_id.parse()?;
/// assert!(std::ptr::eq(by_slug, by_id) && std::ptr::eq(by_id, by_bt24));
/// assert!(std::ptr::eq("core".parse::<&Profile>()?, &profiles::EN16931));
///
/// let basic: &Profile = "zugferd-basic".parse()?;
/// assert!(std::ptr::eq(basic, &profiles::ZUGFERD_BASIC));
///
/// let err = "xrechung".parse::<&Profile>().unwrap_err();
/// assert!(err.to_string().contains("known: en16931, xrechnung"), "{err}");
/// # Ok::<(), profiles::UnknownProfile>(())
/// ```
impl core::str::FromStr for &'static Profile {
    type Err = UnknownProfile;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        let name = name.trim();
        ALL.iter()
            .chain(ZUGFERD)
            .copied()
            .find(|p| {
                p.slug.eq_ignore_ascii_case(name)
                    || p.id.eq_ignore_ascii_case(name)
                    || p.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
                    || p.specification_id == name
            })
            .or_else(|| {
                ZUGFERD_ALIASES
                    .iter()
                    .find(|(id, _)| *id == name)
                    .map(|(_, p)| *p)
            })
            .ok_or_else(|| UnknownProfile {
                name: name.to_owned(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Slugs are unique, shell-safe, and parse back to their profile.
    #[test]
    fn every_slug_is_unique_and_shell_safe() {
        let mut seen = std::collections::BTreeSet::new();
        for p in ALL.iter().chain(ZUGFERD) {
            assert!(
                p.slug
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
                "{}: {:?} is not lower-case ASCII and hyphens",
                p.id,
                p.slug
            );
            assert!(seen.insert(p.slug), "{:?} is used twice", p.slug);
            for spelling in [p.slug, p.id, p.specification_id]
                .into_iter()
                .chain(p.aliases.iter().copied())
            {
                let found: &Profile = spelling.parse().expect(spelling);
                assert!(std::ptr::eq(found, *p), "{spelling:?}");
                // No spelling of one profile is a spelling of another.
                let owners = ALL
                    .iter()
                    .chain(ZUGFERD)
                    .filter(|q| {
                        q.slug.eq_ignore_ascii_case(spelling)
                            || q.id.eq_ignore_ascii_case(spelling)
                            || q.aliases.iter().any(|a| a.eq_ignore_ascii_case(spelling))
                            || q.specification_id == spelling
                    })
                    .count();
                assert_eq!(owners, 1, "{spelling:?} names {owners} profiles");
            }
        }
        let err = "no such profile".parse::<&Profile>().unwrap_err();
        for p in ALL.iter().chain(ZUGFERD) {
            assert!(err.to_string().contains(p.slug), "{err}");
        }
    }

    #[test]
    fn profiles_are_siblings_not_a_scale() {
        // XRechnung permits BT-3 = 389 and Peppol does not.
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
        let known = |id: &str| classify(Some(id)).profile().map(|p| p.id);
        assert_eq!(known(XRECHNUNG.specification_id), Some("XRechnung 3.0"));
        // Each KoSIT variant has its own identifier and selects its own rules.
        assert_eq!(known(XR_SPEC_IDS[1]), Some("XRechnung 3.0 Extension"));
        assert_eq!(known(XR_SPEC_IDS[2]), Some("XRechnung 3.0 CVD"));
        assert_eq!(known(EN16931.specification_id), Some("EN 16931"));
        // An unknown KoSIT variant is not the CIUS it embeds.
        let variant = format!("{}#conformant#urn:nope", XR_SPEC_IDS[0]);
        assert_eq!(
            classify(Some(&variant)),
            Declaration::Unknown(Some(variant.clone()))
        );
        assert_eq!(
            classify(Some("urn:nonsense")),
            Declaration::Unknown(Some("urn:nonsense".to_owned()))
        );
        // Whitespace is a transport artefact, not a different claim.
        assert_eq!(
            known(&format!("  {}\n", XRECHNUNG.specification_id)),
            Some("XRechnung 3.0")
        );
        assert_eq!(classify(Some("   ")), Declaration::Unknown(None));
    }

    #[test]
    fn the_totals_only_profiles_are_known_and_not_invoices() {
        for p in [&ZUGFERD_MINIMUM, &ZUGFERD_BASIC_WL] {
            assert_eq!(classify(Some(p.specification_id)), Declaration::Known(p));
            assert!(!p.is_en16931_invoice(), "{}", p.id);
            assert!(!p.left_out().is_empty(), "{}", p.id);
            assert!(
                p.slug
                    .parse::<&Profile>()
                    .is_ok_and(|found| core::ptr::eq(found, p)),
                "{} parses by its slug",
                p.id
            );
        }
        for p in ALL {
            assert!(p.is_en16931_invoice(), "{}", p.id);
            assert!(p.left_out().is_empty(), "{}", p.id);
        }
        for (alias, p) in ZUGFERD_ALIASES {
            assert_eq!(classify(Some(alias)), Declaration::Known(p));
            assert!(
                alias
                    .parse::<&Profile>()
                    .is_ok_and(|found| core::ptr::eq(found, *p))
            );
        }
        assert_eq!(
            ZUGFERD_MINIMUM.check_ids().collect::<Vec<_>>(),
            ["BR-CO-15"]
        );
        let report = ZUGFERD_MINIMUM.validate(&crate::Invoice::default());
        assert_eq!(report.rules_checked(), 1);
    }

    /// ZUGFeRD BASIC and EXTENDED are lawful EN 16931 invoices: known, and
    /// checked against every core rule.
    #[test]
    fn zugferd_basic_and_extended_are_known_and_run_the_core() {
        for p in [&ZUGFERD_BASIC, &ZUGFERD_EXTENDED] {
            assert_eq!(classify(Some(p.specification_id)), Declaration::Known(p));
            assert!(p.is_en16931_invoice(), "{}", p.id);
            assert!(p.left_out().is_empty(), "{}", p.id);
            assert_eq!(p.check_ids().count(), EN16931.check_ids().count());
            assert!(
                p.slug
                    .parse::<&Profile>()
                    .is_ok_and(|found| core::ptr::eq(found, p)),
                "{} parses by its slug",
                p.id
            );
        }
        assert_eq!(ZUGFERD.len(), 4);
        // A mixed spelling no specification published stays unknown.
        let mixed = "urn:cen.eu:en16931:2017:compliant:factur-x.eu:1p0:extended";
        assert_eq!(
            classify(Some(mixed)),
            Declaration::Unknown(Some(mixed.to_owned()))
        );
    }

    #[test]
    fn self_billing_is_another_process() {
        let id = WRONG_PROCESS[0];
        assert_eq!(classify(Some(id)), Declaration::WrongProcess(id.to_owned()));
    }

    #[test]
    fn every_profile_states_what_4_4_2_requires() {
        for p in ALL {
            assert!(!p.id.is_empty());
            assert!(!p.specification_id.is_empty());
            // A CIUS shall state its underlying specifications; only the core
            // model itself has none.
            assert_eq!(p.underlying.is_empty(), p.id == "EN 16931", "{}", p.id);
            // Conformance follows from whether any level override relaxes a rule.
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

    /// The profiles that are not conformant CIUSes, asserted exactly.
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

    /// A relaxed rule is reported at the profile's level, not dropped.
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
