//! The per-rule conformance corpus — the gate on every release.
//!
//! # The bar
//!
//! **Every rule in the registry has a fixture that makes it fire, and a base
//! document that does not.** A rule with no failing fixture is a rule nobody has
//! ever seen work: it may be inverted, unreachable, or checking the wrong field,
//! and the suite would be just as green either way.
//!
//! The CEN artefacts ship 207 such files, one per rule, as UBL. This crate does
//! not parse XML , so the corpus is written against the
//! semantic model directly — which turns out to be an advantage: a mutation here
//! says *"BT-37 goes missing"* rather than *"delete this element"*, and reads as
//! the rule does.
//!
//! # No silent gaps
//!
//! [`UNCOVERED`] lists every registered rule with no case yet, **by name and
//! with a reason**. The meta-test fails if a rule is uncovered and not on the
//! list, and *also* if a rule is on the list but has since been covered. So the
//! list can only shrink, and it can never quietly grow.
//!
//! That is the same discipline the code-list generator applies to the artefacts:
//! declare the expectation, and fail when reality diverges.

use en16931::invoice::*;
use en16931::profiles;
use en16931::validation::Rule;
use en16931::{
    Attachment, Date, DocumentReference, Identifier, InvoiceAmount, Percentage, Quantity,
    UnitPriceAmount, validate,
};
use rust_decimal::dec;
use std::collections::BTreeSet;

fn amount(s: &str) -> InvoiceAmount {
    InvoiceAmount::parse(s).unwrap()
}

fn pct(v: i64) -> Percentage {
    Percentage::new(rust_decimal::Decimal::from(v))
}

/// A document that satisfies **every** implemented rule, under every profile.
///
/// Every case below mutates a clone of this, so a case can only ever prove that
/// its own mutation causes its own rule to fire.
fn base() -> Invoice {
    let party = |name: &str, country: &str| Party {
        name: Some(name.to_owned()),
        address: PostalAddress {
            line1: Some("Musterstr. 1".to_owned()),
            city: Some("Berlin".to_owned()),
            post_code: Some("10115".to_owned()),
            country: Some(Code::new(country)),
            ..Default::default()
        },
        contact: Contact {
            name: Some("A. Muster".to_owned()),
            phone: Some("+49 30 1234567".to_owned()),
            email: Some("rechnung@example.de".to_owned()),
        },
        electronic_address: Some(Identifier::schemed("DE123456789", "0204")),
        vat_identifier: Some(format!("{country}123456789")),
        ..Default::default()
    };

    Invoice::builder(
        profiles::XRECHNUNG.specification_id,
        "INV-2026-001",
        Date::parse("2026-06-30").unwrap(),
        Code::new("380"),
        Code::new("EUR"),
    )
    .seller(party("Seller GmbH", "DE"))
    .buyer(party("Buyer BV", "NL"))
    .buyer_reference("04011000-12345-34")
    // BT-23 — Peppol's `R001` requires it, `R007` fixes its shape.
    .business_process("urn:fdc:peppol.eu:2017:poacc:billing:01:1.0")
    // BT-72 — XRechnung's `BR-DE-TMP-32` wants a delivery date, a period, or a
    // period on every line. This is the cheapest of the three.
    .delivery(Delivery {
        date: Some(Date::parse("2026-06-30").unwrap()),
        ..Default::default()
    })
    .due_date(Date::parse("2026-07-30").unwrap())
    .payment(PaymentInstructions {
        means_code: Some(Code::new("58")),
        means: Some(PaymentMeans::CreditTransfer(vec![CreditTransfer {
            account_identifier: Some("DE89370400440532013000".to_owned()),
            ..Default::default()
        }])),
        ..Default::default()
    })
    .line(InvoiceLine {
        id: "1".to_owned(),
        note: None,
        order_line_reference: None,
        accounting_reference: None,
        object_identifier: None,
        quantity: Quantity::new(dec!(1)),
        unit_code: Code::new("C62"),
        net_amount: amount("100.00"),
        period: None,
        allowances: vec![],
        charges: vec![],
        price: PriceDetails {
            net_price: UnitPriceAmount::new(dec!(100)),
            price_discount: None,
            gross_price: None,
            base_quantity: None,
            base_quantity_code: None,
        },
        vat: LineVat {
            category: Code::new("S"),
            rate: Some(pct(19)),
        },
        item: Item {
            name: Some("Widget".to_owned()),
            ..Default::default()
        },
    })
    .vat_breakdown(VatBreakdown {
        taxable_amount: amount("100.00"),
        tax_amount: amount("19.00"),
        category: Code::new("S"),
        rate: Some(pct(19)),
        exemption_reason: None,
        exemption_reason_code: None,
    })
    .totals(DocumentTotals {
        line_total: amount("100.00"),
        allowance_total: None,
        charge_total: None,
        taxable_total: amount("100.00"),
        vat_total: Some(amount("19.00")),
        vat_total_accounting: None,
        gross_total: amount("119.00"),
        paid: None,
        rounding: None,
        due: amount("119.00"),
    })
    .build()
}

/// A document-level allowance, for the rules that need one to exist.
fn allowance() -> DocumentAllowanceCharge {
    DocumentAllowanceCharge {
        amount: amount("10.00"),
        base_amount: None,
        percentage: None,
        vat: LineVat {
            category: Code::new("S"),
            rate: Some(pct(19)),
        },
        reason: Some("Rabatt".to_owned()),
        reason_code: None,
    }
}

/// Attach an allowance and keep the totals chain closed, so only the rule under
/// test can fire.
fn with_allowance(inv: &mut Invoice, a: DocumentAllowanceCharge) {
    let amt = a.amount;
    inv.allowances.push(a);
    inv.totals.allowance_total = Some(amt);
    inv.totals.taxable_total = inv.totals.line_total.checked_sub(amt).unwrap();
    inv.vat_breakdown[0].taxable_amount = inv.totals.taxable_total;
    inv.vat_breakdown[0].tax_amount = amount("17.10"); // 90.00 x 19 %
    inv.totals.vat_total = Some(amount("17.10"));
    inv.totals.gross_total = amount("107.10");
    inv.totals.due = amount("107.10");
}

/// One mutation that must make exactly one named rule fire.
///
/// `profile` names which rule set the mutated document is validated against.
/// Core rules fire under every profile; `PEPPOL-EN16931-*` and `BR-DE-*` only
/// fire under the profile that ships them, so a case for one of those has to
/// say so or it would silently prove nothing.
struct Case {
    rule: &'static str,
    mutate: fn(&mut Invoice),
    profile: Option<&'static en16931::Profile>,
}

const fn case(rule: &'static str, mutate: fn(&mut Invoice)) -> Case {
    Case {
        rule,
        mutate,
        profile: None,
    }
}

/// A case for a rule only [`profiles::PEPPOL_BIS_3`] carries.
const fn peppol(rule: &'static str, mutate: fn(&mut Invoice)) -> Case {
    Case {
        rule,
        mutate,
        profile: Some(&profiles::PEPPOL_BIS_3),
    }
}

/// A case for a rule only [`profiles::XRECHNUNG`] carries.
const fn xr(rule: &'static str, mutate: fn(&mut Invoice)) -> Case {
    Case {
        rule,
        mutate,
        profile: Some(&profiles::XRECHNUNG),
    }
}

/// A case for the CVD variant. Mutates [`cvd_base`], not [`base`].
const fn cvd(rule: &'static str, mutate: fn(&mut Invoice)) -> Case {
    Case {
        rule,
        mutate,
        profile: Some(&profiles::XRECHNUNG_CVD),
    }
}

/// A case for the XRechnung Extension. Mutates [`dex_base`].
const fn dex(rule: &'static str, mutate: fn(&mut Invoice)) -> Case {
    Case {
        rule,
        mutate,
        profile: Some(&profiles::XRECHNUNG_EXTENSION),
    }
}

impl Case {
    /// Validate under this case's profile, or the core set.
    fn run(&self, inv: &Invoice) -> en16931::ValidationReport {
        self.profile
            .map_or_else(|| validate(inv), |p| p.validate(inv))
    }

    /// The document this case mutates.
    ///
    /// CVD requires terms no other profile does, so its cases start from a
    /// document that already has them — otherwise every CVD case would fire
    /// `BR-DE-CVD-01` and prove nothing.
    fn start(&self) -> Invoice {
        match self.profile.map(|p| p.id) {
            Some(id) if id == profiles::XRECHNUNG_CVD.id => cvd_base(),
            Some(id) if id == profiles::XRECHNUNG_EXTENSION.id => dex_base(),
            _ => base(),
        }
    }
}

/// [`base`] declared as an XRechnung Extension document.
///
/// The Extension requires nothing extra — it only *permits* more — so this is
/// `base()` with the identifier changed. Each case then adds the group it needs.
fn dex_base() -> Invoice {
    let mut inv = base();
    inv.specification_id = Some(profiles::XRECHNUNG_EXTENSION.specification_id.to_owned());
    inv
}

/// [`base`] plus everything the Clean Vehicles Directive variant requires.
fn cvd_base() -> Invoice {
    let mut inv = base();
    inv.specification_id = Some(profiles::XRECHNUNG_CVD.specification_id.to_owned());
    inv.contract_reference = Some(DocumentReference::new("V-2026-88"));
    inv.tender_reference = Some(DocumentReference::new("LOS-3"));
    inv.lines[0].item.classification_identifiers = vec![Identifier::schemed("N1", "CVD")];
    inv.lines[0].item.attributes = vec![ItemAttribute {
        name: Some("cva".to_owned()),
        value: Some("zero-emission".to_owned()),
    }];
    inv
}

/// Every case. One line per rule, in registry order where practical.
#[rustfmt::skip]
fn cases() -> Vec<Case> {
    vec![
    // ── presence ──────────────────────────────────────────────────────────
    case("BR-01", |i| i.specification_id = None),
    case("BR-02", |i| i.number = None),
    case("BR-03", |i| i.issue_date = None),
    case("BR-04", |i| i.type_code = None),
    case("BR-05", |i| i.currency = None),
    case("BR-06", |i| i.seller.name = None),
    case("BR-07", |i| i.buyer.name = None),
    case("BR-08", |i| i.seller.address = PostalAddress::default()),
    case("BR-09", |i| i.seller.address.country = None),
    case("BR-10", |i| i.buyer.address = PostalAddress::default()),
    case("BR-11", |i| i.buyer.address.country = None),
    case("BR-16", |i| i.lines.clear()),
    case("BR-21", |i| i.lines[0].id = String::new()),
    case("BR-23", |i| i.lines[0].unit_code = Code::new("")),
    case("BR-25", |i| i.lines[0].item.name = None),
    case("BR-27", |i| i.lines[0].price.net_price = UnitPriceAmount::new(dec!(-1))),
    case("BR-28", |i| i.lines[0].price.gross_price = Some(UnitPriceAmount::new(dec!(-1)))),
    case("BR-47", |i| i.vat_breakdown[0].category = Code::new("")),
    case("BR-48", |i| i.vat_breakdown[0].rate = None),
    case("BR-CO-04", |i| i.lines[0].vat.category = Code::new("")),
    case("BR-CO-18", |i| i.vat_breakdown.clear()),

    // ── periods ───────────────────────────────────────────────────────────
    case("BR-29", |i| i.invoicing_period = Some(Period {
        start: Some(Date::parse("2026-06-30").unwrap()),
        end: Some(Date::parse("2026-06-01").unwrap()),
    })),
    case("BR-30", |i| i.lines[0].period = Some(Period {
        start: Some(Date::parse("2026-06-30").unwrap()),
        end: Some(Date::parse("2026-06-01").unwrap()),
    })),
    case("BR-CO-19", |i| i.invoicing_period = Some(Period::default())),

    // ── the totals chain, exact ───────────────────────────────────────────
    case("BR-CO-10", |i| i.totals.line_total = amount("99.00")),
    case("BR-CO-11", |i| { with_allowance(i, allowance()); i.totals.allowance_total = None }),
    case("BR-CO-12", |i| {
        i.charges.push(DocumentAllowanceCharge { reason: Some("Fracht".into()), ..allowance() });
        // charge_total left absent, which BR-CO-12 requires when a charge exists
    }),
    case("BR-CO-13", |i| i.totals.taxable_total = amount("99.00")),
    case("BR-CO-14", |i| i.totals.vat_total = Some(amount("19.01"))),
    case("BR-CO-15", |i| i.totals.gross_total = amount("119.01")),
    case("BR-CO-16", |i| i.totals.due = amount("119.01")),

    // ── the VAT derivation, ±1.00 ─────────────────────────────────────────
    case("BR-CO-17", |i| i.vat_breakdown[0].tax_amount = amount("17.50")),

    // ── header conditionality ─────────────────────────────────────────────
    case("BR-CO-03", |i| {
        i.vat_point_date = Some(Date::parse("2026-06-30").unwrap());
        i.vat_point_date_code = Some(Code::new("3"));
    }),
    case("BR-53", |i| i.vat_accounting_currency = Some(Code::new("SEK"))),
    case("BR-55", |i| i.preceding_invoices.push(PrecedingInvoice {
        reference: en16931::DocumentReference::new("  "),
        issue_date: None,
    })),
    case("BR-52", |i| i.attachments.push(SupportingDocument {
        reference: en16931::DocumentReference::new(""),
        description: None, uri: None, attachment: None,
    })),
    case("BR-57", |i| i.delivery = Some(Delivery {
        address: Some(PostalAddress::default()),
        ..Default::default()
    })),

    // ── parties ───────────────────────────────────────────────────────────
    case("BR-62", |i| i.seller.electronic_address = Some(Identifier::new("bare"))),
    case("BR-63", |i| i.buyer.electronic_address = Some(Identifier::new("bare"))),
    case("BR-64", |i| i.lines[0].item.standard_identifier = Some(Identifier::new("bare"))),
    case("BR-65", |i| i.lines[0].item.classification_identifiers = vec![Identifier::new("bare")]),
    case("BR-CO-09", |i| i.seller.vat_identifier = Some("ZZ123".to_owned())),
    case("BR-CO-26", |i| { i.seller.vat_identifier = None; i.seller.legal_registration = None }),

    // ── payment ───────────────────────────────────────────────────────────
    case("BR-49", |i| i.payment.as_mut().unwrap().means_code = None),
    case("BR-61", |i| i.payment.as_mut().unwrap().means =
        Some(PaymentMeans::CreditTransfer(vec![CreditTransfer::default()]))),
    case("BR-51", |i| {
        let p = i.payment.as_mut().unwrap();
        p.means_code = Some(Code::new("48"));
        p.means = Some(PaymentMeans::Card(PaymentCard {
            primary_account_number: Some("4111111111111111".to_owned()), // full PAN
            holder_name: None,
        }));
    }),

    // ── allowances and charges ────────────────────────────────────────────
    case("BR-CO-21", |i| with_allowance(i, DocumentAllowanceCharge { reason: None, ..allowance() })),
    case("BR-CO-22", |i| {
        i.charges.push(DocumentAllowanceCharge { reason: None, ..allowance() });
        i.totals.charge_total = Some(amount("10.00"));
        i.totals.taxable_total = amount("110.00");
        i.vat_breakdown[0].taxable_amount = amount("110.00");
        i.vat_breakdown[0].tax_amount = amount("20.90");
        i.totals.vat_total = Some(amount("20.90"));
        i.totals.gross_total = amount("130.90");
        i.totals.due = amount("130.90");
    }),
    case("BR-CO-23", |i| i.lines[0].allowances.push(LineAllowanceCharge {
        amount: amount("0.00"), base_amount: None, percentage: None,
        reason: None, reason_code: None,
    })),
    case("BR-CO-24", |i| i.lines[0].charges.push(LineAllowanceCharge {
        amount: amount("0.00"), base_amount: None, percentage: None,
        reason: None, reason_code: None,
    })),

    // ── code lists ────────────────────────────────────────────────────────
    case("BR-CL-01", |i| i.type_code = Some(Code::new("999"))),
    case("BR-CL-04", |i| i.currency = Some(Code::new("EURO"))),
    case("BR-CL-05", |i| i.vat_accounting_currency = Some(Code::new("EURO"))),
    case("BR-CL-06", |i| i.vat_point_date_code = Some(Code::new("99"))),
    // `AAI` is a real UNCL 4451 code; `AA1` is not.
    case("BR-CL-08", |i| i.notes = vec![InvoiceNote::new("x").with_subject("AA1")]),
    case("BR-CL-14", |i| i.seller.address.country = Some(Code::new("XX"))),
    case("BR-CL-16", |i| i.payment.as_mut().unwrap().means_code = Some(Code::new("999"))),
    case("BR-CL-17", |i| i.vat_breakdown[0].category = Code::new("Q")),
    case("BR-CL-18", |i| i.lines[0].vat.category = Code::new("Q")),
    case("BR-CL-19", |i| with_allowance(i, DocumentAllowanceCharge {
        reason_code: Some(Code::new("999")), ..allowance()
    })),
    case("BR-CL-22", |i| i.vat_breakdown[0].exemption_reason_code = Some(Code::new("NOPE"))),
    case("BR-CL-23", |i| i.lines[0].unit_code = Code::new("ZZZ")),

    // ── standard-only, and ours ───────────────────────────────────────────
    case("BR-CO-25", |i| { i.due_date = None; i.payment_terms = None }),
    case("EN-CURRENCY-01", |i| i.currency = Some(Code::new("XXX"))),
    case("EN-EXT-01", |i| i.extensions.advance_payments.push(en16931::AdvancePayment {
        gross: amount("119.00"),
        received_on: None,
        tax: vec![i.vat_breakdown[0].clone()],
        reference: None,
        reference_date: None,
    })),

    // ── VAT category families ─────────────────────────────────────────────
    case("BR-S-01", |i| i.vat_breakdown[0].category = Code::new("Z")),
    case("BR-S-05", |i| i.lines[0].vat.rate = Some(Percentage::ZERO)),
    case("BR-S-08", |i| i.vat_breakdown[0].taxable_amount = amount("90.00")),
    case("BR-S-09", |i| i.vat_breakdown[0].tax_amount = amount("17.50")),
    case("BR-S-10", |i| i.vat_breakdown[0].exemption_reason = Some("nope".to_owned())),

    // `B` — split payment. Only `-01` and `-02` exist in the artefacts, and
    // both are about the document as a whole rather than one group.
    case("BR-B-01", |i| {
        // The base's parties are DE and NL; split payment is Italy-only.
        i.lines[0].vat = LineVat { category: Code::new("B"), rate: Some(pct(22)) };
        i.vat_breakdown[0] = VatBreakdown {
            taxable_amount: amount("100.00"), tax_amount: amount("22.00"),
            category: Code::new("B"), rate: Some(pct(22)),
            exemption_reason: None, exemption_reason_code: None,
        };
        i.totals.vat_total = Some(amount("22.00"));
        i.totals.gross_total = amount("122.00");
        i.totals.due = amount("122.00");
    }),
    case("BR-B-02", |i| {
        // `B` and `S` cannot coexist, whatever the countries.
        i.seller.address.country = Some(Code::new("IT"));
        i.buyer.address.country = Some(Code::new("IT"));
        i.seller.vat_identifier = Some("IT123456789".to_owned());
        i.buyer.vat_identifier = Some("IT987654321".to_owned());
        let mut b = i.lines[0].clone();
        b.id = "2".to_owned();
        b.vat = LineVat { category: Code::new("B"), rate: Some(pct(22)) };
        i.lines.push(b);
        i.vat_breakdown.push(VatBreakdown {
            taxable_amount: amount("100.00"), tax_amount: amount("22.00"),
            category: Code::new("B"), rate: Some(pct(22)),
            exemption_reason: None, exemption_reason_code: None,
        });
        i.totals.line_total = amount("200.00");
        i.totals.taxable_total = amount("200.00");
        i.totals.vat_total = Some(amount("41.00"));
        i.totals.gross_total = amount("241.00");
        i.totals.due = amount("241.00");
    }),
    // ── BG-10 payee, BG-11 tax representative ─────────────────────────────
    case("BR-17", |i| i.payee = Some(Payee::default())),
    case("BR-18", |i| i.tax_representative = Some(TaxRepresentative {
        vat_identifier: Some("DE123456789".to_owned()),
        address: PostalAddress { country: Some(Code::new("DE")), ..Default::default() },
        ..Default::default()
    })),
    case("BR-19", |i| i.tax_representative = Some(TaxRepresentative {
        name: Some("Rep GmbH".to_owned()),
        vat_identifier: Some("DE123456789".to_owned()),
        address: PostalAddress::default(),
    })),
    case("BR-20", |i| i.tax_representative = Some(TaxRepresentative {
        name: Some("Rep GmbH".to_owned()),
        vat_identifier: Some("DE123456789".to_owned()),
        address: PostalAddress { city: Some("Berlin".to_owned()), ..Default::default() },
    })),
    case("BR-56", |i| i.tax_representative = Some(TaxRepresentative {
        name: Some("Rep GmbH".to_owned()),
        vat_identifier: None,
        address: PostalAddress { country: Some(Code::new("DE")), ..Default::default() },
    })),

    // ── allowance and charge reasons, under their own ids ─────────────────
    case("BR-33", |i| with_allowance(i, DocumentAllowanceCharge { reason: None, ..allowance() })),
    case("BR-38", |i| {
        i.charges.push(DocumentAllowanceCharge { reason: None, ..allowance() });
        i.totals.charge_total = Some(amount("10.00"));
        i.totals.taxable_total = amount("110.00");
        i.vat_breakdown[0].taxable_amount = amount("110.00");
        i.vat_breakdown[0].tax_amount = amount("20.90");
        i.totals.vat_total = Some(amount("20.90"));
        i.totals.gross_total = amount("130.90");
        i.totals.due = amount("130.90");
    }),
    case("BR-42", |i| i.lines[0].allowances.push(LineAllowanceCharge {
        amount: amount("0.00"), base_amount: None, percentage: None,
        reason: None, reason_code: None,
    })),
    case("BR-44", |i| i.lines[0].charges.push(LineAllowanceCharge {
        amount: amount("0.00"), base_amount: None, percentage: None,
        reason: None, reason_code: None,
    })),

    // ── remaining presence ────────────────────────────────────────────────
    case("BR-50", |i| i.payment.as_mut().unwrap().means =
        Some(PaymentMeans::CreditTransfer(vec![CreditTransfer::default()]))),
    case("BR-54", |i| i.lines[0].item.attributes.push(ItemAttribute {
        name: Some("Colour".to_owned()), value: None,
    })),
    case("BR-CO-20", |i| i.lines[0].period = Some(Period::default())),
    case("BR-IC-11", |i| {
        i.lines[0].vat = LineVat { category: Code::new("K"), rate: Some(Percentage::ZERO) };
        i.vat_breakdown[0] = VatBreakdown {
            taxable_amount: amount("100.00"), tax_amount: amount("0.00"),
            category: Code::new("K"), rate: Some(Percentage::ZERO),
            exemption_reason: Some("Intra-community supply".to_owned()),
            exemption_reason_code: None,
        };
        i.totals.vat_total = Some(amount("0.00"));
        i.totals.gross_total = amount("100.00");
        i.totals.due = amount("100.00");
        // BR-IC-11 wants a delivery date; `base()` carries one for
        // `BR-DE-TMP-32`, so this case has to take it away again.
        i.delivery = None;
        i.invoicing_period = None;
    }),
    case("BR-IC-12", |i| {
        i.lines[0].vat = LineVat { category: Code::new("K"), rate: Some(Percentage::ZERO) };
        i.vat_breakdown[0] = VatBreakdown {
            taxable_amount: amount("100.00"), tax_amount: amount("0.00"),
            category: Code::new("K"), rate: Some(Percentage::ZERO),
            exemption_reason: Some("Intra-community supply".to_owned()),
            exemption_reason_code: None,
        };
        i.totals.vat_total = Some(amount("0.00"));
        i.totals.gross_total = amount("100.00");
        i.totals.due = amount("100.00");
        // A delivery date, but no deliver-to country.
        i.delivery = Some(Delivery {
            date: Some(Date::parse("2026-06-15").unwrap()),
            ..Default::default()
        });
    }),

    // ── remaining code lists ──────────────────────────────────────────────
    // UNTDID 1153 has `AAJ`; `AA1` is not a qualifier.
    case("BR-CL-07", |i| {
        i.lines[0].object_identifier = Some(Identifier::schemed("X", "AA1"));
    }),
    case("BR-CL-10", |i| i.seller.identifiers = vec![Identifier::schemed("X", "NOPE")]),
    case("BR-CL-11", |i| i.seller.legal_registration = Some(Identifier::schemed("X", "NOPE"))),
    case("BR-CL-13", |i| i.lines[0].item.classification_identifiers =
        vec![Identifier::schemed("X", "NOPE")]),
    // BR-CL-15's artefact context is `cac:OriginCountry` — **BT-159**, the
    // item's country of origin. BT-80 (deliver-to) is `cac:Country`, which is
    // BR-CL-14's. The two rules share a message and check different terms.
    case("BR-CL-15", |i| i.lines[0].item.origin_country = Some(Code::new("XX"))),
    case("BR-CL-21", |i| i.lines[0].item.standard_identifier =
        Some(Identifier::schemed("X", "NOPE"))),
    case("BR-CL-24", |i| i.attachments.push(SupportingDocument {
        reference: en16931::DocumentReference::new("DOC-1"),
        description: None, uri: None,
        attachment: Some(en16931::Attachment::new(vec![], "image/tiff", "scan.tif").expect("valid attachment")),
    })),
    case("BR-CL-25", |i| i.seller.electronic_address = Some(Identifier::schemed("X", "NOPE"))),
    case("BR-CL-26", |i| i.delivery = Some(Delivery {
        location: Some(Identifier::schemed("X", "NOPE")),
        address: Some(PostalAddress { country: Some(Code::new("NL")), ..Default::default() }),
        ..Default::default()
    })),

    // `O` excludes everything else — BR-O-12 for lines, `-13` for allowances,
    // `-14` for charges. All three turn on an `O` breakdown being present.
    case("BR-O-13", |i| {
        i.seller.vat_identifier = None;
        i.buyer.vat_identifier = None;
        i.seller.legal_registration = Some(Identifier::schemed("HRB 12345", "0198"));
        i.lines[0].vat = LineVat { category: Code::new("O"), rate: None };
        with_allowance(i, allowance()); // an allowance still in category S
        i.vat_breakdown[0] = VatBreakdown {
            taxable_amount: amount("90.00"), tax_amount: amount("0.00"),
            category: Code::new("O"), rate: None,
            exemption_reason: Some("Not subject to VAT".to_owned()),
            exemption_reason_code: None,
        };
        i.totals.vat_total = Some(amount("0.00"));
        i.totals.gross_total = amount("90.00");
        i.totals.due = amount("90.00");
    }),
    case("BR-O-14", |i| {
        i.seller.vat_identifier = None;
        i.buyer.vat_identifier = None;
        i.seller.legal_registration = Some(Identifier::schemed("HRB 12345", "0198"));
        i.lines[0].vat = LineVat { category: Code::new("O"), rate: None };
        i.charges.push(DocumentAllowanceCharge {
            reason: Some("Fracht".into()), ..allowance() // still category S
        });
        i.totals.charge_total = Some(amount("10.00"));
        i.totals.taxable_total = amount("110.00");
        i.vat_breakdown[0] = VatBreakdown {
            taxable_amount: amount("110.00"), tax_amount: amount("0.00"),
            category: Code::new("O"), rate: None,
            exemption_reason: Some("Not subject to VAT".to_owned()),
            exemption_reason_code: None,
        };
        i.totals.vat_total = Some(amount("0.00"));
        i.totals.gross_total = amount("110.00");
        i.totals.due = amount("110.00");
    }),
    // BR-O-12 is the line variant.
    case("BR-O-12", |i| {
        i.seller.vat_identifier = None;
        i.buyer.vat_identifier = None;
        i.seller.legal_registration = Some(Identifier::schemed("HRB 12345", "0198"));
        i.vat_breakdown[0] = VatBreakdown {
            taxable_amount: amount("100.00"), tax_amount: amount("0.00"),
            category: Code::new("O"), rate: None,
            exemption_reason: Some("Not subject to VAT".to_owned()),
            exemption_reason_code: None,
        };
        i.totals.vat_total = Some(amount("0.00"));
        i.totals.gross_total = amount("100.00");
        i.totals.due = amount("100.00");
    }),

    // `O` is the awkward category: it forbids the LINE rate outright (BR-O-05,
    // "shall not contain", not "shall be 0") and excludes every other group.
    case("BR-O-05", |i| {
        i.lines[0].vat = LineVat { category: Code::new("O"), rate: Some(Percentage::ZERO) };
        i.vat_breakdown[0] = VatBreakdown {
            taxable_amount: amount("100.00"), tax_amount: amount("0.00"),
            category: Code::new("O"), rate: None,
            exemption_reason: Some("Not subject to VAT".to_owned()),
            exemption_reason_code: None,
        };
        i.totals.vat_total = Some(amount("0.00"));
        i.totals.gross_total = amount("100.00");
        i.totals.due = amount("100.00");
    }),
    case("BR-O-11", |i| {
        i.vat_breakdown.push(VatBreakdown {
            taxable_amount: amount("0.00"), tax_amount: amount("0.00"),
            category: Code::new("O"), rate: None,
            exemption_reason: Some("Not subject to VAT".to_owned()),
            exemption_reason_code: None,
        });
    }),
    case("BR-CL-20", |i| {
        i.charges.push(DocumentAllowanceCharge {
            // `ZZZ` would NOT work: it is a real UNCL 7161 code, "mutually defined".
            reason_code: Some(Code::new("999")), reason: Some("Fracht".into()), ..allowance()
        });
        i.totals.charge_total = Some(amount("10.00"));
        i.totals.taxable_total = amount("110.00");
        i.vat_breakdown[0].taxable_amount = amount("110.00");
        i.vat_breakdown[0].tax_amount = amount("20.90");
        i.totals.vat_total = Some(amount("20.90"));
        i.totals.gross_total = amount("130.90");
        i.totals.due = amount("130.90");
    }),

    // ── Peppol BIS 3.0 — only fire under that profile ─────────────────────
    peppol("PEPPOL-EN16931-R001", |i| i.business_process = None),
    peppol("PEPPOL-EN16931-R007", |i| i.business_process = Some("urn:nope".to_owned())),
    peppol("PEPPOL-EN16931-R002", |i| {
        // Two notes, and the buyer is Dutch — the DE-DE carve-out does not apply.
        i.notes = vec![InvoiceNote::new("one"), InvoiceNote::new("two")];
    }),
    // A disjunction: BT-10 *or* BT-13. Clearing only one must not fire.
    peppol("PEPPOL-EN16931-R003", |i| {
        i.buyer_reference = None;
        i.purchase_order_reference = None;
    }),
    peppol("PEPPOL-EN16931-R004", |i| {
        i.specification_id = Some("urn:cen.eu:en16931:2017".to_owned());
    }),
    peppol("PEPPOL-EN16931-R005", |i| i.vat_accounting_currency = Some(Code::new("EUR"))),
    peppol("PEPPOL-EN16931-R010", |i| i.buyer.electronic_address = None),
    peppol("PEPPOL-EN16931-R020", |i| i.seller.electronic_address = None),
    peppol("PEPPOL-EN16931-R055", |i| {
        i.totals.vat_total = Some(amount("17.10"));
        i.totals.vat_total_accounting = Some(amount("-17.10"));
    }),
    peppol("PEPPOL-EN16931-R110", |i| {
        i.invoicing_period = Some(Period {
            start: Some(Date::parse("2026-06-01").unwrap()),
            end: Some(Date::parse("2026-06-30").unwrap()),
        });
        i.lines[0].period = Some(Period {
            start: Some(Date::parse("2026-05-01").unwrap()),
            end: None,
        });
    }),
    peppol("PEPPOL-EN16931-R111", |i| {
        i.invoicing_period = Some(Period {
            start: Some(Date::parse("2026-06-01").unwrap()),
            end: Some(Date::parse("2026-06-30").unwrap()),
        });
        i.lines[0].period = Some(Period {
            start: None,
            end: Some(Date::parse("2026-07-31").unwrap()),
        });
    }),
    peppol("PEPPOL-EN16931-P0112", |i| {
        // 326 is partial billing; the buyer is Dutch, so Peppol forbids it.
        i.type_code = Some(Code::new("326"));
    }),
    peppol("PEPPOL-EN16931-CL001", |i| {
        i.attachments = vec![SupportingDocument {
            reference: DocumentReference::new("DOC-1"),
            description: None,
            uri: None,
            attachment: Some(Attachment::new(vec![1], "application/zip", "x.zip").expect("valid attachment")),
        }];
    }),
    peppol("PEPPOL-EN16931-CL008", |i| {
        // `0219` is in CEN's EAS list and not in Peppol's 94-code subset.
        i.seller.electronic_address = Some(Identifier::schemed("x", "0219"));
    }),
    peppol("PEPPOL-EN16931-P0100", |i| i.type_code = Some(Code::new("389"))),
    // `261` is a CEN credit-note code (`BR-CL-01` accepts it) that Peppol's
    // five-code credit-note list does not — which is what `P0101` is for.
    peppol("PEPPOL-EN16931-P0101", |i| i.type_code = Some(Code::new("261"))),
    peppol("PEPPOL-EN16931-CL002", |i| {
        i.allowances = vec![DocumentAllowanceCharge {
            amount: amount("10.00"),
            base_amount: Some(amount("100.00")),
            percentage: Some(pct(10)),
            vat: LineVat { category: Code::new("S"), rate: Some(pct(19)) },
            reason: Some("Rabatt".to_owned()),
            reason_code: Some(Code::new("ZZ9")),
        }];
        rebalance_with_allowance(i, "10.00");
    }),
    peppol("PEPPOL-EN16931-CL003", |i| {
        i.charges = vec![DocumentAllowanceCharge {
            amount: amount("10.00"),
            base_amount: Some(amount("100.00")),
            percentage: Some(pct(10)),
            vat: LineVat { category: Code::new("S"), rate: Some(pct(19)) },
            reason: Some("Fracht".to_owned()),
            reason_code: Some(Code::new("ZZ9")),
        }];
        // A charge raises BT-108 and everything below it.
        i.totals.charge_total = Some(amount("10.00"));
        i.totals.taxable_total = amount("110.00");
        i.totals.vat_total = Some(amount("20.90"));
        i.totals.gross_total = amount("130.90");
        i.totals.due = amount("130.90");
        i.vat_breakdown = vec![VatBreakdown {
            taxable_amount: amount("110.00"),
            tax_amount: amount("20.90"),
            category: Code::new("S"),
            rate: Some(pct(19)),
            exemption_reason: None,
            exemption_reason_code: None,
        }];
    }),
    peppol("PEPPOL-EN16931-CL006", |i| {
        i.vat_point_date_code = Some(Code::new("99"));
    }),
    peppol("PEPPOL-EN16931-R040", |i| {
        i.allowances = vec![DocumentAllowanceCharge {
            amount: amount("5.00"),
            base_amount: Some(amount("100.00")),
            percentage: Some(pct(10)),
            vat: LineVat { category: Code::new("S"), rate: Some(pct(19)) },
            reason: Some("Rabatt".to_owned()),
            reason_code: None,
        }];
        rebalance_with_allowance(i, "5.00");
    }),
    peppol("PEPPOL-EN16931-R041", |i| {
        i.allowances = vec![DocumentAllowanceCharge {
            amount: amount("10.00"),
            base_amount: None,
            percentage: Some(pct(10)),
            vat: LineVat { category: Code::new("S"), rate: Some(pct(19)) },
            reason: Some("Rabatt".to_owned()),
            reason_code: None,
        }];
        rebalance_with_allowance(i, "10.00");
    }),
    peppol("PEPPOL-EN16931-R042", |i| {
        i.allowances = vec![DocumentAllowanceCharge {
            amount: amount("10.00"),
            base_amount: Some(amount("100.00")),
            percentage: None,
            vat: LineVat { category: Code::new("S"), rate: Some(pct(19)) },
            reason: Some("Rabatt".to_owned()),
            reason_code: None,
        }];
        rebalance_with_allowance(i, "10.00");
    }),
    peppol("PEPPOL-EN16931-R046", |i| {
        // R046 is exact: gross − discount must land on the net price.
        i.lines[0].price.gross_price = Some(UnitPriceAmount::new(dec!(100)));
        i.lines[0].price.price_discount = Some(UnitPriceAmount::new(dec!(1)));
    }),
    peppol("PEPPOL-EN16931-R120", |i| {
        i.lines[0].price.net_price = UnitPriceAmount::new(dec!(1));
    }),
    peppol("PEPPOL-EN16931-R121", |i| {
        i.lines[0].price.base_quantity = Some(Quantity::new(dec!(0)));
    }),
    peppol("PEPPOL-EN16931-R130", |i| {
        i.lines[0].price.base_quantity = Some(Quantity::new(dec!(1)));
        i.lines[0].price.base_quantity_code = Some(Code::new("KGM"));
    }),
    peppol("PEPPOL-EN16931-R061", |i| {
        i.payment = Some(PaymentInstructions {
            means_code: Some(Code::new("59")),
            means: Some(PaymentMeans::DirectDebit(DirectDebit {
                mandate_reference: None,
                creditor_identifier: Some("DE98ZZZ09999999999".to_owned()),
                debited_account: Some("DE89370400440532013000".to_owned()),
            })),
            ..Default::default()
        });
    }),

    // ── XRechnung — only fire under that profile ──────────────────────────
    xr("BR-DE-16", |i| {
        i.seller.vat_identifier = None;
        i.seller.tax_registration = None;
    }),
    xr("BR-DE-19", |i| {
        i.payment = Some(PaymentInstructions {
            means_code: Some(Code::new("58")),
            means: Some(PaymentMeans::CreditTransfer(vec![CreditTransfer {
                account_identifier: Some("DE00000000000000000000".to_owned()),
                ..Default::default()
            }])),
            ..Default::default()
        });
    }),
    xr("BR-DE-20", |i| {
        i.payment = Some(PaymentInstructions {
            means_code: Some(Code::new("59")),
            means: Some(PaymentMeans::DirectDebit(DirectDebit {
                mandate_reference: Some("MND-1".to_owned()),
                creditor_identifier: Some("DE98ZZZ09999999999".to_owned()),
                debited_account: Some("DE00000000000000000000".to_owned()),
            })),
            ..Default::default()
        });
    }),
    xr("BR-DE-23-a", |i| {
        i.payment = Some(PaymentInstructions {
            means_code: Some(Code::new("58")),
            means: None,
            ..Default::default()
        });
    }),
    xr("BR-DE-24-a", |i| {
        i.payment = Some(PaymentInstructions {
            means_code: Some(Code::new("48")),
            means: None,
            ..Default::default()
        });
    }),
    xr("BR-DE-25-a", |i| {
        i.payment = Some(PaymentInstructions {
            means_code: Some(Code::new("59")),
            means: None,
            ..Default::default()
        });
    }),
    xr("BR-DE-26", |i| {
        i.type_code = Some(Code::new("384"));
        i.preceding_invoices.clear();
    }),
    xr("BR-DE-2", |i| i.seller.contact = Contact::default()),
    xr("BR-DE-10", |i| {
        i.delivery = Some(Delivery {
            address: Some(PostalAddress {
                post_code: Some("10115".to_owned()),
                country: Some(Code::new("DE")),
                ..Default::default()
            }),
            ..Default::default()
        });
    }),
    xr("BR-DE-11", |i| {
        i.delivery = Some(Delivery {
            address: Some(PostalAddress {
                city: Some("Berlin".to_owned()),
                country: Some(Code::new("DE")),
                ..Default::default()
            }),
            ..Default::default()
        });
    }),
    // `PROZENT=3` is not `PROZENT=3.00`: the two decimals are exact.
    xr("BR-DE-18", |i| {
        i.payment_terms = Some("#SKONTO#TAGE=14#PROZENT=3#".to_owned());
    }),
    xr("BR-DE-22", |i| {
        let doc = |name: &str| SupportingDocument {
            reference: DocumentReference::new("DOC"),
            description: None,
            uri: None,
            attachment: Some(Attachment::new(vec![1], "application/pdf", name).expect("valid attachment")),
        };
        i.attachments = vec![doc("same.pdf"), doc("same.pdf")];
    }),
    xr("BR-TMP-2", |i| {
        i.attachments = vec![SupportingDocument {
            reference: DocumentReference::new("DOC"),
            description: None,
            uri: Some("not-a-url".to_owned()),
            attachment: None,
        }];
    }),
    xr("BR-DE-TMP-32", |i| {
        i.delivery = None;
        i.invoicing_period = None;
        i.lines[0].period = None;
    }),

    // ── XRechnung Extension — mutations of `dex_base()` ───────────────────
    // `application/xml` is what the Extension *adds*; `application/zip` is
    // outside both lists.
    dex("BR-DEX-01", |i| {
        i.attachments = vec![SupportingDocument {
            reference: DocumentReference::new("DOC-1"),
            description: None,
            uri: None,
            attachment: Some(Attachment::new(vec![1], "application/zip", "x.zip").expect("valid attachment")),
        }];
    }),
    dex("BR-DEX-02", |i| {
        // BT-131 is 100.00; the sub-lines total 90.00.
        i.extensions.sub_invoice_lines = vec![(0, vec![sub_line("90.00")])];
    }),
    dex("BR-DEX-03", |i| {
        let mut s = sub_line("100.00");
        s.vat = None;
        i.extensions.sub_invoice_lines = vec![(0, vec![s])];
    }),
    dex("BR-DEX-04", |i| {
        i.seller.identifiers = vec![Identifier::schemed("X", "NOPE")];
    }),
    dex("BR-DEX-05", |i| {
        i.seller.legal_registration = Some(Identifier::schemed("X", "NOPE"));
    }),
    dex("BR-DEX-06", |i| {
        i.lines[0].item.standard_identifier = Some(Identifier::schemed("X", "NOPE"));
    }),
    dex("BR-DEX-07", |i| {
        i.seller.electronic_address = Some(Identifier::schemed("x", "NOPE"));
    }),
    dex("BR-DEX-08", |i| {
        i.delivery = Some(Delivery {
            date: Some(Date::parse("2026-06-30").unwrap()),
            location: Some(Identifier::schemed("X", "NOPE")),
            ..Default::default()
        });
    }),
    // A third-party payment changes BT-115, and BR-DEX-09 is the equation that
    // knows it. Leaving BT-115 alone is what makes it fire.
    dex("BR-DEX-09", |i| {
        i.extensions.third_party_payments = vec![third_party("10.00")];
    }),
    dex("BR-DEX-10", |i| {
        let mut p = third_party("10.00");
        p.payment_type = None;
        i.extensions.third_party_payments = vec![p];
        i.totals.due = amount("129.00");
    }),
    dex("BR-DEX-11", |i| {
        let mut p = third_party("10.00");
        p.amount = None;
        i.extensions.third_party_payments = vec![p];
    }),
    dex("BR-DEX-12", |i| {
        let mut p = third_party("10.00");
        p.description = None;
        i.extensions.third_party_payments = vec![p];
        i.totals.due = amount("129.00");
    }),

    // ── XRechnung CVD — mutations of `cvd_base()` ─────────────────────────
    cvd("BR-DE-CVD-01", |i| i.contract_reference = None),
    cvd("BR-DE-CVD-02", |i| i.tender_reference = None),
    cvd("BR-DE-CVD-03", |i| {
        i.lines[0].item.classification_identifiers.clear();
        i.lines[0].item.attributes.clear();
    }),
    // `X9` is not a vehicle category.
    cvd("BR-DE-CVD-04", |i| {
        i.lines[0].item.classification_identifiers = vec![Identifier::schemed("X9", "CVD")];
    }),
    cvd("BR-DE-CVD-05", |i| {
        i.lines[0].item.attributes[0].value = Some("sparkling".to_owned());
    }),
    cvd("BR-DE-CVD-06-a", |i| i.lines[0].item.attributes.clear()),
    cvd("BR-DE-CVD-06-b", |i| {
        i.lines[0].item.classification_identifiers.clear();
    }),
    // `ZZZ` *is* in UNTDID 7143 ("mutually defined") — the third time that has
    // caught a fixture in this suite. `XX9` is genuinely absent.
    cvd("BR-TMP-CVD-01", |i| {
        i.lines[0]
            .item
            .classification_identifiers
            .push(Identifier::schemed("x", "XX9"));
    }),
    // Ours: BT-90 present but not a well-formed EPC AT-02 identifier.
    xr("EN-SEPA-01", |i| {
        i.payment = Some(PaymentInstructions {
            means_code: Some(Code::new("59")),
            means: Some(PaymentMeans::DirectDebit(DirectDebit {
                mandate_reference: Some("MND-1".to_owned()),
                creditor_identifier: Some("not a creditor id".to_owned()),
                debited_account: Some("DE89370400440532013000".to_owned()),
            })),
            ..Default::default()
        });
    }),
    xr("BR-DE-27", |i| i.seller.contact.phone = Some("12".to_owned())),
    xr("BR-DE-28", |i| i.seller.contact.email = Some("no-at-sign".to_owned())),
    xr("BR-DE-30", |i| {
        i.payment = Some(PaymentInstructions {
            means_code: Some(Code::new("59")),
            means: Some(PaymentMeans::DirectDebit(DirectDebit {
                mandate_reference: Some("MND-1".to_owned()),
                creditor_identifier: None,
                debited_account: Some("DE89370400440532013000".to_owned()),
            })),
            ..Default::default()
        });
    }),
    xr("BR-DE-31", |i| {
        i.payment = Some(PaymentInstructions {
            means_code: Some(Code::new("59")),
            means: Some(PaymentMeans::DirectDebit(DirectDebit {
                mandate_reference: Some("MND-1".to_owned()),
                creditor_identifier: Some("DE98ZZZ09999999999".to_owned()),
                debited_account: None,
            })),
            ..Default::default()
        });
    }),
    ]
}

/// One `BG-DEX-01` sub-line carrying `net`, with its VAT information.
fn sub_line(net: &str) -> en16931::SubInvoiceLine {
    let mut line = base().lines[0].clone();
    line.net_amount = amount(net);
    en16931::SubInvoiceLine {
        vat: Some(line.vat.clone()),
        line,
        children: vec![],
    }
}

/// One `BG-DEX-09` third-party payment of `amount`, fully populated.
fn third_party(a: &str) -> en16931::ThirdPartyPayment {
    en16931::ThirdPartyPayment {
        payment_type: Some("DiGA".to_owned()),
        amount: Some(amount(a)),
        description: Some("Krankenkassenanteil".to_owned()),
    }
}

/// A document-level allowance changes BT-107 and every total below it.
///
/// Without this the `R04x` cases would fire `BR-CO-11` and the totals chain too,
/// and prove nothing about the rule they name.
fn rebalance_with_allowance(i: &mut Invoice, allowance: &str) {
    let a = amount(allowance);
    i.totals.allowance_total = Some(a);
    i.totals.taxable_total = i.totals.line_total.checked_sub(a).unwrap();
    // 19 % of the new taxable total, rounded half-up like every other figure here.
    let vat = InvoiceAmount::from_decimal_exact(
        (i.totals.taxable_total.into_decimal() * dec!(0.19)).round_dp(2),
    )
    .unwrap();
    i.totals.vat_total = Some(vat);
    i.totals.gross_total = i.totals.taxable_total.checked_add(vat).unwrap();
    i.totals.due = i.totals.gross_total;
    i.vat_breakdown = vec![VatBreakdown {
        taxable_amount: i.totals.taxable_total,
        tax_amount: vat,
        category: Code::new("S"),
        rate: Some(pct(19)),
        exemption_reason: None,
        exemption_reason_code: None,
    }];
}

/// The eight `P010x` rules pin a VATEX code to one category. One case each,
/// generated from the same table the rules use, so a new pairing cannot be added
/// without a fixture appearing for it.
const VATEX_CASES: &[(&str, &str, &str)] = &[
    ("PEPPOL-EN16931-P0104", "VATEX-EU-G", "G"),
    ("PEPPOL-EN16931-P0105", "VATEX-EU-O", "O"),
    ("PEPPOL-EN16931-P0106", "VATEX-EU-IC", "K"),
    ("PEPPOL-EN16931-P0107", "VATEX-EU-AE", "AE"),
    ("PEPPOL-EN16931-P0108", "VATEX-EU-D", "E"),
    ("PEPPOL-EN16931-P0109", "VATEX-EU-F", "E"),
    ("PEPPOL-EN16931-P0110", "VATEX-EU-I", "E"),
    ("PEPPOL-EN16931-P0111", "VATEX-EU-J", "E"),
];

/// One invoice per `P010x`: a document that is *otherwise valid* in some
/// exempt category, carrying the VATEX code that implies a **different** one.
///
/// Built from [`FAMILIES`] rather than by hand, so the document satisfies the
/// nine category families and the only thing wrong with it is the pairing the
/// rule exists to catch.
fn vatex_cases() -> Vec<(&'static str, Invoice)> {
    VATEX_CASES
        .iter()
        .map(|(rule, vatex, implied)| {
            // Any exempt category other than the one the code implies. `E` and
            // `G` are both reason-requiring and zero-rated, so one of the two is
            // always available as the wrong answer.
            let wrong = if *implied == "E" { "G" } else { "E" };
            let fam = FAMILIES
                .iter()
                .find(|f| f.code == wrong)
                .expect("FAMILIES covers E and G");
            let mut inv = fam.valid();
            inv.vat_breakdown[0].exemption_reason_code = Some(Code::new(*vatex));
            (*rule, inv)
        })
        .collect()
}

const _: () = {
    // Keep the two tables in step: the rules' own table is the source of truth.
    assert!(VATEX_CASES.len() == 8);
};
/// One of §6.4.3's nine tables, as a fixture.
///
/// The rules are table-driven, so the corpus is too: nine categories × five rows
/// from one description each, rather than forty-five hand-written mutations that
/// would drift apart.
struct Family {
    /// The UNCL 5305 code.
    code: &'static str,
    /// The rule-id prefix, which is **not** always the code — `K` is `BR-IC`,
    /// `L` is `BR-AF`, `M` is `BR-AG`.
    prefix: &'static str,
    /// A rate this category accepts.
    rate: Option<Percentage>,
    /// Whether `-10` requires an exemption reason rather than forbidding one.
    needs_reason: bool,
    /// Whether the category levies tax, so `-09` derives rather than zeroes.
    taxed: bool,
}

#[rustfmt::skip]
const FAMILIES: &[Family] = &[
    Family { code: "S",  prefix: "BR-S",  rate: Some(Percentage::new(dec!(19))), needs_reason: false, taxed: true },
    Family { code: "L",  prefix: "BR-AF", rate: Some(Percentage::new(dec!(7))),  needs_reason: false, taxed: true },
    Family { code: "M",  prefix: "BR-AG", rate: Some(Percentage::new(dec!(10))), needs_reason: false, taxed: true },
    Family { code: "Z",  prefix: "BR-Z",  rate: Some(Percentage::ZERO),          needs_reason: false, taxed: false },
    Family { code: "E",  prefix: "BR-E",  rate: Some(Percentage::ZERO),          needs_reason: true,  taxed: false },
    Family { code: "AE", prefix: "BR-AE", rate: Some(Percentage::ZERO),          needs_reason: true,  taxed: false },
    Family { code: "K",  prefix: "BR-IC", rate: Some(Percentage::ZERO),          needs_reason: true,  taxed: false },
    Family { code: "G",  prefix: "BR-G",  rate: Some(Percentage::ZERO),          needs_reason: true,  taxed: false },
    // `O` states no rate at all: BR-O-05 says "shall not contain", where every
    // other zero-tax category says "shall be 0".
    Family { code: "O",  prefix: "BR-O",  rate: None,                            needs_reason: true,  taxed: false },
];

impl Family {
    /// `(BT-116, BT-117, BT-112)` for a single 100.00 line.
    fn amounts(&self) -> (&'static str, &'static str, &'static str) {
        match (self.taxed, self.code) {
            (false, _) => ("100.00", "0.00", "100.00"),
            (true, "S") => ("100.00", "19.00", "119.00"),
            (true, "L") => ("100.00", "7.00", "107.00"),
            (true, _) => ("100.00", "10.00", "110.00"),
        }
    }

    /// BT-117 and BT-112 for a 110.00 base, used by the `-04` fixtures.
    fn tax_on_110(&self) -> &'static str {
        match self.code {
            "S" => "20.90",
            "L" => "7.70",
            _ => "11.00",
        }
    }
    fn gross_on_110(&self) -> &'static str {
        match self.code {
            "S" => "130.90",
            "L" => "117.70",
            _ => "121.00",
        }
    }

    /// A rate this category rejects — the `-05` / `-06` / `-07` mutation.
    fn bad_rate(&self) -> Option<Percentage> {
        match self.code {
            "O" => Some(Percentage::ZERO),                // "shall not contain"
            "S" => Some(Percentage::ZERO),                // "greater than zero"
            "L" | "M" => Some(Percentage::new(dec!(-1))), // "0 or greater"
            _ => Some(Percentage::new(dec!(19))),         // "shall be 0"
        }
    }

    /// A wholly valid invoice in this category.
    fn valid(&self) -> Invoice {
        let mut inv = base();
        if self.code == "K" {
            // BR-IC-11 / -12: an intra-community supply must evidence where and
            // when the goods went, or the zero-rating cannot be substantiated.
            inv.delivery = Some(Delivery {
                date: Some(Date::parse("2026-06-15").unwrap()),
                address: Some(PostalAddress {
                    country: Some(Code::new("NL")),
                    ..Default::default()
                }),
                ..Default::default()
            });
        }
        if self.code == "O" {
            // BR-O-02/03/04 forbid BT-31, BT-63 and BT-48 outright, while
            // BR-CO-26 still requires one of BT-29 / BT-30 / BT-31 — so an
            // out-of-scope seller identifies itself by registration.
            inv.seller.vat_identifier = None;
            inv.buyer.vat_identifier = None;
            inv.seller.legal_registration = Some(Identifier::schemed("HRB 12345", "0198"));
        }
        let (net, tax, gross) = self.amounts();
        inv.lines[0].vat = LineVat {
            category: Code::new(self.code),
            rate: self.rate,
        };
        inv.vat_breakdown[0] = VatBreakdown {
            taxable_amount: amount(net),
            tax_amount: amount(tax),
            category: Code::new(self.code),
            rate: self.rate,
            exemption_reason: self.needs_reason.then(|| "Reason A".to_owned()),
            exemption_reason_code: None,
        };
        inv.totals.vat_total = Some(amount(tax));
        inv.totals.gross_total = amount(gross);
        inv.totals.due = amount(gross);
        inv
    }
}

/// Every family × every row that family has.
fn family_cases() -> Vec<(String, Invoice)> {
    let mut out = Vec::new();
    for fam in FAMILIES {
        let id = |row: &str| format!("{}-{row}", fam.prefix);

        // `-01`: content in this category, but the breakdown names another.
        let mut inv = fam.valid();
        inv.vat_breakdown[0].category = Code::new(if fam.code == "Z" { "E" } else { "Z" });
        inv.vat_breakdown[0].rate = Some(Percentage::ZERO);
        inv.vat_breakdown[0].tax_amount = amount("0.00");
        inv.vat_breakdown[0].exemption_reason = (fam.code == "Z").then(|| "Reason A".to_owned());
        inv.totals.vat_total = Some(amount("0.00"));
        inv.totals.gross_total = amount("100.00");
        inv.totals.due = amount("100.00");
        out.push((id("01"), inv));

        // `-05`: a rate the category does not permit.
        let mut inv = fam.valid();
        inv.lines[0].vat.rate = match fam.code {
            "O" => Some(Percentage::ZERO),                // "shall not contain"
            "S" => Some(Percentage::ZERO),                // "greater than zero"
            "L" | "M" => Some(Percentage::new(dec!(-1))), // "0 or greater"
            _ => Some(Percentage::new(dec!(19))),         // "shall be 0"
        };
        out.push((id("05"), inv));

        // `-02` / `-03` / `-04`: the tax identifiers the category requires.
        // `O` is the inverse — it forbids them — so its mutation adds one.
        for (row, ctx) in [("02", 0usize), ("03", 1), ("04", 2)] {
            let mut inv = fam.valid();
            // Put the category in the context the row is about.
            match ctx {
                1 => {
                    let mut a = allowance();
                    a.vat = LineVat {
                        category: Code::new(fam.code),
                        rate: fam.rate,
                    };
                    with_allowance(&mut inv, a);
                    inv.vat_breakdown[0].category = Code::new(fam.code);
                    inv.vat_breakdown[0].rate = fam.rate;
                    inv.vat_breakdown[0].taxable_amount = amount("90.00");
                    if !fam.taxed {
                        inv.vat_breakdown[0].tax_amount = amount("0.00");
                        inv.totals.vat_total = Some(amount("0.00"));
                        inv.totals.gross_total = amount("90.00");
                        inv.totals.due = amount("90.00");
                    }
                }
                2 => {
                    let mut c = allowance();
                    c.vat = LineVat {
                        category: Code::new(fam.code),
                        rate: fam.rate,
                    };
                    c.reason = Some("Fracht".to_owned());
                    inv.charges.push(c);
                    inv.totals.charge_total = Some(amount("10.00"));
                    inv.totals.taxable_total = amount("110.00");
                    inv.vat_breakdown[0].taxable_amount = amount("110.00");
                    let (t, g) = if fam.taxed {
                        (fam.tax_on_110(), fam.gross_on_110())
                    } else {
                        ("0.00", "110.00")
                    };
                    inv.vat_breakdown[0].tax_amount = amount(t);
                    inv.totals.vat_total = Some(amount(t));
                    inv.totals.gross_total = amount(g);
                    inv.totals.due = amount(g);
                }
                _ => {}
            }
            if fam.code == "O" {
                // The prohibition: stating BT-31 breaks it.
                inv.seller.vat_identifier = Some("DE123456789".to_owned());
            } else {
                inv.seller.vat_identifier = None;
                inv.seller.tax_registration = None;
                inv.tax_representative = None;
                inv.buyer.vat_identifier = None;
                inv.buyer.legal_registration = None;
                // BR-CO-26 still needs the seller identifiable.
                inv.seller.legal_registration = Some(Identifier::schemed("HRB 12345", "0198"));
            }
            out.push((id(row), inv));
        }

        // `-06` / `-07`: the same rate rule, on an allowance and on a charge.
        for (row, is_charge) in [("06", false), ("07", true)] {
            let mut inv = fam.valid();
            let bad_rate = fam.bad_rate();
            let mut ac = allowance();
            ac.vat = LineVat {
                category: Code::new(fam.code),
                rate: bad_rate,
            };
            if is_charge {
                ac.reason = Some("Fracht".to_owned());
                inv.charges.push(ac);
                inv.totals.charge_total = Some(amount("10.00"));
            } else {
                inv.allowances.push(ac);
                inv.totals.allowance_total = Some(amount("10.00"));
            }
            out.push((id(row), inv));
        }

        // `-08`: the taxable amount does not equal the lines in this category.
        let mut inv = fam.valid();
        inv.vat_breakdown[0].taxable_amount = amount("42.00");
        out.push((id("08"), inv));

        // `-09`: the tax amount does not follow.
        let mut inv = fam.valid();
        inv.vat_breakdown[0].tax_amount = if fam.taxed {
            amount("42.00")
        } else {
            amount("1.00")
        };
        out.push((id("09"), inv));

        // `-10`: the exemption reason, in whichever direction applies.
        let mut inv = fam.valid();
        inv.vat_breakdown[0].exemption_reason = (!fam.needs_reason).then(|| "unwanted".to_owned());
        out.push((id("10"), inv));
    }
    out
}

/// Rules with no case, each with the reason.
///
/// **Every entry is a rule the type system retires** — there is no document
/// state that can make it fire, so a fixture is not merely missing but
/// impossible. Every rule that *can* fire has one.
///
/// The meta-test fails if a rule is uncovered and absent here, **and** if a rule
/// is listed here but has since been covered. So this can only shrink — and it
/// has reached its floor.
/// Why a registered rule has no fixture.
///
/// An explicit tag rather than a substring of the prose: classifying a
/// disposition by grepping its own justification is how "no state can make this
/// fire" and "we did not get to it" end up in the same bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Why {
    /// The model's types make the failure unrepresentable. Nothing to test.
    Retired,
    /// Nobody can check it — CEN's own Schematron binds it to `true()`.
    /// See `undecidable!` in `src/validation/rules/structural.rs`.
    Undecidable,
}

/// Registered rules with no fixture, each with its disposition and a reason.
///
/// Only [`Why::Retired`] and [`Why::Undecidable`] exist, and both mean *no
/// validator could do better*. There is deliberately no `Todo` variant: an
/// unimplemented rule must be absent from the registry, where the artefact-gap
/// count sees it, not parked here where it looks handled.
const UNCOVERED: &[(&str, Why, &str)] = &[
    // Retired by the type system — there is no state that makes them fire.
    ("BR-12", Why::Retired, "BT-106 is non-Option"),
    ("BR-13", Why::Retired, "BT-109 is non-Option"),
    ("BR-14", Why::Retired, "BT-112 is non-Option"),
    ("BR-15", Why::Retired, "BT-115 is non-Option"),
    ("BR-22", Why::Retired, "BT-129 is non-Option"),
    ("BR-24", Why::Retired, "BT-131 is non-Option"),
    ("BR-26", Why::Retired, "BT-146 is non-Option"),
    ("BR-31", Why::Retired, "BT-92 is non-Option"),
    ("BR-32", Why::Retired, "BT-95 is non-Option"),
    ("BR-36", Why::Retired, "BT-99 is non-Option"),
    ("BR-37", Why::Retired, "BT-102 is non-Option"),
    ("BR-41", Why::Retired, "BT-136 is non-Option"),
    ("BR-43", Why::Retired, "BT-141 is non-Option"),
    ("BR-45", Why::Retired, "BT-116 is non-Option"),
    ("BR-46", Why::Retired, "BT-117 is non-Option"),
    // The BR-DEC family: a third decimal is not representable.
    (
        "BR-DEC-01",
        Why::Retired,
        "InvoiceAmount is i64 minor units",
    ),
    (
        "BR-DEC-02",
        Why::Retired,
        "InvoiceAmount is i64 minor units",
    ),
    (
        "BR-DEC-05",
        Why::Retired,
        "InvoiceAmount is i64 minor units",
    ),
    (
        "BR-DEC-06",
        Why::Retired,
        "InvoiceAmount is i64 minor units",
    ),
    (
        "BR-DEC-09",
        Why::Retired,
        "InvoiceAmount is i64 minor units",
    ),
    (
        "BR-DEC-10",
        Why::Retired,
        "InvoiceAmount is i64 minor units",
    ),
    (
        "BR-DEC-11",
        Why::Retired,
        "InvoiceAmount is i64 minor units",
    ),
    (
        "BR-DEC-12",
        Why::Retired,
        "InvoiceAmount is i64 minor units",
    ),
    (
        "BR-DEC-13",
        Why::Retired,
        "InvoiceAmount is i64 minor units",
    ),
    (
        "BR-DEC-14",
        Why::Retired,
        "InvoiceAmount is i64 minor units",
    ),
    (
        "BR-DEC-15",
        Why::Retired,
        "InvoiceAmount is i64 minor units",
    ),
    (
        "BR-DEC-16",
        Why::Retired,
        "InvoiceAmount is i64 minor units",
    ),
    (
        "BR-DEC-17",
        Why::Retired,
        "InvoiceAmount is i64 minor units",
    ),
    (
        "BR-DEC-18",
        Why::Retired,
        "InvoiceAmount is i64 minor units",
    ),
    (
        "BR-DEC-19",
        Why::Retired,
        "InvoiceAmount is i64 minor units",
    ),
    (
        "BR-DEC-20",
        Why::Retired,
        "InvoiceAmount is i64 minor units",
    ),
    (
        "BR-DEC-23",
        Why::Retired,
        "InvoiceAmount is i64 minor units",
    ),
    (
        "BR-DEC-24",
        Why::Retired,
        "InvoiceAmount is i64 minor units",
    ),
    (
        "BR-DEC-25",
        Why::Retired,
        "InvoiceAmount is i64 minor units",
    ),
    (
        "BR-DEC-27",
        Why::Retired,
        "InvoiceAmount is i64 minor units",
    ),
    (
        "BR-DEC-28",
        Why::Retired,
        "InvoiceAmount is i64 minor units",
    ),
    (
        "BR-CL-03",
        Why::Retired,
        "one currency per document; amounts carry no @currencyID",
    ),
    // XRechnung: the `PaymentMeans` enum makes "the other two groups shall not
    // be present" unrepresentable. The `-a` halves are real rules and covered.
    (
        "BR-DEX-13",
        Why::Retired,
        "InvoiceAmount is i64 minor units",
    ),
    ("BR-DEX-14", Why::Retired, "one currency per document"),
    ("BR-DE-23-b", Why::Retired, "PaymentMeans is an enum"),
    ("BR-DE-24-b", Why::Retired, "PaymentMeans is an enum"),
    ("BR-DE-25-b", Why::Retired, "PaymentMeans is an enum"),
    // Peppol rules the model retires — each one's reason is on the rule itself.
    (
        "PEPPOL-EN16931-F001",
        Why::Retired,
        "Date parses only YYYY-MM-DD",
    ),
    (
        "PEPPOL-EN16931-R008",
        Why::Retired,
        "no empty elements in a struct",
    ),
    (
        "PEPPOL-EN16931-R043",
        Why::Retired,
        "allowances and charges are separate Vecs",
    ),
    (
        "PEPPOL-EN16931-R044",
        Why::Retired,
        "BG-29 has no price-level charge field",
    ),
    (
        "PEPPOL-EN16931-R051",
        Why::Retired,
        "one currency per document",
    ),
    (
        "PEPPOL-EN16931-CL007",
        Why::Retired,
        "no per-amount @currencyID",
    ),
    (
        "PEPPOL-EN16931-R053",
        Why::Retired,
        "vat_breakdown is one Vec",
    ),
    (
        "PEPPOL-EN16931-R054",
        Why::Retired,
        "BT-111 is one Option field",
    ),
    (
        "PEPPOL-EN16931-R080",
        Why::Retired,
        "BT-11 is Option<DocumentReference>",
    ),
    (
        "PEPPOL-EN16931-R100",
        Why::Retired,
        "BT-128 is Option<Identifier>",
    ),
    (
        "PEPPOL-EN16931-R101",
        Why::Retired,
        "the line's only reference is BT-128",
    ),
    // Undecidable — and CEN's own UBL binding is `value="true()"` for all four,
    // so registering them without a check *matches* the artefacts rather than
    // falling short of them. See `structural::undecidable!`.
    (
        "BR-CO-05",
        Why::Undecidable,
        "CEN binds it to true(): code-vs-free-text equivalence",
    ),
    (
        "BR-CO-06",
        Why::Undecidable,
        "CEN binds it to true(): code-vs-free-text equivalence",
    ),
    (
        "BR-CO-07",
        Why::Undecidable,
        "CEN binds it to true(): code-vs-free-text equivalence",
    ),
    (
        "BR-CO-08",
        Why::Undecidable,
        "CEN binds it to true(): code-vs-free-text equivalence",
    ),
    // The remaining eight category families follow BR-S-* exactly; the shared
    // checkers are exercised by the BR-S-* and zero-tax cases below.
];

/// Every case makes its rule fire, and the base document makes none fire.
#[test]
fn every_case_isolates_its_rule() {
    let clean = validate(&base());
    assert!(
        clean.is_valid() && clean.warnings().count() == 0,
        "the base document must satisfy every rule, or a case proves nothing:\n{clean}"
    );
    // …and under every shipped profile, since cases are checked against core.
    //
    // Each profile gets a base declaring **its own** BT-24. A document names one
    // specification identifier and one only, so "the same bytes are valid under
    // every profile" was never a coherent requirement — `PEPPOL-EN16931-R004`
    // makes that explicit by rejecting a Peppol document that declares
    // XRechnung's identifier, which is exactly right.
    for p in profiles::ALL {
        let mut doc = if p.id == profiles::XRECHNUNG_CVD.id {
            cvd_base()
        } else {
            base()
        };
        doc.specification_id = Some(p.specification_id.to_owned());
        let r = p.validate(&doc);
        assert!(r.is_valid(), "base is not valid under {}:\n{r}", p.id);
    }

    for c in cases() {
        let mut inv = c.start();
        (c.mutate)(&mut inv);
        let report = c.run(&inv);
        assert!(
            report.has(c.rule),
            "case for {} did not make it fire:\n{report}",
            c.rule
        );
    }
}

/// Every family, every row. Nine categories × five rows, from one table.
#[test]
fn every_vat_category_family_fires_every_row() {
    for fam in FAMILIES {
        let report = validate(&fam.valid());
        assert!(
            report.is_valid(),
            "the {} fixture must be valid before it is mutated:\n{report}",
            fam.code
        );
    }
    for (rule, inv) in family_cases() {
        let report = validate(&inv);
        assert!(report.has(&rule), "{rule} did not fire:\n{report}");
    }
    // The `P010x` pairings are Peppol's, so they are checked under Peppol.
    for (rule, inv) in vatex_cases() {
        let report = profiles::PEPPOL_BIS_3.validate(&inv);
        assert!(report.has(rule), "{rule} did not fire:\n{report}");
    }
}

/// **The gate.** Every registered rule is either covered or explicitly listed.
#[test]
fn coverage_is_complete_or_explicitly_declared() {
    let covered: BTreeSet<String> = cases()
        .iter()
        .map(|c| c.rule.to_ascii_uppercase())
        .chain(
            family_cases()
                .into_iter()
                .map(|(r, _)| r.to_ascii_uppercase()),
        )
        .chain(
            vatex_cases()
                .into_iter()
                .map(|(r, _)| r.to_ascii_uppercase()),
        )
        .collect();

    let declared: BTreeSet<String> = UNCOVERED
        .iter()
        .map(|(id, _, _)| (*id).to_ascii_uppercase())
        .collect();

    // Every rule this crate can ever report, not just the core set: a profile's
    // `extra_rules` are rules too, and leaving them out of the gate is how 38
    // Peppol rules got written without a single fixture between them.
    let mut registry: Vec<&'static Rule> = en16931::validation::rules::CORE.to_vec();
    for p in profiles::ALL {
        for r in p.extra_rules {
            if !registry.iter().any(|q| q.id == r.id) {
                registry.push(r);
            }
        }
    }
    let mut missing = Vec::new();
    let mut stale = Vec::new();

    for r in &registry {
        let id = r.id.as_str().to_ascii_uppercase();
        let is_covered = covered.contains(&id);
        let is_declared = declared.contains(&id);
        if !is_covered && !is_declared {
            missing.push(r.id.as_str());
        }
        if is_covered && is_declared {
            stale.push(r.id.as_str());
        }
    }

    assert!(
        missing.is_empty(),
        "{} registered rule(s) have no fixture and are not declared in UNCOVERED.\n\
         A rule nobody has seen fire may be inverted, unreachable, or checking the \
         wrong field — the suite would be green either way.\nAdd a case, or declare \
         it with a reason:\n  {}",
        missing.len(),
        missing.join("\n  ")
    );
    assert!(
        stale.is_empty(),
        "{} rule(s) are covered but still listed in UNCOVERED — remove them:\n  {}",
        stale.len(),
        stale.join("\n  ")
    );

    // The floor: every remaining entry must be a type-retired rule. If a new
    // rule is ever added to UNCOVERED for any other reason, this fails — so
    // "100 % of checkable" cannot quietly become "100 % of what we felt like
    // checking".
    // The floor. `Why` has exactly two variants and both mean "no validator
    // could do better", so this holds by construction — it is spelled out so
    // that adding a third, softer variant fails here first.
    for (id, why, reason) in UNCOVERED {
        assert!(
            matches!(why, Why::Retired | Why::Undecidable),
            "{id} is uncovered for a reason that is an excuse: {reason}"
        );
    }
    let undecidable: Vec<_> = UNCOVERED
        .iter()
        .filter(|(_, why, _)| *why == Why::Undecidable)
        .map(|(id, _, _)| *id)
        .collect();
    assert_eq!(
        undecidable,
        ["BR-CO-05", "BR-CO-06", "BR-CO-07", "BR-CO-08"],
        "exactly the four rules CEN binds to `true()` are undecidable; \
         anything else needs its own justification"
    );

    // Report the real numbers, separating the three populations, so coverage
    // cannot be quietly overstated. "148 rules implemented" is true and says
    // much less than it sounds like.
    let by_type = UNCOVERED
        .iter()
        .filter(|(_, why, _)| *why == Why::Retired)
        .count();
    let undecidable = undecidable.len();
    let n = registry.len();
    let exercised = registry
        .iter()
        .filter(|r| covered.contains(&r.id.as_str().to_ascii_uppercase()))
        .count();
    let checkable = n - by_type - undecidable;
    eprintln!(
        "conformance corpus\n  \
         registered:            {n}\n  \
         retired by the types:  {by_type}  (no state can make them fire)\n  \
         undecidable:           {undecidable}  (CEN binds them to true() too)\n  \
         checkable:             {checkable}\n  \
         exercised by a case:   {exercised}  ({:.0}% of checkable)\n  \
         declared uncovered:    {}",
        100.0 * exercised as f64 / checkable as f64,
        checkable - exercised
    );
}

/// A case must not fire *other* rules than its own, or it proves less than it
/// claims. Reported rather than asserted: some mutations legitimately cascade
/// (breaking BT-109 breaks the chain below it).
#[test]
fn cases_are_reported_when_they_cascade() {
    let mut noisy = Vec::new();
    for c in cases() {
        let mut inv = c.start();
        (c.mutate)(&mut inv);
        let extra: Vec<_> = c
            .run(&inv)
            .findings()
            .iter()
            .map(|f| f.rule.clone())
            .filter(|r| !r.eq_ignore_ascii_case(c.rule))
            .collect();
        if !extra.is_empty() {
            noisy.push(format!("{} also fires {:?}", c.rule, extra));
        }
    }
    // Not a failure — the totals chain is genuinely a chain — but visible, so a
    // case that accidentally breaks half the document is noticed.
    for line in &noisy {
        eprintln!("cascade: {line}");
    }
}
