// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
//! Document onboarding: the single funnel every file passes through
//! before anything else in Cove Studio looks at it.
//!
//! This module is a **boundary**. It is the only place that knows about
//! `pageindex-rs`: it calls the library, translates its types into ours
//! and maps its configuration (environment variables named
//! `PAGEINDEX_*`, model directories relative to the working directory)
//! onto Cove Studio's own conventions (`COVE_*`, `cove-studio-data/`).
//! Everything else — the upload route, the folder scanner, the chat
//! attachment loader, the LLM tools — talks to `ingest`, never to the
//! library. Swapping the extractor later costs one file.
//!
//! What the funnel replaces: three separate extractors, keyed on
//! different things (file extension in the scanner, `file_type` in the
//! chat loader, extension again in the tools), each failing
//! differently — one returned a skip reason, one `.ok()`-ed the error
//! away, one `unwrap_or_default()`-ed it into an empty string. A
//! document that could not be read reached the model as silence, and
//! the row still said `ready`.
//!
//! What it adds beyond unification:
//!
//! * the **defaced pre-check** — a PDF whose text layer does not match
//!   what is printed (tampered `cmap`/`ToUnicode`, PUA or zero-width
//!   characters). For legal documents that is not a curiosity: the
//!   extracted text can say something the reader never sees;
//! * **Office and legacy formats** (`.doc`, `.ppt`, `.rtf`,
//!   spreadsheets) converted to Markdown, so `.doc` and `.ppt` stop
//!   being silently unsupported;
//! * a **section tree with page numbers**, from which we render the
//!   `[Page N]` markers the citation machinery relies on — derived from
//!   structure now, rather than pasted onto the text;
//! * an honest **outcome**: readable, readable-but-empty, or failed
//!   with a reason, which the interface and the prompt can both state.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

pub mod outcome;

pub use outcome::{ExtractionOutcome, IngestError};

/// One section of a document, as the parser found it: a heading with
/// its body. `level` is the heading depth (1 = top), `page` the 1-based
/// page the section starts on when the format has pages.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub level: u8,
    pub title: String,
    pub text: String,
    pub page: Option<u32>,
}

/// A document after onboarding: the text every consumer reads, plus the
/// structure and the verdict.
#[derive(Debug, Clone)]
pub struct IngestedDoc {
    /// Full text with `[Page N]` markers, ready for the prompt, the
    /// tools and the chunker. Empty when `outcome` is not `Ready`.
    pub text: String,
    /// The section tree, flattened in reading order. Empty for formats
    /// without headings.
    pub sections: Vec<Section>,
    /// Document title when the format declares one.
    pub title: Option<String>,
    /// Page count for paged formats.
    pub page_count: Option<u32>,
    /// The real format the parser detected, as a short tag (`pdf`,
    /// `docx`, `rtf`, …). For converted formats this is the *original*
    /// one, not Markdown.
    pub format: String,
    /// Whether the text is usable, and why not when it is not.
    pub outcome: ExtractionOutcome,
}

impl IngestedDoc {
    /// Text usable by a model: `Ready` with something in it.
    pub fn has_text(&self) -> bool {
        matches!(self.outcome, ExtractionOutcome::Ready) && !self.text.trim().is_empty()
    }
}

/// Marker the rest of the codebase matches on to attribute a page to a
/// chunk or a citation. Kept identical to what the previous extractors
/// emitted, so stored chunks and existing citations keep resolving.
pub fn page_marker(page: u32) -> String {
    format!("[Page {page}]")
}

/// Renders the parser's sections into one text, inserting a page marker
/// whenever the page changes.
///
/// Headings are kept as plain lines rather than Markdown hashes: the
/// text goes to a model as context, and `## ` prefixes invite it to
/// answer in Markdown-of-Markdown. The structure is preserved
/// separately in `sections`.
pub fn render_text(sections: &[Section]) -> String {
    let mut out = String::new();
    let mut current_page: Option<u32> = None;
    for section in sections {
        if let Some(page) = section.page {
            if current_page != Some(page) {
                if !out.is_empty() {
                    out.push_str("\n\n");
                }
                out.push_str(&page_marker(page));
                out.push('\n');
                current_page = Some(page);
            }
        }
        let title = section.title.trim();
        // The parser names page-fallback sections "Pagina N"; with the
        // marker already on the line above, repeating it is noise.
        let title_is_page_echo = current_page
            .map(|p| title == format!("Pagina {p}"))
            .unwrap_or(false);
        if !title.is_empty() && !title_is_page_echo {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push_str("\n\n");
            }
            out.push_str(title);
            out.push('\n');
        }
        let body = section.text.trim();
        if !body.is_empty() {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(body);
            out.push('\n');
        }
    }
    out.trim().to_string()
}

/// Onboards a file from disk.
///
/// The library's parser is synchronous and CPU-bound (a 500-page PDF
/// takes seconds), so the whole call happens on a blocking thread.
pub async fn ingest_path(path: &Path) -> Result<IngestedDoc> {
    let owned: PathBuf = path.to_path_buf();
    tokio::task::spawn_blocking(move || ingest_blocking(&owned))
        .await
        .map_err(|e| anyhow!("ingest task join: {e:?}"))?
}

/// Synchronous form, for callers already on a blocking thread (the
/// folder scanner walks thousands of files on one).
pub fn ingest_blocking(path: &Path) -> Result<IngestedDoc> {
    let started_at = std::time::Instant::now();
    match pageindex_rs::parser::parse_file(path) {
        Ok((format, parsed)) => {
            let sections: Vec<Section> = parsed
                .sections
                .into_iter()
                .map(|s| Section {
                    level: s.level,
                    title: s.title,
                    text: s.body,
                    page: s.page,
                })
                .collect();
            let text = render_text(&sections);
            // The verdict is about the *body*, not the rendered string:
            // the parser gives an empty file a synthetic heading
            // ("Introduzione"), which would otherwise count as content
            // and put us back to answering questions about a document
            // that says nothing.
            let has_body = if sections.is_empty() {
                !text.trim().is_empty()
            } else {
                sections.iter().any(|s| !s.text.trim().is_empty())
            };
            let outcome = if !has_body {
                ExtractionOutcome::NoText {
                    reason: outcome::no_text_reason(&format),
                }
            } else {
                ExtractionOutcome::Ready
            };
            let doc = IngestedDoc {
                text,
                sections,
                title: parsed.title,
                page_count: parsed.page_count,
                format: format_tag(&format, path),
                outcome,
            };
            tracing::info!(
                "[ingest] {} → format={} sections={} pages={:?} chars={} outcome={} in {:?}",
                path.display(),
                doc.format,
                doc.sections.len(),
                doc.page_count,
                doc.text.len(),
                doc.outcome.tag(),
                started_at.elapsed(),
            );
            Ok(doc)
        }
        Err(e) => {
            // A parser error is not a panic and not silence: it is a
            // document the user attached and we cannot read. The caller
            // records the reason and the interface shows it.
            let err = IngestError::from_parser_error(&e);
            tracing::warn!(
                "[ingest] {} failed after {:?}: {}",
                path.display(),
                started_at.elapsed(),
                err.reason
            );
            Err(anyhow!(err))
        }
    }
}

/// Short, stable tag for a detected format, in **our** vocabulary.
///
/// The library groups the Office family coarsely — every spreadsheet
/// comes back as `sheet`, whatever its extension. Cove Studio stores
/// the precise type in `documents.file_type` and branches on it (the
/// spreadsheet viewer, the download name, the tabular workflows), so
/// for converted formats we keep the file's own extension and use the
/// library's tag only when there is none.
fn format_tag(format: &pageindex_rs::model::InputFormat, path: &Path) -> String {
    use pageindex_rs::model::InputFormat;
    match format {
        InputFormat::Markdown => "md".to_string(),
        InputFormat::Text => "txt".to_string(),
        InputFormat::Docx => "docx".to_string(),
        InputFormat::Pdf => "pdf".to_string(),
        InputFormat::Office(tag) => path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_else(|| (*tag).to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(level: u8, title: &str, text: &str, page: Option<u32>) -> Section {
        Section {
            level,
            title: title.to_string(),
            text: text.to_string(),
            page,
        }
    }

    #[test]
    fn office_formats_keep_their_own_extension() {
        use pageindex_rs::model::InputFormat;
        // The library says `sheet` for every spreadsheet; our schema
        // and our viewer need to know which one it was.
        assert_eq!(
            format_tag(&InputFormat::Office("sheet"), Path::new("prospetto.xlsx")),
            "xlsx"
        );
        assert_eq!(
            format_tag(&InputFormat::Office("sheet"), Path::new("vecchio.XLS")),
            "xls"
        );
        assert_eq!(
            format_tag(&InputFormat::Office("rtf"), Path::new("verbale.rtf")),
            "rtf"
        );
        // No extension: the library's tag is all we have.
        assert_eq!(
            format_tag(&InputFormat::Office("doc"), Path::new("senza-estensione")),
            "doc"
        );
    }

    #[test]
    fn page_markers_appear_once_per_page_change() {
        let out = render_text(&[
            section(1, "Premesse", "Prima riga.", Some(1)),
            section(2, "Oggetto", "Seconda riga.", Some(1)),
            section(2, "Durata", "Terza riga.", Some(2)),
        ]);
        assert_eq!(out.matches("[Page 1]").count(), 1, "{out}");
        assert_eq!(out.matches("[Page 2]").count(), 1, "{out}");
        // The marker must precede the text it refers to, or citations
        // land one page early.
        let first = out.find("[Page 1]").unwrap();
        let second = out.find("[Page 2]").unwrap();
        assert!(first < out.find("Prima riga.").unwrap());
        assert!(second > out.find("Seconda riga.").unwrap());
        assert!(second < out.find("Terza riga.").unwrap());
    }

    #[test]
    fn headings_are_kept_as_plain_lines() {
        let out = render_text(&[section(1, "Premesse", "Testo.", None)]);
        assert_eq!(out, "Premesse\nTesto.");
        assert!(!out.contains('#'), "no Markdown hashes: {out}");
    }

    #[test]
    fn page_echo_titles_are_not_repeated() {
        // The PDF fallback names its sections "Pagina N"; with the
        // marker on the line above it would read twice.
        let out = render_text(&[section(1, "Pagina 3", "Contenuto.", Some(3))]);
        assert_eq!(out, "[Page 3]\nContenuto.");
    }

    #[test]
    fn a_section_without_a_body_keeps_its_heading() {
        // An empty chapter is information: the document has that
        // heading and nothing under it.
        let out = render_text(&[
            section(1, "Allegati", "", Some(9)),
            section(2, "Allegato A", "Testo.", Some(9)),
        ]);
        assert!(out.contains("Allegati"), "{out}");
        assert!(out.contains("Allegato A"), "{out}");
    }

    #[test]
    fn a_heading_without_any_body_is_not_text() {
        // Mirrors `ingest_blocking`'s rule; asserted here on the same
        // predicate the caller uses.
        let sections = vec![section(1, "Introduzione", "", None)];
        let has_body = sections.iter().any(|s| !s.text.trim().is_empty());
        assert!(!has_body, "a lone heading must not count as content");
        // …while the heading itself is still rendered, for the reader.
        assert_eq!(render_text(&sections), "Introduzione");
    }

    #[test]
    fn no_sections_renders_nothing() {
        assert_eq!(render_text(&[]), "");
    }

    #[test]
    fn whitespace_only_bodies_do_not_produce_text() {
        let out = render_text(&[section(1, "   ", "  \n \t ", None)]);
        assert_eq!(out, "");
    }

    #[test]
    fn has_text_requires_both_a_ready_outcome_and_content() {
        let ready = IngestedDoc {
            text: "Testo".into(),
            sections: vec![],
            title: None,
            page_count: None,
            format: "txt".into(),
            outcome: ExtractionOutcome::Ready,
        };
        assert!(ready.has_text());

        let empty = IngestedDoc {
            text: "   ".into(),
            ..ready.clone()
        };
        assert!(!empty.has_text());

        let no_text = IngestedDoc {
            outcome: ExtractionOutcome::NoText {
                reason: "scansione".into(),
            },
            ..ready.clone()
        };
        assert!(!no_text.has_text());
    }
}
