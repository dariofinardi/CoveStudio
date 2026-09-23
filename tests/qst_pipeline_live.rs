//! Live run of the insurance data-collection pipeline against real
//! insurer questionnaires.
//!
//! Ignored by default: it needs an API key, real PDFs, and it costs
//! tokens. What it produces is a **measurement**, not a green tick —
//! how many questions each model finds, how it types them, how much of
//! the known-good form it reproduces. That is the only way to compare
//! Gemini and Mistral on this task without guessing.
//!
//! ```powershell
//! $env:GEMINI_API_KEY = "…"
//! $env:QST_MODEL = "gemini-3.8-flash"          # or mistral-large-latest
//! $env:QST_DIR   = "C:\Progetti\rctop-material" # default
//! cargo test --features pdf,rag,ner-pii --test qst_pipeline_live -- --ignored --nocapture
//! ```
//!
//! The questionnaires stay **outside** the repository: they are third
//! party material and the client form beside them is confidential.
//! Nothing here writes them anywhere.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use cove_studio::llm::structured::{complete_json, StructuredCreds};
use serde_json::Value;

/// Where the questionnaires live. Outside the repo on purpose.
fn fixtures_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("QST_DIR").unwrap_or_else(|_| r"C:\Progetti\rctop-material".to_string()),
    )
}

fn model() -> String {
    std::env::var("QST_MODEL").unwrap_or_else(|_| "gemini-3.8-flash".to_string())
}

fn creds() -> StructuredCreds {
    StructuredCreds {
        gemini_api_key: std::env::var("GEMINI_API_KEY").ok().filter(|k| !k.is_empty()),
        claude_api_key: std::env::var("CLAUDE_API_KEY").ok().filter(|k| !k.is_empty()),
        ..Default::default()
    }
}

fn has_credentials(model: &str) -> bool {
    match cove_studio::llm::provider_for_model(model) {
        cove_studio::llm::Provider::Gemini => std::env::var("GEMINI_API_KEY").is_ok(),
        cove_studio::llm::Provider::Claude => std::env::var("CLAUDE_API_KEY").is_ok(),
        cove_studio::llm::Provider::Mistral => std::env::var("MISTRAL_API_KEY").is_ok(),
        cove_studio::llm::Provider::OpenAI => true,
    }
}

/// Loads a shipped extraction preset: prompt and schema together.
fn preset(id: &str) -> (String, Value) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config/workflow-presets");
    let presets = cove_studio::presets::workflow::load_workflow_presets(&dir).expect("presets");
    let p = presets
        .into_iter()
        .find(|p| p.id == id)
        .unwrap_or_else(|| panic!("preset {id} not found"));
    (
        p.prompt_md.expect("prompt_md"),
        p.response_schema.expect("response_schema"),
    )
}

/// The questionnaires, with the short company tag used as provenance.
fn questionnaires() -> Vec<(&'static str, &'static str)> {
    vec![
        ("aig", "AIG QST_pre_assuntivo_Casuality_SME__1_.pdf"),
        (
            "axa",
            "AXA QST_Unico_RCT_RCO___Malattie_professionali_e_RC_Prodotti___Mod._2040___Ed._12_2019___Commercial_Lines.pdf",
        ),
        ("chubb", "Chubb QST IT_3481L_RC_Prodotti_bilingual_ed.11.2022.pdf"),
    ]
}

const FIELD_TYPES: [&str; 10] = [
    "bool",
    "text",
    "textarea",
    "number",
    "currency",
    "percentage",
    "date",
    "select",
    "checkgroup",
    "table",
];

#[tokio::test]
#[ignore = "needs an API key and the questionnaires; costs tokens"]
async fn extracts_questions_from_each_questionnaire() {
    let model = model();
    if !has_credentials(&model) {
        eprintln!("SKIP: no API key for {model}");
        return;
    }
    let dir = fixtures_dir();
    if !dir.is_dir() {
        eprintln!("SKIP: {} not found (set QST_DIR)", dir.display());
        return;
    }

    let (prompt, schema) = preset("builtin-insurance-qst-estrazione-domande");
    let creds = creds();
    let mut totals: Vec<(String, usize, HashMap<String, usize>)> = Vec::new();

    for (tag, filename) in questionnaires() {
        let path = dir.join(filename);
        if !path.is_file() {
            eprintln!("SKIP {tag}: {} missing", path.display());
            continue;
        }
        let doc = cove_studio::ingest::ingest_path(&path)
            .await
            .unwrap_or_else(|e| panic!("{tag}: onboarding failed: {e:#}"));
        assert!(doc.has_text(), "{tag}: no text to work from");

        let started = std::time::Instant::now();
        // The pipeline's own entry point, not a bare model call: it
        // normalises, and it retries an answer that falls below what
        // the document's own text implies. Runs of the same model on
        // the same file returned 118 questions and then 3, both valid
        // — the floor is what makes that visible instead of plausible.
        let normalised = cove_studio::intake::extract_questions(
            &model,
            &prompt,
            schema.clone(),
            &doc.text,
            &creds,
        )
        .await
        .unwrap_or_else(|e| panic!("{tag}: extraction failed: {e:#}"));
        let elapsed = started.elapsed();
        assert!(!normalised.questions.is_empty(), "{tag}: no questions extracted");
        let english = cove_studio::intake::english_share(&normalised.questions);
        assert!(
            english < 0.15,
            "{tag}: {:.0}% of the labels read as English — the translation side was listed too",
            english * 100.0
        );

        let mut kinds: HashMap<String, usize> = HashMap::new();
        for q in &normalised.questions {
            assert!(
                !q.original_label.trim().is_empty(),
                "{tag}: a question with no label"
            );
            assert!(
                FIELD_TYPES.contains(&q.kind.as_str()),
                "{tag}: unknown field type {:?} for {:?}",
                q.kind,
                q.original_label
            );
            *kinds.entry(q.kind.clone()).or_default() += 1;
            // "If yes, specify" must be one question with a detail, not
            // two — otherwise the form asks for the detail even when the
            // answer was no.
            if q.has_detail {
                assert_eq!(
                    q.kind, "bool",
                    "{tag}: {:?} has a detail but is not a bool",
                    q.original_label
                );
            }
        }

        println!(
            "\n[{tag}] {} domande ({} rimosse dal normalizzatore) in {:.1}s — \
             {} pagine lette su {}, {} caratteri, {:.0}% etichette inglesi",
            normalised.questions.len(),
            normalised.removed.len(),
            elapsed.as_secs_f32(),
            cove_studio::ingest::pages_covered(&doc.sections),
            doc.page_count.unwrap_or(0),
            doc.text.len(),
            english * 100.0,
        );
        for r in normalised.removed.iter().take(5) {
            println!("        rimossa ({}): {}", r.reason, r.original_label);
        }
        let mut sorted: Vec<_> = kinds.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));
        println!(
            "        tipi: {}",
            sorted
                .iter()
                .map(|(k, n)| format!("{k}={n}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        totals.push((tag.to_string(), normalised.questions.len(), kinds));
    }

    assert!(!totals.is_empty(), "no questionnaire was read");
    println!(
        "\n=== {model}: {} domande in totale su {} questionari ===",
        totals.iter().map(|t| t.1).sum::<usize>(),
        totals.len()
    );
}

#[tokio::test]
#[ignore = "needs an API key and the questionnaires; costs tokens"]
async fn the_bilingual_questionnaire_is_not_counted_twice() {
    // Chubb prints every question in Italian and English. Listing both
    // would double the form the client has to fill in — the single most
    // likely way this pipeline produces something worse than the manual
    // work it replaces.
    let model = model();
    if !has_credentials(&model) {
        eprintln!("SKIP: no API key for {model}");
        return;
    }
    let path = fixtures_dir().join("Chubb QST IT_3481L_RC_Prodotti_bilingual_ed.11.2022.pdf");
    if !path.is_file() {
        eprintln!("SKIP: {} missing", path.display());
        return;
    }

    let (prompt, schema) = preset("builtin-insurance-qst-estrazione-domande");
    let doc = cove_studio::ingest::ingest_path(&path).await.expect("onboarding");
    let normalised =
        cove_studio::intake::extract_questions(&model, &prompt, schema, &doc.text, &creds())
            .await
            .expect("extraction");

    // Counting is the wrong test: many questions on this form are
    // fields without a question mark, and two runs of the same model
    // returned 107 and 132. What must hold is the *language* — the
    // Italian side once, the English side not at all — and that the
    // whole document was read.
    let english = cove_studio::intake::english_share(&normalised.questions);
    println!(
        "[chubb] {} domande dopo la normalizzazione ({} duplicate), {:.0}% inglesi",
        normalised.questions.len(),
        normalised.removed.len(),
        english * 100.0
    );
    assert!(
        english < 0.15,
        "the English side was listed too: {:.0}% of labels read as English",
        english * 100.0
    );
    assert!(
        normalised.questions.len() >= 40,
        "only {} questions from a 16-page questionnaire — something was not read",
        normalised.questions.len()
    );
}
