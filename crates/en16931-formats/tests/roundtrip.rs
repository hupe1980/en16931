#![cfg(any(feature = "ubl", feature = "cii"))]

//! `Invoice → syntax → Invoice` is the identity, in both syntaxes. One test
//! body runs against UBL and CII, so a term written to one element and read
//! from another, in either binding, fails.

mod common;

use en16931::Invoice;
use en16931_formats::report::{Entry, Kind};

/// `inv` with what both schemas require and the fixture does not care about:
/// BT-1, BT-2, a seller, a buyer and, only when it has none, a line.
#[cfg_attr(not(feature = "ubl"), allow(dead_code))]
fn writable(mut inv: Invoice) -> Invoice {
    inv.number = Some("RE-1".to_owned());
    inv.issue_date = Some(en16931::Date::parse("2026-01-15").expect("date"));
    inv.seller.name = Some("Seller".to_owned());
    inv.buyer.name = Some("Buyer".to_owned());
    if inv.lines.is_empty() {
        inv.lines.push(en16931::InvoiceLine::new(
            "1",
            "Widget",
            en16931::Quantity::ONE,
            "C62",
            en16931::InvoiceAmount::parse("10.00").expect("amount"),
            "S",
            None,
        ));
    }
    inv
}

/// The shared body: check what was written, read back, and report the
/// difference per field.
fn check(inv: &Invoice, xml: &str, written: &[Entry], back: &Invoice, read: &[Entry]) {
    assert!(written.is_empty(), "the writer reported {written:?}");
    let malformed: Vec<&Entry> = read.iter().filter(|e| e.kind == Kind::Malformed).collect();
    assert!(
        malformed.is_empty(),
        "the writer emitted values its own reader rejects: {malformed:?}\n{xml}"
    );
    let unmapped: Vec<&Entry> = read.iter().filter(|e| e.kind == Kind::Unmapped).collect();
    assert!(
        unmapped.is_empty(),
        "the writer emitted elements its own reader ignores: {unmapped:?}"
    );
    // The writer's own output must give the reader nothing to report.
    assert!(read.is_empty(), "the reader reported {read:?}\n{xml}");

    let a = canonical(inv);
    let b = canonical(back);
    let (a, b) = (
        a.as_object().expect("object"),
        b.as_object().expect("object"),
    );
    let diffs: Vec<String> = a
        .keys()
        .filter(|k| a.get(*k) != b.get(*k))
        .map(|k| {
            format!(
                "{k}:\n  wrote {}\n  read  {}",
                a[k],
                b.get(k)
                    .map_or_else(|| "<missing>".to_owned(), ToString::to_string)
            )
        })
        .collect();
    assert!(
        diffs.is_empty(),
        "the round-trip is not the identity — {} field(s) differ:\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}

/// An invoice as JSON (the envelope's `invoice` object), with BT-29 and its
/// buyer counterpart sorted.
///
/// EN 16931 gives BT-29's occurrence order no meaning, and CII splits it across
/// `ram:GlobalID` and `ram:ID` in a fixed order. Every other field stays
/// positional.
fn canonical(inv: &Invoice) -> serde_json::Value {
    let mut inv = inv.clone();
    for party in [&mut inv.seller, &mut inv.buyer] {
        party
            .identifiers
            .sort_by_key(|i| (i.content().to_owned(), i.scheme().map(str::to_owned)));
    }
    let mut v = serde_json::to_value(&inv).expect("serialise");
    v.get_mut("invoice")
        .map(serde_json::Value::take)
        .expect("an invoice serialises as {\"en16931\", \"invoice\"}")
}

/// The canonical form forgives exactly a BT-29 reordering, and nothing else.
#[test]
fn the_canonical_form_sorts_bt_29_and_nothing_else() {
    let inv = common::maximal();
    assert!(
        inv.seller.identifiers.len() >= 2,
        "the fixture carries several BT-29"
    );
    let mut reordered = inv.clone();
    reordered.seller.identifiers.reverse();
    assert_ne!(reordered.seller.identifiers, inv.seller.identifiers);
    assert_eq!(
        canonical(&reordered),
        canonical(&inv),
        "BT-29 order is free"
    );

    assert!(inv.lines.len() >= 2, "the fixture carries several lines");
    let mut lines = inv.clone();
    lines.lines.reverse();
    assert_ne!(canonical(&lines), canonical(&inv), "BG-25 order is not");
    // Per field: the difference is named as `lines`, not as the envelope.
    let (a, b) = (canonical(&lines), canonical(&inv));
    let moved: Vec<&String> = a
        .as_object()
        .expect("object")
        .keys()
        .filter(|k| a.get(*k) != b.get(*k))
        .collect();
    assert_eq!(moved, ["lines"]);
}

/// Generate the same tests for every syntax compiled in.
macro_rules! syntaxes {
    ($($feature:literal => $module:ident, $syntax:ident),* $(,)?) => {$(
        #[cfg(feature = $feature)]
        mod $module {
            use super::common;
            use en16931::Invoice;
            use en16931_formats::{Error, Syntax, report::Kind};

            fn roundtrip(inv: &Invoice) {
                let out = en16931_formats::write_unchecked(inv, Syntax::$syntax)
                    .expect("the fixture carries everything the schema requires");
                if stringify!($module) == "cii" {
                    common::assert_cii_bt8_spelled_for_cii("round trip", &out.xml);
                }
                let read = en16931_formats::read(out.xml.as_bytes())
                    .expect("the writer emits readable output");
                assert_eq!(read.syntax, Syntax::$syntax);
                super::check(inv, &out.xml, &out.entries, &read.invoice, &read.entries);
            }

            #[test]
            fn a_maximal_invoice_survives_the_round_trip() {
                roundtrip(&common::maximal());
            }

            #[test]
            fn a_maximal_credit_note_survives_the_round_trip() {
                roundtrip(&common::maximal_credit_note());
            }

            /// Neither syntax can write an empty invoice; `write_unchecked`
            /// refuses it, naming what the model lacks.
            #[test]
            fn an_empty_invoice_is_refused_naming_what_it_lacks() {
                let refused = en16931_formats::write_unchecked(&Invoice::default(), Syntax::$syntax)
                    .expect_err("no schema has a form for an empty model");
                let expected: &[&str] = if stringify!($module) == "cii" {
                    &["BT-1", "BT-2"]
                } else {
                    &["BT-1", "BT-2", "BG-4", "BG-7", "BG-25"]
                };
                let schema: Vec<&str> = refused
                    .entries
                    .iter()
                    .filter(|e| e.kind == Kind::Schema)
                    .map(|e| e.term.as_deref().unwrap_or(e.path.as_str()))
                    .collect();
                assert_eq!(schema, expected, "{:?}", refused.entries);
            }

            /// Several BG-17 accounts survive (one payment-means element
            /// each); `tests/fidelity.rs` pins the wire shape.
            #[test]
            fn several_credit_transfer_accounts_survive_the_round_trip() {
                use en16931::invoice::{CreditTransfer, PaymentMeans};
                let mut inv = common::maximal();
                let p = inv.payment.as_mut().expect("fixture has payment");
                let Some(PaymentMeans::CreditTransfer(ts)) = &mut p.means else {
                    panic!("fixture pays by credit transfer");
                };
                ts.push(CreditTransfer {
                    account_identifier: Some("NL03INGB0004489902".into()),
                    account_name: None,
                    provider_identifier: None,
                });
                roundtrip(&inv);
            }

            /// Input that is not an invoice is refused, not read as empty.
            #[test]
            fn foreign_input_is_rejected() {
                assert!(matches!(en16931_formats::read(b"not xml"), Err(Error::Xml { .. })));
                assert!(matches!(
                    en16931_formats::read(b"<html><body/></html>"),
                    Err(Error::NotAnInvoice { found }) if found == "html"
                ));
            }
        }
    )*};
}

syntaxes! {
    "ubl" => ubl, Ubl,
    "cii" => cii, Cii,
}

/// Write the fixture, which carries everything the schema requires.
#[cfg(any(feature = "ubl", feature = "cii"))]
fn write(inv: &Invoice, syntax: en16931_formats::Syntax) -> en16931_formats::Written {
    en16931_formats::write_unchecked(inv, syntax).expect("the fixture is writable")
}

/// Read a document the suite wrote, requiring the syntax it was written in.
#[cfg(any(feature = "ubl", feature = "cii"))]
fn read(xml: &str, syntax: en16931_formats::Syntax) -> en16931_formats::Read {
    let r = en16931_formats::read(xml.as_bytes()).expect("readable");
    assert_eq!(r.syntax, syntax);
    r
}

/// The exact set of drops the UBL writer reports for what the syntax cannot
/// carry, so the drop mechanism cannot absorb writer bugs.
#[cfg(feature = "ubl")]
#[test]
fn ubl_drops_nothing_it_should_not() {
    use en16931_formats::Syntax;

    let out = write(&common::maximal(), Syntax::Ubl);
    assert!(
        out.entries.is_empty(),
        "an invoice should lose nothing: {:?}",
        out.entries
    );

    // BT-33 is Seller additional legal information; UBL-CR-244 forbids it on
    // the customer.
    let mut with_buyer_legal = common::maximal();
    with_buyer_legal.buyer.additional_legal_information = Some("not a seller term".into());
    let out = write(&with_buyer_legal, Syntax::Ubl);
    let [entry] = &out.entries[..] else {
        panic!("exactly one entry expected: {:?}", out.entries);
    };
    assert_eq!(entry.kind, Kind::Prohibited, "{entry}");
    assert!(entry.path.contains("CompanyLegalForm"), "{entry}");
    assert_eq!(entry.rule.as_deref(), Some("UBL-CR-244"), "{entry}");
}

/// A UBL credit note that carries BT-9 or BT-11 loses them, and reports it:
/// `<CreditNote>` has no `cbc:DueDate` and no `cac:ProjectReference`.
#[cfg(feature = "ubl")]
#[test]
fn a_ubl_credit_note_reports_what_it_cannot_carry() {
    use en16931_formats::Syntax;

    let mut cn = common::maximal_credit_note();
    cn.due_date = Some(en16931::Date::parse("2026-02-15").expect("date"));
    cn.project_reference = Some(en16931::DocumentReference::new("PRJ-1"));

    let out = write(&cn, Syntax::Ubl);
    assert_eq!(out.entries.len(), 2, "{:?}", out.entries);
    assert!(
        out.entries.iter().all(|e| e.kind == Kind::Dropped),
        "{:?}",
        out.entries
    );
    // Each drop names the business term it loses, not only the element.
    assert!(
        out.entries
            .iter()
            .any(|e| e.path.ends_with("cbc:DueDate") && e.term.as_deref() == Some("BT-9")),
        "{:?}",
        out.entries
    );
    assert!(
        out.entries.iter().any(|e| e.path.ends_with("cac:ProjectReference")
            && e.term.as_deref() == Some("BT-11")),
        "{:?}",
        out.entries
    );
}

/// Every `Dropped` entry either writer produces over the maximal fixtures —
/// whichever mechanism noticed the loss — names its business term.
#[test]
fn every_drop_names_its_term() {
    use en16931_formats::Syntax;

    let mut cn = common::maximal_credit_note();
    cn.due_date = Some(en16931::Date::parse("2026-02-15").expect("date"));
    cn.project_reference = Some(en16931::DocumentReference::new("PRJ-1"));
    let syntaxes = [
        #[cfg(feature = "ubl")]
        Syntax::Ubl,
        #[cfg(feature = "cii")]
        Syntax::Cii,
    ];
    for syntax in syntaxes {
        for inv in [common::maximal(), common::maximal_credit_note(), cn.clone()] {
            let written = write(&inv, syntax);
            for e in written.entries.iter().filter(|e| e.kind == Kind::Dropped) {
                assert!(e.term.is_some(), "{syntax:?}: {e}");
            }
        }
    }
}

/// CII carries both terms UBL's credit note cannot: one document element
/// serves both kinds.
#[cfg(feature = "cii")]
#[test]
fn a_cii_credit_note_keeps_what_ubl_would_drop() {
    use en16931_formats::Syntax;

    let mut cn = common::maximal_credit_note();
    cn.due_date = Some(en16931::Date::parse("2026-02-15").expect("date"));
    cn.project_reference = Some(en16931::DocumentReference::new("PRJ-1"));

    let out = write(&cn, Syntax::Cii);
    assert!(out.entries.is_empty(), "{:?}", out.entries);
    let back = read(&out.xml, Syntax::Cii).invoice;
    assert_eq!(back.due_date, cn.due_date);
    assert_eq!(back.project_reference, cn.project_reference);
}

/// An allowance/charge indicator written as a digit keeps the sign of the money.
///
/// `cbc:ChargeIndicator` and `udt:Indicator` are `xs:boolean`: `{true, false, 1, 0}`.
#[cfg(all(feature = "ubl", feature = "cii"))]
#[test]
fn a_charge_written_as_a_digit_is_still_a_charge() {
    use en16931_formats::Syntax;

    let mut inv = en16931::Invoice::default();
    inv.charges.push(en16931::invoice::DocumentAllowanceCharge {
        amount: en16931::InvoiceAmount::parse("10.00").expect("amount"),
        base_amount: None,
        percentage: None,
        vat: en16931::invoice::LineVat {
            category: en16931::invoice::Code::new("S"),
            rate: Some(en16931::Percentage::new(rust_decimal::Decimal::from(19))),
        },
        reason: Some("Handling fee".to_owned()),
        reason_code: None,
    });
    let inv = writable(inv);

    for (syntax, written, word, digit) in [
        (
            Syntax::Ubl,
            write(&inv, Syntax::Ubl).xml,
            "<cbc:ChargeIndicator>true</cbc:ChargeIndicator>",
            "<cbc:ChargeIndicator>1</cbc:ChargeIndicator>",
        ),
        (
            Syntax::Cii,
            write(&inv, Syntax::Cii).xml,
            "<udt:Indicator>true</udt:Indicator>",
            "<udt:Indicator>1</udt:Indicator>",
        ),
    ] {
        assert!(
            written.contains(word),
            "{syntax} writes the word: {written}"
        );
        let as_digit = written.replace(word, digit);
        let r = read(&as_digit, syntax);
        let malformed: Vec<_> = r.entries_of(Kind::Malformed).collect();
        let invoice = &r.invoice;
        assert_eq!(invoice.charges.len(), 1, "{syntax}: still a charge");
        assert!(invoice.allowances.is_empty(), "{syntax}: not an allowance");
        assert!(malformed.is_empty(), "{syntax}: {malformed:?}");
    }
}

/// A charge at price level is not a BT-147 discount.
///
/// EN 16931 gives UBL's `cac:Price/cac:AllowanceCharge` only a discount
/// (`PEPPOL-EN16931-R044` forbids a charge there).
#[cfg(feature = "ubl")]
#[test]
fn a_price_level_charge_is_reported_rather_than_read_as_a_discount() {
    let mut inv = en16931::Invoice::default();
    let mut line = en16931::InvoiceLine::new(
        "1",
        "Widget",
        en16931::Quantity::ONE,
        "C62",
        en16931::InvoiceAmount::parse("90.00").expect("amount"),
        "S",
        None,
    );
    line.price.gross_price = Some(en16931::UnitPriceAmount::new(rust_decimal::dec!(100.00)));
    line.price.price_discount = Some(en16931::UnitPriceAmount::new(rust_decimal::dec!(10.00)));
    inv.lines.push(line);
    let inv = writable(inv);

    let xml = write(&inv, en16931_formats::Syntax::Ubl).xml;
    let r = read(&xml, en16931_formats::Syntax::Ubl);
    assert_eq!(
        r.invoice.lines[0].price.price_discount,
        Some(en16931::UnitPriceAmount::new(rust_decimal::dec!(10.00))),
        "the discount round-trips"
    );

    // Flip the price-level indicator to `true`, which R044 rejects.
    let as_charge = xml.replacen(
        "<cbc:ChargeIndicator>false</cbc:ChargeIndicator>",
        "<cbc:ChargeIndicator>true</cbc:ChargeIndicator>",
        1,
    );
    assert_ne!(as_charge, xml, "the price-level indicator was found");
    let r = read(&as_charge, en16931_formats::Syntax::Ubl);
    assert_eq!(
        r.invoice.lines[0].price.price_discount, None,
        "a charge is not a discount"
    );
    let reported = r
        .entries_of(Kind::Malformed)
        .find(|e| e.path.ends_with("Price/AllowanceCharge/ChargeIndicator"))
        .unwrap_or_else(|| panic!("{:?}", r.entries));
    assert!(reported.message.contains("R044"), "{reported}");
}

/// …and an indicator that is not a boolean is reported, not read as an allowance.
#[cfg(all(feature = "ubl", feature = "cii"))]
#[test]
fn an_unreadable_charge_indicator_is_reported() {
    use en16931_formats::Syntax;

    let mut inv = en16931::Invoice::default();
    inv.charges.push(en16931::invoice::DocumentAllowanceCharge {
        amount: en16931::InvoiceAmount::parse("10.00").expect("amount"),
        base_amount: None,
        percentage: None,
        vat: en16931::invoice::LineVat {
            category: en16931::invoice::Code::new("S"),
            rate: None,
        },
        reason: None,
        reason_code: None,
    });
    let inv = writable(inv);
    let ubl = write(&inv, Syntax::Ubl).xml.replace(
        "<cbc:ChargeIndicator>true</cbc:ChargeIndicator>",
        "<cbc:ChargeIndicator>yes</cbc:ChargeIndicator>",
    );
    let r = read(&ubl, Syntax::Ubl);
    let reported = r
        .entries_of(Kind::Malformed)
        .find(|e| e.path.ends_with("AllowanceCharge/ChargeIndicator"))
        .unwrap_or_else(|| panic!("{:?}", r.entries));
    assert_eq!(reported.value.as_deref(), Some("yes"), "{reported}");
    assert_eq!(
        reported.hint.as_deref(),
        Some("the value must be `true` or `false`"),
        "{reported}"
    );

    let cii = write(&inv, Syntax::Cii).xml.replace(
        "<udt:Indicator>true</udt:Indicator>",
        "<udt:Indicator>yes</udt:Indicator>",
    );
    let r = read(&cii, Syntax::Cii);
    let reported = r
        .entries_of(Kind::Malformed)
        .find(|e| e.path.ends_with("ChargeIndicator/Indicator"))
        .unwrap_or_else(|| panic!("{:?}", r.entries));
    assert_eq!(reported.value.as_deref(), Some("yes"), "{reported}");
    assert_eq!(
        reported.hint.as_deref(),
        Some("the value must be `true` or `false`"),
        "{reported}"
    );
}

/// The reader detects the syntax from the document element: each document is
/// read as its own syntax, never as the other.
#[cfg(all(feature = "ubl", feature = "cii"))]
#[test]
fn each_document_is_read_as_its_own_syntax() {
    use en16931_formats::Syntax;

    for syntax in [Syntax::Ubl, Syntax::Cii] {
        let xml = write(&common::maximal(), syntax).xml;
        assert_eq!(read(&xml, syntax).syntax, syntax);
    }
}

/// …and a build without the other syntax's feature refuses it by name.
#[cfg(not(all(feature = "ubl", feature = "cii")))]
#[test]
fn a_build_refuses_the_syntax_it_cannot_read() {
    #[cfg(not(feature = "cii"))]
    let (doc, feature): (&[u8], _) = (
        br#"<rsm:CrossIndustryInvoice xmlns:rsm="urn:un:unece:uncefact:data:standard:CrossIndustryInvoice:100"/>"#,
        "cii",
    );
    #[cfg(not(feature = "ubl"))]
    let (doc, feature): (&[u8], _) = (
        br#"<Invoice xmlns="urn:oasis:names:specification:ubl:schema:xsd:Invoice-2"/>"#,
        "ubl",
    );
    assert!(
        matches!(
            en16931_formats::read(doc),
            Err(en16931_formats::Error::Unsupported { feature: f, .. }) if f == feature
        ),
        "{feature}"
    );
}

/// The two syntaxes agree about what the invoice is: the same model written
/// both ways reads back the same.
#[cfg(all(feature = "ubl", feature = "cii"))]
#[test]
fn the_two_syntaxes_agree() {
    use en16931_formats::Syntax;

    let inv = common::maximal();
    let via_ubl = read(&write(&inv, Syntax::Ubl).xml, Syntax::Ubl).invoice;
    let via_cii = read(&write(&inv, Syntax::Cii).xml, Syntax::Cii).invoice;

    let a = canonical(&via_ubl);
    let b = canonical(&via_cii);
    let (a, b) = (
        a.as_object().expect("object"),
        b.as_object().expect("object"),
    );
    let diffs: Vec<String> = a
        .keys()
        .filter(|k| a.get(*k) != b.get(*k))
        .map(|k| {
            format!(
                "{k}:\n  ubl {}\n  cii {}",
                a[k],
                b.get(k)
                    .map_or_else(|| "<missing>".to_owned(), ToString::to_string)
            )
        })
        .collect();
    assert!(
        diffs.is_empty(),
        "UBL and CII disagree about {} field(s):\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}

/// Excessive nesting is an error from the API, not a stack overflow.
///
/// Both readers refuse before parsing (`roxmltree` recurses per level). The
/// depth used is just past the limit, not past the overflow, so a broken guard
/// still yields a failure message.
#[test]
fn a_document_nested_past_the_limit_is_an_error_in_both_readers() {
    // In the watchdog's child, a stack overflow fails this test, not the suite.
    common::watchdog::isolated(
        "a_document_nested_past_the_limit_is_an_error_in_both_readers",
        || {
            fn nested(root: &str, ns: &str, n: usize) -> String {
                format!(
                    "<{root} xmlns=\"{ns}\">{}{}</{root}>",
                    "<a>".repeat(n),
                    "</a>".repeat(n)
                )
            }

            use en16931_formats::{Error, MAX_DEPTH};

            let check = |root: &str, ns: &str| {
                let deep = nested(root, ns, MAX_DEPTH + 1);
                match en16931_formats::read(deep.as_bytes()) {
                    Err(Error::TooDeep { depth, limit }) => {
                        assert_eq!(limit, MAX_DEPTH);
                        assert!(depth > limit, "{root}: {depth} vs {limit}");
                    }
                    other => panic!("{root}: expected TooDeep, got {other:?}"),
                }
                // …and one level inside the limit still reads.
                let ok = nested(root, ns, MAX_DEPTH - 1);
                assert!(
                    en16931_formats::read(ok.as_bytes()).is_ok(),
                    "{root}: the limit is off by one"
                );
            };

            #[cfg(feature = "ubl")]
            check(
                "Invoice",
                "urn:oasis:names:specification:ubl:schema:xsd:Invoice-2",
            );
            #[cfg(feature = "cii")]
            check(
                "CrossIndustryInvoice",
                "urn:un:unece:uncefact:data:standard:CrossIndustryInvoice:100",
            );
        },
    );
}

/// Two BG-17 accounts, as a fixture both wire-shape tests share.
#[cfg(any(feature = "ubl", feature = "cii"))]
fn two_account_invoice() -> en16931::Invoice {
    use en16931::invoice::{CreditTransfer, PaymentMeans};
    let mut inv = common::maximal();
    let p = inv.payment.as_mut().expect("fixture has payment");
    let Some(PaymentMeans::CreditTransfer(ts)) = &mut p.means else {
        panic!("fixture pays by credit transfer");
    };
    ts.push(CreditTransfer {
        account_identifier: Some("NL03INGB0004489902".into()),
        account_name: None,
        provider_identifier: None,
    });
    inv
}

/// Several accounts are several `cac:PaymentMeans`: the OASIS schema caps
/// `cac:PayeeFinancialAccount` at 0..1 per element (as in CEN's
/// `guide-example1.xml`).
#[cfg(feature = "ubl")]
#[test]
fn ubl_writes_one_payment_means_element_per_account() {
    let out = write(&two_account_invoice(), en16931_formats::Syntax::Ubl);
    assert_eq!(
        out.xml.matches("<cac:PaymentMeans>").count(),
        2,
        "{}",
        out.xml
    );
    assert_eq!(out.xml.matches("<cac:PayeeFinancialAccount>").count(), 2);
    // BT-81 rides on each element, as in CEN's example.
    assert_eq!(out.xml.matches("<cbc:PaymentMeansCode").count(), 2);
}

/// The CII twin: D16B caps `ram:PayeePartyCreditorFinancialAccount` at 0..1
/// per `ram:SpecifiedTradeSettlementPaymentMeans`.
#[cfg(feature = "cii")]
#[test]
fn cii_writes_one_payment_means_element_per_account() {
    let out = write(&two_account_invoice(), en16931_formats::Syntax::Cii);
    assert_eq!(
        out.xml
            .matches("<ram:SpecifiedTradeSettlementPaymentMeans>")
            .count(),
        2,
        "{}",
        out.xml
    );
    assert_eq!(
        out.xml
            .matches("<ram:PayeePartyCreditorFinancialAccount>")
            .count(),
        2
    );
}

/// BT-7 is a `udt:DateString` in CII; a `udt:DateTimeString` there is read and
/// reported as a [`Kind::Schema`] entry.
#[cfg(feature = "cii")]
#[test]
fn the_vat_point_date_survives_both_cii_forms() {
    use en16931_formats::Syntax;

    let inv = common::maximal();
    assert!(inv.vat_point_date.is_some());
    let written: String = write(&inv, Syntax::Cii)
        .xml
        .lines()
        .map(str::trim)
        .collect();
    let current = "<ram:TaxPointDate><udt:DateString format=\"102\">20260115</udt:DateString></ram:TaxPointDate>";
    assert!(written.contains(current), "{written}");

    let legacy = written.replace(
        current,
        "<ram:TaxPointDate><udt:DateTimeString format=\"102\">20260115</udt:DateTimeString></ram:TaxPointDate>",
    );
    for (xml, is_legacy) in [(&written, false), (&legacy, true)] {
        let r = read(xml, Syntax::Cii);
        assert_eq!(r.invoice.vat_point_date, inv.vat_point_date);
        assert_eq!(r.entries_of(Kind::Malformed).count(), 0, "{:?}", r.entries);
        assert_eq!(
            r.entries_of(Kind::Schema).count(),
            usize::from(is_legacy),
            "{:?}",
            r.entries
        );
    }
}

/// BT-8 is the model's UNTDID 2005 code and CII's UNTDID 2475 one: the
/// fixture's `35` (actual delivery date) goes out as `29` and comes back `35`.
#[cfg(feature = "cii")]
#[test]
fn the_vat_point_date_code_is_spelled_for_cii_and_read_back() {
    use en16931_formats::Syntax;

    let inv = common::maximal();
    assert_eq!(
        inv.vat_point_date_code
            .as_ref()
            .map(en16931::invoice::Code::as_str),
        Some("35")
    );
    let out = write(&inv, Syntax::Cii);
    let written: String = out.xml.lines().map(str::trim).collect();
    assert!(
        written.contains("<ram:DueDateTypeCode>29</ram:DueDateTypeCode>")
            && !written.contains("<ram:DueDateTypeCode>35</ram:DueDateTypeCode>"),
        "{written}"
    );
    let back = read(&out.xml, Syntax::Cii).invoice;
    assert_eq!(back.vat_point_date_code, inv.vat_point_date_code);
}

/// EXTENDED's `ram:SpecifiedLogisticsServiceCharge` is a document-level
/// charge (FeRD BR-FXEXT-CO-12 counts it in BT-108): BG-21 under an extension,
/// unmapped under core, where CEN's BR-CO-12 does not count it.
#[cfg(feature = "cii")]
#[test]
fn a_logistics_service_charge_is_a_charge_only_under_an_extension() {
    use en16931_formats::Syntax;

    let inv = common::maximal();
    let [freight] = &inv.charges[..] else {
        panic!("the fixture carries one charge");
    };
    let written = write(&inv, Syntax::Cii).xml;
    let start = written
        .find("<ram:SpecifiedTradeAllowanceCharge>\n        <ram:ChargeIndicator>\n          <udt:Indicator>true")
        .expect("the charge is written");
    let end = start
        + written[start..]
            .find("</ram:SpecifiedTradeAllowanceCharge>")
            .expect("closed")
        + "</ram:SpecifiedTradeAllowanceCharge>".len();
    let logistics = format!(
        "<ram:SpecifiedLogisticsServiceCharge>\
           <ram:Description>freight</ram:Description>\
           <ram:AppliedAmount>{}</ram:AppliedAmount>\
           <ram:AppliedTradeTax>\
             <ram:TypeCode>VAT</ram:TypeCode>\
             <ram:CategoryCode>{}</ram:CategoryCode>\
             <ram:RateApplicablePercent>19</ram:RateApplicablePercent>\
           </ram:AppliedTradeTax>\
         </ram:SpecifiedLogisticsServiceCharge>",
        freight.amount,
        freight.vat.category.as_str(),
    );
    let core = format!("{}{logistics}{}", &written[..start], &written[end..]);
    let id = inv.specification_id.as_deref().expect("BT-24");
    let extended = core.replace(
        id,
        "urn:cen.eu:en16931:2017#conformant#urn:factur-x.eu:1p0:extended",
    );

    let r = read(&extended, Syntax::Cii);
    let [charge] = &r.invoice.charges[..] else {
        panic!("one charge, got {:?}", r.invoice.charges);
    };
    assert_eq!(charge.amount, freight.amount);
    assert_eq!(charge.vat, freight.vat);
    assert_eq!(charge.reason.as_deref(), Some("freight"));
    assert!(
        !r.entries_of(Kind::Unmapped)
            .any(|e| e.path.contains("LogisticsServiceCharge")),
        "{:?}",
        r.entries
    );
    let report = en16931::validate(&r.invoice);
    assert!(
        !report
            .findings()
            .iter()
            .any(|f| f.rule == "BR-CO-12" || f.rule == "BR-S-08"),
        "{report}"
    );

    let r = read(&core, Syntax::Cii);
    assert!(r.invoice.charges.is_empty(), "{:?}", r.invoice.charges);
    assert!(
        r.entries_of(Kind::Unmapped)
            .any(|e| e.path == "ApplicableHeaderTradeSettlement/SpecifiedLogisticsServiceCharge"),
        "{:?}",
        r.entries
    );
    let report = en16931::validate(&r.invoice);
    assert!(
        report.findings().iter().any(|f| f.rule == "BR-CO-12"),
        "{report}"
    );
}

/// A grouping `cac:SubInvoiceLine` (one with its own child) round-trips
/// without `cbc:InvoicedQuantity` or `cac:Price`: the writer omits them when
/// `en16931::SubInvoiceLine::quantity_stated` / `price_stated` are `false`,
/// and the reader puts both back absent, not as a stated zero.
#[cfg(feature = "ubl")]
#[test]
fn a_grouping_sub_line_round_trips_without_quantity_or_price() {
    use en16931_formats::Syntax;

    let mut inv = common::maximal();
    // A declared profile with no extension groups drops `cac:SubInvoiceLine`
    // outright (`UBL-CR-646`); the Extension is the one that carries it.
    inv.specification_id = Some(
        en16931::profiles::XRECHNUNG_EXTENSION
            .specification_id
            .to_owned(),
    );

    let mut group = inv.lines[0].clone();
    group.id = "1.".to_owned();
    group.quantity = en16931::Quantity::default();
    group.unit_code = en16931::invoice::Code::default();
    group.price = en16931::invoice::PriceDetails::default();
    let mut child = inv.lines[0].clone();
    child.id = "1.1.".to_owned();
    inv.extensions.sub_invoice_lines = vec![(
        0,
        vec![en16931::SubInvoiceLine {
            vat: Some(group.vat.clone()),
            quantity_stated: false,
            price_stated: false,
            line: group,
            children: vec![en16931::SubInvoiceLine {
                vat: Some(child.vat.clone()),
                quantity_stated: true,
                price_stated: true,
                line: child,
                children: vec![],
            }],
        }],
    )];

    let written = write(&inv, Syntax::Ubl);
    assert!(
        !written
            .entries
            .iter()
            .any(|e| e.rule.as_deref() == Some("UBL-CR-646")),
        "the Extension must carry the group: {:?}",
        written.entries
    );
    let group_start = written
        .xml
        .find("<cbc:ID>1.</cbc:ID>")
        .expect("group written");
    let child_start = written.xml[group_start..]
        .find("<cac:SubInvoiceLine>")
        .expect("the child follows the group")
        + group_start;
    let group_xml = &written.xml[group_start..child_start];
    assert!(
        !group_xml.contains("InvoicedQuantity") && !group_xml.contains("<cac:Price>"),
        "the group must not write BT-129 or BG-29:\n{group_xml}"
    );

    let back = read(&written.xml, Syntax::Ubl);
    assert!(
        !back
            .entries
            .iter()
            .any(|e| e.level() == en16931::Severity::Fatal),
        "{:?}",
        back.entries
    );
    let group = &back.invoice.extensions.sub_lines(0)[0];
    assert!(!group.quantity_stated, "the group states no BT-129");
    assert!(!group.price_stated, "the group states no BG-29");
    assert_eq!(group.line.quantity, en16931::Quantity::default());
    assert_eq!(group.line.price, en16931::invoice::PriceDetails::default());
    let [child] = &group.children[..] else {
        panic!("one child, got {:?}", group.children);
    };
    assert!(child.quantity_stated, "the child states BT-129");
    assert!(child.price_stated, "the child states BG-29");
}

/// A childless `cac:SubInvoiceLine` states its quantity as
/// `cbc:CreditedQuantity` when it hangs under a `cac:CreditNoteLine` — `line`
/// reads either tag, so `quantity_stated` must too, not only
/// `cbc:InvoicedQuantity`.
///
/// Hand-built XML rather than a round trip through [`write`]: the generated
/// `cac:CreditNoteLine` sequence table (`ubl::order::ORDER`) has no slot for
/// `cac:SubInvoiceLine` at all (a `cac:CreditNoteLine` legally nests
/// `cac:SubCreditNoteLine`, a distinct element this crate's writer does not
/// emit and its reader does not read), so today's writer drops the group
/// outright before this flag ever matters on a credit note. That gap is
/// pre-existing and separate from `quantity_stated`; this test only pins the
/// flag's own logic against the element name `line` already reads.
#[cfg(feature = "ubl")]
#[test]
fn a_credit_notes_childless_sub_line_keeps_its_credited_quantity_stated() {
    use en16931_formats::Syntax;

    let xml = r#"<CreditNote xmlns="urn:oasis:names:specification:ubl:schema:xsd:CreditNote-2"
        xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2"
        xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2">
      <cac:LegalMonetaryTotal>
        <cbc:LineExtensionAmount currencyID="EUR">300.00</cbc:LineExtensionAmount>
        <cbc:TaxExclusiveAmount currencyID="EUR">300.00</cbc:TaxExclusiveAmount>
        <cbc:TaxInclusiveAmount currencyID="EUR">357.00</cbc:TaxInclusiveAmount>
        <cbc:PayableAmount currencyID="EUR">357.00</cbc:PayableAmount>
      </cac:LegalMonetaryTotal>
      <cac:CreditNoteLine>
        <cbc:ID>1</cbc:ID>
        <cbc:CreditedQuantity unitCode="C62">3</cbc:CreditedQuantity>
        <cbc:LineExtensionAmount currencyID="EUR">300.00</cbc:LineExtensionAmount>
        <cac:Item>
          <cbc:Name>Widget</cbc:Name>
          <cac:ClassifiedTaxCategory>
            <cbc:ID>S</cbc:ID>
            <cbc:Percent>19</cbc:Percent>
            <cac:TaxScheme><cbc:ID>VAT</cbc:ID></cac:TaxScheme>
          </cac:ClassifiedTaxCategory>
        </cac:Item>
        <cac:Price>
          <cbc:PriceAmount currencyID="EUR">100.00</cbc:PriceAmount>
        </cac:Price>
        <cac:SubInvoiceLine>
          <cbc:ID>1.</cbc:ID>
          <cbc:CreditedQuantity unitCode="C62">3</cbc:CreditedQuantity>
          <cbc:LineExtensionAmount currencyID="EUR">300.00</cbc:LineExtensionAmount>
          <cac:Item>
            <cbc:Name>Widget</cbc:Name>
            <cac:ClassifiedTaxCategory>
              <cbc:ID>S</cbc:ID>
              <cbc:Percent>19</cbc:Percent>
              <cac:TaxScheme><cbc:ID>VAT</cbc:ID></cac:TaxScheme>
            </cac:ClassifiedTaxCategory>
          </cac:Item>
          <cac:Price>
            <cbc:PriceAmount currencyID="EUR">100.00</cbc:PriceAmount>
          </cac:Price>
        </cac:SubInvoiceLine>
      </cac:CreditNoteLine>
    </CreditNote>"#;

    let back = read(xml, Syntax::Ubl);
    assert!(
        !back
            .entries
            .iter()
            .any(|e| e.level() == en16931::Severity::Fatal),
        "a childless sub-line with both terms has no fatal entry: {:?}",
        back.entries
    );
    let [sub] = back.invoice.extensions.sub_lines(0) else {
        panic!(
            "one sub-line, got {:?}",
            back.invoice.extensions.sub_lines(0)
        );
    };
    assert!(
        sub.quantity_stated,
        "cbc:CreditedQuantity was present, so the flag must read true, not false"
    );
    assert!(
        sub.price_stated,
        "cac:Price/cbc:PriceAmount was present too"
    );
}

// ── Write, read back: the invoice is unchanged or something says why ───────────

/// The extreme amounts, such as `InvoiceAmount::MIN`, survive both syntaxes.
#[cfg(all(feature = "ubl", feature = "cii"))]
#[test]
fn the_extreme_amounts_survive_both_syntaxes() {
    use en16931::InvoiceAmount;
    use en16931_formats::Syntax;
    let mut inv = common::maximal();
    inv.lines[0].net_amount = InvoiceAmount::MIN;
    inv.totals.vat_total = Some(InvoiceAmount::MAX);
    for syntax in [Syntax::Ubl, Syntax::Cii] {
        let out = write(&inv, syntax);
        let back = read(&out.xml, syntax);
        assert!(back.entries.is_empty(), "{syntax:?}: {:?}", back.entries);
        assert_eq!(back.invoice.lines[0].net_amount, InvoiceAmount::MIN);
        assert_eq!(back.invoice.totals.vat_total, Some(InvoiceAmount::MAX));
    }
}

/// A character XML 1.0 cannot represent (a C0 control, U+FFFF) is left out and
/// reported against its term; a carriage return is written as `&#13;` and
/// survives.
#[cfg(all(feature = "ubl", feature = "cii"))]
#[test]
fn unwritable_characters_are_reported_and_a_carriage_return_survives() {
    use en16931_formats::Syntax;
    use en16931_formats::report::Kind;
    let mut inv = common::maximal();
    inv.lines[0].note = Some("first\rsecond".to_owned());
    for syntax in [Syntax::Ubl, Syntax::Cii] {
        let out = write(&inv, syntax);
        assert!(out.entries.is_empty(), "{syntax:?}: {:?}", out.entries);
        let back = read(&out.xml, syntax);
        assert_eq!(
            back.invoice.lines[0].note.as_deref(),
            Some("first\rsecond"),
            "{syntax:?}"
        );
    }
    inv.lines[0].note = Some("bell\u{7}here\u{FFFF}".to_owned());
    for syntax in [Syntax::Ubl, Syntax::Cii] {
        let out = write(&inv, syntax);
        let dropped: Vec<_> = out
            .entries
            .iter()
            .filter(|e| e.kind == Kind::Dropped)
            .collect();
        assert_eq!(dropped.len(), 1, "{syntax:?}: {:?}", out.entries);
        assert!(dropped[0].path.contains("Note"), "{:?}", dropped[0]);
        assert_eq!(
            read(&out.xml, syntax).invoice.lines[0].note.as_deref(),
            Some("bellhere")
        );
    }
}

/// The UBL reader reports an amount, a decimal or a date it cannot hold.
#[cfg(feature = "ubl")]
#[test]
fn ubl_reports_a_value_the_model_cannot_hold() {
    use en16931_formats::Syntax;
    use en16931_formats::report::Kind;
    let xml = write(&common::maximal(), Syntax::Ubl).xml;
    for (element, bad) in [
        ("cbc:LineExtensionAmount", "-92233720368547758.09"),
        ("cbc:BaseQuantity", "many"),
        ("cbc:DueDate", "2026-02-30"),
    ] {
        let open = format!("<{element}");
        let start = xml.find(&open).expect("the fixture carries it");
        let gt = start + xml[start..].find('>').expect("tag") + 1;
        let end = gt + xml[gt..].find('<').expect("close");
        let mangled = format!("{}{bad}{}", &xml[..gt], &xml[end..]);
        let back = read(&mangled, Syntax::Ubl);
        assert!(
            back.entries
                .iter()
                .any(|e| e.kind == Kind::Malformed && e.value.as_deref() == Some(bad)),
            "{element}={bad} was not reported: {:?}",
            back.entries
        );
    }
}

/// BT-110 and BT-111 are both `cac:TaxTotal/cbc:TaxAmount`, told apart by
/// `@currencyID`; a total in a third currency is an `Unmapped` entry at its path.
#[cfg(feature = "ubl")]
#[test]
fn ubl_reports_a_tax_total_in_a_third_currency() {
    use en16931_formats::Syntax;
    use en16931_formats::report::Kind;
    let inv = common::maximal();
    let xml = write(&inv, Syntax::Ubl).xml;
    let at = xml
        .find("<cac:TaxTotal")
        .expect("the fixture carries BT-110");
    let mangled = format!(
        "{}<cac:TaxTotal><cbc:TaxAmount currencyID=\"USD\">12.34</cbc:TaxAmount></cac:TaxTotal>{}",
        &xml[..at],
        &xml[at..]
    );
    let back = read(&mangled, Syntax::Ubl);
    let third: Vec<_> = back
        .entries
        .iter()
        .filter(|e| e.value.as_deref() == Some("12.34"))
        .collect();
    assert_eq!(third.len(), 1, "{:?}", back.entries);
    // A value present that the model cannot hold: fatal, not information.
    assert_eq!(third[0].kind, Kind::Malformed);
    assert_eq!(third[0].path, "Invoice/TaxTotal/TaxAmount");
    assert!(third[0].message.contains("USD"), "{}", third[0].message);
    assert_eq!(back.invoice.totals.vat_total, inv.totals.vat_total);
    assert_eq!(
        back.invoice.totals.vat_total_accounting,
        inv.totals.vat_total_accounting
    );
}

/// A blank `@currencyID` on the VAT total is the document currency, as an
/// absent one is (the writers write it blank when BT-5 is absent).
#[cfg(all(feature = "ubl", feature = "cii"))]
#[test]
fn a_blank_tax_total_currency_is_the_document_currency() {
    use en16931_formats::Syntax;
    let mut inv = common::maximal();
    inv.currency = None;
    inv.vat_accounting_currency = None;
    inv.totals.vat_total_accounting = None;
    for syntax in [Syntax::Ubl, Syntax::Cii] {
        let out = write(&inv, syntax);
        assert!(
            out.xml.contains("currencyID=\"\""),
            "{syntax:?}: the case under test"
        );
        let back = read(&out.xml, syntax);
        assert_eq!(
            back.invoice.totals.vat_total, inv.totals.vat_total,
            "{syntax:?}"
        );
        assert_eq!(back.invoice.totals.vat_total_accounting, None, "{syntax:?}");
    }
}

/// BG-23 without BT-110 is written with BT-110 derived as the sum of BT-117;
/// when that sum overflows, the UBL writer leaves the group out and reports it.
#[cfg(feature = "ubl")]
#[test]
fn an_overflowing_derived_vat_total_is_reported_not_a_panic() {
    use en16931::InvoiceAmount;
    use en16931_formats::Syntax;
    use en16931_formats::report::Kind;
    let mut inv = common::maximal();
    inv.totals.vat_total = None;
    let mut second = inv.vat_breakdown[0].clone();
    inv.vat_breakdown[0].tax_amount = InvoiceAmount::MAX;
    second.tax_amount = InvoiceAmount::MAX;
    inv.vat_breakdown.push(second);
    let out = write(&inv, Syntax::Ubl);
    assert!(
        out.entries
            .iter()
            .any(|e| e.kind == Kind::Dropped && e.term.as_deref() == Some("BG-23")),
        "{:?}",
        out.entries
    );
    assert!(!out.xml.contains("cac:TaxSubtotal"));
}

/// Tabs and line feeds in an attribute value are written as character
/// references, so attribute-value normalisation keeps them. BT-130 is an
/// attribute in UBL.
#[cfg(all(feature = "ubl", feature = "cii"))]
#[test]
fn whitespace_inside_an_attribute_survives() {
    use en16931::invoice::Code;
    use en16931_formats::Syntax;
    let mut inv = common::maximal();
    inv.lines[0].unit_code = Code::new("C\t6\n2");
    for syntax in [Syntax::Ubl, Syntax::Cii] {
        let back = read(&write(&inv, syntax).xml, syntax);
        assert_eq!(
            back.invoice.lines[0].unit_code.as_str(),
            "C\t6\n2",
            "{syntax:?}"
        );
    }
}

/// The VAT total's `@currencyID` is compared with BT-5 trimmed, as BT-5 is read.
#[cfg(all(feature = "ubl", feature = "cii"))]
#[test]
fn the_vat_total_currency_is_compared_trimmed() {
    use en16931::invoice::Code;
    use en16931_formats::Syntax;
    let mut inv = common::maximal();
    inv.currency = Some(Code::new(" EUR"));
    inv.vat_accounting_currency = None;
    inv.totals.vat_total_accounting = None;
    for syntax in [Syntax::Ubl, Syntax::Cii] {
        let back = read(&write(&inv, syntax).xml, syntax);
        assert_eq!(
            back.invoice.totals.vat_total, inv.totals.vat_total,
            "{syntax:?}"
        );
        assert_eq!(back.invoice.totals.vat_total_accounting, None, "{syntax:?}");
    }
}
