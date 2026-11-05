//! The EN 16931 core invoice model (EN 16931-1 Table 2) as structs.
//!
//! Coded terms are [`Code`], not enums, so an invalid value such as
//! `BT-3 = "999"` still loads and is reported by the rules with its path; only
//! [`crate::VatCategory`] is an enum, because rules branch on its meaning.
//! Terms with cardinality 0..1 are `Option`: absent is not zero (BR-CO-13 and
//! BR-CO-16 branch on presence).

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
        crate::fmt::padded(f, &self.0)
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

impl From<Code> for String {
    fn from(c: Code) -> Self {
        c.0
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
    /// `None` when either endpoint is absent (BR-CO-19 / BR-CO-20 allow one).
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
    /// BT-40 / BT-55 / BT-69 / BT-80 — mandatory country code (BR-09, BR-11).
    pub country: Option<Code>,
}

impl PostalAddress {
    /// An address with street, city, post code and country (BT-35, BT-37,
    /// BT-38, BT-40 on a seller; BT-50, BT-52, BT-53, BT-55 on a buyer).
    ///
    /// The country is checked against ISO 3166-1 (`BR-CL-14`) here. Line 2,
    /// line 3 and the subdivision are set on the fields.
    ///
    /// ```
    /// use en16931::invoice::PostalAddress;
    ///
    /// let a = PostalAddress::new("Hauptstraße 1", "Musterstadt", "12345", "DE")?;
    /// assert_eq!(a.country.as_ref().map(|c| c.as_str()), Some("DE"));
    /// assert!(PostalAddress::new("Hauptstraße 1", "Musterstadt", "12345", "XX").is_err());
    /// # Ok::<(), en16931::codes::guard::CodeError>(())
    /// ```
    ///
    /// # Errors
    /// [`CodeError`](crate::codes::guard::CodeError) when `country` is not an
    /// ISO 3166-1 alpha-2 code.
    pub fn new(
        street: impl Into<String>,
        city: impl Into<String>,
        post_code: impl Into<String>,
        country: &str,
    ) -> Result<Self, crate::codes::guard::CodeError> {
        Ok(Self {
            line1: Some(street.into()),
            city: Some(city.into()),
            post_code: Some(post_code.into()),
            country: Some(crate::codes::guard::country(country)?),
            ..Self::default()
        })
    }
}

/// BG-6 SELLER CONTACT or BG-9 BUYER CONTACT.
///
/// Optional in the core model; XRechnung requires the seller's (`BR-DE-2`) with
/// all three terms (`BR-DE-5` … `BR-DE-7`).
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

impl Contact {
    /// A contact with all three terms — what XRechnung requires of the seller
    /// (`BR-DE-5`, `-6`, `-7`).
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        phone: impl Into<String>,
        email: impl Into<String>,
    ) -> Self {
        Self {
            name: Some(name.into()),
            phone: Some(phone.into()),
            email: Some(email.into()),
        }
    }
}

/// BG-4 SELLER or BG-7 BUYER.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Party {
    /// BT-27 / BT-44 — mandatory name (BR-06, BR-07).
    pub name: Option<String>,
    /// BT-28 / BT-45 — trading name.
    pub trading_name: Option<String>,
    /// BT-29 / BT-46 — identifiers.
    #[cfg_attr(feature = "serde", serde(default))]
    pub identifiers: Vec<Identifier>,
    /// BT-30 / BT-47 — legal registration identifier.
    pub legal_registration: Option<Identifier>,
    /// BT-31 / BT-48 — VAT identifier. BR-CO-09 constrains its prefix.
    pub vat_identifier: Option<String>,
    /// BT-32 — seller tax registration identifier.
    pub tax_registration: Option<String>,
    /// BT-33 — seller additional legal information, such as share capital.
    pub additional_legal_information: Option<String>,
    /// BT-34 / BT-49 — mandatory electronic address with a scheme
    /// (BR-62, BR-63).
    pub electronic_address: Option<Identifier>,
    /// BG-5 / BG-8 — mandatory postal address (BR-08, BR-10).
    pub address: PostalAddress,
    /// BG-6 / BG-9 — contact. XRechnung requires BG-6 (`BR-DE-2`).
    pub contact: Contact,
}

/// Which side of the transaction a [`Party`] is on; seller and buyer use
/// different business terms (BT-37 vs BT-52 for the city).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PartyRole {
    /// BG-4 — the seller.
    Seller,
    /// BG-7 — the buyer.
    Buyer,
}

impl PartyRole {
    /// The [`Group`] a finding about this party carries.
    #[must_use]
    pub const fn group(self) -> Group {
        match self {
            Self::Seller => Group::Seller,
            Self::Buyer => Group::Buyer,
        }
    }
}

impl Party {
    /// A party with name (BT-27 / BT-44, `BR-06` / `BR-07`) and postal address
    /// (BG-5 / BG-8, `BR-08` / `BR-10`). Everything else is set with the
    /// `with_*` methods or on the fields.
    ///
    /// ```
    /// use en16931::Identifier;
    /// use en16931::invoice::{Contact, Party, PostalAddress};
    ///
    /// let seller = Party::new(
    ///         "Stadtwerke Musterstadt GmbH",
    ///         PostalAddress::new("Hauptstraße 1", "Musterstadt", "12345", "DE")?,
    ///     )
    ///     .with_vat_identifier("DE123456789")
    ///     .with_electronic_address(Identifier::eas("4012345000009", "0088")?)
    ///     .with_contact(Contact::new("Erika Muster", "+49 30 1234567", "rechnung@example.de"));
    /// assert_eq!(seller.vat_identifier.as_deref(), Some("DE123456789"));
    /// # Ok::<(), en16931::codes::guard::CodeError>(())
    /// ```
    #[must_use]
    pub fn new(name: impl Into<String>, address: PostalAddress) -> Self {
        Self {
            name: Some(name.into()),
            address,
            ..Self::default()
        }
    }

    /// BT-31 / BT-48 — the VAT identifier, with its country prefix (`BR-CO-09`).
    #[must_use]
    pub fn with_vat_identifier(mut self, id: impl Into<String>) -> Self {
        self.vat_identifier = Some(id.into());
        self
    }

    /// BT-34 / BT-49 — the electronic address; build it with
    /// [`Identifier::eas`], which checks the scheme.
    #[must_use]
    pub fn with_electronic_address(mut self, address: Identifier) -> Self {
        self.electronic_address = Some(address);
        self
    }

    /// BG-6 / BG-9 — the contact.
    #[must_use]
    pub fn with_contact(mut self, contact: Contact) -> Self {
        self.contact = contact;
        self
    }

    /// The terms `profile` requires of this party that it does not yet carry,
    /// checked without an invoice.
    ///
    /// ```
    /// use en16931::invoice::{Party, PartyRole};
    /// use en16931::profiles::XRECHNUNG;
    ///
    /// let gaps = Party::default().missing_for(&XRECHNUNG, PartyRole::Buyer);
    /// let terms: Vec<_> = gaps.iter().map(|m| m.term.0).collect();
    /// assert!(terms.contains(&52), "BT-52 Buyer city — BR-DE-8");
    /// assert!(terms.contains(&53), "BT-53 Buyer post code — BR-DE-9");
    ///
    /// // A seller is asked for its own terms, not the buyer's.
    /// let seller = Party::default().missing_for(&XRECHNUNG, PartyRole::Seller);
    /// assert!(seller.iter().map(|m| m.term.0).any(|t| t == 37));   // BT-37, BR-DE-3
    /// assert!(!seller.iter().map(|m| m.term.0).any(|t| t == 52));
    /// ```
    ///
    /// As with [`Profile::missing_terms`], only restrictions are answered;
    /// conditional rules such as `BR-DE-16` (one of BT-30, BT-31, BT-32) are not.
    ///
    /// [`Profile::missing_terms`]: crate::validation::profile::Profile::missing_terms
    #[must_use]
    pub fn missing_for(
        &self,
        profile: &crate::Profile,
        role: PartyRole,
    ) -> Vec<crate::validation::profile::Missing> {
        // Restrictions read an `Invoice`: place the party in a scratch one and
        // keep the answers for its group.
        let mut scratch = Invoice::default();
        match role {
            PartyRole::Seller => scratch.seller = self.clone(),
            PartyRole::Buyer => scratch.buyer = self.clone(),
        }
        let group = role.group();
        profile
            .missing_terms(&scratch)
            .into_iter()
            .filter(|m| m.path.group == group)
            .collect()
    }
}

/// Whether the document is an invoice or a credit note.
///
/// Not derived from BT-3: it is what the UBL root element states, CEN rules
/// such as `BR-CO-25` key on it, and CEN credit-note test cases carry no BT-3.
/// `BR-CL-01` checks BT-3 against the list for this kind.
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

impl From<&str> for InvoiceNote {
    fn from(note: &str) -> Self {
        Self::new(note)
    }
}

impl From<String> for InvoiceNote {
    fn from(note: String) -> Self {
        Self::new(note)
    }
}

/// BG-10 PAYEE — where payment is directed, when that is not the seller.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Payee {
    /// BT-59 — mandatory payee name (BR-17), when BG-10 differs from BG-4.
    pub name: Option<String>,
    /// BT-60 — payee identifier.
    pub identifier: Option<Identifier>,
    /// BT-61 — payee legal registration identifier.
    pub legal_registration: Option<Identifier>,
}

/// BG-11 SELLER TAX REPRESENTATIVE PARTY. Several VAT category rules accept
/// its BT-63 in place of the seller's BT-31.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TaxRepresentative {
    /// BT-62 — mandatory name (BR-18).
    pub name: Option<String>,
    /// BT-63 — mandatory VAT identifier (BR-56).
    pub vat_identifier: Option<String>,
    /// BG-12 — mandatory postal address (BR-19); BT-69 is mandatory (BR-20).
    pub address: PostalAddress,
}

/// BG-3 PRECEDING INVOICE REFERENCE.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrecedingInvoice {
    /// BT-25 — mandatory reference (BR-55).
    pub reference: DocumentReference,
    /// BT-26 — issue date.
    pub issue_date: Option<Date>,
}

/// BG-24 ADDITIONAL SUPPORTING DOCUMENTS.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportingDocument {
    /// BT-122 — mandatory reference (BR-52).
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
    /// BT-84 — mandatory payment account identifier (BR-50).
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
    /// BT-87 — card primary account number. `BR-51` (a warning): never the full
    /// PAN, at most the first six and last four digits.
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

/// BG-17, BG-18 or BG-19 — mutually exclusive by construction, so
/// `BR-DE-23-b`, `-24-b` and `-25-b` cannot fail. `BR-DE-23-a` … `-25-a`, which
/// tie the variant to BT-81, are still checked.
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
    /// BT-81 — mandatory payment means code (BR-49).
    pub means_code: Option<Code>,
    /// BT-82 — payment means text.
    pub means_text: Option<String>,
    /// BT-83 — remittance information.
    pub remittance_information: Option<String>,
    /// BG-17 / BG-18 / BG-19 — at most one, by construction.
    pub means: Option<PaymentMeans>,
}

impl PaymentInstructions {
    /// A SEPA credit transfer to `iban`: BT-81 = `58` (UNTDID 4461, *SEPA credit
    /// transfer*) and one BG-17 with BT-84 and, when given, BT-86.
    ///
    /// A non-SEPA transfer is code `30`; build it on the fields.
    ///
    /// ```
    /// use en16931::invoice::PaymentInstructions;
    ///
    /// let pay = PaymentInstructions::sepa_credit_transfer("DE02120300000000202051", Some("BYLADEM1001"));
    /// assert_eq!(pay.means_code.as_ref().map(|c| c.as_str()), Some("58"));
    /// assert_eq!(pay.account_identifier(), Some("DE02120300000000202051"));
    /// ```
    ///
    /// The IBAN is not checked here; with the `sepa` feature the rules check
    /// BT-84 at validation.
    #[must_use]
    pub fn sepa_credit_transfer(iban: impl Into<String>, bic: Option<&str>) -> Self {
        Self {
            means_code: Some(Code::new("58")),
            means: Some(PaymentMeans::CreditTransfer(vec![CreditTransfer {
                account_identifier: Some(iban.into()),
                account_name: None,
                provider_identifier: bic.map(str::to_owned),
            }])),
            ..Self::default()
        }
    }

    /// A SEPA direct debit: BT-81 = `59` (UNTDID 4461, *SEPA direct debit*) and
    /// BG-19 with BT-89 (`mandate`), BT-90 (`creditor_id`) and, when given,
    /// BT-91 (`debited_account` — XRechnung requires it, `BR-DE-31`).
    ///
    /// ```
    /// use en16931::invoice::PaymentInstructions;
    ///
    /// let pay = PaymentInstructions::sepa_direct_debit(
    ///     "MANDATE-0001", "DE98ZZZ09999999999", Some("DE75512108001245126199"));
    /// assert_eq!(pay.means_code.as_ref().map(|c| c.as_str()), Some("59"));
    /// assert_eq!(pay.mandate_reference(), Some("MANDATE-0001"));
    /// ```
    #[must_use]
    pub fn sepa_direct_debit(
        mandate: impl Into<String>,
        creditor_id: impl Into<String>,
        debited_account: Option<&str>,
    ) -> Self {
        Self {
            means_code: Some(Code::new("59")),
            means: Some(PaymentMeans::DirectDebit(DirectDebit {
                mandate_reference: Some(mandate.into()),
                creditor_identifier: Some(creditor_id.into()),
                debited_account: debited_account.map(str::to_owned),
            })),
            ..Self::default()
        }
    }

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
    /// BT-151 / BT-95 / BT-102 — mandatory category code
    /// (BR-CO-04, BR-32, BR-37).
    pub category: Code,
    /// BT-152 / BT-96 / BT-103 — the rate.
    ///
    /// `None` for category `O` only (BR-O-05/06/07); other zero-tax categories
    /// state 0. Not BT-119 — see [`crate::VatCategory::states_rate`].
    pub rate: Option<Percentage>,
}

impl LineVat {
    /// The category's semantics, if the code is one of the ten UNCL 5305 values.
    ///
    /// `None` means `BR-CL-17` / `BR-CL-18` fails and category rules skip it.
    #[must_use]
    pub fn semantics(&self) -> Option<crate::VatCategory> {
        crate::VatCategory::from_code(self.category.as_str())
    }
}

/// BG-23 VAT BREAKDOWN — one entry per `(category, rate)`.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VatBreakdown {
    /// BT-116 — mandatory taxable amount (BR-45).
    pub taxable_amount: InvoiceAmount,
    /// BT-117 — mandatory tax amount (BR-46).
    pub tax_amount: InvoiceAmount,
    /// BT-118 — mandatory category code (BR-47).
    pub category: Code,
    /// BT-119 — the category rate. Optional when not subject to VAT (BR-48);
    /// XRechnung requires it always (BR-DE-14), even for category `O`.
    pub rate: Option<Percentage>,
    /// BT-120 — exemption reason text.
    pub exemption_reason: Option<String>,
    /// BT-121 — exemption reason code, from the CEF VATEX list. The category
    /// rules accept either this or BT-120.
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
    /// The rate compares by value, so `19` and `19.00` are one group.
    #[must_use]
    pub fn group_key(&self) -> (Code, Option<Percentage>) {
        (self.category.clone(), self.rate)
    }
}

// ── Allowances and charges ────────────────────────────────────────────────────

/// BG-20 DOCUMENT LEVEL ALLOWANCE or BG-21 DOCUMENT LEVEL CHARGE.
///
/// Which one it is follows from the [`Invoice`] field holding it.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentAllowanceCharge {
    /// BT-92 / BT-99 — mandatory amount (BR-31, BR-36), stated positive.
    pub amount: InvoiceAmount,
    /// BT-93 / BT-100 — base amount. Paired with the percentage (`R041`/`R042`).
    pub base_amount: Option<InvoiceAmount>,
    /// BT-94 / BT-101 — percentage. Paired with the base amount.
    pub percentage: Option<Percentage>,
    /// BT-95 / BT-102 and BT-96 / BT-103 — mandatory VAT (BR-32, BR-37).
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
    /// BT-136 / BT-141 — mandatory amount (BR-41, BR-43).
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
    /// BT-146 — mandatory item net price (BR-26). Not negative (BR-27).
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
    /// BT-160 — mandatory attribute name (BR-54), such as "Colour".
    pub name: Option<String>,
    /// BT-161 — mandatory attribute value (BR-54), such as "Red".
    pub value: Option<String>,
}

/// BG-31 ITEM INFORMATION.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Item {
    /// BT-153 — mandatory item name (BR-25).
    pub name: Option<String>,
    /// BT-154 — item description.
    pub description: Option<String>,
    /// BT-155 — seller's item identifier.
    pub seller_identifier: Option<String>,
    /// BT-156 — buyer's item identifier.
    pub buyer_identifier: Option<String>,
    /// BT-157 — standard identifier. Must carry a scheme (BR-64).
    pub standard_identifier: Option<Identifier>,
    /// BT-158 — classification identifiers. Must carry a scheme (BR-65).
    #[cfg_attr(feature = "serde", serde(default))]
    pub classification_identifiers: Vec<Identifier>,
    /// BT-159 — item country of origin.
    pub origin_country: Option<Code>,
    /// BG-32 — item attributes.
    #[cfg_attr(feature = "serde", serde(default))]
    pub attributes: Vec<ItemAttribute>,
}

/// BG-25 INVOICE LINE.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvoiceLine {
    /// BT-126 — mandatory line identifier (BR-21).
    pub id: String,
    /// BT-127 — line note.
    pub note: Option<String>,
    /// BT-132 — referenced purchase order line reference.
    pub order_line_reference: Option<DocumentReference>,
    /// BT-133 — line buyer accounting reference.
    pub accounting_reference: Option<String>,
    /// BT-128 — line object identifier.
    pub object_identifier: Option<Identifier>,
    /// BT-129 — mandatory invoiced quantity (BR-22). May be negative.
    pub quantity: Quantity,
    /// BT-130 — mandatory unit of measure code (BR-23).
    pub unit_code: Code,
    /// BT-131 — mandatory line net amount (BR-24).
    pub net_amount: InvoiceAmount,
    /// BG-26 — line period.
    pub period: Option<Period>,
    /// BG-27 — line allowances. Already folded into BT-131.
    #[cfg_attr(feature = "serde", serde(default))]
    pub allowances: Vec<LineAllowanceCharge>,
    /// BG-28 — line charges. Already folded into BT-131.
    #[cfg_attr(feature = "serde", serde(default))]
    pub charges: Vec<LineAllowanceCharge>,
    /// BG-29 — mandatory price details.
    pub price: PriceDetails,
    /// BG-30 — mandatory line VAT information (BR-CO-04).
    pub vat: LineVat,
    /// BG-31 — item information.
    pub item: Item,
}

impl InvoiceLine {
    /// A line from BT-126, BT-153, BT-129, BT-130, BT-131, BT-151 and BT-152
    /// (`BR-21` … `BR-25`, `BR-CO-04`); BT-146 is derived.
    ///
    /// ```
    /// use en16931::{InvoiceAmount, InvoiceLine, Percentage, Quantity};
    /// use rust_decimal::dec;
    ///
    /// let line = InvoiceLine::new(
    ///     "1",
    ///     "Netznutzung Arbeitspreis",
    ///     Quantity::new(dec!(1000)),
    ///     "KWH",
    ///     InvoiceAmount::parse("289.01")?,
    ///     "S",
    ///     Some(Percentage::new(dec!(19))),
    /// );
    /// assert_eq!(line.net_amount.to_string(), "289.01");
    /// assert_eq!(line.price.net_price.to_string(), "0.28901");   // derived
    /// # Ok::<(), en16931::ParseAmountError>(())
    /// ```
    ///
    /// BT-146 (`BR-26`) is set to net amount / quantity, or zero for a zero
    /// quantity. Use [`with_price`](Self::with_price) when the price has a base
    /// quantity (BT-149) or the quotient does not terminate.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        item_name: impl Into<String>,
        quantity: Quantity,
        unit_code: impl Into<Code>,
        net_amount: InvoiceAmount,
        vat_category: impl Into<Code>,
        vat_rate: Option<Percentage>,
    ) -> Self {
        let q = quantity.into_decimal();
        let net_price = if q.is_zero() {
            UnitPriceAmount::ZERO
        } else {
            net_amount
                .into_decimal()
                .checked_div(q)
                .map_or(UnitPriceAmount::ZERO, UnitPriceAmount::new)
        };
        Self {
            id: id.into(),
            note: None,
            order_line_reference: None,
            accounting_reference: None,
            object_identifier: None,
            quantity,
            unit_code: unit_code.into(),
            net_amount,
            period: None,
            allowances: Vec::new(),
            charges: Vec::new(),
            price: PriceDetails {
                net_price,
                ..Default::default()
            },
            vat: LineVat {
                category: vat_category.into(),
                rate: vat_rate,
            },
            item: Item {
                name: Some(item_name.into()),
                ..Default::default()
            },
        }
    }

    /// BG-29 — state the price rather than taking the derived quotient.
    #[must_use]
    pub fn with_price(mut self, price: PriceDetails) -> Self {
        self.price = price;
        self
    }

    /// BT-127 — the line note.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// BG-31 — replace the item, for the identifiers and attributes
    /// [`new`](Self::new) does not take.
    #[must_use]
    pub fn with_item(mut self, item: Item) -> Self {
        self.item = item;
        self
    }

    /// BG-26 — the line's own invoicing period.
    #[must_use]
    pub fn with_period(mut self, period: Period) -> Self {
        self.period = Some(period);
        self
    }
}

// ── Totals ────────────────────────────────────────────────────────────────────

/// BG-22 DOCUMENT TOTALS.
///
/// The five `Option` terms distinguish absent from zero.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocumentTotals {
    /// BT-106 — mandatory sum of line net amounts (BR-12).
    pub line_total: InvoiceAmount,
    /// BT-107 — sum of document level allowances.
    pub allowance_total: Option<InvoiceAmount>,
    /// BT-108 — sum of document level charges.
    pub charge_total: Option<InvoiceAmount>,
    /// BT-109 — mandatory total without VAT (BR-13).
    pub taxable_total: InvoiceAmount,
    /// BT-110 — total VAT amount.
    pub vat_total: Option<InvoiceAmount>,
    /// BT-111 — total VAT in the accounting currency. Required with BT-6 (BR-53).
    pub vat_total_accounting: Option<InvoiceAmount>,
    /// BT-112 — mandatory total with VAT (BR-14).
    pub gross_total: InvoiceAmount,
    /// BT-113 — paid amount.
    pub paid: Option<InvoiceAmount>,
    /// BT-114 — rounding amount.
    pub rounding: Option<InvoiceAmount>,
    /// BT-115 — mandatory amount due for payment (BR-15).
    pub due: InvoiceAmount,
}

// ── Invoice ───────────────────────────────────────────────────────────────────

/// An EN 16931 core invoice.
///
/// A data record with public fields; validity is checked by
/// [`crate::validation`]. Mandatory terms are `Option` too, so an incomplete
/// document can be loaded and reported on.
///
/// `#[non_exhaustive]`, so construct it with [`Invoice::builder`] (which takes
/// BT-24, BT-1, BT-2, BT-3 and BT-5) or [`Invoice::default`] plus field
/// assignment.
///
/// Under the `serde` feature it serialises as `{"en16931": "0.10", "invoice":
/// {…}}` ([`JSON_VERSION`]); any other version is refused. List fields may be
/// omitted.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Invoice {
    /// Invoice or credit note — not a business term; see [`DocumentKind`].
    pub kind: DocumentKind,
    /// BT-24 — specification identifier, mandatory (BR-01). Selects the profile.
    pub specification_id: Option<String>,
    /// BT-23 — business process type.
    pub business_process: Option<String>,
    /// BT-1 — invoice number, mandatory (BR-02).
    pub number: Option<String>,
    /// BT-2 — issue date, mandatory (BR-03).
    pub issue_date: Option<Date>,
    /// BT-3 — invoice type code, mandatory (BR-04).
    pub type_code: Option<Code>,
    /// BT-5 — invoice currency code, mandatory (BR-05).
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
    /// BT-12 — contract reference. Required by `BR-DE-CVD-01` on a
    /// clean-vehicles invoice.
    pub contract_reference: Option<DocumentReference>,
    /// BT-13 — purchase order reference.
    pub purchase_order_reference: Option<DocumentReference>,
    /// BT-14 — sales order reference.
    pub sales_order_reference: Option<DocumentReference>,
    /// BT-15 — receiving advice reference.
    pub receiving_advice_reference: Option<DocumentReference>,
    /// BT-16 — despatch advice reference.
    pub despatch_advice_reference: Option<DocumentReference>,
    /// BT-17 — tender or lot reference. Required by `BR-DE-CVD-02` on a
    /// clean-vehicles invoice.
    pub tender_reference: Option<DocumentReference>,
    /// BT-18 — invoiced object identifier (a meter point, a subscriber number).
    /// `BR-CL-07` constrains its scheme, as on BT-128.
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
    /// Data with no core business term — see [`crate::extensions`]. `EN-EXT-01`
    /// warns when the target profile cannot carry it.
    pub extensions: crate::extensions::Extensions,
}

// ── JSON envelope ─────────────────────────────────────────────────────────────

/// The version of the JSON envelope an [`Invoice`] is stored in under the
/// `serde` feature: `{"en16931": "0.10", "invoice": {…}}`.
///
/// A reader refuses any other version.
pub const JSON_VERSION: &str = "0.10";

#[cfg(feature = "serde")]
mod json {
    use super::*;
    use serde::de::{self, MapAccess, SeqAccess, Visitor};
    use serde::ser::SerializeStruct as _;

    /// The field list; the remote derive makes the compiler check it against
    /// [`Invoice`].
    #[derive(serde::Serialize, serde::Deserialize)]
    #[serde(remote = "Invoice")]
    struct Body {
        kind: DocumentKind,
        specification_id: Option<String>,
        business_process: Option<String>,
        number: Option<String>,
        issue_date: Option<Date>,
        type_code: Option<Code>,
        currency: Option<Code>,
        vat_accounting_currency: Option<Code>,
        vat_point_date: Option<Date>,
        vat_point_date_code: Option<Code>,
        due_date: Option<Date>,
        buyer_reference: Option<String>,
        project_reference: Option<DocumentReference>,
        contract_reference: Option<DocumentReference>,
        purchase_order_reference: Option<DocumentReference>,
        sales_order_reference: Option<DocumentReference>,
        receiving_advice_reference: Option<DocumentReference>,
        despatch_advice_reference: Option<DocumentReference>,
        tender_reference: Option<DocumentReference>,
        object_identifier: Option<Identifier>,
        accounting_reference: Option<String>,
        payment_terms: Option<String>,
        #[serde(default)]
        notes: Vec<InvoiceNote>,
        #[serde(default)]
        preceding_invoices: Vec<PrecedingInvoice>,
        seller: Party,
        buyer: Party,
        payee: Option<Payee>,
        tax_representative: Option<TaxRepresentative>,
        delivery: Option<Delivery>,
        invoicing_period: Option<Period>,
        payment: Option<PaymentInstructions>,
        #[serde(default)]
        allowances: Vec<DocumentAllowanceCharge>,
        #[serde(default)]
        charges: Vec<DocumentAllowanceCharge>,
        #[serde(default)]
        vat_breakdown: Vec<VatBreakdown>,
        #[serde(default)]
        attachments: Vec<SupportingDocument>,
        #[serde(default)]
        lines: Vec<InvoiceLine>,
        totals: DocumentTotals,
        #[serde(default)]
        extensions: crate::extensions::Extensions,
    }

    struct Out<'a>(&'a Invoice);

    impl serde::Serialize for Out<'_> {
        fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            Body::serialize(self.0, s)
        }
    }

    struct In(Invoice);

    impl<'de> serde::Deserialize<'de> for In {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            Body::deserialize(d).map(In)
        }
    }

    impl serde::Serialize for Invoice {
        fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            let mut st = s.serialize_struct("Invoice", 2)?;
            st.serialize_field("en16931", JSON_VERSION)?;
            st.serialize_field("invoice", &Out(self))?;
            st.end()
        }
    }

    fn check<E: de::Error>(found: &str) -> Result<(), E> {
        if found == JSON_VERSION {
            Ok(())
        } else {
            Err(E::custom(format!(
                "Invoice JSON version {found:?}; this build reads version {JSON_VERSION:?}"
            )))
        }
    }

    struct Envelope;

    impl<'de> Visitor<'de> for Envelope {
        type Value = Invoice;

        fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(
                f,
                "an Invoice envelope {{\"en16931\": \"{JSON_VERSION}\", \"invoice\": …}}"
            )
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Invoice, A::Error> {
            let mut version = false;
            let mut invoice = None;
            while let Some(key) = map.next_key::<String>()? {
                match key.as_str() {
                    "en16931" => {
                        check(&map.next_value::<String>()?)?;
                        version = true;
                    }
                    "invoice" => invoice = Some(map.next_value::<In>()?.0),
                    other => return Err(de::Error::unknown_field(other, FIELDS)),
                }
            }
            if !version {
                return Err(de::Error::custom(format!(
                    "not an Invoice envelope: no \"en16931\" version (this build reads {JSON_VERSION:?})"
                )));
            }
            invoice.ok_or_else(|| de::Error::missing_field("invoice"))
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Invoice, A::Error> {
            let version: String = seq
                .next_element()?
                .ok_or_else(|| de::Error::invalid_length(0, &self))?;
            check(&version)?;
            let body: In = seq
                .next_element()?
                .ok_or_else(|| de::Error::invalid_length(1, &self))?;
            Ok(body.0)
        }
    }

    const FIELDS: &[&str] = &["en16931", "invoice"];

    impl<'de> serde::Deserialize<'de> for Invoice {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            d.deserialize_struct("Invoice", FIELDS, Envelope)
        }
    }
}

impl Invoice {
    /// Start building an invoice from BT-24, BT-1, BT-2, BT-3 and BT-5 (BR-01 …
    /// BR-05). Everything else is set on the builder and checked by
    /// [`crate::validation::validate`].
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

    /// The credit note that cancels this invoice (*Stornorechnung*).
    ///
    /// Copies everything, then changes:
    ///
    /// | | |
    /// |---|---|
    /// | [`kind`](Self::kind) | [`DocumentKind::CreditNote`] — the UBL root element, and what `BR-CO-25` keys on |
    /// | BT-3 | `381`, the UNTDID 1001 credit-note code |
    /// | BT-1, BT-2 | the caller's — a credit note is its own document with its own number |
    /// | BG-3 | a [`PrecedingInvoice`] naming the original's BT-1 and BT-2 |
    ///
    /// Amounts are not negated: a credit note states positive figures and the
    /// document type carries the direction. BT-9 and BT-20 are kept.
    ///
    /// ```
    /// # use en16931::{Date, Invoice};
    /// # use en16931::invoice::DocumentKind;
    /// # fn demo(original: Invoice) -> Result<(), Box<dyn std::error::Error>> {
    /// let storno = original.to_credit_note("STORNO-2026-0007", Date::parse("2026-08-15")?);
    ///
    /// assert_eq!(storno.kind, DocumentKind::CreditNote);
    /// assert_eq!(storno.type_code.as_ref().map(|c| c.as_str()), Some("381"));
    /// // BG-3 points back at the invoice being cancelled — BR-55 wants BT-25.
    /// assert_eq!(storno.preceding_invoices.len(), 1);
    /// # Ok(()) }
    /// ```
    ///
    /// An invoice with no BT-1 gets no BG-3, since a blank BT-25 fails `BR-55`.
    #[must_use]
    pub fn to_credit_note(&self, number: impl Into<String>, issue_date: Date) -> Self {
        let mut note = self.clone();
        note.kind = DocumentKind::CreditNote;
        note.type_code = Some(Code::new("381"));
        note.number = Some(number.into());
        note.issue_date = Some(issue_date);
        if let Some(original) = self.number.as_deref().filter(|n| !n.trim().is_empty()) {
            note.preceding_invoices = vec![PrecedingInvoice {
                reference: DocumentReference::new(original),
                issue_date: self.issue_date,
            }];
        }
        note
    }

    /// The totals path plus one path per line, VAT breakdown, allowance and
    /// charge; for diagnostics and tests.
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
    /// Used by BR-O-11 … BR-O-14 and to decide which category families apply.
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

/// Named constants for business terms — `terms::BUYER_REFERENCE` rather than
/// `BtId(10)`. Not every term has one.
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
        // Loadable, so BR-CL-01 can report it.
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

    /// Everything is copied; kind, BT-3, BT-1, BT-2 and BG-3 change.
    #[test]
    fn a_credit_note_reverses_the_document_without_touching_the_money() {
        let mut inv = Invoice::builder(
            "urn:cen.eu:en16931:2017",
            "RE-2026-0042",
            Date::parse("2026-07-31").expect("date"),
            "380",
            "EUR",
        )
        .build();
        inv.seller.name = Some("Seller GmbH".into());
        inv.totals.gross_total = crate::InvoiceAmount::parse("1190.00").expect("amount");

        let storno = inv.to_credit_note("STORNO-1", Date::parse("2026-08-15").expect("date"));

        assert_eq!(storno.kind, DocumentKind::CreditNote);
        assert_eq!(storno.type_code.as_ref().map(Code::as_str), Some("381"));
        assert_eq!(storno.number.as_deref(), Some("STORNO-1"));
        assert_eq!(storno.issue_date, Date::parse("2026-08-15").ok());

        // BG-3 points back, with BT-25 non-blank so `BR-55` is satisfied.
        assert_eq!(storno.preceding_invoices.len(), 1);
        assert_eq!(
            storno.preceding_invoices[0].reference.as_str(),
            "RE-2026-0042"
        );
        assert_eq!(storno.preceding_invoices[0].issue_date, inv.issue_date);

        // Everything else is the original's, amounts included and unnegated.
        assert_eq!(storno.seller.name, inv.seller.name);
        assert_eq!(storno.totals.gross_total, inv.totals.gross_total);
        assert_eq!(storno.currency, inv.currency);
    }

    /// A blank BT-25 would fail `BR-55`, so no BG-3 is better than an empty one.
    #[test]
    fn a_credit_note_of_a_numberless_invoice_invents_no_reference() {
        let storno =
            Invoice::default().to_credit_note("STORNO-1", Date::parse("2026-08-15").unwrap());
        assert!(storno.preceding_invoices.is_empty());
        assert_eq!(storno.kind, DocumentKind::CreditNote);
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

    #[test]
    fn the_constructors_set_exactly_the_terms_they_are_given() {
        let built = Party::new(
            "Beispiel AG",
            PostalAddress::new("Marktplatz 2", "Beispielstadt", "54321", "DE").expect("DE"),
        )
        .with_vat_identifier("DE987654321")
        .with_electronic_address(Identifier::schemed("991-01234-56", "0204"))
        .with_contact(Contact::new("Max", "+49 1", "max@example.de"));
        let literal = Party {
            name: Some("Beispiel AG".into()),
            vat_identifier: Some("DE987654321".into()),
            electronic_address: Some(Identifier::schemed("991-01234-56", "0204")),
            address: PostalAddress {
                line1: Some("Marktplatz 2".into()),
                city: Some("Beispielstadt".into()),
                post_code: Some("54321".into()),
                country: Some(Code::new("DE")),
                ..PostalAddress::default()
            },
            contact: Contact {
                name: Some("Max".into()),
                phone: Some("+49 1".into()),
                email: Some("max@example.de".into()),
            },
            ..Party::default()
        };
        assert_eq!(built, literal);

        let err = PostalAddress::new("a", "b", "c", "GERMANY").unwrap_err();
        assert_eq!(err.rule, "BR-CL-14");
    }

    #[test]
    fn the_payment_constructors_set_code_and_group_together() {
        let ct = PaymentInstructions::sepa_credit_transfer("DE02120300000000202051", None);
        assert_eq!(
            ct,
            PaymentInstructions {
                means_code: Some(Code::new("58")),
                means: Some(PaymentMeans::CreditTransfer(vec![CreditTransfer {
                    account_identifier: Some("DE02120300000000202051".into()),
                    ..CreditTransfer::default()
                }])),
                ..PaymentInstructions::default()
            }
        );
        let dd = PaymentInstructions::sepa_direct_debit("M-1", "DE98ZZZ09999999999", None);
        assert_eq!(
            dd,
            PaymentInstructions {
                means_code: Some(Code::new("59")),
                means: Some(PaymentMeans::DirectDebit(DirectDebit {
                    mandate_reference: Some("M-1".into()),
                    creditor_identifier: Some("DE98ZZZ09999999999".into()),
                    debited_account: None,
                })),
                ..PaymentInstructions::default()
            }
        );
    }

    /// Under XRechnung, findings concern only terms the constructors do not
    /// take, and `missing_for` predicts the party ones.
    #[test]
    fn what_the_constructors_leave_out_is_what_xrechnung_asks_for() {
        use crate::profiles::XRECHNUNG;
        let party = |name: &str, vat: &str| {
            Party::new(
                name,
                PostalAddress::new("Hauptstraße 1", "Musterstadt", "12345", "DE").expect("DE"),
            )
            .with_vat_identifier(vat)
        };
        let seller = party("Stadtwerke Musterstadt GmbH", "DE123456789");
        let predicted: Vec<u16> = seller
            .missing_for(&XRECHNUNG, PartyRole::Seller)
            .iter()
            .map(|m| m.term.0)
            .collect();
        let inv = Invoice::builder(
            XRECHNUNG.specification_id,
            "R-1",
            Date::parse("2026-07-31").expect("date"),
            "380",
            "EUR",
        )
        .seller(seller)
        .buyer(party("Beispiel AG", "DE987654321"))
        .payment(PaymentInstructions::sepa_credit_transfer(
            "DE02120300000000202051",
            None,
        ))
        .due_in_days(14)
        .line(InvoiceLine::new(
            "1",
            "Netznutzung",
            crate::Quantity::new(rust_decimal::Decimal::from(10)),
            "KWH",
            crate::InvoiceAmount::from_minor_units(1_000),
            "S",
            Some(crate::Percentage::new(rust_decimal::Decimal::from(19))),
        ))
        .build_reconciled()
        .expect("reconciles");
        let report = XRECHNUNG.validate(&inv);
        let fatal: Vec<&str> = report.fatal().map(|f| f.rule.as_str()).collect();
        assert!(fatal.contains(&"BR-DE-15"), "BT-10: {report}");
        // The seller contact XRechnung wants (BT-41 … BT-43) is predicted…
        for t in [41, 42, 43] {
            assert!(predicted.contains(&t), "BT-{t} predicted: {predicted:?}");
        }
        // …and nothing the constructors were given is reported.
        for f in report.fatal() {
            let terms: Vec<u16> = f.path.term.map(|t| t.0).into_iter().collect();
            for given in [27, 35, 37, 38, 40, 31, 44, 50, 52, 53, 55, 48, 81, 84] {
                assert!(!terms.contains(&given), "BT-{given} was given: {f}");
            }
        }
    }
}

// ── Builder ───────────────────────────────────────────────────────────────────

/// Fluent construction for [`Invoice`]. Obtain one from [`Invoice::builder`].
///
/// The builder does not validate; run [`crate::validation`] on the result.
#[derive(Debug, Clone)]
pub struct InvoiceBuilder {
    inv: Invoice,
}

/// A setter taking the field's own type.
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

/// A setter taking anything that converts into the field's type.
macro_rules! into_setter {
    ($name:ident, $field:ident, $ty:ty, $doc:literal) => {
        #[doc = $doc]
        #[must_use]
        pub fn $name(mut self, value: impl Into<$ty>) -> Self {
            self.inv.$field = Some(value.into());
            self
        }
    };
}

impl InvoiceBuilder {
    setter!(due_date, due_date, Date, "BT-9 — payment due date.");

    /// BT-20 — payment terms, as free text. `BR-CO-25` accepts this or a due
    /// date.
    ///
    /// ```
    /// # use en16931::{Invoice, Date};
    /// let inv = Invoice::builder("urn:cen.eu:en16931:2017", "R-1",
    ///         Date::parse("2026-07-31")?, "380", "EUR")
    ///     .payment_terms("Zahlbar innerhalb 14 Tagen ohne Abzug")
    ///     .build();
    /// assert!(inv.payment_terms.is_some());
    /// # Ok::<(), en16931::ParseDateError>(())
    /// ```
    #[must_use]
    pub fn payment_terms(mut self, value: impl Into<String>) -> Self {
        self.inv.payment_terms = Some(value.into());
        self
    }

    /// BT-9 — a due date `days` after the issue date; unset without BT-2.
    ///
    /// ```
    /// # use en16931::{Invoice, Date};
    /// let inv = Invoice::builder("urn:cen.eu:en16931:2017", "R-1",
    ///         Date::parse("2026-07-31")?, "380", "EUR")
    ///     .due_in_days(14)
    ///     .build();
    /// assert_eq!(inv.due_date.map(|d| d.to_string()).as_deref(), Some("2026-08-14"));
    /// # Ok::<(), en16931::ParseDateError>(())
    /// ```
    #[must_use]
    pub fn due_in_days(mut self, days: i32) -> Self {
        self.inv.due_date = self.inv.issue_date.and_then(|d| d.checked_add_days(days));
        self
    }

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
    into_setter!(
        vat_accounting_currency,
        vat_accounting_currency,
        Code,
        "BT-6 — VAT accounting currency code."
    );

    /// BT-10 — buyer reference. Mandatory in XRechnung (`BR-DE-15`), where it
    /// carries the Leitweg-ID (not BT-49, see [`crate::codes::guard::eas`]).
    #[must_use]
    pub fn buyer_reference(mut self, value: impl Into<String>) -> Self {
        self.inv.buyer_reference = Some(value.into());
        self
    }

    /// BT-23 — business process type. Required by Peppol (`R001`, format `R007`).
    #[must_use]
    pub fn business_process(mut self, value: impl Into<String>) -> Self {
        self.inv.business_process = Some(value.into());
        self
    }

    into_setter!(
        object_identifier,
        object_identifier,
        Identifier,
        "BT-18 — invoiced object identifier: a meter point, a subscriber number, a vehicle."
    );
    into_setter!(
        purchase_order_reference,
        purchase_order_reference,
        DocumentReference,
        "BT-13 — purchase order reference."
    );
    into_setter!(
        contract_reference,
        contract_reference,
        DocumentReference,
        "BT-12 — contract reference. Required by `BR-DE-CVD-01` on a clean-vehicles invoice."
    );
    into_setter!(
        project_reference,
        project_reference,
        DocumentReference,
        "BT-11 — project reference."
    );
    into_setter!(
        accounting_reference,
        accounting_reference,
        String,
        "BT-19 — buyer accounting reference."
    );

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

    /// Append a BG-1 invoice note — text alone (BT-22), or an [`InvoiceNote`]
    /// with its BT-21 subject code.
    ///
    /// ```
    /// # use en16931::{Date, Invoice, InvoiceNote};
    /// let inv = Invoice::builder("urn:cen.eu:en16931:2017", "R-1",
    ///         Date::parse("2026-07-31")?, "380", "EUR")
    ///     .note("Vielen Dank für Ihren Auftrag")
    ///     .note(InvoiceNote::new("Steuerschuldnerschaft des Leistungsempfängers").with_subject("AAI"))
    ///     .build();
    /// assert_eq!(inv.notes.len(), 2);
    /// # Ok::<(), en16931::ParseDateError>(())
    /// ```
    #[must_use]
    pub fn note(mut self, note: impl Into<InvoiceNote>) -> Self {
        self.inv.notes.push(note.into());
        self
    }

    /// BG-22 — the document totals.
    #[must_use]
    pub fn totals(mut self, totals: DocumentTotals) -> Self {
        self.inv.totals = totals;
        self
    }

    /// Finish, stating the totals yourself.
    ///
    /// Totals not set stay zero / absent and are reported by `BR-CO-10` …
    /// `BR-CO-16`; [`build_reconciled`](Self::build_reconciled) computes them.
    #[must_use]
    pub fn build(self) -> Invoice {
        self.inv
    }

    /// Finish, deriving BG-23 and BG-22 from the lines.
    ///
    /// See [`crate::reconcile`](mod@crate::reconcile) for what is computed.
    /// Anything set with [`totals`](Self::totals) or
    /// [`vat_breakdown`](Self::vat_breakdown) is replaced, except exemption
    /// reasons on an existing BG-23, which are kept.
    ///
    /// ```
    /// # use en16931::{Date, Invoice, InvoiceAmount, Percentage, Quantity, validate};
    /// # use en16931::invoice::{Code, InvoiceLine, Item, LineVat, PriceDetails};
    /// # use rust_decimal::dec;
    /// # fn line(id: &str, net: &str, rate: u32) -> InvoiceLine {
    /// #     InvoiceLine { id: id.into(), note: None, order_line_reference: None,
    /// #         accounting_reference: None, object_identifier: None,
    /// #         quantity: Quantity::new(dec!(1)), unit_code: Code::new("C62"),
    /// #         net_amount: InvoiceAmount::parse(net).unwrap(), period: None,
    /// #         allowances: vec![], charges: vec![], price: PriceDetails::default(),
    /// #         vat: LineVat { category: Code::new("S"),
    /// #                        rate: Some(Percentage::new(rate.into())) },
    /// #         item: Item { name: Some("x".into()), ..Default::default() } }
    /// # }
    /// let inv = Invoice::builder("urn:cen.eu:en16931:2017", "R-1",
    ///         Date::parse("2026-07-31")?, "380", "EUR")
    ///     .line(line("1", "1000.00", 19))
    ///     .line(line("2", "500.00", 7))
    ///     .build_reconciled()?;
    ///
    /// assert_eq!(inv.vat_breakdown.len(), 2);
    /// assert_eq!(inv.totals.gross_total.to_string(), "1725.00");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// # Errors
    /// [`crate::ReconcileError`] when the arithmetic cannot be performed at all
    /// — an unknown VAT category, or a taxed category with no rate.
    pub fn build_reconciled(self) -> Result<Invoice, crate::ReconcileError> {
        self.build_reconciled_with(&crate::Reconciler::new())
    }

    /// As [`build_reconciled`](Self::build_reconciled), with a configured
    /// [`Reconciler`](crate::Reconciler) — for a prepayment (BT-113), a rounding
    /// amount (BT-114), or the exemption reasons `BR-E-10` and its four siblings
    /// require.
    ///
    /// # Errors
    /// As [`build_reconciled`](Self::build_reconciled).
    pub fn build_reconciled_with(
        mut self,
        reconciler: &crate::Reconciler,
    ) -> Result<Invoice, crate::ReconcileError> {
        reconciler.apply(&mut self.inv)?;
        Ok(self.inv)
    }
}
