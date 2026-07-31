//! The CEN attribution notice is a **licence condition**, so it is tested.
//!
//! EN 16931-1 and CEN/TS 16931-2 are free of charge under the 2018 agreement
//! between CEN and the European Commission, which permits derivative use *on
//! condition* that the derivative carries a statement, visible to users, that it
//! is an implementation of the semantic data model.
//!
//! the design notes spells out where it has to appear: the crate
//! documentation, `README.md`, and every validation report. All three were
//! there and **none of them was checked**, which is a strange thing to leave to
//! chance — every other invariant in this crate has a test, and this is the one
//! whose loss forfeits the right to ship at all.
//!
//! Reformatting the `Display` impl, or trimming the README, would have silently
//! dropped it. Nothing else in the build would have noticed.

use en16931::{ATTRIBUTION, Invoice, validate};

/// Collapse runs of whitespace, so a markdown line wrap is not a licence breach.
///
/// The condition is that the *statement* appears where a user sees it, not that
/// it occupies one physical line. Prose files wrap; comparing raw bytes would
/// make this test fail on reflow and tempt the next person to delete it.
fn flat(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The three places §13.2 requires, checked against one source of truth.
#[test]
fn attribution_is_carried_everywhere_it_is_required() {
    // 1. Every validation report a user ever sees.
    let rendered = validate(&Invoice::default()).to_string();
    assert!(
        flat(&rendered).contains(&flat(ATTRIBUTION)),
        "a validation report must carry the notice:\n{rendered}"
    );

    // 2. `README.md` — which is also the crate's front page on crates.io and
    //    docs.rs, since `lib.rs` includes it verbatim.
    let readme = include_str!("../README.md");
    assert!(
        flat(readme).contains(&flat(ATTRIBUTION)),
        "README.md must carry the notice verbatim, not a paraphrase"
    );

    // 3. The crate documentation. `lib.rs` includes the README, so (2) covers
    //    the rendered docs — but the dedicated section must survive too, since
    //    that is what a reader of `docs.rs` lands on.
    assert!(
        readme.contains("### Attribution"),
        "README.md must keep its Attribution section"
    );
}

/// The notice is **canonical**, not merely present.
///
/// `flat()` exists so a markdown line wrap is not a licence breach — but it
/// normalises whichever side it is given, so comparing two normalised strings
/// cannot see damage to the source of truth. It did not: a `\`-continued
/// literal collapsed by `rustfmt` left a 32-space gap inside the constant, and
/// every report emitted it that way while this file reported success.
///
/// So the constant is held to a stricter standard than the copies: single
/// spaces, no leading or trailing whitespace, no newlines.
#[test]
fn the_notice_is_canonical_not_merely_present() {
    assert_eq!(
        ATTRIBUTION,
        flat(ATTRIBUTION),
        "the source of truth must be canonical — copies may wrap, this may not"
    );
    assert!(!ATTRIBUTION.contains('\n'));
    assert_eq!(ATTRIBUTION.trim(), ATTRIBUTION);
}

/// The notice says what the agreement requires it to say.
///
/// Pinned word for word: the condition is that the statement identifies the work
/// as an implementation *of the semantic data model*, and a paraphrase that
/// dropped that phrase would satisfy the test above while failing the licence.
#[test]
fn the_notice_states_what_the_agreement_requires() {
    assert!(ATTRIBUTION.contains("implementation of the EN 16931-1 semantic data model"));
    assert!(ATTRIBUTION.contains("© CEN"));
    assert!(ATTRIBUTION.contains("2018 CEN–EC licence agreement"));
}

/// It survives on a *valid* invoice too.
///
/// The obvious way to lose it: emit the notice only alongside findings, so a
/// clean report — the common case in production — carries nothing.
#[test]
fn even_a_clean_report_carries_it() {
    let report = validate(&Invoice::default());
    assert!(!report.is_valid(), "sanity: the empty invoice has findings");

    // A report with no findings at all still has to say it.
    let empty = en16931::validation::validate_with(&Invoice::default(), &[]);
    assert_eq!(empty.findings().len(), 0);
    assert!(
        flat(&empty.to_string()).contains(&flat(ATTRIBUTION)),
        "a report with no findings must still carry the notice:\n{empty}"
    );
}
