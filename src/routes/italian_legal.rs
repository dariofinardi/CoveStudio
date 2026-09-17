//! Italian-legal-corpus routes.
//!
//!   GET    /italian-legal/config             — user enable/sources
//!   PUT    /italian-legal/config
//!   POST   /italian-legal/import             — kick off background bulk import
//!   GET    /italian-legal/import-status      — progress of the bulk import
//!   POST   /italian-legal/search             — FTS5 over local index
//!   POST   /italian-legal/fetch              — pick a row → fetch text + index
//!   GET    /italian-legal/documents          — list synced docs
//!   DELETE /italian-legal/documents/:id      — drop a synced doc
//!
//! All paths are authenticated (AuthUser extractor) — same auth model
//! as the EUR-Lex routes.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;

use crate::{
    auth::middleware::AuthUser, corpora::italian_legal, storage::make_storage,
    AppState,
};

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn err(status: StatusCode, msg: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"detail": msg})))
}

fn storage_root() -> PathBuf {
    PathBuf::from(
        std::env::var("STORAGE_PATH").unwrap_or_else(|_| "./data/storage".to_string()),
    )
}

const CORPUS_ID: &str = "italian-legal";

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/config", get(get_config).put(put_config))
        .route("/import", post(start_import))
        .route("/import-status", get(import_status))
        .route("/search", post(search))
        .route("/fetch", post(fetch_row))
        .route("/documents", get(list_documents))
        .route("/documents/{id}", delete(delete_document))
        .route("/documents/{id}/resync", post(resync_document))
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

async fn get_config(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    let row: Option<(i64, String)> = sqlx::query_as(
        "SELECT enabled, sources FROM italian_corpus_settings WHERE user_id = ?",
    )
    .bind(&auth.user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (enabled, sources_json) = row
        .map(|(e, s)| (e != 0, s))
        .unwrap_or((false, r#"["normattiva","corte_costituzionale"]"#.to_string()));
    let sources: Vec<String> =
        serde_json::from_str(&sources_json).unwrap_or_default();

    Ok(Json(json!({
        "enabled": enabled,
        "sources": sources,
    })))
}

#[derive(Deserialize)]
struct ConfigPayload {
    enabled: bool,
    sources: Option<Vec<String>>,
}

async fn put_config(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<ConfigPayload>,
) -> ApiResult {
    let sources = body.sources.unwrap_or_else(|| {
        italian_legal::DEFAULT_SOURCES
            .iter()
            .map(|s| s.to_string())
            .collect()
    });
    let sources_json = serde_json::to_string(&sources).unwrap();

    sqlx::query(
        "INSERT INTO italian_corpus_settings (user_id, enabled, sources, updated_at) \
         VALUES (?, ?, ?, datetime('now')) \
         ON CONFLICT(user_id) DO UPDATE SET \
           enabled = excluded.enabled, \
           sources = excluded.sources, \
           updated_at = excluded.updated_at",
    )
    .bind(&auth.user_id)
    .bind(body.enabled as i64)
    .bind(&sources_json)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({
        "enabled": body.enabled,
        "sources": sources,
    })))
}

// ---------------------------------------------------------------------------
// Bulk import — kick off + poll
// ---------------------------------------------------------------------------

async fn start_import(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
) -> ApiResult {
    // Refuse if an import is already in flight (single-flight at the
    // user-facing level — the underlying job is also single-instance
    // because writes go through a single `italian_corpus_meta` row).
    let cur = italian_legal::read_progress(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    if cur.job_state == "downloading" || cur.job_state == "importing" {
        return Err(err(
            StatusCode::CONFLICT,
            "Import già in corso. Attendi il completamento.",
        ));
    }

    let db = Arc::new(state.db.clone());
    tokio::spawn(async move {
        if let Err(e) = italian_legal::run_import(db.clone()).await {
            tracing::warn!("[italian-legal] import failed: {e}");
            let _ = sqlx::query(
                "UPDATE italian_corpus_meta \
                 SET job_state = 'failed', job_error = ? WHERE id = 1",
            )
            .bind(e.to_string())
            .execute(&*db)
            .await;
        }
    });

    Ok(Json(json!({"started": true})))
}

async fn import_status(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
) -> ApiResult {
    let p = italian_legal::read_progress(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let percent = if p.total_shards == 0 {
        0
    } else {
        ((p.current_shard as f64 / p.total_shards as f64) * 100.0).round() as i64
    };
    Ok(Json(json!({
        "job_state": p.job_state,
        "current_shard": p.current_shard,
        "total_shards": p.total_shards,
        "rows_imported": p.rows_imported,
        "percent": percent,
        "row_count": p.row_count,
        "last_import_at": p.last_import_at,
        "dataset_revision": p.dataset_revision,
        "job_error": p.job_error,
    })))
}

// ---------------------------------------------------------------------------
// Search — FTS5 over the local index, with metadata filters
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct SearchPayload {
    /// Free-text query. Maps to FTS5 MATCH on title/authority/number.
    /// May be empty when filtering by source/year only.
    query: Option<String>,
    sources: Option<Vec<String>>,
    doc_types: Option<Vec<String>>,
    year_min: Option<i64>,
    year_max: Option<i64>,
    limit: Option<i64>,
}

async fn search(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(body): Json<SearchPayload>,
) -> ApiResult {
    let limit = body.limit.unwrap_or(50).clamp(1, 200);

    // Compose the SQL. We always select the same columns; the WHERE
    // clause varies depending on which filters the user passed.
    // We bind parameters inline for the static fragments and via
    // `bind` for the dynamic ones — placeholders only.
    let mut sql = String::from(
        "SELECT c.hf_id, c.row_offset, c.source, c.doc_type, c.title, \
                c.authority, c.number, c.year, c.date, c.text_length \
         FROM italian_corpus c ",
    );
    let mut binds: Vec<String> = Vec::new();
    let mut int_binds: Vec<i64> = Vec::new();
    let mut where_clauses: Vec<String> = Vec::new();

    let q = body.query.as_deref().map(|s| s.trim()).unwrap_or("");
    if !q.is_empty() {
        sql.push_str(
            "JOIN italian_corpus_fts f ON f.hf_id = c.hf_id ",
        );
        where_clauses.push("italian_corpus_fts MATCH ?".to_string());
        binds.push(escape_fts5(q));
    }

    if let Some(ss) = &body.sources {
        if !ss.is_empty() {
            let placeholders =
                ss.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            where_clauses.push(format!("c.source IN ({})", placeholders));
            for s in ss {
                binds.push(s.clone());
            }
        }
    }
    if let Some(dt) = &body.doc_types {
        if !dt.is_empty() {
            let placeholders =
                dt.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            where_clauses.push(format!("c.doc_type IN ({})", placeholders));
            for s in dt {
                binds.push(s.clone());
            }
        }
    }
    if let Some(y) = body.year_min {
        where_clauses.push("c.year >= ?".to_string());
        int_binds.push(y);
    }
    if let Some(y) = body.year_max {
        where_clauses.push("c.year <= ?".to_string());
        int_binds.push(y);
    }
    if !where_clauses.is_empty() {
        sql.push_str("WHERE ");
        sql.push_str(&where_clauses.join(" AND "));
    }
    if !q.is_empty() {
        sql.push_str(" ORDER BY rank ");
    } else {
        sql.push_str(" ORDER BY c.year DESC, c.date DESC ");
    }
    sql.push_str("LIMIT ?");

    let mut q_builder = sqlx::query_as::<
        _,
        (
            String,
            i64,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<String>,
            i64,
        ),
    >(&sql);
    for b in &binds {
        q_builder = q_builder.bind(b);
    }
    for b in &int_binds {
        q_builder = q_builder.bind(*b);
    }
    q_builder = q_builder.bind(limit);

    let rows = q_builder
        .fetch_all(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let hits: Vec<Value> = rows
        .into_iter()
        .map(
            |(
                hf_id,
                row_offset,
                source,
                doc_type,
                title,
                authority,
                number,
                year,
                date,
                text_length,
            )| {
                json!({
                    "hf_id": hf_id,
                    "row_offset": row_offset,
                    "source": source,
                    "doc_type": doc_type,
                    "title": title,
                    "authority": authority,
                    "number": number,
                    "year": year,
                    "date": date,
                    "text_length": text_length,
                })
            },
        )
        .collect();

    Ok(Json(json!({"hits": hits})))
}

/// Escape FTS5 special characters in user input by double-quoting.
/// Trailing/leading whitespace already trimmed by caller.
fn escape_fts5(q: &str) -> String {
    // Replace internal double-quotes (FTS5 escape) and wrap. Skip
    // wrap if user already used FTS5 boolean syntax with quotes —
    // they're presumably advanced.
    if q.contains('"') {
        q.replace('"', r#""""#)
    } else {
        format!("\"{}\"", q)
    }
}

// ---------------------------------------------------------------------------
// Fetch a row → cache text → index
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct FetchPayload {
    hf_id: String,
}

async fn fetch_row(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<FetchPayload>,
) -> ApiResult {
    // Pull the row's metadata + offset from our local FTS index.
    let row: Option<(
        i64,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<String>,
    )> =
        sqlx::query_as(
            "SELECT row_offset, source, doc_type, title, number, year, date \
             FROM italian_corpus WHERE hf_id = ?",
        )
        .bind(&body.hf_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let (row_offset, source, doc_type, title_metadata, _number, _year, corpus_date) = row
        .ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "ID non presente nell'indice locale. Esegui prima l'import.",
            )
        })?;

    // Dedupe: same hf_id already indexed for this user → return existing.
    let existing: Option<(String, String)> = sqlx::query_as(
        "SELECT id, filename FROM documents \
         WHERE user_id = ? AND corpus_id = ? AND corpus_identifier = ?",
    )
    .bind(&auth.user_id)
    .bind(CORPUS_ID)
    .bind(&body.hf_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    if let Some((id, filename)) = existing {
        if corpus_date.is_some() {
            let _ = sqlx::query(
                "UPDATE documents SET corpus_date = COALESCE(corpus_date, ?) \
                 WHERE id = ? AND user_id = ?",
            )
            .bind(&corpus_date)
            .bind(&id)
            .bind(&auth.user_id)
            .execute(&state.db)
            .await;
        }
        return Ok(Json(json!({
            "id": id,
            "filename": filename,
            "corpus_date": corpus_date,
            "already_indexed": true,
        })));
    }

    // Pull the full text from HuggingFace.
    let client = reqwest::Client::builder()
        .user_agent(crate::product::user_agent(Some("italian-legal fetch")))
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let (title_remote, text) =
        italian_legal::fetch_full_text(&client, row_offset)
            .await
            .map_err(|e| err(StatusCode::BAD_GATEWAY, &e.to_string()))?;

    let title = title_metadata
        .filter(|t| !t.is_empty())
        .unwrap_or(title_remote);

    // Hash the text so re-imports / cross-user dedupe share storage.
    let hash = {
        let mut h = Sha256::new();
        h.update(text.as_bytes());
        format!("{:x}", h.finalize())
    };
    let text_key = format!("cache/{}.txt", hash);

    let storage = make_storage()
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let text_abs =
        storage_root().join(text_key.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !text_abs.exists() {
        storage
            .put(&text_key, text.as_bytes(), "text/plain; charset=utf-8")
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    }

    let doc_id = uuid::Uuid::new_v4().to_string();
    let _ = doc_type;
    let filename = format!("{}.txt", title.chars().take(120).collect::<String>());
    let size = text.len() as i64;

    sqlx::query(
        "INSERT INTO documents \
           (id, user_id, project_id, filename, file_type, size_bytes, \
            storage_path, status, content_hash, extracted_text_path, \
            corpus_id, corpus_identifier, corpus_language, corpus_date, fetched_with_fallback) \
         VALUES (?, ?, NULL, ?, 'txt', ?, ?, 'syncing', ?, ?, ?, ?, 'it', ?, 0)",
    )
    .bind(&doc_id)
    .bind(&auth.user_id)
    .bind(&filename)
    .bind(size)
    .bind(&text_key)
    .bind(&hash)
    .bind(&text_key)
    .bind(CORPUS_ID)
    .bind(&body.hf_id)
    .bind(&corpus_date)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    // Run chunking + embedding synchronously, same lifecycle as
    // EUR-Lex (status: syncing → ready / interrupted on failure).
    #[cfg(feature = "rag")]
    let final_status = if let Some(emb) = state.embeddings.clone() {
        let source_path = format!(
            "https://huggingface.co/datasets/{}/{}",
            italian_legal::DATASET,
            body.hf_id
        );
        match emb
            .index_document(&auth.user_id, None, &doc_id, &source_path, &text)
            .await
        {
            Ok(n) => {
                tracing::info!(
                    "[italian-legal] indexed {} ({}) into {} chunk(s)",
                    body.hf_id,
                    source,
                    n
                );
                "ready"
            }
            Err(e) => {
                tracing::warn!(
                    "[italian-legal] embedding for {} failed: {}",
                    doc_id,
                    e
                );
                "interrupted"
            }
        }
    } else {
        "ready"
    };
    #[cfg(not(feature = "rag"))]
    let final_status = "ready";

    sqlx::query("UPDATE documents SET status = ? WHERE id = ?")
        .bind(final_status)
        .bind(&doc_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({
        "id": doc_id,
        "filename": filename,
        "corpus_id": CORPUS_ID,
        "corpus_identifier": body.hf_id,
        "corpus_date": corpus_date,
        "size_bytes": size,
        "status": final_status,
        "already_indexed": false,
    })))
}

// ---------------------------------------------------------------------------
// List + delete synced docs
// ---------------------------------------------------------------------------

async fn list_documents(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    let rows: Vec<(String, String, Option<String>, Option<String>, i64, String, String)> = sqlx::query_as(
        "SELECT id, filename, corpus_identifier, corpus_date, size_bytes, created_at, status \
         FROM documents \
         WHERE user_id = ? AND corpus_id = ? \
         ORDER BY created_at DESC",
    )
    .bind(&auth.user_id)
    .bind(CORPUS_ID)
    .fetch_all(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let docs: Vec<Value> = rows
        .into_iter()
        .map(|(id, filename, ident, date, size, created, status)| {
            json!({
                "id": id,
                "filename": filename,
                "corpus_identifier": ident,
                "corpus_date": date,
                "size_bytes": size,
                "created_at": created,
                "status": status,
            })
        })
        .collect();
    Ok(Json(json!({"documents": docs})))
}

async fn delete_document(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT storage_path, content_hash FROM documents \
         WHERE id = ? AND user_id = ? AND corpus_id = ?",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .bind(CORPUS_ID)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (storage_path, content_hash) = row.ok_or_else(|| {
        err(StatusCode::NOT_FOUND, "Documento italian-legal non trovato")
    })?;

    let _ = sqlx::query("DELETE FROM doc_chunks WHERE document_id = ?")
        .bind(&id)
        .execute(&state.db)
        .await;
    sqlx::query("DELETE FROM documents WHERE id = ? AND user_id = ?")
        .bind(&id)
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    // Ref-count check for the cache file (same as EUR-Lex).
    if let Some(hash) = content_hash {
        let still: Option<(i64,)> = sqlx::query_as(
            "SELECT 1 FROM documents WHERE content_hash = ? LIMIT 1",
        )
        .bind(&hash)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
        if still.is_none() {
            if let (Ok(storage), Some(key)) = (make_storage(), storage_path) {
                let _ = storage.delete(&key).await;
            }
        }
    }
    Ok(Json(json!({"ok": true, "id": id})))
}

// ---------------------------------------------------------------------------
// POST /italian-legal/documents/:id/resync
// ---------------------------------------------------------------------------
//
// Restart embedding for an interrupted/syncing doc. Reuses the cached
// text on disk if present (same hash policy as chat-cache); otherwise
// re-fetches from HuggingFace by row offset.

async fn resync_document(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let row: Option<(
        Option<String>,
        Option<String>,
        String,
        String,
    )> = sqlx::query_as(
        "SELECT extracted_text_path, corpus_identifier, status, filename \
         FROM documents \
         WHERE id = ? AND user_id = ? AND corpus_id = ?",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .bind(CORPUS_ID)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (text_key, hf_id, _prev_status, _filename) = row.ok_or_else(|| {
        err(StatusCode::NOT_FOUND, "Documento italian-legal non trovato")
    })?;
    let text_key = text_key.ok_or_else(|| {
        err(
            StatusCode::CONFLICT,
            "Documento senza cache testo: rifai il fetch.",
        )
    })?;
    let hf_id = hf_id.ok_or_else(|| {
        err(StatusCode::CONFLICT, "Documento senza hf_id")
    })?;

    // Mark syncing immediately so polling sees the right state.
    let _ = sqlx::query("UPDATE documents SET status = 'syncing' WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await;

    // Read text — from local cache if present, else re-fetch from HF.
    let text = if storage_root()
        .join(text_key.replace('/', std::path::MAIN_SEPARATOR_STR))
        .exists()
    {
        let storage = make_storage()
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
        let bytes = storage
            .get(&text_key)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        // Look up the row offset for this hf_id in our local index.
        let offset: Option<(i64,)> = sqlx::query_as(
            "SELECT row_offset FROM italian_corpus WHERE hf_id = ?",
        )
        .bind(&hf_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
        let offset = offset
            .ok_or_else(|| {
                err(
                    StatusCode::CONFLICT,
                    "hf_id non più presente nell'indice locale; ri-importa l'indice.",
                )
            })?
            .0;
        let client = reqwest::Client::builder()
            .user_agent(crate::product::user_agent(Some("italian-legal resync")))
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
        let (_title, text) = italian_legal::fetch_full_text(&client, offset)
            .await
            .map_err(|e| err(StatusCode::BAD_GATEWAY, &e.to_string()))?;
        // Re-cache the bytes so we don't need to refetch next time.
        let storage = make_storage()
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
        let _ = storage
            .put(&text_key, text.as_bytes(), "text/plain; charset=utf-8")
            .await;
        text
    };

    // Run indexing.
    #[cfg(feature = "rag")]
    let (chunks_indexed, indexing_error, final_status): (
        usize,
        Option<String>,
        &'static str,
    ) = if let Some(emb) = state.embeddings.clone() {
        let source_path = format!(
            "https://huggingface.co/datasets/{}/{}",
            italian_legal::DATASET,
            hf_id
        );
        match emb
            .index_document(&auth.user_id, None, &id, &source_path, &text)
            .await
        {
            Ok(n) => (n, None, "ready"),
            Err(e) => {
                tracing::warn!("[italian-legal] resync embed for {} failed: {}", id, e);
                (0, Some(e.to_string()), "interrupted")
            }
        }
    } else {
        (0, None, "ready")
    };
    #[cfg(not(feature = "rag"))]
    let (chunks_indexed, indexing_error, final_status): (
        usize,
        Option<String>,
        &'static str,
    ) = (0, None, "ready");

    sqlx::query("UPDATE documents SET status = ? WHERE id = ?")
        .bind(final_status)
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({
        "id": id,
        "status": final_status,
        "chunks_indexed": chunks_indexed,
        "indexing_error": indexing_error,
    })))
}
