# en16931 — development task runner
# Install just: https://just.systems/man/en/

# ── Default ───────────────────────────────────────────────────────────────────
# Show all available recipes.
default:
    @just --list --unsorted

# ── Setup ─────────────────────────────────────────────────────────────────────

# Fetch the CEN / KoSIT / Peppol artefacts into ./spec/.
#
# Not committed: the CEN artefacts are EUPL-1.2, a reciprocal licence, and
# keeping them out of the repository is what keeps this crate MIT OR Apache-2.0.
spec:
    cargo xtask fetch

# ── Code quality ──────────────────────────────────────────────────────────────

# Check formatting without making changes.
fmt-check:
    cargo fmt --all --check

# Format all source files.
fmt:
    cargo fmt --all

# Run Clippy on all targets and features (warnings are errors).
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Quick type-check — the fastest feedback loop.
check:
    cargo check --all-targets --all-features

# ── Testing ───────────────────────────────────────────────────────────────────

# Unit + doc tests with default features.
test *ARGS:
    cargo test {{ ARGS }}

# Every feature enabled. The suites that need ./spec/ skip loudly without it.
test-all:
    cargo test --all-targets --all-features

# No default features.
test-no-features:
    cargo test --all-targets --no-default-features

# Run the examples.
examples:
    cargo run --example validate_an_invoice
    cargo run --example profiles_and_proofs
    cargo run --example report_formats --features serde,svrl

# Benchmarks (criterion).
bench:
    cargo bench --all-features

# The crate must keep building for wasm32 — no I/O, no XML, no PDF.
wasm:
    rustup target add wasm32-unknown-unknown
    cargo build --target wasm32-unknown-unknown --no-default-features --features svrl,serde

# ── Generated code ────────────────────────────────────────────────────────────

# Regenerate everything derived from the artefacts.
codegen:
    cargo xtask codegen

# Fail if any generated file no longer matches the artefacts. Runs in CI, so a
# table cannot drift away from the specification it claims to come from.
codegen-check:
    cargo xtask check

# ── Documentation ─────────────────────────────────────────────────────────────

# Build the docs, warnings as errors.
doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features

# Build them exactly as docs.rs will — needs nightly, because the feature
# badges (`#[doc(cfg(...))]`) are still unstable.
doc-docsrs:
    RUSTDOCFLAGS="--cfg docsrs -D warnings" cargo +nightly doc --no-deps --all-features

# Build and open.
doc-open:
    cargo doc --no-deps --all-features --open

# ── Dependencies ──────────────────────────────────────────────────────────────

# Security advisories.
audit:
    cargo audit

# Licence and dependency policy.
deny:
    cargo deny --all-features check

# ── Everything CI runs, locally ───────────────────────────────────────────────
ci: fmt-check lint doc test-all test-no-features codegen-check
    @echo "✓ all green"
