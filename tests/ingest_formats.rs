//! Functional tests of the onboarding funnel against **real files**.
//!
//! The unit tests in `src/ingest` cover the rendering rules on
//! synthetic sections. These answer the question those cannot: does the
//! extractor actually read the formats users bring, and does it say so
//! honestly when it cannot.
//!
//! Fixtures are the PDFs and text files already in `tests/`, plus files
//! this suite writes itself (an empty one, a corrupt one, an
//! unsupported one, a DOCX). Nothing here needs a model, so it runs on
//! a plain `cargo test`.

use cove_studio::ingest::{self, ExtractionOutcome};
use std::io::Write;
use std::path::{Path, PathBuf};

/// A real, text-layer PDF that ships with the repository.
fn sample_pdf() -> PathBuf {
    Path::new("tests/medical/CARTELLA_TEST_001.pdf").to_path_buf()
}

fn write_temp(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join("cove-ingest-tests");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join(name);
    let mut f = std::fs::File::create(&path).expect("create temp file");
    f.write_all(bytes).expect("write temp file");
    path
}

#[tokio::test]
async fn reads_a_pdf_with_a_text_layer() {
    let doc = ingest::ingest_path(&sample_pdf())
        .await
        .expect("a text PDF must parse");

    assert_eq!(doc.format, "pdf");
    assert_eq!(doc.outcome, ExtractionOutcome::Ready);
    assert!(doc.has_text(), "no text extracted");
    assert!(
        doc.page_count.unwrap_or(0) >= 1,
        "page count missing: {:?}",
        doc.page_count
    );
    // Page markers are what the citation machinery resolves against.
    assert!(
        doc.text.contains("[Page 1]"),
        "first page marker missing: {}",
        &doc.text[..doc.text.len().min(200)]
    );
    assert!(!doc.sections.is_empty(), "no sections");
}

#[tokio::test]
async fn page_markers_are_monotonic_and_unique() {
    // A marker emitted twice, or out of order, sends a citation to the
    // wrong page — and nothing else in the system would notice.
    let doc = ingest::ingest_path(&sample_pdf()).await.expect("parse");

    let mut seen: Vec<u32> = Vec::new();
    for line in doc.text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("[Page ") {
            if let Some(num) = rest.strip_suffix(']') {
                let n: u32 = num.parse().expect("page marker must hold a number");
                assert!(
                    !seen.contains(&n),
                    "page {n} marked twice; markers so far: {seen:?}"
                );
                if let Some(last) = seen.last() {
                    assert!(n > *last, "markers out of order: {last} then {n}");
                }
                seen.push(n);
            }
        }
    }
    assert!(!seen.is_empty(), "no page markers at all");
    assert_eq!(seen[0], 1, "numbering must start at page 1, got {:?}", seen);
}

#[tokio::test]
async fn reads_a_plain_text_file() {
    let doc = ingest::ingest_path(Path::new("tests/docs insurance/doc1.txt"))
        .await
        .expect("txt must parse");
    assert_eq!(doc.format, "txt");
    assert!(doc.has_text());
    // No pages, so no markers: their absence is part of the contract
    // (`document_segments` falls back to parts for such formats).
    assert!(!doc.text.contains("[Page "), "text files have no pages");
}

#[tokio::test]
async fn reads_markdown_and_keeps_its_structure() {
    let path = write_temp(
        "struttura.md",
        b"# Contratto\n\nPremessa.\n\n## Oggetto\n\nIl fornitore consegna.\n\n## Durata\n\nDodici mesi.\n",
    );
    let doc = ingest::ingest_path(&path).await.expect("md must parse");

    assert_eq!(doc.format, "md");
    assert!(doc.has_text());
    assert!(
        doc.sections.len() >= 3,
        "expected the headings as sections, got {:?}",
        doc.sections.iter().map(|s| &s.title).collect::<Vec<_>>()
    );
    let titles: Vec<&str> = doc.sections.iter().map(|s| s.title.as_str()).collect();
    assert!(titles.iter().any(|t| t.contains("Oggetto")), "{titles:?}");
    assert!(titles.iter().any(|t| t.contains("Durata")), "{titles:?}");
    // Heading levels must survive: they become the document tree.
    assert!(
        doc.sections.iter().any(|s| s.level >= 2),
        "heading depth was flattened"
    );
    // The body text must reach the model without Markdown hashes.
    assert!(doc.text.contains("Il fornitore consegna."));
    assert!(!doc.text.contains("## "), "{}", doc.text);
}

#[tokio::test]
async fn an_empty_file_is_reported_as_having_no_text() {
    // This is the case that used to be stored as `ready` with an empty
    // body: the model received silence and answered anyway.
    let path = write_temp("vuoto.txt", b"");
    let doc = ingest::ingest_path(&path).await.expect("an empty file parses");

    assert!(!doc.has_text());
    assert_eq!(doc.outcome.tag(), "no_text");
    assert!(
        doc.outcome.reason().is_some_and(|r| !r.is_empty()),
        "a verdict without a reason is no better than silence"
    );
}

#[tokio::test]
async fn an_unsupported_format_fails_with_a_reason() {
    let path = write_temp("archivio.zip", b"PK\x03\x04not-really-a-zip");
    let err = ingest::ingest_path(&path)
        .await
        .expect_err("an unsupported format must not be silently accepted");

    let ingest_err = err
        .downcast_ref::<ingest::IngestError>()
        .expect("the error must carry our own kind and reason");
    assert_eq!(ingest_err.kind, "unsupported_format");
    assert!(!ingest_err.reason.is_empty());
}

#[tokio::test]
async fn a_corrupt_docx_fails_with_a_reason_rather_than_empty_text() {
    // A file whose extension lies. The old tools path turned this into
    // `unwrap_or_default()` — an empty string with no trace.
    let path = write_temp("finto.docx", b"this is not a zip container at all");
    let err = ingest::ingest_path(&path)
        .await
        .expect_err("a corrupt docx must fail");

    let ingest_err = err
        .downcast_ref::<ingest::IngestError>()
        .expect("typed error");
    assert!(
        matches!(ingest_err.kind, "corrupt" | "failed"),
        "unexpected kind: {}",
        ingest_err.kind
    );
    assert!(!ingest_err.reason.is_empty());
}

#[tokio::test]
async fn a_missing_file_fails_and_does_not_panic() {
    let err = ingest::ingest_path(Path::new("tests/does-not-exist.pdf"))
        .await
        .expect_err("a missing file must fail");
    assert!(!format!("{err:#}").is_empty());
}

#[tokio::test]
async fn the_blocking_form_agrees_with_the_async_one() {
    // The folder scanner walks thousands of files on a blocking thread
    // and calls the sync form; the two must not drift.
    let path = sample_pdf();
    let via_async = ingest::ingest_path(&path).await.expect("async");
    let via_blocking = tokio::task::spawn_blocking(move || ingest::ingest_blocking(&path))
        .await
        .expect("join")
        .expect("blocking");

    assert_eq!(via_async.text, via_blocking.text);
    assert_eq!(via_async.sections.len(), via_blocking.sections.len());
    assert_eq!(via_async.outcome, via_blocking.outcome);
}

#[tokio::test]
async fn reads_a_real_docx() {
    // Generated with Cove Studio's own writer rather than committed as
    // a binary fixture: the file is exactly the shape the product
    // produces, so this also guards the round trip write → read.
    let bytes = cove_studio::pdf::docx_writer::markdown_to_docx(
        "Contratto di prova",
        "# Contratto di prova

Le parti convengono quanto segue.

## Oggetto

Fornitura di servizi.
",
    )
    .expect("write docx");
    let path = write_temp("generato.docx", &bytes);

    let doc = ingest::ingest_path(&path).await.expect("docx must parse");
    assert_eq!(doc.format, "docx");
    assert!(doc.has_text(), "no text from a real docx");
    assert!(
        doc.text.contains("Le parti convengono quanto segue."),
        "body missing: {}",
        doc.text
    );
    assert!(
        doc.text.contains("Fornitura di servizi."),
        "second section missing: {}",
        doc.text
    );
}

#[tokio::test]
async fn reads_a_spreadsheet_through_the_office_path() {
    // XLSX is today handled by calamine inside Cove Studio; after the
    // swap it goes through the library's Office conversion. Cell values
    // must still arrive, or tabular workflows lose their input.
    use rust_xlsxwriter::Workbook;
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.write_string(0, 0, "Voce").expect("write");
    sheet.write_string(0, 1, "Importo").expect("write");
    sheet.write_string(1, 0, "Canone locazione").expect("write");
    sheet.write_number(1, 1, 1200.5).expect("write");
    let bytes = workbook.save_to_buffer().expect("save xlsx");
    let path = write_temp("prospetto.xlsx", &bytes);

    let doc = ingest::ingest_path(&path).await.expect("xlsx must parse");
    assert_eq!(doc.format, "xlsx", "the original format must be kept");
    assert!(doc.has_text(), "no text from a spreadsheet");
    assert!(
        doc.text.contains("Canone locazione"),
        "cell text missing: {}",
        doc.text
    );
    assert!(
        doc.text.contains("1200") || doc.text.contains("1200.5") || doc.text.contains("1200,5"),
        "numeric cell missing: {}",
        doc.text
    );
}

#[tokio::test]
async fn reads_an_rtf_file() {
    // Minimal but valid RTF, the shape a word processor emits for two
    // paragraphs. Today `rtf-parser` reads this inside Cove Studio.
    let rtf: &[u8] = b"{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0 Times New Roman;}}\n\\f0\\fs24 Verbale di consegna.\\par\nIl bene risulta conforme.\\par\n}";
    let path = write_temp("verbale.rtf", rtf);

    let doc = ingest::ingest_path(&path).await.expect("rtf must parse");
    assert_eq!(doc.format, "rtf");
    assert!(doc.has_text(), "no text from rtf");
    assert!(
        doc.text.contains("Verbale di consegna"),
        "first paragraph missing: {}",
        doc.text
    );
    assert!(
        doc.text.contains("Il bene risulta conforme"),
        "second paragraph missing: {}",
        doc.text
    );
    // Control words must not leak into what the model reads.
    assert!(!doc.text.contains("\rtf1"), "rtf markup leaked: {}", doc.text);
    assert!(!doc.text.contains("fonttbl"), "rtf markup leaked: {}", doc.text);
}

#[tokio::test]
async fn every_shipped_medical_pdf_reads() {
    // Three real files: if one of them stops reading after a dependency
    // bump, this is where it shows.
    for name in [
        "CARTELLA_TEST_001.pdf",
        "CARTELLA_TEST_002.pdf",
        "CARTELLA_TEST_003.pdf",
    ] {
        let path = Path::new("tests/medical").join(name);
        if !path.exists() {
            continue;
        }
        let doc = ingest::ingest_path(&path)
            .await
            .unwrap_or_else(|e| panic!("{name} failed to parse: {e:#}"));
        assert!(doc.has_text(), "{name} produced no text");
        assert!(doc.text.contains("[Page 1]"), "{name} has no page marker");
    }
}
