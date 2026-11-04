//! The readers over mutated real documents: for any mutation the reader
//! returns `Ok` or `Err` without panicking, and an `Ok` invoice can be
//! validated, rendered, written and re-read without panicking either.
//!
//! Mutating a valid document, rather than generating random bytes, reaches the
//! code after parsing: field mapping, conversions, `Unmapped` / `Malformed` entries.
//!
//! This suite proves survival and reach. That the readers *say* what they
//! refuse, element by element, is `reader_absence`'s sweep.

#![cfg(any(feature = "ubl", feature = "cii"))]

use en16931_formats::report::Kind;
use proptest::prelude::*;

mod common;

thread_local! {
    /// `(unmapped, malformed)` from the most recent successful read.
    static NOTED: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

/// The syntaxes this build carries.
#[expect(
    clippy::vec_init_then_push,
    reason = "the entries are feature-conditional, so `vec![]` cannot express them"
)]
fn syntaxes() -> Vec<en16931_formats::Syntax> {
    let mut out = Vec::new();
    #[cfg(feature = "ubl")]
    out.push(en16931_formats::Syntax::Ubl);
    #[cfg(feature = "cii")]
    out.push(en16931_formats::Syntax::Cii);
    out
}

/// A complete document from each syntax this build carries, as the seed.
fn seeds() -> Vec<(en16931_formats::Syntax, String)> {
    syntaxes()
        .into_iter()
        .map(|s| {
            let written = en16931_formats::write_unchecked(&common::maximal(), s)
                .expect("the maximal fixture carries everything the schemas require");
            (s, written.xml)
        })
        .collect()
}

/// The largest mutated document worth pressing on with.
/// The largest mutated document worth pressing on with: a time budget for the
/// read-validate-write-read cycle, not a library limit.
const MAX_MUTATED_BYTES: usize = 256 * 1024;

/// Read whichever syntax the text is, and exercise everything that follows.
///
/// Returns `true` when the document was readable, so properties can report reach.
fn read_and_press_on(xml: &str) -> bool {
    if xml.len() > MAX_MUTATED_BYTES {
        return false;
    }

    // `read` detects the syntax itself, so every mutation goes through the one
    // entry point a caller has.
    let Ok(read) = en16931_formats::read(xml.as_bytes()) else {
        return false;
    };
    let unmapped = read.entries_of(Kind::Unmapped).count();
    let malformed = read.entries_of(Kind::Malformed).count();
    NOTED.with(|n| n.set((unmapped, malformed)));

    // Validated through `receive` under XRechnung (the widest rule set), and
    // rendered as text and SVRL of every layer, which must stay well-formed.
    let policy = en16931_formats::Policy {
        require: Some(&en16931::profiles::XRECHNUNG),
        ..en16931_formats::Policy::default()
    };
    let received = en16931_formats::receive(xml.as_bytes(), &policy);
    let _ = received.to_string();
    let interchange = received.to_interchange();
    for report in std::iter::once(&interchange).chain(interchange.required.as_deref()) {
        let svrl = en16931::svrl::to_svrl(report);
        if let Err(e) = roxmltree::Document::parse(&svrl) {
            panic!("SVRL of a received document is not well-formed: {e}\n{svrl}");
        }
    }

    // …and written and read back, as `en16931 convert` does; a refusal to
    // write is survival too.
    for syntax in syntaxes() {
        if let Ok(out) = en16931_formats::write_unchecked(&read.invoice, syntax) {
            let _ = en16931_formats::read(out.xml.as_bytes());
        }
    }
    true
}

/// Text a producer might put in an element, including what an attacker would.
fn adversarial_text() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        Just(" ".to_owned()),
        Just("-0".to_owned()),
        Just("1e309".to_owned()), // overflows every float
        Just("99999999999999999999999999999".to_owned()),
        Just("0.000000000000000000000000001".to_owned()),
        Just("2026-13-45".to_owned()),       // a date that is not one
        Just("2026-07-31+02:00".to_owned()), // the zone that cost BT-2 once
        Just("１".to_owned()),               // a full-width digit
        Just("Müller & Söhne".to_owned()),
        Just("\u{1F9FE}".to_owned()),
        Just("]]>".to_owned()), // ends a CDATA section
        Just("<!--".to_owned()),
        ".{0,40}",
    ]
}

/// One edit, of the kind a real pipeline actually inflicts.
#[derive(Debug, Clone)]
enum Mutation {
    /// A stream cut short.
    Truncate(f64),
    /// One character gone.
    DropChar(usize),
    /// An element repeated: schema-valid and semantically wrong.
    DuplicateElement(usize),
    /// A value replaced with something the model may refuse.
    ReplaceText(usize, String),
    /// A tag renamed, so the reader meets an element it does not know.
    RenameTag(usize),
    /// Nesting, against the depth guard.
    Nest(usize),
    /// A doctype, against the entity-expansion and XXE defences.
    Doctype,
}

/// The weighted mutation mix.
///
/// `Truncate`, `DropChar` and `RenameTag` mostly exercise the parser's refusal
/// path; `ReplaceText` and `DuplicateElement` keep the document well-formed and
/// reach the mapping. `the_mutations_actually_reach_the_reader` measures the split.
fn mutation() -> impl Strategy<Value = Mutation> {
    prop_oneof![
        6 => (any::<usize>(), adversarial_text()).prop_map(|(i, t)| Mutation::ReplaceText(i, t)),
        4 => any::<usize>().prop_map(Mutation::DuplicateElement),
        1 => (0.0f64..1.0).prop_map(Mutation::Truncate),
        1 => any::<usize>().prop_map(Mutation::DropChar),
        1 => any::<usize>().prop_map(Mutation::RenameTag),
        1 => (0usize..400).prop_map(Mutation::Nest),
        1 => Just(Mutation::Doctype),
    ]
}

/// Every leaf text node that actually carries a value, as `(start, end)`.
///
///
/// Whitespace-only gaps (indentation) are skipped.
fn value_spans(xml: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some(gt) = xml[at..].find('>') {
        let start = at + gt + 1;
        let Some(lt) = xml[start..].find('<') else {
            break;
        };
        let end = start + lt;
        if start < end && !xml[start..end].trim().is_empty() {
            out.push((start, end));
        }
        at = end;
    }
    out
}

/// Apply one edit to `xml`, keeping the result valid UTF-8.
fn apply(xml: &str, m: &Mutation) -> String {
    // Element spans, found without a parser: the text may not be well-formed.
    let opens: Vec<usize> = xml.match_indices('<').map(|(i, _)| i).collect();

    match m {
        Mutation::Truncate(f) => {
            let target = (xml.len() as f64 * f) as usize;
            let mut cut = target.min(xml.len());
            while cut > 0 && !xml.is_char_boundary(cut) {
                cut -= 1;
            }
            xml[..cut].to_owned()
        }
        Mutation::DropChar(i) => {
            let chars: Vec<char> = xml.chars().collect();
            if chars.is_empty() {
                return xml.to_owned();
            }
            let at = i % chars.len();
            chars
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != at)
                .map(|(_, c)| *c)
                .collect()
        }
        Mutation::DuplicateElement(i) => {
            // A whole element, open tag through matching close, so the result
            // stays well-formed and reaches the reader.
            let Some(&start) = opens.get(i % opens.len().max(1)) else {
                return xml.to_owned();
            };
            let Some(open_end) = xml[start..].find('>').map(|e| start + e + 1) else {
                return xml.to_owned();
            };
            let open = &xml[start..open_end];
            // Self-closing elements are already complete.
            let end = if open.ends_with("/>") {
                open_end
            } else {
                let name: String = open[1..]
                    .chars()
                    .take_while(|c| !c.is_whitespace() && *c != '>' && *c != '/')
                    .collect();
                if name.is_empty() || name.starts_with('?') || name.starts_with('!') {
                    return xml.to_owned();
                }
                // The first matching close: for a group, the innermost, still
                // a well-formed duplication.
                match xml[open_end..].find(&format!("</{name}>")) {
                    Some(at) => open_end + at + name.len() + 3,
                    None => return xml.to_owned(),
                }
            };
            let mut out = String::with_capacity(xml.len() + (end - start));
            out.push_str(&xml[..end]);
            out.push_str(&xml[start..end]);
            out.push_str(&xml[end..]);
            out
        }
        Mutation::ReplaceText(i, text) => {
            // Value text nodes only (non-whitespace between `>` and `<`), where
            // the date, amount, decimal, boolean and code parsing runs.
            let values: Vec<(usize, usize)> = value_spans(xml);
            if values.is_empty() {
                return xml.to_owned();
            }
            let (start, end) = values[i % values.len()];
            let escaped = text.replace('&', "&amp;").replace('<', "&lt;");
            format!("{}{escaped}{}", &xml[..start], &xml[end..])
        }
        Mutation::RenameTag(i) => {
            let Some(&start) = opens.get(i % opens.len().max(1)) else {
                return xml.to_owned();
            };
            let Some(end) = xml[start..].find('>').map(|e| start + e + 1) else {
                return xml.to_owned();
            };
            format!("{}<zz:Unknown/>{}", &xml[..start], &xml[end..])
        }
        Mutation::Nest(depth) => {
            let inner: String = "<a>".repeat(*depth);
            let close: String = "</a>".repeat(*depth);
            // Inside the document element, so the guard is what has to refuse it.
            match xml.find('>') {
                Some(at) => format!("{}{inner}{close}{}", &xml[..=at], &xml[at + 1..]),
                None => xml.to_owned(),
            }
        }
        Mutation::Doctype => {
            let dtd = "<!DOCTYPE Invoice [<!ENTITY xxe SYSTEM \"file:///etc/passwd\">]>";
            match xml.find("?>") {
                Some(at) => format!("{}{dtd}{}", &xml[..at + 2], &xml[at + 2..]),
                None => format!("{dtd}{xml}"),
            }
        }
    }
}

// The random properties cover structural corruption with few cases; value
// corruption is swept densely by `every_converter_meets_every_adversarial_value`.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// One edit to a real document never takes the process down.
    #[test]
    fn one_mutation_is_survivable(m in mutation(), which in 0usize..2) {
        let seeds = seeds();
        let (_, seed) = &seeds[which % seeds.len()];
        let mangled = apply(seed, &m);
        read_and_press_on(&mangled);
    }

    /// Nor do three stacked edits, which reach half-read states.
    #[test]
    fn three_mutations_are_survivable(
        a in mutation(), b in mutation(), c in mutation(), which in 0usize..2
    ) {
        let seeds = seeds();
        let (_, seed) = &seeds[which % seeds.len()];
        let mangled = apply(&apply(&apply(seed, &a), &b), &c);
        read_and_press_on(&mangled);
    }
}

/// Every value in the document replaced with the same adversarial string, so
/// each converter meets each input in both syntaxes.
#[test]
fn every_converter_meets_every_adversarial_value() {
    // Longer than ten bytes with a character crossing the tenth (breaks
    // index-based slicing), plus shapes each converter treats specially.
    const VALUES: [&str; 16] = [
        "",
        " ",
        "\u{9}\u{a}",
        "0",
        "-0",
        "1e309",
        "99999999999999999999999999999",
        "0.000000000000000000000000001",
        "2026-13-45",
        "2026-07-31+02:00",
        "123456789\u{e9}xyz",                         // é crosses byte 9..11
        "abcdefghi\u{1F9FE}jkl",                      // an astral character crossing byte 9..13
        "\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}", // combining marks only
        "Stra\u{df}e 1, M\u{fc}nchen",
        "true",
        "1",
    ];

    common::assert_page_states(
        "conformance.md",
        &[("each of <N> adversarial strings", VALUES.len())],
    );

    let mut pressed = 0usize;
    let mut readable = 0usize;
    let mut noticed = 0usize;
    for (_, seed) in seeds() {
        for value in VALUES {
            let mangled = replace_every_value(&seed, value);
            if read_and_press_on(&mangled) {
                readable += 1;
                let (unmapped, malformed) = NOTED.with(std::cell::Cell::get);
                if unmapped + malformed > 0 {
                    noticed += 1;
                }
            }
            pressed += 1;
        }
    }

    println!(
        "adversarial values\n  documents pressed: {pressed}\n  still readable:    {readable}\n  \
         documents where the reader recorded a refusal: {noticed}"
    );
    // Most leave a well-formed document, so most must still parse.
    assert!(
        readable * 2 >= pressed,
        "only {readable} of {pressed} documents survived; the sweep is not \
         reaching the converters"
    );

    // A reach measure, not the reporting check: with every value replaced at
    // once a document records many entries, and one value dropped in silence
    // hides among them. `reader_absence`'s per-element sweep is the check that
    // the readers record what they refuse.
    assert!(
        noticed * 2 >= readable,
        "only {noticed} of {readable} readable documents recorded an \
         `Unmapped` or `Malformed` entry — the sweep no longer reaches the converters"
    );
}

/// Every leaf value replaced with `value`, in one pass.
fn replace_every_value(xml: &str, value: &str) -> String {
    let escaped = value.replace('&', "&amp;").replace('<', "&lt;");
    let spans = value_spans(xml);
    let mut out = String::with_capacity(xml.len());
    let mut at = 0usize;
    for (start, end) in spans {
        out.push_str(&xml[at..start]);
        out.push_str(&escaped);
        at = end;
    }
    out.push_str(&xml[at..]);
    out
}

/// The mutations reach the reader rather than dying at the parser.
///
/// A suite whose every case dies at the parser passes for the wrong reason.
#[test]
fn the_mutations_actually_reach_the_reader() {
    let seeds = seeds();
    let mut readable = 0usize;
    let mut total = 0usize;

    for (_, seed) in &seeds {
        for i in 0..45usize {
            // The same weighting `mutation()` uses.
            let m = match i % 15 {
                0..=5 => Mutation::ReplaceText(i * 5, "2026-07-31+02:00".to_owned()),
                6..=9 => Mutation::DuplicateElement(i * 3),
                10 => Mutation::Truncate((i as f64 % 97.0) / 97.0),
                11 => Mutation::DropChar(i * 7),
                12 => Mutation::RenameTag(i * 11),
                13 => Mutation::Nest(i % 90),
                _ => Mutation::Doctype,
            };
            if read_and_press_on(&apply(seed, &m)) {
                readable += 1;
            }
            total += 1;
        }
    }

    println!("reader robustness\n  mutations applied: {total}\n  still readable:    {readable}");
    // The weighted mix keeps roughly two documents in three readable; below
    // half, the mutations no longer reach the mapping.
    assert!(
        readable * 2 >= total,
        "only {readable} of {total} mutated documents parsed — the suite is \
         measuring the parser, not the reader. Re-weight `mutation()`."
    );
}

/// A doctype is refused whatever else was done to the document.
///
/// Entity expansion and XXE both require DTD processing.
#[test]
fn a_doctype_is_always_refused() {
    for (syntax, seed) in seeds() {
        let with_dtd = apply(&seed, &Mutation::Doctype);
        let read = en16931_formats::read(with_dtd.as_bytes());
        assert!(
            matches!(read, Err(en16931_formats::Error::Xml { .. })),
            "{syntax}: a document carrying a DTD must be refused, got {read:?}"
        );
    }
}
