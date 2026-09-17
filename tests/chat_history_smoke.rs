//! HTTP-level smoke test for the chat history round-trip — user
//! message metadata (`files`, `workflow`, `template`) persisted in
//! migration 0021 must survive a GET /chat/:id call so the composer
//! pills come back when a chat is reopened on a later day.

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use cove_studio::AppState;
use serde_json::Value;
use sqlx::sqlite::SqlitePoolOptions;
use std::sync::Arc;
use tower::ServiceExt;

async fn fresh_app() -> (axum::Router, Arc<AppState>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("history.db");
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
        .nest("/chat", cove_studio::routes::chat::router())
        .with_state(state.clone());

    std::mem::forget(dir);
    (app, state)
}

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

async fn seed_chat_with_user_message(
    state: &AppState,
    token: &str,
    files_json: Option<&str>,
    workflow_json: Option<&str>,
    template_json: Option<&str>,
    events_json: Option<&str>,
) -> String {
    // Resolve user_id from the session token.
    let user_id: (String,) = sqlx::query_as(
        "SELECT user_id FROM sessions WHERE token = ?",
    )
    .bind(token)
    .fetch_one(&state.db)
    .await
    .expect("session lookup");

    let chat_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO chats (id, user_id, project_id, title) VALUES (?, ?, NULL, 'Smoke')",
    )
    .bind(&chat_id)
    .bind(&user_id.0)
    .execute(&state.db)
    .await
    .expect("insert chat");

    // User message with metadata.
    let user_msg_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO messages (id, chat_id, role, content, files, workflow, template) \
         VALUES (?, ?, 'user', ?, ?, ?, ?)",
    )
    .bind(&user_msg_id)
    .bind(&chat_id)
    .bind("analizza queste polizze")
    .bind(&files_json)
    .bind(&workflow_json)
    .bind(&template_json)
    .execute(&state.db)
    .await
    .expect("insert user msg");

    // Assistant message with events (already covered by migration 0020,
    // here to ensure both columns coexist on the row layout).
    let asst_msg_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO messages (id, chat_id, role, content, events) \
         VALUES (?, ?, 'assistant', ?, ?)",
    )
    .bind(&asst_msg_id)
    .bind(&chat_id)
    .bind("Ecco il riepilogo.")
    .bind(&events_json)
    .execute(&state.db)
    .await
    .expect("insert asst msg");

    chat_id
}

async fn body_to_json(resp: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn get_chat_round_trips_files_workflow_template_on_user_message() {
    let (app, state) = fresh_app().await;
    let token = make_user_and_token(&state).await;

    let files = r#"[{"filename":"polizza-allianz.pdf","document_id":"doc-uuid-1"},{"filename":"app-i-semestre.pdf","document_id":"doc-uuid-2"}]"#;
    let workflow = r#"{"id":"builtin-insurance-property-inventory","title":"Inventario beni assicurati"}"#;
    let template = r#"{"id":"it/inventario-beni-assicurati","title":"Inventario beni assicurati"}"#;
    let events = r#"[{"type":"doc_created","filename":"Inventario beni assicurati.docx","download_url":"/document/abc123/download","document_id":"abc123","isStreaming":false}]"#;

    let chat_id = seed_chat_with_user_message(
        &state,
        &token,
        Some(files),
        Some(workflow),
        Some(template),
        Some(events),
    )
    .await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/chat/{chat_id}"))
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_to_json(resp).await;
    let messages = v["messages"].as_array().expect("messages array");
    assert_eq!(messages.len(), 2, "expected user + assistant message");

    let user_msg = &messages[0];
    assert_eq!(user_msg["role"], "user");
    assert_eq!(user_msg["content"], "analizza queste polizze");

    // Files round-trip — both pills come back with filename + document_id.
    let files_out = user_msg["files"].as_array().expect("files array");
    assert_eq!(files_out.len(), 2);
    assert_eq!(files_out[0]["filename"], "polizza-allianz.pdf");
    assert_eq!(files_out[0]["document_id"], "doc-uuid-1");
    assert_eq!(files_out[1]["filename"], "app-i-semestre.pdf");

    // Workflow chip round-trips with id + title.
    assert_eq!(user_msg["workflow"]["id"], "builtin-insurance-property-inventory");
    assert_eq!(user_msg["workflow"]["title"], "Inventario beni assicurati");

    // Template chip round-trips.
    assert_eq!(user_msg["template"]["id"], "it/inventario-beni-assicurati");

    // Assistant message keeps content + appended doc_created event so the
    // download card re-renders on reopen.
    let asst_msg = &messages[1];
    assert_eq!(asst_msg["role"], "assistant");
    let content_arr = asst_msg["content"].as_array().expect("assistant content array");
    assert_eq!(content_arr[0]["type"], "content");
    assert_eq!(content_arr[0]["text"], "Ecco il riepilogo.");
    assert_eq!(content_arr[1]["type"], "doc_created");
    assert_eq!(content_arr[1]["filename"], "Inventario beni assicurati.docx");
}

#[tokio::test]
async fn get_chat_user_message_without_metadata_omits_optional_fields() {
    let (app, state) = fresh_app().await;
    let token = make_user_and_token(&state).await;

    let chat_id = seed_chat_with_user_message(&state, &token, None, None, None, None).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/chat/{chat_id}"))
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_to_json(resp).await;
    let user_msg = &v["messages"][0];
    // Optional fields are absent when the column is NULL — the frontend's
    // `m.files ?? undefined` fallback handles this without complaint.
    assert!(user_msg.get("files").is_none(), "files must be absent when NULL");
    assert!(user_msg.get("workflow").is_none(), "workflow must be absent when NULL");
    assert!(user_msg.get("template").is_none(), "template must be absent when NULL");
}
