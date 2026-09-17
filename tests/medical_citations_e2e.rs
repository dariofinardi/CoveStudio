//! End-to-end reproduction of the 10-PDF medical-legal chat that
//! exposed the v0.5.x citation issues, OUTSIDE the UI. Hits the real
//! `POST /chat` handler with `tower::ServiceExt::oneshot`, parses the
//! SSE stream, and prints a structured JSON report of citation
//! quality so we can A/B different settings (HyDE on/off, system
//! prompt tweaks, model swaps) without manually clicking through the
//! Tauri window.
//!
//! Skipped by default — requires:
//!   - `GEMINI_API_KEY` env (or other provider key — see code)
//!   - `pdfium-render` native library on the standard repo path
//!
//! Run:
//!
//! ```pwsh
//! $env:GEMINI_API_KEY = "..."
//! cargo test --test medical_citations_e2e --features rag,pdf `
//!     -- --ignored --nocapture
//! ```
//!
//! Output: a final `===== CITATION QUALITY REPORT =====` block with a
//! JSON object summarising the run. Pipe it to a file via
//! `scripts/test-medical-citations.ps1` to accumulate a results.jsonl
//! and compare runs over time.
//!
//! Two scenarios run per invocation, mirroring the failing chat the
//! user reported:
//!   1. `analyze` — POST a fresh chat with all 10 PDFs attached + the
//!      prompt "analizza la cartella clinica".
//!   2. `template` — follow-up turn that asks for the medical-legal
//!      template docx ("prepara la relazione per l'assicurazione").
//!      The `template` chip is NOT applied here (the test stays at
//!      the chat layer); the docx tool isn't exercised, but the
//!      citation behaviour on a multi-turn chat is.

#![allow(clippy::collapsible_else_if)]

use axum::{body::Body, http::Request, http::StatusCode};
use cove_studio::AppState;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::sqlite::SqlitePoolOptions;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tower::ServiceExt;

// ─── per-citation + per-run metrics ────────────────────────────────

#[derive(Debug, Default, Serialize)]
struct PerCitation {
    r#ref: String,
    doc_id: String,
    source: String,
    /// `"number"` for a precise page like `4`, `"range"` for
    /// `"1-3"`, `"absent"` when the model didn't emit one.
    page_kind: String,
    /// First page number we can extract from the page field
    /// (the digit before any `-`). `None` when absent.
    page_first: Option<i64>,
    quote_len: usize,
}

#[derive(Debug, Default, Serialize)]
struct ScenarioReport {
    scenario: String,
    response_chars: usize,
    bracket_count: usize,
    has_citations_block: bool,
    citations_count: usize,
    /// Fraction (0..1) of citations with a precise integer page —
    /// these are the ones the viewer can scroll to.
    pct_precise_page: f64,
    /// Fraction with a page-range string like `"1-3"` — the viewer
    /// falls back to page 1, but loses sub-page positioning.
    pct_range_page: f64,
    /// Fraction with no page at all — viewer text-searches the quote
    /// instead, useful only when `quote` is precise.
    pct_no_page: f64,
    /// Fraction with quotes shorter than 10 chars (empty / generic).
    /// These citations open the doc but cannot highlight a passage.
    pct_short_quote: f64,
    /// Median quote length (chars). Big values mean precise citations.
    median_quote_len: usize,
    /// Distribution of `source` values across citations.
    source_distribution: serde_json::Map<String, Value>,
    /// Distribution of `doc_id` values — useful to spot KB pollution
    /// (when a chat with 10 attached docs cites a `gN` instead).
    doc_id_distribution: serde_json::Map<String, Value>,
    /// First 5 citation entries verbatim, for spot-checking.
    sample_citations: Vec<PerCitation>,
}

#[derive(Debug, Default, Serialize)]
struct RunReport {
    timestamp_utc: String,
    git_commit: String,
    model: String,
    hyde_enabled: bool,
    scenarios: Vec<ScenarioReport>,
}

// ─── test setup helpers ────────────────────────────────────────────

async fn fresh_app() -> (axum::Router, Arc<AppState>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let storage_path = dir.path().join("storage");
    std::fs::create_dir_all(storage_path.join("cache")).expect("mkdir cache");
    // STORAGE_PATH must be set BEFORE any handler runs `storage_root()`.
    // SAFETY: tests run in a separate process here; if cargo test ever
    // parallelises across these files it would race. The lifetime of
    // this env var is the whole test process.
    unsafe { std::env::set_var("STORAGE_PATH", storage_path.to_string_lossy().to_string()) };

    let db_path = dir.path().join("test.db");
    let url = format!("sqlite://{}?mode=rwc", db_path.display().to_string().replace('\\', "/"));

    #[cfg(feature = "rag")]
    cove_studio::embeddings::register_sqlite_vec_auto_extension();

    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connect sqlite");
    sqlx::migrate!("./migrations").run(&pool).await.expect("migrate");

    let sessions = cove_studio::auth::SessionStore::new(pool.clone());
    let state = AppState {
        db: pool,
        sessions,
        biometric_tx: None,
        no_tools_models: Default::default(),
        mcp_discovery_cache: Default::default(),
        #[cfg(feature = "rag")]
        embeddings: None,
        #[cfg(feature = "rag")]
        scans: Default::default(),
        corpus_plugins: Default::default(),
        corpus_adapters: Default::default(),
        corpus_import_progress: Default::default(),
        workflow_presets: Default::default(),
        column_presets: Default::default(),
        model_catalogue: Arc::new(cove_studio::presets::model::ModelCatalogue {
            schema_version: 1,
            providers: vec![],
        }),
        docx_templates: Default::default(),
    };
    let state = Arc::new(state);

    let app = axum::Router::new()
        .nest("/chat", cove_studio::routes::chat::router())
        .with_state(state.clone());

    std::mem::forget(dir); // keep storage alive for the duration of the test
    (app, state)
}

async fn make_user_with_gemini(state: &AppState, api_key: &str, hyde: bool) -> String {
    let user_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO user_profiles (id, username, display_name, pin_hash) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(&user_id)
    .bind(format!("e2e-{}", &user_id[..8]))
    .bind("E2E")
    .bind("dummy-not-a-real-hash")
    .execute(&state.db)
    .await
    .expect("insert user");

    sqlx::query(
        "INSERT INTO user_settings (user_id, main_model, gemini_api_key, hyde_enabled, locale, default_domain, updated_at) \
         VALUES (?, 'gemini-2.5-flash', ?, ?, 'it', 'medical', datetime('now'))",
    )
    .bind(&user_id)
    .bind(api_key)
    .bind(if hyde { 1 } else { 0 })
    .execute(&state.db)
    .await
    .expect("insert user_settings");

    state.sessions.create(&user_id).await.expect("create session")
}

async fn create_chat(state: &AppState, user_id: &str) -> String {
    let chat_id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO chats (id, user_id, project_id, title) VALUES (?, ?, NULL, 'medical-e2e')")
        .bind(&chat_id)
        .bind(user_id)
        .execute(&state.db)
        .await
        .expect("insert chat");
    chat_id
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Place a PDF in `<storage_root>/cache/<hash>.pdf` + an extracted
/// `<hash>.txt`, INSERT a documents row linked to the chat. Mirrors
/// what `POST /document` with `cache=true` would have done.
async fn place_pdf_in_cache(
    state: &AppState,
    user_id: &str,
    chat_id: &str,
    pdf_path: &Path,
) -> (String, String) {
    let bytes = std::fs::read(pdf_path).expect("read pdf");
    let hash = hex_sha256(&bytes);
    let filename = pdf_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let storage_root = PathBuf::from(std::env::var("STORAGE_PATH").expect("STORAGE_PATH"));
    let bin_key = format!("cache/{hash}.pdf");
    let txt_key = format!("cache/{hash}.txt");
    let bin_path = storage_root.join(&bin_key);
    let txt_path = storage_root.join(&txt_key);

    if !bin_path.exists() {
        std::fs::write(&bin_path, &bytes).expect("write cached binary");
    }
    if !txt_path.exists() {
        let (text, _) = cove_studio::sync::scanner::extract_text_dispatch(pdf_path, &bytes)
            .expect("pdfium text extract");
        std::fs::write(&txt_path, text.as_bytes()).expect("write cached text");
    }

    let doc_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO documents \
         (id, user_id, chat_id, filename, file_type, size_bytes, storage_path, status, content_hash, extracted_text_path, domain) \
         VALUES (?, ?, ?, ?, 'pdf', ?, ?, 'ready', ?, ?, 'medical')",
    )
    .bind(&doc_id)
    .bind(user_id)
    .bind(chat_id)
    .bind(&filename)
    .bind(bytes.len() as i64)
    .bind(&bin_key)
    .bind(&hash)
    .bind(&txt_key)
    .execute(&state.db)
    .await
    .expect("insert document row");

    (doc_id, filename)
}

/// Walk the SSE response body, return the LAST `citations` event
/// payload (the final one is the authoritative one — earlier
/// `content_replace` / `citation_data` events are intermediate state).
fn parse_citations_event(body: &str) -> Option<Value> {
    let mut last: Option<Value> = None;
    for line in body.lines() {
        let Some(rest) = line.strip_prefix("data: ") else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(rest) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) == Some("citations") {
            last = Some(v);
        }
    }
    last
}

/// Walk the SSE body and reassemble the assistant's prose from
/// `content_delta` / `content_replace` events. Used to count `[`
/// brackets for the report (matches what the user sees on screen).
fn reassemble_response_text(body: &str) -> String {
    let mut text = String::new();
    for line in body.lines() {
        let Some(rest) = line.strip_prefix("data: ") else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(rest) else {
            continue;
        };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("content_delta") => {
                if let Some(t) = v.get("text").and_then(|x| x.as_str()) {
                    text.push_str(t);
                }
            }
            Some("content_replace") => {
                if let Some(t) = v.get("text").and_then(|x| x.as_str()) {
                    text = t.to_string();
                }
            }
            _ => {}
        }
    }
    text
}

fn classify_page(p: Option<&Value>) -> (&'static str, Option<i64>) {
    match p {
        None | Some(Value::Null) => ("absent", None),
        Some(Value::Number(n)) => ("number", n.as_i64()),
        Some(Value::String(s)) => {
            // "1-3" / "1–3" — take the first run of digits.
            let mut buf = String::new();
            for c in s.chars() {
                if c.is_ascii_digit() {
                    buf.push(c);
                } else if !buf.is_empty() {
                    break;
                }
            }
            let first = buf.parse::<i64>().ok();
            let kind = if s.contains('-') || s.contains('–') {
                "range"
            } else if first.is_some() {
                "number"
            } else {
                "absent"
            };
            (kind, first)
        }
        _ => ("absent", None),
    }
}

fn build_scenario_report(name: &str, sse_body: &str) -> ScenarioReport {
    let response_text = reassemble_response_text(sse_body);
    let bracket_count = response_text.matches('[').count();
    let has_citations_block = response_text.to_ascii_lowercase().contains("<citations>");

    let citations_value = parse_citations_event(sse_body);
    let arr = citations_value
        .as_ref()
        .and_then(|v| v.get("citations"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut per: Vec<PerCitation> = Vec::with_capacity(arr.len());
    let mut quote_lens: Vec<usize> = Vec::with_capacity(arr.len());
    let mut source_count: std::collections::HashMap<String, usize> = Default::default();
    let mut docid_count: std::collections::HashMap<String, usize> = Default::default();
    for c in &arr {
        let r#ref = c.get("ref").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let doc_id = c.get("doc_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let source = c.get("source").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let quote = c.get("quote").and_then(|v| v.as_str()).unwrap_or("");
        let quote_len = quote.chars().count();
        let (page_kind, page_first) = classify_page(c.get("page"));

        *source_count.entry(source.clone()).or_insert(0) += 1;
        *docid_count.entry(doc_id.clone()).or_insert(0) += 1;
        quote_lens.push(quote_len);

        per.push(PerCitation {
            r#ref,
            doc_id,
            source,
            page_kind: page_kind.to_string(),
            page_first,
            quote_len,
        });
    }

    let n = per.len() as f64;
    let pct_precise_page = if n > 0.0 {
        per.iter().filter(|c| c.page_kind == "number").count() as f64 / n
    } else {
        0.0
    };
    let pct_range_page = if n > 0.0 {
        per.iter().filter(|c| c.page_kind == "range").count() as f64 / n
    } else {
        0.0
    };
    let pct_no_page = if n > 0.0 {
        per.iter().filter(|c| c.page_kind == "absent").count() as f64 / n
    } else {
        0.0
    };
    let pct_short_quote = if n > 0.0 {
        per.iter().filter(|c| c.quote_len < 10).count() as f64 / n
    } else {
        0.0
    };
    quote_lens.sort_unstable();
    let median_quote_len = if quote_lens.is_empty() {
        0
    } else {
        quote_lens[quote_lens.len() / 2]
    };

    let mut source_distribution = serde_json::Map::new();
    for (k, v) in source_count {
        source_distribution.insert(k, Value::from(v));
    }
    let mut doc_id_distribution = serde_json::Map::new();
    for (k, v) in docid_count {
        doc_id_distribution.insert(k, Value::from(v));
    }

    let sample_citations = per.iter().take(5).cloned().collect();

    ScenarioReport {
        scenario: name.to_string(),
        response_chars: response_text.chars().count(),
        bracket_count,
        has_citations_block,
        citations_count: per.len(),
        pct_precise_page,
        pct_range_page,
        pct_no_page,
        pct_short_quote,
        median_quote_len,
        source_distribution,
        doc_id_distribution,
        sample_citations,
    }
}

impl Clone for PerCitation {
    fn clone(&self) -> Self {
        Self {
            r#ref: self.r#ref.clone(),
            doc_id: self.doc_id.clone(),
            source: self.source.clone(),
            page_kind: self.page_kind.clone(),
            page_first: self.page_first,
            quote_len: self.quote_len,
        }
    }
}

// ─── the actual test ───────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires GEMINI_API_KEY env + costs LLM tokens"]
async fn medical_citations_quality_report() {
    let api_key = std::env::var("GEMINI_API_KEY").unwrap_or_default();
    if api_key.is_empty() {
        eprintln!("SKIP: GEMINI_API_KEY env not set");
        return;
    }
    let hyde_enabled = std::env::var("E2E_HYDE")
        .map(|v| matches!(v.trim(), "1" | "true" | "yes"))
        .unwrap_or(false);
    let model = std::env::var("E2E_MODEL").unwrap_or_else(|_| "gemini-2.5-flash".to_string());

    let (app, state) = fresh_app().await;
    let token = make_user_with_gemini(&state, &api_key, hyde_enabled).await;
    let user_row: (String,) = sqlx::query_as("SELECT user_id FROM sessions WHERE token = ?")
        .bind(&token)
        .fetch_one(&state.db)
        .await
        .expect("session → user");
    let user_id = user_row.0;
    let chat_id = create_chat(&state, &user_id).await;

    // Upload the 10 PDFs straight into the cache.
    let medical_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/medical");
    let mut doc_refs: Vec<(String, String)> = Vec::with_capacity(10);
    for i in 1..=10 {
        let pdf = medical_dir.join(format!("CARTELLA_TEST_{i:03}.pdf"));
        let (doc_id, filename) = place_pdf_in_cache(&state, &user_id, &chat_id, &pdf).await;
        doc_refs.push((doc_id, filename));
    }

    let files_arr: Vec<Value> = doc_refs
        .iter()
        .map(|(id, name)| json!({
            "document_id": id,
            "filename": name,
            "pii_protected": false,
        }))
        .collect();

    // Scenario 1: analyze.
    let body1 = json!({
        "chat_id": chat_id,
        "model": model,
        "messages": [{
            "role": "user",
            "content": "analizza la cartella clinica",
            "files": files_arr.clone(),
        }],
    });
    eprintln!("[e2e] running scenario `analyze` model={model} hyde={hyde_enabled}");
    let t0 = std::time::Instant::now();
    let resp1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/chat/")
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(body1.to_string()))
                .unwrap(),
        )
        .await
        .expect("scenario 1 request");
    assert_eq!(resp1.status(), StatusCode::OK, "scenario 1 status");
    let body1_bytes = axum::body::to_bytes(resp1.into_body(), usize::MAX)
        .await
        .expect("scenario 1 body");
    let body1_text = String::from_utf8_lossy(&body1_bytes).into_owned();
    let elapsed1 = t0.elapsed();
    eprintln!("[e2e] scenario `analyze` completed in {:.1}s", elapsed1.as_secs_f64());
    let mut report1 = build_scenario_report("analyze", &body1_text);
    report1.scenario = format!("analyze (took {:.1}s)", elapsed1.as_secs_f64());

    // Scenario 2: template follow-up.
    let body2 = json!({
        "chat_id": chat_id,
        "model": model,
        "messages": [
            {
                "role": "user",
                "content": "analizza la cartella clinica",
                "files": files_arr.clone(),
            },
            {
                "role": "assistant",
                "content": "(analisi precedente — vedi turno 1)",
            },
            {
                "role": "user",
                "content": "prepara la relazione medico-legale per l'assicurazione",
                "files": files_arr.clone(),
            },
        ],
    });
    eprintln!("[e2e] running scenario `template`");
    let t1 = std::time::Instant::now();
    let resp2 = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/chat/")
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(body2.to_string()))
                .unwrap(),
        )
        .await
        .expect("scenario 2 request");
    assert_eq!(resp2.status(), StatusCode::OK, "scenario 2 status");
    let body2_bytes = axum::body::to_bytes(resp2.into_body(), usize::MAX)
        .await
        .expect("scenario 2 body");
    let body2_text = String::from_utf8_lossy(&body2_bytes).into_owned();
    let elapsed2 = t1.elapsed();
    eprintln!("[e2e] scenario `template` completed in {:.1}s", elapsed2.as_secs_f64());
    let mut report2 = build_scenario_report("template", &body2_text);
    report2.scenario = format!("template (took {:.1}s)", elapsed2.as_secs_f64());

    let report = RunReport {
        timestamp_utc: chrono::Utc::now().to_rfc3339(),
        git_commit: option_env!("GIT_COMMIT").unwrap_or("unknown").to_string(),
        model,
        hyde_enabled,
        scenarios: vec![report1, report2],
    };

    println!("\n===== CITATION QUALITY REPORT =====");
    println!("{}", serde_json::to_string_pretty(&report).expect("serialize report"));
    println!("===== END REPORT =====\n");

    // Sanity assertions — generous bounds, the point is the report.
    assert!(
        report.scenarios[0].citations_count > 0,
        "analyze scenario produced 0 citations — pipeline broken"
    );
    assert!(
        report.scenarios[0].response_chars > 1000,
        "analyze scenario produced suspiciously short response ({} chars)",
        report.scenarios[0].response_chars
    );
}
