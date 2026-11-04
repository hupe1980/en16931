//! Runs CEN's UBL unit-test suite, KoSIT's XRechnung mutation suite and the
//! published example invoices through [`en16931_formats::read`] and the rules.
//!
//! CEN cases are Difi `testSet` files: `<error>` means the rule must fire on the
//! fragment, `<success>` that it must not; each maps onto `ValidationReport::has`.
//! Cases for type-retired or undecidable rules, and fragments the reader reports
//! as [`Kind::Malformed`], are skipped and counted (an absent `cbc:ChargeIndicator`
//! still runs, see [`absent_charge_indicator`]).

#![cfg(feature = "ubl")]

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use en16931::validate;
use en16931_formats::report::Kind;

/// Read a document with the production reader.
fn read(xml: &str) -> Result<en16931_formats::Read, en16931_formats::Error> {
    en16931_formats::read(xml.as_bytes())
}

/// The read's [`Kind::Malformed`] entries, joined; empty when none.
fn malformed(read: &en16931_formats::Read) -> String {
    join(read.entries_of(Kind::Malformed))
}

/// Whether an entry reports an absent `cbc:ChargeIndicator`.
///
/// CEN's minimal fragments omit it (all `BR-28` and both `BR-CL-17` cases), so
/// these cases run as allowances and are counted in [`CEN_ABSENT_CHARGE_INDICATOR`].
/// A present non-boolean indicator is still a refusal.
fn absent_charge_indicator(e: &en16931_formats::report::Entry) -> bool {
    e.kind == Kind::Malformed && e.value.is_none() && e.path.ends_with("/ChargeIndicator")
}

fn join<'a>(entries: impl Iterator<Item = &'a en16931_formats::report::Entry>) -> String {
    entries
        .map(|e| match &e.value {
            Some(v) => format!("{}={v}", e.path),
            None => e.path.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// One `<test>` in a `testSet`.
struct Case {
    file: String,
    rule: String,
    /// `true` when the suite says the rule must fire.
    must_fire: bool,
    description: String,
    xml: String,
}

/// Exact counts of what each suite runs at the pinned releases (CEN
/// `validation-1.3.17`, KoSIT `v2.6.0`, Peppol BIS Billing 3.0.21), so a
/// truncated fetch or moved directory cannot pass with a smaller denominator.
/// CEN cases for the type-retired rules at the pinned release, each judged by
/// the reader's report rather than skipped.
const CEN_BY_TYPE_ASSERTIONS: usize = 66;

const CEN_ASSERTIONS: usize = 1_025;
/// See [`CEN_ASSERTIONS`]. 381 mutations run of 471 runnable.
const KOSIT_MUTATIONS: usize = 381;
/// See [`CEN_ASSERTIONS`].
const EXAMPLE_INVOICES: usize = 59;
/// Registered core rules for which no CEN unit-test file states any expectation.
const CEN_UNCOVERED_RULES: usize = 35;
/// CEN cases that run despite an absent `cbc:ChargeIndicator`.
const CEN_ABSENT_CHARGE_INDICATOR: usize = 8;
/// CEN cases that disagree for a reason [`DIVERGENCES`] declares.
const CEN_DIVERGED: usize = 11;

/// One line of a suite summary: label, right-aligned count, optional note.
fn line(label: &str, n: usize, note: &str) -> String {
    let label = format!("{label}:");
    if note.is_empty() {
        format!("  {label:<22}{n:>5}")
    } else {
        format!("  {label:<22}{n:>5}  {note}")
    }
}

/// The XML files directly in `dir`, sorted.
fn xml_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "xml"))
        .collect();
    files.sort();
    files
}

fn suite_dirs() -> Vec<PathBuf> {
    let Some(root) = common::require("CEN unit tests") else {
        return Vec::new();
    };
    let root = root.join("eInvoicing-EN16931/test");
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
        for path in xml_files(&dir) {
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
                // A test may state several expectations (`BR-29.xml`,
                // `BR-30.xml`); each becomes its own case.
                let expectations: Vec<(String, bool)> = assertion
                    .children()
                    .filter(roxmltree::Node::is_element)
                    .filter_map(|c| {
                        let must_fire = match c.tag_name().name() {
                            "error" | "fatal" => true,
                            "success" => false,
                            _ => return None,
                        };
                        Some((c.text()?.trim().to_owned(), must_fire))
                    })
                    .collect();
                let Some(root) = test
                    .children()
                    .find(|n| matches!(n.tag_name().name(), "Invoice" | "CreditNote"))
                else {
                    continue;
                };
                let description = assertion
                    .children()
                    .find(|n| n.tag_name().name() == "description")
                    .and_then(|n| n.text())
                    .unwrap_or("")
                    .trim()
                    .to_owned();
                for (rule, must_fire) in expectations {
                    out.push(Case {
                        file: file.clone(),
                        rule,
                        must_fire,
                        description: description.clone(),
                        // Re-serialise the fragment so it can be parsed standalone.
                        xml: text[root.range()].to_owned(),
                    });
                }
            }
        }
    }
    out
}

/// Rules with no evaluation, by construction — the corpus declares why.
fn unevaluated() -> BTreeSet<String> {
    /// The rules `tests/corpus.rs` declares `Why::Retired` or `Why::Undecidable`.
    /// Duplicated, not shared; `the_unevaluated_set_matches_the_corpus` keeps them in sync.
    ///
    /// The type-retired rules are in it too: `the_unevaluated_set_matches_the_corpus`
    /// compares against the corpus's whole list. They are not *skipped*: a case
    /// for one of [`BY_TYPE`] is judged by the reader's report instead.
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

/// The rules the model's types retire, and the term each requires. No state of
/// a built invoice can make them fire; a *document* can lack the term, and the
/// reader must then say so (`Kind::Missing`) — which is what these cases check.
/// `BR-CL-03` (currency codes) is the reader's `@currencyID` check: an amount
/// in another currency than BT-5 is `Malformed`, and an invalid BT-5 is
/// `BR-CL-04`'s.
const BY_TYPE: &[(&str, &str)] = &[
    ("BR-12", "BT-106"),
    ("BR-13", "BT-109"),
    ("BR-14", "BT-112"),
    ("BR-15", "BT-115"),
    ("BR-22", "BT-129"),
    ("BR-24", "BT-131"),
    ("BR-26", "BT-146"),
    ("BR-31", "BT-92"),
    ("BR-32", "BT-95"),
    ("BR-36", "BT-99"),
    ("BR-37", "BT-102"),
    ("BR-41", "BT-136"),
    ("BR-43", "BT-141"),
    ("BR-45", "BT-116"),
    ("BR-46", "BT-117"),
    ("BR-CL-03", "currency"),
];

/// Whether the reader's report answers for a type-retired rule: an entry that
/// the term is missing or unreadable, or for `BR-CL-03` a foreign currency (or
/// `BR-CL-04` on an invalid BT-5).
fn reader_reports(read: &en16931_formats::Read, term: &str) -> bool {
    if term == "currency" {
        return read.entries.iter().any(|e| {
            e.kind == Kind::Malformed
                && (e.message.contains("not in the invoice currency")
                    || e.message.contains("not an ISO 4217 currency code"))
        }) || validate(&read.invoice).has("BR-CL-04");
    }
    read.entries.iter().any(|e| {
        matches!(e.kind, Kind::Missing | Kind::Malformed)
            && e.term
                .as_deref()
                .is_some_and(|t| t == term || t.ends_with(&format!("/{term}")))
    })
}

/// The result of running one case.
enum Outcome {
    /// Agreed; `true` when the fragment omitted `cbc:ChargeIndicator`.
    Agreed(bool),
    /// Disagrees, and this [`DIVERGENCES`] entry says why.
    Diverged(&'static (&'static str, &'static str, &'static str)),
    Disagreed(String),
    SkippedUnevaluated,
    /// A type-retired rule, judged by the reader's report: agreed or not.
    ByType(Result<(), String>),
    SkippedSyntaxRule,
    SkippedMalformed(String),
    SkippedUnreadable(String),
}

/// Where this crate differs from the suite, and why. Asserted exactly: a new
/// disagreement fails, and so does a divergence that stops diverging.
///
/// Mostly one cause: an empty UBL group such as `<cac:PostalAddress/>` is an
/// absent group in this model.
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
    // UBL permits two `cac:TaxTotal` in the same currency; BT-110 is the first,
    // and the reader reports the second as `Unmapped` (Peppol `R053` catches it).
    (
        "BR-CO-15.xml",
        "BR-CO-15",
        "two cac:TaxTotal elements cannot both be BT-110",
    ),
];

fn run(case: &Case, unevaluated: &BTreeSet<String>) -> Outcome {
    // Normalise the suite's spelling to the registry's.
    let canonical = en16931::validation::rules::CORE
        .iter()
        .find(|r| r.id.matches(&case.rule))
        .map(|r| r.id.as_str().to_owned());
    let Some(canonical) = canonical else {
        // `UBL-*` and `CII-*` are syntax rules — element order, cardinality,
        // datatype facets. They belong to `en16931-formats` by construction.
        if case.rule.starts_with("UBL-") || case.rule.starts_with("CII-") {
            return Outcome::SkippedSyntaxRule;
        }
        return Outcome::SkippedUnreadable(format!("{} is not a registered rule", case.rule));
    };
    if let Some((_, term)) = BY_TYPE.iter().find(|(r, _)| *r == canonical) {
        let read = match read(&case.xml) {
            Ok(r) => r,
            Err(e) => return Outcome::SkippedUnreadable(format!("read: {e}")),
        };
        let reported = reader_reports(&read, term);
        return Outcome::ByType(if reported == case.must_fire {
            Ok(())
        } else {
            Err(format!(
                "{} [{}] the suite says it must {}; the reader {} {term}\n      {}",
                case.file,
                case.rule,
                if case.must_fire { "fire" } else { "not fire" },
                if reported {
                    "reported"
                } else {
                    "did not report"
                },
                case.description
            ))
        });
    }
    if unevaluated.contains(&canonical) {
        return Outcome::SkippedUnevaluated;
    }

    let read = match read(&case.xml) {
        Ok(r) => r,
        Err(e) => return Outcome::SkippedUnreadable(format!("read: {e}")),
    };
    let what = join(
        read.entries_of(Kind::Malformed)
            .filter(|e| !absent_charge_indicator(e)),
    );
    if !what.is_empty() {
        // The fragment carries a value the model refuses; the case is about syntax.
        return Outcome::SkippedMalformed(what);
    }
    let no_indicator = read.entries.iter().any(absent_charge_indicator);
    let report = validate(&read.invoice);
    let fired = report.has(&canonical);

    if fired == case.must_fire {
        return Outcome::Agreed(no_indicator);
    }
    if let Some(declared) = DIVERGENCES
        .iter()
        .find(|(f, r, _)| *f == case.file && r.eq_ignore_ascii_case(&case.rule))
    {
        return Outcome::Diverged(declared);
    }
    let grew = read.entries_of(Kind::Unmapped).next().is_some();
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
        eprintln!("skipping: artefacts not present (run `cargo xtask fetch`)");
        return;
    }
    let unevaluated = unevaluated();
    let mut agreed = 0usize;
    let mut absent_indicator = 0usize;
    let mut skipped_unevaluated = 0usize;
    let mut by_type_agreed = 0usize;
    let mut by_type_disagreed: Vec<String> = Vec::new();
    let mut skipped_syntax = 0usize;
    // Keyed by (file, rule): several entries share one reason.
    let mut diverged: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    let mut unreadable: Vec<String> = Vec::new();
    let mut malformed: Vec<String> = Vec::new();
    let mut disagreed: Vec<String> = Vec::new();
    let mut per_rule: BTreeMap<String, (usize, usize)> = BTreeMap::new();

    for case in &cases {
        match run(case, &unevaluated) {
            Outcome::Agreed(no_indicator) => {
                agreed += 1;
                absent_indicator += usize::from(no_indicator);
                per_rule.entry(case.rule.clone()).or_default().0 += 1;
            }
            Outcome::SkippedUnevaluated => skipped_unevaluated += 1,
            Outcome::ByType(Ok(())) => by_type_agreed += 1,
            Outcome::ByType(Err(why)) => by_type_disagreed.push(why),
            Outcome::SkippedSyntaxRule => skipped_syntax += 1,
            Outcome::Diverged((file, rule, _)) => *diverged.entry((file, rule)).or_default() += 1,
            Outcome::SkippedMalformed(what) => malformed.push(format!("{} — {what}", case.file)),
            Outcome::SkippedUnreadable(why) => unreadable.push(format!("{} — {why}", case.file)),
            Outcome::Disagreed(why) => {
                disagreed.push(why);
                per_rule.entry(case.rule.clone()).or_default().1 += 1;
            }
        }
    }

    let diverged_total: usize = diverged.values().sum();
    let run_count = agreed + disagreed.len() + diverged_total;
    let core = &en16931::validation::rules::CORE;
    let uncovered: Vec<&str> = core
        .iter()
        .filter(|r| !cases.iter().any(|c| r.id.matches(&c.rule)))
        .map(|r| r.id.as_str())
        .collect();
    let files: usize = suite_dirs().iter().map(|d| xml_files(d).len()).sum();
    // Per reason, in the order `DIVERGENCES` declares them.
    let mut by_reason: Vec<(&str, usize)> = Vec::new();
    for (file, rule, why) in DIVERGENCES {
        let n = diverged.get(&(*file, *rule)).copied().unwrap_or(0);
        match by_reason.iter_mut().find(|(w, _)| w == why) {
            Some((_, total)) => *total += n,
            None => by_reason.push((why, n)),
        }
    }
    let mut summary = vec![
        "CEN conformance suite".to_owned(),
        line("files", files, ""),
        line("assertions", cases.len(), ""),
        line("run", run_count, ""),
        line(
            "agreed",
            agreed,
            &format!(
                "({:.1}% of run)",
                100.0 * agreed as f64 / run_count.max(1) as f64
            ),
        ),
        line("disagreed", disagreed.len(), ""),
        line("diverged, declared", diverged_total, ""),
    ];
    summary.extend(
        by_reason
            .iter()
            .map(|(why, n)| format!("    {n:>2} × {why}")),
    );
    summary.extend([
        line(
            "type-retired, by the reader",
            by_type_agreed,
            "(the term reported missing exactly when the suite expects the rule)",
        ),
        line("skipped, unevaluated", skipped_unevaluated, "(undecidable)"),
        line(
            "skipped, syntax rules",
            skipped_syntax,
            "(UBL-*/CII-*, the formats crate's job)",
        ),
        line(
            "skipped, malformed",
            malformed.len(),
            "(a value the model refuses)",
        ),
        line("skipped, unreadable", unreadable.len(), ""),
        line(
            "run, no indicator",
            absent_indicator,
            "(absent cbc:ChargeIndicator, read as an allowance)",
        ),
        format!(
            "  core rules with no CEN expectation: {} of {}",
            uncovered.len(),
            core.len()
        ),
    ]);
    let summary = summary.join("\n");
    eprintln!("{summary}");
    for d in disagreed.iter().chain(&by_type_disagreed) {
        eprintln!("  DISAGREES: {d}");
    }
    if !malformed.is_empty() {
        eprintln!("  malformed:");
        for m in &malformed {
            eprintln!("    {m}");
        }
    }

    // An exact count of what ran; see [`CEN_ASSERTIONS`].
    assert_eq!(
        run_count, CEN_ASSERTIONS,
        "{run_count} CEN assertions ran, and {CEN_ASSERTIONS} run at the pinned release. \
         A partial fetch, a directory moved upstream, or a pin bump that has to say so."
    );
    assert_eq!(
        uncovered.len(),
        CEN_UNCOVERED_RULES,
        "{} registered core rules have no CEN expectation, and {CEN_UNCOVERED_RULES} have \
         none at the pinned release: {uncovered:?}",
        uncovered.len()
    );
    assert_eq!(
        absent_indicator, CEN_ABSENT_CHARGE_INDICATOR,
        "{absent_indicator} agreeing CEN cases omitted cbc:ChargeIndicator, and \
         {CEN_ABSENT_CHARGE_INDICATOR} do at the pinned release"
    );
    assert!(
        disagreed.is_empty(),
        "{} of {run_count} CEN conformance assertions disagree with this crate:\n  {}",
        disagreed.len(),
        disagreed.join("\n  ")
    );
    // The type-retired rules: every case judged by the reader's report, none
    // skipped, and every one agreeing.
    assert!(
        by_type_disagreed.is_empty(),
        "{} CEN case(s) for type-retired rules disagree with the reader's report:\n  {}",
        by_type_disagreed.len(),
        by_type_disagreed.join("\n  ")
    );
    assert_eq!(
        by_type_agreed, CEN_BY_TYPE_ASSERTIONS,
        "{by_type_agreed} CEN cases for type-retired rules were judged by the reader, and \
         {CEN_BY_TYPE_ASSERTIONS} are at the pinned release"
    );
    assert!(
        unreadable.is_empty(),
        "{} case(s) could not be run:\n  {}",
        unreadable.len(),
        unreadable.join("\n  ")
    );
    // Every declared divergence must still diverge.
    let unused: Vec<_> = DIVERGENCES
        .iter()
        .filter(|(f, r, _)| !diverged.contains_key(&(*f, *r)))
        .map(|(f, r, _)| format!("{f} [{r}]"))
        .collect();
    assert!(
        unused.is_empty(),
        "{} declared divergence(s) no longer diverge — the rule got better and \
         the table went stale; delete them:\n  {}",
        unused.len(),
        unused.join("\n  ")
    );
    assert_eq!(
        diverged_total, CEN_DIVERGED,
        "{diverged_total} CEN cases diverge as declared, and {CEN_DIVERGED} do at the pinned release"
    );

    // The conformance page pastes this summary and quotes its figures.
    common::assert_page_pastes("conformance.md", &summary);
    common::assert_page_states(
        "conformance.md",
        &[
            ("<N> files and", files),
            ("files and <N> assertions", cases.len()),
            ("about <N> of the", uncovered.len()),
            ("of the <N> core rules", core.len()),
        ],
    );
}

/// The reader maps every element the suite uses; the allowlists name each exception.
#[test]
fn the_reader_maps_everything_the_suite_uses() {
    let cases = cases();
    if cases.is_empty() {
        eprintln!("skipping: artefacts not present");
        return;
    }
    // A document the reader refuses outright is `cen_conformance_suite`'s to report.
    /// `(file, path)`: a mapped element the model has no place for in this document.
    const NO_TERM_IN_THIS_DOCUMENT: &[(&str, &str)] = &[
        // A second VAT total in the invoice currency; BT-110 is the first.
        ("BR-CO-15.xml", "CreditNote/TaxTotal/TaxAmount"),
        ("BR-CO-15.xml", "Invoice/TaxTotal/TaxAmount"),
    ];
    let mut unmapped: BTreeSet<String> = BTreeSet::new();
    let mut excused = BTreeSet::new();
    for case in &cases {
        if let Ok(read) = read(&case.xml) {
            for e in read.entries_of(Kind::Unmapped) {
                if let Some(key) = NO_TERM_IN_THIS_DOCUMENT
                    .iter()
                    .find(|(f, p)| *f == case.file && *p == e.path)
                {
                    excused.insert(*key);
                } else {
                    unmapped.insert(e.path.clone());
                }
            }
        }
    }
    // Exact: an unused entry is a stale table.
    assert_eq!(
        excused.into_iter().collect::<Vec<_>>(),
        NO_TERM_IN_THIS_DOCUMENT,
        "the excused (file, path) pairs and the table differ"
    );
    /// UBL elements with no EN 16931 business term behind them.
    ///
    /// A `cbc:DocumentTypeCode` other than 130: on BG-24 the binding maps no
    /// code (`BR-CL-07`'s cases use 100 to show a reference that is not
    /// BT-18), and on a line it is not BT-128 (`PEPPOL-EN16931-R101`). The
    /// reader says it, as `Unmapped`, rather than dropping it.
    ///
    /// A `cac:PartyTaxScheme` with no scheme identifier: CEN binds BT-31 and
    /// BT-32 by the scheme (`= 'VAT'`, `!= 'VAT'`), so it is neither. A tax
    /// representative's registration under another scheme than VAT: BG-11
    /// holds only BT-63.
    const NO_BUSINESS_TERM: &[&str] = &[
        "AdditionalDocumentReference/DocumentTypeCode",
        "InvoiceLine/DocumentReference/DocumentTypeCode",
        "Party/PartyTaxScheme/TaxScheme",
        "TaxRepresentativeParty/PartyTaxScheme",
    ];
    let unexpected: Vec<_> = unmapped
        .into_iter()
        .filter(|u| !NO_BUSINESS_TERM.contains(&u.as_str()))
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
            let end = rest.find('"').unwrap_or(rest.len());
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
            let end = rest.find('"').unwrap_or(rest.len());
            format!("{}{}", &elem[..i], &rest[end + 1..])
        }
        None => elem.to_owned(),
    }
}

/// Every runnable mutation in KoSIT's instance suite.
///
/// Each file is a valid XRechnung invoice with `<?xmute?>` instructions that
/// apply to the following element, e.g. `mutator="remove"
/// schematron-invalid="xrubl:BR-DE-15"`. `identity` + `schematron-valid` asserts
/// the rule does not fire. The `code` and `whitespace` mutators are skipped and
/// counted: their semantics live in KoSIT's `xml-mutate` jar. Also returns
/// the number of well-formed instances.
fn mutations() -> (usize, Vec<Mutation>) {
    let root = match common::require("KoSIT mutation suite") {
        Some(root) => root.join("xrechnung-schematron/test/instances"),
        None => return (0, Vec::new()),
    };
    let mut out = Vec::new();
    let mut instances = 0usize;
    for sub in ["ubl-inv", "ubl-cn"] {
        for path in xml_files(&root.join(sub)) {
            let text = std::fs::read_to_string(&path).expect("read instance");
            let Ok(doc) = roxmltree::Document::parse(&text) else {
                continue;
            };
            instances += 1;
            let file = path.file_name().unwrap().to_string_lossy().into_owned();
            // The document declares which profile it belongs to.
            let spec = doc
                .root_element()
                .children()
                .find(|n| n.tag_name().name() == "CustomizationID")
                .and_then(|n| n.text())
                .unwrap_or("")
                .trim();
            // Exact match: every instance at the pinned release declares one
            // of the three XRechnung identifiers verbatim.
            let profile = en16931::profiles::classify(Some(spec))
                .profile()
                .unwrap_or_else(|| {
                    panic!("{file}: BT-24 {spec:?} names no profile this build ships")
                });

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
                // `attribute="…"` retargets the mutation to that attribute.
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
    (instances, out)
}

/// KoSIT's mutation suite agrees. Its cases are complete invoices, so
/// `schematron-valid` cases catch rules that fire too eagerly.
#[test]
fn xrechnung_mutation_suite() {
    let (instances, all) = mutations();
    if all.is_empty() {
        eprintln!("skipping: artefacts not present (run `cargo xtask fetch`)");
        return;
    }
    let mut agreed = 0usize;
    let mut skipped = 0usize;
    let mut disagreed: Vec<String> = Vec::new();

    let retired = unevaluated();
    /// Profile rules the model retires (as `tests/corpus.rs` declares them).
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
        let Ok(read) = read(&m.xml) else {
            skipped += 1;
            continue;
        };
        if read.entries_of(Kind::Malformed).next().is_some() {
            skipped += 1;
            continue;
        }
        let report = m.profile.validate(&read.invoice);
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
    let summary = [
        "XRechnung mutation suite".to_owned(),
        line("instances", instances, ""),
        line("runnable mutations", all.len(), ""),
        line("run", run, ""),
        line(
            "agreed",
            agreed,
            &format!("({:.1}% of run)", 100.0 * agreed as f64 / run.max(1) as f64),
        ),
        line("disagreed", disagreed.len(), ""),
        line(
            "skipped",
            skipped,
            "(type-retired, not in the profile, or a value the model refuses)",
        ),
    ]
    .join("\n");
    eprintln!("{summary}");
    assert_eq!(
        run, KOSIT_MUTATIONS,
        "{run} KoSIT mutations ran, and {KOSIT_MUTATIONS} run at the pinned release"
    );
    assert!(
        disagreed.is_empty(),
        "{} of {run} KoSIT mutation assertions disagree with this crate:\n  {}",
        disagreed.len(),
        disagreed.join("\n  ")
    );
    common::assert_page_pastes("conformance.md", &summary);
    common::assert_page_states("conformance.md", &[("<N> *complete, valid*", instances)]);
}

// ── The authorities' example invoices ─────────────────────────────────────────

/// Directories of valid example invoices and their publishers; nothing fatal
/// may fire on them.
const EXAMPLE_DIRS: &[(&str, &str)] = &[
    ("CEN", "eInvoicing-EN16931/ubl/examples"),
    // Peppol BIS Billing samples: electricity trading, factoring, advance
    // payments, hire cars, purchasing cards, interest invoices, telephony.
    ("CEN testfiles", "eInvoicing-EN16931/test/testfiles"),
    ("Peppol", "peppol-bis-invoice-3/rules/examples"),
    (
        "Peppol NO",
        "peppol-bis-invoice-3/rules/national-examples/NO",
    ),
    (
        "Peppol GR",
        "peppol-bis-invoice-3/rules/national-examples/GR",
    ),
];

/// Example files that are not claims of validity, with the reason.
const NOT_A_VALID_EXAMPLE: &[(&str, &str)] = &[
    ("issue116.xml", "a bug-report reproduction, not an example"),
    // The seller has no `cac:PartyLegalEntity`, so CEN's `BR-06` binding
    // rejects it in the reference validator too.
    (
        "GR-base-example-TaxRepresentative.xml",
        "no cac:PartyLegalEntity, so BT-27 is absent — fails CEN's own BR-06",
    ),
];

/// Rules an example directory is known to fail, with the reason.
///
/// CEN's `test/testfiles` use placeholder organisation numbers (`0007`) and
/// GLNs (`0088`) whose check digits fail Peppol `R049` / `R040`.
const KNOWN_FAILING: &[(&str, &[&str], &str)] = &[(
    "CEN testfiles",
    &["PEPPOL-COMMON-R040", "PEPPOL-COMMON-R049"],
    "placeholder identifiers whose check digits fail Peppol's identifier checks",
)];

/// Nothing fatal fires on a published example invoice.
#[test]
fn the_authorities_example_invoices_are_valid() {
    let Some(root) = common::require("published example invoices") else {
        return;
    };
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
            if !matches!(
                doc.root_element().tag_name().name(),
                "Invoice" | "CreditNote"
            ) {
                skipped.push(format!("{name} — not a UBL invoice"));
                continue;
            }
            let read = match read(&text) {
                Ok(r) => r,
                Err(e) => {
                    failures.push(format!("{who}/{name}: the reader refused it: {e}"));
                    continue;
                }
            };
            let what = malformed(&read);
            if !what.is_empty() {
                skipped.push(format!("{name} — value the model refuses: {what}"));
                continue;
            }
            let invoice = read.invoice;
            // The document names its own profile; fall back to core.
            let profile = en16931::profiles::classify(invoice.specification_id.as_deref())
                .profile()
                .unwrap_or(&en16931::profiles::EN16931);
            let report = profile.validate(&invoice);
            checked += 1;
            let excused: &[&str] = KNOWN_FAILING
                .iter()
                .find(|(w, _, _)| *w == *who)
                .map_or(&[], |(_, rules, _)| rules);
            let fatal: Vec<String> = report
                .fatal()
                .filter(|f| !excused.contains(&f.rule.as_str()))
                .map(|f| format!("{} at {}", f.rule, f.path))
                .collect();
            if !fatal.is_empty() {
                failures.push(format!(
                    "{who}/{name} under {}: {}",
                    profile.id,
                    fatal.join("; ")
                ));
            }
        }
    }

    let summary = [
        "published example invoices".to_owned(),
        line("checked", checked, ""),
        line("valid", checked - failures.len(), ""),
        line("skipped", skipped.len(), "(each named with its reason)"),
    ]
    .join("\n");
    eprintln!("{summary}");
    for s in &skipped {
        eprintln!("    skipped: {s}");
    }
    // The corpus exists (`common::require`), so zero documents is a failure.
    assert_eq!(
        checked, EXAMPLE_INVOICES,
        "{checked} published example invoices were checked, and {EXAMPLE_INVOICES} exist \
         at the pinned releases. The reference corpus is present, so an example directory \
         moved, or a pin bump changed the set."
    );
    assert!(
        failures.is_empty(),
        "{} published example invoice(s) are rejected by this crate. Each is \
         either a rule that is too eager or a gap in the reader — never a \
         defect in the authority's own example:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
    common::assert_page_pastes("conformance.md", &summary);
    common::assert_page_states(
        "conformance.md",
        &[
            ("The <N> complete invoices", checked),
            ("mutation suite, <N> published", checked),
        ],
    );
}

// ── The syntax corpora ────────────────────────────────────────────────────────

/// UBL documents in the authority sources at the pinned releases.
#[cfg(feature = "cii")]
const UBL_DOCUMENTS: usize = 326;
/// CII documents in the authority sources at the pinned releases.
#[cfg(feature = "cii")]
const CII_DOCUMENTS: usize = 175;

/// The corpora the syntax suites run over have the size the conformance page
/// states: every UBL and CII file, and all but one of them readable.
#[cfg(feature = "cii")]
#[test]
fn the_syntax_corpora_have_the_documented_size() {
    use en16931_formats::Syntax;

    let ubl = common::authority_documents(Syntax::Ubl, "syntax corpora");
    if ubl.is_empty() {
        return;
    }
    let cii = common::authority_documents(Syntax::Cii, "syntax corpora");
    let readable = ubl
        .iter()
        .chain(&cii)
        .filter(|(_, text)| en16931_formats::read(text.as_bytes()).is_ok())
        .count();
    assert_eq!(ubl.len(), UBL_DOCUMENTS);
    assert_eq!(cii.len(), CII_DOCUMENTS);
    assert_eq!(
        readable,
        UBL_DOCUMENTS + CII_DOCUMENTS - 1,
        "one UBL file is deliberately not well-formed"
    );
    common::assert_page_states(
        "conformance.md",
        &[
            ("holds <N> UBL", ubl.len()),
            ("UBL and <N> CII", cii.len()),
            ("over the <N> that parse", readable),
            ("over **<N> published documents**", readable),
            ("scans all <N> files", ubl.len() + cii.len()),
            ("over all <N>,", readable),
            ("the same <N>,", readable),
            ("The <N> published documents", readable),
            ("<N> documents round-tripped", readable),
        ],
    );
}
