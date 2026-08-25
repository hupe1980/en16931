//! Reading UBL 2.1 into the semantic model.
//!
//! # It never silently drops anything
//!
//! Every element walked is either mapped or recorded in [`Reader::unmapped`],
//! as `Parent/Child`. A reader that quietly ignored `cbc:TaxExclusiveAmount`
//! would report a clean parse and prove nothing — so the conformance suites
//! assert the unmapped set against an explicit allowlist rather than trusting
//! silence.
//!
//! # Malformed values are reported, not panicked on
//!
//! CEN's own suites carry deliberately malformed values — `<cbc:IssueDate>123`,
//! `<cbc:Amount>.00` — to exercise `BR-DEC-*` and the date rules. `en16931`'s
//! types make those unrepresentable, so the reader records them in
//! [`Reader::malformed`] and leaves the field absent. Losing the distinction
//! between "absent" and "present but unreadable" would make a `BR-03` finding
//! unexplainable, which is why the list exists at all.

use std::collections::BTreeSet;

use en16931::invoice::{
    Code, Contact, CreditTransfer, Delivery, DirectDebit, DocumentAllowanceCharge, Invoice,
    InvoiceLine, InvoiceNote, Item, ItemAttribute, LineAllowanceCharge, LineVat, Party, Payee,
    PaymentCard, PaymentInstructions, PaymentMeans, Period, PostalAddress, PrecedingInvoice,
    PriceDetails, SupportingDocument, TaxRepresentative, VatBreakdown,
};
use en16931::{
    Date, DocumentKind, DocumentReference, Identifier, InvoiceAmount, Percentage, Quantity,
};

/// Reads one UBL `Invoice` or `CreditNote` element into an [`Invoice`].
#[derive(Default)]
pub struct Reader {
    /// Element paths encountered and not mapped, as `Parent/Child`.
    pub unmapped: BTreeSet<String>,
    /// Every `cac:TaxTotal/cbc:TaxAmount` seen, with its `@currencyID`.
    ///
    /// Resolved into BT-110 / BT-111 once BT-5 and BT-6 are known — see
    /// [`Reader::tax_total`].
    tax_amounts: Vec<(Option<String>, Option<InvoiceAmount>)>,
    /// Elements whose text is present but not representable in the model —
    /// `<cbc:IssueDate>123</cbc:IssueDate>`, `<cbc:Amount>.00</cbc:Amount>`.
    ///
    /// The suite uses these to exercise rules about *syntax*: BR-03 asks whether
    /// the element is there, not whether it parses. This crate's types refuse
    /// the value at the boundary instead, so the presence question cannot be
    /// asked of the model at all. Cases that hit one are skipped and counted,
    /// never scored.
    pub malformed: Vec<String>,
}

/// Local name, namespace-independent.
fn name<'i>(n: roxmltree::Node<'_, 'i>) -> &'i str {
    n.tag_name().name()
}

/// Direct element children.
fn kids<'a, 'i>(n: roxmltree::Node<'a, 'i>) -> impl Iterator<Item = roxmltree::Node<'a, 'i>> {
    n.children().filter(roxmltree::Node::is_element)
}

/// The first direct child with this local name.
fn kid<'a, 'i>(n: roxmltree::Node<'a, 'i>, want: &str) -> Option<roxmltree::Node<'a, 'i>> {
    kids(n).find(|c| name(*c) == want)
}

/// `None` for an empty string.
///
/// An empty element is an **absent** term, not a term whose value is the empty
/// string: `<cbc:BuyerReference/>` says nothing, and `Some("")` claims it says
/// something blank. The two spellings behave the same under every rule — they
/// all test `is_none_or(str::is_empty)` — and differ everywhere else: in a
/// `match` on `Option`, in what gets written back out, and in whether the two
/// syntaxes agree with each other on the same document.
///
/// This reader was already inconsistent with *itself*: [`text`] returns `None`
/// because `roxmltree` gives an empty element no text node, while the dozen call
/// sites that read an element's own text returned `Some("")`.
fn non_empty(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

/// Trimmed text content of a direct child.
fn text(n: roxmltree::Node<'_, '_>, want: &str) -> Option<String> {
    kid(n, want)
        .and_then(|c| c.text())
        .map(|t| t.trim().to_owned())
}

/// Trimmed text of this element.
fn own_text(n: roxmltree::Node<'_, '_>) -> String {
    n.text().unwrap_or_default().trim().to_owned()
}

/// An amount, or `None` when the text is not a valid EN 16931 amount.
///
/// Returning `None` rather than panicking is deliberate: several CEN test cases
/// carry a deliberately malformed amount to trigger `BR-DEC-*`, which this
/// crate's types make unrepresentable. Those cases are reported as unreadable,
/// not as failures.
fn amount(n: roxmltree::Node<'_, '_>, want: &str) -> Option<InvoiceAmount> {
    text(n, want).and_then(|t| InvoiceAmount::parse(&t).ok())
}

fn amount_here(n: roxmltree::Node<'_, '_>) -> Option<InvoiceAmount> {
    InvoiceAmount::parse(&own_text(n)).ok()
}

fn decimal(n: roxmltree::Node<'_, '_>, want: &str) -> Option<rust_decimal::Decimal> {
    text(n, want).and_then(|t| t.parse().ok())
}

fn date(n: roxmltree::Node<'_, '_>, want: &str) -> Option<Date> {
    text(n, want).and_then(|t| xs_date(&t))
}

/// A UBL date, which is `xs:date` and **not** simply `CCYY-MM-DD`.
///
/// XSD's lexical space for `xs:date` ends with an optional time zone —
/// `2026-07-31`, `2026-07-31Z`, `2026-07-31+02:00` are three spellings of one
/// day — and Java's `XMLGregorianCalendar`, which a great many UBL producers
/// are built on, writes the offset by default. Documents carrying it are
/// schema-valid and no Schematron rule objects to them.
///
/// [`Date`] holds a calendar day and nothing else, deliberately: EN 16931-1
/// §6.5.9 has no term for a time zone, so there is nowhere to put one and
/// nothing is lost by dropping it. Refusing the value instead costs the whole
/// business term — BT-2 absent, and `BR-03` firing on an invoice that states
/// its issue date perfectly well.
///
/// The zone is dropped rather than applied. `2026-07-31+02:00` **is** the day
/// 2026-07-31 — the offset says which day-boundary the value was written
/// against, not that the day should be shifted.
fn xs_date(raw: &str) -> Option<Date> {
    Date::parse(without_timezone(raw.trim())).ok()
}

/// `cbc:ChargeIndicator` — `xs:boolean`, which has **four** lexical forms.
///
/// `1` and `0` are as valid as `true` and `false`, and this reader knew only
/// the two words. A schema-valid `<cbc:ChargeIndicator>1</cbc:ChargeIndicator>`
/// therefore read as *false*, and the element it governs is the one that
/// decides whether an amount is **added to or subtracted from** the invoice —
/// so a fee arrived as a discount, with the sign of the money reversed and
/// nothing in the reader's own output to say so.
///
/// `None` for anything else, which the caller records in
/// [`Reader::malformed`]. There is no safe default: reading an unreadable
/// indicator as a charge invents money and as an allowance destroys it, so the
/// answer is to say the document could not be read rather than to pick.
fn xs_boolean(raw: &str) -> Option<bool> {
    match raw.trim() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

/// `2026-07-31Z` and `2026-07-31+02:00` → `2026-07-31`.
///
/// Anything else is returned untouched, so a genuinely malformed date still
/// fails in [`Date::parse`] and is reported rather than trimmed into shape.
fn without_timezone(s: &str) -> &str {
    let b = s.as_bytes();
    // A zone can only follow the ten characters of a date, so the `-` of an
    // offset is never the `-` inside one.
    let zoned = match b.len() {
        11 => b[10] == b'Z',
        16 => matches!(b[10], b'+' | b'-') && b[13] == b':',
        _ => false,
    };
    if zoned { &s[..10] } else { s }
}

/// Whether an element is present with text the model cannot hold.
fn is_malformed<T>(
    n: roxmltree::Node<'_, '_>,
    want: &str,
    parse: impl Fn(&str) -> Option<T>,
) -> bool {
    text(n, want).is_some_and(|t| !t.is_empty() && parse(&t).is_none())
}

fn code(n: roxmltree::Node<'_, '_>, want: &str) -> Option<Code> {
    text(n, want).map(Code::new)
}

/// `<cbc:ID schemeID="0088">x</cbc:ID>` → an [`Identifier`] with its scheme.
fn identifier(n: roxmltree::Node<'_, '_>) -> Identifier {
    match n.attribute("schemeID") {
        Some(s) => Identifier::schemed(own_text(n), s),
        None => Identifier::new(own_text(n)),
    }
}

impl Reader {
    /// Note an element this reader does not map.
    fn skip(&mut self, parent: &str, n: roxmltree::Node<'_, '_>) {
        self.unmapped.insert(format!("{parent}/{}", name(n)));
    }

    /// Whether a `cac:AllowanceCharge` is a **charge**, recording an
    /// unreadable indicator.
    ///
    /// Mandatory in UBL, so an absent one is as much a defect as an unreadable
    /// one and is reported the same way. False is the fallback because the two
    /// wrong answers are not symmetrical in consequence: a charge read as an
    /// allowance makes the document fail its own totals rules loudly, while an
    /// allowance read as a charge would too — but the caller sees `malformed`
    /// either way, which is the part that matters.
    fn charge_indicator(&mut self, n: roxmltree::Node<'_, '_>, at: &str) -> bool {
        match text(n, "ChargeIndicator").as_deref().map(xs_boolean) {
            Some(Some(b)) => b,
            other => {
                let raw = if other.is_none() {
                    "<absent>".to_owned()
                } else {
                    text(n, "ChargeIndicator").unwrap_or_default()
                };
                self.malformed
                    .push(format!("{at}/ChargeIndicator={raw}: not an xs:boolean"));
                false
            }
        }
    }

    /// Read a UBL `Invoice` or `CreditNote` root element.
    pub fn read(&mut self, root: roxmltree::Node<'_, '_>) -> Invoice {
        let mut inv = Invoice::default();
        // The root element *is* the document kind, in both syntaxes.
        inv.kind = if name(root) == "CreditNote" {
            DocumentKind::CreditNote
        } else {
            DocumentKind::Invoice
        };
        // UBL splits allowances and charges by `ChargeIndicator`, and the totals
        // by which `TaxTotal` carries subtotals. Both need a second pass.
        for c in kids(root) {
            match name(c) {
                "CustomizationID" => inv.specification_id = non_empty(own_text(c)),
                "ProfileID" => inv.business_process = non_empty(own_text(c)),
                "ID" => inv.number = non_empty(own_text(c)),
                "IssueDate" => {
                    if is_malformed(root, "IssueDate", xs_date) {
                        self.malformed.push("IssueDate".to_owned());
                    }
                    inv.issue_date = date(root, "IssueDate");
                }
                "DueDate" => inv.due_date = date(root, "DueDate"),
                "InvoiceTypeCode" | "CreditNoteTypeCode" => {
                    inv.type_code = Some(Code::new(own_text(c)));
                }
                "Note" => inv.notes.push(read_note(&own_text(c))),
                "TaxPointDate" => inv.vat_point_date = date(root, "TaxPointDate"),
                "DocumentCurrencyCode" => inv.currency = Some(Code::new(own_text(c))),
                "TaxCurrencyCode" => inv.vat_accounting_currency = Some(Code::new(own_text(c))),
                "AccountingCost" => inv.accounting_reference = non_empty(own_text(c)),
                "BuyerReference" => inv.buyer_reference = non_empty(own_text(c)),
                "InvoicePeriod" => {
                    // BT-8 rides inside `cac:InvoicePeriod` in UBL, so an
                    // `InvoicePeriod` carrying *only* a `DescriptionCode` is
                    // BT-8 and **not** BG-14. CEN's suite is explicit about it:
                    // "Verify that BT-8 can be informed even if there is no
                    // Invoice period defined." Treating the element itself as
                    // the group makes `BR-CO-19` fire on a document CEN accepts.
                    if let Some(dc) = code(c, "DescriptionCode") {
                        inv.vat_point_date_code = Some(dc);
                    }
                    let p = self.period(c);
                    if p.start.is_some() || p.end.is_some() {
                        inv.invoicing_period = Some(p);
                    }
                }
                "OrderReference" => {
                    inv.purchase_order_reference = text(c, "ID").map(DocumentReference::new);
                    inv.sales_order_reference = text(c, "SalesOrderID").map(DocumentReference::new);
                }
                "BillingReference" => {
                    if let Some(r) = kid(c, "InvoiceDocumentReference") {
                        inv.preceding_invoices.push(PrecedingInvoice {
                            reference: text(r, "ID")
                                .map_or_else(|| DocumentReference::new(""), DocumentReference::new),
                            issue_date: date(r, "IssueDate"),
                        });
                    }
                }
                "DespatchDocumentReference" => {
                    inv.despatch_advice_reference = text(c, "ID").map(DocumentReference::new);
                }
                "ReceiptDocumentReference" => {
                    inv.receiving_advice_reference = text(c, "ID").map(DocumentReference::new);
                }
                "OriginatorDocumentReference" => {
                    inv.tender_reference = text(c, "ID").map(DocumentReference::new);
                }
                "ContractDocumentReference" => {
                    inv.contract_reference = text(c, "ID").map(DocumentReference::new);
                }
                "ProjectReference" => {
                    inv.project_reference = text(c, "ID").map(DocumentReference::new);
                }
                "AdditionalDocumentReference" => self.additional_document(c, &mut inv),
                "AccountingSupplierParty" => {
                    if let Some(p) = kid(c, "Party") {
                        inv.seller = self.party(p);
                    }
                }
                "AccountingCustomerParty" => {
                    if let Some(p) = kid(c, "Party") {
                        inv.buyer = self.party(p);
                    }
                }
                "PayeeParty" => inv.payee = Some(self.payee(c)),
                "TaxRepresentativeParty" => inv.tax_representative = Some(self.tax_rep(c)),
                "Delivery" => inv.delivery = Some(self.delivery(c)),
                // BG-16 is 0..1 in the model and `cac:PaymentMeans` is 0..n in
                // UBL: several elements are one instruction with several BG-17
                // accounts — CEN's `guide-example1.xml` carries two. Merged,
                // not overwritten; overwriting kept only the last account, a
                // silent loss the round trip could not see because the writer
                // had the mirror-image bug.
                "PaymentMeans" => {
                    let next = self.payment(c);
                    inv.payment = Some(match inv.payment.take() {
                        None => next,
                        Some(prev) => self.merge_payment(prev, next),
                    });
                }
                // **Not trimmed.** `BR-DE-18` requires the Skonto block to end
                // with a newline, so BT-20's trailing whitespace is load-bearing
                // and trimming it makes a conforming document fail.
                "PaymentTerms" => {
                    inv.payment_terms = kid(c, "Note").and_then(|n| n.text()).map(str::to_owned);
                }
                "AllowanceCharge" => {
                    let is_charge = self.charge_indicator(c, "AllowanceCharge");
                    let ac = self.allowance(c);
                    if is_charge {
                        inv.charges.push(ac);
                    } else {
                        inv.allowances.push(ac);
                    }
                }
                "TaxTotal" => self.tax_total(c, &mut inv),
                // `cac:PrepaidPayment` is BG-DEX-09, the XRechnung Extension's
                // third-party payment. Core UBL maps BT-113 to the same element,
                // but the Extension's instances carry all three DEX terms.
                "PrepaidPayment" => {
                    inv.extensions
                        .third_party_payments
                        .push(en16931::ThirdPartyPayment {
                            payment_type: text(c, "ID"),
                            amount: amount(c, "PaidAmount"),
                            description: text(c, "InstructionID"),
                        });
                }
                "LegalMonetaryTotal" => self.totals(c, &mut inv),
                "InvoiceLine" | "CreditNoteLine" => {
                    let l = self.line(c);
                    // BG-DEX-01 hangs beneath the line, keyed by its index.
                    let subs = self.sub_lines(c);
                    if !subs.is_empty() {
                        inv.extensions
                            .sub_invoice_lines
                            .push((inv.lines.len(), subs));
                    }
                    inv.lines.push(l);
                }
                // Syntax-only: UBL bookkeeping with no business term.
                "UBLVersionID"
                | "CopyIndicator"
                | "UUID"
                | "IssueTime"
                | "LineCountNumeric"
                | "Signature"
                | "TaxExchangeRate"
                | "InvoiceDocumentReference" => {}
                _ => self.skip("Invoice", c),
            }
        }
        // BT-90 lives on the *seller* in UBL — `cac:PartyIdentification` with
        // `schemeID="SEPA"` — while the model puts it in BG-19, where the rule
        // that needs it is. `BR-DE-30` cannot be checked without this hop, and
        // `super::write` performs it in reverse.
        //
        // **Moved, not copied.** A `schemeID="SEPA"` party identification is not
        // a BT-29 that happens to have an unusual scheme; it is the UBL binding's
        // home for BT-90, which is why `BR-CL-10` admits `SEPA` there and only
        // under the supplier or the payee. Leaving it in `identifiers` as well
        // made the term arrive twice, and a document that crossed into UBL and
        // back grew a seller identifier each time.
        //
        // Only when there is a BG-19 to move it into: without one the model has
        // nowhere else to put it, and dropping it would be worse than leaving it
        // where `BR-CL-10` still accepts it.
        let has_direct_debit = matches!(
            inv.payment.as_ref().and_then(|p| p.means.as_ref()),
            Some(PaymentMeans::DirectDebit(_))
        );
        if has_direct_debit {
            let sepa = inv
                .seller
                .identifiers
                .iter()
                .position(|id| id.scheme() == Some("SEPA"))
                .map(|i| inv.seller.identifiers.remove(i).content().to_owned());
            if let Some(PaymentMeans::DirectDebit(dd)) =
                inv.payment.as_mut().and_then(|p| p.means.as_mut())
            {
                dd.creditor_identifier = sepa;
            }
        }

        // Resolve BT-110 / BT-111 now that BT-5 and BT-6 are known.
        //
        // CEN's own `BR-53` binding is
        // `exists(//cac:TaxTotal/cbc:TaxAmount[@currencyID = $taxcurrency])`,
        // which is satisfied by the *document-currency* total whenever BT-6
        // equals BT-5. So when the two currencies coincide, one element is both
        // BT-110 and BT-111 — and a reader that assigns it to only one of them
        // makes `BR-53` fire on a document CEN publishes as an example.
        let doc_ccy = inv.currency.as_ref().map(|c| c.as_str().to_owned());
        let tax_ccy = inv
            .vat_accounting_currency
            .as_ref()
            .map(|c| c.as_str().to_owned());
        for (ccy, amount) in std::mem::take(&mut self.tax_amounts) {
            // No `@currencyID` means the document currency.
            let is_doc = ccy.is_none() || ccy == doc_ccy;
            let is_tax = tax_ccy.is_some() && ccy == tax_ccy;
            if is_doc && inv.totals.vat_total.is_none() {
                inv.totals.vat_total = amount;
            }
            if is_tax && inv.totals.vat_total_accounting.is_none() {
                inv.totals.vat_total_accounting = amount;
            }
        }
        inv
    }

    fn period(&mut self, n: roxmltree::Node<'_, '_>) -> Period {
        for c in kids(n) {
            if !matches!(name(c), "StartDate" | "EndDate" | "DescriptionCode") {
                self.skip("InvoicePeriod", c);
            }
        }
        Period {
            start: date(n, "StartDate"),
            end: date(n, "EndDate"),
        }
    }

    fn address(&mut self, n: roxmltree::Node<'_, '_>) -> PostalAddress {
        let mut a = PostalAddress::default();
        for c in kids(n) {
            match name(c) {
                "StreetName" => a.line1 = non_empty(own_text(c)),
                "AdditionalStreetName" => a.line2 = non_empty(own_text(c)),
                "AddressLine" => a.line3 = text(c, "Line"),
                "CityName" => a.city = non_empty(own_text(c)),
                "PostalZone" => a.post_code = non_empty(own_text(c)),
                "CountrySubentity" => a.subdivision = non_empty(own_text(c)),
                "Country" => a.country = code(c, "IdentificationCode"),
                _ => self.skip("PostalAddress", c),
            }
        }
        a
    }

    fn party(&mut self, n: roxmltree::Node<'_, '_>) -> Party {
        let mut p = Party::default();
        for c in kids(n) {
            match name(c) {
                "EndpointID" => p.electronic_address = Some(identifier(c)),
                "PartyIdentification" => {
                    if let Some(id) = kid(c, "ID") {
                        p.identifiers.push(identifier(id));
                    }
                }
                "PartyName" => p.trading_name = text(c, "Name"),
                "PostalAddress" => p.address = self.address(c),
                "PartyTaxScheme" => {
                    // BT-31 versus BT-32 is decided by the tax scheme, not by
                    // position: `VAT` is the VAT identifier, anything else the
                    // registration identifier.
                    let scheme = kid(c, "TaxScheme").and_then(|s| text(s, "ID"));
                    let id = text(c, "CompanyID");
                    if scheme.as_deref() == Some("VAT") {
                        p.vat_identifier = id;
                    } else {
                        p.tax_registration = id;
                    }
                }
                "PartyLegalEntity" => {
                    // BT-27 / BT-44 is the *registration* name in UBL, not
                    // `PartyName/Name` — that is BT-28 / BT-45.
                    if let Some(rn) = text(c, "RegistrationName") {
                        p.name = Some(rn);
                    }
                    if let Some(cid) = kid(c, "CompanyID") {
                        p.legal_registration = Some(identifier(cid));
                    }
                    if let Some(extra) = text(c, "CompanyLegalForm") {
                        p.additional_legal_information = Some(extra);
                    }
                }
                "Contact" => {
                    p.contact = Contact {
                        name: text(c, "Name"),
                        phone: text(c, "Telephone"),
                        email: text(c, "ElectronicMail"),
                    };
                }
                _ => self.skip("Party", c),
            }
        }
        p
    }

    fn payee(&mut self, n: roxmltree::Node<'_, '_>) -> Payee {
        // `cac:PayeeParty` *is* a party in UBL, but CEN's suite also nests a
        // `cac:Party` inside it in a few cases. Accept both.
        let p = self.party(kid(n, "Party").unwrap_or(n));
        Payee {
            name: p.name.or(p.trading_name),
            identifier: p.identifiers.into_iter().next(),
            legal_registration: p.legal_registration,
        }
    }

    fn tax_rep(&mut self, n: roxmltree::Node<'_, '_>) -> TaxRepresentative {
        let p = self.party(n);
        TaxRepresentative {
            name: p.name.or(p.trading_name),
            vat_identifier: p.vat_identifier,
            address: p.address,
        }
    }

    fn delivery(&mut self, n: roxmltree::Node<'_, '_>) -> Delivery {
        let mut d = Delivery::default();
        for c in kids(n) {
            match name(c) {
                "ActualDeliveryDate" => d.date = date(n, "ActualDeliveryDate"),
                "DeliveryLocation" => {
                    if let Some(id) = kid(c, "ID") {
                        d.location = Some(identifier(id));
                    }
                    if let Some(a) = kid(c, "Address") {
                        d.address = Some(self.address(a));
                    }
                }
                "DeliveryParty" => d.party_name = kid(c, "PartyName").and_then(|p| text(p, "Name")),
                _ => self.skip("Delivery", c),
            }
        }
        d
    }

    /// Fold a further `cac:PaymentMeans` element into the instruction already
    /// read — the header terms keep their first value, and credit-transfer
    /// accounts accumulate, which is the one way UBL expresses several BG-17.
    ///
    /// Two elements of *different kinds* — a card and a mandate, say — cannot
    /// both live in the model's single BG-16, so the second kind is dropped
    /// and recorded in [`Reader::unmapped`] rather than silently winning.
    fn merge_payment(
        &mut self,
        mut prev: PaymentInstructions,
        next: PaymentInstructions,
    ) -> PaymentInstructions {
        prev.means_code = prev.means_code.or(next.means_code);
        prev.means_text = prev.means_text.or(next.means_text);
        prev.remittance_information = prev.remittance_information.or(next.remittance_information);
        match (&mut prev.means, next.means) {
            (_, None) => {}
            (slot @ None, some) => *slot = some,
            (
                Some(PaymentMeans::CreditTransfer(accounts)),
                Some(PaymentMeans::CreditTransfer(more)),
            ) => accounts.extend(more),
            (Some(_), Some(_)) => {
                self.unmapped
                    .insert("Invoice/PaymentMeans (second, of a different kind)".to_owned());
            }
        }
        prev
    }

    fn payment(&mut self, n: roxmltree::Node<'_, '_>) -> PaymentInstructions {
        let mut p = PaymentInstructions {
            means_code: code(n, "PaymentMeansCode"),
            means_text: kid(n, "PaymentMeansCode")
                .and_then(|c| c.attribute("name").map(str::to_owned)),
            remittance_information: text(n, "PaymentID"),
            means: None,
        };
        for c in kids(n) {
            match name(c) {
                "PayeeFinancialAccount" => {
                    // Accumulate rather than assign: the schema puts this at
                    // 0..1 per `cac:PaymentMeans`, but a lenient reader of a
                    // document that repeats it anyway should not keep only the
                    // last account.
                    let t = CreditTransfer {
                        account_identifier: text(c, "ID"),
                        account_name: text(c, "Name"),
                        provider_identifier: kid(c, "FinancialInstitutionBranch")
                            .and_then(|b| text(b, "ID")),
                    };
                    match &mut p.means {
                        Some(PaymentMeans::CreditTransfer(ts)) => ts.push(t),
                        slot @ None => *slot = Some(PaymentMeans::CreditTransfer(vec![t])),
                        Some(_) => {
                            self.unmapped.insert(
                                "PaymentMeans/PayeeFinancialAccount (beside a card or mandate)"
                                    .to_owned(),
                            );
                        }
                    }
                }
                // BG-18 and BG-19 take the slot only when it is free. The model
                // holds *one* of BG-17/18/19 — the combinations are what
                // `BR-DE-23/24/25` forbid, and KoSIT's mutation corpus carries
                // them on purpose — so the first kind read wins and a later
                // different kind is recorded, never silently the winner.
                // (Overwriting made the invalid combination read as its *last*
                // group, which could satisfy the `-a` rule the document is
                // built to fail.)
                "CardAccount" => {
                    if p.means.is_none() {
                        p.means = Some(PaymentMeans::Card(PaymentCard {
                            primary_account_number: text(c, "PrimaryAccountNumberID"),
                            holder_name: text(c, "HolderName"),
                        }));
                    } else {
                        self.unmapped.insert(
                            "PaymentMeans/CardAccount (beside another payment means)".to_owned(),
                        );
                    }
                }
                "PaymentMandate" => {
                    if p.means.is_none() {
                        p.means = Some(PaymentMeans::DirectDebit(DirectDebit {
                            mandate_reference: text(c, "ID"),
                            creditor_identifier: None,
                            debited_account: kid(c, "PayerFinancialAccount")
                                .and_then(|a| text(a, "ID")),
                        }));
                    } else {
                        self.unmapped.insert(
                            "PaymentMeans/PaymentMandate (beside another payment means)".to_owned(),
                        );
                    }
                }
                "PaymentMeansCode" | "PaymentID" | "PaymentDueDate" | "InstructionNote" => {}
                _ => self.skip("PaymentMeans", c),
            }
        }
        p
    }

    fn tax_category(
        &mut self,
        n: roxmltree::Node<'_, '_>,
    ) -> (Code, Option<Percentage>, Option<String>, Option<Code>) {
        let mut reason = None;
        let mut reason_code = None;
        for c in kids(n) {
            match name(c) {
                "ID" | "Percent" | "TaxScheme" => {}
                "TaxExemptionReason" => reason = non_empty(own_text(c)),
                "TaxExemptionReasonCode" => reason_code = Some(Code::new(own_text(c))),
                _ => self.skip("TaxCategory", c),
            }
        }
        (
            code(n, "ID").unwrap_or_default(),
            decimal(n, "Percent").map(Percentage::new),
            reason,
            reason_code,
        )
    }

    fn allowance(&mut self, n: roxmltree::Node<'_, '_>) -> DocumentAllowanceCharge {
        let mut vat = LineVat::default();
        for c in kids(n) {
            match name(c) {
                "TaxCategory" => {
                    let (cat, pct, _, _) = self.tax_category(c);
                    vat = LineVat {
                        category: cat,
                        rate: pct,
                    };
                }
                "ChargeIndicator"
                | "Amount"
                | "BaseAmount"
                | "MultiplierFactorNumeric"
                | "AllowanceChargeReason"
                | "AllowanceChargeReasonCode"
                | "TaxScheme" => {}
                _ => self.skip("AllowanceCharge", c),
            }
        }
        DocumentAllowanceCharge {
            amount: amount(n, "Amount").unwrap_or_default(),
            base_amount: amount(n, "BaseAmount"),
            percentage: decimal(n, "MultiplierFactorNumeric").map(Percentage::new),
            vat,
            reason: text(n, "AllowanceChargeReason"),
            reason_code: code(n, "AllowanceChargeReasonCode"),
        }
    }

    fn tax_total(&mut self, n: roxmltree::Node<'_, '_>, inv: &mut Invoice) {
        // BT-110 and BT-111 are the **same element** in UBL, told apart only by
        // `@currencyID`. Record every `TaxTotal/TaxAmount` with its currency and
        // resolve after BT-5 and BT-6 are known — deciding by document order
        // instead gets it backwards whenever the accounting-currency total comes
        // first, and cannot express the case where one element is both.
        if let Some(ta) = kid(n, "TaxAmount") {
            self.tax_amounts.push((
                ta.attribute("currencyID").map(str::to_owned),
                amount_here(ta),
            ));
        }
        let subtotals: Vec<_> = kids(n).filter(|c| name(*c) == "TaxSubtotal").collect();
        if subtotals.is_empty() {
            return;
        }
        for st in subtotals {
            let mut cat = Code::default();
            let mut rate = None;
            let mut reason = None;
            let mut reason_code = None;
            for c in kids(st) {
                match name(c) {
                    "TaxCategory" => {
                        let (a, b, r, rc) = self.tax_category(c);
                        cat = a;
                        rate = b;
                        reason = r;
                        reason_code = rc;
                    }
                    "TaxableAmount" | "TaxAmount" => {}
                    _ => self.skip("TaxSubtotal", c),
                }
            }
            inv.vat_breakdown.push(VatBreakdown {
                taxable_amount: amount(st, "TaxableAmount").unwrap_or_default(),
                tax_amount: amount(st, "TaxAmount").unwrap_or_default(),
                category: cat,
                rate,
                exemption_reason: reason,
                exemption_reason_code: reason_code,
            });
        }
    }

    /// An amount, recording the value if the model cannot hold it.
    fn opt_amt(&mut self, n: roxmltree::Node<'_, '_>, what: &str) -> Option<InvoiceAmount> {
        let raw = own_text(n);
        let parsed = InvoiceAmount::parse(&raw).ok();
        if parsed.is_none() && !raw.is_empty() {
            self.malformed.push(format!("{what}={raw}"));
        }
        parsed
    }

    fn amt(&mut self, n: roxmltree::Node<'_, '_>, what: &str) -> InvoiceAmount {
        self.opt_amt(n, what).unwrap_or_default()
    }

    fn totals(&mut self, n: roxmltree::Node<'_, '_>, inv: &mut Invoice) {
        let t = &mut inv.totals;
        for c in kids(n) {
            match name(c) {
                "LineExtensionAmount" => t.line_total = self.amt(c, "LineExtensionAmount"),
                "TaxExclusiveAmount" => t.taxable_total = self.amt(c, "TaxExclusiveAmount"),
                "TaxInclusiveAmount" => t.gross_total = self.amt(c, "TaxInclusiveAmount"),
                "AllowanceTotalAmount" => {
                    t.allowance_total = self.opt_amt(c, "AllowanceTotalAmount");
                }
                "ChargeTotalAmount" => t.charge_total = self.opt_amt(c, "ChargeTotalAmount"),
                "PrepaidAmount" => t.paid = self.opt_amt(c, "PrepaidAmount"),
                "PayableRoundingAmount" => t.rounding = self.opt_amt(c, "PayableRoundingAmount"),
                "PayableAmount" => t.due = self.amt(c, "PayableAmount"),
                _ => self.skip("LegalMonetaryTotal", c),
            }
        }
    }

    fn line(&mut self, n: roxmltree::Node<'_, '_>) -> InvoiceLine {
        let mut l = InvoiceLine {
            id: text(n, "ID").unwrap_or_default(),
            note: text(n, "Note"),
            order_line_reference: None,
            accounting_reference: text(n, "AccountingCost"),
            object_identifier: None,
            quantity: Quantity::new(
                text(n, "InvoicedQuantity")
                    .or_else(|| text(n, "CreditedQuantity"))
                    .and_then(|q| q.parse().ok())
                    .unwrap_or_default(),
            ),
            unit_code: kid(n, "InvoicedQuantity")
                .or_else(|| kid(n, "CreditedQuantity"))
                .and_then(|q| q.attribute("unitCode"))
                .map(Code::new)
                .unwrap_or_default(),
            net_amount: amount(n, "LineExtensionAmount").unwrap_or_default(),
            period: None,
            allowances: vec![],
            charges: vec![],
            price: PriceDetails::default(),
            vat: LineVat::default(),
            item: Item::default(),
        };
        for c in kids(n) {
            match name(c) {
                "InvoicePeriod" => l.period = Some(self.period(c)),
                "OrderLineReference" => {
                    l.order_line_reference = text(c, "LineID").map(DocumentReference::new);
                }
                "DocumentReference" => {
                    if text(c, "DocumentTypeCode").as_deref() == Some("130")
                        && let Some(id) = kid(c, "ID")
                    {
                        l.object_identifier = Some(identifier(id));
                    }
                }
                "AllowanceCharge" => {
                    let is_charge = self.charge_indicator(c, "InvoiceLine/AllowanceCharge");
                    let a = LineAllowanceCharge {
                        amount: amount(c, "Amount").unwrap_or_default(),
                        base_amount: amount(c, "BaseAmount"),
                        percentage: decimal(c, "MultiplierFactorNumeric").map(Percentage::new),
                        reason: text(c, "AllowanceChargeReason"),
                        reason_code: code(c, "AllowanceChargeReasonCode"),
                    };
                    if is_charge {
                        l.charges.push(a);
                    } else {
                        l.allowances.push(a);
                    }
                }
                "Price" => l.price = self.price(c),
                "Item" => {
                    let (item, vat) = self.item(c);
                    l.item = item;
                    l.vat = vat;
                }
                "ID"
                | "Note"
                | "InvoicedQuantity"
                | "CreditedQuantity"
                | "LineExtensionAmount"
                | "AccountingCost"
                | "TaxTotal"
                | "SubInvoiceLine" => {}
                _ => self.skip("InvoiceLine", c),
            }
        }
        l
    }

    /// `cac:SubInvoiceLine` children, recursively — BG-DEX-01.
    fn sub_lines(&mut self, n: roxmltree::Node<'_, '_>) -> Vec<en16931::SubInvoiceLine> {
        kids(n)
            .filter(|c| name(*c) == "SubInvoiceLine")
            .map(|c| {
                let line = self.line(c);
                // BG-DEX-06 is the sub-line's own `ClassifiedTaxCategory`, and
                // `BR-DEX-03` requires **exactly one**. The model holds nought
                // or one, so two is mapped to `None` — the rule fires either
                // way, which is what "exactly one" means.
                let categories = kid(c, "Item").map_or(0, |i| {
                    kids(i)
                        .filter(|x| name(*x) == "ClassifiedTaxCategory")
                        .count()
                });
                en16931::SubInvoiceLine {
                    vat: (categories == 1).then(|| line.vat.clone()),
                    line,
                    children: self.sub_lines(c),
                }
            })
            .collect()
    }

    fn price(&mut self, n: roxmltree::Node<'_, '_>) -> PriceDetails {
        let mut p = PriceDetails {
            net_price: text(n, "PriceAmount")
                .and_then(|t| t.parse().ok())
                .map(en16931::UnitPriceAmount::new)
                .unwrap_or_default(),
            price_discount: None,
            gross_price: None,
            base_quantity: decimal(n, "BaseQuantity").map(Quantity::new),
            // An empty `unitCode` is an absent BT-150, not a BT-150 whose value
            // is the empty string. The difference is a fatal
            // `PEPPOL-EN16931-R130`, which compares BT-150 against BT-130.
            base_quantity_code: kid(n, "BaseQuantity")
                .and_then(|q| q.attribute("unitCode"))
                .filter(|u| !u.trim().is_empty())
                .map(Code::new),
        };
        for c in kids(n) {
            match name(c) {
                "AllowanceCharge" => {
                    // BG-29's discount and gross price hide inside a price-level
                    // allowance in UBL: `Amount` is BT-147, `BaseAmount` BT-148.
                    //
                    // BT-147 is a **discount**, and the model has no term for a
                    // price-level charge — `PEPPOL-EN16931-R044` forbids one
                    // outright. A document carrying `ChargeIndicator=true` here
                    // is therefore saying something EN 16931 cannot hold, and
                    // reading the amount anyway would file an increase as a
                    // reduction: the same money, subtracted instead of added,
                    // under the core profile where no rule objects. So it is
                    // reported and not mapped.
                    if self.charge_indicator(c, "Price/AllowanceCharge") {
                        self.malformed.push(
                            "Price/AllowanceCharge is a charge; BT-147 is a discount and \
                             EN 16931 has no term for a price-level charge (R044)"
                                .to_owned(),
                        );
                        continue;
                    }
                    p.price_discount = text(c, "Amount")
                        .and_then(|t| t.parse().ok())
                        .map(en16931::UnitPriceAmount::new);
                    p.gross_price = text(c, "BaseAmount")
                        .and_then(|t| t.parse().ok())
                        .map(en16931::UnitPriceAmount::new);
                }
                "PriceAmount" | "BaseQuantity" => {}
                _ => self.skip("Price", c),
            }
        }
        p
    }

    fn item(&mut self, n: roxmltree::Node<'_, '_>) -> (Item, LineVat) {
        let mut item = Item {
            name: text(n, "Name"),
            description: text(n, "Description"),
            seller_identifier: kid(n, "SellersItemIdentification").and_then(|i| text(i, "ID")),
            buyer_identifier: kid(n, "BuyersItemIdentification").and_then(|i| text(i, "ID")),
            standard_identifier: kid(n, "StandardItemIdentification")
                .and_then(|i| kid(i, "ID"))
                .map(identifier),
            classification_identifiers: vec![],
            origin_country: kid(n, "OriginCountry").and_then(|c| code(c, "IdentificationCode")),
            attributes: vec![],
        };
        let mut vat = LineVat::default();
        for c in kids(n) {
            match name(c) {
                "ClassifiedTaxCategory" => {
                    let (cat, pct, _, _) = self.tax_category(c);
                    vat = LineVat {
                        category: cat,
                        rate: pct,
                    };
                }
                "CommodityClassification" => {
                    if let Some(icc) = kid(c, "ItemClassificationCode") {
                        // The scheme is `listID`, not `schemeID`, on this one.
                        let id = match icc.attribute("listID") {
                            Some(s) => Identifier::schemed(own_text(icc), s),
                            None => Identifier::new(own_text(icc)),
                        };
                        item.classification_identifiers.push(id);
                    }
                }
                "AdditionalItemProperty" => item.attributes.push(ItemAttribute {
                    name: text(c, "Name"),
                    value: text(c, "Value"),
                }),
                "Name"
                | "Description"
                | "SellersItemIdentification"
                | "BuyersItemIdentification"
                | "StandardItemIdentification"
                | "OriginCountry" => {}
                _ => self.skip("Item", c),
            }
        }
        (item, vat)
    }

    fn additional_document(&mut self, n: roxmltree::Node<'_, '_>, inv: &mut Invoice) {
        // Document type code `130` is BT-18, the invoiced object identifier.
        // Everything else is BG-24, a supporting document.
        if text(n, "DocumentTypeCode").as_deref() == Some("130") {
            if let Some(id) = kid(n, "ID") {
                inv.object_identifier = Some(identifier(id));
            }
            return;
        }
        // An attachment missing its mime code or filename violates §6.5.11, and
        // `Attachment::new` refuses it. The suites contain such documents on
        // purpose, so record it as unreadable rather than unwrapping.
        let attachment = kid(n, "Attachment")
            .and_then(|a| kid(a, "EmbeddedDocumentBinaryObject"))
            .and_then(|b| {
                match en16931::Attachment::new(
                    crate::xml::decode_base64(&own_text(b)),
                    b.attribute("mimeCode").unwrap_or_default(),
                    b.attribute("filename").unwrap_or_default(),
                ) {
                    Ok(a) => Some(a),
                    Err(e) => {
                        self.malformed
                            .push(format!("EmbeddedDocumentBinaryObject: {e}"));
                        None
                    }
                }
            });
        let uri = kid(n, "Attachment")
            .and_then(|a| kid(a, "ExternalReference"))
            .and_then(|e| text(e, "URI"));
        inv.attachments.push(SupportingDocument {
            reference: text(n, "ID")
                .map_or_else(|| DocumentReference::new(""), DocumentReference::new),
            description: text(n, "DocumentDescription"),
            uri,
            attachment,
        });
    }
}

/// UBL embeds BT-21 in the note text as `#AAI#the text`.
fn read_note(raw: &str) -> InvoiceNote {
    if let Some(rest) = raw.strip_prefix('#')
        && let Some((subject, body)) = rest.split_once('#')
        && subject.len() == 3
    {
        return InvoiceNote {
            subject_code: Some(Code::new(subject)),
            note: Some(body.to_owned()),
        };
    }
    InvoiceNote::new(raw)
}

#[cfg(test)]
mod tests {
    use super::xs_date;

    /// `xs:date` permits a time zone, and a great many producers write one.
    ///
    /// `<cbc:IssueDate>2026-07-31+02:00</cbc:IssueDate>` is schema-valid UBL
    /// that no Schematron rule objects to.
    #[test]
    fn a_ubl_date_may_carry_the_time_zone_xs_date_allows() {
        let expected = en16931::Date::parse("2026-07-31").ok();
        for spelling in [
            "2026-07-31",
            "2026-07-31Z",
            "2026-07-31+02:00",
            "2026-07-31-05:00",
            // The zone is dropped, never applied: the offset says which
            // day-boundary the value was written against, not that the day
            // moves.
            "2026-07-31+14:00",
            "  2026-07-31Z  ",
        ] {
            assert_eq!(xs_date(spelling), expected, "{spelling}");
        }
    }

    /// `cbc:ChargeIndicator` is `xs:boolean`, which has four lexical forms.
    ///
    /// The regression, and the worst kind a reader can have: `1` means *true*
    /// and this reader knew only the word, so a schema-valid charge arrived as
    /// an **allowance** — the same money, with the sign reversed, on the one
    /// element that decides which way it goes.
    #[test]
    fn a_charge_indicator_may_be_written_as_a_digit() {
        assert_eq!(super::xs_boolean("true"), Some(true));
        assert_eq!(super::xs_boolean("1"), Some(true));
        assert_eq!(super::xs_boolean("false"), Some(false));
        assert_eq!(super::xs_boolean("0"), Some(false));
        // `xs:boolean` is whitespace-collapsed, so surrounding space is
        // notation rather than a different value.
        assert_eq!(super::xs_boolean("  true "), Some(true));
        // …and nothing else is a boolean. `None` is reported, never defaulted.
        for other in ["yes", "TRUE", "True", "2", "", "-1"] {
            assert_eq!(super::xs_boolean(other), None, "{other}");
        }
    }

    /// A date that is merely wrong still fails, rather than being trimmed into
    /// shape by the zone-stripping.
    #[test]
    fn a_malformed_date_is_still_malformed() {
        for spelling in [
            "2026-13-01",          // no such month
            "2026-02-30",          // no such day
            "31/07/2026",          // not ISO at all
            "2026-07-31T00:00:00", // xs:dateTime, which cbc:IssueDate is not
            "2026-07-31+2:00",     // a zone, malformed
            "2026-07-31QQ",
            "",
        ] {
            assert_eq!(xs_date(spelling), None, "{spelling}");
        }
    }
}
