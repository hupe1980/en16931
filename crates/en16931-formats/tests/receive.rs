#![cfg(all(feature = "ubl", feature = "cii"))]

//! `receive`: container × BT-24 declaration × required profile, with the
//! expected recommendation per cell, plus non-EN 16931 documents, ambiguous and
//! oversized PDFs, and re-levelled rules.

mod common;

use en16931::profiles::{self, Declaration};
use en16931::{Invoice, Severity};
use en16931_formats::report::{Kind, Validity};
use en16931_formats::{Policy, Received, Recommendation, Syntax, receive};

const XOEV: &str = "urn:cen.eu:en16931:2017#compliant#urn:xoev-de:kosit:standard:xrechnung_3.0";
const SELF_BILLING: &str =
    "urn:cen.eu:en16931:2017#compliant#urn:fdc:peppol.eu:2017:poacc:selfbilling:3.0";

/// A valid XRechnung: the maximal invoice without BT-8 (`BR-CO-03`) and with
/// one BT-46 (`UBL-SR-16`).
fn xrechnung() -> Invoice {
    let mut inv = common::maximal();
    inv.vat_point_date_code = None;
    inv.buyer.identifiers.truncate(1);
    inv.specification_id = Some(profiles::XRECHNUNG.specification_id.to_owned());
    assert!(profiles::XRECHNUNG.validate(&inv).is_valid());
    inv
}

fn declaring(bt24: &str) -> Invoice {
    let mut inv = xrechnung();
    inv.specification_id = Some(bt24.to_owned());
    inv
}

fn xml(inv: &Invoice, syntax: Syntax) -> Vec<u8> {
    en16931_formats::write_unchecked(inv, syntax)
        .expect("writable")
        .xml
        .into_bytes()
}

/// A hybrid PDF around `payload`, its XMP declaring `level`.
#[cfg(feature = "zugferd")]
fn pdf(payload: &[u8], level: &str) -> Vec<u8> {
    use lopdf::{Object, Stream, dictionary};
    let packet = format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
 <rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/" pdfaid:part="3" pdfaid:conformance="B"/>
 <rdf:Description xmlns:fx="urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#">
  <fx:DocumentType>INVOICE</fx:DocumentType><fx:DocumentFileName>factur-x.xml</fx:DocumentFileName>
  <fx:Version>1.0</fx:Version><fx:ConformanceLevel>{level}</fx:ConformanceLevel>
 </rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#
    );
    let mut doc = lopdf::Document::with_version("1.7");
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages });
    doc.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! { "Type" => "Pages", "Count" => 1, "Kids" => vec![page.into()] },
        ),
    );
    let stream = doc.add_object(Stream::new(
        dictionary! { "Type" => "EmbeddedFile", "Subtype" => "text/xml" },
        payload.to_vec(),
    ));
    let spec = doc.add_object(dictionary! {
        "Type" => "Filespec",
        "F" => Object::string_literal("factur-x.xml"),
        "UF" => Object::string_literal("factur-x.xml"),
        "AFRelationship" => Object::Name(b"Alternative".to_vec()),
        "EF" => dictionary! { "F" => stream, "UF" => stream },
    });
    let names = doc.add_object(dictionary! {
        "Names" => vec![Object::string_literal("factur-x.xml"), spec.into()],
    });
    let meta = doc.add_object(Stream::new(
        dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
        packet.into_bytes(),
    ));
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages,
        "Names" => dictionary! { "EmbeddedFiles" => names },
        "AF" => vec![Object::Reference(spec)],
        "Metadata" => meta,
    });
    doc.trailer.set("Root", catalog);
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("write the PDF");
    out
}

/// Every container a document can arrive in, each with the same model.
fn containers(inv: &Invoice) -> Vec<(&'static str, Vec<u8>)> {
    #[allow(unused_mut)]
    let mut out = vec![
        ("UBL", xml(inv, Syntax::Ubl)),
        ("CII", xml(inv, Syntax::Cii)),
    ];
    #[cfg(feature = "zugferd")]
    out.push(("PDF", pdf(&xml(inv, Syntax::Cii), "XRECHNUNG")));
    out
}

fn requirements() -> [(&'static str, Option<&'static en16931::Profile>); 3] {
    [
        ("none", None),
        ("same", Some(&profiles::XRECHNUNG)),
        ("different", Some(&profiles::PEPPOL_BIS_3)),
    ]
}

fn policy(require: Option<&'static en16931::Profile>) -> Policy {
    Policy {
        require,
        ..Policy::default()
    }
}

/// Each cell asserts the classification, which reports are present, and the
/// recommendation.
#[test]
fn the_receive_matrix() {
    let mut cells = 0;
    for (declared, bt24) in [
        ("known", profiles::XRECHNUNG.specification_id),
        ("unknown", XOEV),
        ("wrong process", SELF_BILLING),
    ] {
        for (container, bytes) in containers(&declaring(bt24)) {
            for (required, profile) in requirements() {
                let got = receive(&bytes, &policy(profile));
                let cell = format!("{container} × {declared} × {required}");
                match declared {
                    "known" => assert_eq!(
                        got.declared,
                        Some(Declaration::Known(&profiles::XRECHNUNG)),
                        "{cell}"
                    ),
                    "unknown" => assert_eq!(
                        got.declared,
                        Some(Declaration::Unknown(Some(XOEV.to_owned()))),
                        "{cell}"
                    ),
                    _ => assert_eq!(
                        got.declared,
                        Some(Declaration::WrongProcess(SELF_BILLING.to_owned())),
                        "{cell}"
                    ),
                }
                let expected = match (declared, required) {
                    ("known", "none" | "same") => Recommendation::Accept,
                    _ => Recommendation::Reject,
                };
                assert_eq!(got.recommendation, expected, "{cell}: {:?}", got.reasons);
                assert_eq!(
                    got.coincides(),
                    declared == "known" && required == "same",
                    "{cell}"
                );
                assert_eq!(
                    got.report_required.is_some(),
                    required == "different" || (declared != "known" && required == "same"),
                    "{cell}"
                );
                // The declared profile, or the core when BT-24 names none.
                let ran = got
                    .report_declared
                    .rule_set
                    .as_ref()
                    .map(|r| r.profile.as_str());
                let want = if declared == "known" {
                    "XRechnung 3.0"
                } else {
                    "EN 16931"
                };
                assert_eq!(ran, Some(want), "{cell}");
                assert_eq!(
                    got.reasons.is_empty(),
                    expected == Recommendation::Accept,
                    "{cell}"
                );
                cells += 1;
            }
        }
    }
    let containers = if cfg!(feature = "zugferd") { 3 } else { 2 };
    assert_eq!(cells, containers * 3 * 3);
}

/// Declared and required coincide: one report.
#[test]
fn a_document_declaring_what_is_required_is_accepted_with_one_report() {
    let got = receive(
        &xml(&xrechnung(), Syntax::Ubl),
        &policy(Some(&profiles::XRECHNUNG)),
    );
    assert_eq!(
        got.recommendation,
        Recommendation::Accept,
        "{:?}",
        got.reasons
    );
    assert!(got.coincides());
    assert!(got.report_required.is_none());
    assert!(got.report_declared.is_valid());
}

/// A valid Peppol invoice where XRechnung is required: both reports, rejected
/// for the required one's fatal findings.
#[test]
fn a_valid_peppol_invoice_is_rejected_where_xrechnung_is_required() {
    let Some(root) = common::require("Peppol examples") else {
        return;
    };
    let base = root.join("peppol-bis-invoice-3/rules/examples/base-example.xml");
    let ubl = std::fs::read(&base).expect("the Peppol base example");
    let read = en16931_formats::read(&ubl).expect("readable");
    assert!(profiles::PEPPOL_BIS_3.validate(&read.invoice).is_valid());
    let cii = xml(&read.invoice, Syntax::Cii);

    let got = receive(&cii, &policy(Some(&profiles::XRECHNUNG)));
    assert_eq!(
        got.declared,
        Some(Declaration::Known(&profiles::PEPPOL_BIS_3))
    );
    assert!(
        got.report_declared
            .semantic
            .report
            .as_ref()
            .is_some_and(en16931::ValidationReport::is_valid)
    );
    let required = got.report_required.as_ref().expect("a second report");
    assert_eq!(
        required.rule_set.as_ref().map(|r| r.profile.as_str()),
        Some("XRechnung 3.0")
    );
    assert_eq!(got.recommendation, Recommendation::Reject);
    assert!(
        got.reasons
            .iter()
            .any(|r| r.starts_with("under the required XRechnung 3.0")),
        "{:?}",
        got.reasons
    );
}

/// The obsolete XÖV identifier is not XRechnung.
#[test]
fn the_obsolete_xoev_identifier_is_unknown_and_rejected() {
    let got = receive(&xml(&declaring(XOEV), Syntax::Ubl), &Policy::default());
    assert_eq!(
        got.declared,
        Some(Declaration::Unknown(Some(XOEV.to_owned())))
    );
    assert_eq!(got.recommendation, Recommendation::Reject);
    assert!(
        got.reasons[0].contains("names no profile"),
        "{:?}",
        got.reasons
    );
}

/// ZUGFeRD BASIC and EXTENDED, in every published identifier spelling, are
/// known, checked against the core rules, and accepted.
#[test]
fn zugferd_basic_and_extended_are_received() {
    for (id, p) in [
        (profiles::ZUGFERD_BASIC_ID, &profiles::ZUGFERD_BASIC),
        (
            "urn:cen.eu:en16931:2017#compliant#urn:zugferd.de:2p0:basic",
            &profiles::ZUGFERD_BASIC,
        ),
        (profiles::ZUGFERD_EXTENDED_ID, &profiles::ZUGFERD_EXTENDED),
        (
            "urn:cen.eu:en16931:2017#conformant#urn:zugferd.de:2p0:extended",
            &profiles::ZUGFERD_EXTENDED,
        ),
    ] {
        let got = receive(&xml(&declaring(id), Syntax::Cii), &Policy::default());
        assert_eq!(got.declared, Some(Declaration::Known(p)), "{id}");
        assert_eq!(
            got.recommendation,
            Recommendation::Accept,
            "{id}: {:?}",
            got.reasons
        );
        let rule_set = got.report_declared.rule_set.as_ref().expect("ran");
        assert_eq!(rule_set.profile, p.id);
        assert!(rule_set.left_out.is_empty());
    }
}

/// MINIMUM and BASIC WL are not EN 16931 invoices: their totals rules run, the
/// rest are listed as left out, and the answer is Undefined.
#[test]
fn minimum_and_basic_wl_are_undefined() {
    for p in [&profiles::ZUGFERD_MINIMUM, &profiles::ZUGFERD_BASIC_WL] {
        let mut inv = declaring(p.specification_id);
        inv.lines.clear();
        let got = receive(&xml(&inv, Syntax::Cii), &Policy::default());
        assert_eq!(got.declared, Some(Declaration::Known(p)), "{}", p.id);
        assert_eq!(
            got.recommendation,
            Recommendation::Undefined,
            "{}: {:?}",
            p.id,
            got.reasons
        );
        let rule_set = got.report_declared.rule_set.as_ref().expect("ran");
        assert_eq!(rule_set.left_out, p.left_out(), "{}", p.id);
        assert!(rule_set.left_out.iter().any(|r| r == "BR-16"), "{}", p.id);
        assert!(
            got.reasons[0].contains("not an EN 16931 invoice"),
            "{:?}",
            got.reasons
        );
    }
}

/// The ZUGFeRD corpus's MINIMUM and BASIC WL samples have no fatal finding and
/// are Undefined.
#[cfg(feature = "zugferd")]
#[test]
fn the_corpus_minimum_and_basic_wl_samples_are_undefined() {
    use en16931_formats::zugferd::{self, Profile};
    let Some(root) = common::require("ZUGFeRD corpus") else {
        return;
    };
    let mut pdfs = Vec::new();
    collect(&root.join("zugferd-corpus"), &mut pdfs);
    pdfs.sort();
    let mut seen = Vec::new();
    for path in pdfs {
        let bytes = std::fs::read(&path).expect("read");
        let Ok(got) = zugferd::extract(&bytes, &zugferd::Limits::default()) else {
            continue;
        };
        if !matches!(got.profile, Profile::Minimum | Profile::BasicWl) {
            continue;
        }
        let r = receive(&bytes, &Policy::default());
        let fatal: Vec<String> = r
            .report_declared
            .semantic
            .report
            .iter()
            .flat_map(|s| s.fatal().map(|f| f.rule.clone()))
            .collect();
        assert!(fatal.is_empty(), "{}: {fatal:?}", path.display());
        assert_eq!(
            r.recommendation,
            Recommendation::Undefined,
            "{}: {:?}",
            path.display(),
            r.reasons
        );
        seen.push(path);
    }
    // 11 under `correct/`, 3 credit notes under `fail/` (type code, not totals).
    assert_eq!(
        seen.len(),
        14,
        "the corpus's MINIMUM / BASIC WL samples: {seen:#?}"
    );
}

/// The whole ZUGFeRD corpus: the recommendation distribution is pinned, and
/// every `correct/` document declaring a ZUGFeRD / Factur-X BASIC, EN 16931 or
/// EXTENDED identifier is classified as known.
#[cfg(feature = "zugferd")]
#[test]
fn the_zugferd_corpus_is_received() {
    let Some(root) = common::require("ZUGFeRD corpus") else {
        return;
    };
    let root = root.join("zugferd-corpus");
    let mut pdfs = Vec::new();
    collect(&root, &mut pdfs);
    pdfs.sort();
    assert_eq!(pdfs.len(), 151);
    let (mut accept, mut reject, mut undefined) = (0, 0, 0);
    for path in &pdfs {
        let r = receive(&std::fs::read(path).expect("read"), &Policy::default());
        match r.recommendation {
            Recommendation::Accept => accept += 1,
            Recommendation::Reject => reject += 1,
            Recommendation::Undefined => undefined += 1,
        }
        let rel = path.strip_prefix(&root).expect("under root");
        let lawful = rel.components().any(|c| c.as_os_str() == "correct");
        let bt24 = r
            .invoice
            .as_ref()
            .and_then(|i| i.specification_id.as_deref())
            .unwrap_or_default();
        let zugferd = bt24 == profiles::EN16931.specification_id
            || bt24.contains("factur-x.eu:1p0")
            || bt24.contains("zugferd.de:2p0");
        if lawful && zugferd {
            assert!(
                matches!(r.declared, Some(Declaration::Known(_))),
                "{}: {bt24}",
                rel.display()
            );
        }
    }
    assert_eq!((accept, reject, undefined), (81, 56, 14));
    common::assert_page_states(
        "zugferd.md",
        &[
            ("Over the <N> PDFs of the ZUGFeRD", pdfs.len()),
            ("recommends <N> accept", accept),
            ("accept, <N> reject", reject),
            ("reject and <N> undefined", undefined),
        ],
    );
}

#[cfg(feature = "zugferd")]
fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pdf")) {
            out.push(p);
        }
    }
}

/// XMP says MINIMUM, BT-24 says XRechnung: ambiguous, rejected.
#[cfg(feature = "zugferd")]
#[test]
fn a_pdf_declaring_two_profiles_is_ambiguous_and_rejected() {
    let bytes = pdf(&xml(&xrechnung(), Syntax::Cii), "MINIMUM");
    let got = receive(&bytes, &Policy::default());
    assert_eq!(got.report_declared.container.validity, Validity::Invalid);
    assert!(
        got.report_declared
            .container
            .entries
            .iter()
            .any(|e| e.kind == Kind::Ambiguous),
        "{:?}",
        got.report_declared.container
    );
    assert_eq!(got.recommendation, Recommendation::Reject);
}

/// A PDF beyond the receiver's limits: the container is invalid and nothing is
/// validated.
#[cfg(feature = "zugferd")]
#[test]
fn a_pdf_over_the_limits_is_refused_at_the_container() {
    let bytes = pdf(&xml(&xrechnung(), Syntax::Cii), "XRECHNUNG");
    let tight = Policy {
        limits: en16931_formats::zugferd::Limits::new().with_max_stream_bytes(64),
        ..Policy::default()
    };
    let got = receive(&bytes, &tight);
    let report = &got.report_declared;
    assert_eq!(report.container.validity, Validity::Invalid);
    let e = &report.container.entries[0];
    assert_eq!(e.kind, Kind::Unreadable);
    assert!(e.message.contains("inflates past"), "{e}");
    assert!(matches!(report.syntax.validity, Validity::Skipped(_)));
    assert!(matches!(report.semantic.validity, Validity::Skipped(_)));
    assert!(report.semantic.report.is_none(), "nothing validated");
    assert!(got.invoice.is_none() && got.declared.is_none());
    assert_eq!(got.recommendation, Recommendation::Reject);
}

/// Non-invoice bytes: rejected, with the syntax layer's reason.
#[test]
fn an_unreadable_document_is_rejected_with_its_reason() {
    let got = receive(b"<html/>", &Policy::default());
    assert_eq!(got.recommendation, Recommendation::Reject);
    assert_eq!(got.report_declared.syntax.validity, Validity::Invalid);
    assert!(got.reasons[0].contains("found <html>"), "{:?}", got.reasons);
}

/// The report header identifies the document and the rule set.
#[test]
fn the_report_header_names_the_document_and_the_rule_set() {
    let inv = xrechnung();
    let got = receive(&xml(&inv, Syntax::Cii), &Policy::default());
    let r = &got.report_declared;
    assert_eq!(r.identity.seller, inv.seller.name);
    assert_eq!(r.identity.number, inv.number);
    assert_eq!(r.identity.issue_date, inv.issue_date.map(|d| d.to_string()));
    let rs = r.rule_set.as_ref().expect("a rule set");
    assert_eq!(rs.specification_id, profiles::XRECHNUNG.specification_id);
    assert!(
        rs.artefacts.iter().any(|a| a.starts_with("KoSIT")),
        "{rs:?}"
    );
    assert!(r.most_severe().is_none_or(|s| s != Severity::Fatal));
}

/// Re-levelling `BR-CL-23` turns Reject into Accept, is recorded, and blocks a
/// proof.
#[test]
fn a_relevelled_rule_turns_reject_into_accept_and_is_recorded() {
    let mut inv = declaring(profiles::EN16931.specification_id);
    inv.lines[0].unit_code = en16931::invoice::Code::new("NOT-A-REC20-CODE");
    let report = profiles::EN16931.validate(&inv);
    let fatal: Vec<&str> = report.fatal().map(|f| f.rule.as_str()).collect();
    assert_eq!(fatal, ["BR-CL-23"], "the document fails BR-CL-23 alone");

    let bytes = xml(&inv, Syntax::Ubl);
    let strict = receive(&bytes, &Policy::default());
    assert_eq!(strict.recommendation, Recommendation::Reject);

    let lenient = Policy {
        relevel: vec![("BR-CL-23".to_owned(), Severity::Warning)],
        ..Policy::default()
    };
    let got: Received = receive(&bytes, &lenient);
    assert_eq!(
        got.recommendation,
        Recommendation::Accept,
        "{:?}",
        got.reasons
    );
    let semantic = got.report_declared.semantic.report.as_ref().expect("ran");
    assert_eq!(
        semantic.relevelled(),
        [("BR-CL-23".to_owned(), Severity::Warning)]
    );
    assert!(got.report_declared.to_string().contains("re-levelled"));

    let proof = en16931::validation::Check::of::<en16931::En16931>()
        .relevel("BR-CL-23", Severity::Warning)
        .prove::<en16931::En16931>(inv);
    assert!(proof.is_err(), "a re-levelled check proves nothing");
}

/// A received document survives a JSON round trip.
#[cfg(feature = "serde")]
#[test]
fn a_received_document_survives_json() {
    let got = receive(
        &xml(&declaring(XOEV), Syntax::Cii),
        &policy(Some(&profiles::XRECHNUNG)),
    );
    let json = serde_json::to_string(&got).expect("serialise");
    let back: Received = serde_json::from_str(&json).expect("deserialise");
    assert_eq!(serde_json::to_string(&back).expect("again"), json);
    assert_eq!(back.declared, got.declared);
    assert_eq!(back.recommendation, got.recommendation);
    assert_eq!(back.coincides(), got.coincides());
}

/// `CII-SR-452` (a second `ram:SpecifiedTradePaymentTerms`) is a CEN warning
/// but a KoSIT error: rejected under XRechnung, accepted under the core.
#[test]
fn a_duplicated_payment_terms_carrier_is_fatal_under_xrechnung_only() {
    let inv = en16931::examples::for_profile(&profiles::EN16931).expect("shipped");
    let cii = String::from_utf8(xml(&inv, Syntax::Cii)).expect("utf-8");
    let terms = cii
        .find("<ram:SpecifiedTradePaymentTerms>")
        .expect("the example states BT-9");
    let doubled = format!(
        "{}<ram:SpecifiedTradePaymentTerms><ram:Description>Zweite Zahlungsbedingung</ram:Description></ram:SpecifiedTradePaymentTerms>{}",
        &cii[..terms],
        &cii[terms..]
    );

    let core = receive(doubled.as_bytes(), &Policy::default());
    assert_eq!(core.recommendation, Recommendation::Accept, "{core}");
    let entry = core
        .report_declared
        .schema
        .entries
        .iter()
        .find(|e| e.rule.as_deref() == Some("CII-SR-452"))
        .expect("the duplicate is reported");
    assert_eq!(entry.kind, Kind::Repeated, "CEN: a warning");
    assert_eq!(core.report_declared.schema.validity, Validity::Valid);

    let policy = Policy {
        require: Some(&profiles::XRECHNUNG),
        ..Policy::default()
    };
    let xr = receive(doubled.as_bytes(), &policy);
    assert_eq!(xr.recommendation, Recommendation::Reject, "{xr}");
    let required = xr.report_required.as_ref().expect("a second report");
    assert_eq!(
        required
            .schema
            .entries
            .iter()
            .find(|e| e.rule.as_deref() == Some("CII-SR-452"))
            .map(|e| e.kind),
        Some(Kind::Duplicate),
        "KoSIT: an error"
    );
    assert!(matches!(required.schema.validity, Validity::Invalid));
    assert!(
        xr.reasons.iter().any(|r| r.contains("schema layer")),
        "{:?}",
        xr.reasons
    );
    // The declared report (core) keeps CEN's level.
    assert_eq!(xr.report_declared.schema.validity, Validity::Valid);
}

/// An XRechnung Extension `BG-DEX-01` group — a sub-line with its own
/// child — built from `inv.lines[0]` twice: once as the group, which states
/// neither BT-129 nor BG-29, once as its only child, which states both.
/// Cloning the same line keeps `BR-DEX-02` (the group's BT-131 is the sum of
/// its children's) and `BR-DEX-03` (exactly one VAT category each) satisfied
/// by construction.
fn xrechnung_extension_with_a_group() -> Invoice {
    let mut inv = xrechnung();
    inv.specification_id = Some(profiles::XRECHNUNG_EXTENSION.specification_id.to_owned());

    let mut child = en16931::SubInvoiceLine::from(inv.lines[0].clone());
    child.id = "1.1.".to_owned();
    let group = en16931::SubInvoiceLine {
        id: "1.".to_owned(),
        quantity: None,
        price: None,
        children: vec![child.clone()],
        ..child
    };
    inv.extensions.sub_invoice_lines = vec![(0, vec![group])];
    let report = profiles::XRECHNUNG_EXTENSION.validate(&inv);
    assert!(report.is_valid(), "{report}");
    inv
}

/// A grouping `BG-DEX-01` sub-line (one with its own child) may leave BT-129
/// and BG-29 unstated: `BR-22`'s and `BR-26`'s context is `cac:InvoiceLine |
/// cac:CreditNoteLine`, never a `cac:SubInvoiceLine`, and the XRechnung
/// Extension Schematron checks only `BR-DEX-02` and `BR-DEX-03` there. The
/// real-world case this guards is the KoSIT XRechnung Extension testsuite's
/// `04.03a`/`04.04a-INVOICE_ubl.xml`, each a lawful invoice with such a
/// group.
#[test]
fn a_grouping_sub_line_without_quantity_or_price_is_accepted() {
    let inv = xrechnung_extension_with_a_group();
    let bytes = xml(&inv, Syntax::Ubl);

    let read = en16931_formats::read(&bytes).expect("readable");
    let fatal: Vec<_> = read
        .entries
        .iter()
        .filter(|e| e.level() == Severity::Fatal)
        .collect();
    assert!(fatal.is_empty(), "{fatal:?}");
    assert_eq!(
        read.invoice.extensions.sub_invoice_lines, inv.extensions.sub_invoice_lines,
        "the group reads back without quantity or price, the child with both"
    );

    let got = receive(&bytes, &policy(Some(&profiles::XRECHNUNG_EXTENSION)));
    assert_eq!(
        got.recommendation,
        Recommendation::Accept,
        "{:?}",
        got.reasons
    );
}

/// A childless `BG-DEX-01` sub-line may also leave BT-129 and BG-29
/// unstated: `BR-22`'s and `BR-26`'s context is `cac:InvoiceLine |
/// cac:CreditNoteLine`
/// (`eInvoicing-EN16931/ubl/schematron/preprocessed/
/// EN16931-UBL-validation-preprocessed.sch:138-144`), never a sub-line,
/// grouping or not — confirmed against the KoSIT validator, which accepts a
/// childless sub-line missing both.
#[test]
fn a_childless_sub_line_without_quantity_or_price_is_accepted() {
    let mut inv = xrechnung();
    inv.specification_id = Some(profiles::XRECHNUNG_EXTENSION.specification_id.to_owned());
    let leaf = en16931::SubInvoiceLine {
        id: "1.".to_owned(),
        quantity: None,
        price: None,
        ..en16931::SubInvoiceLine::from(inv.lines[0].clone())
    };
    inv.extensions.sub_invoice_lines = vec![(0, vec![leaf])];
    let report = profiles::XRECHNUNG_EXTENSION.validate(&inv);
    assert!(report.is_valid(), "{report}");

    let bytes = xml(&inv, Syntax::Ubl);
    let read = en16931_formats::read(&bytes).expect("readable");
    let fatal: Vec<_> = read
        .entries
        .iter()
        .filter(|e| e.level() == Severity::Fatal)
        .collect();
    assert!(fatal.is_empty(), "{fatal:?}");
    let [sub] = read.invoice.extensions.sub_lines(0) else {
        panic!(
            "one sub-line, got {:?}",
            read.invoice.extensions.sub_lines(0)
        );
    };
    assert_eq!(sub.quantity, None, "no BT-129 was written");
    assert_eq!(sub.price, None, "no BG-29 was written");

    let got = receive(&bytes, &policy(Some(&profiles::XRECHNUNG_EXTENSION)));
    assert_eq!(
        got.recommendation,
        Recommendation::Accept,
        "{:?}",
        got.reasons
    );
}

/// An empty `cbc:InvoicedQuantity` or `cac:Price` without `cbc:PriceAmount`
/// is, unlike an absent one, a schema violation UBL never allows: the UBL 2.1
/// schema requires non-empty `xs:decimal` content wherever the element is
/// present at all. KoSIT's validator refuses both with a 406
/// (`cvc-datatype-valid.1.2.1` for the empty quantity, `cvc-complex-type.2.4.b`
/// for `<cac:Price/>`) on a grouping sub-line exactly as on a childless one;
/// this covers the grouping case, the next test the childless one.
#[test]
fn an_empty_quantity_or_price_on_a_group_is_rejected() {
    let xml = r#"<Invoice xmlns="urn:oasis:names:specification:ubl:schema:xsd:Invoice-2"
        xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2"
        xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2">
      <cbc:CustomizationID>urn:cen.eu:en16931:2017#compliant#urn:xeinkauf.de:kosit:xrechnung_3.0#conformant#urn:xeinkauf.de:kosit:extension:xrechnung_3.0</cbc:CustomizationID>
      <cac:LegalMonetaryTotal>
        <cbc:LineExtensionAmount currencyID="EUR">300.00</cbc:LineExtensionAmount>
        <cbc:TaxExclusiveAmount currencyID="EUR">300.00</cbc:TaxExclusiveAmount>
        <cbc:TaxInclusiveAmount currencyID="EUR">357.00</cbc:TaxInclusiveAmount>
        <cbc:PayableAmount currencyID="EUR">357.00</cbc:PayableAmount>
      </cac:LegalMonetaryTotal>
      <cac:InvoiceLine>
        <cbc:ID>1</cbc:ID>
        <cbc:InvoicedQuantity unitCode="C62">3</cbc:InvoicedQuantity>
        <cbc:LineExtensionAmount currencyID="EUR">300.00</cbc:LineExtensionAmount>
        <cac:Item>
          <cbc:Name>Widget</cbc:Name>
          <cac:ClassifiedTaxCategory>
            <cbc:ID>S</cbc:ID>
            <cbc:Percent>19</cbc:Percent>
            <cac:TaxScheme><cbc:ID>VAT</cbc:ID></cac:TaxScheme>
          </cac:ClassifiedTaxCategory>
        </cac:Item>
        <cac:Price>
          <cbc:PriceAmount currencyID="EUR">100.00</cbc:PriceAmount>
        </cac:Price>
        <cac:SubInvoiceLine>
          <cbc:ID>1.</cbc:ID>
          <cbc:InvoicedQuantity unitCode="C62"></cbc:InvoicedQuantity>
          <cbc:LineExtensionAmount currencyID="EUR">300.00</cbc:LineExtensionAmount>
          <cac:Item>
            <cbc:Name>Widget</cbc:Name>
            <cac:ClassifiedTaxCategory>
              <cbc:ID>S</cbc:ID>
              <cbc:Percent>19</cbc:Percent>
              <cac:TaxScheme><cbc:ID>VAT</cbc:ID></cac:TaxScheme>
            </cac:ClassifiedTaxCategory>
          </cac:Item>
          <cac:Price/>
          <cac:SubInvoiceLine>
            <cbc:ID>1.1.</cbc:ID>
            <cbc:InvoicedQuantity unitCode="C62">3</cbc:InvoicedQuantity>
            <cbc:LineExtensionAmount currencyID="EUR">300.00</cbc:LineExtensionAmount>
            <cac:Item>
              <cbc:Name>Widget</cbc:Name>
              <cac:ClassifiedTaxCategory>
                <cbc:ID>S</cbc:ID>
                <cbc:Percent>19</cbc:Percent>
                <cac:TaxScheme><cbc:ID>VAT</cbc:ID></cac:TaxScheme>
              </cac:ClassifiedTaxCategory>
            </cac:Item>
            <cac:Price>
              <cbc:PriceAmount currencyID="EUR">100.00</cbc:PriceAmount>
            </cac:Price>
          </cac:SubInvoiceLine>
        </cac:SubInvoiceLine>
      </cac:InvoiceLine>
    </Invoice>"#;

    let read = en16931_formats::read(xml.as_bytes()).expect("readable");
    let malformed: Vec<_> = read.entries_of(Kind::Malformed).collect();
    assert!(
        malformed.iter().any(
            |e| e.term.as_deref().is_some_and(|t| t.ends_with("/BT-129"))
                && e.path.contains("SubInvoiceLine")
        ),
        "the empty quantity must be reported: {malformed:?}"
    );
    assert!(
        malformed.iter().any(
            |e| e.term.as_deref().is_some_and(|t| t.ends_with("/BT-146"))
                && e.path.contains("SubInvoiceLine")
        ),
        "the empty price must be reported: {malformed:?}"
    );

    // The fixture lacks more than these two terms; the rejection must name
    // the sub-line's own, or it proves nothing about them.
    let got = receive(
        xml.as_bytes(),
        &policy(Some(&profiles::XRECHNUNG_EXTENSION)),
    );
    let reasons = got.reasons.join("; ");
    assert_eq!(got.recommendation, Recommendation::Reject, "{reasons}");
    for term in ["BT-129", "BT-146"] {
        assert!(
            reasons.contains(&format!("(BG-25[0]/BG-DEX-01[0]/{term})")),
            "the rejection names the sub-line's {term}: {reasons}"
        );
    }
}

/// As the previous test, on a childless sub-line (no `cac:SubInvoiceLine`
/// children) rather than a group — `BR-22`/`BR-26` do not distinguish the
/// two, and neither does UBL's schema.
#[test]
fn an_empty_quantity_or_price_on_a_childless_sub_line_is_rejected() {
    let xml = r#"<Invoice xmlns="urn:oasis:names:specification:ubl:schema:xsd:Invoice-2"
        xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2"
        xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2">
      <cbc:CustomizationID>urn:cen.eu:en16931:2017#compliant#urn:xeinkauf.de:kosit:xrechnung_3.0#conformant#urn:xeinkauf.de:kosit:extension:xrechnung_3.0</cbc:CustomizationID>
      <cac:LegalMonetaryTotal>
        <cbc:LineExtensionAmount currencyID="EUR">300.00</cbc:LineExtensionAmount>
        <cbc:TaxExclusiveAmount currencyID="EUR">300.00</cbc:TaxExclusiveAmount>
        <cbc:TaxInclusiveAmount currencyID="EUR">357.00</cbc:TaxInclusiveAmount>
        <cbc:PayableAmount currencyID="EUR">357.00</cbc:PayableAmount>
      </cac:LegalMonetaryTotal>
      <cac:InvoiceLine>
        <cbc:ID>1</cbc:ID>
        <cbc:InvoicedQuantity unitCode="C62">3</cbc:InvoicedQuantity>
        <cbc:LineExtensionAmount currencyID="EUR">300.00</cbc:LineExtensionAmount>
        <cac:Item>
          <cbc:Name>Widget</cbc:Name>
          <cac:ClassifiedTaxCategory>
            <cbc:ID>S</cbc:ID>
            <cbc:Percent>19</cbc:Percent>
            <cac:TaxScheme><cbc:ID>VAT</cbc:ID></cac:TaxScheme>
          </cac:ClassifiedTaxCategory>
        </cac:Item>
        <cac:Price>
          <cbc:PriceAmount currencyID="EUR">100.00</cbc:PriceAmount>
        </cac:Price>
        <cac:SubInvoiceLine>
          <cbc:ID>1.</cbc:ID>
          <cbc:InvoicedQuantity unitCode="C62"></cbc:InvoicedQuantity>
          <cbc:LineExtensionAmount currencyID="EUR">300.00</cbc:LineExtensionAmount>
          <cac:Item>
            <cbc:Name>Widget</cbc:Name>
            <cac:ClassifiedTaxCategory>
              <cbc:ID>S</cbc:ID>
              <cbc:Percent>19</cbc:Percent>
              <cac:TaxScheme><cbc:ID>VAT</cbc:ID></cac:TaxScheme>
            </cac:ClassifiedTaxCategory>
          </cac:Item>
          <cac:Price/>
        </cac:SubInvoiceLine>
      </cac:InvoiceLine>
    </Invoice>"#;

    let read = en16931_formats::read(xml.as_bytes()).expect("readable");
    let malformed: Vec<_> = read.entries_of(Kind::Malformed).collect();
    assert!(
        malformed.iter().any(
            |e| e.term.as_deref().is_some_and(|t| t.ends_with("/BT-129"))
                && e.path.contains("SubInvoiceLine")
        ),
        "the empty quantity must be reported: {malformed:?}"
    );
    assert!(
        malformed.iter().any(
            |e| e.term.as_deref().is_some_and(|t| t.ends_with("/BT-146"))
                && e.path.contains("SubInvoiceLine")
        ),
        "the empty price must be reported: {malformed:?}"
    );

    // The fixture lacks more than these two terms; the rejection must name
    // the sub-line's own, or it proves nothing about them.
    let got = receive(
        xml.as_bytes(),
        &policy(Some(&profiles::XRECHNUNG_EXTENSION)),
    );
    let reasons = got.reasons.join("; ");
    assert_eq!(got.recommendation, Recommendation::Reject, "{reasons}");
    for term in ["BT-129", "BT-146"] {
        assert!(
            reasons.contains(&format!("(BG-25[0]/BG-DEX-01[0]/{term})")),
            "the rejection names the sub-line's {term}: {reasons}"
        );
    }
}

/// `xml` with the text of the first `<tag …>` after the child sub-line's ID
/// replaced by `value`.
fn on_the_child(xml: &str, tag: &str, value: &str) -> String {
    let child = xml
        .find("<cbc:ID>1.1.</cbc:ID>")
        .expect("the child sub-line");
    let open = child + xml[child..].find(&format!("<{tag}")).expect("its element");
    let content = open + xml[open..].find('>').expect("end of tag") + 1;
    let end = content + xml[content..].find('<').expect("end of content");
    format!("{}{value}{}", &xml[..content], &xml[end..])
}

/// A sub-line may state its quantity without a unit (`BR-23` holds on lines
/// only); written back, the quantity carries no empty `unitCode`.
#[test]
fn a_sub_line_quantity_without_a_unit_is_written_without_one() {
    let written =
        String::from_utf8(xml(&xrechnung_extension_with_a_group(), Syntax::Ubl)).expect("utf-8");
    let child = written
        .find("<cbc:ID>1.1.</cbc:ID>")
        .expect("the child sub-line");
    let open = child
        + written[child..]
            .find("<cbc:InvoicedQuantity")
            .expect("its quantity");
    let close = open + written[open..].find('>').expect("end of tag");
    let probe = format!(
        "{}<cbc:InvoicedQuantity{}",
        &written[..open],
        &written[close..]
    );

    let read = en16931_formats::read(probe.as_bytes()).expect("readable");
    let fatal: Vec<_> = read
        .entries
        .iter()
        .filter(|e| e.level() == Severity::Fatal)
        .collect();
    assert!(fatal.is_empty(), "{fatal:?}");
    let child = &read.invoice.extensions.sub_lines(0)[0].children[0];
    let Some(stated) = &child.quantity else {
        panic!("the child states its quantity: {child:?}");
    };
    assert_eq!(stated.unit_code, None);

    let back = String::from_utf8(xml(&read.invoice, Syntax::Ubl)).expect("utf-8");
    let quantity = format!(
        "<cbc:InvoicedQuantity>{}</cbc:InvoicedQuantity>",
        stated.value
    );
    assert!(back.contains(&quantity), "{quantity} not in {back}");
}

/// A sub-line's quantity or price that does not parse is `Malformed`, and the
/// rest of what the sub-line states is kept and written back, as on a line:
/// the element is stated, its value is a placeholder.
#[test]
fn a_sub_line_value_that_does_not_parse_keeps_its_element() {
    let inv = xrechnung_extension_with_a_group();
    assert!(
        inv.lines[0].price.base_quantity.is_some(),
        "the fixture's price carries BT-149"
    );
    let written = String::from_utf8(xml(&inv, Syntax::Ubl)).expect("utf-8");
    for tag in ["cbc:InvoicedQuantity", "cbc:PriceAmount"] {
        let probe = on_the_child(&written, tag, "abc");
        let read = en16931_formats::read(probe.as_bytes()).expect("readable");
        let values: Vec<_> = read
            .entries_of(Kind::Malformed)
            .map(|e| e.value.as_deref())
            .collect();
        assert_eq!(values, [Some("abc")], "{tag}");
        let child = &read.invoice.extensions.sub_lines(0)[0].children[0];
        assert_eq!(
            child.quantity.as_ref().map(|q| q.unit_code.clone()),
            Some(Some(inv.lines[0].unit_code.clone())),
            "{tag}"
        );
        assert_eq!(
            child.price.as_ref().map(|p| p.base_quantity),
            Some(inv.lines[0].price.base_quantity),
            "{tag}"
        );

        let back = String::from_utf8(xml(&read.invoice, Syntax::Ubl)).expect("utf-8");
        for element in ["<cbc:InvoicedQuantity", "<cac:Price>", "<cbc:BaseQuantity"] {
            assert_eq!(
                back.matches(element).count(),
                written.matches(element).count(),
                "{tag}: {element}"
            );
        }
    }
}

/// A crafted XRechnung whose line quantity is `Decimal::MAX` and whose BT-131
/// is negative: the expected line amount minus the stated one overflows.
/// `receive` rejects it rather than panicking.
#[test]
fn a_quantity_at_the_decimal_limit_is_rejected_not_a_panic() {
    let mut inv = xrechnung();
    inv.lines[0].quantity = en16931::Quantity::new(rust_decimal::Decimal::MAX);
    inv.lines[0].price.net_price = en16931::UnitPriceAmount::new(rust_decimal::Decimal::ONE);
    inv.lines[0].price.base_quantity = None;
    inv.lines[0].allowances.clear();
    inv.lines[0].charges.clear();
    inv.lines[0].net_amount = en16931::InvoiceAmount::parse("-1.00").unwrap();
    let bytes = xml(&inv, Syntax::Ubl);
    assert!(
        String::from_utf8_lossy(&bytes).contains("79228162514264337593543950335"),
        "the quantity is written"
    );
    let got = receive(&bytes, &Policy::default());
    assert_eq!(
        got.recommendation,
        Recommendation::Reject,
        "{:?}",
        got.reasons
    );
}
