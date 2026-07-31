//! [`Attachment`] — EN 16931 §6.5.11 `Binary Object. Type`.

use core::fmt;

/// A file transmitted with the invoice — EN 16931-1 §6.5.11.
///
/// Table 25 gives three components, and **all three are mandatory**:
///
/// | Component | Primitive | Example |
/// |---|---|---|
/// | Content | Binary | |
/// | Mime Code | String | `image/jpeg` |
/// | Filename | String | `drawing5.jpg` |
///
/// So a bare byte slice is not an attachment. The mime code tells the receiver
/// how to open it and the filename is what they see; omitting either produces a
/// document a human cannot use, which is why the standard makes them mandatory
/// rather than conditional.
///
/// # The receiver obligation nobody implements
///
/// §6.5.11 also states a requirement that appears in **no Schematron** —
/// `BR-CL-24` only checks that the value is *a* MIME type:
///
/// > A Receiver of an Invoice, compliant to the core invoice model shall accept
/// > and process attachments that are of the following mime types […]
///
/// [`Attachment::RECEIVER_MUST_ACCEPT`] is that list. It constrains the
/// *receiver*, not the document, so it is not a validation rule — but a sending
/// system that only ever emits, say, `image/tiff` is relying on something the
/// standard does not promise, and [`Attachment::is_universally_accepted`] says
/// so.
///
/// # The invariant is enforced, not merely documented
///
/// `new` **fails** on an empty mime code or filename. Non-`Option` `String`
/// fields encode "present" but not "non-empty", and an empty string is exactly
/// the value a careless mapping produces — so the check lives in the
/// constructor, and [`serde`] deserialisation re-runs it rather than
/// reconstructing the fields directly.
///
/// This matters because nothing else catches it at the model level. `BR-CL-24`
/// rejects an empty mime code as a side effect of checking the list, and the
/// only rule that requires a filename at all is `UBL-DT-07` — a **syntax** rule
/// this crate deliberately does not implement. Without the constructor check, an
/// attachment with no filename would pass every rule here and fail in the format
/// crate, or worse, at the receiver.
///
/// ```
/// use en16931::Attachment;
///
/// let a = Attachment::new(b"%PDF-1.7".to_vec(), "application/pdf", "terms.pdf")?;
/// assert!(a.is_universally_accepted());
///
/// let exotic = Attachment::new(vec![], "image/tiff", "scan.tif")?;
/// assert!(!exotic.is_universally_accepted(), "lawful, but no receiver must take it");
///
/// // §6.5.11 makes both mandatory, so neither may be blank.
/// assert!(Attachment::new(vec![], "", "x.pdf").is_err());
/// assert!(Attachment::new(vec![], "application/pdf", "  ").is_err());
/// # Ok::<(), en16931::AttachmentError>(())
/// ```
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "Parts"))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Attachment {
    content: Vec<u8>,
    mime_code: String,
    filename: String,
}

/// The wire shape, so `serde` re-runs [`Attachment::new`]'s checks.
///
/// Deriving `Deserialize` straight onto [`Attachment`] would rebuild the private
/// fields directly and let a serialised document carry an attachment the
/// constructor would have refused — the invariant would hold everywhere except
/// across the one boundary that matters.
#[cfg(feature = "serde")]
#[derive(serde::Deserialize)]
struct Parts {
    content: Vec<u8>,
    mime_code: String,
    filename: String,
}

#[cfg(feature = "serde")]
impl TryFrom<Parts> for Attachment {
    type Error = AttachmentError;

    fn try_from(p: Parts) -> Result<Self, Self::Error> {
        Self::new(p.content, p.mime_code, p.filename)
    }
}

/// Why an [`Attachment`] could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AttachmentError {
    /// BT-125-1 is missing. §6.5.11 Table 25 makes it mandatory.
    #[error("attachment mime code is required (EN 16931-1 §6.5.11)")]
    MissingMimeCode,
    /// BT-125-2 is missing. §6.5.11 Table 25 makes it mandatory.
    #[error("attachment filename is required (EN 16931-1 §6.5.11)")]
    MissingFilename,
}

impl Attachment {
    /// The mime types §6.5.11 obliges every compliant receiver to accept.
    ///
    /// Normative, and absent from the validation artefacts — see the type
    /// documentation.
    pub const RECEIVER_MUST_ACCEPT: &'static [&'static str] = &[
        "application/pdf",
        "image/png",
        "image/jpeg",
        "text/csv",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "application/vnd.oasis.opendocument.spreadsheet",
    ];

    /// Construct an attachment. All three components are required by §6.5.11.
    ///
    /// # Errors
    /// [`AttachmentError`] when the mime code or the filename is blank.
    pub fn new(
        content: Vec<u8>,
        mime_code: impl Into<String>,
        filename: impl Into<String>,
    ) -> Result<Self, AttachmentError> {
        let mime_code = mime_code.into();
        let filename = filename.into();
        if mime_code.trim().is_empty() {
            return Err(AttachmentError::MissingMimeCode);
        }
        if filename.trim().is_empty() {
            return Err(AttachmentError::MissingFilename);
        }
        Ok(Self {
            content,
            mime_code,
            filename,
        })
    }

    /// The bytes.
    #[must_use]
    pub fn content(&self) -> &[u8] {
        &self.content
    }

    /// The mime code — BT-125 `@mimeCode`.
    #[must_use]
    pub fn mime_code(&self) -> &str {
        &self.mime_code
    }

    /// The filename — BT-125 `@filename`.
    #[must_use]
    pub fn filename(&self) -> &str {
        &self.filename
    }

    /// Whether every compliant receiver is obliged to accept this mime type.
    ///
    /// A `false` is not a validation failure — the standard does not restrict
    /// which types may be *sent* — but it does mean delivery depends on the
    /// counterparty's goodwill rather than on the standard.
    #[must_use]
    pub fn is_universally_accepted(&self) -> bool {
        Self::RECEIVER_MUST_ACCEPT.contains(&self.mime_code.as_str())
    }
}

impl fmt::Display for Attachment {
    /// `filename (mime, N bytes)` — a diagnostic rendering; the bytes are never
    /// printed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}, {} bytes)",
            self.filename,
            self.mime_code,
            self.content.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_three_components_are_present() {
        let a = Attachment::new(vec![1, 2, 3], "application/pdf", "terms.pdf")
            .expect("valid attachment");
        assert_eq!(a.content(), &[1, 2, 3]);
        assert_eq!(a.mime_code(), "application/pdf");
        assert_eq!(a.filename(), "terms.pdf");
        assert_eq!(a.to_string(), "terms.pdf (application/pdf, 3 bytes)");
    }

    #[test]
    fn the_receiver_obligation_list_matches_the_standard() {
        // §6.5.11, verbatim and in order.
        assert_eq!(
            Attachment::RECEIVER_MUST_ACCEPT,
            [
                "application/pdf",
                "image/png",
                "image/jpeg",
                "text/csv",
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                "application/vnd.oasis.opendocument.spreadsheet",
            ]
        );
    }

    #[test]
    fn an_exotic_type_is_lawful_but_not_guaranteed() {
        let csv = Attachment::new(vec![], "text/csv", "a.csv").expect("valid");
        assert!(csv.is_universally_accepted());
        let tiff = Attachment::new(vec![], "image/tiff", "a.tif").expect("valid");
        assert!(!tiff.is_universally_accepted());
    }
}
