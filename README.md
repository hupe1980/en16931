# 🇪🇺 en16931

**The European e-invoice in Rust** — the EN 16931 semantic data model and its
business rules, and the UBL / CII / ZUGFeRD bindings that carry them on the wire.

Two crates, one workspace.

| Crate | | |
|---|---|---|
| [**`en16931`**](crates/en16931) | the semantic model, 316 business rules, the typed proof of validity | 2 dependencies · no XML · no I/O · `wasm32` |
| [**`en16931-formats`**](crates/en16931-formats) | UBL 2.1 and UN/CEFACT CII in both directions, the XRechnung CIUS, ZUGFeRD / Factur-X | +1 339 syntax rules · re-implements none of the 316 |

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
   │    └─────────────────────┘                           │
    ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─
```

**Start with [`crates/en16931`](crates/en16931).** If your system already holds
the invoice as data — from an ERP, from a billing engine, from your own types —
that crate gives you the verdict and the proof, and that is the whole job.
Reach for `en16931-formats` only when a document has to cross a wire.

---

`en16931-formats` depends on `en16931`, never the reverse — so the semantic
rules cannot acquire a syntax, and the model crate's graph stays at ten crates
and reaches `wasm32`. Cargo enforces that between *crates*, which is why they
are two, and why one repository costs nothing.

---

## Development

[`just`](https://just.systems) is the task runner; `just` alone lists every
recipe. Everything runs from this directory.

```sh
just spec            # fetch the CEN / KoSIT / Peppol artefacts into ./spec/ — once, for both crates
just test-all        # every crate, every feature
just ci              # everything CI runs, locally
just codegen-check   # fail if any generated table drifted from the artefacts
just wasm            # en16931 still builds for wasm32
```

`spec/` is **not committed**: the CEN artefacts are EUPL-1.2, a reciprocal
licence, and keeping them out of the tree is what keeps both crates
`MIT OR Apache-2.0`. The suites that need it skip without it, and CI sets
`EN16931_REQUIRE_SPEC=1` so a skip there is a **failure** — `just test-artefacts`
does the same locally. A skipped conformance run and a passing one are the same
summary line, which is exactly how 490 unread documents stay green.

Five files are generated from those artefacts and none is written by hand: the
code lists for `en16931`, and the element-order and prohibition tables for each
syntax in `en16931-formats`. `cargo xtask check` re-derives all five and fails if
a committed one differs.

### Releasing

Both crates share one version (`[workspace.package]`), so one tag releases both:

```sh
git tag v0.2.0 && git push --tags
```

They publish in dependency order. That order is not a convention to remember:
`en16931-formats` requires `en16931` by version as well as by path, so
crates.io rejects it until the model crate is up.

---

## Attribution

Both crates implement the semantic data model of EN 16931-1 and the two
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
