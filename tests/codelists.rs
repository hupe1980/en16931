//! Round-trip the committed code lists against the CEN artefacts.
//!
//! # Why this test exists
//!
//! A Schematron `test` is a *program*, not a data structure. Two of the fourteen
//! `BR-CL-*` rules carry more than one code list in a single expression, for two
//! different reasons:
//!
//! - **`BR-CL-01`** is a disjunction over `self::` — 50 codes when the element is
//!   `cbc:InvoiceTypeCode`, 13 when it is `cbc:CreditNoteTypeCode`.
//! - **`BR-CL-10`** is the ISO 6523 list *plus* a contextual literal: `SEPA` is
//!   admissible, but only on a party identification under
//!   `cac:AccountingSupplierParty` or `cac:PayeeParty`.
//!
//! An extractor that takes the first `contains(…)` literal and stops is
//! confidently, precisely wrong on both. That mistake was made during the design
//! of this crate and produced an incorrect bug report against an upstream
//! project, so the defence is a test rather than a promise to be careful.
//!
//! # Running it
//!
//! Needs the artefacts: run `cargo xtask fetch` first. Without `spec/` the test
//! **skips** rather than fails, so a contributor without the artefacts — or CI
//! without network — can still run the suite. The skip is printed, not silent.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use en16931::codes::generated::*;

/// Location of the code-list Schematron, relative to the crate root.
const CODES_SCH: &str = "spec/eInvoicing-EN16931/ubl/schematron/codelist/EN16931-UBL-codes.sch";

fn artefact() -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(CODES_SCH);
    p.exists().then_some(p)
}

/// Every string literal in the `test` attribute of the assert with `id`, in
/// source order, split into codes.
///
/// Deliberately does **not** stop at the first: see the module docs.
fn literals_of(xml: &str, id: &str) -> Vec<Vec<String>> {
    // Locate the assert by its id attribute, then walk back to the enclosing
    // `test="` and forward to its closing quote. Attribute values in this file
    // contain no escaped quotes, which the assertion below double-checks.
    let mut out = Vec::new();
    let needle = format!("id=\"{id}\"");
    for (idx, _) in xml.match_indices(&needle) {
        let head = &xml[..idx];
        let Some(t0) = head.rfind("test=\"") else {
            continue;
        };
        let body = &xml[t0 + 6..];
        let Some(t1) = body.find('"') else { continue };
        let expr = &body[..t1];
        assert!(
            !expr.contains("&quot;"),
            "{id}: escaped quote in test; the naive slice above is unsafe"
        );
        for lit in expr.split('\'').skip(1).step_by(2) {
            let codes: Vec<String> = lit.split_whitespace().map(str::to_owned).collect();
            if !codes.is_empty() {
                out.push(codes);
            }
        }
    }
    out
}

/// A committed table must equal exactly one of the literals of its rule.
fn assert_table_matches(xml: &str, name: &str, rule: &str, table: &[&str]) {
    let lists = literals_of(xml, rule);
    assert!(
        !lists.is_empty(),
        "{rule}: no code list found in the artefact"
    );
    let want: BTreeSet<&str> = table.iter().copied().collect();
    let found = lists
        .iter()
        .any(|l| l.iter().map(String::as_str).collect::<BTreeSet<_>>() == want);
    assert!(
        found,
        "{name} does not match any list in {rule}.\n  \
         committed: {} values\n  artefact:  {:?} values\n  \
         Regenerate with `cargo xtask codegen` and review the diff.",
        table.len(),
        lists.iter().map(Vec::len).collect::<Vec<_>>()
    );
}

#[test]
fn committed_tables_match_the_artefacts() {
    let Some(path) = artefact() else {
        eprintln!("skipping: {CODES_SCH} not present — run `cargo xtask fetch`");
        return;
    };
    let xml = std::fs::read_to_string(&path).expect("read artefact");

    for (name, rule, table) in [
        ("INVOICE_TYPE_CODES", "BR-CL-01", INVOICE_TYPE_CODES),
        ("CREDIT_NOTE_TYPE_CODES", "BR-CL-01", CREDIT_NOTE_TYPE_CODES),
        ("CURRENCY_CODES", "BR-CL-04", CURRENCY_CODES),
        ("VAT_POINT_DATE_CODES", "BR-CL-06", VAT_POINT_DATE_CODES),
        ("REFERENCE_QUALIFIERS", "BR-CL-07", REFERENCE_QUALIFIERS),
        ("ICD_SCHEMES", "BR-CL-10", ICD_SCHEMES),
        (
            "ITEM_CLASSIFICATION_SCHEMES",
            "BR-CL-13",
            ITEM_CLASSIFICATION_SCHEMES,
        ),
        ("COUNTRY_CODES", "BR-CL-14", COUNTRY_CODES),
        ("PAYMENT_MEANS_CODES", "BR-CL-16", PAYMENT_MEANS_CODES),
        ("VAT_CATEGORY_CODES", "BR-CL-17", VAT_CATEGORY_CODES),
        ("ALLOWANCE_REASON_CODES", "BR-CL-19", ALLOWANCE_REASON_CODES),
        ("CHARGE_REASON_CODES", "BR-CL-20", CHARGE_REASON_CODES),
        ("VATEX_CODES", "BR-CL-22", VATEX_CODES),
        ("UNIT_CODES", "BR-CL-23", UNIT_CODES),
        ("EAS_SCHEMES", "BR-CL-25", EAS_SCHEMES),
    ] {
        assert_table_matches(&xml, name, rule, table);
    }
}

/// The two rules whose tests carry more than one list, pinned explicitly.
///
/// If a future artefact revision collapses or extends either, this fails loudly
/// rather than letting the generator silently pick a different branch.
#[test]
fn the_multi_list_rules_still_have_the_shape_we_assume() {
    let Some(path) = artefact() else {
        eprintln!("skipping: artefacts not present");
        return;
    };
    let xml = std::fs::read_to_string(&path).expect("read artefact");

    // BR-CL-01: a `self::` disjunction, two lists, 50 and 13.
    let mut sizes: Vec<usize> = literals_of(&xml, "BR-CL-01").iter().map(Vec::len).collect();
    sizes.sort_unstable();
    assert_eq!(
        sizes,
        vec![13, 50],
        "BR-CL-01 is no longer two lists of 13 and 50"
    );

    // BR-CL-10: the ICD list plus the contextual `SEPA` literal.
    let l10 = literals_of(&xml, "BR-CL-10");
    let mut sizes: Vec<usize> = l10.iter().map(Vec::len).collect();
    sizes.sort_unstable();
    assert_eq!(
        sizes,
        vec![1, 243],
        "BR-CL-10 is no longer ICD + one literal"
    );
    assert!(
        l10.iter().any(|l| l == &["SEPA"]),
        "BR-CL-10's contextual literal is no longer SEPA"
    );
}

/// `SEPA` is admissible under `BR-CL-10` but is deliberately absent from the
/// table, because its admissibility depends on ancestry the table cannot see.
#[test]
fn sepa_is_not_in_the_icd_table() {
    assert!(
        !ICD_SCHEMES.contains(&"SEPA"),
        "SEPA is contextual — only under AccountingSupplierParty or PayeeParty — \
         so it belongs in the rule, not in a flat lookup table"
    );
}

// ── The artefact gap, measured rather than claimed ────────────────────────────

/// Every rule id asserted anywhere in the syntax-independent artefacts.
///
/// Two files: the abstract model (`BR-*`, `BR-CO-*`, the category families) and
/// the UBL code-list Schematron (`BR-CL-*`). Together these are the 223
/// syntax-independent rules; the ~1 315 `UBL-*` / `CII-*` syntax rules are
/// deliberately out of scope for a syntax-independent model.
fn artefact_rule_ids() -> Option<BTreeSet<String>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let files = [
        "spec/eInvoicing-EN16931/ubl/schematron/abstract/EN16931-model.sch",
        CODES_SCH,
    ];
    let mut out = BTreeSet::new();
    for f in files {
        let p = root.join(f);
        if !p.exists() {
            return None;
        }
        let xml = std::fs::read_to_string(&p).expect("read artefact");
        for (idx, _) in xml.match_indices("id=\"BR-") {
            let rest = &xml[idx + 4..];
            let end = rest.find('"').expect("unterminated id");
            out.insert(normalise(&rest[..end]));
        }
    }
    Some(out)
}

/// `BR-1` and `BR-01` are the same rule: the standard's prose drops the leading
/// zero, the artefacts keep it. Compare on the padded form.
fn normalise(id: &str) -> String {
    match id.rsplit_once('-') {
        Some((head, tail)) if tail.len() < 2 && tail.chars().all(|c| c.is_ascii_digit()) => {
            format!("{head}-0{tail}")
        }
        _ => id.to_owned(),
    }
}

/// The registry covers every syntax-independent artefact rule.
///
/// This is the number that matters and the easiest one to overstate, so it is
/// asserted rather than reported in a README. It compares the **runtime**
/// registry — not a grep of the source, which misses every macro-generated
/// family and would have read 92 where the truth is 223.
#[test]
fn the_registry_covers_the_syntax_independent_artefacts() {
    let Some(artefact) = artefact_rule_ids() else {
        eprintln!("skipping: artefacts not present");
        return;
    };
    let implemented: BTreeSet<String> = en16931::validation::rules::CORE
        .iter()
        .map(|r| normalise(r.id.as_str()))
        .collect();

    let missing: Vec<_> = artefact.difference(&implemented).cloned().collect();
    eprintln!(
        "syntax-independent artefact rules: {} / {} registered",
        artefact.len() - missing.len(),
        artefact.len()
    );
    assert!(
        missing.is_empty(),
        "{} artefact rule(s) are not in the registry:\n  {}",
        missing.len(),
        missing.join("\n  ")
    );
    assert_eq!(
        artefact.len(),
        223,
        "the pinned artefacts carry 223 syntax-independent rules; if this \
         changed, the artefact revision moved and everything here needs a look"
    );
}

/// The profile rule sets cover their own artefacts.
///
/// Same discipline as the core check above, and for the same reason: "XRechnung
/// support" is easy to claim and easy to be 60 % of. Each profile is compared
/// against the Schematron its authority publishes.
///
/// One family is deliberately out of scope and named here rather than silently
/// dropped: **Peppol's national rules** (`DK-R-*`, `GR-R-*`, `IS-R-*`,
/// `IT-R-*`, `NL-R-*`, `NO-R-*`, `SE-R-*`, `DE-R-*`) — country requirements
/// layered on Peppol, each with its own registry formats and check digits.
/// `DE-R-*` is XRechnung translated and is already covered under its own ids.
#[test]
fn the_profile_rule_sets_cover_their_artefacts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));

    /// Profile name, artefact path, and which ids that artefact owns.
    type ProfileArtefact = (&'static str, &'static str, fn(&str) -> bool);

    let cases: &[ProfileArtefact] = &[
        (
            "XRechnung 3.0",
            "spec/xrechnung-schematron/src/validation/schematron/ubl/XRechnung-UBL-validation.sch",
            // Everything, `BR-DEX-*` included — the Extension is shipped as
            // `profiles::XRECHNUNG_EXTENSION` and its rules are registered.
            |_| true,
        ),
        // KoSIT's `rule-list.xml` — the Peppol asserts its build merges in.
        // This file is the *only* place the merged set is written down, and
        // reading the Schematron alone would miss all 21.
        //
        // It must be **parsed**, not scanned: eleven entries are commented out
        // in the XML, and a text search matches straight through the comments.
        (
            "XRechnung 3.0 (merged Peppol rules)",
            "spec/xrechnung-schematron/src/xsl/rule-list.xml",
            |id| id.starts_with("PEPPOL-EN16931-"),
        ),
        (
            "Peppol BIS Billing 3.0",
            "spec/peppol-bis-invoice-3/rules/sch/PEPPOL-EN16931-UBL.sch",
            |id| id.starts_with("PEPPOL-EN16931-") && !id.contains("COMMON"),
        ),
    ];

    // Every id this crate can report, from every shipped profile.
    let mut known: BTreeSet<String> = en16931::validation::rules::CORE
        .iter()
        .map(|r| normalise(r.id.as_str()))
        .collect();
    for p in en16931::profiles::ALL {
        known.extend(p.extra_rules.iter().map(|r| normalise(r.id.as_str())));
        known.extend(p.restrictions.iter().map(|r| normalise(r.id())));
    }

    for (name, file, in_scope) in cases {
        let p = root.join(file);
        if !p.exists() {
            eprintln!("skipping {name}: artefacts not present");
            continue;
        }
        let xml = std::fs::read_to_string(&p).expect("read artefact");
        // `<assert id=…>` only — `<pattern id=…>` and `<rule id=…>` are
        // structure, not rules, and counting them would inflate the denominator.
        let mut want = BTreeSet::new();
        // `rule-list.xml` names rules as `<r:rule>` text, not `<assert id=…>`.
        // Commented-out entries must not count, so this branch parses.
        if file.ends_with("rule-list.xml") {
            let doc = roxmltree::Document::parse(&xml).expect("parse rule-list");
            for e in doc.root_element().children().filter(|n| n.is_element()) {
                if let Some(t) = e.text().map(str::trim).filter(|t| !t.is_empty())
                    && in_scope(t)
                {
                    want.insert(normalise(t));
                }
            }
            let missing: Vec<_> = want.difference(&known).cloned().collect();
            eprintln!(
                "{name}: {} / {} in-scope rules registered",
                want.len() - missing.len(),
                want.len()
            );
            assert!(
                missing.is_empty(),
                "{name} is missing {} rule(s):\n  {}",
                missing.len(),
                missing.join("\n  ")
            );
            continue;
        }
        let marker = "<assert";
        for (idx, _) in xml.match_indices(marker) {
            let id = if marker == "<r:rule" {
                let rest = &xml[idx..];
                let start = rest.find('>').expect("unclosed element") + 1;
                let end = rest[start..].find('<').expect("unterminated text") + start;
                rest[start..end].trim()
            } else {
                let Some(rel) = xml[idx..].find("id=\"") else {
                    continue;
                };
                let rest = &xml[idx + rel + 4..];
                let end = rest.find('"').expect("unterminated id");
                &rest[..end]
            };
            if in_scope(id) {
                want.insert(normalise(id));
            }
        }
        let missing: Vec<_> = want.difference(&known).cloned().collect();
        eprintln!(
            "{name}: {} / {} in-scope rules registered",
            want.len() - missing.len(),
            want.len()
        );
        assert!(
            missing.is_empty(),
            "{name} is missing {} rule(s):\n  {}",
            missing.len(),
            missing.join("\n  ")
        );
    }
}

/// The syntax-rule count this crate quotes is the one the artefacts carry.
///
/// `README.md`, `lib.rs` and the design notes all name a figure for the rules that
/// belong to `en16931-formats`, and a number repeated in three documents and
/// checked in none is how it came to be **1 315**: the sum omitted `UBL-DT-*`
/// while including `CII-DT-*`.
#[test]
fn the_syntax_rule_count_is_measured() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut ids = BTreeSet::new();
    for syntax in ["ubl", "cii"] {
        let dir = root
            .join("spec/eInvoicing-EN16931")
            .join(syntax)
            .join("schematron");
        let Ok(walk) = std::fs::read_dir(&dir) else {
            eprintln!("skipping: artefacts not present");
            return;
        };
        for sub in walk.flatten() {
            // `preprocessed/` is the same rules again, resolved.
            if sub.file_name() == "preprocessed" {
                continue;
            }
            let Ok(files) = std::fs::read_dir(sub.path()) else {
                continue;
            };
            for f in files
                .flatten()
                .filter(|f| f.path().extension().is_some_and(|e| e == "sch"))
            {
                let xml = std::fs::read_to_string(f.path()).expect("read");
                for (idx, _) in xml.match_indices("<assert") {
                    let Some(rel) = xml[idx..].find("id=\"") else {
                        continue;
                    };
                    let rest = &xml[idx + rel + 4..];
                    let end = rest.find('"').expect("unterminated id");
                    let id = &rest[..end];
                    if id.starts_with("UBL-") || id.starts_with("CII-") {
                        ids.insert(id.to_owned());
                    }
                }
            }
        }
    }
    if ids.is_empty() {
        eprintln!("skipping: artefacts not present");
        return;
    }
    eprintln!("syntax rules (`en16931-formats`' job): {}", ids.len());
    assert_eq!(
        ids.len(),
        1339,
        "the documented syntax-rule count must match the artefacts"
    );
}

/// CEN's own unit-test suite agrees with this crate's dispositions.
///
/// `spec/eInvoicing-EN16931/test/{Invoice,CreditNote}-unit-UBL/` holds one XML
/// instance per rule CEN considers worth testing — 208 of them. It is a
/// different kind of artefact from the Schematron: the Schematron says what CEN
/// *specifies*, the suite says what CEN *exercises*. Where this crate declines
/// to evaluate a rule, the suite is independent evidence for or against.
///
/// Two claims are checked, and only two, because only two hold:
///
/// * **Every rule this crate calls undecidable has no CEN unit test.**
///   `BR-CO-05` … `BR-CO-08` are bound to `value="true()"` in the Schematron
///   (§34.3) *and* CEN ships no instance for them. Two independent artefacts,
///   same conclusion.
/// * **No `BR-DEC-*` rule has a CEN unit test.** They constrain decimal places,
///   which `InvoiceAmount`'s `i64` minor units make unrepresentable.
///
/// The converse does **not** hold and is not asserted: 53 artefact rules have no
/// unit test, most of them simply because the suite predates the `BR-AF-*` /
/// `BR-AG-*` / `BR-B-*` families. "Untested by CEN" is not "unimplementable".
#[test]
fn cen_unit_tests_agree_with_our_dispositions() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut tested = BTreeSet::new();
    let mut found_any = false;
    for dir in ["Invoice-unit-UBL", "CreditNote-unit-UBL"] {
        let d = root.join("spec/eInvoicing-EN16931/test").join(dir);
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        found_any = true;
        for e in entries.flatten() {
            let name = e.file_name();
            let Some(stem) = name
                .to_string_lossy()
                .strip_suffix(".xml")
                .map(str::to_owned)
            else {
                continue;
            };
            // `BR-S-08-1`, `-2`, `-3` are three cases of one rule.
            let base = match stem.rsplit_once('-') {
                Some((head, tail))
                    if tail.len() == 1 && tail.chars().all(|c| c.is_ascii_digit()) =>
                {
                    head.to_owned()
                }
                _ => stem,
            };
            // CEN's filenames use the **standard's** family spelling (`BR-IG-*`,
            // `BR-IP-*`) where the Schematron uses the artefacts' (`BR-AF-*`,
            // `BR-AG-*`). That this crate has to alias them is not a theory:
            // here are 22 files proving both spellings are in live use.
            tested.insert(en16931::validation::RuleId::new(Box::leak(
                base.into_boxed_str(),
            )));
        }
    }
    if !found_any {
        eprintln!("skipping: artefacts not present");
        return;
    }

    let has_test = |id: &str| tested.iter().any(|t| t.matches(id));

    for undecidable in ["BR-CO-05", "BR-CO-06", "BR-CO-07", "BR-CO-08"] {
        assert!(
            !has_test(undecidable),
            "{undecidable} is registered as undecidable because CEN binds it to \
             true() — but CEN ships a unit test for it, so one of the two \
             readings is wrong"
        );
    }
    let dec_with_tests: Vec<_> = en16931::validation::rules::CORE
        .iter()
        .filter(|r| r.id.as_str().starts_with("BR-DEC-"))
        .filter(|r| has_test(r.id.as_str()))
        .map(|r| r.id.as_str())
        .collect();
    assert!(
        dec_with_tests.is_empty(),
        "these BR-DEC-* rules are retired by the type system but CEN exercises \
         them, which is worth a look: {dec_with_tests:?}"
    );

    // The alias claim, stated as an assertion rather than a comment.
    assert!(
        has_test("BR-AF-01") && has_test("BR-AG-01"),
        "CEN's suite spells these BR-IG-01 / BR-IP-01; RuleId must alias them"
    );
    eprintln!("CEN unit-test instances: {} rules exercised", tested.len());
}
