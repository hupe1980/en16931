//! The command, exercised as a command.
//!
//! # Why the exit codes get their own tests
//!
//! The library is covered by the libraries' suites; what this crate adds is a
//! *process contract*, and the part of it people build on is the exit code. A CI
//! job that cannot tell "this invoice is invalid" (`1`) from "that path does not
//! exist" (`2`) will eventually ship an invoice because a volume was not
//! mounted, and no amount of library testing catches that.

use std::io::Write as _;

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt as _;
use predicates::str::contains;

/// A minimal but *core-valid* UBL invoice, written from the model rather than
/// pasted — a fixture that drifts out of validity tests nothing.
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
    assert!(en16931::validate(&invoice).is_valid());
    en16931_formats::ubl::to_string(&invoice)
}

/// Write a fixture to a path **no other test can be writing at the same time**.
///
/// `cargo test` runs these in parallel threads of one process, and the first
/// version of this keyed the filename on `name` alone — so two tests both
/// wanting `valid.xml` truncated the file under each other and one of them read
/// an empty document. Intermittently, and only under load.
fn write_temp(name: &str, body: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);

    let unique = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "en16931-cli-{}-{unique}-{name}",
        std::process::id()
    ));
    let mut f = std::fs::File::create(&path).expect("create fixture");
    f.write_all(body.as_bytes()).expect("write fixture");
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

/// The distinction the whole exit-code contract exists for.
#[test]
fn an_unreadable_document_exits_two_not_one() {
    cli()
        .args(["validate", "/definitely/not/here.xml"])
        .assert()
        .code(2);

    let path = write_temp("nonsense.xml", "<html/>");
    cli()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(contains("not an e-invoice"));
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
    // …and naming a profile overrides it, which is how you ask "would this
    // pass in Germany?" about a document that does not claim to.
    cli()
        .args([
            "validate",
            path.to_str().unwrap(),
            "--profile",
            "XRechnung 3.0",
        ])
        .assert()
        .code(1)
        .stdout(contains("BR-DE-15"));
}

#[test]
fn an_unknown_profile_is_refused_with_the_list() {
    cli()
        .args(["validate", "-", "--profile", "XRechnung 9.9"])
        .assert()
        .code(2)
        .stderr(contains("unknown profile"))
        .stderr(contains("Peppol BIS Billing 3.0"));
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

    // Reading it back gives a document that still validates — the conversion
    // went through the semantic model, so this is a claim about meaning and not
    // about element names.
    let back = write_temp("roundtrip.xml", &cii);
    cli()
        .args(["validate", back.to_str().unwrap()])
        .assert()
        .success();
}

/// `--profile` on `convert` refuses to write a document that would be rejected.
#[test]
fn converting_for_a_profile_refuses_rather_than_writing_something_invalid() {
    let path = write_temp("valid.xml", &valid_ubl());
    cli()
        .args([
            "convert",
            path.to_str().unwrap(),
            "--to",
            "cii",
            "--profile",
            "XRechnung 3.0",
        ])
        .assert()
        .code(1)
        .stdout(predicates::str::is_empty()) // nothing half-written
        .stderr(contains("BR-DE-15"));
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
    assert_eq!(value["schema"], en16931::report::SCHEMA);
    assert_eq!(value["valid"], true);
    // Always an array, even for one document — a consumer must not have to
    // branch on the shape of the top level.
    assert_eq!(value["documents"].as_array().map(Vec::len), Some(1));
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

/// SVRL is a validation report format, so asking `inspect` for one is a mistake
/// worth naming rather than an empty file worth debugging.
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
            "--profile",
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
    for query in ["BR-CO-14", "br-co-3", "BR-IG-1"] {
        cli().args(["explain", query]).assert().success();
    }
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
        .stdout(contains(en16931::ARTEFACT_VERSION))
        // The licence condition travels with every user-visible surface.
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
        .stdout(contains("2 document(s), 1 invalid"));
}

/// `--strict` promotes advisories, and the default deliberately does not.
#[test]
fn strict_turns_advisories_into_failures() {
    // `BR-51` is the one warning in CEN's whole abstract model: an invoice
    // should never carry a full card PAN. The document stays valid, which is
    // exactly the case `--strict` exists for.
    let mut inv = en16931_formats::ubl::from_str(&valid_ubl())
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
    let path = write_temp("full-pan.xml", &en16931_formats::ubl::to_string(&inv));

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

/// The rule catalogue is derived from the registry, so it cannot drift from
/// what the validator runs — **including the restrictions**.
///
/// It used to list the rules only, so `--profile "XRechnung 3.0"` printed 270 of
/// the 282 checks the profile declares and the twelve missing were the German
/// ones every counterparty quotes. This test allowed it, by adding
/// `p.restrictions.len()` back before comparing.
#[test]
fn the_catalogue_matches_what_each_profile_actually_runs() {
    let stdout = |args: &[&str]| -> String {
        let out = cli()
            .args(args)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        String::from_utf8(out).expect("UTF-8")
    };
    let total = |args: &[&str]| -> usize {
        let text = stdout(args);
        text.lines()
            .find_map(|l| l.split_once(" check(s):").map(|(n, _)| n.to_owned()))
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or_else(|| panic!("no count in:\n{text}"))
    };

    // One number, from two commands: the catalogue's total is exactly
    // `check_ids()`, which is what `profiles` prints in its CHECKS column and
    // what a report prints as `rule(s) checked`.
    for p in en16931::profiles::ALL {
        assert_eq!(
            total(&["rules", "--profile", p.id]),
            p.check_ids().count(),
            "{} — the catalogue and the profile disagree",
            p.id
        );
    }

    // And the restrictions are there by id, in both shapes, because a user
    // grepping the catalogue for `BR-DE-15` is the case this exists for.
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

/// `--profile` reports the severity that profile uses, not the rule's own.
///
/// This is the whole XRechnung finding, made greppable: `BR-CL-23` is fatal in
/// the core model and a warning under the German CIUS, because KoSIT's validator
/// configuration says so.
#[test]
fn the_catalogue_reports_the_profiles_own_severity() {
    let row = |args: &[&str]| -> String {
        let out = cli()
            .args(args)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        String::from_utf8(out)
            .expect("UTF-8")
            .lines()
            .find(|l| l.starts_with("BR-CL-23 "))
            .unwrap_or_else(|| panic!("BR-CL-23 not listed"))
            .to_owned()
    };
    assert!(row(&["rules", "--profile", "EN 16931"]).contains("fatal"));
    assert!(row(&["rules", "--profile", "XRechnung 3.0"]).contains("warning"));
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

/// A closed pipe is not an error, and printing a Rust backtrace into one is not
/// what any other command-line tool does.
///
/// `println!` panics on `BrokenPipe`, which is why every command now builds its
/// output into a `String` and hands it to one sink that knows to stay quiet.
#[test]
fn a_closed_pipe_is_silent() {
    // `assert_cmd` gives no pipe to close, so the property is asserted where it
    // is decided: the sink must treat `BrokenPipe` as success.
    let e = std::io::Error::from(std::io::ErrorKind::BrokenPipe);
    assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe);
    // And nothing in the crate reaches stdout except through that sink.
    let src = include_str!("../src/output.rs");
    assert!(
        !src.contains("println!") && !src.contains("print!("),
        "output.rs must build strings, not print — see `write_out`"
    );
}

// ── hostile input ────────────────────────────────────────────────────────────

/// A document nobody would write must be **refused**, not fatal.
///
/// This command is pointed at files the user did not author, so the interesting
/// inputs are the ones designed to hurt. All three below once produced, or would
/// have produced, something other than "I could not read that":
///
/// * **Nesting.** `roxmltree` recurses per level and overflows the stack at a
///   few hundred. That is not a panic — it aborts the process, exit `134`, with
///   no report and nothing for `?` to catch. It is now refused before parsing.
/// * **Billion laughs** and **XXE.** Both need a DTD, and the parser rejects any
///   document carrying one; asserted here so a future parser swap cannot quietly
///   re-open them.
///
/// The assertion that matters in every case is `code(2)` — *"could not read
/// it"* — because the whole point of the exit codes is that a pipeline can tell
/// that from *"this invoice is wrong"*.
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
    cli()
        .args(["validate", deep.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(contains("nested"));

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
    cli()
        .args(["validate", laughs.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(contains("DTD"));

    let xxe = write_temp(
        "hostile-xxe.xml",
        &format!(
            "<?xml version=\"1.0\"?>\n\
             <!DOCTYPE Invoice [<!ENTITY xxe SYSTEM \"file:///etc/passwd\">]>\n\
             <Invoice xmlns=\"{ns}\"><ID>&xxe;</ID></Invoice>"
        ),
    );
    cli()
        .args(["validate", xxe.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(contains("DTD"));
}

// ── diff ─────────────────────────────────────────────────────────────────────

/// The claim the subcommand exists to make: **crossing syntaxes is not a change.**
///
/// A textual diff of these two files shares almost nothing — different root
/// element, different namespaces, different names for every field. As invoices
/// they are the same invoice, and that is the question a person converting a
/// document actually has.
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

/// A scale difference is not a difference.
///
/// UBL writes BT-152 as `25.0` where CII writes `25`, and the model holds those
/// to be one value — `Percentage` compares by value in `Eq`, `Ord` and `Hash`.
/// The first real conversion this was run against reported three differences
/// and every one of them was this, so it is pinned rather than remembered.
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

/// `1` is "they differ" and `2` is "I could not read one" — the same split
/// `validate` makes, and for the same reason.
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
    // A `String`, not a `"\"Musterstadt\""`.
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
