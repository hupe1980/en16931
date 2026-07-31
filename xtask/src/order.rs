//! Deriving element order from the authorities' own instances.
//!
//! # The method, and why it is not "read the XSD"
//!
//! This crate ships no schema, and adding one would mean shipping OASIS's and
//! UN/CEFACT's schema sets to answer a question with a much smaller answer:
//! *in what order may these forty-odd elements appear?* The published instances
//! answer it directly, and they are already fetched.
//!
//! For each parent element, every pair of children is counted in the order it
//! was observed. The children are then topologically sorted using the
//! **majority** direction of each pair — majority rather than first-seen,
//! because a large part of the corpus is *deliberately invalid* (KoSIT's
//! mutation instances exist to be rejected) and a single bad document must not
//! reorder the table.
//!
//! Ties are broken by mean relative position, which matters only for elements
//! that never co-occur — where any order is as good as any other and stability
//! across runs is what counts.
//!
//! The generator **fails** rather than emitting a table it could not derive
//! cleanly: an unresolved cycle or a tied pair means the corpus disagrees with
//! itself, and a guess written to disk looks exactly like a fact.

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
            // UBL has two, and they disagree about where `cbc:TaxPointDate`
            // goes — which is why deriving per-root rather than per-syntax is
            // not optional.
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

/// Pairwise precedence counts and observed positions, per parent element.
#[derive(Default)]
struct Observations {
    /// `parent -> (before, after) -> times seen in that order`.
    pairs: BTreeMap<String, BTreeMap<(String, String), u32>>,
    /// `parent -> child -> children in first-seen order`.
    names: Table,
    /// `parent -> child -> (sum of relative positions, count)`.
    positions: BTreeMap<String, BTreeMap<String, (f64, u32)>>,
}

impl Observations {
    fn walk(&mut self, node: roxmltree::Node<'_, '_>) {
        let kids: Vec<&str> = node
            .children()
            .filter(roxmltree::Node::is_element)
            .map(|c| c.tag_name().name())
            .collect();
        let parent = node.tag_name().name();
        if !kids.is_empty() {
            let names = self.names.entry(parent.to_owned()).or_default();
            let last = kids.len().saturating_sub(1);
            for (i, k) in kids.iter().enumerate() {
                if !names.iter().any(|n| n == k) {
                    names.push((*k).to_owned());
                }
                #[allow(clippy::cast_precision_loss)]
                let rel = if last == 0 {
                    0.0
                } else {
                    i as f64 / last as f64
                };
                let e = self
                    .positions
                    .entry(parent.to_owned())
                    .or_default()
                    .entry((*k).to_owned())
                    .or_insert((0.0, 0));
                e.0 += rel;
                e.1 += 1;
            }
            let pairs = self.pairs.entry(parent.to_owned()).or_default();
            for i in 0..kids.len() {
                for j in i + 1..kids.len() {
                    if kids[i] != kids[j] {
                        *pairs
                            .entry((kids[i].to_owned(), kids[j].to_owned()))
                            .or_default() += 1;
                    }
                }
            }
        }
        for c in node.children().filter(roxmltree::Node::is_element) {
            self.walk(c);
        }
    }
}

/// Derive the order table for `syntax` from `files`.
///
/// # Errors
///
/// Fails if no instance of this syntax was found, or if the corpus does not
/// agree on a total order for some parent — a cycle, or a pair seen equally
/// often in both directions.
pub fn derive(syntax: Syntax, files: &[PathBuf], root: &Path) -> Result<Generated, String> {
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
        obs.walk(doc.root_element());
    }

    if instances == 0 {
        return Err(format!("no {} instances found under spec/", syntax.label()));
    }

    let mut table: Vec<(String, Vec<String>)> = Vec::new();
    let mut conflicts: Vec<String> = Vec::new();

    for (parent, names) in &obs.names {
        let empty = BTreeMap::new();
        let pairs = obs.pairs.get(parent).unwrap_or(&empty);
        // `before[a]` = everything that must follow `a`.
        let mut before: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for a in names {
            for b in names {
                if a == b {
                    continue;
                }
                let ab = pairs.get(&(a.clone(), b.clone())).copied().unwrap_or(0);
                let ba = pairs.get(&(b.clone(), a.clone())).copied().unwrap_or(0);
                if ab > ba {
                    before.entry(a).or_default().push(b);
                } else if ab > 0 && ab == ba {
                    conflicts.push(format!("{parent}: {a} vs {b} seen {ab}× each way"));
                }
            }
        }
        let mean = |c: &str| -> f64 {
            obs.positions
                .get(parent)
                .and_then(|m| m.get(c))
                .map_or(0.0, |(sum, n)| sum / f64::from(*n))
        };

        let mut remaining: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut ordered: Vec<String> = Vec::new();
        while !remaining.is_empty() {
            let mut ready: Vec<&str> = remaining
                .iter()
                .copied()
                .filter(|c| {
                    !remaining
                        .iter()
                        .any(|o| o != c && before.get(o).is_some_and(|v| v.contains(c)))
                })
                .collect();
            if ready.is_empty() {
                conflicts.push(format!("{parent}: cycle among {remaining:?}"));
                ready.clone_from(&remaining);
            }
            // Stable and deterministic: least mean position, then by name.
            let pick = ready
                .iter()
                .copied()
                .min_by(|a, b| {
                    mean(a)
                        .partial_cmp(&mean(b))
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| a.cmp(b))
                })
                .expect("ready is non-empty");
            ordered.push(pick.to_owned());
            remaining.retain(|c| *c != pick);
        }
        if ordered.len() > 1 {
            table.push((parent.clone(), ordered));
        }
    }

    if !conflicts.is_empty() {
        return Err(format!(
            "{} could not be derived — the corpus disagrees with itself:\n  {}",
            syntax.label(),
            conflicts.join("\n  ")
        ));
    }

    table.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(render(syntax, instances, &table, root))
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

    let doc = header(
        &format!("The {label} element order, derived from the authorities' own instances."),
        &format!(
            "//! Derived from **{instances}** published `{roots}` documents under\n\
             //! `spec/` — CEN unit tests, KoSIT mutation instances, OpenPeppol examples —\n\
             //! covering {parents} parent elements.\n\
             //!\n\
             //! {label} content models are XSD `sequence`s: a document carrying the right\n\
             //! elements in the wrong order is invalid, and **no Schematron rule reports\n\
             //! it**, because ordering is the schema's job and this crate ships no schema.\n\
             //!\n\
             //! For each parent, the child order is the topological sort of the pairwise\n\
             //! precedences observed across all of them, taking the **majority** direction\n\
             //! where instances disagree — and they do, because much of that corpus is\n\
             //! deliberately invalid. `cargo xtask codegen` exits non-zero rather than\n\
             //! emitting a table it could not derive cleanly, so a cycle or a tied pair\n\
             //! fails the build instead of becoming a guess written to disk.\n\
             //!\n\
             //! [`mod@super::write`] does not consult this table directly — the shared serialiser\n\
             //! sorts by it, so the writer emits in whatever order reads best and cannot\n\
             //! produce a misordered document at all.\n",
            roots = syntax.roots().join("` / `"),
            parents = table.len(),
        ),
    );

    let contents = format!(
        "{doc}\n\
         /// `(parent, children in sequence order)` for every element observed with more\n\
         /// than one distinct child. Sorted by parent, for binary search.\n\
         pub static ORDER: &[(&str, &[&str])] = &[\n\
         {body}];\n\
         \n\
         /// The canonical child order for a parent element, if one was observed.\n\
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

    // `local` is used by the prohibitions generator; referenced here so a
    // future refactor that drops it fails loudly rather than silently.
    let _ = local;

    Generated {
        path: root.join("src").join(module).join("order.rs"),
        contents,
    }
}
