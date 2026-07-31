//! SVRL output — the format every other validator in this field speaks.
//!
//! Behind `features = ["svrl"]`.
//!
//! # Doesn't this break "no XML"?
//!
//! [§2](https://docs.rs/en16931) says no UBL, no CII, no namespaces, no XML
//! parser — and it still holds. The purpose of that invariant is that this crate
//! never learns a *syntax binding*: it must not know that BT-1 is
//! `cbc:ID`, because knowing that is `en16931-formats`' job and the reason they
//! can exist separately.
//!
//! SVRL is not an invoice syntax. It is a **report** format, and emitting one
//! requires no element names from UBL or CII, no schema, and — because writing
//! XML is escaping and nothing else — **no dependency at all**. This module adds
//! zero crates to the graph, builds on `wasm32`, and could not parse an invoice
//! if it wanted to.
//!
//! The alternative considered was a separate `en16931-svrl` crate. For ~120
//! lines of string building that is a lot of ceremony, a second version number
//! to keep in step, and one more thing for a user to discover.
//!
//! # What a consumer gets
//!
//! ```xml
//! <svrl:schematron-output title="EN 16931 — XRechnung 3.0" schemaVersion="EN 16931-1:2017+A1:2019">
//!   <svrl:active-pattern name="XRechnung 3.0"/>
//!   <svrl:failed-assert id="BR-02" flag="fatal" location="BT-1" test="en16931:BR-02">
//!     <svrl:text>An Invoice shall have an Invoice number (BT-1).</svrl:text>
//!   </svrl:failed-assert>
//! </svrl:schematron-output>
//! ```
//!
//! Two attributes deserve a caveat, and get one in the output as a comment
//! rather than being quietly wrong:
//!
//! * **`location`** is normally an XPath into the validated document. There is
//!   no document here, so it carries the business-term path — `BG-25[2]/BT-151`.
//!   A crate holding the XML can map BT → XPath; the reverse is lossy.
//! * **`test`** is normally the XPath expression that failed. These rules are
//!   Rust, so it carries `en16931:<rule-id>` rather than inventing an
//!   expression that was never evaluated.
//!
//! Anything reading SVRL for *which rules failed and why* works unchanged.
//! Anything that tries to resolve `location` as an XPath will not, and that is a
//! property of validating a model rather than a document.

use core::fmt::Write as _;

use crate::validation::{Severity, ValidationReport};

/// The SVRL namespace, as every Schematron implementation writes it.
pub const SVRL_NS: &str = "http://purl.oclc.org/dsdl/svrl";

/// Escape the five XML metacharacters.
///
/// The whole of this module's XML knowledge. Rule text is CEN's and contains
/// `&`, `<` and quotes often enough that this is not theoretical.
fn escape(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // XML 1.0 forbids most control characters outright; dropping them is
            // the only option that yields a well-formed document.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
}

const fn flag(s: Severity) -> &'static str {
    match s {
        Severity::Fatal => "fatal",
        Severity::Warning => "warning",
        Severity::Info => "information",
    }
}

/// Render a report as SVRL.
///
/// Infallible: the only failure mode of writing to a `String` is allocation.
#[must_use]
pub fn to_svrl(report: &ValidationReport) -> String {
    let mut out = String::with_capacity(256 + report.findings().len() * 160);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<svrl:schematron-output xmlns:svrl=\"");
    out.push_str(SVRL_NS);
    out.push_str("\" title=\"EN 16931");
    if let Some(p) = report.profile() {
        out.push_str(" — ");
        escape(p, &mut out);
    }
    out.push_str("\" schemaVersion=\"");
    escape(report.edition().designation(), &mut out);
    out.push_str("\">\n");

    // The caveats, in the document rather than only in the docs — whoever reads
    // this file is exactly the person who needs them.
    out.push_str(
        "  <!-- Produced by en16931 from the semantic model. `location` is a \
         business-term path, not an XPath: there is no source document. `test` \
         names the rule; these rules are code, not XPath. -->\n",
    );
    out.push_str(&format!(
        "  <!-- {} -->\n",
        crate::ATTRIBUTION.replace("--", "-")
    ));

    out.push_str("  <svrl:active-pattern name=\"");
    escape(report.profile().unwrap_or("EN 16931"), &mut out);
    out.push_str("\"/>\n");

    for id in report.suppressed() {
        out.push_str("  <!-- suppressed, NOT checked: ");
        escape(id, &mut out);
        out.push_str(" -->\n");
    }

    for f in report.findings() {
        out.push_str("  <svrl:failed-assert id=\"");
        escape(&f.rule, &mut out);
        out.push_str("\" flag=\"");
        out.push_str(flag(f.severity));
        out.push_str("\" location=\"");
        escape(&f.path.to_string(), &mut out);
        out.push_str("\" test=\"en16931:");
        escape(&f.rule, &mut out);
        out.push_str("\">\n    <svrl:text>");
        escape(&f.message, &mut out);
        if let Some(d) = &f.detail {
            let _ = write!(out, " (expected ");
            escape(&d.expected, &mut out);
            let _ = write!(out, ", found ");
            escape(&d.actual, &mut out);
            out.push(')');
        }
        out.push_str("</svrl:text>\n  </svrl:failed-assert>\n");
    }
    out.push_str("</svrl:schematron-output>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Invoice, profiles, validate};

    #[test]
    fn it_is_well_formed_and_carries_the_findings() {
        let report = profiles::XRECHNUNG.validate(&Invoice::default());
        let xml = to_svrl(&report);

        assert!(xml.starts_with("<?xml version=\"1.0\""));
        assert!(xml.contains("xmlns:svrl=\"http://purl.oclc.org/dsdl/svrl\""));
        assert!(xml.contains("schemaVersion=\"EN 16931-1:2017+A1:2019\""));
        assert!(xml.contains("<svrl:active-pattern name=\"XRechnung 3.0\"/>"));
        assert!(xml.contains("id=\"BR-02\""));
        assert!(xml.contains("flag=\"fatal\""));
        assert!(xml.contains("location=\"BT-1\""));
        assert!(xml.ends_with("</svrl:schematron-output>\n"));
        assert_eq!(
            xml.matches("<svrl:failed-assert").count(),
            report.findings().len()
        );
    }

    /// The licence notice travels with the report, in every form of it.
    #[test]
    fn the_attribution_is_present() {
        let xml = to_svrl(&validate(&Invoice::default()));
        // `--` is illegal inside an XML comment, so the notice is written with
        // the en dash reduced; the substantive words must survive.
        assert!(xml.contains("implementation of the EN 16931-1 semantic data model"));
        assert!(xml.contains("© CEN"));
    }

    /// Suppressions are visible here too, or the output would overstate.
    #[test]
    fn suppressions_are_recorded() {
        let report = crate::validation::Check::new(&profiles::EN16931)
            .without("BR-CO-26")
            .run(&Invoice::default());
        let xml = to_svrl(&report);
        assert!(xml.contains("suppressed, NOT checked: BR-CO-26"));
    }

    /// Rule text is CEN's and contains characters XML cares about.
    #[test]
    fn text_is_escaped() {
        let mut s = String::new();
        escape("a & b < c > d \" e ' f", &mut s);
        assert_eq!(s, "a &amp; b &lt; c &gt; d &quot; e &apos; f");
        // A control character cannot appear in XML 1.0 at all.
        let mut s = String::new();
        escape("a\u{7}b", &mut s);
        assert_eq!(s, "ab");
    }

    /// Escaping must hold for every rule text the crate ships, not a sample.
    #[test]
    fn every_rule_text_survives_escaping() {
        for r in crate::validation::rules::all() {
            let mut s = String::new();
            escape(r.text, &mut s);
            assert!(!s.contains('<'), "{}", r.id);
            assert!(!s.contains('>'), "{}", r.id);
            // `&` may only appear as the start of an entity.
            for (i, _) in s.match_indices('&') {
                assert!(
                    s[i..].starts_with("&amp;")
                        || s[i..].starts_with("&lt;")
                        || s[i..].starts_with("&gt;")
                        || s[i..].starts_with("&quot;")
                        || s[i..].starts_with("&apos;"),
                    "{} has a bare ampersand",
                    r.id
                );
            }
        }
    }
}
