//! The artefact revision is pinned in three places. They have to agree.
//!
//! # This test is the reason the two crates share a repository
//!
//! `xtask` fetches the CEN artefacts at a pinned tag. `en16931` publishes that
//! tag as [`en16931::ARTEFACT_VERSION`], so a bug report can say which rule text
//! was in force. The generated code lists stamp it into their own header. Three
//! copies of one string, and nothing compared them.
//!
//! Both crates' `fetch.rs` *claimed* they were compared — `"Must equal
//! `en16931::ARTEFACT_VERSION`, which `tests/attribution.rs` in that crate pins
//! to the same string"` — and no such assertion existed in either repository.
//! It could not have: while `en16931-formats` was a separate repository, its
//! `CEN_REF` and this crate's `ARTEFACT_VERSION` were in different Cargo graphs,
//! different CI runs and different clones. A comment asking a human to keep two
//! constants equal is not a check, and this one shipped in both crates' 0.1.0
//! releases asserting a test that did not exist.
//!
//! In one workspace it is four lines of `read_to_string`.
//!
//! # Why it reads source rather than importing
//!
//! `xtask` is `publish = false` and is deliberately *not* a dependency of either
//! library — its `roxmltree` must never reach a consumer's graph. So the pin is
//! read out of the file as text. That is uglier than an import and it is the
//! only form that does not put an XML parser in the dependency tree of a crate
//! whose whole claim is that it has none.

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

fn read(rel: &str) -> String {
    let p = workspace_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// Every published member *inherits* its version rather than declaring one, and
/// reaches its siblings through `[workspace.dependencies]`.
///
/// # What cargo already guarantees, and what it does not
///
/// The number appears in `[workspace.package] version` and once per internal
/// edge in `[workspace.dependencies]`, because cargo has no `version.workspace`
/// for a dependency requirement. Bumping only the first is **cargo's** error,
/// not this test's:
///
/// ```text
/// error: failed to select a version for the requirement `en16931 = "^0.2.0"`
/// candidate versions found which didn't match: 0.3.0
/// ```
///
/// What cargo does *not* notice is a member quietly going back to declaring its
/// own `version = "…"`, or pinning a sibling at its own use site. Both resolve
/// fine, build fine, and silently opt the crate out of the workspace bump —
/// which is the whole mechanism. So this test asserts those two, and leaves the
/// equality to the resolver.
///
/// Enumerated from `cargo metadata` rather than from a list: `en16931-cli` was
/// added to the workspace with a hand-written `version = "0.2.0"` on its
/// `en16931-formats` edge, and a hard-coded list of two members would not have
/// noticed.
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
        // An internal edge with a literal `path = "../…"` is one that bypassed
        // `[workspace.dependencies]`, so the resolver never sees the version
        // requirement that makes a bad bump fail.
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

    // The generator writes the revision into the file it produces, so a table
    // regenerated against a different checkout than the one CI fetched is
    // visible in the diff rather than only in someone's shell history.
    let generated = read("crates/en16931/src/codes/generated.rs");
    assert!(
        generated.contains(&cen_ref),
        "crates/en16931/src/codes/generated.rs does not name `{cen_ref}`; \
         it was generated from a different artefact revision"
    );

    // The codegen module stamps the same constant into every table's `Source:`
    // line. It is a third copy, so it is compared too.
    let codes = read("xtask/src/codes.rs");
    let artefact =
        const_str(&codes, "ARTEFACT").expect("xtask/src/codes.rs declares `const ARTEFACT: &str`");
    assert_eq!(
        artefact, cen_ref,
        "the code generator stamps `{artefact}` into every table's provenance \
         line while the fetcher pins `{cen_ref}`"
    );
}

/// `en16931-formats`' generated tables come from the same fetch, so they cannot
/// name a different revision either.
///
/// This is the assertion that was simply impossible to write before the merge:
/// the two crates' tables were derived in separate repositories from separate
/// `spec/` trees, and nothing anywhere compared them.
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

/// Every release a profile claims to be verified against is one `xtask` fetches.
///
/// [`Profile::artefacts`] is the provenance line in every stored report, and it
/// is a `&'static str` typed by hand — so nothing stops it naming a KoSIT tag
/// that was never cloned, or drifting a release behind the pin the conformance
/// suites actually ran on. Either way the report would cite a revision nobody
/// checked, which is worse than citing none: it is a claim rather than a gap.
///
/// So the pins are read out of `xtask/src/fetch.rs` — the one place that
/// decides what lands in `spec/` — and every profile's list has to be a subset.
///
/// The CEN entry gets a second assertion, because it has a second copy:
/// [`en16931::ARTEFACT_VERSION`] is public API and is stamped into every
/// generated table's provenance header.
///
/// [`Profile::artefacts`]: en16931::validation::profile::Profile::artefacts
#[test]
fn every_declared_artefact_ref_is_one_xtask_fetches() {
    let fetch = read("xtask/src/fetch.rs");
    let fetched: Vec<String> = [
        "CEN_REF",
        "PEPPOL_REF",
        "KOSIT_SCHEMATRON_REF",
        "KOSIT_CONFIG_REF",
    ]
    .iter()
    .map(|name| {
        const_str(&fetch, name).unwrap_or_else(|| panic!("xtask/src/fetch.rs defines {name}"))
    })
    .collect();

    for profile in en16931::profiles::ALL {
        for artefact in profile.artefacts {
            assert!(
                fetched.iter().any(|r| r == artefact.git_ref),
                "{} claims to be verified against {} {}, which `xtask fetch` \
                 never clones — the conformance suites cannot have run against \
                 it. Fetched: {fetched:?}",
                profile.id,
                artefact.repo,
                artefact.git_ref,
            );
        }
    }

    // `CEN_REF` is the one with a public copy in the library.
    assert_eq!(
        en16931::profiles::CEN.git_ref,
        en16931::ARTEFACT_VERSION,
        "profiles::CEN and ARTEFACT_VERSION are the same pin"
    );
}
