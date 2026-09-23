// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
//! `.docx` → [`crate::model::Document`].
//!
//! The rule that shapes this module: **never fail on content**. A parser
//! that gives up on the first unknown element is useless against real
//! templates, which are full of things an editor has no opinion about. So
//! anything not modelled becomes an opaque node holding its own XML, and
//! the reader reports how much of the document ended up that way — the
//! difference between "read in full" and "read enough to show" is the
//! caller's to judge, not ours to hide.

pub mod fields;
pub mod props;
pub mod styles;
pub mod walker;

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, Context, Result};

use crate::model::{Block, Comment, Document, Margins, Orientation, Page, RunningHeads, Section};
use crate::ooxml::{read_rels, Element, Package};

pub use props::VMerge;
pub use walker::Image;

/// A document that has been opened, with everything needed to write it
/// back out.
#[derive(Debug)]
pub struct Opened {
    pub document: Document,
    /// Images by the `src` key used in the tree.
    pub images: BTreeMap<String, Image>,
    /// Package parts the opaque XML refers to, with their bytes.
    pub parts: BTreeMap<String, Vec<u8>>,
    /// What could not be modelled faithfully. Not errors: a document with
    /// warnings is still a document.
    pub warnings: Vec<String>,
    /// How many fragments were kept verbatim.
    pub opaque_count: u32,
    /// Placeholder names found, in document order of first appearance.
    pub placeholders: Vec<String>,
}

/// Opens a `.docx`.
///
/// What is promised: everything the model represents comes back as
/// structure, and everything else comes back as the XML it was. What is
/// *not* promised is a byte-identical re-save — attribute order, relation
/// ids and namespace declarations are ours once the document is written
/// again.
pub fn open(bytes: &[u8]) -> Result<Opened> {
    let pkg = Package::open(bytes)?;
    let main = pkg
        .part("word/document.xml")?
        .ok_or_else(|| anyhow!("not a Word document: word/document.xml is missing"))?;
    let (rels, dir) = read_rels(&pkg, "word/document.xml")?;

    let mut doc = Document {
        styles: styles::parse_styles(pkg.part("word/styles.xml")?.as_ref()),
        numbering: styles::parse_numbering(pkg.part("word/numbering.xml")?.as_ref()),
        sections: Vec::new(),
        ..Document::default()
    };
    if let Some(fonts) = pkg.part("word/fontTable.xml")? {
        doc.fonts = fonts
            .children_named("w:font")
            .filter_map(|f| f.attr("w:name").map(str::to_string))
            .collect();
    }

    let mut walker = walker::Walker::new(&pkg, &rels, &dir);

    // The body is a flat list; a `w:sectPr` closes a section, either as a
    // child of the body (the last one) or inside the paragraph properties
    // of the paragraph that ends it.
    let body = main
        .child("w:body")
        .ok_or_else(|| anyhow!("the document has no body"))?;
    let mut pending: Vec<Block> = Vec::new();
    let mut raw_sections: Vec<(Vec<Block>, Option<Element>)> = Vec::new();
    for child in body.elements() {
        match child.name.as_str() {
            "w:sectPr" => {
                raw_sections.push((std::mem::take(&mut pending), Some(child.clone())));
            }
            "w:p" => {
                let closing = child
                    .child("w:pPr")
                    .and_then(|ppr| ppr.child("w:sectPr"))
                    .cloned();
                let one = Element {
                    name: "w:body".into(),
                    attrs: Vec::new(),
                    children: vec![crate::ooxml::Node::Element(child.clone())],
                };
                pending.extend(walker.blocks(&one));
                if let Some(sect) = closing {
                    raw_sections.push((std::mem::take(&mut pending), Some(sect)));
                }
            }
            _ => {
                let one = Element {
                    name: "w:body".into(),
                    attrs: Vec::new(),
                    children: vec![crate::ooxml::Node::Element(child.clone())],
                };
                pending.extend(walker.blocks(&one));
            }
        }
    }
    if !pending.is_empty() || raw_sections.is_empty() {
        raw_sections.push((pending, None));
    }

    // Footnotes: the separator entries (ids 0 and -1) are Word's own
    // furniture, not notes anybody wrote.
    if let Some(root) = pkg.part("word/footnotes.xml")? {
        for note in root.children_named("w:footnote") {
            let Some(id) = note.attr("w:id") else { continue };
            if matches!(
                note.attr("w:type"),
                Some("separator") | Some("continuationSeparator") | Some("continuationNotice")
            ) {
                continue;
            }
            let blocks = walker.blocks(note);
            doc.footnotes.insert(format!("n{id}"), blocks);
        }
    }

    // Comments.
    if let Some(root) = pkg.part("word/comments.xml")? {
        for c in root.children_named("w:comment") {
            let Some(id) = c.attr("w:id") else { continue };
            let text = c
                .children_named("w:p")
                .map(|p| p.text())
                .collect::<Vec<_>>()
                .join("\n");
            doc.comments.insert(
                format!("c{id}"),
                Comment {
                    author: c.attr("w:author").unwrap_or_default().to_string(),
                    date: c.attr("w:date").map(str::to_string),
                    text: text.trim().to_string(),
                    parent_id: None,
                    done: false,
                },
            );
        }
    }

    // Sections, with their running heads.
    let default_page = raw_sections
        .last()
        .and_then(|(_, s)| s.as_ref())
        .map(page_of)
        .unwrap_or_default();
    doc.page = default_page.clone();

    for (blocks, sect) in raw_sections {
        let page = sect.as_ref().map(page_of);
        let (headers, footers) = match &sect {
            Some(sect) => running_heads(&pkg, &rels, &dir, sect, &mut walker)?,
            None => (RunningHeads::default(), RunningHeads::default()),
        };
        doc.sections.push(Section {
            page: page.filter(|p| *p != default_page),
            headers,
            footers,
            body: blocks,
        });
    }
    if doc.sections.is_empty() {
        doc.sections.push(Section::default());
    }

    let collected = walker.collected;
    doc.opaque = collected.opaque;
    doc.fields.extend(collected.fields);

    // The parts an opaque fragment points at travel with the document.
    let mut parts = BTreeMap::new();
    for name in &collected.parts {
        if let Some(bytes) = pkg.raw(name) {
            parts.insert(name.clone(), bytes.to_vec());
        }
    }

    let placeholders = placeholder_names(&doc);
    Ok(Opened {
        document: doc,
        images: collected.images,
        parts,
        warnings: collected.warnings,
        opaque_count: collected.opaque_count,
        placeholders,
    })
}

/// Placeholder names, each once, in the order the fields were created —
/// which is the order they appear in the document.
fn placeholder_names(doc: &Document) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    let mut ids: Vec<&String> = doc.fields.keys().collect();
    // f2 before f10: the ids are sequential, so sort them numerically.
    ids.sort_by_key(|id| id[1..].parse::<u32>().unwrap_or(u32::MAX));
    for id in ids {
        if let Some(key) = doc.fields[id].key.as_ref() {
            if seen.insert(key.clone()) {
                out.push(key.clone());
            }
        }
    }
    out
}

fn page_of(sect: &Element) -> Page {
    let mut page = Page::default();
    if let Some(size) = sect.child("w:pgSz") {
        if let Some(w) = size.attr("w:w").and_then(|v| v.parse().ok()) {
            page.width = w;
        }
        if let Some(h) = size.attr("w:h").and_then(|v| v.parse().ok()) {
            page.height = h;
        }
        if size.attr("w:orient") == Some("landscape") {
            page.orientation = Orientation::Landscape;
        }
    }
    if let Some(m) = sect.child("w:pgMar") {
        let read = |name: &str, fallback: i32| {
            m.attr(name).and_then(|v| v.parse().ok()).unwrap_or(fallback)
        };
        page.margins = Margins {
            top: read("w:top", 1134),
            right: read("w:right", 1134),
            bottom: read("w:bottom", 1134),
            left: read("w:left", 1134),
            header: read("w:header", 709),
            footer: read("w:footer", 709),
        };
    }
    page
}

/// Reads the header and footer parts a section refers to.
fn running_heads(
    pkg: &Package,
    rels: &[crate::ooxml::Rel],
    dir: &str,
    sect: &Element,
    walker: &mut walker::Walker,
) -> Result<(RunningHeads, RunningHeads)> {
    let mut headers = RunningHeads::default();
    let mut footers = RunningHeads::default();

    for (tag, slot) in [("w:headerReference", true), ("w:footerReference", false)] {
        for reference in sect.children_named(tag) {
            let Some(rid) = reference.attr("r:id") else {
                continue;
            };
            let Some(rel) = rels.iter().find(|r| r.id == rid) else {
                continue;
            };
            let part_name = crate::ooxml::resolve_target(dir, &rel.target);
            let Some(root) = pkg
                .part(&part_name)
                .with_context(|| format!("reading {part_name}"))?
            else {
                continue;
            };
            let blocks = walker.blocks(&root);
            let target = if slot { &mut headers } else { &mut footers };
            match reference.attr("w:type").unwrap_or("default") {
                "first" => target.first = Some(blocks),
                "even" => target.even = Some(blocks),
                _ => target.default = Some(blocks),
            }
        }
    }
    Ok((headers, footers))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal `.docx` in memory from the body XML.
    fn docx(body: &str) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::FileOptions<'_, ()> =
                zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            use std::io::Write;
            zip.start_file("[Content_Types].xml", opts).unwrap();
            zip.write_all(br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/></Types>"#).unwrap();
            zip.start_file("word/document.xml", opts).unwrap();
            let doc = format!(
                r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
            );
            zip.write_all(doc.as_bytes()).unwrap();
            zip.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn a_file_that_is_not_a_word_document_is_refused() {
        let err = open(b"not a zip at all").unwrap_err().to_string();
        assert!(err.contains("zip") || err.contains(".docx"), "{err}");
    }

    #[test]
    fn a_zip_without_a_document_part_is_refused() {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let zip = zip::ZipWriter::new(&mut buf);
            zip.finish().unwrap();
        }
        let err = open(&buf.into_inner()).unwrap_err().to_string();
        assert!(err.contains("document.xml"), "{err}");
    }

    #[test]
    fn a_plain_document_opens_with_one_section() {
        let bytes = docx(r#"<w:p><w:r><w:t>ciao</w:t></w:r></w:p>"#);
        let opened = open(&bytes).unwrap();
        assert_eq!(opened.document.sections.len(), 1);
        assert_eq!(opened.document.text(), "ciao");
        assert_eq!(opened.opaque_count, 0);
    }

    #[test]
    fn the_section_properties_give_the_page_setup() {
        let bytes = docx(
            r#"<w:p><w:r><w:t>x</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="16838" w:h="11906" w:orient="landscape"/><w:pgMar w:top="1000" w:right="900" w:bottom="1000" w:left="900" w:header="500" w:footer="500"/></w:sectPr>"#,
        );
        let opened = open(&bytes).unwrap();
        let page = &opened.document.page;
        assert_eq!((page.width, page.height), (16838, 11906));
        assert_eq!(page.orientation, Orientation::Landscape);
        assert_eq!(page.margins.left, 900);
        assert_eq!(page.margins.header, 500);
    }

    #[test]
    fn a_paragraph_that_closes_a_section_splits_the_document() {
        let bytes = docx(
            r#"<w:p><w:pPr><w:sectPr><w:pgSz w:w="11906" w:h="16838"/></w:sectPr></w:pPr><w:r><w:t>prima</w:t></w:r></w:p><w:p><w:r><w:t>seconda</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="11906" w:h="16838"/></w:sectPr>"#,
        );
        let opened = open(&bytes).unwrap();
        assert_eq!(opened.document.sections.len(), 2);
        assert_eq!(opened.document.sections[0].body.len(), 1);
        assert_eq!(opened.document.sections[1].body.len(), 1);
    }

    #[test]
    fn placeholders_are_listed_in_the_order_they_appear() {
        let bytes = docx(
            r#"<w:p><w:r><w:t>{{cliente}} e {{oggetto}}</w:t></w:r></w:p><w:p><w:r><w:t>{{cliente}} di nuovo</w:t></w:r></w:p>"#,
        );
        let opened = open(&bytes).unwrap();
        assert_eq!(opened.placeholders, ["cliente", "oggetto"]);
    }

    #[test]
    fn footnote_separators_are_not_notes() {
        // Word keeps its own separator entries in footnotes.xml; showing
        // them as notes would put two empty notes in every document.
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            use std::io::Write;
            let mut zip = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            zip.start_file("word/document.xml", opts).unwrap();
            zip.write_all(br#"<w:document xmlns:w="x"><w:body><w:p/></w:body></w:document>"#).unwrap();
            zip.start_file("word/footnotes.xml", opts).unwrap();
            zip.write_all(br#"<w:footnotes xmlns:w="x"><w:footnote w:id="0" w:type="separator"><w:p/></w:footnote><w:footnote w:id="2"><w:p><w:r><w:t>la nota</w:t></w:r></w:p></w:footnote></w:footnotes>"#).unwrap();
            zip.finish().unwrap();
        }
        let opened = open(&buf.into_inner()).unwrap();
        assert_eq!(opened.document.footnotes.len(), 1);
        assert!(opened.document.footnotes.contains_key("n2"));
    }
}
