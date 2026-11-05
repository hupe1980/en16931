//! Reading UBL 2.1 into the semantic model.
//!
//! Every element walked is mapped or recorded as an
//! [`Unmapped`](crate::report::Kind::Unmapped) entry (`Parent/Child`). A value
//! the model cannot hold (`<cbc:IssueDate>123`) leaves the term absent and is a
//! [`Malformed`](crate::report::Kind::Malformed) entry, so "absent" and
//! "present but unreadable" stay distinguishable.

use std::collections::BTreeSet;

use crate::report::{Entry, Kind};
use crate::xml::Term;
use crate::xml::{Node, kid, kids, name};

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
pub(crate) struct Reader {
    /// Element paths encountered and not mapped, as `Parent/Child`, each once.
    unmapped: BTreeSet<String>,
    /// Every `cac:TaxTotal/cbc:TaxAmount` seen, resolved into BT-110 / BT-111
    /// once BT-5 and BT-6 are known.
    tax_amounts: Vec<TaxAmountSeen>,
    /// Everything else the reader has to say, in document order — values
    /// present and not representable, combinations the model cannot hold.
    entries: Vec<Entry>,
    /// BT-5, pre-scanned, against which every amount's `@currencyID` is
    /// checked.
    currency: Option<String>,
    /// BT-24, pre-scanned: a term is missing only if the declared profile
    /// runs the rule requiring it (`crate::required`).
    bt24: Option<String>,
}

/// One `cac:TaxTotal/cbc:TaxAmount`, held until BT-5 and BT-6 are known.
struct TaxAmountSeen {
    /// Where it was, for an entry when it is not read.
    path: String,
    /// Its `@currencyID`, trimmed; `None` when absent or blank.
    currency: Option<String>,
    /// Its text, as written.
    raw: String,
    amount: Option<InvoiceAmount>,
}

/// Whether a UBL `cac:TaxScheme/cbc:ID` names VAT, as CEN's binding tests it
/// (`normalize-space(upper-case(cbc:ID)) = 'VAT'`): only then is the category
/// or the company identifier beside it a VAT term.
fn is_vat(scheme: Option<&str>) -> bool {
    scheme.is_some_and(|s| s.trim().eq_ignore_ascii_case("VAT"))
}

/// Trimmed text of the first child named `want`.
fn text(n: Node<'_, '_>, want: &str) -> Option<String> {
    crate::xml::child_text(n, want)
}

/// Trimmed text of this element, empty when there is none.
fn own_text(n: Node<'_, '_>) -> String {
    crate::xml::text(n).unwrap_or_default()
}

/// `None` for an empty string.
fn non_empty(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

/// A UBL `xs:date`, whose optional time zone (`2026-07-31+02:00`) is dropped,
/// not applied: EN 16931-1 §6.5.9 has no term for one, and the day is the day.
fn xs_date(raw: &str) -> Option<Date> {
    Date::parse(without_timezone(raw.trim())).ok()
}

/// `cbc:ChargeIndicator` as `xs:boolean`: `true`, `false`, `1` or `0`.
///
/// `None` for anything else, which the caller reports as `Malformed`.
fn xs_boolean(raw: &str) -> Option<bool> {
    match raw.trim() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

/// `2026-07-31Z` and `2026-07-31+02:00` → `2026-07-31`.
///
/// Anything else is returned untouched, so a malformed date still fails in
/// [`Date::parse`].
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
fn is_malformed<T>(n: Node<'_, '_>, want: &str, parse: impl Fn(&str) -> Option<T>) -> bool {
    text(n, want).is_some_and(|t| !t.is_empty() && parse(&t).is_none())
}

fn code(n: Node<'_, '_>, want: &str) -> Option<Code> {
    text(n, want).map(Code::new)
}

/// `<cbc:ID schemeID="0088">x</cbc:ID>` → an [`Identifier`] with its scheme.
fn identifier(n: Node<'_, '_>) -> Identifier {
    match n.attribute("schemeID") {
        Some(s) => Identifier::schemed(own_text(n), s),
        None => Identifier::new(own_text(n)),
    }
}

impl Reader {
    /// Note an element this reader does not map.
    fn skip(&mut self, parent: &str, n: Node<'_, '_>) {
        self.unmapped.insert(format!("{parent}/{}", name(n)));
    }

    /// Note a combination the model cannot hold, at `path`.
    fn not_held(&mut self, path: &str, message: &str) {
        self.entries
            .push(Entry::new(Kind::Unmapped, path, message.to_owned()));
    }

    /// Everything the reader has to say: its own entries in document order, then
    /// each unmapped path once, sorted.
    pub(crate) fn finish(self) -> Vec<Entry> {
        let mut out = self.entries;
        out.extend(self.unmapped.into_iter().map(|path| {
            Entry::new(
                Kind::Unmapped,
                path,
                "not mapped: outside the EN 16931 subset, or an extension the declared \
                 profile does not define",
            )
        }));
        out
    }

    /// Whether a `cac:AllowanceCharge` is a charge. An absent or unreadable
    /// indicator is a `Malformed` entry and reads as `false`.
    fn charge_indicator(&mut self, n: Node<'_, '_>, at: &str) -> bool {
        let raw = text(n, "ChargeIndicator");
        if let Some(Some(b)) = raw.as_deref().map(xs_boolean) {
            return b;
        }
        let e = Entry::new(
            Kind::Malformed,
            format!("{at}/ChargeIndicator"),
            match &raw {
                Some(_) => "not an xs:boolean",
                None => "absent, and UBL requires it",
            },
        )
        .hint("the value must be `true` or `false`");
        self.entries.push(match raw {
            Some(v) => e.value(v),
            None => e,
        });
        false
    }

    /// The text of `n`'s child `want` parsed by `parse`, or `None` — with a
    /// [`Kind::Malformed`] entry when the text is present and does not parse.
    fn value<T, E: std::fmt::Display>(
        &mut self,
        n: Node<'_, '_>,
        want: &str,
        term: Option<Term<'_>>,
        parse: impl Fn(&str) -> Result<T, E>,
    ) -> Option<T> {
        let raw = text(n, want)?;
        match parse(&raw) {
            Ok(v) => Some(v),
            Err(e) => {
                let entry = Entry::new(
                    Kind::Malformed,
                    format!("{}/{want}", name(n)),
                    e.to_string(),
                )
                .value(raw);
                self.entries.push(match term {
                    Some(t) => entry.term(t.to_string()),
                    None => entry,
                });
                None
            }
        }
    }

    /// An amount in `want`, recording one the model cannot hold: not an
    /// amount, more than two decimals, or in another currency than BT-5.
    fn amount(&mut self, n: Node<'_, '_>, want: &str, term: Term<'_>) -> Option<InvoiceAmount> {
        if let Some(e) = kid(n, want)
            && !self.in_document_currency(e, term)
        {
            return None;
        }
        self.value(n, want, Some(term), InvoiceAmount::parse)
    }

    /// Whether the amount element `e` is in BT-5. A blank or absent
    /// `@currencyID` means BT-5; another currency is recorded and the amount
    /// left out — reading it as BT-5 would change its value.
    fn in_document_currency(&mut self, e: Node<'_, '_>, term: Term<'_>) -> bool {
        let Some(ccy) = e
            .attribute("currencyID")
            .map(str::trim)
            .filter(|c| !c.is_empty())
        else {
            return true;
        };
        // `BR-CL-03`: the code itself must be ISO 4217, whatever BT-5 is.
        if !en16931::codes::contains(en16931::codes::generated::CURRENCY_CODES, ccy) {
            self.entries.push(
                Entry::new(
                    Kind::Malformed,
                    e.parent_element().map_or_else(
                        || name(e).to_owned(),
                        |p| format!("{}/{}", name(p), name(e)),
                    ),
                    format!("{ccy} is not an ISO 4217 currency code; the amount is not read"),
                )
                .term(term.to_string())
                .value(ccy.to_owned())
                .hint("BR-CL-03: currencyID MUST be coded using ISO code list 4217 alpha-3"),
            );
            return false;
        }
        match self.currency.as_deref() {
            Some(doc) if doc != ccy => {
                let path = e.parent_element().map_or_else(
                    || name(e).to_owned(),
                    |p| format!("{}/{}", name(p), name(e)),
                );
                self.entries.push(
                    Entry::new(
                        Kind::Malformed,
                        path,
                        format!("an amount in {ccy}, not in the invoice currency (BT-5 = {doc}); not read"),
                    )
                    .term(term.to_string())
                    .value(ccy.to_owned()),
                );
                false
            }
            _ => true,
        }
    }

    /// Record that `term`, which the model requires, is not in the group `at`.
    fn missing(&mut self, at: Node<'_, '_>, term: Term<'_>, what: &str) {
        self.missing_at(crate::xml::path_of(at), term, what);
    }

    /// [`Self::missing`] at a path, for a group that is itself absent.
    fn missing_at(&mut self, path: String, term: Term<'_>, what: &str) {
        if !crate::required::is_required(self.bt24.as_deref(), term.term()) {
            return;
        }
        let why = crate::required::rule_for(term.term()).map_or_else(
            || "the model requires it".to_owned(),
            |r| format!("{r} requires it"),
        );
        self.entries.push(
            Entry::new(
                Kind::Missing,
                path,
                format!("{what} is absent; {why}, and the model holds a placeholder"),
            )
            .term(term.to_string()),
        );
    }

    /// A required amount: absent or empty is [`Kind::Missing`], unreadable
    /// [`Kind::Malformed`]; either way the placeholder is zero.
    fn required_amount(
        &mut self,
        n: Node<'_, '_>,
        want: &str,
        term: Term<'_>,
        what: &str,
    ) -> InvoiceAmount {
        if text(n, want).is_none() {
            self.missing(n, term, what);
            return InvoiceAmount::default();
        }
        self.amount(n, want, term).unwrap_or_default()
    }

    /// The amount in `n` itself.
    fn amount_here(&mut self, n: Node<'_, '_>) -> Option<InvoiceAmount> {
        let raw = own_text(n);
        match InvoiceAmount::parse(&raw) {
            Ok(a) => Some(a),
            Err(e) => {
                if !raw.is_empty() {
                    let path = n.parent_element().map_or_else(
                        || name(n).to_owned(),
                        |p| format!("{}/{}", name(p), name(n)),
                    );
                    self.entries
                        .push(Entry::new(Kind::Malformed, path, e.to_string()).value(raw));
                }
                None
            }
        }
    }

    /// A unit price in `want`: a number, in BT-5.
    fn unit_price(
        &mut self,
        n: Node<'_, '_>,
        want: &str,
        term: Term<'_>,
    ) -> Option<en16931::UnitPriceAmount> {
        if let Some(e) = kid(n, want)
            && !self.in_document_currency(e, term)
        {
            return None;
        }
        self.decimal(n, want, term)
            .map(en16931::UnitPriceAmount::new)
    }

    /// A number in `want`, recording text that is not one.
    fn decimal(
        &mut self,
        n: Node<'_, '_>,
        want: &str,
        term: Term<'_>,
    ) -> Option<rust_decimal::Decimal> {
        self.value(n, want, Some(term), |t| {
            t.parse::<rust_decimal::Decimal>()
                .map_err(|_| "not a decimal number the model can hold")
        })
    }

    fn date(&mut self, n: Node<'_, '_>, want: &str) -> Option<Date> {
        self.value(n, want, None, |t| xs_date(t).ok_or("not an xs:date"))
    }

    /// Read a UBL `Invoice` or `CreditNote` root element.
    pub(crate) fn read(&mut self, root: Node<'_, '_>) -> Invoice {
        let mut inv = Invoice::default();
        self.bt24 = text(root, "CustomizationID");
        self.currency = text(root, "DocumentCurrencyCode");
        // The root element is the document kind.
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
                        self.entries.push(
                            Entry::new(Kind::Malformed, "Invoice/IssueDate", "not an xs:date")
                                .term("BT-2")
                                .value(own_text(c)),
                        );
                    }
                    inv.issue_date = text(root, "IssueDate").and_then(|t| xs_date(&t));
                }
                "DueDate" => inv.due_date = self.date(root, "DueDate"),
                "InvoiceTypeCode" | "CreditNoteTypeCode" => {
                    inv.type_code = Some(Code::new(own_text(c)));
                }
                "Note" => inv.notes.push(read_note(&own_text(c))),
                "TaxPointDate" => inv.vat_point_date = self.date(root, "TaxPointDate"),
                "DocumentCurrencyCode" => inv.currency = Some(Code::new(own_text(c))),
                "TaxCurrencyCode" => inv.vat_accounting_currency = Some(Code::new(own_text(c))),
                "AccountingCost" => inv.accounting_reference = non_empty(own_text(c)),
                "BuyerReference" => inv.buyer_reference = non_empty(own_text(c)),
                "InvoicePeriod" => {
                    // An `InvoicePeriod` carrying only a `DescriptionCode` is
                    // BT-8, not BG-14 (else `BR-CO-19` fires on a CEN example).
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
                        if text(r, "ID").is_none() {
                            let term = format!("BG-3[{}]/BT-25", inv.preceding_invoices.len() + 1);
                            self.missing(
                                r,
                                Term::new("", &term),
                                "the preceding invoice reference",
                            );
                        }
                        inv.preceding_invoices.push(PrecedingInvoice {
                            reference: text(r, "ID")
                                .map_or_else(|| DocumentReference::new(""), DocumentReference::new),
                            issue_date: self.date(r, "IssueDate"),
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
                // BG-16 is 0..1 in the model and `cac:PaymentMeans` 0..n in
                // UBL: several elements are one instruction with several BG-17
                // accounts, so they are merged.
                "PaymentMeans" => {
                    let next = self.payment(c);
                    inv.payment = Some(match inv.payment.take() {
                        None => next,
                        Some(prev) => self.merge_payment(prev, next),
                    });
                }
                // Not trimmed: `BR-DE-18` requires the Skonto block to end
                // with a newline.
                "PaymentTerms" => {
                    inv.payment_terms = kid(c, "Note").and_then(crate::xml::raw_text);
                }
                "AllowanceCharge" => {
                    let is_charge = self.charge_indicator(c, "AllowanceCharge");
                    let index = if is_charge {
                        inv.charges.len()
                    } else {
                        inv.allowances.len()
                    } + 1;
                    let ac = self.allowance(c, is_charge, index);
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
                            amount: self.amount(
                                c,
                                "PaidAmount",
                                Term::new("BG-DEX-09", "BT-DEX-002"),
                            ),
                            description: text(c, "InstructionID"),
                        });
                }
                "LegalMonetaryTotal" => {
                    self.totals(c, &mut inv);
                    self.required_totals(root, Some(c), &mut inv);
                }
                "InvoiceLine" | "CreditNoteLine" => {
                    // A line's BT-129 and BT-146 are required (`BR-22`,
                    // `BR-26`); an absent one is already `Missing`.
                    let (l, _, _) = self.line(c, inv.lines.len() + 1);
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
        // BT-90 is the seller's `cac:PartyIdentification` with
        // `schemeID="SEPA"` in UBL and lives in BG-19 in the model. It is moved,
        // not copied, and only when there is a BG-19 to move it into.
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

        if kid(root, "LegalMonetaryTotal").is_none() {
            self.required_totals(root, None, &mut inv);
        }
        // Resolve BT-110 / BT-111 now that BT-5 and BT-6 are known. When the
        // two currencies coincide, one element is both, as CEN's `BR-53`
        // binding reads it.
        let doc_ccy = inv.currency.as_ref().map(|c| c.as_str().to_owned());
        let tax_ccy = inv
            .vat_accounting_currency
            .as_ref()
            .map(|c| c.as_str().to_owned());
        let (mut doc_taken, mut tax_taken) = (false, false);
        for seen in std::mem::take(&mut self.tax_amounts) {
            // No `@currencyID`, or a blank one, means the document currency.
            let ccy = seen.currency;
            let is_doc = ccy.is_none() || ccy == doc_ccy;
            let is_tax = tax_ccy.is_some() && ccy == tax_ccy;
            if (is_doc || is_tax) && (!is_doc || doc_taken) && (!is_tax || tax_taken) {
                // The first total in a currency is the one read.
                let term = if is_doc { "BT-110" } else { "BT-111" };
                self.entries.push(
                    Entry::new(
                        Kind::Unmapped,
                        seen.path,
                        format!(
                            "a further tax total in {}; {term} is read from the first; not read",
                            ccy.as_deref()
                                .or(doc_ccy.as_deref())
                                .unwrap_or("the invoice currency"),
                        ),
                    )
                    .term(term.to_string())
                    .value(seen.raw),
                );
                continue;
            }
            if !is_doc && !is_tax {
                // A total in a third currency: the model has no place for it,
                // and as information it let BT-110 go missing unremarked.
                let e = Entry::new(
                    Kind::Malformed,
                    seen.path,
                    format!(
                        "a tax total in {}, which is neither the invoice currency (BT-5{}) \
                         nor the VAT accounting currency (BT-6{}); not read",
                        ccy.as_deref().unwrap_or_default(),
                        doc_ccy
                            .as_deref()
                            .map(|c| format!(" = {c}"))
                            .unwrap_or_default(),
                        tax_ccy
                            .as_deref()
                            .map(|c| format!(" = {c}"))
                            .unwrap_or_default(),
                    ),
                )
                .value(seen.raw);
                self.entries.push(e);
                continue;
            }
            if is_doc && !doc_taken {
                doc_taken = true;
                inv.totals.vat_total = seen.amount;
            }
            if is_tax && !tax_taken {
                tax_taken = true;
                inv.totals.vat_total_accounting = seen.amount;
            }
        }
        inv
    }

    fn period(&mut self, n: Node<'_, '_>) -> Period {
        for c in kids(n) {
            if !matches!(name(c), "StartDate" | "EndDate" | "DescriptionCode") {
                self.skip("InvoicePeriod", c);
            }
        }
        Period {
            start: self.date(n, "StartDate"),
            end: self.date(n, "EndDate"),
        }
    }

    fn address(&mut self, n: Node<'_, '_>) -> PostalAddress {
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

    fn party(&mut self, n: Node<'_, '_>) -> Party {
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
                    match scheme {
                        Some(s) if is_vat(Some(&s)) => p.vat_identifier = id,
                        Some(_) => p.tax_registration = id,
                        // CEN binds both by the scheme (`= 'VAT'` / `!= 'VAT'`),
                        // and neither holds without one: not guessed.
                        None => self.entries.push({
                            let e = Entry::new(
                                Kind::Unmapped,
                                "Party/PartyTaxScheme/TaxScheme",
                                "a party tax scheme without a scheme identifier is neither the VAT \
                                 identifier nor the tax registration identifier; not mapped",
                            );
                            match id {
                                Some(v) => e.value(v),
                                None => e,
                            }
                        }),
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

    fn payee(&mut self, n: Node<'_, '_>) -> Payee {
        // `cac:PayeeParty` *is* a party in UBL, but CEN's suite also nests a
        // `cac:Party` inside it in a few cases. Accept both.
        let p = self.party(kid(n, "Party").unwrap_or(n));
        Payee {
            name: p.name.or(p.trading_name),
            identifier: p.identifiers.into_iter().next(),
            legal_registration: p.legal_registration,
        }
    }

    fn tax_rep(&mut self, n: Node<'_, '_>) -> TaxRepresentative {
        let p = self.party(n);
        if let Some(id) = &p.tax_registration {
            self.entries.push(
                Entry::new(
                    Kind::Unmapped,
                    "TaxRepresentativeParty/PartyTaxScheme",
                    "a tax registration under a scheme other than VAT; BG-11 holds only the VAT \
                     identifier (BT-63); not mapped",
                )
                .term("BG-11")
                .value(id.clone()),
            );
        }
        TaxRepresentative {
            name: p.name.or(p.trading_name),
            vat_identifier: p.vat_identifier,
            address: p.address,
        }
    }

    fn delivery(&mut self, n: Node<'_, '_>) -> Delivery {
        let mut d = Delivery::default();
        for c in kids(n) {
            match name(c) {
                "ActualDeliveryDate" => d.date = self.date(n, "ActualDeliveryDate"),
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
    /// read: header terms keep their first value, credit-transfer accounts
    /// accumulate, and a second kind of means is reported, not read.
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
            (Some(_), Some(_)) => self.not_held(
                "Invoice/PaymentMeans",
                "a further payment means of a different kind; BG-16 holds one kind",
            ),
        }
        prev
    }

    fn payment(&mut self, n: Node<'_, '_>) -> PaymentInstructions {
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
                    // Accumulate: a repeated account is kept, not replaced.
                    let t = CreditTransfer {
                        account_identifier: text(c, "ID"),
                        account_name: text(c, "Name"),
                        provider_identifier: kid(c, "FinancialInstitutionBranch")
                            .and_then(|b| text(b, "ID")),
                    };
                    match &mut p.means {
                        Some(PaymentMeans::CreditTransfer(ts)) => ts.push(t),
                        slot @ None => *slot = Some(PaymentMeans::CreditTransfer(vec![t])),
                        Some(_) => self.not_held(
                            "PaymentMeans/PayeeFinancialAccount",
                            "an account beside a card or mandate; BG-16 holds one kind",
                        ),
                    }
                }
                // The model holds one of BG-17/18/19: the first kind read
                // wins and a later different kind is reported.
                "CardAccount" => {
                    if p.means.is_none() {
                        p.means = Some(PaymentMeans::Card(PaymentCard {
                            primary_account_number: text(c, "PrimaryAccountNumberID"),
                            holder_name: text(c, "HolderName"),
                        }));
                    } else {
                        self.not_held(
                            "PaymentMeans/CardAccount",
                            "a card beside another payment means; BG-16 holds one kind",
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
                        self.not_held(
                            "PaymentMeans/PaymentMandate",
                            "a mandate beside another payment means; BG-16 holds one kind",
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
        n: Node<'_, '_>,
        category: Term<'_>,
        rate: Term<'_>,
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
        // The category is read whatever the scheme: CEN's own `BR-CL-17` /
        // `BR-CL-18` cases carry it with no `cac:TaxScheme` and expect it
        // checked. A scheme stated as something other than VAT is said —
        // EN 16931 carries VAT only, and `BR-47`'s binding would not count it.
        if let Some(scheme) = kid(n, "TaxScheme").and_then(|s| text(s, "ID"))
            && !is_vat(Some(&scheme))
        {
            self.entries.push(
                Entry::new(
                    Kind::Unmapped,
                    "TaxCategory/TaxScheme/ID",
                    "a tax category under a scheme other than VAT; EN 16931 carries VAT only, and \
                     the category is read as a VAT category",
                )
                .term(category.to_string())
                .value(scheme),
            );
        }
        let cat = code(n, "ID").unwrap_or_else(|| {
            self.missing(n, category, "the VAT category code");
            Code::default()
        });
        (
            cat,
            self.decimal(n, "Percent", rate).map(Percentage::new),
            reason,
            reason_code,
        )
    }

    fn allowance(
        &mut self,
        n: Node<'_, '_>,
        is_charge: bool,
        index: usize,
    ) -> DocumentAllowanceCharge {
        let (group, [amount, base, pct, cat, rate]) = if is_charge {
            (
                format!("BG-21[{index}]"),
                ["BT-99", "BT-100", "BT-101", "BT-102", "BT-103"],
            )
        } else {
            (
                format!("BG-20[{index}]"),
                ["BT-92", "BT-93", "BT-94", "BT-95", "BT-96"],
            )
        };
        if kid(n, "TaxCategory").is_none() {
            self.missing(n, Term::new(&group, cat), "the VAT category code");
        }
        let mut vat = LineVat::default();
        for c in kids(n) {
            match name(c) {
                "TaxCategory" => {
                    let (cat, pct, _, _) =
                        self.tax_category(c, Term::new(&group, cat), Term::new(&group, rate));
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
            amount: self.required_amount(
                n,
                "Amount",
                Term::new(&group, amount),
                if is_charge {
                    "the charge amount"
                } else {
                    "the allowance amount"
                },
            ),
            base_amount: self.amount(n, "BaseAmount", Term::new(&group, base)),
            percentage: self
                .decimal(n, "MultiplierFactorNumeric", Term::new(&group, pct))
                .map(Percentage::new),
            vat,
            reason: text(n, "AllowanceChargeReason"),
            reason_code: code(n, "AllowanceChargeReasonCode"),
        }
    }

    fn tax_total(&mut self, n: Node<'_, '_>, inv: &mut Invoice) {
        // BT-110 and BT-111 are the same element, told apart by `@currencyID`;
        // resolved in `read` once BT-5 and BT-6 are known.
        if let Some(ta) = kid(n, "TaxAmount") {
            let amount = self.amount_here(ta);
            self.tax_amounts.push(TaxAmountSeen {
                path: crate::xml::path_of(ta),
                currency: ta
                    .attribute("currencyID")
                    .map(|c| c.trim().to_owned())
                    .filter(|c| !c.is_empty()),
                raw: own_text(ta),
                amount,
            });
        }
        let subtotals: Vec<_> = kids(n).filter(|c| name(*c) == "TaxSubtotal").collect();
        if subtotals.is_empty() {
            return;
        }
        for st in subtotals {
            let group = format!("BG-23[{}]", inv.vat_breakdown.len() + 1);
            if kid(st, "TaxCategory").is_none() {
                self.missing(st, Term::new(&group, "BT-118"), "the VAT category code");
            }
            let mut cat = Code::default();
            let mut rate = None;
            let mut reason = None;
            let mut reason_code = None;
            for c in kids(st) {
                match name(c) {
                    "TaxCategory" => {
                        let (a, b, r, rc) = self.tax_category(
                            c,
                            Term::new(&group, "BT-118"),
                            Term::new(&group, "BT-119"),
                        );
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
                taxable_amount: self.required_amount(
                    st,
                    "TaxableAmount",
                    Term::new(&group, "BT-116"),
                    "the VAT category taxable amount",
                ),
                tax_amount: self.required_amount(
                    st,
                    "TaxAmount",
                    Term::new(&group, "BT-117"),
                    "the VAT category tax amount",
                ),
                category: cat,
                rate,
                exemption_reason: reason,
                exemption_reason_code: reason_code,
            });
        }
    }

    /// An amount, recording the value if the model cannot hold it.
    fn opt_amt(&mut self, n: Node<'_, '_>, what: &str) -> Option<InvoiceAmount> {
        let term = Self::total_term(what);
        if !self.in_document_currency(n, term) {
            return None;
        }
        let raw = own_text(n);
        let parsed = InvoiceAmount::parse(&raw).ok();
        if parsed.is_none() && !raw.is_empty() {
            self.entries.push(
                Entry::new(
                    Kind::Malformed,
                    format!("LegalMonetaryTotal/{what}"),
                    "not an amount the model can hold (at most two decimals)",
                )
                .term(term.to_string())
                .value(raw),
            );
        }
        parsed
    }

    /// The BG-22 term each `cac:LegalMonetaryTotal` child carries.
    fn total_term(what: &str) -> Term<'static> {
        Term::new(
            "BG-22",
            match what {
                "LineExtensionAmount" => "BT-106",
                "TaxExclusiveAmount" => "BT-109",
                "TaxInclusiveAmount" => "BT-112",
                "AllowanceTotalAmount" => "BT-107",
                "ChargeTotalAmount" => "BT-108",
                "PrepaidAmount" => "BT-113",
                "PayableRoundingAmount" => "BT-114",
                _ => "BT-115",
            },
        )
    }

    /// BG-22's four required amounts, said when absent — whether the element
    /// or the whole `cac:LegalMonetaryTotal` is missing (`at` is then the
    /// document element).
    fn required_totals(
        &mut self,
        at: Node<'_, '_>,
        totals: Option<Node<'_, '_>>,
        inv: &mut Invoice,
    ) {
        for (what, label) in [
            ("LineExtensionAmount", "the sum of invoice line net amounts"),
            ("TaxExclusiveAmount", "the invoice total amount without VAT"),
            ("TaxInclusiveAmount", "the invoice total amount with VAT"),
            ("PayableAmount", "the amount due for payment"),
        ] {
            let el = totals.and_then(|t| kid(t, what));
            let value = match el {
                Some(e) if !own_text(e).is_empty() => self.opt_amt(e, what).unwrap_or_default(),
                _ => {
                    match totals {
                        Some(t) => self.missing(t, Self::total_term(what), label),
                        // The whole group is absent: its path is where it would be.
                        None => self.missing_at(
                            format!("{}/LegalMonetaryTotal", crate::xml::path_of(at)),
                            Self::total_term(what),
                            label,
                        ),
                    }
                    InvoiceAmount::default()
                }
            };
            let t = &mut inv.totals;
            match what {
                "LineExtensionAmount" => t.line_total = value,
                "TaxExclusiveAmount" => t.taxable_total = value,
                "TaxInclusiveAmount" => t.gross_total = value,
                _ => t.due = value,
            }
        }
    }

    fn totals(&mut self, n: Node<'_, '_>, inv: &mut Invoice) {
        let t = &mut inv.totals;
        for c in kids(n) {
            match name(c) {
                // Required: read by `required_totals`, so an absent one is said.
                "LineExtensionAmount"
                | "TaxExclusiveAmount"
                | "TaxInclusiveAmount"
                | "PayableAmount" => {}
                "AllowanceTotalAmount" => {
                    t.allowance_total = self.opt_amt(c, "AllowanceTotalAmount");
                }
                "ChargeTotalAmount" => t.charge_total = self.opt_amt(c, "ChargeTotalAmount"),
                "PrepaidAmount" => t.paid = self.opt_amt(c, "PrepaidAmount"),
                "PayableRoundingAmount" => t.rounding = self.opt_amt(c, "PayableRoundingAmount"),
                _ => self.skip("LegalMonetaryTotal", c),
            }
        }
    }

    /// One line; `index` is its 1-based position among its siblings, passed
    /// in rather than counted, which would make a document quadratic.
    ///
    /// Returns, besides the line, whether it carries a quantity element and a
    /// `cac:Price` (`en16931::SubInvoiceLine::quantity_stated`,
    /// `price_stated`). `BR-22`, `BR-23` and `BR-26` hold on `cac:InvoiceLine |
    /// cac:CreditNoteLine` only (`eInvoicing-EN16931/ubl/schematron/
    /// preprocessed/EN16931-UBL-validation-preprocessed.sch`), so a
    /// `cac:SubInvoiceLine`, grouping or childless, may leave BT-129, BT-130
    /// and BG-29 out; the KoSIT validator accepts it. Present but empty is
    /// still refused: UBL's schema requires a value wherever the element is.
    /// A value that is refused is held as a placeholder, as on a line.
    fn line(&mut self, n: Node<'_, '_>, index: usize) -> (InvoiceLine, bool, bool) {
        // A sub-line is BG-DEX-01; its index is among its siblings, and its
        // path says which line it hangs beneath.
        let sub = name(n) == "SubInvoiceLine";
        let group = if sub {
            format!("BG-DEX-01[{index}]")
        } else {
            format!("BG-25[{index}]")
        };
        let quantity_el = if kid(n, "InvoicedQuantity").is_some() {
            "InvoicedQuantity"
        } else {
            "CreditedQuantity"
        };
        let id = text(n, "ID").unwrap_or_else(|| {
            self.missing(
                n,
                Term::new(&group, "BT-126"),
                "the invoice line identifier",
            );
            String::new()
        });
        let quantity = if text(n, quantity_el).is_some() {
            self.decimal(n, quantity_el, Term::new(&group, "BT-129"))
        } else {
            if !sub {
                self.missing(n, Term::new(&group, "BT-129"), "the invoiced quantity");
            } else if kid(n, quantity_el).is_some() {
                self.refused_on_sub_line(
                    n,
                    quantity_el,
                    Term::new(&group, "BT-129"),
                    "present without a value; the UBL schema requires a decimal",
                );
            }
            None
        };
        // `cac:SubInvoiceLine` is UBL's `InvoiceLineType`, which has
        // `cbc:InvoicedQuantity` only; a credit note nests
        // `cac:SubCreditNoteLine`. The quantity is still read.
        if sub && quantity_el == "CreditedQuantity" && kid(n, quantity_el).is_some() {
            self.entries.push(
                Entry::new(
                    Kind::Schema,
                    format!("{}/CreditedQuantity", crate::xml::path_of(n)),
                    "a cac:SubInvoiceLine has cbc:InvoicedQuantity only; a credit note nests \
                     cac:SubCreditNoteLine",
                )
                .term(Term::new(&group, "BT-129").to_string()),
            );
        }
        let quantity_stated = kid(n, quantity_el).is_some();
        let quantity = quantity.map_or(Quantity::ZERO, Quantity::new);
        let unit_code = if let Some(u) = kid(n, quantity_el)
            .and_then(|q| q.attribute("unitCode"))
            .map(str::trim)
            .filter(|u| !u.is_empty())
        {
            Code::new(u)
        } else {
            if !sub {
                self.missing(
                    n,
                    Term::new(&group, "BT-130"),
                    "the invoiced quantity unit of measure code",
                );
            }
            Code::default()
        };
        let net_amount = self.required_amount(
            n,
            "LineExtensionAmount",
            Term::new(&group, "BT-131"),
            "the invoice line net amount",
        );
        if !sub && kid(n, "Price").is_none() {
            self.missing(n, Term::new(&group, "BT-146"), "the item net price");
        }
        // A sub-line's BG-DEX-06 is optional in the model (`BR-DEX-03` checks
        // it); only a line's BT-151 is filled with a placeholder.
        if name(n) != "SubInvoiceLine"
            && kid(n, "Item")
                .and_then(|i| kid(i, "ClassifiedTaxCategory"))
                .is_none()
        {
            self.missing(
                n,
                Term::new(&group, "BT-151"),
                "the invoiced item VAT category code",
            );
        }
        let mut l = InvoiceLine {
            id,
            note: text(n, "Note"),
            order_line_reference: None,
            accounting_reference: text(n, "AccountingCost"),
            object_identifier: None,
            quantity,
            unit_code,
            net_amount,
            period: None,
            allowances: vec![],
            charges: vec![],
            price: PriceDetails::default(),
            vat: LineVat::default(),
            item: Item::default(),
        };
        let mut price_stated = false;
        for c in kids(n) {
            match name(c) {
                "InvoicePeriod" => l.period = Some(self.period(c)),
                "OrderLineReference" => {
                    l.order_line_reference = text(c, "LineID").map(DocumentReference::new);
                }
                "DocumentReference" => {
                    if text(c, "DocumentTypeCode").as_deref() == Some("130") {
                        if let Some(id) = kid(c, "ID") {
                            l.object_identifier = Some(identifier(id));
                        }
                    } else {
                        // BT-128 is the line's only document reference
                        // (`PEPPOL-EN16931-R101`); another is said, not dropped.
                        let e = Entry::new(
                            Kind::Unmapped,
                            "InvoiceLine/DocumentReference/DocumentTypeCode",
                            "a line-level document reference other than the invoiced object \
                             (BT-128, type code 130); not mapped",
                        )
                        .term(format!("{group}/BT-128"))
                        .rule("PEPPOL-EN16931-R101");
                        self.entries.push(match text(c, "DocumentTypeCode") {
                            Some(v) => e.value(v),
                            None => e,
                        });
                    }
                }
                "AllowanceCharge" => {
                    let is_charge = self.charge_indicator(c, "InvoiceLine/AllowanceCharge");
                    let (sub, [amount, base, pct]) = if is_charge {
                        (
                            format!("{group}/BG-28[{}]", l.charges.len() + 1),
                            ["BT-141", "BT-142", "BT-143"],
                        )
                    } else {
                        (
                            format!("{group}/BG-27[{}]", l.allowances.len() + 1),
                            ["BT-136", "BT-137", "BT-138"],
                        )
                    };
                    let a = LineAllowanceCharge {
                        amount: self.required_amount(
                            c,
                            "Amount",
                            Term::new(&sub, amount),
                            if is_charge {
                                "the line charge amount"
                            } else {
                                "the line allowance amount"
                            },
                        ),
                        base_amount: self.amount(c, "BaseAmount", Term::new(&sub, base)),
                        percentage: self
                            .decimal(c, "MultiplierFactorNumeric", Term::new(&sub, pct))
                            .map(Percentage::new),
                        reason: text(c, "AllowanceChargeReason"),
                        reason_code: code(c, "AllowanceChargeReasonCode"),
                    };
                    if is_charge {
                        l.charges.push(a);
                    } else {
                        l.allowances.push(a);
                    }
                }
                "Price" => {
                    l.price = self.price(c, &group, sub);
                    price_stated = true;
                }
                "Item" => {
                    let (item, vat) = self.item(c, &group);
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
        (l, quantity_stated, price_stated)
    }

    /// What UBL's schema refuses in a sub-line although no rule requires the
    /// term there: a quantity element without a value, a `cac:Price` without
    /// `cbc:PriceAmount` (KoSIT answers 406).
    fn refused_on_sub_line(&mut self, at: Node<'_, '_>, element: &str, term: Term<'_>, why: &str) {
        self.entries.push(
            Entry::new(
                Kind::Malformed,
                format!("{}/{element}", crate::xml::path_of(at)),
                why.to_owned(),
            )
            .term(term.to_string()),
        );
    }

    /// `cac:SubInvoiceLine` children, recursively — BG-DEX-01.
    fn sub_lines(&mut self, n: Node<'_, '_>) -> Vec<en16931::SubInvoiceLine> {
        kids(n)
            .filter(|c| name(*c) == "SubInvoiceLine")
            .enumerate()
            .map(|(i, c)| {
                let (line, quantity_stated, price_stated) = self.line(c, i + 1);
                // BG-DEX-06: `BR-DEX-03` requires exactly one, so two map to
                // `None` and the rule fires.
                let categories = kid(c, "Item").map_or(0, |i| {
                    kids(i)
                        .filter(|x| name(*x) == "ClassifiedTaxCategory")
                        .count()
                });
                en16931::SubInvoiceLine {
                    vat: (categories == 1).then(|| line.vat.clone()),
                    line,
                    quantity_stated,
                    price_stated,
                    children: self.sub_lines(c),
                }
            })
            .collect()
    }

    /// The price of a line, or of a sub-line when `sub` (see [`Self::line`]).
    fn price(&mut self, n: Node<'_, '_>, group: &str, sub: bool) -> PriceDetails {
        if text(n, "PriceAmount").is_none() {
            if sub {
                let why = if kid(n, "PriceAmount").is_some() {
                    "present without a value; the UBL schema requires a decimal"
                } else {
                    "missing; the UBL schema requires it in cac:Price"
                };
                self.refused_on_sub_line(n, "PriceAmount", Term::new(group, "BT-146"), why);
            } else {
                self.missing(n, Term::new(group, "BT-146"), "the item net price");
            }
        }
        let mut p = PriceDetails {
            net_price: self
                .unit_price(n, "PriceAmount", Term::new(group, "BT-146"))
                .unwrap_or_default(),
            price_discount: None,
            gross_price: None,
            base_quantity: self
                .decimal(n, "BaseQuantity", Term::new(group, "BT-149"))
                .map(Quantity::new),
            // An empty `unitCode` is an absent BT-150 (`PEPPOL-EN16931-R130`).
            base_quantity_code: kid(n, "BaseQuantity")
                .and_then(|q| q.attribute("unitCode"))
                .filter(|u| !u.trim().is_empty())
                .map(Code::new),
        };
        for c in kids(n) {
            match name(c) {
                "AllowanceCharge" => {
                    // A price-level allowance: `Amount` is BT-147, `BaseAmount`
                    // BT-148. A price-level charge has no term in the model
                    // (`PEPPOL-EN16931-R044`) and is reported, not mapped.
                    if self.charge_indicator(c, "Price/AllowanceCharge") {
                        self.entries.push(
                            Entry::new(
                                Kind::Malformed,
                                "Price/AllowanceCharge/ChargeIndicator",
                                "a price-level charge; BT-147 is a discount and EN 16931 has \
                                 no term for a price-level charge (R044)",
                            )
                            .term("BT-147")
                            .value("true"),
                        );
                        continue;
                    }
                    p.price_discount = self.unit_price(c, "Amount", Term::new(group, "BT-147"));
                    p.gross_price = self.unit_price(c, "BaseAmount", Term::new(group, "BT-148"));
                }
                "PriceAmount" | "BaseQuantity" => {}
                _ => self.skip("Price", c),
            }
        }
        p
    }

    fn item(&mut self, n: Node<'_, '_>, group: &str) -> (Item, LineVat) {
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
                    let (cat, pct, _, _) = self.tax_category(
                        c,
                        Term::new(group, "BT-151"),
                        Term::new(group, "BT-152"),
                    );
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

    fn additional_document(&mut self, n: Node<'_, '_>, inv: &mut Invoice) {
        // Document type code `130` is BT-18, the invoiced object identifier.
        // Everything else is BG-24, a supporting document.
        if text(n, "DocumentTypeCode").as_deref() == Some("130") {
            if let Some(id) = kid(n, "ID") {
                inv.object_identifier = Some(identifier(id));
            }
            return;
        }
        let group = format!("BG-24[{}]", inv.attachments.len() + 1);
        // CEN's binding reads any other code as a supporting document and maps
        // no code there; it is said rather than dropped.
        if let Some(code) = text(n, "DocumentTypeCode") {
            let message = if code == "50" && name(n.parent_element().unwrap_or(n)) == "CreditNote" {
                // UBL-SR-43 tolerates it; CEN's and Peppol's bindings both put
                // BT-17 in `cac:OriginatorDocumentReference`, credit notes too.
                "type code 50 is an older credit-note spelling of the tender reference; \
                 BT-17 is `cac:OriginatorDocumentReference` in both bindings, so this is read \
                 as a supporting document (BG-24) and the code not mapped"
            } else {
                "a document type code on a supporting document (BG-24), which has no term \
                 for it; read as a supporting document, the code not mapped"
            };
            self.entries.push(
                Entry::new(
                    Kind::Unmapped,
                    "AdditionalDocumentReference/DocumentTypeCode",
                    message,
                )
                .term(group.clone())
                .value(code),
            );
        }
        if text(n, "ID").is_none() {
            self.missing(
                n,
                Term::new(&group, "BT-122"),
                "the supporting document reference",
            );
        }
        // An attachment without mime code or filename violates §6.5.11 and is
        // reported.
        let attachment = kid(n, "Attachment")
            .and_then(|a| kid(a, "EmbeddedDocumentBinaryObject"))
            .and_then(|b| {
                let bytes = match crate::xml::decode_base64(&own_text(b)) {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        self.entries.push(
                            Entry::new(Kind::Malformed, "AdditionalDocumentReference/Attachment/EmbeddedDocumentBinaryObject", e).term("BT-125"),
                        );
                        return None;
                    }
                };
                match en16931::Attachment::new(
                    bytes,
                    b.attribute("mimeCode").unwrap_or_default(),
                    b.attribute("filename").unwrap_or_default(),
                ) {
                    Ok(a) => Some(a),
                    Err(e) => {
                        self.entries.push(
                            Entry::new(
                                Kind::Malformed,
                                "AdditionalDocumentReference/Attachment/EmbeddedDocumentBinaryObject",
                                e.to_string(),
                            )
                            .term("BT-125"),
                        );
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

    /// `xs:date` permits a time zone.
    #[test]
    fn a_ubl_date_may_carry_the_time_zone_xs_date_allows() {
        let expected = en16931::Date::parse("2026-07-31").ok();
        for spelling in [
            "2026-07-31",
            "2026-07-31Z",
            "2026-07-31+02:00",
            "2026-07-31-05:00",
            // Dropped, never applied.
            "2026-07-31+14:00",
            "  2026-07-31Z  ",
        ] {
            assert_eq!(xs_date(spelling), expected, "{spelling}");
        }
    }

    /// `cbc:ChargeIndicator` is `xs:boolean`, which has four lexical forms.
    #[test]
    fn a_charge_indicator_may_be_written_as_a_digit() {
        assert_eq!(super::xs_boolean("true"), Some(true));
        assert_eq!(super::xs_boolean("1"), Some(true));
        assert_eq!(super::xs_boolean("false"), Some(false));
        assert_eq!(super::xs_boolean("0"), Some(false));
        // `xs:boolean` is whitespace-collapsed.
        assert_eq!(super::xs_boolean("  true "), Some(true));
        // …and nothing else is a boolean. `None` is reported, never defaulted.
        for other in ["yes", "TRUE", "True", "2", "", "-1"] {
            assert_eq!(super::xs_boolean(other), None, "{other}");
        }
    }

    /// A malformed date still fails after zone-stripping.
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
