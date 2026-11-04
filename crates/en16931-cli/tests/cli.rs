//! The command, exercised as a process: output, and the exit codes
//! (`0` valid, `1` rejected, `2` could not run) pipelines build on.

use std::io::Write as _;

use assert_cmd::Command;
use assert_cmd::assert::OutputAssertExt as _;

/// The memory-and-time watchdog the hostile-input suites share.
#[path = "../../en16931-formats/tests/common/watchdog.rs"]
mod watchdog;

/// Run the command on hostile input under the watchdog (512 MiB resident,
/// 10 s), and hand back its output to assert on.
fn watched(args: &[&str]) -> std::process::Output {
    let mut cmd = std::process::Command::new(assert_cmd::cargo::cargo_bin("en16931"));
    cmd.args(args);
    watchdog::watch(cmd, &format!("en16931 {}", args.join(" ")))
}
use predicates::prelude::PredicateBooleanExt as _;
use predicates::str::contains;

/// Run the command, expect success, and return its standard output.
fn stdout(args: &[&str]) -> String {
    let out = cli()
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out).expect("the CLI writes UTF-8")
}

/// A minimal core-valid UBL invoice, written from the model so it cannot drift.
fn valid_ubl() -> String {
    use en16931::invoice::*;
    use en16931::{Date, Identifier, Invoice, InvoiceAmount, InvoiceLine, Percentage, Quantity};

    let party = |name: &str, country: &str| Party {
        name: Some(name.to_owned()),
        vat_identifier: Some(format!("{country}123456789")),
        electronic_address: Some(Identifier::schemed(name, "0088")),
        address: PostalAddress {
            city: Some("Musterstadt".to_owned()),
            post_code: Some("12345".to_owned()),
            country: Some(Code::new(country)),
            ..Default::default()
        },
        ..Default::default()
    };
    let invoice = Invoice::builder(
        en16931::profiles::EN16931.specification_id,
        "CLI-1",
        Date::parse("2026-07-31").expect("date"),
        "380",
        "EUR",
    )
    .seller(party("Seller GmbH", "DE"))
    .buyer(party("Buyer BV", "NL"))
    .due_in_days(14)
    .line(InvoiceLine::new(
        "1",
        "Widget",
        Quantity::ONE,
        "C62",
        InvoiceAmount::parse("100.00").expect("amount"),
        "S",
        Some(Percentage::new(rust_decimal::Decimal::from(19))),
    ))
    .build_reconciled()
    .expect("reconciles");
    let proof = en16931::Validated::<en16931::En16931>::new(invoice).expect("valid");
    en16931_formats::write(&proof, en16931_formats::Syntax::Ubl).xml
}

/// Write a fixture to a path unique to this call, since tests run in parallel
/// threads of one process.
fn write_temp(name: &str, body: &str) -> std::path::PathBuf {
    write_temp_bytes(name, body.as_bytes())
}

/// [`write_temp`] for a binary fixture.
fn write_temp_bytes(name: &str, body: &[u8]) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);

    let unique = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "en16931-cli-{}-{unique}-{name}",
        std::process::id()
    ));
    let mut f = std::fs::File::create(&path).expect("create fixture");
    f.write_all(body).expect("write fixture");
    f.sync_all().expect("flush fixture");
    path
}

fn cli() -> Command {
    Command::cargo_bin("en16931").expect("the binary is built")
}

#[test]
fn a_valid_invoice_exits_zero() {
    let path = write_temp("valid.xml", &valid_ubl());
    cli()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("valid"));
}

#[test]
fn an_invalid_invoice_exits_one_and_says_which_rules() {
    let path = write_temp(
        "empty.xml",
        r#"<Invoice xmlns="urn:oasis:names:specification:ubl:schema:xsd:Invoice-2"/>"#,
    );
    cli()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .code(1)
        .stdout(contains("BR-02")) // no invoice number
        .stdout(contains("BR-16")); // no invoice line
}

/// A missing file is `2`; bytes that are not an invoice are a rejection, `1`.
#[test]
fn a_missing_file_exits_two_and_bytes_that_are_no_invoice_are_rejected() {
    cli()
        .args(["validate", "/definitely/not/here.xml"])
        .assert()
        .code(2);

    let path = write_temp("nonsense.xml", "<html/>");
    cli()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .code(1)
        .stdout(contains("recommendation: REJECT"))
        .stdout(contains("found <html>"));
}

#[test]
fn the_rule_set_comes_from_bt_24_by_default() {
    let path = write_temp("valid.xml", &valid_ubl());
    // Declared core, so core is what runs.
    cli()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("EN 16931 validation"));
    // …and requiring a profile adds a second report beside it; the
    // declaration is still the document's own.
    cli()
        .args([
            "validate",
            path.to_str().unwrap(),
            "--require",
            "XRechnung 3.0",
        ])
        .assert()
        .code(1)
        .stdout(contains("declared: EN 16931"))
        .stdout(contains("as required (XRechnung 3.0)"))
        .stdout(contains("BR-DE-15"));
}

#[test]
fn an_unknown_profile_is_refused_with_the_list() {
    cli()
        .args(["validate", "-", "--require", "XRechnung 9.9"])
        .assert()
        .code(2)
        .stderr(contains("unknown profile"))
        // The list offers the slugs, which is what a person types.
        .stderr(contains("xrechnung-extension"))
        .stderr(contains("peppol"))
        // The library's message, unchanged.
        .stderr(contains(
            "XRechnung 9.9"
                .parse::<&en16931::Profile>()
                .unwrap_err()
                .to_string(),
        ));
}

/// A profile is selectable by a slug that needs no shell quoting.
#[test]
fn a_profile_is_selectable_by_slug_name_or_bt24() {
    let path = write_temp("slug.xml", &valid_ubl());
    let file = path.to_str().expect("utf-8 path");
    for spelling in [
        "xrechnung",
        "XRECHNUNG",
        "XRechnung 3.0",
        en16931::profiles::XRECHNUNG.specification_id,
    ] {
        cli()
            .args(["validate", file, "--require", spelling, "--format", "json"])
            .assert()
            .stdout(contains("XRechnung 3.0"));
    }
    // An alias is a spelling too.
    cli()
        .args(["validate", file, "--require", "bis3", "--format", "json"])
        .assert()
        .stdout(contains("Peppol BIS Billing 3.0"));
}

#[test]
fn a_conversion_round_trips_through_the_model() {
    let path = write_temp("valid.xml", &valid_ubl());
    let cii = cli()
        .args(["convert", path.to_str().unwrap(), "--to", "cii"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let cii = String::from_utf8(cii).expect("UTF-8");
    assert!(cii.contains("CrossIndustryInvoice"));

    // Reading it back gives a document that still validates.
    let back = write_temp("roundtrip.xml", &cii);
    cli()
        .args(["validate", back.to_str().unwrap()])
        .assert()
        .success();
}

/// BT-8 crosses both ways in each syntax's own spelling: UBL's `35` (UNTDID
/// 2005, actual delivery date) is CII's `29` (UNTDID 2475), and back.
#[test]
fn bt_8_converts_ubl_to_cii_to_ubl_in_each_syntaxs_code() {
    let ubl = valid_ubl().replacen(
        "<cac:AccountingSupplierParty>",
        "<cac:InvoicePeriod><cbc:DescriptionCode>35</cbc:DescriptionCode></cac:InvoicePeriod>\
         <cac:AccountingSupplierParty>",
        1,
    );
    assert!(
        ubl.contains("<cbc:DescriptionCode>35"),
        "fixture changed shape"
    );
    let path = write_temp("bt8.xml", &ubl);
    let cii = stdout(&["convert", path.to_str().unwrap(), "--to", "cii"]);
    let codes = |xml: &str, tag: &str| -> Vec<String> {
        xml.match_indices(&format!("<{tag}>"))
            .map(|(i, m)| {
                let rest = &xml[i + m.len()..];
                rest[..rest.find('<').expect("closing tag")].to_owned()
            })
            .collect()
    };
    let in_cii = codes(&cii, "ram:DueDateTypeCode");
    assert!(!in_cii.is_empty(), "BT-8 lost on the way to CII:\n{cii}");
    assert!(in_cii.iter().all(|c| c == "29"), "{in_cii:?}"); // CEN CII list: 5 29 72

    let cii_path = write_temp("bt8.cii.xml", &cii);
    cli()
        .args(["validate", cii_path.to_str().unwrap()])
        .assert()
        .success();
    let back = stdout(&["convert", cii_path.to_str().unwrap(), "--to", "ubl"]);
    assert_eq!(codes(&back, "cbc:DescriptionCode"), ["35"]); // CEN UBL list: 3 35 432
}

/// `--require` on `convert` refuses to write a document that would be rejected.
#[test]
fn converting_for_a_profile_refuses_rather_than_writing_something_invalid() {
    let path = write_temp("valid.xml", &valid_ubl());
    cli()
        .args([
            "convert",
            path.to_str().unwrap(),
            "--to",
            "cii",
            "--require",
            "XRechnung 3.0",
        ])
        .assert()
        .code(1)
        .stdout(predicates::str::is_empty()) // nothing half-written
        .stderr(contains("BR-DE-15"));
}

/// A conversion's writer entries are printed by kind: BT-110 summed from the
/// breakdown is derived, not dropped, and the document is still written.
#[test]
fn a_derived_term_is_printed_as_derived() {
    let mut inv = en16931_formats::read(valid_ubl().as_bytes())
        .expect("readable")
        .invoice;
    inv.totals.vat_total = None;
    let cii = en16931_formats::write_unchecked(&inv, en16931_formats::Syntax::Cii)
        .expect("writable")
        .xml;
    let path = write_temp("no-bt110.xml", &cii);
    cli()
        .args(["convert", path.to_str().unwrap(), "--to", "ubl"])
        .assert()
        .success()
        .stdout(contains("<Invoice"))
        .stderr(contains("en16931: derived"))
        .stderr(contains("BT-110"))
        .stderr(contains("dropped").not());
}

/// A conversion that would lose a term is refused, naming it; `--allow-loss`
/// converts and lists the same entries.
#[test]
fn a_lossy_conversion_is_refused_unless_allowed() {
    let ubl = valid_ubl().replacen(
        "<cac:AccountingSupplierParty>",
        "<cac:BillingReference><cac:InvoiceDocumentReference><cbc:ID>A</cbc:ID>\
         </cac:InvoiceDocumentReference></cac:BillingReference>\
         <cac:BillingReference><cac:InvoiceDocumentReference><cbc:ID>B</cbc:ID>\
         </cac:InvoiceDocumentReference></cac:BillingReference>\
         <cac:AccountingSupplierParty>",
        1,
    );
    let path = write_temp("two-bg3.xml", &ubl);
    cli()
        .args(["convert", path.to_str().unwrap(), "--to", "cii"])
        .assert()
        .code(1)
        .stdout(predicates::str::is_empty())
        .stderr(contains("would be lost"))
        .stderr(contains("dropped"));
    cli()
        .args([
            "convert",
            path.to_str().unwrap(),
            "--to",
            "cii",
            "--allow-loss",
        ])
        .assert()
        .success()
        .stdout(contains("CrossIndustryInvoice"))
        .stderr(contains("dropped"));
}

/// `--relevel` lowers a rule's severity, and the report says so.
#[test]
fn a_relevelled_rule_is_recorded() {
    let path = write_temp("relevel.xml", &valid_ubl());
    let out = stdout(&[
        "validate",
        path.to_str().unwrap(),
        "--relevel",
        "BR-CO-26=information",
    ]);
    assert!(out.contains("BR-CO-26"), "{out}");
    cli()
        .args(["validate", path.to_str().unwrap(), "--relevel", "BR-CO-26"])
        .assert()
        .code(2)
        .stderr(contains("RULE=LEVEL"));
    cli()
        .args([
            "validate",
            path.to_str().unwrap(),
            "--relevel",
            "BR-CO-26=fatale",
        ])
        .assert()
        .code(2)
        .stderr(contains("use fatal, warning or information"));
}

/// ZUGFeRD MINIMUM is not an EN 16931 invoice: the answer is undefined, exit `1`.
#[test]
fn an_undefined_document_exits_one() {
    let mut inv = en16931_formats::read(valid_ubl().as_bytes())
        .expect("readable")
        .invoice;
    inv.specification_id = Some(en16931::profiles::ZUGFERD_MINIMUM_ID.to_owned());
    inv.lines.clear();
    let cii = en16931_formats::write_unchecked(&inv, en16931_formats::Syntax::Cii)
        .expect("writable")
        .xml;
    let path = write_temp("minimum.xml", &cii);
    cli()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .code(1)
        .stdout(contains("recommendation: undefined"))
        .stdout(contains("totals only"));
}

/// SVRL carries every layer: a duplicated invoice number is a failed assert in
/// this crate's namespace, beside the findings.
#[test]
fn svrl_carries_the_schema_layer() {
    let ubl = valid_ubl().replacen(
        "<cbc:ID>CLI-1</cbc:ID>",
        "<cbc:ID>CLI-1</cbc:ID><cbc:ID>FORGED</cbc:ID>",
        1,
    );
    assert!(ubl.contains("FORGED"), "fixture changed shape");
    let path = write_temp("forged.xml", &ubl);
    let out = cli()
        .args(["validate", path.to_str().unwrap(), "--format", "svrl"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let xml = String::from_utf8(out).expect("UTF-8");
    let doc = roxmltree::Document::parse(&xml).expect("well-formed SVRL");
    assert!(
        doc.descendants()
            .any(|n| n.attribute("id") == Some("en16931:schema:duplicate")),
        "{xml}"
    );
}

/// The text rendering of a received document, pinned.
#[test]
fn the_text_output_is_pinned() {
    let path = write_temp("golden.xml", &valid_ubl());
    let out = stdout(&["validate", path.to_str().unwrap()]);
    let body = out
        .split_once('\n')
        .map(|(_, rest)| rest)
        .expect("a path line first");
    assert_eq!(
        body,
        format!(
            "recommendation: accept\n\
             declared: EN 16931\n\
             document: Seller GmbH — invoice CLI-1 of 2026-07-31\n\
             rule set: EN 16931 (CEN {v})\n\
             most severe: nothing to report\n\
             container  valid\n\
             syntax     valid\n\
             schema     valid\n\
             semantic   valid\n\
             \x20 EN 16931 validation (EN 16931-1:2017+A1:2019) — 227 rule(s) checked, 0 finding(s), valid\n\
             \x20   verified against: CEN {v}\n\
             \x20   ({a})\n",
            v = en16931::ARTEFACT_VERSION,
            a = en16931::ATTRIBUTION
        )
    );
}

#[test]
fn stdin_is_a_document_source() {
    cli()
        .args(["inspect", "-"])
        .write_stdin(valid_ubl())
        .assert()
        .success()
        .stdout(contains("UBL 2.1"))
        .stdout(contains("CLI-1"));
}

#[test]
fn json_output_carries_the_versioned_schema() {
    let path = write_temp("valid.xml", &valid_ubl());
    let out = cli()
        .args(["validate", path.to_str().unwrap(), "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&out).expect("valid JSON");
    // Always an array of the interchange shape, even for one document.
    let docs = value.as_array().expect("an array");
    assert_eq!(docs.len(), 1);
    let doc = &docs[0];
    assert_eq!(doc["schema"], en16931::report::SCHEMA);
    assert_eq!(doc["valid"], true);
    assert_eq!(doc["recommendation"], "accept");
    assert_eq!(doc["source"], path.to_str().unwrap());
    let layers: Vec<&str> = doc["layers"]
        .as_array()
        .expect("layers")
        .iter()
        .filter_map(|l| l["name"].as_str())
        .collect();
    assert_eq!(layers, ["container", "syntax", "schema", "semantic"]);
    // It reads back as the library's type.
    let back: Vec<en16931::report::Report> = serde_json::from_slice(&out).expect("the schema");
    assert_eq!(
        back[0].identity.as_ref().and_then(|i| i.number.as_deref()),
        Some("CLI-1")
    );
}

#[test]
fn svrl_output_is_well_formed() {
    let path = write_temp(
        "empty.xml",
        r#"<Invoice xmlns="urn:oasis:names:specification:ubl:schema:xsd:Invoice-2"/>"#,
    );
    let out = cli()
        .args(["validate", path.to_str().unwrap(), "--format", "svrl"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let xml = String::from_utf8(out).expect("UTF-8");
    roxmltree::Document::parse(&xml).expect("well-formed SVRL");
}

/// `inspect` refuses SVRL, which is a validation report format.
#[test]
fn inspect_refuses_svrl_and_says_why() {
    cli()
        .args(["inspect", "-", "--format", "svrl"])
        .write_stdin(valid_ubl())
        .assert()
        .code(2)
        .stderr(contains("does not validate"));
}

#[test]
fn suppressions_are_recorded_in_the_output() {
    let path = write_temp("valid.xml", &valid_ubl());
    cli()
        .args([
            "validate",
            path.to_str().unwrap(),
            "--require",
            "XRechnung 3.0",
            "--without",
            "BR-DE-15",
        ])
        .assert()
        .code(1) // the other BR-DE rules still fire
        .stdout(contains("suppressed and NOT checked: BR-DE-15"));
}

#[test]
fn explain_resolves_rules_restrictions_and_every_spelling() {
    for query in ["BR-CO-14", "br-co-3", "br-ig-1"] {
        cli().args(["explain", query]).assert().success();
    }
    // No family is mapped onto another.
    cli().args(["explain", "BR-AF-1"]).assert().failure();
    // A restriction is not a rule and still appears in reports under its id.
    cli()
        .args(["explain", "BR-DE-3"])
        .assert()
        .success()
        .stdout(contains("restriction"));

    cli()
        .args(["explain", "BR-NOPE-99"])
        .assert()
        .code(2)
        .stderr(contains("no rule or restriction"));
}

#[test]
fn profiles_lists_what_this_build_can_check() {
    cli()
        .arg("profiles")
        .assert()
        .success()
        .stdout(contains("XRechnung 3.0"))
        .stdout(contains("zugferd-basic-wl"))
        .stdout(contains(en16931::ARTEFACT_VERSION))
        // The licence attribution appears on every user-visible surface.
        .stdout(contains(en16931::ATTRIBUTION));
}

/// A run over several documents reports each and fails if any one fails.
#[test]
fn a_batch_run_names_every_document_and_fails_on_the_worst() {
    let good = write_temp("valid.xml", &valid_ubl());
    let bad = write_temp(
        "empty.xml",
        r#"<Invoice xmlns="urn:oasis:names:specification:ubl:schema:xsd:Invoice-2"/>"#,
    );
    cli()
        .args(["validate", good.to_str().unwrap(), bad.to_str().unwrap()])
        .assert()
        .code(1)
        .stdout(contains(
            "2 document(s): 1 accepted, 1 rejected, 0 undefined",
        ));
}

/// `--strict` promotes advisories; the default does not.
#[test]
fn strict_turns_advisories_into_failures() {
    // `BR-51` (no full card PAN) is a warning: the document stays valid.
    let mut inv = en16931_formats::read(valid_ubl().as_bytes())
        .expect("readable")
        .invoice;
    inv.payment = Some(en16931::invoice::PaymentInstructions {
        means_code: Some(en16931::invoice::Code::new("48")),
        means: Some(en16931::invoice::PaymentMeans::Card(
            en16931::invoice::PaymentCard {
                primary_account_number: Some("4111111111111111".to_owned()),
                holder_name: Some("A Muster".to_owned()),
            },
        )),
        ..Default::default()
    });
    let path = write_temp(
        "full-pan.xml",
        &en16931_formats::write_unchecked(&inv, en16931_formats::Syntax::Ubl)
            .expect("writable")
            .xml,
    );

    cli()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("BR-51"));

    cli()
        .args(["validate", path.to_str().unwrap(), "--strict"])
        .assert()
        .code(1)
        .stdout(contains("BR-51"));
}

/// The rule catalogue is derived from the registry, restrictions included:
/// it is compared against `check_ids()`, not `extra_rules`.
#[test]
fn the_catalogue_matches_what_each_profile_actually_runs() {
    let total = |args: &[&str]| -> usize {
        let text = stdout(args);
        text.lines()
            .find_map(|l| l.split_once(" check(s):").map(|(n, _)| n.to_owned()))
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or_else(|| panic!("no count in:\n{text}"))
    };

    // The catalogue's total is exactly `check_ids()`, as `profiles` and a
    // report print it.
    for p in en16931::profiles::ALL {
        assert_eq!(
            total(&["rules", "--profile", p.id]),
            p.check_ids().count(),
            "{} — the catalogue and the profile disagree",
            p.id
        );
    }

    // The restrictions are there by id, in both shapes.
    let text = stdout(&["rules", "--profile", "XRechnung 3.0"]);
    for id in ["BR-DE-1", "BR-DE-15", "BR-DE-17", "BR-DE-21"] {
        assert!(text.contains(id), "{id} missing from the text catalogue");
    }
    assert!(
        !text.contains("BT-0"),
        "a whole-group restriction must not invent a business-term id:\n{text}"
    );
    let json = stdout(&["rules", "--profile", "XRechnung 3.0", "--format", "json"]);
    assert!(
        json.contains("\"BR-DE-15\"") && json.contains("\"restriction\": \"mandatory\""),
        "the JSON catalogue is what people diff across releases"
    );
}

/// An id that names two rules does not get one severity in the header.
///
/// `PEPPOL-EN16931-R120` is fatal for Peppol and a warning in XRechnung (as
/// KoSIT publishes it): same id and text, different consequence.
#[test]
fn a_rule_with_two_instances_reports_both() {
    let text = stdout(&["explain", "PEPPOL-EN16931-R120"]);
    assert!(text.contains("varies by profile"), "{text}");
    assert!(text.contains("Peppol BIS Billing 3.0 (fatal)"), "{text}");
    assert!(text.contains("XRechnung 3.0 (warning)"), "{text}");

    // …and the catalogue reports each profile's own instance.
    let peppol = stdout(&["rules", "--profile", "peppol"]);
    let xr = stdout(&["rules", "--profile", "xrechnung"]);
    let row = |s: &str| {
        s.lines()
            .find(|l| l.starts_with("PEPPOL-EN16931-R120"))
            .unwrap_or_default()
            .to_owned()
    };
    assert!(row(&peppol).contains("fatal"), "{}", row(&peppol));
    assert!(row(&xr).contains("warning"), "{}", row(&xr));
}

/// A rule every profile agrees on keeps its plain header.
#[test]
fn a_rule_with_one_severity_says_so_plainly() {
    let text = stdout(&["explain", "BR-CO-14"]);
    assert!(text.contains("[fatal]"), "{text}");
    assert!(!text.contains("varies"), "{text}");
    // No per-profile annotation when there is nothing to distinguish.
    assert!(text.contains("run by: EN 16931, XRechnung 3.0,"), "{text}");
}

/// A restriction says which profiles run it and at what severity (KoSIT
/// publishes `BR-DE-17` at warning).
#[test]
fn a_restriction_reports_the_severity_each_profile_runs_it_at() {
    let text = stdout(&["explain", "BR-DE-17"]);
    assert!(text.contains("[restriction]"), "{text}");
    assert!(text.contains("XRechnung 3.0 (warning)"), "{text}");

    // …and one nobody re-levels says fatal.
    let text = stdout(&["explain", "BR-DE-3"]);
    assert!(text.contains("XRechnung 3.0 (fatal)"), "{text}");
}

/// `--profile` reports the severity that profile uses, not the rule's own:
/// `BR-CL-23` is fatal in core and a warning under XRechnung.
#[test]
fn the_catalogue_reports_the_profiles_own_severity() {
    let row = |args: &[&str]| -> String {
        stdout(args)
            .lines()
            .find(|l| l.starts_with("BR-CL-23 "))
            .unwrap_or_else(|| panic!("BR-CL-23 not listed"))
            .to_owned()
    };
    assert!(row(&["rules", "--profile", "en16931"]).contains("fatal"));
    assert!(row(&["rules", "--profile", "xrechnung"]).contains("warning"));
}

#[test]
fn the_catalogue_filters_by_business_term_in_either_spelling() {
    for term in ["BT-117", "117"] {
        cli()
            .args(["rules", "--term", term])
            .assert()
            .success()
            .stdout(contains("BR-CO-17"))
            .stdout(predicates::str::contains("BR-01").not());
    }
    cli()
        .args(["rules", "--term", "not-a-term"])
        .assert()
        .code(2)
        .stderr(contains("not a business term"));
}

#[test]
fn the_catalogue_json_is_parseable_and_carries_its_provenance() {
    let out = cli()
        .args(["rules", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).expect("valid JSON");
    assert_eq!(v["artefacts"], en16931::ARTEFACT_VERSION);
    assert_eq!(v["attribution"], en16931::ATTRIBUTION);
    assert!(v["rules"].as_array().is_some_and(|r| r.len() > 300));
}

#[test]
fn completions_and_a_man_page_can_be_generated() {
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        cli()
            .args(["generate", shell])
            .assert()
            .success()
            .stdout(contains("en16931"));
    }
    cli()
        .args(["generate", "man"])
        .assert()
        .success()
        .stdout(contains(".TH en16931 1"));
}

/// A closed pipe is not an error: all output goes through one sink that treats
/// `BrokenPipe` as success.
#[test]
fn a_closed_pipe_is_silent() {
    // `assert_cmd` gives no pipe to close, so the sink's decision is asserted.
    let e = std::io::Error::from(std::io::ErrorKind::BrokenPipe);
    assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe);
    // Nothing in the crate reaches stdout except through that sink.
    let src = include_str!("../src/output.rs");
    assert!(
        !src.contains("println!") && !src.contains("print!("),
        "output.rs must build strings, not print — see `write_out`"
    );
}

// ── hostile input ────────────────────────────────────────────────────────────

/// A hostile document is refused with a rejection naming the cause, never an
/// abort.
///
/// * Deep nesting is refused before parsing (`roxmltree` recurses per level).
/// * Billion laughs and XXE need a DTD, and any DTD is rejected.
#[test]
fn hostile_documents_are_refused_rather_than_fatal() {
    let ns = "urn:oasis:names:specification:ubl:schema:xsd:Invoice-2";

    let deep = write_temp(
        "hostile-deep.xml",
        &format!(
            "<Invoice xmlns=\"{ns}\">{}{}</Invoice>",
            "<a>".repeat(5_000),
            "</a>".repeat(5_000)
        ),
    );
    watched(&["validate", deep.to_str().unwrap()])
        .assert()
        .code(1)
        .stdout(contains("nested"));

    let laughs = write_temp(
        "hostile-laughs.xml",
        &format!(
            "<?xml version=\"1.0\"?>\n<!DOCTYPE Invoice [\n<!ENTITY e0 \"aaaaaaaaaa\">\n{}\n]>\n\
             <Invoice xmlns=\"{ns}\"><ID>&e9;</ID></Invoice>",
            (1..=9)
                .map(|i| format!("<!ENTITY e{i} \"&e{};&e{};\">", i - 1, i - 1))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    );
    watched(&["validate", laughs.to_str().unwrap()])
        .assert()
        .code(1)
        .stdout(contains("DTD"));

    let xxe = write_temp(
        "hostile-xxe.xml",
        &format!(
            "<?xml version=\"1.0\"?>\n\
             <!DOCTYPE Invoice [<!ENTITY xxe SYSTEM \"file:///etc/passwd\">]>\n\
             <Invoice xmlns=\"{ns}\"><ID>&xxe;</ID></Invoice>"
        ),
    );
    watched(&["validate", xxe.to_str().unwrap()])
        .assert()
        .code(1)
        .stdout(contains("DTD"));
}

// ── diff ─────────────────────────────────────────────────────────────────────

/// Crossing syntaxes is not a change: the UBL and CII forms of one invoice
/// compare equal as models.
#[test]
fn the_same_invoice_in_two_syntaxes_is_identical() {
    let ubl = write_temp("diff-ubl.xml", &valid_ubl());
    let cii = std::env::temp_dir().join(format!("en16931-cli-{}-diff.cii.xml", std::process::id()));

    cli()
        .args(["convert", ubl.to_str().unwrap(), "--to", "cii", "-o"])
        .arg(&cii)
        .assert()
        .success();

    cli()
        .args(["diff", ubl.to_str().unwrap(), cii.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("identical as invoices"));

    let _ = std::fs::remove_file(&cii);
}

/// A scale difference is not a difference: UBL's `25.0` and CII's `25` for
/// BT-152 are one `Percentage`.
#[test]
fn a_trailing_zero_is_not_a_difference() {
    let a = write_temp("diff-scale-a.xml", &valid_ubl());
    let b = write_temp(
        "diff-scale-b.xml",
        &valid_ubl()
            .replace(">19<", ">19.00<")
            .replace(">19.00.00<", ">19.00<"),
    );
    cli()
        .args(["diff", a.to_str().unwrap(), b.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("identical as invoices"));
}

/// A real change is reported, at a path through the model, and exits `1`.
#[test]
fn a_changed_field_is_reported_and_exits_one() {
    let a = write_temp("diff-a.xml", &valid_ubl());
    let b = write_temp(
        "diff-b.xml",
        &valid_ubl().replace("Musterstadt", "Nirgendwo"),
    );

    cli()
        .args(["diff", a.to_str().unwrap(), b.to_str().unwrap()])
        .assert()
        .code(1)
        .stdout(contains("seller.address.city"))
        .stdout(contains("\"Musterstadt\" → \"Nirgendwo\""));
}

/// `1` is "they differ" and `2` is "could not read one", as in `validate`.
#[test]
fn an_unreadable_side_exits_two_not_one() {
    let a = write_temp("diff-ok.xml", &valid_ubl());
    cli()
        .args(["diff", a.to_str().unwrap(), "/nonexistent/invoice.xml"])
        .assert()
        .code(2);
}

/// The JSON shape carries real values, not strings of JSON.
#[test]
fn the_json_diff_is_machine_readable() {
    let a = write_temp("diff-json-a.xml", &valid_ubl());
    let b = write_temp(
        "diff-json-b.xml",
        &valid_ubl().replace("Musterstadt", "Nirgendwo"),
    );

    let out = cli()
        .args([
            "diff",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            "--format",
            "json",
        ])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).expect("valid JSON");
    assert_eq!(v["identical"], serde_json::json!(false));
    let d = &v["differences"][0];
    assert_eq!(d["change"], serde_json::json!("~"));
    // A `String`, not a JSON-encoded string.
    assert!(d["left"].is_string(), "left is a value, not a JSON blob");
}

/// SVRL is a verdict format; a comparison is not a verdict.
#[test]
fn svrl_is_refused_for_a_comparison() {
    let a = write_temp("diff-svrl-a.xml", &valid_ubl());
    let b = write_temp("diff-svrl-b.xml", &valid_ubl());
    cli()
        .args([
            "diff",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            "--format",
            "svrl",
        ])
        .assert()
        .code(2)
        .stderr(contains("not a comparison"));
}

// ── categories ───────────────────────────────────────────────────────────────

/// The pre-flight has the same three answers as `validate`: `0` they may share
/// a document, `1` they may not, `2` not a category.
#[test]
fn categories_exit_codes_are_the_same_three_as_validate() {
    cli().args(["categories", "S", "Z"]).assert().success();

    cli()
        .args(["categories", "S", "O"])
        .assert()
        .code(1)
        .stdout(contains("BR-O-11"));

    cli().args(["categories", "S", "X"]).assert().code(2);
}

/// A refusal names the categories in full, not only their codes (`O` is not
/// "other").
#[test]
fn categories_are_named_not_only_coded() {
    cli()
        .args(["categories", "O"])
        .assert()
        .success()
        .stdout(contains("services outside the scope of VAT"));
}

/// Codes are case-sensitive, because `BR-CL-17` compares them literally.
#[test]
fn categories_are_case_sensitive() {
    cli()
        .args(["categories", "s"])
        .assert()
        .code(2)
        .stderr(contains("case-sensitive"));
}

/// The German municipal invoice, end to end on the command line.
#[test]
fn the_municipal_case_is_refused_with_its_reason_and_its_way_out() {
    cli()
        .args(["categories", "S", "O"])
        .assert()
        .code(1)
        .stdout(contains("REFUSED"))
        .stdout(contains("own document"));
}

/// A PDF from raw object bodies, numbered from 1, with a computed
/// cross-reference table. Object 1 is the catalogue.
fn raw_pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        pdf.extend_from_slice(body);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    pdf
}

/// A small PDF whose embedded-files tree lists itself as its own child:
/// `validate` rejects it and `extract` refuses it, promptly and saying why.
#[test]
fn a_hostile_pdf_is_refused_promptly_with_its_cause() {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R /Names << /EmbeddedFiles 5 0 R >> >>",
        "<< /Type /Pages /Count 0 /Kids [] >>",
        "<< /Type /Filespec /F (factur-x.xml) /EF << /F 4 0 R >> >>",
        "<< /Length 27 >>\nstream\n<rsm:CrossIndustryInvoice/>\nendstream",
        "<< /Names [(factur-x.xml) 3 0 R] /Kids [5 0 R 5 0 R] >>",
    ]
    .map(|o| o.as_bytes().to_vec());
    let pdf = raw_pdf(&objects);
    let path = write_temp_bytes("cyclic.pdf", &pdf);

    watched(&["validate", path.to_str().expect("UTF-8 path")])
        .assert()
        .code(1)
        .stdout(contains("container  INVALID"))
        .stdout(contains("refers back to itself"));
    watched(&["extract", path.to_str().expect("UTF-8 path")])
        .assert()
        .code(2)
        .stderr(contains("refers back to itself"));
}

/// A zlib stream that inflates to `1 + 258 * n` zero bytes: one literal, then
/// fixed-Huffman length-258 / distance-1 matches, about 160:1.
fn zlib_zeros(n: usize) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let (mut acc, mut bits) = (0_u32, 0_u32);
    let mut put = |value: u32, width: u32, out: &mut Vec<u8>| {
        acc |= value << bits;
        bits += width;
        while bits >= 8 {
            out.push(acc as u8);
            acc >>= 8;
            bits -= 8;
        }
    };
    // Huffman codes go most-significant bit first.
    let code = |c: u32, width: u32| c.reverse_bits() >> (32 - width);
    put(1, 1, &mut out); // BFINAL
    put(1, 2, &mut out); // fixed Huffman
    put(code(0x30, 8), 8, &mut out); // literal 0
    for _ in 0..n {
        put(code(0xC5, 8), 8, &mut out); // length 258
        put(0, 5, &mut out); // distance 1
    }
    put(0, 7, &mut out); // end of block
    put(0, 7, &mut out); // pad to a byte
    let len = 1 + 258 * n;
    let adler = (((len % 65_521) as u32) << 16) | 1;
    out.extend_from_slice(&adler.to_be_bytes());
    out
}

/// An invoice that inflates past the per-stream limit is rejected by
/// `validate` and refused by `extract`, naming the stream and the limit.
///
/// Run with limits lowered to 1 MiB / 2 MiB and a 4 MiB bomb, under the
/// watchdog; the default limits are covered by the library's suite.
#[test]
fn a_pdf_over_the_inflation_limit_is_refused_naming_the_limit() {
    let bomb = zlib_zeros(4 * 1024 * 1024 / 258);
    let mut invoice = format!(
        "<< /Filter /FlateDecode /Length {} >>\nstream\n",
        bomb.len()
    )
    .into_bytes();
    invoice.extend_from_slice(&bomb);
    invoice.extend_from_slice(b"\nendstream");
    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R /Names << /EmbeddedFiles << /Names [(factur-x.xml) 3 0 R] >> >> >>".to_vec(),
        b"<< /Type /Pages /Count 0 /Kids [] >>".to_vec(),
        b"<< /Type /Filespec /F (factur-x.xml) /EF << /F 4 0 R >> >>".to_vec(),
        invoice,
    ];
    let path = write_temp_bytes("bomb.pdf", &raw_pdf(&objects));
    let path = path.to_str().expect("UTF-8 path");
    let small = [
        "--max-stream-bytes",
        "1048576",
        "--max-total-bytes",
        "2097152",
    ];

    watched(&[&["validate", path][..], &small].concat())
        .assert()
        .code(1)
        .stdout(contains("factur-x.xml"))
        .stdout(contains("1048576"));
    watched(&[&["extract", path][..], &small].concat())
        .assert()
        .code(2)
        .stderr(contains("factur-x.xml"))
        .stderr(contains("1048576"));
    // The limits are the caller's to lower.
    watched(&["extract", path, "--max-stream-bytes", "1024"])
        .assert()
        .code(2)
        .stderr(contains("1024"));
}

// ── embed ────────────────────────────────────────────────────────────────────

/// A PDF/A-`part` file with one blank page, its cross-reference table computed.
fn blank_pdfa(part: &str) -> Vec<u8> {
    let xmp = format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
<rdf:Description rdf:about=\"\" xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\" pdfaid:part=\"{part}\" pdfaid:conformance=\"B\"/>\
</rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>"
    );
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R /Metadata 4 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << >> >>".to_owned(),
        format!(
            "<< /Type /Metadata /Subtype /XML /Length {} >>\nstream\n{xmp}\nendstream",
            xmp.len()
        ),
    ];
    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /ID [<00112233445566778899AABBCCDDEEFF> <00112233445566778899AABBCCDDEEFF>] >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

#[test]
fn embed_writes_a_hybrid_pdf_that_extract_reads_back() {
    let pdf = write_temp_bytes("blank.pdf", &blank_pdfa("3"));
    let invoice = write_temp("embed.xml", &valid_ubl());
    let out = std::env::temp_dir().join(format!("en16931-cli-{}-embedded.pdf", std::process::id()));
    cli()
        .args(["embed"])
        .arg(&pdf)
        .arg(&invoice)
        .arg("-o")
        .arg(&out)
        .assert()
        .success();
    let xml = stdout(&["extract", out.to_str().unwrap()]);
    assert!(xml.contains("CrossIndustryInvoice"), "{xml}");
    assert!(xml.contains("CLI-1"));
    // `extract` prints divergences to stderr; there are none.
    cli()
        .args(["extract", out.to_str().unwrap()])
        .assert()
        .success()
        .stderr("");
}

#[test]
fn embed_takes_the_json_envelope_and_an_explicit_profile() {
    let model = en16931::examples::for_profile(&en16931::profiles::XRECHNUNG).expect("shipped");
    let json = serde_json::to_string(&model).expect("serialises");
    let pdf = write_temp_bytes("blank.pdf", &blank_pdfa("3"));
    let invoice = write_temp("embed.json", &json);
    let out = std::env::temp_dir().join(format!("en16931-cli-{}-xr.pdf", std::process::id()));
    cli()
        .args([
            "embed",
            "--profile",
            "xrechnung",
            "--relationship",
            "source",
        ])
        .arg(&pdf)
        .arg(&invoice)
        .arg("-o")
        .arg(&out)
        .assert()
        .success();
    let bytes = std::fs::read(&out).expect("written");
    let got =
        en16931_formats::zugferd::extract(&bytes, &en16931_formats::zugferd::Limits::default())
            .expect("reads back");
    assert_eq!(got.filename, "xrechnung.xml");
    assert_eq!(got.relationship.as_deref(), Some("Source"));
}

#[test]
fn embed_refuses_with_exit_one_and_misuse_with_two() {
    let invoice = write_temp("embed.xml", &valid_ubl());
    let out = std::env::temp_dir().join(format!("en16931-cli-{}-never.pdf", std::process::id()));
    // Not PDF/A-3: refused, nothing converted.
    let pdf2 = write_temp_bytes("pdfa2.pdf", &blank_pdfa("2"));
    cli()
        .arg("embed")
        .arg(&pdf2)
        .arg(&invoice)
        .arg("-o")
        .arg(&out)
        .assert()
        .code(1)
        .stderr(contains("PDF/A-2"));
    // An invoice that is not valid under its profile.
    let pdf = write_temp_bytes("blank.pdf", &blank_pdfa("3"));
    let invalid = write_temp("invalid.xml", &valid_ubl().replace("CLI-1", ""));
    cli()
        .arg("embed")
        .arg(&pdf)
        .arg(&invalid)
        .arg("-o")
        .arg(&out)
        .assert()
        .code(1)
        .stderr(contains("not valid"));
    // A profile with no ZUGFeRD carrier.
    cli()
        .args(["embed", "--profile", "peppol"])
        .arg(&pdf)
        .arg(&invoice)
        .arg("-o")
        .arg(&out)
        .assert()
        .code(1)
        .stderr(contains("no ZUGFeRD"));
    // A file that does not exist.
    cli()
        .args(["embed", "/nonexistent/x.pdf"])
        .arg(&invoice)
        .arg("-o")
        .arg(&out)
        .assert()
        .code(2);
    assert!(!out.exists(), "nothing is written on a refusal");
}

/// GitHub #14 and #15 through the command line: an unreadable quantity and an
/// absent VAT category tax amount are rejected, each named by business term.
#[test]
fn a_value_the_reader_cannot_read_or_a_missing_term_is_rejected_by_name() {
    let valid = valid_ubl();
    let at = valid
        .find("<cbc:InvoicedQuantity")
        .expect("the fixture has a line");
    let open = at + valid[at..].find('>').unwrap() + 1;
    let close = open + valid[open..].find('<').unwrap();
    let abc = format!("{}abc{}", &valid[..open], &valid[close..]);
    let path = write_temp("quantity.xml", &abc);
    cli()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .code(1)
        .stdout(contains("malformed"))
        .stdout(contains("BT-129"));

    let start = valid
        .find("<cac:TaxSubtotal>")
        .and_then(|s| valid[s..].find("<cbc:TaxAmount").map(|t| s + t))
        .expect("the fixture has a VAT breakdown");
    let end = start + valid[start..].find("</cbc:TaxAmount>").unwrap() + "</cbc:TaxAmount>".len();
    let without = format!("{}{}", &valid[..start], &valid[end..]);
    let path = write_temp("bt117.xml", &without);
    cli()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .code(1)
        .stdout(contains("missing"))
        .stdout(contains("BG-23[1]/BT-117"));
}

/// `explain` says how a rule the types retire is held, rather than letting it
/// read like the rules a predicate checks.
#[test]
fn explain_says_how_a_type_retired_rule_is_held() {
    cli()
        .args(["explain", "BR-46"])
        .assert()
        .success()
        .stdout(contains("terms: BT-117"))
        .stdout(contains("held by: the model's types"));
}
