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

mod common;

use std::collections::BTreeSet;
use std::path::PathBuf;

use en16931::codes::generated::*;

/// Location of the code-list Schematron, relative to the crate root.
const CODES_SCH: &str = "eInvoicing-EN16931/ubl/schematron/codelist/EN16931-UBL-codes.sch";

fn artefact() -> Option<PathBuf> {
    let p = common::require("code-list verification")?.join(CODES_SCH);
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
    let root = common::spec_root()?;
    let files = [
        "eInvoicing-EN16931/ubl/schematron/abstract/EN16931-model.sch",
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
    let Some(root) = common::spec_root() else {
        return;
    };

    /// Profile name, artefact path, and which ids that artefact owns.
    type ProfileArtefact = (&'static str, &'static str, fn(&str) -> bool);

    let cases: &[ProfileArtefact] = &[
        (
            "XRechnung 3.0",
            "xrechnung-schematron/src/validation/schematron/ubl/XRechnung-UBL-validation.sch",
            // Everything, `BR-DEX-*` included — the Extension is shipped as
            // `profiles::XRECHNUNG_EXTENSION` and its rules are registered.
            |_| true,
        ),
        // KoSIT publishes **two** Schematrons, and two assertions exist only
        // in the CII one — so reading the UBL file alone makes "55 / 55" mean
        // 55 of 57. Both are syntax rules by construction and stay out of
        // scope, by decision rather than by oversight; see
        // `CII_ONLY_SYNTAX_RULES`.
        (
            "XRechnung 3.0 (CII-only assertions)",
            "xrechnung-schematron/src/validation/schematron/cii/XRechnung-CII-validation.sch",
            |id| !CII_ONLY_SYNTAX_RULES.iter().any(|(r, _)| *r == id),
        ),
        // KoSIT's `rule-list.xml` — the Peppol asserts its build merges in.
        // This file is the *only* place the merged set is written down, and
        // reading the Schematron alone would miss all 21.
        //
        // It must be **parsed**, not scanned: eleven entries are commented out
        // in the XML, and a text search matches straight through the comments.
        (
            "XRechnung 3.0 (merged Peppol rules)",
            "xrechnung-schematron/src/xsl/rule-list.xml",
            |id| id.starts_with("PEPPOL-EN16931-"),
        ),
        (
            "Peppol BIS Billing 3.0",
            "peppol-bis-invoice-3/rules/sch/PEPPOL-EN16931-UBL.sch",
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

/// KoSIT assertions that exist only in the CII Schematron, and why a
/// syntax-independent model cannot hold them.
///
/// Both are about the **shape of the CII document**, not about the invoice: the
/// model has one item base quantity and one line structure, so neither
/// condition is expressible in it, let alone violable. They belong to
/// `en16931-formats`, which is where the CII binding lives.
///
/// Named here rather than filtered by a pattern, so adding a third CII-only
/// rule fails this suite and forces the same decision to be made again.
const CII_ONLY_SYNTAX_RULES: &[(&str, &str)] = &[
    (
        "BR-TMP-3",
        "BT-149 / BT-150 must agree between CII's Gross and Net price paths — \
         the model carries one base quantity, so the two cannot disagree",
    ),
    (
        "BR-DEX-15",
        "ram:ParentLineID must not appear — a CII element for sub invoice lines. \
         The Extension model carries them; whether a syntax can is the syntax's question",
    ),
];

/// The syntax-rule count this crate quotes is the one the artefacts carry.
///
/// `README.md`, `lib.rs` and the documentation site all name a figure for the
/// rules that belong to `en16931-formats`, and a number repeated in three
/// documents and checked in none is how it came to be **1 315**: the sum omitted
/// `UBL-DT-*` while including `CII-DT-*`.
#[test]
fn the_syntax_rule_count_is_measured() {
    let Some(root) = common::spec_root() else {
        return;
    };
    let mut ids = BTreeSet::new();
    for syntax in ["ubl", "cii"] {
        let dir = root
            .join("eInvoicing-EN16931")
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
    let Some(root) = common::spec_root() else {
        return;
    };
    let mut tested = BTreeSet::new();
    let mut found_any = false;
    for dir in ["Invoice-unit-UBL", "CreditNote-unit-UBL"] {
        let d = root.join("eInvoicing-EN16931/test").join(dir);
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

// ── Severity overrides, against the authority's own configuration ────────────

/// KoSIT's validator configuration, relative to `spec/`.
const XR_SCENARIOS: &str = "validator-configuration-xrechnung/scenarios.xml";

/// Every `<customLevel>` in a named scenario, as `(rule id, level)`.
fn custom_levels(xml: &str, scenario: &str) -> Vec<(String, String)> {
    let doc = roxmltree::Document::parse(xml).expect("scenarios.xml is XML");
    doc.descendants()
        .filter(|n| n.tag_name().name() == "scenario")
        .find(|n| {
            n.children()
                .find(|c| c.tag_name().name() == "name")
                .and_then(|c| c.text())
                .is_some_and(|t| t.trim() == scenario)
        })
        .into_iter()
        .flat_map(|n| n.descendants().collect::<Vec<_>>())
        .filter(|n| n.tag_name().name() == "customLevel")
        .filter_map(|n| {
            Some((
                n.text()?.trim().to_owned(),
                n.attribute("level")?.to_owned(),
            ))
        })
        .collect()
}

/// KoSIT's Schematrons, relative to `spec/`. Both of them.
const XR_SCHEMATRONS: [&str; 2] = [
    "xrechnung-schematron/src/validation/schematron/ubl/XRechnung-UBL-validation.sch",
    "xrechnung-schematron/src/validation/schematron/cii/XRechnung-CII-validation.sch",
];

/// Every `<assert>`/`<report>` id in KoSIT's own Schematrons, with its `flag`.
///
/// Parsed, not scanned: several `test` expressions contain a literal `>`
/// (`count(cac:SubInvoiceLine) > 0`), so a regular expression over the tag
/// stops early and reports the wrong flag for `BR-DEX-02`.
fn kosit_flags(root: &std::path::Path) -> std::collections::BTreeMap<String, en16931::Severity> {
    let mut out = std::collections::BTreeMap::new();
    for file in XR_SCHEMATRONS {
        let path = root.join(file);
        if !path.exists() {
            continue;
        }
        let xml = std::fs::read_to_string(&path).expect("read Schematron");
        let doc = roxmltree::Document::parse(&xml).expect("Schematron is XML");
        for n in doc.descendants() {
            if !matches!(n.tag_name().name(), "assert" | "report") {
                continue;
            }
            let (Some(id), flag) = (n.attribute("id"), n.attribute("flag").unwrap_or("fatal"))
            else {
                continue;
            };
            if !id.starts_with("BR-DE") && !id.starts_with("BR-TMP") {
                continue; // CEN's and Peppol's rules; their own files own them
            }
            let level = match flag {
                "fatal" => en16931::Severity::Fatal,
                "warning" => en16931::Severity::Warning,
                "information" => en16931::Severity::Info,
                other => panic!("{id}: unknown flag {other:?}"),
            };
            if let Some(prev) = out.insert(id.to_owned(), level) {
                assert_eq!(
                    prev, level,
                    "{id} carries two flags across the two syntaxes"
                );
            }
        }
    }
    out
}

/// Every KoSIT rule must run at the severity **KoSIT's Schematron** gives it.
///
/// # A second file publishes severity, and it was never read
///
/// [`the_severity_overrides_are_kosits_own`] checks `scenarios.xml`, which
/// re-levels *CEN's* rules. KoSIT's own rules carry their severity in the
/// Schematron's `flag` attribute instead, and nothing compared that — so five
/// checks ran as fatal that KoSIT publishes as warnings:
///
/// | | |
/// |---|---|
/// | `BR-DE-26` | *"soll … übermittelt werden"* — a corrected invoice **should** cite the original |
/// | `BR-DE-27`, `BR-DE-28` | a telephone number with two digits, an address that is not quite one |
/// | `BR-DE-17`, `BR-DE-21` | scoping: a lawful EN 16931 type code or another CIUS's BT-24 |
///
/// Each of those rejected an invoice the German reference validator accepts,
/// which is the exact failure this crate exists not to have.
#[test]
fn every_kosit_check_runs_at_the_severity_kosit_publishes() {
    let Some(root) = common::require("XRechnung rule severities") else {
        return;
    };
    let published = kosit_flags(&root);
    if published.is_empty() {
        eprintln!("note: KoSIT Schematrons absent — skipped");
        return;
    }

    let mut checked = 0usize;
    for profile in [
        &en16931::profiles::XRECHNUNG,
        &en16931::profiles::XRECHNUNG_CVD,
        &en16931::profiles::XRECHNUNG_EXTENSION,
    ] {
        for id in profile.check_ids() {
            let Some(want) = published.get(id) else {
                continue; // CEN's or Peppol's; a different authority, a different file
            };
            let got = profile
                .severity_of(id)
                .unwrap_or_else(|| panic!("{}: {id} runs but has no severity", profile.id));
            assert_eq!(
                got, *want,
                "{} reports {id} as {got}, and KoSIT publishes {want}",
                profile.id
            );
            checked += 1;
        }
    }
    assert!(checked > 100, "only {checked} severities compared");
    eprintln!("KoSIT rule severities: {checked} compared against the Schematron");
}

/// Peppol publishes every one of its rules as **fatal**, and one of them stops
/// being fatal when XRechnung merges it.
///
/// The companion to
/// [`every_kosit_check_runs_at_the_severity_kosit_publishes`], for the other
/// authority whose rules this crate carries. It also pins the single
/// consequence-changing rewrite in KoSIT's `peppol-into-xr.xsl`:
///
/// ```xslt
/// <xsl:when test="@id='PEPPOL-EN16931-R120'">
///   <xsl:attribute name="flag">warning</xsl:attribute>
/// ```
///
/// Same id, same text, different consequence — a line whose net amount does not
/// follow from its price **rejects** a Peppol invoice and merely annotates an
/// XRechnung one. That is why severity belongs to the rule instance a profile
/// holds rather than to a global registry, and why this is asserted from the
/// stylesheet rather than remembered.
#[test]
fn every_peppol_rule_runs_at_the_severity_its_authority_publishes() {
    let Some(root) = common::require("Peppol rule severities") else {
        return;
    };
    let sch = root.join("peppol-bis-invoice-3/rules/sch/PEPPOL-EN16931-UBL.sch");
    if !sch.exists() {
        eprintln!("note: {} absent — skipped", sch.display());
        return;
    }
    let xml = std::fs::read_to_string(&sch).expect("read Peppol Schematron");
    let doc = roxmltree::Document::parse(&xml).expect("Schematron is XML");
    let mut published = std::collections::BTreeMap::new();
    for n in doc.descendants() {
        if !matches!(n.tag_name().name(), "assert" | "report") {
            continue;
        }
        if let Some(id) = n
            .attribute("id")
            .filter(|i| i.starts_with("PEPPOL-EN16931"))
        {
            published.insert(id.to_owned(), n.attribute("flag").unwrap_or("fatal"));
        }
    }
    assert!(!published.is_empty(), "no Peppol assertions found");

    let mut checked = 0usize;
    for id in en16931::profiles::PEPPOL_BIS_3.check_ids() {
        let Some(flag) = published.get(id) else {
            continue;
        };
        assert_eq!(*flag, "fatal", "{id}: Peppol publishes everything fatal");
        assert_eq!(
            en16931::profiles::PEPPOL_BIS_3.severity_of(id),
            Some(en16931::Severity::Fatal),
            "{id} under Peppol BIS Billing 3.0"
        );
        checked += 1;
    }
    assert_eq!(checked, 46, "every Peppol rule this crate runs");

    // …and the one XRechnung's build rewrites, read out of the stylesheet that
    // rewrites it rather than trusted from a comment.
    let xsl = std::fs::read_to_string(root.join("xrechnung-schematron/src/xsl/peppol-into-xr.xsl"))
        .expect("read peppol-into-xr.xsl");
    let rewrite = xsl
        .find("@id='PEPPOL-EN16931-R120'")
        .map(|i| &xsl[i..i + 400])
        .expect("the R120 branch");
    assert!(
        rewrite.contains(r#"name="flag">warning"#),
        "the stylesheet no longer downgrades R120:\n{rewrite}"
    );
    assert_eq!(
        en16931::profiles::XRECHNUNG.severity_of("PEPPOL-EN16931-R120"),
        Some(en16931::Severity::Warning),
    );
    eprintln!("Peppol rule severities: {checked} compared, plus XRechnung's R120 rewrite");
}

/// The profiles' [`Profile::levels`] must be what KoSIT publishes — not a
/// reconstruction of "which core rule does each `BR-DEX-*` widen?".
///
/// That reconstruction goes wrong twice over: it names
/// `PEPPOL-EN16931-CL001`, a rule XRechnung's build does not merge in, where
/// CEN's `BR-CL-24` is meant; and it misses `BR-CL-21` and `BR-CL-23`
/// entirely, which reports as **fatal** two rules the German reference
/// validator reports as warnings. So the mapping is measured against
/// `scenarios.xml`, the file the validator actually loads.
///
/// # `levels` has two sources, and this test owns one of them
///
/// `scenarios.xml`'s `customLevel` covers CEN's rules. KoSIT's *own* rules
/// carry their severity in the Schematron `flag`, and two of them are
/// restrictions with no severity of their own, so `levels` states theirs —
/// see `XR_LEVELS`. Those entries are partitioned out here and checked by
/// [`every_kosit_check_runs_at_the_severity_kosit_publishes`] instead.
#[test]
fn the_severity_overrides_are_kosits_own() {
    let Some(root) = common::require("XRechnung severity overrides") else {
        return;
    };
    let path = root.join(XR_SCENARIOS);
    if !path.exists() {
        eprintln!("note: {} absent — skipped", path.display());
        return;
    }
    let xml = std::fs::read_to_string(&path).expect("read scenarios.xml");

    // `UBL-CR-*` and `CII-SR-*` are syntax rules and belong to
    // `en16931-formats`; this crate holds the syntax-independent set only.
    let semantic =
        |(id, level): &(String, String)| id.starts_with("BR-").then(|| (id.clone(), level.clone()));
    let name = |s: &str| match s {
        "warning" => en16931::Severity::Warning,
        "information" => en16931::Severity::Info,
        "error" => en16931::Severity::Fatal,
        other => panic!("unknown customLevel {other:?}"),
    };

    for (scenario, profile) in [
        (
            "EN16931 XRechnung (UBL Invoice)",
            &en16931::profiles::XRECHNUNG,
        ),
        (
            "EN16931 XRechnung CVD (UBL Invoice)",
            &en16931::profiles::XRECHNUNG_CVD,
        ),
        (
            "EN16931 XRechnung Extension (UBL Invoice)",
            &en16931::profiles::XRECHNUNG_EXTENSION,
        ),
    ] {
        let mut want: Vec<_> = custom_levels(&xml, scenario)
            .iter()
            .filter_map(semantic)
            .map(|(id, level)| (id, name(&level)))
            .collect();
        want.sort();
        let mut got: Vec<_> = profile
            .levels
            .iter()
            // KoSIT's own ids are the Schematron's business, not this file's.
            .filter(|(id, _)| !id.starts_with("BR-DE") && !id.starts_with("BR-TMP"))
            .map(|(id, level)| ((*id).to_owned(), *level))
            .collect();
        got.sort();
        assert_eq!(
            got, want,
            "{} does not report what {scenario} configures",
            profile.id
        );
    }
    eprintln!("XRechnung severity overrides: checked against scenarios.xml");
}
