#![cfg(all(feature = "ubl", feature = "cii"))]

//! A reader never invents a value and never drops one silently.
//!
//! Two halves. The named regressions: each one a document a receiver was
//! (or would have been) told to accept although a value in it was invented or
//! lost — GitHub #14 and #15 among them, as their reporter wrote them. And the
//! per-element sweep: every leaf of the maximal invoice and credit note, in
//! both syntaxes, replaced by a value its term cannot hold and, separately,
//! deleted; each case must either carry the document's value into the model
//! or record an entry at that element. That sweep is the check that the
//! readers record what they refuse.

mod common;

use en16931::invoice::Code;
use en16931::{Percentage, Reconciler, Validated, XRechnung, examples, profiles};
use en16931_formats::report::{Entry, Kind};
use en16931_formats::{Policy, Recommendation, Syntax, read, receive, write};
use rust_decimal::Decimal;

// ── helpers ─────────────────────────────────────────────────────────────────

/// The entries of `kind` whose term ends with `/term` or is `term`.
fn on_term<'a>(entries: &'a [Entry], kind: Kind, term: &str) -> Vec<&'a Entry> {
    entries
        .iter()
        .filter(|e| e.kind == kind)
        .filter(|e| {
            e.term
                .as_deref()
                .is_some_and(|t| t == term || t.ends_with(&format!("/{term}")))
        })
        .collect()
}

/// `xml` with the text of the first `<tag …>…</tag>` replaced by `value`.
fn set_text(xml: &str, tag: &str, value: &str) -> String {
    let start = xml
        .find(&format!("<{tag}"))
        .unwrap_or_else(|| panic!("{tag} not in the document"));
    let open = start + xml[start..].find('>').unwrap() + 1;
    let close = open + xml[open..].find('<').unwrap();
    format!("{}{value}{}", &xml[..open], &xml[close..])
}

/// `xml` without its first `<tag …>…</tag>` element.
fn remove(xml: &str, tag: &str) -> String {
    let start = xml
        .find(&format!("<{tag}"))
        .unwrap_or_else(|| panic!("{tag} not in the document"));
    let close = format!("</{tag}>");
    let end = start + xml[start..].find(&close).unwrap() + close.len();
    format!("{}{}", &xml[..start], &xml[end..])
}

/// A valid XRechnung written to `syntax`: the shipped example, accepted as is.
fn xrechnung(syntax: Syntax) -> String {
    let invoice = examples::for_profile(&profiles::XRECHNUNG).unwrap();
    let xml = write(&Validated::<XRechnung>::new(invoice).unwrap(), syntax).xml;
    assert_eq!(
        receive(xml.as_bytes(), &Policy::default()).recommendation,
        Recommendation::Accept,
        "the unmutated example must be accepted, or the tests below prove nothing"
    );
    xml
}

const QUANTITY: [(Syntax, &str); 2] = [
    (Syntax::Cii, "ram:BilledQuantity"),
    (Syntax::Ubl, "cbc:InvoicedQuantity"),
];

// ── #14: a value the reader cannot read ─────────────────────────────────────

/// GitHub #14, the reporter's program: a quantity that is not a number was
/// read as 0, recorded nothing, and was accepted.
#[test]
fn issue_14_a_quantity_that_is_not_a_number_is_malformed() {
    for (syntax, tag) in QUANTITY {
        for bad in ["abc", "79228162514264337593543950336"] {
            let broken = set_text(&xrechnung(syntax), tag, bad);
            let got = read(broken.as_bytes()).unwrap();
            let malformed = on_term(&got.entries, Kind::Malformed, "BT-129");
            assert_eq!(
                malformed.len(),
                1,
                "{syntax} {tag} = {bad}: expected one Malformed BT-129, got {:#?}",
                got.entries
            );
            assert_eq!(malformed[0].value.as_deref(), Some(bad));
            assert_eq!(
                malformed[0].term.as_deref(),
                Some("BG-25[1]/BT-129"),
                "the most specific term path"
            );
            let r = receive(broken.as_bytes(), &Policy::default());
            assert_eq!(r.recommendation, Recommendation::Reject, "{syntax} {bad}");
        }
    }
}

/// The other numeric terms the readers parsed with a discarded error. Each
/// value is read from its element's text; `1,50` is the decimal comma a real
/// producer writes.
#[test]
fn every_numeric_term_that_does_not_parse_is_malformed() {
    let cases: &[(Syntax, &str, &str)] = &[
        (Syntax::Cii, "ram:NetPriceProductTradePrice", "BT-146"),
        (Syntax::Ubl, "cbc:PriceAmount", "BT-146"),
    ];
    for &(syntax, tag, term) in cases {
        let xml = xrechnung(syntax);
        // CII's net price is `ram:ChargeAmount` inside the price group.
        let broken = if tag == "ram:NetPriceProductTradePrice" {
            let at = xml.find(tag).unwrap();
            let inner = at + xml[at..].find("<ram:ChargeAmount").unwrap();
            format!(
                "{}{}",
                &xml[..inner],
                set_text(&xml[inner..], "ram:ChargeAmount", "1,50")
            )
        } else {
            set_text(&xml, tag, "1,50")
        };
        let got = read(broken.as_bytes()).unwrap();
        assert_eq!(
            on_term(&got.entries, Kind::Malformed, term).len(),
            1,
            "{syntax} {term} = 1,50: {:#?}",
            got.entries
        );
        assert_eq!(
            receive(broken.as_bytes(), &Policy::default()).recommendation,
            Recommendation::Reject
        );
    }
    // The VAT rate on a line (BT-152) and in the breakdown (BT-119).
    for (syntax, tag) in [
        (Syntax::Cii, "ram:RateApplicablePercent"),
        (Syntax::Ubl, "cbc:Percent"),
    ] {
        let broken = set_text(&xrechnung(syntax), tag, "19%");
        let got = read(broken.as_bytes()).unwrap();
        assert!(
            got.entries
                .iter()
                .any(|e| e.kind == Kind::Malformed && e.value.as_deref() == Some("19%")),
            "{syntax} {tag} = 19%: {:#?}",
            got.entries
        );
    }
}

// ── #15: a term the document does not carry ─────────────────────────────────

/// GitHub #15, the reporter's program: an exempt breakdown without
/// `ram:CalculatedAmount` read as BT-117 = 0.00, consistent, and accepted.
#[test]
fn issue_15_an_absent_vat_category_tax_amount_is_missing() {
    // the shipped XRechnung example, all lines exempt (E, 0 %)
    let mut invoice = examples::for_profile(&profiles::XRECHNUNG).unwrap();
    for line in &mut invoice.lines {
        line.vat.category = Code::new("E");
        line.vat.rate = Some(Percentage::new(Decimal::ZERO));
    }
    for row in invoice
        .allowances
        .iter_mut()
        .chain(invoice.charges.iter_mut())
    {
        row.vat.category = Code::new("E");
        row.vat.rate = Some(Percentage::new(Decimal::ZERO));
    }
    Reconciler::new().apply(&mut invoice).unwrap();
    for row in &mut invoice.vat_breakdown {
        row.exemption_reason = Some("Exempt".to_owned());
    }
    let xml = write(&Validated::<XRechnung>::new(invoice).unwrap(), Syntax::Cii).xml;
    assert_eq!(
        receive(xml.as_bytes(), &Policy::default()).recommendation,
        Recommendation::Accept
    );

    // drop BT-117 of the breakdown
    let without = remove(&xml, "ram:CalculatedAmount");

    let got = read(without.as_bytes()).unwrap();
    let missing = on_term(&got.entries, Kind::Missing, "BT-117");
    assert_eq!(missing.len(), 1, "{:#?}", got.entries);
    assert_eq!(missing[0].term.as_deref(), Some("BG-23[1]/BT-117"));
    let r = receive(without.as_bytes(), &Policy::default());
    assert_eq!(r.recommendation, Recommendation::Reject);
    assert!(
        r.reasons.iter().any(|x| x.contains("BT-117")),
        "the reason names the term: {:?}",
        r.reasons
    );
}

/// An empty element is an absent one (`PEPPOL-EN16931-R008`'s disposition).
#[test]
fn an_empty_required_element_is_missing() {
    let xml = xrechnung(Syntax::Ubl);
    let at = xml.find("<cbc:TaxableAmount").unwrap();
    let end = at + xml[at..].find("</cbc:TaxableAmount>").unwrap() + "</cbc:TaxableAmount>".len();
    let empty = format!(
        "{}<cbc:TaxableAmount currencyID=\"EUR\"/>{}",
        &xml[..at],
        &xml[end..]
    );
    let got = read(empty.as_bytes()).unwrap();
    assert_eq!(
        on_term(&got.entries, Kind::Missing, "BT-116").len(),
        1,
        "{:#?}",
        got.entries
    );
}

/// A line without its quantity element lacks BT-129 and BT-130, both said, in
/// document order.
#[test]
fn a_line_without_a_quantity_is_missing_both_terms() {
    for (syntax, tag) in QUANTITY {
        let without = remove(&xrechnung(syntax), tag);
        let got = read(without.as_bytes()).unwrap();
        let terms: Vec<&str> = got
            .entries
            .iter()
            .filter(|e| e.kind == Kind::Missing)
            .filter_map(|e| e.term.as_deref())
            .collect();
        assert_eq!(
            terms,
            ["BG-25[1]/BT-129", "BG-25[1]/BT-130"],
            "{syntax}: {:#?}",
            got.entries
        );
    }
}

/// Every required total, and a line's net amount and net price: absent in the
/// document, reported, rejected — however consistent the placeholder.
#[test]
fn every_absent_required_amount_is_missing() {
    let cases: &[(Syntax, &str, &str)] = &[
        (Syntax::Ubl, "cbc:PayableAmount", "BT-115"),
        (Syntax::Ubl, "cbc:TaxExclusiveAmount", "BT-109"),
        (Syntax::Ubl, "cbc:PriceAmount", "BT-146"),
        (Syntax::Cii, "ram:DuePayableAmount", "BT-115"),
        (Syntax::Cii, "ram:TaxBasisTotalAmount", "BT-109"),
        (Syntax::Cii, "ram:BasisAmount", "BT-116"),
    ];
    for &(syntax, tag, term) in cases {
        let without = remove(&xrechnung(syntax), tag);
        let got = read(without.as_bytes()).unwrap();
        assert_eq!(
            on_term(&got.entries, Kind::Missing, term).len(),
            1,
            "{syntax} without {tag}: {:#?}",
            got.entries
        );
        assert_eq!(
            receive(without.as_bytes(), &Policy::default()).recommendation,
            Recommendation::Reject
        );
    }
}

/// An invoice built in code is unaffected: no new finding, nothing reported.
#[test]
fn a_built_invoice_reads_back_with_no_entry() {
    for syntax in [Syntax::Ubl, Syntax::Cii] {
        let got = read(xrechnung(syntax).as_bytes()).unwrap();
        assert!(got.entries.is_empty(), "{syntax}: {:#?}", got.entries);
    }
}

// ── currency ────────────────────────────────────────────────────────────────

/// An amount in another currency than BT-5 was read as if it were in BT-5.
#[test]
fn an_amount_in_a_foreign_currency_is_malformed() {
    for (syntax, tag) in [
        (Syntax::Ubl, "cbc:LineExtensionAmount"),
        (Syntax::Cii, "ram:LineTotalAmount"),
    ] {
        let xml = xrechnung(syntax);
        let at = xml.find(&format!("<{tag}")).unwrap();
        let usd = if xml[at..].starts_with(&format!("<{tag} currencyID=")) {
            let q = at + xml[at..].find("currencyID=\"").unwrap() + "currencyID=\"".len();
            let qe = q + xml[q..].find('"').unwrap();
            format!("{}USD{}", &xml[..q], &xml[qe..])
        } else {
            let gt = at + tag.len() + 1;
            format!("{} currencyID=\"USD\"{}", &xml[..gt], &xml[gt..])
        };
        let got = read(usd.as_bytes()).unwrap();
        let hit: Vec<&Entry> = got
            .entries
            .iter()
            .filter(|e| e.kind == Kind::Malformed && e.message.contains("USD"))
            .collect();
        assert_eq!(hit.len(), 1, "{syntax} {tag}: {:#?}", got.entries);
        assert!(
            hit[0].message.contains("EUR"),
            "names BT-5 too: {}",
            hit[0].message
        );
    }
}

// ── the remaining silent drops ──────────────────────────────────────────────

/// A CII price-level charge was read as a discount: the opposite sign.
#[test]
fn a_cii_price_level_charge_is_not_read_as_a_discount() {
    let mut inv = common::maximal();
    inv.vat_point_date_code = None;
    let xml = en16931_formats::write_unchecked(&inv, Syntax::Cii)
        .unwrap()
        .xml;
    let at = xml.find("<ram:GrossPriceProductTradePrice").unwrap();
    let ind = at + xml[at..].find("<udt:Indicator>").unwrap() + "<udt:Indicator>".len();
    let end = ind + xml[ind..].find('<').unwrap();
    assert_eq!(
        &xml[ind..end],
        "false",
        "the fixture carries a price discount"
    );
    for (value, kind) in [("true", "a charge"), ("maybe", "not a boolean")] {
        let charge = format!("{}{value}{}", &xml[..ind], &xml[end..]);
        let got = read(charge.as_bytes()).unwrap();
        let hit = on_term(&got.entries, Kind::Malformed, "BT-147");
        assert_eq!(hit.len(), 1, "{kind}: {:#?}", got.entries);
        assert!(
            got.invoice.lines[0].price.price_discount.is_none(),
            "{kind}: BT-147 must not be read"
        );
        if value == "true" {
            assert_eq!(hit[0].rule.as_deref(), Some("CII-SR-119"));
        }
    }
}

/// Attachment content that is not base64 decoded into garbage, silently.
#[test]
fn attachment_content_that_is_not_base64_is_malformed() {
    let inv = common::maximal();
    for (syntax, tag) in [
        (Syntax::Ubl, "cbc:EmbeddedDocumentBinaryObject"),
        (Syntax::Cii, "ram:AttachmentBinaryObject"),
    ] {
        let xml = en16931_formats::write_unchecked(&inv, syntax).unwrap().xml;
        let broken = set_text(&xml, tag, "abc!!!");
        let got = read(broken.as_bytes()).unwrap();
        assert_eq!(
            on_term(&got.entries, Kind::Malformed, "BT-125").len(),
            1,
            "{syntax}: {:#?}",
            got.entries
        );
    }
}

/// A CII reference whose `TypeCode` is none of 50, 130, 916 cannot be told to
/// be BT-17, BT-18 or BG-24; it was read as BG-24 without a word. A UBL
/// `DocumentTypeCode` other than 130 is a supporting document by CEN's binding,
/// and its code is now said, not dropped.
#[test]
fn an_unknown_reference_type_code_is_reported() {
    let inv = common::maximal();
    let cii = en16931_formats::write_unchecked(&inv, Syntax::Cii)
        .unwrap()
        .xml;
    let at = cii.find("<ram:AdditionalReferencedDocument").unwrap();
    let cii = format!(
        "{}{}",
        &cii[..at],
        set_text(&cii[at..], "ram:TypeCode", "999")
    );
    let got = read(cii.as_bytes()).unwrap();
    assert!(
        got.entries
            .iter()
            .any(|e| e.kind == Kind::Malformed && e.value.as_deref() == Some("999")),
        "CII: {:#?}",
        got.entries
    );

    let ubl = en16931_formats::write_unchecked(&inv, Syntax::Ubl)
        .unwrap()
        .xml;
    let at = ubl.find("<cac:AdditionalDocumentReference").unwrap();
    let id_end = at + ubl[at..].find("</cbc:ID>").unwrap() + "</cbc:ID>".len();
    let ubl = format!(
        "{}<cbc:DocumentTypeCode>999</cbc:DocumentTypeCode>{}",
        &ubl[..id_end],
        &ubl[id_end..]
    );
    let got = read(ubl.as_bytes()).unwrap();
    assert!(
        got.entries
            .iter()
            .any(|e| e.kind == Kind::Unmapped && e.value.as_deref() == Some("999")),
        "UBL: {:#?}",
        got.entries
    );
}

// ── the per-element sweep ───────────────────────────────────────────────────

/// One case the sweep may pass without a round trip or an entry, with why.
/// Paths are local names from below the document element. The list is
/// asserted exactly: an exception that stops being needed fails the sweep.
const EXCEPTIONS: &[(Syntax, Op, &str, &str)] = &[
    // A tax category's scheme (UBL) or type code (CII) absent: the category
    // is read regardless, as CEN's `BR-CL-17` / `BR-CL-18` cases read it, and
    // the writer states VAT, the only tax EN 16931 carries. A scheme stated as
    // something else is said (`Unmapped`).
    (
        Syntax::Ubl,
        Op::Delete,
        "AllowanceCharge/TaxCategory/TaxScheme",
        "read regardless of scheme; written as VAT",
    ),
    (
        Syntax::Ubl,
        Op::Delete,
        "AllowanceCharge/TaxCategory/TaxScheme/ID",
        "read regardless of scheme; written as VAT",
    ),
    (
        Syntax::Ubl,
        Op::Delete,
        "CreditNoteLine/Item/ClassifiedTaxCategory/TaxScheme",
        "read regardless of scheme; written as VAT",
    ),
    (
        Syntax::Ubl,
        Op::Delete,
        "CreditNoteLine/Item/ClassifiedTaxCategory/TaxScheme/ID",
        "read regardless of scheme; written as VAT",
    ),
    (
        Syntax::Ubl,
        Op::Delete,
        "InvoiceLine/Item/ClassifiedTaxCategory/TaxScheme",
        "read regardless of scheme; written as VAT",
    ),
    (
        Syntax::Ubl,
        Op::Delete,
        "InvoiceLine/Item/ClassifiedTaxCategory/TaxScheme/ID",
        "read regardless of scheme; written as VAT",
    ),
    (
        Syntax::Ubl,
        Op::Delete,
        "TaxTotal/TaxSubtotal/TaxCategory/TaxScheme",
        "read regardless of scheme; written as VAT",
    ),
    (
        Syntax::Ubl,
        Op::Delete,
        "TaxTotal/TaxSubtotal/TaxCategory/TaxScheme/ID",
        "read regardless of scheme; written as VAT",
    ),
    (
        Syntax::Cii,
        Op::Delete,
        "SupplyChainTradeTransaction/ApplicableHeaderTradeSettlement/ApplicableTradeTax/TypeCode",
        "read regardless of type code; written as VAT",
    ),
    (
        Syntax::Cii,
        Op::Delete,
        "SupplyChainTradeTransaction/ApplicableHeaderTradeSettlement/SpecifiedTradeAllowanceCharge/CategoryTradeTax/TypeCode",
        "read regardless of type code; written as VAT",
    ),
    (
        Syntax::Cii,
        Op::Delete,
        "SupplyChainTradeTransaction/IncludedSupplyChainTradeLineItem/SpecifiedLineTradeSettlement/ApplicableTradeTax/TypeCode",
        "read regardless of type code; written as VAT",
    ),
    // Any scheme other than VAT makes the company identifier BT-32 (CEN:
    // `upper-case(cbc:ID) != 'VAT'`); the scheme's own value is not a term,
    // and the writer writes BT-32 under its own.
    (
        Syntax::Ubl,
        Op::Value,
        "AccountingCustomerParty/Party/PartyTaxScheme/TaxScheme/ID",
        "a non-VAT scheme is BT-32; its value is no term",
    ),
    (
        Syntax::Ubl,
        Op::Value,
        "AccountingSupplierParty/Party/PartyTaxScheme/TaxScheme/ID",
        "a non-VAT scheme is BT-32; its value is no term",
    ),
    // D16B requires `ram:Name` beside BT-11 and EN 16931 has no term for it;
    // the writer repeats BT-11 there, as CEN's examples do.
    (
        Syntax::Cii,
        Op::Value,
        "SupplyChainTradeTransaction/ApplicableHeaderTradeAgreement/SpecifiedProcuringProject/Name",
        "no term; the writer repeats BT-11",
    ),
    (
        Syntax::Cii,
        Op::Delete,
        "SupplyChainTradeTransaction/ApplicableHeaderTradeAgreement/SpecifiedProcuringProject/Name",
        "no term; the writer repeats BT-11",
    ),
    // D16B makes a price allowance's indicator optional and CEN only warns
    // (`CII-SR-119`): read as the discount it can only be, it is written back
    // with the indicator stated.
    (
        Syntax::Cii,
        Op::Delete,
        "SupplyChainTradeTransaction/IncludedSupplyChainTradeLineItem/SpecifiedLineTradeAgreement/GrossPriceProductTradePrice/AppliedTradeAllowanceCharge/ChargeIndicator",
        "an allowance on price is a discount; the writer states it",
    ),
    (
        Syntax::Cii,
        Op::Delete,
        "SupplyChainTradeTransaction/IncludedSupplyChainTradeLineItem/SpecifiedLineTradeAgreement/GrossPriceProductTradePrice/AppliedTradeAllowanceCharge/ChargeIndicator/Indicator",
        "an allowance on price is a discount; the writer states it",
    ),
    // CII carries BT-149 on both prices; with the gross one deleted, the
    // net one's value is written on both — the same term, not an invention.
    (
        Syntax::Cii,
        Op::Delete,
        "SupplyChainTradeTransaction/IncludedSupplyChainTradeLineItem/SpecifiedLineTradeAgreement/GrossPriceProductTradePrice/BasisQuantity",
        "one BT-149, written on both prices",
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    /// The element's text replaced by a value its term cannot hold.
    Value,
    /// The element, and everything in it, deleted.
    Delete,
    /// The element's `@currencyID` replaced by a code it cannot be.
    Currency,
}

/// Every element below the document element, as (local-name path, its whole
/// range, its text range when it is a leaf with text, its qualified name).
struct El {
    path: String,
    range: std::ops::Range<usize>,
    text: Option<std::ops::Range<usize>>,
    /// The qualified names of the leaves with text in this element's subtree
    /// (itself, for a leaf): what a reader could invent if it were deleted.
    leaves: Vec<String>,
    currency: Option<std::ops::Range<usize>>,
}

fn qname(xml: &str, n: roxmltree::Node<'_, '_>) -> String {
    let open = &xml[n.range().start + 1..];
    open[..open
        .find(|c: char| c == '>' || c == '/' || c.is_whitespace())
        .unwrap()]
        .to_owned()
}

fn elements(xml: &str) -> Vec<El> {
    let doc = roxmltree::Document::parse(xml).unwrap();
    let root = doc.root_element();
    let mut out = Vec::new();
    for n in root.descendants().filter(|n| n.is_element() && *n != root) {
        let mut names: Vec<&str> = n
            .ancestors()
            .filter(|a| a.is_element() && *a != root)
            .map(|a| a.tag_name().name())
            .collect();
        names.reverse();
        let leaf = !n.children().any(|c| c.is_element());
        let mut leaves: Vec<String> = n
            .descendants()
            .filter(|d| d.is_element() && !d.children().any(|c| c.is_element()))
            .filter(|d| d.text().is_some_and(|t| !t.trim().is_empty()))
            .map(|d| qname(xml, d))
            .collect();
        leaves.sort();
        leaves.dedup();
        let text = n
            .first_child()
            .filter(|c| leaf && c.is_text() && !c.text().unwrap_or("").trim().is_empty())
            .map(|c| c.range());
        let currency = n.attribute_node("currencyID").map(|a| a.range_value());
        out.push(El {
            path: names.join("/"),
            range: n.range(),
            text,
            leaves,
            currency,
        });
    }
    out
}

/// Whether one of `entries` is about this element: the last segment of its
/// path is the element or a group the element sits in. A reader's paths are
/// `Parent/Child` chains, or the full path of the group a required term is
/// missing from — which, for a deleted group, is an ancestor of it.
fn says_something_about(entries: &[Entry], path: &str) -> bool {
    let segments: Vec<&str> = path.split('/').collect();
    let own = segments.last().copied().unwrap_or(path);
    // Writers' paths carry prefixes (`cac:Price/cbc:Amount`); compare local names.
    let local = |s: &str| s.rsplit(':').next().unwrap_or(s).to_owned();
    entries.iter().any(|e| {
        let theirs: Vec<String> = e.path.split('/').map(local).collect();
        theirs
            .last()
            .is_some_and(|last| segments.contains(&last.as_str()))
            || theirs.iter().any(|s| s == own)
    })
}

/// The sweep over one document. Returns the silent cases.
fn sweep(syntax: Syntax, seed: &str) -> (usize, Vec<(Op, String)>) {
    let base = read(seed.as_bytes()).unwrap();
    assert!(
        base.entries.is_empty(),
        "{syntax}: the seed reads clean: {:#?}",
        base.entries
    );
    let mut cases = 0usize;
    let mut silent = Vec::new();
    let mut check = |op: Op, el: &El, mutated: String, marker: Option<&str>| {
        cases += 1;
        let Ok(r) = read(mutated.as_bytes()) else {
            return; // refused outright: said, as `Error`
        };
        if says_something_about(&r.entries, &el.path) {
            return;
        }
        let carried = match en16931_formats::write_unchecked(&r.invoice, syntax) {
            // The writer saying what it derived is saying it.
            Ok(w) if says_something_about(&w.entries, &el.path) => true,
            Ok(w) => match (op, marker) {
                // The value reached the model if the writer emits it back.
                (Op::Value | Op::Currency, Some(m)) => w.xml.contains(m),
                // Nothing was invented if no leaf of the deleted subtree comes
                // back: a schema-required empty shell is not a value.
                (Op::Delete, _) => el.leaves.iter().all(|q| {
                    w.xml.matches(&format!("<{q}>")).count()
                        + w.xml.matches(&format!("<{q} ")).count()
                        <= mutated.matches(&format!("<{q}>")).count()
                            + mutated.matches(&format!("<{q} ")).count()
                }),
                _ => false,
            },
            // The model is too incomplete to write (no BT-1, no BT-2, no
            // line): the absence reached it as an absence.
            Err(_) => op == Op::Delete,
        };
        if !carried {
            silent.push((op, el.path.clone()));
        }
    };
    for el in elements(seed) {
        if let Some(t) = &el.text {
            let numeric = seed[t.clone()].trim().parse::<Decimal>().is_ok();
            for bad in ["abc", "79228162514264337593543950336"] {
                if bad != "abc" && !numeric {
                    continue;
                }
                let m = format!("{}{bad}{}", &seed[..t.start], &seed[t.end..]);
                check(Op::Value, &el, m, Some(&format!(">{bad}<")));
            }
        }
        if let Some(c) = &el.currency {
            for bad in ["QQQ", "USD"] {
                let m = format!("{}{bad}{}", &seed[..c.start], &seed[c.end..]);
                check(Op::Currency, &el, m, Some(&format!("currencyID=\"{bad}\"")));
            }
        }
        let m = format!("{}{}", &seed[..el.range.start], &seed[el.range.end..]);
        check(Op::Delete, &el, m, None);
    }
    (cases, silent)
}

/// **The check that the readers record what they refuse**:
/// every element of the maximal invoice and credit note, in both syntaxes —
/// each leaf's value replaced by text no term can hold (and an out-of-range
/// number where the value is one), each amount's currency by an invalid and a
/// foreign code, and every element, groups included, deleted. Each case must
/// carry the document's value into the model, not invent one, or be said.
#[test]
fn sweep_every_element_value_currency_and_absence() {
    let mut inv = common::maximal();
    // BT-8 and BT-7 together fire BR-CO-03; the reader is what is swept.
    inv.vat_point_date_code = None;
    let mut credit = common::maximal_credit_note();
    credit.vat_point_date_code = None;
    let mut all_silent = Vec::new();
    let mut total = 0usize;
    let mut lines = Vec::new();
    for (doc, invoice) in [("invoice", &inv), ("credit note", &credit)] {
        for (syntax, label) in [(Syntax::Ubl, "UBL"), (Syntax::Cii, "CII")] {
            let seed = en16931_formats::write_unchecked(invoice, syntax)
                .unwrap()
                .xml;
            let (cases, silent) = sweep(syntax, &seed);
            lines.push(format!("  {:<22}{cases:>5}", format!("{label} {doc}:")));
            total += cases;
            for (op, path) in silent {
                all_silent.push((syntax, op, path));
            }
        }
    }
    all_silent.sort_by(|a, b| {
        (a.0.to_string(), format!("{:?}", a.1), &a.2).cmp(&(
            b.0.to_string(),
            format!("{:?}", b.1),
            &b.2,
        ))
    });
    all_silent.dedup();
    let excused: Vec<_> = all_silent
        .iter()
        .filter(|(s, o, p)| {
            EXCEPTIONS
                .iter()
                .any(|(es, eo, ep, _)| es == s && eo == o && ep == p)
        })
        .collect();
    let unexcused: Vec<String> = all_silent
        .iter()
        .filter(|c| !excused.contains(c))
        .map(|(s, o, p)| format!("{s} {o:?} {p}"))
        .collect();
    let stale: Vec<_> = EXCEPTIONS
        .iter()
        .filter(|(s, o, p, _)| {
            !all_silent
                .iter()
                .any(|(as_, ao, ap)| as_ == s && ao == o && ap == p)
        })
        .map(|(s, o, p, _)| format!("{s} {o:?} {p}"))
        .collect();
    let summary = [
        vec!["reader sweep".to_owned()],
        lines,
        vec![
            format!(
                "  {:<22}{total:>5}  (value, currency and absence of every element)",
                "cases:"
            ),
            format!(
                "  {:<22}{:>5}  (each with its reason)",
                "excused:",
                excused.len()
            ),
            format!("  {:<22}{:>5}", "silent:", unexcused.len()),
        ],
    ]
    .concat()
    .join("\n");
    println!("{summary}");
    assert!(
        unexcused.is_empty(),
        "{} case(s) where a reader invented or dropped a value and said nothing:\n  {}",
        unexcused.len(),
        unexcused.join("\n  ")
    );
    assert!(
        stale.is_empty(),
        "exceptions nothing needs any more: {stale:?}"
    );
    assert_eq!(excused.len(), EXCEPTIONS.len());
    common::assert_page_pastes("conformance.md", &summary);
}
