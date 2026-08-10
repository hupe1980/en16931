//! Checking a code **at the point it is written**, not at validation time.
//!
//! # Why a second place to check the same thing
//!
//! [`crate::validate`] already reports every bad code, and it reports them with
//! a business-term path. That is the right answer for a *document* — but it is
//! the wrong moment for a *mapping*.
//!
//! A field mapper writing `Code::new("9958")` is looking at their own ERP's
//! column at that instant. The `BR-CL-25` finding arrives minutes later, in a
//! report about an assembled invoice, by which time the reader is no longer
//! looking at the line of code that produced it. Worse, the finding can only
//! say *"not in the CEF EAS code list"* — it does not know that `9958` was the
//! German Leitweg-ID scheme, that OpenPeppol withdrew it on 31 July 2023, and
//! that the answer is [`0204`](WITHDRAWN).
//!
//! So: the same lists, guarded, returning a [`CodeError`] that says what to do.
//!
//! ```
//! use en16931::codes::guard;
//!
//! // The typo a mapper actually makes.
//! let err = guard::unit("kwh").unwrap_err();
//! assert!(err.to_string().contains("did you mean \"KWH\""));
//!
//! // The code a German integrator actually reaches for.
//! let err = guard::eas("9958").unwrap_err();
//! assert!(err.to_string().contains("0204"));
//!
//! // And the ones that are simply right.
//! assert_eq!(guard::eas("0204")?.as_str(), "0204");
//! assert_eq!(guard::unit("KWH")?.as_str(), "KWH");
//! # Ok::<(), en16931::codes::guard::CodeError>(())
//! ```
//!
//! # This is a convenience, not a second source of truth
//!
//! Every function here checks the *same* generated list the corresponding rule
//! checks, and `guarded_lists_agree_with_their_rules` asserts the pairing. A
//! caller who skips this layer entirely loses nothing but the earlier message —
//! validation still reports the code. That ordering is deliberate: a crate that
//! made bad codes unrepresentable could not load an invalid document in order
//! to explain it, which is the crate's whole design.

use core::fmt;

use super::{contains, generated as lists};
use crate::invoice::Code;

// ── Withdrawn codes ───────────────────────────────────────────────────────────

/// A code that **used to be valid** and has since been withdrawn.
///
/// # Why this table exists at all
///
/// "Not in the list" is an unhelpful thing to tell someone holding a value that
/// their previous integration accepted, that appears in their counterparty's
/// documentation, and that is still printed on a form somewhere. The useful
/// message names the successor.
///
/// Twelve EAS schemes have left the CEF list since CEN artefact
/// `validation-1.2.0` — nine of them in 2023 alone — and every one was in
/// production somewhere. The membership half of this table is **measured**:
/// `withdrawn_codes_are_really_gone` asserts each entry is absent from the
/// current list and each named successor is present, so a code CEN reinstates
/// fails the build rather than producing a wrong hint.
///
/// | Code | Was | Withdrawn | Use |
/// |---|---|---|---|
/// | `9901` | DK:CPR | 2023-11-30 | `0096` |
/// | `9902` | DK:CVR | 2023-11-30 | `0184` |
/// | `9904` | DK:SE | 2023-11-30 | `0198` |
/// | `9905` | DK:VANS | 2023-11-30 | — |
/// | `9906` | IT:VAT | 2023-07-31 | `0211` |
/// | `9907` | IT:CF | 2023-07-31 | `0210` |
/// | `9917` | IS:KT | — | `0196` |
/// | `9921` | IT:IPA | 2023-11-30 | `0201` |
/// | `9954` | NL:OINO | — | `0190` |
/// | `9955` | SE:VAT | 2023-07-31 | `0007` |
/// | `9956` | BE:CBE | 2023-12-31 | `0208` |
/// | `9958` | DE:LID | 2023-07-31 | `0204` |
pub struct Withdrawn {
    /// The code that no longer belongs to the list.
    pub code: &'static str,
    /// What it used to mean.
    pub was: &'static str,
    /// The code to use instead, when the authority names one.
    ///
    /// `None` for `9905`, which OpenPeppol removed without a successor.
    pub use_instead: Option<&'static str>,
    /// The extra sentence worth saying, if any.
    pub note: Option<&'static str>,
}

/// The EAS schemes withdrawn since CEN artefact `validation-1.2.0`.
///
/// Sourced from the OpenPeppol eDEC code-list change log; the *absence* of each
/// from [`lists::EAS_SCHEMES`] is asserted by this module's tests against the
/// pinned artefacts, so the two cannot disagree silently.
pub static WITHDRAWN: &[Withdrawn] = &[
    Withdrawn {
        code: "9901",
        was: "DK:CPR — Danish Ministry of the Interior and Health",
        use_instead: Some("0096"),
        note: None,
    },
    Withdrawn {
        code: "9902",
        was: "DK:CVR — the Danish Commerce and Companies Agency",
        use_instead: Some("0184"),
        note: None,
    },
    Withdrawn {
        code: "9904",
        was: "DK:SE — Danish Ministry of Taxation",
        use_instead: Some("0198"),
        note: None,
    },
    Withdrawn {
        code: "9905",
        was: "DK:VANS — Danish VANS providers",
        use_instead: None,
        note: Some(
            "withdrawn on 2023-11-30 with no successor; use the party's own registry scheme",
        ),
    },
    Withdrawn {
        code: "9906",
        was: "IT:VAT — Italian VAT number",
        use_instead: Some("0211"),
        note: None,
    },
    Withdrawn {
        code: "9907",
        was: "IT:CF — Italian codice fiscale",
        use_instead: Some("0210"),
        note: None,
    },
    Withdrawn {
        code: "9917",
        was: "IS:KT — Icelandic kennitala",
        use_instead: Some("0196"),
        note: None,
    },
    Withdrawn {
        code: "9921",
        was: "IT:IPA — Indice delle Pubbliche Amministrazioni",
        use_instead: Some("0201"),
        note: None,
    },
    Withdrawn {
        code: "9954",
        was: "NL:OINO — Dutch Overheidsidentificatienummer",
        use_instead: Some("0190"),
        note: None,
    },
    Withdrawn {
        code: "9955",
        was: "SE:VAT — Swedish VAT number",
        use_instead: Some("0007"),
        note: None,
    },
    Withdrawn {
        code: "9956",
        was: "BE:CBE — Belgian Crossroads Bank for Enterprises",
        use_instead: Some("0208"),
        note: None,
    },
    Withdrawn {
        code: "9958",
        was: "DE:LID — the Peppol Leitweg-ID scheme",
        use_instead: Some("0204"),
        note: Some(
            "the Leitweg-ID itself belongs in BT-10 (buyer reference) under XRechnung's BR-DE-15; \
             0204 is the scheme for addressing that authority as a Peppol participant",
        ),
    },
];

/// The withdrawal record for `code`, if there is one.
#[must_use]
pub fn withdrawn(code: &str) -> Option<&'static Withdrawn> {
    WITHDRAWN.iter().find(|w| w.code == code)
}

// ── The error ─────────────────────────────────────────────────────────────────

/// A value that is not in the code list its business term draws from.
///
/// Carries the rule that *would* have reported it, so the message and a later
/// [`crate::Finding`] name the same thing and a reader can look it up once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeError {
    /// The business terms the list serves — `"BT-34 / BT-49"`.
    pub terms: &'static str,
    /// What the list is — `"the CEF EAS code list"`.
    pub list: &'static str,
    /// The rule that reports this at validation time — `"BR-CL-25"`.
    pub rule: &'static str,
    /// The offered value.
    pub value: String,
    /// What to do instead, when this crate can tell. See [`CodeList::advice`].
    pub hint: Option<String>,
}

impl fmt::Display for CodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} is not in {} ({}, {})",
            self.value, self.list, self.terms, self.rule
        )?;
        if let Some(h) = &self.hint {
            write!(f, " — {h}")?;
        }
        Ok(())
    }
}

impl core::error::Error for CodeError {}

// ── The lists, as guarded values ──────────────────────────────────────────────

/// One code list, together with what it is for.
///
/// Public so a caller can guard a list this module has no named function for,
/// and so [`crate::validation`] can borrow the same [`CodeList::advice`] without a second
/// copy of the tables.
pub struct CodeList {
    /// The business terms it serves.
    pub terms: &'static str,
    /// A human name for it.
    pub name: &'static str,
    /// The rule that checks it.
    pub rule: &'static str,
    /// The values, sorted.
    pub values: &'static [&'static str],
    /// Whether [`WITHDRAWN`] describes this list. Only the EAS list does.
    pub tracks_withdrawals: bool,
}

impl CodeList {
    /// Whether `value` is in the list — a binary search, as [`super::contains`].
    #[must_use]
    pub fn accepts(&self, value: &str) -> bool {
        contains(self.values, value)
    }

    /// Check `value`, returning it as a [`Code`] or saying what is wrong.
    ///
    /// # Errors
    /// [`CodeError`] when the value is not in the list, carrying a [`hint`]
    /// where one can be derived.
    ///
    /// [`hint`]: CodeError::hint
    pub fn check(&self, value: &str) -> Result<Code, CodeError> {
        if self.accepts(value) {
            return Ok(Code::new(value));
        }
        Err(CodeError {
            terms: self.terms,
            list: self.name,
            rule: self.rule,
            value: value.to_owned(),
            hint: self.advice(value),
        })
    }

    /// The most useful sentence this crate can produce about a rejected value.
    ///
    /// Three sources, in order of confidence:
    ///
    /// 1. **Withdrawn.** The value was in the list and the authority named a
    ///    successor. This is the only case where the advice is authoritative
    ///    rather than inferred.
    /// 2. **Whitespace.** `" KWH"` is one `trim()` from correct, and §6.5.8's
    ///    *"entered exactly as shown"* means the crate must not trim it for you.
    /// 3. **Case.** `"kwh"` for `KWH`, `"eur"` for `EUR`. The single most common
    ///    mapping bug, and unambiguous because no code list here contains two
    ///    values differing only in case — asserted by `case_folding_is_unambiguous`.
    ///
    /// `None` when none of the three applies. **Deliberately no edit distance**:
    /// `"KWO"` is one character from `KWH` and also from `KWT`, and a validator
    /// that guesses between kilowatt-hours and kilowatts is worse than one that
    /// says nothing.
    #[must_use]
    pub fn advice(&self, value: &str) -> Option<String> {
        // Nothing to advise about a value that is already correct. `check`
        // never reaches here with one, but this is public and a caller asking
        // "is there anything to say about X?" deserves `None` for a good X.
        if self.accepts(value) {
            return None;
        }
        if self.tracks_withdrawals
            && let Some(w) = withdrawn(value)
        {
            let mut s = format!("{value} was {}, and has been withdrawn", w.was);
            if let Some(r) = w.use_instead {
                s.push_str(&format!("; use {r} instead"));
            }
            if let Some(n) = w.note {
                s.push_str(&format!(" ({n})"));
            }
            return Some(s);
        }
        let trimmed = value.trim();
        if trimmed != value && self.accepts(trimmed) {
            return Some(format!(
                "{trimmed:?} is valid — this value has surrounding whitespace, and \
                 EN 16931-1 §6.5.8 requires codes \"entered exactly as shown\""
            ));
        }
        self.values
            .iter()
            .find(|c| c.eq_ignore_ascii_case(trimmed))
            .map(|c| format!("did you mean {c:?}? Code lists are case-sensitive"))
    }
}

/// Declare the guarded lists and their one-line constructors together, so a new
/// list cannot get a function without its metadata or the reverse.
macro_rules! guarded {
    ($(
        $(#[$attr:meta])*
        $konst:ident / $func:ident = $values:ident,
        terms: $terms:literal, list: $name:literal, rule: $rule:literal
        $(, withdrawals: $w:literal)?;
    )*) => {
        $(
            #[doc = concat!("The ", $name, " — ", $terms, ", checked by `", $rule, "`.")]
            pub static $konst: CodeList = CodeList {
                terms: $terms,
                name: $name,
                rule: $rule,
                values: lists::$values,
                tracks_withdrawals: false $(|| $w)?,
            };

            $(#[$attr])*
            ///
            /// # Errors
            #[doc = concat!("[`CodeError`] when the value is not in [`", stringify!($konst), "`].")]
            pub fn $func(value: &str) -> Result<Code, CodeError> {
                $konst.check(value)
            }
        )*

        /// Every guarded list, for tests and for listing them.
        pub static ALL: &[&CodeList] = &[$(&$konst),*];
    };
}

guarded! {
    /// BT-34 / BT-49 — the electronic address scheme.
    ///
    /// The list German integrators most often get wrong: `9958` looks right,
    /// was right until 2023-07-31, and is not.
    EAS / eas = EAS_SCHEMES,
        terms: "BT-34 / BT-49", list: "the CEF EAS code list", rule: "BR-CL-25",
        withdrawals: true;

    /// BT-130 / BT-150 — the unit of measure.
    UNIT / unit = UNIT_CODES,
        terms: "BT-130 / BT-150", list: "UN/ECE Recommendation 20 with the Rec 21 extension",
        rule: "BR-CL-23";

    /// BT-5 / BT-6 — the currency.
    CURRENCY / currency = CURRENCY_CODES,
        terms: "BT-5 / BT-6", list: "ISO 4217 alpha-3", rule: "BR-CL-04";

    /// BT-40 / BT-55 / BT-69 / BT-80 — the country.
    COUNTRY / country = COUNTRY_CODES,
        terms: "BT-40 / BT-55 / BT-69 / BT-80", list: "ISO 3166-1 alpha-2", rule: "BR-CL-14";

    /// BT-3 on an **invoice**. A credit note draws from [`CREDIT_NOTE_TYPE`].
    INVOICE_TYPE / invoice_type = INVOICE_TYPE_CODES,
        terms: "BT-3 (invoice)", list: "UNTDID 1001, invoice subset", rule: "BR-CL-01";

    /// BT-3 on a **credit note**.
    CREDIT_NOTE_TYPE / credit_note_type = CREDIT_NOTE_TYPE_CODES,
        terms: "BT-3 (credit note)", list: "UNTDID 1001, credit-note subset", rule: "BR-CL-01";

    /// BT-95 / BT-102 / BT-118 / BT-151 — the VAT category.
    ///
    /// Prefer [`crate::VatCategory::from_code`], which returns the *semantics*
    /// rather than a string. This exists for symmetry and for callers holding a
    /// code they have no branch for.
    VAT_CATEGORY / vat_category = VAT_CATEGORY_CODES,
        terms: "BT-95 / BT-102 / BT-118 / BT-151", list: "UNCL 5305", rule: "BR-CL-17";

    /// BT-81 — the payment means.
    PAYMENT_MEANS / payment_means = PAYMENT_MEANS_CODES,
        terms: "BT-81", list: "UNTDID 4461", rule: "BR-CL-16";

    /// BT-121 — the VAT exemption reason code.
    VATEX / vatex = VATEX_CODES,
        terms: "BT-121", list: "the CEF VATEX code list", rule: "PEPPOL-EN16931-CL002";

    /// BT-98 — the document level allowance reason.
    ALLOWANCE_REASON / allowance_reason = ALLOWANCE_REASON_CODES,
        terms: "BT-98 / BT-140", list: "UNCL 5189", rule: "BR-CL-19";

    /// BT-105 — the document level charge reason.
    CHARGE_REASON / charge_reason = CHARGE_REASON_CODES,
        terms: "BT-105 / BT-145", list: "UNCL 7161", rule: "BR-CL-20";

    /// BT-21 — the invoice note subject code.
    NOTE_SUBJECT / note_subject = NOTE_SUBJECT_CODES,
        terms: "BT-21", list: "UNCL 4451", rule: "BR-CL-08";

    /// BT-8 — the VAT point date code.
    VAT_POINT_DATE / vat_point_date = VAT_POINT_DATE_CODES,
        terms: "BT-8", list: "UNTDID 2005, the three EN 16931 values", rule: "BR-CL-05";

    /// BT-18-1 / BT-128-1 — the invoiced object identifier scheme.
    REFERENCE_QUALIFIER / reference_qualifier = REFERENCE_QUALIFIERS,
        terms: "BT-18-1 / BT-128-1", list: "UNTDID 1153", rule: "BR-CL-07";

    /// BT-29-1 / BT-46-1 / BT-71-1 / BT-157-1 — an ISO 6523 scheme.
    ICD / icd = ICD_SCHEMES,
        terms: "BT-29-1 / BT-46-1 / BT-71-1 / BT-157-1", list: "the ISO 6523 ICD list",
        rule: "BR-CL-10";

    /// BT-158-1 — the item classification scheme.
    ITEM_CLASSIFICATION / item_classification = ITEM_CLASSIFICATION_SCHEMES,
        terms: "BT-158-1", list: "UNTDID 7143", rule: "BR-CL-13";
}

// ── Value shapes, for the schemes that have one ───────────────────────────────

/// The EAS schemes whose **value** this crate can check, not merely the code.
///
/// Deliberately one entry. See [`eas_value`] for why the list is short and why
/// it is a list at all rather than a hidden branch.
pub const CHECKED_EAS_SCHEMES: &[&str] = &["0088"];

/// Whether [`eas_value`] can say anything about a value under `scheme`.
///
/// The honest half of a partial check. `eas_value` returning `Ok` means either
/// *"verified"* or *"not verifiable here"*, and those are very different claims
/// to build on — this is how a caller tells them apart.
#[must_use]
pub fn eas_value_is_checkable(scheme: &str) -> bool {
    CHECKED_EAS_SCHEMES.contains(&scheme)
}

/// Check an electronic address **value** against its EAS scheme's own format.
///
/// # Why this exists, and why it covers one scheme
///
/// [`eas`] validates the scheme code. Nothing validates the content — and
/// neither does `BR-CL-25`, which also only looks at the code. So
/// `Identifier::eas(x, "0088")` asserts *"x is a GS1 GLN"* and no layer anywhere
/// checks it. A downstream user put an eleven-digit BDEW Marktlokations-ID
/// through that call: it returned `Ok`, validation passed, and the document went
/// out claiming an eleven-digit German metering identifier was a thirteen-digit
/// GLN. Syntactically valid, semantically false, unresolvable at the receiver.
///
/// The EAS list has over a hundred schemes and most have no fixed public
/// format, so checking "the value" in general is not a job this crate can do
/// honestly. What it can do is the handful whose format is fixed, published and
/// **self-verifying** — where being wrong is detectable without a registry
/// lookup. Today that is GS1's GLN and nothing else:
///
/// | Scheme | Check |
/// |---|---|
/// | `0088` GLN | 13 digits, GS1 mod-10 check digit |
///
/// Schemes are added here only when their specification is in hand. Guessing a
/// format would produce the failure mode this whole module exists to prevent,
/// one level further in — a check that passes wrong values and is trusted
/// because it exists.
///
/// # Errors
/// [`CodeError`] when `scheme` is one of [`CHECKED_EAS_SCHEMES`] and `value`
/// does not have its shape. `Ok(())` for every other scheme — ask
/// [`eas_value_is_checkable`] whether that means anything.
pub fn eas_value(scheme: &str, value: &str) -> Result<(), CodeError> {
    let problem = match scheme {
        "0088" => gln_problem(value),
        _ => None,
    };
    match problem {
        None => Ok(()),
        Some(hint) => Err(CodeError {
            terms: EAS.terms,
            list: "the set of valid GS1 GLNs",
            rule: EAS.rule,
            value: value.to_owned(),
            hint: Some(hint),
        }),
    }
}

/// What is wrong with `value` as a GS1 GLN, if anything.
///
/// GLN is GS1's 13-digit key and shares GTIN-13's check digit: weights 1 and 3
/// alternating from the left over the first twelve digits, and the check digit
/// is whatever brings the total to a multiple of ten.
fn gln_problem(value: &str) -> Option<String> {
    if value.len() != 13 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Some(format!(
            "a GS1 GLN (scheme 0088) is exactly 13 digits, and this is {} character(s). \
             Identifiers that are not GLNs — a BDEW Marktlokations-ID, a customer number — \
             need their own EAS scheme, not 0088",
            value.chars().count()
        ));
    }
    let digits: Vec<u32> = value.bytes().map(|b| u32::from(b - b'0')).collect();
    let sum: u32 = digits[..12]
        .iter()
        .enumerate()
        .map(|(i, d)| d * if i % 2 == 0 { 1 } else { 3 })
        .sum();
    let expected = (10 - sum % 10) % 10;
    (digits[12] != expected).then(|| {
        format!(
            "the GS1 check digit is wrong: 13 digits, but {} ends in {} where the \
             first twelve require {expected}",
            value, digits[12]
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table's central claim: every entry really is gone from the list.
    ///
    /// If CEN reinstates one, this fails rather than the crate advising a
    /// migration away from a code that is valid again.
    #[test]
    fn withdrawn_codes_are_really_gone() {
        for w in WITHDRAWN {
            assert!(
                !EAS.accepts(w.code),
                "{} is in EAS_SCHEMES, so it is not withdrawn",
                w.code
            );
            if let Some(r) = w.use_instead {
                assert!(
                    EAS.accepts(r),
                    "{} advises {r}, which is not in EAS_SCHEMES",
                    w.code
                );
            }
        }
    }

    /// The papercut this whole module answers.
    #[test]
    fn the_leitweg_scheme_names_its_successor_and_bt_10() {
        let e = eas("9958").unwrap_err();
        let msg = e.to_string();
        assert!(msg.contains("BR-CL-25"), "{msg}");
        assert!(msg.contains("withdrawn"), "{msg}");
        assert!(msg.contains("use 0204 instead"), "{msg}");
        assert!(msg.contains("BT-10"), "{msg}");
        assert!(eas("0204").is_ok(), "the successor is accepted");
        assert!(eas("0088").is_ok(), "GLN, which is what a BDEW MP-ID uses");
    }

    #[test]
    fn case_and_whitespace_are_named_rather_than_silently_fixed() {
        let e = unit("kwh").unwrap_err();
        assert!(e.to_string().contains("did you mean \"KWH\""), "{e}");
        let e = unit(" KWH").unwrap_err();
        assert!(e.to_string().contains("whitespace"), "{e}");
        assert!(unit("KWH").is_ok());
    }

    /// Case folding is only a safe hint because no list is ambiguous under it.
    #[test]
    fn case_folding_is_unambiguous() {
        for list in ALL {
            let mut folded: Vec<String> = list.values.iter().map(|v| v.to_uppercase()).collect();
            folded.sort_unstable();
            let before = folded.len();
            folded.dedup();
            assert_eq!(
                before,
                folded.len(),
                "{} has two values differing only in case, so a case hint could mislead",
                list.name
            );
        }
    }

    /// No user-facing sentence carries a run of spaces.
    ///
    /// The same defect class the crate documents on [`crate::ATTRIBUTION`]: a
    /// string meant as one sentence ends up with the source indentation inside
    /// it, and every message built from it carries the gap. It recurred here —
    /// `gln_problem`'s too-short message shipped with two fourteen-space runs —
    /// so the property is now asserted for every rule text and for the advice
    /// this module generates, not just for the notice.
    #[test]
    fn no_rule_text_or_advice_contains_a_run_of_spaces() {
        for r in crate::validation::rules::all() {
            assert!(
                !r.text.contains("  "),
                "{}'s text contains a run of spaces: {:?}",
                r.id,
                r.text
            );
        }
        for msg in [
            gln_problem("12345").expect("too short"),
            gln_problem("4012345000008").expect("bad check digit"),
            EAS.advice("9958").expect("withdrawn"),
            UNIT.advice("kwh").expect("case"),
            UNIT.advice(" KWH").expect("whitespace"),
        ] {
            assert!(
                !msg.contains("  "),
                "advice contains a run of spaces: {msg:?}"
            );
        }
    }

    /// No advice at all is the right answer for a value nothing explains.
    #[test]
    fn nothing_is_guessed() {
        let e = unit("FURLONG").unwrap_err();
        assert_eq!(e.hint, None, "no edit distance, no guessing");
        // One character from `KWH` and from `KWT`. A validator that guessed
        // between kilowatt-hours and kilowatts would be worse than silent.
        assert!(!UNIT.accepts("KWQ"));
        assert_eq!(UNIT.advice("KWQ"), None);
        // And a value that is simply right gets nothing said about it.
        assert_eq!(UNIT.advice("KWH"), None);
    }

    /// Every guarded list points at the list its rule actually checks.
    #[test]
    fn guarded_lists_agree_with_their_rules() {
        assert_eq!(EAS.values, lists::EAS_SCHEMES);
        assert_eq!(UNIT.values, lists::UNIT_CODES);
        assert_eq!(VAT_CATEGORY.values, lists::VAT_CATEGORY_CODES);
        for list in ALL {
            assert!(!list.values.is_empty(), "{} is empty", list.name);
            assert!(
                list.rule.starts_with("BR-") || list.rule.starts_with("PEPPOL-"),
                "{} names {}, which is not a rule id",
                list.name,
                list.rule
            );
        }
    }

    #[test]
    fn the_two_bt_3_lists_are_guarded_separately() {
        // BR-CL-01 is a disjunction over the document kind, and `381` on an
        // invoice is invalid however plausible it looks.
        assert!(invoice_type("380").is_ok());
        assert!(invoice_type("381").is_err());
        assert!(credit_note_type("381").is_ok());
        assert!(credit_note_type("380").is_err());
    }
}
