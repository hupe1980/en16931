# en16931 workspace — development task runner
# Install just: https://just.systems/man/en/
#
# Three crates: `en16931` (the model and the rules), `en16931-formats` (the
# syntax bindings) and `en16931-cli` (the command). Most recipes run over the
# whole workspace; the ones that assert a *property of one crate* are named for
# it, because "the workspace builds for wasm32" would be a weaker and less
# useful claim than "en16931 does".

# ── Default ───────────────────────────────────────────────────────────────────
# Show all available recipes.
default:
    @just --list --unsorted

# ── Setup ─────────────────────────────────────────────────────────────────────

# Fetch the CEN / KoSIT / Peppol artefacts into ./spec/.
#
# Once, for the whole workspace. Not committed: the CEN artefacts are EUPL-1.2, a
# reciprocal licence, and keeping them out of the repository is what keeps every
# crate here MIT OR Apache-2.0.
[doc("Fetch the CEN / KoSIT / Peppol artefacts into ./spec/.")]
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
# identical in the summary line, which is how 486 documents went unread.
[doc("As test-all, but a missing ./spec/ is a failure rather than a skip.")]
test-artefacts:
    EN16931_REQUIRE_SPEC=1 cargo test --workspace --all-targets --all-features

# No default features.
test-no-features:
    cargo test --workspace --all-targets --no-default-features

# Run the examples of both libraries, and the command's own smoke path.
examples:
    cargo run -p en16931 --example validate_an_invoice
    cargo run -p en16931 --example build_and_reconcile
    cargo run -p en16931 --example profiles_and_proofs
    cargo run -p en16931 --example report_formats --features serde,svrl
    cargo run -p en16931-formats --example write_both_syntaxes
    cargo run -p en16931-formats --example read_and_validate
    cargo run -p en16931-formats --example zugferd_extract --features zugferd
    cargo run -p en16931-cli -- profiles
    cargo run -p en16931-cli -- explain BR-CO-14
    cargo run -p en16931-cli -- rules --term BT-117

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
[doc("en16931 must keep building for wasm32 — no I/O, no XML, no PDF.")]
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
[doc("Every dependency-graph size the documentation claims, measured.")]
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
    # The `billing` feature's whole claim is that the calculation engine costs
    # nothing beyond what the model already has: `billing` depends on
    # `rust_decimal` and `thiserror`, which are `en16931`'s own two. If that ever
    # stops being true the adapter's opening paragraph stops being true with it.
    check "en16931 (billing adapter)"    11 en16931 --features billing
    # No limit on `en16931-cli`: it is a binary, so its graph is in nobody's
    # dependency tree and the argument that shapes the libraries' does not apply.
    test "$fail" = 0 || {
      echo
      echo "A documented graph size is wrong. Either the dependency should not be"
      echo "there, or every file quoting the old number needs updating:"
      echo "  rg -n '<old> crates' crates/ README.md"
      exit 1
    }

# ── Is anything the build needs invisible to git? ─────────────────────────────

# Fail if a source file is gitignored, or is tracked but should not be.
#
# This exists because of a bug that cost a green local build and a red CI one.
# `.gitignore` carried a bare filename for a local working note — and a bare
# pattern matches at **any depth**, on a case-insensitive filesystem in **any
# case**. It matched a documentation page whose name collided, and that page was
# silently untracked: it existed here, the site built here, and CI failed on a
# broken link to a page that had never been pushed.
#
# The reason nobody saw it is the sharp bit: `git status` does not list an
# ignored file. The working tree looked clean because the file was invisible,
# not because it was committed. Anchoring the patterns fixed that instance; this
# recipe is what makes the next one impossible.
#
# CI cannot catch this — it only ever has the tracked files, so the missing one
# simply is not there to notice. It has to run where the file exists.
[doc("Fail if a source file is gitignored, or is missing from git.")]
tracked:
    #!/usr/bin/env bash
    set -euo pipefail
    fail=0
    # Everything under these is source; `site/public/` and `spec/` are not.
    for dir in site crates xtask; do
      while IFS= read -r -d '' f; do
        case "$f" in site/public/*) continue;; esac
        if reason=$(git check-ignore -v "$f" 2>/dev/null); then
          echo "  ignored but needed: $f"
          echo "      by $reason"
          fail=1
        elif ! git ls-files --error-unmatch "$f" >/dev/null 2>&1; then
          echo "  untracked: $f"
          fail=1
        fi
      done < <(find "$dir" -type f -not -path '*/.git/*' -print0)
    done
    test "$fail" = 0 || {
      echo
      echo "A file the build reads is not in the repository. An ignored file does"
      echo "not appear in \`git status\`, so this is the only place it shows up."
      exit 1
    }
    echo "  every source file under site/, crates/ and xtask/ is tracked ✓"

# ── Generated code ────────────────────────────────────────────────────────────

# Regenerate everything derived from the artefacts, in both crates.
codegen:
    cargo xtask codegen

# Fail if any generated file no longer matches the artefacts. Runs in CI, so a
# table cannot drift away from the specification it claims to come from.
[doc("Fail if any generated file no longer matches the artefacts.")]
codegen-check:
    cargo xtask check

# ── Documentation ─────────────────────────────────────────────────────────────

# Build the docs, warnings as errors.
#
# `-D warnings` is what catches a broken intra-doc link. Without it, a
# `[`crate::foo`]` pointing at something deleted is a warning nobody reads.
[doc("Build the docs, warnings as errors.")]
doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features

# Build them exactly as docs.rs will — needs nightly, because the feature
# badges (`#[doc(cfg(...))]`) are still unstable.
[doc("Build the docs exactly as docs.rs will (needs nightly).")]
doc-docsrs:
    RUSTDOCFLAGS="--cfg docsrs -D warnings" cargo +nightly doc --workspace --no-deps --all-features

# Build and open.
doc-open:
    cargo doc --workspace --no-deps --all-features --open

# ── The documentation site ────────────────────────────────────────────────────
#
# `site/` is a Zola site — the landing page and the prose documentation. The API
# reference is rustdoc's job and lives on docs.rs; this is the part that explains
# *why*, which rustdoc is the wrong shape for.
#
# Not part of `just ci`: it needs Zola installed, and a prose change should not
# have to run the conformance suites to land. `.github/workflows/site.yml` builds
# and deploys it on its own.

# Build the site into site/public.
site:
    zola --root site build

# Serve it locally with live reload.
site-serve:
    zola --root site serve

# Fail on a broken internal link. `--skip-external-links` is what CI runs: the
# full check resolves every external URL, which turns someone else's outage into
# a red build here.
[doc("Fail on a broken internal link.")]
site-check: tracked
    zola --root site check --skip-external-links

# Re-render the Open Graph card from its SVG source.
#
# The PNG is committed because the build must not depend on a rasteriser being
# installed, and because the PNG is what X, LinkedIn and Slack actually fetch —
# none of them renders an SVG card, they just show none. Run after editing
# `site/static/social-card.svg`.
[doc("Re-render the Open Graph card PNG from its SVG source.")]
site-card:
    rsvg-convert -w 1200 -h 630 site/static/social-card.svg -o site/static/social-card.png

# ── Dependencies ──────────────────────────────────────────────────────────────

# Security advisories.
audit:
    cargo audit

# Licence and dependency policy, over the union of every crate's graph.
deny:
    cargo deny --all-features check

# ── Release ───────────────────────────────────────────────────────────────────

# Dry-run the release.
#
# `--workspace` derives the order from the dependency graph and waits for the
# index between crates, so the order is not a convention to remember. `xtask` is
# `publish = false` and is skipped.
[doc("Dry-run the release.")]
publish-dry:
    cargo publish --workspace --dry-run

# ── Everything CI runs, locally ───────────────────────────────────────────────
ci: fmt-check lint doc test-artefacts test-no-features codegen-check wasm deps tracked
    @echo "✓ all green"
