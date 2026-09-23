// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
//! Changing the text of a document without flattening it.
//!
//! The naive way to edit a `.docx` is to patch `<w:t>` elements in the
//! XML. It fails on the most ordinary case there is: Word splits a
//! sentence across runs whenever anything about it changes — a spelling
//! correction, a language mark, a tracked edit — so the phrase somebody
//! wants replaced exists in the document and in no single element of it.
//! The search finds nothing and reports success.
//!
//! Here the paragraph's text is reassembled, the change is made in *that*,
//! and the runs are rewritten around it. The replacement keeps the
//! formatting of the text it replaced, because replacing a bold clause
//! with an unformatted one is not what anybody asked for.

use crate::model::{Block, Document, Inline, RunProps};

/// One replacement to make.
#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    pub find: String,
    pub replace: String,
}

/// Applies every edit to the whole document, returning how many times
/// each one matched.
///
/// The count is the point: an edit that matched nothing is not a failure
/// of the document, it is a caller who asked for something that is not
/// there, and they can only know if we say so.
pub fn apply_text_edits(doc: &mut Document, edits: &[Edit]) -> Vec<usize> {
    let mut hits = vec![0usize; edits.len()];
    for section in &mut doc.sections {
        for blocks in [
            Some(&mut section.body),
            section.headers.default.as_mut(),
            section.headers.first.as_mut(),
            section.headers.even.as_mut(),
            section.footers.default.as_mut(),
            section.footers.first.as_mut(),
            section.footers.even.as_mut(),
        ]
        .into_iter()
        .flatten()
        {
            edit_blocks(blocks, edits, &mut hits);
        }
    }
    // Footnotes are text somebody wrote and will expect to be included.
    let ids: Vec<String> = doc.footnotes.keys().cloned().collect();
    for id in ids {
        if let Some(blocks) = doc.footnotes.get_mut(&id) {
            edit_blocks(blocks, edits, &mut hits);
        }
    }
    hits
}

fn edit_blocks(blocks: &mut [Block], edits: &[Edit], hits: &mut [usize]) {
    for block in blocks.iter_mut() {
        match block {
            Block::Paragraph { runs, .. } => {
                for (i, edit) in edits.iter().enumerate() {
                    hits[i] += replace_in_runs(runs, &edit.find, &edit.replace);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows.iter_mut() {
                    for cell in row.cells.iter_mut() {
                        edit_blocks(&mut cell.blocks, edits, hits);
                    }
                }
            }
            // A page break has no text, and an opaque fragment is
            // deliberately none of our business: editing inside XML we
            // did not understand is how a document gets corrupted.
            Block::PageBreak | Block::Opaque { .. } => {}
        }
    }
}

/// Replaces every occurrence of `find` in one paragraph's runs.
///
/// Returns the number of replacements. Non-text inlines (an image, a
/// footnote mark, a field) are barriers: a phrase that spans one was not
/// written as a phrase, and joining across it would move an image.
fn replace_in_runs(runs: &mut Vec<Inline>, find: &str, replace: &str) -> usize {
    if find.is_empty() {
        return 0;
    }
    let mut count = 0;
    let mut out: Vec<Inline> = Vec::with_capacity(runs.len());
    let mut group: Vec<Inline> = Vec::new();

    // Runs between two barriers form one editable stretch.
    let flush = |group: &mut Vec<Inline>, out: &mut Vec<Inline>, count: &mut usize| {
        if group.is_empty() {
            return;
        }
        let joined: String = group
            .iter()
            .map(|i| match i {
                Inline::Text { text, .. } => text.as_str(),
                _ => "",
            })
            .collect();
        if !joined.contains(find) {
            out.append(group);
            return;
        }
        let mut cursor = 0usize;
        let mut rebuilt: Vec<Inline> = Vec::new();
        while let Some(at) = joined[cursor..].find(find) {
            let start = cursor + at;
            let end = start + find.len();
            rebuilt.extend(slice_text(group, cursor, start));
            // The replacement inherits the formatting of the text it
            // replaces: the first run the match touches.
            let props = props_at(group, start);
            let style = style_at(group, start);
            if !replace.is_empty() {
                rebuilt.push(Inline::Text {
                    text: replace.to_string(),
                    style,
                    props,
                });
            }
            cursor = end;
            *count += 1;
        }
        rebuilt.extend(slice_text(group, cursor, joined.len()));
        group.clear();
        out.extend(rebuilt);
    };

    for inline in runs.drain(..) {
        match inline {
            Inline::Text { .. } => group.push(inline),
            other => {
                flush(&mut group, &mut out, &mut count);
                out.push(other);
            }
        }
    }
    flush(&mut group, &mut out, &mut count);
    *runs = out;
    count
}

/// The text inlines covering `[from, to)` of the joined string.
fn slice_text(group: &[Inline], from: usize, to: usize) -> Vec<Inline> {
    if to <= from {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut pos = 0usize;
    for inline in group {
        let Inline::Text { text, style, props } = inline else {
            continue;
        };
        let start = pos;
        let end = pos + text.len();
        pos = end;
        if end <= from || start >= to {
            continue;
        }
        let a = from.saturating_sub(start).min(text.len());
        let b = (to - start).min(text.len());
        // Cutting mid-character would produce invalid UTF-8; the
        // boundaries are moved outwards to the nearest char.
        let a = floor_boundary(text, a);
        let b = ceil_boundary(text, b);
        if a < b {
            out.push(Inline::Text {
                text: text[a..b].to_string(),
                style: style.clone(),
                props: props.clone(),
            });
        }
    }
    out
}

fn floor_boundary(text: &str, mut i: usize) -> usize {
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_boundary(text: &str, mut i: usize) -> usize {
    while i < text.len() && !text.is_char_boundary(i) {
        i += 1;
    }
    i
}

fn props_at(group: &[Inline], offset: usize) -> Option<RunProps> {
    let mut pos = 0usize;
    for inline in group {
        if let Inline::Text { text, props, .. } = inline {
            if offset < pos + text.len() {
                return props.clone();
            }
            pos += text.len();
        }
    }
    None
}

fn style_at(group: &[Inline], offset: usize) -> Option<String> {
    let mut pos = 0usize;
    for inline in group {
        if let Inline::Text { text, style, .. } = inline {
            if offset < pos + text.len() {
                return style.clone();
            }
            pos += text.len();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Block, Cell, Row, Section};

    fn text(s: &str) -> Inline {
        Inline::Text {
            text: s.to_string(),
            style: None,
            props: None,
        }
    }

    fn bold(s: &str) -> Inline {
        Inline::Text {
            text: s.to_string(),
            style: None,
            props: Some(RunProps {
                bold: Some(true),
                ..Default::default()
            }),
        }
    }

    fn paragraph(runs: Vec<Inline>) -> Block {
        Block::Paragraph {
            style: None,
            props: None,
            runs,
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

    fn edit(find: &str, replace: &str) -> Edit {
        Edit {
            find: find.into(),
            replace: replace.into(),
        }
    }

    #[test]
    fn a_phrase_split_across_runs_is_still_replaced() {
        // The case the old find-and-replace could not do, and the reason
        // this exists.
        let mut doc = document(vec![paragraph(vec![
            text("Il contratto scade il "),
            text("31 dicembre"),
            text(" 2026."),
        ])]);
        let hits = apply_text_edits(&mut doc, &[edit("scade il 31 dicembre", "scade il 30 giugno")]);
        assert_eq!(hits, [1]);
        assert_eq!(doc.text(), "Il contratto scade il 30 giugno 2026.");
    }

    #[test]
    fn the_replacement_keeps_the_formatting_it_replaced() {
        let mut doc = document(vec![paragraph(vec![text("Totale: "), bold("1.200 €")])]);
        apply_text_edits(&mut doc, &[edit("1.200 €", "1.350 €")]);
        let Block::Paragraph { runs, .. } = &doc.sections[0].body[0] else {
            panic!("expected a paragraph");
        };
        let replaced = runs
            .iter()
            .find_map(|r| match r {
                Inline::Text { text, props, .. } if text == "1.350 €" => props.clone(),
                _ => None,
            })
            .expect("the replacement is there");
        assert_eq!(replaced.bold, Some(true), "a bold amount stays bold");
    }

    #[test]
    fn every_occurrence_is_replaced_and_counted() {
        let mut doc = document(vec![
            paragraph(vec![text("Roma, Roma e ancora Roma")]),
            paragraph(vec![text("Roma")]),
        ]);
        let hits = apply_text_edits(&mut doc, &[edit("Roma", "Milano")]);
        assert_eq!(hits, [4]);
        assert!(!doc.text().contains("Roma"));
    }

    #[test]
    fn an_edit_that_matches_nothing_says_so() {
        // Silence here is how a caller believes a change was made.
        let mut doc = document(vec![paragraph(vec![text("niente da cambiare")])]);
        let hits = apply_text_edits(&mut doc, &[edit("inesistente", "x")]);
        assert_eq!(hits, [0]);
    }

    #[test]
    fn text_inside_tables_is_edited_too() {
        let mut doc = document(vec![Block::Table {
            style: None,
            props: Default::default(),
            rows: vec![Row {
                props: None,
                cells: vec![Cell {
                    props: None,
                    blocks: vec![paragraph(vec![text("Canone 1.200")])],
                }],
            }],
        }]);
        let hits = apply_text_edits(&mut doc, &[edit("1.200", "1.350")]);
        assert_eq!(hits, [1]);
        assert!(doc.text().contains("1.350"));
    }

    #[test]
    fn an_image_or_a_field_is_a_barrier_not_a_gap() {
        // Joining across an image would let a "phrase" match text that is
        // on either side of a picture, and rewriting it would move the
        // picture.
        let mut doc = document(vec![paragraph(vec![
            text("prima"),
            Inline::Image {
                src: "i1".into(),
                width: 10,
                height: 10,
                alt: None,
            },
            text("dopo"),
        ])]);
        let hits = apply_text_edits(&mut doc, &[edit("primadopo", "x")]);
        assert_eq!(hits, [0]);
        let Block::Paragraph { runs, .. } = &doc.sections[0].body[0] else {
            panic!("expected a paragraph");
        };
        assert!(matches!(runs[1], Inline::Image { .. }), "the image stays put");
    }

    #[test]
    fn opaque_content_is_never_edited() {
        // Editing inside XML we did not understand is how a document
        // stops opening.
        let mut doc = document(vec![
            Block::Opaque { id: "o1".into() },
            paragraph(vec![text("testo modificabile")]),
        ]);
        let hits = apply_text_edits(&mut doc, &[edit("testo", "parola")]);
        assert_eq!(hits, [1]);
        assert!(matches!(doc.sections[0].body[0], Block::Opaque { .. }));
    }

    #[test]
    fn an_empty_search_changes_nothing() {
        let mut doc = document(vec![paragraph(vec![text("intatto")])]);
        let hits = apply_text_edits(&mut doc, &[edit("", "x")]);
        assert_eq!(hits, [0]);
        assert_eq!(doc.text(), "intatto");
    }

    #[test]
    fn a_replacement_with_nothing_deletes_the_text() {
        let mut doc = document(vec![paragraph(vec![text("da togliere: questa parte")])]);
        apply_text_edits(&mut doc, &[edit(": questa parte", "")]);
        assert_eq!(doc.text(), "da togliere");
    }

    #[test]
    fn accented_text_is_cut_on_character_boundaries() {
        // Slicing a multi-byte character in half would produce invalid
        // UTF-8 and panic; this is the ordinary case in Italian.
        let mut doc = document(vec![paragraph(vec![
            text("La società è "),
            text("già costituita"),
        ])]);
        let hits = apply_text_edits(&mut doc, &[edit("è già", "non è ancora")]);
        assert_eq!(hits, [1]);
        assert_eq!(doc.text(), "La società non è ancora costituita");
    }

    #[test]
    fn headers_and_footnotes_are_edited_as_well() {
        let mut doc = Document {
            sections: vec![Section {
                headers: crate::model::RunningHeads {
                    default: Some(vec![paragraph(vec![text("Studio Rossi")])]),
                    ..Default::default()
                },
                body: vec![paragraph(vec![text("corpo")])],
                ..Default::default()
            }],
            ..Default::default()
        };
        doc.footnotes
            .insert("n1".into(), vec![paragraph(vec![text("nota di Rossi")])]);
        let hits = apply_text_edits(&mut doc, &[edit("Rossi", "Bianchi")]);
        assert_eq!(hits, [2], "the letterhead and the footnote both count");
    }
}
