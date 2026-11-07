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

# Fetch the CEN / KoSIT / Peppol artefacts into the reference corpus.
#
# Once, for the whole workspace. Not committed: the CEN artefacts are EUPL-1.2, a
# reciprocal licence, and keeping them out of the repository is what keeps every
# crate here MIT OR Apache-2.0.
[doc("Fetch the CEN / KoSIT / Peppol artefacts into the reference corpus.")]
references:
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

# Every feature enabled. The suites that need the reference corpus skip loudly without it.
test-all:
    cargo test --workspace --all-targets --all-features

# As `test-all`, but a missing the reference corpus is a **failure** rather than a skip.
#
# What CI runs. Use it locally after `just references` to confirm the conformance and
# corpus suites are really executing — a skipped suite and a passing one look
# identical in the summary line, which is how 500 documents went unread.
[doc("As test-all, but a missing the reference corpus is a failure rather than a skip.")]
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
    cargo bench -p en16931-formats --all-features

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
    # nothing beyond what the model already has: `billing` 0.16 depends on
    # `rust_decimal` alone, which is `en16931`'s own. If that ever stops being
    # true the adapter's opening paragraph stops being true with it. Two
    # `billing`s in the graph would also show here, as one more crate.
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
    # Everything under these is source; `site/public/` and the reference corpus are not.
    # `.cargo/` is in the list because it carries `config.toml`, which defines
    # where the reference corpus lives.
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

# ── Do the roadmap and the specifications agree? ───────────────────────────────

# The roadmap and the specifications agree, and nothing published points into
# them. Local only, like `tracked`: `concepts/`, `specs/` and `.specify/` are
# gitignored, so CI has neither side to compare.
#
# 1. Every roadmap item (### `#anchor`) in concepts/ROADMAP.md names one spec
#    folder on a **Specified as:** line, and every folder under specs/ (outside
#    archive/) is named by exactly one item.
# 2. Nothing public — crates' src, tests, examples, benches, the READMEs, the
#    CHANGELOG, the site, xtask, CI — cites a path into concepts/, specs/ or
#    .specify/: its reader cannot open it. The reference corpus is no exception;
#    its location lives in .cargo/config.toml (`EN16931_REFERENCE`) and the code
#    reads the variable.
[doc("Check ROADMAP ↔ specs/, and that nothing published cites internal paths.")]
concepts-check:
    #!/usr/bin/env bash
    set -euo pipefail
    fail=0; roadmap=concepts/ROADMAP.md
    [ -f "$roadmap" ] && [ -d specs ] || { echo "concepts-check: skipped (no internal concepts/ and specs/ in this checkout)"; exit 0; }
    items=$(grep -cE '^### `#[a-z0-9-]+`' "$roadmap" || true)
    named=$(grep -oE '^\*\*Specified as:\*\* `[0-9]{3}-[a-z0-9-]+`' "$roadmap" | sed -E 's/.*`([^`]+)`/\1/' | sort)
    n=$(printf '%s\n' "$named" | grep -c . || true)
    [ "$items" -eq "$n" ] || { echo "✗ $roadmap: $items items but $n **Specified as:** lines" >&2; fail=1; }
    dupes=$(printf '%s\n' "$named" | uniq -d)
    [ -z "$dupes" ] || { echo "✗ named by more than one item: $dupes" >&2; fail=1; }
    for s in $named; do [ -f "specs/$s/spec.md" ] || { echo "✗ $roadmap names $s, but specs/$s/spec.md does not exist" >&2; fail=1; }; done
    for d in specs/[0-9][0-9][0-9]-*/; do
        [ -d "$d" ] || continue; s=$(basename "$d")
        printf '%s\n' "$named" | grep -qx "$s" || { echo "✗ specs/$s is named by no roadmap item" >&2; fail=1; }
    done
    leaks=$(git grep -nP '(?<![\w./-])(concepts/|specs/|\.specify/)|\bconcepts\b|\b(DECISIONS|RISKS|ROADMAP|OVERVIEW|MARKET_LANDSCAPE|VERIFICATION|CODEGEN|SYNTAXES|INTEGRATION|REFERENCES)\.md\b|\b(constitution|speckit)\b' -- \
        README.md CHANGELOG.md crates site/content xtask .github || true)
    [ -z "$leaks" ] || { echo "✗ a public file names an internal document or path:" >&2; echo "$leaks" | sed 's/^/  /' >&2; fail=1; }
    [ "$fail" -eq 0 ] || exit 1
    echo "✓ $items roadmap items, each with its specification; nothing public names an internal document or path"

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

# Security advisories. Nothing is set aside: there is no `.cargo/audit.toml`,
# so every advisory and every yanked crate in `Cargo.lock` fails this.
[doc("Security advisories over Cargo.lock; nothing is ignored.")]
audit:
    cargo audit --deny warnings

# Licence and dependency policy, over the union of every crate's graph.
deny:
    cargo deny --all-features check

# ── Fuzzing (cargo-fuzz, nightly; `fuzz/` is its own unpublished workspace) ───
#
# Four targets: `read` (any bytes through `read`: UBL, CII or PDF), `extract` (a
# PDF under small limits; what comes back stays within them), `roundtrip` (a
# generated invoice written as UBL and CII and read back), `embed` (a fuzzed
# PDF/A as `embed`'s input). CI builds them and runs each for 60 s from the
# committed seeds; a long run is local, before a release.

# Build every target. Skips, saying so, without nightly or cargo-fuzz.
fuzz-build:
    #!/usr/bin/env bash
    if ! cargo +nightly fuzz --version >/dev/null 2>&1; then
        echo "skipped: needs nightly and cargo-fuzz (cargo install cargo-fuzz --locked)"; exit 0
    fi
    # The host triple, explicitly: a prebuilt cargo-fuzz defaults to its own (musl) target.
    cargo +nightly fuzz build --target "$(rustc +nightly -vV | sed -n 's/^host: //p')"

# Run one target for SECS seconds, e.g. `just fuzz read 3600`. New inputs go to
# target/fuzz-corpus/TARGET; the committed seeds stay as they are. With the
# reference corpus present, its documents are read as further seeds (never
# copied into the repository).
[doc("Run one fuzz target for SECS seconds (read, extract, roundtrip, embed).")]
fuzz TARGET SECS="60":
    #!/usr/bin/env bash
    set -euo pipefail
    out="{{ justfile_directory() }}/target/fuzz-corpus/{{ TARGET }}"
    mkdir -p "$out"
    seeds=("{{ justfile_directory() }}/fuzz/seeds/{{ TARGET }}")
    ref="${EN16931_REFERENCE:-{{ justfile_directory() }}/$(sed -n 's/^EN16931_REFERENCE *= *{ *value *= *"\([^"]*\)".*/\1/p' .cargo/config.toml)}"
    case "{{ TARGET }}" in
        read)          extra=("$ref/eInvoicing-EN16931/test/testfiles" "$ref/zugferd-corpus") ;;
        extract|embed) extra=("$ref/zugferd-corpus") ;;
        *)             extra=() ;;
    esac
    # `${a[@]+…}`: bash 3.2 (macOS) calls an empty array unbound under `set -u`.
    for d in ${extra[@]+"${extra[@]}"}; do [ -d "$d" ] && seeds+=("$d"); done
    cargo +nightly fuzz run --target "$(rustc +nightly -vV | sed -n 's/^host: //p')" {{ TARGET }} \
        "$out" "${seeds[@]}" -- -max_total_time={{ SECS }} -timeout=10 -rss_limit_mb=2048 -malloc_limit_mb=2048

# ── Mutation testing (cargo-mutants; config in .cargo/mutants.toml) ───────────
#
# Local only, not in CI. `mutants-diff` mutates the lines a change touches. The full
# sweep (`mutants`) is optional and slow — run it by hand, or through the
# `mutants.yml` workflow. Survivors are recorded in .cargo/mutants-baseline.txt,
# one `survivor  # reason` per line; a run fails on a survivor that is not
# recorded, and the full sweep also on a recorded one that is now caught.
#
# One job, a per-mutant timeout, and only the fast suites (no reference corpus,
# no Docker — see the config): a mutant that breaks a bound turns a hostile-input test into an
# endless walk or a bomb, and those tests run their hostile call in a child
# process under a 512 MiB / 10 s watchdog, so such a mutant is caught rather
# than taking the machine.

# What the full sweep mutates: the hand-written rules.
mutants_scope := "crates/en16931/src/validation/rules/*.rs"

# The full sweep of the rules — optional and slow (about an hour): FAIL on a
# new survivor and on a recorded one that is now caught.
[doc("The full mutation sweep of the rules — optional and slow (about an hour).")]
mutants:
    #!/usr/bin/env bash
    set -uo pipefail
    rm -rf target/mutants-full
    EN16931_REFERENCE="$PWD/target/no-reference-corpus" \
        cargo mutants --jobs 1 --timeout 120 -f '{{ mutants_scope }}' --output target/mutants-full
    just _mutants-check target/mutants-full exact $?

# Mutate only the lines DIFF touches; FAIL on any survivor the baseline does not record.
mutants-diff-gate DIFF:
    #!/usr/bin/env bash
    set -uo pipefail
    rm -rf target/mutants-diff
    EN16931_REFERENCE="$PWD/target/no-reference-corpus" \
        cargo mutants --no-shuffle -vV --in-diff {{ DIFF }} --jobs 1 --timeout 120 \
        --output target/mutants-diff
    just _mutants-check target/mutants-diff subset $?

# What CI runs on a pull request, for what changed since BASE (committed and not).
mutants-diff BASE="origin/main":
    mkdir -p target && git diff {{ BASE }} > target/mutants.diff
    just mutants-diff-gate target/mutants.diff

# Rewrite the baseline from the last full sweep, keeping every recorded reason.
mutants-baseline-update:
    python3 -I scripts/mutants.py update

[private]
_mutants-check OUTDIR MODE STATUS:
    python3 -I scripts/mutants.py check {{ OUTDIR }} {{ MODE }} {{ STATUS }}

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
ci: fmt-check lint features doc test-artefacts test-no-features codegen-check wasm deps tracked site-check concepts-check
    @echo "✓ all green"
