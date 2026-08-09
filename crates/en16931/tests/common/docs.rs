//! Reading the project's own prose back, so a documented number can be checked.
//!
//! Shared by [`documented_numbers`](../documented_numbers.rs) and
//! [`corpus`](../corpus.rs): the figures live in different suites — the registry
//! counts here, the coverage split in the corpus gate — but they are quoted in
//! the same six files, so the scanning belongs in one place.
//!
//! See `tests/documented_numbers.rs` for why any of this exists.

#![allow(dead_code, reason = "each suite uses the part of this it needs")]

use std::path::{Path, PathBuf};

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/en16931 is two levels below the workspace root")
        .to_path_buf()
}

/// Every file whose prose is part of the published documentation.
///
/// The site is in here because it is the *most* public of them and the least
/// likely to be reread: a README is at least seen by anyone opening the
/// repository.
pub fn documentation() -> Vec<(String, String)> {
    let root = workspace_root();
    let mut out = Vec::new();
    let mut add = |rel: &str| {
        let p = root.join(rel);
        if let Ok(text) = std::fs::read_to_string(&p) {
            out.push((rel.to_owned(), text));
        }
    };
    add("README.md");
    for crate_dir in ["en16931", "en16931-formats", "en16931-cli"] {
        add(&format!("crates/{crate_dir}/README.md"));
        add(&format!("crates/{crate_dir}/src/lib.rs"));
        add(&format!("crates/{crate_dir}/src/main.rs"));
    }

    // The site, whatever it happens to contain — enumerated rather than listed,
    // so a page added tomorrow is covered without anyone remembering to.
    let mut stack = vec![root.join("site/content")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "md") {
                let rel = path
                    .strip_prefix(&root)
                    .unwrap_or(&path)
                    .display()
                    .to_string();
                if let Ok(text) = std::fs::read_to_string(&path) {
                    out.push((rel, text));
                }
            }
        }
    }
    assert!(
        out.len() > 10,
        "expected the READMEs and the site; found {} file(s)",
        out.len()
    );
    out
}

/// A claim the documentation makes, and the value it should be making it about.
pub struct Claim {
    /// What the number means, for the failure message.
    pub what: &'static str,
    /// The sentence to look for, with one `<N>` where the number goes.
    pub pattern: &'static str,
    /// The measured value.
    pub expected: usize,
}

/// Strip the thousands separator this project writes as a non-breaking space.
fn parse(found: &str) -> usize {
    found
        .replace(['\u{a0}', ' ', '_'], "")
        .parse()
        .expect("the capture is digits and separators")
}

/// Every maximal run of digits-and-separators, as `(number, start, end)`.
///
/// A run has to *start* at a digit, and may contain separators — `1 339`,
/// `1_339` — because that is how this project writes thousands.
pub fn digit_runs(text: &str) -> Vec<(usize, usize, usize)> {
    let b = text.as_bytes();
    let mut runs = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        if !b[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && (b[i].is_ascii_digit() || b[i] == b' ' || b[i] == b'_') {
            i += 1;
        }
        // Trailing separators belong to the sentence, not to the number.
        let mut end = i;
        while end > start && !b[end - 1].is_ascii_digit() {
            end -= 1;
        }
        runs.push((parse(&text[start..end]), start, end));
    }
    runs
}

/// A minimal matcher, because a regex crate for seven patterns is not worth a
/// dependency in a workspace whose whole argument is dependency count.
///
/// The grammar is exactly what the claims below need: literal text either side
/// of one `<N>`. Case-sensitive, because these are sentences someone wrote.
///
/// Two refinements that are not decoration:
///
/// * **A word boundary after the match.** Without it `"<N> business rules"`
///   matches *"317 business rulesets"*, and a claim that matches more than it
///   means is worse than one that matches nothing.
/// * **`EN 16931` is not a number.** The standard's name ends in four digits
///   and appears in every other sentence here, so an unanchored pattern like
///   `"<N> business rules"` finds it in *"the EN 16931 business rules"*. This is
///   the one place a project-specific exception is honest rather than a hack.
pub fn find_all(pattern: &str, text: &str) -> Vec<usize> {
    let parts: Vec<&str> = pattern.split("<N>").collect();
    assert_eq!(parts.len(), 2, "a claim has exactly one <N>: {pattern}");
    let (before, after) = (parts[0], parts[1]);

    digit_runs(text)
        .into_iter()
        .filter(|&(_, start, end)| {
            text[..start].ends_with(before)
                && text[end..].starts_with(after)
                && !text[..start].ends_with("EN ")
                && text[end + after.len()..]
                    .chars()
                    .next()
                    .is_none_or(|c| !c.is_ascii_alphanumeric())
        })
        .map(|(n, _, _)| n)
        .collect()
}

/// Opening and closing markers for prose that quotes a figure **on purpose**.
///
/// The documentation explains why this checking exists by listing the numbers
/// that were once wrong, so the files contain both current and historical
/// values of the same figure — and a scanner cannot tell them apart.
///
/// An HTML comment rather than a sentence: it is invisible in rendered
/// markdown and in rustdoc, so the reader sees the table and not the
/// bookkeeping. Marking the *region* rather than each line keeps a six-row
/// table from carrying six annotations.
pub const HISTORICAL_OPEN: &str = "<!-- doc-numbers: historical -->";
/// See [`HISTORICAL_OPEN`].
pub const HISTORICAL_CLOSE: &str = "<!-- /doc-numbers -->";

/// Blank out every marked region, preserving byte offsets so nothing else
/// shifts.
///
/// An unclosed marker blanks the rest of the file, which would silently switch
/// the checking off — so it panics instead.
fn strip_historical(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(HISTORICAL_OPEN) {
        out.push_str(&rest[..i]);
        let after = &rest[i..];
        let end = after.find(HISTORICAL_CLOSE).unwrap_or_else(|| {
            panic!("a `{HISTORICAL_OPEN}` region is never closed, which would turn off every check below it")
        }) + HISTORICAL_CLOSE.len();
        out.extend(std::iter::repeat_n(' ', end));
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// Check a set of claims against every documentation file, and fail with all
/// of the mismatches at once.
///
/// All of them, not the first: a number that moved usually moved in six places,
/// and reporting one per run turns a five-minute fix into six.
pub fn check(claims: &[Claim]) {
    let docs: Vec<(String, String)> = documentation()
        .into_iter()
        .map(|(f, t)| (f, strip_historical(&t)))
        .collect();
    let mut wrong = Vec::new();
    let mut matched = 0usize;
    for claim in claims {
        for (file, text) in &docs {
            for got in find_all(claim.pattern, text) {
                matched += 1;
                if got != claim.expected {
                    wrong.push(format!(
                        "  {file}: {} — documented as {got}, measured {}",
                        claim.what, claim.expected
                    ));
                }
            }
        }
    }
    assert!(
        matched >= claims.len(),
        "only {matched} claim(s) of {} matched anywhere, which means the prose \
         was reworded and this is now checking nothing. Update the patterns \
         rather than deleting them.",
        claims.len()
    );
    assert!(
        wrong.is_empty(),
        "{} documented number(s) no longer match the code:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
