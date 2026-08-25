//! Writing the semantic model as UBL 2.1.
//!
//! # No parser, and no dependency
//!
//! Writing XML is escaping and element ordering. There is nothing to parse, so
//! this module pulls in no crate at all — the same reasoning that put SVRL
//! output inside `en16931` rather than beside it.
//!
//! # The two things a writer gets wrong
//!
//! **Order.** UBL content models are XSD `sequence`s, so a document with the
//! right elements in the wrong order is invalid — and no Schematron rule
//! reports it, because ordering is the schema's job. The order used here is
//! [`super::order`]'s, derived from 320 authority instances, and `tests/order.rs`
//! asserts the output against it.
//!
//! **Elements that must not appear.** 1 218 of the 1 339 syntax rules say some
//! UBL element "shall not be used". A writer that only knows the EN 16931
//! subset cannot violate them — which is a claim, so `tests/subset.rs` walks
//! every element name this module can emit and checks it against the allowed
//! set. That converts 1 218 rules from "we believe we comply" into an
//! assertion, and it is the reason the writer is enumerated rather than
//! reflective.
//!
//! # Currency
//!
//! Every amount in UBL carries `@currencyID`, and getting it from BT-5 for some
//! amounts and BT-6 for others is [`en16931`]'s BT-110/BT-111 distinction. The
//! writer takes the document currency once and applies it uniformly except for
//! the VAT-accounting total, which is the single exception the standard makes.

use super::{order, prohibitions};
use crate::xml::{Xml, base64};
use en16931::invoice::{
    Code, DocumentAllowanceCharge, Invoice, InvoiceLine, Item, LineAllowanceCharge, LineVat, Party,
    PaymentInstructions, PaymentMeans, Period, PostalAddress, PriceDetails, SupportingDocument,
};
use en16931::{DocumentKind, DocumentReference, Identifier, InvoiceAmount};

/// UBL 2.1 namespace URIs. Fixed by OASIS; not configurable.
const NS_INVOICE: &str = "urn:oasis:names:specification:ubl:schema:xsd:Invoice-2";
const NS_CREDIT_NOTE: &str = "urn:oasis:names:specification:ubl:schema:xsd:CreditNote-2";
const NS_CAC: &str = "urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2";
const NS_CBC: &str = "urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2";

/// How the serialiser answers UBL's two questions.
static RULES: crate::xml::Rules = crate::xml::Rules {
    order: order::children_of,
    forbidden_path: prohibitions::forbidden_path,
    forbidden_attribute: prohibitions::forbidden_attribute,
};

/// Amounts, formatted as UBL wants them.
///
/// `InvoiceAmount`'s `Display` already yields the two-decimal form the
/// `BR-DEC-*` rules require, so this is a rename rather than a conversion — but
/// it is a rename worth naming, because "the model's `Display` happens to be
/// the wire format" is the kind of coincidence that stops being true silently.
fn amt(a: InvoiceAmount) -> String {
    a.to_string()
}

/// What writing produced.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Written {
    /// The document.
    pub xml: String,
    /// Terms the target document element has no place for, as `Parent/child`.
    ///
    /// Empty for an ordinary invoice. Non-empty means the model carried
    /// something UBL cannot express *here* — BT-11 on a credit note is the
    /// known case — and the caller is told rather than left to discover it at
    /// the counterparty.
    pub dropped: Vec<String>,
}

/// Write an invoice as UBL 2.1, reporting anything the syntax could not carry.
///
/// The document element is `Invoice` or `CreditNote` according to
/// [`Invoice::kind`], because UBL has two roots where CII has one.
#[must_use]
pub fn write(inv: &Invoice) -> Written {
    write_waiving(inv, &[])
}

/// As [`fn@write`], permitting the named prohibitions.
///
/// `waived` comes from the target profile's declared extension capability — see
/// [`crate::ubl::write_for`]. Core EN 16931 waives nothing, so the extension
/// groups are dropped and reported exactly as any other out-of-subset element.
pub(crate) fn write_waiving(inv: &Invoice, waived: &'static [&'static str]) -> Written {
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
    opt(&mut x, "cbc:ID", inv.number.as_deref());
    if let Some(d) = inv.issue_date {
        x.leaf("cbc:IssueDate", &[], &d.to_string());
    }
    // BT-9 is written unconditionally even though UBL's `<CreditNote>` has no
    // `cbc:DueDate`. Skipping it here would drop a payment due date *silently*;
    // letting the sequence table drop it means the caller is told. One rule,
    // applied in one place, is the whole reason ordering became structural.
    if let Some(d) = inv.due_date {
        x.leaf("cbc:DueDate", &[], &d.to_string());
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
        // dangling SalesOrderID.
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
    doc_ref(
        &mut x,
        "cac:ProjectReference",
        inv.project_reference.as_ref(),
    );

    // BT-90 rides on the seller in UBL; see `party`.
    let sepa_creditor = match inv.payment.as_ref().and_then(|p| p.means.as_ref()) {
        Some(PaymentMeans::DirectDebit(d)) => d.creditor_identifier.as_deref(),
        _ => None,
    };
    party(
        &mut x,
        "cac:AccountingSupplierParty",
        &inv.seller,
        sepa_creditor,
    );
    party(&mut x, "cac:AccountingCustomerParty", &inv.buyer, None);
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
    // A BG-19 whose only populated term is BT-90 has no group to live in: UBL
    // carries BT-90 on the *seller*, and `cac:PaymentMandate` with no children is
    // pruned — so the direct debit's *presence* is lost even though its data is
    // not. Named, because `BR-DE-30` and `BR-DE-31` are about that group.
    if let Some(PaymentMeans::DirectDebit(d)) = inv.payment.as_ref().and_then(|p| p.means.as_ref())
        && d.mandate_reference.is_none()
        && d.debited_account.is_none()
    {
        x.dropped(
            "BG-19 DIRECT DEBIT carries only BT-90, which UBL puts on the seller — \
             cac:PaymentMandate has nothing to hold, so the group does not survive \
             (BR-DE-30 / BR-DE-31 report the missing terms on the model)"
                .to_owned(),
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
    if !inv.vat_breakdown.is_empty() || inv.totals.vat_total.is_some() {
        x.group("cac:TaxTotal", |x| {
            if let Some(t) = inv.totals.vat_total {
                x.leaf("cbc:TaxAmount", &[("currencyID", ccy)], &amt(t));
            }
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
    // BT-111: the VAT total in the accounting currency is a *second*
    // cac:TaxTotal carrying only an amount, in BT-6's currency. This is the
    // one place the document currency does not apply.
    if let (Some(t), Some(c)) = (
        inv.totals.vat_total_accounting,
        inv.vat_accounting_currency.as_ref(),
    ) {
        x.group("cac:TaxTotal", |x| {
            x.leaf("cbc:TaxAmount", &[("currencyID", c.as_str())], &amt(t));
        });
    }

    // BG-DEX-09, the XRechnung Extension's third-party payment. `UBL-CR-470`
    // forbids `cac:PrepaidPayment` in core EN 16931, so this is emitted and the
    // serialiser decides: dropped and reported for a core document, kept for a
    // profile that declares the group. Never silently: this is the data
    // `EN-EXT-01` exists to warn about, because in Germany losing it is a §14c
    // Abs. 1 UStG liability.
    for (i, p) in inv.extensions.third_party_payments.iter().enumerate() {
        // All three terms absent is a group with no children, which the
        // serialiser prunes — correctly, since `<cac:PrepaidPayment/>` asserts
        // nothing. `BR-DEX-10` … `-12` are about exactly that document, and
        // KoSIT ships a negative instance of it, so the loss is named.
        if p.payment_type.is_none() && p.amount.is_none() && p.description.is_none() {
            x.dropped(format!(
                "BG-DEX-09[{i}] is present and empty, which cac:PrepaidPayment \
                 cannot express (BR-DEX-10 … -12 report it on the model)"
            ));
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

    for (i, l) in inv.lines.iter().enumerate() {
        line(&mut x, l, i, inv.extensions.sub_lines(i), credit, ccy);
    }

    let (xml, dropped) = x.finish();
    Written { xml, dropped }
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
    // A group with no children is pruned by the serialiser, which is right —
    // `<cac:InvoicePeriod/>` asserts nothing. But the *model* distinguishes an
    // absent BG-14 from a present-and-empty one, and `BR-CO-19` / `BR-CO-20`
    // exist for exactly the second case. So the loss is reported rather than
    // left for a reader to notice that a finding stopped firing.
    if p.start.is_none() && p.end.is_none() && code.is_none() {
        x.dropped(format!(
            "{what} is present with neither a start nor an end date, which \
             {name} cannot express (BR-CO-19 / BR-CO-20 report it on the model)"
        ));
        return;
    }
    x.group(name, |x| {
        if let Some(s) = p.start {
            x.leaf("cbc:StartDate", &[], &s.to_string());
        }
        if let Some(e) = p.end {
            x.leaf("cbc:EndDate", &[], &e.to_string());
        }
        // BT-8 rides on the invoice period as a description code — UBL has no
        // element for it elsewhere.
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
    // `group` drops an aggregate whose body writes nothing, so a blank address
    // needs no separate emptiness check — one rule, not two that can disagree.
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
    // BT-163 goes into cac:AddressLine — UBL has exactly one such element in
    // the EN 16931 subset, which is why the model has a single line3.
    if let Some(l) = &a.line3 {
        x.group("cac:AddressLine", |x| x.leaf("cbc:Line", &[], l));
    }
    if let Some(c) = &a.country {
        x.group("cac:Country", |x| {
            x.leaf("cbc:IdentificationCode", &[], c.as_str());
        });
    }
}

/// A party, and — for the seller only — BT-90.
///
/// # The hop back
///
/// UBL has no element for BT-90 inside BG-19. It carries the bank-assigned
/// creditor identifier as a **seller** `cac:PartyIdentification` with
/// `schemeID="SEPA"`, which is the one place `BR-CL-10` admits a scheme outside
/// ISO 6523 — and only under the supplier or the payee.
///
/// `super::read` already hops it the other way, into BG-19 where `BR-DE-30` can
/// see it. This writer did not hop it back, so **every direct-debit invoice
/// written as UBL lost BT-90 in silence** and failed `BR-DE-30` at the
/// counterparty. A reader and a writer that disagree about where a term lives is
/// exactly the asymmetry `tests/cross_syntax.rs` exists to find: CII keeps BT-90
/// in BG-19, so the loss only shows when a document crosses.
fn party(x: &mut Xml, wrapper: &str, p: &Party, sepa_creditor: Option<&str>) {
    x.group(wrapper, |x| {
        x.group("cac:Party", |x| {
            if let Some(e) = &p.electronic_address {
                ident(x, "cbc:EndpointID", e);
            }
            for i in &p.identifiers {
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
                // A non-VAT registration is the same element with a scheme id
                // that is *not* `VAT` — which is what `UBL-SR-13` counts.
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
    });
}

fn payment_means(x: &mut Xml, p: &PaymentInstructions) {
    // BG-17 is 0..n and UBL puts `cac:PayeeFinancialAccount` at **0..1** inside
    // `cac:PaymentMeans` (OASIS UBL 2.1, `PaymentMeansType`) — so several
    // credit-transfer accounts are several `cac:PaymentMeans` elements, each
    // repeating BT-81 and BT-83. That is how CEN's own `guide-example1.xml`
    // spells two accounts. One aggregate holding two accounts reads naturally
    // and fails the OASIS schema; this writer emitted exactly that for two
    // releases, and the reader kept only the last element, so the loss was
    // invisible to a round trip.
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
                    // UBL makes cbc:NetworkID mandatory inside cac:CardAccount
                    // and EN 16931 has no term for it. `NA` is what CEN's own
                    // examples carry.
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
            // An *empty* credit-transfer list falls through to here: there is
            // no account to carry, so one bare `cac:PaymentMeans` with the
            // code and payment id is all the document can honestly say —
            // exactly as for no means at all.
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

fn line(
    x: &mut Xml,
    l: &InvoiceLine,
    i: usize,
    subs: &[en16931::SubInvoiceLine],
    credit: bool,
    ccy: &str,
) {
    let root = if credit {
        "cac:CreditNoteLine"
    } else {
        "cac:InvoiceLine"
    };
    let qty_name = if credit {
        "cbc:CreditedQuantity"
    } else {
        "cbc:InvoicedQuantity"
    };
    x.group(root, |x| {
        x.leaf("cbc:ID", &[], &l.id);
        if let Some(n) = &l.note {
            x.leaf("cbc:Note", &[], n);
        }
        x.leaf(
            qty_name,
            &[("unitCode", l.unit_code.as_str())],
            &l.quantity.to_string(),
        );
        x.leaf(
            "cbc:LineExtensionAmount",
            &[("currencyID", ccy)],
            &amt(l.net_amount),
        );
        if let Some(a) = &l.accounting_reference {
            x.leaf("cbc:AccountingCost", &[], a);
        }
        if let Some(p) = &l.period {
            period(x, "cac:InvoicePeriod", p, None, "BG-26");
        }
        if let Some(o) = &l.order_line_reference {
            x.group("cac:OrderLineReference", |x| {
                x.leaf("cbc:LineID", &[], o.as_str());
            });
        }
        if let Some(o) = &l.object_identifier {
            x.group("cac:DocumentReference", |x| {
                ident(x, "cbc:ID", o);
                x.leaf("cbc:DocumentTypeCode", &[], "130");
            });
        }
        for (a, c) in l
            .allowances
            .iter()
            .map(|a| (a, false))
            .chain(l.charges.iter().map(|c| (c, true)))
        {
            line_allowance(x, a, c, ccy);
        }
        item(x, &l.item, &l.vat);
        price(x, &l.price, i, ccy);
        // BG-DEX-01, and recursively. `UBL-CR-646` forbids `cac:SubInvoiceLine`
        // in core EN 16931 — that rule was among the 131 the extractor could
        // not read until it learned `(cac:InvoiceLine|cac:CreditNoteLine)/x`,
        // which is why emitting this was silently fine before and is now
        // correctly dropped-and-reported for a core document.
        for s in subs {
            sub_line(x, s, i, credit, ccy);
        }
    });
}

/// One `cac:SubInvoiceLine`, with its own children beneath it.
fn sub_line(x: &mut Xml, s: &en16931::SubInvoiceLine, i: usize, credit: bool, ccy: &str) {
    x.group("cac:SubInvoiceLine", |x| {
        x.leaf("cbc:ID", &[], &s.line.id);
        if let Some(n) = &s.line.note {
            x.leaf("cbc:Note", &[], n);
        }
        let qty_name = if credit {
            "cbc:CreditedQuantity"
        } else {
            "cbc:InvoicedQuantity"
        };
        x.leaf(
            qty_name,
            &[("unitCode", s.line.unit_code.as_str())],
            &s.line.quantity.to_string(),
        );
        x.leaf(
            "cbc:LineExtensionAmount",
            &[("currencyID", ccy)],
            &amt(s.line.net_amount),
        );
        item(x, &s.line.item, &s.line.vat);
        price(x, &s.line.price, i, ccy);
        for child in &s.children {
            sub_line(x, child, i, credit, ccy);
        }
    });
}

fn item(x: &mut Xml, i: &Item, vat: &LineVat) {
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
                // BT-158's scheme is `@listID`, not `@schemeID` — the one
                // element in the EN 16931 subset that differs. `@schemeID` here
                // is well-formed, schema-valid and silently wrong: the reader
                // finds no scheme and BT-158-1 vanishes.
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
        tax_category(x, vat, "cac:ClassifiedTaxCategory");
        for a in &i.attributes {
            x.group("cac:AdditionalItemProperty", |x| {
                x.leaf("cbc:Name", &[], a.name.as_deref().unwrap_or_default());
                x.leaf("cbc:Value", &[], a.value.as_deref().unwrap_or_default());
            });
        }
    });
}

fn price(x: &mut Xml, p: &PriceDetails, i: usize, ccy: &str) {
    // UBL makes `cbc:Amount` mandatory inside `cac:AllowanceCharge`, so BT-148
    // cannot be stated without BT-147. A gross price with no discount therefore
    // reads back as a discount of zero — the same figure `R046` computes, and
    // still a change to the model, so it is named.
    if p.price_discount.is_none() && p.gross_price.is_some() {
        x.dropped(format!(
            "BG-25[{i}]/BT-147 is absent and BT-148 is present; UBL requires \
             cac:Price/cac:AllowanceCharge/cbc:Amount, so it is written as 0.00 and \
             reads back as a stated zero discount"
        ));
    }
    x.group("cac:Price", |x| {
        x.leaf(
            "cbc:PriceAmount",
            &[("currencyID", ccy)],
            &p.net_price.to_string(),
        );
        if let Some(q) = p.base_quantity {
            // **No `unitCode` when BT-150 is absent**, rather than an empty one.
            //
            // An empty `unitCode` is not cosmetic: the reader takes it back as
            // `Some("")`, and `PEPPOL-EN16931-R130` — *"unit code of price base
            // quantity MUST be the same as invoiced quantity"* — then fires on
            // a document that was valid before the round trip.
            //
            // Omitting the attribute is what 29 of the published instances do,
            // `unitCode` is optional on UBL's `QuantityType`, and no `UBL-DT-*`
            // rule requires it here.
            let unit = p
                .base_quantity_code
                .as_ref()
                .map(Code::as_str)
                .filter(|u| !u.is_empty());
            let attrs: Vec<(&str, &str)> = unit.map(|u| ("unitCode", u)).into_iter().collect();
            x.leaf("cbc:BaseQuantity", &attrs, &q.to_string());
        }
        // BT-147 and BT-148 share one price-level allowance: `cbc:Amount` is the
        // discount and `cbc:BaseAmount` the gross price. Neither has an element
        // of its own, and UBL makes `cbc:Amount` mandatory inside the group.
        //
        // Written whenever **either** is present. Requiring both lost BT-147 on
        // seven of the published instances — a discount stated without a gross
        // price is ordinary, and `PEPPOL-EN16931-R046` only has anything to say
        // when the gross price is there.
        if p.price_discount.is_some() || p.gross_price.is_some() {
            x.group("cac:AllowanceCharge", |x| {
                x.leaf("cbc:ChargeIndicator", &[], "false");
                // A gross price with no discount means they are equal, which is
                // `R046` with a discount of zero — so zero is what it says
                // rather than what it omits. UBL makes `cbc:Amount` mandatory
                // inside the group, so there is no way to state one without the
                // other; the model's `None` therefore reads back as `Some(0)`,
                // and that is a change worth naming even though it means the
                // same thing.
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

    /// Ordering is applied at serialisation, so the writer may emit in any
    /// order it likes and still produce a schema-valid document.
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

    /// An element the parent's sequence has no place for is dropped *and
    /// reported* — never silently.
    #[test]
    fn an_unplaceable_element_is_reported() {
        let mut x = Xml::new("Invoice", vec![], &RULES);
        x.leaf("cbc:NotAUblElement", &[], "x");
        let (xml, dropped) = x.finish();
        assert!(!xml.contains("NotAUblElement"), "{xml}");
        assert_eq!(dropped, ["Invoice/cbc:NotAUblElement"]);
    }

    #[test]
    fn a_credit_note_uses_the_other_root() {
        let mut inv = Invoice::default();
        inv.kind = DocumentKind::CreditNote;
        let out = write(&inv);
        assert!(out.xml.contains("<CreditNote "), "{}", out.xml);
        assert!(out.xml.contains("CreditNote-2"));
        assert!(!out.xml.contains("cbc:InvoiceTypeCode"));
    }
}
