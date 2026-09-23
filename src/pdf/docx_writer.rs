//! Minimal DOCX writer + in-place editor.
//!
//! - `markdown_to_docx(title, markdown)` → produces a small but valid .docx
//!   from a Markdown string. Supports headings (#, ##, ###), paragraphs,
//!   bullet/numbered lists, bold/italic emphasis, and code spans.
//! - `apply_text_edits(original, edits)` → reads an existing .docx, walks
//!   `word/document.xml`, performs find/replace inside `<w:t>` runs, and
//!   re-zips the result. Used by the `edit_document` builtin tool.

use anyhow::{anyhow, Result};
use pulldown_cmark::{Event as MdEvent, HeadingLevel, Parser, Tag, TagEnd};
use std::io::{Cursor, Read, Write};

// ---------------------------------------------------------------------------
// generate_docx
// ---------------------------------------------------------------------------

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
  <Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
</Types>"#;

const RELS_ROOT: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

const RELS_DOC: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
</Relationships>"#;

const STYLES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>
  <w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:pPr><w:spacing w:before="240" w:after="120"/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:sz w:val="36"/></w:rPr></w:style>
  <w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:pPr><w:spacing w:before="200" w:after="80"/><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:b/><w:sz w:val="30"/></w:rPr></w:style>
  <w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:pPr><w:spacing w:before="160" w:after="60"/><w:outlineLvl w:val="2"/></w:pPr><w:rPr><w:b/><w:sz w:val="26"/></w:rPr></w:style>
  <w:style w:type="paragraph" w:styleId="ListBullet"><w:name w:val="List Bullet"/><w:basedOn w:val="Normal"/><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr></w:style>
  <w:style w:type="paragraph" w:styleId="ListNumber"><w:name w:val="List Number"/><w:basedOn w:val="Normal"/><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="2"/></w:numPr></w:pPr></w:style>
</w:styles>"#;

/// Produce a DOCX byte buffer from `markdown`. `title` is currently used
/// only as the implicit Heading 1 prepended at the top.
pub fn markdown_to_docx(title: &str, markdown: &str) -> Result<Vec<u8>> {
    let body_xml = render_markdown_to_wml(title, markdown);
    let document_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
{body_xml}    <w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr>
  </w:body>
</w:document>"#
    );

    let buf = Vec::new();
    let cursor = Cursor::new(buf);
    let mut zip = zip::ZipWriter::new(cursor);
    let opts: zip::write::SimpleFileOptions =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("[Content_Types].xml", opts)?;
    zip.write_all(CONTENT_TYPES.as_bytes())?;
    zip.start_file("_rels/.rels", opts)?;
    zip.write_all(RELS_ROOT.as_bytes())?;
    zip.start_file("word/_rels/document.xml.rels", opts)?;
    zip.write_all(RELS_DOC.as_bytes())?;
    zip.start_file("word/styles.xml", opts)?;
    zip.write_all(STYLES_XML.as_bytes())?;
    zip.start_file("word/document.xml", opts)?;
    zip.write_all(document_xml.as_bytes())?;

    let cursor = zip.finish()?;
    Ok(cursor.into_inner())
}

fn render_markdown_to_wml(title: &str, markdown: &str) -> String {
    let mut out = String::new();
    if !title.trim().is_empty() {
        out.push_str(&para("Heading1", &[run(title, false, false, false)]));
    }

    let parser = Parser::new(markdown);
    let mut current_runs: Vec<String> = Vec::new();
    let mut current_style: Option<&str> = None;
    let mut bold = false;
    let mut italic = false;
    let mut in_code_block = false;

    let flush_paragraph = |runs: &mut Vec<String>, style: Option<&str>, out: &mut String| {
        if !runs.is_empty() {
            let style = style.unwrap_or("Normal");
            out.push_str(&para(style, runs));
            runs.clear();
        }
    };

    for ev in parser {
        match ev {
            MdEvent::Start(Tag::Heading { level, .. }) => {
                flush_paragraph(&mut current_runs, current_style, &mut out);
                current_style = Some(match level {
                    HeadingLevel::H1 => "Heading1",
                    HeadingLevel::H2 => "Heading2",
                    HeadingLevel::H3 => "Heading3",
                    _ => "Heading3",
                });
            }
            MdEvent::End(TagEnd::Heading(_)) => {
                flush_paragraph(&mut current_runs, current_style, &mut out);
                current_style = None;
            }
            MdEvent::Start(Tag::Paragraph) => { current_style = Some("Normal"); }
            MdEvent::End(TagEnd::Paragraph) => {
                flush_paragraph(&mut current_runs, current_style, &mut out);
                current_style = None;
            }
            MdEvent::Start(Tag::List(Some(_))) => { /* numbered */ }
            MdEvent::Start(Tag::List(None))    => { /* bullet */ }
            MdEvent::End(TagEnd::List(_)) => {}
            MdEvent::Start(Tag::Item) => { current_style = Some("ListBullet"); }
            MdEvent::End(TagEnd::Item) => {
                flush_paragraph(&mut current_runs, current_style, &mut out);
                current_style = None;
            }
            MdEvent::Start(Tag::Strong)   => bold = true,
            MdEvent::End(TagEnd::Strong)  => bold = false,
            MdEvent::Start(Tag::Emphasis) => italic = true,
            MdEvent::End(TagEnd::Emphasis) => italic = false,
            MdEvent::Start(Tag::CodeBlock(_)) => { in_code_block = true; current_style = Some("Normal"); }
            MdEvent::End(TagEnd::CodeBlock)   => {
                flush_paragraph(&mut current_runs, current_style, &mut out);
                in_code_block = false;
                current_style = None;
            }
            MdEvent::Text(t) => {
                current_runs.push(run(&t, bold, italic, in_code_block));
            }
            MdEvent::Code(t) => {
                current_runs.push(run(&t, bold, italic, true));
            }
            MdEvent::SoftBreak | MdEvent::HardBreak => {
                current_runs.push(r#"<w:r><w:br/></w:r>"#.to_string());
            }
            _ => {}
        }
    }
    flush_paragraph(&mut current_runs, current_style, &mut out);
    out
}

fn para(style: &str, runs: &[String]) -> String {
    let mut s = String::new();
    s.push_str("    <w:p>");
    s.push_str(&format!(r#"<w:pPr><w:pStyle w:val="{style}"/></w:pPr>"#));
    for r in runs { s.push_str(r); }
    s.push_str("</w:p>\n");
    s
}

fn run(text: &str, bold: bool, italic: bool, mono: bool) -> String {
    let mut props = String::new();
    if bold { props.push_str("<w:b/>"); }
    if italic { props.push_str("<w:i/>"); }
    if mono { props.push_str(r#"<w:rFonts w:ascii="Courier New" w:hAnsi="Courier New"/>"#); }
    let rpr = if !props.is_empty() { format!("<w:rPr>{props}</w:rPr>") } else { String::new() };
    format!(
        r#"<w:r>{rpr}<w:t xml:space="preserve">{}</w:t></w:r>"#,
        xml_escape(text)
    )
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Editing a .docx lives in the `docx-roundtrip` crate.
//
// It used to live here, as find-and-replace over `<w:t>` elements, with a
// tolerant second pass for text Word had split across runs. The crate does
// it on the document tree instead — every occurrence, inside tables,
// headers and footnotes, keeping the formatting of the text replaced — so
// keeping this copy would only give the two a chance to disagree.
// ---------------------------------------------------------------------------

fn xml_escape_static(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            other => out.push(other),
        }
    }
    out
}

fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}
