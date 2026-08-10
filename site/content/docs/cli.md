+++
title = "The command line"
weight = 7
description = "en16931-cli: validate, convert, extract, inspect and explain European e-invoices from one static binary, with exit codes a CI job can branch on."
+++

**The EN 16931 validator as a command.** UBL, CII and ZUGFeRD / Factur-X in, a
verdict out — text for a person, JSON for a pipeline, SVRL for every other
Schematron tool in this field. And, because the model is the thing being
checked, two documents can be compared **as invoices** rather than as XML.

```sh
cargo install en16931-cli      # the binary is `en16931`
```

```console
$ en16931 validate rechnung.xml
rechnung.xml — UBL 2.1
XRechnung 3.0 validation (EN 16931-1:2017+A1:2019) — 282 rule(s) checked, 2 finding(s), INVALID
  [BR-DE-15] BT-10 — Buyer reference (BT-10) shall be present
  [BR-CL-23] BG-25[0]/BT-130 — Unit code MUST be coded according to the UN/ECE Recommendation 20 with Rec 21 extension. [hint: did you mean "KWH"? Code lists are case-sensitive]
$ echo $?
1
```

Findings point at a **business term**, never at an XPath. `BG-25[0]/BT-130` is a
field you can find; `/ubl:Invoice/cac:InvoiceLine[1]/cbc:InvoicedQuantity/@unitCode`
is a thing you have to decode first.

## What it is for

Every other validator in this space is a JVM and a Schematron engine: KoSIT's
validator, Mustangproject, phive. They are good, and they are 200 MB of runtime
in a container that exists to answer one yes/no question in CI. This is a single
static binary that answers the same question, from the same artefacts, and names
the business term rather than the XPath.

It is also the fastest way to find out what a file someone sent you actually is.

## Exit codes

| | |
|---|---|
| `0` | every document passed |
| `1` | a document was read and is **invalid** |
| `2` | a document could not be read at all, or the command was misused |

Telling `1` from `2` is the point. A CI job that treats *"this invoice is
invalid"* and *"that path does not exist"* the same way will eventually ship an
invoice because a volume was not mounted.

```sh
en16931 validate out/*.xml --quiet || exit 1
```

Hostile input lands on `2`, never on a crash. A document nested a few hundred
elements deep used to abort the process — the XML parser recurses per level, and
a stack overflow is not something Rust can catch — so `validate` exited `134`
with no report. It is refused before parsing now. Entity expansion and XXE need a
DTD, and the parser rejects any document carrying one.


## `validate`

```sh
en16931 validate INVOICE...
    [--profile auto|<name>|<BT-24>]   # default: whatever the document declares
    [--format text|json|svrl]
    [--strict]                        # warnings and information count as failures
    [--without RULE]                  # skip a rule, loudly. Repeatable.
    [--quiet]
```

`--profile auto` is the default and it reads **BT-24**. §7.6 puts the
specification identifier in the document precisely so a receiver can apply the
rules the sender generated under; validating an XRechnung against the bare core
model is the most common way to ship a document a counterparty then rejects.

Name a profile to ask a different question — *"would this pass in Germany?"* —
about a document that does not claim to.

`--strict` is **off** by default, and that is deliberate. KoSIT reports
`BR-CL-23` at warning on purpose, because CEN's unit-code table lags UN/ECE's; a
build that fails on it fails on invoices Germany accepts. The same document under
two rule sets is the clearest way to see it:

```console
$ en16931 validate rechnung.xml                        # BT-24 says XRechnung
XRechnung 3.0 validation (…) — 282 rule(s) checked, 1 finding(s), valid
  [BR-CL-23] BG-25[0]/BT-130 — Unit code MUST be coded according to …
$ echo $?
0

$ en16931 validate rechnung.xml --profile "EN 16931"   # the core model
EN 16931 validation (…) — 227 rule(s) checked, 1 finding(s), INVALID
  [BR-CL-23] BG-25[0]/BT-130 — Unit code MUST be coded according to …
$ echo $?
1
```

That is not a bug in either answer. It is what the two authorities publish, and
it is why XRechnung is not a *conformant CIUS* under §4.4.2 — see
[severity](@/docs/validation.md#severity-is-the-authority-s-not-ours).

`--without` is recorded on the report and printed in every output format. A
deviation you cannot see in the artefact is worse than one you argued for.

## `convert`

```sh
en16931 convert INVOICE --to ubl|cii [--profile <name>] [-o OUT]
```

The conversion goes **through the semantic model**, so what comes out is what
EN 16931 says the document means, not a transliteration of its elements. That is
why a UBL invoice whose BT-21 is embedded in the note text as `#AAI#…` comes out
of the CII side with its own element.

With `--profile`, nothing is written unless the model passes, and BT-24 is
stamped from the profile that was actually run. Anything the target syntax cannot
carry — BT-11 on a credit note is the known case — goes to **stderr**, so a
redirected stdout is the document and nothing else, and the loss is still
visible.

## `diff`

```sh
en16931 diff LEFT RIGHT [--format text|json]
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

`0` identical, `1` they differ, `2` one could not be read — the same split
`validate` makes.

Two details that are not obvious. The path is a path through the **model**
(`lines[1].vat.rate`), not a business-term path: a hand-written walk over 164
terms would spell it `BG-25[2]/BT-152` and would be a second model to keep in
step with the first, silently missing whichever field was added without a
matching arm. And a **scale difference is not a difference** — UBL writes BT-152
as `25.0` where CII writes `25`, and the model holds those to be one value, so
the diff does too.

## `extract`

```sh
en16931 extract invoice.pdf [-o payload.xml]
```

The embedded payload of a ZUGFeRD / Factur-X PDF, **verbatim**. Whoever diagnoses
a rejected invoice needs the bytes the counterparty sent, not a reconstruction of
them. Disagreements between the PDF's XMP metadata and the payload are reported
on stderr.

## `inspect`

```sh
en16931 inspect INVOICE... [--format text|json]
```

What the file is, without a verdict on it: syntax, declared BT-24, which rule set
that resolves to *here*, the parties, the totals, and anything the reader could
not map. The first command to run on a document you were sent.

```console
$ en16931 inspect invoice.pdf
invoice.pdf
  syntax        ZUGFeRD / Factur-X (PDF/A-3 + CII)
  ZUGFeRD       Minimum  (No("MINIMUM carries no invoice lines"))
  kind          Invoice
  BT-24 profile urn:factur-x.eu:1p0:minimum
  rule set      EN 16931 (BT-24 unknown here)
  …
```

## `explain`, `rules` and `profiles`

```sh
en16931 explain BR-CO-14        # or br-co-14, or BR-CO-3, or BR-IG-1
en16931 profiles
en16931 rules [--profile <name>] [--term BT-117] [--format text|json]
```

`explain` resolves rule ids in every spelling the standard and the artefacts use
— zero-padding, case, and the `BR-IG-*` / `BR-IP-*` families the artefacts call
`BR-AF-*` / `BR-AG-*`. It answers for **profile restrictions** too, which are data
rather than predicates and still appear in reports under their own ids. It also
says which profiles run the rule and at what severity, which is the answer to
*"why did my validator not object to this?"*

`profiles` also prints the **authority releases** this build was verified
against — CEN's, KoSIT's Schematron, KoSIT's validator configuration and
OpenPeppol's, each at a release tag. The same list travels in every report.

`rules` prints the whole catalogue, derived from the registry — so it cannot
drift from what the validator actually runs, the way a hand-maintained table of
317 rules would on the first release nobody remembered to update. It is the thing
to diff across versions:

```sh
en16931 rules --format json > new.json && diff old.json new.json
```

`rules --profile <name>` lists **every check that profile declares**, rules and
§7.3.2 restrictions alike, and its total is the same number `profiles` prints in
its CHECKS column and a report prints as `rule(s) checked`. It used to list the
rules only, so XRechnung showed 270 of its 282 checks and the twelve missing were
the `BR-DE-*` narrowings every German counterparty quotes. A restriction is
marked `profile` in the SOURCE column, and carries `"restriction": "mandatory" |
"not-used" | "code-values"` in the JSON — the wording is this crate's rendering
of a narrowing, not an authority's sentence, and the catalogue does not pretend
otherwise.


…and the shortest way to see the severity question:

```console
$ en16931 rules --profile "EN 16931"      | grep BR-CL-23
BR-CL-23    fatal      artefact    Unit code MUST be coded according to …
$ en16931 rules --profile "XRechnung 3.0" | grep BR-CL-23
BR-CL-23    warning    artefact    Unit code MUST be coded according to …
```

## `generate` — completions and a man page

```sh
en16931 generate bash > /usr/share/bash-completion/completions/en16931
en16931 generate zsh  > ~/.zfunc/_en16931
en16931 generate man  > /usr/share/man/man1/en16931.1
```

`bash`, `zsh`, `fish`, `powershell`, `elvish` and `man`, all generated from the
same argument definitions the binary parses — so a flag cannot exist without its
completion.

## Nothing is read silently

Both readers report what they could not map and what they could not represent,
and those lists are printed under every document. The difference between *"this
document validated"* and *"the parts of it I understood validated"* is the whole
reason they exist.

## What it does not do

No network. No Peppol access point, no VIES lookup, no SMP resolution — the
libraries have no I/O and neither does this. No PDF *writing*; see
[ZUGFeRD](@/docs/zugferd.md#writing-pdfs-not-implemented-and-the-reason-is-not-effort)
for where that stops and why.
