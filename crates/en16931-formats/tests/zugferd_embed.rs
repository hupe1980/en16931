#![cfg(feature = "zugferd")]

//! `zugferd::embed` behaviour, and the veraPDF gate: every constructed PDF/A-3b
//! input (blank page, built here) and every PDF/A-3 sample from the reference
//! corpus's ZUGFeRD `correct/` folders (its invoice removed) is embedded into and
//! checked by veraPDF in its pinned Docker image.
//!
//! Without Docker the gate skips; under `EN16931_REQUIRE_SPEC` it fails.

mod common;

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use en16931::profiles::{self, En16931, XRechnung};
use en16931::validation::profile::Validated;
use en16931_formats::Syntax;
use en16931_formats::zugferd::{
    self, EmbedError, EmbedOptions, Embeddable, Existing, Limits, Metadata, Relationship,
};
use lopdf::{Object, Stream, dictionary};

// ── inputs ────────────────────────────────────────────────────────────────

/// An unrelated extension schema, for an input whose XMP already carries the
/// `pdfaExtension:schemas` bag `embed` must join rather than duplicate.
const OTHER_SCHEMA: &str = r#"<rdf:Description rdf:about=""
   xmlns:pdfaExtension="http://www.aiim.org/pdfa/ns/extension/"
   xmlns:pdfaSchema="http://www.aiim.org/pdfa/ns/schema#"
   xmlns:pdfaProperty="http://www.aiim.org/pdfa/ns/property#">
  <pdfaExtension:schemas>
   <rdf:Bag>
    <rdf:li rdf:parseType="Resource">
     <pdfaSchema:schema>Test schema</pdfaSchema:schema>
     <pdfaSchema:namespaceURI>http://example.com/ns/test/</pdfaSchema:namespaceURI>
     <pdfaSchema:prefix>tst</pdfaSchema:prefix>
     <pdfaSchema:property>
      <rdf:Seq>
       <rdf:li rdf:parseType="Resource">
        <pdfaProperty:name>Note</pdfaProperty:name>
        <pdfaProperty:valueType>Text</pdfaProperty:valueType>
        <pdfaProperty:category>external</pdfaProperty:category>
        <pdfaProperty:description>a note</pdfaProperty:description>
       </rdf:li>
      </rdf:Seq>
     </pdfaSchema:property>
    </rdf:li>
   </rdf:Bag>
  </pdfaExtension:schemas>
 </rdf:Description>
 <rdf:Description rdf:about="" xmlns:tst="http://example.com/ns/test/">
  <tst:Note>kept byte for byte</tst:Note>
 </rdf:Description>"#;

fn packet(part: &str, extra: &str) -> String {
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
 <rdf:Description rdf:about=\"\" xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\" pdfaid:part=\"{part}\" pdfaid:conformance=\"B\"/>\n\
 <rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n\
  <dc:format>application/pdf</dc:format>\n\
  <dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">replaced</rdf:li></rdf:Alt></dc:title>\n\
 </rdf:Description>\n\
 {extra}\n\
</rdf:RDF>\n\
</x:xmpmeta>\n\
<?xpacket end=\"w\"?>"
    )
}

/// The shape of a constructed input.
#[derive(Clone, Copy, Debug)]
struct Shape {
    /// The XMP already carries a `pdfaExtension:schemas` bag.
    bag: bool,
    /// The file uses a cross-reference stream rather than a table.
    xref_stream: bool,
}

const SHAPES: [Shape; 4] = [
    Shape {
        bag: false,
        xref_stream: false,
    },
    Shape {
        bag: true,
        xref_stream: false,
    },
    Shape {
        bag: false,
        xref_stream: true,
    },
    Shape {
        bag: true,
        xref_stream: true,
    },
];

/// A PDF/A-3b file: one blank A4 page, an XMP packet, an `/ID`.
fn blank(shape: Shape) -> Vec<u8> {
    blank_with(
        &packet("3", if shape.bag { OTHER_SCHEMA } else { "" }),
        shape.xref_stream,
        None,
    )
}

fn blank_with(xmp: &str, xref_stream: bool, filter: Option<&str>) -> Vec<u8> {
    let mut doc = lopdf::Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Resources" => dictionary! {},
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1,
        }),
    );
    let mut meta = Stream::new(
        dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
        xmp.as_bytes().to_vec(),
    );
    meta.allows_compression = false;
    if let Some(f) = filter {
        meta.dict.set("Filter", Object::Name(f.as_bytes().to_vec()));
    }
    let meta_id = doc.add_object(meta);
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog", "Pages" => pages_id, "Metadata" => meta_id,
    });
    doc.trailer.set("Root", catalog_id);
    let id = Object::String(
        b"en16931-embed-id".to_vec(),
        lopdf::StringFormat::Hexadecimal,
    );
    doc.trailer.set("ID", vec![id.clone(), id]);
    doc.reference_table.cross_reference_type = if xref_stream {
        lopdf::xref::XrefType::CrossReferenceStream
    } else {
        lopdf::xref::XrefType::CrossReferenceTable
    };
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("save");
    assert_eq!(
        out.windows(5).any(|w| w == b"\nxref"),
        !xref_stream,
        "the constructed file has the cross-reference kind it claims"
    );
    out
}

fn proof<P: Embeddable>(profile: &en16931::Profile) -> Validated<P> {
    let invoice = en16931::examples::for_profile(profile).expect("shipped profile");
    Validated::<P>::new(invoice).expect("the example is valid")
}

fn fixed() -> EmbedOptions {
    EmbedOptions {
        modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_791_640_800)),
        ..EmbedOptions::default()
    }
}

/// The XMP packet of a PDF, as text.
fn xmp_of(pdf: &[u8]) -> String {
    let doc = lopdf::Document::load_mem(pdf).expect("parses");
    let id = doc
        .catalog()
        .and_then(|c| c.get(b"Metadata"))
        .and_then(Object::as_reference)
        .expect("metadata");
    let s = doc
        .get_object(id)
        .and_then(Object::as_stream)
        .expect("stream");
    String::from_utf8(s.content.clone()).expect("utf-8")
}

/// What every successful embed must satisfy, short of veraPDF.
fn assert_embedded<P: Embeddable>(input: &[u8], out: &[u8], proof: &Validated<P>, origin: &str) {
    assert!(
        out.starts_with(input),
        "{origin}: the input is not a prefix"
    );
    let got = zugferd::extract(out, &Limits::default()).expect("reads back");
    assert!(got.divergence.is_empty(), "{origin}: {:?}", got.divergence);
    assert_eq!(
        got.xml,
        en16931_formats::write(proof, Syntax::Cii).xml.as_bytes(),
        "{origin}: payload"
    );
    assert_eq!(got.filename, P::FILENAME, "{origin}");
    assert_eq!(got.profile, P::ZUGFERD, "{origin}");
    assert_eq!(
        got.xmp.document_type.as_deref(),
        Some("INVOICE"),
        "{origin}"
    );
    assert_eq!(got.xmp.version.as_deref(), Some("1.0"), "{origin}");
    assert_eq!(
        got.xmp.conformance_level.as_deref(),
        Some(P::ZUGFERD.as_str()),
        "{origin}"
    );
    let xmp = xmp_of(out);
    assert_eq!(
        xmp.matches("pdfaExtension:schemas>").count(),
        2,
        "{origin}: exactly one pdfaExtension:schemas property (open + close tag)"
    );
}

// ── behaviour ─────────────────────────────────────────────────────────────

#[test]
fn both_profiles_embed_and_read_back_in_every_shape() {
    for shape in SHAPES {
        let input = blank(shape);
        let en = proof::<En16931>(&profiles::EN16931);
        let out = zugferd::embed(&input, &en, &fixed()).expect("embeds");
        assert_embedded(&input, &out, &en, &format!("EN 16931 {shape:?}"));
        let xr = proof::<XRechnung>(&profiles::XRECHNUNG);
        let out = zugferd::embed(&input, &xr, &fixed()).expect("embeds");
        assert_embedded(&input, &out, &xr, &format!("XRechnung {shape:?}"));
        assert_eq!(
            zugferd::extract(&out, &Limits::default()).unwrap().filename,
            "xrechnung.xml"
        );
    }
}

#[test]
fn the_update_chains_to_the_previous_section_and_keeps_identity() {
    let input = blank(SHAPES[0]);
    let out = zugferd::embed(&input, &proof::<En16931>(&profiles::EN16931), &fixed()).unwrap();
    let start = String::from_utf8_lossy(&input);
    let prev: usize = start
        .rsplit("startxref")
        .next()
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let appended = String::from_utf8_lossy(&out[input.len()..]);
    assert!(appended.contains(&format!("/Prev {prev}")), "{appended}");
    let before = lopdf::Document::load_mem(&input).unwrap();
    let after = lopdf::Document::load_mem(&out).unwrap();
    assert_eq!(
        before.trailer.get(b"Root").unwrap(),
        after.trailer.get(b"Root").unwrap(),
        "the catalogue keeps its object number"
    );
    assert_eq!(
        before.trailer.get(b"ID").unwrap(),
        after.trailer.get(b"ID").unwrap()
    );
    assert_eq!(appended.matches("%%EOF").count(), 1, "one revision");
}

#[test]
fn every_other_xmp_property_is_kept_byte_for_byte() {
    let input = blank(SHAPES[1]);
    let out = zugferd::embed(&input, &proof::<En16931>(&profiles::EN16931), &fixed()).unwrap();
    let xmp = xmp_of(&out);
    for kept in [
        "pdfaid:part=\"3\" pdfaid:conformance=\"B\"",
        "<dc:format>application/pdf</dc:format>",
        "<tst:Note>kept byte for byte</tst:Note>",
        "<pdfaSchema:namespaceURI>http://example.com/ns/test/</pdfaSchema:namespaceURI>",
    ] {
        assert!(xmp.contains(kept), "{kept} lost:\n{xmp}");
    }
    assert!(!xmp.contains(">replaced<"), "the old title stays:\n{xmp}");
    assert_eq!(xmp.matches("<dc:title>").count(), 1);
    // The Factur-X description joined the existing bag.
    let bag = &xmp[xmp.find("<rdf:Bag>").unwrap()..];
    let bag = &bag[..bag.find("</rdf:Bag>").unwrap()];
    assert!(bag.contains("urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#"));
    assert!(bag.contains("http://example.com/ns/test/"));
}

#[test]
fn metadata_is_derived_from_the_invoice_and_written_twice() {
    let input = blank(SHAPES[0]);
    let p = proof::<En16931>(&profiles::EN16931);
    let out = zugferd::embed(&input, &p, &fixed()).unwrap();
    let want = Metadata::derived(p.invoice());
    let inv = p.invoice();
    assert_eq!(
        want.title,
        format!(
            "{}: Invoice {}",
            inv.seller.name.as_deref().unwrap(),
            inv.number.as_deref().unwrap()
        )
    );
    let xmp = xmp_of(&out);
    assert!(xmp.contains(&format!(">{}</rdf:li></rdf:Alt></dc:title>", want.title)));
    assert!(xmp.contains(&format!(
        ">{}</rdf:li></rdf:Alt></dc:description>",
        want.subject
    )));
    let doc = lopdf::Document::load_mem(&out).unwrap();
    let info = doc
        .trailer
        .get(b"Info")
        .and_then(|o| doc.dereference(o))
        .and_then(|(_, o)| o.as_dict())
        .unwrap();
    let title = lopdf::decode_text_string(info.get(b"Title").unwrap()).unwrap();
    assert_eq!(title, want.title);
}

#[test]
fn explicit_metadata_replaces_the_derived_and_survives_any_script() {
    let input = blank(SHAPES[0]);
    let meta = Metadata {
        title: "Rechnung <Nr. 1> & Söhne".to_owned(),
        subject: "Prüfung".to_owned(),
        author: "Müller & Co".to_owned(),
    };
    let options = EmbedOptions {
        metadata: Some(meta.clone()),
        ..fixed()
    };
    let out = zugferd::embed(&input, &proof::<En16931>(&profiles::EN16931), &options).unwrap();
    let xmp = xmp_of(&out);
    assert!(xmp.contains("Rechnung &lt;Nr. 1&gt; &amp; Söhne"), "{xmp}");
    let doc = lopdf::Document::load_mem(&out).unwrap();
    let info = doc
        .trailer
        .get(b"Info")
        .and_then(|o| doc.dereference(o))
        .and_then(|(_, o)| o.as_dict())
        .unwrap();
    assert_eq!(
        lopdf::decode_text_string(info.get(b"Author").unwrap()).unwrap(),
        meta.author
    );
}

#[test]
fn the_relationship_defaults_to_alternative_and_source_is_offered() {
    let input = blank(SHAPES[0]);
    let p = proof::<En16931>(&profiles::EN16931);
    let out = zugferd::embed(&input, &p, &fixed()).unwrap();
    let got = zugferd::extract(&out, &Limits::default()).unwrap();
    assert_eq!(got.relationship.as_deref(), Some("Alternative"));
    let options = EmbedOptions {
        relationship: Relationship::Source,
        ..fixed()
    };
    let out = zugferd::embed(&input, &p, &options).unwrap();
    let got = zugferd::extract(&out, &Limits::default()).unwrap();
    assert_eq!(got.relationship.as_deref(), Some("Source"));
    assert!(got.divergence.is_empty());
}

#[test]
fn a_fixed_time_gives_identical_output() {
    let input = blank(SHAPES[3]);
    let p = proof::<XRechnung>(&profiles::XRECHNUNG);
    assert_eq!(
        zugferd::embed(&input, &p, &fixed()).unwrap(),
        zugferd::embed(&input, &p, &fixed()).unwrap()
    );
}

// ── refusals ──────────────────────────────────────────────────────────────

fn refusal(pdf: &[u8]) -> EmbedError {
    zugferd::embed(pdf, &proof::<En16931>(&profiles::EN16931), &fixed())
        .expect_err("must be refused")
}

#[test]
fn bytes_that_are_not_a_pdf_are_refused() {
    assert!(matches!(refusal(b"not a pdf"), EmbedError::Read(_)));
}

#[test]
fn a_pdf_that_is_not_pdf_a_3_is_refused_naming_its_part() {
    for part in ["1", "2"] {
        let pdf = blank_with(&packet(part, ""), false, None);
        match refusal(&pdf) {
            EmbedError::NotPdfA3 { part: Some(p) } => assert_eq!(p, part),
            e => panic!("{e:?}"),
        }
    }
    let none = packet("3", "").replace(" pdfaid:part=\"3\"", "");
    assert!(matches!(
        refusal(&blank_with(&none, false, None)),
        EmbedError::NotPdfA3 { part: None }
    ));
}

#[test]
fn a_pdf_without_metadata_is_refused() {
    let mut doc = lopdf::Document::load_mem(&blank(SHAPES[0])).unwrap();
    let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    doc.get_dictionary_mut(root).unwrap().remove(b"Metadata");
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    assert!(matches!(refusal(&out), EmbedError::NoMetadata));
}

#[test]
fn a_filtered_metadata_stream_is_refused() {
    let pdf = blank_with(&packet("3", ""), false, Some("ASCIIHexDecode"));
    assert!(matches!(refusal(&pdf), EmbedError::FilteredMetadata));
}

#[test]
fn an_unreadable_xmp_packet_is_refused() {
    let pdf = blank_with(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF",
        false,
        None,
    );
    assert!(matches!(refusal(&pdf), EmbedError::Metadata(_)));
}

#[test]
fn an_encrypted_pdf_is_refused() {
    // The last trailer names an `/Encrypt` dictionary; nothing else is needed
    // for the refusal, which comes before any attempt to decrypt.
    let pdf = blank(SHAPES[0]);
    let at = pdf
        .windows(5)
        .rposition(|w| w == b"/Root")
        .expect("a trailer");
    let mut encrypted = pdf[..at].to_vec();
    encrypted.extend_from_slice(b"/Encrypt 99 0 R");
    encrypted.extend_from_slice(&pdf[at..]);
    assert!(matches!(refusal(&encrypted), EmbedError::Encrypted));
}

#[test]
fn existing_invoice_metadata_is_refused() {
    for ns in [
        "urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#",
        "urn:zugferd:pdfa:CrossIndustryDocument:invoice:2p0#",
        "urn:ferd:pdfa:invoice:rc#",
    ] {
        let extra = format!(
            "<rdf:Description rdf:about=\"\" xmlns:fx=\"{ns}\"><fx:DocumentType>INVOICE</fx:DocumentType></rdf:Description>"
        );
        match refusal(&blank_with(&packet("3", &extra), false, None)) {
            EmbedError::AlreadyHybrid(Existing::Xmp { namespace }) => assert_eq!(namespace, ns),
            e => panic!("{e:?}"),
        }
    }
}

#[test]
fn embedding_twice_is_refused() {
    let once = zugferd::embed(
        &blank(SHAPES[0]),
        &proof::<En16931>(&profiles::EN16931),
        &fixed(),
    )
    .unwrap();
    assert!(matches!(
        refusal(&once),
        EmbedError::AlreadyHybrid(Existing::Xmp { .. })
    ));
}

#[test]
fn an_existing_invoice_attachment_is_refused() {
    let mut doc = lopdf::Document::load_mem(&blank(SHAPES[0])).unwrap();
    let file = doc.add_object(Stream::new(
        dictionary! { "Type" => "EmbeddedFile" },
        b"<x/>".to_vec(),
    ));
    doc.add_object(dictionary! {
        "Type" => "Filespec",
        "F" => Object::string_literal("factur-x.xml"),
        "EF" => dictionary! { "F" => file },
    });
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    match refusal(&out) {
        EmbedError::AlreadyHybrid(Existing::Attachment { name }) => {
            assert_eq!(name, "factur-x.xml");
        }
        e => panic!("{e:?}"),
    }
}

// ── the reference corpus: real PDF/A-3 files, their invoice removed ───────

/// Every PDF under a `correct/` folder of the ZUGFeRD corpus.
fn corpus_pdfs(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, correct: bool, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                let c = correct || p.file_name().is_some_and(|n| n == "correct");
                walk(&p, c, out);
            } else if correct && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pdf")) {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(&root.join("zugferd-corpus"), false, &mut out);
    out.sort();
    out
}

/// The ZUGFeRD / Factur-X family, and the ZUGFeRD community's
/// "additional data" schema, which describes a further attachment — removed
/// with the others, so its metadata goes too.
fn is_invoice_namespace(uri: &str) -> bool {
    let uri = uri.trim().to_ascii_lowercase();
    uri.contains("crossindustrydocument")
        || uri.contains("zugferd-community.net/additional-data")
        || ["urn:ferd:", "urn:zugferd:", "urn:factur-x:"]
            .iter()
            .any(|p| uri.starts_with(p))
}

/// `pdf` without its invoice: `/AF`, the embedded-files tree, file
/// specifications and the ZUGFeRD / Factur-X XMP removed, and the file
/// rewritten. `None` when it does not parse or is not PDF/A-3.
fn without_invoice(pdf: &[u8]) -> Option<Vec<u8>> {
    let mut doc = lopdf::Document::load_mem(pdf).ok()?;
    let root = doc.trailer.get(b"Root").ok()?.as_reference().ok()?;
    let names = doc.get_dictionary(root).ok()?.get(b"Names").ok().cloned();
    if let Some(Object::Reference(id)) = names
        && let Ok(d) = doc.get_dictionary_mut(id)
    {
        d.remove(b"EmbeddedFiles");
    }
    let catalog = doc.get_dictionary_mut(root).ok()?;
    catalog.remove(b"AF");
    if let Ok(Object::Dictionary(d)) = catalog.get_mut(b"Names") {
        d.remove(b"EmbeddedFiles");
    }
    let meta = catalog.get(b"Metadata").ok()?.as_reference().ok()?;
    for object in doc.objects.values_mut() {
        let kind = match object {
            Object::Dictionary(d) => d.get(b"Type").ok().cloned(),
            Object::Stream(s) => s.dict.get(b"Type").ok().cloned(),
            _ => None,
        };
        if matches!(kind, Some(Object::Name(ref n)) if n == b"Filespec" || n == b"EmbeddedFile") {
            *object = Object::Null;
        }
    }
    // Annotations pointing at a removed file specification go too.
    for object in doc.objects.values_mut() {
        if let Object::Dictionary(d) = object
            && d.get(b"Subtype").ok().and_then(|s| s.as_name().ok()) == Some(b"FileAttachment")
        {
            *object = Object::Null;
        }
    }
    let stream = doc.get_object_mut(meta).ok()?.as_stream_mut().ok()?;
    let _ = stream.decompress();
    let text = String::from_utf8(stream.content.clone()).ok()?;
    let cleaned = strip_invoice_xmp(&text)?;
    if !cleaned.contains("pdfaid:part=\"3\"") && !cleaned.contains("pdfaid:part>3<") {
        return None;
    }
    stream.set_plain_content(cleaned.into_bytes());
    stream.allows_compression = false;
    let mut out = Vec::new();
    doc.save_to(&mut out).ok()?;
    Some(out)
}

fn strip_invoice_xmp(text: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(text).ok()?;
    let mut cut: Vec<std::ops::Range<usize>> = Vec::new();
    for n in doc.descendants().filter(roxmltree::Node::is_element) {
        if n.tag_name().namespace().is_some_and(is_invoice_namespace) {
            cut.push(n.range());
            continue;
        }
        for a in n.attributes() {
            if a.namespace().is_some_and(is_invoice_namespace) {
                cut.push(a.range());
            }
        }
        // An extension-schema item describing the invoice namespace.
        if n.tag_name().name() == "li"
            && n.children().any(|c| {
                c.tag_name().name() == "namespaceURI" && c.text().is_some_and(is_invoice_namespace)
            })
        {
            cut.push(n.range());
        }
    }
    // Outermost ranges only, applied from the end.
    cut.sort_by_key(|r| (r.start, std::cmp::Reverse(r.end)));
    let mut kept: Vec<std::ops::Range<usize>> = Vec::new();
    for r in cut {
        if kept.last().is_some_and(|k| r.start < k.end) {
            continue;
        }
        kept.push(r);
    }
    let mut out = text.to_owned();
    for r in kept.into_iter().rev() {
        out.replace_range(r, "");
    }
    // The namespace declarations, which carry no range of their own.
    let mut cleaned = String::with_capacity(out.len());
    let mut rest = out.as_str();
    while let Some(at) = rest.find("xmlns:") {
        let decl_end = rest[at..]
            .find('=')
            .and_then(|eq| {
                let quote = rest[at + eq + 1..].chars().next()?;
                let close = rest[at + eq + 2..].find(quote)?;
                Some(at + eq + 2 + close + 1)
            })
            .unwrap_or(at + 6);
        let decl = &rest[at..decl_end];
        let uri = decl
            .split_once('=')
            .map_or("", |(_, v)| v.trim_matches(|c| c == '"' || c == '\''));
        if is_invoice_namespace(uri) {
            cleaned.push_str(rest[..at].trim_end_matches(' '));
            cleaned.push(' ');
        } else {
            cleaned.push_str(&rest[..decl_end]);
        }
        rest = &rest[decl_end..];
    }
    cleaned.push_str(rest);
    Some(cleaned)
}

// ── veraPDF, in its official Docker image ─────────────────────────────────

/// veraPDF's REST service (1.30.3), pinned by tag and digest; linux/amd64 only.
const VERAPDF_IMAGE: &str = "verapdf/rest";
const VERAPDF_TAG: &str =
    "v1.30.3@sha256:b0e4ce6032521f87999e92562dc226a62da1fd30f2325998d1d2f2ed6eeb5d50";

/// The veraPDF container's memory limit (2 GiB) and the JVM heap inside it.
const VERAPDF_MEMORY: i64 = 2 * 1024 * 1024 * 1024;
const VERAPDF_HEAP: &str = "1g";

/// The label every gate container carries, so later runs can find leftovers.
const GATE_LABEL: (&str, &str) = ("en16931.test", "verapdf-gate");

struct VeraPdf {
    container: testcontainers::Container<testcontainers::GenericImage>,
    address: String,
}

/// The IDs of every container carrying [`GATE_LABEL`], running or not.
fn gate_containers() -> Result<Vec<String>, String> {
    let (key, value) = GATE_LABEL;
    let out = std::process::Command::new("docker")
        .args(["ps", "--all", "--quiet", "--filter"])
        .arg(format!("label={key}={value}"))
        .output()
        .map_err(|e| format!("the docker CLI: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "docker ps: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .map(str::to_owned)
        .collect())
}

/// Remove every gate container, including ones a killed run left behind
/// (testcontainers only removes on drop, and has no remove-by-label call).
/// Not safe beside a concurrently running gate.
fn reap_gate_containers() -> Result<usize, String> {
    let stale = gate_containers()?;
    if stale.is_empty() {
        return Ok(0);
    }
    let out = std::process::Command::new("docker")
        .args(["rm", "--force", "--volumes"])
        .args(&stale)
        .output()
        .map_err(|e| format!("the docker CLI: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "docker rm: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(stale.len())
}

/// One veraPDF verdict.
struct Verdict {
    compliant: bool,
    failures: Vec<String>,
}

impl VeraPdf {
    fn start() -> Result<Self, String> {
        use testcontainers::core::ContainerPort;
        use testcontainers::runners::SyncRunner as _;
        use testcontainers::{GenericImage, ImageExt as _};
        let stale = reap_gate_containers()?;
        if stale > 0 {
            eprintln!("veraPDF gate: removed {stale} container(s) an earlier, killed run left");
        }
        let container = GenericImage::new(VERAPDF_IMAGE, VERAPDF_TAG)
            .with_exposed_port(ContainerPort::Tcp(8080))
            .with_label(GATE_LABEL.0, GATE_LABEL.1)
            .with_platform("linux/amd64")
            .with_startup_timeout(Duration::from_secs(300))
            // Capped heap: unbounded, an emulated JVM sizes itself from the
            // whole Docker VM. Swap is held to the same limit.
            .with_env_var("JAVA_TOOL_OPTIONS", format!("-Xmx{VERAPDF_HEAP}"))
            .with_host_config_modifier(|host| {
                host.memory = Some(VERAPDF_MEMORY);
                host.memory_swap = Some(VERAPDF_MEMORY);
            })
            .start()
            .map_err(|e| e.to_string())?;
        let port = container
            .get_host_port_ipv4(8080)
            .map_err(|e| e.to_string())?;
        let host = container.get_host().map_err(|e| e.to_string())?;
        let me = Self {
            container,
            address: format!("{host}:{port}"),
        };
        let deadline = Instant::now() + Duration::from_secs(180);
        while me.request("GET", "/api/info", None).is_err() {
            if Instant::now() > deadline {
                return Err("veraPDF did not answer /api/info within 180 s".to_owned());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        Ok(me)
    }

    /// Remove the container and confirm no gate container is left; unlike
    /// dropping the handle, this returns a failure.
    fn stop(self) -> Result<(), String> {
        self.container.rm().map_err(|e| e.to_string())?;
        match gate_containers()?.as_slice() {
            [] => Ok(()),
            left => Err(format!("gate containers left after teardown: {left:?}")),
        }
    }

    /// A minimal HTTP/1.1 exchange over one connection: status 200's body.
    fn request(&self, method: &str, path: &str, pdf: Option<&[u8]>) -> Result<Vec<u8>, String> {
        let address = self.address.replace("localhost", "127.0.0.1");
        let mut s = std::net::TcpStream::connect(&address).map_err(|e| e.to_string())?;
        s.set_read_timeout(Some(Duration::from_secs(120)))
            .map_err(|e| e.to_string())?;
        const BOUNDARY: &str = "en16931-gate-boundary";
        let body = pdf.map(|bytes| {
            let mut b = format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"input.pdf\"\r\nContent-Type: application/pdf\r\n\r\n"
            )
            .into_bytes();
            b.extend_from_slice(bytes);
            b.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
            b
        });
        let mut head = format!(
            "{method} {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\n"
        );
        if let Some(b) = &body {
            head.push_str(&format!(
                "Content-Type: multipart/form-data; boundary={BOUNDARY}\r\nContent-Length: {}\r\n",
                b.len()
            ));
        }
        head.push_str("\r\n");
        s.write_all(head.as_bytes()).map_err(|e| e.to_string())?;
        if let Some(b) = &body {
            s.write_all(b).map_err(|e| e.to_string())?;
        }
        let mut response = Vec::new();
        s.read_to_end(&mut response).map_err(|e| e.to_string())?;
        let split = response
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or("no HTTP header")?;
        let header = String::from_utf8_lossy(&response[..split]).to_ascii_lowercase();
        if !header.starts_with("http/1.1 200") {
            return Err(header.lines().next().unwrap_or_default().to_owned());
        }
        let body = &response[split + 4..];
        Ok(if header.contains("transfer-encoding: chunked") {
            dechunk(body)
        } else {
            body.to_vec()
        })
    }

    fn validate(&self, pdf: &[u8]) -> Verdict {
        let body = self
            .request("POST", "/api/validate/3b", Some(pdf))
            .unwrap_or_else(|e| {
                // Name a container killed at its memory limit.
                if !self.container.is_running().unwrap_or(false) {
                    panic!(
                        "veraPDF: the container stopped (exit {:?}) — killed at its \
                         {VERAPDF_MEMORY}-byte memory limit? ({e})",
                        self.container.exit_code().ok().flatten()
                    );
                }
                panic!("veraPDF: {e}")
            });
        let v: serde_json::Value = serde_json::from_slice(&body).expect("veraPDF answers JSON");
        let result = &v["report"]["jobs"][0]["validationResult"][0];
        let failures = result["details"]["ruleSummaries"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|r| {
                format!(
                    "{}-{} {}",
                    r["clause"].as_str().unwrap_or("?"),
                    r["testNumber"],
                    r["description"].as_str().unwrap_or_default()
                )
            })
            .collect();
        Verdict {
            compliant: result["compliant"].as_bool() == Some(true),
            failures,
        }
    }
}

fn dechunk(mut body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(eol) = body.windows(2).position(|w| w == b"\r\n") {
        let size = usize::from_str_radix(
            String::from_utf8_lossy(&body[..eol])
                .split(';')
                .next()
                .unwrap_or("0")
                .trim(),
            16,
        )
        .unwrap_or(0);
        if size == 0 {
            break;
        }
        let start = eol + 2;
        out.extend_from_slice(&body[start..(start + size).min(body.len())]);
        body = &body[(start + size + 2).min(body.len())..];
    }
    out
}

/// Skip with a line naming what is missing — or fail, under `EN16931_REQUIRE_SPEC`.
fn skip(what: &str) {
    assert!(
        std::env::var_os(common::REQUIRE).is_none(),
        "the veraPDF gate needs {what}, and {} is set, so skipping is not permitted here",
        common::REQUIRE
    );
    eprintln!(
        "note: veraPDF gate SKIPPED — {what}. Set {}=1 to make this a failure.",
        common::REQUIRE
    );
}

/// Corpus inputs that pass veraPDF PDF/A-3b once their invoice is removed, at
/// the pinned corpus revision. Raise only.
const CORPUS_FLOOR: usize = 99;

/// The gate. Every constructed input and every PDF/A-3 file of the
/// reference corpus's ZUGFeRD samples (invoice removed) is embedded with an
/// EN 16931 and an XRechnung invoice; each output must read back with no
/// divergence and pass veraPDF PDF/A-3b with no failed rule.
#[test]
fn every_embedded_file_is_pdf_a_3b_by_verapdf() {
    let started = Instant::now();
    let vera = match VeraPdf::start() {
        Ok(v) => v,
        Err(e) => return skip(&format!("Docker and the {VERAPDF_IMAGE} image ({e})")),
    };
    let booted = started.elapsed();

    let mut inputs: Vec<(String, Vec<u8>)> = SHAPES
        .iter()
        .map(|s| (format!("constructed {s:?}"), blank(*s)))
        .collect();
    let constructed = inputs.len();
    let mut excluded = Vec::new();
    if let Some(root) = common::require("veraPDF gate (ZUGFeRD corpus)") {
        for path in corpus_pdfs(&root) {
            let rel = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .display()
                .to_string();
            let Some(stripped) = std::fs::read(&path).ok().and_then(|b| without_invoice(&b)) else {
                continue;
            };
            let verdict = vera.validate(&stripped);
            if verdict.compliant {
                inputs.push((rel, stripped));
            } else {
                excluded.push((rel, verdict.failures));
            }
        }
        let corpus = inputs.len() - constructed;
        eprintln!(
            "veraPDF gate: {corpus} corpus inputs, {} excluded (not PDF/A-3b once stripped): {excluded:#?}",
            excluded.len()
        );
        assert!(
            corpus >= CORPUS_FLOOR,
            "only {corpus} corpus inputs pass veraPDF once stripped (floor {CORPUS_FLOOR})"
        );
    }

    let en = proof::<En16931>(&profiles::EN16931);
    let xr = proof::<XRechnung>(&profiles::XRECHNUNG);
    let mut failed = Vec::new();
    let mut checked = 0;
    for (origin, input) in &inputs {
        // A constructed input must itself pass veraPDF, or the results are
        // meaningless (corpus inputs were checked when admitted).
        if origin.starts_with("constructed") {
            let v = vera.validate(input);
            assert!(v.compliant, "{origin} as constructed: {:?}", v.failures);
        }
        let outputs = [
            ("EN 16931", zugferd::embed(input, &en, &fixed())),
            ("XRechnung", zugferd::embed(input, &xr, &fixed())),
        ];
        for (profile, out) in outputs {
            let out = out.unwrap_or_else(|e| panic!("{origin} ({profile}): {e}"));
            if profile == "EN 16931" {
                assert_embedded(input, &out, &en, origin);
            } else {
                assert_embedded(input, &out, &xr, origin);
            }
            let v = vera.validate(&out);
            checked += 1;
            if !v.compliant {
                failed.push((format!("{origin} ({profile})"), v.failures));
            }
        }
    }
    eprintln!(
        "veraPDF gate: {checked} embedded files checked, {} failed; container ready in {:.1?}, gate {:.1?}",
        failed.len(),
        booted,
        started.elapsed()
    );
    vera.stop().expect("the veraPDF container is removed");
    assert!(failed.is_empty(), "not PDF/A-3b after embed: {failed:#?}");
    assert_eq!(checked, 2 * inputs.len());
}

/// The image CI pulls is the image the gate runs: one pin, written twice,
/// asserted equal.
#[test]
fn ci_pulls_the_pinned_verapdf_image() {
    let ci = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/ci.yml"),
    )
    .expect("the CI workflow");
    assert!(
        ci.contains(&format!("{VERAPDF_IMAGE}:{VERAPDF_TAG}")),
        "ci.yml does not pull {VERAPDF_IMAGE}:{VERAPDF_TAG}"
    );
}
