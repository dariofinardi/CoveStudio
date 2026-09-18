// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
//! The verdict of onboarding a document, and the words it is told in.
//!
//! This exists because the previous behaviour was to say nothing. A
//! password-protected PDF, an image-only DOCX, an unsupported format:
//! the row said `ready`, the model received an empty block, and the
//! answer came back confident about a document nobody had read. The
//! outcome travels with the document so three places can be honest
//! about it — the attachment list, the prompt, and the logs.

use std::fmt;

/// Whether a document's text is usable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractionOutcome {
    /// Text was extracted and is not empty.
    Ready,
    /// The file was read but carries no text a model can use: a scan
    /// with no text layer, an image-only document, an empty file.
    /// `reason` is user-facing.
    NoText { reason: String },
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

    /// The explanation, when there is one.
    pub fn reason(&self) -> Option<&str> {
        match self {
            ExtractionOutcome::Ready => None,
            ExtractionOutcome::NoText { reason } => Some(reason),
        }
    }
}

/// Why a document that parsed carries no usable text. Phrased for the
/// person who attached it, not for the log.
pub fn no_text_reason(format: &pageindex_rs::model::InputFormat) -> String {
    use pageindex_rs::model::InputFormat;
    match format {
        // The most common case by far, and the one with an actionable
        // answer: turn on OCR, or use a model that reads images.
        InputFormat::Pdf => {
            "il PDF non contiene testo selezionabile: è una scansione o un'immagine".to_string()
        }
        InputFormat::Docx => {
            "il documento non contiene testo: probabilmente solo immagini".to_string()
        }
        InputFormat::Office(tag) => {
            format!("il file {tag} non contiene testo estraibile")
        }
        InputFormat::Markdown | InputFormat::Text => "il file è vuoto".to_string(),
    }
}

/// A document we could not read at all, with a reason fit to show.
#[derive(Debug, Clone)]
pub struct IngestError {
    /// User-facing sentence.
    pub reason: String,
    /// Canonical identifier for the row and for matching in code.
    pub kind: &'static str,
}

impl fmt::Display for IngestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.reason)
    }
}

impl std::error::Error for IngestError {}

impl IngestError {
    /// Turns the parser's message into something a person can act on.
    ///
    /// The library reports in Italian and with technical detail
    /// (`pdfium load error: …`). Matching on it is admittedly fragile,
    /// so the fallback keeps the original text rather than inventing a
    /// tidy lie: an unrecognised failure the user can paste into a bug
    /// report beats "an error occurred".
    pub fn from_parser_error(err: &anyhow::Error) -> Self {
        let raw = format!("{err:#}");
        let lower = raw.to_lowercase();

        if lower.contains("password") || lower.contains("encrypt") {
            return Self {
                reason: "il PDF è protetto da password: rimuovi la protezione e ricarica il file"
                    .to_string(),
                kind: "encrypted",
            };
        }
        if lower.contains("formato non supportato") || lower.contains("unsupported") {
            return Self {
                reason: "formato non supportato".to_string(),
                kind: "unsupported_format",
            };
        }
        if lower.contains("not a valid docx") || lower.contains("conversione office fallita") {
            return Self {
                reason: "il file è danneggiato o non è del formato che l'estensione dichiara"
                    .to_string(),
                kind: "corrupt",
            };
        }
        Self {
            reason: format!("lettura non riuscita: {raw}"),
            kind: "failed",
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
                reason: "x".into()
            }
            .tag(),
            "no_text"
        );
    }

    #[test]
    fn ready_has_nothing_to_explain() {
        assert!(ExtractionOutcome::Ready.reason().is_none());
    }

    #[test]
    fn a_scanned_pdf_gets_the_actionable_reason() {
        let reason = no_text_reason(&InputFormat::Pdf);
        assert!(reason.contains("scansione"), "{reason}");
    }

    #[test]
    fn office_reason_names_the_original_format() {
        let reason = no_text_reason(&InputFormat::Office("rtf"));
        assert!(reason.contains("rtf"), "{reason}");
    }

    #[test]
    fn a_protected_pdf_is_recognised_and_told_what_to_do() {
        let err = IngestError::from_parser_error(&anyhow::anyhow!(
            "pdfium load error: PdfiumLibraryInternalError(PasswordError)"
        ));
        assert_eq!(err.kind, "encrypted");
        assert!(err.reason.contains("password"), "{}", err.reason);
    }

    #[test]
    fn an_unknown_failure_keeps_the_original_message() {
        // Better a technical sentence the user can paste into a report
        // than a tidy message that hides which file broke and how.
        let err = IngestError::from_parser_error(&anyhow::anyhow!("qualcosa di inatteso 0x8007"));
        assert_eq!(err.kind, "failed");
        assert!(err.reason.contains("0x8007"), "{}", err.reason);
    }

    #[test]
    fn unsupported_and_corrupt_are_distinguished() {
        let unsupported =
            IngestError::from_parser_error(&anyhow::anyhow!("formato non supportato: a.zip"));
        assert_eq!(unsupported.kind, "unsupported_format");

        let corrupt = IngestError::from_parser_error(&anyhow::anyhow!("Not a valid DOCX"));
        assert_eq!(corrupt.kind, "corrupt");
    }
}
