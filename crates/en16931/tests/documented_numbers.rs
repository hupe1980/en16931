//! Numbers the documentation quotes, checked against the code that produces them.
//!
//! # Why this exists
//!
//! Every figure in this project is measured — and then *written down*, in as
//! many as six places: two crate READMEs, the CLI's, two `lib.rs` headers and
//! the documentation site. Measuring something once and copying it six times is
//! how a project ends up asserting, in prose, things that stopped being true.
//!
//! The prose stays — it is what makes the numbers mean something — and the
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
//! # Scanning rather than listing
//!
//! Each claim is a pattern with one number slot, applied to every
//! documentation file. That way a *new* mention of a figure is checked the day
//! it is written, which a list of known locations cannot manage — and adding a
//! seventh copy of a number stops being a risk worth avoiding.

use std::collections::BTreeSet;

mod common;
use common::docs::{Claim, check, documentation, find_all};

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
    let checks = |p: &en16931::Profile| p.check_ids().count();

    let claims = [
        // The five profile check counts, as the comparison tables in five files
        // print them. `tests/profiles.rs` measures the same figures against the
        // registry; this measures the *prose* against them, which is the half
        // that drifts.
        Claim {
            what: "checks the core profile runs",
            pattern: "| EN 16931 core | <N> |",
            expected: checks(&en16931::profiles::EN16931),
        },
        Claim {
            what: "checks XRechnung 3.0 runs",
            pattern: "| XRechnung 3.0 | <N> |",
            expected: checks(&en16931::profiles::XRECHNUNG),
        },
        Claim {
            what: "checks XRechnung 3.0 CVD runs",
            pattern: "| XRechnung 3.0 CVD | <N> |",
            expected: checks(&en16931::profiles::XRECHNUNG_CVD),
        },
        Claim {
            what: "checks XRechnung 3.0 Extension runs",
            pattern: "| XRechnung 3.0 Extension | <N> |",
            expected: checks(&en16931::profiles::XRECHNUNG_EXTENSION),
        },
        Claim {
            what: "checks Peppol BIS Billing 3.0 runs",
            pattern: "| Peppol BIS Billing 3.0 | <N> |",
            expected: checks(&en16931::profiles::PEPPOL_BIS_3),
        },
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
            pattern: "**<N>** of those are this crate's own",
            expected: crate_own,
        },
        Claim {
            what: "this crate's own `EN-*` rules",
            pattern: "| `EN-*` | <N> | this crate's own",
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
        Claim {
            what: "generated code values",
            pattern: "Eighteen lists, **<N> values**",
            expected: code_values,
        },
        // Two figures that live in module headers rather than in a README, and
        // are load-bearing there. The first is the crate's central thesis stated
        // as a number — a *type* retires this many rules — and the second is
        // quoted in a hint the validator prints to users.
        Claim {
            what: "`BR-DEC-*` assertions the two-decimal type retires",
            pattern: "as <N> separate assertions",
            expected: en16931::validation::rules::CORE
                .iter()
                .filter(|r| r.id.as_str().starts_with("BR-DEC-"))
                .count(),
        },
        // A pattern has to fit on one line. Prose in a `//` comment is wrapped,
        // and the wrap inserts a newline and a comment marker in the middle of
        // the sentence — so this one starts at `list of`, not at `a list of`.
        Claim {
            what: "UN/ECE Rec 20/21 unit codes",
            pattern: "list of <N> values",
            expected: en16931::codes::generated::UNIT_CODES.len(),
        },
    ];

    check(&claims);
}

/// The family table is a **partition** of the registry, and is checked as one.
///
/// Nine rows of "how many rules start with this prefix" is nine numbers that can
/// each be wrong on their own, and one of them is easy to get wrong by hand:
/// `PEPPOL-EN16931-R120` exists **twice**, once fatal for Peppol and once
/// rewritten to a warning for XRechnung, so counting instances rather than
/// distinct ids makes the nine rows sum to 318 against a registry of 317.
///
/// So this does not check nine numbers. It classifies every registered rule into
/// exactly one bucket, asserts the buckets sum to the registry, and *then*
/// compares each against the prose. A row that drifts fails; a row that is
/// quietly dropped fails; and a rule family nobody thought to document fails,
/// because it would land in a bucket with no claim behind it.
#[test]
fn the_family_table_partitions_the_registry() {
    /// Longest prefix first, so `BR-DEC-` and `BR-DEX-` are not swallowed by
    /// `BR-DE-`, which is a real hazard rather than a hypothetical one.
    const FAMILIES: &[(&str, &str)] = &[
        ("PEPPOL-EN16931-", "| `PEPPOL-EN16931-*` | <N> |"),
        ("BR-DEC-", "| `BR-DEC-*` | <N> |"),
        ("BR-DEX-", "| `BR-DEX-*` | <N> |"),
        ("BR-DE-", "| `BR-DE-*` | <N> |"),
        ("BR-CO-", "| `BR-CO-*` | <N> |"),
        ("BR-CL-", "| `BR-CL-*` | <N> |"),
        ("BR-TMP-", "| `BR-TMP-*` | <N> |"),
        ("EN-", "| `EN-*` | <N> |"),
    ];
    /// Everything left over: `BR-01`…`BR-65` and the nine VAT category families.
    const REST: &str = "| `BR-*` and the nine VAT families | <N> |";

    let registered: BTreeSet<&str> = en16931::validation::rules::CORE
        .iter()
        .map(|r| r.id.as_str())
        .chain(
            en16931::profiles::ALL
                .iter()
                .flat_map(|p| p.extra_rules.iter().map(|r| r.id.as_str())),
        )
        .collect();

    let mut counts = vec![0usize; FAMILIES.len()];
    let mut rest = 0usize;
    for id in &registered {
        match FAMILIES.iter().position(|(p, _)| id.starts_with(p)) {
            Some(i) => counts[i] += 1,
            None => {
                assert!(
                    id.starts_with("BR-"),
                    "{id} belongs to no documented family — add a row for it"
                );
                rest += 1;
            }
        }
    }
    assert_eq!(
        counts.iter().sum::<usize>() + rest,
        registered.len(),
        "the buckets are not a partition"
    );

    let mut claims: Vec<Claim> = FAMILIES
        .iter()
        .zip(&counts)
        .map(|((prefix, pattern), n)| Claim {
            what: prefix,
            pattern,
            expected: *n,
        })
        .collect();
    claims.push(Claim {
        what: "BR-* and the nine VAT families",
        pattern: REST,
        expected: rest,
    });
    check(&claims);
}

/// Sample **output** in the documentation is produced, not transcribed.
///
/// A claim pattern checks one number in a sentence. A pasted terminal block is a
/// whole line of them — a rule count, a finding count, a verdict — and it is
/// exactly the kind of thing that is copied once and never rerun. The
/// deviations block in `crates/en16931/README.md` had drifted two of its three:
/// it showed `280 rule(s) checked` beside prose saying the count drops to 281,
/// and neither was what the code printed.
///
/// So the line is regenerated here and the file is asked whether it contains it.
#[test]
fn the_pasted_sample_reports_are_the_ones_the_code_prints() {
    use en16931::validation::Check;
    use en16931::{Invoice, profiles};

    let samples = [Check::new(&profiles::XRECHNUNG)
        .without("BR-DE-15")
        .run(&Invoice::default())];

    for report in samples {
        // The header line only: the finding list below it is elided in the
        // documentation on purpose, and the header carries every figure.
        let header = report
            .to_string()
            .lines()
            .next()
            .expect("a report always has a header")
            .to_owned();
        assert!(
            documentation()
                .iter()
                .any(|(_, text)| text.contains(&header)),
            "no documentation file contains the line this run prints:\n  {header}\n\
             A sample report was pasted and has since gone stale."
        );
    }
}

/// The matcher itself, because a silently-never-matching pattern would make the
/// test above pass by doing nothing.
#[test]
fn the_matcher_finds_what_it_should_and_nothing_else() {
    assert_eq!(find_all("<N> business rules", "317 business rules"), [317]);
    assert_eq!(
        find_all("<N> business rules", "the 1 339 business rules here"),
        [1339],
        "a thousands separator is part of the number"
    );
    assert_eq!(
        find_all("<N> business rules", "the 1\u{a0}339 business rules here"),
        [1339],
        "…including an invisible one. This comment claimed a non-breaking space \
         was handled while the scanner split on it, so a figure typed with one \
         silently stopped being checked."
    );
    assert_eq!(
        find_all("<N> business rules", "the 1\u{202f}339 business rules here"),
        [1339],
        "…and the narrow one editors like to insert"
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
        find_all(
            "Code lists — <N> ",
            "## Code lists — 4 887 values, generated"
        ),
        [4887],
        "a pattern ending in a space has already made its own word boundary; \
         demanding a second one made this one match nothing at all"
    );
    assert_eq!(
        find_all("about <N>%", "about 91% of them"),
        [91],
        "…and so has one ending in punctuation"
    );
    assert_eq!(
        find_all("a <N> b", "a 1 b and a 2 b"),
        [1, 2],
        "every occurrence, not just the first"
    );
}
