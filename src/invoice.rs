//! The EN 16931 core invoice model — Table 2, as structs.
//!
//! # Why coded fields are [`Code`] and not enums
//!
//! Only [`crate::VatCategory`] is an enum, because rules *branch* on what it
//! means: you cannot write BR-S-08 without knowing that `S` is "standard rated".
//! Every other coded term — BT-3, BT-5, BT-130, BT-98 — is only ever checked for
//! *membership*, and nothing in the standard behaves differently for `KWH` than
//! for `MTQ`.
//!
//! Membership checks want a [`Code`], not an enum, because of the principle this
//! crate applies throughout: **types enforce representability, rules enforce
//! validity.** A document carrying `BT-3 = "999"` is invalid, and a parser must
//! still be able to load it in order to say so. An enum would turn a reportable
//! finding into an unreportable parse failure with no BT path attached.
//!
//! # Why totals are `Option` where the standard says 0..1
//!
//! Absent is not zero. BR-CO-13 has four branches depending on which of BT-107
//! and BT-108 are present, and BR-CO-16 has four depending on BT-113 and BT-114.
//! Collapsing absent to zero makes this crate disagree with every validator on
//! invoices that omit the fields — which is most of them.

use crate::bt::{Group, Path};
use crate::{
    Attachment, Date, DocumentReference, Identifier, InvoiceAmount, Percentage, Quantity,
    UnitPriceAmount,
};

// ── Code ──────────────────────────────────────────────────────────────────────

/// A value from a code list — EN 16931-1 §6.5.8 `Code. Type`.
///
/// Holds the string verbatim. §6.5.8: *"Codes shall be entered exactly as shown
/// in the selected code list"*, so no case folding and no trimming happens here.
///
/// ```
/// use en16931::invoice::Code;
/// use en16931::codes::generated::UNIT_CODES;
///
/// let unit = Code::new("KWH");
/// assert!(unit.is_in(UNIT_CODES));
/// assert!(!Code::new("kwh").is_in(UNIT_CODES));
/// ```
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Code(String);

impl Code {
    /// Wrap a code value verbatim.
    pub fn new(code: impl Into<String>) -> Self {
        Self(code.into())
    }

    /// The value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the value appears in `list` — see [`crate::codes::contains`].
    #[must_use]
    pub fn is_in(&self, list: &[&str]) -> bool {
        crate::codes::contains(list, &self.0)
    }

    /// Whether the value is empty or whitespace only.
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl core::fmt::Display for Code {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.pad(&self.0)
    }
}

impl From<&str> for Code {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for Code {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

// ── Supporting groups ─────────────────────────────────────────────────────────

/// BG-14 INVOICING PERIOD, or BG-26 at line level.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Period {
    /// BT-73 / BT-134 — start date.
    pub start: Option<Date>,
    /// BT-74 / BT-135 — end date.
    pub end: Option<Date>,
}

impl Period {
    /// Whether the end is on or after the start — BR-29 / BR-30.
    ///
    /// `None` when either endpoint is absent, which BR-CO-19 / BR-CO-20 handle
    /// separately: at least one must be present, but not necessarily both.
    #[must_use]
    pub fn is_ordered(&self) -> Option<bool> {
        match (self.start, self.end) {
            (Some(s), Some(e)) => Some(e >= s),
            _ => None,
        }
    }
}

/// A postal address — BG-5, BG-8, BG-12 or BG-15.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PostalAddress {
    /// BT-35 / BT-50 — address line 1.
    pub line1: Option<String>,
    /// BT-36 / BT-51 — address line 2.
    pub line2: Option<String>,
    /// BT-162 / BT-163 / BT-164 / BT-165 — address line 3, added by A1:2019.
    pub line3: Option<String>,
    /// BT-37 / BT-52 — city.
    pub city: Option<String>,
    /// BT-38 / BT-53 — post code.
    pub post_code: Option<String>,
    /// BT-39 / BT-54 — country subdivision.
    pub subdivision: Option<String>,
    /// BT-40 / BT-55 / BT-69 / BT-80 — **mandatory** country code (BR-09, BR-11).
    pub country: Option<Code>,
}

/// BG-6 SELLER CONTACT or BG-9 BUYER CONTACT.
///
/// Optional in the core model and **mandatory in XRechnung** — `BR-DE-2`
/// requires the group, and `BR-DE-5` … `BR-DE-7` require all three of its terms.
/// That is `Restriction::Mandatory` doing real work: three CIUS rules, three
/// lines of data.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Contact {
    /// BT-41 / BT-56 — contact point.
    pub name: Option<String>,
    /// BT-42 / BT-57 — telephone number.
    pub phone: Option<String>,
    /// BT-43 / BT-58 — email address.
    pub email: Option<String>,
}

/// BG-4 SELLER or BG-7 BUYER.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Party {
    /// BT-27 / BT-44 — **mandatory** name (BR-06, BR-07).
    pub name: Option<String>,
    /// BT-28 / BT-45 — trading name.
    pub trading_name: Option<String>,
    /// BT-29 / BT-46 — identifiers.
    pub identifiers: Vec<Identifier>,
    /// BT-30 / BT-47 — legal registration identifier.
    pub legal_registration: Option<Identifier>,
    /// BT-31 / BT-48 — VAT identifier. BR-CO-09 constrains its prefix.
    pub vat_identifier: Option<String>,
    /// BT-32 — seller tax registration identifier.
    pub tax_registration: Option<String>,
    /// BT-33 — seller additional legal information, such as share capital.
    pub additional_legal_information: Option<String>,
    /// BT-34 / BT-49 — **mandatory** electronic address with a scheme
    /// (BR-62, BR-63).
    pub electronic_address: Option<Identifier>,
    /// BG-5 / BG-8 — **mandatory** postal address (BR-08, BR-10).
    pub address: PostalAddress,
    /// BG-6 / BG-9 — contact. Optional in the core model, mandatory in
    /// XRechnung (`BR-DE-2`).
    pub contact: Contact,
}

/// Whether the document is an invoice or a credit note.
///
/// # Why this is not derived from BT-3
///
/// It used to be, and CEN's conformance suite says that is not enough:
/// `BR-CO-25` must not fire on a credit note, and the suite's credit-note cases
/// carry **no BT-3 at all**. A model that infers the kind from the type code
/// cannot answer the question those cases ask.
///
/// It is also what the syntaxes actually carry. UBL has two root elements,
/// `ubl-invoice:Invoice` and `ubl-creditnote:CreditNote`, and several CEN rules
/// are bound to one of them rather than to a business term. CII marks it on
/// `ExchangedDocument`. Deriving it back out of BT-3 throws away something both
/// syntaxes state outright.
///
/// `BR-CL-01` then becomes exact rather than permissive: an *invoice* carrying
/// `381` is invalid, which is what the artefact's `self::` disjunction says and
/// what a membership test over the union of both lists cannot express.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DocumentKind {
    /// `ubl-invoice:Invoice`.
    #[default]
    Invoice,
    /// `ubl-creditnote:CreditNote`.
    CreditNote,
}

/// BG-1 INVOICE NOTE — free text with an optional subject code.
///
/// BT-21 exists because a note's *purpose* is not inferable from its wording. A
/// German invoice's reverse-charge sentence and its payment instructions are
/// both free text; only the code says which is which, and downstream systems
/// route on the code.
///
/// UBL and CII carry BT-21 differently — UBL embeds it in the note text as
/// `#AAI#the text`, CII gives it its own element — which is precisely the sort
/// of difference this crate exists to keep out of the model.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InvoiceNote {
    /// BT-21 — the note's subject, from UNCL 4451.
    pub subject_code: Option<Code>,
    /// BT-22 — the note itself.
    pub note: Option<String>,
}

impl InvoiceNote {
    /// A note with no subject code.
    #[must_use]
    pub fn new(note: impl Into<String>) -> Self {
        Self {
            subject_code: None,
            note: Some(note.into()),
        }
    }

    /// Set BT-21.
    #[must_use]
    pub fn with_subject(mut self, code: impl Into<String>) -> Self {
        self.subject_code = Some(Code::new(code));
        self
    }
}

/// BG-10 PAYEE — where payment is directed, when that is not the seller.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Payee {
    /// BT-59 — **mandatory** payee name (BR-17), when BG-10 differs from BG-4.
    pub name: Option<String>,
    /// BT-60 — payee identifier.
    pub identifier: Option<Identifier>,
    /// BT-61 — payee legal registration identifier.
    pub legal_registration: Option<Identifier>,
}

/// BG-11 SELLER TAX REPRESENTATIVE PARTY.
///
/// A seller established outside the member state of supply may appoint one; the
/// representative then declares and pays the VAT. Several category rows accept
/// its BT-63 in place of the seller's own BT-31.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TaxRepresentative {
    /// BT-62 — **mandatory** name (BR-18).
    pub name: Option<String>,
    /// BT-63 — **mandatory** VAT identifier (BR-56).
    pub vat_identifier: Option<String>,
    /// BG-12 — **mandatory** postal address (BR-19), whose BT-69 country code is
    /// itself mandatory (BR-20).
    pub address: PostalAddress,
}

/// BG-3 PRECEDING INVOICE REFERENCE.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrecedingInvoice {
    /// BT-25 — **mandatory** reference (BR-55).
    pub reference: DocumentReference,
    /// BT-26 — issue date.
    pub issue_date: Option<Date>,
}

/// BG-24 ADDITIONAL SUPPORTING DOCUMENTS.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportingDocument {
    /// BT-122 — **mandatory** reference (BR-52).
    pub reference: DocumentReference,
    /// BT-123 — description.
    pub description: Option<String>,
    /// BT-124 — external URI.
    pub uri: Option<String>,
    /// BT-125 — the attached file.
    pub attachment: Option<Attachment>,
}

/// BG-17 CREDIT TRANSFER.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CreditTransfer {
    /// BT-84 — **mandatory** payment account identifier (BR-50).
    pub account_identifier: Option<String>,
    /// BT-85 — payment account name.
    pub account_name: Option<String>,
    /// BT-86 — payment service provider identifier.
    pub provider_identifier: Option<String>,
}

/// BG-18 PAYMENT CARD INFORMATION.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PaymentCard {
    /// BT-87 — card primary account number.
    ///
    /// `BR-51` is the **one warning** in the entire CEN abstract model: an
    /// invoice should never carry a full PAN. PCI DSS permits at most the first
    /// six and last four digits.
    pub primary_account_number: Option<String>,
    /// BT-88 — payment card holder name.
    pub holder_name: Option<String>,
}

/// BG-19 DIRECT DEBIT.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DirectDebit {
    /// BT-89 — mandate reference. Required by `PEPPOL-EN16931-R061`.
    pub mandate_reference: Option<String>,
    /// BT-90 — bank assigned creditor identifier. Required by `BR-DE-30`.
    pub creditor_identifier: Option<String>,
    /// BT-91 — debited account identifier. Required by `BR-DE-31`.
    pub debited_account: Option<String>,
}

/// BG-17, BG-18 or BG-19 — **mutually exclusive** by construction.
///
/// The standard nests three sub-groups under BG-16 and every profile treats them
/// as alternatives: XRechnung's `BR-DE-23-b` says that when BT-81 names a credit
/// transfer, *"dürfen BG-18 und BG-19 nicht übermittelt werden"*, and `-24-b` and
/// `-25-b` say the same in the other two directions.
///
/// Modelled as an enum so those three rules have nothing left to check — the
/// combination they forbid cannot be written down. `BR-DE-23-a`, `-24-a` and
/// `-25-a` remain real rules, because they tie the *variant* to BT-81's value,
/// which the type system cannot see.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaymentMeans {
    /// BG-17 — one or more credit transfers.
    CreditTransfer(Vec<CreditTransfer>),
    /// BG-18 — payment card information.
    Card(PaymentCard),
    /// BG-19 — direct debit.
    DirectDebit(DirectDebit),
}

/// BG-16 PAYMENT INSTRUCTIONS.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PaymentInstructions {
    /// BT-81 — **mandatory** payment means code (BR-49).
    pub means_code: Option<Code>,
    /// BT-82 — payment means text.
    pub means_text: Option<String>,
    /// BT-83 — remittance information.
    pub remittance_information: Option<String>,
    /// BG-17 / BG-18 / BG-19 — at most one, by construction.
    pub means: Option<PaymentMeans>,
}

impl PaymentInstructions {
    /// BT-84, when this is a credit transfer.
    #[must_use]
    pub fn account_identifier(&self) -> Option<&str> {
        match &self.means {
            Some(PaymentMeans::CreditTransfer(ts)) => {
                ts.first().and_then(|t| t.account_identifier.as_deref())
            }
            _ => None,
        }
    }

    /// BT-89, when this is a direct debit.
    #[must_use]
    pub fn mandate_reference(&self) -> Option<&str> {
        match &self.means {
            Some(PaymentMeans::DirectDebit(d)) => d.mandate_reference.as_deref(),
            _ => None,
        }
    }
}

/// BG-13 DELIVERY INFORMATION.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Delivery {
    /// BT-70 — deliver-to party name.
    pub party_name: Option<String>,
    /// BT-71 — deliver-to location identifier.
    pub location: Option<Identifier>,
    /// BT-72 — actual delivery date.
    pub date: Option<Date>,
    /// BG-15 — deliver-to address. BT-80 is mandatory if present (BR-57).
    pub address: Option<PostalAddress>,
}

// ── VAT ───────────────────────────────────────────────────────────────────────

/// The VAT treatment of a line, allowance or charge — BG-30, or BT-95/96,
/// or BT-102/103.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LineVat {
    /// BT-151 / BT-95 / BT-102 — **mandatory** category code
    /// (BR-CO-04, BR-32, BR-37).
    pub category: Code,
    /// BT-152 / BT-96 / BT-103 — the rate.
    ///
    /// `None` for category `O` only: BR-O-05/06/07 say the element *"shall not
    /// contain"* a rate, where every other zero-tax category says it *"shall be
    /// 0"*. This is **not** BT-119 — see [`crate::VatCategory::states_rate`].
    pub rate: Option<Percentage>,
}

impl LineVat {
    /// The category's semantics, if the code is one of the ten UNCL 5305 values.
    ///
    /// `None` means `BR-CL-17` / `BR-CL-18` fails, and the category-specific
    /// rules cannot be evaluated for this element.
    #[must_use]
    pub fn semantics(&self) -> Option<crate::VatCategory> {
        crate::VatCategory::from_code(self.category.as_str())
    }
}

/// BG-23 VAT BREAKDOWN — one entry per `(category, rate)`.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VatBreakdown {
    /// BT-116 — **mandatory** taxable amount (BR-45).
    pub taxable_amount: InvoiceAmount,
    /// BT-117 — **mandatory** tax amount (BR-46).
    pub tax_amount: InvoiceAmount,
    /// BT-118 — **mandatory** category code (BR-47).
    pub category: Code,
    /// BT-119 — the category rate.
    ///
    /// BR-48 makes it optional *"if the Invoice is not subject to VAT"*.
    /// XRechnung's **BR-DE-14 requires it unconditionally**, so a document
    /// targeting that profile must fill it even for category `O`.
    pub rate: Option<Percentage>,
    /// BT-120 — exemption reason text.
    pub exemption_reason: Option<String>,
    /// BT-121 — exemption reason code, from the CEF VATEX list.
    ///
    /// The category rules accept *either* this or BT-120; modelling only the
    /// text forces a caller holding a VATEX code to invent prose.
    pub exemption_reason_code: Option<Code>,
}

impl VatBreakdown {
    /// The category's semantics, if the code is valid.
    #[must_use]
    pub fn semantics(&self) -> Option<crate::VatCategory> {
        crate::VatCategory::from_code(self.category.as_str())
    }

    /// Whether either form of exemption reason is stated.
    #[must_use]
    pub fn has_exemption_reason(&self) -> bool {
        self.exemption_reason.is_some() || self.exemption_reason_code.is_some()
    }

    /// The grouping key BR-S-08 and its siblings compare on.
    ///
    /// The rate is compared by value, so `19` and `19.00` are one group — see
    /// [`Percentage`] for why that needs no normalisation step here.
    #[must_use]
    pub fn group_key(&self) -> (Code, Option<Percentage>) {
        (self.category.clone(), self.rate)
    }
}

// ── Allowances and charges ────────────────────────────────────────────────────

/// BG-20 DOCUMENT LEVEL ALLOWANCE or BG-21 DOCUMENT LEVEL CHARGE.
///
/// One type for both, because every rule about them is the same rule with a
/// different BT number. Which one it is comes from the field it lives in.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentAllowanceCharge {
    /// BT-92 / BT-99 — **mandatory** amount (BR-31, BR-36). Stated **positive**.
    pub amount: InvoiceAmount,
    /// BT-93 / BT-100 — base amount. Paired with the percentage (`R041`/`R042`).
    pub base_amount: Option<InvoiceAmount>,
    /// BT-94 / BT-101 — percentage. Paired with the base amount.
    pub percentage: Option<Percentage>,
    /// BT-95 / BT-102 and BT-96 / BT-103 — **mandatory** VAT (BR-32, BR-37).
    pub vat: LineVat,
    /// BT-97 / BT-104 — reason text.
    pub reason: Option<String>,
    /// BT-98 / BT-105 — reason code.
    pub reason_code: Option<Code>,
}

/// BG-27 INVOICE LINE ALLOWANCE or BG-28 INVOICE LINE CHARGE.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineAllowanceCharge {
    /// BT-136 / BT-141 — **mandatory** amount (BR-41, BR-43).
    pub amount: InvoiceAmount,
    /// BT-137 / BT-142 — base amount.
    pub base_amount: Option<InvoiceAmount>,
    /// BT-138 / BT-143 — percentage.
    pub percentage: Option<Percentage>,
    /// BT-139 / BT-144 — reason text.
    pub reason: Option<String>,
    /// BT-140 / BT-145 — reason code.
    pub reason_code: Option<Code>,
}

// ── Invoice line ──────────────────────────────────────────────────────────────

/// BG-29 PRICE DETAILS.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PriceDetails {
    /// BT-146 — **mandatory** item net price (BR-26). Not negative (BR-27).
    pub net_price: UnitPriceAmount,
    /// BT-147 — item price discount.
    pub price_discount: Option<UnitPriceAmount>,
    /// BT-148 — item gross price. Not negative (BR-28). `R046`: net = gross − discount.
    pub gross_price: Option<UnitPriceAmount>,
    /// BT-149 — price base quantity. `None` means 1; `R121` requires it above zero.
    pub base_quantity: Option<Quantity>,
    /// BT-150 — its unit code. `R130`: must equal BT-130.
    pub base_quantity_code: Option<Code>,
}

/// BG-32 ITEM ATTRIBUTES — a name/value pair describing the item.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ItemAttribute {
    /// BT-160 — **mandatory** attribute name (BR-54), such as "Colour".
    pub name: Option<String>,
    /// BT-161 — **mandatory** attribute value (BR-54), such as "Red".
    pub value: Option<String>,
}

/// BG-31 ITEM INFORMATION.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Item {
    /// BT-153 — **mandatory** item name (BR-25).
    pub name: Option<String>,
    /// BT-154 — item description.
    pub description: Option<String>,
    /// BT-155 — seller's item identifier.
    pub seller_identifier: Option<String>,
    /// BT-156 — buyer's item identifier.
    pub buyer_identifier: Option<String>,
    /// BT-157 — standard identifier. **Must** carry a scheme (BR-64).
    pub standard_identifier: Option<Identifier>,
    /// BT-158 — classification identifiers. **Must** carry a scheme (BR-65).
    pub classification_identifiers: Vec<Identifier>,
    /// BT-159 — item country of origin.
    pub origin_country: Option<Code>,
    /// BG-32 — item attributes.
    pub attributes: Vec<ItemAttribute>,
}

/// BG-25 INVOICE LINE.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvoiceLine {
    /// BT-126 — **mandatory** line identifier (BR-21).
    pub id: String,
    /// BT-127 — line note.
    pub note: Option<String>,
    /// BT-132 — referenced purchase order line reference.
    pub order_line_reference: Option<DocumentReference>,
    /// BT-133 — line buyer accounting reference.
    pub accounting_reference: Option<String>,
    /// BT-128 — line object identifier.
    pub object_identifier: Option<Identifier>,
    /// BT-129 — **mandatory** invoiced quantity (BR-22). **May be negative.**
    pub quantity: Quantity,
    /// BT-130 — **mandatory** unit of measure code (BR-23).
    pub unit_code: Code,
    /// BT-131 — **mandatory** line net amount (BR-24).
    pub net_amount: InvoiceAmount,
    /// BG-26 — line period.
    pub period: Option<Period>,
    /// BG-27 — line allowances. Already folded into BT-131.
    pub allowances: Vec<LineAllowanceCharge>,
    /// BG-28 — line charges. Already folded into BT-131.
    pub charges: Vec<LineAllowanceCharge>,
    /// BG-29 — **mandatory** price details.
    pub price: PriceDetails,
    /// BG-30 — **mandatory** line VAT information (BR-CO-04).
    pub vat: LineVat,
    /// BG-31 — item information.
    pub item: Item,
}

// ── Totals ────────────────────────────────────────────────────────────────────

/// BG-22 DOCUMENT TOTALS.
///
/// Five of the ten terms are `Option`, and **absent is not zero** — see the
/// module documentation.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocumentTotals {
    /// BT-106 — **mandatory** sum of line net amounts (BR-12).
    pub line_total: InvoiceAmount,
    /// BT-107 — sum of document level allowances.
    pub allowance_total: Option<InvoiceAmount>,
    /// BT-108 — sum of document level charges.
    pub charge_total: Option<InvoiceAmount>,
    /// BT-109 — **mandatory** total without VAT (BR-13).
    pub taxable_total: InvoiceAmount,
    /// BT-110 — total VAT amount.
    pub vat_total: Option<InvoiceAmount>,
    /// BT-111 — total VAT in the accounting currency. Required with BT-6 (BR-53).
    pub vat_total_accounting: Option<InvoiceAmount>,
    /// BT-112 — **mandatory** total with VAT (BR-14).
    pub gross_total: InvoiceAmount,
    /// BT-113 — paid amount.
    pub paid: Option<InvoiceAmount>,
    /// BT-114 — rounding amount.
    pub rounding: Option<InvoiceAmount>,
    /// BT-115 — **mandatory** amount due for payment (BR-15).
    pub due: InvoiceAmount,
}

// ── Invoice ───────────────────────────────────────────────────────────────────

/// An EN 16931 core invoice.
///
/// Fields are public: this is a data record, and both a builder and a parser
/// need to populate it. Validity is [`crate::validation`]'s job, not the type's.
/// # Constructing one
///
/// [`Invoice`] is `#[non_exhaustive]`, so that the business terms EN 16931-1:2026
/// adds are additive rather than breaking . That means no
/// struct literal from outside this crate — which is deliberate, and leaves two
/// supported ways in:
///
/// - **[`Invoice::builder`]** for producing an invoice, which names the
///   mandatory terms in its signature so the common mistakes are compile errors;
/// - **[`Invoice::default`] plus field assignment** for parsing one, where terms
///   arrive in whatever order the syntax presents them and *any* of them may be
///   missing — which is exactly the document a parser must be able to load in
///   order to report on.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Invoice {
    /// Whether this is an invoice or a credit note.
    ///
    /// Not a business term — it is what the syntax's root element states, and
    /// several CEN rules are bound to one root rather than to a term. See
    /// [`DocumentKind`].
    pub kind: DocumentKind,
    /// BT-24 — **mandatory** specification identifier (BR-01). Selects the profile.
    pub specification_id: Option<String>,
    /// BT-23 — business process type.
    pub business_process: Option<String>,
    /// BT-1 — **mandatory** invoice number (BR-02).
    pub number: Option<String>,
    /// BT-2 — **mandatory** issue date (BR-03).
    pub issue_date: Option<Date>,
    /// BT-3 — **mandatory** invoice type code (BR-04).
    pub type_code: Option<Code>,
    /// BT-5 — **mandatory** invoice currency code (BR-05).
    pub currency: Option<Code>,
    /// BT-6 — VAT accounting currency code.
    pub vat_accounting_currency: Option<Code>,
    /// BT-7 — value added tax point date. Exclusive with BT-8 (BR-CO-03).
    pub vat_point_date: Option<Date>,
    /// BT-8 — value added tax point date code. Exclusive with BT-7.
    pub vat_point_date_code: Option<Code>,
    /// BT-9 — payment due date.
    pub due_date: Option<Date>,
    /// BT-10 — buyer reference.
    pub buyer_reference: Option<String>,
    /// BT-11 — project reference.
    pub project_reference: Option<DocumentReference>,
    /// BT-12 — contract reference.
    ///
    /// Required by XRechnung's `BR-DE-CVD-01` on an invoice for road vehicles
    /// under the Clean Vehicles Directive.
    pub contract_reference: Option<DocumentReference>,
    /// BT-13 — purchase order reference.
    pub purchase_order_reference: Option<DocumentReference>,
    /// BT-14 — sales order reference.
    pub sales_order_reference: Option<DocumentReference>,
    /// BT-15 — receiving advice reference.
    pub receiving_advice_reference: Option<DocumentReference>,
    /// BT-16 — despatch advice reference.
    pub despatch_advice_reference: Option<DocumentReference>,
    /// BT-17 — tender or lot reference.
    ///
    /// Required by XRechnung's `BR-DE-CVD-02` — public procurement of clean
    /// road vehicles has to be traceable back to the tender it came from.
    pub tender_reference: Option<DocumentReference>,
    /// BT-18 — invoiced object identifier, at document level.
    ///
    /// The thing being billed for when it is not a purchase order: a subscriber
    /// number, a meter point, a vehicle. `BR-CL-07` constrains its scheme, and
    /// applies to this **and** to BT-128 on a line — one rule, two contexts.
    pub object_identifier: Option<Identifier>,
    /// BT-19 — buyer accounting reference.
    pub accounting_reference: Option<String>,
    /// BT-20 — payment terms.
    pub payment_terms: Option<String>,
    /// BG-1 — invoice notes.
    pub notes: Vec<InvoiceNote>,
    /// BG-3 — preceding invoice references.
    pub preceding_invoices: Vec<PrecedingInvoice>,
    /// BG-4 — the seller.
    pub seller: Party,
    /// BG-7 — the buyer.
    pub buyer: Party,
    /// BG-10 — the payee, when payment is directed somewhere other than the
    /// seller.
    pub payee: Option<Payee>,
    /// BG-11 — the seller's tax representative.
    pub tax_representative: Option<TaxRepresentative>,
    /// BG-13 — delivery information.
    pub delivery: Option<Delivery>,
    /// BG-14 — invoicing period.
    pub invoicing_period: Option<Period>,
    /// BG-16 — payment instructions.
    pub payment: Option<PaymentInstructions>,
    /// BG-20 — document level allowances.
    pub allowances: Vec<DocumentAllowanceCharge>,
    /// BG-21 — document level charges.
    pub charges: Vec<DocumentAllowanceCharge>,
    /// BG-23 — VAT breakdown. At least one (BR-CO-18).
    pub vat_breakdown: Vec<VatBreakdown>,
    /// BG-24 — additional supporting documents.
    pub attachments: Vec<SupportingDocument>,
    /// BG-25 — invoice lines. At least one (BR-16).
    pub lines: Vec<InvoiceLine>,
    /// BG-22 — document totals.
    pub totals: DocumentTotals,
    /// Data with no core business term — see [`crate::extensions`].
    ///
    /// §4.3's second mechanism: a CIUS *restricts*, an Extension *adds*. Carried
    /// here so a format crate that can represent it does, and `EN-EXT-01` warns
    /// when the target profile cannot — rather than letting it vanish.
    pub extensions: crate::extensions::Extensions,
}

impl Invoice {
    /// Start building an invoice, naming the terms EN 16931 makes mandatory on
    /// every document.
    ///
    /// The five arguments are BT-24, BT-1, BT-2, BT-3 and BT-5 — the terms
    /// BR-01 … BR-05 require unconditionally. Everything else is optional here
    /// and checked by [`crate::validation::validate`], because whether it is
    /// required depends on the document: BR-CO-25 wants a due date only when
    /// something is owed, and the category families want a VAT identifier only
    /// for the categories that levy tax.
    #[must_use]
    pub fn builder(
        specification_id: impl Into<String>,
        number: impl Into<String>,
        issue_date: Date,
        type_code: impl Into<Code>,
        currency: impl Into<Code>,
    ) -> InvoiceBuilder {
        InvoiceBuilder {
            inv: Invoice {
                specification_id: Some(specification_id.into()),
                number: Some(number.into()),
                issue_date: Some(issue_date),
                type_code: Some(type_code.into()),
                currency: Some(currency.into()),
                ..Default::default()
            },
        }
    }

    /// Every path a rule might want to iterate, for diagnostics and tests.
    ///
    /// Not used by the engine — rules iterate the collection they care about —
    /// but useful for asserting that a report covers what it should.
    #[must_use]
    pub fn occupied_groups(&self) -> Vec<Path> {
        let mut v = vec![Path::group(Group::Totals)];
        v.extend((0..self.lines.len()).map(|i| Path::at(Group::Line, i)));
        v.extend((0..self.vat_breakdown.len()).map(|i| Path::at(Group::VatBreakdown, i)));
        v.extend((0..self.allowances.len()).map(|i| Path::at(Group::DocumentAllowance, i)));
        v.extend((0..self.charges.len()).map(|i| Path::at(Group::DocumentCharge, i)));
        v
    }

    /// The VAT category semantics present anywhere in the document.
    ///
    /// Used by the exclusivity rules — BR-O-11 … BR-O-14 — and by the
    /// category-specific families to decide whether they apply at all.
    #[must_use]
    pub fn categories_used(&self) -> Vec<crate::VatCategory> {
        let mut v: Vec<_> = self
            .lines
            .iter()
            .filter_map(|l| l.vat.semantics())
            .chain(self.allowances.iter().filter_map(|a| a.vat.semantics()))
            .chain(self.charges.iter().filter_map(|c| c.vat.semantics()))
            .chain(
                self.vat_breakdown
                    .iter()
                    .filter_map(VatBreakdown::semantics),
            )
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }
}

/// The business terms this module addresses, as constants, so a rule never
/// writes a bare number.
pub mod terms {
    use crate::bt::BtId;

    macro_rules! bt {
        ($($name:ident = $n:literal, $doc:literal;)*) => {
            $(#[doc = $doc] pub const $name: BtId = BtId($n);)*
        };
    }

    bt! {
        NUMBER = 1, "Invoice number";
        ISSUE_DATE = 2, "Invoice issue date";
        TYPE_CODE = 3, "Invoice type code";
        CURRENCY = 5, "Invoice currency code";
        VAT_ACCOUNTING_CURRENCY = 6, "VAT accounting currency code";
        VAT_POINT_DATE = 7, "Value added tax point date";
        VAT_POINT_DATE_CODE = 8, "Value added tax point date code";
        DUE_DATE = 9, "Payment due date";
        BUSINESS_PROCESS = 23, "Business process type";
        BUYER_REFERENCE = 10, "Buyer reference";
        PROJECT_REFERENCE = 11, "Project reference";
        CONTRACT_REFERENCE = 12, "Contract reference";
        PURCHASE_ORDER_REFERENCE = 13, "Purchase order reference";
        SALES_ORDER_REFERENCE = 14, "Sales order reference";
        RECEIVING_ADVICE_REFERENCE = 15, "Receiving advice reference";
        DESPATCH_ADVICE_REFERENCE = 16, "Despatch advice reference";
        TENDER_REFERENCE = 17, "Tender or lot reference";
        OBJECT_IDENTIFIER = 18, "Invoiced object identifier";
        ACCOUNTING_REFERENCE = 19, "Buyer accounting reference";
        PAYMENT_TERMS = 20, "Payment terms";
        SPECIFICATION_ID = 24, "Specification identifier";
        PRECEDING_INVOICE = 25, "Preceding Invoice reference";
        SELLER_NAME = 27, "Seller name";
        SELLER_VAT_ID = 31, "Seller VAT identifier";
        SELLER_TAX_ID = 32, "Seller tax registration identifier";
        SELLER_LEGAL_INFO = 33, "Seller additional legal information";
        SELLER_ELECTRONIC_ADDRESS = 34, "Seller electronic address";
        SELLER_COUNTRY = 40, "Seller country code";
        BUYER_NAME = 44, "Buyer name";
        BUYER_VAT_ID = 48, "Buyer VAT identifier";
        BUYER_ELECTRONIC_ADDRESS = 49, "Buyer electronic address";
        BUYER_COUNTRY = 55, "Buyer country code";
        DELIVERY_DATE = 72, "Actual delivery date";
        PERIOD_START = 73, "Invoicing period start date";
        PERIOD_END = 74, "Invoicing period end date";
        DELIVER_TO_COUNTRY = 80, "Deliver to country code";
        PAYMENT_MEANS_CODE = 81, "Payment means type code";
        PAYMENT_ACCOUNT = 84, "Payment account identifier";
        ALLOWANCE_AMOUNT = 92, "Document level allowance amount";
        ALLOWANCE_BASE = 93, "Document level allowance base amount";
        ALLOWANCE_PERCENTAGE = 94, "Document level allowance percentage";
        ALLOWANCE_VAT_CATEGORY = 95, "Document level allowance VAT category code";
        ALLOWANCE_VAT_RATE = 96, "Document level allowance VAT rate";
        ALLOWANCE_REASON = 97, "Document level allowance reason";
        ALLOWANCE_REASON_CODE = 98, "Document level allowance reason code";
        CHARGE_AMOUNT = 99, "Document level charge amount";
        CHARGE_BASE = 100, "Document level charge base amount";
        CHARGE_PERCENTAGE = 101, "Document level charge percentage";
        CHARGE_VAT_CATEGORY = 102, "Document level charge VAT category code";
        CHARGE_VAT_RATE = 103, "Document level charge VAT rate";
        CHARGE_REASON = 104, "Document level charge reason";
        CHARGE_REASON_CODE = 105, "Document level charge reason code";
        LINE_TOTAL = 106, "Sum of Invoice line net amount";
        ALLOWANCE_TOTAL = 107, "Sum of allowances on document level";
        CHARGE_TOTAL = 108, "Sum of charges on document level";
        TAXABLE_TOTAL = 109, "Invoice total amount without VAT";
        VAT_TOTAL = 110, "Invoice total VAT amount";
        VAT_TOTAL_ACCOUNTING = 111, "Invoice total VAT amount in accounting currency";
        GROSS_TOTAL = 112, "Invoice total amount with VAT";
        PAID = 113, "Paid amount";
        ROUNDING = 114, "Rounding amount";
        DUE = 115, "Amount due for payment";
        VAT_TAXABLE_AMOUNT = 116, "VAT category taxable amount";
        VAT_TAX_AMOUNT = 117, "VAT category tax amount";
        VAT_CATEGORY = 118, "VAT category code";
        VAT_RATE = 119, "VAT category rate";
        EXEMPTION_REASON = 120, "VAT exemption reason text";
        EXEMPTION_REASON_CODE = 121, "VAT exemption reason code";
        SUPPORTING_DOCUMENT = 122, "Supporting document reference";
        LINE_ID = 126, "Invoice line identifier";
        LINE_QUANTITY = 129, "Invoiced quantity";
        LINE_UNIT_CODE = 130, "Invoiced quantity unit of measure code";
        LINE_NET_AMOUNT = 131, "Invoice line net amount";
        LINE_PERIOD_START = 134, "Invoice line period start date";
        LINE_PERIOD_END = 135, "Invoice line period end date";
        LINE_ALLOWANCE_AMOUNT = 136, "Invoice line allowance amount";
        LINE_ALLOWANCE_REASON = 139, "Invoice line allowance reason";
        LINE_ALLOWANCE_REASON_CODE = 140, "Invoice line allowance reason code";
        LINE_CHARGE_AMOUNT = 141, "Invoice line charge amount";
        LINE_CHARGE_REASON = 144, "Invoice line charge reason";
        LINE_CHARGE_REASON_CODE = 145, "Invoice line charge reason code";
        ITEM_NET_PRICE = 146, "Item net price";
        ITEM_PRICE_DISCOUNT = 147, "Item price discount";
        ITEM_GROSS_PRICE = 148, "Item gross price";
        PRICE_BASE_QUANTITY = 149, "Item price base quantity";
        PRICE_BASE_QUANTITY_CODE = 150, "Item price base quantity unit of measure";
        LINE_VAT_CATEGORY = 151, "Invoiced item VAT category code";
        LINE_VAT_RATE = 152, "Invoiced item VAT rate";
        ITEM_NAME = 153, "Item name";
        ITEM_STANDARD_ID = 157, "Item standard identifier";
        ITEM_CLASSIFICATION_ID = 158, "Item classification identifier";
    }
}

#[allow(unused_imports)]
use terms as _terms_are_public;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codes::generated::UNIT_CODES;

    #[test]
    fn code_is_verbatim_and_case_sensitive() {
        assert!(Code::new("KWH").is_in(UNIT_CODES));
        assert!(!Code::new("kwh").is_in(UNIT_CODES));
        assert!(!Code::new(" KWH").is_in(UNIT_CODES), "no trimming");
        assert!(Code::new("  ").is_blank());
    }

    #[test]
    fn an_invalid_code_is_representable() {
        // The whole point: a parser must be able to load `BT-3 = "999"` so the
        // engine can report BR-CL-01 against it, with a path.
        let bad = Code::new("999");
        assert_eq!(bad.as_str(), "999");
        assert!(!bad.is_in(crate::codes::generated::INVOICE_TYPE_CODES));
    }

    #[test]
    fn line_vat_semantics_are_optional() {
        let ok = LineVat {
            category: Code::new("S"),
            rate: Some(Percentage::new(rust_decimal::dec!(19))),
        };
        assert_eq!(ok.semantics(), Some(crate::VatCategory::Standard));

        let bad = LineVat {
            category: Code::new("Q"),
            rate: None,
        };
        assert_eq!(bad.semantics(), None, "so BR-CL-18 fires and others skip");
    }

    #[test]
    fn period_ordering_matches_br_29() {
        let p = Period {
            start: Some(Date::parse("2026-06-01").unwrap()),
            end: Some(Date::parse("2026-06-30").unwrap()),
        };
        assert_eq!(p.is_ordered(), Some(true));

        let reversed = Period {
            start: p.end,
            end: p.start,
        };
        assert_eq!(reversed.is_ordered(), Some(false));

        // BR-CO-19 allows one endpoint; ordering is then not a question.
        let open = Period {
            start: p.start,
            end: None,
        };
        assert_eq!(open.is_ordered(), None);
    }
}

// ── Builder ───────────────────────────────────────────────────────────────────

/// Fluent construction for [`Invoice`]. Obtain one from [`Invoice::builder`].
///
/// The builder deliberately does **not** validate. Validation is a report over a
/// whole document (see [`crate::validation`]), and a builder that refused to
/// produce an invalid invoice would leave a caller unable to ask *why* it is
/// invalid — which is the question they actually have.
#[derive(Debug, Clone)]
pub struct InvoiceBuilder {
    inv: Invoice,
}

macro_rules! setter {
    ($name:ident, $field:ident, $ty:ty, $doc:literal) => {
        #[doc = $doc]
        #[must_use]
        pub fn $name(mut self, value: $ty) -> Self {
            self.inv.$field = Some(value);
            self
        }
    };
}

impl InvoiceBuilder {
    setter!(due_date, due_date, Date, "BT-9 — payment due date.");
    setter!(
        payment_terms,
        payment_terms,
        String,
        "BT-20 — payment terms."
    );
    setter!(
        delivery,
        delivery,
        Delivery,
        "BG-13 — delivery information."
    );
    setter!(
        invoicing_period,
        invoicing_period,
        Period,
        "BG-14 — invoicing period."
    );
    setter!(
        payment,
        payment,
        PaymentInstructions,
        "BG-16 — payment instructions."
    );
    setter!(
        vat_accounting_currency,
        vat_accounting_currency,
        Code,
        "BT-6 — VAT accounting currency code."
    );

    /// BT-10 — buyer reference. Mandatory in XRechnung (`BR-DE-15`), optional in
    /// the core model.
    #[must_use]
    pub fn buyer_reference(mut self, value: impl Into<String>) -> Self {
        self.inv.buyer_reference = Some(value.into());
        self
    }

    /// BT-23 — the business process this invoice belongs to.
    ///
    /// Peppol's `R001` requires it and `R007` fixes its format; core EN 16931
    /// only says it may be there.
    #[must_use]
    pub fn business_process(mut self, value: impl Into<String>) -> Self {
        self.inv.business_process = Some(value.into());
        self
    }

    /// BG-4 — the seller.
    #[must_use]
    pub fn seller(mut self, seller: Party) -> Self {
        self.inv.seller = seller;
        self
    }

    /// BG-7 — the buyer.
    #[must_use]
    pub fn buyer(mut self, buyer: Party) -> Self {
        self.inv.buyer = buyer;
        self
    }

    setter!(payee, payee, Payee, "BG-10 — the payee.");
    setter!(
        tax_representative,
        tax_representative,
        TaxRepresentative,
        "BG-11 — the seller's tax representative."
    );

    /// Append a BG-25 invoice line.
    #[must_use]
    pub fn line(mut self, line: InvoiceLine) -> Self {
        self.inv.lines.push(line);
        self
    }

    /// Append a BG-23 VAT breakdown entry.
    #[must_use]
    pub fn vat_breakdown(mut self, entry: VatBreakdown) -> Self {
        self.inv.vat_breakdown.push(entry);
        self
    }

    /// Append a BG-20 document level allowance.
    #[must_use]
    pub fn allowance(mut self, allowance: DocumentAllowanceCharge) -> Self {
        self.inv.allowances.push(allowance);
        self
    }

    /// Append a BG-21 document level charge.
    #[must_use]
    pub fn charge(mut self, charge: DocumentAllowanceCharge) -> Self {
        self.inv.charges.push(charge);
        self
    }

    /// Mark this document a credit note.
    #[must_use]
    pub fn credit_note(mut self) -> Self {
        self.inv.kind = DocumentKind::CreditNote;
        self
    }

    /// Append a BG-1 invoice note (BT-22), with no subject code.
    #[must_use]
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.inv.notes.push(InvoiceNote::new(note));
        self
    }

    /// Append a BG-1 invoice note with a BT-21 subject code.
    #[must_use]
    pub fn coded_note(mut self, note: InvoiceNote) -> Self {
        self.inv.notes.push(note);
        self
    }

    /// BG-22 — the document totals.
    ///
    /// A setter like every other, and [`build`](Self::build) is the only way to
    /// finish. It used to return `Invoice` directly, which made it the terminal
    /// method *and* a setter — so `.totals(…).build()` did not compile and
    /// nothing in its name said why.
    #[must_use]
    pub fn totals(mut self, totals: DocumentTotals) -> Self {
        self.inv.totals = totals;
        self
    }

    /// Finish.
    ///
    /// Totals left unstated default to zero, which `BR-CO-10` … `BR-CO-16` will
    /// then report against — the builder does not compute them, because
    /// computing an invoice's arithmetic is `billing`'s job and silently
    /// inventing a total is how an unbalanced invoice reaches a counterparty.
    #[must_use]
    pub fn build(self) -> Invoice {
        self.inv
    }
}
