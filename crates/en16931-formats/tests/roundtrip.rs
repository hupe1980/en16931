#![cfg(any(feature = "ubl", feature = "cii"))]

//! `Invoice → syntax → Invoice` is the identity, in **both** syntaxes.
//!
//! This is the test a hand-written serialiser and a hand-written parser make
//! impossible to trust and a shared binding makes routine. It is also the only
//! check that catches the asymmetric bug — a term written into one element and
//! read back out of another — which no amount of schema validation notices,
//! because both documents are perfectly valid.
//!
//! UBL and CII are independent mappings of the same model, so running one test
//! body against both is what catches a bug present in one and not the other.

mod common;

use en16931::Invoice;

/// The shared body: check what was written, read back, and report the
/// difference **per field**.
///
/// Comparing two `Invoice`s with `assert_eq!` prints two 200-line `Debug` dumps
/// and leaves the reader to spot the difference by eye. Naming the field that
/// moved is the difference between a five-second fix and an afternoon.
fn check(
    inv: &Invoice,
    xml: &str,
    dropped: &[String],
    back: &Invoice,
    unmapped: &[String],
    malformed: &[String],
) {
    assert!(dropped.is_empty(), "the writer dropped {dropped:?}");
    assert!(
        malformed.is_empty(),
        "the writer emitted values its own reader rejects: {malformed:?}\n{xml}"
    );
    assert!(
        unmapped.is_empty(),
        "the writer emitted elements its own reader ignores: {unmapped:?}"
    );

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

/// An invoice as JSON, with the one ordering the standard does not fix put into
/// a canonical order.
///
/// BT-29 (and its buyer counterpart) is a **repeatable** term whose occurrence
/// order EN 16931 gives no meaning to. CII splits it across `ram:GlobalID` and
/// `ram:ID` by whether it carries a scheme, and the sequence fixes *those* in a
/// fixed order — so a party with both kinds gets them back in a different order.
/// The set is preserved exactly; only the order is not, and only where the
/// standard never promised one.
///
/// Sorting here rather than comparing as sets keeps every other field strictly
/// positional, so a genuine ordering bug elsewhere still fails.
fn canonical(inv: &Invoice) -> serde_json::Value {
    let mut v = serde_json::to_value(inv).expect("serialise");
    for party in ["seller", "buyer"] {
        if let Some(ids) = v
            .get_mut(party)
            .and_then(|p| p.get_mut("identifiers"))
            .and_then(serde_json::Value::as_array_mut)
        {
            ids.sort_by_key(ToString::to_string);
        }
    }
    v
}

/// Generate the same tests for every syntax compiled in.
macro_rules! syntaxes {
    ($($feature:literal => $module:ident),* $(,)?) => {$(
        #[cfg(feature = $feature)]
        mod $module {
            use super::common;
            use en16931::Invoice;
            use en16931_formats::$module as syntax;

            fn roundtrip(inv: &Invoice) {
                let out = syntax::write(inv);
                let read = syntax::from_str(&out.xml).expect("the writer emits readable output");
                super::check(
                    inv,
                    &out.xml,
                    &out.dropped,
                    &read.invoice,
                    &read.unmapped,
                    &read.malformed,
                );
            }

            #[test]
            fn a_maximal_invoice_survives_the_round_trip() {
                roundtrip(&common::maximal());
            }

            #[test]
            fn a_maximal_credit_note_survives_the_round_trip() {
                roundtrip(&common::maximal_credit_note());
            }

            #[test]
            fn an_empty_invoice_survives_the_round_trip() {
                roundtrip(&Invoice::default());
            }

            /// Several BG-17 accounts survive — which takes several
            /// payment-means elements on the wire, because both schemas put
            /// the creditor account at 0..1 per element. A writer that packs
            /// them into one element and a reader that keeps only the last are
            /// mirror images, so `tests/fidelity.rs` pins the **wire shape**
            /// as well as the round trip.
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

            /// Input that is not this syntax must be refused, not silently read
            /// as an empty invoice.
            #[test]
            fn foreign_input_is_rejected() {
                assert!(syntax::from_str("not xml").is_err());
                assert!(syntax::from_str("<html><body/></html>").is_err());
            }
        }
    )*};
}

syntaxes! {
    "ubl" => ubl,
    "cii" => cii,
}

/// The serialiser drops what the syntax cannot carry, and **says so**. This
/// pins exactly which drops are expected.
///
/// Without it the safety net would quietly absorb writer bugs: an element put
/// in the wrong place would be dropped as "forbidden here" and the subset test
/// would still pass. The writer is required not to need the net.
#[cfg(feature = "ubl")]
#[test]
fn ubl_drops_nothing_it_should_not() {
    use en16931_formats::ubl;

    let out = ubl::write(&common::maximal());
    assert!(
        out.dropped.is_empty(),
        "an invoice should lose nothing: {:?}",
        out.dropped
    );

    // BT-33 is *Seller* additional legal information. The model gives every
    // party the field; UBL-CR-244 forbids it on the customer.
    let mut with_buyer_legal = common::maximal();
    with_buyer_legal.buyer.additional_legal_information = Some("not a seller term".into());
    let out = ubl::write(&with_buyer_legal);
    assert_eq!(out.dropped.len(), 1, "{:?}", out.dropped);
    assert!(
        out.dropped[0].contains("CompanyLegalForm"),
        "{:?}",
        out.dropped
    );
    assert!(out.dropped[0].contains("UBL-CR-244"), "{:?}", out.dropped);
}

/// A UBL credit note that carries BT-9 or BT-11 loses them — and is told.
///
/// UBL's `<CreditNote>` has no `cbc:DueDate` and no `cac:ProjectReference`.
/// Dropping them is correct; dropping them *quietly* would mean a payment due
/// date vanishing between two systems with nothing in any log.
#[cfg(feature = "ubl")]
#[test]
fn a_ubl_credit_note_reports_what_it_cannot_carry() {
    use en16931_formats::ubl;

    let mut cn = common::maximal_credit_note();
    cn.due_date = Some(en16931::Date::parse("2026-02-15").expect("date"));
    cn.project_reference = Some(en16931::DocumentReference::new("PRJ-1"));

    let out = ubl::write(&cn);
    assert_eq!(out.dropped.len(), 2, "{:?}", out.dropped);
    assert!(
        out.dropped.iter().any(|d| d.contains("DueDate")),
        "{:?}",
        out.dropped
    );
    assert!(
        out.dropped.iter().any(|d| d.contains("ProjectReference")),
        "{:?}",
        out.dropped
    );
}

/// CII carries both terms UBL's credit note cannot — one document element
/// serves both kinds. Exactly the sort of asymmetry that running one test body
/// against both syntaxes exposes.
#[cfg(feature = "cii")]
#[test]
fn a_cii_credit_note_keeps_what_ubl_would_drop() {
    use en16931_formats::cii;

    let mut cn = common::maximal_credit_note();
    cn.due_date = Some(en16931::Date::parse("2026-02-15").expect("date"));
    cn.project_reference = Some(en16931::DocumentReference::new("PRJ-1"));

    let out = cii::write(&cn);
    assert!(out.dropped.is_empty(), "{:?}", out.dropped);
    let back = cii::from_str(&out.xml).expect("readable").invoice;
    assert_eq!(back.due_date, cn.due_date);
    assert_eq!(back.project_reference, cn.project_reference);
}

/// An allowance/charge indicator written as a digit keeps the sign of the money.
///
/// `cbc:ChargeIndicator` and `udt:Indicator` are `xs:boolean`, whose lexical
/// space is `{true, false, 1, 0}`. The UBL reader knew only the two words, so a
/// schema-valid `<cbc:ChargeIndicator>1</cbc:ChargeIndicator>` read as *false*
/// and a **fee arrived as a discount** — the same amount, moved to the other
/// side of the total, with nothing in the reader's own output to say so.
#[cfg(all(feature = "ubl", feature = "cii"))]
#[test]
fn a_charge_written_as_a_digit_is_still_a_charge() {
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

    for (syntax, written, word, digit) in [
        (
            "UBL",
            en16931_formats::ubl::to_string(&inv),
            "<cbc:ChargeIndicator>true</cbc:ChargeIndicator>",
            "<cbc:ChargeIndicator>1</cbc:ChargeIndicator>",
        ),
        (
            "CII",
            en16931_formats::cii::to_string(&inv),
            "<udt:Indicator>true</udt:Indicator>",
            "<udt:Indicator>1</udt:Indicator>",
        ),
    ] {
        assert!(
            written.contains(word),
            "{syntax} writes the word: {written}"
        );
        let as_digit = written.replace(word, digit);
        let (invoice, malformed) = if syntax == "UBL" {
            let r = en16931_formats::ubl::from_str(&as_digit).expect("readable");
            (r.invoice, r.malformed)
        } else {
            let r = en16931_formats::cii::from_str(&as_digit).expect("readable");
            (r.invoice, r.malformed)
        };
        assert_eq!(invoice.charges.len(), 1, "{syntax}: still a charge");
        assert!(invoice.allowances.is_empty(), "{syntax}: not an allowance");
        assert!(malformed.is_empty(), "{syntax}: {malformed:?}");
    }
}

/// A **charge** at price level is not a BT-147 discount.
///
/// UBL hides BG-29 inside `cac:Price/cac:AllowanceCharge`, and EN 16931 gives
/// that group only a discount — `PEPPOL-EN16931-R044` forbids a charge there
/// outright. The reader ignored the indicator and mapped the amount anyway, so
/// a price *increase* was filed as a *reduction*: the same money, subtracted
/// instead of added, under the core profile where no rule objects.
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

    let xml = en16931_formats::ubl::to_string(&inv);
    let read = en16931_formats::ubl::from_str(&xml).expect("readable");
    assert_eq!(
        read.invoice.lines[0].price.price_discount,
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
    let read = en16931_formats::ubl::from_str(&as_charge).expect("readable");
    assert_eq!(
        read.invoice.lines[0].price.price_discount, None,
        "a charge is not a discount"
    );
    assert!(
        read.malformed.iter().any(|m| m.contains("R044")),
        "{:?}",
        read.malformed
    );
}

/// …and an indicator that is not a boolean at all is **reported**, not folded
/// into "allowance".
#[cfg(all(feature = "ubl", feature = "cii"))]
#[test]
fn an_unreadable_charge_indicator_is_reported() {
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
    let ubl = en16931_formats::ubl::to_string(&inv).replace(
        "<cbc:ChargeIndicator>true</cbc:ChargeIndicator>",
        "<cbc:ChargeIndicator>yes</cbc:ChargeIndicator>",
    );
    let read = en16931_formats::ubl::from_str(&ubl).expect("readable");
    assert!(
        read.malformed.iter().any(|m| m.contains("ChargeIndicator")),
        "{:?}",
        read.malformed
    );

    let cii = en16931_formats::cii::to_string(&inv).replace(
        "<udt:Indicator>true</udt:Indicator>",
        "<udt:Indicator>yes</udt:Indicator>",
    );
    let read = en16931_formats::cii::from_str(&cii).expect("readable");
    assert!(
        read.malformed.iter().any(|m| m.contains("ChargeIndicator")),
        "{:?}",
        read.malformed
    );
}

/// Each reader refuses the *other* syntax rather than returning an empty
/// invoice that looks like a valid document with nothing in it.
#[cfg(all(feature = "ubl", feature = "cii"))]
#[test]
fn each_reader_refuses_the_other_syntax() {
    use en16931_formats::{cii, ubl};

    let as_ubl = ubl::to_string(&common::maximal());
    let as_cii = cii::to_string(&common::maximal());

    assert!(matches!(cii::from_str(&as_ubl), Err(cii::Error::NotCii(_))));
    assert!(matches!(ubl::from_str(&as_cii), Err(ubl::Error::NotUbl(_))));
}

/// The two syntaxes agree about what the invoice *is*.
///
/// Writing the same model both ways and reading both back must give the same
/// invoice. This catches a term mapped correctly in one binding and to the
/// wrong element in the other — where each round-trips perfectly on its own and
/// the two nevertheless disagree.
#[cfg(all(feature = "ubl", feature = "cii"))]
#[test]
fn the_two_syntaxes_agree() {
    use en16931_formats::{cii, ubl};

    let inv = common::maximal();
    let via_ubl = ubl::from_str(&ubl::to_string(&inv)).expect("ubl").invoice;
    let via_cii = cii::from_str(&cii::to_string(&inv)).expect("cii").invoice;

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

/// **A library caller gets an error, not a dead process.**
///
/// `crates/en16931-cli/tests/cli.rs` asserts the same thing through the command;
/// this asserts it of the API, because most users never see the command and the
/// failure mode is not one they can guard against themselves. `roxmltree`
/// recurses per level of nesting and overflows the stack a few hundred levels
/// in — a stack overflow aborts the process, so there is nothing to catch and
/// nothing to log. Both readers therefore refuse before parsing.
///
/// A depth just past the limit is used deliberately: a depth past the *overflow*
/// would take the test runner with it if the guard ever regressed, and a suite
/// that dies is a suite with no failure message.
#[test]
fn a_document_nested_past_the_limit_is_an_error_in_both_readers() {
    fn nested(root: &str, ns: &str, n: usize) -> String {
        format!(
            "<{root} xmlns=\"{ns}\">{}{}</{root}>",
            "<a>".repeat(n),
            "</a>".repeat(n)
        )
    }

    #[cfg(feature = "ubl")]
    {
        use en16931_formats::ubl;
        let ns = "urn:oasis:names:specification:ubl:schema:xsd:Invoice-2";
        let deep = nested("Invoice", ns, ubl::MAX_DEPTH + 1);
        match ubl::from_str(&deep) {
            Err(ubl::Error::TooDeep { depth, limit }) => {
                assert_eq!(limit, ubl::MAX_DEPTH);
                assert!(depth > limit, "{depth} vs {limit}");
            }
            other => panic!("expected TooDeep, got {other:?}"),
        }
        // …and one level inside the limit still reads.
        let ok = nested("Invoice", ns, ubl::MAX_DEPTH - 1);
        assert!(ubl::from_str(&ok).is_ok(), "the limit is off by one");
    }

    #[cfg(feature = "cii")]
    {
        use en16931_formats::cii;
        let ns = "urn:un:unece:uncefact:data:standard:CrossIndustryInvoice:100";
        let deep = nested("CrossIndustryInvoice", ns, cii::MAX_DEPTH + 1);
        assert!(matches!(
            cii::from_str(&deep),
            Err(cii::Error::TooDeep { .. })
        ));
        let ok = nested("CrossIndustryInvoice", ns, cii::MAX_DEPTH - 1);
        assert!(cii::from_str(&ok).is_ok(), "the limit is off by one");
    }
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

/// The wire shape of several accounts is several `cac:PaymentMeans` — how
/// CEN's own `guide-example1.xml` spells two accounts, and the only shape the
/// OASIS schema admits, which caps `cac:PayeeFinancialAccount` at 0..1 per
/// element.
#[cfg(feature = "ubl")]
#[test]
fn ubl_writes_one_payment_means_element_per_account() {
    let out = en16931_formats::ubl::write(&two_account_invoice());
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
    let out = en16931_formats::cii::write(&two_account_invoice());
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
