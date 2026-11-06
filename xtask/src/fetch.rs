//! Fetching the reference specifications and validation artefacts into the reference corpus.
//!
//! The corpus is not committed: the CEN artefacts are EUPL-1.2 (a reciprocal
//! licence) and the vendor specifications carry their own terms, so keeping them
//! out of the repository keeps the workspace `MIT OR Apache-2.0`.
//!
//! Only what the generators and test suites of both crates read is fetched: the
//! code-list and preprocessed Schematron (the only form with every rule context
//! resolved), every published UBL and CII instance (the element-order tables are
//! derived from them), and the OASIS UBL 2.1 `xsd/` tree for the schema gate.
//!
//! `git`, `curl` and `unzip` are invoked as external tools; a missing one is
//! reported by name and purpose. The archive digest is checked in Rust.
//!
//! # Pinning
//!
//! Every source is pinned to its authority's release: the tag where one exists,
//! otherwise the commit (see [`Ref::Commit`], which states why and until when),
//! or for a download the archive's SHA-256 (see [`Archive`]). A branch is never a
//! pin: it is the authority's next release and would silently change the
//! severities this crate claims to implement. The fetch fails unless the
//! checkout's `HEAD` or the archive's digest matches the pin.
//!
//! The KoSIT pins name the XRechnung version their changelog states (`v2.6.0`:
//! XRechnung 3.0); a KoSIT release for XRechnung 4.0 is a new profile, not a
//! newer pin. [`CEN_REF`] must equal `en16931::ARTEFACT_VERSION` and the
//! revision stamped into every generated table; `crates/en16931/tests/artefact_pin.rs`
//! checks all three.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The pinned CEN artefact tag. Must equal `en16931::ARTEFACT_VERSION` and the
/// code generator's `ARTEFACT` stamp (asserted by `artefact_pin.rs`).
pub const CEN_REF: &str = "validation-1.3.17";

/// OpenPeppol BIS Billing 3.0.21-hotfix (2026-10-28; `PEPPOL-EN16931-*`,
/// `PEPPOL-COMMON-*`, national rule sets), the commit tag `3.0.21` now names.
pub const PEPPOL_REF: &str = "5df0eff7c8efafd7c79c0dacce55082d175db759";

/// The Peppol release [`PEPPOL_REF`] is.
pub const PEPPOL_RELEASE: &str = "3.0.21";

/// KoSIT's XRechnung Schematron release (`v2.6.0`: XRechnung 3.0).
pub const KOSIT_SCHEMATRON_REF: &str = "v2.6.0";

/// KoSIT's validator configuration release: `scenarios.xml` (severity overrides)
/// and the mutation corpus.
pub const KOSIT_CONFIG_REF: &str = "v2026-08-31";

/// The ZUGFeRD community corpus (Apache-2.0), pinned by commit: it has no tags.
pub const ZUGFERD_CORPUS_REF: &str = "82e886fb21369e3e83c1e97170a4ff406deceee7";

/// veraPDF's REST image, the PDF/A-3b gate: the tag and digest the gate test and
/// CI's pull name (asserted by a test).
pub const VERAPDF_TAG: &str = "v1.30.3";
/// The digest of [`VERAPDF_TAG`].
pub const VERAPDF_DIGEST: &str =
    "sha256:b0e4ce6032521f87999e92562dc226a62da1fd30f2325998d1d2f2ed6eeb5d50";

/// SHA-256 of the OASIS UBL 2.1 distribution archive (the schemas written UBL is
/// validated against). Only its `xsd/` tree is kept.
pub const UBL_SHA256: &str = "60b80d76394a8a2add90723ecb8e0e2e9d826775de9749df37a72d60703f86ed";

/// The sources whose published instances choose the parents of the order and
/// single-occurrence tables (the schemas give their children). Must equal the
/// corpus suites' `AUTHORITY_SOURCES` (asserted by a test).
pub const ORDER_SOURCES: &[&str] = &[
    "eInvoicing-EN16931",
    "peppol-bis-invoice-3",
    "xrechnung-schematron",
    "validator-configuration-xrechnung",
];

/// Which kind of ref a source is pinned to (never a branch).
///
/// `eInvoicing-EN16931` has a tag and a branch both named `validation-1.3.16` at
/// different commits, so tags are fetched fully qualified (`refs/tags/...`).
#[derive(Debug)]
pub enum Ref {
    /// A release tag.
    Tag(&'static str),
    /// A full commit SHA, for a release published without a tag.
    Commit {
        /// The 40-hex commit; the checkout's `HEAD` must equal it.
        sha: &'static str,
        /// The release this commit is, if the source numbers its releases.
        release: Option<&'static str>,
        /// Why this is not a tag pin.
        reason: &'static str,
        /// What ends the commit pin.
        until: &'static str,
    },
}

impl Ref {
    /// What to ask the remote for: the fully-qualified tag, or the commit.
    fn wanted(&self) -> String {
        match self {
            Self::Tag(t) => format!("refs/tags/{t}"),
            Self::Commit { sha, .. } => (*sha).to_owned(),
        }
    }

    fn describe(&self) -> String {
        match self {
            Self::Tag(t) => format!("tag {t}"),
            Self::Commit {
                sha, reason, until, ..
            } => {
                format!("commit {sha}\n    pinned by commit: {reason}; {until}")
            }
        }
    }
}

/// Where a source publishes a release without a tag (read by `cargo xtask watch`).
#[derive(Debug)]
pub enum Channel {
    /// Only tags.
    None,
    /// A branch named like a release tag announces that release until it is tagged.
    Branches,
    /// The release-notes page publishes a release; `<YYYY>-Q<n>-QA<m>` branches stage one.
    PeppolQa {
        /// The release-notes page; each release is a heading anchored `version_<N>_<N>_<N>`.
        release_notes: &'static str,
    },
    /// No tags at all: the head of this branch is the latest release.
    Head(&'static str),
}

/// A git repository to clone or update.
pub struct Repo {
    /// Directory name under the reference corpus.
    pub name: &'static str,
    pub url: &'static str,
    /// The pinned ref, tag or commit — see [`Ref`].
    pub reference: Ref,
    /// The prefixes of release tags: a release tag is one of these followed by
    /// digits separated by `.` or `-`, and nothing else.
    pub releases: &'static [&'static str],
    /// Where an untagged release appears.
    pub channel: Channel,
    /// Why this is fetched, printed during the fetch.
    pub purpose: &'static str,
}

pub static REPOS: &[Repo] = &[
    Repo {
        name: "eInvoicing-EN16931",
        url: "https://github.com/ConnectingEurope/eInvoicing-EN16931.git",
        reference: Ref::Tag(CEN_REF),
        releases: &["validation-"],
        channel: Channel::Branches,
        purpose: "CEN/TC 434 — the abstract model, code lists and per-rule test corpus, \
                  the preprocessed Schematron behind the prohibition tables, and 500 \
                  published UBL and CII instances behind the element-order tables.",
    },
    Repo {
        name: "peppol-bis-invoice-3",
        url: "https://github.com/OpenPEPPOL/peppol-bis-invoice-3.git",
        reference: Ref::Commit {
            sha: PEPPOL_REF,
            release: Some(PEPPOL_RELEASE),
            reason: "OpenPeppol re-pointed tag 3.0.21 to its 3.0.21-hotfix \
                     (2026-10-28); a tag that moved once is pinned by the commit it names",
            until: "until OpenPeppol publishes a later release",
        },
        releases: &["", "v"],
        channel: Channel::PeppolQa {
            release_notes: "https://docs.peppol.eu/poacc/billing/3.0/release-notes/",
        },
        purpose: "Peppol BIS Billing 3.0.21 — PEPPOL-EN16931-*, PEPPOL-COMMON-*, the \
                  national rule sets, and the widest real-world sample of UBL in the \
                  corpus suite.",
    },
    Repo {
        name: "xrechnung-schematron",
        url: "https://github.com/itplr-kosit/xrechnung-schematron.git",
        reference: Ref::Tag(KOSIT_SCHEMATRON_REF),
        releases: &["v"],
        channel: Channel::None,
        purpose: "KoSIT — the XRechnung Schematron, BR-DE-*.",
    },
    Repo {
        name: "validator-configuration-xrechnung",
        url: "https://github.com/itplr-kosit/validator-configuration-xrechnung.git",
        reference: Ref::Tag(KOSIT_CONFIG_REF),
        releases: &["v"],
        channel: Channel::None,
        purpose: "KoSIT — validator scenarios and the mutation test instances.",
    },
    Repo {
        name: "zugferd-corpus",
        url: "https://github.com/ZUGFeRD/corpus.git",
        reference: Ref::Commit {
            sha: ZUGFERD_CORPUS_REF,
            release: None,
            reason: "the corpus repository publishes no tags",
            until: "until it tags a release",
        },
        releases: &[],
        channel: Channel::Head("master"),
        purpose: "The ZUGFeRD community corpus (Apache-2.0) — real ZUGFeRD / Factur-X \
                  PDFs from real producers, pinned for the extraction sweep the \
                  assurance work will assert.",
    },
];

/// Repositories not fetched, but watched by `cargo xtask watch`, at the release
/// the workspace targets.
pub static WATCHED: &[Repo] = &[Repo {
    name: "France_RFE",
    url: "https://github.com/fnfempe/France_RFE.git",
    reference: Ref::Tag("v1.4.0.04"),
    releases: &["v", "FNFE_RFE_INVOICE_"],
    channel: Channel::None,
    purpose: "FNFE-MPE — the French rules (XP Z12-012) the French profile targets.",
}];

/// A Docker Hub image the suites or CI run, watched by `cargo xtask watch`.
pub struct Image {
    /// `<namespace>/<repository>`.
    pub name: &'static str,
    /// The tag in use; release tags are `v` and a dotted version.
    pub tag: &'static str,
    /// The tag's pinned digest, where the image is pinned by digest.
    pub digest: Option<&'static str>,
    /// Why this is watched.
    pub purpose: &'static str,
}

pub static IMAGES: &[Image] = &[
    Image {
        name: "verapdf/rest",
        tag: VERAPDF_TAG,
        digest: Some(VERAPDF_DIGEST),
        purpose: "veraPDF's REST service — the PDF/A-3b gate.",
    },
    Image {
        name: "verapdf/cli",
        tag: VERAPDF_TAG,
        digest: None,
        purpose: "veraPDF's command line, released with the REST service: a newer one \
                  is the veraPDF release the gate moves to.",
    },
];

/// A release archive to download, pinned by its SHA-256.
pub struct Archive {
    /// Directory name under the reference corpus.
    pub name: &'static str,
    pub url: &'static str,
    /// The archive's SHA-256, lowercase hex. The fetch fails on any other.
    pub sha256: &'static str,
    /// Why the pin is a digest rather than a git ref.
    reason: &'static str,
    /// The archive members kept, as an `unzip` pattern.
    keep: &'static str,
    /// Why this is here.
    pub purpose: &'static str,
}

pub static ARCHIVES: &[Archive] = &[Archive {
    name: "oasis-ubl-2.1",
    url: "https://docs.oasis-open.org/ubl/os-UBL-2.1/UBL-2.1.zip",
    sha256: UBL_SHA256,
    reason: "OASIS publishes UBL 2.1 as a downloadable OASIS Standard, not from a \
             tagged repository",
    keep: "xsd/*",
    purpose: "OASIS UBL 2.1 — the Invoice and CreditNote schemas every UBL document \
              the suites write is validated against.",
}];

/// Is this command available? `unzip` has no `--version`; `-v` is its own.
fn available(tool: &str) -> bool {
    Command::new(tool)
        .arg(if tool == "unzip" { "-v" } else { "--version" })
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

pub fn run(cmd: &mut Command) -> Result<String, String> {
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

    // Init-and-fetch the fully-qualified ref; `git clone --branch` could resolve
    // a short name to a same-named branch.
    if !dir.join(".git").is_dir() {
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        git(&dir, &["init", "--quiet"])?;
        git(&dir, &["remote", "add", "origin", repo.url])?;
    }
    let wanted = repo.reference.wanted();
    git(
        &dir,
        &["fetch", "--depth", "1", "--quiet", "origin", &wanted],
    )
    .map_err(|e| format!("{} @ {}: {e}", repo.name, repo.reference.describe()))?;
    git(&dir, &["checkout", "--quiet", "--force", "FETCH_HEAD"])?;
    if let Ref::Commit { sha, .. } = repo.reference {
        let head = git(&dir, &["rev-parse", "HEAD"])?;
        if head != sha {
            return Err(format!(
                "{}: HEAD is {head}, but the pin is commit {sha}",
                repo.name
            ));
        }
    }
    println!("    {}", describe(&dir));
    Ok(())
}

/// The SHA-256 of `bytes`, lowercase hex.
pub fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn fetch_archive(spec: &Path, archive: &Archive) -> Result<(), String> {
    let dir = spec.join(archive.name);
    println!(
        "\n==> {} @ sha256 {}\n    pinned by digest: {}",
        archive.name, archive.sha256, archive.reason
    );
    println!("    {}", archive.purpose);

    // Written last: present and equal means this archive was unpacked in full.
    let stamp = dir.join(".sha256");
    if std::fs::read_to_string(&stamp).is_ok_and(|s| s.trim() == archive.sha256) {
        println!("    already at the pinned digest");
        return Ok(());
    }
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let zip = dir.join("archive.zip");
    run(Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--output",
        ])
        .arg(&zip)
        .arg(archive.url))
    .map_err(|e| format!("{}: {e}", archive.url))?;
    let bytes = std::fs::read(&zip).map_err(|e| format!("{}: {e}", zip.display()))?;
    let got = sha256(&bytes);
    if got != archive.sha256 {
        return Err(format!(
            "{}: the archive's SHA-256 is {got}, but the pin is {}",
            archive.name, archive.sha256
        ));
    }
    run(Command::new("unzip")
        .args(["-q", "-o"])
        .arg(&zip)
        .arg(archive.keep)
        .arg("-d")
        .arg(&dir))?;
    std::fs::remove_file(&zip).map_err(|e| format!("{}: {e}", zip.display()))?;
    std::fs::write(&stamp, archive.sha256).map_err(|e| format!("{}: {e}", stamp.display()))?;
    println!("    unpacked {}", archive.keep);
    Ok(())
}

/// Fetch everything into the reference corpus.
///
/// # Errors
///
/// Fails if `git`, `curl` or `unzip` is missing, or if any source cannot be
/// retrieved. Partial success is failure: a half-fetched corpus yields code lists
/// silently missing values.
pub fn run_fetch() -> Result<(), String> {
    if !available("git") {
        return Err(
            "`git` is not on PATH, and is needed to clone the CEN, Peppol and KoSIT \
             artefact repositories"
                .to_owned(),
        );
    }

    let spec = PathBuf::from(env!("EN16931_REFERENCE"));
    std::fs::create_dir_all(&spec).map_err(|e| format!("{}: {e}", spec.display()))?;

    for repo in REPOS {
        fetch_repo(&spec, repo)?;
    }
    for tool in ["curl", "unzip"] {
        if !available(tool) {
            return Err(format!(
                "`{tool}` is not on PATH, and is needed to fetch the OASIS UBL 2.1 schemas"
            ));
        }
    }
    for archive in ARCHIVES {
        fetch_archive(&spec, archive)?;
    }
    let readme = spec.join("README.md");
    std::fs::write(&readme, orientation()).map_err(|e| format!("{}: {e}", readme.display()))?;

    println!("\ndone — see {} for what is where", readme.display());
    Ok(())
}

/// The orientation file written into the reference corpus.
fn orientation() -> String {
    format!(
        r"# The reference corpus

**Nothing here is committed.** Regenerate with `cargo xtask fetch`.

The CEN validation artefacts are EUPL-1.2, a reciprocal licence, and the vendor
specifications carry their own terms. Keeping them out of the repository is what
keeps this crate's `MIT OR Apache-2.0` licensing clean.

| Path | What | Licence |
|---|---|---|
| `eInvoicing-EN16931/` | CEN/TC 434 validation artefacts, pinned at `{CEN_REF}` — abstract model, code lists, per-rule test corpus | EUPL-1.2 |
| `peppol-bis-invoice-3/` | Peppol BIS Billing {PEPPOL_RELEASE}, commit `{PEPPOL_REF}` — `PEPPOL-EN16931-*`, `PEPPOL-COMMON-*` + national rule sets | see repo |
| `xrechnung-schematron/` | KoSIT XRechnung Schematron `{KOSIT_SCHEMATRON_REF}` — `BR-DE-*` | see repo |
| `validator-configuration-xrechnung/` | KoSIT validator scenarios and mutation instances, `{KOSIT_CONFIG_REF}` | see repo |
| `zugferd-corpus/` | The ZUGFeRD community corpus, commit `{ZUGFERD_CORPUS_REF}` | Apache-2.0 |
| `oasis-ubl-2.1/` | OASIS UBL 2.1 schemas (`xsd/`), archive SHA-256 `{UBL_SHA256}` | OASIS |

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

#[cfg(test)]
mod tests {
    use super::{REPOS, Ref};

    /// A digest pin is a full lowercase SHA-256 with a reason.
    #[test]
    fn every_archive_pin_is_a_full_digest_with_a_reason() {
        for archive in super::ARCHIVES {
            assert!(
                archive.sha256.len() == 64
                    && archive
                        .sha256
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "{}: `{}` is not a lowercase SHA-256",
                archive.name,
                archive.sha256
            );
            assert!(!archive.reason.trim().is_empty(), "{}", archive.name);
            assert!(archive.url.starts_with("https://"), "{}", archive.name);
        }
        assert_eq!(
            super::sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// The veraPDF image the watch reads is the one the PDF/A gate runs and CI pulls.
    #[test]
    fn the_watched_verapdf_reference_is_the_one_in_use() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let read = |rel: &str| std::fs::read_to_string(root.join(rel)).expect(rel);
        let rest = &super::IMAGES[0];
        let reference = format!("{}@{}", rest.tag, rest.digest.expect("pinned by digest"));
        assert!(read(".github/workflows/ci.yml").contains(&format!("{}:{reference}", rest.name)));
        let gate = read("crates/en16931-formats/tests/zugferd_embed.rs");
        assert!(gate.contains(&format!("\"{}\"", rest.name)));
        assert!(gate.contains(&format!("\"{reference}\"")));
    }

    /// A commit pin is a full SHA with a reason and an end condition.
    #[test]
    fn every_commit_pin_states_why_and_until_when() {
        for repo in REPOS {
            if let Ref::Commit {
                sha, reason, until, ..
            } = repo.reference
            {
                assert!(
                    sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()),
                    "{}: `{sha}` is not a full commit SHA",
                    repo.name
                );
                assert!(
                    !reason.trim().is_empty(),
                    "{}: a commit pin must say why there is no tag",
                    repo.name
                );
                assert!(
                    until.trim_start().starts_with("until "),
                    "{}: a commit pin must say what ends it",
                    repo.name
                );
            }
        }
    }
}
