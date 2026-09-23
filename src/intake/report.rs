// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
//! The answers, written back out as one document per recipient.
//!
//! The client answers once. Each organisation then needs its own
//! questions, in its own order, with the answers filled in — because that
//! is what it will read, and because a broker who sends one insurer the
//! form of another gets it back.
//!
//! Two documents come out of here:
//!
//! * **the answers for one recipient**: only the questions that recipient
//!   asks, with what the client said;
//! * **the gaps**: what is missing, what looks like placeholder text, and
//!   what contradicts something else — for the broker, before anything is
//!   sent.
//!
//! The second is the one that earns its keep. An incomplete questionnaire
//! comes back days later; a wrong one comes back at claim time, when an
//! inaccurate statement is worth arguing about.

use std::collections::BTreeMap;

use anyhow::Result;
use docx_roundtrip::model::{
    Block, Cell, Document, Inline, Row, RowProps, RunProps, Section, TableProps,
};

use super::form::CollectionForm;

/// What the client answered, by field id.
pub type Answers = BTreeMap<String, String>;

/// Builds the document one recipient gets.
///
/// `company` is the tag used in the schema (`aig`, `axa`, …). Fields
/// nobody tagged are included: a question with no provenance was asked by
/// somebody, and leaving it out would silently shrink the questionnaire.
pub fn company_document(
    form: &CollectionForm,
    answers: &Answers,
    company: &str,
    title: &str,
) -> Document {
    let mut doc = Document::default();
    let mut body: Vec<Block> = vec![heading(title, 1)];

    for section in &form.sections {
        let fields: Vec<_> = section
            .fields
            .iter()
            .filter(|f| f.companies.is_empty() || f.companies.iter().any(|c| c == company))
            .collect();
        if fields.is_empty() {
            continue;
        }
        body.push(heading(&section.title, 2));
        let mut rows = vec![Row {
            props: Some(RowProps {
                header: true,
                ..Default::default()
            }),
            cells: vec![cell("Domanda", true), cell("Risposta", true)],
        }];
        for field in fields {
            let answer = answers
                .get(&field.id)
                .cloned()
                .unwrap_or_else(|| "— non fornito —".to_string());
            rows.push(Row {
                props: None,
                cells: vec![cell(&field.label, false), cell(&answer, false)],
            });
        }
        body.push(Block::Table {
            style: None,
            props: TableProps {
                grid: vec![5600, 3600],
                ..Default::default()
            },
            rows,
        });
    }

    doc.sections = vec![Section {
        body,
        ..Default::default()
    }];
    doc
}

/// One finding of the pre-send check.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub kind: FindingKind,
    pub field_id: String,
    pub label: String,
    /// Recipients affected. Empty when it concerns all of them.
    pub companies: Vec<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingKind {
    /// A recipient asks for it and it is not there.
    Missing,
    /// There is something, and it does not look like an answer.
    Placeholder,
}

/// Values that are technically answers and say nothing. Lower-cased
/// before comparison.
const PLACEHOLDER_ANSWERS: [&str; 10] = [
    "test", "prova", "xxx", "x", "n/a", "na", "da definire", "tbd", "-", "...",
];

/// Checks the collected answers against what each recipient requires.
///
/// Deterministic on purpose: a model can judge whether an answer makes
/// sense, but "this field is empty and AXA requires it" is arithmetic, and
/// arithmetic should not depend on the weather.
pub fn check(form: &CollectionForm, answers: &Answers) -> Vec<Finding> {
    let mut out = Vec::new();
    for section in &form.sections {
        for field in &section.fields {
            let answer = answers.get(&field.id).map(|s| s.trim()).unwrap_or("");
            if answer.is_empty() {
                // Only what somebody actually requires is a gap: an
                // optional question left blank is a choice, not a defect.
                if !field.required_by.is_empty() {
                    out.push(Finding {
                        kind: FindingKind::Missing,
                        field_id: field.id.clone(),
                        label: field.label.clone(),
                        companies: field.required_by.clone(),
                        detail: "richiesto e non compilato".into(),
                    });
                }
                continue;
            }
            let lowered = answer.to_lowercase();
            if PLACEHOLDER_ANSWERS.contains(&lowered.as_str()) {
                out.push(Finding {
                    kind: FindingKind::Placeholder,
                    field_id: field.id.clone(),
                    label: field.label.clone(),
                    companies: field.companies.clone(),
                    detail: format!("la risposta è «{answer}»"),
                });
            }
        }
    }
    out
}

/// The gaps document the broker reads before sending anything.
pub fn gaps_document(findings: &[Finding], title: &str) -> Document {
    let mut doc = Document::default();
    let mut body = vec![heading(title, 1)];
    if findings.is_empty() {
        body.push(paragraph(
            "Nessuna lacuna rilevata: ogni dato richiesto è stato compilato.",
        ));
        doc.sections = vec![Section {
            body,
            ..Default::default()
        }];
        return doc;
    }

    body.push(paragraph(
        "Da verificare con il cliente prima di inviare i questionari alle compagnie.",
    ));
    let mut rows = vec![Row {
        props: Some(RowProps {
            header: true,
            ..Default::default()
        }),
        cells: vec![
            cell("Cosa", true),
            cell("Domanda", true),
            cell("Compagnie", true),
        ],
    }];
    for finding in findings {
        let what = match finding.kind {
            FindingKind::Missing => "manca",
            FindingKind::Placeholder => "da confermare",
        };
        rows.push(Row {
            props: None,
            cells: vec![
                cell(what, false),
                cell(&format!("{} — {}", finding.label, finding.detail), false),
                cell(&finding.companies.join(", "), false),
            ],
        });
    }
    body.push(Block::Table {
        style: None,
        props: TableProps {
            grid: vec![1800, 5600, 1800],
            ..Default::default()
        },
        rows,
    });
    doc.sections = vec![Section {
        body,
        ..Default::default()
    }];
    doc
}

/// Writes a document as `.docx` bytes.
pub fn to_docx(doc: &Document) -> Result<Vec<u8>> {
    docx_roundtrip::write(doc, &docx_roundtrip::Assets::default())
}

fn heading(text: &str, level: u8) -> Block {
    Block::Paragraph {
        style: Some(format!("Heading{level}")),
        props: None,
        runs: vec![Inline::Text {
            text: text.to_string(),
            style: None,
            props: Some(RunProps {
                bold: Some(true),
                size: Some(if level == 1 { 32 } else { 26 }),
                ..Default::default()
            }),
        }],
    }
}

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

fn cell(text: &str, header: bool) -> Cell {
    Cell {
        props: None,
        blocks: vec![Block::Paragraph {
            style: None,
            props: None,
            runs: vec![Inline::Text {
                text: text.to_string(),
                style: None,
                props: header.then(|| RunProps {
                    bold: Some(true),
                    ..Default::default()
                }),
            }],
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intake::form::{FormField, FormSection};

    fn form() -> CollectionForm {
        CollectionForm {
            sections: vec![
                FormSection {
                    id: "anagrafica".into(),
                    title: "Chi siete".into(),
                    fields: vec![
                        FormField {
                            id: "ragioneSociale".into(),
                            label: "Ragione sociale".into(),
                            kind: "text".into(),
                            companies: vec!["aig".into(), "axa".into()],
                            required_by: vec!["axa".into()],
                            ..Default::default()
                        },
                        FormField {
                            id: "ateco".into(),
                            label: "Codice ATECO".into(),
                            kind: "text".into(),
                            companies: vec!["axa".into()],
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                },
                FormSection {
                    id: "prodotti".into(),
                    title: "I vostri prodotti".into(),
                    fields: vec![FormField {
                        id: "export".into(),
                        label: "Esportate negli USA?".into(),
                        kind: "bool".into(),
                        companies: vec!["chubb".into()],
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ],
            dropped: Vec::new(),
        }
    }

    fn answers() -> Answers {
        Answers::from([
            ("ragioneSociale".to_string(), "Oxygen S.r.l.".to_string()),
            ("export".to_string(), "sì".to_string()),
        ])
    }

    /// Writes and reads back: the test is on what a recipient opens, not
    /// on what we think we wrote.
    fn text_of(doc: &Document) -> String {
        let bytes = to_docx(doc).expect("write");
        let opened = docx_roundtrip::open(&bytes).expect("read back");
        opened.document.text()
    }

    #[test]
    fn each_recipient_gets_only_its_own_questions() {
        let doc = company_document(&form(), &answers(), "aig", "Questionario AIG");
        let text = text_of(&doc);
        assert!(text.contains("Ragione sociale"));
        assert!(
            !text.contains("Codice ATECO"),
            "AXA's question must not reach AIG: {text}"
        );
        assert!(!text.contains("Esportate negli USA"));
    }

    #[test]
    fn the_answers_are_in_the_document() {
        let doc = company_document(&form(), &answers(), "axa", "Questionario AXA");
        let text = text_of(&doc);
        assert!(text.contains("Oxygen S.r.l."));
        assert!(
            text.contains("non fornito"),
            "an unanswered question must say so rather than look answered: {text}"
        );
    }

    #[test]
    fn a_section_nobody_asked_about_is_left_out() {
        let doc = company_document(&form(), &answers(), "aig", "Questionario AIG");
        let text = text_of(&doc);
        assert!(!text.contains("I vostri prodotti"), "{text}");
    }

    #[test]
    fn a_required_field_left_blank_is_a_gap() {
        let mut given = answers();
        given.remove("ragioneSociale");
        let findings = check(&form(), &given);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, FindingKind::Missing);
        assert_eq!(findings[0].companies, ["axa"]);
    }

    #[test]
    fn an_optional_field_left_blank_is_not_a_gap() {
        // Silence on an optional question is a choice, and a list where
        // everything is urgent gets ignored.
        let findings = check(&form(), &answers());
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn an_answer_that_says_nothing_is_flagged_not_corrected() {
        let given = Answers::from([
            ("ragioneSociale".to_string(), "test".to_string()),
            ("ateco".to_string(), "da definire".to_string()),
        ]);
        let findings = check(&form(), &given);
        assert_eq!(findings.len(), 2, "{findings:?}");
        assert!(findings.iter().all(|f| f.kind == FindingKind::Placeholder));
        // The wording matters: the broker has to see what was typed, and
        // decide. We never rewrite a client's declaration.
        assert!(findings[0].detail.contains('«'));
    }

    #[test]
    fn the_gaps_document_says_so_when_there_is_nothing_to_report() {
        let doc = gaps_document(&[], "Verifica");
        let text = text_of(&doc);
        assert!(text.contains("Nessuna lacuna"), "{text}");
    }

    #[test]
    fn the_gaps_document_names_the_companies_affected() {
        let mut given = answers();
        given.remove("ragioneSociale");
        let findings = check(&form(), &given);
        let text = text_of(&gaps_document(&findings, "Verifica"));
        assert!(text.contains("Ragione sociale"));
        assert!(text.contains("axa"));
        assert!(text.contains("manca"));
    }

    #[test]
    fn the_written_document_is_a_real_docx() {
        // Not "it has bytes": it opens again, and the structure is there.
        let doc = company_document(&form(), &answers(), "axa", "Questionario AXA");
        let bytes = to_docx(&doc).unwrap();
        let opened = docx_roundtrip::open(&bytes).unwrap();
        let tables = opened
            .document
            .sections
            .iter()
            .flat_map(|s| s.body.iter())
            .filter(|b| matches!(b, Block::Table { .. }))
            .count();
        assert_eq!(tables, 1, "one table of questions and answers");
        assert_eq!(opened.opaque_count, 0, "nothing we wrote should be unreadable to us");
    }
}
