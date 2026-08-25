//! Pull the invoice out of a ZUGFeRD / Factur-X PDF.
//!
//! ```sh
//! cargo run --example zugferd_extract --features zugferd -- invoice.pdf
//! ```
//!
//! With no argument, a small PDF is built in-process so the example runs
//! anywhere — the structure is the same one the specification describes.
//!
//! What this shows beyond "it parses":
//!
//! * **The payload comes back verbatim** *and* as an `Invoice`. Whoever is
//!   diagnosing a rejected invoice needs the bytes the counterparty sent, not a
//!   reconstruction of them.
//! * **The XMP is read separately from the payload**, and the two are compared.
//!   A PDF whose metadata says BASIC while the payload says EN 16931 validates,
//!   opens, and is wrong in a way no schema notices.
//! * **Not every profile is an EN 16931 invoice.** MINIMUM and BASIC WL carry no
//!   lines and cannot satisfy `BR-16`.

use en16931_formats::zugferd::{self, IsInvoice};
use lopdf::dictionary;

fn main() {
    let pdf = match std::env::args().nth(1) {
        Some(path) => std::fs::read(&path).unwrap_or_else(|e| {
            eprintln!("cannot read {path}: {e}");
            std::process::exit(1);
        }),
        None => {
            println!("(no PDF given — using a generated one)\n");
            built_in_pdf()
        }
    };

    let got = match zugferd::extract(&pdf) {
        Ok(g) => g,
        Err(zugferd::Error::NoInvoice { found, .. }) => {
            // A plain PDF is a different problem from a broken one, and the
            // error carries what the file *did* contain — the actual question.
            println!("no embedded invoice. attachments present: {found:?}");
            return;
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    println!("embedded file:  {}", got.filename);
    println!("payload:        {} bytes", got.xml.len());
    println!("claims BT-24:   {:?}", got.specification_id);
    println!("profile:        {}", got.profile);
    println!("XMP declares:   {:?}", got.xmp.conformance_level);

    match got.profile.is_en16931_invoice() {
        IsInvoice::Yes => println!("EN 16931 invoice: yes — the 317 rules apply"),
        IsInvoice::No(why) => println!("EN 16931 invoice: NO — {why}"),
        IsInvoice::Unknown => println!("EN 16931 invoice: unknown profile; not guessing"),
    }

    // One `Display` per divergence, not one `match` arm per divergence here:
    // the sentences belong with the type, and a copy of them in every caller is
    // a copy that goes stale. `Divergence` is also `#[non_exhaustive]` — more
    // kinds of disagreement between a PDF and its payload will be found, and
    // this loop keeps printing them without being recompiled around.
    for d in &got.divergence {
        println!("\n⚠ {d}");
    }

    if let Some(invoice) = &got.invoice {
        println!("\nBT-1: {:?}", invoice.number);
        let report = en16931::validate(invoice);
        // Two fields is not an invoice: this one is short by a seller, a buyer,
        // a line and a total. The point here is that extraction hands back
        // something the rule engine can be pointed at, not that the specimen
        // passes.
        println!(
            "valid: {} ({} findings — the demonstration payload is deliberately bare)",
            report.is_valid(),
            report.findings().len()
        );
    } else {
        println!(
            "\nthe payload did not parse as CII: {:?}",
            got.syntax_findings
        );
    }
}

/// The document-level XMP of a conforming hybrid invoice.
///
/// Two independent claims in one packet: `pdfaid` says which part of ISO 19005
/// the file conforms to, and the `fx:` block says there is an invoice inside
/// and which profile it follows. This is *not* a complete PDF/A-3 packet — a
/// real one also describes the `fx:` properties in a `pdfaExtension:schemas`
/// bag — and the module docs explain why writing one correctly is harder than
/// it looks.
const XMP: &str = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
  xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
 <rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/"
   pdfaid:part="3" pdfaid:conformance="B"/>
 <rdf:Description xmlns:fx="urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#">
  <fx:DocumentType>INVOICE</fx:DocumentType>
  <fx:DocumentFileName>factur-x.xml</fx:DocumentFileName>
  <fx:Version>1.0</fx:Version>
  <fx:ConformanceLevel>EN 16931</fx:ConformanceLevel>
 </rdf:Description>
</rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#;

/// A minimal PDF/A-3-shaped file carrying an invoice, built here rather than
/// checked in — a binary fixture is opaque, and pins one producer's output.
fn built_in_pdf() -> Vec<u8> {
    let invoice = {
        let mut i = en16931::Invoice::default();
        i.number = Some("RE-2026-0042".into());
        i.specification_id = Some("urn:cen.eu:en16931:2017".into());
        i
    };
    let payload = en16931_formats::cii::to_string(&invoice);

    let mut doc = lopdf::Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id });
    doc.objects.insert(
        pages_id,
        lopdf::Object::Dictionary(dictionary! {
            "Type" => "Pages", "Count" => 1, "Kids" => vec![page_id.into()],
        }),
    );
    let stream = doc.add_object(lopdf::Stream::new(
        dictionary! { "Type" => "EmbeddedFile", "Subtype" => "text/xml" },
        payload.into_bytes(),
    ));
    let spec = doc.add_object(dictionary! {
        "Type" => "Filespec",
        "F"  => lopdf::Object::string_literal("factur-x.xml"),
        "UF" => lopdf::Object::string_literal("factur-x.xml"),
        "AFRelationship" => "Alternative",
        "EF" => dictionary! { "F" => stream, "UF" => stream },
    });
    let names = doc.add_object(dictionary! {
        "Names" => vec![lopdf::Object::string_literal("factur-x.xml"), spec.into()],
    });
    // The XMP packet, and it is not decoration: `pdfaid:part` is what makes
    // this a PDF/A-3 file rather than a PDF with an attachment, and the `fx:`
    // block is how a receiver discovers an invoice is here before parsing
    // anything. A file missing either opens fine and is not detected.
    let metadata = doc.add_object(lopdf::Stream::new(
        dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
        XMP.as_bytes().to_vec(),
    ));
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "Names" => dictionary! { "EmbeddedFiles" => names },
        // `/AF` is what makes the attachment an *associated* file. Attaching
        // without associating is the commonest defect in real hybrid invoices.
        "AF" => vec![lopdf::Object::Reference(spec)],
        "Metadata" => metadata,
    });
    doc.trailer.set("Root", catalog);

    let mut out = Vec::new();
    doc.save_to(&mut out).expect("write the demonstration PDF");
    out
}
