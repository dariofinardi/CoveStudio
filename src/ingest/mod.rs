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
    /// Something the reader should know about text that *was*
    /// extracted; see `outcome::warning`. Today: a text layer that
    /// disagrees with what is printed.
    pub warning: Option<&'static str>,
}

impl IngestedDoc {
    /// Text usable by a model: `Ready` with something in it.
    pub fn has_text(&self) -> bool {
        matches!(self.outcome, ExtractionOutcome::Ready) && !self.text.trim().is_empty()
    }
}

/// Titles the library supplies itself. None of them is something the
/// document says, so none may count as content or be rendered as a
/// heading.
///
/// The second one is the important one: a PDF with no text layer comes
/// back as a single section titled "PDF (nessun testo estraibile)" with
/// an empty body. Counting that as a heading declared a scan
/// **readable** and handed the model that sentence as though it were
/// the document's content — the exact defect this funnel exists to
/// remove, in a new hiding place.
const SYNTHETIC_TITLES: &[&str] = &["Introduzione", "PDF (nessun testo estraibile)"];

/// Title the library gives to the text before the first heading.
const SYNTHETIC_INTRO_TITLE: &str = "Introduzione";

/// Formats Cove Studio reads as plain text and the library does not
/// know at all. `.csv` is the case that matters: the library's format
/// detector has no entry for it, so it would come back "unsupported",
/// while here a CSV is a first-class document — the tabular workflows
/// parse the rows themselves, and the assistant reads them as text.
///
/// TODO (upstream, `pageindex-rs`): add `"csv" => InputFormat::Text`
/// to `detect_format` in `pageindex-rs/src/parser/mod.rs` — a CSV is
/// plain text for parsing purposes, and every consumer of that library
/// gains it. Once that lands and the pinned revision moves, this
/// constant and `plain_text_doc` below can go, and the branch at the
/// top of `ingest_blocking` with them.
const PLAIN_TEXT_ONLY: &[&str] = &["csv"];

/// Runs the tamper check on a document that produced text.
///
/// The library runs the same check internally but only logs it: the
/// result never reaches `ParseResult`, so a caller cannot act on it.
/// Until it does (reported upstream), we ask `chk_defaced` ourselves —
/// the same crate, already in the dependency graph. The second pass
/// costs a read of the file; for the one case it catches, that is a
/// trade worth making.
fn defaced_warning(path: &Path) -> Option<&'static str> {
    use chk_defaced::finding::Severity;
    let report = match chk_defaced::scan::scan_path(path, None) {
        Ok(r) => r,
        // Not every format is supported, and a checker failure must
        // never cost us a document we could otherwise read.
        Err(e) => {
            tracing::debug!("[ingest] tamper check skipped for {}: {e}", path.display());
            return None;
        }
    };
    let serious = report
        .findings
        .iter()
        .filter(|f| f.severity >= Severity::High)
        .count();
    if serious == 0 {
        return None;
    }
    tracing::warn!(
        "[ingest] {}: {serious} serious finding(s) — the extracted text may differ from          what is printed",
        path.display()
    );
    Some(outcome::warning::TEXT_LAYER_UNRELIABLE)
}

/// Whether a section title is something the document carries, as
/// opposed to a placeholder the parser supplied: the synthetic
/// "Introduzione", or the file's own name (which is what an empty file
/// comes back with).
fn is_real_heading(title: &str, path: &Path) -> bool {
    let title = title.trim();
    if title.is_empty() || SYNTHETIC_TITLES.contains(&title) {
        return false;
    }
    // Only the name *with its extension* counts as the parser's echo.
    // Comparing the stem too would reject the most ordinary case there
    // is — `scadenze.md` whose first heading is "Scadenze" — and
    // declare a perfectly readable note unreadable.
    let echoes_file_name = path
        .file_name()
        .and_then(|p| p.to_str())
        .map(|p| p.eq_ignore_ascii_case(title))
        .unwrap_or(false);
    !echoes_file_name
}

/// Reads a file as plain UTF-8 text, as one section with no heading.
/// Invalid bytes become U+FFFD rather than an error: a spreadsheet
/// exported in a legacy code page is still worth reading.
fn plain_text_doc(path: &Path, format: &str) -> Result<IngestedDoc> {
    let bytes = std::fs::read(path)
        .map_err(|e| anyhow!(IngestError::from_parser_error(&anyhow!("{e}"))).context(e))?;
    let text = String::from_utf8_lossy(&bytes).trim().to_string();
    let outcome = if text.is_empty() {
        ExtractionOutcome::NoText {
            code: outcome::no_text::EMPTY_FILE,
        }
    } else {
        ExtractionOutcome::Ready
    };
    Ok(IngestedDoc {
        sections: vec![Section {
            level: 1,
            title: String::new(),
            text: text.clone(),
            page: None,
        }],
        text,
        title: None,
        page_count: None,
        format: format.to_string(),
        outcome,
        // Plain text has no font tables to tamper with.
        warning: None,
    })
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
        // For a plain-text file the parser takes the first line as the
        // section title and keeps it in the body. Printing both would
        // duplicate that line, and a model reading a duplicated opening
        // line has to wonder whether the document really says it twice.
        // "Introduzione" is the parser's placeholder for text that
        // precedes the first heading — a structural artefact, not
        // something the document says. It stays in `sections`, where
        // the structure must be faithful, but printing it would put a
        // heading the author never wrote in front of a CSV or a plain
        // note.
        let title_is_synthetic_intro = SYNTHETIC_TITLES.contains(&title);
        let title_opens_the_body = !title.is_empty()
            && section
                .text
                .trim_start()
                .lines()
                .next()
                .map(|first| first.trim() == title)
                .unwrap_or(false);
        if !title.is_empty()
            && !title_is_page_echo
            && !title_opens_the_body
            && !title_is_synthetic_intro
        {
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

/// Onboards content that is not (or may not be) on disk under the name
/// that matters: the storage layer keys blobs by hash, and the
/// rejection-summary path has only bytes plus a file type.
///
/// The extension drives format detection, so the bytes are written to
/// a temporary file that *carries* it. `filename_hint` may be a bare
/// name (`blob.docx`) or a full path; only its extension is used.
pub fn ingest_bytes(filename_hint: &str, bytes: &[u8]) -> Result<IngestedDoc> {
    let ext = Path::new(filename_hint)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin")
        .to_ascii_lowercase();
    let dir = std::env::temp_dir().join(crate::product::SLUG).join("ingest");
    std::fs::create_dir_all(&dir)
        .map_err(|e| anyhow!("creating the ingest scratch dir {}: {e}", dir.display()))?;
    // The name has to be unique: two turns can onboard two different
    // documents of the same type at the same moment.
    let path = dir.join(format!("{}.{ext}", uuid::Uuid::new_v4()));
    std::fs::write(&path, bytes)
        .map_err(|e| anyhow!("writing the ingest scratch file {}: {e}", path.display()))?;
    let result = ingest_blocking(&path);
    // Best effort: a leftover in the OS temp directory is noise, not a
    // failure worth surfacing over the document itself.
    let _ = std::fs::remove_file(&path);
    result
}

/// Onboards a file that may or may not be readable at `path`, with the
/// bytes as the fallback.
///
/// Several callers hold both: the folder scanner has already read the
/// file to hash it, the tabular reader has a real path *and* the bytes,
/// and the summariser has only bytes plus a synthetic name. Preferring
/// the path matters for PDFs — pdfium wants an on-disk file — while the
/// bytes keep the blob-only callers working.
pub fn ingest_path_or_bytes(path: &Path, bytes: &[u8]) -> Result<IngestedDoc> {
    if path.is_file() {
        return ingest_blocking(path);
    }
    ingest_bytes(&path.to_string_lossy(), bytes)
}

/// Synchronous form, for callers already on a blocking thread (the
/// folder scanner walks thousands of files on one).
pub fn ingest_blocking(path: &Path) -> Result<IngestedDoc> {
    let started_at = std::time::Instant::now();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if PLAIN_TEXT_ONLY.contains(&ext.as_str()) {
        let doc = plain_text_doc(path, &ext)?;
        tracing::info!(
            "[ingest] {} → format={} chars={} outcome={} in {:?} (plain text)",
            path.display(),
            doc.format,
            doc.text.len(),
            doc.outcome.tag(),
            started_at.elapsed(),
        );
        return Ok(doc);
    }
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
            // What counts as content: any section body, or any heading
            // the document really carries. Both ends of that matter.
            // A note whose whole content is a heading ("# Scadenze")
            // does say something. But the parser also invents headings:
            // "Introduzione" for text before the first one, and — for a
            // file with nothing in it — the *file's own name*. Neither
            // is content, and counting them would bring back the defect
            // this whole funnel exists to remove: a document reported as
            // readable with nothing to read.
            let has_body = if sections.is_empty() {
                !text.trim().is_empty()
            } else {
                sections
                    .iter()
                    .any(|s| !s.text.trim().is_empty() || is_real_heading(&s.title, path))
            };
            let outcome = if !has_body {
                ExtractionOutcome::NoText {
                    code: outcome::no_text_code(&format),
                }
            } else {
                ExtractionOutcome::Ready
            };
            // Only worth asking when there is text to distrust, and
            // only for the formats the checker understands.
            let warning = if matches!(outcome, ExtractionOutcome::Ready) {
                defaced_warning(path)
            } else {
                None
            };
            let doc = IngestedDoc {
                text,
                sections,
                title: parsed.title,
                page_count: parsed.page_count,
                format: format_tag(&format, path),
                outcome,
                warning,
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
                err
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
    fn a_title_the_body_already_opens_with_is_not_repeated() {
        // Plain text: the parser makes the first line the title and
        // leaves it in the body.
        let out = render_text(&[section(1, "hello", "hello
world", None)]);
        assert_eq!(out, "hello
world");
    }

    #[test]
    fn a_title_the_body_does_not_repeat_is_kept() {
        let out = render_text(&[section(1, "Titolo", "corpo", None)]);
        assert_eq!(out, "Titolo
corpo");
    }

    #[test]
    fn a_real_heading_is_content_but_a_placeholder_is_not() {
        let p = Path::new("C:/docs/scadenze.md");
        // A note whose whole content is "# Scadenze" says something.
        assert!(is_real_heading("Scadenze", p));
        // The parser's placeholders: the intro heading, and the one a
        // PDF with no text layer comes back with.
        assert!(!is_real_heading(SYNTHETIC_INTRO_TITLE, p));
        assert!(!is_real_heading("PDF (nessun testo estraibile)", p));
        // What an empty file comes back with: its own name, extension
        // included, whatever the case.
        assert!(!is_real_heading("scadenze.md", p));
        assert!(!is_real_heading("SCADENZE.MD", p));
        assert!(!is_real_heading("   ", p));
        // The stem alone is NOT a placeholder: a note named
        // `scadenze.md` opening with "# Scadenze" is the ordinary case,
        // and calling it unreadable would be a worse bug than the one
        // this rule fixes.
        assert!(is_real_heading("scadenze", p));
    }

    #[test]
    fn the_synthetic_intro_heading_is_not_rendered() {
        // A CSV or a plain note has no headings; the parser wraps the
        // whole content under "Introduzione". That word must not reach
        // the model as if the author had written it.
        let out = render_text(&[section(1, "Introduzione", "riga1
riga2", None)]);
        assert_eq!(out, "riga1
riga2");
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
        let sections = vec![section(1, "Allegati", "", None)];
        let has_body = sections.iter().any(|s| !s.text.trim().is_empty());
        assert!(!has_body, "a lone heading must not count as content");
        // …while a heading the document really carries is still
        // rendered, for the reader.
        assert_eq!(render_text(&sections), "Allegati");
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
            warning: None,
        };
        assert!(ready.has_text());

        let empty = IngestedDoc {
            text: "   ".into(),
            ..ready.clone()
        };
        assert!(!empty.has_text());

        let no_text = IngestedDoc {
            outcome: ExtractionOutcome::NoText {
                code: outcome::no_text::SCANNED_PDF,
            },
            ..ready.clone()
        };
        assert!(!no_text.has_text());
    }
}
