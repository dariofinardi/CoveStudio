// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
//! Walking a document body into blocks.
//!
//! One rule decides everything here: **what is not understood is kept, not
//! dropped**. Every element this walker has no model for becomes an opaque
//! node carrying its own XML and the relationships that XML points at, so
//! the renderer can put it back exactly where it was. A template full of
//! text boxes, charts and content controls therefore survives an editor
//! that understands paragraphs and tables.
//!
//! The cost of that rule is that this file never returns an error for
//! content. It returns warnings — counts of what it could not model — so a
//! caller can tell the difference between "read in full" and "read enough
//! to show".

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{
    Block, Cell, CellProps, Inline, Opaque, Relationship, Revision, Row, RunProps,
};
use crate::ooxml::{resolve_target, Element, Package, Rel};

use super::fields::extract_placeholders;
use super::props::{self, VMerge};

/// An image found in the document, with the bytes to write back out.
#[derive(Debug, Clone)]
pub struct Image {
    pub part_name: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

/// Everything the walk collects besides the blocks themselves.
#[derive(Debug, Default)]
pub struct Collected {
    pub opaque: BTreeMap<String, Opaque>,
    pub fields: BTreeMap<String, crate::model::Field>,
    pub images: BTreeMap<String, Image>,
    /// Package parts the opaque XML refers to; they travel with the
    /// document so the renderer can copy them back.
    pub parts: BTreeSet<String>,
    pub warnings: Vec<String>,
    pub opaque_count: u32,
}

pub struct Walker<'a> {
    pkg: &'a Package,
    rels: &'a [Rel],
    dir: String,
    pub collected: Collected,
    opaque_seq: u32,
    field_seq: u32,
}

impl<'a> Walker<'a> {
    pub fn new(pkg: &'a Package, rels: &'a [Rel], dir: &str) -> Self {
        Self {
            pkg,
            rels,
            dir: dir.to_string(),
            collected: Collected::default(),
            opaque_seq: 0,
            field_seq: 0,
        }
    }

    /// Blocks of a container (`w:body`, `w:tc`, `w:hdr`, `w:ftr`).
    pub fn blocks(&mut self, container: &Element) -> Vec<Block> {
        let mut out = Vec::new();
        for child in container.elements() {
            match child.name.as_str() {
                "w:p" => out.push(self.paragraph(child)),
                "w:tbl" => out.push(self.table(child)),
                // The section marker is read by the caller, which knows
                // where one section ends and the next begins.
                "w:sectPr" => {}
                // Bookmarks and proofing marks carry no content; keeping
                // them as opaque blocks would litter the editor with
                // untouchable empties.
                "w:bookmarkStart" | "w:bookmarkEnd" | "w:proofErr" | "w:permStart"
                | "w:permEnd" => {}
                other => {
                    let id = self.opaque(child, other, false, false);
                    out.push(Block::Opaque { id });
                }
            }
        }
        out
    }

    fn paragraph(&mut self, p: &Element) -> Block {
        let ppr = p.child("w:pPr");
        let style = ppr.and_then(|e| e.val("w:pStyle")).map(str::to_string);
        let props = props::paragraph_props(ppr);

        let mut runs = Vec::new();
        for child in p.elements() {
            match child.name.as_str() {
                "w:pPr" => {}
                "w:r" => self.run(child, None, &mut runs),
                // A hyperlink is kept whole rather than unwrapped. The
                // model has no place for the address, and a document that
                // loses its links on the first save is exactly the silent
                // impoverishment this crate exists to prevent. The cost
                // is that an editor cannot change the text inside a link
                // — which is the right trade until the model carries one.
                "w:hyperlink" => {
                    let id = self.opaque(child, "w:hyperlink", true, false);
                    runs.push(Inline::OpaqueInline { id });
                }
                "w:ins" | "w:del" => {
                    let revision = Revision {
                        author: child.attr("w:author").unwrap_or_default().to_string(),
                        date: child.attr("w:date").map(str::to_string),
                    };
                    let inserted = child.name == "w:ins";
                    for r in child.children_named("w:r") {
                        self.run(r, Some((inserted, revision.clone())), &mut runs);
                    }
                }
                "w:bookmarkStart" | "w:bookmarkEnd" | "w:proofErr" | "w:commentRangeStart"
                | "w:commentRangeEnd" => {}
                "w:fldSimple" => {
                    let inline = self.simple_field(child);
                    runs.push(inline);
                }
                other => {
                    let id = self.opaque(child, other, true, false);
                    runs.push(Inline::OpaqueInline { id });
                }
            }
        }

        // A paragraph whose whole content is a page break is a page break:
        // keeping the empty paragraph around would add a blank line to the
        // document every time it is re-saved.
        let only_break = runs.len() == 1 && matches!(runs[0], Inline::Break)
            && p.elements()
                .filter(|e| e.name == "w:r")
                .flat_map(|r| r.elements())
                .any(|e| e.name == "w:br" && e.attr("w:type") == Some("page"));
        if only_break && props.is_none() && style.is_none() {
            return Block::PageBreak;
        }

        let (runs, fields) = extract_placeholders(runs, &mut self.field_seq);
        self.collected.fields.extend(fields);
        Block::Paragraph { style, props, runs }
    }

    fn run(&mut self, r: &Element, revision: Option<(bool, Revision)>, out: &mut Vec<Inline>) {
        let rpr = r.child("w:rPr");
        let style = rpr.and_then(|e| e.val("w:rStyle")).map(str::to_string);
        let mut base = props::run_props(rpr).unwrap_or_default();
        if let Some((inserted, rev)) = revision {
            if inserted {
                base.ins = Some(rev);
            } else {
                base.del = Some(rev);
            }
        }
        let props = if base == RunProps::default() {
            None
        } else {
            Some(base)
        };

        for child in r.elements() {
            match child.name.as_str() {
                "w:rPr" => {}
                // `w:delText` is text Word is keeping because a deletion is
                // tracked; dropping it would resolve someone's revision for
                // them.
                "w:t" | "w:delText" => out.push(Inline::Text {
                    text: child.text(),
                    style: style.clone(),
                    props: props.clone(),
                }),
                "w:tab" => out.push(Inline::Tab),
                "w:br" => out.push(Inline::Break),
                "w:footnoteReference" => {
                    if let Some(id) = child.attr("w:id") {
                        out.push(Inline::FootnoteRef {
                            id: format!("n{id}"),
                        });
                    }
                }
                "w:drawing" => match self.image(child) {
                    Some(image) => out.push(image),
                    None => {
                        let id = self.opaque(child, "w:drawing", true, true);
                        out.push(Inline::OpaqueInline { id });
                    }
                },
                "w:commentReference" => {}
                other => {
                    // Symbols, pictures, embedded objects: kept whole and
                    // re-wrapped in a run on the way out.
                    let id = self.opaque(child, other, true, true);
                    out.push(Inline::OpaqueInline { id });
                }
            }
        }
    }

    /// `w:fldSimple`: the simple form of a Word field.
    fn simple_field(&mut self, el: &Element) -> Inline {
        let instr = el.attr("w:instr").unwrap_or_default().trim().to_string();
        let kind = instr.split_whitespace().next().unwrap_or("").to_uppercase();
        let value = el
            .children_named("w:r")
            .map(|r| r.text())
            .collect::<String>();
        let modelled = match kind.as_str() {
            "PAGE" => Some(crate::model::FieldKind::PageNumber),
            "NUMPAGES" => Some(crate::model::FieldKind::PageCount),
            "DATE" | "TIME" => Some(crate::model::FieldKind::Date),
            _ => None,
        };
        match modelled {
            Some(kind) => {
                self.field_seq += 1;
                let id = format!("f{}", self.field_seq);
                self.collected.fields.insert(
                    id.clone(),
                    crate::model::Field {
                        kind,
                        key: None,
                        live: true,
                        value,
                    },
                );
                Inline::Field { id }
            }
            // TOC, REF, MERGEFIELD and the rest: we do not compute them,
            // so they are kept whole rather than frozen at their current
            // value.
            None => {
                let id = self.opaque(el, "field", true, false);
                Inline::OpaqueInline { id }
            }
        }
    }

    /// An inline image, if this drawing is one we can carry.
    fn image(&mut self, drawing: &Element) -> Option<Inline> {
        // Anchored drawings (text wrapping around them) carry positioning
        // we do not model; they stay opaque so the layout survives.
        let inline = drawing.child("wp:inline")?;
        let extent = inline.child("wp:extent")?;
        let width: u64 = extent.attr("cx")?.parse().ok()?;
        let height: u64 = extent.attr("cy")?.parse().ok()?;
        let embed = find_attr_deep(drawing, "r:embed")?;
        let rel = self.rels.iter().find(|r| r.id == embed)?;
        let part_name = resolve_target(&self.dir, &rel.target);
        let bytes = self.pkg.raw(&part_name)?.to_vec();
        let src = format!("img{}", self.collected.images.len() + 1);
        let alt = inline
            .child("wp:docPr")
            .and_then(|d| d.attr("descr").or_else(|| d.attr("name")))
            .map(str::to_string);
        self.collected.images.insert(
            src.clone(),
            Image {
                content_type: content_type_of(&part_name),
                part_name,
                bytes,
            },
        );
        Some(Inline::Image {
            src,
            width,
            height,
            alt,
        })
    }

    fn table(&mut self, t: &Element) -> Block {
        let props = props::table_props(t.child("w:tblPr"), t.child("w:tblGrid"));
        let style = t
            .child("w:tblPr")
            .and_then(|p| p.val("w:tblStyle"))
            .map(str::to_string);

        // Vertical merges arrive as a "restart" cell followed by
        // "continue" cells in the rows below. An editor needs the span on
        // the first cell instead, so the continuations are counted and
        // folded back into it.
        let mut rows: Vec<Row> = Vec::new();
        let mut open_merges: BTreeMap<usize, (usize, usize)> = BTreeMap::new();

        for tr in t.children_named("w:tr") {
            let row_props = props::row_props(tr.child("w:trPr"));
            let mut cells: Vec<Cell> = Vec::new();
            let mut column = 0usize;
            for tc in tr.children_named("w:tc") {
                let (cell_props, merge) = props::cell_props(tc.child("w:tcPr"));
                let span = cell_props
                    .as_ref()
                    .and_then(|p| p.colspan)
                    .unwrap_or(1)
                    .max(1) as usize;
                match merge {
                    VMerge::Continue => {
                        // Count it on the cell that started the merge and
                        // drop the placeholder cell.
                        if let Some((row_index, cell_index)) = open_merges.get(&column).copied() {
                            if let Some(first) = rows
                                .get_mut(row_index)
                                .and_then(|r| r.cells.get_mut(cell_index))
                            {
                                let props = first.props.get_or_insert_with(CellProps::default);
                                props.rowspan = Some(props.rowspan.unwrap_or(1) + 1);
                            }
                        }
                    }
                    VMerge::Restart => {
                        open_merges.insert(column, (rows.len(), cells.len()));
                        cells.push(Cell {
                            props: cell_props,
                            blocks: self.blocks(tc),
                        });
                    }
                    VMerge::None => {
                        open_merges.remove(&column);
                        cells.push(Cell {
                            props: cell_props,
                            blocks: self.blocks(tc),
                        });
                    }
                }
                column += span;
            }
            rows.push(Row {
                props: row_props,
                cells,
            });
        }
        Block::Table { style, props, rows }
    }

    /// Keeps an element verbatim, with the relationships its XML uses.
    pub fn opaque(&mut self, el: &Element, kind: &str, inline: bool, in_run: bool) -> String {
        self.opaque_seq += 1;
        let id = format!("o{}", self.opaque_seq);
        let mut rels = BTreeMap::new();
        collect_rel_ids(el, &mut |rid| {
            if rels.contains_key(rid) {
                return;
            }
            if let Some(rel) = self.rels.iter().find(|r| r.id == rid) {
                let part_name = (rel.mode != "External")
                    .then(|| resolve_target(&self.dir, &rel.target));
                if let Some(part) = &part_name {
                    self.collected.parts.insert(part.clone());
                }
                rels.insert(
                    rid.to_string(),
                    Relationship {
                        kind: rel.kind.clone(),
                        target: rel.target.clone(),
                        mode: Some(rel.mode.clone()),
                        part_name,
                    },
                );
            }
        });
        self.collected.opaque.insert(
            id.clone(),
            Opaque {
                kind: kind.to_string(),
                xml: el.to_xml(),
                inline,
                in_run,
                rels,
            },
        );
        self.collected.opaque_count += 1;
        id
    }
}

/// Every `r:`-prefixed relationship id inside a subtree.
fn collect_rel_ids(el: &Element, found: &mut impl FnMut(&str)) {
    for (name, value) in &el.attrs {
        if matches!(
            name.as_str(),
            "r:embed" | "r:id" | "r:link" | "r:href" | "r:pict" | "r:dm" | "r:lo" | "r:qs" | "r:cs"
        ) {
            found(value);
        }
    }
    for child in el.elements() {
        collect_rel_ids(child, found);
    }
}

/// First occurrence of an attribute anywhere in a subtree.
fn find_attr_deep(el: &Element, name: &str) -> Option<String> {
    if let Some(v) = el.attr(name) {
        return Some(v.to_string());
    }
    el.elements().find_map(|c| find_attr_deep(c, name))
}

fn content_type_of(part_name: &str) -> String {
    let ext = part_name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "svg" => "image/svg+xml",
        "emf" => "image/x-emf",
        "wmf" => "image/x-wmf",
        _ => "application/octet-stream",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ooxml::parse_xml;

    fn walk(xml: &str) -> (Vec<Block>, Collected) {
        // A package with nothing in it: these tests are about the walk,
        // not about parts.
        let empty = Package::open(&empty_zip()).unwrap();
        let rels: Vec<Rel> = Vec::new();
        let mut w = Walker::new(&empty, &rels, "word");
        let root = parse_xml(xml.as_bytes()).unwrap();
        let blocks = w.blocks(&root);
        (blocks, w.collected)
    }

    fn empty_zip() -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        let w = zip::ZipWriter::new(&mut buf);
        w.finish().unwrap();
        buf.into_inner()
    }

    fn texts(block: &Block) -> String {
        match block {
            Block::Paragraph { runs, .. } => runs
                .iter()
                .filter_map(|r| match r {
                    Inline::Text { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect(),
            _ => String::new(),
        }
    }

    #[test]
    fn paragraphs_and_their_styles_come_through() {
        let (blocks, _) = walk(
            r#"<w:body><w:p><w:pPr><w:pStyle w:val="Titolo1"/></w:pPr><w:r><w:t>Contratto</w:t></w:r></w:p></w:body>"#,
        );
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            Block::Paragraph { style, runs, .. } => {
                assert_eq!(style.as_deref(), Some("Titolo1"));
                assert_eq!(runs.len(), 1);
            }
            other => panic!("expected a paragraph, got {other:?}"),
        }
    }

    #[test]
    fn what_we_do_not_model_is_kept_rather_than_dropped() {
        // The whole point of the crate.
        let (blocks, collected) = walk(
            r#"<w:body><w:p><w:r><w:t>prima</w:t></w:r></w:p><w:customXml><w:p><w:r><w:t>dentro</w:t></w:r></w:p></w:customXml></w:body>"#,
        );
        assert_eq!(blocks.len(), 2);
        let Block::Opaque { id } = &blocks[1] else {
            panic!("expected the unknown element to be kept: {:?}", blocks[1]);
        };
        let kept = &collected.opaque[id];
        assert_eq!(kept.kind, "w:customXml");
        assert!(kept.xml.contains("dentro"), "the content must survive");
    }

    #[test]
    fn a_text_box_stays_inside_its_paragraph() {
        let (blocks, collected) = walk(
            r#"<w:body><w:p><w:r><w:pict><v:shape/></w:pict></w:r></w:p></w:body>"#,
        );
        let Block::Paragraph { runs, .. } = &blocks[0] else {
            panic!("expected a paragraph");
        };
        let Inline::OpaqueInline { id } = &runs[0] else {
            panic!("expected an inline opaque, got {:?}", runs[0]);
        };
        let kept = &collected.opaque[id];
        assert!(kept.inline && kept.in_run, "it must go back inside a run");
        assert_eq!(kept.kind, "w:pict");
    }

    #[test]
    fn bookmarks_do_not_become_untouchable_blocks() {
        // They carry no content; keeping them would litter the editor.
        let (blocks, collected) = walk(
            r#"<w:body><w:bookmarkStart w:id="0" w:name="inizio"/><w:p><w:r><w:t>testo</w:t></w:r></w:p><w:bookmarkEnd w:id="0"/></w:body>"#,
        );
        assert_eq!(blocks.len(), 1);
        assert_eq!(collected.opaque_count, 0);
    }

    #[test]
    fn a_page_break_paragraph_becomes_a_page_break() {
        let (blocks, _) = walk(
            r#"<w:body><w:p><w:r><w:br w:type="page"/></w:r></w:p><w:p><w:r><w:t>dopo</w:t></w:r></w:p></w:body>"#,
        );
        assert!(matches!(blocks[0], Block::PageBreak), "{:?}", blocks[0]);
        assert_eq!(texts(&blocks[1]), "dopo", "no empty paragraph is left behind");
    }

    #[test]
    fn tracked_deletions_keep_their_text_and_their_author() {
        let (blocks, _) = walk(
            r#"<w:body><w:p><w:del w:author="Rossi" w:date="2026-09-01T10:00:00Z"><w:r><w:delText>tolto</w:delText></w:r></w:del></w:p></w:body>"#,
        );
        let Block::Paragraph { runs, .. } = &blocks[0] else {
            panic!("expected a paragraph");
        };
        let Inline::Text { text, props, .. } = &runs[0] else {
            panic!("expected text, got {:?}", runs[0]);
        };
        assert_eq!(text, "tolto", "deleted text stays until someone accepts");
        assert_eq!(props.as_ref().unwrap().del.as_ref().unwrap().author, "Rossi");
    }

    #[test]
    fn a_vertical_merge_becomes_a_rowspan_on_the_first_cell() {
        let (blocks, _) = walk(
            r#"<w:body><w:tbl>
                <w:tr><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>unita</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc></w:tr>
                <w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc><w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr>
            </w:tbl></w:body>"#,
        );
        let Block::Table { rows, .. } = &blocks[0] else {
            panic!("expected a table");
        };
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].cells[0].props.as_ref().unwrap().rowspan,
            Some(2),
            "the span belongs to the cell that starts it"
        );
        assert_eq!(
            rows[1].cells.len(),
            1,
            "the continuation cell is not a cell of its own"
        );
    }

    #[test]
    fn a_header_row_and_a_column_span_survive() {
        let (blocks, _) = walk(
            r#"<w:body><w:tbl><w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="2000"/></w:tblGrid>
                <w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>intestazione</w:t></w:r></w:p></w:tc></w:tr>
            </w:tbl></w:body>"#,
        );
        let Block::Table { rows, props, .. } = &blocks[0] else {
            panic!("expected a table");
        };
        assert!(rows[0].props.as_ref().unwrap().header);
        assert_eq!(rows[0].cells[0].props.as_ref().unwrap().colspan, Some(2));
        assert_eq!(props.grid, vec![2000, 2000]);
    }

    #[test]
    fn page_and_date_fields_are_modelled_and_the_rest_is_kept() {
        let (blocks, collected) = walk(
            r#"<w:body><w:p><w:fldSimple w:instr=" PAGE "><w:r><w:t>3</w:t></w:r></w:fldSimple><w:fldSimple w:instr=" TOC \o "><w:r><w:t>indice</w:t></w:r></w:fldSimple></w:p></w:body>"#,
        );
        let Block::Paragraph { runs, .. } = &blocks[0] else {
            panic!("expected a paragraph");
        };
        let Inline::Field { id } = &runs[0] else {
            panic!("expected a field, got {:?}", runs[0]);
        };
        assert_eq!(
            collected.fields[id].kind,
            crate::model::FieldKind::PageNumber
        );
        assert!(
            matches!(runs[1], Inline::OpaqueInline { .. }),
            "a table of contents is not something we recompute: {:?}",
            runs[1]
        );
    }

    #[test]
    fn a_hyperlink_keeps_its_address() {
        // Unwrapping the runs would keep the words and lose the link —
        // a document silently impoverished on every save.
        let (blocks, collected) = walk(
            r#"<w:body><w:p><w:hyperlink r:id="rId7"><w:r><w:t>il sito</w:t></w:r></w:hyperlink></w:p></w:body>"#,
        );
        let Block::Paragraph { runs, .. } = &blocks[0] else {
            panic!("expected a paragraph");
        };
        let Inline::OpaqueInline { id } = &runs[0] else {
            panic!("expected the link to be kept whole: {:?}", runs[0]);
        };
        let kept = &collected.opaque[id];
        assert_eq!(kept.kind, "w:hyperlink");
        assert!(kept.xml.contains("il sito"), "the words are still there");
        assert!(kept.xml.contains("rId7"), "and so is the address");
    }

    #[test]
    fn a_footnote_reference_keeps_its_id() {
        let (blocks, _) = walk(
            r#"<w:body><w:p><w:r><w:footnoteReference w:id="4"/></w:r></w:p></w:body>"#,
        );
        let Block::Paragraph { runs, .. } = &blocks[0] else {
            panic!("expected a paragraph");
        };
        assert_eq!(runs[0], Inline::FootnoteRef { id: "n4".into() });
    }

    #[test]
    fn placeholders_split_across_runs_are_found_in_a_real_paragraph() {
        let (blocks, collected) = walk(
            r#"<w:body><w:p><w:r><w:t xml:space="preserve">Spett.le {{</w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>cliente</w:t></w:r><w:r><w:t>}}</w:t></w:r></w:p></w:body>"#,
        );
        let Block::Paragraph { runs, .. } = &blocks[0] else {
            panic!("expected a paragraph");
        };
        assert!(runs.iter().any(|r| matches!(r, Inline::Field { .. })));
        let keys: Vec<&str> = collected
            .fields
            .values()
            .filter_map(|f| f.key.as_deref())
            .collect();
        assert_eq!(keys, ["cliente"]);
    }
}
