#![cfg(feature = "zugferd")]

//! Extraction, against PDFs built here rather than checked in.
//!
//! A fixture PDF committed as a binary blob is opaque: nobody can review it,
//! and it pins one producer's output rather than the structure the
//! specification describes. Building the PDFs in the test makes the thing being
//! asserted visible in the source — including the awkward cases, which is where
//! extraction actually fails.

use en16931_formats::zugferd::{Divergence, IsInvoice};
use lopdf::{Object, Stream, dictionary};

const INVOICE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rsm:CrossIndustryInvoice xmlns:rsm="urn:un:unece:uncefact:data:standard:CrossIndustryInvoice:100"
                          xmlns:ram="urn:un:unece:uncefact:data:standard:ReusableAggregateBusinessInformationEntity:100">
  <rsm:ExchangedDocumentContext>
    <ram:GuidelineSpecifiedDocumentContextParameter>
      <ram:ID>urn:cen.eu:en16931:2017</ram:ID>
    </ram:GuidelineSpecifiedDocumentContextParameter>
  </rsm:ExchangedDocumentContext>
</rsm:CrossIndustryInvoice>"#;

/// An XMP packet declaring a ZUGFeRD invoice. ⚠
fn xmp(level: &str, filename: &str) -> String {
    format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
  xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
 <rdf:Description xmlns:fx="urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#">
  <fx:DocumentType>INVOICE</fx:DocumentType>
  <fx:DocumentFileName>{filename}</fx:DocumentFileName>
  <fx:Version>1.0</fx:Version>
  <fx:ConformanceLevel>{level}</fx:ConformanceLevel>
 </rdf:Description>
</rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#
    )
}

fn pdf_with(attachments: &[(&str, &[u8])]) -> Vec<u8> {
    build_rel(attachments, None, "Alternative")
}

/// A PDF whose attachment declares `relationship` — the field that says whether
/// the XML *is* the invoice or merely accompanies one.
fn pdf_with_relationship(attachments: &[(&str, &[u8])], relationship: &str) -> Vec<u8> {
    build_rel(attachments, None, relationship)
}

/// A PDF carrying `attachments` as `/Filespec` objects in the catalogue's
/// `/Names/EmbeddedFiles` name tree — where PDF/A-3 says they belong — and
/// optionally an XMP metadata packet.
fn build(attachments: &[(&str, &[u8])], metadata: Option<&str>) -> Vec<u8> {
    build_rel(attachments, metadata, "Alternative")
}

fn build_rel(attachments: &[(&str, &[u8])], metadata: Option<&str>, relationship: &str) -> Vec<u8> {
    let mut doc = lopdf::Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Count" => 1, "Kids" => vec![page_id.into()],
        }),
    );

    let mut names = Vec::new();
    for (name, bytes) in attachments {
        let stream = doc.add_object(Stream::new(
            dictionary! { "Type" => "EmbeddedFile", "Subtype" => "text/xml" },
            bytes.to_vec(),
        ));
        let spec = doc.add_object(dictionary! {
            "Type" => "Filespec",
            "F" => Object::string_literal(*name),
            "UF" => Object::string_literal(*name),
            "AFRelationship" => relationship,
            "EF" => dictionary! { "F" => stream, "UF" => stream },
        });
        names.push(Object::string_literal(*name));
        names.push(spec.into());
    }

    let embedded = doc.add_object(dictionary! { "Names" => names });
    let mut catalog_dict = dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "Names" => dictionary! { "EmbeddedFiles" => embedded },
    };
    if let Some(packet) = metadata {
        let meta = doc.add_object(Stream::new(
            dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
            packet.as_bytes().to_vec(),
        ));
        catalog_dict.set("Metadata", meta);
    }
    let catalog = doc.add_object(catalog_dict);
    doc.trailer.set("Root", catalog);

    let mut out = Vec::new();
    doc.save_to(&mut out).expect("write the fixture PDF");
    out
}

#[test]
fn the_invoice_comes_back_verbatim() {
    let pdf = pdf_with(&[("factur-x.xml", INVOICE.as_bytes())]);
    let got = en16931_formats::zugferd::extract(&pdf).expect("extract");

    assert_eq!(got.xml, INVOICE, "the payload must be byte-identical");
    assert_eq!(got.filename, "factur-x.xml");
    assert_eq!(got.profile, en16931_formats::zugferd::Profile::En16931);
    assert_eq!(
        got.specification_id.as_deref(),
        Some("urn:cen.eu:en16931:2017")
    );
}

/// ⚠ ZUGFeRD 2.0 used a different filename. A reader does not choose what
/// arrives, so it must accept every convention it might be sent.
#[test]
fn the_older_filename_is_accepted_too() {
    let pdf = pdf_with(&[("zugferd-invoice.xml", INVOICE.as_bytes())]);
    let got = en16931_formats::zugferd::extract(&pdf).expect("extract");
    assert_eq!(got.filename, "zugferd-invoice.xml");
}

/// Producers disagree with the specification about casing, and have for years.
#[test]
fn the_filename_match_is_case_insensitive() {
    let pdf = pdf_with(&[("ZUGFeRD-invoice.XML", INVOICE.as_bytes())]);
    assert!(en16931_formats::zugferd::extract(&pdf).is_ok());
}

/// Two payloads should not happen and do. The newer convention wins, rather
/// than whichever the PDF happened to list first.
#[test]
fn preference_order_decides_when_a_pdf_carries_two() {
    let pdf = pdf_with(&[
        ("zugferd-invoice.xml", b"<old/>"),
        ("factur-x.xml", INVOICE.as_bytes()),
    ]);
    let got = en16931_formats::zugferd::extract(&pdf).expect("extract");
    assert_eq!(got.filename, "factur-x.xml", "the newer convention wins");
    assert_eq!(got.xml, INVOICE);
}

/// A plain PDF is a different problem from a broken one, and gets a different
/// message — including what it *did* carry, which is the actual support
/// question.
#[test]
fn a_pdf_without_an_invoice_says_what_it_has() {
    let pdf = pdf_with(&[("terms-and-conditions.pdf", b"%PDF-1.7")]);
    let err = en16931_formats::zugferd::extract(&pdf).expect_err("no invoice");
    let en16931_formats::zugferd::Error::NoInvoice { found, looked_for } = &err else {
        panic!("expected NoInvoice, got {err:?}");
    };
    assert_eq!(found, &["terms-and-conditions.pdf".to_owned()]);
    assert!(looked_for.contains(&"factur-x.xml"));
    assert!(
        err.to_string().contains("terms-and-conditions.pdf"),
        "{err}"
    );
}

#[test]
fn a_pdf_with_no_attachments_at_all_is_not_a_parse_failure() {
    let pdf = pdf_with(&[]);
    assert!(matches!(
        en16931_formats::zugferd::extract(&pdf),
        Err(en16931_formats::zugferd::Error::NoInvoice { .. })
    ));
}

/// Everything attached is listed, not just what looks like an invoice —
/// "no invoice found" is far easier to act on with the contents in hand.
#[test]
fn every_attachment_is_enumerable() {
    let pdf = pdf_with(&[
        ("factur-x.xml", INVOICE.as_bytes()),
        ("timesheet.csv", b"a,b,c"),
    ]);
    let files = en16931_formats::zugferd::embedded_files(&pdf).expect("enumerate");
    assert_eq!(files.len(), 2);
    assert_eq!(files["timesheet.csv"], b"a,b,c");
}

/// A payload that is not UTF-8 is reported, not lossily decoded. Silently
/// replacing bytes would corrupt an invoice number and validate cleanly.
#[test]
fn a_non_utf8_payload_is_reported() {
    let pdf = pdf_with(&[("factur-x.xml", &[0xff, 0xfe, 0x00])]);
    assert!(matches!(
        en16931_formats::zugferd::extract(&pdf),
        Err(en16931_formats::zugferd::Error::Encoding(_))
    ));
}

/// The profile is what the document *claims*. MINIMUM claims to be a ZUGFeRD
/// document and is not an EN 16931 invoice, and the caller must be able to
/// tell — this is the trap the profile matrix exists for.
#[test]
fn a_minimum_profile_is_extracted_but_not_an_invoice() {
    let xml = INVOICE.replace("urn:cen.eu:en16931:2017", "urn:factur-x.eu:1p0:minimum");
    let pdf = pdf_with(&[("factur-x.xml", xml.as_bytes())]);
    let got = en16931_formats::zugferd::extract(&pdf).expect("extract");
    assert_eq!(got.profile, en16931_formats::zugferd::Profile::Minimum);
    let en16931_formats::zugferd::IsInvoice::No(why) = got.profile.is_en16931_invoice() else {
        panic!("MINIMUM must not be typed as an EN 16931 invoice");
    };
    assert!(why.contains("BR-16"), "{why}");
}

// ---------------------------------------------------------------------------
// XMP — how a receiver finds the invoice before parsing anything
// ---------------------------------------------------------------------------

#[test]
fn the_xmp_is_read_and_agrees_with_the_payload() {
    let pdf = build(
        &[("factur-x.xml", INVOICE.as_bytes())],
        Some(&xmp("EN 16931", "factur-x.xml")),
    );
    let got = en16931_formats::zugferd::extract(&pdf).expect("extract");

    assert_eq!(got.xmp.document_type.as_deref(), Some("INVOICE"));
    assert_eq!(got.xmp.conformance_level.as_deref(), Some("EN 16931"));
    assert_eq!(got.xmp.document_filename.as_deref(), Some("factur-x.xml"));
    assert_eq!(got.xmp.version.as_deref(), Some("1.0"));
    assert!(got.divergence.is_empty(), "{:?}", got.divergence);
}

/// The XMP writes `EN 16931`; BT-24 writes a URN. Comparing the literals would
/// report a divergence on every conforming document, so profiles are compared.
#[test]
fn different_spellings_of_the_same_profile_are_not_a_divergence() {
    let pdf = build(
        &[("factur-x.xml", INVOICE.as_bytes())],
        Some(&xmp("urn:cen.eu:en16931:2017", "factur-x.xml")),
    );
    assert!(
        en16931_formats::zugferd::extract(&pdf)
            .expect("extract")
            .divergence
            .is_empty()
    );
}

/// A PDF whose metadata says BASIC while the payload says EN 16931. Both
/// halves are valid; a receiver routing on the XMP and one routing on BT-24
/// process the same file differently, and both are behaving correctly.
#[test]
fn a_profile_mismatch_is_reported() {
    let pdf = build(
        &[("factur-x.xml", INVOICE.as_bytes())],
        Some(&xmp("BASIC", "factur-x.xml")),
    );
    let got = en16931_formats::zugferd::extract(&pdf).expect("extract");
    assert_eq!(
        got.profile,
        en16931_formats::zugferd::Profile::En16931,
        "the payload wins"
    );
    let [en16931_formats::zugferd::Divergence::Profile { xmp, payload }] = &got.divergence[..]
    else {
        panic!("expected one profile divergence, got {:?}", got.divergence);
    };
    assert_eq!(xmp, "BASIC");
    assert_eq!(payload, "urn:cen.eu:en16931:2017");
}

#[test]
fn a_filename_mismatch_is_reported() {
    let pdf = build(
        &[("factur-x.xml", INVOICE.as_bytes())],
        Some(&xmp("EN 16931", "zugferd-invoice.xml")),
    );
    let got = en16931_formats::zugferd::extract(&pdf).expect("extract");
    let [en16931_formats::zugferd::Divergence::Filename { xmp, actual }] = &got.divergence[..]
    else {
        panic!("expected one filename divergence, got {:?}", got.divergence);
    };
    assert_eq!(xmp, "zugferd-invoice.xml");
    assert_eq!(actual, "factur-x.xml");
}

/// Readable here, because the attachment was found by name — but a counterparty
/// scanning metadata first will not see an e-invoice at all. That is worth
/// saying rather than reporting a clean extraction.
#[test]
fn a_pdf_with_no_xmp_says_so() {
    let pdf = pdf_with(&[("factur-x.xml", INVOICE.as_bytes())]);
    let got = en16931_formats::zugferd::extract(&pdf).expect("extract");
    assert_eq!(
        got.divergence,
        vec![en16931_formats::zugferd::Divergence::NoXmp]
    );
    assert_eq!(got.xmp, en16931_formats::zugferd::Xmp::default());
}

/// A mangled metadata packet must not stop a readable invoice being extracted.
#[test]
fn broken_xmp_does_not_prevent_extraction() {
    let pdf = build(
        &[("factur-x.xml", INVOICE.as_bytes())],
        Some("<x:xmpmeta not closed"),
    );
    let got = en16931_formats::zugferd::extract(&pdf).expect("the invoice is still readable");
    assert_eq!(got.xml, INVOICE);
    assert_eq!(
        got.divergence,
        vec![en16931_formats::zugferd::Divergence::NoXmp]
    );
}

// ---------------------------------------------------------------------------
// The payload as a model, not a string
// ---------------------------------------------------------------------------

/// `extract` returns an `Invoice`, because the CII binding is in this crate.
///
/// This is what a hybrid-PDF crate is *for*: a caller receiving a ZUGFeRD file
/// should get business terms, not XML to find a parser for.
#[test]
fn the_payload_comes_back_as_the_model() {
    let inv = {
        let mut i = en16931::Invoice::default();
        i.number = Some("RE-2026-0001".into());
        i.issue_date = Some(en16931::Date::parse("2026-01-15").expect("date"));
        i.currency = Some(en16931::invoice::Code::new("EUR"));
        i.specification_id = Some("urn:cen.eu:en16931:2017".into());
        i
    };
    let payload = en16931_formats::cii::to_string(&inv);
    let pdf = pdf_with(&[("factur-x.xml", payload.as_bytes())]);

    let got = en16931_formats::zugferd::extract(&pdf).expect("extract");
    let read = got.invoice.expect("the payload parses as CII");

    assert_eq!(read.number.as_deref(), Some("RE-2026-0001"));
    assert_eq!(read.issue_date, inv.issue_date);
    assert_eq!(got.profile, en16931_formats::zugferd::Profile::En16931);
    assert!(got.syntax_findings.is_empty(), "{:?}", got.syntax_findings);
    assert_eq!(got.xml, payload, "the bytes are still verbatim");
}

/// The whole chain: extract, then validate with `en16931`.
///
/// `zugferd` re-implements no rule — it delegates every one — so this asserts
/// the seam works rather than the rules do.
#[test]
fn an_extracted_invoice_can_be_validated() {
    let payload = en16931_formats::cii::to_string(&en16931::Invoice::default());
    let pdf = pdf_with(&[("factur-x.xml", payload.as_bytes())]);
    let got = en16931_formats::zugferd::extract(&pdf).expect("extract");
    let inv = got.invoice.expect("parses");

    let report = en16931::validate(&inv);
    assert!(!report.is_valid(), "an empty invoice is not valid");
    assert!(
        report.findings().iter().any(|f| f.rule == "BR-02"),
        "BR-02 (invoice number) should fire"
    );
}

/// A payload that is not CII at all is reported, not silently empty.
#[test]
fn a_non_cii_payload_is_reported() {
    let pdf = pdf_with(&[("factur-x.xml", b"<Invoice xmlns='urn:x'/>")]);
    let got = en16931_formats::zugferd::extract(&pdf).expect("the PDF is still readable");
    assert!(got.invoice.is_none());
    assert!(!got.syntax_findings.is_empty(), "the reason must be given");
}

// ── /AFRelationship ───────────────────────────────────────────────────────────

/// The value is read back verbatim, because a receiver may route on it.
#[test]
fn the_attachment_relationship_is_reported() {
    let pdf = pdf_with(&[("factur-x.xml", INVOICE.as_bytes())]);
    let got = en16931_formats::zugferd::extract(&pdf).expect("extract");
    assert_eq!(got.relationship.as_deref(), Some("Alternative"));
    assert!(
        !got.divergence
            .iter()
            .any(|d| matches!(d, Divergence::Relationship { .. })),
        "Alternative on a full-invoice profile is not the case sources disagree about"
    );
}

/// `Data` says "the pages are the invoice, this XML is supplementary". On a
/// profile that carries lines that is wrong, and every published source agrees
/// it is wrong — which is why this is the one relationship check made.
#[test]
fn data_on_a_full_invoice_profile_is_a_divergence() {
    let pdf = pdf_with_relationship(&[("factur-x.xml", INVOICE.as_bytes())], "Data");
    let got = en16931_formats::zugferd::extract(&pdf).expect("extract");

    assert_eq!(got.relationship.as_deref(), Some("Data"));
    let found = got
        .divergence
        .iter()
        .find_map(|d| match d {
            Divergence::Relationship { found, profile } => Some((found.clone(), *profile)),
            _ => None,
        })
        .expect("a full-invoice profile attached as /Data must be reported");
    assert_eq!(found.0, "Data");
    assert_eq!(found.1.is_en16931_invoice(), IsInvoice::Yes);
}

/// …and is *not* a divergence for the two profiles that genuinely are
/// supplementary data. This is the half that stops the check being noise.
#[test]
fn data_is_correct_for_the_profiles_that_carry_no_lines() {
    // MINIMUM claims a BT-24 that is not an EN 16931 invoice.
    let minimum = INVOICE.replace("urn:cen.eu:en16931:2017", "urn:factur-x.eu:1p0:minimum");
    let pdf = pdf_with_relationship(&[("factur-x.xml", minimum.as_bytes())], "Data");
    let got = en16931_formats::zugferd::extract(&pdf).expect("extract");

    assert_ne!(
        got.profile.is_en16931_invoice(),
        IsInvoice::Yes,
        "the premise"
    );
    assert!(
        !got.divergence
            .iter()
            .any(|d| matches!(d, Divergence::Relationship { .. })),
        "Data is the correct relationship for a booking aid: {:?}",
        got.divergence
    );
}
