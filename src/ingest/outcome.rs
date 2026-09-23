// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
//! The verdict of onboarding a document.
//!
//! This exists because the previous behaviour was to say nothing. A
//! password-protected PDF, an image-only DOCX, an unsupported format:
//! the row said `ready`, the model received an empty block, and the
//! answer came back confident about a document nobody had read. The
//! outcome travels with the document so three places can be honest
//! about it — the attachment list, the prompt, and the logs.
//!
//! **Codes, not sentences.** What lands in `documents.extraction_reason`
//! is a canonical English identifier (`scanned_pdf`, `encrypted_pdf`,
//! …), and the interface translates it into the user's language like
//! every other display string. Storing an Italian sentence would hide
//! an untranslatable string in the database, and the same value has to
//! read well in six languages — in the attachment list, in the
//! data-sources panel, and in the prompt the model receives.

use std::fmt;

/// Whether a document's text is usable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractionOutcome {
    /// Text was extracted and is not empty.
    Ready,
    /// The file was read but carries no text a model can use.
    NoText { code: &'static str },
}

/// Codes for a file that was read but has nothing usable in it. Each
/// one has a matching entry in the interface catalogues.
pub mod no_text {
    /// A PDF with no text layer: a scan or an image. The one case with
    /// an actionable answer — turn OCR on, or use a model that reads
    /// images.
    pub const SCANNED_PDF: &str = "scanned_pdf";
    /// A word-processing document with no text, typically only images.
    pub const IMAGE_ONLY: &str = "image_only";
    /// Nothing in the file at all.
    pub const EMPTY_FILE: &str = "empty_file";
    /// An Office or legacy format that converted to nothing.
    pub const OFFICE_NO_TEXT: &str = "office_no_text";
    /// An image (or a scan rendered as images) attached while the
    /// selected model cannot see images. Not an extraction failure —
    /// the file is fine, the pairing is not — but from the answer's
    /// point of view the content is just as unavailable, and saying so
    /// is the difference between "I cannot read this" and an answer
    /// invented over an attachment the model never received.
    pub const IMAGE_NEEDS_VISION_MODEL: &str = "image_needs_vision_model";
}

/// Codes for a document that was read, with something the reader
/// should know about it.
pub mod warning {
    /// The text layer does not match what is printed: tampered
    /// `cmap`/`ToUnicode` tables, private-use or zero-width characters.
    /// The extracted text can say something the reader never sees —
    /// for a contract or a court filing that is the dangerous case, not
    /// a curiosity, and it must reach both the user and the model.
    pub const TEXT_LAYER_UNRELIABLE: &str = "text_layer_unreliable";
}

/// Codes for a file that could not be read at all.
pub mod failure {
    /// Encrypted PDF: the user can act on this one.
    pub const ENCRYPTED_PDF: &str = "encrypted_pdf";
    /// A format the product does not read.
    pub const UNSUPPORTED_FORMAT: &str = "unsupported_format";
    /// Damaged container, or an extension that lies about the content.
    pub const CORRUPT_FILE: &str = "corrupt_file";
    /// Anything else. The technical detail rides along, because a
    /// sentence the user can paste into a report beats a tidy message
    /// that hides which file broke and how.
    pub const READ_FAILED: &str = "read_failed";
}

impl ExtractionOutcome {
    /// Canonical identifier stored in `documents.status` and matched in
    /// code. English snake_case, like every other schema value.
    pub fn tag(&self) -> &'static str {
        match self {
            ExtractionOutcome::Ready => "ready",
            ExtractionOutcome::NoText { .. } => "no_text",
        }
    }

    /// The code explaining the verdict, when there is one.
    pub fn code(&self) -> Option<&'static str> {
        match self {
            ExtractionOutcome::Ready => None,
            ExtractionOutcome::NoText { code } => Some(code),
        }
    }
}

/// Why a document that parsed carries no usable text.
pub fn no_text_code(format: &pageindex_rs::model::InputFormat) -> &'static str {
    use pageindex_rs::model::InputFormat;
    match format {
        InputFormat::Pdf => no_text::SCANNED_PDF,
        InputFormat::Docx => no_text::IMAGE_ONLY,
        InputFormat::Office(_) => no_text::OFFICE_NO_TEXT,
        InputFormat::Markdown | InputFormat::Text => no_text::EMPTY_FILE,
    }
}

/// A document we could not read, as a code plus whatever detail is
/// worth carrying.
#[derive(Debug, Clone)]
pub struct IngestError {
    /// Canonical identifier; see [`failure`].
    pub kind: &'static str,
    /// Technical detail, kept for `read_failed` and for the log.
    pub detail: Option<String>,
}

impl fmt::Display for IngestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.detail {
            Some(d) => write!(f, "{}: {d}", self.kind),
            None => write!(f, "{}", self.kind),
        }
    }
}

impl std::error::Error for IngestError {}

impl IngestError {
    /// What goes into `documents.extraction_reason`: the code, with the
    /// detail appended when there is one. The interface translates the
    /// code and shows the tail as-is.
    pub fn stored_reason(&self) -> String {
        match &self.detail {
            Some(d) => format!("{}: {d}", self.kind),
            None => self.kind.to_string(),
        }
    }

    /// Classifies the parser's message.
    ///
    /// Matching on message text is fragile by nature, so anything
    /// unrecognised becomes `read_failed` *with the original text
    /// attached* rather than a tidier code that would lose it.
    pub fn from_parser_error(err: &anyhow::Error) -> Self {
        let raw = format!("{err:#}");
        let lower = raw.to_lowercase();

        if lower.contains("password") || lower.contains("encrypt") {
            return Self {
                kind: failure::ENCRYPTED_PDF,
                detail: None,
            };
        }
        if lower.contains("formato non supportato") || lower.contains("unsupported") {
            return Self {
                kind: failure::UNSUPPORTED_FORMAT,
                detail: None,
            };
        }
        if lower.contains("not a valid docx")
            || lower.contains("conversione office fallita")
            || lower.contains("malformed")
        {
            return Self {
                kind: failure::CORRUPT_FILE,
                detail: None,
            };
        }
        Self {
            kind: failure::READ_FAILED,
            detail: Some(raw),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pageindex_rs::model::InputFormat;

    #[test]
    fn tags_are_the_canonical_status_values() {
        assert_eq!(ExtractionOutcome::Ready.tag(), "ready");
        assert_eq!(
            ExtractionOutcome::NoText {
                code: no_text::EMPTY_FILE
            }
            .tag(),
            "no_text"
        );
    }

    #[test]
    fn ready_has_nothing_to_explain() {
        assert!(ExtractionOutcome::Ready.code().is_none());
    }

    #[test]
    fn a_pdf_without_a_text_layer_is_reported_as_a_scan() {
        // The one code with an actionable answer for the user.
        assert_eq!(no_text_code(&InputFormat::Pdf), no_text::SCANNED_PDF);
    }

    #[test]
    fn each_format_family_has_its_own_code() {
        assert_eq!(no_text_code(&InputFormat::Docx), no_text::IMAGE_ONLY);
        assert_eq!(
            no_text_code(&InputFormat::Office("rtf")),
            no_text::OFFICE_NO_TEXT
        );
        assert_eq!(no_text_code(&InputFormat::Text), no_text::EMPTY_FILE);
        assert_eq!(no_text_code(&InputFormat::Markdown), no_text::EMPTY_FILE);
    }

    #[test]
    fn a_protected_pdf_is_recognised() {
        let err = IngestError::from_parser_error(&anyhow::anyhow!(
            "pdfium load error: PdfiumLibraryInternalError(PasswordError)"
        ));
        assert_eq!(err.kind, failure::ENCRYPTED_PDF);
        assert_eq!(err.stored_reason(), "encrypted_pdf");
    }

    #[test]
    fn unsupported_and_corrupt_are_distinguished() {
        assert_eq!(
            IngestError::from_parser_error(&anyhow::anyhow!("formato non supportato: a.zip")).kind,
            failure::UNSUPPORTED_FORMAT
        );
        assert_eq!(
            IngestError::from_parser_error(&anyhow::anyhow!("Not a valid DOCX")).kind,
            failure::CORRUPT_FILE
        );
        assert_eq!(
            IngestError::from_parser_error(&anyhow::anyhow!("malformed RTF: unexpected token"))
                .kind,
            failure::CORRUPT_FILE
        );
    }

    #[test]
    fn an_unknown_failure_keeps_the_original_message() {
        let err = IngestError::from_parser_error(&anyhow::anyhow!("qualcosa di inatteso 0x8007"));
        assert_eq!(err.kind, failure::READ_FAILED);
        let stored = err.stored_reason();
        assert!(stored.starts_with("read_failed: "), "{stored}");
        assert!(stored.contains("0x8007"), "{stored}");
    }
}
