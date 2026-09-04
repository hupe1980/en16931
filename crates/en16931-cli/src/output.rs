//! Turning results into the three shapes the command can print.
//!
//! # Why the JSON is not `serde_json::to_value(&report)`
//!
//! [`en16931::report::Report`] exists precisely so a stored report has a shape
//! that is allowed to be boring, versioned by [`en16931::report::SCHEMA`]. The
//! command emits that, plus the two things only the command knows: which file a
//! report came from, and what the reader could not map on the way in.

use std::fmt::Write as _;

use en16931::report::Report;
use en16931::validation::profile::{Profile, Restriction};
use en16931::validation::{Rule, Source, ValidationReport};

use crate::input::Loaded;
use crate::{CatalogueFormat, Format};

/// Print validation results in the requested shape.
pub fn validation(o: &mut String, results: &[(Loaded, ValidationReport)], format: Format) {
    match format {
        Format::Text => validation_text(o, results),
        Format::Json => validation_json(o, results),
        Format::Svrl => validation_svrl(o, results),
    }
}

fn validation_text(o: &mut String, results: &[(Loaded, ValidationReport)]) {
    for (i, (loaded, report)) in results.iter().enumerate() {
        if i > 0 {
            let _ = writeln!(o);
        }
        // The path first: a run over a directory is unreadable otherwise, and
        // this is the line a person greps for.
        let _ = writeln!(o, "{} — {}", loaded.source.display(), loaded.container);
        for note in &loaded.notes {
            let _ = writeln!(o, "  ! {note}");
        }
        let _ = writeln!(o, "{report}");
    }
    if results.len() > 1 {
        let bad = results.iter().filter(|(_, r)| !r.is_valid()).count();
        let _ = writeln!(o, "\n{} document(s), {bad} invalid", results.len());
    }
}

fn validation_json(o: &mut String, results: &[(Loaded, ValidationReport)]) {
    let docs: Vec<_> = results
        .iter()
        .map(|(loaded, report)| {
            let mut value = serde_json::to_value(Report::of(report))
                .expect("a Report serialises; every field is a string, a bool or a number");
            if let Some(obj) = value.as_object_mut() {
                obj.insert(
                    "source".to_owned(),
                    loaded.source.display().to_string().into(),
                );
                obj.insert("container".to_owned(), loaded.container.to_string().into());
                obj.insert("readerNotes".to_owned(), loaded.notes.clone().into());
            }
            value
        })
        .collect();
    // A single document still comes back as an array. A consumer that has to
    // branch on the shape of the top level to find out how many documents it
    // asked about is a consumer that will get it wrong once.
    let out = serde_json::json!({
        "schema": en16931::report::SCHEMA,
        "valid": results.iter().all(|(_, r)| r.is_valid()),
        "documents": docs,
    });
    let _ = writeln!(
        o,
        "{}",
        serde_json::to_string_pretty(&out).expect("the value was just built")
    );
}

fn validation_svrl(o: &mut String, results: &[(Loaded, ValidationReport)]) {
    // SVRL has one `schematron-output` per validated document, so several
    // documents are several documents. Concatenating them into one file would
    // produce something no SVRL reader accepts.
    for (loaded, report) in results {
        if results.len() > 1 {
            let _ = writeln!(o, "<!-- {} -->", loaded.source.display());
        }
        let _ = write!(o, "{}", en16931::svrl::to_svrl(report));
    }
}

// ── inspect ──────────────────────────────────────────────────────────────────

/// What a document is, without a verdict on it.
pub fn inspect_text(o: &mut String, docs: &[Loaded]) {
    for (i, d) in docs.iter().enumerate() {
        if i > 0 {
            let _ = writeln!(o);
        }
        let inv = &d.invoice;
        let _ = writeln!(o, "{}", d.source.display());
        let _ = writeln!(o, "  syntax        {}", d.container);
        if let Some(p) = d.zugferd_profile {
            let _ = writeln!(o, "  ZUGFeRD       {p:?}  ({:?})", p.is_en16931_invoice());
        }
        let _ = writeln!(o, "  kind          {:?}", inv.kind);
        row(o, "BT-24 profile", inv.specification_id.as_deref());
        // What the declared BT-24 resolves to here, which is the question behind
        // "will this validate against what I think it will".
        let resolved = inv
            .specification_id
            .as_deref()
            .and_then(en16931::profiles::for_specification_id);
        let _ = writeln!(
            o,
            "  rule set      {}",
            resolved.map_or("EN 16931 (BT-24 unknown here)", |p| p.id)
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
        for note in &d.notes {
            let _ = writeln!(o, "  ! {note}");
        }
    }
}

fn row(o: &mut String, label: &str, value: Option<&str>) {
    let _ = writeln!(o, "  {label:<13} {}", value.unwrap_or("—"));
}

/// The same, as JSON.
pub fn inspect_json(o: &mut String, docs: &[Loaded]) {
    let out: Vec<_> = docs
        .iter()
        .map(|d| {
            serde_json::json!({
                "source": d.source.display().to_string(),
                "container": d.container.to_string(),
                "zugferdProfile": d.zugferd_profile.map(|p| format!("{p:?}")),
                "specificationId": d.invoice.specification_id,
                "ruleSet": d.invoice.specification_id.as_deref()
                    .and_then(en16931::profiles::for_specification_id)
                    .map(|p| p.id),
                "kind": format!("{:?}", d.invoice.kind),
                "number": d.invoice.number,
                "issueDate": d.invoice.issue_date.map(|x| x.to_string()),
                "typeCode": d.invoice.type_code.as_ref().map(|c| c.as_str()),
                "currency": d.invoice.currency.as_ref().map(|c| c.as_str()),
                "sellerName": d.invoice.seller.name,
                "buyerName": d.invoice.buyer.name,
                "lineCount": d.invoice.lines.len(),
                "vatBreakdownCount": d.invoice.vat_breakdown.len(),
                "grossTotal": d.invoice.totals.gross_total.to_string(),
                "amountDue": d.invoice.totals.due.to_string(),
                "readerNotes": d.notes,
            })
        })
        .collect();
    let _ = writeln!(
        o,
        "{}",
        serde_json::to_string_pretty(&out).expect("the value was just built")
    );
}

// ── explain ──────────────────────────────────────────────────────────────────

/// One rule, with its provenance and the terms it touches.
pub fn rule(o: &mut String, r: &'static Rule) {
    // Which profiles run it, and at what severity — the answer to "why did my
    // validator not object to this?"
    let carried: Vec<(&'static str, en16931::Severity)> = en16931::profiles::ALL
        .iter()
        .filter_map(|p| {
            let level = p.severity_of(r.id.as_str())?;
            p.check_ids()
                .any(|id| r.id.matches(id))
                .then_some((p.id, level))
        })
        .collect();

    // An id can name two rules with two consequences — `PEPPOL-EN16931-R120` is
    // Peppol's *fatal* rule and XRechnung's *warning* one, because KoSIT's build
    // rewrites the flag on the way in. Printing either instance's severity in
    // the header states one authority's answer as if it were the answer, so
    // where they differ the header declines to and the `run by` line carries
    // every profile's.
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
    // Which profiles run it, and at what severity — the same line `explain`
    // gives for a rule, and the answer to "why did this not reject my
    // invoice?". A restriction is fatal by construction, so a lower severity
    // here is always an authority's published `flag`, and always the thing the
    // reader wanted to know: `BR-DE-17` is a **warning** in Germany because a
    // lawful EN 16931 type code XRechnung does not admit is a scoping question
    // rather than a malformed document.
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

/// Compare two invoices field by field.
///
/// # Why through `serde`, and what that costs
///
/// A hand-written walk over 164 business terms would give paths spelled
/// `BG-25[2]/BT-152`, which is this project's usual currency — and it would be
/// a second model to keep in step with the first, silently missing whichever
/// field was added without a matching arm. The serialised form cannot miss one:
/// it *is* the model.
///
/// The cost is that a path reads `lines[1].vat.rate` rather than
/// `BG-25[2]/BT-152`. For a diff that is arguably the better half of the trade —
/// the reader is looking at their own data structure, not at a rule — but it is
/// a real difference from how findings are reported, and worth knowing.
///
/// Arrays of unequal length are reported per index rather than as one
/// wholesale difference, so removing the first of five lines does not read as
/// five unrelated changes plus a length change. It reads as five, which is
/// honest: after a removal every subsequent index really does hold something
/// else.
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

/// Whether two JSON values are the same number written to two **scales**.
///
/// The model's numeric types serialise as **strings**, deliberately: a JSON
/// number is a float in most readers, and an invoice total that survives a
/// round trip through `f64` is a coincidence rather than a guarantee. The
/// consequence here is that a scale difference looks like a text difference —
/// UBL writes `25.0` for BT-152 where CII writes `25`.
///
/// The model does not agree that those differ: `Percentage` compares by value,
/// and `19` and `19.00` are one VAT breakdown group in `Eq`, `Ord` **and**
/// `Hash`. A diff that contradicts the model it is diffing is worse than no
/// diff, because it manufactures work — the first real conversion this was run
/// against reported three differences and all three were this.
///
/// # Scale only, and that word is load-bearing
///
/// This compared `x.parse::<Decimal>() == y.parse::<Decimal>()`, which is a
/// wider claim than the one above and a wrong one. Going through `serde` means
/// the *type* is gone by the time the two values meet, so every `String` term
/// was being compared as a number if it happened to look like one — and then
/// **`"0001"` and `"1"` were the same invoice number.** So were the post codes
/// `01067` (Dresden) and `1067`, the line ids `01` and `1`, and the order
/// reference `007`. `en16931 diff` would print *"identical as invoices"* and
/// exit `0` on a conversion that had eaten a leading zero, which is the one
/// answer a pipeline acts on.
///
/// A scale difference is precisely a difference in *trailing fractional* zeros,
/// so that is what is compared. Leading zeros are text, and text is what a
/// document reference is.
///
/// Only string-to-string: a string against a number, or against `null`, is a
/// genuine change of shape.
fn same_number(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    let (Some(x), Some(y)) = (a.as_str(), b.as_str()) else {
        return false;
    };
    // Both have to be decimals at all, so a term like `"REF.0"` against `"REF"`
    // is never quietly folded together.
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
///
/// `+` and `-` are relative to the **left** document, which is the one named
/// first — the same convention as `diff(1)`, so nobody has to look it up.
fn marker(d: &Difference) -> char {
    match (d.left.as_str(), d.right.as_str()) {
        ("null", _) => '+',
        (_, "null") => '-',
        _ => '~',
    }
}

pub fn diff_text(o: &mut String, a: &Loaded, b: &Loaded, differences: &[Difference]) {
    let _ = writeln!(o, "--- {} — {}", a.source.display(), a.container);
    let _ = writeln!(o, "+++ {} — {}", b.source.display(), b.container);
    for note in a.notes.iter().chain(&b.notes) {
        let _ = writeln!(o, "  ! {note}");
    }

    if differences.is_empty() {
        // Worth saying explicitly when the two are in different syntaxes: that
        // is the case where "no output" would look like the command had failed
        // to notice they are different files.
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
        "left":  { "source": a.source.display().to_string(), "container": a.container.to_string() },
        "right": { "source": b.source.display().to_string(), "container": b.container.to_string() },
        "identical": differences.is_empty(),
        "differences": differences.iter().map(|d| serde_json::json!({
            "at": d.at,
            "change": marker(d).to_string(),
            // Parsed back from the rendered form, so the JSON carries real
            // values rather than strings of JSON — a consumer should be able to
            // compare `left` against its own data without unquoting first.
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

/// Every profile this build can validate against.
/// `en16931 categories S O` — the verdict, and the way out of a refusal.
///
/// Prints the categories with their full names first: a caller who typed `O`
/// meaning "other" rather than "outside the scope of VAT" has made the mistake
/// this command exists to catch, and only the name shows it.
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

pub fn profiles(o: &mut String) {
    // Column widths are measured, never guessed. The `verified against` block
    // below was written with a hard-coded `{:<44}`, and the longest repository
    // name is 45 characters — so the table shipped one column out of true, and
    // would have again the next time an authority renamed a repository.
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
            // §4.4.2 asks this of a CIUS, and the core model is not one — "no"
            // there would read as a defect rather than as "the question does not
            // arise". Of the four that are CIUSes, three answer no, each because
            // its authority reports a core rule at a lower severity.
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
        "\nPROFILE is what `--profile` takes; NAME is what a report prints. CHECKS\n\
         counts the rules and restrictions a profile declares, and a report says the\n\
         same number for every document: every check runs, whether or not it has\n\
         anything to report. `en16931 rules --profile <PROFILE>` lists them."
    );
    let _ = writeln!(o, "\nedition: {}", en16931::DEFAULT_EDITION);

    // The releases these profiles were verified against. Deduped and listed
    // once rather than repeated per profile: four rows of the same four
    // repositories is a wall, and which profile draws on which is already the
    // answer `explain` gives for a specific rule. A single global
    // "artefacts: validation-1.3.16" would be worse than either — `BR-DE-15` is
    // KoSIT's and moves on KoSIT's cadence, so that line names the wrong
    // authority for four of the five profiles.
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

/// The widest of `values` and `header`, in characters.
///
/// Characters rather than bytes: a repository name is ASCII today and a profile
/// name need not be, and `{:<n}` counts characters.
fn width<'a>(values: impl Iterator<Item = &'a str>, header: &str) -> usize {
    values
        .map(|v| v.chars().count())
        .chain(std::iter::once(header.chars().count()))
        .max()
        .unwrap_or(0)
}

// ── text wrapping ────────────────────────────────────────────────────────────

/// Greedy wrap. Rule texts are long sentences and a terminal is not.
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

/// Every rule the build ships, optionally narrowed to a profile or a term.
///
/// # Why a command and not a documentation page
///
/// The catalogue is derived from the registry, so it cannot drift from what the
/// validator actually runs — which a hand-written table of 317 rules would, on
/// the first release nobody remembered to update it. It is also the shape you
/// want when a counterparty quotes an id you do not recognise, or when you need
/// to know what changed between two versions:
///
/// ```sh
/// en16931 rules --format json > new.json && diff old.json new.json
/// ```
///
/// # Restrictions are in it
///
/// A profile's §7.3.2 narrowings are *data* rather than predicates, so they are
/// not `Rule`s — but they are published under real ids, they are what a German
/// counterparty quotes (`BR-DE-3`, `BR-DE-15`, `BR-DE-17`), and they are counted
/// in the CHECKS column `en16931 profiles` prints. Leaving them out made
/// `--profile "XRechnung 3.0"` list 270 of the 282 checks it declares, and the
/// twelve missing were the German ones. The JSON did not so much as mention
/// them, which is the shape people diff across releases.
///
/// They are listed with `source: "profile"` — the same word
/// [`en16931::report::Entry`] uses for a finding whose id resolves to no rule —
/// because the wording is this crate's rendering of a narrowing, not an
/// authority's sentence, and the catalogue must not imply otherwise.
pub fn catalogue(
    o: &mut String,
    profile: Option<&'static Profile>,
    term: Option<en16931::BtId>,
    format: CatalogueFormat,
) {
    // With a profile named: exactly what that profile runs. Without one: every
    // rule the build ships, which is the union across profiles and therefore has
    // duplicates — XRechnung's rules reach the registry three times, once per
    // KoSIT variant.
    //
    // Deduplicated on `(id, severity, text)` rather than on the id alone,
    // because the registry deliberately holds **two** `PEPPOL-EN16931-R120`:
    // Peppol's, which is fatal, and the one XRechnung's build rewrites to a
    // warning. Collapsing by id would hide the one difference between them that
    // matters, and listing every copy would print the other 150 rules three
    // times over.
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
                    // A restriction always rejects; §7.3.2 has no advisory
                    // narrowing, and `Restriction::check` writes `Fatal`.
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

/// A restriction rendered as one sentence, matching what a finding would say.
///
/// Uses [`TermAccessor::label`] rather than formatting the pair here, so this
/// and the finding cannot describe the same narrowing two different ways.
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
///
/// Truncating at a fixed width cuts mid-word and mid-`(BT-…)`; the rules are
/// written as sentences, so the first one is a summary the authority wrote.
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

    /// A scale difference is not a difference; a **leading zero** is.
    ///
    /// Comparing parsed `Decimal`s would make `"0001"` and `"1"` the same
    /// invoice number, and `01067` and `1067` the same Dresden post code — on a
    /// command whose exit code says whether a conversion preserved the
    /// document.
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
        // Not a number at all, and not folded together by the trimming.
        assert!(!same_number(&s("REF.0"), &s("REF")));
        // And a shape change is never a scale difference.
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
