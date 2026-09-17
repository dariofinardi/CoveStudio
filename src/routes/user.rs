use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::sse::{Event as SseEvent, KeepAlive, Sse},
    routing::{delete, get},
    Json, Router,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::Arc;

use crate::{auth::middleware::AuthUser, AppState};

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn err(status: StatusCode, msg: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"detail": msg})))
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/profile", get(get_profile).put(update_profile))
        .route("/llm-settings", get(get_llm_settings).put(update_llm_settings))
        .route("/locale", get(get_locale).put(update_locale))
        .route("/default-domain", get(get_default_domain).put(update_default_domain))
        .route("/enabled-domains", get(get_enabled_domains).put(update_enabled_domains))
        .route("/hyde-enabled", get(get_hyde_enabled).put(update_hyde_enabled))
        // v0.5.6 "Modalità sicura locale" plug-and-play endpoints.
        // The frontend Settings UI hits these to (a) detect whether
        // Ollama is reachable on loopback, (b) enumerate the curated
        // model catalogue + which entries are already installed and
        // (c) idempotently install / uninstall a curated entry with
        // streaming progress.
        .route(
            "/local-secure/heartbeat",
            get(local_secure_heartbeat),
        )
        .route(
            "/local-secure/models",
            get(local_secure_models),
        )
        .route(
            "/local-secure/ensure/{model_id}",
            axum::routing::post(local_secure_ensure),
        )
        .route(
            "/local-secure/uninstall/{model_id}",
            delete(local_secure_uninstall),
        )
        .route("/local-secure/migrate", axum::routing::post(local_secure_migrate))
        .route("/local-secure/remove-legacy", axum::routing::post(local_secure_remove_legacy))
        .route("/account", delete(delete_account))
        .route("/mcp-servers", get(list_mcp_servers).post(upsert_mcp_server))
        .route("/mcp-servers/probe", axum::routing::post(probe_mcp_server))
        .route("/mcp-servers/{name}", delete(delete_mcp_server).put(upsert_mcp_server_named))
}

// ---------------------------------------------------------------------------
// GET /user/locale  →  { locale: "it" | "en" | "fr" | "de" | "es" | "pt" | null }
// PUT /user/locale  body { locale: "it" | "en" | "fr" | "de" | "es" | "pt" }
//
// Persists the UI locale in user_settings so the choice follows the data
// folder rather than living only in the browser. The Next.js frontend
// keeps a cookie for SSR, but on profile load it reconciles that cookie
// with this value.
// ---------------------------------------------------------------------------
async fn get_locale(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT locale FROM user_settings WHERE user_id = ?")
            .bind(&auth.user_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let locale = row.and_then(|(l,)| l);
    Ok(Json(json!({ "locale": locale })))
}

#[derive(Deserialize)]
struct UpdateLocaleBody {
    locale: String,
}

async fn update_locale(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<UpdateLocaleBody>,
) -> ApiResult {
    let normalized = match body.locale.as_str() {
        "it" | "en" | "fr" | "de" | "es" | "pt" => body.locale,
        _ => return Err(err(StatusCode::BAD_REQUEST, "unsupported locale")),
    };
    sqlx::query(
        "INSERT INTO user_settings (user_id, locale, updated_at) \
         VALUES (?, ?, datetime('now')) \
         ON CONFLICT(user_id) DO UPDATE SET locale = excluded.locale, \
             updated_at = datetime('now')",
    )
    .bind(&auth.user_id)
    .bind(&normalized)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    Ok(Json(json!({ "ok": true, "locale": normalized })))
}

// ---------------------------------------------------------------------------
// GET /user/default-domain  →  { default_domain: "legal" | … | null }
// PUT /user/default-domain  body { default_domain: "<canonical id>" }
//
// User preference for the default professional vertical used by the
// create dialogs (workflow / project / document). Stored on the same
// `user_settings` row as locale. NULL means "no preference yet" — the
// frontend then falls back to the canonical `legal` default that
// matches the per-row schema default from migration 0018. Canonical
// IDs come from `crate::domain::DOMAINS` (English snake_case keys
// like `legal`, `medical`, `real_estate`). UI translation is done at
// display time via the i18n `Domains.values.*` namespace; only the
// raw English ID travels over the wire and lives in the DB.
// ---------------------------------------------------------------------------
async fn get_default_domain(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT default_domain FROM user_settings WHERE user_id = ?",
    )
    .bind(&auth.user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let d = row.and_then(|(d,)| d);
    Ok(Json(json!({ "default_domain": d })))
}

#[derive(Deserialize)]
struct UpdateDefaultDomainBody {
    default_domain: String,
}

async fn update_default_domain(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<UpdateDefaultDomainBody>,
) -> ApiResult {
    if !crate::domain::is_valid(&body.default_domain) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "default_domain must be a canonical English ID (e.g. legal, medical, finance, real_estate, hr, insurance, ip, compliance, others)",
        ));
    }
    sqlx::query(
        "INSERT INTO user_settings (user_id, default_domain, updated_at) \
         VALUES (?, ?, datetime('now')) \
         ON CONFLICT(user_id) DO UPDATE SET default_domain = excluded.default_domain, \
             updated_at = datetime('now')",
    )
    .bind(&auth.user_id)
    .bind(&body.default_domain)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    Ok(Json(json!({ "ok": true, "default_domain": body.default_domain })))
}

// ---------------------------------------------------------------------------
// GET /user/enabled-domains  →  { enabled_domains: ["legal", …] | null }
// PUT /user/enabled-domains  body { enabled_domains: ["legal", …] }
//
// User preference for the subset of professional verticals visible in
// the app. Stored as a JSON array of canonical English IDs in
// `user_settings.enabled_domains`. NULL = "no explicit preference" =
// every domain is visible (backwards-compatible default). The PUT
// handler collapses an explicit "all 10 enabled" payload back to NULL
// so a user resetting the preference doesn't leave a permanent
// snapshot of the current shipped domain set in the DB.
// ---------------------------------------------------------------------------
async fn get_enabled_domains(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT enabled_domains FROM user_settings WHERE user_id = ?",
    )
    .bind(&auth.user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let parsed: Option<Value> = row
        .and_then(|(s,)| s)
        .and_then(|s| serde_json::from_str::<Value>(&s).ok());
    Ok(Json(json!({ "enabled_domains": parsed })))
}

#[derive(Deserialize)]
struct UpdateEnabledDomainsBody {
    enabled_domains: Vec<String>,
}

async fn update_enabled_domains(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<UpdateEnabledDomainsBody>,
) -> ApiResult {
    if body.enabled_domains.is_empty() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "enabled_domains must contain at least one domain",
        ));
    }
    for d in &body.enabled_domains {
        if !crate::domain::is_valid(d) {
            return Err(err(
                StatusCode::BAD_REQUEST,
                &format!("'{d}' is not a canonical domain id"),
            ));
        }
    }
    // Dedup while preserving caller order, then collapse the all-enabled
    // case to NULL so the DB doesn't pin the current shipped set.
    let mut seen = std::collections::HashSet::new();
    let mut domains: Vec<String> = Vec::with_capacity(body.enabled_domains.len());
    for d in body.enabled_domains {
        if seen.insert(d.clone()) {
            domains.push(d);
        }
    }
    let store_as: Option<String> = if domains.len() == crate::domain::DOMAINS.len() {
        None
    } else {
        Some(serde_json::to_string(&domains).map_err(|e| {
            err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
        })?)
    };
    sqlx::query(
        "INSERT INTO user_settings (user_id, enabled_domains, updated_at) \
         VALUES (?, ?, datetime('now')) \
         ON CONFLICT(user_id) DO UPDATE SET enabled_domains = excluded.enabled_domains, \
             updated_at = datetime('now')",
    )
    .bind(&auth.user_id)
    .bind(&store_as)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    Ok(Json(json!({
        "ok": true,
        "enabled_domains": if store_as.is_some() { json!(domains) } else { Value::Null },
    })))
}

// ---------------------------------------------------------------------------
// GET /user/hyde-enabled  →  { hyde_enabled: bool }
// PUT /user/hyde-enabled  body { hyde_enabled: bool }
//
// Opt-in switch for the HyDE (Hypothetical Document Embeddings)
// retrieval pass (migration 0030). When ON, `retrieve_kb_chunks` in
// chat.rs fires one extra LLM call to draft a domain-aware pseudo-
// answer, embeds both the query and the hypothesis, and merges the
// two KNN rankings via Reciprocal Rank Fusion before applying the
// usual top-K + distance threshold.
//
// First panel of the future "Recupero documenti" Settings section —
// dedicated endpoint (instead of bundling into /llm-settings) so the
// UI toggle keeps a clean small surface and future RAG knobs
// (adaptive top-K, BM25+RRF weight, MMR λ) can each get their own.
// ---------------------------------------------------------------------------
async fn get_hyde_enabled(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT hyde_enabled FROM user_settings WHERE user_id = ?",
    )
    .bind(&auth.user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let enabled = row.map(|(v,)| v != 0).unwrap_or(false);
    Ok(Json(json!({ "hyde_enabled": enabled })))
}

#[derive(Deserialize)]
struct UpdateHydeEnabledBody {
    hyde_enabled: bool,
}

async fn update_hyde_enabled(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<UpdateHydeEnabledBody>,
) -> ApiResult {
    let value: i64 = if body.hyde_enabled { 1 } else { 0 };
    sqlx::query(
        "INSERT INTO user_settings (user_id, hyde_enabled, updated_at) \
         VALUES (?, ?, datetime('now')) \
         ON CONFLICT(user_id) DO UPDATE SET hyde_enabled = excluded.hyde_enabled, \
             updated_at = datetime('now')",
    )
    .bind(&auth.user_id)
    .bind(value)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    tracing::info!(
        "[user] PUT /hyde-enabled user={} enabled={}",
        auth.user_id,
        body.hyde_enabled,
    );
    Ok(Json(json!({ "ok": true, "hyde_enabled": body.hyde_enabled })))
}

// ---------------------------------------------------------------------------
// GET /user/profile
// ---------------------------------------------------------------------------
async fn get_profile(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    let row: Option<(String, String, Option<String>, String)> =
        sqlx::query_as(
            "SELECT id, username, display_name, created_at FROM user_profiles WHERE id = ?",
        )
        .bind(&auth.user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (id, username, display_name, created_at) =
        row.ok_or_else(|| err(StatusCode::NOT_FOUND, "Profile not found"))?;

    Ok(Json(json!({
        "id": id,
        "username": username,
        "display_name": display_name,
        "created_at": created_at,
    })))
}

// ---------------------------------------------------------------------------
// PUT /user/profile
// Body: { display_name? }
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct UpdateProfileBody {
    display_name: Option<String>,
}

async fn update_profile(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<UpdateProfileBody>,
) -> ApiResult {
    sqlx::query("UPDATE user_profiles SET display_name = ? WHERE id = ?")
        .bind(&body.display_name)
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// GET /user/llm-settings
// ---------------------------------------------------------------------------
#[derive(Default, Serialize, Deserialize)]
pub struct LlmSettings {
    pub main_model: Option<String>,
    pub title_model: Option<String>,
    pub tabular_model: Option<String>,
    pub claude_api_key: Option<String>,
    pub gemini_api_key: Option<String>,
    pub gemini_region: Option<String>,
    pub gemini_model: Option<String>,
    pub openai_api_key: Option<String>,
    pub openai_model: Option<String>,
    pub local_base_url: Option<String>,
    pub local_api_key: Option<String>,
    pub local_model: Option<String>,
    pub active_provider: Option<String>,
    pub mistral_api_key: Option<String>,
    pub mistral_model: Option<String>,
    /// Opt-in HyDE (Hypothetical Document Embeddings) at chat-time
    /// retrieval. Persisted in `user_settings.hyde_enabled` (migration
    /// 0030). Toggled from Settings → Recupero documenti. Default OFF
    /// because the technique adds one extra LLM call per chat turn.
    pub hyde_enabled: bool,
    /// "Modalità sicura locale" — when ON the local provider only
    /// talks to loopback and only accepts the models of the local-models
    /// catalogue (`config/local-models/ollama.json`). Persisted in
    /// `user_settings.local_secure_mode` (migration 0032). Toggled
    /// from Settings → Modelli LLM. Default OFF for retro-compat on
    /// existing installs.
    pub local_secure_mode: bool,
    /// Mistral `safe_prompt` request parameter (migration 0033).
    /// OFF by default — the Mistral safety wrapper false-flags
    /// legitimate legal content. Toggled from Settings → Modelli
    /// LLM → Mistral AI.
    pub mistral_safe_prompt: bool,
    /// Mistral `parallel_tool_calls` (migration 0033). OFF by
    /// default — sequential tool execution is more predictable in
    /// legal workflows. Toggled from Settings → Modelli LLM →
    /// Mistral AI.
    pub mistral_parallel_tools: bool,
}

// We're past sqlx's 16-element `FromRow` tuple limit (added
// local_secure_mode in v0.5.6 brings us to 17 columns; v0.6.0
// migration 0033 added two more), so switch to a #[derive(FromRow)]
// struct with the field names matching the SQL column names exactly.
#[derive(sqlx::FromRow)]
struct LlmRow {
    main_model: Option<String>,
    title_model: Option<String>,
    tabular_model: Option<String>,
    claude_api_key: Option<String>,
    gemini_api_key: Option<String>,
    gemini_region: Option<String>,
    gemini_model: Option<String>,
    openai_api_key: Option<String>,
    openai_model: Option<String>,
    local_base_url: Option<String>,
    local_api_key: Option<String>,
    local_model: Option<String>,
    active_provider: Option<String>,
    mistral_api_key: Option<String>,
    mistral_model: Option<String>,
    hyde_enabled: i64,
    local_secure_mode: i64,
    mistral_safe_prompt: i64,
    mistral_parallel_tools: i64,
}

const SELECT_COLUMNS: &str =
    "main_model, title_model, tabular_model, claude_api_key, gemini_api_key, gemini_region, \
     gemini_model, openai_api_key, openai_model, local_base_url, local_api_key, local_model, \
     active_provider, mistral_api_key, mistral_model, hyde_enabled, local_secure_mode, \
     mistral_safe_prompt, mistral_parallel_tools";

pub async fn fetch_llm_settings(
    db: &sqlx::SqlitePool,
    user_id: &str,
) -> Result<LlmSettings, sqlx::Error> {
    let row: Option<LlmRow> = sqlx::query_as(&format!(
        "SELECT {SELECT_COLUMNS} FROM user_settings WHERE user_id = ?"
    ))
    .bind(user_id)
    .fetch_optional(db)
    .await?;

    Ok(row.map(row_to_settings).unwrap_or_default())
}

fn row_to_settings(r: LlmRow) -> LlmSettings {
    LlmSettings {
        main_model: r.main_model,
        title_model: r.title_model,
        tabular_model: r.tabular_model,
        claude_api_key: r.claude_api_key,
        gemini_api_key: r.gemini_api_key,
        gemini_region: r.gemini_region,
        gemini_model: r.gemini_model,
        openai_api_key: r.openai_api_key,
        openai_model: r.openai_model,
        local_base_url: r.local_base_url,
        local_api_key: r.local_api_key,
        local_model: r.local_model,
        active_provider: r.active_provider,
        mistral_api_key: r.mistral_api_key,
        mistral_model: r.mistral_model,
        hyde_enabled: r.hyde_enabled != 0,
        local_secure_mode: r.local_secure_mode != 0,
        mistral_safe_prompt: r.mistral_safe_prompt != 0,
        mistral_parallel_tools: r.mistral_parallel_tools != 0,
    }
}

async fn get_llm_settings(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    let settings = fetch_llm_settings(&state.db, &auth.user_id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    tracing::info!(
        "[user] GET /llm-settings user={} has_gemini_key={} gemini_model={:?} gemini_region={:?} active={:?}",
        auth.user_id,
        settings.gemini_api_key.is_some(),
        settings.gemini_model,
        settings.gemini_region,
        settings.active_provider,
    );
    Ok(Json(serde_json::to_value(settings).unwrap()))
}

// ---------------------------------------------------------------------------
// PUT /user/llm-settings
//
// Patch semantics: every Option<String> field is "unchanged when absent
// or null". This is critical for API keys — the client must be able to
// save other settings (e.g. region, model) without the user re-typing
// the API key. The UPDATE branch uses COALESCE(?, column) so a NULL
// bind keeps the existing value; the INSERT branch (first save) writes
// whatever was provided. To explicitly *clear* a key the client sends
// an empty string (which we treat as null at read time).
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct UpdateLlmSettingsBody {
    #[serde(default)] main_model: Option<String>,
    #[serde(default)] title_model: Option<String>,
    #[serde(default)] tabular_model: Option<String>,
    #[serde(default)] claude_api_key: Option<String>,
    #[serde(default)] gemini_api_key: Option<String>,
    #[serde(default)] gemini_region: Option<String>,
    #[serde(default)] gemini_model: Option<String>,
    #[serde(default)] openai_api_key: Option<String>,
    #[serde(default)] openai_model: Option<String>,
    #[serde(default)] local_base_url: Option<String>,
    #[serde(default)] local_api_key: Option<String>,
    #[serde(default)] local_model: Option<String>,
    #[serde(default)] active_provider: Option<String>,
    #[serde(default)] mistral_api_key: Option<String>,
    #[serde(default)] mistral_model: Option<String>,
    /// v0.5.6 "Modalità sicura locale" toggle. The Settings UI sends
    /// this whenever the user flips the switch; absent → leave
    /// whatever was in the DB (typical for partial saves of other
    /// fields).
    #[serde(default)] local_secure_mode: Option<bool>,
    /// v0.6.0 Mistral provider options. Migration 0033.
    #[serde(default)] mistral_safe_prompt: Option<bool>,
    #[serde(default)] mistral_parallel_tools: Option<bool>,
}

async fn update_llm_settings(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<UpdateLlmSettingsBody>,
) -> ApiResult {
    tracing::info!(
        "[user] PUT /llm-settings user={} dirty={{openai_key:{},claude_key:{},gemini_key:{},gemini_model:{:?},gemini_region:{:?},local_base_url:{:?},local_model:{:?},active_provider:{:?}}}",
        auth.user_id,
        body.openai_api_key.is_some(),
        body.claude_api_key.is_some(),
        body.gemini_api_key.is_some(),
        body.gemini_model,
        body.gemini_region,
        body.local_base_url,
        body.local_model,
        body.active_provider,
    );
    // Two-step upsert:
    //  1. INSERT OR IGNORE → seeds an empty row for first-time users
    //     without clobbering existing values.
    //  2. UPDATE with COALESCE(?, col) → only writes the columns the
    //     client actually sent; absent fields retain whatever was there.
    sqlx::query("INSERT OR IGNORE INTO user_settings (user_id, updated_at) VALUES (?, datetime('now'))")
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let secure_value: Option<i64> = body.local_secure_mode.map(|b| if b { 1 } else { 0 });
    let mistral_safe_value: Option<i64> = body.mistral_safe_prompt.map(|b| if b { 1 } else { 0 });
    let mistral_parallel_value: Option<i64> =
        body.mistral_parallel_tools.map(|b| if b { 1 } else { 0 });
    sqlx::query(
        "UPDATE user_settings SET \
            main_model             = COALESCE(?, main_model), \
            title_model            = COALESCE(?, title_model), \
            tabular_model          = COALESCE(?, tabular_model), \
            claude_api_key         = COALESCE(?, claude_api_key), \
            gemini_api_key         = COALESCE(?, gemini_api_key), \
            gemini_region          = COALESCE(?, gemini_region), \
            gemini_model           = COALESCE(?, gemini_model), \
            openai_api_key         = COALESCE(?, openai_api_key), \
            openai_model           = COALESCE(?, openai_model), \
            local_base_url         = COALESCE(?, local_base_url), \
            local_api_key          = COALESCE(?, local_api_key), \
            local_model            = COALESCE(?, local_model), \
            active_provider        = COALESCE(?, active_provider), \
            mistral_api_key        = COALESCE(?, mistral_api_key), \
            mistral_model          = COALESCE(?, mistral_model), \
            local_secure_mode      = COALESCE(?, local_secure_mode), \
            mistral_safe_prompt    = COALESCE(?, mistral_safe_prompt), \
            mistral_parallel_tools = COALESCE(?, mistral_parallel_tools), \
            updated_at             = datetime('now') \
         WHERE user_id = ?",
    )
    .bind(&body.main_model)
    .bind(&body.title_model)
    .bind(&body.tabular_model)
    .bind(&body.claude_api_key)
    .bind(&body.gemini_api_key)
    .bind(&body.gemini_region)
    .bind(&body.gemini_model)
    .bind(&body.openai_api_key)
    .bind(&body.openai_model)
    .bind(&body.local_base_url)
    .bind(&body.local_api_key)
    .bind(&body.local_model)
    .bind(&body.active_provider)
    .bind(&body.mistral_api_key)
    .bind(&body.mistral_model)
    .bind(secure_value)
    .bind(mistral_safe_value)
    .bind(mistral_parallel_value)
    .bind(&auth.user_id)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// MCP servers — per-user configurations
//
// Schema mirrors Anthropic's `claude_desktop_config.json`:
//   stdio servers → { command, args, env } (transport: "stdio")
//   remote (HTTP/SSE) → { url, headers, api_key } (transport: "http"|"sse")
// ---------------------------------------------------------------------------
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct McpServerOut {
    pub name: String,
    pub transport: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: serde_json::Map<String, Value>,
    #[serde(default)]
    pub headers: serde_json::Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    pub enabled: bool,
}

fn row_to_server(
    name: String,
    transport: String,
    url: Option<String>,
    command: Option<String>,
    args_json: String,
    env_json: String,
    headers_json: String,
    api_key: Option<String>,
    enabled: i64,
) -> McpServerOut {
    let args: Vec<String> = serde_json::from_str(&args_json).unwrap_or_default();
    let env: serde_json::Map<String, Value> =
        serde_json::from_str(&env_json).unwrap_or_default();
    let headers: serde_json::Map<String, Value> =
        serde_json::from_str(&headers_json).unwrap_or_default();
    McpServerOut {
        name,
        transport,
        url,
        command,
        args,
        env,
        headers,
        api_key,
        enabled: enabled != 0,
    }
}

pub async fn fetch_mcp_servers(
    db: &sqlx::SqlitePool,
    user_id: &str,
) -> Result<Vec<McpServerOut>, sqlx::Error> {
    let rows: Vec<(String, String, Option<String>, Option<String>, String, String, String, Option<String>, i64)> =
        sqlx::query_as(
            "SELECT name, transport, url, command, args_json, env_json, headers_json, api_key, enabled \
             FROM mcp_servers WHERE user_id = ? ORDER BY name ASC",
        )
        .bind(user_id)
        .fetch_all(db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(n, t, u, c, a, e, h, k, en)| row_to_server(n, t, u, c, a, e, h, k, en))
        .collect())
}

async fn list_mcp_servers(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    let servers = fetch_mcp_servers(&state.db, &auth.user_id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    Ok(Json(json!({ "servers": servers })))
}

#[derive(Deserialize)]
struct UpsertMcpBody {
    name: String,
    #[serde(default = "default_transport")]
    transport: String,
    url: Option<String>,
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: serde_json::Map<String, Value>,
    #[serde(default)]
    headers: serde_json::Map<String, Value>,
    api_key: Option<String>,
    #[serde(default = "default_enabled")]
    enabled: bool,
}
fn default_transport() -> String { "http".to_string() }
fn default_enabled() -> bool { true }

async fn upsert_mcp_server(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<UpsertMcpBody>,
) -> ApiResult {
    upsert_mcp_inner(state, auth, None, body).await
}

async fn upsert_mcp_server_named(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    axum::extract::Path(path_name): axum::extract::Path<String>,
    Json(body): Json<UpsertMcpBody>,
) -> ApiResult {
    upsert_mcp_inner(state, auth, Some(path_name), body).await
}

async fn upsert_mcp_inner(
    state: Arc<AppState>,
    auth: AuthUser,
    rename_from: Option<String>,
    body: UpsertMcpBody,
) -> ApiResult {
    let target_name = body.name.trim().to_string();
    if target_name.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "Server name cannot be empty"));
    }
    let transport = match body.transport.as_str() {
        "http" | "sse" | "stdio" => body.transport.clone(),
        other => return Err(err(StatusCode::BAD_REQUEST, &format!("Unsupported transport: {other}"))),
    };
    if transport == "stdio" {
        if body.command.as_deref().map(|s| s.trim().is_empty()).unwrap_or(true) {
            return Err(err(StatusCode::BAD_REQUEST, "stdio server requires `command`"));
        }
    } else {
        if body.url.as_deref().map(|s| s.trim().is_empty()).unwrap_or(true) {
            return Err(err(StatusCode::BAD_REQUEST, "http/sse server requires `url`"));
        }
    }

    let args_json = serde_json::to_string(&body.args).unwrap_or_else(|_| "[]".into());
    let env_json = serde_json::to_string(&body.env).unwrap_or_else(|_| "{}".into());
    let headers_json = serde_json::to_string(&body.headers).unwrap_or_else(|_| "{}".into());

    // If renaming, drop the old row first.
    if let Some(old) = rename_from.as_ref().filter(|n| n != &&target_name) {
        let _ = sqlx::query("DELETE FROM mcp_servers WHERE user_id = ? AND name = ?")
            .bind(&auth.user_id)
            .bind(old)
            .execute(&state.db)
            .await;
    }

    sqlx::query(
        "INSERT INTO mcp_servers (\
            user_id, name, transport, url, command, args_json, env_json, headers_json, api_key, enabled, updated_at\
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now')) \
         ON CONFLICT(user_id, name) DO UPDATE SET \
           transport    = excluded.transport, \
           url          = excluded.url, \
           command      = excluded.command, \
           args_json    = excluded.args_json, \
           env_json     = excluded.env_json, \
           headers_json = excluded.headers_json, \
           api_key      = excluded.api_key, \
           enabled      = excluded.enabled, \
           updated_at   = excluded.updated_at",
    )
    .bind(&auth.user_id)
    .bind(&target_name)
    .bind(&transport)
    .bind(&body.url)
    .bind(&body.command)
    .bind(&args_json)
    .bind(&env_json)
    .bind(&headers_json)
    .bind(&body.api_key)
    .bind(if body.enabled { 1 } else { 0 })
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    // Drop the chat handler's MCP discovery cache for this user — the
    // server config just changed, the cached `tools/list` snapshot is
    // probably stale (different URL, different auth, possibly disabled).
    state.invalidate_mcp_cache_for_user(&auth.user_id).await;

    Ok(Json(json!({ "ok": true, "name": target_name })))
}

// ---------------------------------------------------------------------------
// POST /user/mcp-servers/probe
// Body: { url, api_key?, headers? }
// Performs the MCP `initialize` handshake and `tools/list` discovery.
// Auto-detects transport: tries HTTP POST first, falls back to SSE on 405.
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct ProbeBody {
    url: String,
    api_key: Option<String>,
    #[serde(default)]
    headers: serde_json::Map<String, Value>,
}

async fn probe_mcp_server(
    _state: State<Arc<AppState>>,
    _auth: AuthUser,
    Json(body): Json<ProbeBody>,
) -> ApiResult {
    let url = body.url.trim().to_string();
    if url.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "url is required"));
    }

    let mut req_headers = reqwest::header::HeaderMap::new();
    req_headers.insert("Content-Type", "application/json".parse().unwrap());
    req_headers.insert("Accept", "application/json, text/event-stream".parse().unwrap());
    if let Some(key) = body.api_key.as_ref().filter(|k| !k.trim().is_empty()) {
        if let Ok(v) = format!("Bearer {key}").parse() {
            req_headers.insert("Authorization", v);
        }
    }
    for (k, v) in &body.headers {
        if let (Ok(name), Some(val_str), Ok(val_hv)) = (
            reqwest::header::HeaderName::from_bytes(k.as_bytes()),
            v.as_str(),
            v.as_str().unwrap_or("").parse::<reqwest::header::HeaderValue>(),
        ) {
            let _ = val_str;
            req_headers.insert(name, val_hv);
        }
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let init_body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": crate::product::NAME, "version": env!("CARGO_PKG_VERSION") }
        }
    });

    // Try a base URL plus a small set of well-known MCP suffixes (some
    // servers mount the JSON-RPC handler on `/mcp` or `/messages` rather
    // than the root). Returns the first URL that yields a 2xx initialize.
    async fn try_initialize_with_fallback(
        client: &reqwest::Client,
        base_url: &str,
        headers: &reqwest::header::HeaderMap,
        init_body: &Value,
    ) -> (String, Result<reqwest::Response, reqwest::Error>) {
        let trimmed = base_url.trim_end_matches('/').to_string();
        // Original URL first; only try fallbacks if root path is "/" or empty.
        let url_obj = url::Url::parse(base_url).ok();
        let path_is_root = url_obj
            .as_ref()
            .map(|u| u.path().is_empty() || u.path() == "/")
            .unwrap_or(false);

        let candidates: Vec<String> = if path_is_root {
            vec![
                base_url.to_string(),
                format!("{trimmed}/mcp"),
                format!("{trimmed}/messages"),
                format!("{trimmed}/api/mcp"),
            ]
        } else {
            vec![base_url.to_string()]
        };

        let mut last_resp: Option<(String, Result<reqwest::Response, reqwest::Error>)> = None;
        for candidate in candidates {
            let resp = client
                .post(&candidate)
                .headers(headers.clone())
                .json(init_body)
                .send()
                .await;
            match &resp {
                Ok(r) if r.status().is_success() => {
                    return (candidate, resp);
                }
                Ok(r) if r.status().as_u16() == 401 || r.status().as_u16() == 403 => {
                    // Auth required at this path — that's a strong signal it's the right path.
                    return (candidate, resp);
                }
                _ => {
                    last_resp = Some((candidate, resp));
                }
            }
        }
        last_resp.expect("at least one candidate")
    }

    let (matched_url, resp) =
        try_initialize_with_fallback(&client, &url, &req_headers, &init_body).await;
    let resp = match resp {
        Ok(r) => r,
        Err(e) => return Err(err(StatusCode::BAD_GATEWAY, &format!("connection failed: {e}"))),
    };

    let status = resp.status();
    // Capture the session id the server returned — Streamable HTTP MCP
    // requires it on every subsequent request, otherwise the server replies
    // with "Unexpected message, expect initialize request".
    let session_id = resp
        .headers()
        .get("mcp-session-id")
        .or_else(|| resp.headers().get("Mcp-Session-Id"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let detected_transport: &str;
    let init_value: Value;

    let suggested_url = if matched_url != url {
        Some(matched_url.clone())
    } else {
        None
    };

    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(err(
            status,
            &format!(
                "MCP server requires authentication{}. Configure API key or headers and retry.",
                suggested_url
                    .as_ref()
                    .map(|u| format!(" (path discovered: {u})"))
                    .unwrap_or_default()
            ),
        ));
    }
    if status.as_u16() == 405 {
        return Ok(Json(json!({
            "ok": false,
            "transport_detected": "sse",
            "suggested_url": suggested_url,
            "hint": "POST returned 405; this URL appears to use the legacy HTTP+SSE transport (GET /sse + POST messages). Save it with transport=\"sse\" and the tools list will be loaded at runtime."
        })));
    }
    if !status.is_success() {
        let body_text = resp.text().await.unwrap_or_default();
        return Err(err(
            status,
            &format!(
                "MCP error {status} at {matched_url}: {}",
                body_text.chars().take(300).collect::<String>()
            ),
        ));
    }
    detected_transport = "http";

    init_value = read_jsonrpc_response(resp, 1, 8)
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, &e.to_string()))?;

    if let Some(rpc_err) = init_value.get("error") {
        return Err(err(
            StatusCode::BAD_GATEWAY,
            &format!("MCP initialize error: {rpc_err}"),
        ));
    }

    let server_info = init_value["result"]["serverInfo"].clone();
    let capabilities = init_value["result"]["capabilities"].clone();
    // The MCP `initialize` response can include a free-form `instructions`
    // field — the spec analogue of a `skill.md` body: a Markdown explanation
    // of what the server does and how to use it.
    let instructions = init_value["result"]["instructions"].as_str().map(|s| s.to_string());

    // Build the headers used for follow-up requests: same as initialize
    // plus `Mcp-Session-Id` so the server recognises the session.
    let mut session_headers = req_headers.clone();
    if let Some(sid) = session_headers.clone().get("Mcp-Session-Id") {
        // already set by user — leave it
        let _ = sid;
    } else if let Some(sid) = &session_id {
        if let Ok(v) = sid.parse() {
            session_headers.insert("Mcp-Session-Id", v);
        }
    }

    // 2) Send the `notifications/initialized` handshake (no id = notification,
    // server returns 202 Accepted with empty body). Required by the spec
    // before any other request on the same session.
    let _ = client
        .post(&matched_url)
        .headers(session_headers.clone())
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        }))
        .send()
        .await;

    // Helper that calls a JSON-RPC method against the URL we just initialized
    // and returns the array under `result.{key}`, ignoring failures.
    async fn list_method(
        client: &reqwest::Client,
        url: &str,
        headers: &reqwest::header::HeaderMap,
        method: &str,
        result_key: &str,
        id: u64,
    ) -> Vec<Value> {
        let resp = client
            .post(url)
            .headers(headers.clone())
            .json(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": {}
            }))
            .send()
            .await;
        let Ok(r) = resp else { return Vec::new() };
        let Ok(v) = read_jsonrpc_response(r, id, 8).await else { return Vec::new() };
        v["result"][result_key].as_array().cloned().unwrap_or_default()
    }

    // 3) Discover tools, prompts (skill-like templates), resources — all on
    // the same session.
    let raw_tools = list_method(&client, &matched_url, &session_headers, "tools/list", "tools", 2).await;
    let raw_prompts = list_method(&client, &matched_url, &session_headers, "prompts/list", "prompts", 3).await;
    let raw_resources = list_method(&client, &matched_url, &session_headers, "resources/list", "resources", 4).await;

    let tools: Vec<Value> = raw_tools
        .into_iter()
        .map(|t| json!({
            "name": t["name"].as_str().unwrap_or(""),
            "description": t["description"].as_str().unwrap_or(""),
        }))
        .collect();

    let prompts: Vec<Value> = raw_prompts
        .into_iter()
        .map(|p| {
            // Each prompt may declare `arguments: [{name, description, required}]`
            let args: Vec<Value> = p["arguments"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|a| json!({
                    "name": a["name"].as_str().unwrap_or(""),
                    "description": a["description"].as_str().unwrap_or(""),
                    "required": a["required"].as_bool().unwrap_or(false),
                }))
                .collect();
            json!({
                "name": p["name"].as_str().unwrap_or(""),
                "description": p["description"].as_str().unwrap_or(""),
                "arguments": args,
            })
        })
        .collect();

    let resources: Vec<Value> = raw_resources
        .into_iter()
        .map(|r| json!({
            "uri": r["uri"].as_str().unwrap_or(""),
            "name": r["name"].as_str().unwrap_or(""),
            "description": r["description"].as_str().unwrap_or(""),
            "mimeType": r["mimeType"].as_str().unwrap_or(""),
        }))
        .collect();

    Ok(Json(json!({
        "ok": true,
        "transport_detected": detected_transport,
        "suggested_url": suggested_url,
        "server_info": server_info,
        "capabilities": capabilities,
        "instructions": instructions,
        "tools": tools,
        "tool_count": tools.len(),
        "prompts": prompts,
        "prompt_count": prompts.len(),
        "resources": resources,
        "resource_count": resources.len(),
    })))
}

/// Read a JSON-RPC response from a Streamable-HTTP MCP endpoint.
///
/// MCP servers using the Streamable HTTP transport often respond with an SSE
/// stream that may begin with keep-alive frames before the actual JSON-RPC
/// payload, and may stay open afterwards to push server→client notifications.
/// This means `Response::text()` would block until either the connection is
/// closed or the global request timeout elapses — too slow for an interactive
/// "Test & detect" probe.
///
/// Instead we incrementally read chunks, look for `data: {…}` lines, and
/// return as soon as we find a JSON-RPC envelope whose `id` matches the
/// expected request id. Pure-JSON (non-SSE) responses are also handled.
pub async fn read_jsonrpc_response(
    resp: reqwest::Response,
    expect_id: u64,
    max_secs: u64,
) -> Result<Value, anyhow::Error> {
    use futures_util::StreamExt;
    use tokio::time::{timeout, Duration, Instant};

    let deadline = Instant::now() + Duration::from_secs(max_secs);
    let started_at = Instant::now();
    // Periodic "still waiting" log when an SSE stream is silent for a
    // while — useful for tools like Edge's pseudonymise-with-approval
    // where the server holds the connection open while a human clicks
    // "Conferma" in their UI. Without this log, the dispatch appeared
    // to hang silently for minutes; now it's clear we're alive and
    // waiting on the server.
    let mut next_heartbeat = started_at + Duration::from_secs(15);
    let mut chunk_count = 0usize;
    let mut bytes_received = 0usize;

    let mut buf = String::new();
    let mut stream = resp.bytes_stream();

    loop {
        let now = Instant::now();
        if now >= deadline { break; }
        let remaining = deadline.duration_since(now);

        // Wake every 15 s (or remaining, whichever is shorter) so we
        // can emit a heartbeat log even if the stream is silent.
        let wait = std::cmp::min(
            remaining,
            next_heartbeat.saturating_duration_since(now).max(Duration::from_millis(1)),
        );

        match timeout(wait, stream.next()).await {
            Err(_) => {
                // Wait timed out — but is it the heartbeat or the
                // overall deadline? If we still have time left, log
                // a heartbeat and keep going.
                if Instant::now() < deadline {
                    let elapsed_secs = started_at.elapsed().as_secs();
                    tracing::info!(
                        "[mcp/sse] still waiting on response… ({}s elapsed, {} chunks, {} bytes received so far, deadline at {}s)",
                        elapsed_secs, chunk_count, bytes_received, max_secs
                    );
                    next_heartbeat = Instant::now() + Duration::from_secs(15);
                    continue;
                }
                break;                                  // real overall timeout
            }
            Ok(None) => break,                          // stream ended
            Ok(Some(Err(e))) => return Err(anyhow::anyhow!("stream error: {e}")),
            Ok(Some(Ok(bytes))) => {
                chunk_count += 1;
                bytes_received += bytes.len();
                tracing::debug!(
                    "[mcp/sse] chunk #{}: +{} bytes (total {} bytes, {}s elapsed)",
                    chunk_count, bytes.len(), bytes_received, started_at.elapsed().as_secs()
                );
                buf.push_str(&String::from_utf8_lossy(&bytes));

                // Pure-JSON response (e.g. when server doesn't use SSE).
                let trimmed = buf.trim_start();
                if trimmed.starts_with('{') {
                    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
                        return Ok(v);
                    }
                }

                // SSE: scan all complete `data:` lines we have so far.
                for line in buf.lines() {
                    let l = line.trim();
                    if let Some(rest) = l.strip_prefix("data:") {
                        let data = rest.trim();
                        if data.is_empty() || data == "[DONE]" { continue; }
                        if let Ok(v) = serde_json::from_str::<Value>(data) {
                            // Match by id when one is present; otherwise
                            // accept the first parseable payload (some
                            // servers omit the id on errors).
                            let id_match = v
                                .get("id")
                                .and_then(|i| i.as_u64())
                                .map(|i| i == expect_id)
                                .unwrap_or(true);
                            if id_match {
                                return Ok(v);
                            }
                        }
                    }
                }
            }
        }
    }

    // Final attempt on whatever we've accumulated.
    parse_jsonrpc_payload(&buf)
}

/// Parse a Streamable-HTTP MCP response which may be either:
///   - a single JSON object: `{"jsonrpc":"2.0","id":...,"result":...}`
///   - an SSE stream with `data: {...}` lines
fn parse_jsonrpc_payload(raw: &str) -> Result<Value, anyhow::Error> {
    let trimmed = raw.trim_start();
    if trimmed.starts_with('{') {
        return serde_json::from_str(trimmed)
            .map_err(|e| anyhow::anyhow!("invalid JSON: {e}"));
    }
    // SSE: pick the first `data:` line that parses as JSON.
    for line in raw.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("data:") {
            let data = rest.trim();
            if data.is_empty() || data == "[DONE]" { continue; }
            if let Ok(v) = serde_json::from_str::<Value>(data) {
                return Ok(v);
            }
        }
    }
    Err(anyhow::anyhow!("no parseable JSON-RPC payload in response"))
}

async fn delete_mcp_server(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> ApiResult {
    sqlx::query("DELETE FROM mcp_servers WHERE user_id = ? AND name = ?")
        .bind(&auth.user_id)
        .bind(&name)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    state.invalidate_mcp_cache_for_user(&auth.user_id).await;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// /user/local-secure/*  — "Modalità sicura locale"
//
// Backs Settings → Modelli LLM in secure mode: (a) is Ollama reachable on
// loopback, (b) the local-models catalogue with installed / legacy state,
// (c) install / uninstall with streamed progress, (d) migration of models
// created under names from earlier releases and, only after the user
// agrees in the UI, removal of those old names.
//
// Every endpoint requires an authenticated session: they drive the Ollama
// process on the host (pull, copy, delete).
// ---------------------------------------------------------------------------
async fn local_secure_heartbeat(_auth: AuthUser) -> Json<Value> {
    let alive = crate::llm::ollama_manager::heartbeat().await;
    Json(json!({
        "ollama_running": alive,
        "base_url": crate::llm::ollama_manager::SECURE_BASE_URL,
    }))
}

async fn local_secure_models(_auth: AuthUser) -> Json<Value> {
    use crate::llm::ollama_manager::{find_legacy_installs, is_installed, list_installed};

    // Without Ollama the catalogue is still returned, so the UI can show
    // the offline state without another round-trip.
    let installed = list_installed().await.unwrap_or_default();
    let catalogue = crate::presets::local_models::catalogue();
    let legacy = find_legacy_installs(&crate::presets::local_models::name_migrations(), &installed);

    let models: Vec<Value> = catalogue
        .iter()
        .map(|m| {
            let legacy_installed: Vec<&str> = legacy
                .iter()
                .filter(|l| l.current_name == m.id)
                .map(|l| l.installed_name.as_str())
                .collect();
            json!({
                "id": m.id,
                "base_model": m.base_model,
                "display_name": m.display_name,
                "approx_size_gb": m.approx_size_gb,
                "min_ram_gb": m.min_ram_gb,
                // The derivation exists: the base model alone does not
                // give the configured behaviour.
                "ready": is_installed(&installed, &m.id),
                "base_present": is_installed(&installed, &m.base_model),
                "legacy_installed": legacy_installed,
            })
        })
        .collect();
    Json(json!({ "models": models }))
}

async fn local_secure_ensure(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(model_id): Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    let to_sse = |value: Value| {
        Ok::<SseEvent, Infallible>(SseEvent::default().json_data(value).unwrap_or_default())
    };

    if crate::presets::local_models::find(&model_id).is_none() {
        let payload = json!({ "phase": "error", "message": format!("Modello non in catalogo: {model_id}") });
        let stream = futures_util::stream::once(async move { to_sse(payload) }).boxed();
        return Sse::new(stream).keep_alive(KeepAlive::default()).into_response();
    }

    // When the model is ready, settings that still point at one of its
    // legacy names are moved to the current name.
    let sse_stream = crate::llm::ollama_manager::ensure(model_id)
        .then(move |event| {
            let state = state.clone();
            async move {
                if let crate::llm::ollama_manager::EnsureEvent::Ready { model_id } = &event {
                    for migration in crate::presets::local_models::name_migrations()
                        .into_iter()
                        .filter(|m| &m.current == model_id)
                    {
                        rewrite_legacy_model_settings(&state, &migration.legacy, &migration.current).await;
                    }
                }
                to_sse(serde_json::to_value(&event).unwrap_or_else(
                    |_| json!({"phase": "error", "message": "serialise failed"}),
                ))
            }
        })
        .boxed();
    Sse::new(sse_stream).keep_alive(KeepAlive::default()).into_response()
}

async fn local_secure_uninstall(_auth: AuthUser, Path(model_id): Path<String>) -> ApiResult {
    crate::llm::ollama_manager::uninstall(&model_id)
        .await
        .map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

/// Copies models installed under legacy names to their current names
/// (no download, nothing deleted) and updates the settings that referred
/// to the old names. The response lists what still needs the user: names
/// that can be removed and models that must be installed again.
async fn local_secure_migrate(State(state): State<Arc<AppState>>, _auth: AuthUser) -> ApiResult {
    let report = crate::llm::ollama_manager::migrate_legacy()
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, &e.to_string()))?;
    for renamed in &report.migrated {
        rewrite_legacy_model_settings(&state, &renamed.from, &renamed.to).await;
    }
    Ok(Json(serde_json::to_value(&report).unwrap_or_else(|_| json!({}))))
}

#[derive(Deserialize)]
struct RemoveLegacyBody {
    names: Vec<String>,
}

/// Removes legacy model names from Ollama. Called by the UI only after the
/// user has confirmed; names that are not legacy catalogue ids are refused.
async fn local_secure_remove_legacy(_auth: AuthUser, Json(body): Json<RemoveLegacyBody>) -> ApiResult {
    let removed = crate::llm::ollama_manager::remove_legacy(&body.names)
        .await
        .map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
    Ok(Json(json!({ "removed": removed })))
}

/// Points every settings column that references `legacy` at `current`.
/// `local_model` stores the bare name; the role columns store it with the
/// `local:` dispatch prefix. The installed name may carry `:latest`.
async fn rewrite_legacy_model_settings(state: &AppState, legacy: &str, current: &str) {
    let legacy = legacy.strip_suffix(":latest").unwrap_or(legacy);
    let current = current.strip_suffix(":latest").unwrap_or(current);
    let bare = [legacy.to_string(), format!("{legacy}:latest")];
    let prefixed = [format!("local:{legacy}"), format!("local:{legacy}:latest")];
    let current_prefixed = format!("local:{current}");

    let updates: [(&str, &str, &[String; 2]); 4] = [
        ("local_model", current, &bare),
        ("main_model", current_prefixed.as_str(), &prefixed),
        ("title_model", current_prefixed.as_str(), &prefixed),
        ("tabular_model", current_prefixed.as_str(), &prefixed),
    ];
    for (column, value, old) in updates {
        let sql = format!("UPDATE user_settings SET {column} = ? WHERE {column} IN (?, ?)");
        match sqlx::query(&sql)
            .bind(value)
            .bind(&old[0])
            .bind(&old[1])
            .execute(&state.db)
            .await
        {
            Ok(r) if r.rows_affected() > 0 => tracing::info!(
                "[local-secure] settings.{column}: {} row(s) moved from {legacy} to {current}",
                r.rows_affected()
            ),
            Ok(_) => {}
            Err(e) => tracing::warn!("[local-secure] settings.{column} update failed: {e}"),
        }
    }
}

// ---------------------------------------------------------------------------
// DELETE /user/account  — irreversible, deletes all user data via CASCADE
// ---------------------------------------------------------------------------
async fn delete_account(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    sqlx::query("DELETE FROM user_profiles WHERE id = ?")
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({ "ok": true })))
}
