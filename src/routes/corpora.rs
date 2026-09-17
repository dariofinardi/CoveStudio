//! `/corpora` — list the JSON-manifest-driven corpus plugin registry.
//!
//! Read-only: the manifests live on disk under `corpora-plugins/` and
//! are loaded once at startup into `AppState::corpus_plugins`. This
//! endpoint surfaces the registry to the UI (settings panel can list
//! every available corpus uniformly, regardless of whether it's
//! served by a builtin Rust adapter or — eventually — a declarative
//! HTTP-fetch strategy).
//!
//! Per-user enable/disable state is NOT here; that still lives in
//! `corpus_settings` (see /eurlex/config etc.) keyed per-corpus.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;

use crate::{
    auth::middleware::AuthUser,
    corpora::plugin::{Capabilities, CorpusDiscovery, CorpusPlugin, CorpusSource},
    storage::make_storage,
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

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list_corpora))
        .route("/{id}", get(get_corpus))
        // Per-user enable/disable (+ language) for any corpus, stored
        // in the shared `corpus_settings` table. Every corpus is
        // deactivatable — the route is not gated on a capability.
        .route("/{id}/config", get(generic_get_config).put(generic_put_config))
        // Generic operations dispatched by corpus id. Each handler
        // looks the corpus up in `state.corpus_plugins`, validates
        // the capability is enabled, then delegates to the adapter
        // in `state.corpus_adapters` (declarative corpora only —
        // builtin ones keep their `/eurlex` `/italian-legal` routes
        // for now).
        .route("/{id}/search", post(generic_search))
        .route("/{id}/fetch", post(generic_fetch))
        // Read-only preview of a corpus document body. Same dispatch as
        // /fetch (DilaBulkXml → corpus_documents; italian-legal →
        // HuggingFace /rows; ManifestAdapter → live HTTP), but does NOT
        // persist or chunk. Used by the UI's "view text" modal so the
        // user can read the source before deciding to index it.
        .route("/{id}/preview", get(generic_preview))
        .route("/{id}/documents", get(generic_list_documents))
        .route("/{id}/documents/{doc_id}", delete(generic_delete_document))
        .route("/{id}/documents/{doc_id}/resync", post(generic_resync_document))
        // Bulk import (DILA-style: download tar.gz, walk XML, populate
        // corpus_documents + FTS5). Synchronous today; the route
        // blocks until the import finishes. Acceptable for CNIL
        // (~18 MB, ~10s); larger fondi like LEGI will need async +
        // progress polling.
        .route("/{id}/import", post(generic_import))
        .route("/{id}/import-status", get(generic_import_status))
        .route("/{id}/import-progress", get(generic_import_progress))
}

/// Public projection of a `CorpusPlugin` for the API. Strips the
/// `strategy` discriminator (an implementation detail) and exposes
/// `runnable` so the UI can dim entries that are declared but not
/// yet wired (e.g. future http-fetch-per-id manifests).
///
/// `capabilities` and `sources` are passed through verbatim because
/// they ARE the public contract the UI consumes.
#[derive(Debug, Serialize)]
struct CorpusItem {
    id: String,
    display_name: String,
    description: Option<String>,
    homepage: Option<String>,
    languages: Vec<String>,
    default_language: String,
    supports_language_fallback: bool,
    fallback_language: Option<String>,
    identifier_label: String,
    identifier_example: Option<String>,
    enabled_by_default: bool,
    /// Manifest-level kill switch — `false` retires a corpus whose
    /// connector isn't verified working; the UI hides it.
    available: bool,
    runnable: bool,
    capabilities: Capabilities,
    sources: Vec<CorpusSource>,
    discovery: Option<CorpusDiscovery>,
}

fn project(p: &crate::corpora::plugin::CorpusPlugin) -> CorpusItem {
    CorpusItem {
        id: p.id.clone(),
        display_name: p.display_name.clone(),
        description: p.description.clone(),
        homepage: p.homepage.clone(),
        languages: p.languages.clone(),
        default_language: p.default_language.clone(),
        supports_language_fallback: p.supports_language_fallback,
        fallback_language: p.fallback_language.clone(),
        identifier_label: p.identifier_label.clone(),
        identifier_example: p.identifier_example.clone(),
        enabled_by_default: p.enabled_by_default,
        available: p.available,
        runnable: p.is_runnable(),
        capabilities: p.capabilities.clone(),
        sources: p.sources.clone(),
        discovery: p.discovery.clone(),
    }
}

async fn list_corpora(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
) -> ApiResult {
    let items: Vec<CorpusItem> = state
        .corpus_plugins
        .read()
        .unwrap()
        .iter()
        .map(project)
        .collect();
    Ok(Json(json!({ "corpora": items })))
}

async fn get_corpus(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let plugin = lookup_plugin(&state, &id)?;
    Ok(Json(serde_json::to_value(project(&plugin)).unwrap()))
}

/// Find a corpus plugin by id or 404, returning an owned clone. The
/// registry lives behind a `RwLock` (hot-reloadable in dev), so a
/// borrow can't outlive the read guard — handlers get their own copy.
fn lookup_plugin(
    state: &AppState,
    id: &str,
) -> Result<CorpusPlugin, (StatusCode, Json<Value>)> {
    state
        .corpus_plugins
        .read()
        .unwrap()
        .iter()
        .find(|p| p.id == id)
        .cloned()
        .ok_or_else(|| err(StatusCode::NOT_FOUND, &format!("corpus {id:?} not found")))
}

// ---------------------------------------------------------------------------
// GET|PUT /corpora/:id/config — per-user enable/disable + language
// ---------------------------------------------------------------------------
//
// Shares the `corpus_settings` table with EUR-Lex (`/eurlex/config`).
// When no row exists yet the response reflects the manifest defaults
// (`enabled_by_default`, `default_language`).

#[derive(Deserialize)]
struct CorpusConfigPayload {
    enabled: bool,
    language: Option<String>,
    fallback_en: Option<bool>,
}

async fn generic_get_config(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let plugin = lookup_plugin(&state, &id)?;
    let row: Option<(i64, Option<String>, i64)> = sqlx::query_as(
        "SELECT enabled, language, fallback_en FROM corpus_settings \
         WHERE user_id = ? AND corpus_id = ?",
    )
    .bind(&auth.user_id)
    .bind(&id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (enabled, language, fallback_en) = match row {
        Some((e, l, f)) => (e != 0, l, f != 0),
        None => (
            plugin.enabled_by_default,
            Some(plugin.default_language.clone()),
            plugin.supports_language_fallback,
        ),
    };
    Ok(Json(json!({
        "enabled": enabled,
        "language": language.unwrap_or_else(|| plugin.default_language.clone()),
        "fallback_en": fallback_en,
    })))
}

async fn generic_put_config(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<CorpusConfigPayload>,
) -> ApiResult {
    let plugin = lookup_plugin(&state, &id)?;
    let language = body
        .language
        .as_deref()
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| plugin.default_language.clone());
    let fallback_en = body
        .fallback_en
        .unwrap_or(plugin.supports_language_fallback);

    sqlx::query(
        "INSERT INTO corpus_settings \
           (user_id, corpus_id, enabled, language, fallback_en, updated_at) \
         VALUES (?, ?, ?, ?, ?, datetime('now')) \
         ON CONFLICT(user_id, corpus_id) DO UPDATE SET \
           enabled = excluded.enabled, \
           language = excluded.language, \
           fallback_en = excluded.fallback_en, \
           updated_at = excluded.updated_at",
    )
    .bind(&auth.user_id)
    .bind(&id)
    .bind(body.enabled as i64)
    .bind(&language)
    .bind(fallback_en as i64)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({
        "enabled": body.enabled,
        "language": language,
        "fallback_en": fallback_en,
    })))
}

fn de_gesetze_slug_for_code(code: &str) -> Option<&'static str> {
    match code {
        "BGB" => Some("bgb"),
        "STGB" => Some("stgb"),
        "HGB" => Some("hgb"),
        "GG" => Some("gg"),
        "ZPO" => Some("zpo"),
        "STPO" => Some("stpo"),
        "AO" => Some("ao_1977"),
        "VWGO" => Some("vwgo"),
        "BVERFGG" => Some("bverfgg"),
        _ => None,
    }
}

fn is_section_token(token: &str) -> bool {
    let mut chars = token.chars();
    let mut saw_digit = false;
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            saw_digit = true;
            continue;
        }
        if c.is_ascii_alphabetic() {
            continue;
        }
        return false;
    }
    saw_digit
}

fn extract_section_after_paragraph_sign(query: &str) -> Option<String> {
    let idx = query.find('§')?;
    let mut out = String::new();
    let mut started = false;
    for c in query[idx + '§'.len_utf8()..].chars() {
        if !started && c.is_whitespace() {
            continue;
        }
        if c.is_ascii_digit() || c.is_ascii_alphabetic() {
            started = true;
            out.push(c.to_ascii_lowercase());
            continue;
        }
        break;
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn parse_de_gesetze_identifier(query: &str) -> Option<String> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }

    // Already a direct path (advanced users / pasted href).
    if q.contains('/') && q.ends_with(".html") && !q.contains(' ') {
        return Some(q.trim_start_matches('/').to_string());
    }

    let tokens: Vec<String> = q
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_uppercase())
        .collect();
    let (code_idx, code, slug) = tokens
        .iter()
        .enumerate()
        .find_map(|(i, t)| de_gesetze_slug_for_code(t).map(|slug| (i, t.clone(), slug)))?;

    let section = extract_section_after_paragraph_sign(q).or_else(|| {
        let neigh = [
            code_idx.checked_add(1),
            code_idx.checked_sub(1),
            code_idx.checked_add(2),
            code_idx.checked_sub(2),
        ];
        for i in neigh.into_iter().flatten() {
            if let Some(tok) = tokens.get(i) {
                let n = tok.to_ascii_lowercase();
                if is_section_token(&n) {
                    return Some(n);
                }
            }
        }
        None
    })?;

    let _ = code; // kept for future metadata/debug use
    Some(format!("{slug}/__{section}.html"))
}

// ---------------------------------------------------------------------------
// POST /corpora/:id/search  — { query, language?, limit? }
// ---------------------------------------------------------------------------
//
// Dispatches to the corpus's declarative adapter (when the manifest
// uses `http-fetch-per-id`). Builtin corpora (EUR-Lex, Italian Legal)
// don't pass through this route yet — they still serve via their own
// `/eurlex/search`, `/italian-legal/search` endpoints. The handler
// returns 501 with a hint so a misconfigured frontend gets a
// readable error instead of a silent miss.

#[derive(Deserialize)]
struct SearchPayload {
    query: String,
    language: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn generic_search(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<SearchPayload>,
) -> ApiResult {
    let plugin = lookup_plugin(&state, &id)?;
    if !plugin.capabilities.search {
        return Err(err(
            StatusCode::METHOD_NOT_ALLOWED,
            &format!("corpus {id} does not declare capabilities.search"),
        ));
    }

    let q = body.query.trim();
    if q.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "query is empty"));
    }
    let lang = body.language.as_deref();
    let limit = body.limit.unwrap_or(20).min(100);

    // Bulk-indexed corpora (DILA): query corpus_documents FTS5
    // directly — there's no live HTTP adapter, the data is already
    // in the local DB. MUST short-circuit BEFORE the
    // corpus_adapters lookup below, otherwise the registry-miss
    // branch would 501 every bulk corpus.
    if matches!(
        plugin.strategy,
        crate::corpora::plugin::CorpusStrategy::DilaBulkXml(_)
    ) {
        let hits =
            crate::corpora::dila_bulk::search_local_index(&state.db, &id, q, limit)
                .await
                .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
        return Ok(Json(json!({ "hits": hits })));
    }

    // Remaining strategies need a runtime adapter in the registry.
    // Clone the Arc out of the lock so no guard is held across await.
    let adapter = state.corpus_adapters.read().unwrap().get(&id).cloned();
    let Some(adapter) = adapter else {
        return Err(err(
            StatusCode::NOT_IMPLEMENTED,
            &format!(
                "corpus {id} has no runtime adapter via the generic router \
                 (likely a builtin corpus — use its dedicated /{id} route instead)"
            ),
        ));
    };

    // Routing policy: if `capabilities.search` is true we MUST honour
    // it. Prefer the keyword search (it handles human references AND
    // free text); fall back to identifier probe only when the manifest
    // doesn't expose a keyword search. The previous "no-whitespace
    // = identifier" heuristic mis-routed corpus-specific reference
    // shapes (e.g. CNIL "SAN-2024-013" went through search_by_id and
    // tried to fetch a URL templated with the human ref, which 404'd
    // because the canonical identifier is the opaque CNILTEXT id).

    let has_keyword = match &plugin.strategy {
        crate::corpora::plugin::CorpusStrategy::HttpFetchPerId(spec) => {
            spec.search_by_keyword.is_some()
        }
        // Builtin adapters in the registry (Fedlex) implement keyword
        // search natively.
        crate::corpora::plugin::CorpusStrategy::Builtin { .. } => true,
        _ => false,
    };
    let hits = if id == "de-gesetze" {
        if let Some(identifier) = parse_de_gesetze_identifier(q) {
            let direct = adapter
                .search_by_id(&identifier, lang)
                .await
                .map_err(|e| err(StatusCode::BAD_GATEWAY, &e.to_string()))?;
            if !direct.is_empty() {
                direct
            } else if has_keyword {
                adapter
                    .search_by_keyword(q, lang, limit)
                    .await
                    .map_err(|e| err(StatusCode::BAD_GATEWAY, &e.to_string()))?
            } else {
                Vec::new()
            }
        } else if has_keyword {
            adapter
                .search_by_keyword(q, lang, limit)
                .await
                .map_err(|e| err(StatusCode::BAD_GATEWAY, &e.to_string()))?
        } else {
            adapter
                .search_by_id(q, lang)
                .await
                .map_err(|e| err(StatusCode::BAD_GATEWAY, &e.to_string()))?
        }
    } else if has_keyword {
        // Flexible single-box search: run the keyword search first;
        // if it yields nothing, the query may be a corpus-native
        // identifier the user pasted verbatim — probe it by id so
        // one input accepts both free text and identifiers. An
        // id-probe failure here is non-fatal: keep the (empty)
        // keyword result rather than surfacing a gateway error.
        let kw = adapter
            .search_by_keyword(q, lang, limit)
            .await
            .map_err(|e| err(StatusCode::BAD_GATEWAY, &e.to_string()))?;
        if kw.is_empty() {
            adapter.search_by_id(q, lang).await.unwrap_or_default()
        } else {
            kw
        }
    } else {
        adapter
            .search_by_id(q, lang)
            .await
            .map_err(|e| err(StatusCode::BAD_GATEWAY, &e.to_string()))?
    };
    Ok(Json(json!({ "hits": hits })))
}

// search_local_index lives in src/corpora/dila_bulk.rs (so it's
// testable end-to-end with a fixture); routes/corpora.rs above
// delegates to it directly.

// ---------------------------------------------------------------------------
// POST /corpora/:id/fetch  — { identifier, language? }
// ---------------------------------------------------------------------------
//
// Fetches one document via the corpus adapter, stores its bytes in
// the shared hash-keyed cache (same layout as EUR-Lex's
// `cache/<sha256>.txt`), and inserts a `documents` row. Indexing is
// kicked off only when the `rag` feature is built in. Returns the
// new document id + chunk count so the UI can refresh the list.

#[derive(Deserialize)]
struct FetchPayload {
    identifier: String,
    language: Option<String>,
    date: Option<String>,
}

async fn generic_fetch(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<FetchPayload>,
) -> ApiResult {
    let plugin = lookup_plugin(&state, &id)?;
    if !plugin.capabilities.fetch {
        return Err(err(
            StatusCode::METHOD_NOT_ALLOWED,
            &format!("corpus {id} does not declare capabilities.fetch"),
        ));
    }

    let identifier = body.identifier.trim().to_string();
    if identifier.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "identifier is empty"));
    }
    let lang = body
        .language
        .clone()
        .unwrap_or_else(|| plugin.default_language.clone())
        .to_ascii_lowercase();

    // Bulk-indexed strategy: the doc body is already in
    // corpus_documents. Skip the adapter dispatch and synthesise
    // a CorpusDocument from the DB row instead.
    let fetched: crate::corpora::CorpusDocument = if matches!(
        plugin.strategy,
        crate::corpora::plugin::CorpusStrategy::DilaBulkXml(_)
    ) {
        match fetch_corpus_document(&state.db, &id, &identifier).await {
            Ok(Some(doc)) => doc,
            Ok(None) => {
                return Err(err(
                    StatusCode::NOT_FOUND,
                    &format!(
                        "corpus {id}: identifier {identifier:?} not in local index — \
                         run /corpora/{id}/import first"
                    ),
                ));
            }
            Err(e) => {
                return Err(err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()));
            }
        }
    } else {
        let adapter = state.corpus_adapters.read().unwrap().get(&id).cloned();
        let Some(adapter) = adapter else {
            return Err(err(
                StatusCode::NOT_IMPLEMENTED,
                &format!(
                    "corpus {id} has no runtime adapter via the generic router \
                     (likely a builtin corpus — use its dedicated /{id} route instead)"
                ),
            ));
        };
        adapter
            .fetch(&identifier, Some(&lang), plugin.supports_language_fallback)
            .await
            .map_err(|e| err(StatusCode::BAD_GATEWAY, &e.to_string()))?
    };

    let corpus_date = body
        .date
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(str::to_string)
        .or(fetched.date.clone());

    // Dedupe by (corpus_id, identifier, language) — same policy the
    // EUR-Lex route uses.
    let existing: Option<(String, String)> = sqlx::query_as(
        "SELECT id, filename FROM documents \
         WHERE user_id = ? AND corpus_id = ? AND corpus_identifier = ? AND corpus_language = ?",
    )
    .bind(&auth.user_id)
    .bind(&plugin.id)
    .bind(&identifier)
    .bind(&lang)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    if let Some((eid, fname)) = existing {
        if corpus_date.is_some() {
            let _ = sqlx::query(
                "UPDATE documents SET corpus_date = COALESCE(corpus_date, ?) \
                 WHERE id = ? AND user_id = ?",
            )
            .bind(&corpus_date)
            .bind(&eid)
            .bind(&auth.user_id)
            .execute(&state.db)
            .await;
        }
        return Ok(Json(json!({
            "id": eid, "filename": fname, "already_indexed": true,
            "corpus_id": plugin.id, "corpus_identifier": identifier,
            "corpus_language": lang,
            "corpus_date": corpus_date,
        })));
    }

    // Hash-keyed cache (same layout as chat attachments + EUR-Lex).
    let hash = {
        let mut h = Sha256::new();
        h.update(&fetched.bytes);
        format!("{:x}", h.finalize())
    };
    let bin_key = format!("cache/{}.txt", hash);
    let storage = make_storage()
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let bin_abs =
        storage_root().join(bin_key.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !bin_abs.exists() {
        storage
            .put(&bin_key, &fetched.bytes, "text/plain; charset=utf-8")
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    }

    let doc_id = uuid::Uuid::new_v4().to_string();
    let filename = format!(
        "{} ({}).txt",
        fetched.title,
        fetched.language.to_uppercase()
    );
    let size = fetched.bytes.len() as i64;
    sqlx::query(
        "INSERT INTO documents \
           (id, user_id, project_id, filename, file_type, size_bytes, \
            storage_path, status, content_hash, extracted_text_path, \
            corpus_id, corpus_identifier, corpus_language, corpus_date, fetched_with_fallback) \
         VALUES (?, ?, NULL, ?, 'txt', ?, ?, 'syncing', ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&doc_id)
    .bind(&auth.user_id)
    .bind(&filename)
    .bind(size)
    .bind(&bin_key)
    .bind(&hash)
    .bind(&bin_key)
    .bind(&plugin.id)
    .bind(&fetched.identifier)
    .bind(&fetched.language)
    .bind(&corpus_date)
    .bind(fetched.fetched_with_fallback as i64)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    // Indexing: when rag is on, kick off chunk+embed via the shared
    // helper. When off, just mark ready (text is on disk).
    let text = String::from_utf8_lossy(&fetched.bytes).into_owned();
    let chunk_source_path = bin_abs.to_string_lossy().to_string();
    let (chunks_indexed, indexing_error, final_status) =
        index_text(&state, &auth.user_id, &doc_id, &chunk_source_path, &text).await;

    sqlx::query("UPDATE documents SET status = ? WHERE id = ?")
        .bind(&final_status)
        .bind(&doc_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({
        "id": doc_id,
        "filename": filename,
        "corpus_id": plugin.id,
        "corpus_identifier": fetched.identifier,
        "corpus_language": fetched.language,
        "corpus_date": corpus_date,
        "fetched_with_fallback": fetched.fetched_with_fallback,
        "source_url": fetched.source_url,
        "size_bytes": size,
        "already_indexed": false,
        "chunks_indexed": chunks_indexed,
        "indexing_error": indexing_error,
        "status": final_status,
    })))
}

// ---------------------------------------------------------------------------
// GET /corpora/:id/preview?identifier=…&language=…
// ---------------------------------------------------------------------------
//
// Read-only "what's in this document?" — returns the plain-text body
// of a corpus document so the UI can show it in a scrollable viewer
// before the user decides to index it. Mirrors `generic_fetch`'s
// strategy dispatch but DOES NOT persist anything (no `documents` row,
// no cache file, no chunking, no embedding side-effects).
//
// Strategy fan-out:
//   * DilaBulkXml         — read body from `corpus_documents`
//   * builtin italian-legal — look up row_offset in `italian_corpus`,
//                            then fetch the HF /rows endpoint
//   * ManifestAdapter     — invoke the live adapter `.fetch()` and
//                            return its bytes verbatim
//
// Response shape: `{ identifier, title, source_url, text }`.

#[derive(Deserialize)]
struct PreviewQuery {
    identifier: String,
    language: Option<String>,
}

async fn generic_preview(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(id): Path<String>,
    Query(q): Query<PreviewQuery>,
) -> ApiResult {
    let plugin = lookup_plugin(&state, &id)?;
    let identifier = q.identifier.trim().to_string();
    if identifier.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "identifier is empty"));
    }

    // Branch 1: bulk-XML strategy — body already in corpus_documents.
    if matches!(
        plugin.strategy,
        crate::corpora::plugin::CorpusStrategy::DilaBulkXml(_)
    ) {
        let doc = fetch_corpus_document(&state.db, &id, &identifier)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?
            .ok_or_else(|| {
                err(
                    StatusCode::NOT_FOUND,
                    &format!(
                        "corpus {id}: identifier {identifier:?} not in local index — \
                         run /corpora/{id}/import first"
                    ),
                )
            })?;
        let text = String::from_utf8_lossy(&doc.bytes).into_owned();
        return Ok(Json(json!({
            "identifier": doc.identifier,
            "title": doc.title,
            "source_url": doc.source_url,
            "text": text,
        })));
    }

    // Branch 2: italian-legal builtin — text lives in the HuggingFace
    // dataset, fetched by row_offset (kept in `italian_corpus`).
    if id == "italian-legal" {
        let row: Option<(i64, Option<String>)> = sqlx::query_as(
            "SELECT row_offset, title FROM italian_corpus WHERE hf_id = ?",
        )
        .bind(&identifier)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
        let Some((row_offset, title_opt)) = row else {
            return Err(err(
                StatusCode::NOT_FOUND,
                &format!(
                    "italian-legal: hf_id {identifier:?} not in local index — \
                     run the corpus import first"
                ),
            ));
        };
        let client = reqwest::Client::builder()
            .user_agent(crate::product::user_agent(Some("italian-legal-corpus preview")))
            .build()
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
        let (hf_title, text) =
            crate::corpora::italian_legal::fetch_full_text(&client, row_offset)
                .await
                .map_err(|e| {
                    err(
                        StatusCode::BAD_GATEWAY,
                        &format!("HuggingFace fetch: {e}"),
                    )
                })?;
        let title = title_opt
            .filter(|s| !s.is_empty())
            .unwrap_or(if hf_title.is_empty() {
                identifier.clone()
            } else {
                hf_title
            });
        return Ok(Json(json!({
            "identifier": identifier,
            "title": title,
            "source_url": format!(
                "https://huggingface.co/datasets/{ds}/viewer/default/train?row={row_offset}",
                ds = crate::corpora::italian_legal::DATASET,
            ),
            "text": text,
        })));
    }

    // Branch 3: manifest-adapter live fetch — ephemeral, no persistence.
    let adapter = state.corpus_adapters.read().unwrap().get(&id).cloned();
    let Some(adapter) = adapter else {
        return Err(err(
            StatusCode::NOT_IMPLEMENTED,
            &format!(
                "corpus {id} has no preview path (no DilaBulkXml store, no \
                 builtin handler, no manifest adapter registered)"
            ),
        ));
    };
    let lang = q
        .language
        .clone()
        .unwrap_or_else(|| plugin.default_language.clone())
        .to_ascii_lowercase();
    let fetched = adapter
        .fetch(&identifier, Some(&lang), plugin.supports_language_fallback)
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, &format!("adapter fetch: {e}")))?;
    let text = String::from_utf8_lossy(&fetched.bytes).into_owned();
    Ok(Json(json!({
        "identifier": fetched.identifier,
        "title": fetched.title,
        "source_url": fetched.source_url,
        "text": text,
    })))
}

/// Run chunking + embedding. Tuple semantics identical to
/// `eurlex.rs::run_indexing` (kept separate to avoid cross-module
/// pub coupling on a 20-line helper).
async fn index_text(
    state: &AppState,
    user_id: &str,
    doc_id: &str,
    source_path: &str,
    text: &str,
) -> (usize, Option<String>, String) {
    #[cfg(feature = "rag")]
    {
        if let Some(emb) = state.embeddings.clone() {
            return match emb
                .index_document(user_id, None, doc_id, source_path, text)
                .await
            {
                Ok(0) => {
                    let msg = format!(
                        "Indicizzazione completata ma 0 chunk creati (testo: {} caratteri).",
                        text.len()
                    );
                    (0, Some(msg), "interrupted".to_string())
                }
                Ok(n) => (n, None, "ready".to_string()),
                Err(e) => (0, Some(e.to_string()), "interrupted".to_string()),
            };
        }
    }
    let _ = (state, user_id, doc_id, source_path, text);
    (0, None, "ready".to_string())
}

// ---------------------------------------------------------------------------
// GET /corpora/:id/documents — list docs the user synced for this corpus
// ---------------------------------------------------------------------------

async fn generic_list_documents(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let plugin = lookup_plugin(&state, &id)?;
    if !plugin.capabilities.documents {
        return Err(err(
            StatusCode::METHOD_NOT_ALLOWED,
            &format!("corpus {id} does not declare capabilities.documents"),
        ));
    }

    let rows: Vec<(
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
        i64,
        String,
        String,
    )> = sqlx::query_as(
        "SELECT id, filename, corpus_identifier, corpus_language, \
                corpus_date, fetched_with_fallback, size_bytes, created_at, status \
         FROM documents \
         WHERE user_id = ? AND corpus_id = ? \
         ORDER BY created_at DESC",
    )
    .bind(&auth.user_id)
    .bind(&id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let docs: Vec<Value> = rows
        .into_iter()
        .map(|(doc_id, filename, ident, lang, date, fb, size, created, status)| {
            json!({
                "id": doc_id, "filename": filename,
                "corpus_identifier": ident, "corpus_language": lang,
                "corpus_date": date,
                "fetched_with_fallback": fb != 0,
                "size_bytes": size, "created_at": created, "status": status,
            })
        })
        .collect();
    Ok(Json(json!({ "documents": docs })))
}

// ---------------------------------------------------------------------------
// DELETE /corpora/:id/documents/:doc_id — remove a synced doc
// ---------------------------------------------------------------------------
//
// Mirrors `/eurlex/documents/:id` policy: drop the documents row +
// embedding chunks, then delete the on-disk cache file only if no
// other documents row still references the same content hash
// (ref-counted across users / chats).
async fn generic_delete_document(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((id, doc_id)): Path<(String, String)>,
) -> ApiResult {
    let plugin = lookup_plugin(&state, &id)?;
    if !plugin.capabilities.documents_delete {
        return Err(err(
            StatusCode::METHOD_NOT_ALLOWED,
            &format!("corpus {id} does not declare capabilities.documents_delete"),
        ));
    }

    let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT storage_path, content_hash FROM documents \
         WHERE id = ? AND user_id = ? AND corpus_id = ?",
    )
    .bind(&doc_id)
    .bind(&auth.user_id)
    .bind(&id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (storage_path, content_hash) =
        row.ok_or_else(|| err(StatusCode::NOT_FOUND, "Document not found"))?;

    let _ = sqlx::query("DELETE FROM doc_chunks WHERE document_id = ?")
        .bind(&doc_id)
        .execute(&state.db)
        .await;

    sqlx::query("DELETE FROM documents WHERE id = ? AND user_id = ?")
        .bind(&doc_id)
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    if let Some(hash) = content_hash {
        let still_referenced: Option<(i64,)> = sqlx::query_as(
            "SELECT 1 FROM documents WHERE content_hash = ? LIMIT 1",
        )
        .bind(&hash)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
        if still_referenced.is_none() {
            if let (Ok(storage), Some(key)) = (make_storage(), storage_path) {
                if let Err(e) = storage.delete(&key).await {
                    tracing::warn!(
                        "[corpora/{}] failed to delete cache file {} for doc {}: {}",
                        id,
                        key,
                        doc_id,
                        e
                    );
                }
            }
        }
    }

    Ok(Json(json!({ "ok": true, "id": doc_id })))
}

// ---------------------------------------------------------------------------
// POST /corpora/:id/documents/:doc_id/resync — restart indexing for a doc
// ---------------------------------------------------------------------------
//
// Mirrors EUR-Lex/Italian Legal behaviour: keeps the cached corpus text,
// marks the row as syncing, re-runs chunk+embed, then writes terminal status.
async fn generic_resync_document(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((id, doc_id)): Path<(String, String)>,
) -> ApiResult {
    let plugin = lookup_plugin(&state, &id)?;
    if !plugin.capabilities.documents_resync {
        return Err(err(
            StatusCode::METHOD_NOT_ALLOWED,
            &format!("corpus {id} does not declare capabilities.documents_resync"),
        ));
    }

    let row: Option<(Option<String>, Option<String>, String)> = sqlx::query_as(
        "SELECT extracted_text_path, corpus_identifier, status FROM documents \
         WHERE id = ? AND user_id = ? AND corpus_id = ?",
    )
    .bind(&doc_id)
    .bind(&auth.user_id)
    .bind(&id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (text_path, _identifier, prev_status) =
        row.ok_or_else(|| err(StatusCode::NOT_FOUND, "Document not found"))?;
    let text_key = text_path.ok_or_else(|| {
        err(
            StatusCode::CONFLICT,
            "Documento senza testo estratto: re-fetch necessario",
        )
    })?;

    let _ = sqlx::query("UPDATE documents SET status = 'syncing' WHERE id = ?")
        .bind(&doc_id)
        .execute(&state.db)
        .await;

    let storage =
        make_storage().map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let bytes = storage
        .get(&text_key)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let source_path = storage_root()
        .join(text_key.replace('/', std::path::MAIN_SEPARATOR_STR))
        .to_string_lossy()
        .to_string();

    let (chunks_indexed, indexing_error, final_status) =
        index_text(&state, &auth.user_id, &doc_id, &source_path, &text).await;

    sqlx::query("UPDATE documents SET status = ? WHERE id = ?")
        .bind(&final_status)
        .bind(&doc_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({
        "id": doc_id,
        "corpus_id": id,
        "previous_status": prev_status,
        "status": final_status,
        "chunks_indexed": chunks_indexed,
        "indexing_error": indexing_error,
    })))
}

// ---------------------------------------------------------------------------
// Bulk-indexed corpus helpers (DILA today)
// ---------------------------------------------------------------------------

/// Synthesize a `CorpusDocument` from the local `corpus_documents`
/// row. Returns `Ok(None)` when the identifier isn't in the local
/// index — caller should hint at running the importer.
async fn fetch_corpus_document(
    db: &sqlx::SqlitePool,
    corpus_id: &str,
    identifier: &str,
) -> Result<Option<crate::corpora::CorpusDocument>, sqlx::Error> {
    let row: Option<(
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
    )> = sqlx::query_as(
        "SELECT identifier, titre_full, titre, numero, date_texte, date_publi, body \
         FROM corpus_documents \
         WHERE corpus_id = ? AND identifier = ?",
    )
    .bind(corpus_id)
    .bind(identifier)
    .fetch_optional(db)
    .await?;
    Ok(row.map(|(id, titre_full, titre, numero, date_texte, date_publi, body)| {
        let title = titre_full
            .filter(|s| !s.is_empty())
            .or(titre.filter(|s| !s.is_empty()))
            .or(numero.filter(|s| !s.is_empty()))
            .unwrap_or_else(|| id.clone());
        crate::corpora::CorpusDocument {
            identifier: id,
            title,
            date: date_texte
                .filter(|s| !s.is_empty())
                .or(date_publi.filter(|s| !s.is_empty())),
            language: String::new(), // DILA is corpus-monolingual; UI fills in
            fetched_with_fallback: false,
            bytes: body.into_bytes(),
            mime: "text/plain; charset=utf-8",
            source_url: String::new(),
        }
    }))
}

// ---------------------------------------------------------------------------
// POST /corpora/:id/import — bulk import (DILA tar.gz today)
// ---------------------------------------------------------------------------
//
// Synchronous: the request blocks until the archive is downloaded,
// extracted, and every XML inserted. Fine for CNIL (~18 MB, ~10s on
// a typical link); LEGI-scale fondi will need async + progress
// polling, tracked as a follow-up.

async fn generic_import(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    use crate::corpora::plugin::CorpusStrategy;
    let plugin = lookup_plugin(&state, &id)?;
    if !plugin.capabilities.bulk_import {
        return Err(err(
            StatusCode::METHOD_NOT_ALLOWED,
            &format!("corpus {id} does not declare capabilities.bulk_import"),
        ));
    }
    let spec = match &plugin.strategy {
        CorpusStrategy::DilaBulkXml(s) => s.clone(),
        other => {
            return Err(err(
                StatusCode::NOT_IMPLEMENTED,
                &format!(
                    "corpus {id}: bulk_import is declared but no bulk strategy \
                     is wired in this router (strategy: {})",
                    strategy_kind(other)
                ),
            ));
        }
    };

    // Refuse to start a second import if one is already running for
    // the same corpus. The progress map is the source of truth: a
    // phase in {discovering, downloading, extracting, inserting} means
    // a task is in flight; anything else (done/error/idle/missing)
    // means we're free to start.
    {
        let guard = state.corpus_import_progress.read().await;
        if let Some(sink) = guard.get(&id) {
            let progress = sink.read().await;
            let in_flight = matches!(
                progress.phase.as_str(),
                "discovering" | "downloading" | "extracting" | "inserting"
            );
            if in_flight {
                return Ok(Json(json!({
                    "started": false,
                    "already_running": true,
                    "phase": progress.phase,
                    "message": progress.message,
                })));
            }
        }
    }

    // Create / reset the progress sink for this corpus.
    let sink = Arc::new(tokio::sync::RwLock::new(
        crate::corpora::dila_bulk::ImportProgress {
            phase: "discovering".to_string(),
            message: "Avvio import…".to_string(),
            current: 0,
            total: 0,
            error: None,
        },
    ));
    {
        let mut guard = state.corpus_import_progress.write().await;
        guard.insert(id.clone(), sink.clone());
    }

    // Spawn the task. The progress sink lets the user poll while the
    // worker runs; the task itself awaits on the import and stamps
    // `done` / `error` on the sink before exiting.
    let db = state.db.clone();
    let corpus_id = id.clone();
    tokio::spawn(async move {
        let _ = crate::corpora::dila_bulk::run_import(&spec, &db, &corpus_id, Some(sink))
            .await;
        // The result is reflected in the progress sink; nothing more
        // to do here. Errors are logged inside run_import.
    });

    Ok(Json(json!({
        "started": true,
        "already_running": false,
    })))
}

// ---------------------------------------------------------------------------
// GET /corpora/:id/import-progress — live phase + counter for the bar
// ---------------------------------------------------------------------------
//
// Returns null when no import has been started for this corpus in
// the current process lifetime. The UI uses that to render the
// "Importa ora" state vs the live progress section.

async fn generic_import_progress(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let guard = state.corpus_import_progress.read().await;
    let Some(sink) = guard.get(&id).cloned() else {
        return Ok(Json(serde_json::Value::Null));
    };
    drop(guard);
    let progress = sink.read().await.clone();
    let value = serde_json::to_value(progress)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    Ok(Json(value))
}

// ---------------------------------------------------------------------------
// GET /corpora/:id/import-status — snapshot date + doc count
// ---------------------------------------------------------------------------

async fn generic_import_status(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let plugin = lookup_plugin(&state, &id)?;
    if !plugin.capabilities.bulk_import {
        return Err(err(
            StatusCode::METHOD_NOT_ALLOWED,
            &format!("corpus {id} does not declare capabilities.bulk_import"),
        ));
    }
    let info = crate::corpora::dila_bulk::read_import_status(&state.db, &id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    match info {
        None => Ok(Json(json!({
            "imported": false,
            "doc_count": 0,
        }))),
        Some((url, ts, at, n)) => Ok(Json(json!({
            "imported": true,
            "last_archive_url": url,
            "last_archive_ts":  ts,
            "last_imported_at": at,
            "doc_count":        n,
        }))),
    }
}

fn strategy_kind(s: &crate::corpora::plugin::CorpusStrategy) -> &'static str {
    use crate::corpora::plugin::CorpusStrategy;
    match s {
        CorpusStrategy::Builtin { .. } => "builtin",
        CorpusStrategy::HttpFetchPerId(_) => "http-fetch-per-id",
        CorpusStrategy::DilaBulkXml(_) => "dila-bulk-xml",
        CorpusStrategy::HfDatasetBulk(_) => "hf-dataset-bulk",
    }
}
