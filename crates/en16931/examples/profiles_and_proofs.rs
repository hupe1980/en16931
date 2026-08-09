//! Validate against a profile, and turn a clean run into a **typed proof**.
//!
//! ```sh
//! cargo run --example profiles_and_proofs
//! ```
//!
//! Two things this shows that a boolean cannot:
//!
//! * **A profile is a restriction, not a different standard.** XRechnung adds
//!   `BR-DE-*` on top of EN 16931's 317; nothing is removed. So an invoice that
//!   passes XRechnung passes EN 16931 by construction — §4.4.4 of the standard,
//!   and a `Underlies` impl in the type system.
//! * **`Validated<P>` is a proof you cannot forge.** A serialiser that demands
//!   one physically cannot be handed an unchecked invoice, or one checked
//!   against a *different* profile.

use en16931::profiles::{self, XRechnung};
use en16931::validation::profile::Validated;
use en16931::{Invoice, validate};

fn main() {
    let invoice = Invoice::default();

    // Every profile the crate knows, and how strict each is.
    //
    // The width comes from the data. A hard-coded one was too narrow by a
    // single character for "XRechnung 3.0 Extension", which pushed that row's
    // columns out of line — the sort of thing nobody notices until the output
    // is in a bug report.
    let width = profiles::ALL
        .iter()
        .map(|p| p.id.len())
        .chain(std::iter::once("profile".len()))
        .max()
        .unwrap_or(24);

    println!("{:<width$} {:>6} {:>9}", "profile", "rules", "findings");
    for profile in profiles::ALL {
        let report = profile.validate(&invoice);
        println!(
            "{:<width$} {:>6} {:>9}",
            profile.id,
            report.rules_checked(),
            report.findings().len()
        );
    }

    println!(
        "\nthe bare core model:  {} rules",
        validate(&invoice).rules_checked()
    );

    // The typed proof. An empty invoice cannot produce one, and the rejection
    // hands back both the invoice and the reasons — losing the invoice on a
    // failed validation would make the error useless.
    println!("\n── asking for a proof of XRechnung conformance ──");
    match Validated::<XRechnung>::new(invoice) {
        Ok(proof) => println!("proved: {:?}", proof.invoice().number),
        Err(rejected) => {
            // `Rejected` is boxed: a failed validation carries the whole
            // invoice back, and an unboxed `Result` would make every success
            // pay for that in stack size.
            let (_invoice, report) = *rejected;
            println!("refused, with {} finding(s):", report.findings().len());

            // Findings are ordered by **business-term path**, not by rule id —
            // BT-1, BT-2, … BT-24, then the groups — because that is the order
            // someone fixing the invoice works in. Printing the path makes the
            // ordering self-evident; truncating without it made `BR-01` (BT-24)
            // look absent when it was simply further down.
            const SHOWN: usize = 8;
            let shown = &report.findings()[..SHOWN.min(report.findings().len())];
            // From the data again. A fixed width was wrong here too:
            // `PEPPOL-EN16931-R001` is nineteen characters.
            let w = shown.iter().map(|f| f.rule.len()).max().unwrap_or(0);
            for f in shown {
                println!("  {:<w$} {}", f.rule, f.path);
            }
            if let Some(rest) = report.findings().len().checked_sub(SHOWN)
                && rest > 0
            {
                println!("  … and {rest} more, in business-term order");
            }
            println!("\nThe invoice comes back with the report, so a caller can fix and retry.");
        }
    }
}
