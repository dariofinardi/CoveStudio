use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::{
    auth::{
        biometric,
        middleware::AuthUser,
        pin::{hash_pin, validate_pin_format, verify_pin},
    },
    AppState,
};

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn err(status: StatusCode, msg: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"detail": msg})))
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/status",              get(status))
        .route("/setup",               post(setup))
        .route("/unlock",              post(unlock_pin))
        .route("/unlock-biometric",    post(unlock_biometric))
        .route("/biometric-available", get(biometric_available))
        .route("/biometric-enable",    post(biometric_enable))
        .route("/biometric-disable",   post(biometric_disable))
        .route("/change-pin",          post(change_pin))
        .route("/change-pin-biometric", post(change_pin_biometric))
        .route("/logout",              post(logout))
        .route("/me",                  get(me))
}

// ---------------------------------------------------------------------------
// GET /auth/status
// Returns whether a profile exists and if the app needs first-run setup.
// ---------------------------------------------------------------------------
async fn status(State(state): State<Arc<AppState>>) -> Json<Value> {
    // display_name belongs in this response so the client can repopulate
    // the user header on cold start without a second round-trip — the
    // frontend treats /auth/status as "what does the app know about me
    // right now" and was previously losing the nickname after a restart
    // because it wasn't selected here.
    let row: Option<(String, String, Option<String>, i64)> = sqlx::query_as(
        "SELECT id, username, display_name, biometric_enrolled FROM user_profiles LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None);

    match row {
        None => Json(json!({ "setup_required": true })),
        Some((id, username, display_name, bio)) => Json(json!({
            "setup_required": false,
            "user": {
                "id": id,
                "username": username,
                "display_name": display_name,
            },
            "biometric_enrolled": bio == 1,
        })),
    }
}

// ---------------------------------------------------------------------------
// POST /auth/setup  — first-run: create the local profile
// Body: { username, pin, display_name? }
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct SetupBody {
    username: String,
    pin: String,
    display_name: Option<String>,
}

async fn setup(State(state): State<Arc<AppState>>, Json(body): Json<SetupBody>) -> ApiResult {
    // Only one profile allowed in local mode
    let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM user_profiles LIMIT 1")
        .fetch_optional(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    if exists.is_some() {
        return Err(err(StatusCode::CONFLICT, "Profile already exists. Use /auth/unlock."));
    }

    let username = body.username.trim().to_string();
    if username.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "Username cannot be empty"));
    }
    if !validate_pin_format(&body.pin) {
        return Err(err(StatusCode::BAD_REQUEST, "PIN must be 4–8 digits"));
    }

    let pin_hash = hash_pin(&body.pin)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let user_id = uuid::Uuid::new_v4().to_string();

    sqlx::query(
        "INSERT INTO user_profiles (id, username, display_name, pin_hash) VALUES (?, ?, ?, ?)",
    )
    .bind(&user_id)
    .bind(&username)
    .bind(&body.display_name)
    .bind(&pin_hash)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let token = state
        .sessions
        .create(&user_id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({
        "token": token,
        "user": {
            "id": user_id,
            "username": username,
            "display_name": body.display_name,
        }
    })))
}

// ---------------------------------------------------------------------------
// POST /auth/unlock  — PIN login
// Body: { pin }
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct UnlockBody {
    pin: String,
}

async fn unlock_pin(
    State(state): State<Arc<AppState>>,
    Json(body): Json<UnlockBody>,
) -> ApiResult {
    let row: Option<(String, String, Option<String>, String)> = sqlx::query_as(
        "SELECT id, username, display_name, pin_hash FROM user_profiles LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (user_id, username, display_name, pin_hash) =
        row.ok_or_else(|| err(StatusCode::NOT_FOUND, "No profile found. Run setup first."))?;

    let valid = verify_pin(&body.pin, &pin_hash)
        .unwrap_or(false);
    if !valid {
        return Err(err(StatusCode::UNAUTHORIZED, "Wrong PIN"));
    }

    let token = state
        .sessions
        .create(&user_id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({
        "token": token,
        "user": {
            "id": user_id,
            "username": username,
            "display_name": display_name,
        }
    })))
}

// ---------------------------------------------------------------------------
// GET /auth/biometric-available
// ---------------------------------------------------------------------------
async fn biometric_available(State(state): State<Arc<AppState>>) -> Json<Value> {
    let available = biometric::is_available().await;
    let enabled: Option<(i64,)> =
        sqlx::query_as("SELECT biometric_enrolled FROM user_profiles LIMIT 1")
            .fetch_optional(&state.db)
            .await
            .unwrap_or(None);
    let enrolled = enabled.map(|(v,)| v == 1).unwrap_or(false);
    Json(json!({ "available": available, "enabled": enrolled }))
}

// ---------------------------------------------------------------------------
// POST /auth/unlock-biometric  — Windows Hello / Touch ID
// ---------------------------------------------------------------------------
async fn unlock_biometric(State(state): State<Arc<AppState>>) -> ApiResult {
    tracing::info!("[auth] POST /auth/unlock-biometric called");
    let row: Option<(String, String, Option<String>, i64)> = sqlx::query_as(
        "SELECT id, username, display_name, biometric_enrolled FROM user_profiles LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (user_id, username, display_name, enrolled) =
        row.ok_or_else(|| err(StatusCode::NOT_FOUND, "No profile found"))?;

    tracing::info!("[auth] user='{}' enrolled={}", username, enrolled);
    tracing::info!("[auth] bio channel present: {}", state.biometric_tx.is_some());

    if enrolled == 0 {
        tracing::warn!("[auth] biometric not enrolled, returning 400");
        return Err(err(StatusCode::BAD_REQUEST, "Biometric not enrolled for this profile"));
    }

    tracing::info!("[auth] calling bio_verify...");
    let verified = bio_verify(&state, &format!("Unlock {}", crate::product::NAME))
        .await
        .map_err(|e| { tracing::error!("[auth] bio_verify error: {e}"); err(StatusCode::SERVICE_UNAVAILABLE, &e.to_string()) })?;

    tracing::info!("[auth] bio_verify result: {verified}");
    if !verified {
        return Err(err(StatusCode::UNAUTHORIZED, "Biometric verification failed or cancelled"));
    }

    let token = state
        .sessions
        .create(&user_id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({
        "token": token,
        "user": {
            "id": user_id,
            "username": username,
            "display_name": display_name,
        }
    })))
}

// ---------------------------------------------------------------------------
// POST /auth/change-pin  — requires auth
// Body: { current_pin, new_pin }
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct ChangePinBody {
    current_pin: String,
    new_pin: String,
}

async fn change_pin(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<ChangePinBody>,
) -> ApiResult {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT pin_hash FROM user_profiles WHERE id = ?")
            .bind(&auth.user_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (pin_hash,) = row.ok_or_else(|| err(StatusCode::NOT_FOUND, "Profile not found"))?;

    let valid = verify_pin(&body.current_pin, &pin_hash).unwrap_or(false);
    if !valid {
        return Err(err(StatusCode::UNAUTHORIZED, "Current PIN is incorrect"));
    }
    if !validate_pin_format(&body.new_pin) {
        return Err(err(StatusCode::BAD_REQUEST, "New PIN must be 4–8 digits"));
    }

    let new_hash = hash_pin(&body.new_pin)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    sqlx::query("UPDATE user_profiles SET pin_hash = ? WHERE id = ?")
        .bind(&new_hash)
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// POST /auth/change-pin-biometric  — requires auth
// Body: { new_pin }
// Lets a user who has forgotten the current PIN set a new one by
// proving identity with the OS biometric prompt instead.
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct ResetPinBody {
    new_pin: String,
}

async fn change_pin_biometric(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<ResetPinBody>,
) -> ApiResult {
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT biometric_enrolled FROM user_profiles WHERE id = ?")
            .bind(&auth.user_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let (enrolled,) = row.ok_or_else(|| err(StatusCode::NOT_FOUND, "Profile not found"))?;
    if enrolled == 0 {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Biometric not enrolled for this profile",
        ));
    }

    let verified = bio_verify(&state, &format!("Reset {} PIN", crate::product::NAME))
        .await
        .map_err(|e| err(StatusCode::SERVICE_UNAVAILABLE, &e.to_string()))?;
    if !verified {
        return Err(err(
            StatusCode::UNAUTHORIZED,
            "Biometric verification failed or cancelled",
        ));
    }

    if !validate_pin_format(&body.new_pin) {
        return Err(err(StatusCode::BAD_REQUEST, "New PIN must be 4–8 digits"));
    }
    let new_hash = hash_pin(&body.new_pin)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    sqlx::query("UPDATE user_profiles SET pin_hash = ? WHERE id = ?")
        .bind(&new_hash)
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// POST /auth/biometric-enable  — requires auth
// ---------------------------------------------------------------------------
async fn biometric_enable(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    if !biometric::is_available().await {
        return Err(err(StatusCode::SERVICE_UNAVAILABLE, "Biometric not available on this device"));
    }
    let verified = bio_verify(&state, &format!("Enable biometric unlock for {}", crate::product::NAME))
        .await
        .map_err(|e| err(StatusCode::SERVICE_UNAVAILABLE, &e.to_string()))?;
    if !verified {
        return Err(err(StatusCode::UNAUTHORIZED, "Biometric verification failed or cancelled"));
    }
    sqlx::query("UPDATE user_profiles SET biometric_enrolled = 1 WHERE id = ?")
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    Ok(Json(json!({ "ok": true, "enabled": true })))
}

// ---------------------------------------------------------------------------
// POST /auth/biometric-disable  — requires auth
// ---------------------------------------------------------------------------
async fn biometric_disable(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    sqlx::query("UPDATE user_profiles SET biometric_enrolled = 0 WHERE id = ?")
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    Ok(Json(json!({ "ok": true, "enabled": false })))
}

// ---------------------------------------------------------------------------
// POST /auth/logout
// ---------------------------------------------------------------------------
async fn logout(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    state
        .sessions
        .revoke_all(&auth.user_id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// GET /auth/me
//
// Cheap session validity probe + user-profile rehydration. The frontend
// calls this on app boot to decide whether the cached bearer token in
// `localStorage` is still live (returns 200 with profile) or expired
// (401 → drop the token, redirect to /unlock). Without this endpoint
// the client could only discover invalid tokens by triggering a real
// API call and parsing the 401 response, which is awkward on every
// component that needs the user id.
//
// Returns the same `{ id, username, display_name }` shape that
// `/auth/unlock` returns on success, so the client can swap one for
// the other without UI changes.
// ---------------------------------------------------------------------------
async fn me(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    let row: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, username, display_name FROM user_profiles WHERE id = ?",
    )
    .bind(&auth.user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (id, username, display_name) = row.ok_or_else(|| {
        // The session token is valid but the user row was deleted out
        // from under it (e.g. profile reset). Treat as 401 — the
        // frontend will drop the token and route to /setup.
        err(
            StatusCode::UNAUTHORIZED,
            "Session points at a missing user profile",
        )
    })?;

    Ok(Json(json!({
        "id": id,
        "username": username,
        "display_name": display_name,
    })))
}

// ---------------------------------------------------------------------------
// Helper: route biometric verification through Tauri HWND channel if
// available, otherwise fall back to direct WinRT call.
// ---------------------------------------------------------------------------
async fn bio_verify(state: &AppState, reason: &str) -> anyhow::Result<bool> {
    if let Some(tx) = &state.biometric_tx {
        tracing::info!("[bio_verify] routing via Tauri channel, reason='{reason}'");
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        tx.send((reason.to_string(), reply_tx))
            .await
            .map_err(|_| anyhow::anyhow!("Biometric channel closed"))?;
        tracing::debug!("[bio_verify] request sent, awaiting reply...");
        let result = reply_rx
            .await
            .map_err(|_| anyhow::anyhow!("Biometric reply channel dropped"))?
            .map_err(|e| anyhow::anyhow!("{e}"));
        tracing::info!("[bio_verify] reply received: {:?}", result);
        result
    } else {
        tracing::info!("[bio_verify] no Tauri channel, calling biometric::verify directly");
        biometric::verify(reason).await
    }
}
