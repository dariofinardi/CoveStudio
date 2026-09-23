// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

//! Fitting attached documents into the model's context window.
//!
//! Attachments are normally sent whole. When their text does not fit the
//! token budget left by the rest of the prompt, the budget is shared out:
//! documents that fit their share stay whole, the others are reduced to
//! the pages (or parts) most relevant to the user's question, ranked with
//! BM25 and kept in document order with their `[Page N]` markers so
//! citations still point at the right page. The model is told the
//! document is excerpted and reads the rest with `read_document` ranges or
//! `find_in_document`.

use crate::document_segments::{rank, render, segment, SegmentUnit};
use crate::llm::summarize::estimate_tokens;

use super::DocPayload;

/// Smallest share given to an excerpted document, so every attachment
/// contributes at least its most relevant page.
const MIN_SHARE_TOKENS: usize = 1_000;

/// Overhead per kept segment (marker line and separators).
const SEGMENT_OVERHEAD_TOKENS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExcerptInfo {
    pub unit: SegmentUnit,
    /// Segments kept in the prompt.
    pub kept: usize,
    /// Segments in the whole document.
    pub total: usize,
}

/// Reduces the attachments that do not fit `budget_tokens`. Returns the
/// indices of the documents that were excerpted.
pub(super) fn fit(docs: &mut [DocPayload], query: &str, budget_tokens: usize) -> Vec<usize> {
    let sizes: Vec<Option<usize>> = docs
        .iter()
        .map(|d| d.text.as_deref().map(estimate_tokens))
        .collect();
    let total: usize = sizes.iter().flatten().sum();
    if total <= budget_tokens {
        return Vec::new();
    }

    // Smallest documents first: each keeps its whole text when it fits an
    // equal share of what is left; the remaining ones split the rest.
    let mut order: Vec<usize> = (0..docs.len()).filter(|&i| sizes[i].is_some()).collect();
    order.sort_by_key(|&i| sizes[i]);
    let mut remaining = budget_tokens;
    let mut shares: Vec<(usize, usize)> = Vec::new();
    for (pos, &i) in order.iter().enumerate() {
        let left = order.len() - pos;
        let share = remaining / left;
        let size = sizes[i].unwrap_or(0);
        if size <= share {
            remaining -= size;
        } else {
            let share = share.max(MIN_SHARE_TOKENS);
            remaining = remaining.saturating_sub(share);
            shares.push((i, share));
        }
    }

    let mut excerpted = Vec::new();
    for (i, share) in shares {
        let Some(text) = docs[i].text.take() else { continue };
        let (reduced, info) = excerpt(&text, query, share);
        tracing::info!(
            "[chat] attachment {} reduced to {} of {} {:?}s (budget {share} tokens, original ≈{} tokens)",
            docs[i].filename,
            info.kept,
            info.total,
            info.unit,
            estimate_tokens(&text)
        );
        docs[i].text = Some(reduced);
        docs[i].excerpt = Some(info);
        excerpted.push(i);
    }
    excerpted.sort_unstable();
    excerpted
}

/// The most relevant segments of `text` for `query` within `budget_tokens`,
/// rendered in document order. Without usable query terms the document is
/// read from its beginning.
fn excerpt(text: &str, query: &str, budget_tokens: usize) -> (String, ExcerptInfo) {
    let (unit, segments) = segment(text);
    let scores = rank(query, &segments);
    let mut order: Vec<usize> = (0..segments.len()).collect();
    if scores.iter().any(|s| *s > 0.0) {
        order.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]).then(a.cmp(&b)));
    }

    let mut chosen: Vec<usize> = Vec::new();
    let mut used = 0usize;
    for i in order {
        let cost = estimate_tokens(&segments[i].text) + SEGMENT_OVERHEAD_TOKENS;
        if used + cost > budget_tokens {
            continue;
        }
        chosen.push(i);
        used += cost;
    }

    let info_total = segments.len();
    if chosen.is_empty() {
        // Even the best segment is larger than the share: keep its start.
        let best = (0..segments.len())
            .max_by(|&a, &b| scores[a].total_cmp(&scores[b]).then(b.cmp(&a)))
            .unwrap_or(0);
        let Some(seg) = segments.get(best) else {
            return (String::new(), ExcerptInfo { unit, kept: 0, total: 0 });
        };
        let max_chars = budget_tokens.saturating_sub(SEGMENT_OVERHEAD_TOKENS) * 4;
        let cut = crate::document_segments::Segment {
            number: seg.number,
            text: seg.text.chars().take(max_chars).collect(),
        };
        return (render(unit, &[&cut]), ExcerptInfo { unit, kept: 1, total: info_total });
    }

    chosen.sort_unstable();
    let kept: Vec<&crate::document_segments::Segment> = chosen.iter().map(|&i| &segments[i]).collect();
    (render(unit, &kept), ExcerptInfo { unit, kept: kept.len(), total: info_total })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(name: &str, text: &str) -> DocPayload {
        DocPayload {
            filename: name.into(),
            text: Some(text.into()),
            images: vec![],
            excerpt: None,
            unreadable: None,
        }
    }

    fn pages(n: usize, filler: &str) -> String {
        (1..=n).map(|i| format!("[Page {i}]\n{filler} pagina {i}\n")).collect()
    }

    #[test]
    fn documents_that_fit_are_untouched() {
        let mut docs = vec![doc("a.pdf", "[Page 1]\nbreve\n")];
        assert!(fit(&mut docs, "breve", 10_000).is_empty());
        assert!(docs[0].excerpt.is_none());
    }

    #[test]
    fn large_document_keeps_relevant_pages_in_order() {
        let filler = "testo generico ".repeat(200); // ≈ 750 tokens per page
        let mut text = pages(20, &filler);
        text = text.replace("pagina 7\n", "pagina 7\nclausola di recesso con preavviso\n");
        text = text.replace("pagina 15\n", "pagina 15\nrecesso anticipato e penale\n");
        let mut docs = vec![doc("contratto.pdf", &text)];
        let excerpted = fit(&mut docs, "come funziona il recesso?", 2_000);
        assert_eq!(excerpted, vec![0]);
        let info = docs[0].excerpt.unwrap();
        assert_eq!(info.unit, SegmentUnit::Page);
        assert_eq!(info.total, 20);
        assert!(info.kept >= 2 && info.kept < 20);
        let out = docs[0].text.as_deref().unwrap();
        let p7 = out.find("[Page 7]").expect("page 7 kept");
        let p15 = out.find("[Page 15]").expect("page 15 kept");
        assert!(p7 < p15, "pages stay in document order");
        assert!(estimate_tokens(out) <= 2_000 + 100);
    }

    #[test]
    fn small_documents_stay_whole_when_a_large_one_is_reduced() {
        let small = "[Page 1]\npiccolo documento\n";
        let big = pages(40, &"parole ".repeat(400));
        let mut docs = vec![doc("big.pdf", &big), doc("small.pdf", small)];
        let excerpted = fit(&mut docs, "documento", 3_000);
        assert_eq!(excerpted, vec![0]);
        assert_eq!(docs[1].text.as_deref(), Some(small));
    }

    #[test]
    fn query_without_terms_reads_from_the_start() {
        let big = pages(30, &"contenuto ".repeat(300));
        let mut docs = vec![doc("big.pdf", &big)];
        fit(&mut docs, "?", 1_500);
        assert!(docs[0].text.as_deref().unwrap().starts_with("[Page 1]"));
    }

    #[test]
    fn oversized_single_segment_is_cut_to_the_share() {
        let text = "x".repeat(200_000); // one part larger than the share
        let mut docs = vec![doc("blob.txt", &text)];
        fit(&mut docs, "x", 1_000);
        let out = docs[0].text.as_deref().unwrap();
        assert!(out.starts_with("[Part 1]"));
        assert!(estimate_tokens(out) <= 1_000);
    }
}
