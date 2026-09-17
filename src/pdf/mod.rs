pub mod docx_writer;

use anyhow::Result;

// ---------------------------------------------------------------------------
// PDF extraction via pdfium — requires feature "pdf" + bundled pdfium DLL
// ---------------------------------------------------------------------------

#[cfg(feature = "pdf")]
use anyhow::anyhow;
#[cfg(feature = "pdf")]
use pdfium_render::prelude::*;
#[cfg(feature = "pdf")]
use std::path::Path;

#[cfg(feature = "pdf")]
const OCR_FALLBACK_THRESHOLD: usize = 10;

/// Load pdfium from the bundled DLL in libs/pdfium/.
///
/// Looks (in order):
///   1. `$PDFIUM_DYNAMIC_LIB_PATH` if set (explicit override).
///   2. `<exe_dir>/libs/pdfium/<arch>/` and `<exe_dir>/resources/libs/pdfium/<arch>/`
///      (Tauri MSI install: `bundle.resources` staged under `resources/`).
///   3. `<cwd>/libs/pdfium/<arch>/`
///   4. The legacy flat `<base>/libs/pdfium/` path (no arch subdir), so a
///      pre-existing developer checkout that downloaded the wrong layout
///      doesn't immediately break.
///   5. Ancestor walk: each ancestor of `<cwd>` and `<exe_dir>` checked for
///      `libs/pdfium/<arch>/` first, then the legacy flat path.
///
/// Steps 2–4 cover the bundled-MSI layout (`scripts/fetch-native-libs.ps1`
/// populates `libs/pdfium/win-x64/` and `libs/pdfium/win-arm64/`; the
/// `scripts/build-release.ps1` bundle.resources overlay carries only the
/// matching arch into the install). Step 5 still covers the
/// `target/debug/cove-studio.exe` dev layout where the DLL lives at
/// `<workspace>/libs/pdfium/<arch>/` further up the tree.
#[cfg(feature = "pdf")]
fn load_pdfium() -> Result<Pdfium> {
    #[cfg(target_os = "windows")]
    const DLL_NAME: &str = "pdfium.dll";
    #[cfg(target_os = "linux")]
    const DLL_NAME: &str = "libpdfium.so";
    #[cfg(target_os = "macos")]
    const DLL_NAME: &str = "libpdfium.dylib";

    /// Per-process compile target → matching subdirectory name for the
    /// vendored DLL tree. Mirrors `embeddings::service::onnxruntime_subdir_and_filename`.
    const ARCH_SUB: &str = {
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        { "win-x64" }
        #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
        { "win-arm64" }
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        { "linux-x64" }
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        { "linux-aarch64" }
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        { "macos-x64" }
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        { "macos-arm64" }
    };

    fn try_load(dir: &std::path::Path) -> Option<Pdfium> {
        let dll = dir.join(DLL_NAME);
        if dll.exists() {
            tracing::info!("[pdf] loading pdfium from {}", dll.display());
            Pdfium::bind_to_library(dll)
                .map_err(|e| anyhow!("pdfium bind: {e}"))
                .ok()
                .map(Pdfium::new)
        } else {
            None
        }
    }

    // 1. Explicit override.
    if let Ok(path) = std::env::var("PDFIUM_DYNAMIC_LIB_PATH") {
        let p = std::path::PathBuf::from(&path);
        let dir = if p.is_file() { p.parent().map(|x| x.to_path_buf()) } else { Some(p) };
        if let Some(d) = dir {
            if let Some(p) = try_load(&d) {
                return Ok(p);
            }
        }
    }

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|x| x.to_path_buf()));
    let cwd = std::env::current_dir().ok();
    // Tauri MSI install layout: `bundle.resources` files land in
    // `<install>/resources/` next to `<install>/cove-studio.exe`.
    let exe_resources = exe_dir.as_ref().map(|d| d.join("resources"));

    let bases: Vec<&std::path::Path> = exe_dir
        .as_deref()
        .into_iter()
        .chain(exe_resources.as_deref().into_iter())
        .chain(cwd.as_deref().into_iter())
        .collect();

    // 2 & 3: per-arch lookup first.
    for base in &bases {
        let arch_dir = base.join("libs").join("pdfium").join(ARCH_SUB);
        if let Some(p) = try_load(&arch_dir) {
            return Ok(p);
        }
    }
    // 4: legacy flat layout (pre-arch-subdir checkouts).
    for base in &bases {
        if let Some(p) = try_load(&base.join("libs").join("pdfium")) {
            return Ok(p);
        }
    }

    // 5: ancestor walk — per-arch first, legacy flat as last resort.
    for base in cwd.iter().chain(exe_dir.iter()) {
        for ancestor in base.ancestors() {
            let arch_dir = ancestor.join("libs").join("pdfium").join(ARCH_SUB);
            if let Some(p) = try_load(&arch_dir) {
                return Ok(p);
            }
            if let Some(p) = try_load(&ancestor.join("libs").join("pdfium")) {
                return Ok(p);
            }
        }
    }

    Err(anyhow!(
        "pdfium library not found. Run `./scripts/fetch-native-libs.ps1` \
         to download the matching binary into libs/pdfium/{}/{}, or set \
         $PDFIUM_DYNAMIC_LIB_PATH explicitly.",
        ARCH_SUB,
        DLL_NAME
    ))
}

#[cfg(feature = "pdf")]
pub struct PageText {
    pub page: usize,
    pub text: String,
    pub needs_ocr: bool,
}

/// Pass 1: native text extraction via pdfium content stream.
/// Returns per-page text + flag for pages that need OCR fallback.
///
/// Logs progress every 10 pages on documents larger than that — for a
/// 200-page brief the user wants to see "10/200, 20/200…" rather than
/// silence punctuated by the final result.
#[cfg(feature = "pdf")]
pub fn extract_text(path: &Path) -> Result<Vec<PageText>> {
    let pdfium = load_pdfium()?;
    let doc = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| anyhow!("pdfium load error: {e}"))?;

    let total = doc.pages().len() as usize;
    if total > 0 {
        tracing::info!(
            "[pdf] {}: extracting {} pages",
            path.file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
            total
        );
    }

    let mut pages = Vec::new();
    for (i, page) in doc.pages().iter().enumerate() {
        let text = page.text().map_err(|e| anyhow!("page text error: {e}"))?.all();
        let alpha_count = text.chars().filter(|c| c.is_alphanumeric()).count();
        pages.push(PageText {
            page: i + 1,
            text,
            needs_ocr: alpha_count < OCR_FALLBACK_THRESHOLD,
        });
        if total > 10 && (i + 1) % 10 == 0 {
            tracing::info!(
                "[pdf] {}: {}/{} pages",
                path.file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default(),
                i + 1,
                total
            );
        }
    }
    Ok(pages)
}

/// Convenience: join all pages into a single string with [Page N] markers.
#[cfg(feature = "pdf")]
pub fn extract_full_text(path: &Path) -> Result<String> {
    let pages = extract_text(path)?;
    let mut out = String::new();
    for p in pages {
        out.push_str(&format!("[Page {}]\n{}\n", p.page, p.text));
    }
    Ok(out)
}

/// Heuristic: a PDF is "scanned" (image-only) when the *majority* of pages
/// have almost no extractable text. Used to decide whether to fall back to
/// rendering pages as images for a vision-capable model.
#[cfg(feature = "pdf")]
pub fn is_scanned_pdf(pages: &[PageText]) -> bool {
    if pages.is_empty() { return false; }
    let scanned = pages.iter().filter(|p| p.needs_ocr).count();
    scanned * 2 >= pages.len()
}

/// Render PDF pages as PNG bytes at the given DPI.
/// Used for vision-capable models when text extraction fails (scanned PDFs).
#[cfg(feature = "pdf")]
pub fn render_pdf_pages(path: &Path, dpi: f32, max_pages: usize) -> Result<Vec<Vec<u8>>> {
    use image::ImageFormat;
    use pdfium_render::prelude::PdfRenderConfig;
    use std::io::Cursor;

    let pdfium = load_pdfium()?;
    let doc = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| anyhow!("pdfium load error: {e}"))?;

    // PDF base resolution is 72 DPI; scale = target / 72.
    let scale = dpi / 72.0;
    let config = PdfRenderConfig::new().scale_page_by_factor(scale);

    let mut out = Vec::new();
    for (i, page) in doc.pages().iter().enumerate() {
        if i >= max_pages { break; }
        let bitmap = page
            .render_with_config(&config)
            .map_err(|e| anyhow!("render page {i}: {e}"))?;
        let dyn_img = bitmap.as_image();
        let mut buf = Vec::new();
        dyn_img
            .write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
            .map_err(|e| anyhow!("encode png page {i}: {e}"))?;
        out.push(buf);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// TIFF → JPEG conversion (handles single and multi-page TIFFs)
// ---------------------------------------------------------------------------

/// Decode every frame in a TIFF and re-encode each as JPEG (quality 85).
/// Single-page TIFFs return a 1-element Vec; multi-page TIFFs return one
/// JPEG per frame in source order. Used for vision-capable models that
/// cannot consume TIFF natively.
pub fn convert_tiff_to_jpegs(data: &[u8]) -> Result<Vec<Vec<u8>>> {
    use anyhow::anyhow;
    use std::io::Cursor;
    use tiff::decoder::{Decoder, DecodingResult};
    use tiff::ColorType;

    let mut decoder = Decoder::new(Cursor::new(data.to_vec()))
        .map_err(|e| anyhow!("tiff decoder init: {e}"))?;

    let mut out = Vec::new();
    loop {
        let (w, h) = decoder
            .dimensions()
            .map_err(|e| anyhow!("tiff dimensions: {e}"))?;
        let color = decoder
            .colortype()
            .map_err(|e| anyhow!("tiff colortype: {e}"))?;
        let pixels = decoder
            .read_image()
            .map_err(|e| anyhow!("tiff read frame: {e}"))?;

        let dyn_img: image::DynamicImage = match (color, pixels) {
            (ColorType::RGB(8), DecodingResult::U8(buf)) => {
                let img = image::RgbImage::from_raw(w, h, buf)
                    .ok_or_else(|| anyhow!("tiff RGB frame buffer mismatch"))?;
                image::DynamicImage::ImageRgb8(img)
            }
            (ColorType::RGBA(8), DecodingResult::U8(buf)) => {
                let img = image::RgbaImage::from_raw(w, h, buf)
                    .ok_or_else(|| anyhow!("tiff RGBA frame buffer mismatch"))?;
                image::DynamicImage::ImageRgba8(img)
            }
            (ColorType::Gray(8), DecodingResult::U8(buf)) => {
                let img = image::GrayImage::from_raw(w, h, buf)
                    .ok_or_else(|| anyhow!("tiff Gray frame buffer mismatch"))?;
                image::DynamicImage::ImageLuma8(img)
            }
            (ColorType::GrayA(8), DecodingResult::U8(buf)) => {
                let img = image::GrayAlphaImage::from_raw(w, h, buf)
                    .ok_or_else(|| anyhow!("tiff GrayA frame buffer mismatch"))?;
                image::DynamicImage::ImageLumaA8(img)
            }
            // 16-bit channels — downscale to 8-bit by truncating low byte.
            (ColorType::RGB(16), DecodingResult::U16(buf)) => {
                let bytes: Vec<u8> = buf.into_iter().map(|v| (v >> 8) as u8).collect();
                let img = image::RgbImage::from_raw(w, h, bytes)
                    .ok_or_else(|| anyhow!("tiff RGB16 frame buffer mismatch"))?;
                image::DynamicImage::ImageRgb8(img)
            }
            (ColorType::Gray(16), DecodingResult::U16(buf)) => {
                let bytes: Vec<u8> = buf.into_iter().map(|v| (v >> 8) as u8).collect();
                let img = image::GrayImage::from_raw(w, h, bytes)
                    .ok_or_else(|| anyhow!("tiff Gray16 frame buffer mismatch"))?;
                image::DynamicImage::ImageLuma8(img)
            }
            (ct, _) => {
                return Err(anyhow!("Unsupported TIFF color type: {:?}", ct));
            }
        };

        let mut jpeg_buf = Vec::new();
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_buf, 85);
        dyn_img
            .write_with_encoder(encoder)
            .map_err(|e| anyhow!("jpeg encode: {e}"))?;
        out.push(jpeg_buf);

        if !decoder.more_images() {
            break;
        }
        decoder
            .next_image()
            .map_err(|e| anyhow!("tiff next frame: {e}"))?;
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// XLSX extraction — calamine, pure Rust
// ---------------------------------------------------------------------------

pub fn extract_xlsx_text(data: &[u8]) -> Result<String> {
    use anyhow::anyhow;
    use calamine::{Reader, Xlsx};
    use std::io::Cursor;

    let cursor = Cursor::new(data.to_vec());
    let mut workbook: Xlsx<_> = calamine::open_workbook_from_rs(cursor)
        .map_err(|e| anyhow!("xlsx open error: {e}"))?;

    let mut out = String::new();
    let sheet_names = workbook.sheet_names();
    for name in &sheet_names {
        if let Ok(range) = workbook.worksheet_range(name) {
            out.push_str(&format!("=== Sheet: {name} ===\n"));
            for row in range.rows() {
                let cells: Vec<String> = row
                    .iter()
                    .map(|c| c.to_string())
                    .collect();
                out.push_str(&cells.join("\t"));
                out.push('\n');
            }
            out.push('\n');
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// DOCX extraction — pure Rust ZIP+XML, no external process
// ---------------------------------------------------------------------------

/// Extract the body text of a DOCX, surfacing two classes of "removed"
/// content that legal redlines depend on:
///
///   * Tracked deletions — `<w:del>…<w:delText>X</w:delText>…</w:del>`
///     blocks. Word emits these when the doc was edited with track-
///     changes on; X is the literal text the author marked for removal.
///   * Strike-through formatting — runs whose `<w:rPr>` carries
///     `<w:strike/>` or `<w:dstrike/>`. This is purely visual styling
///     (no track-changes session needed) but is the convention some
///     contracts use to signal "this clause is no longer in force."
///
/// Both kinds are wrapped in `[removed by author: …]` markers so the
/// LLM can reason about the redline structure, e.g.:
///
/// ```text
/// The contract clauses are: clause 1, [removed by author: clause 2],
/// clause 3.
/// ```
///
/// Paragraph boundaries (`<w:p>`) and line breaks (`<w:br/>`) are
/// emitted as newlines so the output keeps some shape; tab elements
/// (`<w:tab/>`) become single spaces.
pub fn extract_docx_text(data: &[u8]) -> Result<String> {
    use anyhow::anyhow;
    use std::io::Cursor;

    let cursor = Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor)?;

    let xml = {
        let mut file = archive
            .by_name("word/document.xml")
            .map_err(|_| anyhow!("Not a valid DOCX: missing word/document.xml"))?;
        let mut buf = String::new();
        use std::io::Read;
        file.read_to_string(&mut buf)?;
        buf
    };

    Ok(extract_docx_body_text(&xml))
}

/// Pure-string entry point for the docx XML → annotated plain text
/// conversion. Kept separate from `extract_docx_text` so unit tests
/// can exercise the extraction without packaging a real ZIP.
fn extract_docx_body_text(xml: &str) -> String {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut out = String::with_capacity(xml.len() / 4);
    // Stack-style depth counter for tracked deletions. <w:del> blocks
    // can in principle nest; the depth lets us notice the *outermost*
    // close to flip the "removed" flag back off.
    let mut del_depth: usize = 0;
    // Strike-through is run-scoped: <w:strike/> appears inside the
    // run's <w:rPr>, applies to the run's text, ends at </w:r>.
    let mut current_run_struck = false;
    let mut in_run = false;
    let mut in_rpr = false;
    // True while we have an unclosed "[removed by author: " in the
    // output — closes on whichever event ends the removed region first.
    let mut removal_open = false;

    let close_removal = |out: &mut String, removal_open: &mut bool| {
        if *removal_open {
            out.push(']');
            *removal_open = false;
        }
    };

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                match local_name_str(e.name()).as_deref() {
                    Some("p") => {
                        close_removal(&mut out, &mut removal_open);
                        if !out.ends_with('\n') && !out.is_empty() {
                            out.push('\n');
                        }
                    }
                    Some("r") => {
                        in_run = true;
                        current_run_struck = false;
                    }
                    Some("rPr") => {
                        if in_run {
                            in_rpr = true;
                        }
                    }
                    Some("del") => del_depth += 1,
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => match local_name_str(e.name()).as_deref() {
                Some("strike") | Some("dstrike") => {
                    if in_rpr {
                        current_run_struck = true;
                    }
                }
                Some("br") => {
                    close_removal(&mut out, &mut removal_open);
                    out.push('\n');
                }
                Some("tab") => out.push(' '),
                _ => {}
            },
            Ok(Event::End(e)) => match local_name_str(e.name()).as_deref() {
                Some("r") => {
                    if removal_open && current_run_struck && del_depth == 0 {
                        close_removal(&mut out, &mut removal_open);
                    }
                    in_run = false;
                    current_run_struck = false;
                }
                Some("rPr") => {
                    in_rpr = false;
                }
                Some("del") => {
                    if del_depth > 0 {
                        del_depth -= 1;
                    }
                    if del_depth == 0 {
                        close_removal(&mut out, &mut removal_open);
                    }
                }
                Some("p") => {
                    close_removal(&mut out, &mut removal_open);
                }
                _ => {}
            },
            Ok(Event::Text(t)) => {
                let raw = t.unescape().unwrap_or_default().into_owned();
                if raw.is_empty() {
                    continue;
                }
                let removed = del_depth > 0 || current_run_struck;
                if removed {
                    if !removal_open {
                        if !out.is_empty() && !out.ends_with(char::is_whitespace) {
                            out.push(' ');
                        }
                        out.push_str("[removed by author: ");
                        removal_open = true;
                    }
                    out.push_str(&raw);
                } else {
                    close_removal(&mut out, &mut removal_open);
                    out.push_str(&raw);
                }
            }
            Ok(Event::CData(c)) => {
                let raw = String::from_utf8_lossy(c.as_ref()).into_owned();
                if raw.is_empty() {
                    continue;
                }
                let removed = del_depth > 0 || current_run_struck;
                if removed {
                    if !removal_open {
                        if !out.is_empty() && !out.ends_with(char::is_whitespace) {
                            out.push(' ');
                        }
                        out.push_str("[removed by author: ");
                        removal_open = true;
                    }
                    out.push_str(&raw);
                } else {
                    close_removal(&mut out, &mut removal_open);
                    out.push_str(&raw);
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    close_removal(&mut out, &mut removal_open);
    collapse_inline_whitespace(&out).trim().to_string()
}

fn local_name_str(name: quick_xml::name::QName) -> Option<String> {
    let bytes = name.local_name().into_inner();
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

fn collapse_inline_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_was_inline_space = false;
    for ch in s.chars() {
        if ch == '\n' {
            while out.ends_with(' ') {
                out.pop();
            }
            out.push('\n');
            last_was_inline_space = false;
        } else if ch.is_whitespace() {
            if !last_was_inline_space && !out.is_empty() && !out.ends_with('\n') {
                out.push(' ');
                last_was_inline_space = true;
            }
        } else {
            out.push(ch);
            last_was_inline_space = false;
        }
    }
    out
}

#[cfg(test)]
mod docx_tests {
    use super::extract_docx_body_text;

    fn wrap(body: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
             <w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
             <w:body>{body}</w:body></w:document>",
        )
    }

    fn run(text: &str) -> String {
        format!("<w:r><w:t xml:space=\"preserve\">{text}</w:t></w:r>")
    }

    fn struck_run(text: &str) -> String {
        format!(
            "<w:r><w:rPr><w:strike/></w:rPr><w:t xml:space=\"preserve\">{text}</w:t></w:r>"
        )
    }

    fn deleted_run(author: &str, text: &str) -> String {
        format!(
            "<w:del w:id=\"1\" w:author=\"{author}\" w:date=\"2024-01-01T00:00:00Z\">\
             <w:r><w:delText xml:space=\"preserve\">{text}</w:delText></w:r></w:del>",
        )
    }

    #[test]
    fn plain_paragraphs_extract_unchanged() {
        let xml = wrap(&format!(
            "<w:p>{}</w:p><w:p>{}</w:p>",
            run("Hello world."),
            run("Second line.")
        ));
        let got = extract_docx_body_text(&xml);
        assert_eq!(got, "Hello world.\nSecond line.");
    }

    #[test]
    fn tracked_deletion_is_marked() {
        let xml = wrap(&format!(
            "<w:p>{}{}{}</w:p>",
            run("Keep "),
            deleted_run("Alice", "this part"),
            run(" then more.")
        ));
        let got = extract_docx_body_text(&xml);
        assert!(
            got.contains("[removed by author: this part]"),
            "expected del marker, got {got:?}"
        );
        assert!(got.contains("Keep"));
        assert!(got.contains("then more."));
    }

    #[test]
    fn strike_through_run_is_marked() {
        let xml = wrap(&format!(
            "<w:p>{}{}{}</w:p>",
            run("Before "),
            struck_run("STRUCK"),
            run(" after.")
        ));
        let got = extract_docx_body_text(&xml);
        assert!(
            got.contains("[removed by author: STRUCK]"),
            "expected strike marker, got {got:?}"
        );
        assert!(got.contains("Before"));
        assert!(got.contains("after."));
    }

    #[test]
    fn dstrike_run_is_marked() {
        let xml = wrap(&format!(
            "<w:p><w:r><w:rPr><w:dstrike/></w:rPr><w:t>X</w:t></w:r></w:p>"
        ));
        let got = extract_docx_body_text(&xml);
        assert!(
            got.contains("[removed by author: X]"),
            "expected dstrike marker, got {got:?}"
        );
    }

    #[test]
    fn removal_does_not_span_paragraphs() {
        // A pathological case: a deletion followed by a paragraph
        // close should not leave the bracket open across <w:p>.
        let xml = wrap(&format!(
            "<w:p>{}{}</w:p><w:p>{}</w:p>",
            run("a "),
            deleted_run("X", "b"),
            run("c")
        ));
        let got = extract_docx_body_text(&xml);
        // No stray '[' without a matching ']' on either line.
        for line in got.lines() {
            let opens = line.matches("[removed by author:").count();
            let closes = line.matches(']').count();
            assert!(
                opens <= closes,
                "unbalanced brackets on line {line:?} (full: {got:?})"
            );
        }
    }

    #[test]
    fn run_without_strike_is_plain() {
        let xml = wrap(&format!("<w:p>{}</w:p>", run("not struck")));
        let got = extract_docx_body_text(&xml);
        assert_eq!(got, "not struck");
        assert!(!got.contains("removed by author"));
    }

    #[test]
    fn empty_document_returns_empty_string() {
        let xml = wrap("");
        assert_eq!(extract_docx_body_text(&xml), "");
    }
}
