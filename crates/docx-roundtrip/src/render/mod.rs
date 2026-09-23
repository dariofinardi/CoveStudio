// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
//! [`crate::model::Document`] → `.docx`.
//!
//! Word is forgiving: it opens packages that violate the schema and
//! repairs them silently. That tolerance is a trap for a writer, because
//! a mistake surfaces on somebody else's machine, in LibreOffice, or in a
//! converter — never here. So this module is deliberate about the parts
//! that must exist and the order things go in, even where Word would
//! shrug.
//!
//! Namespaces are declared up front on every part, including the ones only
//! an opaque fragment uses (`v:`, `o:`, `m:`, `wps:`). A kept fragment
//! carries prefixes it did not declare itself — they were on the root of
//! the document it came from — and a package that does not declare them is
//! not XML any more.

pub mod document_xml;

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Write};

use anyhow::Result;

use crate::model::{Block, Document};
use crate::ooxml::{Element, Node};
use crate::parse::Image;

use document_xml::{header_rel_id, Renderer};

/// Namespaces every part declares.
///
/// More than a simple document needs: a fragment kept verbatim from a
/// template may use any of them, and an undeclared prefix makes the file
/// unreadable rather than merely odd.
const NAMESPACES: &[(&str, &str)] = &[
    ("xmlns:w", "http://schemas.openxmlformats.org/wordprocessingml/2006/main"),
    ("xmlns:r", "http://schemas.openxmlformats.org/officeDocument/2006/relationships"),
    ("xmlns:wp", "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"),
    ("xmlns:a", "http://schemas.openxmlformats.org/drawingml/2006/main"),
    ("xmlns:pic", "http://schemas.openxmlformats.org/drawingml/2006/picture"),
    ("xmlns:v", "urn:schemas-microsoft-com:vml"),
    ("xmlns:o", "urn:schemas-microsoft-com:office:office"),
    ("xmlns:w10", "urn:schemas-microsoft-com:office:word"),
    ("xmlns:m", "http://schemas.openxmlformats.org/officeDocument/2006/math"),
    ("xmlns:wps", "http://schemas.microsoft.com/office/word/2010/wordprocessingShape"),
    ("xmlns:wpg", "http://schemas.microsoft.com/office/word/2010/wordprocessingGroup"),
    ("xmlns:mc", "http://schemas.openxmlformats.org/markup-compatibility/2006"),
];

const XML_DECL: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;

/// What to write beside the document: the images it shows and the package
/// parts its kept fragments point at.
#[derive(Debug, Default)]
pub struct Assets {
    pub images: BTreeMap<String, Image>,
    pub parts: BTreeMap<String, Vec<u8>>,
}

impl Assets {
    pub fn from_opened(opened: &crate::parse::Opened) -> Self {
        Self {
            images: opened.images.clone(),
            parts: opened.parts.clone(),
        }
    }
}

/// Writes a document as a `.docx`.
pub fn write(doc: &Document, assets: &Assets) -> Result<Vec<u8>> {
    let mut renderer = Renderer::new(doc, &assets.images);
    let body = renderer.body();

    // Running heads become their own parts, with deterministic names so
    // the references written into `sectPr` always find them.
    let mut head_parts: Vec<(String, String, Element)> = Vec::new();
    for (i, section) in doc.sections.iter().enumerate() {
        for (tag, heads, root) in [
            ("w:headerReference", &section.headers, "w:hdr"),
            ("w:footerReference", &section.footers, "w:ftr"),
        ] {
            for (kind, blocks) in [
                ("default", heads.default.as_ref()),
                ("first", heads.first.as_ref()),
                ("even", heads.even.as_ref()),
            ] {
                let Some(blocks) = blocks else { continue };
                let rel_id = header_rel_id(tag, kind, i);
                let file = format!(
                    "word/{}{}{}.xml",
                    if root == "w:hdr" { "header" } else { "footer" },
                    i,
                    kind
                );
                let element = renderer.blocks_into(blocks, root);
                head_parts.push((rel_id, file, element));
            }
        }
    }

    // Footnotes: Word wants its two separator entries present, even in a
    // document that has none of its own.
    let footnotes = (!doc.footnotes.is_empty()).then(|| {
        let mut root = Element::new("w:footnotes");
        for (kind, id) in [("separator", "-1"), ("continuationSeparator", "0")] {
            let run = Element::new("w:r").with_child(Element::new(match kind {
                "separator" => "w:separator",
                _ => "w:continuationSeparator",
            }));
            root.children.push(Node::Element(
                Element::new("w:footnote")
                    .with_attr("w:type", kind)
                    .with_attr("w:id", id)
                    .with_child(Element::new("w:p").with_child(run)),
            ));
        }
        let mut ids: Vec<&String> = doc.footnotes.keys().collect();
        ids.sort_by_key(|id| id[1..].parse::<i32>().unwrap_or(i32::MAX));
        for id in ids {
            let blocks: &Vec<Block> = &doc.footnotes[id];
            let mut note = Element::new("w:footnote").with_attr("w:id", id.trim_start_matches('n'));
            let rendered = renderer.blocks_into(blocks, "w:footnote");
            note.children = rendered.children;
            note.attrs = vec![("w:id".into(), id.trim_start_matches('n').to_string())];
            root.children.push(Node::Element(note));
        }
        root
    });

    let mut document = Element::new("w:document");
    for (k, v) in NAMESPACES {
        document = document.with_attr(*k, *v);
    }
    document.children.push(Node::Element(body));

    // Relationships of the main part: the running heads, the styles, plus
    // whatever the renderer allocated for images and kept fragments.
    let mut rels: Vec<(String, String, String, Option<String>)> = Vec::new();
    rels.push((
        "rIdStyles".to_string(),
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles".into(),
        "styles.xml".into(),
        None,
    ));
    if footnotes.is_some() {
        rels.push((
            "rIdFootnotes".to_string(),
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes".into(),
            "footnotes.xml".into(),
            None,
        ));
    }
    if !doc.numbering.is_empty() {
        rels.push((
            "rIdNumbering".to_string(),
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering".into(),
            "numbering.xml".into(),
            None,
        ));
    }
    for (rel_id, file, _) in &head_parts {
        let kind = if file.contains("header") {
            "header"
        } else {
            "footer"
        };
        rels.push((
            rel_id.clone(),
            format!("http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}"),
            file.trim_start_matches("word/").to_string(),
            None,
        ));
    }
    rels.extend(renderer.rels.clone());

    // Parts the kept fragments and images need, copied in unchanged.
    let mut binary: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for name in &renderer.parts {
        if let Some(bytes) = assets.parts.get(name) {
            binary.insert(name.clone(), bytes.clone());
        } else if let Some(image) = assets.images.values().find(|i| &i.part_name == name) {
            binary.insert(name.clone(), image.bytes.clone());
        }
    }

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let extensions: BTreeSet<String> = binary
        .keys()
        .filter_map(|name| name.rsplit('.').next())
        .map(|e| e.to_ascii_lowercase())
        .collect();

    zip.start_file("[Content_Types].xml", options)?;
    zip.write_all(content_types(&extensions, &head_parts, footnotes.is_some(), !doc.numbering.is_empty()).as_bytes())?;

    zip.start_file("_rels/.rels", options)?;
    zip.write_all(
        format!(
            r#"{XML_DECL}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#
        )
        .as_bytes(),
    )?;

    zip.start_file("word/document.xml", options)?;
    zip.write_all(format!("{XML_DECL}{}", document.to_xml()).as_bytes())?;

    zip.start_file("word/_rels/document.xml.rels", options)?;
    zip.write_all(rels_xml(&rels).as_bytes())?;

    zip.start_file("word/styles.xml", options)?;
    zip.write_all(format!("{XML_DECL}{}", styles_xml(doc).to_xml()).as_bytes())?;

    if !doc.numbering.is_empty() {
        zip.start_file("word/numbering.xml", options)?;
        zip.write_all(format!("{XML_DECL}{}", numbering_xml(doc).to_xml()).as_bytes())?;
    }

    if let Some(mut notes) = footnotes {
        for (k, v) in NAMESPACES {
            notes = notes.with_attr(*k, *v);
        }
        zip.start_file("word/footnotes.xml", options)?;
        zip.write_all(format!("{XML_DECL}{}", notes.to_xml()).as_bytes())?;
    }

    for (_, file, element) in &head_parts {
        let mut part = element.clone();
        for (k, v) in NAMESPACES {
            part = part.with_attr(*k, *v);
        }
        zip.start_file(file, options)?;
        zip.write_all(format!("{XML_DECL}{}", part.to_xml()).as_bytes())?;
    }

    for (name, bytes) in &binary {
        zip.start_file(name, options)?;
        zip.write_all(bytes)?;
    }

    Ok(zip.finish()?.into_inner())
}

fn rels_xml(rels: &[(String, String, String, Option<String>)]) -> String {
    let mut out = String::from(XML_DECL);
    out.push_str(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    for (id, kind, target, mode) in rels {
        out.push_str(&format!(
            r#"<Relationship Id="{}" Type="{}" Target="{}"{}/>"#,
            escape_attr(id),
            escape_attr(kind),
            escape_attr(target),
            match mode.as_deref() {
                Some("External") => r#" TargetMode="External""#,
                _ => "",
            }
        ));
    }
    out.push_str("</Relationships>");
    out
}

fn escape_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('"', "&quot;")
}

fn content_types(
    extensions: &BTreeSet<String>,
    heads: &[(String, String, Element)],
    footnotes: bool,
    numbering: bool,
) -> String {
    let mut out = String::from(XML_DECL);
    out.push_str(r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#);
    out.push_str(r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#);
    out.push_str(r#"<Default Extension="xml" ContentType="application/xml"/>"#);
    for ext in extensions {
        if matches!(ext.as_str(), "rels" | "xml") {
            continue;
        }
        let mime = match ext.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "bmp" => "image/bmp",
            "tif" | "tiff" => "image/tiff",
            "svg" => "image/svg+xml",
            "emf" => "image/x-emf",
            "wmf" => "image/x-wmf",
            "bin" => "application/vnd.openxmlformats-officedocument.oleObject",
            _ => continue,
        };
        out.push_str(&format!(
            r#"<Default Extension="{ext}" ContentType="{mime}"/>"#
        ));
    }
    let word = "application/vnd.openxmlformats-officedocument.wordprocessingml";
    out.push_str(&format!(
        r#"<Override PartName="/word/document.xml" ContentType="{word}.document.main+xml"/>"#
    ));
    out.push_str(&format!(
        r#"<Override PartName="/word/styles.xml" ContentType="{word}.styles+xml"/>"#
    ));
    if numbering {
        out.push_str(&format!(
            r#"<Override PartName="/word/numbering.xml" ContentType="{word}.numbering+xml"/>"#
        ));
    }
    if footnotes {
        out.push_str(&format!(
            r#"<Override PartName="/word/footnotes.xml" ContentType="{word}.footnotes+xml"/>"#
        ));
    }
    for (_, file, _) in heads {
        let kind = if file.contains("header") {
            "header"
        } else {
            "footer"
        };
        out.push_str(&format!(
            r#"<Override PartName="/{file}" ContentType="{word}.{kind}+xml"/>"#
        ));
    }
    out.push_str("</Types>");
    out
}

fn styles_xml(doc: &Document) -> Element {
    let mut root = Element::new("w:styles");
    for (k, v) in NAMESPACES {
        root = root.with_attr(*k, *v);
    }
    // Document defaults first: everything else is read against them.
    if let Some(rpr) = document_xml::run_props_xml(None, Some(&doc.styles.defaults)) {
        root.children.push(Node::Element(
            Element::new("w:docDefaults").with_child(
                Element::new("w:rPrDefault").with_child(rpr),
            ),
        ));
    }
    for (id, style) in &doc.styles.paragraph {
        let mut el = Element::new("w:style")
            .with_attr("w:type", "paragraph")
            .with_attr("w:styleId", id)
            .with_child(
                Element::new("w:name").with_attr("w:val", style.name.as_deref().unwrap_or(id)),
            );
        if let Some(based_on) = &style.based_on {
            el = el.with_child(Element::new("w:basedOn").with_attr("w:val", based_on));
        }
        if let Some(next) = &style.next {
            el = el.with_child(Element::new("w:next").with_attr("w:val", next));
        }
        if let Some(ppr) = document_xml::paragraph_props_xml(None, style.paragraph.as_ref()) {
            el = el.with_child(ppr);
        }
        if let Some(rpr) = document_xml::run_props_xml(None, style.run.as_ref()) {
            el = el.with_child(rpr);
        }
        root.children.push(Node::Element(el));
    }
    for (id, props) in &doc.styles.character {
        let mut el = Element::new("w:style")
            .with_attr("w:type", "character")
            .with_attr("w:styleId", id)
            .with_child(Element::new("w:name").with_attr("w:val", id));
        if let Some(rpr) = document_xml::run_props_xml(None, Some(props)) {
            el = el.with_child(rpr);
        }
        root.children.push(Node::Element(el));
    }
    root
}

fn numbering_xml(doc: &Document) -> Element {
    let mut root = Element::new("w:numbering");
    for (k, v) in NAMESPACES {
        root = root.with_attr(*k, *v);
    }
    // One abstract definition per concrete list: the model does not share
    // them, and sharing them would make editing one list change another.
    for (i, (id, numbering)) in doc.numbering.iter().enumerate() {
        let abstract_id = format!("{i}");
        let mut def = Element::new("w:abstractNum").with_attr("w:abstractNumId", &abstract_id);
        for (level, spec) in numbering.levels.iter().enumerate() {
            let mut lvl = Element::new("w:lvl").with_attr("w:ilvl", level.to_string());
            lvl = lvl.with_child(Element::new("w:start").with_attr("w:val", "1"));
            lvl = lvl.with_child(
                Element::new("w:numFmt")
                    .with_attr("w:val", spec.format.as_deref().unwrap_or("decimal")),
            );
            lvl = lvl.with_child(
                Element::new("w:lvlText").with_attr("w:val", spec.text.as_deref().unwrap_or("%1.")),
            );
            lvl = lvl.with_child(Element::new("w:lvlJc").with_attr("w:val", "left"));
            let indent = spec.indent_left.unwrap_or(720 + 360 * level as i32);
            lvl = lvl.with_child(
                Element::new("w:pPr").with_child(
                    Element::new("w:ind")
                        .with_attr("w:left", indent.to_string())
                        .with_attr("w:hanging", spec.hanging.unwrap_or(360).to_string()),
                ),
            );
            def.children.push(Node::Element(lvl));
        }
        root.children.push(Node::Element(def));
        root.children.push(Node::Element(
            Element::new("w:num").with_attr("w:numId", id).with_child(
                Element::new("w:abstractNumId").with_attr("w:val", &abstract_id),
            ),
        ));
    }
    root
}
