//! Writing the semantic model as UBL 2.1.
//!
//! Element order comes from [`super::order`], applied by the serialiser;
//! prohibited elements are dropped and reported there too (`tests/order.rs`,
//! `tests/subset.rs`).
//!
//! Every amount carries `@currencyID` from BT-5, except the VAT total in
//! accounting currency (BT-111), which uses BT-6.

use super::{order, prohibitions};
use crate::Written;
use crate::report::{Entry, Kind};
use crate::xml::{Xml, base64};
use en16931::invoice::{
    Code, DocumentAllowanceCharge, Invoice, InvoiceLine, Item, LineAllowanceCharge, LineVat, Party,
    PaymentInstructions, PaymentMeans, Period, PostalAddress, PriceDetails, SupportingDocument,
};
use en16931::{DocumentKind, DocumentReference, Identifier, InvoiceAmount, Quantity};

/// UBL 2.1 namespace URIs. Fixed by OASIS; not configurable.
const NS_INVOICE: &str = "urn:oasis:names:specification:ubl:schema:xsd:Invoice-2";
const NS_CREDIT_NOTE: &str = "urn:oasis:names:specification:ubl:schema:xsd:CreditNote-2";
const NS_CAC: &str = "urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2";
const NS_CBC: &str = "urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2";

/// How the serialiser answers UBL's two questions.
static RULES: crate::xml::Rules = crate::xml::Rules {
    order: children_of,
    forbidden_path: prohibitions::forbidden_path,
    forbidden_attribute: prohibitions::forbidden_attribute,
};

/// The element order of `parent`'s children. `cac:SubInvoiceLine` is UBL's
/// `InvoiceLineType`, so it takes `cac:InvoiceLine`'s sequence: the corpus
/// shows sub-lines with only a few of their elements, and an element the
/// table has no place for would be dropped.
fn children_of(parent: &str) -> Option<&'static [&'static str]> {
    order::children_of(if parent == "SubInvoiceLine" {
        "InvoiceLine"
    } else {
        parent
    })
}

/// Amounts, formatted as UBL wants them.
///
/// `InvoiceAmount`'s `Display` is the two-decimal form `BR-DEC-*` requires.
fn amt(a: InvoiceAmount) -> String {
    a.to_string()
}

/// Write an invoice as UBL 2.1, permitting the named prohibitions, and report
/// anything the syntax could not carry.
///
/// The document element is `Invoice` or `CreditNote` according to
/// [`Invoice::kind`]. `waived` comes from [`super::waivers`]; core EN 16931
/// waives nothing.
pub(crate) fn write(inv: &Invoice, waived: &'static [&'static str]) -> Written {
    let credit = matches!(inv.kind, DocumentKind::CreditNote);
    let (root, ns) = if credit {
        ("CreditNote", NS_CREDIT_NOTE)
    } else {
        ("Invoice", NS_INVOICE)
    };
    let ccy = inv.currency.as_ref().map_or("", Code::as_str);

    let mut x = Xml::new(
        root,
        vec![
            ("xmlns".to_owned(), ns.to_owned()),
            ("xmlns:cac".to_owned(), NS_CAC.to_owned()),
            ("xmlns:cbc".to_owned(), NS_CBC.to_owned()),
        ],
        &RULES,
    )
    .waiving(waived);

    // ---- header, in the order `super::order` derived --------------------
    opt(
        &mut x,
        "cbc:CustomizationID",
        inv.specification_id.as_deref(),
    );
    opt(&mut x, "cbc:ProfileID", inv.business_process.as_deref());
    // UBL 2.1 requires `cbc:ID` and `cbc:IssueDate`; a missing BT-1 or BT-2 is
    // reported, not invented (BR-02 / BR-03 stop `crate::write` before this).
    for (absent, term, element, rule) in [
        (inv.number.is_none(), "BT-1", "cbc:ID", "BR-02"),
        (inv.issue_date.is_none(), "BT-2", "cbc:IssueDate", "BR-03"),
    ] {
        if absent {
            x.note(
                Entry::new(
                    Kind::Schema,
                    format!("{root}/{element}"),
                    format!(
                        "absent, and UBL 2.1 requires it: the document is not schema-valid \
                         ({rule} reports the absence on the model)"
                    ),
                )
                .term(term),
            );
        }
    }
    opt(&mut x, "cbc:ID", inv.number.as_deref());
    if let Some(d) = inv.issue_date {
        x.leaf("cbc:IssueDate", &[], &d.to_string());
    }
    // BT-9 is written even for `<CreditNote>`, which has no `cbc:DueDate`, so
    // the sequence table drops and reports it.
    if let Some(d) = inv.due_date {
        x.term("BT-9", |x| x.leaf("cbc:DueDate", &[], &d.to_string()));
    }
    if let Some(c) = &inv.type_code {
        let name = if credit {
            "cbc:CreditNoteTypeCode"
        } else {
            "cbc:InvoiceTypeCode"
        };
        x.leaf(name, &[], c.as_str());
    }
    for n in &inv.notes {
        // BT-21 has no element of its own in UBL: the subject code is spliced
        // into the note text as `#AAI#`, which `super::read` undoes.
        let body = n.note.as_deref().unwrap_or_default();
        match &n.subject_code {
            Some(c) => x.leaf("cbc:Note", &[], &format!("#{}#{}", c.as_str(), body)),
            None => x.leaf("cbc:Note", &[], body),
        }
    }
    if let Some(d) = inv.vat_point_date {
        x.leaf("cbc:TaxPointDate", &[], &d.to_string());
    }
    if !ccy.is_empty() {
        x.leaf("cbc:DocumentCurrencyCode", &[], ccy);
    }
    if let Some(c) = &inv.vat_accounting_currency {
        x.leaf("cbc:TaxCurrencyCode", &[], c.as_str());
    }
    opt(
        &mut x,
        "cbc:AccountingCost",
        inv.accounting_reference.as_deref(),
    );
    opt(&mut x, "cbc:BuyerReference", inv.buyer_reference.as_deref());

    if let Some(p) = &inv.invoicing_period {
        period(
            &mut x,
            "cac:InvoicePeriod",
            p,
            inv.vat_point_date_code.as_ref(),
            "BG-14",
        );
    } else if let Some(c) = &inv.vat_point_date_code {
        // BT-8 without BG-14 still goes in `cac:InvoicePeriod` (`BR-CO-19`
        // admits a period holding only the code).
        x.group("cac:InvoicePeriod", |x| {
            x.leaf("cbc:DescriptionCode", &[], c.as_str());
        });
    }
    if let Some(r) = &inv.purchase_order_reference {
        x.group("cac:OrderReference", |x| {
            x.leaf("cbc:ID", &[], r.as_str());
            if let Some(s) = &inv.sales_order_reference {
                x.leaf("cbc:SalesOrderID", &[], s.as_str());
            }
        });
    } else if let Some(s) = &inv.sales_order_reference {
        // BT-14 without BT-13 still needs the wrapper; UBL makes cbc:ID
        // mandatory inside it, so an empty one is written rather than a
        // dangling SalesOrderID — and said.
        x.note(
            Entry::new(
                Kind::Derived,
                "cac:OrderReference/cbc:ID",
                "absent while BT-14 is present, and UBL requires the element: written empty",
            )
            .term("BT-13"),
        );
        x.group("cac:OrderReference", |x| {
            x.leaf("cbc:ID", &[], "");
            x.leaf("cbc:SalesOrderID", &[], s.as_str());
        });
    }
    for p in &inv.preceding_invoices {
        x.group("cac:BillingReference", |x| {
            x.group("cac:InvoiceDocumentReference", |x| {
                x.leaf("cbc:ID", &[], p.reference.as_str());
                if let Some(d) = p.issue_date {
                    x.leaf("cbc:IssueDate", &[], &d.to_string());
                }
            });
        });
    }
    doc_ref(
        &mut x,
        "cac:DespatchDocumentReference",
        inv.despatch_advice_reference.as_ref(),
    );
    doc_ref(
        &mut x,
        "cac:ReceiptDocumentReference",
        inv.receiving_advice_reference.as_ref(),
    );
    doc_ref(
        &mut x,
        "cac:OriginatorDocumentReference",
        inv.tender_reference.as_ref(),
    );
    doc_ref(
        &mut x,
        "cac:ContractDocumentReference",
        inv.contract_reference.as_ref(),
    );
    for a in &inv.attachments {
        attachment(&mut x, a);
    }
    if let Some(o) = &inv.object_identifier {
        // BT-18 rides on an AdditionalDocumentReference with type code 130.
        x.group("cac:AdditionalDocumentReference", |x| {
            ident(x, "cbc:ID", o);
            x.leaf("cbc:DocumentTypeCode", &[], "130");
        });
    }
    x.term("BT-11", |x| {
        doc_ref(x, "cac:ProjectReference", inv.project_reference.as_ref());
    });

    // BT-90 rides on the seller in UBL; see `party`.
    let sepa_creditor = match inv.payment.as_ref().and_then(|p| p.means.as_ref()) {
        Some(PaymentMeans::DirectDebit(d)) => d.creditor_identifier.as_deref(),
        _ => None,
    };
    // Both parties are required in UBL 2.1; an empty one is pruned (BR-06 /
    // BR-07 report the absence on the model).
    for (wrapper, p, creditor, term) in [
        (
            "cac:AccountingSupplierParty",
            &inv.seller,
            sepa_creditor,
            "BG-4",
        ),
        ("cac:AccountingCustomerParty", &inv.buyer, None, "BG-7"),
    ] {
        if !party(&mut x, wrapper, p, creditor) {
            x.note(
                Entry::new(
                    Kind::Schema,
                    format!("{root}/{wrapper}"),
                    "empty, and UBL 2.1 requires it: the document is not schema-valid",
                )
                .term(term),
            );
        }
    }
    if let Some(p) = &inv.payee {
        x.group("cac:PayeeParty", |x| {
            if let Some(i) = &p.identifier {
                x.group("cac:PartyIdentification", |x| ident(x, "cbc:ID", i));
            }
            if let Some(n) = &p.name {
                x.group("cac:PartyName", |x| x.leaf("cbc:Name", &[], n));
            }
            if let Some(l) = &p.legal_registration {
                x.group("cac:PartyLegalEntity", |x| ident(x, "cbc:CompanyID", l));
            }
        });
    }
    if let Some(t) = &inv.tax_representative {
        x.group("cac:TaxRepresentativeParty", |x| {
            if let Some(n) = &t.name {
                x.group("cac:PartyName", |x| x.leaf("cbc:Name", &[], n));
            }
            address(x, &t.address);
            if let Some(v) = &t.vat_identifier {
                x.group("cac:PartyTaxScheme", |x| {
                    x.leaf("cbc:CompanyID", &[], v);
                    x.group("cac:TaxScheme", |x| x.leaf("cbc:ID", &[], "VAT"));
                });
            }
        });
    }
    if let Some(d) = &inv.delivery {
        x.group("cac:Delivery", |x| {
            if let Some(date) = d.date {
                x.leaf("cbc:ActualDeliveryDate", &[], &date.to_string());
            }
            if d.location.is_some() || d.address.is_some() {
                x.group("cac:DeliveryLocation", |x| {
                    if let Some(l) = &d.location {
                        ident(x, "cbc:ID", l);
                    }
                    if let Some(a) = &d.address {
                        x.group("cac:Address", |x| address_body(x, a));
                    }
                });
            }
            if let Some(n) = &d.party_name {
                x.group("cac:DeliveryParty", |x| {
                    x.group("cac:PartyName", |x| x.leaf("cbc:Name", &[], n));
                });
            }
        });
    }
    // A BG-19 holding only BT-90 (carried on the seller) prunes to nothing, so
    // the group's presence is reported lost (`BR-DE-30`, `BR-DE-31`).
    if let Some(PaymentMeans::DirectDebit(d)) = inv.payment.as_ref().and_then(|p| p.means.as_ref())
        && d.mandate_reference.is_none()
        && d.debited_account.is_none()
    {
        x.note(
            Entry::new(
                Kind::Dropped,
                "cac:PaymentMeans/cac:PaymentMandate",
                "carries only BT-90, which UBL puts on the seller — cac:PaymentMandate has \
                 nothing to hold, so the group does not survive (BR-DE-30 / BR-DE-31 report \
                 the missing terms on the model)",
            )
            .term("BG-19"),
        );
    }
    if let Some(p) = &inv.payment {
        payment_means(&mut x, p);
        if let Some(t) = &inv.payment_terms {
            x.group("cac:PaymentTerms", |x| x.leaf("cbc:Note", &[], t));
        }
    } else if let Some(t) = &inv.payment_terms {
        x.group("cac:PaymentTerms", |x| x.leaf("cbc:Note", &[], t));
    }
    for (a, is_charge) in inv
        .allowances
        .iter()
        .map(|a| (a, false))
        .chain(inv.charges.iter().map(|c| (c, true)))
    {
        doc_allowance(&mut x, a, is_charge, ccy);
    }

    // ---- tax ------------------------------------------------------------
    // UBL requires `cbc:TaxAmount` in every `cac:TaxTotal`, which holds BG-23,
    // so BG-23 without BT-110 gets BT-110 = Σ BT-117 (BR-CO-14), reported as
    // derived. If the sum overflows, BG-23 is left out and reported.
    let vat_total = inv.totals.vat_total.or_else(|| {
        if inv.vat_breakdown.is_empty() {
            return None;
        }
        let Ok(sum) = InvoiceAmount::checked_sum(inv.vat_breakdown.iter().map(|b| b.tax_amount))
        else {
            x.note(
                Entry::new(
                    Kind::Dropped,
                    "cac:TaxTotal",
                    "absent BT-110 beside BG-23, and the sum of BT-117 that would stand in \
                     for it overflows: UBL 2.1 has no cac:TaxTotal without cbc:TaxAmount",
                )
                .term("BG-23"),
            );
            return None;
        };
        x.note(
            Entry::new(
                Kind::Derived,
                "cac:TaxTotal/cbc:TaxAmount",
                "absent beside BG-23, and UBL 2.1 requires cbc:TaxAmount in cac:TaxTotal: \
                 written as the sum of BT-117, as BR-CO-14 defines it",
            )
            .term("BT-110")
            .value(sum.to_string()),
        );
        Some(sum)
    });
    if let Some(total) = vat_total {
        x.group("cac:TaxTotal", |x| {
            x.leaf("cbc:TaxAmount", &[("currencyID", ccy)], &amt(total));
            for b in &inv.vat_breakdown {
                x.group("cac:TaxSubtotal", |x| {
                    x.leaf(
                        "cbc:TaxableAmount",
                        &[("currencyID", ccy)],
                        &amt(b.taxable_amount),
                    );
                    x.leaf("cbc:TaxAmount", &[("currencyID", ccy)], &amt(b.tax_amount));
                    x.group("cac:TaxCategory", |x| {
                        x.leaf("cbc:ID", &[], b.category.as_str());
                        if let Some(r) = b.rate {
                            x.leaf("cbc:Percent", &[], &r.to_string());
                        }
                        if let Some(c) = &b.exemption_reason_code {
                            x.leaf("cbc:TaxExemptionReasonCode", &[], c.as_str());
                        }
                        if let Some(r) = &b.exemption_reason {
                            x.leaf("cbc:TaxExemptionReason", &[], r);
                        }
                        x.group("cac:TaxScheme", |x| x.leaf("cbc:ID", &[], "VAT"));
                    });
                });
            }
        });
    }
    // BT-111: a second cac:TaxTotal carrying only an amount, in BT-6's currency.
    if let (Some(t), Some(c)) = (
        inv.totals.vat_total_accounting,
        inv.vat_accounting_currency.as_ref(),
    ) {
        x.group("cac:TaxTotal", |x| {
            x.leaf("cbc:TaxAmount", &[("currencyID", c.as_str())], &amt(t));
        });
    }

    // BG-DEX-09 (XRechnung Extension). `UBL-CR-470` forbids
    // `cac:PrepaidPayment` in core EN 16931: the serialiser drops and reports it
    // unless the profile declares the group.
    for (i, p) in inv.extensions.third_party_payments.iter().enumerate() {
        // An empty group is pruned; reported because `BR-DEX-10` … `-12`
        // concern it.
        if p.payment_type.is_none() && p.amount.is_none() && p.description.is_none() {
            x.note(
                Entry::new(
                    Kind::Dropped,
                    "cac:PrepaidPayment",
                    "present and empty, which cac:PrepaidPayment cannot express \
                     (BR-DEX-10 … -12 report it on the model)",
                )
                .term(format!("BG-DEX-09[{i}]")),
            );
            continue;
        }
        x.group("cac:PrepaidPayment", |x| {
            if let Some(t) = &p.payment_type {
                x.leaf("cbc:ID", &[], t);
            }
            if let Some(a) = p.amount {
                x.leaf("cbc:PaidAmount", &[("currencyID", ccy)], &amt(a));
            }
            if let Some(d) = &p.description {
                x.leaf("cbc:InstructionID", &[], d);
            }
        });
    }

    // ---- totals ---------------------------------------------------------
    let t = &inv.totals;
    x.group("cac:LegalMonetaryTotal", |x| {
        x.leaf(
            "cbc:LineExtensionAmount",
            &[("currencyID", ccy)],
            &amt(t.line_total),
        );
        x.leaf(
            "cbc:TaxExclusiveAmount",
            &[("currencyID", ccy)],
            &amt(t.taxable_total),
        );
        x.leaf(
            "cbc:TaxInclusiveAmount",
            &[("currencyID", ccy)],
            &amt(t.gross_total),
        );
        if let Some(a) = t.allowance_total {
            x.leaf("cbc:AllowanceTotalAmount", &[("currencyID", ccy)], &amt(a));
        }
        if let Some(c) = t.charge_total {
            x.leaf("cbc:ChargeTotalAmount", &[("currencyID", ccy)], &amt(c));
        }
        if let Some(p) = t.paid {
            x.leaf("cbc:PrepaidAmount", &[("currencyID", ccy)], &amt(p));
        }
        if let Some(r) = t.rounding {
            x.leaf("cbc:PayableRoundingAmount", &[("currencyID", ccy)], &amt(r));
        }
        x.leaf("cbc:PayableAmount", &[("currencyID", ccy)], &amt(t.due));
    });

    if inv.lines.is_empty() {
        x.note(
            Entry::new(
                Kind::Schema,
                format!("{root}/cac:{root}Line"),
                "absent, and UBL 2.1 requires at least one line: the document is not \
                 schema-valid (BR-16 reports the absence on the model)",
            )
            .term("BG-25"),
        );
    }
    for (i, l) in inv.lines.iter().enumerate() {
        line(&mut x, l, i, inv.extensions.sub_lines(i), credit, ccy);
    }

    let (xml, entries) = x.finish();
    Written { xml, entries }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn opt(x: &mut Xml, name: &str, v: Option<&str>) {
    if let Some(v) = v {
        x.leaf(name, &[], v);
    }
}

/// An `Identifier`, with `@schemeID` and `@schemeVersionID` when present.
fn ident(x: &mut Xml, name: &str, i: &Identifier) {
    let mut attrs: Vec<(&str, &str)> = Vec::new();
    if let Some(s) = i.scheme() {
        attrs.push(("schemeID", s));
    }
    if let Some(v) = i.scheme_version() {
        attrs.push(("schemeVersionID", v));
    }
    x.leaf(name, &attrs, i.content());
}

fn doc_ref(x: &mut Xml, name: &str, r: Option<&DocumentReference>) {
    if let Some(r) = r {
        x.group(name, |x| x.leaf("cbc:ID", &[], r.as_str()));
    }
}

fn period(x: &mut Xml, name: &str, p: &Period, code: Option<&Code>, what: &str) {
    // An empty BG-14 is pruned; reported, since the model distinguishes it
    // from an absent one (`BR-CO-19` / `BR-CO-20`).
    if p.start.is_none() && p.end.is_none() && code.is_none() {
        x.note(
            Entry::new(
                Kind::Dropped,
                name,
                "present with neither a start nor an end date, which this element cannot \
                 express (BR-CO-19 / BR-CO-20 report it on the model)",
            )
            .term(what),
        );
        return;
    }
    x.group(name, |x| {
        if let Some(s) = p.start {
            x.leaf("cbc:StartDate", &[], &s.to_string());
        }
        if let Some(e) = p.end {
            x.leaf("cbc:EndDate", &[], &e.to_string());
        }
        // BT-8 is the invoice period's description code.
        if let Some(c) = code {
            x.leaf("cbc:DescriptionCode", &[], c.as_str());
        }
    });
}

fn attachment(x: &mut Xml, a: &SupportingDocument) {
    x.group("cac:AdditionalDocumentReference", |x| {
        x.leaf("cbc:ID", &[], a.reference.as_str());
        if let Some(d) = &a.description {
            x.leaf("cbc:DocumentDescription", &[], d);
        }
        if a.uri.is_some() || a.attachment.is_some() {
            x.group("cac:Attachment", |x| {
                if let Some(f) = &a.attachment {
                    x.leaf(
                        "cbc:EmbeddedDocumentBinaryObject",
                        &[("mimeCode", f.mime_code()), ("filename", f.filename())],
                        &base64(f.content()),
                    );
                }
                if let Some(u) = &a.uri {
                    x.group("cac:ExternalReference", |x| x.leaf("cbc:URI", &[], u));
                }
            });
        }
    });
}

fn address(x: &mut Xml, a: &PostalAddress) {
    // `group` drops a blank address.
    x.group("cac:PostalAddress", |x| address_body(x, a));
}

fn address_body(x: &mut Xml, a: &PostalAddress) {
    if let Some(l) = &a.line1 {
        x.leaf("cbc:StreetName", &[], l);
    }
    if let Some(l) = &a.line2 {
        x.leaf("cbc:AdditionalStreetName", &[], l);
    }
    if let Some(c) = &a.city {
        x.leaf("cbc:CityName", &[], c);
    }
    if let Some(p) = &a.post_code {
        x.leaf("cbc:PostalZone", &[], p);
    }
    if let Some(s) = &a.subdivision {
        x.leaf("cbc:CountrySubentity", &[], s);
    }
    // BT-163 goes into the one cac:AddressLine of the subset.
    if let Some(l) = &a.line3 {
        x.group("cac:AddressLine", |x| x.leaf("cbc:Line", &[], l));
    }
    if let Some(c) = &a.country {
        x.group("cac:Country", |x| {
            x.leaf("cbc:IdentificationCode", &[], c.as_str());
        });
    }
}

/// Write one party; whether anything was written.
///
/// For the seller, `sepa_creditor` is BT-90: UBL carries it as a seller
/// `cac:PartyIdentification` with `schemeID="SEPA"` (admitted by `BR-CL-10`),
/// which `super::read` moves back into BG-19.
fn party(x: &mut Xml, wrapper: &str, p: &Party, sepa_creditor: Option<&str>) -> bool {
    x.group_emitted(wrapper, |x| {
        x.group("cac:Party", |x| {
            if let Some(e) = &p.electronic_address {
                ident(x, "cbc:EndpointID", e);
            }
            // BT-46 is 0..1 (`UBL-SR-16`); a further buyer identifier is
            // reported, not written. BT-29 repeats.
            let buyer = wrapper == "cac:AccountingCustomerParty";
            for (n, i) in p.identifiers.iter().enumerate() {
                if buyer && n > 0 {
                    x.note(
                        Entry::new(
                            Kind::Dropped,
                            "cac:AccountingCustomerParty/cac:Party/cac:PartyIdentification",
                            "UBL carries one buyer identifier",
                        )
                        .term("BT-46")
                        .value(i.content())
                        .rule("UBL-SR-16"),
                    );
                    continue;
                }
                x.group("cac:PartyIdentification", |x| ident(x, "cbc:ID", i));
            }
            if let Some(c) = sepa_creditor.filter(|c| !c.trim().is_empty()) {
                // Not duplicated if the caller already carries it as BT-29.
                let already = p
                    .identifiers
                    .iter()
                    .any(|i| i.scheme() == Some("SEPA") && i.content() == c);
                if !already {
                    x.group("cac:PartyIdentification", |x| {
                        x.leaf("cbc:ID", &[("schemeID", "SEPA")], c);
                    });
                }
            }
            if let Some(n) = &p.trading_name {
                x.group("cac:PartyName", |x| x.leaf("cbc:Name", &[], n));
            }
            address(x, &p.address);
            if let Some(v) = &p.vat_identifier {
                x.group("cac:PartyTaxScheme", |x| {
                    x.leaf("cbc:CompanyID", &[], v);
                    x.group("cac:TaxScheme", |x| x.leaf("cbc:ID", &[], "VAT"));
                });
            }
            if let Some(t) = &p.tax_registration {
                // A non-VAT registration: scheme id other than `VAT` (`UBL-SR-13`).
                x.group("cac:PartyTaxScheme", |x| {
                    x.leaf("cbc:CompanyID", &[], t);
                    x.group("cac:TaxScheme", |x| x.leaf("cbc:ID", &[], "FC"));
                });
            }
            if p.name.is_some()
                || p.legal_registration.is_some()
                || p.additional_legal_information.is_some()
            {
                x.group("cac:PartyLegalEntity", |x| {
                    if let Some(n) = &p.name {
                        x.leaf("cbc:RegistrationName", &[], n);
                    }
                    if let Some(l) = &p.legal_registration {
                        ident(x, "cbc:CompanyID", l);
                    }
                    if let Some(a) = &p.additional_legal_information {
                        x.leaf("cbc:CompanyLegalForm", &[], a);
                    }
                });
            }
            let c = &p.contact;
            if c.name.is_some() || c.phone.is_some() || c.email.is_some() {
                x.group("cac:Contact", |x| {
                    if let Some(n) = &c.name {
                        x.leaf("cbc:Name", &[], n);
                    }
                    if let Some(t) = &c.phone {
                        x.leaf("cbc:Telephone", &[], t);
                    }
                    if let Some(e) = &c.email {
                        x.leaf("cbc:ElectronicMail", &[], e);
                    }
                });
            }
        });
    })
}

fn payment_means(x: &mut Xml, p: &PaymentInstructions) {
    // BG-17 is 0..n but `cac:PayeeFinancialAccount` is 0..1 in
    // `cac:PaymentMeans`, so each account gets its own `cac:PaymentMeans`
    // repeating BT-81 and BT-83 (as CEN's `guide-example1.xml` does).
    let head = |x: &mut Xml| {
        if let Some(c) = &p.means_code {
            let mut attrs: Vec<(&str, &str)> = Vec::new();
            if let Some(t) = &p.means_text {
                attrs.push(("name", t));
            }
            x.leaf("cbc:PaymentMeansCode", &attrs, c.as_str());
        }
        if let Some(r) = &p.remittance_information {
            x.leaf("cbc:PaymentID", &[], r);
        }
    };
    if let Some(PaymentMeans::CreditTransfer(ts)) = &p.means
        && !ts.is_empty()
    {
        for t in ts {
            x.group("cac:PaymentMeans", |x| {
                head(x);
                x.group("cac:PayeeFinancialAccount", |x| {
                    if let Some(a) = &t.account_identifier {
                        x.leaf("cbc:ID", &[], a);
                    }
                    if let Some(n) = &t.account_name {
                        x.leaf("cbc:Name", &[], n);
                    }
                    if let Some(p) = &t.provider_identifier {
                        x.group("cac:FinancialInstitutionBranch", |x| {
                            x.leaf("cbc:ID", &[], p);
                        });
                    }
                });
            });
        }
        return;
    }
    x.group("cac:PaymentMeans", |x| {
        head(x);
        match &p.means {
            Some(PaymentMeans::Card(c)) => {
                x.group("cac:CardAccount", |x| {
                    if let Some(n) = &c.primary_account_number {
                        x.leaf("cbc:PrimaryAccountNumberID", &[], n);
                    }
                    // cbc:NetworkID is mandatory and has no EN 16931 term;
                    // `NA` as in CEN's examples.
                    x.leaf("cbc:NetworkID", &[], "NA");
                    if let Some(h) = &c.holder_name {
                        x.leaf("cbc:HolderName", &[], h);
                    }
                });
            }
            Some(PaymentMeans::DirectDebit(d)) => {
                x.group("cac:PaymentMandate", |x| {
                    if let Some(m) = &d.mandate_reference {
                        x.leaf("cbc:ID", &[], m);
                    }
                    if let Some(a) = &d.debited_account {
                        x.group("cac:PayerFinancialAccount", |x| x.leaf("cbc:ID", &[], a));
                    }
                });
            }
            // An empty credit-transfer list: one bare `cac:PaymentMeans`.
            Some(PaymentMeans::CreditTransfer(_)) | None => {}
        }
    });
}

fn tax_category(x: &mut Xml, v: &LineVat, wrapper: &str) {
    x.group(wrapper, |x| {
        x.leaf("cbc:ID", &[], v.category.as_str());
        if let Some(r) = v.rate {
            x.leaf("cbc:Percent", &[], &r.to_string());
        }
        x.group("cac:TaxScheme", |x| x.leaf("cbc:ID", &[], "VAT"));
    });
}

fn doc_allowance(x: &mut Xml, a: &DocumentAllowanceCharge, is_charge: bool, ccy: &str) {
    x.group("cac:AllowanceCharge", |x| {
        x.leaf(
            "cbc:ChargeIndicator",
            &[],
            if is_charge { "true" } else { "false" },
        );
        if let Some(c) = &a.reason_code {
            x.leaf("cbc:AllowanceChargeReasonCode", &[], c.as_str());
        }
        if let Some(r) = &a.reason {
            x.leaf("cbc:AllowanceChargeReason", &[], r);
        }
        if let Some(p) = a.percentage {
            x.leaf("cbc:MultiplierFactorNumeric", &[], &p.to_string());
        }
        x.leaf("cbc:Amount", &[("currencyID", ccy)], &amt(a.amount));
        if let Some(b) = a.base_amount {
            x.leaf("cbc:BaseAmount", &[("currencyID", ccy)], &amt(b));
        }
        tax_category(x, &a.vat, "cac:TaxCategory");
    });
}

fn line_allowance(x: &mut Xml, a: &LineAllowanceCharge, is_charge: bool, ccy: &str) {
    x.group("cac:AllowanceCharge", |x| {
        x.leaf(
            "cbc:ChargeIndicator",
            &[],
            if is_charge { "true" } else { "false" },
        );
        if let Some(c) = &a.reason_code {
            x.leaf("cbc:AllowanceChargeReasonCode", &[], c.as_str());
        }
        if let Some(r) = &a.reason {
            x.leaf("cbc:AllowanceChargeReason", &[], r);
        }
        if let Some(p) = a.percentage {
            x.leaf("cbc:MultiplierFactorNumeric", &[], &p.to_string());
        }
        x.leaf("cbc:Amount", &[("currencyID", ccy)], &amt(a.amount));
        if let Some(b) = a.base_amount {
            x.leaf("cbc:BaseAmount", &[("currencyID", ccy)], &amt(b));
        }
    });
}

/// What a `cac:InvoiceLine` and a `cac:SubInvoiceLine` carry; both are UBL's
/// `InvoiceLineType`, so one writer keeps them from drifting apart.
struct LineTerms<'a> {
    id: &'a str,
    note: Option<&'a str>,
    /// BT-129 with its unit, BT-130.
    quantity: Option<(Quantity, Option<&'a str>)>,
    net_amount: InvoiceAmount,
    accounting_reference: Option<&'a str>,
    period: Option<&'a Period>,
    order_line_reference: Option<&'a DocumentReference>,
    object_identifier: Option<&'a Identifier>,
    allowances: &'a [LineAllowanceCharge],
    charges: &'a [LineAllowanceCharge],
    item: &'a Item,
    vat: Option<&'a LineVat>,
    price: Option<&'a PriceDetails>,
}

/// The elements of one line or sub-line, in `InvoiceLineType`'s order;
/// `period_term` names its period group for a note.
fn line_terms(
    x: &mut Xml,
    t: &LineTerms<'_>,
    qty_name: &str,
    period_term: &str,
    i: usize,
    ccy: &str,
) {
    x.leaf("cbc:ID", &[], t.id);
    if let Some(n) = t.note {
        x.leaf("cbc:Note", &[], n);
    }
    if let Some((q, unit)) = t.quantity {
        quantity_leaf(x, qty_name, &q.to_string(), unit);
    }
    x.leaf(
        "cbc:LineExtensionAmount",
        &[("currencyID", ccy)],
        &amt(t.net_amount),
    );
    if let Some(a) = t.accounting_reference {
        x.leaf("cbc:AccountingCost", &[], a);
    }
    if let Some(p) = t.period {
        period(x, "cac:InvoicePeriod", p, None, period_term);
    }
    if let Some(o) = t.order_line_reference {
        x.group("cac:OrderLineReference", |x| {
            x.leaf("cbc:LineID", &[], o.as_str());
        });
    }
    if let Some(o) = t.object_identifier {
        x.group("cac:DocumentReference", |x| {
            ident(x, "cbc:ID", o);
            x.leaf("cbc:DocumentTypeCode", &[], "130");
        });
    }
    for (a, c) in t
        .allowances
        .iter()
        .map(|a| (a, false))
        .chain(t.charges.iter().map(|c| (c, true)))
    {
        line_allowance(x, a, c, ccy);
    }
    item(x, t.item, t.vat);
    if let Some(p) = t.price {
        price(x, p, i, ccy);
    }
}

fn line(
    x: &mut Xml,
    l: &InvoiceLine,
    i: usize,
    subs: &[en16931::SubInvoiceLine],
    credit: bool,
    ccy: &str,
) {
    let (root, qty_name) = if credit {
        ("cac:CreditNoteLine", "cbc:CreditedQuantity")
    } else {
        ("cac:InvoiceLine", "cbc:InvoicedQuantity")
    };
    let terms = LineTerms {
        id: &l.id,
        note: l.note.as_deref(),
        quantity: Some((l.quantity, Some(l.unit_code.as_str()))),
        net_amount: l.net_amount,
        accounting_reference: l.accounting_reference.as_deref(),
        period: l.period.as_ref(),
        order_line_reference: l.order_line_reference.as_ref(),
        object_identifier: l.object_identifier.as_ref(),
        allowances: &l.allowances,
        charges: &l.charges,
        item: &l.item,
        vat: Some(&l.vat),
        price: Some(&l.price),
    };
    x.group(root, |x| {
        line_terms(x, &terms, qty_name, "BG-26", i, ccy);
        // BG-DEX-01, recursively. `UBL-CR-646` forbids `cac:SubInvoiceLine` in
        // core EN 16931, so it is dropped and reported unless waived.
        for s in subs {
            sub_line(x, s, i, ccy);
        }
    });
}

/// A quantity leaf with its `unitCode`, or without one when the unit is absent
/// or empty: an empty attribute reads back as a stated, empty unit.
fn quantity_leaf(x: &mut Xml, name: &str, value: &str, unit: Option<&str>) {
    match unit.filter(|u| !u.is_empty()) {
        Some(u) => x.leaf(name, &[("unitCode", u)], value),
        None => x.leaf(name, &[], value),
    }
}

/// One `cac:SubInvoiceLine`, with its own children beneath it. It has
/// `cbc:InvoicedQuantity` only, also beneath a credit note's line.
fn sub_line(x: &mut Xml, s: &en16931::SubInvoiceLine, i: usize, ccy: &str) {
    let terms = LineTerms {
        id: &s.id,
        note: s.note.as_deref(),
        quantity: s
            .quantity
            .as_ref()
            .map(|q| (q.value, q.unit_code.as_ref().map(Code::as_str))),
        net_amount: s.net_amount,
        accounting_reference: s.accounting_reference.as_deref(),
        period: s.period.as_ref(),
        order_line_reference: s.order_line_reference.as_ref(),
        object_identifier: s.object_identifier.as_ref(),
        allowances: &s.allowances,
        charges: &s.charges,
        item: &s.item,
        vat: s.vat.as_ref(),
        price: s.price.as_ref(),
    };
    x.group("cac:SubInvoiceLine", |x| {
        line_terms(x, &terms, "cbc:InvoicedQuantity", "BG-DEX-05", i, ccy);
        for child in &s.children {
            sub_line(x, child, i, ccy);
        }
    });
}

fn item(x: &mut Xml, i: &Item, vat: Option<&LineVat>) {
    x.group("cac:Item", |x| {
        if let Some(d) = &i.description {
            x.leaf("cbc:Description", &[], d);
        }
        if let Some(n) = &i.name {
            x.leaf("cbc:Name", &[], n);
        }
        if let Some(b) = &i.buyer_identifier {
            x.group("cac:BuyersItemIdentification", |x| x.leaf("cbc:ID", &[], b));
        }
        if let Some(s) = &i.seller_identifier {
            x.group("cac:SellersItemIdentification", |x| {
                x.leaf("cbc:ID", &[], s);
            });
        }
        if let Some(s) = &i.standard_identifier {
            x.group("cac:StandardItemIdentification", |x| ident(x, "cbc:ID", s));
        }
        if let Some(c) = &i.origin_country {
            x.group("cac:OriginCountry", |x| {
                x.leaf("cbc:IdentificationCode", &[], c.as_str());
            });
        }
        for c in &i.classification_identifiers {
            x.group("cac:CommodityClassification", |x| {
                // BT-158's scheme is `@listID`, not `@schemeID`.
                let mut attrs: Vec<(&str, &str)> = Vec::new();
                if let Some(l) = c.scheme() {
                    attrs.push(("listID", l));
                }
                if let Some(v) = c.scheme_version() {
                    attrs.push(("listVersionID", v));
                }
                x.leaf("cbc:ItemClassificationCode", &attrs, c.content());
            });
        }
        if let Some(vat) = vat {
            tax_category(x, vat, "cac:ClassifiedTaxCategory");
        }
        for (j, a) in i.attributes.iter().enumerate() {
            // UBL requires both; an absent one is written empty and said
            // (`BR-54` already finds the attribute incomplete).
            for (el, term, v) in [
                ("cbc:Name", "BT-160", &a.name),
                ("cbc:Value", "BT-161", &a.value),
            ] {
                if v.is_none() {
                    x.note(
                        Entry::new(
                            Kind::Derived,
                            format!("cac:Item/cac:AdditionalItemProperty/{el}"),
                            "absent, and UBL requires the element: written empty",
                        )
                        .term(format!("BG-32[{}]/{term}", j + 1)),
                    );
                }
            }
            x.group("cac:AdditionalItemProperty", |x| {
                x.leaf("cbc:Name", &[], a.name.as_deref().unwrap_or_default());
                x.leaf("cbc:Value", &[], a.value.as_deref().unwrap_or_default());
            });
        }
    });
}

fn price(x: &mut Xml, p: &PriceDetails, i: usize, ccy: &str) {
    // `cbc:Amount` is mandatory in `cac:AllowanceCharge`, so BT-148 without
    // BT-147 is written with a zero discount, reported as derived.
    if p.price_discount.is_none() && p.gross_price.is_some() {
        x.note(
            Entry::new(
                Kind::Derived,
                "cac:Price/cac:AllowanceCharge/cbc:Amount",
                "absent while BT-148 is present, and UBL requires the element: written as \
                 0.00, so it reads back as a stated zero discount",
            )
            .term(format!("BG-25[{i}]/BT-147"))
            .value("0.00"),
        );
    }
    x.group("cac:Price", |x| {
        x.leaf(
            "cbc:PriceAmount",
            &[("currencyID", ccy)],
            &p.net_price.to_string(),
        );
        if let Some(q) = p.base_quantity {
            // BT-150 is optional; an empty `unitCode` would read back as
            // `Some("")` and trip `PEPPOL-EN16931-R130`.
            quantity_leaf(
                x,
                "cbc:BaseQuantity",
                &q.to_string(),
                p.base_quantity_code.as_ref().map(Code::as_str),
            );
        }
        // BT-147 (`cbc:Amount`) and BT-148 (`cbc:BaseAmount`) share one
        // price-level allowance, written when either is present.
        if p.price_discount.is_some() || p.gross_price.is_some() {
            x.group("cac:AllowanceCharge", |x| {
                x.leaf("cbc:ChargeIndicator", &[], "false");
                // No discount: `cbc:Amount` is mandatory, so zero is written
                // (reads back as `Some(0)`).
                let discount = p.price_discount.unwrap_or(en16931::UnitPriceAmount::ZERO);
                x.leaf("cbc:Amount", &[("currencyID", ccy)], &discount.to_string());
                if let Some(g) = p.gross_price {
                    x.leaf("cbc:BaseAmount", &[("currencyID", ccy)], &g.to_string());
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ordering is applied at serialisation, whatever order the writer emits.
    #[test]
    fn children_are_sorted_into_schema_order() {
        let mut x = Xml::new("Invoice", vec![], &RULES);
        x.leaf("cbc:ID", &[], "1");
        x.leaf("cbc:CustomizationID", &[], "urn:x");
        let (xml, _) = x.finish();
        let cust = xml.find("CustomizationID").expect("present");
        let id = xml.find("<cbc:ID>").expect("present");
        assert!(cust < id, "CustomizationID must precede ID\n{xml}");
    }

    /// Repeated elements keep the order they were written in.
    #[test]
    fn sorting_is_stable_for_repeats() {
        let mut x = Xml::new("Invoice", vec![], &RULES);
        x.leaf("cbc:Note", &[], "first");
        x.leaf("cbc:Note", &[], "second");
        let (xml, _) = x.finish();
        assert!(
            xml.find("first").unwrap() < xml.find("second").unwrap(),
            "{xml}"
        );
    }

    /// An element the parent's sequence has no place for is dropped and reported.
    #[test]
    fn an_unplaceable_element_is_reported() {
        let mut x = Xml::new("Invoice", vec![], &RULES);
        x.leaf("cbc:NotAUblElement", &[], "x");
        let (xml, dropped) = x.finish();
        assert!(!xml.contains("NotAUblElement"), "{xml}");
        let paths: Vec<&str> = dropped.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["Invoice/cbc:NotAUblElement"]);
        assert_eq!(dropped[0].kind, Kind::Dropped);
    }

    #[test]
    fn a_credit_note_uses_the_other_root() {
        let mut inv = Invoice::default();
        inv.kind = DocumentKind::CreditNote;
        let out = write(&inv, &[]);
        assert!(out.xml.contains("<CreditNote "), "{}", out.xml);
        assert!(out.xml.contains("CreditNote-2"));
        assert!(!out.xml.contains("cbc:InvoiceTypeCode"));
    }

    /// BG-23 without BT-110 has no schema-valid UBL form: BT-110 is written as
    /// the sum of BT-117 and the addition is reported.
    #[test]
    fn a_breakdown_without_bt_110_writes_the_sum_and_says_so() {
        let mut inv = Invoice::default();
        inv.vat_breakdown = vec![en16931::invoice::VatBreakdown {
            taxable_amount: InvoiceAmount::from_decimal_exact(rust_decimal::Decimal::new(
                10_000, 2,
            ))
            .expect("amount"),
            tax_amount: InvoiceAmount::default(),
            category: Code::new("O"),
            rate: None,
            exemption_reason: None,
            exemption_reason_code: None,
        }];
        let out = write(&inv, &[]);
        assert!(
            out.xml
                .contains(r#"<cbc:TaxAmount currencyID="">0.00</cbc:TaxAmount>"#),
            "{}",
            out.xml
        );
        assert!(
            out.entries.iter().any(|d| d.kind == Kind::Derived
                && d.term.as_deref() == Some("BT-110")
                && d.message.contains("BR-CO-14")),
            "{:?}",
            out.entries
        );
    }

    /// What UBL 2.1 requires and the model lacks is named, never invented.
    #[test]
    fn an_empty_invoice_names_everything_ubl_requires() {
        let out = write(&Invoice::default(), &[]);
        for term in ["BT-1", "BT-2", "BG-4", "BG-7", "BG-25"] {
            assert!(
                out.entries.iter().any(|d| d.kind == Kind::Schema
                    && d.term.as_deref().is_some_and(|t| t.starts_with(term))
                    && d.message.contains("not schema-valid")),
                "{term}: {:?}",
                out.entries
            );
        }
    }
}
