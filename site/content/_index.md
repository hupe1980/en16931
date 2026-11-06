+++
title = "en16931"
description = "The EN 16931 European e-invoice as Rust types: 334 business rules, UBL 2.1 and UN/CEFACT CII in both directions, XRechnung, Peppol BIS Billing 3.0 and ZUGFeRD / Factur-X. No XML in the model, no I/O, wasm32."
template = "index.html"

[extra]
lede = "Validate the invoice, not the document. Findings point at BT-151 on line 3 — never at an XPath — because the rules run against the semantic model instead of a serialised file. Two libraries and a single static binary."
stats = [
  { n = "334", k = "rules registered" },
  { n = "277", k = "rules exercised by a failing fixture" },
  { n = "1 027", k = "CEN unit-test assertions run, none disagreeing" },
  { n = "500", k = "published documents read, written and crossed" },
]
+++

## What this is

**EN 16931** is the European e-invoice: a semantic data model of business terms
and groups, business rules on it, and two mandatory XML syntaxes, UBL 2.1 and
UN/CEFACT CII. National usage specifications such as XRechnung and Peppol BIS
Billing 3.0 restrict it further.

These crates make the **model** the primary artefact and run the rules against
it, so a finding says `BG-25[2]/BT-151` and an invoice can be validated while it
is still being assembled, before any XML exists.

```rust
let report = en16931::validate(&invoice);

assert!(report.has("BR-16"));                       // no invoice line
assert_eq!(report.fatal().next().unwrap().path.to_string(), "BT-1");
```

## Three crates

| | |
|---|---|
| **`en16931`** | the semantic model, the rules, the typed proof of validity. Two dependencies, no XML, no I/O, builds for `wasm32`. |
| **`en16931-formats`** | UBL 2.1 and UN/CEFACT CII in both directions, ZUGFeRD / Factur-X extraction and embedding. Re-implements none of the 334 rules. |
| **`en16931-cli`** | the same as a command, with exit codes a CI job can branch on. |

## What makes it different

**Types retire rules.** `InvoiceAmount` holds exactly two decimals, so the 21
`BR-DEC-*` rules cannot fire; `PaymentMeans` is an enum, so the rules forbidding
two payment groups at once have nothing left to check. Of the 334 rules
registered, 53 are retired this way and 4 are undecidable at the semantic layer,
with the remaining 277 exercised by their own failing fixtures.

**Measured against the authorities.** Code lists, element order, prohibitions
and severities are generated from or checked against CEN's, KoSIT's and
OpenPeppol's artefacts at pinned releases. 1 027 CEN unit-test assertions run
with no disagreement (11 divergences declared), 381 KoSIT mutations agree, and
59 published invoices validate. See [conformance](@/docs/conformance.md).

**Severity from the authority.** KoSIT re-levels nine CEN rules for XRechnung;
`BR-CL-23` is a warning there. A validator that reports it as fatal rejects
invoices Germany accepts.

**A proof you cannot forge.** `Validated<XRechnung>` is a type. A serialiser
that demands one cannot be handed an unchecked invoice, or one checked against a
different profile.

**Two documents compared as invoices.** `en16931 diff a.xml b.cii.xml` reads
both into the model, so a UBL invoice and its CII translation compare equal.
