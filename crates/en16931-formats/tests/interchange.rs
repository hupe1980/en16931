#![cfg(feature = "ubl")]

//! The interchange shape and SVRL of every layer (`en16931-report/4`): a
//! received document's report as [`en16931::report::Report`], with the four
//! layers, identity, declaration and recommendation; SVRL carries the same layers.

use en16931::report::SCHEMA;
use en16931_formats::{Policy, Recommendation, receive};

/// Example 8's head with a second invoice number.
const PROBE: &str = r#"<Invoice xmlns="urn:oasis:names:specification:ubl:schema:xsd:Invoice-2"
  xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2"
  xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2">
  <cbc:UBLVersionID>2.1</cbc:UBLVersionID>
  <cbc:CustomizationID>urn:cen.eu:en16931:2017</cbc:CustomizationID>
  <cbc:ID>INV-1</cbc:ID>
  <cbc:ID>FORGED-999</cbc:ID>
  <cbc:IssueDate>2026-10-10</cbc:IssueDate>
</Invoice>"#;

#[test]
fn a_received_document_carries_its_layers() {
    let got = receive(PROBE.as_bytes(), &Policy::default());
    assert_eq!(got.recommendation, Recommendation::Reject);
    let out = got.to_interchange();

    assert_eq!(out.schema, SCHEMA);
    assert_eq!(SCHEMA, "en16931-report/4");
    assert!(!out.valid);
    assert_eq!(out.recommendation.as_deref(), Some("reject"));
    assert_eq!(out.reasons, got.reasons);
    assert_eq!(out.declaration.as_deref(), Some("EN 16931"));
    assert_eq!(out.profile.as_deref(), Some("EN 16931"));
    let id = out.identity.as_ref().expect("identity");
    assert_eq!(id.number.as_deref(), Some("INV-1"));
    assert_eq!(id.issue_date.as_deref(), Some("2026-10-10"));

    let names: Vec<&str> = out.layers.iter().map(|l| &*l.name).collect();
    assert_eq!(names, ["container", "syntax", "schema", "semantic"]);
    let schema = &out.layers[2];
    assert_eq!(schema.validity, "invalid");
    let dup = schema
        .entries
        .iter()
        .find(|e| e.kind == "duplicate")
        .expect("the duplicate");
    assert_eq!(dup.level, "fatal");
    assert_eq!(dup.value.as_deref(), Some("FORGED-999"));
    // The semantic layer's verdicts are the findings, not entries.
    assert_eq!(out.layers[3].validity, "invalid");
    assert!(out.layers[3].entries.is_empty());
    assert!(!out.findings.is_empty());
    assert!(out.required.is_none());
}

#[test]
fn the_report_as_required_is_nested() {
    let policy = Policy {
        require: Some(&en16931::profiles::XRECHNUNG),
        ..Policy::default()
    };
    let out = receive(PROBE.as_bytes(), &policy).to_interchange();
    let required = out.required.as_deref().expect("required differs");
    assert_eq!(required.profile.as_deref(), Some("XRechnung 3.0"));
    assert_eq!(required.layers.len(), 4);
    assert!(
        required.recommendation.is_none(),
        "the decision is the outer one"
    );
}

#[test]
fn an_unreadable_document_has_no_rules_and_says_which_layer_refused() {
    let out = receive(b"not xml", &Policy::default()).to_interchange();
    assert!(!out.valid);
    assert_eq!(out.rules_checked, 0);
    assert!(out.profile.is_none());
    assert!(out.declaration.is_none());
    let syntax = &out.layers[1];
    assert_eq!(syntax.validity, "invalid");
    assert_eq!(syntax.entries[0].kind, "unreadable");
    assert_eq!(out.layers[2].validity, "skipped");
    assert_eq!(out.layers[2].reason.as_deref(), Some("nothing was read"));
}

#[test]
fn reading_alone_runs_no_rules() {
    let read = en16931_formats::read(PROBE.as_bytes()).expect("readable");
    let out = read.report().to_interchange();
    assert_eq!(out.rules_checked, 0);
    assert!(out.findings.is_empty());
    assert_eq!(out.layers[3].validity, "skipped");
    assert_eq!(out.layers[3].reason.as_deref(), Some("not validated"));
    assert_eq!(out.layers[2].validity, "invalid");
}

#[test]
fn svrl_renders_every_layer() {
    let out = receive(PROBE.as_bytes(), &Policy::default()).to_interchange();
    let svrl = en16931::svrl::to_svrl(&out);
    let doc = roxmltree::Document::parse(&svrl).expect("well-formed");
    let ns = en16931::svrl::SVRL_NS;
    let asserts: Vec<_> = doc
        .root_element()
        .children()
        .filter(|n| n.has_tag_name((ns, "failed-assert")))
        .collect();
    // The layers' entries first, in layer order: the probe has no BG-22, so
    // the syntax layer says each required total is missing; then the schema
    // layer's duplicate BT-1.
    let layer_ids: Vec<&str> = asserts
        .iter()
        .filter_map(|n| n.attribute("id"))
        .filter(|id| id.starts_with("en16931:"))
        .collect();
    assert_eq!(
        layer_ids,
        [
            "en16931:syntax:missing",
            "en16931:syntax:missing",
            "en16931:syntax:missing",
            "en16931:syntax:missing",
            "en16931:schema:duplicate",
        ],
        "{svrl}"
    );
    assert!(
        asserts
            .iter()
            .take(5)
            .all(|n| n.attribute("flag") == Some("fatal"))
    );
    // Every finding follows, in the report's order.
    let ids: Vec<&str> = asserts
        .iter()
        .filter_map(|n| n.attribute("id"))
        .filter(|id| !id.starts_with("en16931:"))
        .collect();
    let rules: Vec<&str> = out.findings.iter().map(|f| f.rule.as_str()).collect();
    assert_eq!(ids, rules);
    assert!(svrl.contains("recommendation: reject"));
}

#[cfg(feature = "serde")]
#[test]
fn the_interchange_survives_a_round_trip() {
    let policy = Policy {
        require: Some(&en16931::profiles::XRECHNUNG),
        ..Policy::default()
    };
    let out = receive(PROBE.as_bytes(), &policy).to_interchange();
    let json = serde_json::to_string(&out).expect("serialise");
    assert!(json.contains("\"layers\""), "{json}");
    let back: en16931::report::Report = serde_json::from_str(&json).expect("deserialise");
    assert_eq!(back, out);
}
