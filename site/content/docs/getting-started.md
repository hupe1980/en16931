+++
title = "Getting started"
weight = 1
description = "Install the en16931 crates or the CLI, build your first EN 16931 invoice, reconcile the totals and read the validation report."
+++

Two ways in. If what you have is a **file and a question**, use the command. If
what you have is **data in a program**, use the library.

## The command

```sh
cargo install en16931-cli
```

One binary, named `en16931`. It reads UBL, CII and ZUGFeRD PDFs, and picks the
rule set from the document's own BT-24 customization identifier.

```sh
en16931 validate rechnung.xml                # accept, reject or undefined — and why
en16931 validate rechnung.xml -r xrechnung   # …and require a profile of it
en16931 inspect  rechnung.pdf                # what *is* this file?
en16931 convert  rechnung.xml --to cii       # through the model; refuses to lose anything
en16931 extract  rechnung.pdf                # the embedded XML, verbatim
en16931 embed    gerendert.pdf rechnung.xml -o rechnung.pdf   # a ZUGFeRD hybrid
en16931 explain  BR-CO-14                    # what does this rule say, and who runs it?
```

Exit `0` accepted, `1` rejected or undefined, `2` a file that could not be
opened — so a pipeline can tell *this invoice is wrong* from *that path does not
exist*. The [CLI page](@/docs/cli.md) covers every subcommand.

## The library

```toml
[dependencies]
en16931 = "0.9"
rust_decimal = "1"
```

Two dependencies — `rust_decimal` and `thiserror`. No XML parser, no I/O, no
async, no `unsafe`, and it builds for `wasm32`, so an invoice can be validated in
a browser without leaving the client.

Add the syntax layer only if a document has to cross a wire:

```toml
en16931-formats = { version = "0.9", features = ["ubl", "cii"] }
```

## A first invoice

Two lines at two VAT rates, built, reconciled and validated. Nothing is elided:

```rust
use en16931::invoice::{Party, PaymentInstructions, PostalAddress};
use en16931::{Date, Identifier, InvoiceAmount, Percentage, Quantity, prelude::*};
use rust_decimal::dec;

let seller = Party::new(
        "Stadtwerke Musterstadt GmbH",
        PostalAddress::new("Hauptstraße 1", "Musterstadt", "12345", "DE")?,  // BR-09
    )
    .with_vat_identifier("DE123456789")
    .with_electronic_address(Identifier::eas("4012345000009", "0088")?);    // GLN
let buyer = Party::new(
        "Beispiel AG",
        PostalAddress::new("Marktplatz 2", "Beispielstadt", "54321", "DE")?, // BR-11
    )
    .with_electronic_address(Identifier::eas("991-01234-56", "0204")?);

let invoice = Invoice::builder(
        "urn:cen.eu:en16931:2017",       // BT-24 — which rule set applies
        "R-2026-0001",                   // BT-1
        Date::parse("2026-07-31")?,      // BT-2
        "380",                           // BT-3 — commercial invoice
        "EUR",                           // BT-5
    )
    .seller(seller)
    .buyer(buyer)
    .payment(PaymentInstructions::sepa_credit_transfer("DE02120300000000202051", None))
    .due_in_days(14)                     // BT-9 — satisfies BR-CO-25
    .line(InvoiceLine::new(
        "1", "Netznutzung Arbeitspreis",
        Quantity::new(dec!(10000)), "KWH",
        InvoiceAmount::parse("2890.00")?,
        "S", Some(Percentage::new(dec!(19))),
    ))
    .line(InvoiceLine::new(
        "2", "Messstellenbetrieb",
        Quantity::new(dec!(12)), "MON",
        InvoiceAmount::parse("120.00")?,
        "S", Some(Percentage::new(dec!(7))),
    ))
    .build_reconciled()?;

assert_eq!(invoice.vat_breakdown.len(), 2);          // one group per rate
assert_eq!(invoice.totals.gross_total.to_string(), "3567.50");
assert!(validate(&invoice).is_valid());
```

Three things in there are worth pausing on.

**`Identifier::eas` is checked at the call**, not at validation time. `9958`,
withdrawn on 2023-07-31, is rejected with its successor named; for a GLN
(`0088`) the check digit is checked too.

**`build_reconciled` computes BG-22 and BG-23**, the document totals and the VAT
breakdown. They are a *function* of the lines — `BR-CO-10` … `BR-CO-16`, the
`-08` and `-09` rows of all nine VAT category families, and `BR-CO-18` are that
one function checked from the outside. If your engine already produced the
positions, it should not also have to know that BT-107 is *absent* rather than
zero when there are no allowances. Use `.build()` instead when the totals came
from somewhere authoritative and you want them checked rather than replaced.

**Mandatory terms are arguments.** BT-24, BT-1, BT-2, BT-3 and BT-5 are
arguments to `builder`, so code that builds an invoice cannot forget them. In
`Invoice` itself they are `Option`, so that a received document missing one can
still be read and reported on (`BR-01` … `BR-05`).

**Or start from a valid invoice.** `en16931::examples::for_profile(&profile)`
returns a valid invoice for every shipped profile, and `examples::annex_a()` the
eight worked examples of EN 16931-1 Annex A. Change one field and the report
names exactly what that change broke:

```rust
use en16931::{examples, profiles};

let mut invoice = examples::for_profile(&profiles::XRECHNUNG).expect("shipped");
invoice.buyer_reference = None;                     // drop the Leitweg-ID
let report = profiles::XRECHNUNG.validate(&invoice);
assert_eq!(report.fatal().map(|f| f.rule.as_str()).collect::<Vec<_>>(), ["BR-DE-15"]);
```

## Reading a report

```rust
use en16931::{validate, prelude::*};

let invoice = Invoice::default();          // nothing filled in
let report = validate(&invoice);

assert!(!report.is_valid());
assert!(report.has("BR-02"));              // no invoice number
assert!(report.has("BR-16"));              // no invoice line

// Findings point at business terms, never at an XPath.
assert_eq!(report.fatal().next().unwrap().path.to_string(), "BT-1");
```

A report carries **every** finding, ordered stably so a CI diff means something.
`report.into_result()?` is there for the ergonomic path, but the report is the
product: a rejection from a clearing platform lists every problem at once, and a
validator that reports one is a validator you run in a loop.

Rule ids normalise, because the standard and the artefacts spell them
differently — EN 16931-1 writes `BR-CO-4`, the CEN artefacts write `BR-CO-04`:

```rust
use en16931::validation::rules;

assert_eq!(rules::explain("BR-CO-4").map(|r| r.id.as_str()), Some("BR-CO-04"));
assert_eq!(rules::explain("br-1").map(|r| r.id.as_str()), Some("BR-01"));
```

## Then a profile

Plain `validate` runs core EN 16931. A German public-sector invoice must also
satisfy XRechnung, and that is a different call — or, better, a *proof*:

```rust
use en16931::profiles::XRechnung;
use en16931::validation::profile::Validated;

let proof: Validated<XRechnung> = Validated::new(invoice)?;   // Err carries the report
```

`Validated<XRechnung>` is a type you cannot forge. Hand it to a serialiser and
that serialiser physically cannot be given an unchecked invoice, or one checked
against a different profile. See [profiles](@/docs/profiles.md).

## Then a document

When the invoice has to cross a wire, `en16931-formats` has one call per task.
Receiving is the one a mandate makes everyone do — bytes and a policy in, a
decision out:

```rust
use en16931::profiles;
use en16931_formats::{Policy, Recommendation, Syntax};

let policy = Policy { require: Some(&profiles::XRECHNUNG), ..Policy::default() };
let got = en16931_formats::receive(&bytes, &policy);
if got.recommendation != Recommendation::Accept {
    eprintln!("{got}");                 // the decision, why, and every layer's report
}

let out = en16931_formats::write(&proof, Syntax::Ubl);   // a proof in, BT-24 stamped
```

See [syntaxes](@/docs/syntaxes.md) for reading, writing and converting, and
[ZUGFeRD](@/docs/zugferd.md) for hybrid PDFs.

## See also

- **[The semantic model](@/docs/model.md)** — business terms, the ten data types, and why amounts are not `f64`.
- **[Validation](@/docs/validation.md)** — the 334 rules, severity, tolerance regimes, and reports you can store and diff.
- **[Profiles](@/docs/profiles.md)** — XRechnung, Peppol BIS Billing 3.0, CIUS versus Extension.
- **[Syntaxes](@/docs/syntaxes.md)** — UBL 2.1 and UN/CEFACT CII, both directions.
- **[Conformance](@/docs/conformance.md)** — what is measured, and against whose artefacts.
