#![cfg(all(feature = "ubl", feature = "cii"))]

//! Every document the suites write is valid against its syntax's schema: CII
//! against CEN's uncoupled D16B subset schema (code lists are the `BR-CL-*`
//! rules' business), UBL against the OASIS UBL 2.1 `Invoice` or `CreditNote`
//! schema. The round-trip suites read by local name, so they cannot catch a
//! wrong child type or namespace.
//!
//! Uses `xmllint` at test time. Without it or the reference corpus the gate
//! skips; under `EN16931_REQUIRE_SPEC` it fails.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use en16931::Invoice;
use en16931::codes::generated::VAT_POINT_DATE_CODES;
use en16931::invoice::Code;
use en16931::profiles::{En16931, PeppolBis3, XRechnung, XRechnungCvd, XRechnungExtension};
use en16931::validation::profile::{ProfileMarker, Validated};
use en16931_formats::report::Kind;
use en16931_formats::{Syntax, Unwritable};

/// CEN's uncoupled D16B subset schema, relative to the reference corpus.
const CII_XSD: &str = "eInvoicing-EN16931/cii/schema/D16B SCRDM (Subset)/uncoupled clm/CII/uncefact/data/standard/CrossIndustryInvoice_100pD16B.xsd";

/// The OASIS UBL 2.1 schemas, relative to the reference corpus: one per root.
const UBL_INVOICE_XSD: &str = "oasis-ubl-2.1/xsd/maindoc/UBL-Invoice-2.1.xsd";
const UBL_CREDIT_NOTE_XSD: &str = "oasis-ubl-2.1/xsd/maindoc/UBL-CreditNote-2.1.xsd";

/// The number of documents gated per syntax at the pinned releases (CEN
/// `validation-1.3.17`, KoSIT `v2.6.0` / `v2026-08-31`, Peppol 3.0.21). Raise only.
const CII_FLOOR: usize = 509;
const UBL_FLOOR: usize = 509;

/// Models the schema has no valid form for, which the writer refuses
/// (`Unwritable`). Exact per syntax.
///
/// CII (D16B requires BT-1 and BT-2): the empty invoice `roundtrip` and
/// `zugferd` write, and KoSIT's `processing-valid/ubl005.xml`, whose
/// `cbc:IssueDate` is commented out.
const CII_EXEMPT: usize = 2;
/// UBL 2.1 also requires the seller, the buyer and a line: those two, and the
/// header-only payload `zugferd` embeds.
const UBL_EXEMPT: usize = 3;

/// `xmllint`, if it is on `PATH`.
fn xmllint() -> Option<()> {
    Command::new("xmllint")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|_| ())
}

/// Skip with a line naming what is missing — or fail, under `EN16931_REQUIRE_SPEC`.
fn skip(gate: &str, what: &str) {
    assert!(
        std::env::var_os(common::REQUIRE).is_none(),
        "the {gate} schema gate needs {what}, and {} is set, so skipping is not \
         permitted here",
        common::REQUIRE
    );
    eprintln!(
        "note: {gate} schema gate SKIPPED — {what}. Set {}=1 to make this a failure.",
        common::REQUIRE
    );
}

/// One written document: where it came from, the schema it is checked
/// against, and the XML.
struct Doc {
    origin: String,
    xsd: &'static str,
    xml: String,
}

/// Write `inv` in `syntax`: the XML and the schema the document's root
/// selects — or the writer's refusal of a model the schema has no form for.
fn write(syntax: Syntax, inv: &Invoice) -> Result<(String, &'static str), Unwritable> {
    let w = en16931_formats::write_unchecked(inv, syntax)?;
    Ok((w.xml, xsd_for(syntax, inv)))
}

/// The schema a document of `syntax` written from `inv` is checked against.
fn xsd_for(syntax: Syntax, inv: &Invoice) -> &'static str {
    match syntax {
        Syntax::Cii => CII_XSD,
        Syntax::Ubl => ubl_xsd(inv),
        other => unreachable!("no schema gate for {other}"),
    }
}

/// `inv`, stamped with `P`'s BT-24, validated as `P` and written — `None` when
/// it does not pass `P`.
fn scoped<P: ProfileMarker>(syntax: Syntax, inv: &Invoice) -> Option<Doc> {
    let mut inv = inv.clone();
    inv.specification_id = Some(P::PROFILE.specification_id.to_owned());
    let proof = Validated::<P>::new(inv).ok()?;
    let w = en16931_formats::write(&proof, syntax);
    Some(Doc {
        origin: format!("profile-scoped: {}", P::PROFILE.id),
        xsd: xsd_for(syntax, proof.invoice()),
        xml: w.xml,
    })
}

fn ubl_xsd(inv: &Invoice) -> &'static str {
    if matches!(inv.kind, en16931::DocumentKind::CreditNote) {
        UBL_CREDIT_NOTE_XSD
    } else {
        UBL_INVOICE_XSD
    }
}

/// Every `syntax` document the suites write, with where it came from — minus
/// the exempt ones, which are counted.
fn written(syntax: Syntax) -> (Vec<Doc>, usize) {
    let (gate, other) = match syntax {
        Syntax::Cii => ("CII schema gate", Syntax::Ubl),
        Syntax::Ubl => ("UBL schema gate", Syntax::Cii),
        other => unreachable!("no schema gate for {other}"),
    };
    let mut out: Vec<Doc> = Vec::new();
    let mut exempt = 0;
    let mut push = |origin: String, inv: &Invoice| match write(syntax, inv) {
        Ok((xml, xsd)) => {
            assert!(
                inv.number.is_some() && inv.issue_date.is_some(),
                "{origin}: no BT-1 / BT-2, and the writer did not refuse it"
            );
            out.push(Doc { origin, xsd, xml });
        }
        Err(refused) => {
            assert!(
                refused.entries.iter().any(|e| e.kind == Kind::Schema),
                "{origin}: refused without naming what the schema lacks: {refused:?}"
            );
            exempt += 1;
        }
    };

    // The fixtures `roundtrip`, `order` and `subset` write.
    push("fixture: maximal invoice".into(), &common::maximal());
    push(
        "fixture: maximal credit note".into(),
        &common::maximal_credit_note(),
    );
    push("fixture: empty invoice".into(), &Invoice::default());
    // `subset`: the maximal invoice with sub invoice lines.
    let mut sub = common::maximal();
    let child = sub.lines[0].clone();
    sub.extensions.sub_invoice_lines = vec![(
        0,
        vec![en16931::SubInvoiceLine {
            line: child,
            vat: Some(sub.lines[0].vat.clone()),
            quantity_stated: true,
            price_stated: true,
            children: vec![],
        }],
    )];
    push("fixture: sub invoice lines".into(), &sub);
    // `zugferd`: the header-only payload it embeds.
    let mut header = Invoice::default();
    header.number = Some("RE-2026-0001".into());
    header.issue_date = Some(en16931::Date::parse("2026-01-15").expect("date"));
    header.currency = Some(Code::new("EUR"));
    header.specification_id = Some("urn:cen.eu:en16931:2017".into());
    push("fixture: header-only payload".into(), &header);
    // `billing_seam`: a derived `billing` document through the adapter.
    push("fixture: billing seam".into(), &common::billing_invoice());
    // BT-8 in each of its three spellings (`vat_point_date_code`).
    for code in VAT_POINT_DATE_CODES {
        let mut inv = common::maximal();
        inv.vat_point_date = None;
        inv.vat_point_date_code = Some(Code::new(*code));
        push(format!("fixture: BT-8 {code}"), &inv);
    }
    // `fidelity`: every authority document of this syntax, read and rewritten;
    // `cross_syntax`: every one of the other, converted.
    for (from, verb) in [(syntax, "rewritten"), (other, "converted")] {
        for (path, text) in common::authority_documents(from, gate) {
            if let Ok(read) = en16931_formats::read(text.as_bytes()) {
                push(format!("{verb}: {}", path.display()), &read.invoice);
            }
        }
    }

    // `profile_scoped`: the valid fixture written for every profile it passes.
    let mut valid = common::maximal();
    valid.vat_point_date_code = None; // BR-CO-03: BT-7 xor BT-8
    let docs = [
        scoped::<En16931>(syntax, &valid),
        scoped::<XRechnung>(syntax, &valid),
        scoped::<XRechnungCvd>(syntax, &valid),
        scoped::<XRechnungExtension>(syntax, &valid),
        scoped::<PeppolBis3>(syntax, &valid),
    ];
    assert_eq!(
        docs.len(),
        en16931::profiles::ALL.len(),
        "a profile was added: give its marker a line here"
    );
    let scoped = docs.iter().flatten().count();
    out.extend(docs.into_iter().flatten());
    assert!(scoped > 0, "the valid fixture passed no profile");
    (out, exempt)
}

/// Validate `docs` against `xsd` in one `xmllint` run; the failures, by origin.
fn validate(xsd: &Path, docs: &[&Doc]) -> BTreeMap<String, Vec<String>> {
    let stem = xsd.file_stem().and_then(|s| s.to_str()).unwrap_or("schema");
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("schema-gate-{stem}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch directory");
    let files: Vec<PathBuf> = docs
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let f = dir.join(format!("{i:05}.xml"));
            std::fs::write(&f, &d.xml).expect("write scratch document");
            f
        })
        .collect();

    let run = Command::new("xmllint")
        .arg("--noout")
        .arg("--schema")
        .arg(xsd) // one argument: the path holds spaces and parentheses
        .args(&files)
        .output()
        .expect("run xmllint");
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        !stderr.contains("failed to compile") && !stderr.contains("WXS schema"),
        "xmllint could not load the schema {}:\n{stderr}",
        xsd.display()
    );

    // Lines look like `<file>:<line>: element …: Schemas validity error : …`
    // and `<file> fails to validate`; map each back to its origin.
    let mut failures: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in stderr.lines() {
        let Some((i, f)) = files
            .iter()
            .enumerate()
            .find(|(_, f)| line.starts_with(&*f.to_string_lossy()))
        else {
            continue;
        };
        // Status lines carry no message; the errors before them do.
        let Some(message) = line[f.to_string_lossy().len()..].strip_prefix(':') else {
            continue;
        };
        failures
            .entry(docs[i].origin.clone())
            .or_default()
            .push(format!("line {message}"));
    }
    // A non-zero exit with nothing attributed means the output was misread.
    assert!(
        run.status.success() || !failures.is_empty(),
        "xmllint exited {:?} and no document was named:\n{stderr}",
        run.status.code()
    );
    failures
}

/// Gate every `syntax` document the suites write against `schemas`.
fn gate(syntax: Syntax, name: &str, schemas: &[&str], floor: usize, expected_exempt: usize) {
    let Some(root) = common::spec_root() else {
        skip(name, "the reference corpus (`cargo xtask fetch`)");
        return;
    };
    for xsd in schemas {
        if !root.join(xsd).is_file() {
            skip(name, &format!("the schema at {}", root.join(xsd).display()));
            return;
        }
    }
    if xmllint().is_none() {
        skip(name, "`xmllint` (libxml2) on PATH");
        return;
    }

    let (docs, exempt) = written(syntax);
    assert_eq!(
        exempt, expected_exempt,
        "{name}: models the writer refuses as unwritable"
    );
    let mut failures = BTreeMap::new();
    for xsd in schemas {
        let group: Vec<&Doc> = docs.iter().filter(|d| d.xsd == *xsd).collect();
        failures.extend(validate(&root.join(xsd), &group));
    }
    let report: Vec<String> = failures
        .iter()
        .map(|(origin, errs)| format!("{origin}\n    {}", errs.join("\n    ")))
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} written {name} document(s) fail the schema:\n\n{}",
        failures.len(),
        docs.len(),
        report.join("\n\n")
    );
    println!(
        "{name} schema gate: {} written documents validate ({exempt} exempt: refused as \
         unwritable)",
        docs.len()
    );
    assert!(
        docs.len() >= floor,
        "only {} {name} documents gated, the floor is {floor} — is the corpus complete?",
        docs.len()
    );
}

#[test]
fn every_written_cii_document_is_valid_against_d16b() {
    gate(Syntax::Cii, "CII", &[CII_XSD], CII_FLOOR, CII_EXEMPT);
}

#[test]
fn every_written_ubl_document_is_valid_against_ubl_2_1() {
    gate(
        Syntax::Ubl,
        "UBL",
        &[UBL_INVOICE_XSD, UBL_CREDIT_NOTE_XSD],
        UBL_FLOOR,
        UBL_EXEMPT,
    );
}
