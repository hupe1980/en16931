//! Reading UN/CEFACT CII D16B into the semantic model.
//!
//! # It never silently drops anything
//!
//! Every element walked is either mapped or recorded in [`Reader::unmapped`],
//! as `Parent/Child`. A reader that quietly ignored `ram:TaxBasisTotalAmount`
//! would report a clean parse and prove nothing.
//!
//! # Malformed values are reported, not panicked on
//!
//! CEN's suites carry deliberately malformed values to exercise `BR-DEC-*` and
//! the date rules. [`en16931`]'s types make those unrepresentable, so the reader
//! records them in [`Reader::malformed`] and leaves the field absent. Losing the
//! distinction between "absent" and "present but unreadable" would make a
//! `BR-03` finding unexplainable.
//!
//! # Two shapes that have to be undone
//!
//! **`ram:ID` versus `ram:GlobalID`.** CII splits one business term across two
//! elements by whether it carries a scheme. Both are read back into
//! `Party::identifiers`, so the round-trip holds.
//!
//! **BT-7/BT-8 repeat.** The VAT point date and its code sit on *each* tax
//! breakdown entry rather than on the document. They are collapsed to the
//! document-level term, which is where the model puts them — and where the
//! standard's own semantics put them, since one invoice has one tax point.

use std::collections::BTreeSet;

use en16931::invoice::*;
use en16931::{Date, DocumentKind, DocumentReference, Identifier, InvoiceAmount, Quantity};

/// Reads one `rsm:CrossIndustryInvoice` into an [`Invoice`].
#[derive(Default)]
pub struct Reader {
    /// Element paths encountered and not mapped, as `Parent/Child`.
    pub unmapped: BTreeSet<String>,
    /// Values present in the document but not representable in the model.
    pub malformed: Vec<String>,
}

type Node<'a, 'i> = roxmltree::Node<'a, 'i>;

fn name<'i>(n: Node<'_, 'i>) -> &'i str {
    n.tag_name().name()
}

fn kids<'a, 'i>(n: Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> {
    n.children().filter(roxmltree::Node::is_element)
}

fn kid<'a, 'i>(n: Node<'a, 'i>, want: &str) -> Option<Node<'a, 'i>> {
    kids(n).find(|c| name(*c) == want)
}

fn own_text(n: Node<'_, '_>) -> String {
    n.text().unwrap_or_default().trim().to_owned()
}

/// Trimmed text of a direct child, or `None` when the element is absent **or
/// empty**.
///
/// # Why empty is absent
///
/// `<ram:Description/>` is not a description whose value is the empty string;
/// it is a description that is not there. The UBL reader has always said so —
/// `roxmltree` gives an empty element no text node, so its `text()` yields
/// `None` — and this one used to map the same element to `Some("")`.
///
/// Two readers of one model disagreeing about that is a real difference and not
/// a cosmetic one. Rules mostly paper over it (`is_none_or(str::is_empty)` reads
/// both the same way), but `Restriction::NotUsed` treats blank as absent, an
/// empty `DocumentReference` serialises back out as an empty element, and a
/// caller matching on `Option` sees two different documents. Nine of the
/// published CII instances differ from their UBL crossing on this alone.
fn text(n: Node<'_, '_>, want: &str) -> Option<String> {
    kid(n, want).map(own_text).filter(|t| !t.is_empty())
}

/// `None` for an empty string, so the two sites that read an element's own text
/// agree with [`text`] about what an empty element means.
fn non_empty(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

fn code(n: Node<'_, '_>, want: &str) -> Option<Code> {
    text(n, want).filter(|t| !t.is_empty()).map(Code::new)
}

fn decimal(n: Node<'_, '_>, want: &str) -> Option<rust_decimal::Decimal> {
    text(n, want).and_then(|t| t.parse().ok())
}

/// An `Identifier` from an element with an optional `@schemeID`.
fn identifier(n: Node<'_, '_>) -> Identifier {
    let content = own_text(n);
    match n.attribute("schemeID").or_else(|| n.attribute("listID")) {
        Some(s) => Identifier::schemed(content, s),
        None => Identifier::new(content),
    }
}

impl Reader {
    fn amount(&mut self, n: Node<'_, '_>, want: &str) -> Option<InvoiceAmount> {
        let raw = text(n, want)?;
        match InvoiceAmount::parse(&raw) {
            Ok(a) => Some(a),
            Err(e) => {
                self.malformed.push(format!("{want}: {raw:?} ({e})"));
                None
            }
        }
    }

    /// `<ram:Foo><udt:DateTimeString format="102">20260115</udt:DateTimeString></ram:Foo>`
    ///
    /// A format other than `102` is *reported*, not guessed at: `20260103`
    /// under format `101` (`YYMMDD`) would silently become a date six years
    /// wrong, and the standard permits only `102`.
    fn date(&mut self, n: Node<'_, '_>, want: &str) -> Option<Date> {
        let wrapper = kid(n, want)?;
        let s = kid(wrapper, "DateTimeString")?;
        let raw = own_text(s);
        match s.attribute("format") {
            Some(super::write::DATE_FORMAT) | None => {}
            Some(other) => {
                self.malformed
                    .push(format!("{want}: format {other:?}, only 102 is permitted"));
                return None;
            }
        }
        if raw.len() != 8 || !raw.bytes().all(|b| b.is_ascii_digit()) {
            self.malformed
                .push(format!("{want}: {raw:?} is not CCYYMMDD"));
            return None;
        }
        let f = format!("{}-{}-{}", &raw[0..4], &raw[4..6], &raw[6..8]);
        match Date::parse(&f) {
            Ok(d) => Some(d),
            Err(e) => {
                self.malformed.push(format!("{want}: {raw:?} ({e})"));
                None
            }
        }
    }

    fn note(&mut self, parent: &str, n: Node<'_, '_>) {
        for c in kids(n) {
            if !matches!(name(c), "Content" | "SubjectCode") {
                self.unmapped.push_path(parent, name(c));
            }
        }
    }

    /// Read the document element.
    pub fn read(&mut self, root: Node<'_, '_>) -> Invoice {
        let mut inv = Invoice::default();
        for c in kids(root) {
            match name(c) {
                "ExchangedDocumentContext" => self.context(&mut inv, c),
                "ExchangedDocument" => self.document(&mut inv, c),
                "SupplyChainTradeTransaction" => self.transaction(&mut inv, c),
                other => self.unmapped.push_path("CrossIndustryInvoice", other),
            }
        }
        // CII has one document element, so the kind comes from BT-3 alone —
        // and from the whole UNTDID 1001 credit-note list, not just `381`.
        //
        // Checking only `381` was wrong in a way the corpus could not show:
        // every published CII credit note uses `381`, so nothing exercised
        // `396` (factored credit note), `532` (forwarder's credit note) or `83`.
        // Those read back as *invoices*, and `BR-CL-01` then checked them
        // against the 50 invoice codes and reported a violation that is not one.
        //
        // `81` is on **both** lists. It is read as a credit note here, which is
        // what `PEPPOL-EN16931-P0101` expects and what
        // `rules::peppol::is_credit_note` already assumes — one reading, in two
        // places, rather than two.
        if inv
            .type_code
            .as_ref()
            .is_some_and(|c| c.is_in(en16931::codes::generated::CREDIT_NOTE_TYPE_CODES))
        {
            inv.kind = DocumentKind::CreditNote;
        }
        inv
    }

    fn context(&mut self, inv: &mut Invoice, n: Node<'_, '_>) {
        for c in kids(n) {
            match name(c) {
                "BusinessProcessSpecifiedDocumentContextParameter" => {
                    inv.business_process = text(c, "ID");
                }
                "GuidelineSpecifiedDocumentContextParameter" => {
                    inv.specification_id = text(c, "ID");
                }
                other => self.unmapped.push_path("ExchangedDocumentContext", other),
            }
        }
    }

    fn document(&mut self, inv: &mut Invoice, n: Node<'_, '_>) {
        for c in kids(n) {
            match name(c) {
                "ID" => inv.number = non_empty(own_text(c)),
                "TypeCode" => inv.type_code = Some(Code::new(own_text(c))),
                "IssueDateTime" => inv.issue_date = self.date(n, "IssueDateTime"),
                "IncludedNote" => {
                    self.note("IncludedNote", c);
                    inv.notes.push(InvoiceNote {
                        subject_code: code(c, "SubjectCode"),
                        note: text(c, "Content"),
                    });
                }
                other => self.unmapped.push_path("ExchangedDocument", other),
            }
        }
    }

    fn transaction(&mut self, inv: &mut Invoice, n: Node<'_, '_>) {
        for c in kids(n) {
            match name(c) {
                "IncludedSupplyChainTradeLineItem" => inv.lines.push(self.line(c)),
                "ApplicableHeaderTradeAgreement" => self.agreement(inv, c),
                "ApplicableHeaderTradeDelivery" => self.delivery(inv, c),
                "ApplicableHeaderTradeSettlement" => self.settlement(inv, c),
                other => self
                    .unmapped
                    .push_path("SupplyChainTradeTransaction", other),
            }
        }
    }

    // ---- agreement ------------------------------------------------------

    fn agreement(&mut self, inv: &mut Invoice, n: Node<'_, '_>) {
        for c in kids(n) {
            match name(c) {
                "BuyerReference" => inv.buyer_reference = non_empty(own_text(c)),
                "SellerTradeParty" => inv.seller = self.party(c, true),
                "BuyerTradeParty" => inv.buyer = self.party(c, false),
                "SellerTaxRepresentativeTradeParty" => {
                    inv.tax_representative = Some(TaxRepresentative {
                        name: text(c, "Name"),
                        vat_identifier: Self::tax_registration(c, "VA"),
                        address: kid(c, "PostalTradeAddress")
                            .map(|a| self.address(a))
                            .unwrap_or_default(),
                    });
                }
                "SellerOrderReferencedDocument" => {
                    inv.sales_order_reference =
                        text(c, "IssuerAssignedID").map(DocumentReference::new);
                }
                "BuyerOrderReferencedDocument" => {
                    inv.purchase_order_reference =
                        text(c, "IssuerAssignedID").map(DocumentReference::new);
                }
                "ContractReferencedDocument" => {
                    inv.contract_reference =
                        text(c, "IssuerAssignedID").map(DocumentReference::new);
                }
                "AdditionalReferencedDocument" => self.additional_document(inv, c),
                "SpecifiedProcuringProject" => {
                    inv.project_reference = text(c, "ID").map(DocumentReference::new);
                }
                other => self
                    .unmapped
                    .push_path("ApplicableHeaderTradeAgreement", other),
            }
        }
    }

    /// `ram:AdditionalReferencedDocument` carries three different terms, told
    /// apart by `ram:TypeCode`: 50 is BT-17, 130 is BT-18, 916 is BG-24.
    fn additional_document(&mut self, inv: &mut Invoice, n: Node<'_, '_>) {
        let id = text(n, "IssuerAssignedID").unwrap_or_default();
        match text(n, "TypeCode").as_deref() {
            Some("50") => inv.tender_reference = Some(DocumentReference::new(id)),
            Some("130") => {
                let mut i = Identifier::new(id);
                if let Some(s) = text(n, "ReferenceTypeCode") {
                    i = Identifier::schemed(i.content().to_owned(), s);
                }
                inv.object_identifier = Some(i);
            }
            _ => {
                let attachment = kid(n, "AttachmentBinaryObject").and_then(|b| {
                    match en16931::Attachment::new(
                        crate::xml::decode_base64(&own_text(b)),
                        b.attribute("mimeCode").unwrap_or_default(),
                        b.attribute("filename").unwrap_or_default(),
                    ) {
                        Ok(a) => Some(a),
                        Err(e) => {
                            self.malformed.push(format!("AttachmentBinaryObject: {e}"));
                            None
                        }
                    }
                });
                inv.attachments.push(SupportingDocument {
                    reference: DocumentReference::new(id),
                    description: text(n, "Name"),
                    uri: text(n, "URIID"),
                    attachment,
                });
            }
        }
    }

    fn party(&mut self, n: Node<'_, '_>, seller: bool) -> Party {
        let mut p = Party::default();
        for c in kids(n) {
            match name(c) {
                // One business term across two elements: scheme-qualified goes
                // to GlobalID, unqualified to ID. Merged back here.
                "ID" | "GlobalID" => p.identifiers.push(identifier(c)),
                "Name" => p.name = non_empty(own_text(c)),
                "Description" if seller => p.additional_legal_information = non_empty(own_text(c)),
                "SpecifiedLegalOrganization" => {
                    p.legal_registration = kid(c, "ID").map(identifier);
                    p.trading_name = text(c, "TradingBusinessName");
                }
                "DefinedTradeContact" => {
                    p.contact = Contact {
                        name: text(c, "PersonName").or_else(|| text(c, "DepartmentName")),
                        phone: kid(c, "TelephoneUniversalCommunication")
                            .and_then(|t| text(t, "CompleteNumber")),
                        email: kid(c, "EmailURIUniversalCommunication")
                            .and_then(|e| text(e, "URIID")),
                    };
                }
                "PostalTradeAddress" => p.address = self.address(c),
                "URIUniversalCommunication" => {
                    p.electronic_address = kid(c, "URIID").map(identifier);
                }
                "SpecifiedTaxRegistration" => {
                    let Some(id) = kid(c, "ID") else { continue };
                    // `VA` is BT-31/BT-48; anything else is BT-32.
                    if id.attribute("schemeID") == Some("VA") {
                        p.vat_identifier = non_empty(own_text(id));
                    } else {
                        p.tax_registration = non_empty(own_text(id));
                    }
                }
                other => self.unmapped.push_path("TradeParty", other),
            }
        }
        p
    }

    fn tax_registration(n: Node<'_, '_>, scheme: &str) -> Option<String> {
        kids(n)
            .filter(|c| name(*c) == "SpecifiedTaxRegistration")
            .filter_map(|c| kid(c, "ID"))
            .find(|id| id.attribute("schemeID") == Some(scheme))
            .map(own_text)
    }

    fn address(&mut self, n: Node<'_, '_>) -> PostalAddress {
        let mut a = PostalAddress::default();
        for c in kids(n) {
            match name(c) {
                "PostcodeCode" => a.post_code = non_empty(own_text(c)),
                "LineOne" => a.line1 = non_empty(own_text(c)),
                "LineTwo" => a.line2 = non_empty(own_text(c)),
                "LineThree" => a.line3 = non_empty(own_text(c)),
                "CityName" => a.city = non_empty(own_text(c)),
                "CountryID" => a.country = Some(Code::new(own_text(c))),
                "CountrySubDivisionName" => a.subdivision = non_empty(own_text(c)),
                other => self.unmapped.push_path("PostalTradeAddress", other),
            }
        }
        a
    }

    // ---- delivery -------------------------------------------------------

    fn delivery(&mut self, inv: &mut Invoice, n: Node<'_, '_>) {
        let mut d = Delivery::default();
        let mut any = false;
        for c in kids(n) {
            match name(c) {
                "ShipToTradeParty" => {
                    for g in kids(c) {
                        match name(g) {
                            "ID" | "GlobalID" => {
                                d.location = Some(identifier(g));
                                any = true;
                            }
                            "Name" => {
                                d.party_name = non_empty(own_text(g));
                                any = true;
                            }
                            "PostalTradeAddress" => {
                                d.address = Some(self.address(g));
                                any = true;
                            }
                            other => self.unmapped.push_path("ShipToTradeParty", other),
                        }
                    }
                }
                "ActualDeliverySupplyChainEvent" => {
                    d.date = self.date(c, "OccurrenceDateTime");
                    any |= d.date.is_some();
                }
                "DespatchAdviceReferencedDocument" => {
                    inv.despatch_advice_reference =
                        text(c, "IssuerAssignedID").map(DocumentReference::new);
                }
                "ReceivingAdviceReferencedDocument" => {
                    inv.receiving_advice_reference =
                        text(c, "IssuerAssignedID").map(DocumentReference::new);
                }
                other => self
                    .unmapped
                    .push_path("ApplicableHeaderTradeDelivery", other),
            }
        }
        if any {
            inv.delivery = Some(d);
        }
    }

    // ---- settlement -----------------------------------------------------

    #[allow(clippy::too_many_lines)]
    fn settlement(&mut self, inv: &mut Invoice, n: Node<'_, '_>) {
        let mut payment = PaymentInstructions::default();
        let mut have_payment = false;
        let mut creditor_id = None;
        let mut mandate = None;
        for c in kids(n) {
            match name(c) {
                "CreditorReferenceID" => creditor_id = non_empty(own_text(c)),
                "PaymentReference" => {
                    payment.remittance_information = non_empty(own_text(c));
                    have_payment = true;
                }
                "TaxCurrencyCode" => inv.vat_accounting_currency = Some(Code::new(own_text(c))),
                "InvoiceCurrencyCode" => inv.currency = Some(Code::new(own_text(c))),
                "PayeeTradeParty" => {
                    inv.payee = Some(Payee {
                        name: text(c, "Name"),
                        identifier: kid(c, "GlobalID").or_else(|| kid(c, "ID")).map(identifier),
                        legal_registration: kid(c, "SpecifiedLegalOrganization")
                            .and_then(|l| kid(l, "ID"))
                            .map(identifier),
                    });
                }
                "SpecifiedTradeSettlementPaymentMeans" => {
                    self.payment_means(&mut payment, c);
                    have_payment = true;
                }
                "ApplicableTradeTax" => {
                    // BT-7/BT-8 repeat on every entry; the model has one of
                    // each, so the first wins and the rest agree by construction.
                    if inv.vat_point_date.is_none() {
                        inv.vat_point_date = self.date(c, "TaxPointDate");
                    }
                    if inv.vat_point_date_code.is_none() {
                        inv.vat_point_date_code = code(c, "DueDateTypeCode");
                    }
                    inv.vat_breakdown.push(VatBreakdown {
                        taxable_amount: self.amount(c, "BasisAmount").unwrap_or_default(),
                        tax_amount: self.amount(c, "CalculatedAmount").unwrap_or_default(),
                        category: code(c, "CategoryCode").unwrap_or_else(|| Code::new("")),
                        rate: decimal(c, "RateApplicablePercent")
                            .map(|d| d / rust_decimal::Decimal::from(100))
                            .and_then(en16931::Percentage::from_fraction),
                        exemption_reason: text(c, "ExemptionReason"),
                        exemption_reason_code: code(c, "ExemptionReasonCode"),
                    });
                }
                "BillingSpecifiedPeriod" => inv.invoicing_period = Some(self.period(c)),
                "SpecifiedTradeAllowanceCharge" => {
                    let is_charge = Self::is_charge(c);
                    let a = DocumentAllowanceCharge {
                        amount: self.amount(c, "ActualAmount").unwrap_or_default(),
                        base_amount: self.amount(c, "BasisAmount"),
                        percentage: decimal(c, "CalculationPercent")
                            .map(|d| d / rust_decimal::Decimal::from(100))
                            .and_then(en16931::Percentage::from_fraction),
                        vat: kid(c, "CategoryTradeTax")
                            .map(|t| Self::line_vat(t))
                            .unwrap_or_default(),
                        reason: text(c, "Reason"),
                        reason_code: code(c, "ReasonCode"),
                    };
                    if is_charge {
                        inv.charges.push(a);
                    } else {
                        inv.allowances.push(a);
                    }
                }
                "SpecifiedTradePaymentTerms" => {
                    // **Not trimmed**, exactly as the UBL reader does not trim
                    // it. `BR-DE-18` requires the Skonto block to end with a
                    // newline, so BT-20's trailing whitespace is load-bearing —
                    // and XRechnung is carried in CII as well as UBL, with a
                    // KoSIT scenario of its own. This reader used `own_text`,
                    // which trims, so a CII XRechnung with a Skonto lost its
                    // terminator and was rejected. 36 of the corpus documents
                    // that survive UBL→CII conversion differ on this one field.
                    inv.payment_terms = kid(c, "Description")
                        .and_then(|d| d.text())
                        .map(str::to_owned);
                    inv.due_date = self.date(c, "DueDateDateTime");
                    mandate = text(c, "DirectDebitMandateID");
                }
                "SpecifiedTradeSettlementHeaderMonetarySummation" => self.totals(inv, c),
                "InvoiceReferencedDocument" => {
                    inv.preceding_invoices.push(PrecedingInvoice {
                        reference: text(c, "IssuerAssignedID")
                            .map_or_else(|| DocumentReference::new(""), DocumentReference::new),
                        issue_date: self.date(c, "FormattedIssueDateTime"),
                    });
                }
                "ReceivableSpecifiedTradeAccountingAccount" => {
                    inv.accounting_reference = text(c, "ID");
                }
                other => self
                    .unmapped
                    .push_path("ApplicableHeaderTradeSettlement", other),
            }
        }
        // The mandate and creditor identifier arrive from two different
        // aggregates and belong to one `DirectDebit`, so they are joined last.
        if creditor_id.is_some() || mandate.is_some() {
            let existing = match payment.means.take() {
                Some(PaymentMeans::DirectDebit(d)) => d,
                other => {
                    payment.means = other;
                    DirectDebit::default()
                }
            };
            payment.means = Some(PaymentMeans::DirectDebit(DirectDebit {
                mandate_reference: mandate.or(existing.mandate_reference),
                creditor_identifier: creditor_id.or(existing.creditor_identifier),
                debited_account: existing.debited_account,
            }));
            have_payment = true;
        }
        if have_payment {
            inv.payment = Some(payment);
        }
    }

    fn payment_means(&mut self, p: &mut PaymentInstructions, n: Node<'_, '_>) {
        let mut transfers: Vec<CreditTransfer> = Vec::new();
        for c in kids(n) {
            match name(c) {
                "TypeCode" => p.means_code = Some(Code::new(own_text(c))),
                "Information" => p.means_text = non_empty(own_text(c)),
                "ApplicableTradeSettlementFinancialCard" => {
                    p.means = Some(PaymentMeans::Card(PaymentCard {
                        primary_account_number: text(c, "ID"),
                        holder_name: text(c, "CardholderName"),
                    }));
                }
                "PayerPartyDebtorFinancialAccount" => {
                    p.means = Some(PaymentMeans::DirectDebit(DirectDebit {
                        debited_account: text(c, "IBANID").or_else(|| text(c, "ProprietaryID")),
                        ..DirectDebit::default()
                    }));
                }
                "PayeePartyCreditorFinancialAccount" => transfers.push(CreditTransfer {
                    account_identifier: text(c, "IBANID").or_else(|| text(c, "ProprietaryID")),
                    account_name: text(c, "AccountName"),
                    provider_identifier: None,
                }),
                "PayeeSpecifiedCreditorFinancialInstitution" => {
                    // BT-86 arrives *after* the account it belongs to, as a
                    // sibling rather than a child, so it is attached here.
                    if let Some(t) = transfers.last_mut() {
                        t.provider_identifier = text(c, "BICID");
                    }
                }
                other => self
                    .unmapped
                    .push_path("SpecifiedTradeSettlementPaymentMeans", other),
            }
        }
        if !transfers.is_empty() {
            p.means = Some(PaymentMeans::CreditTransfer(transfers));
        }
    }

    fn totals(&mut self, inv: &mut Invoice, n: Node<'_, '_>) {
        let mut t = DocumentTotals::default();
        // Two `ram:TaxTotalAmount` elements may appear, told apart only by
        // `@currencyID`: the second is BT-111, in the accounting currency. When
        // BT-6 equals BT-5 they are the same element twice over, which is the
        // case the match below exists for.
        let doc_ccy = inv.currency.as_ref().map(Code::as_str);
        let tax_ccy = inv.vat_accounting_currency.as_ref().map(Code::as_str);
        for c in kids(n) {
            match name(c) {
                "LineTotalAmount" => {
                    t.line_total = self.amount(n, "LineTotalAmount").unwrap_or_default();
                }
                "ChargeTotalAmount" => t.charge_total = self.amount(n, "ChargeTotalAmount"),
                "AllowanceTotalAmount" => {
                    t.allowance_total = self.amount(n, "AllowanceTotalAmount");
                }
                "TaxBasisTotalAmount" => {
                    t.taxable_total = self.amount(n, "TaxBasisTotalAmount").unwrap_or_default();
                }
                "TaxTotalAmount" => {
                    let raw = own_text(c);
                    let parsed = InvoiceAmount::parse(&raw).ok();
                    if parsed.is_none() {
                        self.malformed.push(format!("TaxTotalAmount: {raw:?}"));
                    }
                    match c.attribute("currencyID") {
                        // BT-6 == BT-5 makes one element **both** totals, the
                        // same case the UBL reader handles: `BR-53`'s binding is
                        // satisfied by the document-currency total whenever the
                        // two currencies coincide, so assigning it to only one
                        // of them manufactures a `BR-53` finding on a document
                        // CEN publishes as an example.
                        Some(cur) if Some(cur) == tax_ccy => {
                            if Some(cur) == doc_ccy && t.vat_total.is_none() {
                                t.vat_total = parsed;
                            }
                            t.vat_total_accounting = parsed;
                        }
                        Some(cur) if Some(cur) != doc_ccy => t.vat_total_accounting = parsed,
                        _ => t.vat_total = parsed,
                    }
                }
                "RoundingAmount" => t.rounding = self.amount(n, "RoundingAmount"),
                "GrandTotalAmount" => {
                    t.gross_total = self.amount(n, "GrandTotalAmount").unwrap_or_default();
                }
                "TotalPrepaidAmount" => t.paid = self.amount(n, "TotalPrepaidAmount"),
                "DuePayableAmount" => {
                    t.due = self.amount(n, "DuePayableAmount").unwrap_or_default();
                }
                other => self
                    .unmapped
                    .push_path("SpecifiedTradeSettlementHeaderMonetarySummation", other),
            }
        }
        inv.totals = t;
    }

    fn period(&mut self, n: Node<'_, '_>) -> Period {
        Period {
            start: self.date(n, "StartDateTime"),
            end: self.date(n, "EndDateTime"),
        }
    }

    /// `ram:ChargeIndicator/udt:Indicator` — `true` is a charge.
    fn is_charge(n: Node<'_, '_>) -> bool {
        kid(n, "ChargeIndicator")
            .and_then(|i| kid(i, "Indicator"))
            .map(own_text)
            .is_some_and(|t| t == "true" || t == "1")
    }

    fn line_vat(n: Node<'_, '_>) -> LineVat {
        LineVat {
            category: code(n, "CategoryCode").unwrap_or_else(|| Code::new("")),
            rate: decimal(n, "RateApplicablePercent")
                .map(|d| d / rust_decimal::Decimal::from(100))
                .and_then(en16931::Percentage::from_fraction),
        }
    }

    // ---- lines ----------------------------------------------------------

    #[allow(clippy::too_many_lines)]
    fn line(&mut self, n: Node<'_, '_>) -> InvoiceLine {
        let mut l = InvoiceLine {
            id: String::new(),
            note: None,
            order_line_reference: None,
            accounting_reference: None,
            object_identifier: None,
            quantity: Quantity::ZERO,
            unit_code: Code::new(""),
            net_amount: InvoiceAmount::default(),
            period: None,
            allowances: Vec::new(),
            charges: Vec::new(),
            price: PriceDetails::default(),
            vat: LineVat::default(),
            item: Item::default(),
        };
        for c in kids(n) {
            match name(c) {
                "AssociatedDocumentLineDocument" => {
                    l.id = text(c, "LineID").unwrap_or_default();
                    l.note = kid(c, "IncludedNote").and_then(|x| text(x, "Content"));
                }
                "SpecifiedTradeProduct" => l.item = self.item(c),
                "SpecifiedLineTradeAgreement" => {
                    for g in kids(c) {
                        match name(g) {
                            "BuyerOrderReferencedDocument" => {
                                l.order_line_reference =
                                    text(g, "LineID").map(DocumentReference::new);
                            }
                            "GrossPriceProductTradePrice" => {
                                l.price.gross_price =
                                    decimal(g, "ChargeAmount").map(en16931::UnitPriceAmount::new);
                                l.price.price_discount = kid(g, "AppliedTradeAllowanceCharge")
                                    .and_then(|a| decimal(a, "ActualAmount"))
                                    .map(en16931::UnitPriceAmount::new);
                            }
                            "NetPriceProductTradePrice" => {
                                l.price.net_price = decimal(g, "ChargeAmount")
                                    .map(en16931::UnitPriceAmount::new)
                                    .unwrap_or_default();
                                l.price.base_quantity =
                                    decimal(g, "BasisQuantity").map(Quantity::new);
                                // An empty `unitCode` is an absent BT-150, not a
                                // BT-150 whose value is the empty string. The
                                // difference is a fatal `PEPPOL-EN16931-R130`,
                                // which compares BT-150 against BT-130.
                                l.price.base_quantity_code = kid(g, "BasisQuantity")
                                    .and_then(|q| q.attribute("unitCode"))
                                    .filter(|u| !u.trim().is_empty())
                                    .map(Code::new);
                            }
                            other => self
                                .unmapped
                                .push_path("SpecifiedLineTradeAgreement", other),
                        }
                    }
                }
                "SpecifiedLineTradeDelivery" => {
                    if let Some(q) = kid(c, "BilledQuantity") {
                        l.quantity = own_text(q).parse().map_or(Quantity::ZERO, Quantity::new);
                        l.unit_code = q
                            .attribute("unitCode")
                            .map_or_else(|| Code::new(""), Code::new);
                    }
                }
                "SpecifiedLineTradeSettlement" => self.line_settlement(&mut l, c),
                other => self
                    .unmapped
                    .push_path("IncludedSupplyChainTradeLineItem", other),
            }
        }
        l
    }

    fn line_settlement(&mut self, l: &mut InvoiceLine, n: Node<'_, '_>) {
        for c in kids(n) {
            match name(c) {
                "ApplicableTradeTax" => l.vat = Self::line_vat(c),
                "BillingSpecifiedPeriod" => l.period = Some(self.period(c)),
                "SpecifiedTradeAllowanceCharge" => {
                    let is_charge = Self::is_charge(c);
                    let a = LineAllowanceCharge {
                        amount: self.amount(c, "ActualAmount").unwrap_or_default(),
                        base_amount: self.amount(c, "BasisAmount"),
                        percentage: decimal(c, "CalculationPercent")
                            .map(|d| d / rust_decimal::Decimal::from(100))
                            .and_then(en16931::Percentage::from_fraction),
                        reason: text(c, "Reason"),
                        reason_code: code(c, "ReasonCode"),
                    };
                    if is_charge {
                        l.charges.push(a);
                    } else {
                        l.allowances.push(a);
                    }
                }
                "SpecifiedTradeSettlementLineMonetarySummation" => {
                    l.net_amount = self.amount(c, "LineTotalAmount").unwrap_or_default();
                }
                "AdditionalReferencedDocument" => {
                    let id = text(c, "IssuerAssignedID").unwrap_or_default();
                    l.object_identifier = Some(match text(c, "ReferenceTypeCode") {
                        Some(s) => Identifier::schemed(id, s),
                        None => Identifier::new(id),
                    });
                }
                "ReceivableSpecifiedTradeAccountingAccount" => {
                    l.accounting_reference = text(c, "ID");
                }
                other => self
                    .unmapped
                    .push_path("SpecifiedLineTradeSettlement", other),
            }
        }
    }

    fn item(&mut self, n: Node<'_, '_>) -> Item {
        let mut i = Item::default();
        for c in kids(n) {
            match name(c) {
                "GlobalID" => i.standard_identifier = Some(identifier(c)),
                "SellerAssignedID" => i.seller_identifier = non_empty(own_text(c)),
                "BuyerAssignedID" => i.buyer_identifier = non_empty(own_text(c)),
                "Name" => i.name = non_empty(own_text(c)),
                "Description" => i.description = non_empty(own_text(c)),
                "ApplicableProductCharacteristic" => i.attributes.push(ItemAttribute {
                    name: text(c, "Description"),
                    value: text(c, "Value"),
                }),
                "DesignatedProductClassification" => {
                    if let Some(cc) = kid(c, "ClassCode") {
                        i.classification_identifiers.push(identifier(cc));
                    }
                }
                "OriginTradeCountry" => i.origin_country = code(c, "ID"),
                other => self.unmapped.push_path("SpecifiedTradeProduct", other),
            }
        }
        i
    }
}

/// Recording an unmapped element, in the one shape the whole reader uses.
trait PushPath {
    fn push_path(&mut self, parent: &str, child: &str);
}

impl PushPath for BTreeSet<String> {
    fn push_path(&mut self, parent: &str, child: &str) {
        self.insert(format!("{parent}/{child}"));
    }
}
