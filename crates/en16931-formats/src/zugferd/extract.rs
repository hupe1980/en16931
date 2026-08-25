//! Getting the invoice out of the PDF.
//!
//! # Why this direction first
//!
//! Receiving is more common than sending, and reading carries none of the
//! risk. Extraction is: parse the cross-reference table, walk the document
//! catalogue to `/Names/EmbeddedFiles`, and inflate one stream. No rendering,
//! no fonts, no text layout, and — critically — no chance of breaking the
//! PDF/A-3 conformance the file already had.
//!
//! # The name lookup, and why it is a list
//!
//! ⚠ ZUGFeRD 2.1 and Factur-X use `factur-x.xml`; ZUGFeRD 2.0 used
//! `zugferd-invoice.xml`; ZUGFeRD 1.0 used `ZUGFeRD-invoice.xml`. A *reader*
//! must accept all of them — it does not choose what arrives — while a writer
//! picks one. That asymmetry is why [`FILENAMES`] exists as a list rather than
//! a constant, and the names are ⚠ until checked against a fetched
//! specification.
//!
//! Matching is case-insensitive because the specification's casing and real
//! producers' casing have not historically agreed.

use std::collections::BTreeMap;

use super::{Error, IsInvoice, Profile};

/// Embedded filenames a ZUGFeRD or Factur-X invoice may use. ⚠
///
/// Ordered by preference: a file carrying two of them — which should not
/// happen, and does — resolves to the most recent convention.
pub const FILENAMES: &[&str] = &[
    "factur-x.xml",
    "zugferd-invoice.xml",
    "xrechnung.xml",
    "order-x.xml",
];

/// What the PDF's XMP metadata declares about the invoice inside it.
///
/// This is how a receiver discovers that an invoice is there, and which profile
/// it claims, **before parsing anything**. Getting it wrong yields a file that
/// opens fine and that no counterparty detects as an e-invoice — which is why
/// it is read and reported rather than ignored as decoration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Xmp {
    /// `fx:DocumentType` — `INVOICE` or `ORDER`. ⚠
    pub document_type: Option<String>,
    /// `fx:DocumentFileName` — the embedded file's name, as the metadata
    /// claims it. May disagree with the file actually attached. ⚠
    pub document_filename: Option<String>,
    /// `fx:Version` — the version of the Factur-X **XMP schema**, *not* of
    /// ZUGFeRD.
    ///
    /// Constant `1.0` since Factur-X 1.0. A ZUGFeRD 2.3 file carries `1.0` here,
    /// and so does every other conforming file: the reference implementation
    /// hardcodes it beside the producer string and the timestamp, and never
    /// derives it from the profile or the document version.
    ///
    /// ```python
    /// # akretion/factur-x, src/facturx/facturx.py, _prepare_pdf_metadata_xml
    /// key2value = { …, "version": "1.0", … }
    /// ```
    ///
    /// Read as *"the ZUGFeRD / Factur-X version"* it invites exactly the wrong
    /// check: code comparing it against `"2.3"` rejects every conforming
    /// ZUGFeRD 2.3 file. No XMP property carries the ZUGFeRD version — the
    /// profile is [`conformance_level`](Self::conformance_level), and the
    /// document version is only in the payload's BT-24.
    pub version: Option<String>,
    /// `fx:ConformanceLevel` — the profile, in the XMP's own vocabulary. ⚠
    pub conformance_level: Option<String>,
    /// `pdfaid:part` — which part of ISO 19005 the file claims.
    ///
    /// Not a ZUGFeRD property, and read anyway: **`3` is the only answer a
    /// hybrid invoice may give.** PDF/A-1 and PDF/A-2 forbid embedding a file
    /// of arbitrary type at all, so a PDF declaring either is not a conforming
    /// ZUGFeRD or Factur-X document however good the rest of its metadata is.
    /// See [`Divergence::NotPdfA3`].
    pub pdfa_part: Option<String>,
    /// `pdfaid:conformance` — `A`, `B` or `U`.
    ///
    /// All three are permitted; this is carried for diagnosis rather than
    /// checked.
    pub pdfa_conformance: Option<String>,
}

/// A disagreement between what the PDF declares and what it contains.
///
/// Each of these is a document that validates, opens, and is wrong in a way no
/// schema notices. They are warnings rather than errors: the payload is still
/// readable, and refusing an invoice over a metadata mismatch would be worse
/// than reporting it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Divergence {
    /// The XMP names a different profile than the payload's BT-24.
    ///
    /// A receiver that routes on the XMP and a receiver that routes on BT-24
    /// then process the same file differently — and both are behaving
    /// correctly.
    Profile {
        /// What `fx:ConformanceLevel` says.
        xmp: String,
        /// What BT-24 says.
        payload: String,
    },
    /// The XMP names a different embedded filename than the one attached.
    Filename {
        /// What `fx:DocumentFileName` says.
        xmp: String,
        /// The file actually found.
        actual: String,
    },
    /// A full-invoice profile is attached as `/AFRelationship /Data`.
    ///
    /// `Data` says the XML is *supplementary* to a PDF that is itself the
    /// invoice. That is right for MINIMUM and BASIC WL, which carry no lines
    /// and are booking aids. For BASIC, EN 16931, EXTENDED and XRECHNUNG the
    /// XML **is** the invoice, and every published source agrees `Data` is
    /// wrong there — while disagreeing on the replacement (`Alternative` in
    /// German guidance, `Source` in PDFlib's for Factur-X abroad).
    ///
    /// So this reports the case they agree on and stays silent on the rest. A
    /// receiver that routes on `/AFRelationship` may treat such a file as a
    /// PDF with an attachment rather than as an e-invoice.
    Relationship {
        /// The value found.
        found: String,
        /// The profile the payload claims, which is what makes it wrong.
        profile: Profile,
    },
    /// The PDF carries no XMP invoice metadata at all.
    ///
    /// Readable here, because the embedded file was found by name — but a
    /// counterparty scanning metadata first will not see an e-invoice.
    NoXmp,
    /// The invoice is not listed in the document catalogue's `/AF` array.
    ///
    /// `/AF` is what makes an embedded file an **associated** file, and it is
    /// the key a PDF/A-3-aware receiver reads first. Without it the XML is an
    /// ordinary attachment: the file opens, a human sees the pages, and an
    /// automated pipeline that asks the catalogue what is associated with this
    /// document is told *nothing*.
    ///
    /// The commonest defect in the wild, because every PDF library can attach a
    /// file and only some can associate one.
    NotAssociated,
    /// The invoice is not reachable from `/Names/EmbeddedFiles`.
    ///
    /// The other half of the same requirement, and the half that older tools
    /// use: ZUGFeRD asks for the payload in the catalogue's embedded-files name
    /// tree so that readers with no PDF/A-3 support still find it. Reached here
    /// through a page annotation or a bare file specification instead.
    NotInEmbeddedFiles,
    /// The invoice's file specification carries no `/AFRelationship`.
    ///
    /// Lawful PDF, and a signal in itself: the file was attached by something
    /// that does not know it is attaching an invoice. A receiver routing on the
    /// relationship cannot tell whether the XML **is** the invoice or merely
    /// accompanies one — which is the distinction the key exists for.
    NoRelationship,
    /// The PDF does not claim PDF/A-3.
    ///
    /// ZUGFeRD 2.x and Factur-X both make PDF/A-3 normative, and it is not
    /// decoration: parts 1 and 2 of ISO 19005 **forbid** embedding a file of
    /// arbitrary type, so a document claiming either is self-contradictory, and
    /// one claiming nothing has no conformance to lose but also none to offer.
    /// A recipient that runs veraPDF on what arrives will reject it.
    NotPdfA3 {
        /// `pdfaid:part`, when the file states one at all.
        part: Option<String>,
    },
}

impl std::fmt::Display for Divergence {
    /// One sentence naming both halves of the disagreement.
    ///
    /// A diagnostic type with no `Display` is a diagnostic nobody prints: every
    /// caller ends up with `{:?}`, which puts Rust syntax in front of an
    /// operator. These are the sentences a person needs, so they live here
    /// rather than being rewritten at each call site.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Profile { xmp, payload } => write!(
                f,
                "the PDF's XMP declares profile {xmp:?} and the payload's BT-24 says {payload:?} — \
                 a receiver routing on the metadata and one routing on the invoice will disagree"
            ),
            Self::Filename { xmp, actual } => write!(
                f,
                "the PDF's XMP names the embedded file {xmp:?} and the attached file is {actual:?}"
            ),
            Self::Relationship { found, profile } => write!(
                f,
                "the invoice is attached as /AFRelationship /{found}, but {profile:?} carries the \
                 invoice itself rather than accompanying one — /Data says the pages are the invoice"
            ),
            Self::NoXmp => f.write_str(
                "the PDF carries no XMP invoice metadata; the payload was found by filename, and \
                 a counterparty scanning metadata first will not see an e-invoice here",
            ),
            Self::NotAssociated => f.write_str(
                "the invoice is not in the document catalogue's /AF array, so it is an ordinary \
                 attachment rather than an associated file — a PDF/A-3 receiver asking what is \
                 associated with this document is told nothing",
            ),
            Self::NotInEmbeddedFiles => f.write_str(
                "the invoice is not reachable from /Names/EmbeddedFiles, where ZUGFeRD requires \
                 it so that readers without PDF/A-3 support still find it",
            ),
            Self::NoRelationship => f.write_str(
                "the invoice's file specification carries no /AFRelationship, so nothing in the \
                 PDF says whether the XML is the invoice or merely accompanies one",
            ),
            Self::NotPdfA3 { part } => match part {
                Some(p) => write!(
                    f,
                    "the PDF claims PDF/A-{p}; ZUGFeRD and Factur-X require PDF/A-3, and parts 1 \
                     and 2 forbid embedding a file of arbitrary type at all"
                ),
                None => f.write_str(
                    "the PDF claims no PDF/A conformance; ZUGFeRD and Factur-X require PDF/A-3, \
                     and a recipient running veraPDF on what arrives will reject it",
                ),
            },
        }
    }
}

/// What extraction produced.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Extracted {
    /// The payload, **verbatim**.
    ///
    /// Whoever is diagnosing a rejected invoice needs the bytes the
    /// counterparty actually sent, not this crate's reconstruction of them.
    pub xml: String,
    /// The embedded file's name, as the PDF records it.
    pub filename: String,
    /// The profile the document claims, read from its BT-24.
    ///
    /// A *claim*, not a finding. What a document says it is and what it is are
    /// different questions, and the gap between them is a real diagnostic.
    pub profile: Profile,
    /// BT-24 verbatim, when the payload carries one.
    pub specification_id: Option<String>,
    /// The payload as the semantic model, when the `cii` feature is on.
    ///
    /// `None` means the payload could not be read *as CII* — which for a
    /// ZUGFeRD file means something is wrong with it, and [`Extracted::xml`] is
    /// where to look. Reporting that rather than erroring keeps the bytes
    /// available for diagnosis.
    #[cfg(feature = "cii")]
    #[cfg_attr(docsrs, doc(cfg(feature = "cii")))]
    pub invoice: Option<en16931::Invoice>,
    /// Elements in the payload outside the EN 16931 subset, and values present
    /// but not representable. Empty for a conforming document.
    #[cfg(feature = "cii")]
    #[cfg_attr(docsrs, doc(cfg(feature = "cii")))]
    pub syntax_findings: Vec<String>,
    /// `/AFRelationship` for the embedded invoice, verbatim.
    ///
    /// `None` when the attachment carries none — lawful in PDF, and a signal in
    /// itself that the file was produced by a generic attacher rather than by
    /// something that knows about hybrid invoices. See [`Divergence::Relationship`].
    pub relationship: Option<String>,
    /// What the PDF's own metadata declares, independently of the payload.
    pub xmp: Xmp,
    /// Disagreements between the metadata and the payload. Usually empty.
    pub divergence: Vec<Divergence>,
}

/// Every embedded file in a PDF, by name.
///
/// Exposed because "no invoice found" is much easier to act on when the caller
/// can see what *was* attached — and because a PDF carrying a differently named
/// invoice is the common support question.
pub fn embedded_files(pdf: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    Ok(collect_embedded(&lopdf::Document::load_mem(pdf)?))
}

/// The embedded files of an already-parsed document.
///
/// Split out so [`extract`] parses the PDF once rather than twice — it needs
/// both the attachments and the XMP.
fn collect_embedded(doc: &lopdf::Document) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();

    // Two places hold embedded files, and real documents use both:
    //   * the catalogue's /Names/EmbeddedFiles name tree — where PDF/A-3 and
    //     the ZUGFeRD specification say it belongs, and
    //   * a page's /Annots as a /FileAttachment annotation.
    // Reading only the first is the common bug, and it fails on exactly the
    // files a user reports as "works in every other viewer".
    for object in doc.objects.values() {
        let Ok(dict) = object.as_dict() else { continue };
        let is_filespec = dict
            .get(b"Type")
            .ok()
            .and_then(|t| t.as_name().ok())
            .is_some_and(|n| n == b"Filespec");
        if !is_filespec {
            continue;
        }
        let Some(name) = filespec_name(dict) else {
            continue;
        };
        let Some(bytes) = filespec_bytes(doc, dict) else {
            continue;
        };
        out.insert(name, bytes);
    }
    out
}

/// Where the PDF puts the invoice, structurally.
///
/// Three independent questions with three independent answers, which is exactly
/// why they are three fields: a file can be an associated file and not in the
/// name tree, or in the name tree with no relationship, and each combination
/// fails a different receiver.
#[derive(Debug, Default)]
struct Placement {
    /// Referenced from the document catalogue's `/AF` array.
    associated: bool,
    /// Reachable from the catalogue's `/Names/EmbeddedFiles` name tree.
    in_name_tree: bool,
    /// `/AFRelationship`, verbatim.
    relationship: Option<String>,
}

/// How the file specification for `filename` is wired into the document.
///
/// One pass over the objects rather than three: the same file specification
/// answers all three questions, and finding it three times invites the three
/// answers to be about three different objects.
fn placement(doc: &lopdf::Document, filename: &str) -> Placement {
    let Some((id, dict)) = doc.objects.iter().find_map(|(id, object)| {
        let dict = object.as_dict().ok()?;
        (dict.get(b"Type").ok()?.as_name().ok()? == b"Filespec"
            && filespec_name(dict).is_some_and(|n| n.eq_ignore_ascii_case(filename)))
        .then_some((*id, dict))
    }) else {
        return Placement::default();
    };
    Placement {
        associated: catalog_associated_files(doc).contains(&id),
        in_name_tree: embedded_file_names(doc)
            .iter()
            .any(|n| n.eq_ignore_ascii_case(filename)),
        relationship: dict
            .get(b"AFRelationship")
            .ok()
            .and_then(|o| o.as_name().ok())
            .map(|raw| String::from_utf8_lossy(raw).into_owned()),
    }
}

/// The object ids in the document catalogue's `/AF` array.
///
/// Empty when there is no `/AF` at all, which is the common defect rather than
/// an exotic one — most PDF libraries can attach a file and fewer can associate
/// one.
fn catalog_associated_files(doc: &lopdf::Document) -> Vec<lopdf::ObjectId> {
    let Ok(catalog) = doc.catalog() else {
        return Vec::new();
    };
    let Ok(af) = catalog.get(b"AF").and_then(lopdf::Object::as_array) else {
        return Vec::new();
    };
    af.iter()
        .filter_map(|o| match o {
            lopdf::Object::Reference(id) => Some(*id),
            _ => None,
        })
        .collect()
}

/// Every name in the catalogue's `/Names/EmbeddedFiles` name tree.
///
/// A name tree is a *tree*: a small document holds its entries in one `/Names`
/// array, and a large one splits them across `/Kids`. Reading only the flat
/// case is the bug that makes a big invoice archive look empty.
fn embedded_file_names(doc: &lopdf::Document) -> Vec<String> {
    /// A name tree deep enough to need this is already malformed; the bound is
    /// what makes a cyclic `/Kids` a wrong answer rather than a hung process.
    const MAX_DEPTH: usize = 32;

    fn walk(doc: &lopdf::Document, node: &lopdf::Dictionary, depth: usize, out: &mut Vec<String>) {
        if depth > MAX_DEPTH {
            return;
        }
        if let Ok(names) = node.get(b"Names").and_then(lopdf::Object::as_array) {
            // `[name value name value …]` — the names are the even positions.
            for entry in names.iter().step_by(2) {
                if let Ok(raw) = entry.as_str() {
                    out.push(String::from_utf8_lossy(raw).into_owned());
                }
            }
        }
        if let Ok(kids) = node.get(b"Kids").and_then(lopdf::Object::as_array) {
            for kid in kids {
                if let Ok(dict) = doc.dereference(kid).and_then(|(_, o)| o.as_dict()) {
                    walk(doc, dict, depth + 1, out);
                }
            }
        }
    }

    let mut out = Vec::new();
    if let Ok(catalog) = doc.catalog()
        && let Ok(names) = catalog.get(b"Names").and_then(lopdf::Object::as_dict)
        && let Ok(tree) = names
            .get(b"EmbeddedFiles")
            .and_then(|o| doc.dereference(o))
            .and_then(|(_, o)| o.as_dict())
    {
        walk(doc, tree, 0, &mut out);
    }
    out
}

/// A `/Filespec`'s name, preferring `/UF` (Unicode) over `/F` (byte string).
///
/// `/Desc` is **not** consulted: it is a human-readable description, not a
/// name, and falling back to it would extract an attachment *described* as
/// "factur-x.xml" whatever it is actually named — name confusion in the one
/// place where the name decides which bytes a receiver treats as the
/// document.
fn filespec_name(dict: &lopdf::Dictionary) -> Option<String> {
    for key in [&b"UF"[..], &b"F"[..]] {
        if let Ok(obj) = dict.get(key)
            && let Ok(s) = obj.as_str()
        {
            return Some(String::from_utf8_lossy(s).into_owned());
        }
    }
    None
}

/// The decompressed contents of a `/Filespec`'s embedded stream.
fn filespec_bytes(doc: &lopdf::Document, dict: &lopdf::Dictionary) -> Option<Vec<u8>> {
    let ef = dict.get(b"EF").ok()?.as_dict().ok()?;
    // `/F` is the usual key; `/UF` appears alongside it in files produced for
    // Unicode-aware readers and is the same stream.
    let stream_ref = ef.get(b"F").or_else(|_| ef.get(b"UF")).ok()?;
    let stream = match stream_ref {
        lopdf::Object::Reference(id) => doc.get_object(*id).ok()?.as_stream().ok()?,
        other => other.as_stream().ok()?,
    };
    // `get_plain_content` returns the raw bytes when the stream is not
    // compressed, which several producers' output is.
    stream
        .decompressed_content()
        .ok()
        .or_else(|| Some(stream.content.clone()))
}

/// Extract the invoice from a ZUGFeRD / Factur-X PDF.
///
/// # Errors
///
/// [`Error::Pdf`] if the bytes are not a readable PDF, [`Error::NoInvoice`] if
/// it carries no embedded file under a name this crate recognises — with the
/// names it *does* carry, so the caller can say something useful — and
/// [`Error::Encoding`] if the payload is not UTF-8.
pub fn extract(pdf: &[u8]) -> Result<Extracted, Error> {
    let doc = lopdf::Document::load_mem(pdf)?;
    let xmp = read_xmp(&doc);
    let mut files = collect_embedded(&doc);

    // Preference order, not first match: a file carrying both `factur-x.xml`
    // and `zugferd-invoice.xml` resolves to the newer convention rather than to
    // whichever the PDF happened to list first.
    let wanted = FILENAMES
        .iter()
        .find_map(|want| files.keys().find(|k| k.eq_ignore_ascii_case(want)).cloned());

    // `remove_entry` rather than `get`: the payload is taken out and owned
    // without a clone, and no lookup remains that could fail — so this function
    // has no panicking path at all, which is worth more than a `# Panics`
    // section explaining one.
    let Some((filename, bytes)) = wanted.and_then(|f| files.remove_entry(&f)) else {
        return Err(Error::NoInvoice {
            looked_for: FILENAMES,
            found: files.into_keys().collect(),
        });
    };
    let xml = String::from_utf8(bytes)?;
    let specification_id = specification_id(&xml);
    let profile = specification_id
        .as_deref()
        .map_or(Profile::Unknown, Profile::parse);

    // The payload is CII, and `crate::cii` is the crate's own reader — so
    // `extract` returns the model rather than a string the caller must find a
    // parser for. Without the feature it returns the bytes and says so.
    #[cfg(feature = "cii")]
    let (invoice, syntax_findings) = match crate::cii::from_str(&xml) {
        Ok(r) => {
            let mut findings = r.unmapped;
            findings.extend(r.malformed);
            (Some(r.invoice), findings)
        }
        Err(e) => (None, vec![e.to_string()]),
    };

    let placement = placement(&doc, &filename);
    let relationship = placement.relationship.clone();

    Ok(Extracted {
        divergence: diverge(
            &xmp,
            &filename,
            profile,
            specification_id.as_deref(),
            &placement,
        ),
        #[cfg(feature = "cii")]
        invoice,
        #[cfg(feature = "cii")]
        syntax_findings,
        xml,
        filename,
        profile,
        specification_id,
        relationship,
        xmp,
    })
}

/// Compare what the PDF declares against what it contains.
fn diverge(
    xmp: &Xmp,
    filename: &str,
    profile: Profile,
    specification_id: Option<&str>,
    placement: &Placement,
) -> Vec<Divergence> {
    let mut out = Vec::new();

    // ── how the file is wired in ─────────────────────────────────────────────
    //
    // Checked before the XMP, and independently of it: a file can carry perfect
    // metadata and still be attached in a way that no receiver recognises. Each
    // of these is a document that opens, renders and is wrong in a way no PDF
    // reader complains about.
    if !placement.associated {
        out.push(Divergence::NotAssociated);
    }
    if !placement.in_name_tree {
        out.push(Divergence::NotInEmbeddedFiles);
    }
    match placement.relationship.as_deref() {
        None => out.push(Divergence::NoRelationship),
        // Only the case every source agrees on — `Data` on a profile that has
        // lines. Where the sources disagree, this crate takes no position; see
        // `Divergence::Relationship`.
        Some(found)
            if found.eq_ignore_ascii_case("Data")
                && profile.is_en16931_invoice() == IsInvoice::Yes =>
        {
            out.push(Divergence::Relationship {
                found: found.to_owned(),
                profile,
            });
        }
        Some(_) => {}
    }

    // ── what the metadata says ───────────────────────────────────────────────
    if *xmp == Xmp::default() {
        // A packet with nothing in it cannot state a PDF/A part either, and
        // saying so twice adds no information.
        out.push(Divergence::NoXmp);
        return out;
    }
    if xmp.pdfa_part.as_deref() != Some("3") {
        out.push(Divergence::NotPdfA3 {
            part: xmp.pdfa_part.clone(),
        });
    }
    if let Some(level) = &xmp.conformance_level {
        let claimed = Profile::parse(level);
        // Compare *profiles*, not strings: the XMP writes `EN 16931` where
        // BT-24 writes a URN, and they mean the same thing. Comparing the
        // literals would report a divergence on every conforming document.
        if claimed != Profile::Unknown && claimed != profile {
            out.push(Divergence::Profile {
                xmp: level.clone(),
                payload: specification_id.unwrap_or("<absent>").to_owned(),
            });
        }
    }
    if let Some(declared) = &xmp.document_filename
        && !declared.eq_ignore_ascii_case(filename)
    {
        out.push(Divergence::Filename {
            xmp: declared.clone(),
            actual: filename.to_owned(),
        });
    }
    out
}

/// The document-level XMP packet, if there is one.
///
/// Read with substring extraction rather than an XML parser, for the same
/// reason [`specification_id`] is: this crate does not own an XML binding, and
/// pulling in a parser to read four fields would be the beginning of owning
/// one. XMP is RDF, and a *general* reader of it would need one — but these
/// four elements are flat text in a namespace nothing else uses.
fn read_xmp(doc: &lopdf::Document) -> Xmp {
    let Some(packet) = xmp_packet(doc) else {
        return Xmp::default();
    };
    Xmp {
        document_type: xmp_field(&packet, "DocumentType"),
        document_filename: xmp_field(&packet, "DocumentFileName"),
        version: xmp_field(&packet, "Version"),
        conformance_level: xmp_field(&packet, "ConformanceLevel"),
        pdfa_part: xmp_field(&packet, "part"),
        pdfa_conformance: xmp_field(&packet, "conformance"),
    }
}

fn xmp_packet(doc: &lopdf::Document) -> Option<String> {
    let catalog = doc.catalog().ok()?;
    let meta = catalog.get(b"Metadata").ok()?;
    let stream = match meta {
        lopdf::Object::Reference(id) => doc.get_object(*id).ok()?.as_stream().ok()?,
        other => other.as_stream().ok()?,
    };
    let bytes = stream
        .decompressed_content()
        .unwrap_or_else(|_| stream.content.clone());
    // XMP is required to be UTF-8; `from_utf8_lossy` rather than a failure
    // because a mangled metadata packet must not stop a readable invoice
    // being extracted.
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// One namespaced property's value, in either of the two shapes RDF allows.
///
/// The prefix is matched loosely — `fx:`, `zf:` and `pdfaExtension`-declared
/// variants all appear in the wild — by looking for the local name preceded by
/// a colon.
///
/// Both shapes, because both occur and mean the same thing. XMP is RDF, and RDF
/// lets a simple property be an element or an attribute:
///
/// ```xml
/// <rdf:Description pdfaid:part="3">          <!-- attribute form -->
/// <rdf:Description><pdfaid:part>3</pdfaid:part>  <!-- element form -->
/// ```
///
/// The Factur-X reference implementation writes `fx:*` as elements and the
/// PDF/A identification as attributes, and other producers do the reverse.
/// Reading only one shape means reading only some producers' files.
fn xmp_field(packet: &str, local_name: &str) -> Option<String> {
    element(packet, local_name).or_else(|| attribute(packet, local_name))
}

/// `<ns:Name>value</…>`.
fn element(packet: &str, local_name: &str) -> Option<String> {
    let needle = format!(":{local_name}>");
    let start = packet.find(&needle)? + needle.len();
    let rest = &packet[start..];
    let end = rest.find("</")?;
    let value = rest[..end].trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// `ns:Name="value"`.
fn attribute(packet: &str, local_name: &str) -> Option<String> {
    let needle = format!(":{local_name}=\"");
    let start = packet.find(&needle)? + needle.len();
    let rest = &packet[start..];
    let end = rest.find('"')?;
    let value = rest[..end].trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// BT-24 from a CII payload, without a parser.
///
/// The element is `ram:GuidelineSpecifiedDocumentContextParameter/ram:ID`, and
/// finding it is a substring search rather than a parse on purpose: this crate
/// does not own the CII binding, and pulling in an XML parser to read one
/// element would be the beginning of owning it. `xrechnung` is where a real CII
/// reader belongs; here the identifier is metadata about the payload, and the
/// payload is handed back verbatim for whoever does parse it.
fn specification_id(xml: &str) -> Option<String> {
    let anchor = xml.find("GuidelineSpecifiedDocumentContextParameter")?;
    let rest = &xml[anchor..];
    let open = rest.find(":ID>").map(|i| i + 4)?;
    let close = rest[open..].find("</")?;
    let value = rest[open..open + close].trim();
    (!value.is_empty()).then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bt24_is_found_in_a_cii_fragment() {
        let xml = r"<rsm:ExchangedDocumentContext>
          <ram:GuidelineSpecifiedDocumentContextParameter>
            <ram:ID>urn:cen.eu:en16931:2017</ram:ID>
          </ram:GuidelineSpecifiedDocumentContextParameter>
        </rsm:ExchangedDocumentContext>";
        assert_eq!(
            specification_id(xml).as_deref(),
            Some("urn:cen.eu:en16931:2017")
        );
    }

    /// The *guideline* parameter, not the business-process one that precedes it.
    #[test]
    fn bt23_is_not_mistaken_for_bt24() {
        let xml = r"<ram:BusinessProcessSpecifiedDocumentContextParameter>
            <ram:ID>urn:process</ram:ID>
          </ram:BusinessProcessSpecifiedDocumentContextParameter>
          <ram:GuidelineSpecifiedDocumentContextParameter>
            <ram:ID>urn:factur-x.eu:1p0:basic</ram:ID>
          </ram:GuidelineSpecifiedDocumentContextParameter>";
        assert_eq!(
            specification_id(xml).as_deref(),
            Some("urn:factur-x.eu:1p0:basic")
        );
    }

    #[test]
    fn a_payload_without_bt24_reports_none() {
        assert_eq!(specification_id("<rsm:CrossIndustryInvoice/>"), None);
        assert_eq!(
            specification_id("<ram:GuidelineSpecifiedDocumentContextParameter/>"),
            None
        );
    }

    #[test]
    fn bytes_that_are_not_a_pdf_say_so() {
        assert!(matches!(extract(b"not a pdf"), Err(Error::Pdf(_))));
    }
}
