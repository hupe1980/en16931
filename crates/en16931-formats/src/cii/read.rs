//! Reading UN/CEFACT CII D16B into the semantic model.
//!
//! Every element walked is mapped or recorded as
//! [`Unmapped`](crate::report::Kind::Unmapped) (`Parent/Child`). A value the
//! model cannot hold is recorded as [`Malformed`](crate::report::Kind::Malformed)
//! and the field left absent.
//!
//! `ram:ID` and `ram:GlobalID` are both read into `Party::identifiers`. BT-7
//! and BT-8, repeated on each tax breakdown entry, collapse to the document
//! level; BT-8 is translated from UNTDID 2475 to 2005 (see [`super`]).

use std::collections::BTreeSet;

use en16931::invoice::*;
use en16931::{
    Date, DocumentKind, DocumentReference, Identifier, InvoiceAmount, Percentage, Quantity,
};

use crate::report::{Entry, Kind};
use crate::xml::Term;
use crate::xml::{Node, kid, kids, name};

/// Reads one `rsm:CrossIndustryInvoice` into an [`Invoice`].
#[derive(Default)]
pub(crate) struct Reader {
    /// Element paths encountered and not mapped, as `Parent/Child`, each once.
    unmapped: BTreeSet<String>,
    /// Everything else the reader has to say, in document order.
    entries: Vec<Entry>,
    /// BT-5, pre-scanned: lines come before the header settlement that
    /// carries it, and every amount's `@currencyID` is checked against it.
    currency: Option<String>,
    /// BT-24, pre-scanned: a term is missing only if the declared profile
    /// runs the rule requiring it (`crate::required`).
    bt24: Option<String>,
}

/// Trimmed text of this element, empty when there is none.
fn own_text(n: Node<'_, '_>) -> String {
    crate::xml::text(n).unwrap_or_default()
}

/// Trimmed text of the first child named `want`, or `None` when the element is
/// absent or empty ([`crate::xml::child_text`], shared with the UBL reader).
fn text(n: Node<'_, '_>, want: &str) -> Option<String> {
    crate::xml::child_text(n, want)
}

/// `None` for an empty string.
fn non_empty(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

fn code(n: Node<'_, '_>, want: &str) -> Option<Code> {
    text(n, want).map(Code::new)
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
    /// Note an element this reader does not map.
    fn push_path(&mut self, parent: &str, child: &str) {
        self.unmapped.insert(format!("{parent}/{child}"));
    }

    /// Note a combination the model cannot hold, at `path`.
    fn not_held(&mut self, path: &str, message: &str) {
        self.entries
            .push(Entry::new(Kind::Unmapped, path, message.to_owned()));
    }

    /// Note a value the model cannot hold.
    fn malformed(&mut self, path: String, message: impl Into<String>, value: String) {
        self.entries
            .push(Entry::new(Kind::Malformed, path, message).value(value));
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

    /// An amount in `want`, recording one the model cannot hold: not an
    /// amount, more than two decimals, or in another currency than BT-5.
    fn amount(&mut self, n: Node<'_, '_>, want: &str, term: Term<'_>) -> Option<InvoiceAmount> {
        let raw = text(n, want)?;
        let path = format!("{}/{want}", name(n));
        if let Some(e) = kid(n, want)
            && !self.in_document_currency(e, &path, term)
        {
            return None;
        }
        match InvoiceAmount::parse(&raw) {
            Ok(a) => Some(a),
            Err(e) => {
                self.entries.push(
                    Entry::new(Kind::Malformed, path, e.to_string())
                        .term(term.to_string())
                        .value(raw),
                );
                None
            }
        }
    }

    /// Whether the amount element `e` is in BT-5. A blank or absent
    /// `@currencyID` means BT-5; another currency is recorded and the amount
    /// left out — reading it as BT-5 would change its value (`BR-CL-03`'s
    /// premise that one document has one currency).
    fn in_document_currency(&mut self, e: Node<'_, '_>, path: &str, term: Term<'_>) -> bool {
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
                    path.to_owned(),
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
                self.entries.push(
                    Entry::new(
                        Kind::Malformed,
                        path.to_owned(),
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

    /// A unit price in `want`: a number, in BT-5.
    fn unit_price(
        &mut self,
        n: Node<'_, '_>,
        want: &str,
        term: Term<'_>,
    ) -> Option<en16931::UnitPriceAmount> {
        if let Some(e) = kid(n, want)
            && !self.in_document_currency(e, &format!("{}/{want}", name(n)), term)
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
        let raw = text(n, want)?;
        if let Ok(d) = raw.parse::<rust_decimal::Decimal>() {
            Some(d)
        } else {
            self.entries.push(
                Entry::new(
                    Kind::Malformed,
                    format!("{}/{want}", name(n)),
                    "not a decimal number the model can hold",
                )
                .term(term.to_string())
                .value(raw),
            );
            None
        }
    }

    /// A percentage in `want` (`19` is 19 %), recording text that is not one.
    fn percent(&mut self, n: Node<'_, '_>, want: &str, term: Term<'_>) -> Option<Percentage> {
        self.decimal(n, want, term)
            .map(|d| d / rust_decimal::Decimal::from(100))
            .and_then(Percentage::from_fraction)
    }

    /// Record that `term`, which the model requires, is not in the group `at`.
    fn missing(&mut self, at: Node<'_, '_>, term: Term<'_>, what: &str) {
        self.missing_at(crate::xml::path_of(at), term, what);
    }

    /// A `ram:*TradeTax` whose type code is stated and is not VAT: EN 16931
    /// carries VAT only (CEN binds `BR-47` to `upper-case(ram:TypeCode) =
    /// 'VAT'`), so it is said; the category is still read, as the code-list
    /// rules read it.
    fn say_non_vat(&mut self, n: Node<'_, '_>, category: Term<'_>) {
        if let Some(t) = text(n, "TypeCode")
            && !t.eq_ignore_ascii_case("VAT")
        {
            self.entries.push(
                Entry::new(
                    Kind::Unmapped,
                    format!("{}/TypeCode", name(n)),
                    "a tax whose type code is not VAT; EN 16931 carries VAT only, and the category \
                     is read as a VAT category",
                )
                .term(category.to_string())
                .value(t),
            );
        }
    }

    /// BG-22's four required amounts, when the whole group is absent at `path`.
    fn missing_totals(&mut self, path: &str) {
        for (term, what) in [
            (
                Term::new("BG-22", "BT-106"),
                "the sum of invoice line net amounts",
            ),
            (
                Term::new("BG-22", "BT-109"),
                "the invoice total amount without VAT",
            ),
            (
                Term::new("BG-22", "BT-112"),
                "the invoice total amount with VAT",
            ),
            (Term::new("BG-22", "BT-115"), "the amount due for payment"),
        ] {
            self.missing_at(path.to_owned(), term, what);
        }
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

    /// A required code: absent or empty is [`Kind::Missing`].
    fn required_code(&mut self, n: Node<'_, '_>, want: &str, term: Term<'_>, what: &str) -> Code {
        code(n, want).unwrap_or_else(|| {
            self.missing(n, term, what);
            Code::new("")
        })
    }

    /// A VAT category code and rate from a `ram:*TradeTax`, under the terms
    /// the caller's group gives them (BT-151/152 on a line, BT-95/96 on an
    /// allowance, BT-102/103 on a charge).
    fn trade_tax(&mut self, n: Node<'_, '_>, category: Term<'_>, rate: Term<'_>) -> LineVat {
        self.say_non_vat(n, category);
        LineVat {
            category: self.required_code(n, "CategoryCode", category, "the VAT category code"),
            rate: self.percent(n, "RateApplicablePercent", rate),
        }
    }

    /// [`Self::trade_tax`] from the child `want` of `n`, which the model
    /// requires: absent, the category is [`Kind::Missing`].
    fn required_trade_tax(
        &mut self,
        n: Node<'_, '_>,
        want: &str,
        category: Term<'_>,
        rate: Term<'_>,
    ) -> LineVat {
        if let Some(t) = kid(n, want) {
            self.trade_tax(t, category, rate)
        } else {
            self.missing(n, category, "the VAT category code");
            LineVat::default()
        }
    }

    /// `<ram:Foo><udt:DateTimeString format="102">20260115</udt:DateTimeString></ram:Foo>`
    ///
    /// `ram:TaxPointDate` (BT-7) carries a `udt:DateString` instead. Either
    /// child is read on any wrapper; a format other than `102` is reported, not
    /// guessed at.
    fn date(&mut self, n: Node<'_, '_>, want: &str) -> Option<Date> {
        let wrapper = kid(n, want)?;
        let s = kid(wrapper, "DateTimeString").or_else(|| kid(wrapper, "DateString"))?;
        let path = format!("{}/{want}/{}", name(n), name(s));
        let raw = own_text(s);
        // D16B types BT-7 `udt:DateType` (`udt:DateString`) and every other
        // date `udt:DateTimeString`. The other spelling is read and reported.
        let expected = if want == "TaxPointDate" {
            "DateString"
        } else {
            "DateTimeString"
        };
        if name(s) != expected {
            self.entries.push(
                Entry::new(
                    Kind::Schema,
                    path.clone(),
                    format!(
                        "the D16B schema has a {expected} here; this spelling is read and \
                         reported"
                    ),
                )
                .value(raw.clone()),
            );
        }
        match s.attribute("format") {
            Some(super::write::DATE_FORMAT) | None => {}
            Some(other) => {
                self.malformed(
                    path,
                    format!("format {other:?}; only 102 (CCYYMMDD) is permitted"),
                    raw,
                );
                return None;
            }
        }
        if raw.len() != 8 || !raw.bytes().all(|b| b.is_ascii_digit()) {
            self.malformed(path, "not CCYYMMDD", raw);
            return None;
        }
        let f = format!("{}-{}-{}", &raw[0..4], &raw[4..6], &raw[6..8]);
        match Date::parse(&f) {
            Ok(d) => Some(d),
            Err(e) => {
                self.malformed(path, e.to_string(), raw);
                None
            }
        }
    }

    fn note(&mut self, parent: &str, n: Node<'_, '_>) {
        for c in kids(n) {
            if !matches!(name(c), "Content" | "SubjectCode") {
                self.push_path(parent, name(c));
            }
        }
    }

    /// Read the document element.
    pub(crate) fn read(&mut self, root: Node<'_, '_>) -> Invoice {
        let mut inv = Invoice::default();
        self.bt24 = kid(root, "ExchangedDocumentContext")
            .and_then(|c| kid(c, "GuidelineSpecifiedDocumentContextParameter"))
            .and_then(|g| text(g, "ID"));
        self.currency = kid(root, "SupplyChainTradeTransaction")
            .and_then(|t| kid(t, "ApplicableHeaderTradeSettlement"))
            .and_then(|s| text(s, "InvoiceCurrencyCode"));
        for c in kids(root) {
            match name(c) {
                "ExchangedDocumentContext" => self.context(&mut inv, c),
                "ExchangedDocument" => self.document(&mut inv, c),
                "SupplyChainTradeTransaction" => self.transaction(&mut inv, c),
                other => self.push_path("CrossIndustryInvoice", other),
            }
        }
        if kid(root, "SupplyChainTradeTransaction").is_none() {
            self.missing_totals(&format!(
                "{}/SupplyChainTradeTransaction/ApplicableHeaderTradeSettlement/\
                 SpecifiedTradeSettlementHeaderMonetarySummation",
                crate::xml::path_of(root)
            ));
        }
        // The kind comes from BT-3 against the whole UNTDID 1001 credit-note
        // list (`381`, `396`, `532`, `83`, …). `81` is on both lists and is
        // read as a credit note, as `PEPPOL-EN16931-P0101` expects.
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
                other => self.push_path("ExchangedDocumentContext", other),
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
                other => self.push_path("ExchangedDocument", other),
            }
        }
    }

    fn transaction(&mut self, inv: &mut Invoice, n: Node<'_, '_>) {
        for c in kids(n) {
            match name(c) {
                "IncludedSupplyChainTradeLineItem" => {
                    let line = self.line(c, inv.lines.len() + 1);
                    inv.lines.push(line);
                }
                "ApplicableHeaderTradeAgreement" => self.agreement(inv, c),
                "ApplicableHeaderTradeDelivery" => self.delivery(inv, c),
                "ApplicableHeaderTradeSettlement" => self.settlement(inv, c),
                other => self.push_path("SupplyChainTradeTransaction", other),
            }
        }
        // No settlement at all: BG-22's required amounts are not there either.
        if kid(n, "ApplicableHeaderTradeSettlement").is_none() {
            self.missing_totals(&format!(
                "{}/ApplicableHeaderTradeSettlement/SpecifiedTradeSettlementHeaderMonetarySummation",
                crate::xml::path_of(n)
            ));
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
                other => self.push_path("ApplicableHeaderTradeAgreement", other),
            }
        }
    }

    /// `ram:AdditionalReferencedDocument` carries three different terms, told
    /// apart by `ram:TypeCode`: 50 is BT-17, 130 is BT-18, 916 is BG-24.
    fn additional_document(&mut self, inv: &mut Invoice, n: Node<'_, '_>) {
        let id = text(n, "IssuerAssignedID").unwrap_or_default();
        match text(n, "TypeCode").as_deref() {
            Some("50" | "130" | "916") => {}
            // The code is what tells BT-17, BT-18 and BG-24 apart; without a
            // known one, reading it as a supporting document is a guess.
            other => {
                let e = Entry::new(
                    Kind::Malformed,
                    "ApplicableHeaderTradeAgreement/AdditionalReferencedDocument/TypeCode",
                    match other {
                        Some(_) => {
                            "a type code the CII binding does not define; which of BT-17 (50), \
                                    BT-18 (130) or BG-24 (916) this is cannot be told; not read"
                        }
                        None => {
                            "no type code; which of BT-17 (50), BT-18 (130) or BG-24 (916) \
                                 this is cannot be told; not read"
                        }
                    },
                )
                .term(format!("BG-24[{}]", inv.attachments.len() + 1));
                self.entries.push(match other {
                    Some(v) => e.value(v.to_owned()),
                    None => e,
                });
                return;
            }
        }
        if id.is_empty() {
            let term = match text(n, "TypeCode").as_deref() {
                Some("50") => "BT-17".to_owned(),
                Some("130") => "BT-18".to_owned(),
                _ => format!("BG-24[{}]/BT-122", inv.attachments.len() + 1),
            };
            self.missing(
                n,
                Term::new("", &term),
                "the referenced document's identifier",
            );
        }
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
                    let bytes = match crate::xml::decode_base64(&own_text(b)) {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            self.entries.push(
                                Entry::new(
                                    Kind::Malformed,
                                    "AdditionalReferencedDocument/AttachmentBinaryObject",
                                    e,
                                )
                                .term("BT-125"),
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
                                    "AdditionalReferencedDocument/AttachmentBinaryObject",
                                    e.to_string(),
                                )
                                .term("BT-125"),
                            );
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
                // BT-29 from both `ram:GlobalID` and `ram:ID`.
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
                other => self.push_path("TradeParty", other),
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
                other => self.push_path("PostalTradeAddress", other),
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
                            other => self.push_path("ShipToTradeParty", other),
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
                other => self.push_path("ApplicableHeaderTradeDelivery", other),
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
        // BT-8 is read from its first occurrence: the rest are required to
        // agree with it (CII-SR-462), and one that does not is reported.
        let mut bt8_first: Option<String> = None;
        let mut creditor_id = None;
        let mut mandate = None;
        let mut have_totals = false;
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
                    // BT-7/BT-8 repeat per entry; the first wins.
                    if inv.vat_point_date.is_none() {
                        inv.vat_point_date = self.date(c, "TaxPointDate");
                    }
                    // A repetition that differs (CII-SR-462) is reported.
                    if let Some(raw) = text(c, "DueDateTypeCode") {
                        match &bt8_first {
                            None => {
                                inv.vat_point_date_code = self.vat_point_date_code(&raw);
                                bt8_first = Some(raw);
                            }
                            Some(first) if *first != raw => self.entries.push(
                                Entry::new(
                                    Kind::Duplicate,
                                    "ApplicableHeaderTradeSettlement/ApplicableTradeTax/DueDateTypeCode",
                                    format!(
                                        "a second, different VAT point date code; the first ({first}) is read"
                                    ),
                                )
                                .term("BT-8")
                                .value(raw)
                                .rule("CII-SR-462"),
                            ),
                            Some(_) => {}
                        }
                    }
                    let g = format!("BG-23[{}]", inv.vat_breakdown.len() + 1);
                    inv.vat_breakdown.push(VatBreakdown {
                        taxable_amount: self.required_amount(
                            c,
                            "BasisAmount",
                            Term::new(&g, "BT-116"),
                            "the VAT category taxable amount",
                        ),
                        tax_amount: self.required_amount(
                            c,
                            "CalculatedAmount",
                            Term::new(&g, "BT-117"),
                            "the VAT category tax amount",
                        ),
                        category: {
                            self.say_non_vat(c, Term::new(&g, "BT-118"));
                            self.required_code(
                                c,
                                "CategoryCode",
                                Term::new(&g, "BT-118"),
                                "the VAT category code",
                            )
                        },
                        rate: self.percent(c, "RateApplicablePercent", Term::new(&g, "BT-119")),
                        exemption_reason: text(c, "ExemptionReason"),
                        exemption_reason_code: code(c, "ExemptionReasonCode"),
                    });
                }
                "BillingSpecifiedPeriod" => inv.invoicing_period = Some(self.period(c)),
                "SpecifiedTradeAllowanceCharge" => {
                    let is_charge = self.is_charge(c, "SpecifiedTradeAllowanceCharge");
                    let (g, [amount, base, pct, cat]) = if is_charge {
                        (
                            format!("BG-21[{}]", inv.charges.len() + 1),
                            ["BT-99", "BT-100", "BT-101", "BT-102"],
                        )
                    } else {
                        (
                            format!("BG-20[{}]", inv.allowances.len() + 1),
                            ["BT-92", "BT-93", "BT-94", "BT-95"],
                        )
                    };
                    let rate = if is_charge { "BT-103" } else { "BT-96" };
                    let vat = self.required_trade_tax(
                        c,
                        "CategoryTradeTax",
                        Term::new(&g, cat),
                        Term::new(&g, rate),
                    );
                    let a = DocumentAllowanceCharge {
                        amount: self.required_amount(
                            c,
                            "ActualAmount",
                            Term::new(&g, amount),
                            if is_charge {
                                "the charge amount"
                            } else {
                                "the allowance amount"
                            },
                        ),
                        base_amount: self.amount(c, "BasisAmount", Term::new(&g, base)),
                        percentage: self.percent(c, "CalculationPercent", Term::new(&g, pct)),
                        vat,
                        reason: text(c, "Reason"),
                        reason_code: code(c, "ReasonCode"),
                    };
                    if is_charge {
                        inv.charges.push(a);
                    } else {
                        inv.allowances.push(a);
                    }
                }
                // ZUGFeRD / Factur-X EXTENDED's freight and packing charge, a
                // document-level charge (FeRD's BR-FXEXT-CO-12 counts it in
                // BT-108). Mapped only under an extension (`#conformant#` in
                // BT-24, EN 16931 §4.3).
                "SpecifiedLogisticsServiceCharge"
                    if inv.specification_id.as_deref().is_some_and(|s| {
                        s.contains("#conformant#") || s.contains(":conformant:")
                    }) =>
                {
                    let g = format!("BG-21[{}]", inv.charges.len() + 1);
                    let vat = self.required_trade_tax(
                        c,
                        "AppliedTradeTax",
                        Term::new(&g, "BT-102"),
                        Term::new(&g, "BT-103"),
                    );
                    inv.charges.push(DocumentAllowanceCharge {
                        amount: self.required_amount(
                            c,
                            "AppliedAmount",
                            Term::new(&g, "BT-99"),
                            "the charge amount",
                        ),
                        base_amount: None,
                        percentage: None,
                        vat,
                        reason: text(c, "Description"),
                        reason_code: None,
                    });
                }
                // Under the core: CII-SR-396 says it should not be present and
                // BR-CO-12 does not count it. Not mapped.
                "SpecifiedLogisticsServiceCharge" => self.entries.push(
                    Entry::new(
                        Kind::Unmapped,
                        "ApplicableHeaderTradeSettlement/SpecifiedLogisticsServiceCharge",
                        "a logistics service charge, which the core does not count",
                    )
                    .rule("CII-SR-396")
                    .hint(
                        "declare the extension in BT-24 (`#conformant#`), or send the charge \
                         as a document-level charge (BG-21)",
                    ),
                ),
                "SpecifiedTradePaymentTerms" => {
                    // Not trimmed: `BR-DE-18` requires the Skonto block to
                    // end with a newline.
                    inv.payment_terms = kid(c, "Description").and_then(crate::xml::raw_text);
                    inv.due_date = self.date(c, "DueDateDateTime");
                    mandate = text(c, "DirectDebitMandateID");
                }
                "SpecifiedTradeSettlementHeaderMonetarySummation" => {
                    have_totals = true;
                    self.totals(inv, c);
                }
                "InvoiceReferencedDocument" => {
                    if text(c, "IssuerAssignedID").is_none() {
                        let term = format!("BG-3[{}]/BT-25", inv.preceding_invoices.len() + 1);
                        self.missing(c, Term::new("", &term), "the preceding invoice reference");
                    }
                    inv.preceding_invoices.push(PrecedingInvoice {
                        reference: text(c, "IssuerAssignedID")
                            .map_or_else(|| DocumentReference::new(""), DocumentReference::new),
                        issue_date: self.date(c, "FormattedIssueDateTime"),
                    });
                }
                "ReceivableSpecifiedTradeAccountingAccount" => {
                    inv.accounting_reference = text(c, "ID");
                }
                other => self.push_path("ApplicableHeaderTradeSettlement", other),
            }
        }
        // BG-22 is required, and its four required amounts with it.
        if !have_totals {
            self.missing_totals(&format!(
                "{}/SpecifiedTradeSettlementHeaderMonetarySummation",
                crate::xml::path_of(n)
            ));
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
        // Called once per `ram:SpecifiedTradeSettlementPaymentMeans`; several
        // are one BG-16 with several BG-17 accounts (0..1 per element), so
        // everything accumulates into `p`.
        let mut transfers: Vec<CreditTransfer> = Vec::new();
        for c in kids(n) {
            match name(c) {
                "TypeCode" => {
                    if p.means_code.is_none() {
                        p.means_code = Some(Code::new(own_text(c)));
                    }
                }
                "Information" => {
                    if p.means_text.is_none() {
                        p.means_text = non_empty(own_text(c));
                    }
                }
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
                other => self.push_path("SpecifiedTradeSettlementPaymentMeans", other),
            }
        }
        if !transfers.is_empty() {
            match &mut p.means {
                Some(PaymentMeans::CreditTransfer(existing)) => existing.extend(transfers),
                slot @ None => *slot = Some(PaymentMeans::CreditTransfer(transfers)),
                // An account on top of a card or mandate cannot be
                // represented; reported.
                Some(_) => self.not_held(
                    "SpecifiedTradeSettlementPaymentMeans/PayeePartyCreditorFinancialAccount",
                    "an account beside a card or mandate; BG-16 holds one kind",
                ),
            }
        }
    }

    fn totals(&mut self, inv: &mut Invoice, n: Node<'_, '_>) {
        let mut t = DocumentTotals::default();
        // Two `ram:TaxTotalAmount` elements may appear, told apart by
        // `@currencyID`: BT-110 and BT-111 (accounting currency).
        let doc_ccy = inv.currency.as_ref().map(Code::as_str);
        let tax_ccy = inv.vat_accounting_currency.as_ref().map(Code::as_str);
        for c in kids(n) {
            match name(c) {
                // Required: read once, after the loop, so an absent one is said.
                "LineTotalAmount"
                | "TaxBasisTotalAmount"
                | "GrandTotalAmount"
                | "DuePayableAmount" => {}
                "ChargeTotalAmount" => {
                    t.charge_total =
                        self.amount(n, "ChargeTotalAmount", Term::new("BG-22", "BT-108"));
                }
                "AllowanceTotalAmount" => {
                    t.allowance_total =
                        self.amount(n, "AllowanceTotalAmount", Term::new("BG-22", "BT-107"));
                }
                "TaxTotalAmount" => {
                    let raw = own_text(c);
                    let parsed = InvoiceAmount::parse(&raw).ok();
                    if parsed.is_none() {
                        self.malformed(
                            "SpecifiedTradeSettlementHeaderMonetarySummation/TaxTotalAmount"
                                .to_owned(),
                            "not an amount the model can hold (at most two decimals)",
                            raw,
                        );
                    }
                    // Trimmed; a blank `@currencyID` means the document currency.
                    match c
                        .attribute("currencyID")
                        .map(str::trim)
                        .filter(|c| !c.is_empty())
                    {
                        // BT-6 == BT-5: one element is both totals (`BR-53`).
                        Some(cur) if Some(cur) == tax_ccy => {
                            if Some(cur) == doc_ccy && t.vat_total.is_none() {
                                t.vat_total = parsed;
                            }
                            t.vat_total_accounting = parsed;
                        }
                        // A third currency, or a second one with no BT-6: the
                        // model has no place for it, and reading it as BT-111
                        // would invent a VAT accounting currency total.
                        Some(cur) if Some(cur) != doc_ccy => self.entries.push(
                            Entry::new(
                                Kind::Malformed,
                                "SpecifiedTradeSettlementHeaderMonetarySummation/TaxTotalAmount",
                                format!(
                                    "a tax total in {cur}, which is neither the invoice currency \
                                     (BT-5{}) nor the VAT accounting currency (BT-6{}); not read",
                                    doc_ccy.map(|c| format!(" = {c}")).unwrap_or_default(),
                                    tax_ccy.map(|c| format!(" = {c}")).unwrap_or_default(),
                                ),
                            )
                            .term("BG-22/BT-110")
                            .value(cur.to_owned()),
                        ),
                        _ => t.vat_total = parsed,
                    }
                }
                "RoundingAmount" => {
                    t.rounding = self.amount(n, "RoundingAmount", Term::new("BG-22", "BT-114"));
                }
                "TotalPrepaidAmount" => {
                    t.paid = self.amount(n, "TotalPrepaidAmount", Term::new("BG-22", "BT-113"));
                }
                other => self.push_path("SpecifiedTradeSettlementHeaderMonetarySummation", other),
            }
        }
        t.line_total = self.required_amount(
            n,
            "LineTotalAmount",
            Term::new("BG-22", "BT-106"),
            "the sum of invoice line net amounts",
        );
        t.taxable_total = self.required_amount(
            n,
            "TaxBasisTotalAmount",
            Term::new("BG-22", "BT-109"),
            "the invoice total amount without VAT",
        );
        t.gross_total = self.required_amount(
            n,
            "GrandTotalAmount",
            Term::new("BG-22", "BT-112"),
            "the invoice total amount with VAT",
        );
        t.due = self.required_amount(
            n,
            "DuePayableAmount",
            Term::new("BG-22", "BT-115"),
            "the amount due for payment",
        );
        inv.totals = t;
    }

    fn period(&mut self, n: Node<'_, '_>) -> Period {
        Period {
            start: self.date(n, "StartDateTime"),
            end: self.date(n, "EndDateTime"),
        }
    }

    /// `ram:ChargeIndicator/udt:Indicator` — `true` is a charge.
    ///
    /// All four `xs:boolean` lexical forms are accepted; anything else is
    /// reported, never defaulted to an allowance.
    fn is_charge(&mut self, n: Node<'_, '_>, at: &str) -> bool {
        let raw = kid(n, "ChargeIndicator")
            .and_then(|i| kid(i, "Indicator"))
            .map(own_text);
        match raw.as_deref() {
            Some("true" | "1") => true,
            Some("false" | "0") => false,
            _ => {
                let e = Entry::new(
                    Kind::Malformed,
                    format!("{at}/ChargeIndicator/Indicator"),
                    match &raw {
                        Some(_) => "not an xs:boolean",
                        None => "absent, and CII requires it",
                    },
                )
                .hint("the value must be `true` or `false`");
                self.entries.push(match raw {
                    Some(v) => e.value(v),
                    None => e,
                });
                false
            }
        }
    }

    /// BT-8 from its CII spelling (UNTDID 2475) to the model's (UNTDID 2005).
    ///
    /// A value outside CII's list (including a model code like `3`) is not
    /// passed on.
    fn vat_point_date_code(&mut self, raw: &str) -> Option<Code> {
        if let Some(model) = super::vat_point_date_code_from_cii(raw) {
            return Some(Code::new(model));
        }
        self.entries.push(
            Entry::new(
                Kind::Malformed,
                "ApplicableTradeTax/DueDateTypeCode",
                "not a CII VAT point date code (5, 29 or 72, UNTDID 2475)",
            )
            .term("BT-8")
            .value(raw)
            .hint(
                "CII spells the model's 3, 35 and 432 as 5, 29 and 72 — the same three \
                 meanings",
            ),
        );
        None
    }

    // ---- lines ----------------------------------------------------------

    #[allow(clippy::too_many_lines)]
    /// One line; `index` is its 1-based position, passed in rather than
    /// counted, which would make a document quadratic.
    fn line(&mut self, n: Node<'_, '_>, index: usize) -> InvoiceLine {
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
        let group = format!("BG-25[{index}]");
        let mut gross_basis = None;
        for c in kids(n) {
            match name(c) {
                "AssociatedDocumentLineDocument" => {
                    l.id = text(c, "LineID").unwrap_or_default();
                    l.note = kid(c, "IncludedNote").and_then(|x| text(x, "Content"));
                }
                "SpecifiedTradeProduct" => l.item = self.item(c),
                "SpecifiedLineTradeAgreement" => {
                    for p in kids(c) {
                        match name(p) {
                            "BuyerOrderReferencedDocument" => {
                                l.order_line_reference =
                                    text(p, "LineID").map(DocumentReference::new);
                            }
                            "GrossPriceProductTradePrice" => {
                                l.price.gross_price =
                                    self.unit_price(p, "ChargeAmount", Term::new(&group, "BT-148"));
                                gross_basis = Some(p);
                                l.price.price_discount = match kid(p, "AppliedTradeAllowanceCharge")
                                {
                                    Some(a) => self.price_discount(a, &group),
                                    None => None,
                                };
                            }
                            "NetPriceProductTradePrice" => {
                                l.price.net_price = self
                                    .unit_price(p, "ChargeAmount", Term::new(&group, "BT-146"))
                                    .unwrap_or_default();
                                l.price.base_quantity = self
                                    .decimal(p, "BasisQuantity", Term::new(&group, "BT-149"))
                                    .map(Quantity::new);
                                // An empty `unitCode` is an absent BT-150
                                // (else `PEPPOL-EN16931-R130` fires).
                                l.price.base_quantity_code = kid(p, "BasisQuantity")
                                    .and_then(|q| q.attribute("unitCode"))
                                    .filter(|u| !u.trim().is_empty())
                                    .map(Code::new);
                            }
                            other => self.push_path("SpecifiedLineTradeAgreement", other),
                        }
                    }
                }
                "SpecifiedLineTradeDelivery" => {
                    if let Some(q) = kid(c, "BilledQuantity") {
                        l.quantity = self
                            .decimal(c, "BilledQuantity", Term::new(&group, "BT-129"))
                            .map_or(Quantity::ZERO, Quantity::new);
                        l.unit_code = q
                            .attribute("unitCode")
                            .map_or_else(|| Code::new(""), |u| Code::new(u.trim()));
                    }
                }
                "SpecifiedLineTradeSettlement" => self.line_settlement(&mut l, c, &group),
                other => self.push_path("IncludedSupplyChainTradeLineItem", other),
            }
        }
        // CII carries BT-149 on both prices; the net price's is read. A gross
        // one that does not parse, or says something else, is reported.
        if let Some(p) = gross_basis
            && let Some(raw) = text(p, "BasisQuantity")
        {
            let parsed = self.decimal(p, "BasisQuantity", Term::new(&group, "BT-149"));
            if parsed.is_some() && parsed.map(Quantity::new) != l.price.base_quantity {
                self.entries.push(
                    Entry::new(
                        Kind::Malformed,
                        "GrossPriceProductTradePrice/BasisQuantity",
                        "the gross price's base quantity differs from the net price's; BT-149 is \
                         one term, read from the net price",
                    )
                    .term(format!("{group}/BT-149"))
                    .value(raw),
                );
            }
        }
        self.require_line_terms(n, &group);
        l
    }

    /// The terms a line requires, checked by their paths once the line is
    /// read, so an absent group (no `ram:SpecifiedLineTradeDelivery` at all)
    /// is said as plainly as an absent element. A present value that did not
    /// parse was recorded where it was read and is not reported twice.
    fn require_line_terms(&mut self, n: Node<'_, '_>, group: &str) {
        let at = |path: &[&str]| path.iter().try_fold(n, |node, want| kid(node, want));
        let has_text = |path: &[&str]| at(path).and_then(crate::xml::text).is_some();
        if !has_text(&["AssociatedDocumentLineDocument", "LineID"]) {
            self.missing(n, Term::new(group, "BT-126"), "the invoice line identifier");
        }
        if !has_text(&["SpecifiedLineTradeDelivery", "BilledQuantity"]) {
            self.missing(n, Term::new(group, "BT-129"), "the invoiced quantity");
        }
        if at(&["SpecifiedLineTradeDelivery", "BilledQuantity"])
            .and_then(|q| q.attribute("unitCode"))
            .is_none_or(|u| u.trim().is_empty())
        {
            self.missing(
                n,
                Term::new(group, "BT-130"),
                "the invoiced quantity unit of measure code",
            );
        }
        if !has_text(&[
            "SpecifiedLineTradeSettlement",
            "SpecifiedTradeSettlementLineMonetarySummation",
            "LineTotalAmount",
        ]) {
            self.missing(n, Term::new(group, "BT-131"), "the invoice line net amount");
        }
        if !has_text(&[
            "SpecifiedLineTradeAgreement",
            "NetPriceProductTradePrice",
            "ChargeAmount",
        ]) {
            self.missing(n, Term::new(group, "BT-146"), "the item net price");
        }
        // A present `ram:ApplicableTradeTax` without a category was said by
        // `trade_tax`; here only its absence.
        if at(&["SpecifiedLineTradeSettlement", "ApplicableTradeTax"]).is_none() {
            self.missing(
                n,
                Term::new(group, "BT-151"),
                "the invoiced item VAT category code",
            );
        }
    }

    /// BT-147 from the gross price's `ram:AppliedTradeAllowanceCharge`. Only
    /// an allowance is permitted there (`CII-SR-119`): a price-level charge
    /// has no term in EN 16931, and reading its amount as a discount would
    /// reverse its sign.
    fn price_discount(&mut self, a: Node<'_, '_>, group: &str) -> Option<en16931::UnitPriceAmount> {
        let term = Term::new(group, "BT-147");
        let raw = kid(a, "ChargeIndicator")
            .and_then(|i| kid(i, "Indicator"))
            .map(own_text);
        match raw.as_deref() {
            // D16B makes the indicator optional and CEN's `CII-SR-119` only
            // warns when it is missing beside an amount: an allowance on the
            // gross price has one meaning — BT-147 is a discount — so it is
            // read, as intarsys' corpus samples need.
            Some("false" | "0") | None => {}
            Some("true" | "1") => {
                self.entries.push(
                    Entry::new(
                        Kind::Malformed,
                        crate::xml::path_of(a) + "/ChargeIndicator/Indicator",
                        "a price-level charge; BT-147 is a discount, and EN 16931 has no term \
                         for a price-level charge; not read",
                    )
                    .term(term.to_string())
                    .value(raw.unwrap_or_default())
                    .rule("CII-SR-119"),
                );
                return None;
            }
            Some(other) => {
                self.entries.push(
                    Entry::new(
                        Kind::Malformed,
                        crate::xml::path_of(a) + "/ChargeIndicator/Indicator",
                        "not an xs:boolean; whether this is a discount cannot be told; not read",
                    )
                    .term(term.to_string())
                    .value(other.to_owned())
                    .hint("the value must be `true` or `false`"),
                );
                return None;
            }
        }
        self.unit_price(a, "ActualAmount", term)
    }

    fn line_settlement(&mut self, l: &mut InvoiceLine, n: Node<'_, '_>, group: &str) {
        for c in kids(n) {
            match name(c) {
                "ApplicableTradeTax" => {
                    l.vat =
                        self.trade_tax(c, Term::new(group, "BT-151"), Term::new(group, "BT-152"));
                }
                "BillingSpecifiedPeriod" => l.period = Some(self.period(c)),
                "SpecifiedTradeAllowanceCharge" => {
                    let is_charge = self.is_charge(
                        c,
                        "IncludedSupplyChainTradeLineItem/SpecifiedTradeAllowanceCharge",
                    );
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
                            "ActualAmount",
                            Term::new(&sub, amount),
                            if is_charge {
                                "the line charge amount"
                            } else {
                                "the line allowance amount"
                            },
                        ),
                        base_amount: self.amount(c, "BasisAmount", Term::new(&sub, base)),
                        percentage: self.percent(c, "CalculationPercent", Term::new(&sub, pct)),
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
                    // Its absence is said by `require_line_terms`.
                    l.net_amount = self
                        .amount(c, "LineTotalAmount", Term::new(group, "BT-131"))
                        .unwrap_or_default();
                }
                // BT-128 is the line's only referenced document, type 130.
                "AdditionalReferencedDocument" if text(c, "TypeCode").as_deref() != Some("130") => {
                    let e = Entry::new(
                        Kind::Malformed,
                        "SpecifiedLineTradeSettlement/AdditionalReferencedDocument/TypeCode",
                        "a line-level referenced document is BT-128, type code 130; this one is not; \
                         not read",
                    )
                    .term(format!("{group}/BT-128"));
                    self.entries.push(match text(c, "TypeCode") {
                        Some(v) => e.value(v),
                        None => e,
                    });
                }
                "AdditionalReferencedDocument" if text(c, "IssuerAssignedID").is_none() => {
                    self.missing(
                        c,
                        Term::new(group, "BT-128"),
                        "the invoice line object identifier",
                    );
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
                other => self.push_path("SpecifiedLineTradeSettlement", other),
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
                other => self.push_path("SpecifiedTradeProduct", other),
            }
        }
        i
    }
}
