# Changelog

All three published crates — [`en16931`], [`en16931-formats`] and
[`en16931-cli`] — share one version and one entry per release. They are released
from a single tag for the reason [`Cargo.toml`](Cargo.toml) gives: the bindings
re-export the model's types across their whole surface, so a breaking change to
the model breaks them either way, and two version numbers would only record that
fact twice.

The format is [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Before 1.0, a minor bump may break: the promise is that a break is **deliberate
and written down here**, not that there are none.

Entries say *why*, not only *what*. A line that reads "fixed rule count" tells a
reader upgrading nothing about whether it affected them.

[`en16931`]: https://crates.io/crates/en16931
[`en16931::fmt`]: https://docs.rs/en16931/latest/en16931/fmt/index.html
[`en16931-formats`]: https://crates.io/crates/en16931-formats
[`en16931-cli`]: https://crates.io/crates/en16931-cli

## [Unreleased]

Nothing yet.

## [0.5.0] — 2026-08-10

### Fixed

- **A second credit-transfer account no longer vanishes between two systems.**
  BG-17 is 0..n, and both schemas put the account at **0..1 per payment-means
  element** — several accounts are several `cac:PaymentMeans` /
  `ram:SpecifiedTradeSettlementPaymentMeans` elements, which is how CEN's own
  `guide-example1.xml` spells two accounts. Both writers packed every account
  into one element (schema-invalid output for two or more), and both readers
  assigned instead of accumulating, keeping only the **last** element's
  accounts. The two defects were mirror images, so the round-trip suite could
  not see either: an invoice with two bank accounts survived its own round
  trip and lost an account at any counterparty. The writers now emit one
  element per account, repeating BT-81 and BT-83 as CEN's example does; the
  readers merge repeated elements into the one BG-16. New round-trip and
  wire-shape tests pin both directions in both syntaxes.

- **Reading one of `BR-DE-23/24/25`'s forbidden combinations now reports the
  loss — and reaches KoSIT's verdict.** `PaymentMeans` is deliberately an
  enum, so a document carrying BG-17 *and* BG-18 (KoSIT ships these as
  mutation instances) cannot be held whole. Both readers used to let the last
  group win silently — which could satisfy the `-a` rule the document is built
  to fail. The first kind read now wins, the later kind is recorded in
  `unmapped`, and the `-a` rule fires on the mismatch, agreeing with the
  German reference validator on these files.

- **`Identifier::eas_checked`'s too-short-GLN message carried two
  fourteen-space runs** — the same collapsed-literal defect class documented
  on `ATTRIBUTION`, recurring in a new place. Fixed, and a tripwire test now
  asserts no registered rule text and none of `codes::guard`'s generated
  advice contains a run of spaces.

### Documentation

- **The ZUGFeRD writer guidance gained the second XMP pitfall**, reported back
  by the downstream team whose PDF/A-3 writer the guidance already served, and
  found only by veraPDF: the Factur-X extension-schema block is not a
  self-contained fragment. XMP allows each property once per packet and
  `pdfaExtension:schemas` is a property, so a generator that already writes
  extension schemas of its own (Typst/krilla does) already carries the bag, and
  pasting the fx description in as a second `rdf:Description` makes
  Adobe-lineage parsers and veraPDF reject the whole packet — the file silently
  stops being PDF/A, and neither an XML parser nor `en16931 validate` can see
  it. The fx `rdf:li` must merge into the existing `pdfaExtension:schemas` bag.
  In `zugferd`'s module documentation and the site's ZUGFeRD page.

- **The empty `ram:ApplicableHeaderTradeDelivery` is now defended at the code,
  with the authorities' own evidence.** An external validator run reported
  `PEPPOL-EN16931-R008` ("document MUST not contain empty elements") against
  the CII writer's output and proposed omitting the element. That proposal is
  wrong twice over, and the writer comment now says why so the next report can
  be answered from source: the D16B XSD gives the element no `minOccurs`, which
  defaults to **1** — omitting it fails schema validation outright — and
  Peppol BIS publishes no CII Schematron at all, so R008 reaches CII only
  through KoSIT's translation (`peppol-into-xr.xsl`), which authors the rule
  with a hand-written carve-out of exactly this element — under the comment
  "add R008 to CII", the context reads
  `//*[not(name() = 'ram:ApplicableHeaderTradeDelivery') and not(*) and
  not(normalize-space())]`. A validator flagging it is applying Peppol's
  UBL-targeted rule to CII without the authority's carve-out. Re-verified
  end-to-end: the 0.4.0 writer emits `<ram:ApplicableHeaderTradeDelivery/>`
  for a delivery-less invoice, exactly as the report observed — and that is
  the only output that satisfies both the XSD and the authority.

### Audited

- A full external audit pass on 2026-08-10 found the four artefact pins —
  CEN `validation-1.3.16`, Peppol `v3.0.20`, KoSIT Schematron `v2.5.0` and
  KoSIT validator configuration `v2026-01-31` — are each the **latest published
  release** of their repository, so nothing is stale. The next upstream move is
  XRechnung 4.0 (announced for late 2026, implementing EN 16931-1:2026), which
  per the pinning policy in `xtask/src/fetch.rs` will be a **new profile**, not
  a newer pin on the old one.

## [0.4.0] — 2026-08-10

The first release with a changelog, and the reason it needed one: **eight
defects, four of which produce a wrong answer rather than an error.** If you
upgrade one thing on this list, make it the formatting fix — it was reachable
from `{:.2}`.

### Security

- **A deeply-nested document aborted the process.** `roxmltree::Document::parse`
  recurses once per level of XML nesting and overflows the stack at a few
  hundred; a stack overflow is not a panic, so Rust cannot unwind it and cannot
  catch it. `en16931 validate theirs.xml` exited `134` instead of `2`, and any
  service embedding `ubl::from_str` or `cii::from_str` simply died. Two lines of
  XML from a counterparty were a denial of service on a library whose entire job
  is reading documents somebody else wrote.

  It cannot be handled after the fact, so it is refused before: both readers now
  measure nesting in one linear scan and return `Error::TooDeep` past
  `MAX_DEPTH`. The limit is 64; the deepest of the 487 published instances in the
  artefact tree is **9**, and `the_depth_limit_clears_every_published_document`
  measures that rather than assuming it.

  Two neighbouring attacks were already closed and are now pinned by tests so a
  future parser swap cannot re-open them: billion-laughs entity expansion and XXE
  file disclosure both need a DTD, and the parser rejects any document carrying
  one.

### Fixed

- **`{:.2}` on an amount of `1190.00` printed `11`.** Every `Display` in both
  crates went through `Formatter::pad`, which — because that is what precision
  means for a *string* — truncates to N **characters**. A caller writing `{:.2}`
  to get two decimal places got eleven euros; `{:>12.4}` got a wrong number
  neatly right-aligned in a column. Fifteen types were affected, including
  `Date` (`{:.4}` → `2026`) and `Path` (a truncated `BG-25[2]/BT-151` is a
  different location, not a shorter one).

  This crate refuses to round an amount at a boundary because a plausible wrong
  number is worse than an error. Printing one at a hundredth of its value is the
  same failure with a wider blast radius, and it was reachable from ordinary
  formatting syntax. Nothing truncates now: the new [`en16931::fmt`] module has
  the two helpers every `Display` here uses, and **precision on the numeric
  types is a minimum number of fraction digits** — `{:.4}` on `1190.00` is
  `1190.0000`, `{:.0}` is still `1190.00`. Padding is lossless; rounding for
  display would put a number on an invoice that differs from the one the invoice
  states.

  Found while acting on a downstream report that there was *no* way to ask these
  types for a decimal scale. There was — it just gave the wrong answer.

- **`Xmp::version` was documented as the ZUGFeRD version.** It is the version of
  the Factur-X **XMP schema**, and it has been the constant `1.0` since
  Factur-X 1.0 — a ZUGFeRD 2.3 file still carries `1.0`. Code comparing it
  against `"2.3"`, which the old comment invited, rejects every conforming file
  it is given. Reported downstream and confirmed against the reference
  implementation, which hardcodes it beside the producer string.

- **`Date` did not bound its year, so it disagreed with itself.** §6.5.9 cites
  ISO 8601's *calendar date complete representation* — four digits — and
  `Date::parse` enforced that while `Date::new` did not. `Date::new(50_000, 1, 1)`
  succeeded, printed `50000-01-01`, and `Date::parse` then rejected that value's
  own `Display` output. The year is now `0000..=9999`, with `Date::MIN` /
  `Date::MAX`.

- **`Check::prove` ignored the profile it was built for.** `prove::<P>()` read
  only its type parameter, so `Check::new(&profiles::XRECHNUNG).prove::<En16931>(inv)`
  announced an XRechnung run, evaluated the bare core rule set, and handed back a
  proof — and the mirror image silently ran the *stricter* set. The profile the
  caller named and the profile that ran were unrelated choices, in the one method
  whose entire job is to say which rule set a document passed. It now returns
  [`ProveError::WrongProfile`] instead, and `Check::of::<P>()` makes the mismatch
  unrepresentable.

- **Every documented per-profile check count was one higher than the tool
  printed.** `EN-EXT-01` was filtered out of the rule sequence whenever the
  target could represent the extension data an invoice carried — which, for an
  invoice carrying none, is every profile and every document. So five files
  quoted 282 checks for XRechnung and a report read `281 rule(s) checked`. The
  rule now runs like any other and its *finding* is withdrawn where a profile can
  hold the data, which is the only place the distinction was ever load-bearing.
  `ValidationReport::rules_checked()` is now exactly `Profile::check_ids().count()`
  — 227 / 282 / 290 / 296 / 273 — on every document.

- **`sniff` read the contents of an XML comment as markup.** A document opening
  `<!-- exported from <ERP> 4.2 -->` sniffed as `ERP` and came back `None`, so a
  perfectly ordinary UBL invoice was reported as not an e-invoice at all.
  Comments, processing instructions and doctypes are now each skipped by their
  own terminator.

- **Two of the seven documentation-number patterns had been matching nothing.**
  `check()` compared the *total* number of matches against the number of claims,
  which one popular pattern matching a dozen times satisfies on its own — so a
  claim whose sentence had been reworded stopped checking anything and the suite
  stayed green. Every pattern must now find its own sentence. Separately, the
  word-boundary rule was applied even to patterns that end in punctuation or a
  space, which made `"Code lists — <N> "` dead by construction.

- The deviations sample in `crates/en16931/README.md` showed `280 rule(s)
  checked` beside prose saying the count drops to 281, and neither was what the
  code printed. Pasted sample reports are now regenerated and matched against the
  documentation by `the_pasted_sample_reports_are_the_ones_the_code_prints`.

- **`en16931 rules --profile <name>` dropped the profile's restrictions.** It
  listed 270 of the 282 checks XRechnung declares, and the twelve missing were
  the `BR-DE-*` narrowings every German counterparty quotes. The text output at
  least said so in a footnote; the JSON — the shape people diff across releases —
  did not mention them at all. Both now list them, marked `profile` in the SOURCE
  column and carrying a `restriction` kind in the JSON, and the catalogue's total
  is now the same number `en16931 profiles` prints and a report prints.

- **`en16931 diff` reported two different invoices as identical.** Numbers
  serialise as strings — a JSON number is an `f64` in most readers — so the diff
  folds `25` and `25.0` together, which is right: the model does not think a VAT
  rate changed. It did that by comparing *parsed decimals*, which is a much wider
  claim, and going through `serde` means the type is gone by then. So **every**
  string term was compared as a number if it looked like one, and `"0001"` and
  `"1"` were the same invoice number — as were the post codes `01067` and `1067`,
  the line ids `01` and `1`, and the order reference `007`. The command printed
  *"identical as invoices"* and exited `0` on a conversion that had eaten a
  leading zero. A scale difference is a difference in *trailing fractional*
  zeros, and that is now what is compared.

- **The family table on the site did not add up.** It listed 47
  `PEPPOL-EN16931-*` where the registry holds 46 distinct ids — the duplicate is
  real (`R120` exists twice, once fatal for Peppol and once rewritten to a
  warning for XRechnung), which is exactly why counting it by hand went wrong —
  so the composition of 317 rules summed to 318. It is now derived as a
  **partition**: every registered rule is classified into exactly one bucket, the
  buckets are asserted to sum to the registry, and a family nobody documented
  fails the build.

- **A whole-group restriction invented a business-term id.** `BtId(0)` is the
  sentinel for *"this accessor stands for a group"*, and it was formatted like a
  term, so `BR-DE-1` reported *"PAYMENT INSTRUCTIONS (BG-16) **(BT-0)** shall be
  present"* — an id nobody can look up, in the text of a finding. The new
  `TermAccessor::label` is the one place a term is rendered, so a finding and the
  catalogue cannot describe the same narrowing two different ways.

- The benchmark module documented two benchmark ids — `core/5-lines` and
  `xrechnung/5-lines` — that have never existed, in the one file whose subject is
  measuring rather than asserting.

### Added

- `Check::of::<P>()` — start a run against the profile a marker type names, so
  the profile that is checked and the profile a proof claims are the same by
  construction. `Check::new` remains, for the runtime case a CLI or a service
  has.
- `Check::profile()`, for the profile a runtime-constructed `Check` will run.
- `TryFrom<Date> for time::Date`, under the `time` feature. The feature was
  documented as *"convert to and from `time::Date`"* and only ever went one way;
  its `chrono` twin had both directions from the start.
- `XRechnungCvd` and `XRechnungExtension` are re-exported from the crate root and
  the prelude, alongside the three markers that already were. They are the two
  profiles a German integrator reaches for after the CIUS, and being reachable
  only as `profiles::XRechnungCvd` read as though they were second-class.
- **[`en16931::fmt`]** — `padded` and `number`, the two `Display` helpers that
  replace `Formatter::pad` throughout both crates. Public because
  `en16931-formats` needs the same guarantee and one implementation is better
  than two, and because anyone writing a `Display` for a value that must not be
  silently shortened wants it too.
- **`Identifier::eas_checked`** and `codes::guard::eas_value`, which check the
  electronic address **value** against its EAS scheme — for the schemes whose
  format is fixed, published and self-verifying. Today that is `0088` GS1 GLN
  (13 digits, mod-10 check digit) and nothing else, and
  `guard::CHECKED_EAS_SCHEMES` / `guard::eas_value_is_checkable` say so, because
  `Ok` from a partial check means "verified" for one scheme and "nothing to
  verify" for the other hundred, and those are very different claims to build
  on.

  `Identifier::eas` validates the *scheme* and never the content — and neither
  does `BR-CL-25`. A downstream user put an eleven-digit BDEW
  Marktlokations-ID through `eas(malo, "0088")`; it returned `Ok`, validation
  passed, and the document went out asserting that an eleven-digit German
  metering identifier was a thirteen-digit GLN. Its doc comment now says what
  the call asserts, which is the half that was missing.
- `From<Date> for time::Date` and `From<Date> for chrono::NaiveDate`, both
  **infallible**. The `time` direction did not exist at all — its feature was
  documented as *"to and from"* and only ever went one way. The `chrono`
  direction was a `TryFrom` whose error arm no caller could reach. Bounding the
  year (above) is what made both total.
- `ubl::MAX_DEPTH` / `cii::MAX_DEPTH` and `Error::TooDeep` on both readers — see
  **Security** above.
- `TermAccessor::label()`, the one place a business term is rendered for a
  reader.
- A property that **every finding points somewhere that can exist**:
  `Group::repeats` says which groups may occur more than once, and an occurrence
  index means nothing in the others — `BG-4[3]` claims a fourth seller. Until
  this, `repeats` was public API whose only caller was its own unit test: a
  documented invariant with nothing enforcing it, which is how it came to
  disagree with the paths four rules were already emitting.
- Five new documentation claims covering the per-profile check counts, so the
  comparison table in `README.md`, `lib.rs`, the CLI's README and the site's
  profile page cannot drift from `check_ids()` again — and
  `the_pasted_sample_reports_are_the_ones_the_code_prints`, which regenerates
  sample terminal output and asks the documentation whether it still contains it.
- **The documented-number scan now reads every source file**, not the two crate
  roots. This project puts as much explanatory prose in module headers as in its
  READMEs — the twenty-one `BR-DEC-*` in `amount.rs`, the withdrawn-scheme table
  in `codes/guard.rs` — and rustdoc publishes all of it, so the most detailed
  prose in the workspace was the least checked. Scanning source turned up two
  scanner bugs of its own: a twenty-two-digit IBAN fixture *panicked* the parser,
  and a figure written with a non-breaking space was silently invisible — one
  character nobody can see switching off a check nobody would miss, in a helper
  whose own comment claimed the opposite.
- A **Deviations** section on the site's validation page. `Check` had no
  documentation outside rustdoc, which is a strange place to leave the API that
  decides whether a proof can be made.
- This file.

### Changed

- `ProveError` is `#[non_exhaustive]` and carries a new `WrongProfile` variant.
  An exhaustive `match` over it no longer compiles; add a `_` arm.
- `TryFrom<Date> for chrono::NaiveDate` is gone, replaced by an infallible
  `From`. `d.try_into()` becomes `d.into()`; a `?` on it no longer compiles.
- The `zugferd` feature is documented as **reading only** in the feature table,
  on crates.io and on the site. A downstream user read the one-word name,
  assumed a writer, found none in the summary, and reimplemented the extractor
  by hand before discovering `zugferd::extract` already did more than their
  version. The module docs always said this; the feature list is what people see
  first.
- The ⚠ provenance markers on the ZUGFeRD values now mean *"corroborated against
  the reference implementation, not against CEN"* rather than *"unchecked"* —
  see **Corroborated** below.
- **Performance.** Two allocations that bought nothing are gone.

  `validate()` built a 226-element `Vec<&Rule>` on the heap for every call on the
  common path, purely to drop one rule from the sequence; the iterator overload
  it needed already existed.

  The XML serialiser cloned `node.children` at every level so that
  `order_children` had something mutable to sort — and `Node` is recursive, so
  each clone was a deep copy of the subtree. Rendering a thousand-line invoice
  therefore copied the whole document, every string in it, once per level of
  nesting before writing a byte.

  The benchmark figures in the READMEs and on the site were re-measured on the
  machine this was done on, so they are not a before-and-after of these two
  changes and should not be read as one; `validate/core/5` is 1.50 µs there.

### Corroborated

- Every ⚠ value in `zugferd::{profile, extract}` was independently checked
  against the Factur-X reference implementation by a downstream user building a
  writer, and all but one are correct: the five level names (including the space
  in `EN 16931`), `FILENAMES` and their preference order, the four `fx:` XMP
  properties, and the observation that published guidance genuinely disagrees on
  `/AFRelationship` — so declining to pick a default is right rather than
  evasive. The exception is `Xmp::version`, fixed above.

  The two artefacts they used are now cited in the module docs, along with the
  XMP namespace URI (`urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#`,
  whose mixed case and trailing `#` are both load-bearing) — a value a writer
  needs and this crate does not currently hold.

  They also built a PDF/A-3 writer under the constraints this crate declines to
  work around, and report that the composition described in *"What composes
  today"* holds: delegate conformance to a generator that guarantees PDF/A-3,
  and add only the XMP afterwards. The decision not to ship a writer stands.

### Internal

- `tests/ubl.rs`, the test-only UBL reader the conformance suite parses the
  authorities' fixtures with, moved to `tests/common/ubl.rs`. Cargo makes every
  `tests/*.rs` an integration-test binary, so as a sibling file it was compiled
  twice and reported `0 passed; 0 failed` on every run — a suite that has never
  contained a test printing a green result line, which is the exact shape of the
  failure the conformance gate exists to prevent.

## [0.3.0] — 2026-08-10

### Added

- The documentation site (Zola, in `site/`), published to GitHub Pages, and
  `just site` / `site-serve` / `site-check` to build, serve and link-check it.
- `just tracked` and a CI job behind it: a source file that is gitignored, or
  present on disk and missing from git, now fails the build. A crate that
  compiles locally and not from a fresh clone is the failure this catches.
- `just audit` and `cargo audit` in CI, with every ignore in
  `.cargo/audit.toml` re-checked rather than accumulating.
- `just features`, running Clippy over every feature combination a consumer can
  actually select, rather than only the two ends of the range.

## [0.2.0] — 2026-07-31

### Changed

- **`en16931-formats` moved into this repository as a workspace member.** The two
  crates derive their tables from the same four upstream artefact repositories at
  the same pinned revision; split across two repositories that meant two
  `xtask/src/fetch.rs`, two `spec/` trees of the same 136 MB, and two `CEN_REF`
  constants kept in step by hand and by a comment. `every_artefact_pin_in_the_workspace_agrees`
  now checks it instead.
- The crate boundary is unchanged and still load-bearing: `en16931-formats`
  depends on `en16931`, so rustc forbids the reverse and *"the semantic rules do
  not depend on a syntax"* is enforced rather than requested.
- Coordinated breaking changes no longer need a `[patch.crates-io]` block, and
  `cargo publish` derives the release order from the dependency graph.

## [0.1.0] — 2026-07-31

First release. The EN 16931 semantic data model as Rust types, its
syntax-independent business rules, the UBL 2.1 and UN/CEFACT CII bindings in both
directions, ZUGFeRD / Factur-X extraction, and the command.

- The ten semantic data types of EN 16931-1 §6.5, one Rust type each, with the
  normative constraints in the type rather than in a rule: `InvoiceAmount` cannot
  hold a third decimal, and 21 `BR-DEC-*` assertions are retired by that alone.
- All 223 syntax-independent rules of CEN's pinned validation artefacts, plus
  this crate's own four `EN-*`, checked against the authorities' conformance
  suites rather than against their rule lists.
- Five profiles — EN 16931 core, XRechnung 3.0 and its CVD and Extension
  variants, Peppol BIS Billing 3.0 — modelled as §7.3.2 *restrictions* where the
  standard says restriction, at the severities the authorities publish.
- `Validated<P>`, the typed proof, with widening only where §4.4.4 actually
  grants it.
- `en16931-formats`: UBL and CII in both directions, element order and the 1 339
  syntax rules derived from CEN's own artefacts rather than transcribed, and a
  serialiser that cannot emit a prohibited element.
- `en16931-cli`: `validate`, `convert`, `diff`, `extract`, `inspect`, `explain`,
  `rules`, `profiles`, and CI-shaped exit codes.

[Unreleased]: https://github.com/hupe1980/en16931/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/hupe1980/en16931/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/hupe1980/en16931/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/hupe1980/en16931/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/hupe1980/en16931/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/hupe1980/en16931/releases/tag/v0.1.0
