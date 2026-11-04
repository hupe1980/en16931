//! Shared test support: a maximal invoice (every term the model carries,
//! populated, so writer suites reach every branch) and access to the
//! reference corpus.

#![allow(dead_code)]

pub mod watchdog;

/// The documentation reader `en16931`'s `documented_numbers` suite uses.
#[path = "../../../en16931/tests/common/docs.rs"]
pub mod docs;

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
/// `UnitPriceAmount`'s `Display` normalises (`100.00` → `100`), so the fixture
/// uses already-normalised values.
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
        // BT-33 is the seller's; `UBL-CR-244` forbids it on the customer.
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
    // BT-46, the buyer's identifier, is 0..1 (BT-29, the seller's, repeats):
    // `UBL-SR-16` rejects a second one, so a maximal *lawful* buyer has one.
    inv.buyer.identifiers.truncate(1);
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
    // UBL's `<CreditNote>` has no slot for `cbc:DueDate` or
    // `cac:ProjectReference`; the writer drops and reports them
    // (`nothing_is_dropped_that_should_not_be`), so the fixture clears them.
    inv.due_date = None;
    inv.project_reference = None;
    inv
}

// ── Finding the reference corpus ───────────────────────────────────────────
//
// The location comes from `EN16931_REFERENCE`, set once for the workspace.

use std::path::{Path, PathBuf};

/// The sources in the reference corpus the corpus suites read: the
/// authorities' own published instances. Must equal `ORDER_SOURCES` in
/// `xtask` (asserted by `crates/en16931/tests/artefact_pin.rs`).
pub const AUTHORITY_SOURCES: &[&str] = &[
    "eInvoicing-EN16931",
    "peppol-bis-invoice-3",
    "xrechnung-schematron",
    "validator-configuration-xrechnung",
];

/// The environment variable CI sets to forbid skipping.
pub const REQUIRE: &str = "EN16931_REQUIRE_SPEC";

/// The reference corpus, if it has been fetched (`cargo xtask fetch`).
pub fn spec_root() -> Option<PathBuf> {
    option_env!("EN16931_REFERENCE")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

/// `spec_root()`, or `None` — and a failure instead when CI forbids skipping.
///
/// # Panics
/// When the reference corpus is absent and [`REQUIRE`] is set.
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
                "note: {suite} SKIPPED — no reference corpus. Run `cargo xtask fetch`. \
                 Set {REQUIRE}=1 to make this a failure."
            );
            None
        }
    }
}

/// CEN's CII list for BT-8 (`ram:DueDateTypeCode`, `BR-CL-06` in the CII code
/// Schematron). `tests/vat_point_date_code.rs` asserts it against the artefact.
pub const CII_VAT_POINT_DATE_CODES: &[&str] = &["5", "29", "72"];

/// Every `ram:DueDateTypeCode` a written CII document carries is in CEN's CII
/// list — the model's UNTDID 2005 code never leaks into CII.
///
/// # Panics
/// When one is not, naming `origin`.
pub fn assert_cii_bt8_spelled_for_cii(origin: &str, xml: &str) {
    let doc = roxmltree::Document::parse(xml).expect("the writer emits well-formed XML");
    for n in doc
        .descendants()
        .filter(|n| n.tag_name().name() == "DueDateTypeCode")
    {
        let code = n.text().unwrap_or_default().trim();
        assert!(
            CII_VAT_POINT_DATE_CODES.contains(&code),
            "{origin}: ram:DueDateTypeCode {code:?} is not in CEN's CII list \
             {CII_VAT_POINT_DATE_CODES:?}"
        );
    }
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

/// The syntax a document's element names, by its local name; `None` for
/// anything else, malformed XML included.
#[must_use]
pub fn sniff(xml: &str) -> Option<en16931_formats::Syntax> {
    let mut rest = xml;
    let name = loop {
        rest = &rest[rest.find('<')? + 1..];
        if let Some(body) = rest.strip_prefix("!--") {
            rest = &body[body.find("-->")? + 3..];
        } else if rest.starts_with('?') {
            rest = &rest[rest.find("?>")? + 2..];
        } else if rest.starts_with('!') {
            rest = &rest[rest.find('>')? + 1..];
        } else {
            break rest
                .split([' ', '>', '\t', '\n', '\r', '/'])
                .next()?
                .rsplit(':')
                .next()?;
        }
    };
    match name {
        #[cfg(feature = "ubl")]
        "Invoice" | "CreditNote" => Some(en16931_formats::Syntax::Ubl),
        #[cfg(feature = "cii")]
        "CrossIndustryInvoice" => Some(en16931_formats::Syntax::Cii),
        _ => None,
    }
}

/// Every document of `syntax` in the authority sources, sorted by path.
///
/// Empty when the corpus is absent (and a failure under [`REQUIRE`]).
pub fn authority_documents(syntax: en16931_formats::Syntax, suite: &str) -> Vec<(PathBuf, String)> {
    let Some(root) = require(suite) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for source in AUTHORITY_SOURCES {
        collect(&root.join(source), &mut paths);
    }
    paths.sort();
    paths
        .into_iter()
        .filter_map(|p| {
            let text = std::fs::read_to_string(&p).ok()?;
            (sniff(&text) == Some(syntax)).then_some((p, text))
        })
        .collect()
}

/// A derived `billing` document: a 19 % and a 7 % line, a 10 % allowance
/// apportioned over both VAT groups, and cash rounding to 0.05.
#[must_use]
pub fn billing_document() -> billing::Document {
    use billing::{
        Adjust, AdjustmentKind, AmountScale, CashRounding, Currency, Document, Line, Percent,
        Price, Quantity as Qty, Rounding, UnitPrice, Vat,
    };
    let vat = |p| Vat::standard(Percent::new(p)).expect("a standard rate");
    Document::builder(Currency::EUR, AmountScale::EN16931)
        .default_vat(vat(dec!(19)))
        .line(Line::new(
            "Wartung",
            Qty::new(dec!(3), "HUR"),
            UnitPrice::new(Price::new(dec!(133.33))),
        ))
        .line(Line::fixed("Fachbuch", dec!(100)).with_vat(vat(dec!(7))))
        .apportion(
            AdjustmentKind::Allowance,
            "10 % Rabatt",
            Adjust::Percent(Percent::new(dec!(10))),
        )
        .cash_rounding(CashRounding {
            increment: dec!(0.05),
            rounding: Rounding::HalfAwayFromZero,
        })
        .build()
        .expect("a lawful billing document")
}

/// [`billing_document`] under a core-valid header, through
/// `billing_adapter::from_billing`.
#[must_use]
pub fn billing_invoice() -> Invoice {
    let party = |name: &str, country: &str| Party {
        name: Some(name.to_owned()),
        address: PostalAddress {
            city: Some("Berlin".to_owned()),
            post_code: Some("10115".to_owned()),
            country: Some(Code::new(country)),
            ..Default::default()
        },
        electronic_address: Some(Identifier::schemed("4006381333931", "0088")),
        vat_identifier: Some(format!("{country}123456789")),
        ..Default::default()
    };
    let header = Invoice::builder(
        "urn:cen.eu:en16931:2017",
        "R-2026-0001",
        d("2026-07-31"),
        "380",
        "EUR",
    )
    .seller(party("Stadtwerke Musterstadt GmbH", "DE"))
    .buyer(party("Beispiel AG", "DE"))
    .due_date(d("2026-08-14"))
    .build();
    en16931::billing_adapter::from_billing(&billing_document(), header).expect("converts")
}

// ── The site's figures ─────────────────────────────────────────────────────

/// A page under `site/content/docs/`, or `None` outside the repository (the
/// site is not part of a published crate).
#[must_use]
pub fn site_page(name: &str) -> Option<String> {
    // A Windows checkout may carry CRLF; the pasted blocks are compared as LF.
    std::fs::read_to_string(docs::workspace_root().join("site/content/docs").join(name))
        .ok()
        .map(|p| p.replace("\r\n", "\n"))
}

/// `page` quotes each figure where its pattern (one `<N>`) says, at least once
/// and never as anything else.
///
/// # Panics
/// On a pattern that matches nothing or a figure that differs.
pub fn assert_page_states(name: &str, figures: &[(&str, usize)]) {
    let Some(page) = site_page(name) else {
        return;
    };
    for (pattern, expected) in figures {
        let found = docs::find_all(pattern, &page);
        assert!(
            !found.is_empty(),
            "{name} no longer matches {pattern:?}; measured {expected}"
        );
        assert!(
            found.iter().all(|n| n == expected),
            "{name}: {pattern:?} documented as {found:?}, measured {expected}"
        );
    }
}

/// `page` pastes `block`, a suite's printed summary, verbatim.
///
/// # Panics
/// When it does not.
pub fn assert_page_pastes(name: &str, block: &str) {
    let Some(page) = site_page(name) else {
        return;
    };
    assert!(
        page.contains(&format!("```text\n{block}\n```")),
        "{name} does not paste this output verbatim:\n{block}"
    );
}
