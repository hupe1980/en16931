//! Turn a report into something another system can read.
//!
//! ```sh
//! cargo run --example report_formats --features serde,svrl
//! ```
//!
//! Both shapes are feature-gated, so this example is too.
//!
//! Two shapes, for two audiences:
//!
//! * **`Report`** — a versioned JSON interchange shape. `ValidationReport`
//!   already derives `Serialize`, but that form is the crate's internal layout
//!   and changes when the internals do. Anything that *stores* reports needs a
//!   shape allowed to be boring.
//! * **SVRL** — what every Schematron tool in this field already speaks: phive,
//!   Mustangproject, the KoSIT validator. Emitting it costs **no dependency**,
//!   because writing XML is escaping and nothing else.
//!
//! One field deliberately differs from SVRL's usual meaning, and the output
//! says so in a comment rather than being quietly wrong: `location` carries a
//! business-term path, because there is no source document to point into.

use en16931::{Invoice, profiles};

fn main() {
    let report = profiles::XRECHNUNG.validate(&Invoice::default());

    {
        let interchange = en16931::Report::of(&report);
        let json = serde_json::to_string_pretty(&interchange).expect("serialise");
        // Only the head: an empty invoice produces 28 findings.
        println!("── JSON ({}) ──", en16931::report::SCHEMA);
        for line in json.lines().take(24) {
            println!("{line}");
        }
        println!("  …\n");
    }

    {
        println!("── SVRL ──");
        for line in en16931::svrl::to_svrl(&report).lines().take(14) {
            println!("{line}");
        }
        println!("  …");
    }
}
