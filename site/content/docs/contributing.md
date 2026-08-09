+++
title = "Contributing"
weight = 10
description = "Set up the en16931 workspace: fetch the CEN, KoSIT and Peppol artefacts, run the full CI gate locally, and regenerate the tables derived from the standard."
+++

Three crates, one workspace, one version. [`just`](https://just.systems) is the
task runner; `just` alone lists every recipe.

```sh
git clone https://github.com/hupe1980/en16931
cd en16931

just spec            # fetch the CEN / KoSIT / Peppol artefacts into ./spec/ — once
just ci              # everything CI runs, locally
```

## The artefacts

`spec/` is **not committed**. The CEN artefacts are EUPL-1.2, a reciprocal
licence, and keeping them out of the tree is what keeps every crate here
`MIT OR Apache-2.0`.

The suites that need it skip without it, so `just spec` is the first thing to
run. CI sets `EN16931_REQUIRE_SPEC=1`, which turns a skip into a **failure**;
`just test-artefacts` holds you to the same standard locally. A skipped
conformance run and a passing one produce the same summary line, which is exactly
how 486 unread documents stay green.

## The recipes worth knowing

| | |
|---|---|
| `just test` | unit and doc tests, default features — the fast loop |
| `just test-all` | every crate, every feature |
| `just test-artefacts` | as above, but a missing `spec/` fails |
| `just lint` | Clippy over every target and feature, warnings as errors |
| `just doc` | rustdoc with `-D warnings`, which is what catches a broken intra-doc link |
| `just wasm` | `en16931` still builds for `wasm32` |
| `just deps` | every documented dependency-graph size, measured |
| `just codegen-check` | fail if a generated table drifted from the artefacts |
| `just bench` | the criterion benchmarks |
| `just site` | build this site with [Zola](https://www.getzola.org) |
| `just ci` | all of the above that CI runs |

Two of those are less obvious than they look.

**`just deps`** measures the dependency-graph sizes the documentation quotes.
They had already drifted once — the ZUGFeRD graph was documented as 56 in three
places and 57 in two, and it is 57. A number repeated in five files is a number
nobody rechecks, so it is checked here instead. Raising a limit is a decision,
not a chore: the small graph is why `en16931` reaches `wasm32`, and why the PDF
parser is behind a non-default feature.

**`just wasm`** builds `-p en16931`, never `--workspace`. The formats crate pulls
a PDF parser that does not build for that target and is not meant to; a
workspace-wide wasm build would fail for a reason that says nothing about the
model crate. Running it per crate is not tidiness, it *is* the assertion.

## The documentation is tested

Every figure in this project is measured — and then written down, in as many as
six places: three crate READMEs, two `lib.rs` headers and this site. Measuring
once and copying six times is how a project ends up asserting, in prose, things
that stopped being true. It is not hypothetical; all of these had drifted:

<!-- doc-numbers: historical -->

| | was documented | measured |
|---|---|---|
| profile check counts | 226 / 281 / 289 / 295 / 272 | one higher, all five |
| rules retired by the types | 36 | 53 |
| rules registered | 149 | 317 |
| declared divergences | 13 | 11 |
| this crate's own rules | three | four |
| published corpus documents | 490 | 486 |

<!-- /doc-numbers -->

So the prose stays — it is what makes a number mean something — and the numbers
in it are read back and compared against the code that produces them. The
scanner walks every README, every `lib.rs` and every page of this site, so a
*new* mention of a figure is checked the day it is written.

If you reword a sentence containing one of these numbers, the test will tell you
it stopped matching rather than silently checking nothing.

## Generated code

Five files are derived from the artefacts and none is written by hand: the code
lists for `en16931`, and the element-order and prohibition tables for each syntax
in `en16931-formats`.

```sh
cargo xtask codegen     # regenerate all five
cargo xtask check       # fail if a committed one differs
```

Each generator exits non-zero rather than emitting a table it could not derive
cleanly. If you change one, change the generator — a hand-edit will be reverted
by the next `codegen` run and caught by CI in between.

## Adding a rule

Every registered rule must either have a fixture that makes it **fire**, or be a
rule the type system retires so that no document can trigger it. There is no
third category, and the coverage gate enforces all three directions: it fails if
a rule is uncovered and undeclared, if a declared rule has since become covered,
and if anything is declared for a reason other than being type-retired.

A rule nobody has seen fire may be inverted, unreachable, or checking the wrong
field, and a suite of valid documents would be green either way.

If the rule is this crate's own rather than an authority's, namespace it `EN-*`.
A `BR-` id that CEN did not publish is indistinguishable from one that it did,
and someone will eventually go looking for it in CEN's index.

## Releasing

All three crates share one version, so one tag releases all of them:

```sh
git tag v0.3.0 && git push --tags
```

They publish in dependency order, and that order is not a convention to remember:
each crate requires the one below it by version as well as by path, so crates.io
rejects a publish until the model crate is up.

## This site

[Zola](https://www.getzola.org), in `site/`. One stylesheet, no framework, no
webfont, no external request.

```sh
just site-serve      # localhost with live reload
just site            # build into site/public
```

It deploys to GitHub Pages from `main` on every push that touches `site/`.
