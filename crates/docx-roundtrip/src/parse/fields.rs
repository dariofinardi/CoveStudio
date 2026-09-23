// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
//! Placeholders, and the reason they are harder than they look.
//!
//! A template says `{{cliente}}`. Word does not keep that in one run: the
//! moment someone corrects the spelling, changes the language or moves the
//! cursor through it, the braces end up split across two or three runs —
//! `{{`, `cliente`, `}}` — each with its own properties. Searching run by
//! run finds nothing, and the template silently loses its fields.
//!
//! So the paragraph's text is reassembled, the placeholders are found in
//! *that*, and the runs are rewritten around them. Non-text inlines (an
//! image, a footnote mark) take one placeholder character in the
//! reassembled string so that offsets keep matching the run list.

use std::collections::BTreeMap;

use crate::model::{Field, FieldKind, Inline, RunProps};

/// Stands in for a non-text inline while scanning. U+0000 cannot appear in
/// XML text, so it can never collide with the document's own content.
const SENTINEL: char = '\u{0}';

/// Finds `{{ name }}` with optional inner spaces; the name is restricted
/// to what a template author can type without ambiguity.
fn find_placeholder(haystack: &str, from: usize) -> Option<(usize, usize, String)> {
    let start = haystack[from..].find("{{")? + from;
    let end_rel = haystack[start + 2..].find("}}")?;
    let end = start + 2 + end_rel + 2;
    let key = haystack[start + 2..end - 2].trim();
    if key.is_empty()
        || !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
    {
        // Not a placeholder: `{{` can legitimately appear in a document
        // about code or mathematics. Keep looking after it.
        return find_placeholder(haystack, start + 2);
    }
    Some((start, end, key.to_string()))
}

/// Rewrites a run list so that every placeholder becomes a field.
///
/// Returns the new list and the fields it created. The field inherits the
/// properties of the first text run it touched: a placeholder written in
/// bold must produce bold text once filled, or filling a template silently
/// changes its typography.
pub fn extract_placeholders(
    runs: Vec<Inline>,
    next_id: &mut u32,
) -> (Vec<Inline>, BTreeMap<String, Field>) {
    let mut joined = String::new();
    for inline in &runs {
        match inline {
            Inline::Text { text, .. } => joined.push_str(text),
            _ => joined.push(SENTINEL),
        }
    }
    let mut fields = BTreeMap::new();
    if !joined.contains("{{") {
        return (runs, fields);
    }

    let mut out: Vec<Inline> = Vec::with_capacity(runs.len());
    let mut cursor = 0usize;
    let mut search_from = 0usize;

    while let Some((start, end, key)) = find_placeholder(&joined, search_from) {
        // A placeholder split across a non-text inline is not a
        // placeholder — an image in the middle of the braces means the
        // author put it there.
        if joined[start..end].contains(SENTINEL) {
            search_from = start + 2;
            continue;
        }
        out.extend(slice_runs(&runs, cursor, start));
        let props = props_at(&runs, start);
        let id = format!("f{}", {
            *next_id += 1;
            *next_id
        });
        fields.insert(
            id.clone(),
            Field {
                kind: FieldKind::Placeholder,
                key: Some(key),
                // Linked by default: filling the document from data keeps
                // updating it until someone edits the text by hand.
                live: true,
                value: String::new(),
            },
        );
        // The field carries the formatting of the text it replaced; the
        // renderer applies it when the value is written.
        if let Some(props) = props {
            out.push(Inline::Text {
                text: String::new(),
                style: None,
                props: Some(props),
            });
            // An empty text run beside the field would be noise: fold the
            // properties into the field's own run by dropping it again and
            // relying on the renderer's default. Kept explicit here so the
            // intent survives a later change.
            out.pop();
        }
        out.push(Inline::Field { id });
        cursor = end;
        search_from = end;
    }
    out.extend(slice_runs(&runs, cursor, joined.chars().count().max(cursor)));
    (out, fields)
}

/// The inlines covering `[from, to)` in the reassembled string, with text
/// runs cut at the boundaries and their properties kept.
fn slice_runs(runs: &[Inline], from: usize, to: usize) -> Vec<Inline> {
    if to <= from {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut pos = 0usize;
    for inline in runs {
        let len = match inline {
            Inline::Text { text, .. } => text.len(),
            _ => SENTINEL.len_utf8(),
        };
        let start = pos;
        let end = pos + len;
        pos = end;
        if end <= from || start >= to {
            continue;
        }
        match inline {
            Inline::Text { text, style, props } => {
                let a = from.saturating_sub(start).min(text.len());
                let b = (to - start).min(text.len());
                let cut = &text[a..b];
                if !cut.is_empty() {
                    out.push(Inline::Text {
                        text: cut.to_string(),
                        style: style.clone(),
                        props: props.clone(),
                    });
                }
            }
            other => out.push(other.clone()),
        }
    }
    out
}

/// Properties of the text run containing `offset`.
fn props_at(runs: &[Inline], offset: usize) -> Option<RunProps> {
    let mut pos = 0usize;
    for inline in runs {
        let len = match inline {
            Inline::Text { text, .. } => text.len(),
            _ => SENTINEL.len_utf8(),
        };
        if offset < pos + len {
            return match inline {
                Inline::Text { props, .. } => props.clone(),
                _ => None,
            };
        }
        pos += len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn keys(fields: &BTreeMap<String, Field>) -> Vec<String> {
        fields.values().filter_map(|f| f.key.clone()).collect()
    }

    #[test]
    fn a_placeholder_in_one_run_becomes_a_field() {
        let mut n = 0;
        let (out, fields) = extract_placeholders(vec![text("Spett.le {{cliente}},")], &mut n);
        assert_eq!(keys(&fields), ["cliente"]);
        assert_eq!(out.len(), 3, "before, field, after: {out:?}");
        assert!(matches!(&out[1], Inline::Field { .. }));
    }

    #[test]
    fn a_placeholder_split_across_runs_is_still_found() {
        // This is what Word actually writes after any edit near the
        // braces; missing it loses the template's fields.
        let mut n = 0;
        let (out, fields) = extract_placeholders(
            vec![text("Spett.le {{"), text("cliente"), text("}},")],
            &mut n,
        );
        assert_eq!(keys(&fields), ["cliente"]);
        let has_field = out.iter().any(|i| matches!(i, Inline::Field { .. }));
        assert!(has_field, "{out:?}");
        let leftover: String = out
            .iter()
            .filter_map(|i| match i {
                Inline::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(leftover, "Spett.le ,", "the braces must not survive");
    }

    #[test]
    fn two_placeholders_in_one_paragraph() {
        let mut n = 0;
        let (out, fields) =
            extract_placeholders(vec![text("{{citta}}, {{data}}")], &mut n);
        assert_eq!(keys(&fields), ["citta", "data"]);
        assert_eq!(
            out.iter().filter(|i| matches!(i, Inline::Field { .. })).count(),
            2
        );
    }

    #[test]
    fn braces_that_are_not_a_placeholder_are_left_alone() {
        // A document about templating, or about maths, has every right to
        // contain `{{`.
        let mut n = 0;
        let (out, fields) = extract_placeholders(vec![text("l'insieme {{1, 2}} è finito")], &mut n);
        assert!(fields.is_empty(), "{fields:?}");
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn an_unclosed_placeholder_is_not_one() {
        let mut n = 0;
        let (_, fields) = extract_placeholders(vec![text("apertura {{cliente e basta")], &mut n);
        assert!(fields.is_empty());
    }

    #[test]
    fn the_field_inherits_the_formatting_it_replaced() {
        // A bold placeholder must produce bold text once filled.
        let mut n = 0;
        let (_, fields) = extract_placeholders(vec![text("Totale "), bold("{{importo}}")], &mut n);
        assert_eq!(keys(&fields), ["importo"]);
    }

    #[test]
    fn an_image_between_the_braces_means_they_are_not_a_placeholder() {
        let mut n = 0;
        let (_, fields) = extract_placeholders(
            vec![
                text("{{"),
                Inline::Image {
                    src: "i1".into(),
                    width: 100,
                    height: 100,
                    alt: None,
                },
                text("}}"),
            ],
            &mut n,
        );
        assert!(fields.is_empty(), "{fields:?}");
    }

    #[test]
    fn surrounding_text_keeps_its_own_formatting() {
        let mut n = 0;
        let (out, _) = extract_placeholders(
            vec![bold("Spett.le "), text("{{cliente}}"), bold(" S.r.l.")],
            &mut n,
        );
        let first_is_bold = matches!(&out[0], Inline::Text { props: Some(p), .. } if p.bold == Some(true));
        let last_is_bold = matches!(out.last(), Some(Inline::Text { props: Some(p), .. }) if p.bold == Some(true));
        assert!(first_is_bold && last_is_bold, "{out:?}");
    }
}
