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

use en16931_formats::zugferd::{self, Divergence, IsInvoice};
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
        IsInvoice::Yes => println!("EN 16931 invoice: yes — the 316 rules apply"),
        IsInvoice::No(why) => println!("EN 16931 invoice: NO — {why}"),
        IsInvoice::Unknown => println!("EN 16931 invoice: unknown profile; not guessing"),
    }

    for d in &got.divergence {
        match d {
            Divergence::Profile { xmp, payload } => {
                println!("\n⚠ the PDF says {xmp:?} and the payload says {payload:?}");
                println!("  a receiver routing on the XMP and one routing on BT-24 would");
                println!("  process this file differently, and both would be correct");
            }
            Divergence::Filename { xmp, actual } => {
                println!("\n⚠ metadata names {xmp:?} but {actual:?} is attached");
            }
            Divergence::NoXmp => {
                println!("\n⚠ no XMP invoice metadata — readable here, because the");
                println!("  attachment was found by name, but a counterparty scanning");
                println!("  metadata first will not see an e-invoice at all");
            }
            // `Divergence` is `#[non_exhaustive]`: more kinds of disagreement
            // between a PDF and its payload will be found, and a caller written
            // today should not stop compiling when they are.
            other => println!("\n⚠ {other:?}"),
        }
    }

    if let Some(invoice) = &got.invoice {
        println!("\nBT-1: {:?}", invoice.number);
        let report = en16931::validate(invoice);
        println!(
            "valid: {} ({} findings)",
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
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "Names" => dictionary! { "EmbeddedFiles" => names },
    });
    doc.trailer.set("Root", catalog);

    let mut out = Vec::new();
    doc.save_to(&mut out).expect("write the demonstration PDF");
    out
}
