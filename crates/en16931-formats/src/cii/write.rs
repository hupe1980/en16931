//! Writing the semantic model as UN/CEFACT CII D16B.
//!
//! Ordering (from [`super::order`], generated from the D16B schema) and the
//! prohibitions are the shared serialiser's job; this module maps each business
//! term into CII's three document parts.

use en16931::invoice::*;
use en16931::{Date, DocumentReference, Identifier, InvoiceAmount};

use super::{ns, order, prohibitions};
use crate::Written;
use crate::report::{Entry, Kind};
use crate::xml::{Xml, base64};

/// How the serialiser answers CII's two questions.
static RULES: crate::xml::Rules = crate::xml::Rules {
    order: order::children_of,
    forbidden_path: prohibitions::forbidden_path,
    forbidden_attribute: prohibitions::forbidden_attribute,
};

/// The only date format EN 16931 permits in CII: `CCYYMMDD`, on every
/// `udt:DateTimeString`, `udt:DateString` and `qdt:DateTimeString` alike.
pub(super) const DATE_FORMAT: &str = "102";

/// An amount, as CII writes it.
fn amt(a: InvoiceAmount) -> String {
    a.to_string()
}

/// `<ram:Foo><udt:DateTimeString format="102">20260115</udt:DateTimeString></ram:Foo>`
///
/// The `format` attribute is always written.
fn date(x: &mut Xml, wrapper: &str, d: Date) {
    dated(x, wrapper, "udt:DateTimeString", d);
}

/// `<ram:TaxPointDate><udt:DateString format="102">…`
///
/// D16B types `ram:TaxPointDate` (BT-7) as `udt:DateType`, not
/// `udt:DateTimeType`: its child is `udt:DateString`, and a
/// `udt:DateTimeString` there fails the schema. It is the only such date.
fn plain_date(x: &mut Xml, wrapper: &str, d: Date) {
    dated(x, wrapper, "udt:DateString", d);
}

/// `<ram:FormattedIssueDateTime><qdt:DateTimeString format="102">…`
///
/// The same content as [`date`], in the other namespace: D16B types
/// `ram:FormattedIssueDateTime` as `qdt:FormattedDateTimeType`, whose child is
/// `qdt:DateTimeString`. A `udt:` child there fails the schema.
fn formatted_date(x: &mut Xml, wrapper: &str, d: Date) {
    dated(x, wrapper, "qdt:DateTimeString", d);
}

fn dated(x: &mut Xml, wrapper: &str, element: &str, d: Date) {
    x.group(wrapper, |x| {
        x.leaf(
            element,
            &[("format", DATE_FORMAT)],
            &format!("{:04}{:02}{:02}", d.year(), d.month(), d.day()),
        );
    });
}

/// BT-149 with BT-150, and no `unitCode` when BT-150 is absent (an empty one
/// reads back as `Some("")` and trips `PEPPOL-EN16931-R130`).
fn basis_quantity(x: &mut Xml, p: &en16931::invoice::PriceDetails) {
    let Some(q) = p.base_quantity else { return };
    let unit = p
        .base_quantity_code
        .as_ref()
        .map(Code::as_str)
        .filter(|u| !u.is_empty());
    let attrs: Vec<(&str, &str)> = unit.map(|u| ("unitCode", u)).into_iter().collect();
    x.leaf("ram:BasisQuantity", &attrs, &q.to_string());
}

/// The `ram:ChargeIndicator` wrapper, telling an allowance from a charge.
fn charge_indicator(x: &mut Xml, is_charge: bool) {
    x.group("ram:ChargeIndicator", |x| {
        x.leaf(
            "udt:Indicator",
            &[],
            if is_charge { "true" } else { "false" },
        );
    });
}

/// An `Identifier`, with `@schemeID` when present.
fn ident(x: &mut Xml, name: &str, i: &Identifier) {
    let mut attrs: Vec<(&str, &str)> = Vec::new();
    if let Some(s) = i.scheme() {
        attrs.push(("schemeID", s));
    }
    x.leaf(name, &attrs, i.content());
}

/// Write an invoice as CII D16B, reporting anything the syntax could not carry.
///
/// One document element for invoices and credit notes; CII distinguishes them
/// by BT-3 alone.
#[must_use]
pub(crate) fn write(inv: &Invoice) -> Written {
    let ccy = inv.currency.as_ref().map_or("", Code::as_str);

    let mut x = Xml::new(
        "rsm:CrossIndustryInvoice",
        vec![
            ("xmlns:rsm".to_owned(), ns::RSM.to_owned()),
            ("xmlns:ram".to_owned(), ns::RAM.to_owned()),
            ("xmlns:udt".to_owned(), ns::UDT.to_owned()),
            ("xmlns:qdt".to_owned(), ns::QDT.to_owned()),
        ],
        &RULES,
    );

    // ---- 1. context: what specification this document claims ------------
    x.group_required("rsm:ExchangedDocumentContext", |x| {
        if let Some(p) = &inv.business_process {
            x.group(
                "ram:BusinessProcessSpecifiedDocumentContextParameter",
                |x| {
                    x.leaf("ram:ID", &[], p);
                },
            );
        }
        if let Some(s) = &inv.specification_id {
            x.group("ram:GuidelineSpecifiedDocumentContextParameter", |x| {
                x.leaf("ram:ID", &[], s);
            });
        }
    });

    // ---- 2. the document itself -----------------------------------------
    // D16B requires `ram:ID` and `ram:IssueDateTime`; a missing BT-1 or BT-2 is
    // reported, not invented (BR-02 / BR-03 stop `crate::write` before this).
    for (absent, term, element, rule) in [
        (inv.number.is_none(), "BT-1", "ram:ID", "BR-02"),
        (
            inv.issue_date.is_none(),
            "BT-2",
            "ram:IssueDateTime",
            "BR-03",
        ),
    ] {
        if absent {
            x.note(
                Entry::new(
                    Kind::Schema,
                    format!("rsm:ExchangedDocument/{element}"),
                    format!(
                        "absent, and D16B requires it: the document is not schema-valid \
                         ({rule} reports the absence on the model)"
                    ),
                )
                .term(term),
            );
        }
    }
    x.group_required("rsm:ExchangedDocument", |x| {
        if let Some(n) = &inv.number {
            x.leaf("ram:ID", &[], n);
        }
        if let Some(c) = &inv.type_code {
            x.leaf("ram:TypeCode", &[], c.as_str());
        }
        if let Some(d) = inv.issue_date {
            date(x, "ram:IssueDateTime", d);
        }
        for n in &inv.notes {
            if n.note.is_none() {
                x.note(
                    Entry::new(
                        Kind::Derived,
                        "rsm:ExchangedDocument/ram:IncludedNote/ram:Content",
                        "absent while BT-21 is present, and D16B requires the element: written empty",
                    )
                    .term("BG-1/BT-22"),
                );
            }
            x.group("ram:IncludedNote", |x| {
                x.leaf("ram:Content", &[], n.note.as_deref().unwrap_or_default());
                // BT-21 has its own element in CII.
                if let Some(c) = &n.subject_code {
                    x.leaf("ram:SubjectCode", &[], c.as_str());
                }
            });
        }
    });

    // The credit-note distinction lives only in BT-3, so a credit note with an
    // invoice type code reads back as an invoice; that loss is reported.
    if matches!(inv.kind, DocumentKind::CreditNote)
        && !inv
            .type_code
            .as_ref()
            .is_some_and(|c| c.is_in(en16931::codes::generated::CREDIT_NOTE_TYPE_CODES))
    {
        let e = Entry::new(
            Kind::Dropped,
            "rsm:ExchangedDocument/ram:TypeCode",
            "the document is a credit note and BT-3 is not a UNTDID 1001 credit-note \
             code — CII carries the distinction in BT-3 alone, so it reads back as an \
             invoice (BR-CL-01 reports the same disagreement on the model)",
        )
        .term("BT-3");
        x.note(match &inv.type_code {
            Some(c) => e.value(c.as_str()),
            None => e,
        });
    }

    // BG-DEX-01 and BG-DEX-09 (XRechnung Extension) are not written, for
    // different reasons. Sub invoice lines: XRechnung refuses them in CII
    // (`BR-DEX-15`, `not(exists(//ram:ParentLineID))`).
    if !inv.extensions.sub_invoice_lines.is_empty() {
        x.note(
            Entry::new(
                Kind::Dropped,
                "ram:IncludedSupplyChainTradeLineItem",
                "XRechnung does not support sub invoice lines in CII at all (ram:ParentLineID \
                 is forbidden), so they are not written",
            )
            .term("BG-DEX-01")
            .rule("BR-DEX-15")
            .hint("UBL carries them; write UBL"),
        );
    }
    // Third party payments: no rule forbids them in CII, but this binding does
    // not write them; reported as dropped.
    if !inv.extensions.third_party_payments.is_empty() {
        x.note(
            Entry::new(
                Kind::Dropped,
                "ram:ApplicableHeaderTradeSettlement",
                "the CII binding does not write this XRechnung Extension group yet",
            )
            .term("BG-DEX-09")
            .hint("UBL carries it; write UBL"),
        );
    }

    // ---- 3. the transaction ---------------------------------------------
    x.group_required("rsm:SupplyChainTradeTransaction", |x| {
        for (i, l) in inv.lines.iter().enumerate() {
            line(x, l, i, ccy);
        }
        agreement(x, inv);
        delivery(x, inv);
        settlement(x, inv, ccy);
    });

    let (xml, entries) = x.finish();
    Written { xml, entries }
}

// ---------------------------------------------------------------------------
// header: agreement — who, and against which references
// ---------------------------------------------------------------------------

fn agreement(x: &mut Xml, inv: &Invoice) {
    x.group_required("ram:ApplicableHeaderTradeAgreement", |x| {
        if let Some(b) = &inv.buyer_reference {
            x.leaf("ram:BuyerReference", &[], b);
        }
        party(x, "ram:SellerTradeParty", &inv.seller, true);
        party(x, "ram:BuyerTradeParty", &inv.buyer, false);
        if let Some(t) = &inv.tax_representative {
            x.group("ram:SellerTaxRepresentativeTradeParty", |x| {
                if let Some(n) = &t.name {
                    x.leaf("ram:Name", &[], n);
                }
                address(x, &t.address);
                if let Some(v) = &t.vat_identifier {
                    x.group("ram:SpecifiedTaxRegistration", |x| {
                        x.leaf("ram:ID", &[("schemeID", "VA")], v);
                    });
                }
            });
        }
        doc_ref(
            x,
            "ram:SellerOrderReferencedDocument",
            inv.sales_order_reference.as_ref(),
        );
        doc_ref(
            x,
            "ram:BuyerOrderReferencedDocument",
            inv.purchase_order_reference.as_ref(),
        );
        doc_ref(
            x,
            "ram:ContractReferencedDocument",
            inv.contract_reference.as_ref(),
        );
        for a in &inv.attachments {
            attachment(x, a);
        }
        // BT-17 and BT-18 ride on `AdditionalReferencedDocument` with a type code.
        if let Some(t) = &inv.tender_reference {
            x.group("ram:AdditionalReferencedDocument", |x| {
                x.leaf("ram:IssuerAssignedID", &[], t.as_str());
                x.leaf("ram:TypeCode", &[], "50");
            });
        }
        if let Some(o) = &inv.object_identifier {
            x.group("ram:AdditionalReferencedDocument", |x| {
                x.leaf("ram:IssuerAssignedID", &[], o.content());
                x.leaf("ram:TypeCode", &[], "130");
                if let Some(s) = o.scheme() {
                    x.leaf("ram:ReferenceTypeCode", &[], s);
                }
            });
        }
        if let Some(p) = &inv.project_reference {
            x.group("ram:SpecifiedProcuringProject", |x| {
                x.leaf("ram:ID", &[], p.as_str());
                // `ram:Name` is mandatory with no EN 16931 term; CEN's
                // examples repeat the reference.
                x.leaf("ram:Name", &[], p.as_str());
            });
        }
    });
}

fn doc_ref(x: &mut Xml, name: &str, r: Option<&DocumentReference>) {
    if let Some(r) = r {
        x.group(name, |x| x.leaf("ram:IssuerAssignedID", &[], r.as_str()));
    }
}

fn attachment(x: &mut Xml, a: &SupportingDocument) {
    x.group("ram:AdditionalReferencedDocument", |x| {
        x.leaf("ram:IssuerAssignedID", &[], a.reference.as_str());
        if let Some(u) = &a.uri {
            x.leaf("ram:URIID", &[], u);
        }
        x.leaf("ram:TypeCode", &[], "916");
        if let Some(d) = &a.description {
            x.leaf("ram:Name", &[], d);
        }
        if let Some(f) = &a.attachment {
            x.leaf(
                "ram:AttachmentBinaryObject",
                &[("mimeCode", f.mime_code()), ("filename", f.filename())],
                &base64(f.content()),
            );
        }
    });
}

/// A trade party.
///
/// `seller` selects the seller-only terms, such as BT-33.
fn party(x: &mut Xml, wrapper: &str, p: &Party, seller: bool) {
    x.group(wrapper, |x| {
        for i in &p.identifiers {
            // Scheme-qualified: `ram:GlobalID` with `@schemeID`; else `ram:ID`.
            if i.scheme().is_some() {
                ident(x, "ram:GlobalID", i);
            } else {
                x.leaf("ram:ID", &[], i.content());
            }
        }
        if let Some(n) = &p.name {
            x.leaf("ram:Name", &[], n);
        }
        if seller && let Some(a) = &p.additional_legal_information {
            x.leaf("ram:Description", &[], a);
        }
        if p.legal_registration.is_some() || p.trading_name.is_some() {
            x.group("ram:SpecifiedLegalOrganization", |x| {
                if let Some(l) = &p.legal_registration {
                    ident(x, "ram:ID", l);
                }
                if let Some(t) = &p.trading_name {
                    x.leaf("ram:TradingBusinessName", &[], t);
                }
            });
        }
        let c = &p.contact;
        if c.name.is_some() || c.phone.is_some() || c.email.is_some() {
            x.group("ram:DefinedTradeContact", |x| {
                if let Some(n) = &c.name {
                    x.leaf("ram:PersonName", &[], n);
                }
                if let Some(t) = &c.phone {
                    x.group("ram:TelephoneUniversalCommunication", |x| {
                        x.leaf("ram:CompleteNumber", &[], t);
                    });
                }
                if let Some(e) = &c.email {
                    x.group("ram:EmailURIUniversalCommunication", |x| {
                        x.leaf("ram:URIID", &[], e);
                    });
                }
            });
        }
        address(x, &p.address);
        if let Some(e) = &p.electronic_address {
            x.group("ram:URIUniversalCommunication", |x| {
                ident(x, "ram:URIID", e);
            });
        }
        if let Some(v) = &p.vat_identifier {
            x.group("ram:SpecifiedTaxRegistration", |x| {
                x.leaf("ram:ID", &[("schemeID", "VA")], v);
            });
        }
        if let Some(t) = &p.tax_registration {
            // `FC`, not `VA`: BT-32 is a non-VAT registration.
            x.group("ram:SpecifiedTaxRegistration", |x| {
                x.leaf("ram:ID", &[("schemeID", "FC")], t);
            });
        }
    });
}

fn address(x: &mut Xml, a: &PostalAddress) {
    // `group` drops a blank address.
    x.group("ram:PostalTradeAddress", |x| {
        if let Some(p) = &a.post_code {
            x.leaf("ram:PostcodeCode", &[], p);
        }
        if let Some(l) = &a.line1 {
            x.leaf("ram:LineOne", &[], l);
        }
        if let Some(l) = &a.line2 {
            x.leaf("ram:LineTwo", &[], l);
        }
        if let Some(l) = &a.line3 {
            x.leaf("ram:LineThree", &[], l);
        }
        if let Some(c) = &a.city {
            x.leaf("ram:CityName", &[], c);
        }
        if let Some(c) = &a.country {
            x.leaf("ram:CountryID", &[], c.as_str());
        }
        if let Some(s) = &a.subdivision {
            x.leaf("ram:CountrySubDivisionName", &[], s);
        }
    });
}

// ---------------------------------------------------------------------------
// header: delivery
// ---------------------------------------------------------------------------

fn delivery(x: &mut Xml, inv: &Invoice) {
    // `ram:ApplicableHeaderTradeDelivery` is mandatory in D16B (no
    // `minOccurs`) and may be empty. KoSIT's CII translation of Peppol R008
    // exempts it from the empty-element rule:
    //
    //   //*[not(name() = 'ram:ApplicableHeaderTradeDelivery')
    //      and not(*) and not(normalize-space())]
    //
    // Do not omit it: that fails the XSD.
    x.group_required("ram:ApplicableHeaderTradeDelivery", |x| {
        if let Some(d) = &inv.delivery {
            if d.party_name.is_some() || d.location.is_some() || d.address.is_some() {
                x.group("ram:ShipToTradeParty", |x| {
                    if let Some(l) = &d.location {
                        if l.scheme().is_some() {
                            ident(x, "ram:GlobalID", l);
                        } else {
                            x.leaf("ram:ID", &[], l.content());
                        }
                    }
                    if let Some(n) = &d.party_name {
                        x.leaf("ram:Name", &[], n);
                    }
                    if let Some(a) = &d.address {
                        address(x, a);
                    }
                });
            }
            if let Some(date_) = d.date {
                x.group("ram:ActualDeliverySupplyChainEvent", |x| {
                    date(x, "ram:OccurrenceDateTime", date_);
                });
            }
        }
        doc_ref(
            x,
            "ram:DespatchAdviceReferencedDocument",
            inv.despatch_advice_reference.as_ref(),
        );
        doc_ref(
            x,
            "ram:ReceivingAdviceReferencedDocument",
            inv.receiving_advice_reference.as_ref(),
        );
    });
}

// ---------------------------------------------------------------------------
// header: settlement — money, tax, totals
// ---------------------------------------------------------------------------

fn settlement(x: &mut Xml, inv: &Invoice, ccy: &str) {
    // BT-8 in CII's spelling (UNTDID 2475). A value with no CII spelling is
    // not written; `BR-CL-06` reports it on the model.
    let bt8 = inv.vat_point_date_code.as_ref().and_then(|c| {
        let cii = super::vat_point_date_code_to_cii(c.as_str());
        if cii.is_none() {
            x.note(
                Entry::new(
                    Kind::Dropped,
                    "ram:ApplicableTradeTax/ram:DueDateTypeCode",
                    "no CII spelling — ram:DueDateTypeCode carries 5, 29 or 72 for the \
                     model's 3, 35 or 432 (BR-CL-06 reports it on the model)",
                )
                .term("BT-8")
                .value(c.as_str()),
            );
        }
        cii
    });
    x.group_required("ram:ApplicableHeaderTradeSettlement", |x| {
        if let Some(p) = &inv.payment
            && let Some(PaymentMeans::DirectDebit(d)) = &p.means
            && let Some(c) = &d.creditor_identifier
        {
            x.leaf("ram:CreditorReferenceID", &[], c);
        }
        if let Some(p) = &inv.payment
            && let Some(r) = &p.remittance_information
        {
            x.leaf("ram:PaymentReference", &[], r);
        }
        if let Some(c) = &inv.vat_accounting_currency {
            x.leaf("ram:TaxCurrencyCode", &[], c.as_str());
        }
        if !ccy.is_empty() {
            x.leaf("ram:InvoiceCurrencyCode", &[], ccy);
        }
        if let Some(p) = &inv.payee {
            x.group("ram:PayeeTradeParty", |x| {
                if let Some(i) = &p.identifier {
                    if i.scheme().is_some() {
                        ident(x, "ram:GlobalID", i);
                    } else {
                        x.leaf("ram:ID", &[], i.content());
                    }
                }
                if let Some(n) = &p.name {
                    x.leaf("ram:Name", &[], n);
                }
                if let Some(l) = &p.legal_registration {
                    x.group("ram:SpecifiedLegalOrganization", |x| ident(x, "ram:ID", l));
                }
            });
        }
        if let Some(p) = &inv.payment {
            payment_means(x, p);
        }
        for (i, b) in inv.vat_breakdown.iter().enumerate() {
            x.group("ram:ApplicableTradeTax", |x| {
                x.leaf("ram:CalculatedAmount", &[], &amt(b.tax_amount));
                x.leaf("ram:TypeCode", &[], "VAT");
                if let Some(r) = &b.exemption_reason {
                    x.leaf("ram:ExemptionReason", &[], r);
                }
                x.leaf("ram:BasisAmount", &[], &amt(b.taxable_amount));
                x.leaf("ram:CategoryCode", &[], b.category.as_str());
                if let Some(c) = &b.exemption_reason_code {
                    x.leaf("ram:ExemptionReasonCode", &[], c.as_str());
                }
                // BT-7 and BT-8 sit on the tax breakdown. BT-8 may repeat if
                // identical (`CII-SR-462`); BT-7 only once in the document
                // (`CII-SR-461`), so it goes on the first entry only.
                if let Some(c) = bt8 {
                    x.leaf("ram:DueDateTypeCode", &[], c);
                }
                if i == 0
                    && let Some(d) = inv.vat_point_date
                {
                    plain_date(x, "ram:TaxPointDate", d);
                }
                if let Some(r) = b.rate {
                    x.leaf("ram:RateApplicablePercent", &[], &r.to_string());
                }
            });
        }
        if let Some(p) = &inv.invoicing_period {
            period(x, "ram:BillingSpecifiedPeriod", p, "BG-14");
        }
        for (a, is_charge) in inv
            .allowances
            .iter()
            .map(|a| (a, false))
            .chain(inv.charges.iter().map(|c| (c, true)))
        {
            x.group("ram:SpecifiedTradeAllowanceCharge", |x| {
                charge_indicator(x, is_charge);
                if let Some(p) = a.percentage {
                    x.leaf("ram:CalculationPercent", &[], &p.to_string());
                }
                if let Some(b) = a.base_amount {
                    x.leaf("ram:BasisAmount", &[], &amt(b));
                }
                x.leaf("ram:ActualAmount", &[], &amt(a.amount));
                if let Some(c) = &a.reason_code {
                    x.leaf("ram:ReasonCode", &[], c.as_str());
                }
                if let Some(r) = &a.reason {
                    x.leaf("ram:Reason", &[], r);
                }
                x.group("ram:CategoryTradeTax", |x| {
                    x.leaf("ram:TypeCode", &[], "VAT");
                    x.leaf("ram:CategoryCode", &[], a.vat.category.as_str());
                    if let Some(r) = a.vat.rate {
                        x.leaf("ram:RateApplicablePercent", &[], &r.to_string());
                    }
                });
            });
        }
        // BT-89 lives in the payment terms too, so it alone is reason to write them.
        let mandate = inv.payment.as_ref().and_then(|p| match &p.means {
            Some(PaymentMeans::DirectDebit(dd)) => dd.mandate_reference.as_ref(),
            _ => None,
        });
        if inv.payment_terms.is_some() || inv.due_date.is_some() || mandate.is_some() {
            x.group("ram:SpecifiedTradePaymentTerms", |x| {
                if let Some(t) = &inv.payment_terms {
                    x.leaf("ram:Description", &[], t);
                }
                if let Some(d) = inv.due_date {
                    date(x, "ram:DueDateDateTime", d);
                }
                if let Some(m) = mandate {
                    x.leaf("ram:DirectDebitMandateID", &[], m);
                }
            });
        }
        let t = &inv.totals;
        x.group("ram:SpecifiedTradeSettlementHeaderMonetarySummation", |x| {
            x.leaf("ram:LineTotalAmount", &[], &amt(t.line_total));
            if let Some(c) = t.charge_total {
                x.leaf("ram:ChargeTotalAmount", &[], &amt(c));
            }
            if let Some(a) = t.allowance_total {
                x.leaf("ram:AllowanceTotalAmount", &[], &amt(a));
            }
            x.leaf("ram:TaxBasisTotalAmount", &[], &amt(t.taxable_total));
            // `@currencyID` tells BT-110 from BT-111 (accounting currency).
            if let Some(v) = t.vat_total {
                x.leaf("ram:TaxTotalAmount", &[("currencyID", ccy)], &amt(v));
            }
            if let (Some(v), Some(c)) =
                (t.vat_total_accounting, inv.vat_accounting_currency.as_ref())
            {
                x.leaf("ram:TaxTotalAmount", &[("currencyID", c.as_str())], &amt(v));
            }
            if let Some(r) = t.rounding {
                x.leaf("ram:RoundingAmount", &[], &amt(r));
            }
            x.leaf("ram:GrandTotalAmount", &[], &amt(t.gross_total));
            if let Some(p) = t.paid {
                x.leaf("ram:TotalPrepaidAmount", &[], &amt(p));
            }
            x.leaf("ram:DuePayableAmount", &[], &amt(t.due));
        });
        // BG-3 is 0..n in the model but D16B allows one
        // `ram:InvoiceReferencedDocument`: a second fails the schema. The first
        // is written, the rest reported.
        if let Some((p, rest)) = inv.preceding_invoices.split_first() {
            x.group("ram:InvoiceReferencedDocument", |x| {
                x.leaf("ram:IssuerAssignedID", &[], p.reference.as_str());
                if let Some(d) = p.issue_date {
                    formatted_date(x, "ram:FormattedIssueDateTime", d);
                }
            });
            for (i, r) in rest.iter().enumerate() {
                x.note(
                    Entry::new(
                        Kind::Dropped,
                        "ram:ApplicableHeaderTradeSettlement/ram:InvoiceReferencedDocument",
                        "CII allows one ram:InvoiceReferencedDocument, so only the first \
                         preceding invoice is written",
                    )
                    .term(format!("BG-3[{}]", i + 1))
                    .value(r.reference.as_str())
                    .hint("UBL carries them all"),
                );
            }
        }
        if let Some(a) = &inv.accounting_reference {
            x.group("ram:ReceivableSpecifiedTradeAccountingAccount", |x| {
                x.leaf("ram:ID", &[], a);
            });
        }
    });
}

fn period(x: &mut Xml, wrapper: &str, p: &Period, what: &str) {
    // An empty BG-14 is pruned; reported, since the model distinguishes it
    // from an absent one (`BR-CO-19` / `BR-CO-20`).
    if p.start.is_none() && p.end.is_none() {
        x.note(
            Entry::new(
                Kind::Dropped,
                wrapper,
                "present with neither a start nor an end date, which this element cannot \
                 express (BR-CO-19 / BR-CO-20 report it on the model)",
            )
            .term(what),
        );
        return;
    }
    x.group(wrapper, |x| {
        if let Some(s) = p.start {
            date(x, "ram:StartDateTime", s);
        }
        if let Some(e) = p.end {
            date(x, "ram:EndDateTime", e);
        }
    });
}

fn payment_means(x: &mut Xml, p: &PaymentInstructions) {
    // BG-17 is 0..n but `ram:PayeePartyCreditorFinancialAccount` is 0..1 in
    // `ram:SpecifiedTradeSettlementPaymentMeans`, so each account gets its own
    // payment-means element.
    let head = |x: &mut Xml| {
        if let Some(c) = &p.means_code {
            x.leaf("ram:TypeCode", &[], c.as_str());
        }
        if let Some(t) = &p.means_text {
            x.leaf("ram:Information", &[], t);
        }
    };
    if let Some(PaymentMeans::CreditTransfer(ts)) = &p.means
        && !ts.is_empty()
    {
        for t in ts {
            x.group("ram:SpecifiedTradeSettlementPaymentMeans", |x| {
                head(x);
                if let Some(a) = &t.account_identifier {
                    x.group("ram:PayeePartyCreditorFinancialAccount", |x| {
                        x.leaf("ram:IBANID", &[], a);
                        if let Some(n) = &t.account_name {
                            x.leaf("ram:AccountName", &[], n);
                        }
                    });
                }
                if let Some(p) = &t.provider_identifier {
                    x.group("ram:PayeeSpecifiedCreditorFinancialInstitution", |x| {
                        x.leaf("ram:BICID", &[], p);
                    });
                }
            });
        }
        return;
    }
    x.group("ram:SpecifiedTradeSettlementPaymentMeans", |x| {
        head(x);
        match &p.means {
            Some(PaymentMeans::Card(c)) => {
                x.group("ram:ApplicableTradeSettlementFinancialCard", |x| {
                    if let Some(n) = &c.primary_account_number {
                        x.leaf("ram:ID", &[], n);
                    }
                    if let Some(h) = &c.holder_name {
                        x.leaf("ram:CardholderName", &[], h);
                    }
                });
            }
            Some(PaymentMeans::DirectDebit(d)) => {
                if let Some(a) = &d.debited_account {
                    x.group("ram:PayerPartyDebtorFinancialAccount", |x| {
                        x.leaf("ram:IBANID", &[], a);
                    });
                }
            }
            // An empty credit-transfer list: one bare payment-means element.
            Some(PaymentMeans::CreditTransfer(_)) | None => {}
        }
    });
}

// ---------------------------------------------------------------------------
// lines
// ---------------------------------------------------------------------------

fn line(x: &mut Xml, l: &InvoiceLine, i: usize, ccy: &str) {
    let _ = ccy;
    x.group("ram:IncludedSupplyChainTradeLineItem", |x| {
        x.group("ram:AssociatedDocumentLineDocument", |x| {
            x.leaf("ram:LineID", &[], &l.id);
            if let Some(n) = &l.note {
                x.group("ram:IncludedNote", |x| x.leaf("ram:Content", &[], n));
            }
        });
        x.group("ram:SpecifiedTradeProduct", |x| {
            let i = &l.item;
            if let Some(s) = &i.standard_identifier {
                ident(x, "ram:GlobalID", s);
            }
            if let Some(s) = &i.seller_identifier {
                x.leaf("ram:SellerAssignedID", &[], s);
            }
            if let Some(b) = &i.buyer_identifier {
                x.leaf("ram:BuyerAssignedID", &[], b);
            }
            if let Some(n) = &i.name {
                x.leaf("ram:Name", &[], n);
            }
            if let Some(d) = &i.description {
                x.leaf("ram:Description", &[], d);
            }
            for (j, a) in i.attributes.iter().enumerate() {
                // D16B requires both; an absent one is written empty and said
                // (`BR-54` already finds the attribute incomplete).
                for (el, term, v) in [
                    ("ram:Description", "BT-160", &a.name),
                    ("ram:Value", "BT-161", &a.value),
                ] {
                    if v.is_none() {
                        x.note(
                            Entry::new(
                                Kind::Derived,
                                format!("ram:SpecifiedTradeProduct/ram:ApplicableProductCharacteristic/{el}"),
                                "absent, and D16B requires the element: written empty",
                            )
                            .term(format!("BG-32[{}]/{term}", j + 1)),
                        );
                    }
                }
                x.group("ram:ApplicableProductCharacteristic", |x| {
                    x.leaf(
                        "ram:Description",
                        &[],
                        a.name.as_deref().unwrap_or_default(),
                    );
                    x.leaf("ram:Value", &[], a.value.as_deref().unwrap_or_default());
                });
            }
            for c in &i.classification_identifiers {
                x.group("ram:DesignatedProductClassification", |x| {
                    let mut attrs: Vec<(&str, &str)> = Vec::new();
                    if let Some(s) = c.scheme() {
                        attrs.push(("listID", s));
                    }
                    x.leaf("ram:ClassCode", &attrs, c.content());
                });
            }
            if let Some(c) = &i.origin_country {
                x.group("ram:OriginTradeCountry", |x| {
                    x.leaf("ram:ID", &[], c.as_str());
                });
            }
        });
        x.group("ram:SpecifiedLineTradeAgreement", |x| {
            if let Some(o) = &l.order_line_reference {
                x.group("ram:BuyerOrderReferencedDocument", |x| {
                    x.leaf("ram:LineID", &[], o.as_str());
                });
            }
            // BT-147 is an applied allowance inside BT-148's gross-price
            // aggregate; BT-146 is a sibling.
            if let Some(g) = l.price.gross_price {
                x.group("ram:GrossPriceProductTradePrice", |x| {
                    x.leaf("ram:ChargeAmount", &[], &g.to_string());
                    basis_quantity(x, &l.price);
                    if let Some(d) = l.price.price_discount {
                        x.group("ram:AppliedTradeAllowanceCharge", |x| {
                            charge_indicator(x, false);
                            x.leaf("ram:ActualAmount", &[], &d.to_string());
                        });
                    }
                });
            } else if l.price.price_discount.is_some() {
                // BT-147 without a gross price has nowhere to go in CII.
                x.note(
                    Entry::new(
                        Kind::Dropped,
                        "ram:GrossPriceProductTradePrice/ram:AppliedTradeAllowanceCharge",
                        "CII nests the discount inside the gross price, and BT-148 is absent",
                    )
                    .term(format!("BG-25[{i}]/BT-147"))
                    .value(
                        l.price
                            .price_discount
                            .map(|d| d.to_string())
                            .unwrap_or_default(),
                    ),
                );
            }
            x.group("ram:NetPriceProductTradePrice", |x| {
                x.leaf("ram:ChargeAmount", &[], &l.price.net_price.to_string());
                basis_quantity(x, &l.price);
            });
        });
        x.group("ram:SpecifiedLineTradeDelivery", |x| {
            x.leaf(
                "ram:BilledQuantity",
                &[("unitCode", l.unit_code.as_str())],
                &l.quantity.to_string(),
            );
        });
        x.group("ram:SpecifiedLineTradeSettlement", |x| {
            x.group("ram:ApplicableTradeTax", |x| {
                x.leaf("ram:TypeCode", &[], "VAT");
                x.leaf("ram:CategoryCode", &[], l.vat.category.as_str());
                if let Some(r) = l.vat.rate {
                    x.leaf("ram:RateApplicablePercent", &[], &r.to_string());
                }
            });
            if let Some(p) = &l.period {
                period(x, "ram:BillingSpecifiedPeriod", p, "BG-26");
            }
            for (a, is_charge) in l
                .allowances
                .iter()
                .map(|a| (a, false))
                .chain(l.charges.iter().map(|c| (c, true)))
            {
                x.group("ram:SpecifiedTradeAllowanceCharge", |x| {
                    charge_indicator(x, is_charge);
                    if let Some(p) = a.percentage {
                        x.leaf("ram:CalculationPercent", &[], &p.to_string());
                    }
                    if let Some(b) = a.base_amount {
                        x.leaf("ram:BasisAmount", &[], &amt(b));
                    }
                    x.leaf("ram:ActualAmount", &[], &amt(a.amount));
                    if let Some(c) = &a.reason_code {
                        x.leaf("ram:ReasonCode", &[], c.as_str());
                    }
                    if let Some(r) = &a.reason {
                        x.leaf("ram:Reason", &[], r);
                    }
                });
            }
            x.group("ram:SpecifiedTradeSettlementLineMonetarySummation", |x| {
                x.leaf("ram:LineTotalAmount", &[], &amt(l.net_amount));
            });
            if let Some(o) = &l.object_identifier {
                x.group("ram:AdditionalReferencedDocument", |x| {
                    x.leaf("ram:IssuerAssignedID", &[], o.content());
                    x.leaf("ram:TypeCode", &[], "130");
                    if let Some(s) = o.scheme() {
                        x.leaf("ram:ReferenceTypeCode", &[], s);
                    }
                });
            }
            if let Some(a) = &l.accounting_reference {
                x.group("ram:ReceivableSpecifiedTradeAccountingAccount", |x| {
                    x.leaf("ram:ID", &[], a);
                });
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_wrapped_and_formatted() {
        let mut inv = Invoice::default();
        inv.issue_date = Some(Date::parse("2026-01-15").expect("date"));
        let out = write(&inv);
        assert!(
            out.xml
                .contains("<udt:DateTimeString format=\"102\">20260115</udt:DateTimeString>"),
            "{}",
            out.xml
        );
    }

    /// The document without its indentation, so a nested fragment can be
    /// matched whole.
    fn flat(xml: &str) -> String {
        xml.lines().map(str::trim).collect()
    }

    /// BT-26 sits in a `qdt:FormattedDateTimeType`, not a `udt:DateTimeType`.
    #[test]
    fn the_preceding_invoice_date_is_a_qualified_date() {
        let mut inv = Invoice::default();
        inv.preceding_invoices = vec![PrecedingInvoice {
            reference: DocumentReference::new("INV-1"),
            issue_date: Some(Date::parse("2026-10-01").expect("date")),
        }];
        let out = write(&inv);
        assert!(
            flat(&out.xml).contains(
                "<ram:FormattedIssueDateTime><qdt:DateTimeString format=\"102\">20261001</qdt:DateTimeString></ram:FormattedIssueDateTime>"
            ),
            "{}",
            out.xml
        );
    }

    /// D16B allows one `ram:InvoiceReferencedDocument`; BG-3 beyond the first
    /// is reported, not written.
    #[test]
    fn only_the_first_preceding_invoice_is_written() {
        let mut inv = Invoice::default();
        inv.preceding_invoices = ["INV-1", "INV-2", "INV-3"]
            .into_iter()
            .map(|r| PrecedingInvoice {
                reference: DocumentReference::new(r),
                issue_date: None,
            })
            .collect();
        let out = write(&inv);
        assert_eq!(
            out.xml.matches("<ram:InvoiceReferencedDocument>").count(),
            1,
            "{}",
            out.xml
        );
        assert!(
            out.xml
                .contains("<ram:IssuerAssignedID>INV-1</ram:IssuerAssignedID>")
        );
        // BT-1 and BT-2 are absent here too; only the BG-3 note is this test's.
        let lost: Vec<_> = out
            .entries
            .iter()
            .filter(|d| d.term.as_deref().is_some_and(|t| t.starts_with("BG-3")))
            .map(|d| d.value.as_deref().unwrap_or_default())
            .collect();
        assert_eq!(lost, ["INV-2", "INV-3"], "{:?}", out.entries);
    }

    /// BT-7 sits in a `udt:DateType`, whose child is `udt:DateString`.
    #[test]
    fn the_vat_point_date_is_a_date_string() {
        let mut inv = Invoice::default();
        inv.vat_point_date = Some(Date::parse("2026-01-04").expect("date"));
        // BT-7 rides on the breakdown in CII, so there must be one.
        inv.vat_breakdown = vec![VatBreakdown {
            taxable_amount: InvoiceAmount::parse("100.00").expect("amount"),
            tax_amount: InvoiceAmount::parse("19.00").expect("amount"),
            category: Code::new("S"),
            rate: None,
            exemption_reason: None,
            exemption_reason_code: None,
        }];
        let out = write(&inv);
        assert!(
            flat(&out.xml).contains(
                "<ram:TaxPointDate><udt:DateString format=\"102\">20260104</udt:DateString></ram:TaxPointDate>"
            ),
            "{}",
            out.xml
        );
    }

    /// One document element for both kinds — CII tells them apart by BT-3.
    #[test]
    fn a_credit_note_uses_the_same_root() {
        let mut inv = Invoice::default();
        inv.kind = en16931::DocumentKind::CreditNote;
        inv.type_code = Some(Code::new("381"));
        let out = write(&inv);
        assert!(out.xml.contains("<rsm:CrossIndustryInvoice"), "{}", out.xml);
        assert!(out.xml.contains("<ram:TypeCode>381</ram:TypeCode>"));
    }

    /// The delivery section is mandatory in the D16B sequence even when there
    /// is nothing to say, so it must survive `group`'s empty-aggregate pruning.
    #[test]
    fn the_mandatory_delivery_section_is_always_present() {
        let out = write(&Invoice::default());
        assert!(
            out.xml.contains("ram:ApplicableHeaderTradeDelivery"),
            "{}",
            out.xml
        );
    }

    #[test]
    fn an_empty_invoice_writes_the_three_parts() {
        let out = write(&Invoice::default());
        for part in [
            "rsm:ExchangedDocumentContext",
            "rsm:ExchangedDocument",
            "rsm:SupplyChainTradeTransaction",
        ] {
            assert!(out.xml.contains(part), "missing {part}\n{}", out.xml);
        }
        // D16B requires BT-1 and BT-2; their absence is reported, nothing else.
        let [bt1, bt2] = &out.entries[..] else {
            panic!("expected the BT-1 and BT-2 notes, got {:?}", out.entries);
        };
        assert_eq!(bt1.term.as_deref(), Some("BT-1"));
        assert_eq!(bt2.term.as_deref(), Some("BT-2"));
        assert!(bt1.kind == crate::report::Kind::Schema && bt2.kind == crate::report::Kind::Schema);
    }
}
