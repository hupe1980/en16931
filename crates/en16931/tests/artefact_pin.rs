//! Artefact pins, versions and source lists that are repeated across the
//! workspace must agree: `xtask`'s fetch pins, [`en16931::ARTEFACT_VERSION`],
//! generated-table headers, profile provenance and the published pin table.
//!
//! `xtask` is not a dependency (its XML parser must stay out of the library's
//! graph), so its sources are read as text.

use std::path::{Path, PathBuf};

/// The workspace root, from this crate's manifest directory.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/en16931 is two levels below the workspace root")
        .to_path_buf()
}

/// The value of a `const NAME: &str = "…";` in a Rust source file.
fn const_str(source: &str, name: &str) -> Option<String> {
    let decl = source.find(&format!("{name}: &str"))?;
    let open = source[decl..].find('"')? + decl + 1;
    let close = source[open..].find('"')? + open;
    Some(source[open..close].to_owned())
}

/// The string literals of a `const NAME: &[&str] = &[ … ];` in a Rust source.
fn const_list(source: &str, name: &str) -> Option<Vec<String>> {
    let decl = source.find(&format!("{name}: &[&str]"))?;
    let open = source[decl..].find("= &[")? + decl;
    let close = source[open..].find("];")? + open;
    Some(
        source[open..close]
            .split('"')
            .skip(1)
            .step_by(2)
            .map(str::to_owned)
            .collect(),
    )
}

/// Every git source `fetch.rs` fetches (its `REPOS`, not the repositories it
/// only watches), as `(owner/repo, ref)`: the repository from its `url`, the ref
/// from the constant its `reference` names.
fn fetched_pairs(fetch: &str) -> Vec<(String, String)> {
    let repos = fetch
        .split_once("static REPOS: &[Repo] = &[")
        .and_then(|(_, rest)| rest.split_once("\n];"))
        .map(|(repos, _)| repos)
        .expect("fetch.rs declares REPOS");
    let consts = [
        "CEN_REF",
        "PEPPOL_REF",
        "KOSIT_SCHEMATRON_REF",
        "KOSIT_CONFIG_REF",
        "ZUGFERD_CORPUS_REF",
    ];
    let mut out = Vec::new();
    for block in repos.split("Repo {").skip(1) {
        let block = block.split("purpose:").next().unwrap_or(block);
        let Some(slug) = block
            .split_once("url: \"https://github.com/")
            .and_then(|(_, rest)| rest.split_once(".git\""))
            .map(|(slug, _)| slug.to_owned())
        else {
            continue;
        };
        let reference = block.split_once("reference:").map_or("", |(_, r)| r);
        let name = consts
            .iter()
            .find(|c| {
                reference
                    .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                    .any(|w| w == **c)
            })
            .unwrap_or_else(|| panic!("{slug}: its reference names no known pin constant"));
        let value = const_str(fetch, name).unwrap_or_else(|| panic!("fetch.rs defines {name}"));
        out.push((slug, value));
    }
    out
}

fn read(rel: &str) -> String {
    let p = workspace_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// Every workspace member (from `cargo metadata`) inherits its version and
/// reaches siblings through `[workspace.dependencies]`, so a workspace bump
/// cannot skip one. Version equality itself is left to the resolver.
#[test]
fn every_member_inherits_its_version() {
    let meta = std::process::Command::new(env!("CARGO"))
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(workspace_root())
        .output()
        .expect("cargo metadata runs");
    let stdout = String::from_utf8(meta.stdout).expect("metadata is UTF-8");

    // The manifest paths of every workspace member, without a JSON dependency:
    // each appears as `"manifest_path":"…/Cargo.toml"`.
    let manifests: Vec<&str> = stdout
        .match_indices("\"manifest_path\":\"")
        .map(|(i, m)| {
            let start = i + m.len();
            let end = start + stdout[start..].find('"').expect("closing quote");
            &stdout[start..end]
        })
        .collect();
    assert!(
        manifests.len() >= 4,
        "expected at least four members, found {manifests:?}"
    );

    for path in manifests {
        let manifest = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert!(
            manifest.contains("version.workspace"),
            "{path} declares its own version instead of inheriting \
             [workspace.package]"
        );
        // A `path = "../…"` edge bypasses `[workspace.dependencies]`.
        assert!(
            !manifest.contains("path = \"../"),
            "{path} reaches a sibling by path instead of through \
             [workspace.dependencies], which puts the version requirement out \
             of the resolver's reach"
        );
    }
}

/// The pin the fetcher uses, the pin the crate publishes, and the pin stamped
/// into the generated tables are one value.
#[test]
fn every_artefact_pin_in_the_workspace_agrees() {
    let fetch = read("xtask/src/fetch.rs");
    let cen_ref = const_str(&fetch, "CEN_REF")
        .expect("xtask/src/fetch.rs declares `pub const CEN_REF: &str`");

    assert_eq!(
        cen_ref,
        en16931::ARTEFACT_VERSION,
        "xtask fetches `{cen_ref}` but `en16931::ARTEFACT_VERSION` says \
         `{}`. A finding would then cite a rule revision the tables did not \
         come from. Bump both, or neither.",
        en16931::ARTEFACT_VERSION
    );

    // The generated tables name the revision they came from.
    let generated = read("crates/en16931/src/codes/generated.rs");
    assert!(
        generated.contains(&cen_ref),
        "crates/en16931/src/codes/generated.rs does not name `{cen_ref}`; \
         it was generated from a different artefact revision"
    );

    // The codegen's `ARTEFACT` stamps every table's `Source:` line.
    let codes = read("xtask/src/codes.rs");
    let artefact =
        const_str(&codes, "ARTEFACT").expect("xtask/src/codes.rs declares `const ARTEFACT: &str`");
    assert_eq!(
        artefact, cen_ref,
        "the code generator stamps `{artefact}` into every table's provenance \
         line while the fetcher pins `{cen_ref}`"
    );
}

/// `en16931-formats`' generated tables name the same revision.
#[test]
fn the_syntax_bindings_were_derived_from_the_same_artefacts() {
    let cen_ref = const_str(&read("xtask/src/fetch.rs"), "CEN_REF").expect("CEN_REF");

    for table in [
        "crates/en16931-formats/src/ubl/prohibitions_generated.rs",
        "crates/en16931-formats/src/cii/prohibitions_generated.rs",
    ] {
        let source = read(table);
        assert!(
            source.contains("Generated by `cargo xtask codegen`"),
            "{table} has lost its generated-file header"
        );
        assert!(
            !source.contains("validation-") || source.contains(&cen_ref),
            "{table} names an artefact revision other than `{cen_ref}`"
        );
    }
}

/// Every `(repo, ref)` in a profile's [`Profile::artefacts`] is one
/// `xtask/src/fetch.rs` fetches, and `profiles::CEN` equals
/// [`en16931::ARTEFACT_VERSION`].
///
/// [`Profile::artefacts`]: en16931::validation::profile::Profile::artefacts
#[test]
fn every_declared_artefact_ref_is_one_xtask_fetches() {
    let fetch = read("xtask/src/fetch.rs");
    // Compare pairs, so one repository with another's ref fails.
    let fetched = fetched_pairs(&fetch);
    assert!(
        fetched.len() >= 4,
        "fetch.rs parsed to too few sources: {fetched:?}"
    );

    for profile in en16931::profiles::ALL {
        for artefact in profile.artefacts {
            assert!(
                fetched
                    .iter()
                    .any(|(repo, r)| repo == artefact.repo && r == artefact.git_ref),
                "{} claims to be verified against {} {}, which `xtask fetch` \
                 never clones — the conformance suites cannot have run against \
                 it. Fetched: {fetched:?}",
                profile.id,
                artefact.repo,
                artefact.git_ref,
            );
        }
    }

    assert_eq!(
        en16931::profiles::CEN.git_ref,
        en16931::ARTEFACT_VERSION,
        "profiles::CEN and ARTEFACT_VERSION are the same pin"
    );
}

/// `fetch.rs`'s `ORDER_SOURCES` equals the formats tests' `AUTHORITY_SOURCES`.
#[test]
fn the_authority_source_lists_agree() {
    let order = const_list(&read("xtask/src/fetch.rs"), "ORDER_SOURCES")
        .expect("fetch.rs declares ORDER_SOURCES");
    let suites = const_list(
        &read("crates/en16931-formats/tests/common/mod.rs"),
        "AUTHORITY_SOURCES",
    )
    .expect("the formats test support declares AUTHORITY_SOURCES");
    assert!(!order.is_empty());
    assert_eq!(order, suites, "ORDER_SOURCES and AUTHORITY_SOURCES differ");
}

/// The pin table on the site's conformance page names every pin `fetch.rs`
/// holds, at its value.
#[test]
fn the_published_pin_tables_name_every_pin() {
    let fetch = read("xtask/src/fetch.rs");
    for page in ["site/content/docs/conformance.md"] {
        if !workspace_root().join(page).exists() {
            continue;
        }
        let text = read(page);
        for name in [
            "CEN_REF",
            "PEPPOL_REF",
            "KOSIT_SCHEMATRON_REF",
            "KOSIT_CONFIG_REF",
            "ZUGFERD_CORPUS_REF",
            "UBL_SHA256",
        ] {
            let value =
                const_str(&fetch, name).unwrap_or_else(|| panic!("fetch.rs defines {name}"));
            assert!(
                text.contains(&format!("`{value}`")),
                "{page} does not state {name} = {value}"
            );
        }
    }
}

// ── Every authority's CEN release ─────────────────────────────────────────────

mod common;

/// The CEN release an authority's pinned artefact says it was built on.
struct Declaration {
    authority: &'static str,
    /// Relative to the reference corpus.
    file: &'static str,
    /// The text immediately before the version — the first occurrence counts.
    needle: &'static str,
}

/// Every pinned authority whose artefact names a CEN release: KoSIT in its
/// changelog, OpenPeppol in its bundled CEN Schematron's header.
const DECLARATIONS: &[Declaration] = &[
    Declaration {
        authority: "KoSIT",
        file: "validator-configuration-xrechnung/CHANGELOG.md",
        needle: "CEN Schematron Rules ",
    },
    Declaration {
        authority: "OpenPeppol",
        file: "peppol-bis-invoice-3/rules/sch/CEN-EN16931-UBL.sch",
        needle: "Schematron version ",
    },
];

/// A declared difference between an authority's CEN release and this crate's;
/// `diverging` asserts each code value the two releases disagree on.
struct Skew {
    authority: &'static str,
    /// The CEN release the authority names.
    names: &'static str,
    diverging: fn(),
}

/// KoSIT's configuration and OpenPeppol's bundled CEN Schematron both build on
/// CEN 1.3.16; this crate runs 1.3.17. Their CEN rules differ in the code lists
/// [`cen_1_3_17_lists`] asserts, and in naming the IGIC / IPSI rules
/// `BR-AF-*` / `BR-AG-*`.
const DECLARED_SKEWS: &[Skew] = &[
    Skew {
        authority: "KoSIT",
        names: "1.3.16",
        diverging: cen_1_3_17_lists,
    },
    Skew {
        authority: "OpenPeppol",
        names: "1.3.16",
        diverging: cen_1_3_17_lists,
    },
];

/// The code values CEN 1.3.17 changed against 1.3.16 (#509, #510, #525, #527).
fn cen_1_3_17_lists() {
    use en16931::codes::{contains, generated as g};
    // ISO 4217: São Tomé's dobra.
    assert!(contains(g::CURRENCY_CODES, "STN") && !contains(g::CURRENCY_CODES, "STD"));
    // ISO 6523 ICD: three new schemes.
    for icd in ["0249", "0250", "0251"] {
        assert!(contains(g::ICD_SCHEMES, icd), "{icd}");
    }
    // VATEX: the Spanish exemption reasons.
    assert!(contains(g::VATEX_CODES, "VATEX-ES-IGIC11"));
    assert!(contains(g::VATEX_CODES, "VATEX-ES-IPSI7"));
    // CEF EAS: two withdrawn, one added.
    assert!(!contains(g::EAS_SCHEMES, "0219") && !contains(g::EAS_SCHEMES, "0220"));
    assert!(contains(g::EAS_SCHEMES, "9960"));
}

/// Every pinned authority builds on the CEN release this crate runs, or the
/// skew is declared in [`DECLARED_SKEWS`] (and fails once it disappears).
#[test]
fn every_authority_names_the_cen_release_we_run() {
    let Some(spec) = common::require("the per-authority CEN release check") else {
        return;
    };
    for d in DECLARATIONS {
        let path = spec.join(d.file);
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let at = text.find(d.needle).unwrap_or_else(|| {
            panic!(
                "{} ({}) no longer names a CEN release",
                d.authority,
                path.display()
            )
        }) + d.needle.len();
        let named: String = text[at..]
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let ours = en16931::ARTEFACT_VERSION
            .strip_prefix("validation-")
            .expect("ARTEFACT_VERSION is a `validation-` tag");

        match DECLARED_SKEWS.iter().find(|s| s.authority == d.authority) {
            None => assert_eq!(
                named, ours,
                "{} now builds on CEN {named}, and this crate runs CEN {ours}. Read the \
                 diff between the two releases: either move the pins together, or declare \
                 the skew in DECLARED_SKEWS with each diverging code value asserted.",
                d.authority
            ),
            Some(skew) => {
                assert_ne!(
                    named, ours,
                    "{} has caught up with CEN {ours}: delete its DECLARED_SKEWS entry",
                    d.authority
                );
                assert_eq!(
                    named, skew.names,
                    "{} now names CEN {named}, not the declared {}",
                    d.authority, skew.names
                );
                (skew.diverging)();
            }
        }
    }
}

/// CEN 1.3.16's ISO 4217 list where it differs from 1.3.15: `BGN` and `ANG`
/// removed, `XCG` added.
#[test]
fn cen_1_3_16_currency_codes() {
    use en16931::invoice::Code;
    use en16931::{Invoice, validate};

    let fires = |cur: &str| {
        let mut inv = Invoice::default();
        inv.currency = Some(Code::new(cur));
        validate(&inv).has("BR-CL-04")
    };

    assert!(fires("BGN"), "CEN 1.3.16 removed BGN");
    assert!(fires("ANG"), "CEN 1.3.16 removed ANG");
    assert!(!fires("XCG"), "CEN 1.3.16 added XCG");
    assert!(!fires("EUR"), "the control");
}
