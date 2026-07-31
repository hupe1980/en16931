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
//! A finding carrying a [`crate::Finding::hint`] adds Schematron's own
//! supplementary-text element, ahead of `svrl:text` as the content model
//! requires:
//!
//! ```xml
//! <svrl:failed-assert id="BR-CL-25" flag="fatal" location="BG-7/BT-49" test="en16931:BR-CL-25">
//!   <svrl:diagnostic-reference diagnostic="en16931-hint">9958 was DE:LID … use 0204 instead</svrl:diagnostic-reference>
//!   <svrl:text>Endpoint identifier scheme identifier MUST belong to the CEF EAS code list.</svrl:text>
//! </svrl:failed-assert>
//! ```
//!
//! `svrl:text` stays **byte-identical to the authority's wording**, which is
//! what makes a finding look up in CEN's or KoSIT's index. See
//! [`HINT_DIAGNOSTIC`] for the one caveat on `@diagnostic`.
//!
//! Anything reading SVRL for *which rules failed and why* works unchanged.
//! Anything that tries to resolve `location` as an XPath will not, and that is a
//! property of validating a model rather than a document.

use core::fmt::Write as _;

use crate::validation::{Severity, ValidationReport};

/// The SVRL namespace, as every Schematron implementation writes it.
pub const SVRL_NS: &str = "http://purl.oclc.org/dsdl/svrl";

/// The `@diagnostic` value on the element carrying a [`Finding::hint`].
///
/// In ISO Schematron `@diagnostic` is an `IDREF` into the *schema*, and this
/// crate ships no schema — there is nothing for it to point at. It is
/// namespaced so it cannot collide with a real diagnostic id if a consumer
/// merges this output with a Schematron tool's, and a reader looking for hints
/// can select on it.
///
/// [`Finding::hint`]: crate::Finding::hint
pub const HINT_DIAGNOSTIC: &str = "en16931-hint";

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

/// Write `<!-- body -->`, safely.
///
/// XML forbids `--` **anywhere inside** a comment and forbids a comment ending
/// in `-`. Both are reachable from outside this crate: [`ValidationReport::suppressed`]
/// carries rule ids a caller chose, and `Check::without("BR--CO-26")` produced a
/// document no parser would accept.
///
/// Entities are *not* escaped here, and that is deliberate: a comment's content
/// is not parsed, so writing `&amp;` inside one puts the five literal characters
/// `&amp;` in front of the reader rather than an ampersand.
fn comment(body: &str, out: &mut String) {
    out.push_str("<!-- ");
    let mut last_was_dash = false;
    for c in body.chars() {
        match c {
            // Collapse a run of hyphens to one. Replacing rather than dropping
            // keeps `BR--CO-26` legible as `BR-CO-26` instead of `BRCO-26`.
            '-' if last_was_dash => {}
            '-' => {
                last_was_dash = true;
                out.push('-');
            }
            // XML 1.0 cannot represent these at all, comment or not.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => {
                last_was_dash = false;
                out.push(c);
            }
        }
    }
    // A comment may not end with `-`; the space before `-->` also guarantees it.
    out.push_str(" -->");
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
    out.push_str("  ");
    comment(
        "Produced by en16931 from the semantic model. `location` is a \
         business-term path, not an XPath: there is no source document. `test` \
         names the rule; these rules are code, not XPath.",
        &mut out,
    );
    out.push('\n');
    out.push_str("  ");
    comment(crate::ATTRIBUTION, &mut out);
    out.push('\n');

    out.push_str("  <svrl:active-pattern name=\"");
    escape(report.profile().unwrap_or("EN 16931"), &mut out);
    out.push_str("\"/>\n");

    for id in report.suppressed() {
        out.push_str("  ");
        comment(&format!("suppressed, NOT checked: {id}"), &mut out);
        out.push('\n');
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
        out.push_str("\">\n");
        // Schematron's own slot for supplementary text, and it comes *before*
        // `text` in SVRL's content model. Putting the hint here rather than
        // appending it to `svrl:text` is what keeps `svrl:text` byte-identical
        // to the authority's wording — see `Finding::hint`.
        if let Some(h) = &f.hint {
            out.push_str("    <svrl:diagnostic-reference diagnostic=\"");
            out.push_str(HINT_DIAGNOSTIC);
            out.push_str("\">");
            escape(h, &mut out);
            out.push_str("</svrl:diagnostic-reference>\n");
        }
        out.push_str("    <svrl:text>");
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

    /// **Parse** the output, rather than looking for substrings in it.
    ///
    /// Every other test here used `contains`, and an unbalanced tag, a bad
    /// escape or an illegal comment would have satisfied all of them. It did:
    /// a suppressed rule id containing `--` produced a document no parser
    /// accepts, and nothing noticed until someone ran it.
    #[test]
    fn the_output_is_well_formed_xml() {
        let report = profiles::XRECHNUNG.validate(&Invoice::default());
        let xml = to_svrl(&report);
        let doc = roxmltree::Document::parse(&xml).expect("well-formed SVRL");

        let root = doc.root_element();
        assert_eq!(root.tag_name().name(), "schematron-output");
        assert_eq!(root.tag_name().namespace(), Some(SVRL_NS));
        assert!(
            root.attribute("title")
                .is_some_and(|t| t.contains("XRechnung"))
        );
        assert_eq!(
            root.attribute("schemaVersion"),
            Some("EN 16931-1:2017+A1:2019")
        );

        let asserts: Vec<_> = root
            .children()
            .filter(|n| n.has_tag_name((SVRL_NS, "failed-assert")))
            .collect();
        assert_eq!(asserts.len(), report.findings().len());

        for (node, finding) in asserts.iter().zip(report.findings()) {
            // SVRL requires all three; a consumer keying on any of them must
            // not meet an absent attribute.
            assert_eq!(node.attribute("id"), Some(finding.rule.as_str()));
            assert_eq!(
                node.attribute("location"),
                Some(finding.path.to_string().as_str())
            );
            assert_eq!(
                node.attribute("test"),
                Some(format!("en16931:{}", finding.rule).as_str())
            );
            assert!(matches!(
                node.attribute("flag"),
                Some("fatal" | "warning" | "information")
            ));
            let text = node
                .children()
                .find(|n| n.has_tag_name((SVRL_NS, "text")))
                .and_then(|n| n.text())
                .unwrap_or_default();
            assert!(text.starts_with(&finding.message), "{text:?}");
        }
    }

    /// Comment content that a caller can choose must not break the document.
    ///
    /// `Check::without` takes a `&str`, so the suppressed list is user input
    /// that lands inside an XML comment — where `--` is illegal outright.
    #[test]
    fn hostile_suppressions_stay_well_formed() {
        for id in [
            "BR--CO-26",  // `--` is illegal anywhere inside a comment
            "BR-CO-26--", // …including immediately before the close
            "BR-CO-26-",  // a comment may not end with `-`
            "a<b&c>d",    // metacharacters, which a comment does not escape
            "BR\u{7}CO",  // a control character XML cannot represent
            "----------",
        ] {
            let report = crate::validation::Check::new(&profiles::EN16931)
                .without(id)
                .run(&Invoice::default());
            let xml = to_svrl(&report);
            roxmltree::Document::parse(&xml)
                .unwrap_or_else(|e| panic!("suppressing {id:?} produced invalid XML: {e}\n{xml}"));
        }
    }

    /// A comment does not interpret entities, so escaping inside one is wrong:
    /// it puts the five characters `&amp;` in front of the reader.
    #[test]
    fn comments_are_not_entity_escaped() {
        let mut s = String::new();
        comment("a & b < c", &mut s);
        assert_eq!(s, "<!-- a & b < c -->");
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
