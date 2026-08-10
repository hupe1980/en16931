# 🇪🇺 en16931

**The European e-invoice in Rust** — the EN 16931 semantic data model and its
business rules, and the UBL / CII / ZUGFeRD bindings that carry them on the wire.

[**Documentation**](https://hupe1980.github.io/en16931) ·
[API reference](https://docs.rs/en16931) ·
[crates.io](https://crates.io/crates/en16931) ·
[Changelog](CHANGELOG.md)

Three crates, one workspace.

| Crate | | |
|---|---|---|
| [**`en16931`**](crates/en16931) | the semantic model, 317 business rules, the typed proof of validity | 2 dependencies · no XML · no I/O · `wasm32` |
| [**`en16931-formats`**](crates/en16931-formats) | UBL 2.1 and UN/CEFACT CII in both directions, the XRechnung CIUS, ZUGFeRD / Factur-X | +1 339 syntax rules · re-implements none of the 317 |
| [**`en16931-cli`**](crates/en16931-cli) | the same thing as a command: `validate`, `convert`, `diff`, `extract`, `inspect`, `explain` | one static binary · exit codes for CI |

```text
   ┌──────────────┐   ┌─────────────┐
   │   billing    │   │   your ERP  │
   │ calculations │   │             │
   └──────┬───────┘   └──────┬──────┘
          │ adapter (feature)│
          └─────────┬────────┘
                    ▼
        ┌─────────────────────┐
        │      en16931        │   build an Invoice, get a verdict.
        │  semantic model     │   Complete on its own — most users
        │  validation engine  │   need nothing else.
        │  proof of validity  │
        └─────────┬───────────┘
                  │  Validated<P> — the typed proof
                  ▼
   ╭ ─ only if you exchange documents ─ ─ ─ ─ ─ ─ ─ ╮
        ┌─────────────────────┐
   │    │   en16931-formats   │  parses inbound UBL/CII,   │
        │  UBL · CII · PDF/A  │  writes outbound
   │    └──────────┬──────────┘                           │
    ─ ─ ─ ─ ─ ─ ─ ─│─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─
                   ▼
        ┌─────────────────────┐
        │     en16931-cli     │   `en16931 validate rechnung.xml`
        └─────────────────────┘   no Rust required
```

**Start with [`crates/en16931`](crates/en16931).** If your system already holds
the invoice as data — from an ERP, from a billing engine, from your own types —
that crate gives you the verdict and the proof, and that is the whole job.
Reach for `en16931-formats` only when a document has to cross a wire.

**Or start with the command**, if what you have is a file and a question:

```sh
cargo install en16931-cli

en16931 validate rechnung.xml          # UBL, CII or a ZUGFeRD PDF; BT-24 picks the rule set
en16931 inspect  rechnung.pdf          # what *is* this file?
en16931 convert  rechnung.xml --to cii # through the model, not element by element
en16931 diff     ours.xml theirs.xml   # as invoices, not as XML — syntax is not a difference
en16931 explain  BR-CO-14              # what does this rule say, and who runs it?
en16931 rules --format json            # the whole catalogue, to diff across releases
```

Exit `0` valid, `1` invalid, `2` unreadable — so a pipeline can tell "this
invoice is wrong" from "that path does not exist".

---

`en16931-formats` depends on `en16931`, never the reverse — so the semantic
rules cannot acquire a syntax, and the model crate's graph stays at ten crates
and reaches `wasm32`. Cargo enforces that between *crates*, which is why the
boundary is a crate boundary, and why one repository costs nothing.

`en16931-cli` sits below both and is depended on by nothing, which is why it may
turn every feature on and take an argument parser: a binary is not in anybody's
dependency graph, so the reasoning that shapes the libraries' feature flags does
not apply to it. A validator that could not read the PDF you handed it because
of a compile-time flag would be a worse tool for no gain.

---

## Development

[`just`](https://just.systems) is the task runner; `just` alone lists every
recipe. Everything runs from this directory.

```sh
just spec            # fetch the CEN / KoSIT / Peppol artefacts into ./spec/ — once, for the workspace
just test-all        # every crate, every feature
just ci              # everything CI runs, locally
just codegen-check   # fail if any generated table drifted from the artefacts
just wasm            # en16931 still builds for wasm32
just features        # Clippy over every feature combination a consumer can select
just tracked         # fail if a source file is gitignored, or missing from git
just site-serve      # the documentation site (Zola, in ./site/) with live reload
```

`spec/` is **not committed**: the CEN artefacts are EUPL-1.2, a reciprocal
licence, and keeping them out of the tree is what keeps every crate here
`MIT OR Apache-2.0`. The suites that need it skip without it, and CI sets
`EN16931_REQUIRE_SPEC=1` so a skip there is a **failure** — `just test-artefacts`
does the same locally. A skipped conformance run and a passing one are the same
summary line, which is exactly how 486 unread documents stay green.

All four sources are pinned to **release tags**. Three used to track `master`,
which is not merely irreproducible: an authority's `master` is its *next*
release, and KoSIT's carried two severity overrides that appear in no published
one — so a crate claiming to report rules at the severities the authorities
publish was reading severities nobody had published.

Five files are generated from those artefacts and none is written by hand: the
code lists for `en16931`, and the element-order and prohibition tables for each
syntax in `en16931-formats`. `cargo xtask check` re-derives all five and fails if
a committed one differs.

The **documentation** is checked the same way. Every measured figure — rule
counts, the coverage split, code-list totals — is read back out of every README,
`lib.rs` and documentation page and compared against the code that produces it,
because a number written down in six places is a number nobody rechecks.

### Releasing

All three crates share one version (`[workspace.package]`), so one tag releases
all of them, and [`CHANGELOG.md`](CHANGELOG.md) carries one entry for the three:

```sh
# 1. move the Unreleased section of CHANGELOG.md under the new version
# 2. bump `[workspace.package] version` and the two dependency requirements
git tag v0.4.0 && git push --tags
```

They publish in dependency order. That order is not a convention to remember:
each crate requires the one below it by version as well as by path, so
crates.io rejects it until the model crate is up.

The changelog says *why*, not only *what*, and it is the one document that is
**not** scanned by the documented-number suite: recording what was true at each
release means holding superseded figures on purpose, which is the one thing that
scanner cannot tell from drift.

---

## Attribution

These crates implement the semantic data model of EN 16931-1 and the two
mandatory syntaxes listed in CEN/TS 16931-2. EN 16931-1 and CEN/TS 16931-2 are
made available free of charge by CEN and the European Commission under their
2018 licence agreement, which permits derivative use **on condition** that
derivative applications carry a statement to this effect. Copyright in the
standard remains with CEN.

That notice is a licence condition rather than decoration, so it is a `const`
with a test: it appears in the crate documentation, in `README.md`, and in the
header of every validation report, and `crates/en16931/tests/attribution.rs`
asserts all three still agree.

## Licence

`MIT OR Apache-2.0`, at your option. See [LICENSE-MIT](LICENSE-MIT) and
[LICENSE-APACHE](LICENSE-APACHE).
