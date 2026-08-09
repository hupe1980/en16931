+++
title = "en16931"
description = "The EN 16931 European e-invoice as Rust types: 317 business rules, UBL 2.1 and UN/CEFACT CII in both directions, XRechnung, Peppol BIS Billing 3.0 and ZUGFeRD / Factur-X. No XML in the model, no I/O, wasm32."
template = "index.html"

[extra]
lede = "Validate the invoice, not the document. Findings point at BT-151 on line 3 — never at an XPath — because the rules run against the semantic model instead of a serialised file. Two libraries and a single static binary."

[[extra.stats]]
n = "317"
k = "business rules, all exercised"

[[extra.stats]]
n = "100 %"
k = "agreement with CEN's and KoSIT's own suites"

[[extra.stats]]
n = "486"
k = "published documents read, written and crossed"

[[extra.stats]]
n = "2 µs"
k = "to validate a five-line invoice"
+++

## What this is

From 2025 onwards a German business must be able to *receive* a structured
e-invoice, and from 2027–2028 it must send one. The rest of the EU is arriving at
the same place through ViDA. The format is **EN 16931** — a semantic data model
of 164 business terms, a few hundred business rules, and two mandatory XML
syntaxes that say the same thing in different words.

Almost every implementation of it is *XML in, Schematron out*. That works, and it
means your development loop is: build the document, serialise it, validate it,
parse the error, and guess which field the XPath meant.

These crates invert that. The **model** is the primary artefact and the rules run
against it, so a finding says `BG-25[2]/BT-151` — the language the standard, the
rules and you already speak. You can validate an invoice you are still
assembling, before a single byte of XML exists.

```rust
let report = en16931::validate(&invoice);

assert!(report.has("BR-16"));                       // no invoice line
assert_eq!(report.fatal().next().unwrap().path.to_string(), "BT-1");
```

## Three crates

| | |
|---|---|
| **`en16931`** | the semantic model, the rules, the typed proof of validity. Two dependencies, no XML, no I/O, builds for `wasm32`. |
| **`en16931-formats`** | UBL 2.1 and UN/CEFACT CII in both directions, XRechnung, ZUGFeRD / Factur-X. Re-implements none of the 317 rules. |
| **`en16931-cli`** | the same thing as a command, with exit codes a CI job can branch on. |

Most people need only the first. Reach for the second when a document has to
cross a wire, and the third when what you have is a file and a question.

## What makes it different

**The type system retires rules.** `InvoiceAmount` is `i64` minor units, so a
third decimal is not representable — and the 21 `BR-DEC-*` rules that forbid one
cannot fire. `PaymentMeans` is an enum over BG-17/18/19, so the three XRechnung
rules forbidding two at once have nothing left to check. Fifty-three rules are
retired this way: encoded as *types*, not as runtime checks.

**Nothing is claimed that is not measured.** The rule coverage, the code lists,
the element order, the prohibition tables and the severity levels are all
generated or checked against CEN's, KoSIT's and OpenPeppol's own artefacts, and
CI fails if a committed table drifts. The conformance suites run the
authorities' own test corpora — 1 013 CEN unit-test assertions, 381 KoSIT
mutations, 58 published invoices — and agreement is asserted **exactly**.

**Severity comes from the authority, not from us.** KoSIT's validator
configuration re-levels nine CEN rules; `BR-CL-23` is a *warning* for every
XRechnung scenario because CEN's unit-code table lags UN/ECE's. A validator that
reports it as fatal rejects invoices Germany accepts.

**Two documents can be compared as invoices.** `en16931 diff a.xml b.cii.xml`
reads both into the model first, so a UBL invoice and its CII translation come
out *identical* — where a textual diff of the same pair shares almost nothing.
No XML-first tool can answer that question, because it never has the invoice.

**A proof you cannot forge.** `Validated<XRechnung>` is a type. A serialiser can
demand one and then physically cannot be handed an unchecked invoice, or one
checked against a different profile.
