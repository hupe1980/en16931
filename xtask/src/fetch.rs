//! Fetching the reference specifications and validation artefacts into `spec/`.
//!
//! # Why `spec/` is not committed
//!
//! The CEN validation artefacts are **EUPL-1.2**, a reciprocal licence, and the
//! vendor specifications carry their own terms. Keeping them out of the
//! repository is what keeps this crate's `MIT OR Apache-2.0` licensing clean —
//! and it is why `deny.toml`'s allow-list does not mention EUPL. Everything
//! fetched here is publicly downloadable, including the normative text of
//! EN 16931-1 itself.
//!
//! # What is fetched, and what is not
//!
//! Only what the generators and the test suites actually read — for **both**
//! crates, which is the point of fetching once. `en16931` needs the code-list
//! Schematron; `en16931-formats` needs the *preprocessed* Schematron, the only
//! form in which a rule's context is fully resolved rather than a `$Variable`,
//! and every published UBL and CII instance, because its element-order tables
//! are derived from 320 and 170 documents respectively. A table derived from
//! three examples is a guess with a large sample size written on it.
//!
//! Earlier versions of this also pulled `phive-rules` — 3.6 GB in full,
//! sparse-checked out to three modules — and three specification PDFs. Nothing
//! consumed any of them: they were research material, and a fetch that
//! downloads what nobody reads is a fetch people learn to skip.
//!
//! The normative text of EN 16931-1 is free of charge with derivative use
//! permitted under the 2018 CEN–European Commission agreement, and
//! `spec/README.md` says where to get it — but obtaining it is a reading task,
//! not a build step.
//!
//! # Why this shells out to git
//!
//! `git` is the right tool for cloning a repository, and it is already present
//! on any machine that can build this crate. The alternative is `git2` plus a
//! TLS stack — upwards of a hundred crates and an OpenSSL decision — to do what
//! one command already does correctly, paid for by every contributor running
//! `cargo xtask`.
//!
//! A missing `git` is reported by name and purpose rather than as an OS error.
//!
//! # Pinning
//!
//! The CEN artefacts are pinned to a tag, so regenerating a table is a
//! reviewable diff rather than a moving target. Bump [`CEN_REF`] deliberately,
//! and update `en16931::ARTEFACT_VERSION` to match —
//! `crates/en16931/tests/artefact_pin.rs` checks that they agree, along with the
//! revision stamped into every generated table.
//!
//! **One pin, for both crates.** Split across two repositories this was two
//! constants and a comment asking that they be kept equal; a workspace makes it
//! one, so "the two crates derived their tables from different artefact
//! revisions" is no longer a state that can be reached.

use std::path::Path;
use std::process::Command;

use crate::root;

/// The pinned CEN artefact **tag**.
///
/// Must equal `en16931::ARTEFACT_VERSION`, and the `ARTEFACT` constant the code
/// generator stamps into every table's provenance line.
/// `crates/en16931/tests/artefact_pin.rs` asserts all three.
pub const CEN_REF: &str = "validation-1.3.16";

/// Which kind of ref a source is pinned to.
///
/// This distinction is not pedantry. `eInvoicing-EN16931` publishes
/// `validation-1.3.16` as **both a tag and a branch, pointing at different
/// commits** — `refs/tags/…` is the release, `refs/heads/…` is the working
/// branch that produced it. `git clone --branch` prefers the branch, so two
/// clones of the same "pinned" ref land on different trees and the generated
/// tables differ for no reason anyone can see in a diff.
///
/// Naming the ref kind makes the pin unambiguous, and fetching the
/// fully-qualified ref makes git agree.
enum Ref {
    /// A release tag. The right pin for anything that must not move.
    Tag(&'static str),
    /// A moving branch, for sources with no releases.
    Branch(&'static str),
}

impl Ref {
    /// The fully-qualified ref, so git cannot resolve it to the other kind.
    fn qualified(&self) -> String {
        match self {
            Self::Tag(t) => format!("refs/tags/{t}"),
            Self::Branch(b) => format!("refs/heads/{b}"),
        }
    }

    fn describe(&self) -> String {
        match self {
            Self::Tag(t) => format!("tag {t}"),
            Self::Branch(b) => format!("branch {b}"),
        }
    }
}

/// A git repository to clone or update.
struct Repo {
    /// Directory name under `spec/`.
    name: &'static str,
    url: &'static str,
    /// The pinned ref, tag or branch — see [`Ref`].
    reference: Ref,
    /// Why this is here, printed so a contributor can see what they are pulling.
    purpose: &'static str,
}

static REPOS: &[Repo] = &[
    Repo {
        name: "eInvoicing-EN16931",
        url: "https://github.com/ConnectingEurope/eInvoicing-EN16931.git",
        reference: Ref::Tag(CEN_REF),
        purpose: "CEN/TC 434 — the abstract model, code lists and per-rule test corpus, \
                  the preprocessed Schematron behind the prohibition tables, and 490 \
                  published UBL and CII instances behind the element-order tables.",
    },
    Repo {
        name: "peppol-bis-invoice-3",
        url: "https://github.com/OpenPEPPOL/peppol-bis-invoice-3.git",
        reference: Ref::Branch("master"),
        purpose: "Peppol BIS Billing 3.0 — PEPPOL-EN16931-*, the national rule sets, and \
                  the widest real-world sample of UBL in the corpus suite.",
    },
    Repo {
        name: "xrechnung-schematron",
        url: "https://github.com/itplr-kosit/xrechnung-schematron.git",
        reference: Ref::Branch("master"),
        purpose: "KoSIT — the XRechnung Schematron, BR-DE-*.",
    },
    Repo {
        name: "validator-configuration-xrechnung",
        url: "https://github.com/itplr-kosit/validator-configuration-xrechnung.git",
        reference: Ref::Branch("master"),
        purpose: "KoSIT — validator scenarios and the mutation test instances.",
    },
];

/// Is this command available?
fn available(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn run(cmd: &mut Command) -> Result<String, String> {
    let out = cmd
        .output()
        .map_err(|e| format!("{:?}: {e}", cmd.get_program()))?;
    if !out.status.success() {
        return Err(format!(
            "{:?} failed: {}",
            cmd.get_program(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    run(Command::new("git").arg("-C").arg(dir).args(args))
}

/// The one-line log of whatever is checked out.
fn describe(dir: &Path) -> String {
    git(
        dir,
        &[
            "--no-pager",
            "log",
            "-1",
            "--format=%h %ad %s",
            "--date=short",
        ],
    )
    .unwrap_or_else(|_| "(no commit)".to_owned())
}

fn fetch_repo(spec: &Path, repo: &Repo) -> Result<(), String> {
    let dir = spec.join(repo.name);
    println!(
        "
==> {} @ {}",
        repo.name,
        repo.reference.describe()
    );
    println!("    {}", repo.purpose);

    // Init-and-fetch rather than clone, uniformly for a fresh and an existing
    // checkout. `git clone --branch` takes a short name and resolves it to
    // whichever ref kind it finds first; fetching the fully-qualified ref
    // cannot pick the wrong one.
    if !dir.join(".git").is_dir() {
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        git(&dir, &["init", "--quiet"])?;
        git(&dir, &["remote", "add", "origin", repo.url])?;
    }
    let qualified = repo.reference.qualified();
    git(
        &dir,
        &["fetch", "--depth", "1", "--quiet", "origin", &qualified],
    )?;
    git(&dir, &["checkout", "--quiet", "--force", "FETCH_HEAD"])?;
    println!("    {}", describe(&dir));
    Ok(())
}

/// Fetch everything into `spec/`.
///
/// # Errors
///
/// Fails if `git` is missing, or if any source cannot be retrieved. Partial
/// success is treated as failure: a half-fetched `spec/` produces a code list
/// quietly missing values, which is worse than no code list at all.
pub fn run_fetch() -> Result<(), String> {
    if !available("git") {
        return Err(
            "`git` is not on PATH, and is needed to clone the CEN, Peppol and KoSIT \
             artefact repositories"
                .to_owned(),
        );
    }

    let spec = root().join("spec");
    std::fs::create_dir_all(&spec).map_err(|e| format!("{}: {e}", spec.display()))?;

    for repo in REPOS {
        fetch_repo(&spec, repo)?;
    }
    std::fs::write(spec.join("README.md"), orientation())
        .map_err(|e| format!("spec/README.md: {e}"))?;

    println!("\ndone — see spec/README.md for what is where");
    Ok(())
}

/// The orientation file written into `spec/`.
fn orientation() -> String {
    format!(
        r"# spec/ — reference material

**Nothing here is committed.** Regenerate with `cargo xtask fetch`.

The CEN validation artefacts are EUPL-1.2, a reciprocal licence, and the vendor
specifications carry their own terms. Keeping them out of the repository is what
keeps this crate's `MIT OR Apache-2.0` licensing clean.

| Path | What | Licence |
|---|---|---|
| `eInvoicing-EN16931/` | CEN/TC 434 validation artefacts, pinned at `{CEN_REF}` — abstract model, code lists, per-rule test corpus | EUPL-1.2 |
| `peppol-bis-invoice-3/` | Peppol BIS Billing 3.0 — `PEPPOL-EN16931-*` + national rule sets | see repo |
| `xrechnung-schematron/` | KoSIT XRechnung Schematron — `BR-DE-*` | see repo |
| `validator-configuration-xrechnung/` | KoSIT validator scenarios and mutation instances | see repo |

Only what the generator and the suites read is fetched. The specification PDFs
and `phive-rules` are not: nothing consumes them, and downloading gigabytes
nobody reads is how a fetch step gets skipped.

## Where things are

    eInvoicing-EN16931/ubl/schematron/abstract/EN16931-model.sch      201 assertions, 66 contexts
    eInvoicing-EN16931/ubl/schematron/codelist/EN16931-UBL-codes.sch  22 BR-CL-* rules, ~4400 code values
    eInvoicing-EN16931/*/schematron/preprocessed/                     contexts fully resolved — the
                                                                     only form in which a prohibition
                                                                     can be read with its context
    eInvoicing-EN16931/test/Invoice-unit-UBL/                         207 per-rule test files
    peppol-bis-invoice-3/rules/sch/PEPPOL-EN16931-UBL.sch             Peppol + country rules
    xrechnung-schematron/src/validation/schematron/                   BR-DE-*

## The standard itself

Not fetched — reading it is a research task, not a build step. Parts 1 and 2 are
**free of charge with derivative use permitted** under the 2018 CEN–European
Commission agreement. Routes, best first:

| Route | What you get | Language | Notes |
|---|---|---|---|
| **ÚNMS SR (Slovakia)** — [direct PDF](https://www.normoff.gov.sk/files/docs/e-fakturacia-stn-en-16931-1-a1-614d692fbcaa2.pdf) | STN EN 16931-1+A1, 158 pp | **English** | Body is the English version of the EN verbatim. No registration. |
| **DIN Media (Germany)** — [DIN EN 16931-1:2020-12](https://www.dinmedia.de/de/norm/din-en-16931-1/327729047) | 162 pp | German | PDF download **0,00 EUR**. Free account needed. |
| **DIN Media** — [DIN CEN/TS 16931-2:2017-11](https://www.dinmedia.de/de/vornorm/din-cen-ts-16931-2/274991011) | 14 pp | German | PDF download **0,00 EUR** — this is Part 2. |
| **Any other NSB** | varies | varies | Find yours: <https://standards.cencenelec.eu/dyn/www/f?p=CEN:5> |

The CEN portal lists EN 16931-1:2017+A1:2019/AC:2020 as **Withdrawn**, superseded
by EN 16931-1:2026. The withdrawn edition is still what every deployed CIUS
validates against, so it is the right target for v1 — but obtain :2026 as well
before starting `Edition::En2026`.

Parts 3-x (syntax bindings), 4, 5 and 6 must be purchased. They are
`en16931-formats`' concern, not this crate's.

Anything obtained manually goes in `manual/`; the fetch never touches it.
"
    )
}
