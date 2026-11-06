//! Deriving element order from the schemas: CEN's D16B subset for CII, OASIS
//! UBL 2.1 for UBL.
//!
//! Each parent element the authorities' instances show gets the whole
//! `xsd:sequence` of its type, not only the children seen: an element the
//! corpus never carries would otherwise have no place, and the writer would
//! drop it. Fails if a parent's type cannot be resolved, if one element name
//! has two different sequences, or if an observed child is not in its parent's
//! sequence. (Instances alone get the order wrong: KoSIT's mutation instances
//! outvote valid ones, and mutually exclusive elements never co-occur.)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::{Generated, Table, escape, header, local};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Syntax {
    Ubl,
    Cii,
}

impl Syntax {
    /// The document elements that start a document in this syntax.
    fn roots(self) -> &'static [&'static str] {
        match self {
            // Invoice and CreditNote disagree on where `cbc:TaxPointDate` goes.
            Self::Ubl => &["Invoice", "CreditNote"],
            Self::Cii => &["CrossIndustryInvoice"],
        }
    }

    fn module(self) -> &'static str {
        match self {
            Self::Ubl => "ubl",
            Self::Cii => "cii",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Ubl => "UBL 2.1",
            Self::Cii => "CII D16B",
        }
    }
}

/// UBL's own namespaces: the two document elements' and the aggregates'.
/// Anything beneath `ext:UBLExtensions` is a foreign vocabulary.
const UBL_NAMESPACES: &[&str] = &[
    "urn:oasis:names:specification:ubl:schema:xsd:Invoice-2",
    "urn:oasis:names:specification:ubl:schema:xsd:CreditNote-2",
    "urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2",
];

/// The children each parent element is seen with, in first-seen order.
#[derive(Default)]
struct Observations {
    names: Table,
}

impl Observations {
    fn walk(&mut self, syntax: Syntax, node: roxmltree::Node<'_, '_>) {
        if syntax == Syntax::Ubl
            && !node
                .tag_name()
                .namespace()
                .is_some_and(|ns| UBL_NAMESPACES.contains(&ns))
        {
            return;
        }
        let parent = node.tag_name().name();
        for c in node.children().filter(roxmltree::Node::is_element) {
            let names = self.names.entry(parent.to_owned()).or_default();
            let k = c.tag_name().name();
            if !names.iter().any(|n| n == k) {
                names.push(k.to_owned());
            }
            self.walk(syntax, c);
        }
    }
}

/// Derive the order table for `syntax`: each parent element the corpus shows,
/// with the whole `xsd:sequence` of its type.
///
/// The schema gives the order and every child the type allows, so an element
/// no instance happens to carry still has its place. The instances choose the
/// parents and are checked against the schema.
///
/// # Errors
///
/// No instance of this syntax was found, the schema is unreadable, an observed
/// parent has no sequence, or an instance carries a child its parent's
/// sequence does not contain.
pub fn derive(
    syntax: Syntax,
    files: &[PathBuf],
    spec: &Path,
    root: &Path,
) -> Result<Generated, String> {
    let mut obs = Observations::default();
    let mut instances = 0usize;

    for path in files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(doc) = roxmltree::Document::parse(&text) else {
            continue; // the corpus deliberately contains malformed documents
        };
        let name = doc.root_element().tag_name().name();
        if !syntax.roots().contains(&name) {
            continue;
        }
        instances += 1;
        obs.walk(syntax, doc.root_element());
    }

    if instances == 0 {
        return Err(format!(
            "no {} instances found under the reference corpus",
            syntax.label()
        ));
    }

    let sequences = match syntax {
        Syntax::Cii => d16b_sequences(&spec.join(D16B))?,
        Syntax::Ubl => crate::once::sequences(syntax, spec)?,
    };
    let label = syntax.label();
    let mut table: Vec<(String, Vec<String>)> = Vec::new();
    for (parent, kids) in &obs.names {
        let seq = sequences
            .get(parent.as_str())
            .ok_or_else(|| format!("{label}: <{parent}> has no element sequence in the schema"))?;
        if let Some(stray) = kids.iter().find(|k| !seq.contains(k)) {
            return Err(format!(
                "{label}: <{stray}> observed under <{parent}>, whose schema sequence does \
                 not contain it"
            ));
        }
        if seq.len() > 1 {
            table.push((parent.clone(), seq.clone()));
        }
    }

    table.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(render(syntax, instances, &table, root))
}

/// The uncoupled D16B subset schema directory, relative to the reference corpus.
const D16B: &str =
    "eInvoicing-EN16931/cii/schema/D16B SCRDM (Subset)/uncoupled clm/CII/uncefact/data/standard";

/// `element name -> child element names in sequence order`, for every element
/// with element content reachable from `rsm:CrossIndustryInvoice`.
fn d16b_sequences(dir: &Path) -> Result<BTreeMap<String, Vec<String>>, String> {
    const XS: &str = "http://www.w3.org/2001/XMLSchema";
    let mut types: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let mut root_type = None;
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().is_none_or(|x| x != "xsd") {
            continue;
        }
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let doc =
            roxmltree::Document::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let unprefixed = |t: &str| t.rsplit(':').next().unwrap_or(t).to_owned();
        for n in doc
            .root_element()
            .children()
            .filter(roxmltree::Node::is_element)
        {
            match (n.tag_name().namespace(), n.tag_name().name()) {
                (Some(XS), "complexType") => {
                    let Some(name) = n.attribute("name") else {
                        continue;
                    };
                    let kids = n
                        .descendants()
                        .filter(|d| {
                            d.tag_name().namespace() == Some(XS) && d.tag_name().name() == "element"
                        })
                        // An element with an inline type (`udt:DateTimeString`)
                        // has no `@type`; it is a child all the same.
                        .filter_map(|d| {
                            Some((
                                d.attribute("name")?.to_owned(),
                                d.attribute("type").map(unprefixed).unwrap_or_default(),
                            ))
                        })
                        .collect();
                    types.insert(name.to_owned(), kids);
                }
                (Some(XS), "element") if n.attribute("name") == Some("CrossIndustryInvoice") => {
                    root_type = n.attribute("type").map(unprefixed);
                }
                _ => {}
            }
        }
    }
    let root_type = root_type.ok_or("D16B: no CrossIndustryInvoice element declaration")?;

    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut stack = vec![("CrossIndustryInvoice".to_owned(), root_type)];
    let mut seen = std::collections::BTreeSet::new();
    while let Some((name, ty)) = stack.pop() {
        let Some(kids) = types.get(&ty) else { continue };
        if kids.is_empty() || !seen.insert((name.clone(), ty.clone())) {
            continue;
        }
        let seq: Vec<String> = kids.iter().map(|(k, _)| k.clone()).collect();
        if let Some(prev) = out.get(&name)
            && *prev != seq
        {
            return Err(format!("D16B: <{name}> has two different content models"));
        }
        out.insert(name, seq);
        stack.extend(kids.iter().cloned());
    }
    Ok(out)
}

fn render(
    syntax: Syntax,
    instances: usize,
    table: &[(String, Vec<String>)],
    root: &Path,
) -> Generated {
    let label = syntax.label();
    let module = syntax.module();
    let mut body = String::new();
    for (parent, kids) in table {
        let list = kids
            .iter()
            .map(|k| format!("\"{}\"", escape(k)))
            .collect::<Vec<_>>()
            .join(", ");
        body.push_str(&format!("    (\"{}\", &[{list}]),\n", escape(parent)));
    }

    let schema = match syntax {
        Syntax::Cii => "CEN's uncoupled D16B subset schema",
        Syntax::Ubl => "the OASIS UBL 2.1 schemas",
    };
    let method = format!(
        "//! For each parent, the children are the whole `xsd:sequence` of its type in\n\
         //! {schema}, so an element no instance carries still has its place.\n\
         //! `cargo xtask codegen` fails if an instance carries a child the sequence\n\
         //! does not contain.\n"
    );
    let title = format!("The {label} element order, from {schema}.");
    let doc = header(
        &title,
        &format!(
            "//! The {parents} parent elements seen in **{instances}** published `{roots}`\n\
             //! documents of the reference corpus.\n\
             //! {label} content models are XSD `sequence`s, and no Schematron rule\n\
             //! checks order.\n\
             //!\n\
             {method}\
             //!\n\
             //! The shared serialiser sorts every written element by this table.\n",
            roots = syntax.roots().join("` / `"),
            parents = table.len(),
        ),
    );

    let contents = format!(
        "{doc}\n\
         /// `(parent, children in sequence order)` for every observed parent whose\n\
         /// type has more than one child. Sorted by parent, for binary search.\n\
         pub static ORDER: &[(&str, &[&str])] = &[\n\
         {body}];\n\
         \n\
         /// The child order for a parent element, if it is in the table.\n\
         #[must_use]\n\
         pub fn children_of(parent: &str) -> Option<&'static [&'static str]> {{\n\
         \x20   ORDER\n\
         \x20       .binary_search_by_key(&parent, |(p, _)| *p)\n\
         \x20       .ok()\n\
         \x20       .map(|i| ORDER[i].1)\n\
         }}\n\
         \n\
         /// The position of `child` within `parent`'s sequence.\n\
         #[must_use]\n\
         pub fn index_of(parent: &str, child: &str) -> Option<usize> {{\n\
         \x20   children_of(parent)?.iter().position(|c| *c == child)\n\
         }}\n"
    );

    // `crate::local` has no other caller; this keeps it referenced.
    let _ = local;

    Generated {
        path: root.join("src").join(module).join("order.rs"),
        contents,
    }
}
