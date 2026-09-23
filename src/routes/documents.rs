use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;

use crate::{auth::middleware::AuthUser, storage::make_storage, AppState};

fn storage_root() -> PathBuf {
    // Mirror `LocalStorage::new()` (src/storage/mod.rs) so the
    // `STORAGE_PATH` env override and the user-writable default land
    // both layers on the same base directory. Using the old hardcoded
    // `./data/storage` literal here was the v0.3.0 ACCESS_DENIED bug
    // for installed MSIs all over again, just on a different route.
    PathBuf::from(std::env::var("STORAGE_PATH").unwrap_or_else(|_| {
        crate::storage::default_storage_path()
    }))
}

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn err(status: StatusCode, msg: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"detail": msg})))
}

pub fn router() -> Router<Arc<AppState>> {
    // axum's DefaultBodyLimit caps multipart bodies at 2 MB out of the box.
    // A handful of docx/pdf docs uploaded together blow past that and the
    // connection is reset mid-stream — the browser surfaces it as
    // `TypeError: Failed to fetch`, not as an HTTP 413, which is why the
    // backend log shows nothing when concurrent uploads fail. 100 MB is
    // safely above any realistic legal document we expect.
    Router::new()
        .route("/", get(list_documents).post(upload_document))
        .route("/{id}", get(get_document).delete(delete_document))
        // Display endpoint used by the in-app viewer (DocView.tsx / DocxView.tsx).
        // Returns the file bytes with the appropriate Content-Type so the
        // frontend can pick PDF.js or docx-preview based on it.
        .route("/{id}/display", get(display_document))
        .route("/{id}/docx", get(display_document))
        .route("/{id}/text", get(display_document))
        .route("/{id}/download", get(download_document))
        .route("/{id}/url", get(get_document_url))
        // Per-chat Accept / Reject decision — see migration 0029.
        .route("/{id}/decision", axum::routing::post(set_decision))
        // Absolute on-disk path for the Tauri shell's open-in-Word command.
        .route("/{id}/file_path", get(get_document_file_path))
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
}

// ---------------------------------------------------------------------------
// POST /document/:id/decision
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct DecisionBody {
    /// `accepted` or `rejected`.
    decision: String,
    /// Required when `decision == "rejected"`. Free-text explanation
    /// the user wants the model to take into account on the next try.
    #[serde(default)]
    reason: Option<String>,
}

/// Flip the per-chat accept/reject state of a document. Rejecting also
/// generates a one-shot LLM summary of the document so subsequent chat
/// turns can swap the full text with `"<filename> rejected with reason
/// X — summary: Y — try again"` (see `routes/chat.rs::load_attached_docs`).
async fn set_decision(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(doc_id): Path<String>,
    Json(body): Json<DecisionBody>,
) -> ApiResult {
    let decision = body.decision.as_str();
    if !matches!(decision, "accepted" | "rejected") {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "decision must be 'accepted' or 'rejected'",
        ));
    }

    let row: Option<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT filename, file_type, storage_path, extracted_text_path \
         FROM documents WHERE id = ? AND user_id = ?",
    )
    .bind(&doc_id)
    .bind(&auth.user_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    let Some((filename, file_type, storage_path, extracted_text_path)) = row else {
        return Err(err(StatusCode::NOT_FOUND, "document not found"));
    };

    if decision == "accepted" {
        // Flip back to the default. Reason/summary are intentionally
        // preserved so a future re-reject doesn't lose the previous
        // motivation — the UI can pre-fill the modal with the prior
        // text if the user re-opens it.
        let _ = sqlx::query(
            "UPDATE documents SET decision = 'accepted' WHERE id = ? AND user_id = ?",
        )
        .bind(&doc_id)
        .bind(&auth.user_id)
        .execute(&state.db)
        .await;
        let archived: Option<(Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT decision_reason, decision_summary FROM documents WHERE id = ?",
        )
        .bind(&doc_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
        let (reason, summary) = archived.unwrap_or((None, None));
        return Ok(Json(json!({
            "decision": "accepted",
            "reason": reason,
            "summary": summary,
        })));
    }

    // decision == "rejected"
    let reason = body
        .reason
        .as_deref()
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if reason.chars().count() < 10 {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "reason must be at least 10 characters",
        ));
    }

    // Load the document text. The cached extracted-text path wins —
    // it's already plain UTF-8 and skips re-running the per-format
    // extractor. Only fall back to a fresh extraction when no cache
    // exists (rare for generate_docx output, which always writes the
    // cache; common for legacy uploaded files predating migration
    // 0014).
    let text = load_text_for_summary(&file_type, storage_path.as_deref(), extracted_text_path.as_deref())
        .await
        .unwrap_or_default();
    if text.trim().is_empty() {
        tracing::warn!(
            "[doc-decision] no extractable text for {filename} (id={doc_id}) — \
             persisting reject with empty summary"
        );
    }

    // Summarizer credentials come from the user's saved settings —
    // same model the chat handler would have used for this turn.
    let user_settings = crate::routes::user::fetch_llm_settings(&state.db, &auth.user_id)
        .await
        .ok();
    let raw_model = user_settings
        .as_ref()
        .and_then(|s| s.main_model.clone())
        .unwrap_or_else(|| "gemini-2.5-flash".to_string());
    let local_config = crate::routes::chat::build_local_config(&raw_model, user_settings.as_ref());
    let creds = crate::llm::summarize::SummarizerCreds {
        local_config,
        claude_api_key: user_settings.as_ref().and_then(|s| s.claude_api_key.clone()),
        gemini_api_key: user_settings.as_ref().and_then(|s| s.gemini_api_key.clone()),
        gemini_region: user_settings.as_ref().and_then(|s| s.gemini_region.clone()),
    };

    let summary = generate_rejection_summary(&text, &reason, &filename, &raw_model, &creds)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(
                "[doc-decision] rejection summary call failed for {filename} (id={doc_id}): {e:#}"
            );
            format!(
                "(Riassunto automatico non disponibile: {e}) \
                 Motivo utente: {reason}"
            )
        });

    let _ = sqlx::query(
        "UPDATE documents SET decision = 'rejected', decision_reason = ?, \
         decision_summary = ? WHERE id = ? AND user_id = ?",
    )
    .bind(&reason)
    .bind(&summary)
    .bind(&doc_id)
    .bind(&auth.user_id)
    .execute(&state.db)
    .await;

    tracing::info!(
        "[doc-decision] rejected {filename} (id={doc_id}) — reason {} chars, summary {} chars",
        reason.chars().count(),
        summary.chars().count()
    );

    Ok(Json(json!({
        "decision": "rejected",
        "reason": reason,
        "summary": summary,
    })))
}

// ---------------------------------------------------------------------------
// GET /document/:id/file_path
// ---------------------------------------------------------------------------

/// Resolve the document's absolute on-disk path so the Tauri shell can
/// pass it to `crate::open::that` for the new "Apri in Word" /
/// "Open externally" toolbar action introduced alongside the
/// Accept/Reject decision flow. The frontend never sees the path —
/// the Svelte side calls this endpoint, immediately hands the result
/// to the Tauri IPC `open_external_path` command, and drops it.
///
/// Auth-gated and ownership-checked (user_id), so a leaked endpoint
/// can't be hit to enumerate other users' files.
async fn get_document_file_path(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(doc_id): Path<String>,
) -> ApiResult {
    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT storage_path FROM documents WHERE id = ? AND user_id = ?",
    )
    .bind(&doc_id)
    .bind(&auth.user_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    let Some((Some(storage_key),)) = row else {
        return Err(err(StatusCode::NOT_FOUND, "document not found"));
    };
    let base = storage_root();
    let abs = base.join(&storage_key);
    if !abs.exists() {
        return Err(err(
            StatusCode::NOT_FOUND,
            "document bytes missing on disk",
        ));
    }
    Ok(Json(json!({
        "path": abs.to_string_lossy().to_string(),
        "storage_root": base.to_string_lossy().to_string(),
    })))
}

/// Best-effort text load for the summarizer. Pulls the cached
/// extracted text (set at upload time / by generate_docx) when
/// available; otherwise re-runs `extract_text_dispatch` on the raw
/// bytes. Either path returns "" rather than an error — the caller
/// logs and persists the rejection with whatever it got.
async fn load_text_for_summary(
    file_type: &str,
    storage_path: Option<&str>,
    extracted_text_path: Option<&str>,
) -> Option<String> {
    let storage = make_storage().ok()?;
    if let Some(key) = extracted_text_path {
        if let Ok(bytes) = storage.get(key).await {
            let s = String::from_utf8_lossy(&bytes).into_owned();
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    let storage_key = storage_path?;
    let bytes = storage.get(storage_key).await.ok()?;
    let tmp_name = std::path::PathBuf::from(format!("blob.{file_type}"));
    let (text, _skip) = crate::sync::scanner::extract_text_dispatch(&tmp_name, &bytes).ok()?;
    Some(text)
}

async fn generate_rejection_summary(
    doc_text: &str,
    reason: &str,
    filename: &str,
    model: &str,
    creds: &crate::llm::summarize::SummarizerCreds,
) -> anyhow::Result<String> {
    // Truncate to keep the prompt tractable on smaller-context models.
    // ~30k chars ~ 7-8k tokens of doc body, well under every supported
    // model's context budget after the prompt scaffolding lands.
    const MAX_DOC_CHARS: usize = 30_000;
    let truncated = if doc_text.chars().count() > MAX_DOC_CHARS {
        let slice: String = doc_text.chars().take(MAX_DOC_CHARS).collect();
        format!("{slice}\n[...truncato per limiti di prompt]")
    } else {
        doc_text.to_string()
    };

    let prompt = format!(
        "Riassumi in al massimo 700 caratteri il seguente documento, \
         preservando *integralmente* le parti che rispondono o si riferiscono \
         alla richiesta iniziale dell'utente. L'utente sta rifiutando questa \
         versione del file `{filename}` con il seguente motivo:\n\
         \n\
         «{reason}»\n\
         \n\
         Il riassunto deve permettere a un modello AI in un turno successivo \
         di capire (a) di cosa trattava il documento e (b) cosa l'utente \
         considerava sbagliato. Niente preamboli, niente \"Il documento \
         tratta…\" — vai diretto al contenuto.\n\
         \n\
         === Documento ===\n{truncated}\n=== Fine documento ===\n\
         \n\
         Riassunto (max 700 caratteri):"
    );

    let params = crate::llm::types::StreamParams {
        model: model.to_string(),
        system_prompt: "Sei un assistente che produce riassunti tecnici e \
                        concisi. Rispondi solo con il riassunto richiesto."
            .to_string(),
        system_volatile: String::new(),
        messages: vec![crate::llm::types::Message::user(prompt)],
        tools: vec![],
        max_iterations: 1,
        enable_thinking: false,
        local_config: creds.local_config.clone(),
        claude_api_key: creds.claude_api_key.clone(),
        gemini_api_key: creds.gemini_api_key.clone(),
        gemini_region: creds.gemini_region.clone(),
        // Doc-summarisation is one-shot — no Mistral cache benefit.
        chat_id: None,
        // One-shot path inherits Mistral defaults (Commit A behaviour).
        mistral_opts: None,
        response_schema: None,
    };

    let summary = match crate::llm::provider_for_model(model) {
        crate::llm::Provider::Claude => crate::llm::claude::complete(params).await?,
        crate::llm::Provider::OpenAI => crate::llm::local::complete(params).await?,
        crate::llm::Provider::Gemini => crate::llm::gemini::complete(params).await?,
        crate::llm::Provider::Mistral => crate::llm::mistral::complete(params).await?,
    };
    Ok(summary.trim().to_string())
}

// ---------------------------------------------------------------------------
// GET /document?project_id=…
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct ListQuery {
    project_id: Option<String>,
    /// Optional `?domain=…` filter added with migration 0018. Useful
    /// for the global-documents (project_id IS NULL) view in the UI
    /// where users want to slice their personal pool by vertical.
    domain: Option<String>,
}

async fn list_documents(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<ListQuery>,
) -> ApiResult {
    let domain_filter: Option<&str> = q
        .domain
        .as_deref()
        .filter(|s| !s.is_empty() && crate::domain::is_valid(s));

    let mut sql = String::from(
        "SELECT id, filename, file_type, size_bytes, status, created_at, domain, \
                project_folder_id, project_id \
         FROM documents WHERE user_id = ?",
    );
    if q.project_id.is_some() {
        sql.push_str(" AND project_id = ?");
    }
    if domain_filter.is_some() {
        sql.push_str(" AND domain = ?");
    }
    sql.push_str(" ORDER BY created_at DESC");

    let mut query = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            i64,
            Option<String>,
            String,
            String,
            Option<String>,
            Option<String>,
        ),
    >(&sql)
    .bind(&auth.user_id);
    if let Some(pid) = &q.project_id {
        query = query.bind(pid);
    }
    if let Some(d) = domain_filter {
        query = query.bind(d);
    }
    let rows = query
        .fetch_all(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let docs: Vec<Value> = rows
        .into_iter()
        .map(|(id, filename, file_type, size, status, created_at, domain, project_folder_id, project_id)| {
            json!({ "id": id, "filename": filename, "file_type": file_type,
                    "size_bytes": size, "status": status,
                    "domain": domain, "created_at": created_at,
                    "project_folder_id": project_folder_id,
                    "project_id": project_id })
        })
        .collect();

    Ok(Json(json!({ "documents": docs })))
}

// ---------------------------------------------------------------------------
// POST /document  — multipart upload
// Fields: file (binary), project_id? (text)
// ---------------------------------------------------------------------------
async fn upload_document(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    mut multipart: Multipart,
) -> ApiResult {
    tracing::info!("[upload] POST /document user={}", auth.user_id);
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut filename: Option<String> = None;
    let mut project_id: Option<String> = None;
    // `cache=true` is the chat-composer signal: store the binary +
    // extracted text under data/storage/cache, keyed by SHA-256 of the
    // bytes. The chat row may not exist at upload time (the composer
    // materialises the chat on first send), so chat_id is wired up
    // later by the /chat send handler — and the chat-delete handler
    // ref-counts by content_hash before unlinking the on-disk files.
    let mut cache = false;
    // Optional `domain` multipart field — when omitted, project-scoped
    // uploads inherit the project's domain below, and standalone
    // uploads fall back to the schema default ('legal') via the
    // INSERT (no need to bind a value).
    let mut domain: Option<String> = None;
    while let Some(field) = multipart.next_field().await.map_err(|e| {
        tracing::warn!("[upload] multipart parse error: {e}");
        err(StatusCode::BAD_REQUEST, &e.to_string())
    })? {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "file" => {
                filename = field.file_name().map(|s| s.to_string());
                let bytes = field.bytes().await.map_err(|e| {
                    tracing::warn!(
                        "[upload] failed reading file field (filename={:?}): {e}",
                        filename
                    );
                    err(StatusCode::BAD_REQUEST, &e.to_string())
                })?;
                tracing::info!(
                    "[upload] received file field name={:?} size={} bytes",
                    filename,
                    bytes.len()
                );
                file_bytes = Some(bytes.to_vec());
            }
            "project_id" => {
                let text = field.text().await.map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
                if !text.trim().is_empty() {
                    project_id = Some(text.trim().to_string());
                }
            }
            "cache" => {
                let text = field.text().await.map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
                cache = matches!(text.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes");
            }
            "domain" => {
                let text = field.text().await.map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
                let trimmed = text.trim();
                if !trimmed.is_empty() && crate::domain::is_valid(trimmed) {
                    domain = Some(trimmed.to_string());
                }
            }
            _ => {}
        }
    }

    let data = file_bytes.ok_or_else(|| err(StatusCode::BAD_REQUEST, "No file field in multipart"))?;
    let fname = filename.unwrap_or_else(|| "upload".to_string());
    let ext = fname.rsplit('.').next().unwrap_or("").to_lowercase();
    let file_type = match ext.as_str() {
        "pdf" => "pdf",
        "docx" => "docx",
        "rtf" => "rtf",
        "xlsx" => "xlsx",
        "xls" => "xls",
        "xlsb" => "xlsb",
        "ods" => "ods",
        // Readable since the onboarding funnel arrived: the legacy
        // Office family converts to Markdown before parsing. `.doc`
        // was already offered in the composer while nothing could read
        // it, which is the worst of both.
        "doc" => "doc",
        "ppt" | "pps" | "pot" => "ppt",
        "xlsm" => "xlsm",
        "csv" => "csv",
        "txt" => "txt",
        "md" => "md",
        "png" => "png",
        "jpg" | "jpeg" => "jpeg",
        "tif" | "tiff" => "tiff",
        _ => "other",
    };

    let doc_id = uuid::Uuid::new_v4().to_string();
    let storage = make_storage().map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let size = data.len() as i64;

    // Cache uploads (chat-attached): key files by SHA-256 of the
    // binary so re-uploads of identical content dedupe and same
    // user-facing filename across different chats can't collide on
    // disk. We also extract plain text once per unique hash so the
    // chat send handler doesn't re-parse a 200-page PDF on every
    // turn. Skip extraction silently if the binary or text already
    // exist on disk — same hash means identical bytes.
    // Filled by the extraction below. The default covers the
    // non-cache branch, where text is read lazily and the verdict is
    // recorded the first time the document is actually parsed.
    let mut extraction_status = "ready".to_string();
    let mut extraction_reason: Option<String> = None;

    let (storage_key, content_hash, extracted_text_path) = if cache {
        let hash = {
            let mut hasher = Sha256::new();
            hasher.update(&data);
            format!("{:x}", hasher.finalize())
        };
        let bin_ext = if ext.is_empty() { "bin".to_string() } else { ext.clone() };
        let bin_key = format!("cache/{}.{}", hash, bin_ext);
        // `.extracted.txt`, not `.txt`: for a .txt or .md upload the
        // binary key would otherwise be the *same path* as the
        // extracted-text key, so writing the binary made the text look
        // already-extracted and the extraction step was skipped
        // silently. Old caches under `<hash>.txt` stay readable —
        // documents store their own `extracted_text_path`.
        let txt_key = format!("cache/{}.extracted.txt", hash);

        let root = storage_root();
        let bin_abs = root.join(bin_key.replace('/', std::path::MAIN_SEPARATOR_STR));
        let txt_abs = root.join(txt_key.replace('/', std::path::MAIN_SEPARATOR_STR));

        if !bin_abs.exists() {
            storage
                .put(&bin_key, &data, "application/octet-stream")
                .await
                .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
            tracing::info!("[upload] cache binary written: {} ({} bytes)", bin_key, data.len());
        } else {
            tracing::info!("[upload] cache binary already exists, reusing: {}", bin_key);
        }

        if !txt_abs.exists() {
            // The funnel keys off the path, so the absolute path of the
            // binary we just wrote is what it needs (pdfium also wants
            // an on-disk path for PDFs).
            let bin_for_ingest = bin_abs.clone();
            match tokio::task::spawn_blocking(move || {
                crate::ingest::ingest_blocking(&bin_for_ingest)
            })
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &format!("ingest join: {e:?}")))?
            {
                Ok(doc) => {
                    if let Some(code) = doc.outcome.code() {
                        tracing::info!(
                            "[upload] {fname} ({hash}) carries no usable text: {code}"
                        );
                    }
                    extraction_status = doc.outcome.tag().to_string();
                    extraction_reason = doc.outcome.code().map(str::to_string);
                    storage
                        .put(&txt_key, doc.text.as_bytes(), "text/plain; charset=utf-8")
                        .await
                        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
                    tracing::info!(
                        "[upload] cache text written: {} ({} chars, {} sections)",
                        txt_key,
                        doc.text.len(),
                        doc.sections.len(),
                    );
                }
                Err(e) => {
                    // Not a silent empty string any more: the row says
                    // `failed` and carries the sentence the interface
                    // shows and the prompt states.
                    let reason = e
                        .downcast_ref::<crate::ingest::IngestError>()
                        .map(|ie| ie.stored_reason())
                        .unwrap_or_else(|| format!("read_failed: {e:#}"));
                    tracing::warn!("[upload] {fname} ({hash}) could not be read: {reason}");
                    extraction_status = "failed".to_string();
                    extraction_reason = Some(reason);
                    // Keep the empty marker so a reload does not retry
                    // a file we already know we cannot read.
                    let _ = storage
                        .put(&txt_key, b"", "text/plain; charset=utf-8")
                        .await;
                }
            }
        } else {
            // A re-upload of content we have already extracted. The
            // verdict has to come from that text, not from an
            // assumption: a cached empty extraction means the file was
            // unreadable then and still is now.
            let cached = storage.get(&txt_key).await.unwrap_or_default();
            let cached = String::from_utf8_lossy(&cached);
            if cached.trim().is_empty() {
                extraction_status = "no_text".to_string();
                extraction_reason =
                    Some(crate::ingest::outcome::no_text::EMPTY_FILE.to_string());
            }
            tracing::info!(
                "[upload] cache text already exists, reusing: {} ({} chars, status={})",
                txt_key,
                cached.len(),
                extraction_status,
            );
        }

        (bin_key, Some(hash), Some(txt_key))
    } else {
        // Legacy (non-cache) layout: per-user, per-doc-id. Text
        // extraction still happens lazily in load_document_text (with
        // file-type-driven extension synthesis). We DO compute the
        // SHA-256 content hash though — `tabular_reviews` needs it to
        // dedupe re-uploads of the same content within a single
        // review (UX policy from 2026-06-06: a row for a file that
        // matches an existing row by (filename, content_hash) inherits
        // the existing row's extracted cells instead of re-running
        // the LLM). Hashing 50 MB takes <100ms; the cost is dwarfed
        // by upload bandwidth.
        let key = format!("documents/{}/{}", auth.user_id, doc_id);
        storage
            .put(&key, &data, "application/octet-stream")
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
        let hash = {
            let mut hasher = Sha256::new();
            hasher.update(&data);
            format!("{:x}", hasher.finalize())
        };
        (key, Some(hash), None)
    };

    // Resolve domain: explicit field wins; otherwise inherit from the
    // parent project; otherwise fall back to schema default ('legal').
    let resolved_domain: Option<String> = if let Some(d) = domain {
        Some(d)
    } else if let Some(pid) = &project_id {
        sqlx::query_as::<_, (String,)>(
            "SELECT domain FROM projects WHERE id = ? AND user_id = ?",
        )
        .bind(pid)
        .bind(&auth.user_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .map(|(d,)| d)
    } else {
        None
    };

    sqlx::query(
        "INSERT INTO documents (id, user_id, project_id, filename, file_type, size_bytes, storage_path, status, extraction_reason, content_hash, extracted_text_path, domain) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, COALESCE(?, 'legal'))",
    )
    .bind(&doc_id)
    .bind(&auth.user_id)
    .bind(&project_id)
    .bind(&fname)
    .bind(file_type)
    .bind(size)
    .bind(&storage_key)
    .bind(&extraction_status)
    .bind(&extraction_reason)
    .bind(&content_hash)
    .bind(&extracted_text_path)
    .bind(&resolved_domain)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({
        "id": doc_id,
        "filename": fname,
        "file_type": file_type,
        "size_bytes": size,
        "domain": resolved_domain.unwrap_or_else(|| "legal".to_string()),
        "status": extraction_status,
        "extraction_reason": extraction_reason,
    })))
}

// ---------------------------------------------------------------------------
// GET /document/:id
// ---------------------------------------------------------------------------
async fn get_document(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let row: Option<(
        String,
        String,
        String,
        i64,
        Option<String>,
        Option<String>,
        String,
        String,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT id, filename, file_type, size_bytes, storage_path, status, created_at, \
                decision, decision_reason, decision_summary \
         FROM documents WHERE id = ? AND user_id = ?",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (
        id,
        filename,
        file_type,
        size,
        storage_path,
        status,
        created_at,
        decision,
        decision_reason,
        decision_summary,
    ) = row.ok_or_else(|| err(StatusCode::NOT_FOUND, "Document not found"))?;

    Ok(Json(json!({
        "id": id,
        "filename": filename,
        "file_type": file_type,
        "size_bytes": size,
        "storage_path": storage_path,
        "status": status,
        "created_at": created_at,
        // Migration 0029 — per-chat accept/reject state. `accepted` is
        // the default for every row including pre-migration data.
        "decision": decision,
        "decision_reason": decision_reason,
        "decision_summary": decision_summary,
    })))
}

// ---------------------------------------------------------------------------
// GET /document/:id/display, /docx, /text — stream raw bytes for the viewer
// ---------------------------------------------------------------------------
async fn display_document(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Response {
    let row: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT filename, file_type, storage_path FROM documents WHERE id = ? AND user_id = ?",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();

    let Some((filename, file_type, Some(storage_path))) = row else {
        return (StatusCode::NOT_FOUND, "Document not found").into_response();
    };

    let storage = match crate::storage::make_storage() {
        Ok(s) => s,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let bytes = match storage.get(&storage_path).await {
        Ok(b) => b,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let content_type = match file_type.as_str() {
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "rtf" => "application/rtf",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "xls" => "application/vnd.ms-excel",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "csv" => "text/csv; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "md" => "text/markdown; charset=utf-8",
        "png" => "image/png",
        "jpeg" | "jpg" => "image/jpeg",
        "tiff" | "tif" => "image/tiff",
        _ => "application/octet-stream",
    };

    let mut resp = Response::new(Body::from(bytes));
    resp.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    if let Ok(disp) = HeaderValue::from_str(&format!("inline; filename=\"{filename}\"")) {
        resp.headers_mut().insert(header::CONTENT_DISPOSITION, disp);
    }
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=60"),
    );
    resp
}

// ---------------------------------------------------------------------------
// GET /document/:id/download — same bytes as /display but with
// `Content-Disposition: attachment` so a plain browser navigation
// triggers the OS save dialog. Used by the chat's download card to
// hand a generated .docx / .xlsx / … out of Cove Studio as a normal file
// the user can keep, mail, archive, etc. The /display sibling stays
// `inline` because the in-app PDF.js / docx-preview viewers need the
// webview to render the bytes in place rather than save them.
// ---------------------------------------------------------------------------
async fn download_document(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Response {
    let row: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT filename, file_type, storage_path FROM documents WHERE id = ? AND user_id = ?",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();

    let Some((filename, file_type, Some(storage_path))) = row else {
        return (StatusCode::NOT_FOUND, "Document not found").into_response();
    };

    let storage = match crate::storage::make_storage() {
        Ok(s) => s,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let bytes = match storage.get(&storage_path).await {
        Ok(b) => b,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let content_type = mime_for_extension(&file_type);
    let mut resp = Response::new(Body::from(bytes));
    resp.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    // RFC 6266: filename* with UTF-8 encoding for non-ASCII filenames,
    // plus a quoted ASCII fallback so older clients still pick something
    // sensible. The fallback strips any character outside the printable
    // ASCII range so a curly-quote in the filename can't break the
    // Content-Disposition header parser.
    let ascii_fallback: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii() && c != '"' && !c.is_control() {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded =
        percent_encoding::utf8_percent_encode(&filename, percent_encoding::NON_ALPHANUMERIC)
            .to_string();
    let disp = format!(
        "attachment; filename=\"{ascii_fallback}\"; filename*=UTF-8''{encoded}"
    );
    if let Ok(value) = HeaderValue::from_str(&disp) {
        resp.headers_mut().insert(header::CONTENT_DISPOSITION, value);
    }
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    resp
}

/// Map our `documents.file_type` column to a MIME string. Kept in sync
/// with the `display_document` handler so download and inline-display
/// return identical Content-Type for the same row.
fn mime_for_extension(ext: &str) -> &'static str {
    match ext {
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "rtf" => "application/rtf",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "xls" => "application/vnd.ms-excel",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "csv" => "text/csv; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "md" => "text/markdown; charset=utf-8",
        "png" => "image/png",
        "jpeg" | "jpg" => "image/jpeg",
        "tiff" | "tif" => "image/tiff",
        _ => "application/octet-stream",
    }
}

// ---------------------------------------------------------------------------
// GET /document/:id/url — frontend convenience: returns a URL the viewer
// can fetch later. In Cove Studio it's just an absolute /display URL because
// storage is local; remote-storage backends could return a presigned URL.
// ---------------------------------------------------------------------------
async fn get_document_url(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let owns: Option<(String,)> =
        sqlx::query_as("SELECT id FROM documents WHERE id = ? AND user_id = ?")
            .bind(&id)
            .bind(&auth.user_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    if owns.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "Document not found"));
    }
    let api_base = std::env::var("API_BASE_URL")
        .unwrap_or_else(|_| "http://localhost:3001".to_string());
    Ok(Json(json!({
        "url": format!("{api_base}/document/{id}/display"),
    })))
}

// ---------------------------------------------------------------------------
// DELETE /document/:id
// ---------------------------------------------------------------------------
async fn delete_document(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT storage_path FROM documents WHERE id = ? AND user_id = ?")
            .bind(&id)
            .bind(&auth.user_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (storage_path,) = row.ok_or_else(|| err(StatusCode::NOT_FOUND, "Document not found"))?;

    // Delete from storage
    if let Some(key) = storage_path {
        if let Ok(storage) = make_storage() {
            let _ = storage.delete(&key).await;
        }
    }

    sqlx::query("DELETE FROM documents WHERE id = ? AND user_id = ?")
        .bind(&id)
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({ "ok": true })))
}
