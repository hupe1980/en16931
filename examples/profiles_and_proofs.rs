//! Validate against a profile, and turn a clean run into a **typed proof**.
//!
//! ```sh
//! cargo run --example profiles_and_proofs
//! ```
//!
//! Two things this shows that a boolean cannot:
//!
//! * **A profile is a restriction, not a different standard.** XRechnung adds
//!   `BR-DE-*` on top of EN 16931's 316; nothing is removed. So an invoice that
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
    println!("{:<22} {:>6} {:>9}", "profile", "rules", "findings");
    for profile in profiles::ALL {
        let report = profile.validate(&invoice);
        println!(
            "{:<22} {:>6} {:>9}",
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
            for f in report.findings().iter().take(5) {
                println!("  {} — {}", f.rule, f.path);
            }
            println!("  …");
            println!("\nThe invoice comes back with the report, so a caller can fix and retry.");
        }
    }
}
