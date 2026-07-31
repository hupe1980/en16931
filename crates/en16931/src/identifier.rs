//! [`Identifier`] and [`DocumentReference`] — EN 16931 §6.5.6 and §6.5.7.
//!
//! These are two **different** semantic data types, and the difference is not
//! cosmetic. Modelling a document reference as an identifier would permit a
//! scheme the standard does not allow there; modelling an identifier as a plain
//! string loses the scheme that several rules require (BR-62, BR-63, BR-64,
//! BR-65) and that tells a receiver how to interpret the value at all.

use core::fmt;

/// An identifier — EN 16931-1 §6.5.6 `Identifier. Type`.
///
/// Three components, per Table 20:
///
/// | Component | Use | Example |
/// |---|---|---|
/// | Content | mandatory | `abc:123-DEF` |
/// | Scheme identifier | conditional | `GLN` |
/// | Scheme version identifier | conditional | `1.0` |
///
/// The **scheme version** is the component most implementations forget. It is
/// separate from the scheme, and both are conditional per business term — the
/// semantic model states, for each identifier, whether a scheme may or shall be
/// given and from which list.
///
/// Several rules are exactly "this identifier shall carry a scheme":
///
/// | Rule | Term |
/// |---|---|
/// | BR-62 | Seller electronic address (BT-34) |
/// | BR-63 | Buyer electronic address (BT-49) |
/// | BR-64 | Item standard identifier (BT-157) |
/// | BR-65 | Item classification identifier (BT-158) |
///
/// so [`Identifier::scheme`] being `None` is a *reportable finding*, not an
/// impossible state — the type must be able to hold an invalid document in
/// order to explain why it is invalid.
///
/// ```
/// use en16931::Identifier;
///
/// // A Peppol electronic address: scheme is the CEF EAS code, not free text.
/// let endpoint = Identifier::schemed("9958:DE123456789", "0204");
/// assert_eq!(endpoint.scheme(), Some("0204"));
/// assert_eq!(endpoint.to_string(), "0204:9958:DE123456789");
///
/// // A bare identifier is representable; whether it is *valid* is a rule.
/// assert_eq!(Identifier::new("INV-2026-001").scheme(), None);
/// ```
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Identifier {
    content: String,
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    scheme: Option<String>,
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    scheme_version: Option<String>,
}

impl Identifier {
    /// An identifier with no scheme.
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            scheme: None,
            scheme_version: None,
        }
    }

    /// An identifier with a scheme identifier.
    pub fn schemed(content: impl Into<String>, scheme: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            scheme: Some(scheme.into()),
            scheme_version: None,
        }
    }

    /// BT-34 / BT-49 — an electronic address, with its EAS scheme **checked**.
    ///
    /// The one identifier in the model whose scheme comes from a code list that
    /// changes: nine EAS schemes were withdrawn in 2023 alone, and `9958` —
    /// the one a German integrator reaches for — is among them. Checking here
    /// turns `BR-CL-25` from a finding on an assembled document into an error
    /// at the line of code that produced it, with the successor named.
    ///
    /// ```
    /// use en16931::Identifier;
    ///
    /// let ok = Identifier::eas("4012345000009", "0088")?;      // GLN
    /// assert_eq!(ok.scheme(), Some("0088"));
    ///
    /// // The Leitweg-ID scheme, withdrawn on 2023-07-31.
    /// let err = Identifier::eas("991-01234-56", "9958").unwrap_err();
    /// assert!(err.to_string().contains("use 0204 instead"));
    /// # Ok::<(), en16931::codes::guard::CodeError>(())
    /// ```
    ///
    /// # Errors
    /// [`CodeError`](crate::codes::guard::CodeError) when `scheme` is not in the
    /// CEF EAS code list, carrying the successor of a withdrawn code where the
    /// authority names one.
    pub fn eas(
        content: impl Into<String>,
        scheme: &str,
    ) -> Result<Self, crate::codes::guard::CodeError> {
        let scheme = crate::codes::guard::eas(scheme)?;
        Ok(Self::schemed(content, scheme.as_str()))
    }

    /// Attach a scheme version identifier.
    #[must_use]
    pub fn with_scheme_version(mut self, version: impl Into<String>) -> Self {
        self.scheme_version = Some(version.into());
        self
    }

    /// The identifier value.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }

    /// The scheme identifier, if any — BR-62 … BR-65 require one on four terms.
    #[must_use]
    pub fn scheme(&self) -> Option<&str> {
        self.scheme.as_deref()
    }

    /// The scheme version identifier, if any.
    #[must_use]
    pub fn scheme_version(&self) -> Option<&str> {
        self.scheme_version.as_deref()
    }

    /// Whether the content is empty or whitespace only.
    ///
    /// §6.5 says content is always mandatory — *"Whenever a business term is
    /// used in a core Invoice this term shall always have content"* — so an
    /// empty identifier is a finding rather than an absent term.
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.content.trim().is_empty()
    }
}

impl fmt::Display for Identifier {
    /// `scheme:content` when a scheme is present, otherwise the bare content.
    ///
    /// This is a *diagnostic* rendering for reports and logs, not a wire format:
    /// each syntax carries the scheme in its own attribute
    /// (`@schemeID` in UBL), never concatenated.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.scheme {
            Some(s) => write!(f, "{s}:{}", self.content),
            None => f.write_str(&self.content),
        }
    }
}

impl From<&str> for Identifier {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for Identifier {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

/// A reference to another document — EN 16931-1 §6.5.7 `Document Reference. Type`.
///
/// > Identifiers that were assigned to a document or document line by the Buyer,
/// > the Seller or by a third party.
///
/// Table 21 gives it **one** component: content. It is deliberately *not* an
/// [`Identifier`] — there is no scheme, because the issuer of the referenced
/// document is already known from which business term carries it.
///
/// Used by BT-12 (contract), BT-13 (purchase order), BT-14 (sales order),
/// BT-16 (despatch advice), BT-25 (preceding invoice), BT-122 (supporting
/// document) and others.
///
/// ```
/// use en16931::DocumentReference;
///
/// let po = DocumentReference::new("PO-4711");
/// assert_eq!(po.as_str(), "PO-4711");
/// ```
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DocumentReference(String);

impl DocumentReference {
    /// Wrap a reference value.
    pub fn new(content: impl Into<String>) -> Self {
        Self(content.into())
    }

    /// The reference value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the content is empty or whitespace only.
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl fmt::Display for DocumentReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&self.0)
    }
}

impl From<&str> for DocumentReference {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for DocumentReference {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_carries_all_three_components() {
        let id = Identifier::schemed("123456789", "0088").with_scheme_version("1.0");
        assert_eq!(id.content(), "123456789");
        assert_eq!(id.scheme(), Some("0088"));
        assert_eq!(id.scheme_version(), Some("1.0"), "the forgotten component");
    }

    #[test]
    fn a_missing_scheme_is_representable_so_it_can_be_reported() {
        // BR-62/63/64/65 require a scheme on four terms. The type must be able
        // to hold the invalid state, or a parser cannot explain what is wrong.
        let bare = Identifier::new("DE123456789");
        assert_eq!(bare.scheme(), None);
        assert_eq!(bare.to_string(), "DE123456789");
    }

    #[test]
    fn blank_content_is_detectable() {
        assert!(Identifier::new("   ").is_blank());
        assert!(!Identifier::new("x").is_blank());
        assert!(DocumentReference::new("").is_blank());
    }

    #[test]
    fn document_reference_has_no_scheme_by_construction() {
        // §6.5.7 Table 21: one component. Not an Identifier with None.
        let r = DocumentReference::new("PO-4711");
        assert_eq!(r.as_str(), "PO-4711");
        assert_eq!(r.to_string(), "PO-4711");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_omits_absent_components() {
        let bare = serde_json::to_string(&Identifier::new("X")).unwrap();
        assert_eq!(bare, r#"{"content":"X"}"#);
        let full = serde_json::to_string(&Identifier::schemed("X", "0088")).unwrap();
        assert_eq!(full, r#"{"content":"X","scheme":"0088"}"#);
        // Document Reference is transparent — a bare string, not an object.
        assert_eq!(
            serde_json::to_string(&DocumentReference::new("PO-1")).unwrap(),
            r#""PO-1""#
        );
    }
}
