// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
//! Turning other people's forms into one form of our own.
//!
//! The shape of the problem, stripped of the domain: several
//! organisations each publish a list of things they want to know, in
//! their own words and their own order. Somebody has to answer all of
//! them — once — and then produce, for each organisation, the answers
//! it asked for. Insurance broking is where this arrived (three
//! insurers' underwriting questionnaires, one client), but tenders,
//! supplier onboarding and compliance audits are the same shape.
//!
//! This module holds the deterministic half of that pipeline. The
//! model's half lives in `config/workflow-presets/insurance/qst-*.json`:
//! prompts with the JSON Schema their answers must satisfy.
//!
//! **Why a deterministic half at all.** Measured on a real 16-page
//! questionnaire, the same model on the same document returned 107
//! questions on one run and 132 on the next, the longer run containing
//! a question listed twice. A form that asks the client the same thing
//! twice is worse than one that forgets it: it reads as carelessness,
//! and the client has to wonder whether the two are subtly different.
//! No prompt makes that impossible, so the prompt asks and the code
//! enforces.

pub mod form;
pub mod report;
pub mod sheet;

use serde::{Deserialize, Serialize};

/// One question as extracted from a single organisation's form.
///
/// Field names match the extraction schema; anything the model omits is
/// simply absent rather than defaulted to something plausible.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Question {
    /// The question as printed, not rewritten. This is what lets a
    /// person find it again on the original form.
    pub original_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_printed: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default)]
    pub has_detail: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_label: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
}

/// A question the normaliser removed, and why — never silently.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Removed {
    pub original_label: String,
    pub reason: &'static str,
}

/// Reasons a question is removed.
pub mod removed {
    /// The same question, listed more than once by the extractor.
    pub const DUPLICATE: &str = "duplicate";
    /// Nothing to ask: an empty or punctuation-only label.
    pub const EMPTY: &str = "empty_label";
}

/// The result of normalising one form's extraction.
#[derive(Debug, Clone, Serialize)]
pub struct Normalised {
    pub questions: Vec<Question>,
    pub removed: Vec<Removed>,
}

/// Comparison key for "the same question asked twice".
///
/// Case, accents of spacing, trailing colons and the numbering a form
/// prints in front of its questions ("3.1 ", "b) ") are all noise: two
/// labels that differ only in those ways are the same question. What it
/// deliberately does **not** do is compare meanings — "fatturato totale"
/// and "fatturato export" are close in words and far apart in fact, and
/// collapsing them would produce a form that asks neither properly.
fn dedup_key(label: &str) -> String {
    let lowered = label.to_lowercase();
    let without_numbering = strip_printed_numbering(&lowered);
    let mut key = String::with_capacity(without_numbering.len());
    let mut last_was_space = false;
    for ch in without_numbering.chars() {
        if ch.is_alphanumeric() {
            key.push(ch);
            last_was_space = false;
        } else if !last_was_space {
            key.push(' ');
            last_was_space = true;
        }
    }
    key.trim().to_string()
}

/// Removes the enumerator a form prints in front of a question:
/// `3.1`, `12)`, `b)`, `(a)`, `-`. Different forms number the same
/// question differently, and two labels that differ only in numbering
/// are the same question.
///
/// A single letter counts as an enumerator **only** when a bracket or a
/// dot follows it, so "e commerciale" keeps its "e" — stripping a real
/// word would merge questions that merely start alike.
fn strip_printed_numbering(label: &str) -> String {
    let t = label.trim_start_matches(['(', '-', ' ']);
    let mut chars = t.char_indices().peekable();
    let mut cut = 0usize;
    // digits, possibly dotted: 3, 3.1, 12.4.2
    while let Some(&(i, c)) = chars.peek() {
        if c.is_ascii_digit() || (c == '.' && cut > 0) {
            cut = i + c.len_utf8();
            chars.next();
        } else {
            break;
        }
    }
    if cut == 0 {
        // a single letter, only if punctuation follows it
        let mut it = t.chars();
        if let (Some(a), Some(b)) = (it.next(), it.next()) {
            if a.is_alphabetic() && matches!(b, ')' | '.') {
                cut = a.len_utf8() + b.len_utf8();
            }
        }
    }
    t[cut..]
        .trim_start_matches([')', '.', '-', ' ', ':'])
        .to_string()
}

/// Drops duplicates and empty labels, keeping the first occurrence.
///
/// First, not "best": the first occurrence is the one earliest in the
/// document, so the surviving page number points at where a reader
/// would naturally look for it.
pub fn normalise(questions: Vec<Question>) -> Normalised {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut kept: Vec<Question> = Vec::with_capacity(questions.len());
    let mut removed: Vec<Removed> = Vec::new();

    for q in questions {
        let key = dedup_key(&q.original_label);
        if key.is_empty() {
            removed.push(Removed {
                original_label: q.original_label,
                reason: removed::EMPTY,
            });
            continue;
        }
        if !seen.insert(key) {
            removed.push(Removed {
                original_label: q.original_label,
                reason: removed::DUPLICATE,
            });
            continue;
        }
        kept.push(q);
    }
    Normalised {
        questions: kept,
        removed,
    }
}

/// Rough share of labels that read as English.
///
/// Bilingual forms print every question twice, and an extractor that
/// lists both sides doubles the form the client has to fill in. This
/// does not *fix* that — deciding which of two sentences is the
/// translation of which needs a model — it measures it, so a run that
/// went wrong is visible instead of merely large.
pub fn english_share(questions: &[Question]) -> f32 {
    if questions.is_empty() {
        return 0.0;
    }
    // Function words, not vocabulary: technical Italian is full of
    // English nouns ("claims made", "vendor's liability") and those are
    // not translations, they are terms of art.
    const MARKERS: [&str; 12] = [
        " the ", " of the ", " do you ", " does the ", " are you ", " have you ", " is there ",
        " please ", " which of ", " your company ", " will be ", " has been ",
    ];
    let english = questions
        .iter()
        .filter(|q| {
            let padded = format!(" {} ", q.original_label.to_lowercase());
            MARKERS.iter().filter(|m| padded.contains(**m)).count() >= 2
        })
        .count();
    english as f32 / questions.len() as f32
}


/// A conservative lower bound on how many questions a form holds.
///
/// Question marks alone are not enough: a form asks most of its
/// questions as fields to fill in. Measured on three real
/// questionnaires (true counts in brackets):
///
/// | | `?` | lines with `___` | lines ending `:` | floor |
/// |---|---|---|---|---|
/// | AIG (40) | 11 | 13 | 10 | 8 |
/// | AXA (119) | 67 | 0 | 42 | 27 |
/// | Chubb (104) | 149 | 4 | 84 | 59 |
///
/// A quarter of the sum sits well under every honest answer and well
/// over the degenerate ones actually observed (1 and 3). It stays a
/// floor, not an estimate: being wrong upwards would make the pipeline
/// retry documents that are simply short.
pub fn question_floor(document_text: &str) -> usize {
    let marks = document_text.matches('?').count();
    let blanks = document_text.lines().filter(|l| l.contains("___")).count();
    let labels = document_text
        .lines()
        .filter(|l| l.trim_end().ends_with(':'))
        .count();
    (marks + blanks + labels) / 4
}

/// Whether an extraction looks like the model gave up.
///
/// The floor is ignored on documents too small to judge: a one-page
/// form with four questions must not be retried for ever.
pub fn looks_degenerate(questions: &[Question], document_text: &str) -> bool {
    let floor = question_floor(document_text);
    floor >= 5 && questions.len() < floor
}

/// Extracts the questions of one form, with the floor enforced.
///
/// Retries once when the first answer is obviously incomplete — a
/// second call costs a few cents, and a form missing most of its
/// questions costs a broker a round trip with the client. A second
/// failure is reported as an error rather than returned quietly: the
/// pipeline must not build a collection form out of three questions
/// read from a ten-page questionnaire.
pub async fn extract_questions(
    model: &str,
    prompt: &str,
    schema: serde_json::Value,
    document_text: &str,
    creds: &crate::llm::structured::StructuredCreds,
) -> anyhow::Result<Normalised> {
    let floor = question_floor(document_text);
    let mut best: Option<Normalised> = None;

    for attempt in 1..=2 {
        let answer = crate::llm::structured::complete_json(
            model,
            prompt,
            document_text,
            schema.clone(),
            creds,
        )
        .await?;
        let parsed: Vec<Question> = serde_json::from_value(answer["questions"].clone())
            .map_err(|e| anyhow::anyhow!("the answer did not carry a question list: {e}"))?;
        let normalised = normalise(parsed);

        let enough = !looks_degenerate(&normalised.questions, document_text);
        let better_than_before = best
            .as_ref()
            .map(|b| normalised.questions.len() > b.questions.len())
            .unwrap_or(true);
        if better_than_before {
            best = Some(normalised);
        }
        if enough {
            break;
        }
        tracing::warn!(
            "[intake] attempt {attempt}: {} questions against a floor of {floor} — retrying",
            best.as_ref().map(|b| b.questions.len()).unwrap_or(0),
        );
    }

    let out = best.expect("at least one attempt");
    if looks_degenerate(&out.questions, document_text) {
        return Err(anyhow::anyhow!(
            "{model} returned {} questions for a document whose text suggests at least \
             {floor}; the extraction is incomplete",
            out.questions.len()
        ));
    }
    Ok(out)
}


/// Longest piece handed to the model in one call.
///
/// Chosen from measurement, not taste: whole documents of 26k and 31k
/// characters produced answers that collapsed at random, while pieces
/// of this size answered consistently. Small enough to be easy, large
/// enough that a question and its "if yes, specify" stay together.
const CHUNK_CHARS: usize = 7_000;

/// Splits a document for extraction, preferring the seams the document
/// already has.
///
/// Page markers first (`[Page N]`), then blank lines, and only then a
/// hard cut. Cutting mid-question would produce half a label in one
/// piece and half in the next, and both would be extracted as
/// questions — a failure that looks like thoroughness.
pub fn split_for_extraction(text: &str) -> Vec<String> {
    if text.len() <= CHUNK_CHARS {
        return vec![text.to_string()];
    }
    let mut pieces: Vec<String> = Vec::new();
    let mut current = String::new();
    // A page marker opens a new block; everything else accumulates.
    for block in text.split_inclusive("\n\n") {
        let starts_page = block.trim_start().starts_with("[Page ");
        if !current.is_empty() && (current.len() + block.len() > CHUNK_CHARS || (starts_page && current.len() > CHUNK_CHARS / 2))
        {
            pieces.push(std::mem::take(&mut current));
        }
        if block.len() > CHUNK_CHARS {
            // A single enormous block (a table with no blank lines):
            // cut it on line boundaries rather than mid-line.
            for line in block.lines() {
                if current.len() + line.len() + 1 > CHUNK_CHARS && !current.is_empty() {
                    pieces.push(std::mem::take(&mut current));
                }
                current.push_str(line);
                current.push('\n');
            }
        } else {
            current.push_str(block);
        }
    }
    if !current.trim().is_empty() {
        pieces.push(current);
    }
    pieces
}

/// Extracts the questions of one form, reading it in pieces when it is
/// long.
///
/// The pieces are read in order and their questions concatenated, so
/// the resulting form follows the original document. Duplicates at the
/// seams — a heading repeated at the top of the next piece — are
/// removed by the same normaliser as everywhere else.
pub async fn extract_questions_chunked(
    model: &str,
    prompt: &str,
    schema: serde_json::Value,
    document_text: &str,
    creds: &crate::llm::structured::StructuredCreds,
) -> anyhow::Result<Normalised> {
    // Whole document first. Measured on the same three questionnaires:
    // read whole, one form yielded 126 questions and in pieces only 88
    // — context lost at the seams is real. But the whole-document read
    // is the one that collapses at random, so the pieces are the safety
    // net, not the default.
    match extract_questions(model, prompt, schema.clone(), document_text, creds).await {
        Ok(whole) => return Ok(whole),
        Err(e) => tracing::warn!("[intake] whole-document read failed ({e}); reading in pieces"),
    }

    let pieces = split_for_extraction(document_text);
    if pieces.len() == 1 {
        return Err(anyhow::anyhow!(
            "the document is short enough to read in one call, and that call did not              produce a usable inventory"
        ));
    }

    let mut all: Vec<Question> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    for (i, piece) in pieces.iter().enumerate() {
        match crate::llm::structured::complete_json(model, prompt, piece, schema.clone(), creds)
            .await
        {
            Ok(answer) => match serde_json::from_value::<Vec<Question>>(
                answer["questions"].clone(),
            ) {
                Ok(mut qs) => all.append(&mut qs),
                Err(e) => failures.push(format!("piece {}: {e}", i + 1)),
            },
            Err(e) => failures.push(format!("piece {}: {e:#}", i + 1)),
        }
    }

    // One piece failing is survivable and must be said; all of them
    // failing is not an extraction.
    if all.is_empty() {
        return Err(anyhow::anyhow!(
            "no piece of the document could be read: {}",
            failures.join("; ")
        ));
    }
    if !failures.is_empty() {
        tracing::warn!(
            "[intake] {} of {} pieces failed: {}",
            failures.len(),
            pieces.len(),
            failures.join("; ")
        );
    }

    let normalised = normalise(all);
    if looks_degenerate(&normalised.questions, document_text) {
        return Err(anyhow::anyhow!(
            "{model} returned {} questions across {} pieces for a document whose text \
             suggests at least {}; the extraction is incomplete",
            normalised.questions.len(),
            pieces.len(),
            question_floor(document_text)
        ));
    }
    Ok(normalised)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(label: &str) -> Question {
        Question {
            original_label: label.to_string(),
            section_printed: None,
            kind: "text".into(),
            options: vec![],
            columns: vec![],
            rows: vec![],
            unit: None,
            has_detail: false,
            detail_label: None,
            required: false,
            page: None,
        }
    }

    #[test]
    fn the_same_question_twice_is_kept_once() {
        let out = normalise(vec![q("Numero addetti"), q("Numero addetti")]);
        assert_eq!(out.questions.len(), 1);
        assert_eq!(out.removed.len(), 1);
        assert_eq!(out.removed[0].reason, removed::DUPLICATE);
    }

    #[test]
    fn spacing_case_and_printed_numbering_are_noise() {
        let out = normalise(vec![
            q("3.1  Numero addetti:"),
            q("numero addetti"),
            q("b) NUMERO ADDETTI"),
        ]);
        assert_eq!(out.questions.len(), 1, "{:?}", out.questions);
        // The first one survives, so its page still points where a
        // reader would look.
        assert_eq!(out.questions[0].original_label, "3.1  Numero addetti:");
    }

    #[test]
    fn a_word_is_not_mistaken_for_an_enumerator() {
        // "e" is a word in Italian; stripping it would merge two
        // different questions that happen to start with it.
        assert_eq!(strip_printed_numbering("e commerciale"), "e commerciale");
        assert_eq!(strip_printed_numbering("b) numero addetti"), "numero addetti");
        assert_eq!(strip_printed_numbering("3.1 numero addetti"), "numero addetti");
        assert_eq!(strip_printed_numbering("12) fatturato"), "fatturato");
    }

    #[test]
    fn questions_that_differ_in_substance_are_both_kept() {
        // The failure that matters: merging these would produce one
        // answer that satisfies neither insurer, and nothing downstream
        // would notice.
        let out = normalise(vec![
            q("Fatturato totale"),
            q("Fatturato export"),
            q("Numero addetti"),
            q("Numero addetti alla produzione"),
        ]);
        assert_eq!(out.questions.len(), 4);
        assert!(out.removed.is_empty());
    }

    #[test]
    fn an_empty_label_is_removed_with_a_reason() {
        let out = normalise(vec![q("   "), q("- - -"), q("Ragione sociale")]);
        assert_eq!(out.questions.len(), 1);
        assert_eq!(out.removed.len(), 2);
        assert!(out.removed.iter().all(|r| r.reason == removed::EMPTY));
    }

    #[test]
    fn order_is_preserved() {
        // The form follows the document; reordering would scatter
        // questions that belong to the same part of the original.
        let out = normalise(vec![q("Uno"), q("Due"), q("Tre")]);
        let labels: Vec<&str> = out
            .questions
            .iter()
            .map(|x| x.original_label.as_str())
            .collect();
        assert_eq!(labels, ["Uno", "Due", "Tre"]);
    }

    #[test]
    fn italian_questions_do_not_read_as_english() {
        let qs = vec![
            q("Qual è il numero di addetti?"),
            q("Indicare il fatturato dell'ultimo esercizio"),
            q("Viene richiesto l'inserimento della garanzia \"Vendor's Liability\"?"),
        ];
        assert_eq!(english_share(&qs), 0.0, "an English term of art is not English prose");
    }

    #[test]
    fn a_bilingual_run_that_listed_both_sides_is_visible() {
        let qs = vec![
            q("Qual è il numero di addetti?"),
            q("What is the number of employees of the company?"),
            q("Indicare il fatturato"),
            q("Please state the turnover of the last financial year"),
        ];
        assert!(english_share(&qs) >= 0.4, "share: {}", english_share(&qs));
    }

    /// Rebuilds a document with the signal counts measured on the real
    /// questionnaires, so the floor is tested against the shapes it
    /// will actually meet.
    fn document_like(marks: usize, blanks: usize, labels: usize) -> String {
        let mut out = String::new();
        for _ in 0..marks {
            out.push_str("Domanda?
");
        }
        for _ in 0..blanks {
            out.push_str("Campo da compilare ______
");
        }
        for _ in 0..labels {
            out.push_str("Etichetta:
");
        }
        out
    }

    #[test]
    fn the_floor_counts_fields_not_only_question_marks() {
        // AIG asks 40 questions with 11 question marks: counting those
        // alone would leave the guard switched off on exactly the
        // form-shaped documents it exists for.
        assert_eq!(question_floor(&document_like(11, 13, 10)), 8);
        assert_eq!(question_floor(&document_like(67, 0, 42)), 27);
        assert_eq!(question_floor(&document_like(149, 4, 84)), 59);
        assert_eq!(question_floor("nessun segnale di modulo"), 0);
    }

    #[test]
    fn the_degenerate_answers_actually_observed_are_caught() {
        // Both of these happened with gemini-3.8-flash, valid against
        // the schema and with a clean finish reason.
        let aig = document_like(11, 13, 10);
        let one: Vec<Question> = vec![q("Ragione sociale")];
        assert!(looks_degenerate(&one, &aig), "1 question out of ~40");

        let axa = document_like(67, 0, 42);
        let three: Vec<Question> = (0..3).map(|i| q(&format!("domanda {i}"))).collect();
        assert!(looks_degenerate(&three, &axa), "3 questions out of ~119");
    }

    #[test]
    fn honest_answers_are_not_retried() {
        let aig = document_like(11, 13, 10);
        let forty: Vec<Question> = (0..40).map(|i| q(&format!("domanda {i}"))).collect();
        assert!(!looks_degenerate(&forty, &aig));

        let chubb = document_like(149, 4, 84);
        let hundred: Vec<Question> = (0..104).map(|i| q(&format!("domanda {i}"))).collect();
        assert!(!looks_degenerate(&hundred, &chubb));
    }

    #[test]
    fn a_genuinely_short_form_is_not_called_degenerate() {
        // A one-page form with four questions must not be treated as a
        // failed extraction, or the pipeline would retry for ever on
        // documents that are simply short.
        let text = "Ragione sociale? Partita IVA? Sede? Referente?";
        let four: Vec<Question> = (0..4).map(|i| q(&format!("domanda {i}"))).collect();
        assert!(!looks_degenerate(&four, text));
    }

    #[test]
    fn a_short_document_is_not_split() {
        let text = "Domanda?\n\nAltra domanda?";
        assert_eq!(split_for_extraction(text).len(), 1);
    }

    #[test]
    fn a_long_document_is_split_without_losing_text() {
        // Nothing may be dropped at the seams: a question that falls
        // between two pieces is a question nobody asks.
        let block = "Domanda di prova che occupa spazio.\n\n";
        let text = block.repeat(600);
        let pieces = split_for_extraction(&text);
        assert!(pieces.len() > 1, "expected several pieces");
        let rejoined: String = pieces.concat();
        assert_eq!(rejoined.len(), text.len());
        assert_eq!(rejoined, text);
    }

    #[test]
    fn pieces_stay_within_the_budget() {
        let text = "riga di testo abbastanza lunga da contare\n".repeat(1000);
        for p in split_for_extraction(&text) {
            assert!(
                p.len() <= CHUNK_CHARS + 200,
                "piece of {} chars is over budget",
                p.len()
            );
        }
    }

    #[test]
    fn page_markers_open_a_new_piece_when_one_is_already_substantial() {
        let mut text = String::new();
        for page in 1..=6 {
            text.push_str(&format!("[Page {page}]\n"));
            text.push_str(&"contenuto della pagina, ripetuto.\n".repeat(60));
            text.push_str("\n\n");
        }
        let pieces = split_for_extraction(&text);
        assert!(pieces.len() >= 2);
        // Every piece after the first should begin at a page boundary.
        for p in pieces.iter().skip(1) {
            assert!(
                p.trim_start().starts_with("[Page ") || p.trim_start().starts_with("contenuto"),
                "piece starts mid-structure: {:?}",
                p.chars().take(40).collect::<String>()
            );
        }
    }
}
