+++
title = "Profiles"
weight = 4
description = "XRechnung 3.0, Peppol BIS Billing 3.0 and the XRechnung Extension as EN 16931 profiles — CIUS versus Extension, restrictions as data, and the typed Validated<P> proof."
+++

Almost nobody sends a bare core EN 16931 invoice. What actually crosses a wire is
a **CIUS** — a Core Invoice Usage Specification — and which one applies is
carried in BT-24, the customization identifier, in the document itself.

Five profiles ship:

| Profile | Checks | Conformant CIUS? | BT-24 |
|---|---:|---|---|
| EN 16931 core | 227 | n/a | `urn:cen.eu:en16931:2017` |
| XRechnung 3.0 | 282 | no | `…#compliant#urn:xeinkauf.de:kosit:xrechnung_3.0` |
| XRechnung 3.0 CVD | 290 | no | `…#compliant#…:xrechnung:cvd_0.9` |
| XRechnung 3.0 Extension | 296 | no | `…#conformant#…:extension:xrechnung_3.0` |
| Peppol BIS Billing 3.0 | 273 | **yes** | `…#compliant#urn:fdc:peppol.eu:2017:poacc:billing:3.0` |

*Checks* counts what a profile declares. A report says how many actually ran for
the document in hand, which can be one lower — `EN-EXT-01` has nothing to say
about an invoice carrying no extension data.

## A CIUS is a set of restrictions

Every Schematron-based tool models a CIUS as "core rules plus extra rules",
because Schematron has no other vocabulary. **That is not how EN 16931 defines
one.**

§7.3.2 is a normative table of the thirteen kinds of change a CIUS may make
across six axes, and **only one of those axes is "add a rule."** The other five
are *restrictions on the model* — so here they are data, and the rules are
derived from them:

```rust
use en16931::profiles;
use en16931::validation::profile::Restriction;

// Eleven of XRechnung's BR-DE rules are pure `Mandatory` restrictions, and two
// are `CodeValues`. All thirteen are data, not code; the rest need code.
let ids: Vec<_> = profiles::XRECHNUNG.restrictions.iter().map(Restriction::id).collect();
assert!(ids.contains(&"BR-DE-3"));    // Seller city (BT-37) shall be present
assert!(ids.contains(&"BR-DE-17"));   // BT-3 restricted to eight codes
```

Each keeps its real published id, so a finding is lookup-able in KoSIT's index —
and the path still names the business term:

```text
[BR-DE-3] BG-4/BT-37 — Seller city (BT-37) shall be present
```

A restriction model that pretended to cover *everything* would be worse than one
honest about where it stops. The conditional payment-means rules
(`BR-DE-23/24/25`), the `BR-DE-16` identifier requirement and `BR-DE-26`
genuinely need code, and they are code.

### What this buys

**A CIUS can be checked for conformance.** §4.4.2 requires that *"the resulting
invoice document instance shall be fully compliant to the core invoice model"*.
Every restriction variant is by construction a narrowing, so a profile that tried
to *loosen* something cannot be expressed as one. Loosening is an **Extension**
(§4.3, CEN/TR 16931-5), a different mechanism entirely.

**Validation widens for free — from a *conformant* CIUS.** §4.4.4 says an
instance complying with one *"can still be received and processed by a party who
is not supporting the CIUS"*. So the proof converts, infallibly.

## The typed proof

```rust
use en16931::profiles::PeppolBis3;
use en16931::validation::profile::Validated;

let proof: Validated<PeppolBis3> = Validated::new(invoice)?;  // Err carries the report
serialise_peppol(&proof);            // signature demands the CIUS proof
accepts_core(proof.widen());         // §4.4.4 — free, no re-validation
```

`Validated<P>` cannot be constructed except by passing profile `P`. A serialiser
that takes one physically cannot be handed an unchecked invoice, or one checked
against a different profile — the mistake that produces a document which passed
your validator and fails the receiver's.

### The proof has to be earned

`widen()` exists only where §4.4.4 actually holds, and whether it holds is
computed rather than declared: `Profile::is_conformant_cius()` reads the
profile's severity overrides and answers `false` if any of them relaxes a core
rule.

It answers `false` for three shipped profiles, and every one of those `false`s
closed a real hole. A `Validated<XRechnungCvd> → Validated<En16931>` conversion
once compiled, producing a **proof of core validity for an invoice violating
`BR-CL-13`**. A serialiser trusting the core proof — the entire purpose of the
type — would have been handed a document no core-only receiver can process. The
same hole existed one layer up for plain XRechnung, because KoSIT relaxes
`BR-CL-23` for every scenario, so a unit code outside CEN's Rec 20 table leaves
an invoice valid as an XRechnung and invalid as a core invoice.

Both conversions are gone, and a property test asserts the surviving guarantee
over generated documents rather than one fixture: *if a conformant CIUS accepts
it, core accepts it.*

Re-validate instead. `Validated::<En16931>::new(invoice)` is one line, and it is
a line that can honestly fail.

## Profiles are siblings, not levels

It is tempting to model these as an ordered scale. There is no such order, and
tests pin it: **XRechnung permits `BT-3 = 389`** (self-billed) and Peppol does
not; **Peppol permits `386`** (prepayment invoice) and XRechnung does not.
Neither is "more restrictive".

The sharpest case is BT-119. CEN's `BR-48` exempts VAT category `O` from stating
a breakdown rate; XRechnung's `BR-DE-14` requires it **unconditionally**.
Suppressing BT-119 for `O` — on the strength of `BR-O-05`, which governs BT-152,
a *different* term — is the natural mistake, and it fails the KoSIT validator.

## XRechnung merges 31 of Peppol's rules — and rewrites two

The Schematron in KoSIT's repository contains only `BR-DE-*`, `BR-DEX-*` and
`BR-TMP-*`. **That file is an input, not the artefact.** The build runs
`peppol-into-xr.xsl` over it, splicing in every Peppol assert named in
`rule-list.xml`, and *that* is what ships.

31 of Peppol's 46. The fifteen left out are `CL001`…`CL008` (Peppol's own
narrower code lists) and `P0104`…`P0112` (the VATEX-to-category pinning, plus the
German-parties type-code rule that would be circular inside a German CIUS).

Two are **rewritten on the way in**, and both differences are observable:

| | Peppol | XRechnung |
|---|---|---|
| `R120` severity | `fatal` | **`warning`** |
| `R040` / `R120` slack | `0.02` always | **`0.5` for HUF**, 0.02 otherwise |

So the two profiles hold **separate instances** of those rules rather than
sharing one.

Reading KoSIT's validator configuration suggests the opposite — it lists CEN's
Schematron and KoSIT's own, and no Peppol. That is true, and it does not mean
what it looks like: the second one *already contains* Peppol's rules by the time
the validator loads it.

## An Extension adds — and KoSIT ships one of each

`profiles::XRECHNUNG_EXTENSION` is §4.3's second mechanism in the wild. Where the
CIUS narrows, it **widens**:

| | Core / CIUS | Extension |
|---|---|---|
| BT-125 mime code | six codes | + `application/xml` |
| scheme identifiers | ISO 6523 ICD / CEF EAS | + `XR01`–`XR03` (DiGA) |
| BT-115 | `BR-CO-16` | **`BR-DEX-09`** — third-party payments added back |

It also adds two groups the core model has no term for: `BG-DEX-01` sub-invoice
lines, for positions that decompose, and `BG-DEX-09` third-party payments, for
the German digital-health case where a statutory insurer settles part of an
invoice addressed to the insured. Both live in `en16931::extensions`, not on
`InvoiceLine` — a core line has no child, and putting one there would make every
consumer carry a field only one Extension populates.

Because it widens, §4.4.4's guarantee does not run: an Extension-valid invoice
need **not** be core-valid, and there is deliberately no widening from it.

**The CVD variant is the awkward case.** Its identifier says `#compliant#` —
§4.3's word for a *CIUS* — but `BR-TMP-CVD-01` checks BT-158's scheme against
UNTDID 7143 **plus `CVD`**, and `CVD` is not in UNTDID 7143. So a conforming CVD
invoice violates core `BR-CL-13`, which a CIUS may not cause. This crate follows
the behaviour rather than the label: reporting `BR-CL-13` as fatal on every CVD
invoice would be a false positive on a document KoSIT accepts.

## Payment means, and rules that enums retire

`BR-DE-23-b`, `-24-b` and `-25-b` each forbid the two payment groups BT-81 did
not name. Because `PaymentMeans` is an **enum** over BG-17 / BG-18 / BG-19, that
combination cannot be written down, so all three have nothing left to check:

```rust
pub enum PaymentMeans {
    CreditTransfer(Vec<CreditTransfer>),  // BG-17
    Card(PaymentCard),                    // BG-18
    DirectDebit(DirectDebit),             // BG-19
}
```

The `-a` halves stay real: they tie the *variant* to BT-81's **value**, which no
type can see.

What about *reading* a document that carries the forbidden combination? KoSIT
ships exactly such files as mutation instances. The readers keep the **first**
group, record the later one as unmapped rather than letting it silently win —
and the `-a` rule then fires on the mismatch, which is the verdict KoSIT's own
validator reaches on those files.

`BR-DE-19` and `-20` want a *correct IBAN*, and the crate implements ISO 7064
mod-97-10 — no registry, no network, so it still runs on `wasm32`. It cannot tell
you the account exists, only that the string is not a typo, which catches the
overwhelming majority of real errors. Both are **warnings**, matching KoSIT's
*soll*: a suspicion, not a rejection.

## Pre-flight: which fields will this profile ask me for?

`validate` answers *"is this document acceptable?"*. On a half-built invoice that
is a hundred findings, most of them about lines and totals that are not there
yet. Because restrictions are data, a profile can answer a different question
**before** the data is fetched:

```rust
use en16931::invoice::{Party, PartyRole};
use en16931::profiles::XRECHNUNG;

let gaps = Party::default().missing_for(&XRECHNUNG, PartyRole::Buyer);
let terms: Vec<_> = gaps.iter().map(|m| m.term.0).collect();
assert!(terms.contains(&52));   // BT-52 Buyer city      — BR-DE-8
assert!(terms.contains(&53));   // BT-53 Buyer post code — BR-DE-9
```

So a seller whose master data lives in a contract service fetches what XRechnung
needs in one round trip, instead of a build-validate-fetch loop.
`XRECHNUNG.missing_terms(&invoice)` does the same for a whole document.

Restrictions only, and deliberately: they are the §7.3.2 axis that is pure data,
so the answer is exact. The conditional rules cannot be answered before the
document exists — `BR-DE-23-a` asks for BT-84 only if BT-81 names a credit
transfer — and `validate` remains the complete check.

## What is next

- **[Syntaxes](@/docs/syntaxes.md)** — turning a proof into UBL or CII.
- **[Conformance](@/docs/conformance.md)** — how the profile claims are measured.
