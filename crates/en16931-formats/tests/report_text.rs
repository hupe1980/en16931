#![cfg(all(feature = "ubl", feature = "cii"))]

//! The text rendering, pinned: every entry kind, a finding with and without
//! its detail and hint, every layer state, and a report header. The CLI prints
//! these unchanged.

use en16931::Severity;
use en16931_formats::report::{Entry, Kind, Layer, Validity};
use en16931_formats::{Policy, Syntax, receive};

#[test]
fn every_kind_renders_one_way() {
    let cases = [
        (Kind::Unmapped, "unmapped     Invoice/Foo — not mapped"),
        (Kind::Malformed, "malformed    Invoice/Foo — not mapped"),
        (Kind::Missing, "missing      Invoice/Foo — not mapped"),
        (Kind::Duplicate, "duplicate    Invoice/Foo — not mapped"),
        (Kind::Repeated, "repeated     Invoice/Foo — not mapped"),
        (Kind::Schema, "schema       Invoice/Foo — not mapped"),
        (Kind::Unreadable, "unreadable   Invoice/Foo — not mapped"),
        (Kind::Container, "container    Invoice/Foo — not mapped"),
        (Kind::Ambiguous, "ambiguous    Invoice/Foo — not mapped"),
        (Kind::Dropped, "dropped      Invoice/Foo — not mapped"),
        (Kind::Prohibited, "prohibited   Invoice/Foo — not mapped"),
        (Kind::Derived, "derived      Invoice/Foo — not mapped"),
    ];
    for (kind, text) in cases {
        assert_eq!(
            Entry::new(kind, "Invoice/Foo", "not mapped").to_string(),
            text
        );
    }
    let levels: Vec<Severity> = cases.iter().map(|(k, _)| k.level()).collect();
    use Severity::{Fatal, Info, Warning};
    assert_eq!(
        levels,
        [
            Info, Fatal, Fatal, Fatal, Warning, Fatal, Fatal, Warning, Fatal, Warning, Warning,
            Info
        ]
    );
}

/// A `yes` indicator: path, value, business term absent (the binding cannot
/// tell allowance from charge), and the hint.
#[test]
fn every_part_of_an_entry_renders_in_order() {
    let e = Entry::new(
        Kind::Malformed,
        "ApplicableTradeTax/DueDateTypeCode",
        "not a CII code",
    )
    .term("BT-8")
    .value("3")
    .rule("BR-CL-06")
    .hint("spell it 5");
    assert_eq!(
        e.to_string(),
        "malformed    ApplicableTradeTax/DueDateTypeCode (BT-8) = \"3\" — not a CII code \
         [BR-CL-06] [hint: spell it 5]"
    );
}

/// Through the reader: the CII `ChargeIndicator` of `yes`, and the core
/// logistics charge with both remedies.
#[test]
fn the_readers_hints_are_the_ones_the_spec_names() {
    let mut inv = en16931::Invoice::default();
    inv.number = Some("1".into());
    inv.issue_date = en16931::Date::parse("2026-10-10").ok();
    inv.specification_id = Some(en16931::profiles::EN16931.specification_id.to_owned());
    inv.charges.push(en16931::invoice::DocumentAllowanceCharge {
        amount: en16931::InvoiceAmount::parse("1.00").expect("amount"),
        base_amount: None,
        percentage: None,
        vat: en16931::invoice::LineVat::default(),
        reason: Some("fee".into()),
        reason_code: None,
    });
    let cii = en16931_formats::write_unchecked(&inv, Syntax::Cii)
        .expect("writable")
        .xml;
    let yes = cii.replacen(
        "<udt:Indicator>true</udt:Indicator>",
        "<udt:Indicator>yes</udt:Indicator>",
        1,
    );
    assert_ne!(yes, cii);
    let logistics = yes.replacen(
        "</ram:ApplicableHeaderTradeSettlement>",
        "<ram:SpecifiedLogisticsServiceCharge><ram:Description>Freight</ram:Description>\
         <ram:AppliedAmount>5.00</ram:AppliedAmount></ram:SpecifiedLogisticsServiceCharge>\
         </ram:ApplicableHeaderTradeSettlement>",
        1,
    );
    let got = receive(logistics.as_bytes(), &Policy::default());
    let syntax = &got.report_declared.syntax;
    assert_eq!(syntax.validity, Validity::Invalid);
    let text: Vec<String> = syntax.entries.iter().map(ToString::to_string).collect();
    assert_eq!(
        text,
        [
            "malformed    SpecifiedTradeAllowanceCharge/ChargeIndicator/Indicator = \"yes\" — \
             not an xs:boolean [hint: the value must be `true` or `false`]",
            // The fixture's charge has no VAT category, and read as an
            // allowance it lacks BT-95.
            "missing      CrossIndustryInvoice/SupplyChainTradeTransaction/\
             ApplicableHeaderTradeSettlement/SpecifiedTradeAllowanceCharge/CategoryTradeTax \
             (BG-20[1]/BT-95) — the VAT category code is absent; BR-32 requires it, and the \
             model holds a placeholder",
            "unmapped     ApplicableHeaderTradeSettlement/SpecifiedLogisticsServiceCharge — a \
             logistics service charge, which the core does not count [CII-SR-396] [hint: declare \
             the extension in BT-24 (`#conformant#`), or send the charge as a document-level \
             charge (BG-21)]",
        ]
    );
}

#[test]
fn every_layer_state_renders_one_way() {
    assert_eq!(Validity::Valid.to_string(), "valid");
    assert_eq!(Validity::Invalid.to_string(), "INVALID");
    assert_eq!(
        Validity::Skipped("nothing was read".into()).to_string(),
        "skipped (nothing was read)"
    );
    assert_eq!(Layer::of(Vec::new()).validity, Validity::Valid);
    assert_eq!(
        Layer::of(vec![Entry::new(Kind::Unmapped, "a/b", "x")]).validity,
        Validity::Valid,
        "an unmapped element never invalidates"
    );
    assert_eq!(
        Layer::of(vec![Entry::new(Kind::Duplicate, "a/b", "x")]).validity,
        Validity::Invalid
    );
}

/// A finding renders as the model crate renders it, with and without detail
/// and hint.
#[test]
fn a_finding_renders_one_way() {
    use en16931::validation::{Detail, Finding};
    let f = Finding {
        rule: "BR-CO-15".into(),
        severity: Severity::Fatal,
        path: en16931::Path::group_term(
            en16931::Group::Totals,
            en16931::invoice::terms::GROSS_TOTAL,
        ),
        message: "Invoice total amount with VAT = …".into(),
        detail: Some(Detail {
            expected: "119.00".into(),
            actual: "118.00".into(),
        }),
        hint: Some("check BT-110".into()),
    };
    assert_eq!(
        f.to_string(),
        "fatal        [BR-CO-15] BG-22/BT-112 — Invoice total amount with VAT = … \
         (expected 119.00, found 118.00) [hint: check BT-110]"
    );
    let bare = Finding {
        detail: None,
        hint: None,
        ..f
    };
    assert_eq!(
        bare.to_string(),
        "fatal        [BR-CO-15] BG-22/BT-112 — Invoice total amount with VAT = …"
    );
}

/// The header names the document, the rule set and the most severe level.
#[test]
fn the_report_header_renders_one_way() {
    let got = receive(b"<html/>", &Policy::default());
    assert_eq!(
        got.report_declared.to_string(),
        "document: (absent) — invoice (absent) of (absent)\n\
         rule set: none ran\n\
         most severe: fatal\n\
         container  valid\n\
         syntax     INVALID\n  \
         unreadable   XML — expected a UBL <Invoice> or <CreditNote>, a CII \
         <CrossIndustryInvoice>, or a PDF; found <html>\n\
         schema     skipped (nothing was read)\n\
         semantic   skipped (nothing was read)"
    );
}

#[cfg(feature = "serde")]
#[test]
fn entries_and_reports_survive_json() {
    let e = Entry::new(Kind::Repeated, "a/b", "x")
        .term("BT-1")
        .value("v")
        .rule("R")
        .hint("h");
    let json = serde_json::to_string(&e).expect("serialise");
    assert_eq!(serde_json::from_str::<Entry>(&json).expect("back"), e);
    let report = receive(b"<html/>", &Policy::default()).report_declared;
    let json = serde_json::to_string(&report).expect("serialise");
    let back: en16931_formats::report::Report = serde_json::from_str(&json).expect("back");
    assert_eq!(back, report);
    let declared = en16931::profiles::Declaration::Known(&en16931::profiles::ZUGFERD_BASIC_WL);
    let json = serde_json::to_string(&declared).expect("serialise");
    assert_eq!(json, r#"{"known":"zugferd-basic-wl"}"#);
    assert_eq!(
        serde_json::from_str::<en16931::profiles::Declaration>(&json).expect("back"),
        declared
    );
}

/// The rendering of a received document: recommendation and why, what was
/// declared, then the report — pinned for a document that cannot be read.
#[test]
fn a_received_document_renders_its_decision_first() {
    let got = receive(b"not xml", &Policy::default());
    assert_eq!(
        got.to_string(),
        "recommendation: REJECT\n\
         \x20 because unreadable: not well-formed XML at line 1, column 1: unknown token\n\
         declared: nothing was read\n\
         document: (absent) — invoice (absent) of (absent)\n\
         rule set: none ran\n\
         most severe: fatal\n\
         container  valid\n\
         syntax     INVALID\n\
         \x20 unreadable   XML — not well-formed XML at line 1, column 1: unknown token\n\
         schema     skipped (nothing was read)\n\
         semantic   skipped (nothing was read)"
    );
}
