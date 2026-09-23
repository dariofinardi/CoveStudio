// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
//! What changed between two versions of a document.
//!
//! Not a byte diff: two saves of the same document differ in ways nobody
//! wrote (relationship ids, attribute order), and a reader confronted with
//! those stops reading. This compares what a person would compare — the
//! text of each paragraph, where it sits, and the style it carries — and
//! reports only what moved.
//!
//! Two decisions shape the output, both about being read rather than
//! being clever:
//!
//! * **Unchanged paragraphs are counted, never listed.** A comparison
//!   that prints the whole document to show three changes hides them.
//! * **A deletion immediately followed by an insertion in the same place
//!   is one rewrite**, not two events. That is what actually happened,
//!   and it lets the reader see the before and after side by side.

use crate::model::{Block, Document, Inline};

/// One paragraph, flattened for comparison.
#[derive(Debug, Clone, PartialEq)]
pub struct Para {
    /// Where it is, in words: `corpo`, `intestazione`, `nota 2`,
    /// `corpo/tabella r2c1`.
    pub place: String,
    pub text: String,
    pub style: Option<String>,
    /// Every run in it is a tracked deletion: the paragraph is on its way
    /// out and should not read as ordinary text.
    pub only_deleted: bool,
}

/// What happened to one paragraph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Added,
    Removed,
    Rewritten,
}

/// One line of the comparison.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub change: Change,
    pub place: String,
    pub before: String,
    pub after: String,
    /// Word-level detail, for a rewrite.
    pub pieces: Vec<Piece>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub kind: PieceKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieceKind {
    Same,
    Removed,
    Added,
}

/// The whole comparison.
#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    pub rows: Vec<Row>,
    /// Paragraphs that did not change. Counted so a reader can tell "three
    /// changes in a long document" from "three changes in a short one".
    pub unchanged: usize,
    pub before_paragraphs: usize,
    pub after_paragraphs: usize,
}

/// Flattens a document into comparable paragraphs, in reading order.
pub fn paragraphs(doc: &Document) -> Vec<Para> {
    let mut out = Vec::new();
    let many = doc.sections.len() > 1;
    for (i, section) in doc.sections.iter().enumerate() {
        let suffix = if many { format!(" {}", i + 1) } else { String::new() };
        for (kind, blocks) in [
            ("intestazione", section.headers.default.as_ref()),
            ("intestazione (prima pagina)", section.headers.first.as_ref()),
            ("intestazione (pagine pari)", section.headers.even.as_ref()),
        ] {
            if let Some(blocks) = blocks {
                collect(blocks, &format!("{kind}{suffix}"), &mut out);
            }
        }
        collect(&section.body, &format!("corpo{suffix}"), &mut out);
        for (kind, blocks) in [
            ("piè di pagina", section.footers.default.as_ref()),
            ("piè di pagina (prima pagina)", section.footers.first.as_ref()),
            ("piè di pagina (pagine pari)", section.footers.even.as_ref()),
        ] {
            if let Some(blocks) = blocks {
                collect(blocks, &format!("{kind}{suffix}"), &mut out);
            }
        }
    }
    for (id, blocks) in &doc.footnotes {
        collect(blocks, &format!("nota {}", id.trim_start_matches('n')), &mut out);
    }
    out
}

fn collect(blocks: &[Block], place: &str, out: &mut Vec<Para>) {
    for block in blocks {
        match block {
            Block::Paragraph { style, runs, .. } => {
                let mut text = String::new();
                let mut any = false;
                let mut all_deleted = true;
                for run in runs {
                    match run {
                        Inline::Text { text: t, props, .. } => {
                            any = true;
                            if props.as_ref().and_then(|p| p.del.as_ref()).is_none() {
                                all_deleted = false;
                            }
                            text.push_str(t);
                        }
                        Inline::Tab => text.push('\t'),
                        _ => {}
                    }
                }
                out.push(Para {
                    place: place.to_string(),
                    text,
                    style: style.clone(),
                    only_deleted: any && all_deleted,
                });
            }
            Block::Table { rows, .. } => {
                for (r, row) in rows.iter().enumerate() {
                    for (c, cell) in row.cells.iter().enumerate() {
                        collect(
                            &cell.blocks,
                            &format!("{place}/tabella r{}c{}", r + 1, c + 1),
                            out,
                        );
                    }
                }
            }
            // A page break is worth showing as a line of its own: moving
            // one changes the document for the reader even though no word
            // changed.
            Block::PageBreak => out.push(Para {
                place: place.to_string(),
                text: "— interruzione di pagina —".to_string(),
                style: None,
                only_deleted: false,
            }),
            Block::Opaque { .. } => {}
        }
    }
}

/// Splits text into words *and* the whitespace between them, so joining
/// the pieces back reproduces the input exactly.
pub fn words(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut in_space: Option<bool> = None;
    for (i, ch) in text.char_indices() {
        let space = ch.is_whitespace();
        match in_space {
            None => in_space = Some(space),
            Some(was) if was != space => {
                out.push(&text[start..i]);
                start = i;
                in_space = Some(space);
            }
            _ => {}
        }
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// One step of an alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Same,
    Remove,
    Add,
}

/// Longest-common-subsequence alignment of two sequences.
///
/// The tie-break matters: when removing and adding are equally good, this
/// removes first, so a rewritten passage reads as "the old text, then the
/// new" rather than as an interleaving nobody can follow.
///
/// `ceiling` keeps the quadratic table from swallowing a machine on two
/// long documents; beyond it the answer degrades to "all of the first
/// removed, all of the second added", which is true and useless — but so
/// is an editor that stops responding.
fn align<T: PartialEq>(a: &[T], b: &[T], ceiling: usize) -> Vec<Step> {
    if a.len() > ceiling || b.len() > ceiling {
        let mut out = vec![Step::Remove; a.len()];
        out.extend(std::iter::repeat_n(Step::Add, b.len()));
        return out;
    }
    let n = a.len();
    let m = b.len();
    let mut dp = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[at(i, j)] = if a[i] == b[j] {
                dp[at(i + 1, j + 1)] + 1
            } else {
                dp[at(i + 1, j)].max(dp[at(i, j + 1)])
            };
        }
    }
    let mut out = Vec::with_capacity(n.max(m));
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if a[i] == b[j] {
            out.push(Step::Same);
            i += 1;
            j += 1;
        } else if dp[at(i + 1, j)] >= dp[at(i, j + 1)] {
            out.push(Step::Remove);
            i += 1;
        } else {
            out.push(Step::Add);
            j += 1;
        }
    }
    while i < n {
        out.push(Step::Remove);
        i += 1;
    }
    while j < m {
        out.push(Step::Add);
        j += 1;
    }
    out
}

/// Word-level difference between two strings.
pub fn text_diff(before: &str, after: &str) -> Vec<Piece> {
    let a = words(before);
    let b = words(after);
    let steps = align(&a, &b, 4000);
    let mut out: Vec<Piece> = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    for step in steps {
        let (kind, text) = match step {
            Step::Same => {
                let t = a[i];
                i += 1;
                j += 1;
                (PieceKind::Same, t)
            }
            Step::Remove => {
                let t = a[i];
                i += 1;
                (PieceKind::Removed, t)
            }
            Step::Add => {
                let t = b[j];
                j += 1;
                (PieceKind::Added, t)
            }
        };
        // Adjacent pieces of the same kind are one piece: a reader wants
        // "questo testo è cambiato", not a word-by-word ticker.
        match out.last_mut() {
            Some(last) if last.kind == kind => last.text.push_str(text),
            _ => out.push(Piece {
                kind,
                text: text.to_string(),
            }),
        }
    }
    out
}

/// Compares two versions of a document.
pub fn compare(before: &Document, after: &Document) -> Comparison {
    let a = paragraphs(before);
    let b = paragraphs(after);
    // Two paragraphs are the same when they are in the same place, say
    // the same thing, and carry the same style. Anything else is a change
    // somebody made.
    let key = |p: &Para| (p.place.clone(), p.text.clone(), p.style.clone());
    let ka: Vec<_> = a.iter().map(key).collect();
    let kb: Vec<_> = b.iter().map(key).collect();
    let steps = align(&ka, &kb, 4000);

    let mut rows = Vec::new();
    let mut unchanged = 0usize;
    let (mut i, mut j) = (0usize, 0usize);
    let mut k = 0usize;
    while k < steps.len() {
        match steps[k] {
            Step::Same => {
                unchanged += 1;
                i += 1;
                j += 1;
                k += 1;
            }
            Step::Remove => {
                // A removal followed by an addition in the same place is
                // one rewrite.
                let rewritten = steps
                    .get(k + 1)
                    .map(|s| *s == Step::Add)
                    .unwrap_or(false)
                    && b.get(j).map(|p| p.place.clone()) == Some(a[i].place.clone());
                if rewritten {
                    rows.push(Row {
                        change: Change::Rewritten,
                        place: a[i].place.clone(),
                        before: a[i].text.clone(),
                        after: b[j].text.clone(),
                        pieces: text_diff(&a[i].text, &b[j].text),
                    });
                    i += 1;
                    j += 1;
                    k += 2;
                } else {
                    rows.push(Row {
                        change: Change::Removed,
                        place: a[i].place.clone(),
                        before: a[i].text.clone(),
                        after: String::new(),
                        pieces: vec![Piece {
                            kind: PieceKind::Removed,
                            text: a[i].text.clone(),
                        }],
                    });
                    i += 1;
                    k += 1;
                }
            }
            Step::Add => {
                rows.push(Row {
                    change: Change::Added,
                    place: b[j].place.clone(),
                    before: String::new(),
                    after: b[j].text.clone(),
                    pieces: vec![Piece {
                        kind: PieceKind::Added,
                        text: b[j].text.clone(),
                    }],
                });
                j += 1;
                k += 1;
            }
        }
    }
    Comparison {
        rows,
        unchanged,
        before_paragraphs: a.len(),
        after_paragraphs: b.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Block, Cell, Row as TableRow, RunProps, Section};

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

    fn document(blocks: Vec<Block>) -> Document {
        Document {
            sections: vec![Section {
                body: blocks,
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn words_keep_the_spaces_between_them() {
        // Joining the pieces must give back the input, or a rebuilt
        // paragraph would lose its spacing.
        let text = "Il  contratto\tscade\ndomani";
        assert_eq!(words(text).concat(), text);
    }

    #[test]
    fn a_document_compared_with_itself_has_nothing_to_report() {
        let doc = document(vec![paragraph("uno"), paragraph("due")]);
        let out = compare(&doc, &doc);
        assert!(out.rows.is_empty());
        assert_eq!(out.unchanged, 2);
    }

    #[test]
    fn a_changed_paragraph_reads_as_one_rewrite() {
        let before = document(vec![paragraph("Il canone è 4.000 euro")]);
        let after = document(vec![paragraph("Il canone è 4.800 euro")]);
        let out = compare(&before, &after);
        assert_eq!(out.rows.len(), 1);
        assert_eq!(out.rows[0].change, Change::Rewritten);
        assert_eq!(out.rows[0].before, "Il canone è 4.000 euro");
        assert_eq!(out.rows[0].after, "Il canone è 4.800 euro");
    }

    #[test]
    fn the_detail_is_word_level_not_line_level() {
        let pieces = text_diff("Il canone è 4.000 euro", "Il canone è 4.800 euro");
        let kinds: Vec<PieceKind> = pieces.iter().map(|p| p.kind).collect();
        assert_eq!(
            kinds,
            [
                PieceKind::Same,
                PieceKind::Removed,
                PieceKind::Added,
                PieceKind::Same
            ],
            "{pieces:?}"
        );
        assert!(pieces[1].text.contains("4.000"));
        assert!(pieces[2].text.contains("4.800"));
    }

    #[test]
    fn additions_and_removals_are_told_apart() {
        let before = document(vec![paragraph("resta"), paragraph("sparisce")]);
        let after = document(vec![paragraph("resta"), paragraph("nuovo")]);
        let out = compare(&before, &after);
        assert_eq!(out.rows.len(), 1, "{:?}", out.rows);
        assert_eq!(out.rows[0].change, Change::Rewritten);

        let before = document(vec![paragraph("uno")]);
        let after = document(vec![paragraph("uno"), paragraph("due")]);
        let out = compare(&before, &after);
        assert_eq!(out.rows[0].change, Change::Added);
        assert_eq!(out.rows[0].after, "due");
    }

    #[test]
    fn the_place_of_a_change_is_said_in_words() {
        let before = Document {
            sections: vec![Section {
                headers: crate::model::RunningHeads {
                    default: Some(vec![paragraph("Studio Rossi")]),
                    ..Default::default()
                },
                body: vec![Block::Table {
                    style: None,
                    props: Default::default(),
                    rows: vec![TableRow {
                        props: None,
                        cells: vec![Cell {
                            props: None,
                            blocks: vec![paragraph("1.200")],
                        }],
                    }],
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let places: Vec<String> = paragraphs(&before).into_iter().map(|p| p.place).collect();
        assert_eq!(places, ["intestazione", "corpo/tabella r1c1"]);
    }

    #[test]
    fn a_paragraph_that_is_entirely_a_tracked_deletion_is_marked() {
        let doc = document(vec![Block::Paragraph {
            style: None,
            props: None,
            runs: vec![Inline::Text {
                text: "in uscita".into(),
                style: None,
                props: Some(RunProps {
                    del: Some(crate::model::Revision {
                        author: "Rossi".into(),
                        date: None,
                    }),
                    ..Default::default()
                }),
            }],
        }]);
        assert!(paragraphs(&doc)[0].only_deleted);
    }

    #[test]
    fn a_style_change_alone_is_a_change() {
        // The words are identical; the document is not.
        let before = document(vec![paragraph("Titolo")]);
        let after = document(vec![Block::Paragraph {
            style: Some("Heading1".into()),
            props: None,
            runs: vec![Inline::Text {
                text: "Titolo".into(),
                style: None,
                props: None,
            }],
        }]);
        let out = compare(&before, &after);
        assert_eq!(out.rows.len(), 1);
        assert_eq!(out.rows[0].change, Change::Rewritten);
    }

    #[test]
    fn unchanged_paragraphs_are_counted_and_not_listed() {
        let mut before_blocks: Vec<Block> = (0..50).map(|i| paragraph(&format!("riga {i}"))).collect();
        let mut after_blocks = before_blocks.clone();
        after_blocks[20] = paragraph("riga cambiata");
        before_blocks.push(paragraph("in fondo"));
        after_blocks.push(paragraph("in fondo"));

        let out = compare(&document(before_blocks), &document(after_blocks));
        assert_eq!(out.rows.len(), 1, "a long document with one change");
        assert_eq!(out.unchanged, 50);
    }

    #[test]
    fn two_very_long_documents_do_not_hang_the_comparison() {
        // The answer degrades rather than the machine.
        let a: Vec<String> = (0..5000).map(|i| format!("riga {i}")).collect();
        let b: Vec<String> = (0..5000).map(|i| format!("riga {}", i + 1)).collect();
        let steps = align(&a, &b, 4000);
        assert_eq!(steps.len(), 10_000);
    }
}
