//! `/sync` routes — folder configuration, scan trigger, status.
//!
//! All endpoints require an authenticated user. The vector store and
//! folder records are user-scoped: the same Cove Studio install can host
//! multiple users with separate Lance databases under
//! `<storage>/lance/<user_id>/`.

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::middleware::AuthUser, AppState};

#[cfg(feature = "rag")]
use crate::sync::scanner::{ScanProgress, ScanProgressHandle};

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn err(status: StatusCode, msg: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"detail": msg})))
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/folders", get(list_folders).post(add_folder))
        .route("/folders/{id}", delete(delete_folder))
        .route("/folders/{id}/scan", post(start_scan))
        .route("/folders/{id}/status", get(scan_status))
        .route("/folders/{id}/files", get(list_files))
        // Open a previously-indexed KB document. The DocPanel uses this
        // to fetch the bytes when the user clicks a [g1]/[p1] citation.
        .route("/kb-doc", get(get_kb_doc))
        // Live status of the embedding model — Idle / Downloading /
        // Loading / Ready / Failed. The frontend polls this during a
        // scan to render a progress bar for the one-shot model fetch
        // (~280 MB on first run).
        .route("/model-status", get(model_status))
        // Live status of the GLiNER2 PII engine — Idle / Loading /
        // Ready / Failed. Polled by the chat composer while a file is
        // flagged PII-protected so the user sees a "loading PII
        // model…" stripe instead of a silent multi-minute hang on
        // first use.
        .route("/ner-status", get(ner_status))
        // Purge orphan embeddings — `documents` rows whose
        // `storage_path` file no longer exists on disk, plus their
        // associated `doc_chunks` entries. Surfaced as a `Resync` /
        // `Pulisci` button when the chat-time retrieval reports
        // orphan KB chunks (v0.5.4+).
        .route("/cleanup-orphans", post(cleanup_orphans))
}

// ---------------------------------------------------------------------------
// GET /sync/folders
// ---------------------------------------------------------------------------
#[derive(Serialize)]
struct FolderOut {
    id: String,
    path: String,
    label: Option<String>,
    recursive: bool,
    enabled: bool,
    last_scan_at: Option<String>,
    /// `None` → folder belongs to the global pool, visible from any
    /// chat. `Some(id)` → folder belongs to a specific project.
    project_id: Option<String>,
}

async fn list_folders(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    let rows: Vec<(
        String, String, Option<String>, i64, i64, Option<String>, Option<String>,
    )> = sqlx::query_as(
        "SELECT id, path, label, recursive, enabled, last_scan_at, project_id \
         FROM sync_folders WHERE user_id = ? ORDER BY created_at DESC",
    )
    .bind(&auth.user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let out: Vec<FolderOut> = rows
        .into_iter()
        .map(
            |(id, path, label, recursive, enabled, last_scan_at, project_id)| FolderOut {
                id,
                path,
                label,
                recursive: recursive == 1,
                enabled: enabled == 1,
                last_scan_at,
                project_id,
            },
        )
        .collect();
    Ok(Json(serde_json::to_value(out).unwrap()))
}

// ---------------------------------------------------------------------------
// POST /sync/folders   { path, recursive?, label? }
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct AddFolderBody {
    path: String,
    #[serde(default = "default_true")]
    recursive: bool,
    label: Option<String>,
    /// `None` (or omitted) → global pool. `Some(id)` → bind to that
    /// project; only chats inside that project will see the chunks.
    project_id: Option<String>,
}
fn default_true() -> bool { true }

async fn add_folder(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<AddFolderBody>,
) -> ApiResult {
    let path = body.path.trim().to_string();
    if path.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "path cannot be empty"));
    }
    let pb = std::path::PathBuf::from(&path);
    if !pb.is_dir() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "path is not an existing directory",
        ));
    }

    // Validate project ownership when present so a user can't bind a
    // folder to someone else's project as a side-channel.
    if let Some(pid) = body.project_id.as_deref() {
        let owns: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM projects WHERE id = ? AND user_id = ?",
        )
        .bind(pid)
        .bind(&auth.user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
        if owns.is_none() {
            return Err(err(StatusCode::NOT_FOUND, "project not found"));
        }
    }

    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO sync_folders (id, user_id, path, recursive, enabled, label, project_id) \
         VALUES (?,?,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .bind(&path)
    .bind(if body.recursive { 1 } else { 0 })
    .bind(1_i64)
    .bind(body.label.as_deref())
    .bind(body.project_id.as_deref())
    .execute(&state.db)
    .await
    .map_err(|e| {
        let msg = e.to_string();
        // Friendlier error for the unique(user_id, path) violation.
        if msg.contains("UNIQUE") {
            err(StatusCode::CONFLICT, "folder already configured")
        } else {
            err(StatusCode::INTERNAL_SERVER_ERROR, &msg)
        }
    })?;
    Ok(Json(json!({ "id": id })))
}

// ---------------------------------------------------------------------------
// DELETE /sync/folders/:id
// Removes the folder and all its synced_files records (cascade) but
// does NOT delete chunks from the vector store — call /folders/:id/purge
// for that. The vector cleanup is opt-in so the user can disable a
// folder temporarily without losing the embeddings.
// ---------------------------------------------------------------------------
async fn delete_folder(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let res = sqlx::query("DELETE FROM sync_folders WHERE id = ? AND user_id = ?")
        .bind(&id)
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    if res.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "folder not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// POST /sync/folders/:id/scan
// Kicks off a scan in the background. Idempotent — re-running while
// one is in flight returns the existing progress handle.
// ---------------------------------------------------------------------------
#[cfg(feature = "rag")]
async fn start_scan(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let row: Option<(String, i64, Option<String>)> = sqlx::query_as(
        "SELECT path, recursive, project_id FROM sync_folders \
         WHERE id = ? AND user_id = ?",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let (path, recursive, project_id) =
        row.ok_or_else(|| err(StatusCode::NOT_FOUND, "folder not found"))?;

    let embeddings = state
        .embeddings
        .as_ref()
        .ok_or_else(|| {
            err(
                StatusCode::SERVICE_UNAVAILABLE,
                "RAG not initialised — STORAGE_PATH missing or feature off",
            )
        })?
        .clone();

    let mut scans = state.scans.write().await;
    if let Some(existing) = scans.get(&id) {
        let cur = existing.read().await.clone();
        if matches!(cur.status, crate::sync::ScanStatus::Running) {
            return Ok(Json(json!({ "already_running": true })));
        }
    }
    let progress: ScanProgressHandle =
        Arc::new(tokio::sync::RwLock::new(ScanProgress::default()));
    scans.insert(id.clone(), progress.clone());
    drop(scans);

    let db = state.db.clone();
    let user_id = auth.user_id.clone();
    let folder_id = id.clone();
    let folder_path = std::path::PathBuf::from(path);
    let prog = progress.clone();
    let proj = project_id.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::sync::scan_folder(
            db,
            embeddings,
            user_id,
            folder_id,
            proj,
            folder_path,
            recursive == 1,
            prog.clone(),
        )
        .await
        {
            tracing::error!("[sync] scan failed: {e}");
            let mut p = prog.write().await;
            p.status = crate::sync::ScanStatus::Failed;
            p.last_error = Some(e.to_string());
        }
    });

    Ok(Json(json!({ "started": true })))
}

#[cfg(not(feature = "rag"))]
async fn start_scan(
    State(_): State<Arc<AppState>>,
    _: AuthUser,
    Path(_): Path<String>,
) -> ApiResult {
    Err(err(
        StatusCode::SERVICE_UNAVAILABLE,
        "RAG feature not compiled in this build",
    ))
}

// ---------------------------------------------------------------------------
// GET /sync/folders/:id/status
// ---------------------------------------------------------------------------
#[cfg(feature = "rag")]
async fn scan_status(
    State(state): State<Arc<AppState>>,
    _: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let scans = state.scans.read().await;
    let Some(handle) = scans.get(&id) else {
        return Ok(Json(json!({ "status": "idle" })));
    };
    let p = handle.read().await;
    Ok(Json(json!({
        "status": match p.status {
            crate::sync::ScanStatus::Idle    => "idle",
            crate::sync::ScanStatus::Running => "running",
            crate::sync::ScanStatus::Done    => "done",
            crate::sync::ScanStatus::Failed  => "failed",
        },
        "total":         p.total,
        "processed":     p.processed,
        "indexed":       p.indexed,
        "skipped":       p.skipped,
        "failed":        p.failed,
        "current_file":  p.current_file,
        "current_step":  p.current_step,
        "last_error":    p.last_error,
    })))
}

// ---------------------------------------------------------------------------
// GET /sync/model-status
// Frontend renders a progress bar based on the returned snapshot.
// ---------------------------------------------------------------------------
#[cfg(feature = "rag")]
async fn model_status(
    State(state): State<Arc<AppState>>,
    _: AuthUser,
) -> ApiResult {
    let Some(svc) = state.embeddings.as_ref() else {
        return Ok(Json(json!({ "state": "unavailable" })));
    };
    use crate::embeddings::service::ModelStatus;
    Ok(Json(match svc.status().await {
        ModelStatus::Idle => json!({ "state": "idle" }),
        ModelStatus::Downloading { downloaded, total, file } => json!({
            "state": "downloading",
            "downloaded": downloaded,
            "total": total,
            "file": file,
        }),
        ModelStatus::Loading => json!({ "state": "loading" }),
        ModelStatus::Ready => json!({ "state": "ready" }),
        ModelStatus::Failed(msg) => json!({ "state": "failed", "error": msg }),
    }))
}

#[cfg(not(feature = "rag"))]
async fn model_status(
    State(_): State<Arc<AppState>>,
    _: AuthUser,
) -> ApiResult {
    Ok(Json(json!({ "state": "unavailable" })))
}

// ---------------------------------------------------------------------------
// GET /sync/ner-status
// Mirrors /sync/model-status for the GLiNER2 PII engine bootstrap.
// `unavailable` when the `ner-pii` feature isn't compiled in — the
// frontend can use the same polling loop in every build.
// ---------------------------------------------------------------------------
#[cfg(feature = "ner-pii")]
async fn ner_status(
    State(_): State<Arc<AppState>>,
    _: AuthUser,
) -> ApiResult {
    use crate::ner::NerStatus;
    Ok(Json(match crate::ner::status().await {
        NerStatus::Idle => json!({ "state": "idle" }),
        NerStatus::Downloading {
            downloaded,
            total,
            file,
        } => json!({
            "state": "downloading",
            "downloaded": downloaded,
            "total": total,
            "file": file,
        }),
        NerStatus::Loading => json!({ "state": "loading" }),
        NerStatus::Ready => json!({ "state": "ready" }),
        NerStatus::Failed { error } => json!({
            "state": "failed",
            "error": error,
        }),
    }))
}

#[cfg(not(feature = "ner-pii"))]
async fn ner_status(
    State(_): State<Arc<AppState>>,
    _: AuthUser,
) -> ApiResult {
    Ok(Json(json!({ "state": "unavailable" })))
}

#[cfg(not(feature = "rag"))]
async fn scan_status(
    State(_): State<Arc<AppState>>,
    _: AuthUser,
    Path(_): Path<String>,
) -> ApiResult {
    Ok(Json(json!({ "status": "idle" })))
}

// ---------------------------------------------------------------------------
// GET /sync/kb-doc?path=...
// Stream the bytes of a previously-indexed KB document so the
// frontend's DocPanel can display it after a citation click.
//
// Security: we only serve files that exist in `synced_files` for the
// authenticated user — the path is validated against the indexed set,
// not used as a free filesystem reference. This prevents path
// traversal even if the path query parameter contains `..` or
// references outside any sync folder.
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct KbDocQuery {
    path: String,
}

async fn get_kb_doc(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<KbDocQuery>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    tracing::info!("[kb-doc] requested path={:?} user={}", q.path, auth.user_id);
    // Allowlist: the path must either be a currently-indexed KB file
    // (synced_files) for this user, OR resolve to a corpus document
    // (documents.storage_path) the user has fetched. Corpus docs aren't
    // in synced_files — they live in `documents` keyed by user_id and
    // have a relative storage_path like `cache/<hash>.txt`. We resolve
    // that to its absolute on-disk path and compare against the
    // requested path so the same kb-doc endpoint serves both.
    let in_synced_files: bool = sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM synced_files WHERE user_id = ? AND path = ? LIMIT 1",
    )
    .bind(&auth.user_id)
    .bind(&q.path)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?
    .is_some();

    let in_corpus: bool = if !in_synced_files {
        let storage_root = std::path::PathBuf::from(
            std::env::var("STORAGE_PATH")
                .unwrap_or_else(|_| "./data/storage".to_string()),
        );
        let rows: Vec<(Option<String>,)> = sqlx::query_as(
            "SELECT storage_path FROM documents \
             WHERE user_id = ? AND corpus_id IS NOT NULL AND storage_path IS NOT NULL",
        )
        .bind(&auth.user_id)
        .fetch_all(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
        rows.into_iter().any(|(sp,)| {
            let sp = sp.unwrap_or_default();
            let abs = storage_root
                .join(sp.replace('/', std::path::MAIN_SEPARATOR_STR))
                .to_string_lossy()
                .to_string();
            abs == q.path
        })
    } else {
        false
    };

    if !in_synced_files && !in_corpus {
        // Diagnose the mismatch: dump every indexed path for this user
        // so we can compare against `q.path` byte-by-byte (case, slashes,
        // trailing whitespace, NFC vs NFD on macOS, etc.).
        let all: Vec<(String,)> =
            sqlx::query_as("SELECT path FROM synced_files WHERE user_id = ? LIMIT 20")
                .bind(&auth.user_id)
                .fetch_all(&state.db)
                .await
                .unwrap_or_default();
        tracing::warn!(
            "[kb-doc] path NOT in synced_files NOR documents (corpus) for user. \
             Requested:\n  {:?}\nIndexed paths ({}):\n{}",
            q.path,
            all.len(),
            all.iter()
                .map(|(p,)| format!("  {p:?}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        return Err(err(
            StatusCode::NOT_FOUND,
            "document not found in your sync index",
        ));
    }

    let bytes = std::fs::read(&q.path).map_err(|e| {
        tracing::warn!("[kb-doc] fs::read failed for {:?}: {e}", q.path);
        err(StatusCode::NOT_FOUND, &format!("read failed: {e}"))
    })?;
    tracing::info!("[kb-doc] served {} bytes from {:?}", bytes.len(), q.path);

    let ext = std::path::Path::new(&q.path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mime = match ext.as_str() {
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "rtf" => "application/rtf",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "txt" | "md" | "csv" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    };
    let basename = std::path::Path::new(&q.path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "document".to_string());

    Ok((
        [
            (header::CONTENT_TYPE, mime.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("inline; filename=\"{basename}\""),
            ),
        ],
        bytes,
    )
        .into_response())
}

// ---------------------------------------------------------------------------
// GET /sync/folders/:id/files
// Paginated listing of indexed files. The UI uses this to show the
// "skipped" list with reasons so the user understands why a scanned PDF
// wasn't picked up.
// ---------------------------------------------------------------------------
async fn list_files(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let rows: Vec<(
        String, String, String, Option<String>, i64, i64, String, Option<String>,
    )> = sqlx::query_as(
        "SELECT path, status, document_id, skip_reason, size_bytes, chunk_count, \
                indexed_at, mtime \
         FROM synced_files \
         WHERE user_id = ? AND folder_id = ? \
         ORDER BY path",
    )
    .bind(&auth.user_id)
    .bind(&id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let out: Vec<Value> = rows
        .into_iter()
        .map(
            |(path, status, doc, reason, size, chunks, indexed_at, mtime)| {
                json!({
                    "path":         path,
                    "status":       status,
                    "document_id":  doc,
                    "skip_reason":  reason,
                    "size_bytes":   size,
                    "chunk_count":  chunks,
                    "indexed_at":   indexed_at,
                    "mtime":        mtime,
                })
            },
        )
        .collect();
    Ok(Json(json!(out)))
}

// ---------------------------------------------------------------------------
// POST /sync/cleanup-orphans
// ---------------------------------------------------------------------------
/// Delete every `documents` row whose `storage_path` file no longer
/// exists on disk, plus the matching `doc_chunks` entries (cascade
/// via the FK declared in migration 0013) and `synced_files` rows
/// referencing the deleted document id.
///
/// Returns a summary `{ scanned, orphans, deleted_docs, deleted_chunks }`.
/// Scoped to the calling user — orphans owned by other users on the
/// same install are not touched.
///
/// Triggered by the chat-time orphan-KB-chunk diagnostic warning in
/// `retrieve_kb_chunks` (v0.5.4+) and by the frontend "Pulisci sorgenti
/// rimosse" button in the file-missing modal.
async fn cleanup_orphans(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    // Pull every document this user owns that carries a storage_path
    // (URL-only corpus rows have storage_path = NULL and are skipped
    // — they're tracked via `corpus_id` + `corpus_identifier`).
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, storage_path FROM documents \
         WHERE user_id = ? AND storage_path IS NOT NULL",
    )
    .bind(&auth.user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let scanned = rows.len();
    let mut orphan_ids: Vec<String> = Vec::new();
    let storage_base =
        std::path::PathBuf::from(std::env::var("STORAGE_PATH").unwrap_or_else(|_| {
            crate::storage::default_storage_path()
        }));
    for (doc_id, storage_path) in rows {
        // storage_path is the *key* relative to STORAGE_PATH (e.g.
        // `cache/<hash>.pdf`, or `documents/<user>/<doc_id>`). Some
        // legacy rows carry an absolute filesystem path (synced
        // files on the user's machine — `C:\Users\…\file.pdf`).
        // Resolve both shapes before probing.
        let p = std::path::Path::new(&storage_path);
        let abs = if p.is_absolute() {
            p.to_path_buf()
        } else {
            storage_base.join(&storage_path)
        };
        if !abs.exists() {
            orphan_ids.push(doc_id);
        }
    }

    if orphan_ids.is_empty() {
        tracing::info!(
            "[sync] cleanup-orphans for user={}: scanned {scanned}, 0 orphans found",
            auth.user_id
        );
        return Ok(Json(json!({
            "scanned": scanned,
            "orphans": 0,
            "deleted_docs": 0,
            "deleted_chunks": 0,
        })));
    }

    // Build the IN clause for the cascade delete. SQLite caps each
    // statement at 999 parameters; chunk if we ever cross that. For
    // realistic user libraries (<1000 docs) one pass is enough.
    let mut deleted_chunks: u64 = 0;
    let mut deleted_docs: u64 = 0;
    let mut deleted_synced: u64 = 0;
    for chunk in orphan_ids.chunks(900) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");

        // 1. doc_chunks (vector index)
        let q = format!("DELETE FROM doc_chunks WHERE document_id IN ({placeholders})");
        let mut query = sqlx::query(&q);
        for id in chunk {
            query = query.bind(id);
        }
        deleted_chunks += query
            .execute(&state.db)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?
            .rows_affected();

        // 2. synced_files (folder-sync tracking)
        let q = format!("DELETE FROM synced_files WHERE document_id IN ({placeholders})");
        let mut query = sqlx::query(&q);
        for id in chunk {
            query = query.bind(id);
        }
        deleted_synced += query
            .execute(&state.db)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?
            .rows_affected();

        // 3. documents (the canonical row). Ownership re-checked here
        // even though we already filtered by user_id above — defence
        // in depth against any race where another path reassigned a
        // doc to a different user between SELECT and DELETE.
        let q = format!(
            "DELETE FROM documents WHERE id IN ({placeholders}) AND user_id = ?"
        );
        let mut query = sqlx::query(&q);
        for id in chunk {
            query = query.bind(id);
        }
        query = query.bind(&auth.user_id);
        deleted_docs += query
            .execute(&state.db)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?
            .rows_affected();
    }

    tracing::info!(
        "[sync] cleanup-orphans for user={}: scanned {scanned}, \
         {} orphans found, deleted {deleted_docs} document rows, \
         {deleted_chunks} doc_chunks rows, {deleted_synced} synced_files rows",
        auth.user_id,
        orphan_ids.len(),
    );

    Ok(Json(json!({
        "scanned": scanned,
        "orphans": orphan_ids.len(),
        "deleted_docs": deleted_docs,
        "deleted_chunks": deleted_chunks,
        "deleted_synced": deleted_synced,
    })))
}
