//! Writing the semantic model as UN/CEFACT CII D16B.
//!
//! Ordering and the prohibitions are the shared serialiser's job, so this module is
//! purely the *mapping*: which of CII's three document parts each business term
//! lives in. That is the part no amount of cleverness derives — it was read off
//! 170 published instances into [`super::order`], and this file places terms
//! into the structure that table describes.

use en16931::invoice::*;
use en16931::{Date, DocumentReference, Identifier, InvoiceAmount};

use super::{ns, order, prohibitions};
use crate::xml::{Xml, base64};

/// How the serialiser answers CII's two questions.
static RULES: crate::xml::Rules = crate::xml::Rules {
    order: order::children_of,
    forbidden_path: prohibitions::forbidden_path,
    forbidden_attribute: prohibitions::forbidden_attribute,
};

/// The only `udt:DateTimeString` format EN 16931 permits: `CCYYMMDD`.
pub(super) const DATE_FORMAT: &str = "102";

/// What writing produced.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Written {
    /// The document.
    pub xml: String,
    /// Terms CII has no place for, as `Parent/child`. Normally empty.
    pub dropped: Vec<String>,
}

/// An amount, as CII writes it.
fn amt(a: InvoiceAmount) -> String {
    a.to_string()
}

/// `<ram:Foo><udt:DateTimeString format="102">20260115</udt:DateTimeString></ram:Foo>`
///
/// CII never writes a bare date. Every one is wrapped and formatted, and the
/// format attribute is not optional — a `udt:DateTimeString` without it is
/// ambiguous between `CCYYMMDD` and half a dozen UN/EDIFACT 2379 codes.
fn date(x: &mut Xml, wrapper: &str, d: Date) {
    x.group(wrapper, |x| {
        x.leaf(
            "udt:DateTimeString",
            &[("format", DATE_FORMAT)],
            &format!("{:04}{:02}{:02}", d.year(), d.month(), d.day()),
        );
    });
}

/// BT-149 with BT-150, and **no `unitCode` when BT-150 is absent**.
///
/// Writing `unitCode=""` instead made the reader hand back `Some("")`, which
/// `PEPPOL-EN16931-R130` compares against BT-130 and rejects — a fatal finding
/// created by the round trip, on six of the published CII instances.
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

/// The `ram:ChargeIndicator` wrapper, which is how CII tells an allowance from
/// a charge — the same job UBL's `cbc:ChargeIndicator` does one level up.
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
/// There is one document element for both invoices and credit notes: CII
/// distinguishes them by BT-3 alone, so unlike [`fn@crate::ubl::write`] this
/// function never branches on [`en16931::DocumentKind`].
#[must_use]
pub fn write(inv: &Invoice) -> Written {
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
            x.group("ram:IncludedNote", |x| {
                x.leaf("ram:Content", &[], n.note.as_deref().unwrap_or_default());
                // BT-21 has an element of its own here, where UBL splices it
                // into the note text as `#AAI#`. CII is the tidier of the two.
                if let Some(c) = &n.subject_code {
                    x.leaf("ram:SubjectCode", &[], c.as_str());
                }
            });
        }
    });

    // CII has **one** document element where UBL has two, so the credit-note
    // distinction lives entirely in BT-3. A model that says "credit note" while
    // carrying an invoice type code therefore cannot be written down here — the
    // reader on the far side has nothing but BT-3 to go on and will read it back
    // as an invoice.
    //
    // That is a property of the syntax, not a bug, and it is exactly why UBL's
    // `<CreditNote>` root exists. What would be a bug is losing it in silence:
    // KoSIT ships negative instances of precisely this shape.
    if matches!(inv.kind, DocumentKind::CreditNote)
        && !inv
            .type_code
            .as_ref()
            .is_some_and(|c| c.is_in(en16931::codes::generated::CREDIT_NOTE_TYPE_CODES))
    {
        x.dropped(format!(
            "the document is a credit note and BT-3 is {}, which is not a UNTDID 1001 \
             credit-note code — CII carries the distinction in BT-3 alone, so it reads \
             back as an invoice (BR-CL-01 reports the same disagreement on the model)",
            inv.type_code
                .as_ref()
                .map_or("absent".to_owned(), |c| format!("{:?}", c.as_str()))
        ));
    }

    // BG-DEX-01 and BG-DEX-09 are the XRechnung Extension's, and this binding
    // does not write them. KoSIT ships a CII Extension scenario, so this is a
    // gap rather than an impossibility — named here so it cannot be mistaken for
    // an invoice that never carried the data.
    if !inv.extensions.sub_invoice_lines.is_empty() {
        x.dropped(
            "BG-DEX-01 SUB INVOICE LINE — the CII binding does not write the XRechnung \
             Extension groups yet; use UBL, where it does"
                .to_owned(),
        );
    }
    if !inv.extensions.third_party_payments.is_empty() {
        x.dropped(
            "BG-DEX-09 THIRD PARTY PAYMENT — the CII binding does not write the XRechnung \
             Extension groups yet; use UBL, where it does"
                .to_owned(),
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

    let (xml, dropped) = x.finish();
    Written { xml, dropped }
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
        // BT-17 and BT-18 ride on `AdditionalReferencedDocument` with a type
        // code, exactly as they ride on UBL's `AdditionalDocumentReference`.
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
                // `ram:Name` is mandatory inside this aggregate and EN 16931
                // has no term for it. CEN's own examples carry the reference
                // again rather than inventing a title.
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
/// `seller` selects the terms that exist only on the seller: BT-33 (additional
/// legal information) has no buyer counterpart, and writing one would be the
/// CII equivalent of the `UBL-CR-244` bug.
fn party(x: &mut Xml, wrapper: &str, p: &Party, seller: bool) {
    x.group(wrapper, |x| {
        for i in &p.identifiers {
            // A scheme-qualified identifier is `ram:GlobalID` with `@schemeID`;
            // an unqualified one is `ram:ID`. Two elements for one business
            // term, which is why the reader has to merge them back.
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
            // `FC` rather than `VA`: a non-VAT registration, which is what
            // BT-32 is and what the reader tells apart by the same code.
            x.group("ram:SpecifiedTaxRegistration", |x| {
                x.leaf("ram:ID", &[("schemeID", "FC")], t);
            });
        }
    });
}

fn address(x: &mut Xml, a: &PostalAddress) {
    // `group` drops an aggregate whose body writes nothing, so a blank address
    // needs no separate emptiness check — one rule, not two that can disagree.
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
    // `ram:ApplicableHeaderTradeDelivery` is mandatory in the D16B sequence
    // and may legitimately be empty — a minimal invoice delivers nothing.
    //
    // Empty is *correct*, not merely tolerated, and this has been questioned
    // from outside, so the evidence: the D16B XSD gives the element no
    // `minOccurs`, which defaults to 1 — omitting it fails schema validation
    // outright — and KoSIT carves exactly this element out of the
    // empty-element rule *by hand*. Peppol BIS publishes no CII Schematron at
    // all (it is UBL-only), so R008 reaches CII only through KoSIT's
    // translation, `xrechnung-schematron`'s src/xsl/peppol-into-xr.xsl —
    // which, under the comment "add R008 to CII", authors the context as
    //
    //   //*[not(name() = 'ram:ApplicableHeaderTradeDelivery')
    //      and not(*) and not(normalize-space())]
    //
    // A validator that flags `<ram:ApplicableHeaderTradeDelivery/>` under
    // R008 is applying Peppol's UBL-targeted rule to CII without the
    // authority's carve-out. Do not "fix" this by omitting the element: that
    // trades a spurious warning for a hard XSD failure.
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
        for b in &inv.vat_breakdown {
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
                // BT-7 and BT-8 sit on the *tax breakdown* in CII, not on the
                // document as they do in UBL. Repeated on each entry, which is
                // what the standard says and what the reader collapses.
                if let Some(c) = &inv.vat_point_date_code {
                    x.leaf("ram:DueDateTypeCode", &[], c.as_str());
                }
                if let Some(d) = inv.vat_point_date {
                    date(x, "ram:TaxPointDate", d);
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
        if inv.payment_terms.is_some() || inv.due_date.is_some() {
            x.group("ram:SpecifiedTradePaymentTerms", |x| {
                if let Some(t) = &inv.payment_terms {
                    x.leaf("ram:Description", &[], t);
                }
                if let Some(d) = inv.due_date {
                    date(x, "ram:DueDateDateTime", d);
                }
                if let Some(p) = &inv.payment
                    && let Some(PaymentMeans::DirectDebit(dd)) = &p.means
                    && let Some(m) = &dd.mandate_reference
                {
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
            // The one amount CII requires a currency on, and the reason is
            // BT-111: the VAT total may be stated in the accounting currency
            // as well, and the two are told apart by `@currencyID` alone.
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
        for p in &inv.preceding_invoices {
            x.group("ram:InvoiceReferencedDocument", |x| {
                x.leaf("ram:IssuerAssignedID", &[], p.reference.as_str());
                if let Some(d) = p.issue_date {
                    date(x, "ram:FormattedIssueDateTime", d);
                }
            });
        }
        if let Some(a) = &inv.accounting_reference {
            x.group("ram:ReceivableSpecifiedTradeAccountingAccount", |x| {
                x.leaf("ram:ID", &[], a);
            });
        }
    });
}

fn period(x: &mut Xml, wrapper: &str, p: &Period, what: &str) {
    // See the UBL writer's twin: a pruned empty group is right for the document
    // and loses the model's distinction between "absent" and "present and
    // empty", which `BR-CO-19` / `BR-CO-20` are about. Reported, not silent.
    if p.start.is_none() && p.end.is_none() {
        x.dropped(format!(
            "{what} is present with neither a start nor an end date, which \
             {wrapper} cannot express (BR-CO-19 / BR-CO-20 report it on the model)"
        ));
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
    // BG-17 is 0..n and D16B puts `ram:PayeePartyCreditorFinancialAccount` at
    // **0..1** inside `ram:SpecifiedTradeSettlementPaymentMeans`
    // (`TradeSettlementPaymentMeansType` in the XSD) — several accounts are
    // several payment-means elements, exactly as in UBL, where CEN's own
    // `guide-example1.xml` repeats `cac:PaymentMeans` per account. Two
    // accounts inside one aggregate fails the schema — and a reader that keeps
    // only the last element's accounts makes a round trip agree with itself
    // while losing one, which is why both halves are pinned by tests.
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
            // An empty credit-transfer list: no account to carry, so one bare
            // payment-means element with the code is all there is to say.
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
            for a in &i.attributes {
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
            // BT-148's gross price and BT-147's discount are one aggregate:
            // the discount is an *applied allowance* on the gross price, and
            // the net price BT-146 is a sibling. Same shape as UBL's
            // price-level `cac:AllowanceCharge`, differently named.
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
                // CII nests BT-147 **inside** the gross-price aggregate, so a
                // discount stated without a gross price has nowhere to go. UBL
                // can carry it; CII cannot. No table sees that, because it is a
                // property of the content model rather than of a prohibition —
                // so the writer says so itself.
                x.dropped(format!(
                    "BG-25[{i}]/BT-147 (ram:AppliedTradeAllowanceCharge needs \
                     ram:GrossPriceProductTradePrice, and BT-148 is absent)"
                ));
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
        assert!(out.dropped.is_empty(), "{:?}", out.dropped);
    }
}
