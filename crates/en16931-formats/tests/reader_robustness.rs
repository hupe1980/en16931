//! **The readers, over documents somebody else's tooling mangled.**
//!
//! An inbound invoice is this crate's only genuinely untrusted input, and the
//! 486 published documents it is otherwise measured against are all *well-formed
//! by construction*: the authorities publish invalid **documents**, not broken
//! **files**. A truncated stream, a duplicated element, a value no producer
//! would emit — none of that is in any corpus, and all of it arrives from a real
//! counterparty.
//!
//! # Why mutation rather than random bytes
//!
//! `from_str` takes a `&str`, so arbitrary bytes cannot reach it and arbitrary
//! *text* dies at the first parse. Mutating a real document reaches the code
//! that runs **after** parsing succeeds — the field mapping, the number and date
//! conversions, the `unmapped` / `malformed` bookkeeping — which is where this
//! crate's two real reader bugs lived: an `xs:date` carrying a time zone, and
//! `<cbc:ChargeIndicator>1</cbc:ChargeIndicator>`.
//!
//! # The property
//!
//! > For **any** mutation of a valid document the reader returns — `Ok` or
//! > `Err` — and never panics. If `Ok`, the invoice it produces can be
//! > validated, serialised and re-read, and none of those panics either.
//!
//! Survival, not correctness: what a mangled document *means* is not a question
//! with an answer.

#![cfg(any(feature = "ubl", feature = "cii"))]

use proptest::prelude::*;

mod common;

/// The part of a `Read` both syntaxes share.
struct Shared {
    invoice: en16931::Invoice,
}

thread_local! {
    /// `(unmapped, malformed)` from the most recent successful read.
    ///
    /// A side channel rather than a return value because every property here
    /// cares only whether the reader *survived*; one test cares what it
    /// **noticed**, and threading an unused pair through the rest would be
    /// noise in the common case.
    static NOTED: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

/// A complete document from each syntax this build carries, as the seed.
#[expect(
    clippy::vec_init_then_push,
    reason = "the entries are feature-conditional, so `vec![]` cannot express them"
)]
fn seeds() -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    #[cfg(feature = "ubl")]
    out.push(("ubl", en16931_formats::ubl::to_string(&common::maximal())));
    #[cfg(feature = "cii")]
    out.push(("cii", en16931_formats::cii::to_string(&common::maximal())));
    out
}

/// The largest mutated document worth pressing on with.
///
/// Three stacked duplications of an outer group is eight times the seed, and the
/// cost of this suite is the read-validate-write-read cycle rather than the
/// parse. Beyond this the extra bytes are more of the same elements, which buys
/// no coverage and turned the suite into 72 seconds. Size limits belong to the
/// *caller* in the library itself (D20) — here it is only a budget.
const MAX_MUTATED_BYTES: usize = 256 * 1024;

/// Read whichever syntax the text is, and exercise everything that follows.
///
/// Returns `true` when the document was readable, so a property can report how
/// much of the corpus it actually reached rather than passing vacuously.
fn read_and_press_on(xml: &str) -> bool {
    // `sniff` runs before any parse and is the first thing a caller touches.
    // It is called on *every* input, including the oversized ones skipped below,
    // because it is the one function that sees unbounded text.
    let _ = en16931_formats::sniff(xml);
    if xml.len() > MAX_MUTATED_BYTES {
        return false;
    }

    // The two syntaxes have distinct `Read` types, so the shared part — the
    // model, and the two lists saying what did not reach it — is taken out here.
    let read: Option<(en16931::Invoice, usize, usize)> = match en16931_formats::sniff(xml) {
        #[cfg(feature = "ubl")]
        Some(en16931_formats::Syntax::Ubl) => en16931_formats::ubl::from_str(xml)
            .ok()
            .map(|r| (r.invoice, r.unmapped.len(), r.malformed.len())),
        #[cfg(feature = "cii")]
        Some(en16931_formats::Syntax::Cii) => en16931_formats::cii::from_str(xml)
            .ok()
            .map(|r| (r.invoice, r.unmapped.len(), r.malformed.len())),
        _ => None,
    };
    let Some((invoice, unmapped, malformed)) = read else {
        return false;
    };
    NOTED.with(|n| n.set((unmapped, malformed)));
    let read = Shared { invoice };

    // A reader that produced an invoice has handed it to a validator, so the
    // validator sees these documents too. XRechnung rather than all five: it
    // runs the widest rule set, and five profiles per case turned this suite
    // into two minutes for coverage the widest one already gives.
    let report = en16931::profiles::XRECHNUNG.validate(&read.invoice);
    let _ = report.to_string();

    // …and the writers see it, which is the path `en16931 convert` takes:
    // writing what was read from a mangled document, then reading it back.
    #[cfg(feature = "ubl")]
    {
        let out = en16931_formats::ubl::to_string(&read.invoice);
        let _ = en16931_formats::ubl::from_str(&out);
    }
    #[cfg(feature = "cii")]
    {
        let out = en16931_formats::cii::to_string(&read.invoice);
        let _ = en16931_formats::cii::from_str(&out);
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
    /// A stream cut short — the commonest corruption there is.
    Truncate(f64),
    /// One character gone: bad transcoding, a dropped packet, a bad diff.
    DropChar(usize),
    /// An element repeated. Schema-valid and semantically wrong, which is the
    /// combination no schema catches.
    DuplicateElement(usize),
    /// A value replaced with something the model may refuse.
    ReplaceText(usize, String),
    /// A tag renamed, so the reader meets an element it does not know.
    RenameTag(usize),
    /// Nesting, against the depth guard that exists because a stack overflow
    /// cannot be caught.
    Nest(usize),
    /// A doctype, against the entity-expansion and XXE defences.
    Doctype,
}

/// The mix is **weighted**, and the weights are the design.
///
/// `Truncate`, `DropChar` and `RenameTag` mostly leave text that is no longer
/// well-formed, so they exercise the parser's refusal path. `ReplaceText` and
/// `DuplicateElement` keep the document well-formed and so reach the field
/// mapping and the conversions, which is what this suite is for.
/// `the_mutations_actually_reach_the_reader` measures the split, because an
/// unweighted mix passes for the wrong reason.
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
/// Whitespace-only gaps are the indentation between elements and are skipped:
/// they are the overwhelming majority of `>`…`<` pairs and the least
/// interesting place to put a value.
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
    // Element spans, found without a parser: this has to work on text that is
    // no longer well-formed.
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
            // A **whole** element, open tag through matching close — because
            // duplicating only the open tag leaves it unclosed, and a document
            // that is no longer well-formed dies at the parser instead of
            // reaching the reader. Copying the element is also the realistic
            // corruption: a producer whose loop ran twice.
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
                // The first matching close. Leaf elements are the interesting
                // ones and they have no nested twin; for a group this takes the
                // innermost, which is still a well-formed duplication.
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
            // **Value** text nodes only — a gap between `>` and `<` that holds
            // something other than whitespace.
            //
            // Targeting any gap spends most cases on the indentation between
            // elements, where a replacement is well-formed and reaches no
            // conversion at all. The interesting code is the date, amount,
            // decimal, boolean and code parsing that runs on a leaf's text, and
            // this crate's two real reader bugs — an `xs:date` carrying a time
            // zone, and `<cbc:ChargeIndicator>1</cbc:ChargeIndicator>` — both
            // lived there.
            //
            // Aiming at gaps rather than values was the difference between this
            // suite catching a deliberately reintroduced slicing bug and not.
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

// The random properties are deliberately small. They cover **structural**
// corruption — a truncated stream, an element left open, a tag the reader has
// never seen — which is a different class from the value corruption that
// `every_converter_meets_every_adversarial_value` sweeps densely and far more
// cheaply. Twenty-four cases of each is enough for a class whose failures are
// not value-dependent, and it keeps this file under twenty seconds rather than
// the two minutes an undirected version cost.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// **One edit to a real document never takes the process down.**
    #[test]
    fn one_mutation_is_survivable(m in mutation(), which in 0usize..2) {
        let seeds = seeds();
        let (_, seed) = &seeds[which % seeds.len()];
        let mangled = apply(seed, &m);
        read_and_press_on(&mangled);
    }

    /// **Nor do three**, which is where a truncated element meets a renamed tag.
    ///
    /// Single edits leave a document mostly intact and mostly parseable. Stacking
    /// them is what reaches the half-read states — an element open and never
    /// closed, a value replaced inside a group that was itself duplicated.
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

/// **Every value in the document replaced with the same adversarial string.**
///
/// # Why dense rather than random
///
/// The properties above pick *one* value node per case. A complete invoice has
/// around a hundred and only a handful reach any given converter, so a random
/// case puts an adversarial value into a **date** field a few per cent of the
/// time. Replacing *every* value guarantees each converter meets each input, in
/// both syntaxes, in about thirty cases rather than thousands.
#[test]
fn every_converter_meets_every_adversarial_value() {
    // Longer than ten bytes with a character crossing the tenth, which is what
    // an index-based slice needs to fall over; plus the shapes each converter
    // has its own opinion about.
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
    // Most of these leave a well-formed document, so most must still parse —
    // otherwise the converters are not being reached.
    assert!(
        readable * 2 >= pressed,
        "only {readable} of {pressed} documents survived; the sweep is not \
         reaching the converters"
    );

    // **And the reader said so.** Its contract is that a value it cannot hold
    // is recorded in `malformed` and an element it does not know in `unmapped`
    // — "it never means the reader gave up quietly". A document whose every
    // value is `2026-13-45` must produce entries; one that produced none would
    // be a reader silently dropping fields, which is the defect nothing
    // downstream can notice.
    assert!(
        noticed * 2 >= readable,
        "only {noticed} of {readable} readable documents recorded anything in \
         `unmapped` or `malformed` — values are being dropped silently"
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
/// A fuzz suite whose every case is rejected before the interesting code runs
/// is a suite that passes for the wrong reason, and nothing in a green run says
/// which it was. This measures it.
#[test]
fn the_mutations_actually_reach_the_reader() {
    let seeds = seeds();
    let mut readable = 0usize;
    let mut total = 0usize;

    for (_, seed) in &seeds {
        for i in 0..45usize {
            // The same weighting `mutation()` uses, so this measures the mix
            // the properties actually run.
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
    // Set from the measurement rather than from hope: the weighted mix keeps
    // roughly two documents in three readable, and a drop below half means the
    // mutations have stopped reaching the code this suite exists to exercise.
    assert!(
        readable * 2 >= total,
        "only {readable} of {total} mutated documents parsed — the suite is \
         measuring the parser, not the reader. Re-weight `mutation()`."
    );
}

/// A doctype is refused whatever else was done to the document.
///
/// Entity expansion and XXE are both "the parser must not process a DTD", and
/// the refusal has to survive the document being mangled around it.
#[test]
fn a_doctype_is_always_refused() {
    for (name, seed) in seeds() {
        let with_dtd = apply(&seed, &Mutation::Doctype);
        let read = match name {
            #[cfg(feature = "ubl")]
            "ubl" => en16931_formats::ubl::from_str(&with_dtd).is_ok(),
            #[cfg(feature = "cii")]
            "cii" => en16931_formats::cii::from_str(&with_dtd).is_ok(),
            _ => false,
        };
        assert!(!read, "{name}: a document carrying a DTD must be refused");
    }
}
