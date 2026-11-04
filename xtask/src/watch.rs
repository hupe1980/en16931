//! `cargo xtask watch`: is any pin behind its source?
//!
//! Every pin in [`crate::fetch`] is compared with what its source publishes now:
//! the fetched repositories and archive, the watched-only repositories, and the
//! images. A source is `current`, or has findings, each one of:
//!
//! - `released` — a newer release: a release tag, a release on the source's
//!   release channel (the Peppol release notes), a moved head of an untagged
//!   repository, a changed archive or image digest;
//! - `announced` — a release prepared but neither tagged nor declared released:
//!   a CEN `validation-*` branch without its tag, a Peppol QA branch;
//! - `unknown` — the source could not be read. Never reported as current.
//!
//! The report is one JSON document (see [`SourceReport::json`]), with each drifted
//! source's issue title and body ready to post. Observing is `git ls-remote`,
//! `curl` (the Docker Hub tags API, the release-notes page, the archive, hashed in
//! memory); comparing is a pure function of what was observed. Nothing is written
//! into the reference corpus and no pin moves.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use serde_json::{Value, json};

use crate::fetch::{self, Archive, Channel, Image, Ref, Repo};

/// How far a source is behind its pin, least to most severe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    Current,
    Announced,
    Released,
    Unknown,
}

impl State {
    fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Announced => "announced",
            Self::Released => "released",
            Self::Unknown => "unknown",
        }
    }
}

/// One thing found upstream.
#[derive(Debug, PartialEq, Eq)]
pub struct Finding {
    pub state: State,
    pub text: String,
}

fn finding(state: State, text: impl Into<String>) -> Finding {
    Finding {
        state,
        text: text.into(),
    }
}

/// A release version: the numeric components after a release-tag prefix.
type Version = Vec<u64>;

/// The version of `tag` if it is a release tag: one of `prefixes`, then digits
/// separated by `.` or `-`, and nothing else (so no `-rc1`, no `pre-release-…`).
fn version(tag: &str, prefixes: &[&str]) -> Option<Version> {
    prefixes.iter().find_map(|prefix| {
        let rest = tag.strip_prefix(prefix)?;
        if !rest.starts_with(|c: char| c.is_ascii_digit()) {
            return None;
        }
        rest.split(['.', '-'])
            .map(|n| {
                if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) {
                    n.parse().ok()
                } else {
                    None
                }
            })
            .collect()
    })
}

/// Numeric comparison; a missing component counts as 0 (`1.5` = `1.5.0`).
fn compare(a: &[u64], b: &[u64]) -> Ordering {
    (0..a.len().max(b.len()))
        .map(|i| {
            a.get(i)
                .copied()
                .unwrap_or(0)
                .cmp(&b.get(i).copied().unwrap_or(0))
        })
        .find(|o| o.is_ne())
        .unwrap_or(Ordering::Equal)
}

fn dotted(v: &[u64]) -> String {
    v.iter().map(u64::to_string).collect::<Vec<_>>().join(".")
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

/// A repository's tags (peeled to the commit) and branch heads.
#[derive(Debug, Default)]
pub struct Refs {
    pub tags: BTreeMap<String, String>,
    pub heads: BTreeMap<String, String>,
}

impl Refs {
    /// Parse `git ls-remote --tags --heads` output. An annotated tag's peeled
    /// `^{}` line, the commit it points at, wins over the tag object.
    pub fn parse(ls_remote: &str) -> Self {
        let mut refs = Self::default();
        for line in ls_remote.lines() {
            let Some((sha, name)) = line.split_once(char::is_whitespace) else {
                continue;
            };
            let (sha, name) = (sha.to_owned(), name.trim());
            if let Some(tag) = name.strip_prefix("refs/tags/") {
                match tag.strip_suffix("^{}") {
                    Some(peeled) => {
                        refs.tags.insert(peeled.to_owned(), sha);
                    }
                    None => {
                        refs.tags.entry(tag.to_owned()).or_insert(sha);
                    }
                }
            } else if let Some(head) = name.strip_prefix("refs/heads/") {
                refs.heads.insert(head.to_owned(), sha);
            }
        }
        refs
    }
}

/// A Peppol QA branch: exactly `<YYYY>-Q<n>-QA<m>` (`m` may be absent).
fn is_qa_branch(name: &str) -> bool {
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    match name.split('-').collect::<Vec<_>>()[..] {
        [year, quarter, qa] => {
            year.len() == 4
                && digits(year)
                && quarter
                    .strip_prefix('Q')
                    .is_some_and(|q| matches!(q, "1" | "2" | "3" | "4"))
                && qa.strip_prefix("QA").is_some_and(digits)
        }
        _ => false,
    }
}

/// A release a release-notes page lists: its version (a suffixed release such as
/// a hotfix sorts after its base) and its name.
pub type Listed = (Version, String);

/// The newest release a release-notes page lists: the highest heading anchored
/// `id="_version_<N>_<N>_<N>"` (or without the leading `_`), where a trailing
/// `_<word>` (`_hotfix`) is a later release of the same version.
pub fn release_notes_version(html: &str) -> Option<Listed> {
    html.split("id=\"")
        .skip(1)
        .filter_map(|rest| {
            let id = rest[..rest.find('"')?].trim_start_matches('_');
            let parts: Vec<&str> = id.strip_prefix("version_")?.split('_').collect();
            let numeric = parts
                .iter()
                .take_while(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
                .count();
            if numeric == 0 {
                return None;
            }
            let mut version: Version = parts[..numeric]
                .iter()
                .map(|n| n.parse().ok())
                .collect::<Option<_>>()?;
            let mut name = dotted(&version);
            if numeric < parts.len() {
                version.push(1);
                name = format!("{name}-{}", parts[numeric..].join("-"));
            }
            Some((version, name))
        })
        .max_by(|a, b| compare(&a.0, &b.0))
}

/// Compare a repository's pin with its refs and, for a Peppol channel, the
/// newest release the release notes list (or why they could not be read).
pub fn compare_repo(
    repo: &Repo,
    refs: &Refs,
    notes: Option<&Result<Listed, String>>,
) -> Vec<Finding> {
    let mut out = Vec::new();
    let (pinned, pinned_commit, by_commit) = match repo.reference {
        Ref::Tag(tag) => {
            let commit = refs.tags.get(tag).map(String::as_str);
            if commit.is_none() {
                out.push(finding(
                    State::Unknown,
                    format!("the pinned tag `{tag}` is not upstream"),
                ));
            }
            (version(tag, repo.releases), commit, false)
        }
        Ref::Commit { sha, release, .. } => {
            (release.and_then(|r| version(r, &[""])), Some(sha), true)
        }
    };

    // Release tags: newer than the pin; for a commit pin also the pinned release's
    // own tag. An untagged, unnumbered source: any tag at all.
    let mut tagged: Vec<(Version, &str, &str)> = Vec::new();
    for (tag, sha) in &refs.tags {
        match (&pinned, version(tag, repo.releases)) {
            (Some(p), Some(v)) => {
                let order = compare(&v, p);
                if order.is_gt() || (by_commit && order.is_eq()) {
                    tagged.push((v, tag, sha));
                }
            }
            (None, _) if by_commit => tagged.push((Vec::new(), tag, sha)),
            _ => {}
        }
    }
    tagged.sort();
    for (v, tag, sha) in tagged {
        let text = match pinned_commit {
            Some(pin) if by_commit && pinned.as_deref() == Some(v.as_slice()) => {
                // The release's tag naming the pinned commit is the strongest
                // pin: both agree, and a later re-tag is noticed here.
                if sha == pin {
                    continue;
                } else {
                    format!(
                        "tag `{tag}` is the pinned release, but points at {}, not at the \
                         pinned commit {}",
                        short(sha),
                        short(pin)
                    )
                }
            }
            _ if pinned.is_none() => format!("tag `{tag}` ({}) appeared", short(sha)),
            _ => format!("release tag `{tag}` ({})", short(sha)),
        };
        out.push(finding(State::Released, text));
    }

    match repo.channel {
        Channel::None => {}
        Channel::Branches => {
            for (head, sha) in &refs.heads {
                let newer = version(head, repo.releases)
                    .zip(pinned.as_ref())
                    .is_some_and(|(v, p)| compare(&v, p).is_gt());
                if newer && !refs.tags.contains_key(head) {
                    out.push(finding(
                        State::Announced,
                        format!(
                            "branch `{head}` ({}) has no tag: announced, not released",
                            short(sha)
                        ),
                    ));
                }
            }
        }
        Channel::PeppolQa { release_notes } => {
            match notes {
                Some(Ok((listed, name))) => {
                    if pinned.as_ref().is_some_and(|p| compare(listed, p).is_gt()) {
                        out.push(finding(
                            State::Released,
                            format!("the release notes ({release_notes}) list {name}"),
                        ));
                    }
                }
                Some(Err(e)) => out.push(finding(
                    State::Unknown,
                    format!("the release notes ({release_notes}): {e}"),
                )),
                None => out.push(finding(
                    State::Unknown,
                    format!("the release notes ({release_notes}) were not read"),
                )),
            }
            for (head, sha) in &refs.heads {
                if is_qa_branch(head) && Some(sha.as_str()) != pinned_commit {
                    out.push(finding(
                        State::Announced,
                        format!(
                            "QA branch `{head}` ({}) stages a release: announced, not released",
                            short(sha)
                        ),
                    ));
                }
            }
        }
        Channel::Head(branch) => match (refs.heads.get(branch), pinned_commit) {
            (Some(head), Some(pin)) if head != pin => out.push(finding(
                State::Released,
                format!("`{branch}` is at {head}; the pin is {pin}"),
            )),
            (Some(_), _) => {}
            (None, _) => out.push(finding(
                State::Unknown,
                format!("branch `{branch}` is not upstream"),
            )),
        },
    }
    out
}

/// Compare an archive's pinned digest with the digest it has now.
pub fn compare_archive(archive: &Archive, digest: &str) -> Vec<Finding> {
    if digest == archive.sha256 {
        Vec::new()
    } else {
        vec![finding(
            State::Released,
            format!("the archive at {} now has SHA-256 {digest}", archive.url),
        )]
    }
}

/// Compare an image's pin with its Docker Hub tags (name, digest).
pub fn compare_image(image: &Image, tags: &[(String, Option<String>)]) -> Vec<Finding> {
    const RELEASES: &[&str] = &["v"];
    let mut out = Vec::new();
    match tags.iter().find(|(t, _)| t == image.tag) {
        None => out.push(finding(
            State::Unknown,
            format!("the pinned tag `{}` is not on Docker Hub", image.tag),
        )),
        Some((_, digest)) => {
            if let Some(pin) = image.digest
                && digest.as_deref() != Some(pin)
            {
                out.push(finding(
                    State::Released,
                    format!(
                        "tag `{}` now has digest {}; the pin is {pin}",
                        image.tag,
                        digest.as_deref().unwrap_or("(none)")
                    ),
                ));
            }
        }
    }
    let Some(pinned) = version(image.tag, RELEASES) else {
        return out;
    };
    let mut newer: Vec<(Version, &str)> = tags
        .iter()
        .filter_map(|(t, _)| {
            let v = version(t, RELEASES)?;
            compare(&v, &pinned).is_gt().then_some((v, t.as_str()))
        })
        .collect();
    newer.sort();
    for (_, tag) in newer {
        out.push(finding(State::Released, format!("release tag `{tag}`")));
    }
    out
}

/// One watched source in the report.
#[derive(Debug)]
pub struct SourceReport {
    pub name: String,
    pub kind: &'static str,
    pub upstream: String,
    pub pin: String,
    /// Why the source is pinned or watched.
    pub purpose: String,
    pub findings: Vec<Finding>,
}

impl SourceReport {
    /// The most severe finding's state; `current` without findings.
    pub fn state(&self) -> State {
        self.findings
            .iter()
            .map(|f| f.state)
            .max()
            .unwrap_or(State::Current)
    }

    /// The issue's title: one issue per source.
    fn title(&self) -> String {
        format!("Upstream: {} moved", self.name)
    }

    /// The issue's body, dated so a stale date shows the watch stopped.
    fn body(&self, date: &str) -> String {
        let mut body = format!(
            "**Source:** `{}` ({})\n{}\n\n**Pin:** {}\n**State:** {}\n**Last run:** {date}\n\n",
            self.name,
            self.upstream,
            self.purpose,
            self.pin,
            self.state().as_str()
        );
        for f in &self.findings {
            body.push_str(&format!("- **{}** — {}\n", f.state.as_str(), f.text));
        }
        body.push_str(
            "\nReported by `cargo xtask watch`. Moving the pin is a reviewed change; \
             this issue is closed when the pin catches up.\n",
        );
        body
    }

    /// `{name, kind, upstream, pin, state, findings: [{state, text}], issue?: {title, body}}`.
    pub fn json(&self, date: &str) -> Value {
        let mut v = json!({
            "name": self.name,
            "kind": self.kind,
            "upstream": self.upstream,
            "pin": self.pin,
            "state": self.state().as_str(),
            "findings": self.findings.iter().map(|f| json!({
                "state": f.state.as_str(),
                "text": f.text,
            })).collect::<Vec<_>>(),
        });
        if self.state() != State::Current {
            v["issue"] = json!({ "title": self.title(), "body": self.body(date) });
        }
        v
    }
}

/// The whole report (schema 1).
pub fn report(date: &str, sources: &[SourceReport]) -> Value {
    json!({
        "schema": 1,
        "date": date,
        "sources": sources.iter().map(|s| s.json(date)).collect::<Vec<_>>(),
    })
}

// ── Observing ────────────────────────────────────────────────────────────────

fn ls_remote(url: &str) -> Result<Refs, String> {
    fetch::run(Command::new("git").env("GIT_TERMINAL_PROMPT", "0").args([
        "ls-remote",
        "--tags",
        "--heads",
        url,
    ]))
    .map(|out| Refs::parse(&out))
}

fn curl(url: &str) -> Result<Vec<u8>, String> {
    let out = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            "600",
            url,
        ])
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(format!(
            "{url}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

fn read_release_notes(url: &str) -> Result<Listed, String> {
    let html = String::from_utf8(curl(url)?).map_err(|e| format!("{url}: {e}"))?;
    release_notes_version(&html).ok_or_else(|| format!("{url}: no `version_…` heading"))
}

/// Every tag of a Docker Hub repository, with its digest.
fn docker_tags(name: &str) -> Result<Vec<(String, Option<String>)>, String> {
    let (namespace, repository) = name
        .split_once('/')
        .ok_or_else(|| format!("`{name}` is not `<namespace>/<repository>`"))?;
    let mut url = Some(format!(
        "https://hub.docker.com/v2/namespaces/{namespace}/repositories/{repository}/tags?page_size=100"
    ));
    let mut tags = Vec::new();
    // Bounded: 20 pages of 100 tags is far beyond any image watched here.
    for _ in 0..20 {
        let Some(page) = url.take() else {
            return Ok(tags);
        };
        let body: Value =
            serde_json::from_slice(&curl(&page)?).map_err(|e| format!("{page}: {e}"))?;
        let results = body["results"]
            .as_array()
            .ok_or_else(|| format!("{page}: no `results`"))?;
        for r in results {
            if let Some(tag) = r["name"].as_str() {
                tags.push((tag.to_owned(), r["digest"].as_str().map(str::to_owned)));
            }
        }
        url = body["next"].as_str().map(str::to_owned);
    }
    Err(format!("{name}: more than 20 pages of tags"))
}

fn observe_repo(repo: &Repo) -> SourceReport {
    let pin = match repo.reference {
        Ref::Tag(tag) => format!("tag `{tag}`"),
        Ref::Commit {
            sha,
            release: Some(r),
            ..
        } => format!("commit `{sha}` (release {r})"),
        Ref::Commit { sha, .. } => format!("commit `{sha}`"),
    };
    let notes = match repo.channel {
        Channel::PeppolQa { release_notes } => Some(read_release_notes(release_notes)),
        _ => None,
    };
    let findings = match ls_remote(repo.url) {
        Ok(refs) => compare_repo(repo, &refs, notes.as_ref()),
        Err(e) => {
            let mut f = vec![finding(State::Unknown, format!("`git ls-remote`: {e}"))];
            if let Some(Err(e)) = notes {
                f.push(finding(State::Unknown, e));
            }
            f
        }
    };
    SourceReport {
        name: repo.name.to_owned(),
        kind: "git",
        upstream: repo.url.to_owned(),
        pin,
        purpose: repo.purpose.to_owned(),
        findings,
    }
}

fn observe_archive(archive: &Archive) -> SourceReport {
    let findings = match curl(archive.url) {
        Ok(bytes) => compare_archive(archive, &fetch::sha256(&bytes)),
        Err(e) => vec![finding(State::Unknown, e)],
    };
    SourceReport {
        name: archive.name.to_owned(),
        kind: "archive",
        upstream: archive.url.to_owned(),
        pin: format!("SHA-256 `{}`", archive.sha256),
        purpose: archive.purpose.to_owned(),
        findings,
    }
}

fn observe_image(image: &Image) -> SourceReport {
    let findings = match docker_tags(image.name) {
        Ok(tags) => compare_image(image, &tags),
        Err(e) => vec![finding(State::Unknown, e)],
    };
    SourceReport {
        name: image.name.to_owned(),
        kind: "image",
        upstream: format!("https://hub.docker.com/r/{}", image.name),
        pin: match image.digest {
            Some(d) => format!("`{}@{d}`", image.tag),
            None => format!("tag `{}`", image.tag),
        },
        purpose: image.purpose.to_owned(),
        findings,
    }
}

/// Today's date (UTC), `YYYY-MM-DD`.
fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (y, m, d) = civil(i64::try_from(secs / 86_400).unwrap_or(0));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Days since 1970-01-01 to a proleptic Gregorian date.
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// `cargo xtask watch [--out FILE]`.
///
/// # Errors
///
/// Fails on a bad argument, if the report cannot be written, and if any source
/// is `unknown` — after every other source has been reported.
pub fn run_watch(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let mut out: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                out = Some(args.next().ok_or("`--out` needs a file")?.into());
            }
            other => return Err(format!("watch: unknown argument {other:?}")),
        }
    }

    let mut sources = Vec::new();
    for repo in fetch::REPOS.iter().chain(fetch::WATCHED) {
        sources.push(observe_repo(repo));
    }
    for archive in fetch::ARCHIVES {
        sources.push(observe_archive(archive));
    }
    for image in fetch::IMAGES {
        sources.push(observe_image(image));
    }

    for s in &sources {
        eprintln!("  {:<9}  {}  ({})", s.state().as_str(), s.name, s.pin);
        for f in &s.findings {
            eprintln!("             {}: {}", f.state.as_str(), f.text);
        }
    }
    let json = serde_json::to_string_pretty(&report(&today(), &sources))
        .map_err(|e| format!("report: {e}"))?;
    match out {
        Some(path) => {
            std::fs::write(&path, json + "\n").map_err(|e| format!("{}: {e}", path.display()))?
        }
        None => println!("{json}"),
    }

    let unknown: Vec<&str> = sources
        .iter()
        .filter(|s| s.state() == State::Unknown)
        .map(|s| s.name.as_str())
        .collect();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(format!("could not read: {}", unknown.join(", ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 2026-10-10 pins (before the refresh), as `fetch.rs` would declare them.
    static PINS_2026_10_10: &[Repo] = &[
        Repo {
            name: "eInvoicing-EN16931",
            url: "",
            reference: Ref::Tag("validation-1.3.16"),
            releases: &["validation-"],
            channel: Channel::Branches,
            purpose: "",
        },
        Repo {
            name: "peppol-bis-invoice-3",
            url: "",
            reference: Ref::Tag("v3.0.20"),
            releases: &["", "v"],
            channel: Channel::PeppolQa { release_notes: "" },
            purpose: "",
        },
        Repo {
            name: "xrechnung-schematron",
            url: "",
            reference: Ref::Tag("v2.5.0"),
            releases: &["v"],
            channel: Channel::None,
            purpose: "",
        },
        Repo {
            name: "validator-configuration-xrechnung",
            url: "",
            reference: Ref::Tag("v2026-01-31"),
            releases: &["v"],
            channel: Channel::None,
            purpose: "",
        },
    ];

    const CEN: &str = "\
b6c9e06a59 refs/tags/validation-1.3.16
bf3a7d8d71 refs/tags/validation-1.3.15
9542e51e37 refs/tags/validation-1.3.14.1
6ce8fa23d4 refs/heads/validation-1.3.15
1111111111 refs/heads/validation-1.3.16
d0dea7300c refs/heads/validation-1.3.17
07812a7c6f refs/heads/master
";
    const PEPPOL: &str = "\
a29924ed6a refs/tags/3.0.18
78d7f7dfa2 refs/tags/v3.0.19
261c458474 refs/tags/v3.0.20
261c458474 refs/heads/master
806866bd2b refs/heads/2026-Q2-QA2
7735168a66 refs/heads/2026-Q4-DEV
c954a27354 refs/heads/2026-Q4-MR
d2c19fc61a refs/heads/qa/f-b1-remove-stale-build-evidence
ceaf0a1a4a refs/heads/selfbilling-2026-Q4-QA1
";
    const SCHEMATRON: &str = "\
9efea62274 refs/tags/2.5.0-rc1
373367e2fc refs/tags/2.6.0-rc1
1b5ef35173 refs/tags/pre-release-2018-08-22
d846db09d2 refs/tags/release-2.4.0
c4eb3dae54 refs/tags/v2.5.0
0000000000 refs/tags/v2.6.0
217c6cbf8a refs/tags/v2.6.0^{}
";
    const CONFIG: &str = "\
2bed845bfe refs/tags/2026-08-31-rc1
c5782214ad refs/tags/release-2025-07-10
4b8cd64524 refs/tags/v2026-01-31
b436fc4bf0 refs/tags/v2026-08-31
";

    fn texts(f: &[Finding]) -> Vec<(State, &str)> {
        f.iter().map(|f| (f.state, f.text.as_str())).collect()
    }

    /// SC-001: on 2026-10-10 three sources were behind a release (both KoSIT,
    /// Peppol through its release notes) and CEN 1.3.17 was announced.
    #[test]
    fn the_2026_10_10_pins_drift_as_observed_that_day() {
        let notes = Ok((vec![3, 0, 21], "3.0.21".to_owned()));
        let [cen, peppol, schematron, config] = PINS_2026_10_10 else {
            unreachable!()
        };

        let f = compare_repo(cen, &Refs::parse(CEN), None);
        assert_eq!(
            texts(&f),
            [(
                State::Announced,
                "branch `validation-1.3.17` (d0dea73) has no tag: announced, not released"
            )]
        );

        let f = compare_repo(peppol, &Refs::parse(PEPPOL), Some(&notes));
        assert_eq!(
            texts(&f),
            [
                (State::Released, "the release notes () list 3.0.21"),
                (
                    State::Announced,
                    "QA branch `2026-Q2-QA2` (806866b) stages a release: announced, not released"
                ),
            ]
        );

        let f = compare_repo(schematron, &Refs::parse(SCHEMATRON), None);
        assert_eq!(
            texts(&f),
            [(State::Released, "release tag `v2.6.0` (217c6cb)")]
        );

        let f = compare_repo(config, &Refs::parse(CONFIG), None);
        assert_eq!(
            texts(&f),
            [(State::Released, "release tag `v2026-08-31` (b436fc4)")]
        );

        let states: Vec<State> = PINS_2026_10_10
            .iter()
            .zip([CEN, PEPPOL, SCHEMATRON, CONFIG])
            .map(|(repo, refs)| SourceReport {
                name: repo.name.to_owned(),
                kind: "git",
                upstream: String::new(),
                pin: String::new(),
                purpose: String::new(),
                findings: compare_repo(repo, &Refs::parse(refs), Some(&notes)),
            })
            .map(|s| s.state())
            .collect();
        assert_eq!(
            states,
            [
                State::Announced,
                State::Released,
                State::Released,
                State::Released
            ]
        );
    }

    /// Today's pins against today's refs: CEN 1.3.17 stays announced, Peppol is
    /// current while the notes list 3.0.21 and no QA branch exists.
    #[test]
    fn todays_peppol_pin_is_current() {
        let peppol = &fetch::REPOS[1];
        assert_eq!(peppol.name, "peppol-bis-invoice-3");
        let refs = Refs::parse(
            "261c458474 refs/tags/v3.0.20\nbd9f24ed25 refs/heads/selfbilling-2026-Q2-QA\n",
        );
        assert!(
            compare_repo(
                peppol,
                &refs,
                Some(&Ok((vec![3, 0, 21], "3.0.21".to_owned())))
            )
            .is_empty()
        );
    }

    /// US2-3: a commit pin's release gets a tag — at the pinned commit or not.
    #[test]
    fn a_commit_pins_release_tag_says_whether_it_is_the_pinned_commit() {
        let peppol = &fetch::REPOS[1];
        let notes = Ok((vec![3, 0, 21], "3.0.21".to_owned()));
        // The tag names the pinned commit: current, nothing to report.
        let at_pin = Refs::parse(&format!("{} refs/tags/v3.0.21\n", fetch::PEPPOL_REF));
        assert!(compare_repo(peppol, &at_pin, Some(&notes)).is_empty());
        let elsewhere = Refs::parse("abcdef0123 refs/tags/3.0.21\nfedcba9876 refs/tags/3.0.22\n");
        assert_eq!(
            texts(&compare_repo(peppol, &elsewhere, Some(&notes))),
            [
                (
                    State::Released,
                    "tag `3.0.21` is the pinned release, but points at abcdef0, not at the \
                     pinned commit 5df0eff"
                ),
                (State::Released, "release tag `3.0.22` (fedcba9)"),
            ]
        );
    }

    /// US2-4: the untagged ZUGFeRD corpus is behind when its head moved.
    #[test]
    fn an_untagged_repositorys_moved_head_names_both_commits() {
        let corpus = &fetch::REPOS[4];
        assert_eq!(corpus.name, "zugferd-corpus");
        let pin = fetch::ZUGFERD_CORPUS_REF;
        assert!(
            compare_repo(
                corpus,
                &Refs::parse(&format!("{pin} refs/heads/master")),
                None
            )
            .is_empty()
        );
        let moved = compare_repo(corpus, &Refs::parse("0123456789 refs/heads/master"), None);
        assert_eq!(
            texts(&moved),
            [(
                State::Released,
                format!("`master` is at 0123456789; the pin is {pin}").as_str()
            )]
        );
        let tagged = compare_repo(
            corpus,
            &Refs::parse(&format!(
                "{pin} refs/heads/master\n0123456789 refs/tags/2.0\n"
            )),
            None,
        );
        assert_eq!(
            texts(&tagged),
            [(State::Released, "tag `2.0` (0123456) appeared")]
        );
    }

    /// US3: what cannot be read is unknown — and dominates the source's state.
    #[test]
    fn what_cannot_be_read_is_unknown_never_current() {
        let peppol = &fetch::REPOS[1];
        let refs = Refs::parse("261c458474 refs/tags/v3.0.20\n");
        let f = compare_repo(peppol, &refs, Some(&Err("HTTP 503".to_owned())));
        assert_eq!(f[0].state, State::Unknown);

        let schematron = &fetch::REPOS[2];
        let f = compare_repo(schematron, &Refs::default(), None);
        assert_eq!(
            texts(&f),
            [(State::Unknown, "the pinned tag `v2.6.0` is not upstream")]
        );

        let report = SourceReport {
            name: "x".to_owned(),
            kind: "git",
            upstream: String::new(),
            pin: String::new(),
            purpose: String::new(),
            findings: vec![
                finding(State::Released, "a"),
                finding(State::Unknown, "b"),
                finding(State::Announced, "c"),
            ],
        };
        assert_eq!(report.state(), State::Unknown);
        let json = report.json("2026-10-11");
        assert_eq!(json["state"], "unknown");
        assert_eq!(json["issue"]["title"], "Upstream: x moved");
        assert!(
            json["issue"]["body"]
                .as_str()
                .unwrap()
                .contains("**Last run:** 2026-10-11")
        );

        let current = SourceReport {
            findings: Vec::new(),
            ..report
        };
        assert_eq!(current.state(), State::Current);
        assert!(current.json("2026-10-11").get("issue").is_none());
    }

    #[test]
    fn release_tags_are_a_declared_prefix_and_a_version_only() {
        assert_eq!(version("v2.6.0", &["v"]), Some(vec![2, 6, 0]));
        assert_eq!(version("v2026-08-31", &["v"]), Some(vec![2026, 8, 31]));
        assert_eq!(version("3.0.18", &["", "v"]), Some(vec![3, 0, 18]));
        assert_eq!(version("v3.0.20", &["", "v"]), Some(vec![3, 0, 20]));
        assert_eq!(
            version("FNFE_RFE_INVOICE_1.4.0", &["v", "FNFE_RFE_INVOICE_"]),
            Some(vec![1, 4, 0])
        );
        for not_a_release in [
            "2.6.0-rc1",
            "pre-release-2018-08-22",
            "release-2.4.0",
            "v",
            "v1..2",
            "xrechnung-1_1-2017-12-19",
        ] {
            assert_eq!(version(not_a_release, &["v"]), None, "{not_a_release}");
        }
        assert!(compare(&[1, 5], &[1, 4, 0, 4]).is_gt());
        assert!(compare(&[1, 4], &[1, 4, 0]).is_eq());
        assert!(compare(&[3, 0, 9], &[3, 0, 20]).is_lt());
    }

    #[test]
    fn every_tag_pin_is_a_release_under_its_own_prefixes() {
        for repo in fetch::REPOS.iter().chain(fetch::WATCHED) {
            if let Ref::Tag(tag) = repo.reference {
                assert!(
                    version(tag, repo.releases).is_some(),
                    "{}: `{tag}`",
                    repo.name
                );
            }
        }
        for image in fetch::IMAGES {
            assert!(version(image.tag, &["v"]).is_some(), "{}", image.name);
        }
    }

    #[test]
    fn qa_branches_are_exactly_year_quarter_qa() {
        for qa in ["2026-Q2-QA2", "2026-Q4-QA1", "2025-Q1-QA"] {
            assert!(is_qa_branch(qa), "{qa}");
        }
        for not in [
            "2026-Q4-DEV",
            "2026-Q4-MR",
            "selfbilling-2026-Q4-QA1",
            "qa/f-m4-p0112-message",
            "2026-Q5-QA1",
            "master",
        ] {
            assert!(!is_qa_branch(not), "{not}");
        }
    }

    #[test]
    fn the_release_notes_newest_version_heading_is_read() {
        // As docs.peppol.eu renders them on 2026-10-11.
        let html = r#"<h2 id="_version_3_0_9">Version 3.0.9</h2><h2 id="_version_3_0_21">3.0.21</h2>
            <h2 id="_version_3_0_20_hotfix">3.0.20-hotfix</h2><a id="version_notes">x</a>"#;
        assert_eq!(
            release_notes_version(html),
            Some((vec![3, 0, 21], "3.0.21".to_owned()))
        );
        let hotfix = r#"<h2 id="_version_3_0_21">x</h2><h2 id="_version_3_0_21_hotfix">x</h2>"#;
        assert_eq!(
            release_notes_version(hotfix),
            Some((vec![3, 0, 21, 1], "3.0.21-hotfix".to_owned()))
        );
        assert_eq!(
            release_notes_version(r#"<h2 id="version_3_0_20">x</h2>"#)
                .unwrap()
                .1,
            "3.0.20"
        );
        assert_eq!(release_notes_version("<h1>Release notes</h1>"), None);
    }

    #[test]
    fn images_report_newer_tags_and_a_moved_digest() {
        let rest = &fetch::IMAGES[0];
        let digest = rest.digest.map(str::to_owned);
        let tags = |extra: &[(&str, Option<String>)]| -> Vec<(String, Option<String>)> {
            [
                ("latest", digest.clone()),
                ("v1.30.2", None),
                ("v1.30.3", digest.clone()),
            ]
            .iter()
            .chain(extra)
            .map(|(t, d)| ((*t).to_owned(), d.clone()))
            .collect()
        };
        assert!(compare_image(rest, &tags(&[])).is_empty());
        assert_eq!(
            texts(&compare_image(
                rest,
                &tags(&[("v1.30.4", None), ("v1.31.0", None)])
            )),
            [
                (State::Released, "release tag `v1.30.4`"),
                (State::Released, "release tag `v1.31.0`")
            ]
        );
        let repushed = vec![("v1.30.3".to_owned(), Some("sha256:00".to_owned()))];
        assert_eq!(compare_image(rest, &repushed)[0].state, State::Released);
        assert_eq!(compare_image(rest, &[])[0].state, State::Unknown);
    }

    #[test]
    fn an_archive_with_another_digest_is_released() {
        let ubl = &fetch::ARCHIVES[0];
        assert!(compare_archive(ubl, ubl.sha256).is_empty());
        assert_eq!(compare_archive(ubl, "00")[0].state, State::Released);
    }

    #[test]
    fn peeled_tags_win_and_heads_stay_apart() {
        let refs = Refs::parse("aaaa refs/tags/t^{}\nbbbb refs/tags/t\ncccc refs/heads/t\n");
        assert_eq!(refs.tags["t"], "aaaa");
        assert_eq!(refs.heads["t"], "cccc");
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(11_017), (2000, 3, 1));
        assert_eq!(civil(20_737), (2026, 10, 11));
        assert_eq!(civil(-1), (1969, 12, 31));
    }
}
