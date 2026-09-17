//! HTTP-level smoke test for the /workflow CRUD route.
//!
//! Spins up the axum router with an isolated SQLite DB, registers a
//! user, and round-trips the exact payload shape the frontend sends
//! from `NewWorkflowModal`. This is the test that would have caught
//! the "missing field `prompt_md`" deserialization bug at PR time.

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
    // Use an isolated on-disk DB per test so migrations apply and the
    // sqlite-vec auto-extension works (in-memory shared-cache + vec
    // requires extra plumbing).
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("smoke.db");
    let url = format!("sqlite://{}?mode=rwc", db_path.display().to_string().replace('\\', "/"));

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

    // Hand-build an AppState that owns the same pool.
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
        .nest("/workflow", cove_studio::routes::workflows::router())
        .with_state(state.clone());

    // Tempdir must outlive the test — leak it for the duration.
    std::mem::forget(dir);
    (app, state)
}

/// Insert a user row + an active session via SessionStore. Returns
/// the bearer token the test should send in `Authorization`.
async fn make_user_and_token(state: &AppState) -> String {
    let user_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO user_profiles (id, username, display_name, pin_hash) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(&user_id)
    .bind(format!("smoke-{}", &user_id[..8]))
    .bind("Smoke")
    .bind("dummy-not-a-real-hash")
    .execute(&state.db)
    .await
    .expect("insert user");

    state.sessions.create(&user_id).await.expect("create session")
}

async fn body_to_json(resp: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn create_workflow_accepts_modal_payload() {
    let (app, state) = fresh_app().await;
    let token = make_user_and_token(&state).await;

    // Exact payload shape the NewWorkflowModal sends — title + type +
    // practice. No prompt_md, no columns_config. This was the deser
    // failure point before migration 0010 + the optional-field route.
    let body = json!({
        "title": "Test",
        "type": "tabular",
        "practice": "Corporate"
    });

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflow")
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK, "create should succeed");
    let v = body_to_json(resp).await;
    assert_eq!(v["title"], "Test");
    assert_eq!(v["type"], "tabular");
    assert_eq!(v["practice"], "Corporate");
    assert!(v["prompt_md"].is_null(), "no prompt_md → null in response");
    assert_eq!(v["columns_config"], json!([]));
    assert_eq!(v["is_owner"], true);
    assert!(v["id"].is_string());
}

#[tokio::test]
async fn create_workflow_with_full_payload_persists_columns() {
    let (app, state) = fresh_app().await;
    let token = make_user_and_token(&state).await;

    let body = json!({
        "title": "Full",
        "type": "tabular",
        "prompt_md": "# Heading\n\nBody",
        "practice": "Litigation",
        "columns_config": [
            { "index": 0, "name": "Parties", "prompt": "Who?" },
            { "index": 1, "name": "Dates",   "prompt": "When?" }
        ]
    });

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflow")
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_to_json(resp).await;
    assert_eq!(v["columns_config"].as_array().unwrap().len(), 2);
    assert_eq!(v["columns_config"][0]["name"], "Parties");
    assert_eq!(v["prompt_md"], "# Heading\n\nBody");
    let id = v["id"].as_str().unwrap().to_string();

    // GET should round-trip the same shape.
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/workflow/{id}"))
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_to_json(resp).await;
    assert_eq!(v["title"], "Full");
    assert_eq!(v["practice"], "Litigation");
    assert_eq!(v["columns_config"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn list_filters_by_type() {
    let (app, state) = fresh_app().await;
    let token = make_user_and_token(&state).await;

    for (title, ty) in &[("A1", "assistant"), ("T1", "tabular"), ("T2", "tabular")] {
        let body = json!({ "title": title, "type": ty });
        let resp = app.clone().oneshot(
            Request::builder().method("POST").uri("/workflow")
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string())).unwrap(),
        ).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    let resp = app.clone().oneshot(
        Request::builder().method("GET").uri("/workflow?type=tabular")
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::empty()).unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_to_json(resp).await;
    let workflows = v["workflows"].as_array().unwrap();
    assert_eq!(workflows.len(), 2);
    assert!(workflows.iter().all(|w| w["type"] == "tabular"));
}

#[tokio::test]
async fn patch_updates_partial_fields() {
    let (app, state) = fresh_app().await;
    let token = make_user_and_token(&state).await;

    let resp = app.clone().oneshot(
        Request::builder().method("POST").uri("/workflow")
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .body(Body::from(json!({"title":"X","type":"assistant"}).to_string())).unwrap(),
    ).await.unwrap();
    let id = body_to_json(resp).await["id"].as_str().unwrap().to_string();

    let resp = app.clone().oneshot(
        Request::builder().method("PATCH").uri(format!("/workflow/{id}"))
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .body(Body::from(json!({"prompt_md":"hello"}).to_string())).unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_to_json(resp).await;
    assert_eq!(v["prompt_md"], "hello");
    assert_eq!(v["title"], "X", "title left unchanged");
}

#[tokio::test]
async fn rejects_empty_title() {
    let (app, state) = fresh_app().await;
    let token = make_user_and_token(&state).await;

    let resp = app.oneshot(
        Request::builder().method("POST").uri("/workflow")
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .body(Body::from(json!({"title":"   ","type":"assistant"}).to_string())).unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn rejects_invalid_type() {
    let (app, state) = fresh_app().await;
    let token = make_user_and_token(&state).await;

    let resp = app.oneshot(
        Request::builder().method("POST").uri("/workflow")
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .body(Body::from(json!({"title":"X","type":"nonsense"}).to_string())).unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
