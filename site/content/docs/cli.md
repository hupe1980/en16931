+++
title = "The command line"
weight = 7
description = "en16931-cli: validate, receive, convert, extract, embed, inspect and explain European e-invoices from one static binary, with exit codes a CI job can branch on."
+++

**The EN 16931 validator as a command.** UBL, CII and ZUGFeRD / Factur-X in,
a decision out — *accept*, *reject* or *undefined*, the three answers KoSIT's
validator gives — as text for a person, JSON for a pipeline, SVRL for every
other Schematron tool in this field. And, because the model is the thing being
checked, two documents can be compared **as invoices** rather than as XML.

```console
$ en16931 validate rechnung.xml
rechnung.xml
recommendation: REJECT
  because under the declared XRechnung 3.0: 1 fatal finding(s): BR-DE-15
declared: XRechnung 3.0
document: Lieferant GmbH — invoice 1234567 of 2018-04-13
rule set: XRechnung 3.0 (CEN validation-1.3.17, KoSIT v2.6.0, KoSIT v2026-08-31, OpenPeppol 5df0eff…)
most severe: fatal
container  valid
syntax     valid
schema     valid
semantic   INVALID
  XRechnung 3.0 validation (EN 16931-1:2017+A1:2019) — 282 rule(s) checked, 1 finding(s), INVALID
    fatal        [BR-DE-15] BT-10 — Buyer reference (BT-10) shall be present
    …
$ echo $?
1
```

Findings point at a **business term**, never at an XPath, because the rules run
against the semantic model rather than the serialised document. `BT-10` is a
field you can find; `/ubl:Invoice/cbc:BuyerReference` is a thing you have to
decode first.

```sh
cargo install en16931-cli      # the binary is `en16931`
```

---

## Exit codes, because this belongs in a pipeline

`validate` answers the receiver's question — *may this document be processed?*

| | |
|---|---|
| `0` | every document is **accepted** |
| `1` | a document is **rejected** — invalid, not an invoice at all, or declaring a BT-24 no profile matches — or **undefined**: ZUGFeRD MINIMUM / BASIC WL, lawful and not an EN 16931 invoice |
| `2` | a file could not be opened, or the command was misused |

Bytes that are not an invoice are a *rejection*, as KoSIT's validator answers;
a path that does not exist is `2`, so a pipeline can tell the two apart.

```sh
en16931 validate out/*.xml --quiet || exit 1
```

Hostile input is rejected, never a crash. A document nested more than 64 elements
deep (`en16931_formats::MAX_DEPTH`) is refused **before** parsing, since the XML
parser recurses per level. Entity expansion and XXE need a DTD,
and the parser rejects any document carrying one. A PDF is extracted under limits —
see `extract` below.

## The commands

### `validate`

```sh
en16931 validate INVOICE...
    [-r|--require <profile>]          # a second check, under the profile you require
    [--relevel RULE=LEVEL]            # report a rule at another severity. Repeatable.
    [--without RULE]                  # skip a rule, loudly. Repeatable.
    [--format text|json|svrl]
    [--strict]                        # an accepted document reporting anything fails
    [--max-stream-bytes N] [--max-total-bytes N] [--max-objects N]   # PDF limits
    [--quiet]
```

Every document is checked **as it declares itself**: BT-24 names the profile.
§7.6 puts the specification identifier in the document precisely so a receiver
can apply the rules the sender generated under. A BT-24 that names no profile
this build ships — a foreign CIUS, an obsolete XRechnung identifier, a typo — is
rejected, as KoSIT rejects a document no scenario matches; the core rules run
for information. ZUGFeRD / Factur-X documents are recognised by their own
identifiers: BASIC and EXTENDED are EN 16931 invoices and run every core rule,
MINIMUM and BASIC WL are not and run only the totals rules their terms carry
(*undefined*).

`--require <profile>` asks a second question — *"does this also pass the profile
I require?"* — whatever the document declares. Both reports are printed, and the
decision is **reject** if either holds a fatal finding:

```console
$ en16931 validate rechnung.xml --require peppol
rechnung.xml
recommendation: REJECT
  because under the required Peppol BIS Billing 3.0: 5 fatal finding(s): PEPPOL-EN16931-R004, …
declared: XRechnung 3.0
…
as required (Peppol BIS Billing 3.0):
…
```

A profile answers to three spellings, and `en16931 profiles` prints all of them:

| | |
|---|---|
| `xrechnung` | the **slug** — lower case, no spaces, what you type (`-r xrechnung`) |
| `"XRechnung 3.0"` | the display name, as a report prints it |
| `core`, `bis3`, `peppol-bis-3` | an alias — EN 16931 and Peppol BIS Billing 3.0 by the names people use |
| `urn:cen.eu:en16931:2017#compliant#…` | BT-24 verbatim, which a script has and a person does not |

An unknown name is refused with the list of slugs — the library's message
(`en16931::profiles::UnknownProfile`), so a binding or a config loader says the
same thing. `--relevel RULE=LEVEL` takes `fatal` (or `error`), `warning` or
`information` (or `info`), parsed the same way.

```console
$ en16931 profiles
PROFILE              NAME                     CHECKS  CIUS?  BT-24
en16931              EN 16931                    227  n/a    urn:cen.eu:en16931:2017
xrechnung            XRechnung 3.0               282  no     urn:cen.eu:en16931:2017#compliant#…
xrechnung-cvd        XRechnung 3.0 CVD           290  no     …#compliant#…:xrechnung:cvd_0.9
xrechnung-extension  XRechnung 3.0 Extension     296  no     …#conformant#…:extension:xrechnung_3.0
peppol               Peppol BIS Billing 3.0      290  yes    urn:cen.eu:en16931:2017#compliant#urn:fdc:peppol.eu:…
```

Below the table it lists the ZUGFeRD / Factur-X profiles — `zugferd-minimum`,
`zugferd-basic-wl`, `zugferd-basic`, `zugferd-extended` — which `--require`
also takes; nothing is written for them.

**The report has four layers**, each `valid`, `INVALID` or `skipped`:
*container* (a hybrid PDF's disagreements with its payload), *syntax* (elements
the reader does not map, values it cannot hold), *schema* (a second occurrence
of an element the schema allows once — the first is read, the second reported —
and spellings the schema does not admit) and *semantic* (the profile's
findings), under a header naming the document, the rule set and the most severe
level. Nothing the reader set aside is silent: the difference between *"this
document validated"* and *"the parts of it I understood validated"* is the whole
reason the layers exist.

`--relevel` and `--without` are a receiver's deviations — KoSIT's
`customLevel`, and a rule not run at all. Both are recorded on the report and
printed in every output format, and neither can yield a proof: a deviation you
cannot see in the artefact is worse than one you argued for.

`--strict` is **off** by default, and that is deliberate. KoSIT reports
`BR-CL-23` at warning on purpose, because CEN's unit-code table lags UN/ECE's;
a build that fails on it fails on invoices Germany accepts. That is not a bug in
either answer — it is what the two authorities publish, and it is why
XRechnung is not a *conformant CIUS* under §4.4.2 — see
[severity](@/docs/validation.md#severity-is-the-authority-s-not-ours) for the
evidence.

**JSON** is the interchange shape `en16931-report/4` — an array with one report
per document, carrying the layers and their entries, the identity, the
declaration, the recommendation and its reasons, the findings, and the report
as required nested in `required`. A consumer that knows the schema version
reads it with `en16931::report::Report`. **SVRL** is one
`svrl:schematron-output` per report: a fatal or warning layer entry is an
`svrl:failed-assert` with an id in this crate's namespace
(`en16931:schema:duplicate`), an unmapped element an `svrl:successful-report`,
and the findings follow unchanged.

### `convert`

```sh
en16931 convert INVOICE --to ubl|cii [--allow-loss] [-r <profile>] [-o OUT]
```

The conversion goes **through the semantic model**, so what comes out is what
EN 16931 says the document means — not a transliteration of its elements. That
is why a UBL invoice whose BT-21 is embedded in the note text as `#AAI#…` comes
out of the CII side with its own element.

Anything that would not reach the target — an element the reader did not map, a
value it could not hold, a duplicate it did not read, a term the target syntax
cannot carry — **refuses the conversion** and names each item; `--allow-loss`
converts and lists them. Everything the reader and the writer had to say goes
to **stderr**, one entry per line by its kind, so a redirected stdout is the
document and nothing else:

```console
$ en16931 convert rechnung.cii.xml --to ubl > rechnung.xml
en16931: derived      cac:TaxTotal/cbc:TaxAmount (BT-110) = "19.00" — absent beside BG-23, and UBL 2.1 requires cbc:TaxAmount in cac:TaxTotal: written as the sum of BT-117, as BR-CO-14 defines it
```

A *derived* entry is a value the writer computed because the syntax requires
one and the source did not state it; *dropped* is a term the target has no
place for. `--require <profile>` converts only a document `validate --require`
accepts. Validity is otherwise not a refusal: converting an invalid invoice
says so on stderr.

### `diff`

```sh
en16931 diff LEFT RIGHT [--format text|json|svrl]
```

Compare two documents **as invoices, not as XML**. Both sides are read into the
semantic model first, so a UBL invoice and its CII translation come out
identical where a textual diff shares almost nothing — different root element,
different namespaces, a different name for every field.

```console
$ en16931 convert rechnung.xml --to cii -o rechnung.cii.xml
$ en16931 diff rechnung.xml rechnung.cii.xml
--- rechnung.xml — UBL 2.1
+++ rechnung.cii.xml — UN/CEFACT CII D16B

identical as invoices
$ echo $?
0
```

That is the question worth asking of a conversion, a migration, or a
counterparty who says they received something else. And when they did:

```console
$ en16931 diff ours.xml theirs.xml
--- ours.xml — UBL 2.1
+++ theirs.xml — UBL 2.1

2 difference(s)
  ~ seller.address.city    "Musterstadt" → "Nirgendwo"
  - lines[2].note          "Rabatt" → null
$ echo $?
1
```

`0` identical, `1` they differ, `2` one could not be read.

Paths run through the **model** (`lines[1].vat.rate`), and a **scale
difference is not a difference**: UBL's `25.0` and CII's `25` for BT-152 are one
value.

### `extract`

```sh
en16931 extract invoice.pdf [-o payload.xml]
    [--max-stream-bytes N] [--max-total-bytes N] [--max-objects N]
```

The embedded payload of a ZUGFeRD / Factur-X PDF, **verbatim**. Whoever
diagnoses a rejected invoice needs the bytes the counterparty sent, not a
reconstruction of them. Disagreements between the PDF's XMP metadata and the
payload are reported on stderr.

The PDF is walked and inflated under limits — 64 MiB per stream, 128 MiB in
all, 10 000 objects by default, the library's defaults — and a PDF past one is
refused naming the limit, not hung on or inflated without end. A service that
receives from strangers lowers them; `validate` takes the same three flags.

### `embed`

```sh
en16931 embed rendered.pdf invoice.xml -o invoice.pdf
    [--profile en16931|xrechnung] [--relationship alternative|source]
```

A ZUGFeRD / Factur-X hybrid from a PDF/A-3 rendering and an invoice — UBL, CII
or the JSON envelope. The invoice is validated under the profile its BT-24
declares (or `--profile`), which must be EN 16931 or XRechnung, and embedded as
CII under `factur-x.xml` or `xrechnung.xml` with the Factur-X XMP, `/AF`,
`/AFRelationship` and a title, subject and author taken from the invoice. The
PDF must already be PDF/A-3: nothing is converted, and anything else is refused.
The output is the PDF followed by one incremental update, read back before it
is written.

Exit `0` written; `1` the invoice is not valid under the profile (the report is
printed), the profile has no ZUGFeRD carrier, or the PDF was refused, naming why;
`2` a file could not be read or written.

### `inspect`

```sh
en16931 inspect INVOICE... [--format text|json|svrl]
```

What the file is, without a verdict on it: syntax, declared BT-24 and what it
classifies as *here*, the parties, the totals, and everything the reader had to
say. The first command to run on a document you were sent. Its JSON is the
report shape with no rules run.

```console
$ en16931 inspect minimum.pdf
minimum.pdf
  syntax        ZUGFeRD / Factur-X (PDF/A-3 + UN/CEFACT CII D16B)
  ZUGFeRD       Minimum
  kind          Invoice
  BT-24 profile urn:zugferd.de:2p0:minimum
  declares      ZUGFeRD MINIMUM (totals only)
  …
```

### `categories`

**The one command that takes no document.** Two of the ten VAT categories are
exclusive, and they are the only rules in EN 16931 that a set of category codes
decides on its own — so the question is answerable while a biller is still
deciding what to put on the invoice, rather than after assembling one they then
have to throw away.

```console
$ en16931 categories S Z
  S   standard rate
  Z   zero rated goods

these categories may share one invoice
$ echo $?
0

$ en16931 categories S O
  S   standard rate
  O   services outside the scope of VAT

REFUSED: VAT categories O and S cannot share one document (BR-O-11, BR-O-12,
BR-O-13, BR-O-14) — "Not subject to VAT" is exclusive: an invoice carrying it
may carry nothing else. Bill the out-of-scope items as their own document
$ echo $?
1
```

Same three exit codes as `validate`: `0` they may share a document, `1` they may
not, `2` that is not a category code. A caller that cannot tell "these cannot be
billed together" from "you typed a code that does not exist" will eventually
treat a typo as a business rule.

The categories are printed with their **names**, since `O` is not "other".
Codes are case-sensitive; `BR-CL-17` compares them literally.

### `explain`, `rules` and `profiles`

```sh
en16931 explain BR-CO-14        # or br-co-14, or BR-CO-3
en16931 profiles
en16931 rules [-p <profile>] [--term BT-117] [--format text|json]
```

`explain` resolves rule ids up to zero-padding and case. It does not map one
rule family onto another: the IGIC and IPSI rules are `BR-IG-*` and `BR-IP-*`,
CEN's ids since 1.3.17 (`BR-AF-*` / `BR-AG-*` before). It answers for **profile restrictions** too, which are
data rather than predicates and still appear in reports under their own ids.

It also says which profiles run the check and at what severity, which is the
answer to *"why did my validator not object to this?"* — and where an id names
two rules, it says so rather than picking one:

```console
$ en16931 explain PEPPOL-EN16931-R120
PEPPOL-EN16931-R120  [varies by profile]  the artefacts only — an authority's addition
  …
  run by: XRechnung 3.0 (warning), … , Peppol BIS Billing 3.0 (fatal)
```

Same id, same text, different consequence: KoSIT's build rewrites the flag when
it merges Peppol's rule into XRechnung's Schematron.

`profiles` names each profile twice — the slug `--require` takes and the
display name a report prints — because the two differ and only one of them is
typeable. It lists the ZUGFeRD / Factur-X profiles a received document's BT-24
is recognised as, and the **authority releases** this build was verified
against — CEN's, KoSIT's Schematron, KoSIT's validator configuration and
OpenPeppol's. The same list travels in every report, per profile, because
`BR-DE-15` is KoSIT's rule on KoSIT's cadence and stamping `validation-1.3.17`
beside it would name the wrong authority.

`rules` prints the whole catalogue, derived from the registry the validator
runs. It is the thing to diff across versions:

```sh
en16931 rules --format json > new.json && diff old.json new.json
```

`rules --profile <name>` lists **every check that profile declares**, rules and
§7.3.2 restrictions alike, and its total is the same number `profiles` prints in
its CHECKS column and a report prints as `rule(s) checked` — restrictions
included, because the twelve `BR-DE-*` narrowings are the ones every German
counterparty quotes. A restriction is marked `profile` in the SOURCE column and
carries `"restriction": "mandatory" | "not-used" | "code-values"` in the JSON;
its wording is this crate's rendering of a narrowing, not an authority's
sentence, and the catalogue does not pretend otherwise.

…and the shortest way to see the severity question the libraries spend a page
on:

```console
$ en16931 rules --profile en16931    | grep BR-CL-23
BR-CL-23    fatal      artefact    Unit code MUST be coded according to …
$ en16931 rules --profile xrechnung  | grep BR-CL-23
BR-CL-23    warning    artefact    Unit code MUST be coded according to …
```

### `generate` — completions and a man page

```sh
en16931 generate bash > /usr/share/bash-completion/completions/en16931
en16931 generate zsh  > ~/.zfunc/_en16931
en16931 generate man  > /usr/share/man/man1/en16931.1
```

`bash`, `zsh`, `fish`, `powershell`, `elvish` and `man`, all generated from the
same argument definitions the binary parses — so a flag cannot exist without its
completion.

## One rendering, not eight

Its text is the libraries' `Display` (`en16931_formats::Received`,
`report::Report`, `report::Entry`, `en16931::ValidationReport`), its JSON the
interchange shape and its SVRL what `en16931::svrl` renders from that shape.

## What it does not do

No network. No Peppol access point, no VIES lookup, no SMP resolution — the
libraries have no I/O and neither does this. No PDF *rendering* and no
conversion to PDF/A: `embed` puts an invoice into a PDF/A-3 file you rendered,
and refuses one that is not PDF/A-3 — [ZUGFeRD](@/docs/zugferd.md) says why.

## Licence

`MIT OR Apache-2.0`. The rules and code lists are derived from CEN's validation
artefacts; see the attribution notice the command prints under every report.
