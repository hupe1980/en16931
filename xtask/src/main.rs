//! Build tasks for `en16931`.
//!
//! ```text
//! cargo xtask codegen      regenerate src/codes/generated.rs from spec/
//! cargo xtask check        regenerate into memory and fail if it differs
//! ```
//!
//! # Why a generator at all
//!
//! The `BR-CL-*` rules reference roughly 4 400 code values across fifteen lists
//! — 2 162 UN/ECE Rec 20/21 unit codes alone. Hand-maintaining them is not an
//! option, and skipping them is not either: a wrong unit code is a rejected
//! invoice.
//!
//! # Why the extraction is careful
//!
//! A Schematron `test` is a **program**, not a data structure. `BR-CL-01`'s test
//! is a disjunction over `self::` with *two* different lists — 50 codes for
//! `cbc:InvoiceTypeCode` and 13 for `cbc:CreditNoteTypeCode`. An extractor that
//! takes the first `contains(…)` literal and stops reports a confident, precise,
//! wrong answer.
//!
//! So every table declares how its list is selected ([`Select`]), and the
//! generator **fails** when the artefact does not match that declaration:
//! a rule that suddenly carries two different lists where one was expected
//! stops the build rather than silently picking one.
//!
//! # Why an xtask and not a script
//!
//! The generator is compiled, type-checked and linted like the rest of the
//! crate. `cargo xtask check` runs in CI, so the committed tables cannot drift
//! away from the artefacts they claim to come from.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The pinned artefact revision. Bump deliberately, in step with
/// `fetch-spec.sh` and `en16931::ARTEFACT_VERSION`.
const ARTEFACT: &str = "validation-1.3.16";

const CODES_SCH: &str = "eInvoicing-EN16931/ubl/schematron/codelist/EN16931-UBL-codes.sch";
const PEPPOL_SCH: &str = "peppol-bis-invoice-3/rules/sch/PEPPOL-EN16931-UBL.sch";

/// The three CEN syntax bindings, for the `BR-CL-08` union.
const PREPROCESSED: &[(&str, &str)] = &[
    (
        "ubl",
        "eInvoicing-EN16931/ubl/schematron/preprocessed/EN16931-UBL-validation-preprocessed.sch",
    ),
    (
        "cii",
        "eInvoicing-EN16931/cii/schematron/preprocessed/EN16931-CII-validation-preprocessed.sch",
    ),
    (
        "edifact",
        "eInvoicing-EN16931/edifact/schematron/codelist/EN16931-EDIFACT-codes.sch",
    ),
];

/// How a table's code list is picked out of its rule's test expression.
enum Select {
    /// The rule carries exactly one list. Fails if it carries more than one
    /// *distinct* list — identical repeats across contexts are collapsed.
    Unique,
    /// A disjunctive test: take the arm guarded by `self::<name>`.
    Guard(&'static str),
    /// A disjunctive test with no usable guard: take list `index`, and say why.
    Index { index: usize, why: &'static str },
}

struct Table {
    name: &'static str,
    rule: &'static str,
    select: Select,
    doc: &'static str,
}

struct PeppolTable {
    name: &'static str,
    list: &'static str,
    rule: &'static str,
    doc: &'static str,
}

/// A Peppol list that must remain byte-identical to a CEN table.
///
/// The rule documentation says they mirror; the generator proves it, so the
/// claim cannot rot into a lie.
struct Mirror {
    list: &'static str,
    cen_table: &'static str,
    rule: &'static str,
}

static TABLES: &[Table] = &[
    Table {
        name: "INVOICE_TYPE_CODES",
        rule: "BR-CL-01",
        select: Select::Guard("cbc:InvoiceTypeCode"),
        doc: "BT-3 on an invoice — UNTDID 1001, restricted. Note this is *not* the same list as `CREDIT_NOTE_TYPE_CODES`.",
    },
    Table {
        name: "CREDIT_NOTE_TYPE_CODES",
        rule: "BR-CL-01",
        select: Select::Guard("cbc:CreditNoteTypeCode"),
        doc: "BT-3 on a credit note — UNTDID 1001, restricted. Overlaps the invoice list in exactly one code, `81`.",
    },
    Table {
        name: "CURRENCY_CODES",
        rule: "BR-CL-04",
        select: Select::Unique,
        doc: "BT-5 / BT-6 — ISO 4217 alphabetic. Includes `XXX` (\"no currency involved\"), which this crate rejects separately.",
    },
    Table {
        name: "VAT_POINT_DATE_CODES",
        rule: "BR-CL-06",
        select: Select::Unique,
        doc: "BT-8 — a restriction of UNTDID 2005.",
    },
    Table {
        name: "REFERENCE_QUALIFIERS",
        rule: "BR-CL-07",
        select: Select::Unique,
        doc: "BT-128 scheme — a restriction of UNTDID 1153.",
    },
    Table {
        name: "ICD_SCHEMES",
        rule: "BR-CL-10",
        select: Select::Index {
            index: 0,
            why: "BR-CL-10's test is `<ICD list> or (@schemeID = 'SEPA' and (ancestor::cac:AccountingSupplierParty or ancestor::cac:PayeeParty))`. The literal `SEPA` is a CONTEXTUAL extension, not an alternative list: it is admissible only on a party identification under the supplier or the payee. It therefore belongs in the rule, which knows the context, and not in a flat lookup table that does not.",
        },
        doc: "Identifier scheme identifiers — ISO 6523 ICD. See `BR-CL-10`'s contextual `SEPA` extension, which is *not* in this table.",
    },
    Table {
        name: "ITEM_CLASSIFICATION_SCHEMES",
        rule: "BR-CL-13",
        select: Select::Unique,
        doc: "BT-158 scheme — UNTDID 7143.",
    },
    Table {
        name: "COUNTRY_CODES",
        rule: "BR-CL-14",
        select: Select::Unique,
        doc: "BT-40 / BT-55 / BT-69 / BT-80 — ISO 3166-1 alpha-2.",
    },
    Table {
        name: "PAYMENT_MEANS_CODES",
        rule: "BR-CL-16",
        select: Select::Unique,
        doc: "BT-81 — UNTDID 4461.",
    },
    Table {
        name: "VAT_CATEGORY_CODES",
        rule: "BR-CL-17",
        select: Select::Unique,
        doc: "BT-118 / BT-151 / BT-95 / BT-102 — UNCL 5305. Ten codes; see [`crate::codes::VatCategory`].",
    },
    Table {
        name: "ALLOWANCE_REASON_CODES",
        rule: "BR-CL-19",
        select: Select::Unique,
        doc: "BT-98 / BT-140 — UNCL 5189.",
    },
    Table {
        name: "CHARGE_REASON_CODES",
        rule: "BR-CL-20",
        select: Select::Unique,
        doc: "BT-105 / BT-145 — UNCL 7161.",
    },
    Table {
        name: "VATEX_CODES",
        rule: "BR-CL-22",
        select: Select::Unique,
        doc: "BT-121 — the CEF VATEX code list.",
    },
    Table {
        name: "UNIT_CODES",
        rule: "BR-CL-23",
        select: Select::Unique,
        doc: "BT-130 / BT-150 — UN/ECE Recommendation 20 with Rec 21 extensions.",
    },
    Table {
        name: "EAS_SCHEMES",
        rule: "BR-CL-25",
        select: Select::Unique,
        doc: "BT-34 / BT-49 scheme — the CEF Electronic Address Scheme list.",
    },
];

static PEPPOL_TABLES: &[PeppolTable] = &[
    PeppolTable {
        name: "PEPPOL_EAS_SCHEMES",
        list: "eaid",
        rule: "CL008",
        doc: "BT-34 / BT-49 scheme under Peppol BIS 3.0 — a **strict subset** of [`EAS_SCHEMES`].",
    },
    PeppolTable {
        name: "PEPPOL_MIME_CODES",
        list: "MIMECODE",
        rule: "CL001",
        doc: "BT-125 mime code under Peppol BIS 3.0. EN 16931 says only \"MIMEMediaType\"; Peppol names six.",
    },
];

static MIRRORS: &[Mirror] = &[
    Mirror {
        list: "UNCL5189",
        cen_table: "ALLOWANCE_REASON_CODES",
        rule: "CL002",
    },
    Mirror {
        list: "UNCL7161",
        cen_table: "CHARGE_REASON_CODES",
        rule: "CL003",
    },
    Mirror {
        list: "UNCL2005",
        cen_table: "VAT_POINT_DATE_CODES",
        rule: "CL006",
    },
];

// ---------------------------------------------------------------------------
// plumbing
// ---------------------------------------------------------------------------

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask/ has a parent")
        .to_path_buf()
}

fn spec() -> Result<PathBuf, String> {
    let p = root().join("spec");
    if p.is_dir() {
        Ok(p)
    } else {
        Err("no spec/ directory — run ./fetch-spec.sh".to_owned())
    }
}

fn read(spec: &Path, rel: &str) -> Result<String, String> {
    let p = spec.join(rel);
    std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))
}

/// One arm of a rule's test: the `self::X` guard that selects it, if any, and
/// the codes that arm accepts.
type Branch = (Option<String>, Vec<String>);

/// Every rule's arms, by rule id.
type Asserts = BTreeMap<String, Vec<Branch>>;

/// Every `contains(' … ')` literal in a `test`, attributed to the `self::X`
/// guard that precedes it.
///
/// Splitting the file on `<assert` and pattern-matching inside each chunk looks
/// like it works and then silently attributes one assertion's literals to its
/// neighbour — the same class of error as reading only the first branch of a
/// disjunction. So the file is parsed as XML.
fn load_asserts(xml: &str) -> Result<Asserts, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
    let mut out: Asserts = Asserts::new();
    for node in doc.descendants().filter(|n| n.has_tag_name("assert")) {
        let (Some(id), Some(expr)) = (node.attribute("id"), node.attribute("test")) else {
            continue;
        };
        for (literal, before) in single_quoted(expr) {
            let codes: Vec<String> = literal.split_whitespace().map(str::to_owned).collect();
            if codes.is_empty() {
                continue;
            }
            let guard = last_self_axis(before);
            out.entry(id.to_owned()).or_default().push((guard, codes));
        }
    }
    Ok(out)
}

/// Each `contains(' … ')` literal, with everything in the expression before it.
///
/// Only literals that are the argument of `contains(` count: a `test` also
/// carries separators and short guards in single quotes, and treating those as
/// code lists is how a table gains three spurious entries.
fn single_quoted(expr: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let bytes = expr.as_bytes();
    let mut i = 0;
    while let Some(rel) = expr[i..].find("contains(") {
        let open = i + rel;
        let mut j = open + "contains(".len();
        while j < bytes.len() && (bytes[j] as char).is_whitespace() {
            j += 1;
        }
        if j >= bytes.len() || bytes[j] != b'\'' {
            i = open + 1;
            continue;
        }
        let start = j + 1;
        let Some(end) = expr[start..].find('\'').map(|e| start + e) else {
            break;
        };
        out.push((&expr[start..end], &expr[..open]));
        i = end + 1;
    }
    out
}

/// The last `self::name` in an expression fragment.
fn last_self_axis(before: &str) -> Option<String> {
    let mut last = None;
    let mut i = 0;
    while let Some(rel) = before[i..].find("self::") {
        let start = i + rel + "self::".len();
        let end = before[start..]
            .find(|c: char| !(c.is_alphanumeric() || c == ':' || c == '_' || c == '-'))
            .map_or(before.len(), |e| start + e);
        if end > start {
            last = Some(before[start..end].to_owned());
        }
        i = start.max(i + 1);
    }
    last
}

/// Every `<let name=… value="' … '"/>` code list in Peppol's Schematron.
///
/// The value is an XPath `tokenize(' A B C ', '\s')`; the code list is the long
/// literal, never the separator.
fn peppol_lists(xml: &str) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    for chunk in xml.split("<let ").skip(1) {
        let Some(name) = attribute(chunk, "name") else {
            continue;
        };
        let Some(value) = attribute(chunk, "value") else {
            continue;
        };
        let decoded = value
            .replace("&quot;", "\"")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&");
        let best = decoded
            .split('\'')
            .skip(1)
            .step_by(2)
            .max_by_key(|l| l.split_whitespace().count());
        if let Some(best) = best
            && best.split_whitespace().count() > 2
        {
            let codes: BTreeSet<String> = best.split_whitespace().map(str::to_owned).collect();
            out.insert(name.to_owned(), codes.into_iter().collect());
        }
    }
    out
}

/// A double-quoted attribute value from a raw element fragment.
fn attribute<'a>(chunk: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    let start = chunk.find(&needle)? + needle.len();
    let end = chunk[start..].find('"')? + start;
    Some(&chunk[start..end])
}

/// UNCL 4451 for `BR-CL-08`, as the **union** over CEN's three syntax bindings.
///
/// The three disagree, because each froze a different UNTDID directory
/// revision: EDIFACT has 381 codes, UBL 383, CII 401. They form a strict chain,
/// so this is directory drift rather than a substantive disagreement about what
/// UNCL 4451 contains. This crate is syntax-independent and cannot know which
/// syntax an invoice will be written to, so it takes the union: rejecting a code
/// that a CEN binding accepts is a false positive, and a false positive on a
/// legally valid invoice is worse than accepting a code some older directory
/// lacked.
///
/// If the chain ever *breaks* — one binding gaining a code another dropped —
/// that is a real divergence, and this fails rather than papering over it.
fn note_subject_codes(spec: &Path, log: &mut Vec<String>) -> Result<Vec<String>, String> {
    let mut found: Vec<(&str, BTreeSet<String>)> = Vec::new();
    for (syntax, rel) in PREPROCESSED {
        let xml = read(spec, rel)?;
        let doc = roxmltree::Document::parse(&xml).map_err(|e| format!("{rel}: {e}"))?;
        let mut codes = BTreeSet::new();
        for node in doc.descendants().filter(|n| n.has_tag_name("assert")) {
            let hit = if *syntax == "edifact" {
                node.text().is_some_and(|t| t.contains("UNTDID 4451"))
            } else {
                node.attribute("id") == Some("BR-CL-08")
            };
            if !hit {
                continue;
            }
            let expr = node.attribute("test").unwrap_or_default();
            for literal in expr.split('\'').skip(1).step_by(2) {
                // The test also carries `'#'` separators and short guards; the
                // code list is the only literal with hundreds of tokens in it.
                if literal.split_whitespace().count() > 100 {
                    codes.extend(literal.split_whitespace().map(str::to_owned));
                }
            }
        }
        if codes.is_empty() {
            return Err(format!("BR-CL-08 code list not found in {syntax}"));
        }
        found.push((syntax, codes));
    }
    found.sort_by_key(|(_, c)| c.len());
    for pair in found.windows(2) {
        let [(small_name, small), (big_name, big)] = pair else {
            continue;
        };
        if !small.is_subset(big) {
            let extra: Vec<&String> = small.difference(big).take(8).collect();
            return Err(format!(
                "BR-CL-08 lists are no longer nested — {small_name} has {extra:?} which \
                 {big_name} lacks. That is a real divergence between CEN bindings, not \
                 directory drift; a union would silently hide it. Inspect the artefacts."
            ));
        }
    }
    log.push(format!(
        "    (BR-CL-08: {} — nested, union taken)",
        found
            .iter()
            .map(|(n, c)| format!("{n}={}", c.len()))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    Ok(found
        .last()
        .expect("three bindings")
        .1
        .iter()
        .cloned()
        .collect())
}

/// One `pub static NAME: &[&str]` table.
fn rust_slice(name: &str, doc: &str, rule: &str, codes: &[String]) -> String {
    let uniq: BTreeSet<&String> = codes.iter().collect();
    let mut s = format!(
        "/// {doc}\n///\n/// Source: `{rule}`, CEN validation artefacts `{ARTEFACT}`. {} values.\n\
         /// Sorted, so [`lookup`](super::contains) can binary-search it.\n\
         pub static {name}: &[&str] = &[\n",
        uniq.len()
    );
    // rustfmt reflows this anyway; emitting one per line keeps the generator
    // simple and the pre-format diff readable.
    for c in &uniq {
        s.push_str(&format!(
            "    \"{}\",\n",
            c.replace('\\', "\\\\").replace('"', "\\\"")
        ));
    }
    s.push_str("];\n");
    s
}

fn generate() -> Result<(String, Vec<String>), String> {
    let spec = spec()?;
    let asserts = load_asserts(&read(&spec, CODES_SCH)?)?;
    let mut log = Vec::new();
    let mut parts = vec![format!(
        "//! Code lists generated from the CEN validation artefacts.\n\
         //!\n\
         //! **Do not edit.** Regenerate with `cargo xtask codegen` after\n\
         //! `./fetch-spec.sh`, and review the diff.\n\
         //!\n\
         //! Artefact revision: `{ARTEFACT}`. Peppol tables: Peppol BIS Billing 3.0.\n\
         //!\n\
         //! Every table here is re-verified against the artefacts by\n\
         //! `tests/codelists.rs`, which runs whenever `spec/` is present. That test\n\
         //! exists because a Schematron `test` is a program, not a data structure:\n\
         //! `BR-CL-01` alone carries two different lists in one disjunctive\n\
         //! expression, and an extractor that reads only the first is confidently\n\
         //! wrong.\n"
    )];

    let mut emitted: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut total = 0usize;

    for table in TABLES {
        let branches = asserts
            .get(table.rule)
            .ok_or_else(|| format!("{} not found in the artefact", table.rule))?;
        let codes: Vec<String> = match &table.select {
            Select::Index { index, why } => {
                let branch = branches.get(*index).ok_or_else(|| {
                    format!(
                        "{} has {} lists; index {index} is out of range",
                        table.rule,
                        branches.len()
                    )
                })?;
                let dropped: Vec<usize> = branches
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| i != index)
                    .map(|(_, (_, c))| c.len())
                    .collect();
                log.push(format!(
                    "    ({}: took list {index}, dropped {dropped:?} — {})",
                    table.rule,
                    why.chars().take(60).collect::<String>()
                ));
                branch.1.clone()
            }
            Select::Guard(want) => {
                let matching: BTreeSet<Vec<String>> = branches
                    .iter()
                    .filter(|(g, _)| g.as_deref() == Some(*want))
                    .map(|(_, c)| {
                        c.iter()
                            .cloned()
                            .collect::<BTreeSet<_>>()
                            .into_iter()
                            .collect()
                    })
                    .collect();
                if matching.len() != 1 {
                    return Err(format!(
                        "{} has {} distinct lists guarded by {want}; expected exactly one",
                        table.rule,
                        matching.len()
                    ));
                }
                matching.into_iter().next().expect("exactly one")
            }
            Select::Unique => {
                let distinct: BTreeSet<Vec<String>> = branches
                    .iter()
                    .map(|(_, c)| {
                        c.iter()
                            .cloned()
                            .collect::<BTreeSet<_>>()
                            .into_iter()
                            .collect()
                    })
                    .collect();
                if distinct.len() != 1 {
                    let sizes: Vec<usize> = distinct.iter().map(Vec::len).collect();
                    return Err(format!(
                        "{} carries {} DIFFERENT lists {sizes:?} but {} expects one. Either \
                         the artefact changed shape or the test is disjunctive — inspect it \
                         and add a branch selector.",
                        table.rule,
                        distinct.len(),
                        table.name
                    ));
                }
                if branches.len() > 1 {
                    log.push(format!(
                        "    ({} repeats identically in {} contexts)",
                        table.rule,
                        branches.len()
                    ));
                }
                distinct.into_iter().next().expect("exactly one")
            }
        };
        let uniq: Vec<String> = codes
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        total += uniq.len();
        log.push(format!(
            "  {:30} {:5}  ({})",
            table.name,
            uniq.len(),
            table.rule
        ));
        parts.push(rust_slice(table.name, table.doc, table.rule, &uniq));
        emitted.insert(table.name, uniq);
    }

    // Peppol's own lists, and proof that the mirrored ones really do mirror.
    let peppol = peppol_lists(&read(&spec, PEPPOL_SCH)?);
    for m in MIRRORS {
        let got: BTreeSet<&String> = peppol.get(m.list).into_iter().flatten().collect();
        let want: BTreeSet<&String> = emitted
            .get(m.cen_table)
            .ok_or_else(|| format!("{} was not emitted", m.cen_table))?
            .iter()
            .collect();
        if got != want {
            let only_peppol: Vec<&&String> = got.difference(&want).take(8).collect();
            let only_cen: Vec<&&String> = want.difference(&got).take(8).collect();
            return Err(format!(
                "Peppol's ${} ({}) is no longer identical to {}: peppol-only {only_peppol:?}, \
                 cen-only {only_cen:?}. The rule doc says they mirror; either the artefacts \
                 diverged or the claim was always wrong. Emit a separate table.",
                m.list, m.rule, m.cen_table
            ));
        }
        log.push(format!(
            "    ({}: ${} is identical to {} — mirrored, not duplicated)",
            m.rule, m.list, m.cen_table
        ));
    }
    for t in PEPPOL_TABLES {
        let codes = peppol
            .get(t.list)
            .ok_or_else(|| format!("Peppol list ${} ({}) not found", t.list, t.rule))?;
        total += codes.len();
        let rule = format!("PEPPOL-EN16931-{}", t.rule);
        log.push(format!("  {:30} {:5}  ({rule})", t.name, codes.len()));
        parts.push(rust_slice(t.name, t.doc, &rule, codes));
    }

    let notes = note_subject_codes(&spec, &mut log)?;
    total += notes.len();
    log.push(format!(
        "  {:30} {:5}  (BR-CL-08)",
        "NOTE_SUBJECT_CODES",
        notes.len()
    ));
    parts.push(rust_slice(
        "NOTE_SUBJECT_CODES",
        "BT-21 — UNCL 4451. The **union** over CEN's UBL, CII and EDIFACT bindings, which \
         froze different UNTDID directory revisions; see `xtask/src/main.rs`.",
        "BR-CL-08",
        &notes,
    ));

    let tables = TABLES.len() + PEPPOL_TABLES.len() + 1;
    log.push(format!("\n{total} code values across {tables} tables"));
    Ok((parts.join("\n"), log))
}

/// Run the source through `rustfmt`.
///
/// Without this, `cargo xtask check` fails immediately after `cargo fmt`: the
/// generator's line breaks and rustfmt's disagree, so a freshly generated file
/// and a committed one never match.
fn rustfmt(source: &str) -> String {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    let Ok(mut child) = Command::new("rustfmt")
        .args(["--edition", "2024", "--emit", "stdout"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        eprintln!("warning: rustfmt not found — generated file will not be formatted");
        return source.to_owned();
    };
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all(source.as_bytes());
    }
    match child.wait_with_output() {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).into_owned(),
        _ => {
            eprintln!("warning: rustfmt failed — emitting unformatted source");
            source.to_owned()
        }
    }
}

const USAGE: &str = "\
usage: cargo xtask <task>

  codegen   regenerate src/codes/generated.rs from spec/
  check     regenerate and fail if the committed file differs
";

fn run(check_only: bool) -> Result<(), String> {
    let (source, log) = generate()?;
    let formatted = rustfmt(&source);
    for line in &log {
        println!("{line}");
    }

    let out = root().join("src").join("codes").join("generated.rs");
    let rel = out
        .strip_prefix(root())
        .unwrap_or(&out)
        .display()
        .to_string();
    let current = std::fs::read_to_string(&out)
        .map(|d| d.replace("\r\n", "\n") == formatted.replace("\r\n", "\n"))
        .unwrap_or(false);

    if current {
        println!("  up to date  {rel}");
        return Ok(());
    }
    if check_only {
        return Err(format!(
            "{rel} does not match the artefacts.\n\n\
             Run `cargo xtask codegen` and commit the result."
        ));
    }
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{rel}: {e}"))?;
    }
    std::fs::write(&out, &formatted).map_err(|e| format!("{rel}: {e}"))?;
    println!("  written     {rel}");
    println!("regeneration is deterministic: re-running produces a byte-identical file");
    Ok(())
}

fn main() -> ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("codegen") => run(false),
        Some("check") => run(true),
        Some(other) => Err(format!("unknown task {other:?}\n\n{USAGE}")),
        None => Err(format!("no task given\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
