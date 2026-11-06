#![cfg(all(feature = "ubl", feature = "cii"))]

//! One call per task: `read`, `write`, `write_unchecked`, `convert`.

mod common;

use en16931::profiles::XRechnung;
use en16931::{Invoice, Validated};
use en16931_formats::report::Kind;
use en16931_formats::{ConvertError, Error, Loss, Syntax, convert, read, write, write_unchecked};

/// A valid XRechnung invoice: the maximal fixture without BT-8 (`BR-CO-03`)
/// and with one buyer identifier.
fn xrechnung() -> Invoice {
    let mut inv = common::maximal();
    inv.vat_point_date_code = None;
    inv.buyer.identifiers.truncate(1);
    inv.specification_id = Some(en16931::profiles::XRECHNUNG.specification_id.to_owned());
    inv
}

fn bytes(inv: &Invoice, syntax: Syntax) -> Vec<u8> {
    write_unchecked(inv, syntax)
        .expect("writable")
        .xml
        .into_bytes()
}

/// The same invoice reads to equal models from UBL and CII through one call.
#[test]
fn one_read_for_both_syntaxes() {
    let inv = xrechnung();
    let ubl = read(&bytes(&inv, Syntax::Ubl)).expect("UBL");
    let cii = read(&bytes(&inv, Syntax::Cii)).expect("CII");
    assert_eq!(ubl.syntax, Syntax::Ubl);
    assert_eq!(cii.syntax, Syntax::Cii);
    assert_eq!(ubl.invoice.number, cii.invoice.number);
    assert_eq!(ubl.invoice.lines.len(), cii.invoice.lines.len());
    assert_eq!(ubl.invoice.totals, cii.invoice.totals);
    assert!(ubl.entries.is_empty(), "{:#?}", ubl.entries);
    assert!(cii.entries.is_empty(), "{:#?}", cii.entries);
}

fn utf16(xml: &str, le: bool) -> Vec<u8> {
    let mark: [u8; 2] = if le { [0xFF, 0xFE] } else { [0xFE, 0xFF] };
    let xml = xml.replacen("encoding=\"UTF-8\"", "encoding=\"UTF-16\"", 1);
    mark.into_iter()
        .chain(
            xml.encode_utf16()
                .flat_map(|u| if le { u.to_le_bytes() } else { u.to_be_bytes() }),
        )
        .collect()
}

/// Four encodings, one model; ISO-8859-1 named.
#[test]
fn the_four_accepted_encodings_read_alike_and_others_are_named() {
    let xml = String::from_utf8(bytes(&xrechnung(), Syntax::Ubl)).expect("UTF-8");
    let mut bom = b"\xEF\xBB\xBF".to_vec();
    bom.extend_from_slice(xml.as_bytes());
    let base = read(xml.as_bytes()).expect("UTF-8").invoice;
    for encoded in [bom, utf16(&xml, true), utf16(&xml, false)] {
        assert_eq!(read(&encoded).expect("decodes").invoice, base);
    }
    let latin = xml.replacen("encoding=\"UTF-8\"", "encoding=\"ISO-8859-1\"", 1);
    match read(latin.as_bytes()) {
        Err(Error::Encoding { found }) => assert_eq!(found, "ISO-8859-1"),
        other => panic!("{other:?}"),
    }
}

/// Every authority document reads to the same model in each accepted encoding.
#[test]
fn the_corpus_reads_alike_in_every_accepted_encoding() {
    let mut checked = 0;
    for syntax in [Syntax::Ubl, Syntax::Cii] {
        for (path, text) in common::authority_documents(syntax, "encodings") {
            let Ok(base) = read(text.as_bytes()) else {
                continue;
            };
            let decl = text.trim_start_matches('\u{feff}');
            let utf16_text = if decl.starts_with("<?xml") {
                let end = decl.find("?>").expect("declaration closed");
                let head = &decl[..end];
                let head = if head.contains("encoding") {
                    head.replace("UTF-8", "UTF-16").replace("utf-8", "UTF-16")
                } else {
                    head.to_owned()
                };
                format!("{head}{}", &decl[end..])
            } else {
                decl.to_owned()
            };
            let mut bom = b"\xEF\xBB\xBF".to_vec();
            bom.extend_from_slice(decl.as_bytes());
            for (label, encoded) in [
                ("UTF-8 + BOM", bom),
                ("UTF-16 LE", utf16(&utf16_text, true)),
                ("UTF-16 BE", utf16(&utf16_text, false)),
            ] {
                let got =
                    read(&encoded).unwrap_or_else(|e| panic!("{} {label}: {e}", path.display()));
                assert_eq!(got.invoice, base.invoice, "{} {label}", path.display());
            }
            checked += 1;
        }
    }
    if common::spec_root().is_some() {
        assert_eq!(
            checked, 500,
            "every readable authority document, in both syntaxes"
        );
    }
}

/// A document that is not an invoice says what it is.
#[test]
fn a_foreign_document_names_its_root() {
    match read(b"<html><body/></html>") {
        Err(Error::NotAnInvoice { found }) => assert_eq!(found, "html"),
        other => panic!("{other:?}"),
    }
    match read(b"<Invoice xmlns='urn:x'/>") {
        Err(Error::NotAnInvoice { found }) => assert!(found.contains("urn:x"), "{found}"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(read(b"not xml"), Err(Error::Xml { .. })));
}

/// `write` stamps BT-24 from the proof and carries its entries.
#[test]
fn write_stamps_the_profile_it_proved() {
    let mut inv = xrechnung();
    inv.specification_id = Some(en16931::profiles::EN16931.specification_id.to_owned());
    // BR-DE-21 is a warning, so a core BT-24 still proves XRechnung; the
    // document must not leave declaring anything else.
    let proof = Validated::<XRechnung>::new(inv).expect("valid XRechnung");
    for syntax in [Syntax::Ubl, Syntax::Cii] {
        let out = write(&proof, syntax);
        assert!(out.xml.contains("xrechnung_3.0"), "{syntax}");
        assert!(out.entries.is_empty(), "{syntax}: {:#?}", out.entries);
    }
}

/// No public function emits a document its schema rejects.
#[test]
fn an_unvalidated_model_without_bt_1_is_refused() {
    let mut inv = xrechnung();
    inv.number = None;
    for syntax in [Syntax::Ubl, Syntax::Cii] {
        let refused = write_unchecked(&inv, syntax).expect_err("no form for it");
        assert!(
            refused
                .entries
                .iter()
                .any(|e| e.kind == Kind::Schema && e.term.as_deref() == Some("BT-1")),
            "{syntax}: {refused:?}"
        );
        assert!(refused.to_string().contains("BT-1"), "{refused}");
    }
}

/// A conversion that would lose a term is refused, naming it; allowed, it
/// returns the document and the loss.
#[test]
fn a_lossy_conversion_is_refused_unless_allowed() {
    use en16931::invoice::PrecedingInvoice;
    let mut inv = xrechnung();
    inv.preceding_invoices = ["INV-1", "INV-2"]
        .into_iter()
        .map(|r| PrecedingInvoice {
            reference: en16931::DocumentReference::new(r),
            issue_date: None,
        })
        .collect();
    let ubl = bytes(&inv, Syntax::Ubl);
    match convert(&ubl, Syntax::Cii, Loss::Refuse) {
        Err(ConvertError::Lossy { lost }) => {
            assert_eq!(lost.len(), 1, "{lost:#?}");
            assert_eq!(lost[0].term.as_deref(), Some("BG-3[0]"));
            assert_eq!(lost[0].value.as_deref(), Some("INV-2"));
        }
        other => panic!("{other:?}"),
    }
    let converted = convert(&ubl, Syntax::Cii, Loss::Allow).expect("allowed");
    assert_eq!(converted.lost.len(), 1);
    assert!(converted.document.contains("INV-1") && !converted.document.contains("INV-2"));
    assert!(converted.report.is_valid(), "{}", converted.report);

    // Without loss, `Refuse` converts.
    let clean = convert(&bytes(&xrechnung(), Syntax::Ubl), Syntax::Cii, Loss::Refuse)
        .expect("nothing lost");
    assert!(clean.lost.is_empty());
    assert_eq!(
        read(clean.document.as_bytes())
            .expect("readable")
            .invoice
            .number,
        xrechnung().number
    );
}

/// A PDF reaching a build without the feature is refused by name.
#[cfg(not(feature = "zugferd"))]
#[test]
fn a_pdf_without_the_feature_is_refused_by_name() {
    assert!(matches!(
        read(b"%PDF-1.7\n"),
        Err(Error::Unsupported {
            found: "a PDF",
            feature: "zugferd"
        })
    ));
}
