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
//! invoice. If the format crates need them, they define them.

use crate::validation::profile::{Profile, ProfileMarker, Restriction, Underlies, terms as t};
use crate::validation::rules::peppol;

// ── EN 16931 core ─────────────────────────────────────────────────────────────

/// The core invoice model, with no usage specification on top.
pub static EN16931: Profile = Profile {
    id: "EN 16931",
    edition: crate::Edition::En2017A1,
    specification_id: "urn:cen.eu:en16931:2017",
    underlying: &[],
    restrictions: &[],
    extra_rules: &[],
    extensions: &[],
    suppressed: &[],
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
    // XRechnung 3.0 **withdrew** BR-DE-29 because Peppol's
    // `PEPPOL-EN16931-R061` covers direct-debit mandate references. A profile
    // model of "core plus its own rules" cannot express a withdrawal, and would
    // either double-report the requirement or lose it.
    suppressed: &[],
};

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

// §4.4.4: a conformant CIUS only restricts, so anything valid under XRechnung is
// valid under the core model. This impl is the type-level statement of that.
impl Underlies<XRechnung> for En16931 {}

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
    suppressed: &[],
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
/// is what a validator has to reproduce: `BR-CL-13` is **suppressed** and
/// `BR-TMP-CVD-01` put in its place. The alternative — reporting `BR-CL-13` on
/// every conforming CVD invoice — would be a false positive on a document KoSIT
/// accepts.
///
/// Everything else XRechnung requires, this requires too — [`Underlies`] states
/// that at the type level.
pub static XRECHNUNG_CVD: Profile = Profile {
    id: "XRechnung 3.0 CVD",
    edition: crate::Edition::En2017A1,
    specification_id: XR_SPEC_IDS[2],
    underlying: &["EN 16931", "XRechnung 3.0"],
    restrictions: XRECHNUNG.restrictions,
    extra_rules: XR_CVD_EXTRA,
    extensions: &[],
    // See above: CVD widens UNTDID 7143 by one value, so the core code-list rule
    // is replaced by `BR-TMP-CVD-01` rather than layered on top of.
    suppressed: &["BR-CL-13"],
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
// The comment is false. CVD **suppresses `BR-CL-13`**, because it marks vehicle
// lines with a `BT-158` scheme that is not in UNTDID 7143 — so a conforming CVD
// invoice violates the core model, exactly as `XRECHNUNG_CVD`'s own
// documentation says.
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
/// `BR-CO-16` is [`Profile::suppressed`] rather than layered over, because with
/// a third-party payment present the two equations disagree by exactly that sum
/// and reporting both would be reporting the same fact twice, once wrongly.
pub static XRECHNUNG_EXTENSION: Profile = Profile {
    id: "XRechnung 3.0 Extension",
    edition: crate::Edition::En2017A1,
    specification_id: XR_SPEC_IDS[1],
    underlying: &["XRechnung 3.0"],
    restrictions: XRECHNUNG.restrictions,
    extra_rules: XR_EXT_EXTRA,
    extensions: &[
        crate::extensions::SUB_INVOICE_LINES,
        crate::extensions::THIRD_PARTY_PAYMENTS,
    ],
    // `BR-DEX-09` replaces it, and `BR-DEX-01` / `-04` … `-08` replace the
    // narrower code-list rules they widen.
    suppressed: &[
        "BR-CO-16",
        "PEPPOL-EN16931-CL001",
        "BR-CL-10",
        "BR-CL-11",
        "BR-CL-25",
        "BR-CL-26",
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
            // Conformance is a *property*, not a constant. Suppressing a core
            // rule is exactly what §4.4.2 forbids, and two shipped profiles do
            // it deliberately because they are not CIUSes.
            assert_eq!(
                p.is_conformant_cius(),
                p.suppressed.is_empty(),
                "{} — conformance must follow suppression",
                p.id
            );
        }
    }

    /// The profiles that are **not** conformant CIUSes, named.
    ///
    /// Asserted exactly, so neither direction can drift silently: adding a
    /// suppression to a CIUS fails here, and so does removing one without
    /// revisiting whether widening out of it is now sound.
    #[test]
    fn exactly_two_profiles_are_not_conformant_ciuses() {
        let not: Vec<&str> = ALL
            .iter()
            .filter(|p| !p.is_conformant_cius())
            .map(|p| p.id)
            .collect();
        assert_eq!(not, ["XRechnung 3.0 CVD", "XRechnung 3.0 Extension"]);
    }
}
