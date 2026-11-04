//! Rendering results as text, JSON or SVRL.
//!
//! Text is `en16931_formats::Received`'s `Display`, JSON the interchange shape
//! [`en16931::report::Report`], SVRL what `en16931::svrl` renders from it. This
//! module arranges documents and adds the source file of each report.

use std::fmt::Write as _;
use std::path::PathBuf;

use en16931::validation::profile::{Profile, Restriction};
use en16931::validation::{Rule, Source};
use en16931_formats::{Received, Recommendation};

use crate::input::Loaded;
use crate::{CatalogueFormat, Format};

/// Print received documents in the requested shape.
pub fn validation(o: &mut String, results: &[(PathBuf, Received)], format: Format) {
    match format {
        Format::Text => validation_text(o, results),
        Format::Json => validation_json(o, results),
        Format::Svrl => validation_svrl(o, results),
    }
}

fn validation_text(o: &mut String, results: &[(PathBuf, Received)]) {
    for (i, (source, received)) in results.iter().enumerate() {
        if i > 0 {
            let _ = writeln!(o);
        }
        let _ = writeln!(o, "{}", source.display());
        let _ = writeln!(o, "{received}");
    }
    if results.len() > 1 {
        let count = |r: Recommendation| {
            results
                .iter()
                .filter(|(_, x)| x.recommendation == r)
                .count()
        };
        let _ = writeln!(
            o,
            "\n{} document(s): {} accepted, {} rejected, {} undefined",
            results.len(),
            count(Recommendation::Accept),
            count(Recommendation::Reject),
            count(Recommendation::Undefined)
        );
    }
}

/// The interchange shape of one received document, naming its file.
fn interchange(source: &std::path::Path, received: &Received) -> en16931::report::Report {
    let mut report = received.to_interchange();
    report.source = Some(source.display().to_string());
    if let Some(required) = report.required.as_deref_mut() {
        required.source.clone_from(&report.source);
    }
    report
}

fn validation_json(o: &mut String, results: &[(PathBuf, Received)]) {
    // Always an array, even for one document.
    let reports: Vec<_> = results
        .iter()
        .map(|(source, received)| interchange(source, received))
        .collect();
    let _ = writeln!(
        o,
        "{}",
        serde_json::to_string_pretty(&reports).expect("a report serialises")
    );
}

fn validation_svrl(o: &mut String, results: &[(PathBuf, Received)]) {
    // One `schematron-output` per report; with several, each is preceded by
    // a comment naming it.
    let reports: Vec<_> = results
        .iter()
        .map(|(source, received)| interchange(source, received))
        .collect();
    let several = reports.len() > 1 || reports.iter().any(|r| r.required.is_some());
    for report in &reports {
        for (r, label) in std::iter::once((report, "as declared"))
            .chain(report.required.as_deref().map(|r| (r, "as required")))
        {
            if several {
                let name = r.source.as_deref().unwrap_or("-").replace("--", "-");
                let _ = writeln!(o, "<!-- {name} {label} -->");
            }
            let _ = write!(o, "{}", en16931::svrl::to_svrl(r));
        }
    }
}

// ── inspect ──────────────────────────────────────────────────────────────────

/// What a document is: its syntax, inside a PDF or not.
fn container(d: &Loaded) -> String {
    if d.read.pdf.is_some() {
        format!("ZUGFeRD / Factur-X (PDF/A-3 + {})", d.read.syntax)
    } else {
        d.read.syntax.to_string()
    }
}

/// What a document is, without a verdict on it.
pub fn inspect_text(o: &mut String, docs: &[Loaded]) {
    for (i, d) in docs.iter().enumerate() {
        if i > 0 {
            let _ = writeln!(o);
        }
        let inv = &d.read.invoice;
        let _ = writeln!(o, "{}", d.source.display());
        let _ = writeln!(o, "  syntax        {}", container(d));
        if let Some(pdf) = &d.read.pdf {
            let _ = writeln!(o, "  ZUGFeRD       {:?}", pdf.profile);
        }
        let _ = writeln!(o, "  kind          {:?}", inv.kind);
        row(o, "BT-24 profile", inv.specification_id.as_deref());
        let _ = writeln!(
            o,
            "  declares      {}",
            en16931::profiles::classify(inv.specification_id.as_deref())
        );
        row(o, "BT-1 number", inv.number.as_deref());
        let _ = writeln!(
            o,
            "  BT-2 issued   {}",
            inv.issue_date.map_or("—".to_owned(), |d| d.to_string())
        );
        row(o, "BT-3 type", inv.type_code.as_ref().map(|c| c.as_str()));
        row(
            o,
            "BT-5 currency",
            inv.currency.as_ref().map(|c| c.as_str()),
        );
        row(o, "BT-27 seller", inv.seller.name.as_deref());
        row(o, "BT-44 buyer", inv.buyer.name.as_deref());
        let _ = writeln!(o, "  BG-25 lines   {}", inv.lines.len());
        let _ = writeln!(o, "  BG-23 groups  {}", inv.vat_breakdown.len());
        let _ = writeln!(o, "  BT-112 gross  {}", inv.totals.gross_total);
        let _ = writeln!(o, "  BT-115 due    {}", inv.totals.due);
        for e in d.read.report().entries() {
            let _ = writeln!(o, "  ! {e}");
        }
    }
}

fn row(o: &mut String, label: &str, value: Option<&str>) {
    let _ = writeln!(o, "  {label:<13} {}", value.unwrap_or("—"));
}

/// The same, as the interchange shape with no rules run.
pub fn inspect_json(o: &mut String, docs: &[Loaded]) {
    let reports: Vec<_> = docs
        .iter()
        .map(|d| {
            let mut r = d.read.report().to_interchange();
            r.source = Some(d.source.display().to_string());
            r.declaration = Some(
                en16931::profiles::classify(d.read.invoice.specification_id.as_deref()).to_string(),
            );
            r
        })
        .collect();
    let _ = writeln!(
        o,
        "{}",
        serde_json::to_string_pretty(&reports).expect("a report serialises")
    );
}

// ── explain ──────────────────────────────────────────────────────────────────

/// One rule, with its provenance and the terms it touches.
pub fn rule(o: &mut String, r: &'static Rule) {
    // Which profiles run it, and at what severity.
    let carried: Vec<(&'static str, en16931::Severity)> = en16931::profiles::ALL
        .iter()
        .filter_map(|p| {
            let level = p.severity_of(r.id.as_str())?;
            p.check_ids()
                .any(|id| r.id.matches(id))
                .then_some((p.id, level))
        })
        .collect();

    // Severity can differ by profile (`PEPPOL-EN16931-R120` is fatal in Peppol,
    // a warning in XRechnung); then the `run by` line carries each profile's.
    let varies = carried.iter().any(|(_, level)| *level != r.severity);
    let header = if varies {
        "varies by profile".to_owned()
    } else {
        r.severity.to_string()
    };
    let _ = writeln!(o, "{}  [{header}]  {}", r.id, provenance(r.source));
    let _ = writeln!(o);
    for line in wrap(r.text, 88) {
        let _ = writeln!(o, "  {line}");
    }
    if !r.terms.is_empty() {
        let terms: Vec<String> = r.terms.iter().map(ToString::to_string).collect();
        let _ = writeln!(o, "\n  terms: {}", terms.join(", "));
    }
    // A rule the model's types retire runs no predicate; say how it is held,
    // or a reader of `explain` would take it for checked like the others.
    if en16931::validation::rules::structural::STRUCTURAL_BY_TYPE
        .iter()
        .any(|b| core::ptr::eq(*b, r))
    {
        let _ = writeln!(
            o,
            "  held by: the model's types — an invoice built in code cannot break it; a\n           \
             document that does is reported by its reader (`missing`, or `malformed`\n           \
             for a currency), and `receive` rejects it"
        );
    }
    if !carried.is_empty() {
        let list: Vec<String> = carried
            .iter()
            .map(|(id, level)| {
                if varies {
                    format!("{id} ({level})")
                } else {
                    (*id).to_owned()
                }
            })
            .collect();
        let _ = writeln!(o, "  run by: {}", list.join(", "));
    }
}

/// One profile restriction — §7.3.2 data, not a predicate.
pub fn restriction(o: &mut String, profile: &'static Profile, r: &'static Restriction) {
    let kind = match r {
        Restriction::Mandatory { .. } => "cardinality 0..x → 1..x (mandatory)",
        Restriction::NotUsed { .. } => "cardinality 0..x → 0..0 (not used)",
        Restriction::CodeValues { .. } => "code list narrowed",
    };
    let _ = writeln!(o, "{}  [restriction]  {}", r.id(), profile.id);
    let _ = writeln!(o, "\n  {kind}");
    let _ = writeln!(o, "  term: {:?}", r.term());
    if let Restriction::CodeValues { allowed, .. } = r {
        let _ = writeln!(o, "  allowed: {}", allowed.join(", "));
    }
    // Which profiles run it, and at what severity. A restriction is fatal
    // unless an authority's published `flag` lowers it (`BR-DE-17` is a warning).
    let carried: Vec<String> = en16931::profiles::ALL
        .iter()
        .filter_map(|p| {
            let level = p.severity_of(r.id())?;
            p.check_ids()
                .any(|id| en16931::validation::RuleId::new(id).matches(r.id()))
                .then(|| format!("{} ({level})", p.id))
        })
        .collect();
    if !carried.is_empty() {
        let _ = writeln!(o, "  run by: {}", carried.join(", "));
    }
    let _ = writeln!(
        o,
        "\n  A restriction is data, not code — see EN 16931-1 §7.3.2 and \
         `en16931::validation::profile::Restriction`."
    );
}

const fn provenance(s: Source) -> &'static str {
    match s {
        Source::Both => "EN 16931-1 and the CEN artefacts",
        Source::StandardOnly => "EN 16931-1 only — not shipped as an artefact assertion",
        Source::ArtefactOnly => "the artefacts only — an authority's addition",
        Source::Crate => "this crate's own, outside the standard",
    }
}

// ── diff ─────────────────────────────────────────────────────────────────────

/// One place two invoices disagree.
pub struct Difference {
    /// Where, as a path through the model — `lines[1].vat.rate`.
    pub at: String,
    /// The left document's value, rendered as JSON. `null` means absent.
    pub left: String,
    /// The right document's value. `null` means absent.
    pub right: String,
}

/// Compare two invoices field by field, through their `serde` form.
///
/// Paths are model paths (`lines[1].vat.rate`), not BT/BG paths. Arrays are
/// compared per index.
pub fn model_differences(
    a: &en16931::Invoice,
    b: &en16931::Invoice,
) -> Result<Vec<Difference>, String> {
    let to_value = |inv: &en16931::Invoice| {
        serde_json::to_value(inv).map_err(|e| format!("the model did not serialise: {e}"))
    };
    let (a, b) = (to_value(a)?, to_value(b)?);
    let mut out = Vec::new();
    walk(&a, &b, "", &mut out);
    Ok(out)
}

fn walk(a: &serde_json::Value, b: &serde_json::Value, at: &str, out: &mut Vec<Difference>) {
    use serde_json::Value;
    if a == b {
        return;
    }
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let keys: std::collections::BTreeSet<_> = x.keys().chain(y.keys()).collect();
            for k in keys {
                let null = Value::Null;
                let path = if at.is_empty() {
                    k.clone()
                } else {
                    format!("{at}.{k}")
                };
                walk(
                    x.get(k).unwrap_or(&null),
                    y.get(k).unwrap_or(&null),
                    &path,
                    out,
                );
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            let null = Value::Null;
            for i in 0..x.len().max(y.len()) {
                walk(
                    x.get(i).unwrap_or(&null),
                    y.get(i).unwrap_or(&null),
                    &format!("{at}[{i}]"),
                    out,
                );
            }
        }
        _ if same_number(a, b) => {}
        _ => out.push(Difference {
            at: at.to_owned(),
            left: a.to_string(),
            right: b.to_string(),
        }),
    }
}

/// Whether two JSON strings are the same decimal written to two scales
/// (`25.0` vs `25`), which the model treats as equal.
///
/// Only trailing fractional zeros are ignored: leading zeros are text
/// (`"0001"` and `"1"` are different invoice numbers). Only string-to-string.
fn same_number(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    let (Some(x), Some(y)) = (a.as_str(), b.as_str()) else {
        return false;
    };
    // Both must be decimals, so `"REF.0"` and `"REF"` stay different.
    if x.parse::<rust_decimal::Decimal>().is_err() || y.parse::<rust_decimal::Decimal>().is_err() {
        return false;
    }
    trimmed_scale(x) == trimmed_scale(y)
}

/// `"25.00"` → `"25"`, `"1.50"` → `"1.5"`, `"0001"` → `"0001"`.
fn trimmed_scale(s: &str) -> &str {
    let Some((int, frac)) = s.split_once('.') else {
        return s;
    };
    let kept = frac.trim_end_matches('0').len();
    if kept == 0 {
        int
    } else {
        &s[..int.len() + 1 + kept]
    }
}

/// A one-character marker: added, removed, or changed.
/// `+` and `-` are relative to the left document, as in `diff(1)`.
fn marker(d: &Difference) -> char {
    match (d.left.as_str(), d.right.as_str()) {
        ("null", _) => '+',
        (_, "null") => '-',
        _ => '~',
    }
}

pub fn diff_text(o: &mut String, a: &Loaded, b: &Loaded, differences: &[Difference]) {
    let _ = writeln!(o, "--- {} — {}", a.source.display(), container(a));
    let _ = writeln!(o, "+++ {} — {}", b.source.display(), container(b));
    for e in a.read.entries.iter().chain(&b.read.entries) {
        let _ = writeln!(o, "  ! {e}");
    }

    if differences.is_empty() {
        let _ = writeln!(o, "\nidentical as invoices");
        return;
    }

    let width = differences
        .iter()
        .map(|d| d.at.chars().count())
        .max()
        .unwrap_or(0)
        .min(44);
    let _ = writeln!(o, "\n{} difference(s)", differences.len());
    for d in differences {
        let _ = writeln!(
            o,
            "  {} {:<width$}  {} → {}",
            marker(d),
            d.at,
            d.left,
            d.right
        );
    }
}

pub fn diff_json(
    o: &mut String,
    a: &Loaded,
    b: &Loaded,
    differences: &[Difference],
) -> Result<(), String> {
    let value = serde_json::json!({
        "left":  { "source": a.source.display().to_string(), "container": container(a) },
        "right": { "source": b.source.display().to_string(), "container": container(b) },
        "identical": differences.is_empty(),
        "differences": differences.iter().map(|d| serde_json::json!({
            "at": d.at,
            "change": marker(d).to_string(),
            // Parsed back so the JSON carries values, not JSON strings.
            "left": serde_json::from_str::<serde_json::Value>(&d.left)
                .unwrap_or(serde_json::Value::Null),
            "right": serde_json::from_str::<serde_json::Value>(&d.right)
                .unwrap_or(serde_json::Value::Null),
        })).collect::<Vec<_>>(),
    });
    let text = serde_json::to_string_pretty(&value)
        .map_err(|e| format!("the comparison did not serialise: {e}"))?;
    let _ = writeln!(o, "{text}");
    Ok(())
}

// ── profiles ─────────────────────────────────────────────────────────────────

/// `en16931 categories S O` — the verdict, and the way out of a refusal.
///
/// Prints the categories with their full names first, so a misread code shows.
pub fn categories(o: &mut String, cats: &[en16931::VatCategory]) {
    for c in cats {
        let _ = writeln!(o, "  {:<3} {}", c.code(), c.name());
    }
    let _ = writeln!(o);
    match en16931::VatCategory::can_share_document(cats) {
        Ok(()) => {
            let _ = writeln!(o, "these categories may share one invoice");
        }
        Err(conflict) => {
            let _ = writeln!(o, "REFUSED: {conflict}");
            let _ = writeln!(
                o,
                "\nthe rules that govern it: {}\n\
                 (which of them reports depends on whether the other category is on a line, \
                 an allowance, a charge or a breakdown group — `en16931 explain {}` for each)",
                conflict.rules.join(", "),
                conflict.rules[0]
            );
        }
    }
}

/// Every profile this build can validate against.
pub fn profiles(o: &mut String) {
    // Column widths are measured from the data.
    let name = width(en16931::profiles::ALL.iter().map(|p| p.slug), "PROFILE");
    let id = width(en16931::profiles::ALL.iter().map(|p| p.id), "NAME");
    let _ = writeln!(
        o,
        "{:<name$}  {:<id$}  {:>6}  {:<5}  BT-24",
        "PROFILE", "NAME", "CHECKS", "CIUS?"
    );
    for p in en16931::profiles::ALL {
        let _ = writeln!(
            o,
            "{:<name$}  {:<id$}  {:>6}  {:<5}  {}",
            p.slug,
            p.id,
            p.check_ids().count(),
            // §4.4.2 conformance; the core model is not a CIUS.
            if p.underlying.is_empty() {
                "n/a"
            } else if p.is_conformant_cius() {
                "yes"
            } else {
                "no"
            },
            p.specification_id
        );
    }
    let _ = writeln!(
        o,
        "\nPROFILE is what `--require` takes; NAME is what a report prints. CHECKS\n\
         counts the rules and restrictions a profile declares, and a report says the\n\
         same number for every document: every check runs, whether or not it has\n\
         anything to report. `en16931 rules --profile <PROFILE>` lists them."
    );
    // ZUGFeRD / Factur-X: read and required, never written.
    let _ = writeln!(o, "\nZUGFeRD / Factur-X (read and required, not written):");
    let zslug = width(en16931::profiles::ZUGFERD.iter().map(|p| p.slug), "");
    let zid = width(en16931::profiles::ZUGFERD.iter().map(|p| p.id), "");
    for p in en16931::profiles::ZUGFERD {
        let _ = writeln!(
            o,
            "  {:<zslug$}  {:<zid$}  {}",
            p.slug, p.id, p.specification_id
        );
    }
    let _ = writeln!(o, "\nedition: {}", en16931::DEFAULT_EDITION);

    // The artefact releases these profiles were verified against, deduplicated.
    let mut seen: Vec<&en16931::validation::profile::ArtefactRef> = Vec::new();
    for p in en16931::profiles::ALL {
        for a in p.artefacts {
            if !seen
                .iter()
                .any(|s| s.repo == a.repo && s.git_ref == a.git_ref)
            {
                seen.push(a);
            }
        }
    }
    let _ = writeln!(o, "verified against:");
    let authority = width(seen.iter().map(|a| a.authority), "");
    let repo = width(seen.iter().map(|a| a.repo), "");
    for a in &seen {
        let _ = writeln!(
            o,
            "  {:<authority$}  {:<repo$}  {}",
            a.authority, a.repo, a.git_ref
        );
    }
    let _ = writeln!(o, "({})", en16931::ATTRIBUTION);
}

/// The widest of `values` and `header`, in characters (as `{:<n}` counts).
fn width<'a>(values: impl Iterator<Item = &'a str>, header: &str) -> usize {
    values
        .map(|v| v.chars().count())
        .chain(std::iter::once(header.chars().count()))
        .max()
        .unwrap_or(0)
}

// ── text wrapping ────────────────────────────────────────────────────────────

/// Greedy word wrap.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

// ── the rule catalogue ───────────────────────────────────────────────────────

/// Every rule the build ships, optionally narrowed to a profile or a term,
/// derived from the registry.
///
/// With a profile, its §7.3.2 restrictions (`BR-DE-3`, …) are listed after the
/// rules: source `profile` in text, a `restriction` kind in JSON.
pub fn catalogue(
    o: &mut String,
    profile: Option<&'static Profile>,
    term: Option<en16931::BtId>,
    format: CatalogueFormat,
) {
    // Without a profile: the union across profiles, deduplicated on
    // `(id, severity, text)` so both `PEPPOL-EN16931-R120` variants (fatal in
    // Peppol, warning in XRechnung) are kept.
    let candidates: Vec<&'static Rule> = match profile {
        Some(p) => en16931::validation::rules::CORE
            .iter()
            .copied()
            .chain(p.extra_rules.iter().copied())
            .collect(),
        None => en16931::validation::rules::all().collect(),
    };
    let mut seen = std::collections::BTreeSet::new();
    let rules: Vec<&'static Rule> = candidates
        .into_iter()
        .filter(|r| term.is_none_or(|t| r.terms.contains(&t)))
        .filter(|r| seen.insert((r.id.as_str(), r.severity, r.text)))
        .collect();
    // The profile's narrowings, filtered by the same `--term` as the rules.
    let restrictions: Vec<&'static Restriction> = profile
        .into_iter()
        .flat_map(|p| p.restrictions.iter())
        .filter(|r| term.is_none_or(|t| r.term().term == t))
        .collect();

    match format {
        CatalogueFormat::Text => {
            let _ = writeln!(o, "{:<26} {:<12} {:<18} TEXT", "RULE", "SEVERITY", "SOURCE");
            for r in &rules {
                let severity = profile
                    .and_then(|p| p.severity_of(r.id.as_str()))
                    .unwrap_or(r.severity);
                let _ = writeln!(
                    o,
                    "{:<26} {severity:<12} {:<18} {}",
                    r.id.as_str(),
                    short_source(r.source),
                    first_sentence(r.text),
                );
            }
            for r in &restrictions {
                let _ = writeln!(
                    o,
                    "{:<26} {:<12} {:<18} {}",
                    r.id(),
                    // `Restriction::check` always writes `Fatal`.
                    "fatal",
                    "profile",
                    restriction_text(r),
                );
            }
            let _ = writeln!(
                o,
                "\n{} check(s): {} rule(s) and {} restriction(s)",
                rules.len() + restrictions.len(),
                rules.len(),
                restrictions.len()
            );
            if let Some(p) = profile {
                let _ = writeln!(
                    o,
                    "as {p} runs them. A restriction is data rather than a predicate \
                     (§7.3.2) — `en16931 explain BR-DE-3` says which kind.",
                    p = p.id
                );
            }
        }
        CatalogueFormat::Json => {
            let mut out: Vec<_> = rules
                .iter()
                .map(|r| {
                    let severity = profile
                        .and_then(|p| p.severity_of(r.id.as_str()))
                        .unwrap_or(r.severity);
                    serde_json::json!({
                        "rule": r.id.as_str(),
                        "severity": severity.to_string(),
                        "source": provenance(r.source),
                        "terms": r.terms.iter().map(ToString::to_string).collect::<Vec<_>>(),
                        "text": r.text,
                    })
                })
                .collect();
            out.extend(restrictions.iter().map(|r| {
                serde_json::json!({
                    "rule": r.id(),
                    "severity": "fatal",
                    "source": "a profile narrowing (EN 16931-1 §7.3.2), not a rule",
                    "terms": [r.term().term.to_string()],
                    "text": restriction_text(r),
                    "restriction": restriction_kind(r),
                })
            }));
            let _ = writeln!(
                o,
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "edition": en16931::DEFAULT_EDITION.to_string(),
                    "artefacts": en16931::ARTEFACT_VERSION,
                    "profile": profile.map(|p| p.id),
                    "attribution": en16931::ATTRIBUTION,
                    "rules": out,
                }))
                .expect("the value was just built")
            );
        }
    }
}

/// Which of §7.3.2's narrowings this is, as one stable token.
const fn restriction_kind(r: &Restriction) -> &'static str {
    match r {
        Restriction::Mandatory { .. } => "mandatory",
        Restriction::NotUsed { .. } => "not-used",
        Restriction::CodeValues { .. } => "code-values",
    }
}

/// A restriction rendered as one sentence, using the term's `label()` as a
/// finding does.
fn restriction_text(r: &Restriction) -> String {
    let label = r.term().label();
    match r {
        Restriction::Mandatory { .. } => format!("{label} shall be present."),
        Restriction::NotUsed { .. } => format!("{label} shall not be used."),
        Restriction::CodeValues { allowed, .. } => {
            format!("{label} shall be one of: {}.", allowed.join(", "))
        }
    }
}

const fn short_source(s: Source) -> &'static str {
    match s {
        Source::Both => "standard+artefact",
        Source::StandardOnly => "standard",
        Source::ArtefactOnly => "artefact",
        Source::Crate => "en16931",
    }
}

/// The first sentence of a rule text, for a one-line table.
fn first_sentence(text: &str) -> String {
    let line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.find(". ") {
        Some(i) => line[..=i].to_owned(),
        None => line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scale difference is not a difference; a leading zero is.
    #[test]
    fn only_a_trailing_fractional_zero_is_not_a_difference() {
        let s = |v: &str| serde_json::Value::String(v.to_owned());
        for (x, y) in [("25", "25.0"), ("25.00", "25"), ("1.50", "1.5")] {
            assert!(
                same_number(&s(x), &s(y)),
                "{x} vs {y} is a scale difference"
            );
        }
        for (x, y) in [
            ("0001", "1"),     // BT-1, an invoice number
            ("01067", "1067"), // BT-53, a post code
            ("01", "1"),       // BT-126, a line id
            ("007", "7"),      // BT-13, an order reference
            ("+5", "5"),       // a sign form is text too
        ] {
            assert!(
                !same_number(&s(x), &s(y)),
                "{x} vs {y} differs in more than scale"
            );
        }
        assert!(!same_number(&s("REF.0"), &s("REF")));
        assert!(!same_number(&s("1"), &serde_json::Value::Null));
    }

    #[test]
    fn wrapping_never_splits_a_word_or_loses_one() {
        let text = "Sum of Invoice line net amount (BT-106) = Σ Invoice line net amount (BT-131).";
        let lines = wrap(text, 20);
        assert!(
            lines
                .iter()
                .all(|l| l.chars().count() <= 20 || !l.contains(' '))
        );
        assert_eq!(
            lines.join(" ").split_whitespace().collect::<Vec<_>>(),
            text.split_whitespace().collect::<Vec<_>>()
        );
    }

    #[test]
    fn every_shipped_rule_has_a_provenance_sentence() {
        for r in en16931::validation::rules::all() {
            assert!(!provenance(r.source).is_empty(), "{}", r.id);
        }
    }
}
