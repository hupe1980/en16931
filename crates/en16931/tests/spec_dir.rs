//! `spec/` is found where the workspace keeps it, and its absence is reported.
//!
//! See `tests/common/mod.rs` for why this exists: the crate's move into
//! `crates/en16931/` silently disabled every artefact-reading suite, and they
//! reported green.

mod common;

use common::{REQUIRE, spec_root};
use std::path::Path;

/// The suite is only meaningful with the artefacts, so it says so either way.

#[test]
fn the_artefact_directory_is_found_or_its_absence_is_reported() {
    match spec_root() {
        Some(p) => {
            assert!(
                p.join("eInvoicing-EN16931").is_dir(),
                "{} exists but holds no CEN artefacts — a partial fetch is worse \
                 than none, because the suites would check a subset and pass",
                p.display()
            );
            // The path that broke: `spec/` is the workspace's, never a member's.
            assert_ne!(
                p,
                Path::new(env!("CARGO_MANIFEST_DIR")).join("spec"),
                "spec/ resolved inside the crate rather than at the workspace root"
            );
        }
        None => {
            assert!(
                std::env::var_os(REQUIRE).is_none(),
                "{REQUIRE} is set but there is no spec/ anywhere above {}",
                env!("CARGO_MANIFEST_DIR")
            );
        }
    }
}
