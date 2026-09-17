//! Health / liveness / readiness endpoint.
//!
//! `GET /healthz` is the single canonical signal of "is the desktop app
//! actually serving?". It returns a JSON envelope with:
//!
//!   - `status`: "ok" if the backend is up at all (the request reached
//!     this handler), "degraded" if one or more subsystems are not
//!     ready but the backend can still serve some routes.
//!   - `uptime_secs`: seconds since the process started.
//!   - `version`: the Cargo.toml package version baked at compile time.
//!   - `db`: pool stats — `size` (current connections) and `idle`.
//!   - `rag`: embedding model status — `ready` / `not-yet` / `failed`
//!     and the cumulative init time when known.
//!   - `presets`: counts loaded at boot (`workflows`, `columns`,
//!     `docx_templates`, `model_providers`).
//!
//! No authentication — this is the kind of probe a Tauri tray icon,
//! a local healthcheck script, or a future systemd unit can hit
//! without holding a session.

use crate::AppState;
use axum::{extract::State, response::IntoResponse, routing::get, Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;

/// Process start time, captured the first time the router is built.
/// `OnceLock` so the first call to `router()` fixes a baseline; the
/// `/healthz` handler reads from this for the `uptime_secs` field.
static PROCESS_START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

pub fn router() -> Router<Arc<AppState>> {
    let _ = PROCESS_START.set(Instant::now());
    Router::new().route("/", get(healthz))
}

async fn healthz(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let uptime_secs = PROCESS_START
        .get()
        .map(|s| s.elapsed().as_secs())
        .unwrap_or(0);

    let db_size = state.db.size();
    let db_idle = state.db.num_idle();

    #[cfg(feature = "rag")]
    let rag = rag_status(&state).await;
    #[cfg(not(feature = "rag"))]
    let rag = json!({ "status": "disabled" });

    let presets = json!({
        "workflows": state.workflow_presets.len(),
        "columns": state.column_presets.len(),
        "docx_templates": state.docx_templates.len(),
        "model_providers": state.model_catalogue.providers.len(),
    });

    let status = match rag.get("status").and_then(|v| v.as_str()) {
        Some("failed") => "degraded",
        _ => "ok",
    };

    Json(json!({
        "status": status,
        "uptime_secs": uptime_secs,
        "version": env!("CARGO_PKG_VERSION"),
        "db": {
            "size": db_size,
            "idle": db_idle,
        },
        "rag": rag,
        "presets": presets,
    }))
}

#[cfg(feature = "rag")]
async fn rag_status(state: &AppState) -> Value {
    use crate::embeddings::service::ModelStatus;
    match state.embeddings.as_ref() {
        None => json!({ "status": "disabled" }),
        Some(svc) => match svc.status_snapshot().await {
            ModelStatus::Idle => json!({ "status": "idle" }),
            ModelStatus::Downloading { downloaded, total, file } => json!({
                "status": "downloading",
                "file": file,
                "downloaded": downloaded,
                "total": total,
            }),
            ModelStatus::Loading => json!({ "status": "loading" }),
            ModelStatus::Ready => json!({ "status": "ready" }),
            ModelStatus::Failed(msg) => json!({
                "status": "failed",
                "error": msg,
            }),
        },
    }
}
