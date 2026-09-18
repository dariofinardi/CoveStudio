//! HTTP-level tests for `/user/pii-threshold` (migration 0035).
//!
//! The endpoint drives the masking-caution slider in Settings →
//! Sicurezza. These tests are cheap — no model, no inference — so they
//! run on a plain `cargo test`. They cover what a UI can actually do
//! wrong: nothing saved yet, a value outside the slider's range, a
//! nonsense number, and a value that has to survive a round trip.

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use cove_studio::AppState;
use serde_json::{json, Value};
use sqlx::sqlite::SqlitePoolOptions;
use std::sync::Arc;
use tower::ServiceExt; // for `oneshot`

async fn fresh_app() -> (axum::Router, Arc<AppState>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("pii-threshold.db");
    let url = format!(
        "sqlite://{}?mode=rwc",
        db_path.display().to_string().replace('\\', "/")
    );

    #[cfg(feature = "rag")]
    cove_studio::embeddings::register_sqlite_vec_auto_extension();

    let pool = SqlitePoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("connect sqlite");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrate");

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
        .nest("/user", cove_studio::routes::user::router())
        .with_state(state.clone());

    std::mem::forget(dir); // outlive the test
    (app, state)
}

async fn make_user_and_token(state: &AppState) -> String {
    let user_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO user_profiles (id, username, display_name, pin_hash) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(&user_id)
    .bind(format!("pii-{}", &user_id[..8]))
    .bind("PII")
    .bind("dummy-not-a-real-hash")
    .execute(&state.db)
    .await
    .expect("insert user");

    state.sessions.create(&user_id).await.expect("create session")
}

async fn body_to_json(resp: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn get_threshold(app: &axum::Router, token: &str) -> Value {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/user/pii-threshold")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    body_to_json(resp).await
}

async fn put_threshold(
    app: &axum::Router,
    token: &str,
    value: Value,
) -> (StatusCode, Value) {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/user/pii-threshold")
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(json!({ "pii_threshold": value }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    (status, body_to_json(resp).await)
}

#[tokio::test]
async fn defaults_to_the_calibrated_value_before_anything_is_saved() {
    let (app, state) = fresh_app().await;
    let token = make_user_and_token(&state).await;

    let v = get_threshold(&app, &token).await;
    assert_eq!(v["pii_threshold"], 0.5, "model's calibrated default");
    // The UI needs the bounds to render the slider; they must travel
    // with the value rather than being duplicated in the frontend.
    assert_eq!(v["min"], 0.05);
    assert_eq!(v["max"], 0.95);
    assert_eq!(v["default"], 0.5);
}

#[tokio::test]
async fn saved_value_survives_a_round_trip() {
    let (app, state) = fresh_app().await;
    let token = make_user_and_token(&state).await;

    let (status, put) = put_threshold(&app, &token, json!(0.35)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(put["ok"], true);
    assert_eq!(put["pii_threshold"], 0.35);

    let got = get_threshold(&app, &token).await;
    assert_eq!(got["pii_threshold"], 0.35);
}

#[tokio::test]
async fn out_of_range_values_are_clamped_not_rejected() {
    let (app, state) = fresh_app().await;
    let token = make_user_and_token(&state).await;

    // A slider that snaps back to a legal value is friendlier than one
    // that errors — but the caller must be told what was stored.
    let (status, low) = put_threshold(&app, &token, json!(-1.0)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(low["pii_threshold"], 0.05);
    assert_eq!(get_threshold(&app, &token).await["pii_threshold"], 0.05);

    let (status, high) = put_threshold(&app, &token, json!(42.0)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(high["pii_threshold"], 0.95);
    assert_eq!(get_threshold(&app, &token).await["pii_threshold"], 0.95);
}

#[tokio::test]
async fn a_zero_threshold_never_reaches_the_detector() {
    // 0.0 would mark almost every capitalised word; the floor exists so
    // a mis-sent value cannot turn the document into placeholders.
    let (app, state) = fresh_app().await;
    let token = make_user_and_token(&state).await;

    let (_, body) = put_threshold(&app, &token, json!(0.0)).await;
    assert_eq!(body["pii_threshold"], 0.05);
}

#[tokio::test]
async fn nan_is_refused() {
    let (app, state) = fresh_app().await;
    let token = make_user_and_token(&state).await;

    // JSON has no NaN literal, so this is how a broken client sends
    // one. Storing it would make every later comparison false and the
    // detector would silently accept every span.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/user/pii-threshold")
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"pii_threshold": 1e400}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        resp.status() == StatusCode::BAD_REQUEST
            || resp.status() == StatusCode::UNPROCESSABLE_ENTITY,
        "an infinite threshold must not be stored, got {}",
        resp.status()
    );
    // Whatever the rejection shape, nothing was written.
    assert_eq!(get_threshold(&app, &token).await["pii_threshold"], 0.5);
}

#[tokio::test]
async fn the_threshold_is_per_user() {
    let (app, state) = fresh_app().await;
    let alice = make_user_and_token(&state).await;
    let bob = make_user_and_token(&state).await;

    put_threshold(&app, &alice, json!(0.2)).await;

    assert_eq!(get_threshold(&app, &alice).await["pii_threshold"], 0.2);
    assert_eq!(
        get_threshold(&app, &bob).await["pii_threshold"],
        0.5,
        "one user's choice must not change another's masking"
    );
}

#[tokio::test]
async fn a_hand_edited_database_cannot_widen_the_range() {
    // The column is NOT NULL DEFAULT 0.5, so the only way to store
    // something illegal is to edit the file. The reader must still not
    // hand that value to the detector: 0.0 would mask almost every
    // capitalised word in the document.
    let (app, state) = fresh_app().await;
    let token = make_user_and_token(&state).await;
    // Create the row through the API, then corrupt it underneath.
    put_threshold(&app, &token, json!(0.5)).await;
    sqlx::query("UPDATE user_settings SET pii_threshold = ?")
        .bind(0.0_f64)
        .execute(&state.db)
        .await
        .expect("hand edit");

    let user_id: (String,) = sqlx::query_as("SELECT user_id FROM user_settings LIMIT 1")
        .fetch_one(&state.db)
        .await
        .expect("read user_id");
    let effective =
        cove_studio::routes::user::fetch_pii_threshold(&state.db, &user_id.0).await;
    assert_eq!(
        effective, 0.5,
        "an out-of-range stored value must fall back to the default"
    );

    // Same through the endpoint the UI reads.
    assert_eq!(get_threshold(&app, &token).await["pii_threshold"], 0.5);
}

#[tokio::test]
async fn requires_authentication() {
    let (app, _state) = fresh_app().await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/user/pii-threshold")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
