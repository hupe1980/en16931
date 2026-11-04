# en16931-formats

**UBL 2.1, UN/CEFACT CII and ZUGFeRD / Factur-X for the EN 16931 e-invoice.**
Read, receive, write and convert documents in both syntaxes, and extract or
embed the invoice in a PDF/A-3 hybrid — on top of the
[`en16931`](https://crates.io/crates/en16931) semantic model, which owns every
business rule. This crate re-implements none of the 334.

[Documentation](https://hupe1980.github.io/en16931/docs/syntaxes/) ·
[API reference](https://docs.rs/en16931-formats) ·
[Changelog](https://github.com/hupe1980/en16931/blob/main/CHANGELOG.md)

## Install

```toml
[dependencies]
en16931 = "0.9"
en16931-formats = { version = "0.9", features = ["ubl", "cii"] }
```

| Feature | Default | Graph | What |
|---|---|---|---|
| `ubl` | ✅ | 13 crates | UBL 2.1 `Invoice` / `CreditNote`, both directions |
| `cii` | — | 13 crates | UN/CEFACT CII D16B, both directions |
| `zugferd` | — | 57 crates | ZUGFeRD / Factur-X: `extract` and `embed`; not `wasm32` |
| `serde` | — | + `serde` | `Serialize` / `Deserialize` on this crate's types |

The writers need no dependency; only reading pulls in an XML parser.

## One call per task

| Task | Call | Returns |
|---|---|---|
| read UBL, CII or a ZUGFeRD PDF | `read(&bytes)` | `Result<Read, Error>` |
| read under your own PDF limits | `read_with(&bytes, &Limits)` | `Result<Read, Error>` |
| receive and decide | `receive(&bytes, &Policy)` | `Received` |
| write a validated invoice | `write(&proof, Syntax::Ubl)` | `Written { xml, entries }` |
| write a model nobody validated | `write_unchecked(&invoice, Syntax::Cii)` | `Result<Written, Unwritable>` |
| convert between syntaxes | `convert(&bytes, Syntax::Cii, Loss::Refuse)` | `Result<Converted, ConvertError>` |
| extract a hybrid PDF's payload | `zugferd::extract(&pdf, &Limits::default())` | `Result<Extracted, zugferd::Error>` |
| embed into your PDF/A-3 | `zugferd::embed(&pdf, &proof, &EmbedOptions::default())` | `Result<Vec<u8>, EmbedError>` |

```rust,ignore
use en16931::profiles;
use en16931_formats::{Policy, Recommendation};

let policy = Policy { require: Some(&profiles::XRECHNUNG), ..Policy::default() };
let got = en16931_formats::receive(&bytes, &policy);
match got.recommendation {
    Recommendation::Accept => process(got.invoice.unwrap()),
    Recommendation::Reject => refuse(&got.reasons),
    // ZUGFeRD MINIMUM / BASIC WL: lawful, and not an EN 16931 invoice.
    Recommendation::Undefined => review(&got),
}
println!("{got}");   // the decision, why, and every layer's report
```

`receive` recommends as KoSIT's validator does: **reject** when a layer is
invalid, when BT-24 names no profile this build ships, names another process or
is ambiguous, or when a finding is fatal; **undefined** for ZUGFeRD MINIMUM and
BASIC WL; **accept** otherwise.

## The report

| Layer | Holds | Invalid when |
|---|---|---|
| container | a PDF's divergences from its payload | the PDF was refused, or its XMP and BT-24 disagree |
| syntax | unmapped elements, malformed values, missing required terms | a value or the document could not be read, or a required term is absent |
| schema | duplicates of single-occurrence elements, spellings the schema does not admit | any of them, unless CEN publishes the rule as a warning |
| semantic | the profile's findings | a finding is fatal |

Every item of the first three is a typed `report::Entry` — kind, syntax path,
business term, value, cited rule, message and hint. Nothing is dropped
silently: no public function returns a document without its entries or emits
one its schema rejects. `Received::to_interchange()` gives the whole result as
`en16931-report/4`, ready to store or to render as SVRL.

## Writing

```rust,ignore
use en16931::profiles::XRechnung;
use en16931::Validated;
use en16931_formats::Syntax;

let proof: Validated<XRechnung> = Validated::new(invoice)?;
let out = en16931_formats::write(&proof, Syntax::Ubl);   // BT-24 stamped from the proof
```

The element order comes from tables generated from the authorities' artefacts
(CII from CEN's D16B schema, UBL from the published instances), and every
element the writers emit is checked against the prohibitions extracted from
CEN's syntax rules.

## Reading hostile input

The XML parser rejects any document with a DTD, so entity expansion and XXE are
unreachable, and nesting deeper than `MAX_DEPTH` (64) is refused before parsing.
A PDF is read under `zugferd::Limits`: the defaults by `read`, the caller's by
`read_with` and by `receive` (`Policy::limits`).

| Limit | Default | Bounds |
|---|---|---|
| `max_stream_bytes` | 64 MiB | what any one stream may inflate to |
| `max_total_bytes` | 128 MiB | what one call may inflate in sum |
| `max_objects` | 10 000 | objects one walk over the PDF's structure may visit |

Input size and parse time are the caller's to bound.

## ZUGFeRD / Factur-X

`zugferd::extract` returns the payload verbatim, the profile it claims, the
PDF's XMP, and every `Divergence` between them — including a payload that is
attached but not associated (`/AF`), or not in the embedded-files tree.
`zugferd::embed` writes a validated EN 16931 or XRechnung invoice into a PDF/A-3
file you rendered, as one incremental update, and reads the result back before
returning it; the test suite validates its output with veraPDF. It never
renders and never converts to PDF/A.

## Examples

```sh
cargo run --example read_and_validate
cargo run --example write_both_syntaxes --features cii
cargo run --example zugferd_extract --features zugferd
```

## Documentation

- [Syntaxes: UBL and CII](https://hupe1980.github.io/en16931/docs/syntaxes/)
- [ZUGFeRD and Factur-X](https://hupe1980.github.io/en16931/docs/zugferd/)
- [Conformance](https://hupe1980.github.io/en16931/docs/conformance/)

Minimum supported Rust version: 1.88.

## Licence

MIT OR Apache-2.0, at your option. The CEN attribution notice is re-exported as
`en16931_formats::ATTRIBUTION`.
