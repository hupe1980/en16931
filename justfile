# en16931 workspace — development task runner
# Install just: https://just.systems/man/en/
#
# Two crates: `en16931` (the model and the rules) and `en16931-formats` (the
# syntax bindings). Most recipes run over the whole workspace; the ones that
# assert a *property of one crate* are named for it, because "the workspace
# builds for wasm32" would be a weaker and less useful claim than "en16931 does".

# ── Default ───────────────────────────────────────────────────────────────────
# Show all available recipes.
default:
    @just --list --unsorted

# ── Setup ─────────────────────────────────────────────────────────────────────

# Fetch the CEN / KoSIT / Peppol artefacts into ./spec/.
#
# Once, for both crates. Not committed: the CEN artefacts are EUPL-1.2, a
# reciprocal licence, and keeping them out of the repository is what keeps both
# crates MIT OR Apache-2.0.
spec:
    cargo xtask fetch

# ── Code quality ──────────────────────────────────────────────────────────────

# Check formatting without making changes.
fmt-check:
    cargo fmt --all --check

# Format all source files.
fmt:
    cargo fmt --all

# Run Clippy on every target and feature (warnings are errors).
lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Quick type-check — the fastest feedback loop.
check:
    cargo check --workspace --all-targets --all-features

# ── Testing ───────────────────────────────────────────────────────────────────

# Unit + doc tests with default features.
test *ARGS:
    cargo test --workspace {{ ARGS }}

# Every feature enabled. The suites that need ./spec/ skip loudly without it.
test-all:
    cargo test --workspace --all-targets --all-features

# As `test-all`, but a missing ./spec/ is a **failure** rather than a skip.
#
# What CI runs. Use it locally after `just spec` to confirm the conformance and
# corpus suites are really executing — a skipped suite and a passing one look
# identical in the summary line, which is how 490 documents went unread.
test-artefacts:
    EN16931_REQUIRE_SPEC=1 cargo test --workspace --all-targets --all-features

# No default features.
test-no-features:
    cargo test --workspace --all-targets --no-default-features

# Run the examples of both crates.
examples:
    cargo run -p en16931 --example validate_an_invoice
    cargo run -p en16931 --example build_and_reconcile
    cargo run -p en16931 --example profiles_and_proofs
    cargo run -p en16931 --example report_formats --features serde,svrl
    cargo run -p en16931-formats --example write_both_syntaxes
    cargo run -p en16931-formats --example read_and_validate
    cargo run -p en16931-formats --example zugferd_extract --features zugferd

# Benchmarks (criterion).
bench:
    cargo bench -p en16931 --all-features

# ── Per-crate guarantees ──────────────────────────────────────────────────────
#
# These are the claims a workspace-wide command would quietly destroy. Running
# them per crate is not tidiness; it is the assertion.

# `en16931` must keep building for wasm32 — no I/O, no XML, no PDF.
#
# `-p en16931`, never `--workspace`: `en16931-formats` pulls `lopdf`, which does
# not build for this target and is not meant to. A workspace-wide wasm build
# would fail for a reason that says nothing about the model crate.
wasm:
    rustup target add wasm32-unknown-unknown
    cargo build -p en16931 --target wasm32-unknown-unknown --no-default-features --features svrl,serde

# Every dependency-graph size the documentation claims, measured.
#
# Both READMEs, both `lib.rs` headers and two `Cargo.toml` feature docs quote
# these numbers, and they had already drifted — the ZUGFeRD graph was documented
# as 56 in three places and 57 in two, and it is 57. A number repeated in five
# files is a number nobody rechecks, so it is checked here instead.
#
# `--edges normal` excludes dev and build edges: this is the graph a *consumer*
# gets, which is the only one worth quoting. Counts include the crate itself.
#
# Raising a limit is a decision, not a chore. The small graph is why `en16931`
# reaches `wasm32`, and why the PDF parser is behind a non-default feature.
deps:
    #!/usr/bin/env bash
    set -euo pipefail
    fail=0
    check() {                       # check <label> <expected> <pkg> [flags…]
      local label="$1" want="$2" pkg="$3"; shift 3
      local n
      n=$(cargo tree -p "$pkg" --edges normal --prefix none "$@" \
          | sed 's/ (\*)$//' | sort -u | grep -vc '^$')
      if [ "$n" = "$want" ]; then
        printf '  %-34s %3s ✓\n' "$label" "$n"
      else
        printf '  %-34s %3s ✗ documented as %s\n' "$label" "$n" "$want"
        fail=1
      fi
    }
    check "en16931 (default)"            10 en16931
    check "en16931-formats (ubl)"        13 en16931-formats
    check "en16931-formats (cii)"        13 en16931-formats --no-default-features --features cii
    check "en16931-formats (zugferd)"    57 en16931-formats --features zugferd
    test "$fail" = 0 || {
      echo
      echo "A documented graph size is wrong. Either the dependency should not be"
      echo "there, or every file quoting the old number needs updating:"
      echo "  rg -n '<old> crates' crates/ README.md"
      exit 1
    }

# ── Generated code ────────────────────────────────────────────────────────────

# Regenerate everything derived from the artefacts, in both crates.
codegen:
    cargo xtask codegen

# Fail if any generated file no longer matches the artefacts. Runs in CI, so a
# table cannot drift away from the specification it claims to come from.
codegen-check:
    cargo xtask check

# ── Documentation ─────────────────────────────────────────────────────────────

# Build the docs, warnings as errors.
#
# `-D warnings` is what catches a broken intra-doc link. Without it, a
# `[`crate::foo`]` pointing at something deleted is a warning nobody reads.
doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features

# Build them exactly as docs.rs will — needs nightly, because the feature
# badges (`#[doc(cfg(...))]`) are still unstable.
doc-docsrs:
    RUSTDOCFLAGS="--cfg docsrs -D warnings" cargo +nightly doc --workspace --no-deps --all-features

# Build and open.
doc-open:
    cargo doc --workspace --no-deps --all-features --open

# ── Dependencies ──────────────────────────────────────────────────────────────

# Security advisories.
audit:
    cargo audit

# Licence and dependency policy, over the union of both crates' graphs.
deny:
    cargo deny --all-features check

# ── Release ───────────────────────────────────────────────────────────────────

# Dry-run the release.
#
# `--workspace` derives the order from the dependency graph and waits for the
# index between crates, so neither is a convention to remember. `xtask` is
# `publish = false` and is skipped.
publish-dry:
    cargo publish --workspace --dry-run

# ── Everything CI runs, locally ───────────────────────────────────────────────
ci: fmt-check lint doc test-artefacts test-no-features codegen-check wasm deps
    @echo "✓ all green"
