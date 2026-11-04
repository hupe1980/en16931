# en16931

**The EN 16931 European e-invoice as Rust types:** the semantic data model, its
business rules, and the national profiles built on it — XRechnung 3.0 and
Peppol BIS Billing 3.0.

Build an invoice, reconcile its totals, validate it, and get a typed proof of
validity. Findings name business terms (`BG-25[2]/BT-151`), not XPaths, because
the rules run against the model rather than a serialised document. No XML, no
I/O, no `unsafe`; two dependencies, and it builds for `wasm32`.

[Documentation](https://hupe1980.github.io/en16931) ·
[API reference](https://docs.rs/en16931) ·
[Changelog](https://github.com/hupe1980/en16931/blob/main/CHANGELOG.md)

## Install

```toml
[dependencies]
en16931 = "0.9"
rust_decimal = "1"
```

Reading or writing UBL, CII or ZUGFeRD / Factur-X documents is
[`en16931-formats`](https://crates.io/crates/en16931-formats); the same as a
command is [`en16931-cli`](https://crates.io/crates/en16931-cli).

## Example

Two lines at two VAT rates, built, reconciled and validated:

```rust
use en16931::invoice::{Party, PaymentInstructions, PostalAddress};
use en16931::{Date, Identifier, InvoiceAmount, Percentage, Quantity, prelude::*};
use rust_decimal::dec;

# fn main() -> Result<(), Box<dyn std::error::Error>> {
// The country is checked here (BR-CL-14), and the EAS scheme and GLN check
// digit at `Identifier::eas`, not in the final report.
let seller = Party::new(
        "Stadtwerke Musterstadt GmbH",
        PostalAddress::new("Hauptstraße 1", "Musterstadt", "12345", "DE")?,
    )
    .with_vat_identifier("DE123456789")
    .with_electronic_address(Identifier::eas("4012345000009", "0088")?);
let buyer = Party::new(
        "Beispiel AG",
        PostalAddress::new("Marktplatz 2", "Beispielstadt", "54321", "DE")?,
    )
    .with_electronic_address(Identifier::eas("991-01234-56", "0204")?);

let invoice = Invoice::builder(
        "urn:cen.eu:en16931:2017",       // BT-24
        "R-2026-0001",                   // BT-1
        Date::parse("2026-07-31")?,      // BT-2
        "380",                           // BT-3
        "EUR",                           // BT-5
    )
    .seller(seller)
    .buyer(buyer)
    .payment(PaymentInstructions::sepa_credit_transfer("DE02120300000000202051", None))
    .due_in_days(14)                     // BT-9
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
    .build_reconciled()?;                // BG-22 and BG-23 computed from the lines

assert_eq!(invoice.vat_breakdown.len(), 2);
assert_eq!(invoice.totals.gross_total.to_string(), "3567.50");
assert!(validate(&invoice).is_valid());

// The typed proof. A rejection hands back the invoice and its report.
let proof: Validated<En16931> = Validated::new(invoice)?;
assert_eq!(proof.invoice().number.as_deref(), Some("R-2026-0001"));
# Ok(()) }
```

Or start from a valid invoice and change what differs; the report names
exactly what the change broke:

```rust
use en16931::{examples, profiles};

let mut invoice = examples::for_profile(&profiles::XRECHNUNG).expect("shipped");
invoice.buyer_reference = None;                     // drop the Leitweg-ID
let report = profiles::XRECHNUNG.validate(&invoice);
assert_eq!(report.fatal().map(|f| f.rule.as_str()).collect::<Vec<_>>(), ["BR-DE-15"]);
```

## What it covers

- **The model.** `Invoice` holds the business terms and groups of
  EN 16931-1, with a type per semantic data type: `InvoiceAmount` (exactly two
  decimals, checked arithmetic), `UnitPriceAmount`, `Quantity`, `Percentage`
  (per cent, not a fraction), `Date` (a calendar day), `Identifier`, `Code`.
  The terms the standard makes mandatory are still `Option` in the model, so an
  invalid document can be represented and reported on; the builder takes them as
  arguments.
- **Reconciliation.** `build_reconciled` and `Reconciler` derive the VAT
  breakdown (BG-23) and the document totals (BG-22) from the lines.
- **Validation.** `validate` runs the core rules; `Profile::validate` and
  `Check` run a profile, with suppression (`without`) and re-levelling
  (`relevel`) recorded on the report. Every finding carries the rule id, its
  severity, the business-term path and, where the crate knows a remedy, a hint.
- **Profiles.** Five ship, each with its checks and the authority releases
  they were verified against:

  | Profile | Checks |
  |---|---:|
  | EN 16931 core | 227 |
  | XRechnung 3.0 | 282 |
  | XRechnung 3.0 CVD | 290 |
  | XRechnung 3.0 Extension | 296 |
  | Peppol BIS Billing 3.0 | 290 |

  `profiles::classify` reads a document's BT-24, including the ZUGFeRD /
  Factur-X profiles a received document can declare.
- **The typed proof.** `Validated<P>` exists only for an invoice that passed
  profile `P` in full; `en16931_formats::write` takes one and stamps BT-24 from
  it.
- **Severity from the authority.** KoSIT's and Peppol's published severities,
  including KoSIT's re-levelling of CEN rules, per profile.
- **Explaining.** `validation::rules::explain` resolves any registered rule id;
  the registry is data — id, severity, provenance, business terms, wording.
- **Pre-flight.** `Party::missing_for` and `Profile::missing_terms` list what a
  profile will ask for; `VatCategory::can_share_document` answers whether VAT
  categories may share one invoice.
- **Reports.** `Report` is a versioned JSON shape (`en16931-report/4`); the
  `svrl` feature renders it as SVRL.
- **Code lists.** Generated from the pinned CEN artefacts; `codes::guard`
  checks a code at the point of mapping and names the fix (`9958` → `0204`).

## Conformance

334 rules registered: 53 retired by the type system (no state can make them
fire), 4 undecidable at the semantic layer, and every one of the other 277
exercised by its own failing fixture. Against the authorities' own suites, at
pinned releases: 1 025 CEN unit-test assertions run with no disagreement (11
declared divergences), 381 KoSIT mutations agreed, 59 published example invoices
valid. The [conformance page](https://hupe1980.github.io/en16931/docs/conformance/)
has the details.

The 1 339 syntax rules (`UBL-*`, `CII-*`) belong to `en16931-formats`.

## Features

| Feature | Adds |
|---|---|
| `serde` | `Serialize` / `Deserialize`; types with invariants re-check them |
| `svrl` | SVRL rendering of a report; no dependencies |
| `sepa` | IBAN checks against the ISO 13616 registry, and `EN-SEPA-01` on BT-90 |
| `chrono`, `time` | conversions to and from their date types |
| `billing` | `billing_adapter::from_billing`: a [`billing`](https://crates.io/crates/billing) 0.16 document and a header to an invoice |

## Documentation

- [Getting started](https://hupe1980.github.io/en16931/docs/getting-started/)
- [The semantic model](https://hupe1980.github.io/en16931/docs/model/)
- [Validation](https://hupe1980.github.io/en16931/docs/validation/)
- [Profiles](https://hupe1980.github.io/en16931/docs/profiles/)
- [Bridging a billing engine](https://hupe1980.github.io/en16931/docs/billing/)
- [Conformance](https://hupe1980.github.io/en16931/docs/conformance/)

Minimum supported Rust version: 1.88.

## Licence

MIT OR Apache-2.0, at your option.

### Attribution

> implementation of the EN 16931-1 semantic data model; © CEN, used under the 2018 CEN–EC licence agreement

This crate is an implementation of the semantic data model of EN 16931-1 and of
the two mandatory syntaxes listed in CEN/TS 16931-2. EN 16931-1 and
CEN/TS 16931-2 are made available free of charge by CEN and the European
Commission under their 2018 licence agreement, which permits derivative use on
condition that derivative applications carry a statement to this effect.
Copyright in the standard remains with CEN.

The notice above is `en16931::ATTRIBUTION`, and every `ValidationReport`
carries it verbatim.
