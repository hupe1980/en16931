#![cfg(any(feature = "ubl", feature = "cii"))]

//! Reading the authorities' own documents.
//!
//! The fixtures in `tests/common` are this crate's idea of an invoice. These
//! are CEN's, KoSIT's and OpenPeppol's — including several hundred deliberately
//! *invalid* ones, which is the point: a reader that only survives well-formed
//! input has not been tested, it has been demonstrated.
//!
//! # Skipped when the artefacts are absent — and not in CI
//!
//! `spec/` is not committed: the CEN artefacts are EUPL-1.2 and the vendor
//! specifications carry their own terms. Run `cargo xtask fetch`.
//!
//! A skipped suite prints why. That is not enough on its own — nobody reads the
//! stdout of a passing test, and this suite once reported four green tests
//! having read zero of its 486 documents. So CI sets `EN16931_REQUIRE_SPEC=1`
//! and the skip becomes a failure; see `tests/common/mod.rs`.

mod common;

use std::path::{Path, PathBuf};

/// The workspace's artefacts, or a loud skip.
///
/// One tree at the workspace root serves both crates, at one pinned revision —
/// see `tests/common/mod.rs`.
fn spec_root() -> Option<PathBuf> {
    common::require("corpus")
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "xml") {
            out.push(p);
        }
    }
}

/// Every document in the corpus written in `syntax`, with its text.
fn documents(syntax: en16931_formats::Syntax) -> Vec<(PathBuf, String)> {
    let Some(root) = spec_root() else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    collect(&root, &mut paths);
    paths
        .into_iter()
        .filter_map(|p| {
            let text = std::fs::read_to_string(&p).ok()?;
            (en16931_formats::sniff(&text) == Some(syntax)).then_some((p, text))
        })
        .collect()
}

macro_rules! syntaxes {
    ($($feature:literal => ($module:ident, $syntax:ident, $min:literal, $allow:expr)),* $(,)?) => {$(
        #[cfg(feature = $feature)]
        mod $module {
            use super::{documents, spec_root};
            use en16931_formats::{$module as syntax, Syntax};

            #[test]
            fn the_reader_survives_every_published_instance() {
                let docs = documents(Syntax::$syntax);
                if docs.is_empty() {
                    println!("SKIPPED: no artefacts — run `cargo xtask fetch`");
                    assert!(spec_root().is_none(), "artefacts present but no documents found");
                    return;
                }
                let (mut read, mut rejected, mut malformed) = (0usize, 0usize, 0usize);
                for (path, text) in &docs {
                    match syntax::from_str(text) {
                        Ok(r) => {
                            read += 1;
                            malformed += r.malformed.len();
                        }
                        // Some instances are truncated on purpose — the
                        // authorities test that a validator rejects a document
                        // that is not well-formed. Refusing them *is* correct.
                        Err(syntax::Error::Xml(_)) => rejected += 1,
                        // Sniffing said this syntax and the reader disagreed.
                        Err(e) => panic!("{}: {e}", path.display()),
                    }
                }
                println!(
                    "{}: {read} documents read, {rejected} correctly rejected as not \
                     well-formed; {malformed} values present but not representable \
                     (deliberate — the corpus is partly invalid by design)",
                    stringify!($module)
                );
                assert!(read > $min, "only {read} documents — is the corpus complete?");
                assert!(
                    rejected < read / 10,
                    "{rejected} of {} unreadable — that is a reader bug, not a corpus of \
                     negative tests",
                    read + rejected
                );
            }

            /// Every element in the corpus is either mapped or **named**.
            ///
            /// A reader that quietly ignores an amount reports a clean parse and
            /// proves nothing. The allowlist is explicit so adding to it is a
            /// deliberate act with a reason, not a silent widening.
            #[test]
            fn nothing_is_ignored_without_being_named() {
                let docs = documents(Syntax::$syntax);
                if docs.is_empty() {
                    println!("SKIPPED: no artefacts — run `cargo xtask fetch`");
                    return;
                }
                let mut unmapped = std::collections::BTreeSet::new();
                for (_, text) in &docs {
                    if let Ok(r) = syntax::from_str(text) {
                        unmapped.extend(r.unmapped);
                    }
                }
                let expected: &[&str] = $allow;
                let unexpected: Vec<&String> = unmapped
                    .iter()
                    .filter(|u| !expected.contains(&u.as_str()))
                    .collect();
                assert!(
                    unexpected.is_empty(),
                    "{} ignored {} element path(s) nobody has justified:\n  {}",
                    stringify!($module),
                    unexpected.len(),
                    unexpected
                        .iter()
                        .map(|u| u.as_str())
                        .collect::<Vec<_>>()
                        .join("\n  ")
                );
                println!(
                    "{}: {} unmapped element paths, all accounted for",
                    stringify!($module),
                    unmapped.len()
                );
            }
        }
    )*};
}

syntaxes! {
    // `UBLExtensions` is forbidden by UBL-CR-001 and appears only in negative
    // instances; `CreditAccount` is a UBL payment element EN 16931 does not use.
    "ubl" => (ubl, Ubl, 200, &["Invoice/UBLExtensions", "PaymentMeans/CreditAccount"]),
    // The *debtor's* bank. EN 16931 has BT-86 for the payee's BIC and no term
    // for the payer's, so this is outside the subset rather than a reader gap.
    "cii" => (cii, Cii, 100, &["SpecifiedTradeSettlementPaymentMeans/PayerSpecifiedDebtorFinancialInstitution"]),
}
