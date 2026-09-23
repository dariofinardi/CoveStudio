// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
//! The document tree back into `word/document.xml`.
//!
//! Opaque fragments are written **in place**, as themselves. The
//! JavaScript original had to emit a marker and swap it for the real XML
//! afterwards, because a library stood between it and the file; writing
//! the XML ourselves means the fragment simply goes where it belongs, with
//! no second pass and no marker that could survive into a delivered
//! document.
//!
//! What this file must never do is invent. A property the model does not
//! carry is not written, so a run that inherited its font from a style
//! still inherits it after a round trip.

use std::collections::BTreeMap;

use crate::model::{
    Block, Borders, Cell, Document, Field, FieldKind, Inline, Page, ParagraphProps, Row, RunProps,
    Section, TableProps,
};
use crate::ooxml::{parse_xml, Element, Node};

/// Relationship ids assigned while rendering, so the package writer can
/// declare them.
pub struct Rendered {
    pub root: Element,
    /// New relationships this part needs: id → (type URI, target, mode).
    pub rels: Vec<(String, String, String, Option<String>)>,
    /// Package parts that must be copied in, by name.
    pub parts: Vec<String>,
}

/// Renders the body of a document part.
pub struct Renderer<'a> {
    doc: &'a Document,
    images: &'a BTreeMap<String, crate::parse::Image>,
    rel_seq: u32,
    pub rels: Vec<(String, String, String, Option<String>)>,
    pub parts: Vec<String>,
}

impl<'a> Renderer<'a> {
    pub fn new(doc: &'a Document, images: &'a BTreeMap<String, crate::parse::Image>) -> Self {
        Self {
            doc,
            images,
            rel_seq: 0,
            rels: Vec::new(),
            parts: Vec::new(),
        }
    }

    fn next_rel_id(&mut self) -> String {
        self.rel_seq += 1;
        // Prefixed so these can never collide with ids we copy from an
        // opaque fragment's own package.
        format!("rIdDr{}", self.rel_seq)
    }

    /// `w:body` for the main document part.
    pub fn body(&mut self) -> Element {
        let mut body = Element::new("w:body");
        let last = self.doc.sections.len().saturating_sub(1);
        for (i, section) in self.doc.sections.iter().enumerate() {
            for block in &section.body {
                self.block(block, &mut body);
            }
            let page = section.page.clone().unwrap_or_else(|| self.doc.page.clone());
            let sect_pr = self.section_properties(section, &page, i);
            if i == last {
                // The last section's properties belong to the body.
                body.children.push(Node::Element(sect_pr));
            } else {
                // Any earlier section is closed by an empty paragraph
                // carrying its properties — that is how Word marks the
                // boundary, and a document without it collapses into one
                // section on the next save.
                let ppr = Element::new("w:pPr").with_child(sect_pr);
                body.children
                    .push(Node::Element(Element::new("w:p").with_child(ppr)));
            }
        }
        body
    }

    /// Blocks of a header, footer or footnote part.
    pub fn blocks_into(&mut self, blocks: &[Block], root_name: &str) -> Element {
        let mut root = Element::new(root_name);
        for block in blocks {
            self.block(block, &mut root);
        }
        root
    }

    fn block(&mut self, block: &Block, into: &mut Element) {
        match block {
            Block::Paragraph { style, props, runs } => {
                into.children
                    .push(Node::Element(self.paragraph(style.as_deref(), props.as_ref(), runs)));
            }
            Block::Table { style, props, rows } => {
                into.children
                    .push(Node::Element(self.table(style.as_deref(), props, rows)));
            }
            Block::PageBreak => {
                let br = Element::new("w:br").with_attr("w:type", "page");
                let run = Element::new("w:r").with_child(br);
                into.children
                    .push(Node::Element(Element::new("w:p").with_child(run)));
            }
            Block::Opaque { id } => {
                if let Some(kept) = self.doc.opaque.get(id) {
                    self.write_opaque(kept, into);
                }
            }
        }
    }

    /// Writes a kept fragment, rewriting the relationship ids it uses.
    fn write_opaque(&mut self, kept: &crate::model::Opaque, into: &mut Element) {
        let mut xml = kept.xml.clone();
        for (old_id, rel) in &kept.rels {
            let new_id = self.next_rel_id();
            // Only in attribute position: the id string could otherwise
            // appear inside text and be corrupted.
            for attr in ["r:embed", "r:id", "r:link", "r:href", "r:pict"] {
                xml = xml.replace(
                    &format!("{attr}=\"{old_id}\""),
                    &format!("{attr}=\"{new_id}\""),
                );
            }
            self.rels.push((
                new_id,
                rel.kind.clone(),
                rel.target.clone(),
                rel.mode.clone(),
            ));
            if let Some(part) = &rel.part_name {
                self.parts.push(part.clone());
            }
        }
        // A fragment that no longer parses would poison the whole
        // document; dropping it loses content but keeps the file
        // openable, and the count of opaque fragments tells the caller.
        if let Ok(element) = parse_xml(xml.as_bytes()) {
            let element = if kept.in_run {
                Element::new("w:r").with_child(element)
            } else {
                element
            };
            into.children.push(Node::Element(element));
        }
    }

    fn paragraph(
        &mut self,
        style: Option<&str>,
        props: Option<&ParagraphProps>,
        runs: &[Inline],
    ) -> Element {
        let mut p = Element::new("w:p");
        if let Some(ppr) = paragraph_props_xml(style, props) {
            p.children.push(Node::Element(ppr));
        }
        for inline in runs {
            self.inline(inline, &mut p);
        }
        p
    }

    fn inline(&mut self, inline: &Inline, into: &mut Element) {
        match inline {
            Inline::Text { text, style, props } => {
                let run = self.text_run(text, style.as_deref(), props.as_ref());
                // Tracked changes wrap the run rather than decorate it.
                let wrapped = match props.as_ref() {
                    Some(p) if p.del.is_some() => {
                        Some(revision_wrapper("w:del", p.del.as_ref().unwrap(), run.clone()))
                    }
                    Some(p) if p.ins.is_some() => {
                        Some(revision_wrapper("w:ins", p.ins.as_ref().unwrap(), run.clone()))
                    }
                    _ => None,
                };
                into.children.push(Node::Element(wrapped.unwrap_or(run)));
            }
            Inline::Tab => {
                into.children.push(Node::Element(
                    Element::new("w:r").with_child(Element::new("w:tab")),
                ));
            }
            Inline::Break => {
                into.children.push(Node::Element(
                    Element::new("w:r").with_child(Element::new("w:br")),
                ));
            }
            Inline::FootnoteRef { id } => {
                let number = id.trim_start_matches('n');
                let reference = Element::new("w:footnoteReference").with_attr("w:id", number);
                let rpr = Element::new("w:rPr").with_child(
                    Element::new("w:rStyle").with_attr("w:val", "FootnoteReference"),
                );
                into.children.push(Node::Element(
                    Element::new("w:r").with_child(rpr).with_child(reference),
                ));
            }
            Inline::Field { id } => {
                let field = self.doc.fields.get(id);
                into.children.push(Node::Element(self.field_run(field)));
            }
            Inline::Image {
                src,
                width,
                height,
                alt,
            } => {
                if let Some(run) = self.image_run(src, *width, *height, alt.as_deref()) {
                    into.children.push(Node::Element(run));
                }
            }
            Inline::OpaqueInline { id } => {
                if let Some(kept) = self.doc.opaque.get(id).cloned() {
                    self.write_opaque(&kept, into);
                }
            }
        }
    }

    fn text_run(&self, text: &str, style: Option<&str>, props: Option<&RunProps>) -> Element {
        let mut run = Element::new("w:r");
        if let Some(rpr) = run_props_xml(style, props) {
            run.children.push(Node::Element(rpr));
        }
        // Deleted text lives in `w:delText`: writing it as `w:t` would
        // quietly accept someone's tracked deletion.
        let tag = match props {
            Some(p) if p.del.is_some() => "w:delText",
            _ => "w:t",
        };
        let mut t = Element::new(tag);
        // Leading or trailing spaces are content, and Word drops them
        // without this attribute.
        if text.starts_with(' ') || text.ends_with(' ') || text.contains("  ") {
            t = t.with_attr("xml:space", "preserve");
        }
        run.children.push(Node::Element(t.with_text(text)));
        run
    }

    /// A field: computed ones become real Word fields, the rest become the
    /// text they currently read.
    fn field_run(&self, field: Option<&Field>) -> Element {
        let Some(field) = field else {
            return Element::new("w:r");
        };
        match field.kind {
            FieldKind::PageNumber | FieldKind::PageCount => {
                let instr = if field.kind == FieldKind::PageNumber {
                    " PAGE "
                } else {
                    " NUMPAGES "
                };
                let text = Element::new("w:t").with_text(&field.value);
                Element::new("w:fldSimple")
                    .with_attr("w:instr", instr)
                    .with_child(Element::new("w:r").with_child(text))
            }
            // A placeholder that was never filled is written back as
            // `{{key}}` so the template stays a template: a round trip
            // must not consume its own fields.
            FieldKind::Placeholder => {
                let shown = if field.value.is_empty() {
                    field
                        .key
                        .as_ref()
                        .map(|k| format!("{{{{{k}}}}}"))
                        .unwrap_or_default()
                } else {
                    field.value.clone()
                };
                self.text_run(&shown, None, None)
            }
            // A date or a sum is written as its value, deliberately
            // frozen: recomputing it on open would change a document
            // somebody signed.
            FieldKind::Date | FieldKind::ColumnSum => self.text_run(&field.value, None, None),
        }
    }

    fn image_run(
        &mut self,
        src: &str,
        width: u64,
        height: u64,
        alt: Option<&str>,
    ) -> Option<Element> {
        let image = self.images.get(src)?;
        let rel_id = self.next_rel_id();
        self.rels.push((
            rel_id.clone(),
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image".to_string(),
            image.part_name.trim_start_matches("word/").to_string(),
            None,
        ));
        self.parts.push(image.part_name.clone());

        let blip = Element::new("a:blip").with_attr("r:embed", &rel_id);
        let stretch = Element::new("a:stretch").with_child(Element::new("a:fillRect"));
        let blip_fill = Element::new("pic:blipFill")
            .with_child(blip)
            .with_child(stretch);
        let nv = Element::new("pic:nvPicPr")
            .with_child(
                Element::new("pic:cNvPr")
                    .with_attr("id", "0")
                    .with_attr("name", src)
                    .with_attr("descr", alt.unwrap_or("")),
            )
            .with_child(Element::new("pic:cNvPicPr"));
        let xfrm = Element::new("a:xfrm")
            .with_child(
                Element::new("a:off")
                    .with_attr("x", "0")
                    .with_attr("y", "0"),
            )
            .with_child(
                Element::new("a:ext")
                    .with_attr("cx", width.to_string())
                    .with_attr("cy", height.to_string()),
            );
        let geom = Element::new("a:prstGeom")
            .with_attr("prst", "rect")
            .with_child(Element::new("a:avLst"));
        let sp_pr = Element::new("pic:spPr").with_child(xfrm).with_child(geom);
        let pic = Element::new("pic:pic")
            .with_attr(
                "xmlns:pic",
                "http://schemas.openxmlformats.org/drawingml/2006/picture",
            )
            .with_child(nv)
            .with_child(blip_fill)
            .with_child(sp_pr);
        let graphic_data = Element::new("a:graphicData")
            .with_attr(
                "uri",
                "http://schemas.openxmlformats.org/drawingml/2006/picture",
            )
            .with_child(pic);
        let graphic = Element::new("a:graphic")
            .with_attr(
                "xmlns:a",
                "http://schemas.openxmlformats.org/drawingml/2006/main",
            )
            .with_child(graphic_data);
        let inline_el = Element::new("wp:inline")
            .with_attr("distT", "0")
            .with_attr("distB", "0")
            .with_attr("distL", "0")
            .with_attr("distR", "0")
            .with_child(
                Element::new("wp:extent")
                    .with_attr("cx", width.to_string())
                    .with_attr("cy", height.to_string()),
            )
            .with_child(
                Element::new("wp:docPr")
                    .with_attr("id", "1")
                    .with_attr("name", src)
                    .with_attr("descr", alt.unwrap_or("")),
            )
            .with_child(graphic);
        let drawing = Element::new("w:drawing").with_child(inline_el);
        Some(Element::new("w:r").with_child(drawing))
    }

    fn table(&mut self, style: Option<&str>, props: &TableProps, rows: &[Row]) -> Element {
        let mut tbl = Element::new("w:tbl");
        let mut tbl_pr = Element::new("w:tblPr");
        if let Some(style) = style {
            tbl_pr.children.push(Node::Element(
                Element::new("w:tblStyle").with_attr("w:val", style),
            ));
        }
        if let Some(width) = props.width {
            tbl_pr.children.push(Node::Element(
                Element::new("w:tblW")
                    .with_attr("w:w", width.to_string())
                    .with_attr(
                        "w:type",
                        if props.layout.as_deref() == Some("pct") {
                            "pct"
                        } else {
                            "dxa"
                        },
                    ),
            ));
        }
        if let Some(align) = &props.align {
            tbl_pr.children.push(Node::Element(
                Element::new("w:jc").with_attr("w:val", align_value(align)),
            ));
        }
        if let Some(borders) = &props.borders {
            tbl_pr
                .children
                .push(Node::Element(borders_xml("w:tblBorders", borders)));
        }
        if !tbl_pr.children.is_empty() {
            tbl.children.push(Node::Element(tbl_pr));
        }
        if !props.grid.is_empty() {
            let mut grid = Element::new("w:tblGrid");
            for w in &props.grid {
                grid.children.push(Node::Element(
                    Element::new("w:gridCol").with_attr("w:w", w.to_string()),
                ));
            }
            tbl.children.push(Node::Element(grid));
        }

        // A vertical span is written back as a "restart" cell plus one
        // continuation cell per extra row — the shape Word reads.
        let mut carry: BTreeMap<usize, (usize, Cell)> = BTreeMap::new();
        for row in rows {
            let mut tr = Element::new("w:tr");
            if let Some(rp) = &row.props {
                let mut tr_pr = Element::new("w:trPr");
                if rp.header {
                    tr_pr.children.push(Node::Element(Element::new("w:tblHeader")));
                }
                if rp.cant_split {
                    tr_pr.children.push(Node::Element(Element::new("w:cantSplit")));
                }
                if let Some(h) = rp.height {
                    tr_pr.children.push(Node::Element(
                        Element::new("w:trHeight").with_attr("w:val", h.to_string()),
                    ));
                }
                if !tr_pr.children.is_empty() {
                    tr.children.push(Node::Element(tr_pr));
                }
            }

            let mut column = 0usize;
            let mut cells = row.cells.iter();
            loop {
                // A span opened in an earlier row occupies this column.
                if let Some((left, original)) = carry.remove(&column) {
                    let span = original
                        .props
                        .as_ref()
                        .and_then(|p| p.colspan)
                        .unwrap_or(1)
                        .max(1) as usize;
                    tr.children
                        .push(Node::Element(self.cell(&original, Some("continue"))));
                    if left > 1 {
                        carry.insert(column, (left - 1, original));
                    }
                    column += span;
                    continue;
                }
                let Some(cell) = cells.next() else { break };
                let span = cell
                    .props
                    .as_ref()
                    .and_then(|p| p.colspan)
                    .unwrap_or(1)
                    .max(1) as usize;
                let rowspan = cell.props.as_ref().and_then(|p| p.rowspan).unwrap_or(1);
                let merge = (rowspan > 1).then_some("restart");
                tr.children.push(Node::Element(self.cell(cell, merge)));
                if rowspan > 1 {
                    carry.insert(column, (rowspan as usize - 1, cell.clone()));
                }
                column += span;
            }
            tbl.children.push(Node::Element(tr));
        }
        tbl
    }

    fn cell(&mut self, cell: &Cell, merge: Option<&str>) -> Element {
        let mut tc = Element::new("w:tc");
        let mut tc_pr = Element::new("w:tcPr");
        if let Some(props) = &cell.props {
            if let Some(w) = props.width {
                tc_pr.children.push(Node::Element(
                    Element::new("w:tcW")
                        .with_attr("w:w", w.to_string())
                        .with_attr("w:type", "dxa"),
                ));
            }
            if let Some(span) = props.colspan.filter(|s| *s > 1) {
                tc_pr.children.push(Node::Element(
                    Element::new("w:gridSpan").with_attr("w:val", span.to_string()),
                ));
            }
            if let Some(shading) = &props.shading {
                tc_pr.children.push(Node::Element(
                    Element::new("w:shd")
                        .with_attr("w:val", "clear")
                        .with_attr("w:color", "auto")
                        .with_attr("w:fill", shading),
                ));
            }
            if let Some(borders) = &props.borders {
                tc_pr
                    .children
                    .push(Node::Element(borders_xml("w:tcBorders", borders)));
            }
            if let Some(v) = &props.v_align {
                tc_pr.children.push(Node::Element(
                    Element::new("w:vAlign").with_attr("w:val", v),
                ));
            }
        }
        if let Some(kind) = merge {
            let el = Element::new("w:vMerge");
            let el = if kind == "restart" {
                el.with_attr("w:val", "restart")
            } else {
                el
            };
            tc_pr.children.push(Node::Element(el));
        }
        if !tc_pr.children.is_empty() {
            tc.children.push(Node::Element(tc_pr));
        }

        // A continuation cell shows nothing of its own, and every cell
        // needs at least one paragraph or Word calls the file corrupt.
        if merge == Some("continue") {
            tc.children
                .push(Node::Element(Element::new("w:p")));
            return tc;
        }
        if cell.blocks.is_empty() {
            tc.children.push(Node::Element(Element::new("w:p")));
        } else {
            for block in &cell.blocks {
                self.block(block, &mut tc);
            }
            let has_paragraph = cell
                .blocks
                .iter()
                .any(|b| matches!(b, Block::Paragraph { .. }));
            if !has_paragraph {
                tc.children.push(Node::Element(Element::new("w:p")));
            }
        }
        tc
    }

    fn section_properties(&self, section: &Section, page: &Page, index: usize) -> Element {
        let mut sect = Element::new("w:sectPr");
        for (tag, heads) in [
            ("w:headerReference", &section.headers),
            ("w:footerReference", &section.footers),
        ] {
            for (kind, present) in [
                ("default", heads.default.is_some()),
                ("first", heads.first.is_some()),
                ("even", heads.even.is_some()),
            ] {
                if present {
                    sect.children.push(Node::Element(
                        Element::new(tag)
                            .with_attr("w:type", kind)
                            .with_attr("r:id", header_rel_id(tag, kind, index)),
                    ));
                }
            }
        }
        if section.headers.first.is_some() || section.footers.first.is_some() {
            sect.children
                .push(Node::Element(Element::new("w:titlePg")));
        }
        let mut size = Element::new("w:pgSz")
            .with_attr("w:w", page.width.to_string())
            .with_attr("w:h", page.height.to_string());
        if page.orientation == crate::model::Orientation::Landscape {
            size = size.with_attr("w:orient", "landscape");
        }
        sect.children.push(Node::Element(size));
        sect.children.push(Node::Element(
            Element::new("w:pgMar")
                .with_attr("w:top", page.margins.top.to_string())
                .with_attr("w:right", page.margins.right.to_string())
                .with_attr("w:bottom", page.margins.bottom.to_string())
                .with_attr("w:left", page.margins.left.to_string())
                .with_attr("w:header", page.margins.header.to_string())
                .with_attr("w:footer", page.margins.footer.to_string()),
        ));
        sect
    }
}

/// Relationship id of a running-head part. Deterministic, so the part
/// name, the relationship and the reference always agree.
pub fn header_rel_id(tag: &str, kind: &str, section: usize) -> String {
    let what = if tag.starts_with("w:header") { "hdr" } else { "ftr" };
    format!("rId{what}{section}{kind}")
}

fn align_value(align: &str) -> &str {
    match align {
        "justify" => "both",
        other => other,
    }
}

fn revision_wrapper(tag: &str, revision: &crate::model::Revision, run: Element) -> Element {
    let mut el = Element::new(tag)
        .with_attr("w:id", "1")
        .with_attr("w:author", &revision.author);
    if let Some(date) = &revision.date {
        el = el.with_attr("w:date", date);
    }
    el.with_child(run)
}

pub fn run_props_xml(style: Option<&str>, props: Option<&RunProps>) -> Option<Element> {
    if style.is_none() && props.is_none() {
        return None;
    }
    let mut rpr = Element::new("w:rPr");
    if let Some(style) = style {
        rpr.children.push(Node::Element(
            Element::new("w:rStyle").with_attr("w:val", style),
        ));
    }
    if let Some(p) = props {
        if let Some(font) = &p.font {
            rpr.children.push(Node::Element(
                Element::new("w:rFonts")
                    .with_attr("w:ascii", font)
                    .with_attr("w:hAnsi", font),
            ));
        }
        // A toggle that is off must be written explicitly: leaving it out
        // would let the style turn it back on.
        for (tag, value) in [
            ("w:b", p.bold),
            ("w:i", p.italic),
            ("w:strike", p.strike),
            ("w:caps", p.caps),
        ] {
            if let Some(on) = value {
                let el = Element::new(tag);
                rpr.children.push(Node::Element(if on {
                    el
                } else {
                    el.with_attr("w:val", "0")
                }));
            }
        }
        if let Some(on) = p.underline {
            rpr.children.push(Node::Element(
                Element::new("w:u").with_attr("w:val", if on { "single" } else { "none" }),
            ));
        }
        if let Some(size) = p.size {
            rpr.children.push(Node::Element(
                Element::new("w:sz").with_attr("w:val", size.to_string()),
            ));
        }
        if let Some(color) = &p.color {
            rpr.children.push(Node::Element(
                Element::new("w:color").with_attr("w:val", color),
            ));
        }
        if let Some(highlight) = &p.highlight {
            rpr.children.push(Node::Element(
                Element::new("w:highlight").with_attr("w:val", highlight),
            ));
        }
        if let Some(v) = &p.vert_align {
            rpr.children.push(Node::Element(
                Element::new("w:vertAlign").with_attr("w:val", v),
            ));
        }
    }
    (!rpr.children.is_empty()).then_some(rpr)
}

pub fn paragraph_props_xml(style: Option<&str>, props: Option<&ParagraphProps>) -> Option<Element> {
    if style.is_none() && props.is_none() {
        return None;
    }
    let mut ppr = Element::new("w:pPr");
    if let Some(style) = style {
        ppr.children.push(Node::Element(
            Element::new("w:pStyle").with_attr("w:val", style),
        ));
    }
    if let Some(p) = props {
        if let Some(n) = &p.numbering {
            ppr.children.push(Node::Element(
                Element::new("w:numPr")
                    .with_child(Element::new("w:ilvl").with_attr("w:val", n.level.to_string()))
                    .with_child(Element::new("w:numId").with_attr("w:val", &n.id)),
            ));
        }
        if p.keep_next {
            ppr.children.push(Node::Element(Element::new("w:keepNext")));
        }
        if p.keep_lines {
            ppr.children.push(Node::Element(Element::new("w:keepLines")));
        }
        if p.page_break_before {
            ppr.children
                .push(Node::Element(Element::new("w:pageBreakBefore")));
        }
        if let Some(shading) = &p.shading {
            ppr.children.push(Node::Element(
                Element::new("w:shd")
                    .with_attr("w:val", "clear")
                    .with_attr("w:color", "auto")
                    .with_attr("w:fill", shading),
            ));
        }
        if let Some(borders) = &p.borders {
            ppr.children
                .push(Node::Element(borders_xml("w:pBdr", borders)));
        }
        let mut spacing = Element::new("w:spacing");
        let mut any_spacing = false;
        if let Some(v) = p.spacing_before {
            spacing = spacing.with_attr("w:before", v.to_string());
            any_spacing = true;
        }
        if let Some(v) = p.spacing_after {
            spacing = spacing.with_attr("w:after", v.to_string());
            any_spacing = true;
        }
        if let Some(v) = p.line_spacing {
            spacing = spacing
                .with_attr("w:line", v.to_string())
                .with_attr("w:lineRule", p.line_rule.as_deref().unwrap_or("auto"));
            any_spacing = true;
        }
        if any_spacing {
            ppr.children.push(Node::Element(spacing));
        }
        let mut ind = Element::new("w:ind");
        let mut any_ind = false;
        for (attr, value) in [
            ("w:left", p.indent_left),
            ("w:right", p.indent_right),
            ("w:firstLine", p.first_line),
            ("w:hanging", p.hanging),
        ] {
            if let Some(v) = value {
                ind = ind.with_attr(attr, v.to_string());
                any_ind = true;
            }
        }
        if any_ind {
            ppr.children.push(Node::Element(ind));
        }
        if let Some(align) = &p.align {
            ppr.children.push(Node::Element(
                Element::new("w:jc").with_attr("w:val", align_value(align)),
            ));
        }
    }
    (!ppr.children.is_empty()).then_some(ppr)
}

fn borders_xml(tag: &str, borders: &Borders) -> Element {
    let mut el = Element::new(tag);
    for (name, border) in [
        ("w:top", &borders.top),
        ("w:left", &borders.left),
        ("w:bottom", &borders.bottom),
        ("w:right", &borders.right),
        ("w:insideH", &borders.inside_h),
        ("w:insideV", &borders.inside_v),
    ] {
        if let Some(b) = border {
            let mut side = Element::new(name)
                .with_attr("w:val", b.style.as_deref().unwrap_or("single"))
                .with_attr("w:sz", b.size.unwrap_or(4).to_string());
            if let Some(color) = &b.color {
                side = side.with_attr("w:color", color);
            }
            el.children.push(Node::Element(side));
        }
    }
    el
}
