//! Numbers the documentation quotes, checked against the code that produces them.
//!
//! # Why this exists
//!
//! Every figure in this project is measured — and then *written down*, in as
//! many as six places: two crate READMEs, the CLI's, two `lib.rs` headers and
//! the documentation site. Measuring something once and copying it six times is
//! how a project ends up asserting, in prose, things that stopped being true.
//!
//! It is not hypothetical. Every one of these had already drifted:
//!
//! | | was documented | measured |
//! |---|---|---|
//! | profile check counts | 226 / 281 / 289 / 295 / 272 | one higher, all five |
//! | rules retired by the types | 36 | 53 |
//! | rules registered | 149 | 317 |
//! | declared divergences | 13 | 11 |
//! | this crate's own rules | three | four |
//! | published corpus documents | 490 | 486 |
//!
//! Six of those came from one root cause each, and none of them from
//! carelessness: a rule was added, a pin moved, a category was renamed. The
//! copies are the problem, not the people.
//!
//! So the prose stays — it is what makes the numbers mean something — and the
//! numbers in it are read back and compared. `just deps` does the same for the
//! dependency-graph sizes, and `tests/artefact_pin.rs` for the artefact
//! revision; this covers what is countable from the library itself.
//!
//! # What is deliberately *not* checked here
//!
//! Anything needing `spec/`. The corpus sizes, the CEN assertion totals and the
//! mutation counts are measured by the suites that read those artefacts, and
//! they skip when it is absent. Duplicating them here would mean this test also
//! skipped, and a test that quietly does nothing is worse than none —
//! `crates/en16931-formats/tests/subset.rs` asserts the prohibition total,
//! `tests/codelists.rs` the syntax-rule total, `tests/conformance.rs` the
//! agreement figures.
//!
//! # Quoting a figure on purpose
//!
//! The table above is exactly the problem this test looks for — old values of
//! current figures, written down deliberately. Wrap such a passage in
//! [`HISTORICAL_OPEN`] / [`HISTORICAL_CLOSE`]; the markers are HTML comments, so
//! they are invisible in rendered markdown and in rustdoc, and they mark a
//! region rather than a line so a six-row table needs two of them rather than
//! six annotations.
//!
//! [`HISTORICAL_OPEN`]: common::docs::HISTORICAL_OPEN
//! [`HISTORICAL_CLOSE`]: common::docs::HISTORICAL_CLOSE
//!
//! # Scanning rather than listing
//!
//! Each claim is a pattern with one number slot, applied to every
//! documentation file. That way a *new* mention of a figure is checked the day
//! it is written, which a list of known locations cannot manage — and adding a
//! seventh copy of a number stops being a risk worth avoiding.

use std::collections::BTreeSet;

mod common;
use common::docs::{Claim, check, find_all};

#[test]
fn every_number_the_documentation_quotes_is_the_one_the_code_produces() {
    // ── measure ──────────────────────────────────────────────────────────────
    // `check_ids` includes a profile's *restrictions*, which carry real
    // published ids (`BR-DE-3`) and are counted in the CHECKS column — but they
    // are §7.3.2 narrowings, not rules, and the figure the prose calls "business
    // rules" is the registry. Counting the union of `check_ids` gives 329, and
    // the twelve extra are exactly the restrictions.
    let registered: BTreeSet<&str> = en16931::validation::rules::CORE
        .iter()
        .map(|r| r.id.as_str())
        .chain(
            en16931::profiles::ALL
                .iter()
                .flat_map(|p| p.extra_rules.iter().map(|r| r.id.as_str())),
        )
        .collect();
    let crate_own = registered.iter().filter(|id| id.starts_with("EN-")).count();
    let tables = en16931::codes::generated::TABLES;
    let code_values: usize = tables.iter().map(|(_, codes)| codes.len()).sum();

    let claims = [
        Claim {
            what: "rules registered across every shipped profile",
            pattern: "<N> business rules",
            expected: registered.len(),
        },
        Claim {
            what: "rules registered across every shipped profile",
            pattern: "**<N>** rules registered",
            expected: registered.len(),
        },
        Claim {
            what: "rules registered across every shipped profile",
            pattern: "<N> rules registered",
            expected: registered.len(),
        },
        Claim {
            what: "this crate's own `EN-*` rules",
            pattern: "<N> of those 317 are this crate's own",
            expected: crate_own,
        },
        Claim {
            what: "generated code values",
            pattern: "<N> values across",
            expected: code_values,
        },
        Claim {
            what: "generated code lists",
            pattern: "values across <N> tables",
            expected: tables.len(),
        },
        Claim {
            what: "generated code lists",
            pattern: "Code lists — <N> ",
            expected: code_values,
        },
    ];

    check(&claims);
}

/// The matcher itself, because a silently-never-matching pattern would make the
/// test above pass by doing nothing.
#[test]
fn the_matcher_finds_what_it_should_and_nothing_else() {
    assert_eq!(find_all("<N> business rules", "317 business rules"), [317]);
    assert_eq!(
        find_all("<N> business rules", "the 1 339 business rules here"),
        [1339],
        "a non-breaking-space thousands separator is part of the number"
    );
    assert_eq!(
        find_all("<N> business rules", "317 business rulesets"),
        Vec::<usize>::new(),
        "a word boundary after the match, or `rulesets` counts as `rules`"
    );
    assert_eq!(
        find_all("<N> business rules", "the EN 16931 business rules"),
        Vec::<usize>::new(),
        "the standard's name ends in four digits and is not a count"
    );
    assert_eq!(
        find_all("<N> business rules", "EN 16931 has 317 business rules"),
        [317],
        "…but a real count in the same sentence still matches"
    );
    assert_eq!(
        find_all("<N> tables", "no number here"),
        Vec::<usize>::new()
    );
    assert_eq!(
        find_all("a <N> b", "a 1 b and a 2 b"),
        [1, 2],
        "every occurrence, not just the first"
    );
}
