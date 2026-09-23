use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{sse::Event, IntoResponse, Response, Sse},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc};
use tokio_stream::wrappers::ReceiverStream;

use crate::{
    auth::middleware::AuthUser,
    llm::{self, Message, StreamParams},
    routes::chat::build_local_config,
    routes::user::{fetch_llm_settings, LlmSettings},
    storage::make_storage,
    AppState,
};

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn err(status: StatusCode, msg: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"detail": msg})))
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list_tabular_reviews).post(create_tabular_review))
        .route("/import", post(import_tabular_review))
        .route(
            "/{id}",
            get(get_tabular_review)
                .patch(patch_tabular_review)
                .delete(delete_tabular_review),
        )
        .route("/{id}/generate", post(generate_cells))
        .route("/{id}/regenerate-cell", post(regenerate_cell))
        .route("/{id}/clear-cells", post(clear_cells))
        .route("/{id}/export", get(export_tabular_review))
}

// ---------------------------------------------------------------------------
// GET /tabular-review?project_id=...
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct ListQuery {
    project_id: Option<String>,
    /// Optional `?domain=legal|medical|…` filter added with migration 0018.
    domain: Option<String>,
}

type TabularReviewRow = (
    String,         // id
    String,         // title
    Option<String>, // project_id
    Option<String>, // workflow_id
    String,         // columns_config (JSON)
    String,         // created_at
    String,         // updated_at
    String,         // domain (0018, default 'legal')
);

async fn list_tabular_reviews(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<ListQuery>,
) -> ApiResult {
    let domain_filter = q
        .domain
        .as_deref()
        .filter(|s| !s.is_empty() && crate::domain::is_valid(s));

    let mut sql = String::from(
        "SELECT id, title, project_id, workflow_id, columns_config, created_at, updated_at, domain \
         FROM tabular_reviews WHERE user_id = ?",
    );
    if q.project_id.is_some() {
        sql.push_str(" AND project_id = ?");
    }
    if domain_filter.is_some() {
        sql.push_str(" AND domain = ?");
    }
    sql.push_str(" ORDER BY updated_at DESC");

    let mut query = sqlx::query_as::<_, TabularReviewRow>(&sql).bind(&auth.user_id);
    if let Some(ref pid) = q.project_id {
        query = query.bind(pid);
    }
    if let Some(d) = domain_filter {
        query = query.bind(d);
    }
    let rows: Vec<TabularReviewRow> = query
        .fetch_all(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let reviews: Vec<Value> = rows
        .into_iter()
        .map(|(id, title, project_id, workflow_id, columns_config, created_at, updated_at, domain)| {
            json!({
                "id": id,
                "title": title,
                "project_id": project_id,
                "workflow_id": workflow_id,
                "columns_config": serde_json::from_str::<Value>(&columns_config).unwrap_or(json!([])),
                "domain": domain,
                "created_at": created_at,
                "updated_at": updated_at
            })
        })
        .collect();

    Ok(Json(json!(reviews)))
}

// ---------------------------------------------------------------------------
// POST /tabular-review
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct CreateTabularReviewBody {
    title: Option<String>,
    project_id: Option<String>,
    workflow_id: Option<String>,
    columns_config: Option<Value>,
    domain: Option<String>,
}

async fn create_tabular_review(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<CreateTabularReviewBody>,
) -> ApiResult {
    let id = uuid::Uuid::new_v4().to_string();
    let title = body.title.unwrap_or_else(|| "Untitled Review".to_string());
    let columns_config = body.columns_config.unwrap_or(json!([])).to_string();
    let dom = crate::domain::normalise_or_default(body.domain.as_deref());

    sqlx::query(
        "INSERT INTO tabular_reviews (id, user_id, project_id, workflow_id, title, columns_config, domain) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .bind(&body.project_id)
    .bind(&body.workflow_id)
    .bind(&title)
    .bind(&columns_config)
    .bind(dom)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({ "id": id, "title": title, "domain": dom })))
}

// ---------------------------------------------------------------------------
// POST /tabular-review/import — create reviews from an uploaded spreadsheet
// ---------------------------------------------------------------------------
//
// One review per worksheet (the user confirmed this mapping): the first
// row becomes the column headers, every following row becomes a
// document-less data row with its cells pre-filled (status = "done").
// The whole conversion is local + deterministic via calamine — no LLM.
async fn import_tabular_review(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    body: axum::body::Bytes,
) -> ApiResult {
    use calamine::Reader;
    use std::io::Cursor;

    if body.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "empty file"));
    }
    let mut workbook =
        calamine::open_workbook_auto_from_rs(Cursor::new(body.to_vec()))
            .map_err(|e| {
                err(
                    StatusCode::BAD_REQUEST,
                    &format!("not a valid spreadsheet: {e}"),
                )
            })?;

    let dom = crate::domain::normalise_or_default(None);
    let mut created: Vec<Value> = Vec::new();

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    for sheet_name in workbook.sheet_names() {
        let Ok(range) = workbook.worksheet_range(&sheet_name) else {
            continue;
        };
        let mut rows_iter = range.rows();
        // First row = headers. Skip empty sheets.
        let Some(header) = rows_iter.next() else { continue };
        if header.is_empty() {
            continue;
        }

        // Header cells, left to right → review columns.
        let columns: Vec<Value> = header
            .iter()
            .enumerate()
            .map(|(i, cell)| {
                let raw = cell.to_string();
                let name = if raw.trim().is_empty() {
                    format!("Column {}", i + 1)
                } else {
                    raw.trim().to_string()
                };
                json!({
                    "key": format!("col_{}", i + 1),
                    "name": name,
                    "prompt": "",
                    "format": "free_text",
                })
            })
            .collect();
        let n_cols = columns.len();
        let columns_json =
            serde_json::to_string(&columns).unwrap_or_else(|_| "[]".into());

        let review_id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO tabular_reviews \
               (id, user_id, title, columns_config, domain, status) \
             VALUES (?, ?, ?, ?, ?, 'done')",
        )
        .bind(&review_id)
        .bind(&auth.user_id)
        .bind(&sheet_name)
        .bind(&columns_json)
        .bind(dom)
        .execute(&mut *tx)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

        // Data rows → document-less rows with pre-filled cells.
        for (ri, row) in rows_iter.enumerate() {
            let cells: Vec<Value> = (0..n_cols)
                .map(|ci| {
                    let content = row
                        .get(ci)
                        .map(|c| c.to_string())
                        .unwrap_or_default();
                    json!({
                        "key": format!("col_{}", ci + 1),
                        "status": "done",
                        "content": content,
                    })
                })
                .collect();
            sqlx::query(
                "INSERT INTO tabular_review_rows \
                   (id, tabular_review_id, document_id, row_index, cells, status) \
                 VALUES (?, ?, NULL, ?, ?, 'done')",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&review_id)
            .bind(ri as i64)
            .bind(
                serde_json::to_string(&cells)
                    .unwrap_or_else(|_| "[]".into()),
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
        }
        created.push(json!({ "id": review_id, "title": sheet_name }));
    }

    if created.is_empty() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "no non-empty worksheets found in the workbook",
        ));
    }
    tx.commit()
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({ "reviews": created })))
}

// ---------------------------------------------------------------------------
// Shared: load a review (verifying ownership) + its rows.
// ---------------------------------------------------------------------------

/// One document row of a review, with its per-column cells.
struct ReviewRow {
    id: String,
    document_id: Option<String>,
    filename: Option<String>,
    status: String,
    cells: Vec<Value>,
}

async fn load_review_meta(
    state: &AppState,
    user_id: &str,
    id: &str,
) -> Result<TabularReviewRow, (StatusCode, Json<Value>)> {
    sqlx::query_as::<_, TabularReviewRow>(
        "SELECT id, title, project_id, workflow_id, columns_config, created_at, updated_at, domain \
         FROM tabular_reviews WHERE id = ? AND user_id = ?",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "Tabular review not found"))
}

async fn load_review_rows(
    state: &AppState,
    review_id: &str,
) -> Result<Vec<ReviewRow>, (StatusCode, Json<Value>)> {
    let rows: Vec<(String, Option<String>, Option<String>, String, String)> = sqlx::query_as(
        "SELECT r.id, r.document_id, d.filename, r.status, r.cells \
         FROM tabular_review_rows r \
         LEFT JOIN documents d ON d.id = r.document_id \
         WHERE r.tabular_review_id = ? \
         ORDER BY r.row_index, r.created_at",
    )
    .bind(review_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(rows
        .into_iter()
        .map(|(id, document_id, filename, status, cells)| ReviewRow {
            id,
            document_id,
            filename,
            status,
            cells: serde_json::from_str::<Vec<Value>>(&cells).unwrap_or_default(),
        })
        .collect())
}

fn row_json(r: &ReviewRow) -> Value {
    json!({
        "id": r.id,
        "document_id": r.document_id,
        "filename": r.filename,
        "status": r.status,
        "cells": r.cells,
    })
}

// ---------------------------------------------------------------------------
// GET /tabular-review/:id/export — download the grid as an .xlsx
// ---------------------------------------------------------------------------
async fn export_tabular_review(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    use rust_xlsxwriter::{Format, Workbook};

    // TabularReviewRow is a tuple: (id, title, project_id, workflow_id,
    // columns_config, created_at, updated_at, domain).
    let meta = load_review_meta(&state, &auth.user_id, &id).await?;
    let cols = parse_columns(&meta.4);
    let rows = load_review_rows(&state, &id).await?;

    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    let header = Format::new().set_bold();
    let xerr = |e: rust_xlsxwriter::XlsxError| {
        err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
    };

    // Header: first column is the document name, then one per review column.
    sheet
        .write_with_format(0, 0, "Document", &header)
        .map_err(xerr)?;
    for (i, (_, label, _, _)) in cols.iter().enumerate() {
        sheet
            .write_with_format(0, (i + 1) as u16, label.as_str(), &header)
            .map_err(xerr)?;
    }

    for (r, row) in rows.iter().enumerate() {
        let rr = (r + 1) as u32;
        sheet
            .write(rr, 0, row.filename.as_deref().unwrap_or(""))
            .map_err(xerr)?;
        for (i, (key, _, _, _)) in cols.iter().enumerate() {
            let content = row
                .cells
                .iter()
                .find(|c| {
                    c.get("key").and_then(|v| v.as_str()) == Some(key.as_str())
                })
                .and_then(|c| c.get("content").and_then(|v| v.as_str()))
                .unwrap_or("");
            sheet.write(rr, (i + 1) as u16, content).map_err(xerr)?;
        }
    }

    let bytes = workbook.save_to_buffer().map_err(xerr)?;
    let safe_title: String = meta
        .1
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let filename = format!("{}.xlsx", safe_title.trim());

    Ok((
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                    .to_string(),
            ),
            (
                axum::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        bytes,
    )
        .into_response())
}

// ---------------------------------------------------------------------------
// GET /tabular-review/:id  — review metadata + document rows with cells
// ---------------------------------------------------------------------------
async fn get_tabular_review(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let (id, title, project_id, workflow_id, columns_config, created_at, updated_at, domain) =
        load_review_meta(&state, &auth.user_id, &id).await?;
    let rows = load_review_rows(&state, &id).await?;

    Ok(Json(json!({
        "id": id,
        "title": title,
        "project_id": project_id,
        "workflow_id": workflow_id,
        "columns_config": serde_json::from_str::<Value>(&columns_config).unwrap_or(json!([])),
        "domain": domain,
        "created_at": created_at,
        "updated_at": updated_at,
        "rows": rows.iter().map(row_json).collect::<Vec<_>>(),
    })))
}

// ---------------------------------------------------------------------------
// PATCH /tabular-review/:id  — update title / columns / attached documents
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct PatchBody {
    title: Option<String>,
    columns_config: Option<Value>,
    /// When present, reconciles the review's document rows to exactly
    /// this set: rows for new ids are created, rows for dropped ids
    /// are deleted (cascading their cells).
    document_ids: Option<Vec<String>>,
}

async fn patch_tabular_review(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<PatchBody>,
) -> ApiResult {
    load_review_meta(&state, &auth.user_id, &id).await?;

    if let Some(title) = body.title.as_deref() {
        sqlx::query("UPDATE tabular_reviews SET title = ? WHERE id = ?")
            .bind(title)
            .bind(&id)
            .execute(&state.db)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    }

    if let Some(cols) = body.columns_config.as_ref() {
        sqlx::query("UPDATE tabular_reviews SET columns_config = ? WHERE id = ?")
            .bind(cols.to_string())
            .bind(&id)
            .execute(&state.db)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    }

    if let Some(doc_ids) = body.document_ids.as_ref() {
        let existing: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT id, document_id FROM tabular_review_rows WHERE tabular_review_id = ?",
        )
        .bind(&id)
        .fetch_all(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

        // Delete rows whose document is no longer in the set.
        for (row_id, doc) in &existing {
            let keep = doc.as_ref().map(|d| doc_ids.contains(d)).unwrap_or(false);
            if !keep {
                let _ = sqlx::query("DELETE FROM tabular_review_rows WHERE id = ?")
                    .bind(row_id)
                    .execute(&state.db)
                    .await;
            }
        }
        // Insert rows for documents not yet present.
        let present: Vec<String> = existing.iter().filter_map(|(_, d)| d.clone()).collect();
        for (idx, doc_id) in doc_ids.iter().enumerate() {
            if present.contains(doc_id) {
                continue;
            }
            let row_id = uuid::Uuid::new_v4().to_string();

            // Dedup against earlier rows in THIS review: if the user
            // re-uploads a file that is byte-identical and same-named
            // to one that has already been extracted in this review,
            // inherit the extracted cells instead of asking the LLM to
            // do the same work twice. The match key is (filename,
            // content_hash) per the 2026-06-06 UX policy; rows whose
            // status is still 'pending' (no extraction attempted) are
            // ignored — there is nothing to copy from those yet.
            let dedup: Option<(String, String)> = sqlx::query_as(
                "SELECT trr.cells, trr.status \
                 FROM tabular_review_rows trr \
                 JOIN documents existing_doc ON existing_doc.id = trr.document_id \
                 JOIN documents new_doc ON new_doc.id = ? \
                 WHERE trr.tabular_review_id = ? \
                   AND existing_doc.filename = new_doc.filename \
                   AND existing_doc.content_hash = new_doc.content_hash \
                   AND new_doc.content_hash IS NOT NULL \
                   AND trr.status != 'pending' \
                 ORDER BY trr.row_index ASC \
                 LIMIT 1",
            )
            .bind(doc_id)
            .bind(&id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();

            let (cells, row_status): (String, String) = match dedup {
                Some((cells, status)) => {
                    tracing::info!(
                        "[tabular][dedup] review={} new_doc={} inherits cells (status={}) from earlier row with matching (filename, content_hash)",
                        id,
                        doc_id,
                        status
                    );
                    (cells, status)
                }
                None => ("[]".to_string(), "pending".to_string()),
            };

            let _ = sqlx::query(
                "INSERT INTO tabular_review_rows (id, tabular_review_id, document_id, row_index, cells, status) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&row_id)
            .bind(&id)
            .bind(doc_id)
            .bind(idx as i64)
            .bind(&cells)
            .bind(&row_status)
            .execute(&state.db)
            .await;
        }
    }

    sqlx::query("UPDATE tabular_reviews SET updated_at = datetime('now') WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    get_tabular_review(State(state), auth, Path(id)).await
}

// ---------------------------------------------------------------------------
// DELETE /tabular-review/:id
// ---------------------------------------------------------------------------
async fn delete_tabular_review(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let result = sqlx::query("DELETE FROM tabular_reviews WHERE id = ? AND user_id = ?")
        .bind(&id)
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    if result.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "Tabular review not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Cell extraction helpers
// ---------------------------------------------------------------------------

/// Load a document's plain text (extracted-text sidecar, else extracted
/// from the binary on the fly). Returns `None` when unavailable.
async fn load_document_text(state: &AppState, user_id: &str, doc_id: &str) -> Option<String> {
    let row: Option<(Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT storage_path, extracted_text_path, file_type FROM documents \
         WHERE id = ? AND user_id = ?",
    )
    .bind(doc_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    let Some((storage_path, text_path, file_type)) = row else {
        tracing::warn!(
            "[tabular][doc-text] doc_id={} user={} — row not found",
            doc_id, user_id
        );
        return None;
    };
    let storage = make_storage().ok()?;

    if let Some(key) = text_path.as_ref() {
        match storage.get(key).await {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                let len = text.trim().len();
                tracing::info!(
                    "[tabular][doc-text] doc_id={} sidecar='{}' bytes={} non_ws_chars={}",
                    doc_id, key, bytes.len(), len
                );
                if len > 0 {
                    return Some(text);
                }
            }
            Err(e) => {
                tracing::warn!(
                    "[tabular][doc-text] doc_id={} sidecar='{}' read FAILED: {}",
                    doc_id, key, e
                );
            }
        }
    } else {
        tracing::info!(
            "[tabular][doc-text] doc_id={} no extracted_text_path sidecar — falling back to binary",
            doc_id
        );
    }

    // Fall back to extracting from the binary. The dispatcher dispatches
    // on the path EXTENSION (and for PDFs re-opens the file via pdfium,
    // which takes a path not bytes), so the legacy `cache=false` upload
    // layout — `documents/<uid>/<docid>` with NO extension — defeats
    // both halves of that contract: pdfium gets a path it can't open
    // (relative + no extension) AND the dispatcher falls into the
    // catch-all branch that returns "format not supported".
    //
    // To make on-the-fly extraction work for those rows, we:
    //   1. Look up the doc's `file_type` from the documents row above.
    //   2. Resolve the storage key to an absolute path via STORAGE_PATH
    //      / default_storage_path.
    //   3. Append `.<file_type>` so the dispatcher's extension-based
    //      match hits the right branch and pdfium's open-by-path
    //      succeeds.
    //
    // The cache=true path remains unaffected — it persists the sidecar
    // at upload time, so we short-circuit above before reaching here.
    let Some(key) = storage_path else {
        tracing::warn!(
            "[tabular][doc-text] doc_id={} — storage_path is NULL",
            doc_id
        );
        return None;
    };
    let bytes = match storage.get(&key).await {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(
                "[tabular][doc-text] doc_id={} key='{}' storage.get FAILED: {}",
                doc_id, key, e
            );
            return None;
        }
    };

    let base_abs = std::env::var("STORAGE_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(crate::storage::default_storage_path()));
    let mut abs_path = base_abs.join(key.replace('/', std::path::MAIN_SEPARATOR_STR.to_string().as_str()));

    // The legacy upload key `documents/<uid>/<docid>` lacks an
    // extension. If the on-disk key has no extension yet but the DB
    // tells us this is e.g. a pdf, append `.pdf` so the dispatcher
    // picks the right branch. pdfium then opens this exact path —
    // which is also where the bytes physically live on disk, since
    // LocalStorage::put stored them at `<base>/<key>` with no
    // extension. To avoid pdfium reading a non-existent
    // `<base>/<key>.pdf`, we materialise a sibling symlink/copy at
    // the extension-suffixed path when needed.
    let needs_ext = abs_path.extension().is_none();
    if needs_ext {
        if let Some(ft) = file_type.as_deref() {
            let suffixed = abs_path.with_extension(ft);
            if !suffixed.exists() {
                if let Err(e) = std::fs::copy(&abs_path, &suffixed) {
                    tracing::warn!(
                        "[tabular][doc-text] doc_id={} could not materialise sibling \
                         {} for extension dispatch: {}",
                        doc_id,
                        suffixed.display(),
                        e
                    );
                }
            }
            abs_path = suffixed;
        }
    }

    tracing::info!(
        "[tabular][doc-text] doc_id={} dispatching extract_text_dispatch(path='{}', bytes={}, file_type={:?})",
        doc_id,
        abs_path.display(),
        bytes.len(),
        file_type
    );

    match crate::sync::scanner::extract_text_dispatch(&abs_path, &bytes) {
        Ok((text, skip)) => {
            tracing::info!(
                "[tabular][doc-text] doc_id={} extracted chars={} skip_reason={:?}",
                doc_id,
                text.len(),
                skip
            );
            if text.trim().is_empty() {
                None
            } else {
                Some(text)
            }
        }
        Err(e) => {
            tracing::warn!(
                "[tabular][doc-text] doc_id={} extract_text_dispatch ERROR: {}",
                doc_id, e
            );
            None
        }
    }
}

/// Pick the model used for tabular extraction: the user's `tabular_model`,
/// falling back to `main_model`. Returns `None` when neither is set.
fn pick_tabular_model(s: &LlmSettings) -> Option<String> {
    s.tabular_model
        .clone()
        .filter(|m| !m.trim().is_empty())
        .or_else(|| s.main_model.clone().filter(|m| !m.trim().is_empty()))
}

const DOC_TEXT_LIMIT: usize = 60_000;

/// Run a single extraction: returns the model's answer or an error string.
async fn extract_cell(
    model: &str,
    settings: &LlmSettings,
    column_label: &str,
    column_prompt: &str,
    column_format: &str,
    doc_text: &str,
) -> Result<String, String> {
    let system = format!(
        "You are a data-extraction assistant. From the document supplied by the user, \
         extract the value for one column of a review table.\n\
         Column: {column_label}\n\
         Instruction: {column_prompt}\n\
         Expected output format: {column_format}\n\
         Reply with ONLY the extracted value — concise, no preamble, no explanation. \
         If the document does not contain the information, reply exactly \"N/A\"."
    );
    let truncated: String = doc_text.chars().take(DOC_TEXT_LIMIT).collect();

    let params = StreamParams {
        model: model.to_string(),
        system_prompt: system,
        system_volatile: String::new(),
        messages: vec![Message::user(truncated)],
        tools: vec![],
        max_iterations: 1,
        enable_thinking: false,
        local_config: build_local_config(model, Some(settings)),
        claude_api_key: settings.claude_api_key.clone(),
        gemini_api_key: settings.gemini_api_key.clone(),
        gemini_region: settings.gemini_region.clone(),
        // Tabular-cell extraction is per-row one-shot; Mistral
        // prompt-cache wouldn't help (each cell has a different
        // user prompt). Could be revisited if we batch cells.
        chat_id: None,
        mistral_opts: crate::routes::chat::build_mistral_opts(model, Some(settings)),
        response_schema: None,
    };

    let result = match llm::provider_for_model(model) {
        llm::Provider::Claude => llm::claude::complete(params).await,
        llm::Provider::OpenAI => llm::local::complete(params).await,
        llm::Provider::Gemini => llm::gemini::complete(params).await,
        llm::Provider::Mistral => llm::mistral::complete(params).await,
    };
    result
        .map(|t| t.trim().to_string())
        .map_err(|e| e.to_string())
}

/// Column definitions reduced to the (key, label, prompt, format) tuples
/// the extractor needs.
fn parse_columns(columns_config: &str) -> Vec<(String, String, String, String)> {
    serde_json::from_str::<Vec<Value>>(columns_config)
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(i, c)| {
            let key = c
                .get("key")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("col_{}", i + 1));
            // Presets use `name`; the create modal historically used `label`.
            let label = c
                .get("name")
                .and_then(|v| v.as_str())
                .or_else(|| c.get("label").and_then(|v| v.as_str()))
                .unwrap_or(&key)
                .to_string();
            let prompt = c
                .get("prompt")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let format = c
                .get("format")
                .and_then(|v| v.as_str())
                .unwrap_or("free_text")
                .to_string();
            (key, label, prompt, format)
        })
        .collect()
}

fn cell(key: &str, status: &str, content: &str) -> Value {
    json!({ "key": key, "status": status, "content": content })
}

async fn persist_cells(state: &AppState, row_id: &str, cells: &[Value], status: &str) {
    let _ = sqlx::query("UPDATE tabular_review_rows SET cells = ?, status = ? WHERE id = ?")
        .bind(serde_json::to_string(cells).unwrap_or_else(|_| "[]".to_string()))
        .bind(status)
        .bind(row_id)
        .execute(&state.db)
        .await;
}

fn sse(payload: Value) -> Result<Event, Infallible> {
    Ok(Event::default().data(payload.to_string()))
}

// ---------------------------------------------------------------------------
// POST /tabular-review/:id/generate  — SSE stream of cell updates
// ---------------------------------------------------------------------------
async fn generate_cells(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let meta = load_review_meta(&state, &auth.user_id, &id).await?;
    let columns = parse_columns(&meta.4);
    if columns.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "The review has no columns."));
    }
    let rows = load_review_rows(&state, &id).await?;
    if rows.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "The review has no documents."));
    }
    let settings = fetch_llm_settings(&state.db, &auth.user_id)
        .await
        .unwrap_or_default();
    let Some(model) = pick_tabular_model(&settings) else {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "No tabular-review model configured. Set one in Settings → Models.",
        ));
    };

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(64);
    let state_bg = state.clone();
    let user_id = auth.user_id.clone();

    tokio::spawn(async move {
        for row in rows {
            let Some(doc_id) = row.document_id.clone() else {
                continue;
            };
            // Existing cells keyed for skip-if-done.
            let mut cells: Vec<Value> = columns
                .iter()
                .map(|(key, _, _, _)| {
                    row.cells
                        .iter()
                        .find(|c| c.get("key").and_then(|v| v.as_str()) == Some(key.as_str()))
                        .cloned()
                        .unwrap_or_else(|| cell(key, "pending", ""))
                })
                .collect();

            let doc_text = load_document_text(&state_bg, &user_id, &doc_id).await;

            for (ci, (key, label, prompt, format)) in columns.iter().enumerate() {
                let done = cells[ci].get("status").and_then(|v| v.as_str()) == Some("done");
                if done {
                    continue;
                }
                cells[ci] = cell(key, "generating", "");
                let _ = tx
                    .send(sse(json!({
                        "type": "cell_update", "row_id": row.id,
                        "column_key": key, "status": "generating", "content": ""
                    })))
                    .await;

                let (status, content) = match doc_text.as_deref() {
                    None => (
                        "error".to_string(),
                        "Document text unavailable".to_string(),
                    ),
                    Some(text) => {
                        match extract_cell(&model, &settings, label, prompt, format, text).await {
                            Ok(answer) => ("done".to_string(), answer),
                            Err(e) => ("error".to_string(), e),
                        }
                    }
                };
                cells[ci] = cell(key, &status, &content);
                let _ = tx
                    .send(sse(json!({
                        "type": "cell_update", "row_id": row.id,
                        "column_key": key, "status": status, "content": content
                    })))
                    .await;
                persist_cells(&state_bg, &row.id, &cells, "done").await;
            }
        }
        let _ = sqlx::query("UPDATE tabular_reviews SET updated_at = datetime('now') WHERE id = ?")
            .bind(&id)
            .execute(&state_bg.db)
            .await;
        let _ = tx.send(sse(json!({ "type": "done" }))).await;
    });

    Ok(Sse::new(ReceiverStream::new(rx))
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response())
}

// ---------------------------------------------------------------------------
// POST /tabular-review/:id/regenerate-cell  { row_id, column_key }
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct RegenBody {
    row_id: String,
    column_key: String,
}

async fn regenerate_cell(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<RegenBody>,
) -> ApiResult {
    let meta = load_review_meta(&state, &auth.user_id, &id).await?;
    let columns = parse_columns(&meta.4);
    let Some((key, label, prompt, format)) =
        columns.iter().find(|(k, _, _, _)| k == &body.column_key)
    else {
        return Err(err(StatusCode::BAD_REQUEST, "Unknown column."));
    };

    let row: Option<(Option<String>, String)> = sqlx::query_as(
        "SELECT document_id, cells FROM tabular_review_rows \
         WHERE id = ? AND tabular_review_id = ?",
    )
    .bind(&body.row_id)
    .bind(&id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let Some((Some(doc_id), cells_json)) = row else {
        return Err(err(StatusCode::NOT_FOUND, "Row not found."));
    };

    let settings = fetch_llm_settings(&state.db, &auth.user_id)
        .await
        .unwrap_or_default();
    let Some(model) = pick_tabular_model(&settings) else {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "No tabular-review model configured.",
        ));
    };

    let (status, content) = match load_document_text(&state, &auth.user_id, &doc_id).await {
        None => (
            "error".to_string(),
            "Document text unavailable".to_string(),
        ),
        Some(text) => match extract_cell(&model, &settings, label, prompt, format, &text).await {
            Ok(answer) => ("done".to_string(), answer),
            Err(e) => ("error".to_string(), e),
        },
    };

    // Merge the regenerated cell back into the row's cell array.
    let mut cells: Vec<Value> = serde_json::from_str(&cells_json).unwrap_or_default();
    let updated = cell(key, &status, &content);
    if let Some(slot) = cells
        .iter_mut()
        .find(|c| c.get("key").and_then(|v| v.as_str()) == Some(key.as_str()))
    {
        *slot = updated.clone();
    } else {
        cells.push(updated.clone());
    }
    persist_cells(&state, &body.row_id, &cells, "done").await;

    Ok(Json(json!({
        "row_id": body.row_id,
        "column_key": key,
        "status": status,
        "content": content,
    })))
}

// ---------------------------------------------------------------------------
// POST /tabular-review/:id/clear-cells  { row_ids? }
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct ClearBody {
    /// When omitted, clears every row of the review.
    row_ids: Option<Vec<String>>,
}

async fn clear_cells(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<ClearBody>,
) -> ApiResult {
    load_review_meta(&state, &auth.user_id, &id).await?;

    match body.row_ids {
        Some(ids) => {
            for row_id in ids {
                let _ = sqlx::query(
                    "UPDATE tabular_review_rows SET cells = '[]', status = 'pending' \
                     WHERE id = ? AND tabular_review_id = ?",
                )
                .bind(&row_id)
                .bind(&id)
                .execute(&state.db)
                .await;
            }
        }
        None => {
            let _ = sqlx::query(
                "UPDATE tabular_review_rows SET cells = '[]', status = 'pending' \
                 WHERE tabular_review_id = ?",
            )
            .bind(&id)
            .execute(&state.db)
            .await;
        }
    }

    Ok(Json(json!({ "ok": true })))
}
