//! Finding `spec/`, and refusing to be quietly green without it.
//!
//! # Why this is not `env!("CARGO_MANIFEST_DIR").join("spec")`
//!
//! It was, in every suite that reads the artefacts — and when the crate moved
//! from the repository root into `crates/en16931/`, all of them started looking
//! in `crates/en16931/spec/`, found nothing, and **passed**. The conformance
//! suite reported four green tests in 0.00 s having checked no document at all.
//!
//! `spec/` belongs to the workspace, not to a member: it is one 136 MB tree
//! serving both crates. So it is resolved by walking up to the directory that
//! actually holds it, which survives the crate being moved again.
//!
//! # Why skipping is not enough
//!
//! `spec/` is not committed — the CEN artefacts are EUPL-1.2 — so a contributor
//! without it must still be able to run the suite. Skipping is right for them
//! and **wrong for CI**, where a skipped conformance run is indistinguishable
//! from a passing one in exactly the situation that matters.
//!
//! Both READMEs claim these suites "skip *loudly*". A `println!` in a green run
//! is not loud; nobody reads stdout of a passing test. So CI sets
//! `EN16931_REQUIRE_SPEC=1` and [`require`] turns the skip into a failure that
//! names what was missing.

#![allow(dead_code)] // not every including suite uses every helper

use std::path::{Path, PathBuf};

/// The environment variable CI sets to forbid skipping.
pub const REQUIRE: &str = "EN16931_REQUIRE_SPEC";

/// The workspace's `spec/` directory, if it has been fetched.
///
/// Walks up from this crate's manifest directory rather than assuming a depth,
/// so moving the crate within the workspace cannot silently disable the suites
/// again.
#[must_use]
pub fn spec_root() -> Option<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .map(|dir| dir.join("spec"))
        .find(|p| p.is_dir())
}

/// `spec_root()`, or `None` — and a failure instead when CI forbids skipping.
///
/// # Panics
/// When `spec/` is absent and [`REQUIRE`] is set, which is how a CI run that
/// was supposed to fetch the artefacts and did not is told apart from a
/// developer laptop that never had them.
#[must_use]
pub fn require(suite: &str) -> Option<PathBuf> {
    match spec_root() {
        Some(p) => Some(p),
        None => {
            assert!(
                std::env::var_os(REQUIRE).is_none(),
                "{suite} needs the artefacts and {REQUIRE} is set, so skipping is \
                 not permitted here. Run `cargo xtask fetch`.\n\
                 This is the check that stops a conformance run reporting green \
                 having validated nothing."
            );
            eprintln!(
                "note: {suite} SKIPPED — no spec/ directory. Run `cargo xtask fetch`. \
                 Set {REQUIRE}=1 to make this a failure."
            );
            None
        }
    }
}
