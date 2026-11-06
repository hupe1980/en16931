#![cfg(all(feature = "ubl", feature = "cii"))]

//! A second occurrence of a single-occurrence element is reported, and the
//! first is read.

mod common;

use en16931_formats::report::Kind;
use en16931_formats::{Policy, Recommendation, Syntax, read, receive};

/// `xml` with a copy of the first `<tag>…</tag>` inserted right after it.
fn twice(xml: &str, tag: &str) -> String {
    twice_after(xml, "", tag)
}

/// As [`twice`], for the first `<tag>` after `anchor`.
fn twice_after(xml: &str, anchor: &str, tag: &str) -> String {
    let from = xml.find(anchor).expect("anchor present");
    let start = xml[from..]
        .find(&format!("<{tag}"))
        .expect("element present")
        + from;
    let close = format!("</{tag}>");
    let end = xml[start..].find(&close).expect("closed") + start + close.len();
    format!("{}{}{}", &xml[..end], &xml[start..end], &xml[end..])
}

fn example8() -> Option<String> {
    let root = common::require("CEN example 8")?;
    Some(
        std::fs::read_to_string(
            root.join("eInvoicing-EN16931/ubl/examples/ubl-tc434-example8.xml"),
        )
        .expect("example 8"),
    )
}

/// A forged second `cbc:ID` and `cbc:IssueDate` are reported, not read.
#[test]
fn the_forged_second_number_is_reported_and_not_read() {
    let Some(xml) = example8() else { return };
    let base = read(xml.as_bytes()).expect("readable");
    let first_id = base.invoice.number.clone().expect("BT-1");
    let probe = xml
        .replacen(
            &format!("<cbc:ID>{first_id}</cbc:ID>"),
            &format!("<cbc:ID>{first_id}</cbc:ID><cbc:ID>FORGED-999</cbc:ID>"),
            1,
        )
        .replacen(
            "</cbc:IssueDate>",
            "</cbc:IssueDate><cbc:IssueDate>2099-01-01</cbc:IssueDate>",
            1,
        );
    let got = read(probe.as_bytes()).expect("readable");
    assert_eq!(got.invoice.number.as_deref(), Some(first_id.as_str()));
    assert_eq!(got.invoice.issue_date, base.invoice.issue_date);
    let dups: Vec<_> = got.entries_of(Kind::Duplicate).collect();
    assert_eq!(dups.len(), 2, "{dups:#?}");
    assert_eq!(dups[0].path, "Invoice/ID");
    assert_eq!(dups[0].value.as_deref(), Some("FORGED-999"));
    assert_eq!(dups[1].path, "Invoice/IssueDate");
    assert_eq!(
        got.invoice, base.invoice,
        "nothing of the duplicates reached the model"
    );

    let received = receive(probe.as_bytes(), &Policy::default());
    assert_eq!(received.recommendation, Recommendation::Reject);
}

/// The CII twin: two `ram:ID` and two `ram:IssueDateTime` in the document.
#[test]
fn the_cii_twin_is_reported_too() {
    let Some(xml) = example8() else { return };
    let base = read(xml.as_bytes()).expect("readable");
    let cii = en16931_formats::write_unchecked(&base.invoice, Syntax::Cii)
        .expect("writable")
        .xml;
    let probe = twice_after(
        &twice(&cii, "ram:IssueDateTime"),
        "<rsm:ExchangedDocument>",
        "ram:ID",
    );
    let got = read(probe.as_bytes()).expect("readable");
    let paths: Vec<&str> = got
        .entries_of(Kind::Duplicate)
        .map(|e| e.path.as_str())
        .collect();
    assert_eq!(
        paths,
        [
            "CrossIndustryInvoice/ExchangedDocument/ID",
            "CrossIndustryInvoice/ExchangedDocument/IssueDateTime",
        ]
    );
    assert_eq!(got.invoice.number, base.invoice.number);
    assert_eq!(got.invoice.issue_date, base.invoice.issue_date);
}

/// A duplicated group is read whole from its first occurrence and reported
/// whole — no field-by-field merge of the two.
#[test]
fn a_second_supplier_party_is_reported_whole() {
    let Some(xml) = example8() else { return };
    let base = read(xml.as_bytes()).expect("readable");
    let second = twice(&xml, "cac:AccountingSupplierParty").replacen(
        "</cac:AccountingSupplierParty><cac:AccountingSupplierParty>",
        "</cac:AccountingSupplierParty><cac:AccountingSupplierParty><!-- a forged seller -->",
        1,
    );
    // Make the second seller different, so a merge would show.
    let pos = second.rfind("<!-- a forged seller -->").expect("marker");
    let (head, tail) = second.split_at(pos);
    let forged = format!(
        "{head}{}",
        tail.replacen("<cbc:RegistrationName>", "<cbc:RegistrationName>FORGED ", 1)
    );
    let got = read(forged.as_bytes()).expect("readable");
    let dups: Vec<_> = got.entries_of(Kind::Duplicate).collect();
    assert_eq!(dups.len(), 1, "one entry for the whole group: {dups:#?}");
    assert_eq!(dups[0].path, "Invoice/AccountingSupplierParty");
    assert_eq!(dups[0].value, None, "a group has no single value");
    assert_eq!(got.invoice.seller, base.invoice.seller);
}

/// A malformed first occurrence stays absent; the second is never a fallback.
#[test]
fn a_malformed_first_occurrence_is_not_replaced_by_the_second() {
    let Some(xml) = example8() else { return };
    let start = xml.find("<cbc:IssueDate>").expect("BT-2");
    let end = xml[start..].find("</cbc:IssueDate>").expect("closed") + start;
    let probe = format!(
        "{}<cbc:IssueDate>123</cbc:IssueDate><cbc:IssueDate>2026-01-01{}",
        &xml[..start],
        &xml[end..]
    );
    let got = read(probe.as_bytes()).expect("readable");
    assert_eq!(
        got.invoice.issue_date, None,
        "the first is unreadable and stays absent"
    );
    assert_eq!(got.entries_of(Kind::Malformed).count(), 1);
    assert_eq!(got.entries_of(Kind::Duplicate).count(), 1);
}

/// Lawful repetitions — lines, notes, payment means merged into BG-16 — are
/// not duplicates.
#[test]
fn lawful_repetitions_are_silent() {
    let Some(root) = common::require("CEN examples") else {
        return;
    };
    let mut checked = 0;
    for name in [
        "ubl-tc434-example1.xml",
        "ubl-tc434-example2.xml",
        "ubl-tc434-example5.xml",
    ] {
        let xml = std::fs::read(root.join("eInvoicing-EN16931/ubl/examples").join(name))
            .expect("example");
        let got = read(&xml).expect("readable");
        assert!(got.invoice.lines.len() > 1, "{name} repeats lines");
        assert_eq!(got.entries_of(Kind::Duplicate).count(), 0, "{name}");
        checked += 1;
    }
    assert_eq!(checked, 3);
}

/// KoSIT's `BR-TMP-4` instance: two `ram:Name` in one CII BG-24. CEN's
/// `CII-SR-475` (corrected in 1.3.17 to count per BG-24) is a warning, so it
/// is `Repeated` — read first-wins and reported, the schema layer valid.
#[test]
fn a_second_name_in_one_supporting_document_is_repeated() {
    let Some(root) = common::require("KoSIT BR-TMP-4 instance") else {
        return;
    };
    let path = root.join("xrechnung-schematron/test/instances/cii/cii-br-tmp-4-test.xml");
    let text = std::fs::read_to_string(&path).expect("instance");
    let doc = roxmltree::Document::parse(&text).expect("well-formed");
    let invoice = doc
        .descendants()
        .find(|n| n.has_tag_name("CrossIndustryInvoice"))
        .expect("an invoice in the test file");
    let xml = &text[invoice.range()];
    let got = read(xml.as_bytes()).expect("readable");
    let repeated: Vec<_> = got.entries_of(Kind::Repeated).collect();
    assert!(
        repeated
            .iter()
            .any(|e| e.rule.as_deref() == Some("CII-SR-475")),
        "{:#?}",
        got.entries
    );
    let received = receive(xml.as_bytes(), &Policy::default());
    assert_ne!(
        received.report_declared.schema.validity,
        en16931_formats::report::Validity::Invalid
    );
}

/// The `udt:DateTimeString` spelling of BT-7 is read, and reported because the
/// schema does not admit it.
#[test]
fn the_legacy_tax_point_date_spelling_is_read_and_reported() {
    let Some(xml) = example8() else { return };
    let mut inv = read(xml.as_bytes()).expect("readable").invoice;
    inv.vat_point_date = en16931::Date::parse("2014-11-10").ok();
    inv.vat_point_date_code = None;
    let cii = en16931_formats::write_unchecked(&inv, Syntax::Cii)
        .expect("writable")
        .xml;
    let at = cii
        .find("<ram:TaxPointDate>")
        .expect("the fixture carries BT-7");
    let end = cii[at..].find("</ram:TaxPointDate>").expect("closed") + at;
    let legacy = format!(
        "{}{}{}",
        &cii[..at],
        cii[at..end].replace("udt:DateString", "udt:DateTimeString"),
        &cii[end..]
    );
    assert_ne!(legacy, cii, "the fixture carries BT-7");
    let got = read(legacy.as_bytes()).expect("readable");
    assert_eq!(got.invoice.vat_point_date, inv.vat_point_date);
    let schema: Vec<_> = got.entries_of(Kind::Schema).collect();
    assert_eq!(schema.len(), 1, "{:#?}", got.entries);
    assert!(
        schema[0].path.ends_with("TaxPointDate/DateTimeString"),
        "{}",
        schema[0]
    );
}

/// SC-005: every schema row of the generated set, duplicated in a published
/// document, is reported once and changes nothing in the model.
///
/// A row whose child the document carries once is doubled in place. A row no
/// published document carries (the table holds every child the schema allows)
/// is probed by writing the child into a parent the corpus has, once and then
/// twice: the table matches local names, so an empty element is enough.
#[test]
fn the_sweep_covers_the_generated_set() {
    for (syntax, rows, expected) in [
        (Syntax::Ubl, en16931_formats::ubl::once::ONCE, (170, 435)),
        (Syntax::Cii, en16931_formats::cii::once::ONCE, (137, 408)),
    ] {
        let docs = common::authority_documents(syntax, "duplicate sweep");
        if docs.is_empty() {
            return;
        }
        let (mut in_place, mut probed) = (0usize, 0usize);
        for (_, _, parent, child) in rows.iter().filter(|r| r.0.is_empty()) {
            let found = docs.iter().find_map(|(_, text)| {
                let doc = roxmltree::Document::parse(text).ok()?;
                let node = doc.descendants().find(|n| {
                    n.tag_name().name() == *child
                        && n.parent_element()
                            .is_some_and(|p| p.tag_name().name() == *parent)
                        && n.parent_element().is_some_and(|p| {
                            p.children()
                                .filter(|c| c.tag_name().name() == *child)
                                .count()
                                == 1
                        })
                })?;
                let r = node.range();
                let base = read(text.as_bytes()).ok()?;
                Some((
                    format!("{}{}{}", &text[..r.end], &text[r.clone()], &text[r.end..]),
                    base,
                ))
            });
            let (doubled, base) = match found {
                Some(pair) => {
                    in_place += 1;
                    pair
                }
                None => {
                    probed += 1;
                    probe(&docs, parent, child)
                        .unwrap_or_else(|| panic!("{syntax}: no document has <{parent}>"))
                }
            };
            let got = read(doubled.as_bytes()).expect("still readable");
            // One more than the document already carries.
            let before: Vec<_> = base.entries_of(Kind::Duplicate).collect();
            let dups: Vec<_> = got
                .entries_of(Kind::Duplicate)
                .filter(|d| !before.contains(d))
                .collect();
            assert_eq!(dups.len(), 1, "{syntax}: {parent}/{child}: {dups:#?}");
            assert!(
                dups[0].path.ends_with(&format!("{parent}/{child}")),
                "{}",
                dups[0]
            );
            assert_eq!(got.invoice, base.invoice, "{syntax}: {parent}/{child}");
        }
        println!("{syntax}: {in_place} doubled in place, {probed} probed");
        assert_eq!((in_place, probed), expected, "{syntax}");
    }
}

/// `child` written into the first non-empty `<parent>` of a published
/// document twice, with the same document carrying it once as the baseline.
fn probe(
    docs: &[(std::path::PathBuf, String)],
    parent: &str,
    child: &str,
) -> Option<(String, en16931_formats::Read)> {
    docs.iter().find_map(|(_, text)| {
        let doc = roxmltree::Document::parse(text).ok()?;
        let node = doc
            .descendants()
            .find(|n| n.tag_name().name() == parent && n.has_children())?;
        let r = node.range();
        let close = r.start + text[r.clone()].rfind("</")?;
        let one = format!("<p:{child} xmlns:p=\"urn:probe\"/>");
        let with = |n: usize| format!("{}{}{}", &text[..close], one.repeat(n), &text[close..]);
        let base = read(with(1).as_bytes()).ok()?;
        Some((with(2), base))
    })
}

/// CII repeats BT-8 per breakdown group and CEN's `CII-SR-462` requires the
/// repetitions to agree. KoSIT's test instance carries `5` and `35`: the first
/// is read, the second reported.
#[test]
fn a_second_different_vat_point_date_code_is_reported() {
    let Some(root) = common::require("CII-SR-462 instance") else {
        return;
    };
    let path = root.join(
        "validator-configuration-xrechnung/src/test/cen-unit-test/cii-cii-sr-462-bt-8-cardinality-test.xml",
    );
    let got = read(&std::fs::read(path).expect("the instance")).expect("readable");
    assert_eq!(
        got.invoice.vat_point_date_code.as_ref().map(|c| c.as_str()),
        Some("3"),
        "CII 5 is the model's 3"
    );
    let dup: Vec<_> = got.entries_of(Kind::Duplicate).collect();
    assert_eq!(dup.len(), 1, "{dup:?}");
    assert_eq!(dup[0].rule.as_deref(), Some("CII-SR-462"));
    assert_eq!(dup[0].value.as_deref(), Some("35"));
    assert_eq!(dup[0].term.as_deref(), Some("BT-8"));
}

/// The CII single-occurrence rules KoSIT raises to *error* under XRechnung are
/// exactly [`en16931_formats::cii::KOSIT_RAISED`]: every `customLevel
/// level="error"` of the configuration's `scenarios.xml` that names a
/// warning-level row of the duplicate table, and nothing else.
#[test]
fn kosits_raised_duplicate_rules_are_the_configurations() {
    let Some(root) = common::require("KoSIT scenarios") else {
        return;
    };
    let scenarios =
        std::fs::read_to_string(root.join("validator-configuration-xrechnung/scenarios.xml"))
            .expect("scenarios.xml");
    let warning_rows: std::collections::BTreeSet<&str> = en16931_formats::cii::once::ONCE
        .iter()
        .filter(|(rule, level, _, _)| !rule.is_empty() && *level == "warning")
        .map(|(rule, ..)| *rule)
        .collect();
    let mut raised = std::collections::BTreeSet::new();
    let mut rest = scenarios.as_str();
    let open = r#"<customLevel level="error">"#;
    while let Some(i) = rest.find(open) {
        rest = &rest[i + open.len()..];
        let end = rest.find("</customLevel>").expect("closed");
        let id = rest[..end].trim();
        if warning_rows.contains(id) {
            raised.insert(id.to_owned());
        }
    }
    let ours: std::collections::BTreeSet<String> = en16931_formats::cii::KOSIT_RAISED
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    assert_eq!(raised, ours);
}

/// A second UBL `cac:TaxTotal` in the invoice currency: BT-110 is read from the
/// first, and the second amount is an `Unmapped` entry rather than ignored.
#[test]
fn ubl_reports_a_second_tax_total_in_the_same_currency() {
    let inv = common::maximal();
    let ccy = inv.currency.as_ref().expect("BT-5").as_str().to_owned();
    let xml = en16931_formats::write_unchecked(&inv, Syntax::Ubl)
        .expect("writable")
        .xml;
    let at = xml
        .find("</cac:TaxTotal>")
        .expect("the fixture carries BT-110")
        + "</cac:TaxTotal>".len();
    let probe = format!(
        "{}<cac:TaxTotal><cbc:TaxAmount currencyID=\"{ccy}\">99.99</cbc:TaxAmount></cac:TaxTotal>{}",
        &xml[..at],
        &xml[at..]
    );
    let base = read(xml.as_bytes()).expect("readable");
    let got = read(probe.as_bytes()).expect("readable");
    assert_eq!(got.invoice.totals.vat_total, base.invoice.totals.vat_total);
    let further: Vec<_> = got
        .entries
        .iter()
        .filter(|e| !base.entries.contains(e))
        .collect();
    assert_eq!(further.len(), 1, "{further:#?}");
    assert_eq!(further[0].kind, Kind::Unmapped);
    assert_eq!(further[0].path, "Invoice/TaxTotal/TaxAmount");
    assert_eq!(further[0].term.as_deref(), Some("BT-110"));
    assert_eq!(further[0].value.as_deref(), Some("99.99"));
}
