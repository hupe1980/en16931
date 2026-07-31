#!/usr/bin/env bash
#
# Fetch the reference specifications and validation artefacts into ./spec/.
#
# ./spec/ is gitignored on purpose. The CEN validation artefacts are EUPL-1.2
# (a reciprocal licence) and the vendor specifications carry their own terms;
# keeping them out of the repository keeps this crate's MIT OR Apache-2.0
# licensing clean. See CONCEPT.md §13.2.
#
# Everything fetched here is publicly downloadable, including the normative text
# of EN 16931-1 itself — see the STANDARD section below and spec/README.md.

set -euo pipefail

cd "$(dirname "$0")"
SPEC="$PWD/spec"
mkdir -p "$SPEC"

log() { printf '\n\033[1;34m==>\033[0m %s\n' "$*"; }

# Pinned so a regeneration is a reviewable diff rather than a moving target.
# Bump deliberately, and record the new value in en16931::ARTEFACT_VERSION.
CEN_REF="validation-1.3.16"

fetch_repo() {   # name url ref
  local name=$1 url=$2 ref=$3
  log "$name @ $ref"
  if [ -d "$SPEC/$name/.git" ]; then
    git -C "$SPEC/$name" fetch --depth 1 origin "$ref" --tags --quiet
    git -C "$SPEC/$name" checkout --quiet FETCH_HEAD
  else
    rm -rf "${SPEC:?}/$name"
    git clone --depth 1 --branch "$ref" --quiet "$url" "$SPEC/$name" 2>/dev/null \
      || git clone --depth 1 --quiet "$url" "$SPEC/$name"
  fi
  git -C "$SPEC/$name" --no-pager log -1 --format='    %h %ad %s' --date=short
}

fetch_file() {   # url dest
  local url=$1 dest=$2
  log "$(basename "$dest")"
  curl -fsSL --retry 3 -o "$SPEC/$dest" "$url" && printf '    %s\n' "$(du -h "$SPEC/$dest" | cut -f1)"
}

# Sparse variant, for repositories that carry every historical artefact and are
# gigabytes in full. Blobless + sparse keeps it to the directories we name.
fetch_sparse() { # name url ref dir...
  local name=$1 url=$2 ref=$3; shift 3
  log "$name @ $ref (sparse: $*)"
  if [ ! -d "$SPEC/$name/.git" ]; then
    rm -rf "${SPEC:?}/$name"
    git clone --filter=blob:none --sparse --depth 1 --branch "$ref" --quiet "$url" "$SPEC/$name"
  fi
  git -C "$SPEC/$name" sparse-checkout set --no-cone "$@"
  git -C "$SPEC/$name" --no-pager log -1 --format='    %h %ad %s' --date=short
  printf '    %s\n' "$(du -sh "$SPEC/$name" | cut -f1)"
}

# ── CEN/TC 434 — the abstract model, the code lists, the per-rule test corpus ──
# EUPL-1.2. This is the primary source for rule ids, severities, contexts and
# the ~4 400 code-list values.
fetch_repo eInvoicing-EN16931 https://github.com/ConnectingEurope/eInvoicing-EN16931.git "$CEN_REF"

# ── Peppol BIS Billing 3.0 — PEPPOL-EN16931-* and the national rule sets ──────
fetch_repo peppol-bis-invoice-3 https://github.com/OpenPEPPOL/peppol-bis-invoice-3.git master

# ── phive-rules — the widest available collection of preconfigured rule sets ──
# Carries PINT (incl. EU PINT Billing) and the national CIUSes. OpenPEPPOL does
# not publish the PINT artefacts in a public repository of its own, so this is
# the practical source — and it doubles as the reference implementation to diff
# our rule coverage against (CONCEPT.md §18).
#
# Sparse: the full repository is ~3.6 GB because it retains every historical
# rule set, plus a PDF of every published spec version. We want the Schematron
# resources of three modules, and nothing else.
fetch_sparse phive-rules https://github.com/phax/phive-rules.git master \
  'phive-rules-peppol-pint/src/main/resources/*' \
  'phive-rules-en16931/src/main/resources/*' \
  'phive-rules-xrechnung/src/main/resources/*' \
  'README.md' 'LICENSE'

# ── KoSIT — XRechnung Schematron (BR-DE-*) and the validator configuration ────
fetch_repo xrechnung-schematron https://github.com/itplr-kosit/xrechnung-schematron.git master
fetch_repo validator-configuration-xrechnung \
  https://github.com/itplr-kosit/validator-configuration-xrechnung.git master

# ── XRechnung specification (CIUS + Extension), 3.0.2 ────────────────────────
fetch_file https://xeinkauf.de/app/uploads/2024/07/302-XRechnung-2024-06-20.pdf \
           XRechnung-3.0.2-spec.pdf
fetch_file https://xeinkauf.de/app/uploads/2024/10/XRechnung-EnglishSummary-v302.pdf \
           XRechnung-3.0.2-english-summary.pdf

# ── The standard itself ───────────────────────────────────────────────────────
# EN 16931-1 and CEN/TS 16931-2 are free of charge under the 2018 CEN–European
# Commission agreement, which also permits derivative use. They are distributed
# through National Standardisation Bodies, and several serve them as plain PDFs.
#
# ÚNMS SR (Slovakia) publishes STN EN 16931-1+A1, whose body is the **English
# version of the European Standard verbatim** (158 pp), as an open download.
# That is the normative wording this crate's rule texts are sourced from.
#
# See spec/README.md for the other routes, including DIN Media's free German PDF.
mkdir -p "$SPEC/manual"
if [ ! -f "$SPEC/manual/STN-EN-16931-1-A1.pdf" ]; then
  fetch_file https://www.normoff.gov.sk/files/docs/e-fakturacia-stn-en-16931-1-a1-614d692fbcaa2.pdf \
             manual/STN-EN-16931-1-A1.pdf
fi
# Plain-text extraction, so the normative text is greppable next to the artefacts.
if command -v pdftotext >/dev/null 2>&1 && [ ! -f "$SPEC/manual/EN16931-1.txt" ]; then
  pdftotext -layout "$SPEC/manual/STN-EN-16931-1-A1.pdf" "$SPEC/manual/EN16931-1.txt"
  log "EN16931-1.txt ($(wc -l < "$SPEC/manual/EN16931-1.txt" | tr -d ' ') lines)"
fi

# ── Orientation file ──────────────────────────────────────────────────────────
cat > "$SPEC/README.md" <<EOF
# spec/ — reference material (gitignored, regenerate with \`./fetch-spec.sh\`)

Fetched $(date -u +%Y-%m-%d). **Nothing here is committed** — see CONCEPT.md §13.2
for why (the CEN artefacts are EUPL-1.2; this crate is MIT OR Apache-2.0).

| Path | What | Licence |
|---|---|---|
| \`eInvoicing-EN16931/\` | CEN/TC 434 validation artefacts, pinned at \`$CEN_REF\` — abstract model, code lists, per-rule test corpus | EUPL-1.2 |
| \`peppol-bis-invoice-3/\` | Peppol BIS Billing 3.0 — \`PEPPOL-EN16931-*\` + national rule sets | see repo |
| \`xrechnung-schematron/\` | KoSIT XRechnung Schematron — \`BR-DE-*\` | see repo |
| \`validator-configuration-xrechnung/\` | KoSIT validator scenarios | see repo |
| \`phive-rules/\` | PINT (incl. \`pint-eu\`), EN 16931 and XRechnung rule sets, sparse checkout | Apache-2.0 |
| \`XRechnung-3.0.2-*.pdf\` | XRechnung 3.0.2 CIUS + Extension specification | © KoSIT |
| \`manual/STN-EN-16931-1-A1.pdf\` | **EN 16931-1:2017+A1:2019, English, full normative text** | © CEN, free access + derivative use |
| \`manual/EN16931-1.txt\` | the same, extracted for grepping | — |

## Where things are

    eInvoicing-EN16931/ubl/schematron/abstract/EN16931-model.sch     201 assertions, 66 contexts
    eInvoicing-EN16931/ubl/schematron/codelist/EN16931-UBL-codes.sch 22 BR-CL-* rules, ~4400 code values
    eInvoicing-EN16931/ubl/schematron/abstract/EN16931-syntax.sch    UBL-CR-* / UBL-SR-*  (not our layer)
    eInvoicing-EN16931/cii/schematron/abstract/EN16931-CII-syntax.sch CII-SR-* / CII-DT-* (not our layer)
    eInvoicing-EN16931/test/Invoice-unit-UBL/                        207 per-rule test files
    peppol-bis-invoice-3/rules/sch/PEPPOL-EN16931-UBL.sch            Peppol + country rules
    xrechnung-schematron/src/validation/schematron/                  BR-DE-*
    phive-rules/phive-rules-peppol-pint/.../schematron/pint-eu/      EU PINT Billing

## The standard itself — \`manual/\`

Parts 1 and 2 are **free of charge with derivative use permitted** under the
2018 CEN–European Commission agreement. Routes, best first:

| Route | What you get | Language | Notes |
|---|---|---|---|
| **ÚNMS SR (Slovakia)** — [direct PDF](https://www.normoff.gov.sk/files/docs/e-fakturacia-stn-en-16931-1-a1-614d692fbcaa2.pdf) | STN EN 16931-1+A1, 158 pp | **English** | Body is the English version of the EN verbatim. No registration. **Fetched automatically.** |
| **DIN Media (Germany)** — [DIN EN 16931-1:2020-12](https://www.dinmedia.de/de/norm/din-en-16931-1/327729047) | 162 pp | German | PDF download **0,00 EUR**. Free account needed. English translation costs €386. |
| **DIN Media** — [DIN CEN/TS 16931-2:2017-11](https://www.dinmedia.de/de/vornorm/din-cen-ts-16931-2/274991011) | 14 pp | German | PDF download **0,00 EUR** — this is Part 2. |
| **Any other NSB** | varies | varies | Find yours: <https://standards.cencenelec.eu/dyn/www/f?p=CEN:5> |
| **CEN project pages** | metadata only, no download | — | [EN 16931-1](https://standards.cencenelec.eu/dyn/www/f?p=CEN:110::::::FSP_PROJECT:71870), [CEN/TS 16931-2](https://standards.cencenelec.eu/dyn/www/f?p=CEN:110::::::FSP_PROJECT:60603) |

Note the CEN portal lists EN 16931-1:2017+A1:2019/AC:2020 as **Withdrawn**,
superseded by EN 16931-1:2026. The withdrawn edition is still what every
deployed CIUS validates against (CONCEPT.md §3), so it is the right target for
v1 — but obtain :2026 as well before starting \`Edition::En2026\`.

Parts 3-x (syntax bindings), 4, 5 and 6 must be purchased. They are the format
crates' concern, not this one's.

Anything you obtain manually goes in \`manual/\` — the script only ever adds to it.
EOF

log "done — $(du -sh "$SPEC" | cut -f1) in $SPEC"
printf '    see spec/README.md for what is where\n'
