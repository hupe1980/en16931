//! A **maximal** invoice — every business term the model carries, populated.
//!
//! The point of a fixture here is coverage, not realism. `tests/order.rs` and
//! `tests/subset.rs` inspect what the writer *emits*, so an invoice with empty
//! optionals would exercise a fraction of the writer and report success. Every
//! `Option` that is `None` here is a branch no test reaches.
//!
//! `every_writer_branch_is_covered` in `tests/subset.rs` is the guard against
//! this file quietly falling behind the model.

#![allow(dead_code)]

use en16931::invoice::*;
use en16931::{
    Attachment, Date, DocumentKind, DocumentReference, Identifier, InvoiceAmount, Percentage,
    Quantity, UnitPriceAmount,
};
use rust_decimal::dec;

fn a(s: &str) -> InvoiceAmount {
    InvoiceAmount::parse(s).expect("fixture amount")
}
/// A unit price.
///
/// `UnitPriceAmount`'s `Display` normalises — `100.00` is written `100` — which
/// is deliberate: `BR-DEC-*` constrains amounts to two decimals but prices are
/// allowed more, so the model refuses to invent a scale. The consequence is
/// that a price round-trips *up to normalisation*, so this fixture uses values
/// that are already normalised. A fixture that ignored this would report a
/// round-trip failure for a property the model never claimed.
fn u(v: &str) -> UnitPriceAmount {
    let d: rust_decimal::Decimal = v.parse().expect("fixture unit price");
    assert_eq!(d, d.normalize(), "fixture prices must be normalised: {v}");
    UnitPriceAmount::new(d)
}
fn d(s: &str) -> Date {
    Date::parse(s).expect("fixture date")
}
fn pct(v: i64) -> Percentage {
    Percentage::from_fraction(rust_decimal::Decimal::from(v) / rust_decimal::Decimal::from(100))
        .expect("fixture percentage")
}

fn address(tag: &str) -> PostalAddress {
    PostalAddress {
        line1: Some(format!("{tag} line 1")),
        line2: Some(format!("{tag} line 2")),
        line3: Some(format!("{tag} line 3")),
        city: Some(format!("{tag} city")),
        post_code: Some("12345".into()),
        subdivision: Some("Bavaria".into()),
        country: Some(Code::new("DE")),
    }
}

fn party(tag: &str) -> Party {
    Party {
        name: Some(format!("{tag} GmbH")),
        trading_name: Some(format!("{tag} Trading")),
        identifiers: vec![
            Identifier::new(format!("{tag}-1")),
            Identifier::schemed(format!("{tag}-2"), "0088"),
        ],
        legal_registration: Some(Identifier::schemed("HRB 1234", "0198")),
        vat_identifier: Some("DE123456789".into()),
        tax_registration: Some("FC-987".into()),
        // BT-33 is the *seller's*; `UBL-CR-244` forbids `cbc:CompanyLegalForm`
        // on the customer. The model gives every party the field, so the
        // fixture must not populate it for the buyer or the round-trip is
        // asserting something UBL cannot express.
        additional_legal_information: None,
        electronic_address: Some(Identifier::schemed(format!("{tag}@example.test"), "EM")),
        address: address(tag),
        contact: Contact {
            name: Some(format!("{tag} contact")),
            phone: Some("+49 89 000000".into()),
            email: Some(format!("{tag}@example.test")),
        },
    }
}

fn line_vat() -> LineVat {
    LineVat {
        category: Code::new("S"),
        rate: Some(pct(19)),
    }
}

fn line(id: &str) -> InvoiceLine {
    InvoiceLine {
        id: id.into(),
        note: Some("line note".into()),
        order_line_reference: Some(DocumentReference::new("PO-1-1")),
        accounting_reference: Some("cost centre 42".into()),
        object_identifier: Some(Identifier::schemed("OBJ-1", "AAJ")),
        quantity: Quantity::new(dec!(3)),
        unit_code: Code::new("C62"),
        net_amount: a("300.00"),
        period: Some(Period {
            start: Some(d("2026-01-01")),
            end: Some(d("2026-01-31")),
        }),
        allowances: vec![LineAllowanceCharge {
            amount: a("10.00"),
            base_amount: Some(a("100.00")),
            percentage: Some(pct(10)),
            reason: Some("volume".into()),
            reason_code: Some(Code::new("95")),
        }],
        charges: vec![LineAllowanceCharge {
            amount: a("5.00"),
            base_amount: Some(a("100.00")),
            percentage: Some(pct(5)),
            reason: Some("packing".into()),
            reason_code: Some(Code::new("ABK")),
        }],
        price: PriceDetails {
            net_price: u("100.55"),
            price_discount: Some(u("10.05")),
            gross_price: Some(u("110.6")),
            base_quantity: Some(Quantity::new(dec!(1))),
            base_quantity_code: Some(Code::new("C62")),
        },
        vat: line_vat(),
        item: Item {
            name: Some("Widget".into()),
            description: Some("A widget".into()),
            seller_identifier: Some("S-1".into()),
            buyer_identifier: Some("B-1".into()),
            standard_identifier: Some(Identifier::schemed("4012345678901", "0160")),
            classification_identifiers: vec![Identifier::schemed("65434567", "STI")],
            origin_country: Some(Code::new("DE")),
            attributes: vec![ItemAttribute {
                name: Some("Colour".into()),
                value: Some("Blue".into()),
            }],
        },
    }
}

/// An invoice with every term the model carries populated.
#[must_use]
pub fn maximal() -> Invoice {
    let mut inv = Invoice::default();
    inv.seller.additional_legal_information = Some("Amtsgericht München HRB 1234".into());
    inv.kind = DocumentKind::Invoice;
    inv.specification_id =
        Some("urn:cen.eu:en16931:2017#compliant#urn:xoev-de:kosit:standard:xrechnung_3.0".into());
    inv.business_process = Some("urn:fdc:peppol.eu:2017:poacc:billing:01:1.0".into());
    inv.number = Some("RE-2026-0001".into());
    inv.issue_date = Some(d("2026-01-15"));
    inv.type_code = Some(Code::new("380"));
    inv.currency = Some(Code::new("EUR"));
    inv.vat_accounting_currency = Some(Code::new("SEK"));
    inv.vat_point_date = Some(d("2026-01-15"));
    inv.vat_point_date_code = Some(Code::new("35"));
    inv.due_date = Some(d("2026-02-15"));
    inv.buyer_reference = Some("04011000-12345-34".into());
    inv.project_reference = Some(DocumentReference::new("PRJ-1"));
    inv.contract_reference = Some(DocumentReference::new("CTR-1"));
    inv.purchase_order_reference = Some(DocumentReference::new("PO-1"));
    inv.sales_order_reference = Some(DocumentReference::new("SO-1"));
    inv.receiving_advice_reference = Some(DocumentReference::new("RA-1"));
    inv.despatch_advice_reference = Some(DocumentReference::new("DA-1"));
    inv.tender_reference = Some(DocumentReference::new("TND-1"));
    inv.object_identifier = Some(Identifier::schemed("OBJ-DOC", "AAJ"));
    inv.accounting_reference = Some("4711".into());
    inv.payment_terms = Some("Net 30".into());
    inv.notes = vec![
        InvoiceNote::new("a plain note"),
        InvoiceNote {
            subject_code: Some(Code::new("AAI")),
            note: Some("a coded note".into()),
        },
    ];
    inv.preceding_invoices = vec![PrecedingInvoice {
        reference: DocumentReference::new("RE-2025-0999"),
        issue_date: Some(d("2025-12-01")),
    }];
    inv.seller = party("Seller");
    inv.buyer = party("Buyer");
    inv.payee = Some(Payee {
        name: Some("Payee GmbH".into()),
        identifier: Some(Identifier::schemed("PAY-1", "0088")),
        legal_registration: Some(Identifier::schemed("HRB 5678", "0198")),
    });
    inv.tax_representative = Some(TaxRepresentative {
        name: Some("Rep GmbH".into()),
        vat_identifier: Some("DE987654321".into()),
        address: address("Rep"),
    });
    inv.delivery = Some(Delivery {
        party_name: Some("Delivery GmbH".into()),
        location: Some(Identifier::schemed("LOC-1", "0088")),
        date: Some(d("2026-01-10")),
        address: Some(address("Delivery")),
    });
    inv.invoicing_period = Some(Period {
        start: Some(d("2026-01-01")),
        end: Some(d("2026-01-31")),
    });
    inv.payment = Some(PaymentInstructions {
        means_code: Some(Code::new("58")),
        means_text: Some("SEPA credit transfer".into()),
        remittance_information: Some("RE-2026-0001".into()),
        means: Some(PaymentMeans::CreditTransfer(vec![CreditTransfer {
            account_identifier: Some("DE02120300000000202051".into()),
            account_name: Some("Seller GmbH".into()),
            provider_identifier: Some("BYLADEM1001".into()),
        }])),
    });
    inv.allowances = vec![DocumentAllowanceCharge {
        amount: a("20.00"),
        base_amount: Some(a("200.00")),
        percentage: Some(pct(10)),
        vat: line_vat(),
        reason: Some("discount".into()),
        reason_code: Some(Code::new("95")),
    }];
    inv.charges = vec![DocumentAllowanceCharge {
        amount: a("15.00"),
        base_amount: Some(a("300.00")),
        percentage: Some(pct(5)),
        vat: line_vat(),
        reason: Some("freight".into()),
        reason_code: Some(Code::new("FC")),
    }];
    inv.vat_breakdown = vec![
        VatBreakdown {
            taxable_amount: a("595.00"),
            tax_amount: a("113.05"),
            category: Code::new("S"),
            rate: Some(pct(19)),
            exemption_reason: None,
            exemption_reason_code: None,
        },
        VatBreakdown {
            taxable_amount: a("0.00"),
            tax_amount: a("0.00"),
            category: Code::new("E"),
            rate: Some(pct(0)),
            exemption_reason: Some("Exempt under §4".into()),
            exemption_reason_code: Some(Code::new("VATEX-EU-79-C")),
        },
    ];
    inv.attachments = vec![SupportingDocument {
        reference: DocumentReference::new("DOC-1"),
        description: Some("timesheet".into()),
        uri: Some("https://example.test/doc".into()),
        attachment: Some(
            Attachment::new(b"hello".to_vec(), "application/pdf", "doc.pdf")
                .expect("fixture attachment"),
        ),
    }];
    inv.lines = vec![line("1"), line("2")];
    inv.totals = DocumentTotals {
        line_total: a("600.00"),
        allowance_total: Some(a("20.00")),
        charge_total: Some(a("15.00")),
        taxable_total: a("595.00"),
        vat_total: Some(a("113.05")),
        vat_total_accounting: Some(a("1200.00")),
        gross_total: a("708.05"),
        paid: Some(a("8.05")),
        rounding: Some(a("0.00")),
        due: a("700.00"),
    };
    inv
}

/// The same, as a credit note — UBL's other document element.
#[must_use]
pub fn maximal_credit_note() -> Invoice {
    let mut inv = maximal();
    inv.kind = DocumentKind::CreditNote;
    inv.type_code = Some(Code::new("381"));
    // Two terms UBL's credit note cannot carry. Neither is a writer bug: no
    // authority instance places `cbc:DueDate` or `cac:ProjectReference` under
    // `<CreditNote>`, so the derived sequence has no slot for them and the
    // serialiser drops them — reporting each one. `nothing_is_dropped_that_
    // should_not_be` asserts exactly that behaviour; the fixture clears them so
    // the round-trip tests the writer rather than re-testing the schema.
    inv.due_date = None;
    inv.project_reference = None;
    inv
}

// ── Finding `spec/` ───────────────────────────────────────────────────────────
//
// See `crates/en16931/tests/common/mod.rs` for the full account. The short
// version: this was `PathBuf::from("spec")`, which resolved against the package
// directory. When the crate moved into `crates/en16931-formats/` the corpus
// suite stopped finding 490 documents and reported green.

use std::path::{Path, PathBuf};

/// The environment variable CI sets to forbid skipping.
pub const REQUIRE: &str = "EN16931_REQUIRE_SPEC";

/// The workspace's `spec/` directory, if it has been fetched.
///
/// Walks up rather than assuming a depth, so moving the crate within the
/// workspace cannot silently disable the corpus again.
pub fn spec_root() -> Option<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .map(|dir| dir.join("spec"))
        .find(|p| p.is_dir())
}

/// `spec_root()`, or `None` — and a failure instead when CI forbids skipping.
///
/// # Panics
/// When `spec/` is absent and [`REQUIRE`] is set. A skipped corpus run is
/// indistinguishable from a passing one in exactly the situation that matters.
pub fn require(suite: &str) -> Option<PathBuf> {
    match spec_root() {
        Some(p) => Some(p),
        None => {
            assert!(
                std::env::var_os(REQUIRE).is_none(),
                "{suite} needs the artefacts and {REQUIRE} is set, so skipping is \
                 not permitted here. Run `cargo xtask fetch`."
            );
            eprintln!(
                "note: {suite} SKIPPED — no spec/ directory. Run `cargo xtask fetch`. \
                 Set {REQUIRE}=1 to make this a failure."
            );
            None
        }
    }
}
