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

/// Both crates *inherit* their version rather than declaring one.
///
/// # What cargo already guarantees, and what it does not
///
/// The number appears twice — `[workspace.package] version`, and the
/// requirement in `[workspace.dependencies]`, because cargo has no
/// `version.workspace` for a dependency requirement. Bumping only the first is
/// **cargo's** error, not this test's:
///
/// ```text
/// error: failed to select a version for the requirement `en16931 = "^0.2.0"`
/// candidate versions found which didn't match: 0.3.0
/// ```
///
/// What cargo does *not* notice is a member quietly going back to declaring its
/// own `version = "…"`. That resolves fine, builds fine, and silently opts the
/// crate out of the workspace bump — which is the whole mechanism. So this test
/// asserts only the inheritance, and leaves the equality to the resolver.
#[test]
fn both_crates_inherit_their_version() {
    for member in ["crates/en16931", "crates/en16931-formats"] {
        let manifest = read(&format!("{member}/Cargo.toml"));
        assert!(
            manifest.contains("version.workspace"),
            "{member} declares its own version instead of inheriting \
             [workspace.package]"
        );
    }
    assert!(
        read("crates/en16931-formats/Cargo.toml").contains("en16931.workspace"),
        "en16931-formats pins en16931 itself instead of using the workspace \
         entry, which puts the requirement out of the resolver's reach"
    );
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
