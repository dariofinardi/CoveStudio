//! `/models` — read-only view onto the LLM provider/model/region
//! catalogue loaded from `config/model.json` at startup.
//!
//! Consumed by the Settings → Modelli LLM page to drive the provider
//! sections, the model dropdowns, and the region dropdowns. The shape
//! is the catalogue JSON verbatim (`{ providers: [...] }`).
//!
//! No write endpoint — to change the catalogue, edit `config/model.json`
//! and restart.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::{auth::middleware::AuthUser, AppState};

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list_models))
        .route("/local/probe", get(probe_local_models))
        .route("/context-window", get(context_window))
}

#[derive(Debug, Deserialize)]
struct ContextWindowQuery {
    /// Model id as stored in the settings (`claude-…`, `openai:…`,
    /// `mistral:…`, `local:…`).
    model: String,
    /// Local base URL typed in the form but not saved yet; defaults to the
    /// saved settings.
    #[serde(default)]
    base_url: Option<String>,
    /// Secure local mode as shown in the form; defaults to the saved value.
    #[serde(default)]
    secure: Option<bool>,
}

/// Context window of a model, for the Settings page: for local Ollama
/// models the value comes from the server, otherwise from the catalogue or
/// the built-in estimate. The response says which, and for Ollama also
/// whether the model is loaded, the Modelfile `num_ctx` and the model's
/// maximum.
async fn context_window(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(q): Query<ContextWindowQuery>,
) -> ApiResult {
    let model = q.model.trim();
    if model.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": "model is required" }))));
    }
    let settings = crate::routes::user::fetch_llm_settings(&state.db, &auth.user_id).await.ok();
    let mut local = if model.starts_with("local:") {
        crate::routes::chat::build_local_config(model, settings.as_ref()).or_else(|| {
            q.base_url.as_ref().map(|base| crate::llm::types::LocalConfig {
                base_url: base.clone(),
                api_key: None,
                model: model.trim_start_matches("local:").to_string(),
                secure_mode: q.secure.unwrap_or(false),
            })
        })
    } else {
        None
    };
    if let Some(cfg) = local.as_mut() {
        if let Some(base) = q.base_url.as_ref().filter(|b| !b.trim().is_empty()) {
            cfg.base_url = base.clone();
        }
        if let Some(secure) = q.secure {
            cfg.secure_mode = secure;
        }
        cfg.model = model.trim_start_matches("local:").to_string();
    }
    let report = crate::llm::context_window::describe(
        model,
        local.as_ref(),
        Some(state.model_catalogue.as_ref()),
    )
    .await;
    Ok(Json(json!({ "model": model, "report": report })))
}

async fn list_models(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
) -> ApiResult {
    let payload = serde_json::to_value(state.model_catalogue.as_ref())
        .unwrap_or_else(|_| json!({ "providers": [] }));
    Ok(Json(payload))
}

#[derive(Debug, Deserialize)]
struct LocalProbeQuery {
    /// OpenAI-compatible base URL, e.g. `http://192.168.1.10:11434/v1`
    /// for Ollama or `http://127.0.0.1:8080/v1` for llama.cpp's
    /// `llama-server`. Trailing slash optional; we normalise.
    base: String,
    /// Optional bearer token; some OpenAI-compatible runtimes (LM
    /// Studio with auth, vLLM behind a gateway) reject anonymous
    /// requests. Forwarded as `Authorization: Bearer <key>`.
    #[serde(default)]
    api_key: String,
}

/// Server-side proxy for the Settings → Modelli LLM "list local
/// runtime models" probe. Until v0.3.2 the Svelte page issued the
/// `fetch(${base}/models)` directly from the WebView (origin
/// `http://tauri.localhost`). External Ollama instances rarely send
/// `Access-Control-Allow-Origin: http://tauri.localhost`, so the
/// browser blocked the request with the "No 'Access-Control-Allow-
/// Origin' header is present" message users reported in
/// cove-studio.log.
///
/// Running the request from the backend sidesteps CORS entirely
/// (server-to-server fetch, no Origin header involved). We also
/// surface a typed error shape so the UI can render a clearer message
/// than "Failed to fetch" when the runtime is unreachable.
async fn probe_local_models(
    _auth: AuthUser,
    Query(q): Query<LocalProbeQuery>,
) -> ApiResult {
    let base = q.base.trim().trim_end_matches('/').to_string();
    if base.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "base is required" })),
        ));
    }
    let url = format!("{base}/models");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("client: {e}") })),
            )
        })?;
    let mut req = client.get(&url);
    let key = q.api_key.trim();
    if !key.is_empty() {
        req = req.bearer_auth(key);
    }
    let res = req.send().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": format!("upstream unreachable: {e}") })),
        )
    })?;
    let status = res.status();
    if !status.is_success() {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "error": format!("upstream {status}"),
                "upstream_status": status.as_u16(),
            })),
        ));
    }
    let payload: Value = res.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": format!("invalid json: {e}") })),
        )
    })?;
    Ok(Json(payload))
}
