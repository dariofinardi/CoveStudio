// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

//! Addressable segments of extracted document text.
//!
//! Text extracted from PDFs carries `[Page N]` markers, so it is split by
//! page; any other text is split into parts of about [`PART_CHARS`]
//! characters, cut at a line break when one is close. The same segments
//! are used by `read_document` (reading a long document a range at a
//! time) and by the chat when a large attachment has to be reduced to
//! its most relevant excerpts, so page and part numbers mean the same
//! thing everywhere.

use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// Target size of a part for text without page markers.
pub const PART_CHARS: usize = 8_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentUnit {
    Page,
    Part,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// Page number from the marker, or 1-based part number.
    pub number: usize,
    /// Segment text without its marker.
    pub text: String,
}

impl Segment {
    pub fn chars(&self) -> usize {
        self.text.chars().count()
    }
}

/// Splits `text` into pages when it has `[Page N]` markers, into parts
/// otherwise. Text before the first page marker joins the first page.
pub fn segment(text: &str) -> (SegmentUnit, Vec<Segment>) {
    let pages = split_pages(text);
    if !pages.is_empty() {
        return (SegmentUnit::Page, pages);
    }
    (SegmentUnit::Part, split_parts(text, PART_CHARS))
}

fn page_marker(line: &str) -> Option<usize> {
    line.trim()
        .strip_prefix("[Page ")?
        .strip_suffix(']')?
        .parse()
        .ok()
}

fn split_pages(text: &str) -> Vec<Segment> {
    let mut pages: Vec<Segment> = Vec::new();
    let mut preamble = String::new();
    for line in text.lines() {
        if let Some(number) = page_marker(line) {
            let mut initial = String::new();
            if pages.is_empty() && !preamble.trim().is_empty() {
                initial = std::mem::take(&mut preamble);
            }
            pages.push(Segment { number, text: initial });
            continue;
        }
        let target = match pages.last_mut() {
            Some(page) => &mut page.text,
            None => &mut preamble,
        };
        target.push_str(line);
        target.push('\n');
    }
    for page in &mut pages {
        page.text = page.text.trim_end().to_string();
    }
    pages
}

fn split_parts(text: &str, target: usize) -> Vec<Segment> {
    let chars: Vec<char> = text.chars().collect();
    let mut parts = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let mut end = (start + target).min(chars.len());
        if end < chars.len() {
            // Prefer a line break in the last fifth of the part.
            let floor = start + target * 4 / 5;
            if let Some(nl) = (floor..end).rev().find(|&i| chars[i] == '\n') {
                end = nl + 1;
            }
        }
        let piece: String = chars[start..end].iter().collect();
        if !piece.trim().is_empty() {
            parts.push(Segment { number: parts.len() + 1, text: piece.trim_end().to_string() });
        }
        start = end;
    }
    parts
}

/// Text of segments with their markers, in the given order, so citations
/// can still refer to page numbers.
pub fn render(unit: SegmentUnit, segments: &[&Segment]) -> String {
    let label = match unit {
        SegmentUnit::Page => "Page",
        SegmentUnit::Part => "Part",
    };
    let mut out = String::new();
    for s in segments {
        out.push_str(&format!("[{label} {}]\n{}\n", s.number, s.text));
    }
    out
}

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "from", "are", "was", "what", "which", "how",
    "che", "per", "con", "del", "della", "delle", "dei", "degli", "nel", "nella", "sono", "una",
    "uno", "gli", "le", "la", "il", "di", "da", "in", "su", "come", "cosa", "quale", "quali",
    "des", "les", "une", "pour", "dans", "der", "die", "das", "und", "mit", "los", "las", "para",
];

fn terms(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.chars().count() >= 3)
        .map(str::to_lowercase)
        .filter(|t| !STOPWORDS.contains(&t.as_str()))
        .collect()
}

/// BM25 relevance of each segment for `query` (same order as `segments`).
/// All scores are zero when the query has no usable terms.
pub fn rank(query: &str, segments: &[Segment]) -> Vec<f32> {
    const K1: f32 = 1.2;
    const B: f32 = 0.75;
    let query_terms: HashSet<String> = terms(query).into_iter().collect();
    if query_terms.is_empty() || segments.is_empty() {
        return vec![0.0; segments.len()];
    }
    let docs: Vec<Vec<String>> = segments.iter().map(|s| terms(&s.text)).collect();
    let n = docs.len() as f32;
    let avg_len = docs.iter().map(Vec::len).sum::<usize>() as f32 / n.max(1.0);
    let df: HashMap<&str, usize> = query_terms
        .iter()
        .map(|t| (t.as_str(), docs.iter().filter(|d| d.iter().any(|w| w == t)).count()))
        .collect();

    docs.iter()
        .map(|doc| {
            let len = doc.len() as f32;
            query_terms
                .iter()
                .map(|t| {
                    let tf = doc.iter().filter(|w| *w == t).count() as f32;
                    if tf == 0.0 {
                        return 0.0;
                    }
                    let df = df[t.as_str()] as f32;
                    let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
                    idf * tf * (K1 + 1.0) / (tf + K1 * (1.0 - B + B * len / avg_len.max(1.0)))
                })
                .sum()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_text_is_split_by_page_markers() {
        let text = "intro\n[Page 1]\nprima pagina\n[Page 2]\nseconda\npagina\n";
        let (unit, pages) = segment(text);
        assert_eq!(unit, SegmentUnit::Page);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].number, 1);
        assert!(pages[0].text.starts_with("intro"));
        assert_eq!(pages[1].text, "seconda\npagina");
    }

    #[test]
    fn plain_text_is_split_into_parts_at_line_breaks() {
        let line = "a".repeat(99) + "\n";
        let text = line.repeat(200); // 20 000 chars
        let (unit, parts) = segment(&text);
        assert_eq!(unit, SegmentUnit::Part);
        assert!(parts.len() >= 3);
        assert!(parts.iter().all(|p| p.chars() <= PART_CHARS));
        assert!(parts.iter().enumerate().all(|(i, p)| p.number == i + 1));
        let rejoined: usize = parts.iter().map(|p| p.chars() + 1).sum();
        assert_eq!(rejoined, text.chars().count());
    }

    #[test]
    fn multibyte_text_does_not_panic() {
        let text = "è".repeat(PART_CHARS * 2 + 7);
        let (_, parts) = segment(&text);
        assert_eq!(parts.iter().map(Segment::chars).sum::<usize>(), text.chars().count());
    }

    #[test]
    fn render_keeps_markers() {
        let (unit, pages) = segment("[Page 3]\nuno\n[Page 7]\ndue\n");
        let out = render(unit, &[&pages[1]]);
        assert_eq!(out, "[Page 7]\ndue\n");
    }

    #[test]
    fn rank_prefers_segments_with_the_query_terms() {
        let (_, pages) = segment(
            "[Page 1]\nIntroduzione generale al contratto.\n\
             [Page 2]\nLa clausola di recesso consente il recesso con preavviso di trenta giorni.\n\
             [Page 3]\nFirme delle parti.\n",
        );
        let scores = rank("quando è possibile il recesso?", &pages);
        let best = scores
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;
        assert_eq!(best, 1);
        assert!(rank("??", &pages).iter().all(|s| *s == 0.0));
    }
}
