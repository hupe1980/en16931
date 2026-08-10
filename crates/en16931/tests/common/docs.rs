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
///
/// `CHANGELOG.md` is deliberately **not** here. Its job is to record what was
/// true at each release, so the current value of a figure and a superseded one
/// sit in the same file by design, and a scanner cannot tell them apart. That is
/// exactly what [`HISTORICAL_OPEN`] exists for in the other files — but there it
/// marks the exception, and here it would have to mark almost everything.
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
    }

    // **Every** source file, not only the two crate roots. This project puts as
    // much explanatory prose in module headers as in its READMEs — the twenty-one
    // `BR-DEC-*` in `amount.rs`, the tolerance table in `validation/rules/`, the
    // withdrawn-scheme table in `codes/guard.rs` — and rustdoc publishes all of
    // it. Scanning `lib.rs` alone meant the most detailed prose in the workspace
    // was the least checked.
    //
    // Scanning code as well as comments is harmless: a claim is literal prose
    // either side of one number, so an array of code values matches nothing.
    let mut stack = vec![root.join("site/content")];
    for crate_dir in ["en16931", "en16931-formats", "en16931-cli"] {
        stack.push(root.join(format!("crates/{crate_dir}/src")));
    }
    stack.push(root.join("xtask/src"));
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "md" || e == "rs") {
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

/// The characters this project uses between the thousands of a figure.
///
/// The two invisible ones are not decoration. A number typed with a
/// non-breaking space looks identical to one typed with a plain space, and if
/// the scanner split on it the figure would simply stop being checked — one
/// character nobody can see switching off a test nobody would miss.
const SEPARATORS: [char; 4] = [' ', '_', '\u{a0}', '\u{202f}'];

/// Strip the separators and read the figure.
///
/// `None` for a run that cannot be a count. Since this scans source as well as
/// prose it meets things that are digits without being numbers — the IBAN
/// `DE89370400440532013000` in `rules/xrechnung.rs` is twenty-two of them — and a
/// documentation scanner that panics on a test fixture is a scanner nobody can
/// point at the whole tree.
fn parse(found: &str) -> Option<usize> {
    found.replace(SEPARATORS, "").parse().ok()
}

/// Every maximal run of digits-and-separators, as `(number, start, end)`.
///
/// A run has to *start* at a digit, and may contain [`SEPARATORS`] — `1 339`,
/// `1_339` — because that is how this project writes thousands. `end` is always
/// just past the last **digit**, so a trailing separator belongs to the sentence
/// rather than to the number. Runs too large for a `usize` are skipped: see
/// [`parse`].
///
/// Over `char`s rather than bytes, because two of the separators are multi-byte.
/// A byte-wise version accepted only ASCII space and underscore, which made
/// `parse`'s handling of `\u{a0}` unreachable and its doc comment — and a test
/// asserting the behaviour — describe something that never happened.
pub fn digit_runs(text: &str) -> Vec<(usize, usize, usize)> {
    let mut runs = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        if !c.is_ascii_digit() {
            continue;
        }
        let mut end = start + c.len_utf8();
        while let Some(&(i, c)) = chars.peek() {
            if c.is_ascii_digit() {
                end = i + c.len_utf8();
            } else if !SEPARATORS.contains(&c) {
                break;
            }
            chars.next();
        }
        if let Some(n) = parse(&text[start..end]) {
            runs.push((n, start, end));
        }
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
///
/// The boundary applies only when the pattern's own tail could run into a word —
/// that is, when it ends in an alphanumeric. A pattern ending in punctuation or
/// a space has already established the boundary itself, and demanding a second
/// one makes it match nothing: `"Code lists — <N> "` was silently dead for
/// exactly that reason, because the character after its trailing space is the
/// `v` of *values*.
pub fn find_all(pattern: &str, text: &str) -> Vec<usize> {
    let parts: Vec<&str> = pattern.split("<N>").collect();
    assert_eq!(parts.len(), 2, "a claim has exactly one <N>: {pattern}");
    let (before, after) = (parts[0], parts[1]);
    let needs_boundary = after
        .chars()
        .last()
        .is_none_or(|c| c.is_ascii_alphanumeric());

    digit_runs(text)
        .into_iter()
        .filter(|&(_, start, end)| {
            text[..start].ends_with(before)
                && text[end..].starts_with(after)
                && !text[..start].ends_with("EN ")
                && (!needs_boundary
                    || text[end + after.len()..]
                        .chars()
                        .next()
                        .is_none_or(|c| !c.is_ascii_alphanumeric()))
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
    let mut never_matched = Vec::new();
    for claim in claims {
        let mut matched = 0usize;
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
        // **Per claim**, not in aggregate. The total used to be compared against
        // `claims.len()`, which one popular pattern matching a dozen times
        // satisfies on its own — so a claim whose sentence had been reworded
        // stopped checking anything and the suite stayed green. Every pattern
        // has to find its own sentence.
        if matched == 0 {
            never_matched.push(format!(
                "  {:?} — for {}; measured {}",
                claim.pattern, claim.what, claim.expected
            ));
        }
    }
    assert!(
        never_matched.is_empty(),
        "{} claim pattern(s) matched nothing anywhere, which means the prose was \
         reworded and they are now checking nothing. Update the pattern rather \
         than deleting it:\n{}",
        never_matched.len(),
        never_matched.join("\n")
    );
    assert!(
        wrong.is_empty(),
        "{} documented number(s) no longer match the code:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
