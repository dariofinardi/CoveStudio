// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
//! The document tree: what an editor edits, and what a `.docx` is written
//! back from.
//!
//! Two rules shape every type here.
//!
//! **What is modelled is editable; what is not is kept.** A real company
//! template carries content this model has no opinion about — a text box, a
//! chart, a field nobody should touch. Rather than dropping it (which
//! impoverishes the document on every save) or refusing the file (which
//! makes the editor useless on the documents people actually have), the
//! original XML is stored in [`Document::opaque`] and written back where it
//! was. The editor shows it as an untouchable block.
//!
//! **Word's units, not ours.** Sizes are half-points, indents and spacing
//! are twentieths of a point (twips), image sizes are EMU. Converting to
//! millimetres on the way in and back on the way out would lose precision
//! and introduce rounding differences between a document and its own
//! re-save; a reader who wants millimetres can convert at the edge.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Format version of the serialised tree. Bumped when a change would make
/// an older reader misread a newer document.
pub const MODEL_VERSION: u32 = 1;

/// A whole document.
///
/// `BTreeMap` rather than `HashMap` throughout: serialising a document must
/// produce the same bytes twice, or a diff between two saves would show
/// changes nobody made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    #[serde(rename = "v")]
    pub version: u32,
    pub page: Page,
    #[serde(default)]
    pub styles: Styles,
    /// Fonts the template declares, in `fontTable.xml` order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fonts: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub numbering: BTreeMap<String, Numbering>,
    /// At least one. A document with no section cannot be written.
    pub sections: Vec<Section>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub footnotes: BTreeMap<String, Vec<Block>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, Field>,
    /// XML kept verbatim; see the module note.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub opaque: BTreeMap<String, Opaque>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub comments: BTreeMap<String, Comment>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            version: MODEL_VERSION,
            page: Page::default(),
            styles: Styles::default(),
            fonts: Vec::new(),
            numbering: BTreeMap::new(),
            sections: vec![Section::default()],
            footnotes: BTreeMap::new(),
            fields: BTreeMap::new(),
            opaque: BTreeMap::new(),
            comments: BTreeMap::new(),
        }
    }
}

impl Document {
    /// Every block of every section, headers and footers included, in the
    /// order they are written back.
    pub fn blocks(&self) -> impl Iterator<Item = &Block> {
        self.sections.iter().flat_map(|s| {
            s.headers
                .all()
                .chain(s.footers.all())
                .chain(s.body.iter())
        })
    }

    /// The document's text, paragraphs separated by newlines. Opaque
    /// content contributes nothing: it is not text we can claim to have
    /// read.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for section in &self.sections {
            for block in &section.body {
                block.write_text(&mut out);
            }
        }
        out.trim_end().to_string()
    }
}

/// Page geometry. Twips, like Word.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub width: u32,
    pub height: u32,
    pub margins: Margins,
    #[serde(default)]
    pub orientation: Orientation,
}

impl Default for Page {
    /// A4 portrait with 2 cm margins — the shape of nearly every document
    /// this will meet in Europe.
    fn default() -> Self {
        Self {
            width: 11906,
            height: 16838,
            margins: Margins::default(),
            orientation: Orientation::Portrait,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    #[default]
    Portrait,
    Landscape,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Margins {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
    #[serde(default)]
    pub header: i32,
    #[serde(default)]
    pub footer: i32,
}

impl Default for Margins {
    fn default() -> Self {
        // 2 cm sides, 1.25 cm for the running head.
        Self {
            top: 1134,
            right: 1134,
            bottom: 1134,
            left: 1134,
            header: 709,
            footer: 709,
        }
    }
}

/// Named styles, with the `basedOn` chain already resolved into effective
/// properties — but the names kept, because a document that loses its style
/// names stops being the company's document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Styles {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub paragraph: BTreeMap<String, ParagraphStyle>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub character: BTreeMap<String, RunProps>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub table: BTreeMap<String, TableProps>,
    #[serde(default)]
    pub defaults: RunProps,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ParagraphStyle {
    /// What Word shows in the styles gallery, when it differs from the id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub based_on: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paragraph: Option<ParagraphProps>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<RunProps>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Numbering {
    /// One entry per level, level 0 first.
    #[serde(default)]
    pub levels: Vec<NumberingLevel>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct NumberingLevel {
    /// `bullet`, `decimal`, `lowerLetter`, … as Word spells them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent_left: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hanging: Option<i32>,
}

/// One section: a page setup plus the running heads that go with it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Section {
    /// Only when this section differs from the document's page setup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<Page>,
    #[serde(default)]
    pub headers: RunningHeads,
    #[serde(default)]
    pub footers: RunningHeads,
    #[serde(default)]
    pub body: Vec<Block>,
}

/// The three running heads Word allows per section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RunningHeads {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Vec<Block>>,
    /// Used on the first page when the section asks for a different one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first: Option<Vec<Block>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub even: Option<Vec<Block>>,
}

impl RunningHeads {
    pub fn all(&self) -> impl Iterator<Item = &Block> {
        self.default
            .iter()
            .chain(self.first.iter())
            .chain(self.even.iter())
            .flat_map(|v| v.iter())
    }

    pub fn is_empty(&self) -> bool {
        self.default.is_none() && self.first.is_none() && self.even.is_none()
    }
}

/// Block-level content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Block {
    Paragraph {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        props: Option<ParagraphProps>,
        #[serde(default)]
        runs: Vec<Inline>,
    },
    Table {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<String>,
        #[serde(default)]
        props: TableProps,
        #[serde(default)]
        rows: Vec<Row>,
    },
    PageBreak,
    /// XML we kept rather than understood; the id looks it up in
    /// [`Document::opaque`].
    Opaque { id: String },
}

impl Block {
    fn write_text(&self, out: &mut String) {
        match self {
            Block::Paragraph { runs, .. } => {
                for r in runs {
                    r.write_text(out);
                }
                out.push('\n');
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        for b in &cell.blocks {
                            b.write_text(out);
                        }
                    }
                }
            }
            // A page break is not text, and opaque content is not text we
            // can honestly claim to have read.
            Block::PageBreak | Block::Opaque { .. } => {}
        }
    }
}

/// Inline content, inside a paragraph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Inline {
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        props: Option<RunProps>,
    },
    /// Reference to a footnote body in [`Document::footnotes`].
    FootnoteRef {
        id: String,
    },
    /// A placeholder or a live field; see [`Document::fields`].
    Field {
        id: String,
    },
    Image {
        /// Key into the image set that travels beside the document.
        src: String,
        /// EMU, like Word.
        width: u64,
        height: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alt: Option<String>,
    },
    Tab,
    Break,
    OpaqueInline {
        id: String,
    },
}

impl Inline {
    fn write_text(&self, out: &mut String) {
        match self {
            Inline::Text { text, .. } => out.push_str(text),
            Inline::Tab => out.push('\t'),
            Inline::Break => out.push('\n'),
            // A field's *value* is not in the tree; the renderer resolves
            // it. Writing its id here would put "f3" in the text.
            Inline::Field { .. }
            | Inline::FootnoteRef { .. }
            | Inline::Image { .. }
            | Inline::OpaqueInline { .. } => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Row {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub props: Option<RowProps>,
    #[serde(default)]
    pub cells: Vec<Cell>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RowProps {
    /// Repeats at the top of every page: `w:tblHeader`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub header: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cant_split: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Cell {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub props: Option<CellProps>,
    #[serde(default)]
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CellProps {
    /// Width in twips.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colspan: Option<u32>,
    /// Vertical merges arrive as "restart" plus continuations; the parser
    /// folds them into a span on the first cell, because that is how an
    /// editor has to show them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rowspan: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shading: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub borders: Option<Borders>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v_align: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TableProps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<i32>,
    /// `auto`, `fixed`, `pct`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grid: Vec<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub borders: Option<Borders>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Borders {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<Border>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<Border>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<Border>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<Border>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inside_h: Option<Border>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inside_v: Option<Border>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Border {
    /// `single`, `dashed`, `none`, … as Word spells it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    /// Eighths of a point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ParagraphProps {
    /// `left`, `center`, `right`, `justify`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing_before: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing_after: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_spacing: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_rule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent_left: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent_right: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_line: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hanging: Option<i32>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep_next: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep_lines: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub page_break_before: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shading: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub numbering: Option<NumberingRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub borders: Option<Borders>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NumberingRef {
    pub id: String,
    #[serde(default)]
    pub level: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RunProps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// Half-points, like Word: 24 is a 12-point body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strike: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caps: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight: Option<String>,
    /// `superscript` or `subscript`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vert_align: Option<String>,
    /// Tracked insertion: who and when.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ins: Option<Revision>,
    /// Tracked deletion. The text stays in the tree — that is the point of
    /// tracking — and the renderer writes it back as `w:del`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub del: Option<Revision>,
    /// Comments covering this run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Revision {
    pub author: String,
    /// ISO 8601, as Word writes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Comment {
    pub author: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    pub text: String,
    /// Replies carry the id of the comment they answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub done: bool,
}

/// A field: a placeholder to fill, or something Word computes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub kind: FieldKind,
    /// For a placeholder, the name between the braces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Whether Word recomputes it (page numbers) or it is written once.
    #[serde(default)]
    pub live: bool,
    /// What it currently reads.
    #[serde(default)]
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FieldKind {
    Placeholder,
    PageNumber,
    PageCount,
    Date,
    ColumnSum,
}

/// XML kept as it came.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Opaque {
    /// The OOXML tag it stands for (`w:pict`, `w:drawing`, `field`, …),
    /// so an editor can say what it is showing.
    pub kind: String,
    /// The original XML, serialised.
    pub xml: String,
    /// Sits inside a paragraph rather than between blocks.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inline: bool,
    /// Must be wrapped back in a `w:r` when written.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub in_run: bool,
    /// Relationships the XML refers to, by the id used inside it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub rels: BTreeMap<String, Relationship>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relationship {
    /// The relationship type URI.
    #[serde(rename = "type")]
    pub kind: String,
    pub target: String,
    /// `Internal` or `External`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Resolved package part name, for internal targets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paragraph(text: &str) -> Block {
        Block::Paragraph {
            style: None,
            props: None,
            runs: vec![Inline::Text {
                text: text.to_string(),
                style: None,
                props: None,
            }],
        }
    }

    #[test]
    fn a_new_document_has_one_section_and_can_be_written() {
        let d = Document::default();
        assert_eq!(d.version, MODEL_VERSION);
        assert_eq!(d.sections.len(), 1, "a document with no section cannot be written");
    }

    #[test]
    fn serialising_twice_gives_the_same_bytes() {
        // Two saves of an unchanged document must not differ, or every
        // version comparison would show phantom changes.
        let mut d = Document::default();
        d.sections[0].body = vec![paragraph("uno"), paragraph("due")];
        d.fields.insert(
            "f1".into(),
            Field {
                kind: FieldKind::Placeholder,
                key: Some("CLIENTE".into()),
                live: false,
                value: String::new(),
            },
        );
        d.opaque.insert(
            "o1".into(),
            Opaque {
                kind: "w:pict".into(),
                xml: "<w:pict/>".into(),
                inline: false,
                in_run: false,
                rels: BTreeMap::new(),
            },
        );
        let a = serde_json::to_string(&d).unwrap();
        let b = serde_json::to_string(&d).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn a_document_survives_a_trip_through_json() {
        let mut d = Document::default();
        d.sections[0].body = vec![
            paragraph("titolo"),
            Block::Table {
                style: Some("Griglia".into()),
                props: TableProps {
                    grid: vec![2000, 3000],
                    ..Default::default()
                },
                rows: vec![Row {
                    props: Some(RowProps {
                        header: true,
                        ..Default::default()
                    }),
                    cells: vec![
                        Cell {
                            props: None,
                            blocks: vec![paragraph("voce")],
                        },
                        Cell {
                            props: Some(CellProps {
                                colspan: Some(2),
                                ..Default::default()
                            }),
                            blocks: vec![paragraph("importo")],
                        },
                    ],
                }],
            },
            Block::PageBreak,
            Block::Opaque { id: "o1".into() },
        ];
        let json = serde_json::to_string(&d).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(d, back);
    }

    #[test]
    fn text_reads_paragraphs_and_cells_but_not_opaque_content() {
        let mut d = Document::default();
        d.sections[0].body = vec![
            paragraph("prima riga"),
            Block::Opaque { id: "o1".into() },
            paragraph("seconda riga"),
        ];
        // One newline per paragraph, and the opaque block contributes
        // nothing at all — not even a blank line for something we did not
        // read.
        assert_eq!(d.text(), "prima riga\nseconda riga");
    }

    #[test]
    fn a_field_contributes_no_text_under_its_own_id() {
        // Writing the id would put "f1" in the document's text, which is
        // the sort of thing that reaches a model and gets quoted back.
        let mut d = Document::default();
        d.sections[0].body = vec![Block::Paragraph {
            style: None,
            props: None,
            runs: vec![
                Inline::Text {
                    text: "Spett.le ".into(),
                    style: None,
                    props: None,
                },
                Inline::Field { id: "f1".into() },
            ],
        }];
        assert_eq!(d.text(), "Spett.le");
    }
}
