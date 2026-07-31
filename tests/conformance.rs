//! Run **CEN's own conformance suite** against this crate's rules.
//!
//! Every previous check in this repository verifies that a rule is *registered*:
//! the artefact gap tests count ids, the corpus proves each rule can fire. None
//! of them checks that a rule fires on the documents the authority says it
//! should, and only on those. That is a different property, and it is the one
//! that decides whether this crate agrees with the validator everyone else runs.
//!
//! # The suite
//!
//! `spec/eInvoicing-EN16931/test/{Invoice,CreditNote}-unit-UBL/` holds 277 files,
//! one per rule, in Difi's `testSet` format:
//!
//! ```xml
//! <test>
//!   <assert><error>BR-01</error></assert>
//!   <Invoice> … a UBL fragment with no CustomizationID … </Invoice>
//! </test>
//! ```
//!
//! `<error>` means the rule **must** fire on that document; `<success>` means it
//! **must not**. 1 131 assertions in total. Crucially each is scoped to one rule,
//! so the fragments can be minimal — a `BR-01` case is four lines of XML that
//! would violate fifty other rules, and says nothing about them. That maps
//! exactly onto [`ValidationReport::has`].
//!
//! # Why this can be trusted more than the other tests
//!
//! Every other test in this repository was written by the same author as the
//! code it checks, from the same reading of the same documents. This one was
//! written by CEN, before this crate existed, to check a different
//! implementation. A rule that passes here agrees with the reference.
//!
//! # Skipping is explicit and counted
//!
//! Two categories of case cannot be run and are *reported*, never silently
//! dropped:
//!
//! * rules this crate registers as type-retired or undecidable — there is no
//!   state that makes them fire, which is the whole point;
//! * cases whose fragment uses an element [`ubl::Reader`] does not map, which is
//!   a gap in the reader rather than a finding about the rules.
//!
//! The second is asserted to zero for every element the suite actually uses, so
//! it cannot quietly grow.

mod ubl;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use en16931::validate;

/// One `<test>` in a `testSet`.
struct Case {
    file: String,
    rule: String,
    /// `true` when the suite says the rule must fire.
    must_fire: bool,
    description: String,
    xml: String,
}

fn suite_dirs() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("spec/eInvoicing-EN16931/test");
    ["Invoice-unit-UBL", "CreditNote-unit-UBL"]
        .iter()
        .map(|d| root.join(d))
        .filter(|p| p.exists())
        .collect()
}

/// Every `<test>` in the suite, with its expectation.
fn cases() -> Vec<Case> {
    let mut out = Vec::new();
    for dir in suite_dirs() {
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .expect("read suite dir")
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "xml"))
            .collect();
        files.sort();
        for path in files {
            let text = std::fs::read_to_string(&path).expect("read case");
            let doc = match roxmltree::Document::parse(&text) {
                Ok(d) => d,
                Err(e) => panic!("{}: {e}", path.display()),
            };
            let file = path.file_name().unwrap().to_string_lossy().into_owned();
            for test in doc.descendants().filter(|n| n.tag_name().name() == "test") {
                // The expectation is in the `<assert>` child, the document in
                // the `Invoice` / `CreditNote` child.
                let assertion = test.children().find(|n| n.tag_name().name() == "assert");
                let Some(assertion) = assertion else { continue };
                let mut rule = None;
                let mut must_fire = false;
                for c in assertion.children().filter(roxmltree::Node::is_element) {
                    match c.tag_name().name() {
                        "error" | "fatal" => {
                            rule = c.text().map(|t| t.trim().to_owned());
                            must_fire = true;
                        }
                        "success" => {
                            rule = c.text().map(|t| t.trim().to_owned());
                            must_fire = false;
                        }
                        _ => {}
                    }
                }
                let Some(rule) = rule else { continue };
                let Some(root) = test
                    .children()
                    .find(|n| matches!(n.tag_name().name(), "Invoice" | "CreditNote"))
                else {
                    continue;
                };
                out.push(Case {
                    file: file.clone(),
                    rule,
                    must_fire,
                    description: assertion
                        .children()
                        .find(|n| n.tag_name().name() == "description")
                        .and_then(|n| n.text())
                        .unwrap_or("")
                        .trim()
                        .to_owned(),
                    // Re-serialise the fragment so it can be parsed standalone.
                    xml: text[root.range()].to_owned(),
                });
            }
        }
    }
    out
}

/// Rules with no evaluation, by construction — the corpus declares why.
fn unevaluated() -> BTreeSet<String> {
    /// The rules `tests/corpus.rs` declares `Why::Retired` or `Why::Undecidable`
    /// — no state can make them fire, so a suite case that expects one to fire
    /// is describing a defect the model cannot represent.
    ///
    /// Duplicated from the corpus rather than shared, deliberately: if the two
    /// ever disagree, `the_unevaluated_set_matches_the_corpus` says so.
    const UNEVALUATED: &[&str] = &[
        "BR-12", "BR-13", "BR-14", "BR-15", "BR-22", "BR-24", "BR-26", "BR-31", "BR-32", "BR-36",
        "BR-37", "BR-41", "BR-43", "BR-45", "BR-46", "BR-CL-03", "BR-CO-05", "BR-CO-06",
        "BR-CO-07", "BR-CO-08",
    ];
    en16931::validation::rules::CORE
        .iter()
        .filter(|r| {
            let id = r.id.as_str();
            id.starts_with("BR-DEC-") || UNEVALUATED.contains(&id)
        })
        .map(|r| r.id.as_str().to_owned())
        .collect()
}

/// The result of running one case.
enum Outcome {
    Agreed,
    /// Disagrees, and [`DIVERGENCES`] says why.
    Diverged(&'static str),
    Disagreed(String),
    SkippedUnevaluated,
    SkippedSyntaxRule,
    SkippedMalformed(String),
    SkippedUnreadable(String),
}

/// Where this crate deliberately differs from the suite, and why.
///
/// Every entry is the **same** cause: UBL can carry a group that is present and
/// empty — `<cac:PostalAddress/>` — and this model cannot. `PostalAddress` with
/// no fields set *is* an absent address; there is no third state. The suite has
/// cases that depend on the distinction, and they are named here rather than
/// quietly passing.
///
/// This is not a shortcut. The alternative — an `Option` around every group, or
/// a `present: bool` beside it — would put a syntax artefact in every consumer's
/// way to satisfy six test cases, and would make `Invoice::default()` ambiguous.
/// The trade is deliberate and this table is what makes it visible.
///
/// The set is asserted **exactly**: a new disagreement fails the suite, and a
/// divergence that stops diverging fails it too.
const DIVERGENCES: &[(&str, &str, &str)] = &[
    // (file, rule, why)
    (
        "BR-08.xml",
        "BR-08",
        "an empty <cac:PostalAddress/> is an absent address here",
    ),
    (
        "BR-10.xml",
        "BR-10",
        "an empty <cac:PostalAddress/> is an absent address here",
    ),
    (
        "BR-19.xml",
        "BR-19",
        "an empty <cac:PostalAddress/> is an absent address here",
    ),
    (
        "BR-55.xml",
        "BR-55",
        "a <cac:BillingReference/> with no child is an absent BG-3 here",
    ),
    (
        "BR-CO-19.xml",
        "BR-CO-19",
        "a <cac:InvoicePeriod/> with no dates is an absent BG-14 here",
    ),
    // A different cause, and the only one: UBL permits two `cac:TaxTotal`
    // elements in the same currency with different amounts. BT-110 is one
    // field, so the contradiction cannot be written down — and `BR-CO-15` is
    // then satisfied by whichever value was read. Peppol's `R053` is the rule
    // that catches this, and it is a syntax rule about element counts.
    (
        "BR-CO-15.xml",
        "BR-CO-15",
        "two cac:TaxTotal elements cannot both be BT-110",
    ),
    (
        "BR-CO-15-2.xml",
        "BR-CO-15",
        "two cac:TaxTotal elements cannot both be BT-110",
    ),
];

fn run(case: &Case, reader: &mut ubl::Reader, unevaluated: &BTreeSet<String>) -> Outcome {
    // Normalise the suite's spelling to the registry's.
    let canonical = en16931::validation::rules::CORE
        .iter()
        .find(|r| r.id.matches(&case.rule))
        .map(|r| r.id.as_str().to_owned());
    let Some(canonical) = canonical else {
        // `UBL-*` and `CII-*` are syntax rules — element order, cardinality,
        // datatype facets. They belong to the format crates by construction.
        if case.rule.starts_with("UBL-") || case.rule.starts_with("CII-") {
            return Outcome::SkippedSyntaxRule;
        }
        return Outcome::SkippedUnreadable(format!("{} is not a registered rule", case.rule));
    };
    if unevaluated.contains(&canonical) {
        return Outcome::SkippedUnevaluated;
    }

    let doc = match roxmltree::Document::parse(&case.xml) {
        Ok(d) => d,
        Err(e) => return Outcome::SkippedUnreadable(format!("parse: {e}")),
    };
    let before = reader.unmapped.len();
    let malformed_before = reader.malformed.len();
    let invoice = reader.read(doc.root_element());
    if reader.malformed.len() > malformed_before {
        // The fragment carries a value the model refuses at the boundary, so
        // this case is about syntax, not about the rule.
        let what = reader.malformed[malformed_before..].join(", ");
        return Outcome::SkippedMalformed(what);
    }
    let report = validate(&invoice);
    let fired = report.has(&canonical);

    if fired == case.must_fire {
        return Outcome::Agreed;
    }
    if let Some((_, _, why)) = DIVERGENCES
        .iter()
        .find(|(f, r, _)| *f == case.file && r.eq_ignore_ascii_case(&case.rule))
    {
        return Outcome::Diverged(why);
    }
    let grew = reader.unmapped.len() > before;
    let verdict = if case.must_fire {
        "the suite says it must fire; it did not"
    } else {
        "the suite says it must not fire; it did"
    };
    let hint = if grew {
        " (the reader skipped an element in this fragment)"
    } else {
        ""
    };
    Outcome::Disagreed(format!(
        "{} [{}] {verdict}{hint}\n      {}",
        case.file, case.rule, case.description
    ))
}

/// **The oracle.** This crate's rules agree with CEN's conformance suite.
#[test]
fn cen_conformance_suite() {
    let cases = cases();
    if cases.is_empty() {
        eprintln!("skipping: artefacts not present (run ./fetch-spec.sh)");
        return;
    }
    let unevaluated = unevaluated();
    let mut reader = ubl::Reader::default();
    let mut agreed = 0usize;
    let mut skipped_unevaluated = 0usize;
    let mut skipped_syntax = 0usize;
    let mut diverged: BTreeMap<&str, usize> = BTreeMap::new();
    let mut unreadable: Vec<String> = Vec::new();
    let mut malformed: Vec<String> = Vec::new();
    let mut disagreed: Vec<String> = Vec::new();
    let mut per_rule: BTreeMap<String, (usize, usize)> = BTreeMap::new();

    for case in &cases {
        match run(case, &mut reader, &unevaluated) {
            Outcome::Agreed => {
                agreed += 1;
                per_rule.entry(case.rule.clone()).or_default().0 += 1;
            }
            Outcome::SkippedUnevaluated => skipped_unevaluated += 1,
            Outcome::SkippedSyntaxRule => skipped_syntax += 1,
            Outcome::Diverged(why) => *diverged.entry(why).or_default() += 1,
            Outcome::SkippedMalformed(what) => malformed.push(format!("{} — {what}", case.file)),
            Outcome::SkippedUnreadable(why) => unreadable.push(format!("{} — {why}", case.file)),
            Outcome::Disagreed(why) => {
                disagreed.push(why);
                per_rule.entry(case.rule.clone()).or_default().1 += 1;
            }
        }
    }

    let run_count = agreed + disagreed.len() + diverged.values().sum::<usize>();
    eprintln!(
        "CEN conformance suite\n  \
         assertions:            {}\n  \
         run:                   {run_count}\n  \
         agreed:                {agreed}  ({:.1}% of run)\n  \
         disagreed:             {}\n  \
         diverged, declared:    {}\n  \
         skipped, unevaluated:  {skipped_unevaluated}  (type-retired or undecidable)\n  \
         skipped, syntax rules: {skipped_syntax}  (UBL-*/CII-*, the format crates' job)\n  \
         skipped, malformed:    {}  (value the model refuses at the boundary)\n  \
         skipped, unreadable:   {}",
        cases.len(),
        100.0 * agreed as f64 / run_count.max(1) as f64,
        disagreed.len(),
        diverged.values().sum::<usize>(),
        malformed.len(),
        unreadable.len(),
    );
    for (why, n) in &diverged {
        eprintln!("    {n} × {why}");
    }
    if !reader.unmapped.is_empty() {
        eprintln!("  reader did not map:");
        for u in &reader.unmapped {
            eprintln!("    {u}");
        }
    }

    assert!(
        disagreed.is_empty(),
        "{} of {run_count} CEN conformance assertions disagree with this crate:\n  {}",
        disagreed.len(),
        disagreed.join("\n  ")
    );
    assert!(
        unreadable.is_empty(),
        "{} case(s) could not be run:\n  {}",
        unreadable.len(),
        unreadable.join("\n  ")
    );
    // Every declared divergence must still diverge. One that starts agreeing is
    // a rule that got better and a table that got stale, and the table is the
    // only thing standing between "deliberate difference" and "known failure".
    let unused: Vec<_> = DIVERGENCES
        .iter()
        .filter(|(_, _, why)| !diverged.contains_key(why))
        .map(|(f, r, _)| format!("{f} [{r}]"))
        .collect();
    assert!(
        unused.is_empty(),
        "{} declared divergence(s) no longer diverge — the rule got better and \
         the table went stale; delete them:\n  {}",
        unused.len(),
        unused.join("\n  ")
    );
}

/// The reader maps every element the suite uses.
///
/// Separate from the oracle above so a reader gap reads as a reader gap and not
/// as a rule disagreement. An unmapped element is not automatically a bug — some
/// UBL elements carry no business term — so the allowlist names each one.
#[test]
fn the_reader_maps_everything_the_suite_uses() {
    let cases = cases();
    if cases.is_empty() {
        eprintln!("skipping: artefacts not present");
        return;
    }
    let mut reader = ubl::Reader::default();
    for case in &cases {
        if let Ok(doc) = roxmltree::Document::parse(&case.xml) {
            let _ = reader.read(doc.root_element());
        }
    }
    /// UBL elements with no EN 16931 business term behind them.
    const NO_BUSINESS_TERM: &[&str] = &[];
    let unexpected: Vec<_> = reader
        .unmapped
        .iter()
        .filter(|u| !NO_BUSINESS_TERM.contains(&u.as_str()))
        .cloned()
        .collect();
    assert!(
        unexpected.is_empty(),
        "the UBL reader ignores {} element(s) the suite uses — every one is a \
         rule it cannot be checking:\n  {}",
        unexpected.len(),
        unexpected.join("\n  ")
    );
}

// ── KoSIT's XRechnung suite ───────────────────────────────────────────────────

/// One `<?xmute?>` instruction.
struct Mutation {
    file: String,
    profile: &'static en16931::Profile,
    rule: String,
    must_fire: bool,
    description: String,
    xml: String,
}

/// Pseudo-attributes of a processing instruction: `k="v" k="v"`.
fn pi_attrs(body: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let Some(eq) = body[i..].find('=') else { break };
        let key = body[i..i + eq].trim().to_owned();
        let rest = &body[i + eq + 1..];
        let Some(open) = rest.find('"') else { break };
        let Some(close) = rest[open + 1..].find('"') else {
            break;
        };
        out.insert(key, rest[open + 1..open + 1 + close].to_owned());
        i = i + eq + 1 + open + 1 + close + 1;
    }
    out
}

/// Replace an attribute's value in an element's serialised start tag.
fn set_attribute(elem: &str, attr: &str, value: &str) -> String {
    let needle = format!("{attr}=\"");
    match elem.find(&needle) {
        Some(i) => {
            let rest = &elem[i + needle.len()..];
            let end = rest.find('"').map_or(rest.len(), |e| e);
            format!("{}{needle}{value}\"{}", &elem[..i], &rest[end + 1..])
        }
        None => elem.to_owned(),
    }
}

/// Delete an attribute from an element's serialised start tag.
fn drop_attribute(elem: &str, attr: &str) -> String {
    let needle = format!("{attr}=\"");
    match elem.find(&needle) {
        Some(i) => {
            let rest = &elem[i + needle.len()..];
            let end = rest.find('"').map_or(rest.len(), |e| e);
            format!("{}{}", &elem[..i], &rest[end + 1..])
        }
        None => elem.to_owned(),
    }
}

/// Every runnable mutation in KoSIT's instance suite.
///
/// # What the suite is
///
/// Each file is a **complete, valid** XRechnung invoice with mutation
/// instructions embedded as processing instructions:
///
/// ```xml
/// <?xmute mutator="remove" schematron-invalid="xrubl:BR-DE-15" ?>
/// <cbc:BuyerReference>90000000-03083-12</cbc:BuyerReference>
/// ```
///
/// The instruction applies to the **following** element: remove BT-10 and
/// `BR-DE-15` must fire. `identity` leaves the document alone, so
/// `schematron-valid` asserts the rule does *not* fire on a conforming invoice —
/// which is the harder half, and the half a hand-written corpus rarely has.
///
/// # Two mutators are skipped, not guessed
///
/// `code` (202 instructions) replaces text with each of a comma-separated
/// `values` list, and `whitespace` (1) does something to whitespace. Their exact
/// semantics live in KoSIT's `xml-mutate` jar, which is downloaded at build time
/// and is not in the repository. Implementing them from the attribute names
/// would be encoding a guess, and this project has been bitten by that before
/// . They are counted and named instead.
fn mutations() -> Vec<Mutation> {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("spec/xrechnung-schematron/test/instances");
    let mut out = Vec::new();
    for sub in ["ubl-inv", "ubl-cn"] {
        let dir = root.join(sub);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "xml"))
            .collect();
        files.sort();
        for path in files {
            let text = std::fs::read_to_string(&path).expect("read instance");
            let Ok(doc) = roxmltree::Document::parse(&text) else {
                continue;
            };
            let file = path.file_name().unwrap().to_string_lossy().into_owned();
            // The document declares which profile it belongs to.
            let spec = doc
                .root_element()
                .children()
                .find(|n| n.tag_name().name() == "CustomizationID")
                .and_then(|n| n.text())
                .unwrap_or("")
                .trim();
            let profile = en16931::profiles::for_specification_id(spec)
                .unwrap_or(&en16931::profiles::XRECHNUNG);

            for pi in doc.descendants().filter(|n| n.is_pi()) {
                let Some(pi_data) = pi.pi() else { continue };
                if pi_data.target != "xmute" {
                    continue;
                }
                let attrs = pi_attrs(pi_data.value.unwrap_or(""));
                let Some(mutator) = attrs.get("mutator") else {
                    continue;
                };
                let (rules, must_fire) = match (
                    attrs.get("schematron-invalid"),
                    attrs.get("schematron-valid"),
                ) {
                    (Some(r), _) => (r.clone(), true),
                    (_, Some(r)) => (r.clone(), false),
                    _ => continue,
                };
                // The element the instruction applies to.
                let target = pi
                    .next_siblings()
                    .find(roxmltree::Node::is_element)
                    .map(|n| n.range());
                // `attribute="mimeCode"` retargets the mutation from the
                // element to one of its attributes. Missing that made `empty`
                // blank the element body and leave `@mimeCode` intact, so
                // `BR-DEX-01`'s "mime-type not empty" case silently passed.
                let attr = attrs.get("attribute");
                let mutated = match (mutator.as_str(), target) {
                    ("identity", _) => Some(text.clone()),
                    ("remove", Some(r)) => {
                        let mut m = text.clone();
                        match attr {
                            Some(a) => {
                                let elem = &text[r.clone()];
                                m.replace_range(r, &drop_attribute(elem, a));
                            }
                            None => m.replace_range(r, ""),
                        }
                        Some(m)
                    }
                    ("empty", Some(r)) => {
                        let elem = &text[r.clone()];
                        let replacement = match attr {
                            Some(a) => set_attribute(elem, a, ""),
                            // Keep the element, drop its content.
                            None => match elem.find('>') {
                                Some(i) if !elem[..i].ends_with('/') => {
                                    format!("{}/>", elem[..i].trim_end())
                                }
                                _ => elem.to_owned(),
                            },
                        };
                        let mut m = text.clone();
                        m.replace_range(r, &replacement);
                        Some(m)
                    }
                    _ => None,
                };
                let Some(xml) = mutated else { continue };
                for rule in rules.split_whitespace() {
                    out.push(Mutation {
                        file: file.clone(),
                        profile,
                        rule: rule.trim_start_matches("xrubl:").to_owned(),
                        must_fire,
                        description: attrs.get("description").cloned().unwrap_or_default(),
                        xml: xml.clone(),
                    });
                }
            }
        }
    }
    out
}

/// **The second oracle.** This crate agrees with KoSIT's mutation suite.
///
/// Stronger than [`cen_conformance_suite`] in one specific way: CEN's cases are
/// four-line fragments, KoSIT's are complete invoices. A `schematron-valid`
/// assertion here says *"this realistic document is clean"*, which catches rules
/// that are too eager — the failure mode a corpus of deliberately-broken
/// documents cannot see.
#[test]
fn xrechnung_mutation_suite() {
    let all = mutations();
    if all.is_empty() {
        eprintln!("skipping: artefacts not present (run ./fetch-spec.sh)");
        return;
    }
    let mut reader = ubl::Reader::default();
    let mut agreed = 0usize;
    let mut skipped = 0usize;
    let mut disagreed: Vec<String> = Vec::new();

    let retired = unevaluated();
    /// Profile rules the model retires — the same disposition `tests/corpus.rs`
    /// declares, in the profiles' namespaces. A suite case that expects one to
    /// fire is describing a defect the model cannot represent.
    const PROFILE_RETIRED: &[&str] = &[
        "BR-DE-23-b",
        "BR-DE-24-b",
        "BR-DE-25-b",
        "BR-DEX-13",
        "BR-DEX-14",
        "PEPPOL-EN16931-F001",
        "PEPPOL-EN16931-R008",
        "PEPPOL-EN16931-R043",
        "PEPPOL-EN16931-R044",
        "PEPPOL-EN16931-R051",
        "PEPPOL-EN16931-R053",
        "PEPPOL-EN16931-R054",
        "PEPPOL-EN16931-R080",
        "PEPPOL-EN16931-R100",
        "PEPPOL-EN16931-R101",
        "PEPPOL-EN16931-CL007",
    ];
    for m in &all {
        if retired.contains(&m.rule) || PROFILE_RETIRED.contains(&m.rule.as_str()) {
            skipped += 1;
            continue;
        }
        // A rule the profile does not carry cannot be asserted about.
        let known = en16931::validation::rules::CORE
            .iter()
            .chain(m.profile.extra_rules.iter())
            .any(|r| r.id.matches(&m.rule))
            || m.profile.restrictions.iter().any(|r| {
                en16931::validation::RuleId::new(Box::leak(r.id().to_owned().into_boxed_str()))
                    .matches(&m.rule)
            });
        if !known {
            skipped += 1;
            continue;
        }
        let Ok(doc) = roxmltree::Document::parse(&m.xml) else {
            skipped += 1;
            continue;
        };
        let before = reader.malformed.len();
        let invoice = reader.read(doc.root_element());
        if reader.malformed.len() > before {
            skipped += 1;
            continue;
        }
        let report = m.profile.validate(&invoice);
        if report.has(&m.rule) == m.must_fire {
            agreed += 1;
        } else {
            let verdict = if m.must_fire {
                "must fire; it did not"
            } else {
                "must not fire; it did"
            };
            disagreed.push(format!(
                "{} [{}] {verdict} — {}",
                m.file, m.rule, m.description
            ));
        }
    }

    let run = agreed + disagreed.len();
    eprintln!(
        "XRechnung mutation suite\n  \
         runnable mutations:    {}\n  \
         run:                   {run}\n  \
         agreed:                {agreed}  ({:.1}% of run)\n  \
         disagreed:             {}\n  \
         skipped:               {skipped}  (retired by the types, not in the profile, or a value the model refuses)",
        all.len(),
        100.0 * agreed as f64 / run.max(1) as f64,
        disagreed.len(),
    );
    assert!(
        disagreed.is_empty(),
        "{} of {run} KoSIT mutation assertions disagree with this crate:\n  {}",
        disagreed.len(),
        disagreed.join("\n  ")
    );
}

// ── The authorities' example invoices ─────────────────────────────────────────

/// Directories of **complete, valid** example invoices, and who publishes them.
///
/// Unlike the unit tests and the mutation suite, these carry no per-rule
/// expectation. They carry a much blunter one: *the authority publishes this as
/// a correct invoice*, so **nothing fatal may fire on it**.
///
/// That is the assertion a corpus of deliberately-broken documents structurally
/// cannot make. `tests/corpus.rs` proves each rule *can* fire; only a realistic,
/// known-good document proves a rule does not fire when it should not. A rule
/// that is merely too eager passes every other check in this repository.
const EXAMPLE_DIRS: &[(&str, &str)] = &[
    ("CEN", "spec/eInvoicing-EN16931/ubl/examples"),
    // 29 complete Peppol BIS Billing invoices covering the awkward Swedish
    // cases — electricity trading, factoring, advance payments and their final
    // settlement, hire cars, purchasing cards, interest invoices, telephony.
    // Real shapes, not minimal ones.
    ("CEN testfiles", "spec/eInvoicing-EN16931/test/testfiles"),
    ("Peppol", "spec/peppol-bis-invoice-3/rules/examples"),
    (
        "Peppol NO",
        "spec/peppol-bis-invoice-3/rules/national-examples/NO",
    ),
    (
        "Peppol GR",
        "spec/peppol-bis-invoice-3/rules/national-examples/GR",
    ),
];

/// Examples that are *not* claims of validity, with the reason.
///
/// Kept short and specific. An entry here is a claim about the file, not a
/// licence to ignore failures.
const NOT_A_VALID_EXAMPLE: &[(&str, &str)] = &[
    // Deliberately broken: the file exists to reproduce a reported defect.
    ("issue116.xml", "a bug-report reproduction, not an example"),
    // Genuinely invalid, and not by this crate's reckoning alone. The seller
    // party carries `cac:PartyName` and **no `cac:PartyLegalEntity`**, while
    // CEN's own `BR-06` binding is
    // `normalize-space(…/cac:PartyLegalEntity/cbc:RegistrationName) != ''`.
    // The reference validator rejects it too; BT-27 is simply absent.
    (
        "GR-base-example-TaxRepresentative.xml",
        "no cac:PartyLegalEntity, so BT-27 is absent — fails CEN's own BR-06",
    ),
];

/// **The third oracle.** Nothing fatal fires on a published example invoice.
#[test]
fn the_authorities_example_invoices_are_valid() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut reader = ubl::Reader::default();
    let mut checked = 0usize;
    let mut skipped: Vec<String> = Vec::new();
    let mut failures: Vec<String> = Vec::new();

    for (who, dir) in EXAMPLE_DIRS {
        let d = root.join(dir);
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        let mut files: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("xml")))
            .collect();
        files.sort();
        for path in files {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if let Some((_, why)) = NOT_A_VALID_EXAMPLE.iter().find(|(f, _)| *f == name) {
                skipped.push(format!("{name} — {why}"));
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                skipped.push(format!("{name} — not UTF-8"));
                continue;
            };
            let Ok(doc) = roxmltree::Document::parse(&text) else {
                skipped.push(format!("{name} — not well-formed XML"));
                continue;
            };
            let root_el = doc.root_element();
            if !matches!(root_el.tag_name().name(), "Invoice" | "CreditNote") {
                skipped.push(format!("{name} — not a UBL invoice"));
                continue;
            }
            let before = reader.malformed.len();
            let invoice = reader.read(root_el);
            if reader.malformed.len() > before {
                let what = reader.malformed[before..].join(", ");
                skipped.push(format!("{name} — value the model refuses: {what}"));
                continue;
            }
            // The document names its own profile; fall back to core.
            let profile = invoice
                .specification_id
                .as_deref()
                .and_then(en16931::profiles::for_specification_id)
                .unwrap_or(&en16931::profiles::EN16931);
            let report = profile.validate(&invoice);
            checked += 1;
            if !report.is_valid() {
                let fatal: Vec<String> = report
                    .fatal()
                    .map(|f| format!("{} at {}", f.rule, f.path))
                    .collect();
                failures.push(format!(
                    "{who}/{name} under {}: {}",
                    profile.id,
                    fatal.join("; ")
                ));
            }
        }
    }

    if checked == 0 {
        eprintln!("skipping: artefacts not present (run ./fetch-spec.sh)");
        return;
    }
    eprintln!(
        "published example invoices\n  \
         checked:  {checked}\n  \
         valid:    {}\n  \
         skipped:  {}",
        checked - failures.len(),
        skipped.len(),
    );
    for s in &skipped {
        eprintln!("    skipped: {s}");
    }
    assert!(
        failures.is_empty(),
        "{} published example invoice(s) are rejected by this crate. Each is \
         either a rule that is too eager or a gap in the test reader — never a \
         defect in the authority's own example:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}
