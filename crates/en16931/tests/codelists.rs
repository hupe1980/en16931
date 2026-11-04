//! Checks the committed code lists, rule coverage and severities against the
//! reference corpus (`cargo xtask fetch`); skips with a message when it is absent.
//!
//! `BR-CL-01` (invoice vs credit-note type codes) and `BR-CL-10` (ICD plus a
//! contextual `SEPA`) carry more than one list in one `test` expression.

mod common;

use std::collections::BTreeSet;
use std::path::PathBuf;

use en16931::codes::generated::*;

/// The code-list Schematron, relative to the reference corpus.
const CODES_SCH: &str = "eInvoicing-EN16931/ubl/schematron/codelist/EN16931-UBL-codes.sch";

fn artefact() -> Option<PathBuf> {
    let p = common::require("code-list verification")?.join(CODES_SCH);
    p.exists().then_some(p)
}

/// Every string literal in the `test` of the assert with `id`, split into codes.
fn literals_of(xml: &str, id: &str) -> Vec<Vec<String>> {
    // Find the id, walk back to `test="` and forward to the closing quote.
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

/// The two multi-list rules keep the shape the generator assumes.
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
        vec![1, 246],
        "BR-CL-10 is no longer ICD + one literal"
    );
    assert!(
        l10.iter().any(|l| l == &["SEPA"]),
        "BR-CL-10's contextual literal is no longer SEPA"
    );
}

/// `SEPA` is admissible under `BR-CL-10` only by ancestry, so it is not in the table.
#[test]
fn sepa_is_not_in_the_icd_table() {
    assert!(
        !ICD_SCHEMES.contains(&"SEPA"),
        "SEPA is contextual — only under AccountingSupplierParty or PayeeParty — \
         so it belongs in the rule, not in a flat lookup table"
    );
}

// ── Rule coverage ─────────────────────────────────────────────────────────────

/// Every `BR-*` id in the abstract-model and code-list Schematrons: the
/// syntax-independent rules.
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

/// Pads a one-digit suffix (`BR-1` → `BR-01`).
fn normalise(id: &str) -> String {
    match id.rsplit_once('-') {
        Some((head, tail)) if tail.len() < 2 && tail.chars().all(|c| c.is_ascii_digit()) => {
            format!("{head}-0{tail}")
        }
        _ => id.to_owned(),
    }
}

/// The runtime registry covers every syntax-independent artefact rule.
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

/// Each profile covers the Schematron its authority publishes, except
/// [`KOSIT_SYNTAX_ONLY_RULES`] and Peppol's national rules
/// (`common::peppol::is_national`).
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
            // Everything (`BR-DEX-*` included) except syntax-only assertions.
            |id| !KOSIT_SYNTAX_ONLY_RULES.iter().any(|(r, _)| *r == id),
        ),
        // Some KoSIT assertions exist only in the CII Schematron.
        (
            "XRechnung 3.0 (CII-only assertions)",
            "xrechnung-schematron/src/validation/schematron/cii/XRechnung-CII-validation.sch",
            |id| !KOSIT_SYNTAX_ONLY_RULES.iter().any(|(r, _)| *r == id),
        ),
        // KoSIT's `rule-list.xml`: the Peppol asserts its build merges in. Parsed,
        // because some entries are commented out.
        (
            "XRechnung 3.0 (merged Peppol rules)",
            "xrechnung-schematron/src/xsl/rule-list.xml",
            |id| id.starts_with("PEPPOL-EN16931-"),
        ),
        // Every Peppol assertion except the national rules.
        ("Peppol BIS Billing 3.0", common::peppol::UBL_SCH, |id| {
            !common::peppol::is_national(id)
        }),
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
        let mut want = BTreeSet::new();
        // `rule-list.xml` names rules as element text, not `<assert id=…>`.
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
        // Parsed: a text scan would count asserts inside XML comments.
        for id in common::peppol::flags(&xml).into_keys() {
            if in_scope(&id) {
                want.insert(normalise(&id));
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

/// KoSIT assertions about the document's syntax, which the model cannot hold,
/// with what the `en16931-formats` readers do instead.
const KOSIT_SYNTAX_ONLY_RULES: &[(&str, &str)] = &[
    (
        "BR-TMP-3",
        "CII only. BT-149 / BT-150 must agree between CII's Gross and Net price paths — \
         the model carries one base quantity, so the two cannot disagree",
    ),
    (
        "BR-TMP-4",
        "CII only, warning. At most one `ram:Name` (BT-123) in a BG-24 document — the \
         model holds one; the CII reader keeps the first and reports the second",
    ),
    (
        "BR-TMP-5",
        "CII only, warning. At most one `ram:AttachmentBinaryObject` (BT-125) in a BG-24 \
         document — the model holds one; the CII reader keeps the first and reports the second",
    ),
    (
        "BR-TMP-6",
        "UBL only, warning. A date is written `YYYY-MM-DD` — `Date` holds a calendar day, \
         not a spelling. The UBL reader accepts `xs:date` with a time zone (dropped) and \
         leaves a value it cannot read as a date absent",
    ),
    (
        "BR-TMP-7",
        "CII only, warning. A date carries `format=\"102\"` and `YYYYMMDD` — the CII \
         reader accepts a missing `format`, and records any other format or a \
         non-`CCYYMMDD` value as malformed and leaves the term absent",
    ),
    (
        "BR-DEX-15",
        "CII only. ram:ParentLineID must not appear — a CII element for sub invoice lines. \
         The Extension model carries them; whether a syntax can is the syntax's question",
    ),
];

/// The documented syntax-rule (`UBL-*` / `CII-*`) count matches the artefacts.
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
            // `preprocessed/` repeats the same rules.
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
        1346,
        "the documented syntax-rule count must match the artefacts"
    );
}

/// CEN's UBL unit-test suite in the reference corpus agrees with this crate:
/// the undecidable `BR-CO-05` … `BR-CO-08` and the type-retired `BR-DEC-*`
/// rules have no CEN unit test. (The converse is not asserted.)
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

    assert!(
        has_test("BR-IG-01") && has_test("BR-IP-01"),
        "CEN's suite names the IGIC and IPSI rules BR-IG-01 / BR-IP-01"
    );
    eprintln!("CEN unit-test instances: {} rules exercised", tested.len());
}

// ── Severities against each authority's configuration ────────────────────────

/// KoSIT's validator configuration, relative to the reference corpus.
const XR_SCENARIOS: &str = "validator-configuration-xrechnung/scenarios.xml";

/// The `customLevel` overrides in KoSIT's `scenarios.xml` this crate does not
/// apply: each re-levels a syntax rule no rule set here registers.
const OUT_OF_SCOPE_LEVELS: &[(&str, &str)] = &[
    (
        "UBL-CR-470",
        "UBL syntax rule (cac:PrepaidPayment); the UBL writer waives it for the \
         XRechnung Extension, which is what the override is for",
    ),
    (
        "UBL-CR-646",
        "UBL syntax rule (cac:SubInvoiceLine); the UBL writer waives it for the \
         XRechnung Extension, which is what the override is for",
    ),
    (
        "CII-SR-452",
        "CII syntax rule; registered by no rule set here",
    ),
    (
        "CII-SR-453",
        "CII syntax rule; registered by no rule set here",
    ),
    (
        "CII-SR-454",
        "CII syntax rule; registered by no rule set here",
    ),
    (
        "CII-SR-465",
        "CII syntax rule, raised to error in v2026-08-31; registered by no rule set here",
    ),
    (
        "CII-SR-466",
        "CII syntax rule, raised to error in v2026-08-31; registered by no rule set here",
    ),
    (
        "CII-SR-475",
        "CII syntax rule, lowered to information in v2026-08-31; registered by no rule \
         set here",
    ),
    (
        "CII-SR-476",
        "CII syntax rule, lowered to information in v2026-08-31; registered by no rule \
         set here",
    ),
];

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

/// KoSIT's UBL and CII Schematrons, relative to the reference corpus.
const XR_SCHEMATRONS: [&str; 2] = [
    "xrechnung-schematron/src/validation/schematron/ubl/XRechnung-UBL-validation.sch",
    "xrechnung-schematron/src/validation/schematron/cii/XRechnung-CII-validation.sch",
];

/// Every `<assert>`/`<report>` id in KoSIT's own Schematrons, with its `flag`.
/// Parsed, since some `test` expressions contain a literal `>`.
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

/// Every KoSIT rule runs at the severity its Schematron `flag` gives it.
/// (CEN rules re-levelled in `scenarios.xml` are checked by
/// [`the_severity_overrides_are_kosits_own`].)
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
    // Exact: the sources are pinned, so this moves only with a pin.
    assert_eq!(checked, 121, "KoSIT severities compared");
    eprintln!("KoSIT rule severities: {checked} compared against the Schematron");
}

/// Every Peppol rule runs at the flag Peppol's Schematron gives it, plus the
/// two rewrites in KoSIT's `peppol-into-xr.xsl`: `R120` becomes a warning under
/// XRechnung, and `R008` (retired by the model) is one shared instance.
#[test]
fn every_peppol_rule_runs_at_the_severity_its_authority_publishes() {
    let Some(root) = common::require("Peppol rule severities") else {
        return;
    };
    let sch = root.join(common::peppol::UBL_SCH);
    if !sch.exists() {
        eprintln!("note: {} absent — skipped", sch.display());
        return;
    }
    let published =
        common::peppol::flags(&std::fs::read_to_string(&sch).expect("read Peppol Schematron"));
    assert!(!published.is_empty(), "no Peppol assertions found");

    let level = |flag: &str| match flag {
        "fatal" => en16931::Severity::Fatal,
        "warning" => en16931::Severity::Warning,
        "information" => en16931::Severity::Info,
        other => panic!("unknown flag {other:?}"),
    };
    let mut checked = 0usize;
    let mut warnings = 0usize;
    for id in en16931::profiles::PEPPOL_BIS_3.check_ids() {
        let Some(flag) = published.get(id) else {
            continue;
        };
        assert_eq!(
            en16931::profiles::PEPPOL_BIS_3.severity_of(id),
            Some(level(flag)),
            "{id} under Peppol BIS Billing 3.0: Peppol publishes {flag}"
        );
        checked += 1;
        warnings += usize::from(flag == "warning");
    }
    assert_eq!(checked, 63, "every Peppol rule this crate runs");
    assert_eq!(warnings, 9, "the PEPPOL-COMMON-* warnings");

    // The rewrites, read from the stylesheet.
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
    assert!(
        xsl.contains("@id='PEPPOL-EN16931-R008' and $syntax='UBL'"),
        "the stylesheet no longer rewrites R008 for UBL; re-read what it does now"
    );
    let r008 = |p: &en16931::Profile| {
        p.extra_rules
            .iter()
            .find(|r| r.id.as_str() == "PEPPOL-EN16931-R008")
            .map(|r| std::ptr::from_ref::<en16931::validation::Rule>(*r))
    };
    assert_eq!(
        r008(&en16931::profiles::XRECHNUNG),
        r008(&en16931::profiles::PEPPOL_BIS_3),
        "both profiles hold the one retired R008"
    );
    eprintln!(
        "Peppol rule severities: {checked} compared, plus XRechnung's R120 and R008 rewrites"
    );
}

/// The XRechnung profiles' `levels` for CEN rules equal the `customLevel`s in
/// KoSIT's `scenarios.xml`. KoSIT's own ids (`BR-DE*`, `BR-TMP*`) are checked by
/// [`every_kosit_check_runs_at_the_severity_kosit_publishes`].
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

    // Every non-`BR-*` override must be listed in `OUT_OF_SCOPE_LEVELS`, and vice versa.
    let all: BTreeSet<String> = roxmltree::Document::parse(&xml)
        .expect("scenarios.xml is XML")
        .descendants()
        .filter(|n| n.tag_name().name() == "customLevel")
        .filter_map(|n| n.text().map(|t| t.trim().to_owned()))
        .filter(|id| !id.starts_with("BR-"))
        .collect();
    let listed: BTreeSet<String> = OUT_OF_SCOPE_LEVELS
        .iter()
        .map(|(id, _)| (*id).to_owned())
        .collect();
    assert_eq!(
        all, listed,
        "customLevel overrides that are neither BR-* nor recorded out of scope (left), \
         or recorded but no longer configured (right)"
    );
    assert!(
        OUT_OF_SCOPE_LEVELS
            .iter()
            .all(|(_, why)| !why.trim().is_empty())
    );

    let semantic = |(id, level): &(String, String)| {
        (!listed.contains(id)).then(|| (id.clone(), level.clone()))
    };
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
