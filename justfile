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

# Clippy over the feature combinations a consumer can actually select.
#
# `just lint` runs `--all-features`, the one combination where nothing is
# `cfg`-ed out — so it cannot see code that is dead without a feature, or code
# that fails to compile without one. `Xml::waiving` is called only by the UBL
# writer, and under `--features cii` alone it is dead code `-D warnings`
# rejects.
#
# The same list as `.github/workflows/ci.yml`: CI calling this recipe would put
# a `just` install in front of every job for one step's worth of sharing.
[doc("Clippy over every feature combination CI checks.")]
features:
    #!/usr/bin/env bash
    set -euo pipefail
    run() { echo "  $*"; cargo clippy -q "$@" -- -D warnings; }
    # en16931 — the model. Its whole claim is that the default build is small,
    # so every optional piece is checked alone.
    run -p en16931 --all-targets --no-default-features
    run -p en16931 --all-targets --no-default-features --features svrl
    run -p en16931 --all-targets --no-default-features --features serde
    run -p en16931 --all-targets --no-default-features --features billing
    # en16931-formats — the syntax bindings, each alone and neither.
    run -p en16931-formats --all-targets
    run -p en16931-formats --all-targets --no-default-features --features cii
    run -p en16931-formats --all-targets --no-default-features
    echo "  every feature combination is clean ✓"

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

# Every dependency-graph size the documentation claims, measured. A number
# repeated in five files is a number nobody rechecks.
#
# `--edges normal` excludes dev and build edges: this is the graph a *consumer*
# gets. Counts include the crate itself. Raising a limit is a decision — the
# small graph is why `en16931` reaches `wasm32`.
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
# A bare `.gitignore` pattern matches at **any depth**, and on a
# case-insensitive filesystem in **any case** — so one can silently swallow a
# source file that exists locally and was never pushed. `git status` does not
# list an ignored file, so the working tree looks clean.
#
# CI cannot catch this: it only ever has the tracked files, so the missing one
# is not there to notice. It has to run where the file exists.
[doc("Fail if a source file is gitignored, or is missing from git.")]
tracked:
    #!/usr/bin/env bash
    set -euo pipefail
    fail=0
    # Everything under these is source; `site/public/` and `spec/` are not.
    # `.cargo/` is in the list because it carries `audit.toml`, whose absence
    # from a checkout turns a reasoned advisory ignore back into a red CI job.
    for dir in site crates xtask .cargo .github; do
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
    echo "  every source file the build reads is tracked ✓"

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

# What the Site workflow runs: a full build, then internal links.
#
# The **build** is the half that catches a fenced code block naming a language
# Zola has no grammar for — `zola check` renders no markdown and passes on one.
# `--skip-external-links` is what CI runs too: resolving every external URL
# turns someone else's outage into a red build here.
[doc("Build the site and fail on a broken internal link.")]
site-check: tracked
    zola --root site build
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

# Security advisories — and a re-check of every advisory set aside.
#
# `cargo audit` reads `Cargo.lock`, which is feature-independent, so it can flag
# a crate that is never compiled. `.cargo/audit.toml` ignores one such advisory.
#
# An ignore nobody rechecks is how a real exposure ends up filed under a
# resolved one, so every crate named there must be absent from the **build**
# graph across the whole workspace with every feature on. If one appears, the
# ignore is no longer true and this fails.
[doc("Security advisories, and a re-check of every ignore in .cargo/audit.toml.")]
audit:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo audit
    # Crates the ignore list claims are not compiled. Keep in step with
    # `.cargo/audit.toml`; the file says so too.
    not_built=(rkyv)
    graph=$(cargo tree --workspace --all-features --prefix none --edges normal,build,dev)
    fail=0
    for crate in "${not_built[@]}"; do
      if grep -qE "^${crate} v" <<<"$graph"; then
        echo "  ${crate} IS in the build graph — the audit ignore for it is now false"
        fail=1
      else
        printf '  %-12s absent from the build graph ✓\n' "$crate"
      fi
    done
    test "$fail" = 0 || {
      echo
      echo "An advisory was set aside because the crate is never compiled, and it"
      echo "now is. Remove the ignore from .cargo/audit.toml and fix the advisory."
      exit 1
    }

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
ci: fmt-check lint features doc test-artefacts test-no-features codegen-check wasm deps tracked site-check
    @echo "✓ all green"
