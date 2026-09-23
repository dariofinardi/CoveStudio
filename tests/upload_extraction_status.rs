//! HTTP-level tests of what `POST /document` records about extraction.
//!
//! The bug these pin down: every upload used to be stored as `ready`,
//! whatever extraction produced. A file the product could not read was
//! indistinguishable, in the database and in the interface, from one it
//! had read perfectly — and the assistant answered about both.
//!
//! No model needed, so these run on a plain `cargo test`.

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use cove_studio::AppState;
use serde_json::Value;
use sqlx::sqlite::SqlitePoolOptions;
use std::sync::Arc;
use tower::ServiceExt; // for `oneshot`

/// Multipart body with one file part, plus `cache=true` so the upload
/// takes the chat-attachment path (the one that extracts text).
fn multipart(filename: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    let boundary = "----covestudiotestboundary";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"cache\"\r\n\r\ntrue\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (
        format!("multipart/form-data; boundary={boundary}"),
        body,
    )
}

/// `STORAGE_PATH` is process-global and the storage layer reads it on
/// every call, so two of these tests running at once would write into
/// each other's temp directory. Each test holds this lock for as long
/// as its storage must stay installed.
static STORAGE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The app plus everything that must outlive the test: the temp
/// directory and the lock guard.
struct Fixture {
    app: axum::Router,
    state: Arc<AppState>,
    _dir: tempfile::TempDir,
    _guard: std::sync::MutexGuard<'static, ()>,
}

async fn fresh_app() -> Fixture {
    // A panicking test poisons the lock; the env var is rewritten below
    // anyway, so the poison carries no stale state.
    let guard = STORAGE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("upload.db");
    let url = format!(
        "sqlite://{}?mode=rwc",
        db_path.display().to_string().replace('\\', "/")
    );
    // The upload handler writes through `crate::storage`, which honours
    // STORAGE_PATH; point it inside the temp dir so the test leaves
    // nothing behind in the user's data folder.
    // SAFETY: tests in this file are the only ones touching it, and the
    // value is set before the first storage call.
    unsafe {
        std::env::set_var("STORAGE_PATH", dir.path().join("storage"));
    }

    #[cfg(feature = "rag")]
    cove_studio::embeddings::register_sqlite_vec_auto_extension();

    let pool = SqlitePoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("connect sqlite");
    sqlx::migrate!("./migrations").run(&pool).await.expect("migrate");

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
        .nest("/document", cove_studio::routes::documents::router())
        .with_state(state.clone());
    Fixture {
        app,
        state,
        _dir: dir,
        _guard: guard,
    }
}

async fn make_user_and_token(state: &AppState) -> String {
    let user_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO user_profiles (id, username, display_name, pin_hash) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(&user_id)
    .bind(format!("up-{}", &user_id[..8]))
    .bind("Upload")
    .bind("dummy-not-a-real-hash")
    .execute(&state.db)
    .await
    .expect("insert user");
    state.sessions.create(&user_id).await.expect("session")
}

async fn upload(
    app: &axum::Router,
    token: &str,
    filename: &str,
    bytes: &[u8],
) -> (StatusCode, Value) {
    let (content_type, body) = multipart(filename, bytes);
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/document")
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

/// Reads back what the row actually says, which is what later turns use.
async fn stored_status(state: &AppState, doc_id: &str) -> (String, Option<String>) {
    sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT status, extraction_reason FROM documents WHERE id = ?",
    )
    .bind(doc_id)
    .fetch_one(&state.db)
    .await
    .expect("row")
}

#[tokio::test]
async fn a_readable_file_is_stored_as_ready_with_no_reason() {
    let f = fresh_app().await;
    let (app, state) = (&f.app, &f.state);
    let token = make_user_and_token(state).await;

    let (code, body) = upload(
        app,
        &token,
        "nota.txt",
        "Il contratto scade il 31 dicembre.".as_bytes(),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "ready");
    assert!(body["extraction_reason"].is_null());

    let (status, reason) = stored_status(state, body["id"].as_str().unwrap()).await;
    assert_eq!(status, "ready");
    assert!(reason.is_none());
}

#[tokio::test]
async fn an_empty_file_is_stored_as_no_text_with_a_reason() {
    let f = fresh_app().await;
    let (app, state) = (&f.app, &f.state);
    let token = make_user_and_token(state).await;

    let (code, body) = upload(app, &token, "vuoto.txt", b"").await;
    assert_eq!(code, StatusCode::OK, "the upload itself succeeds: {body}");
    assert_eq!(
        body["status"], "no_text",
        "an unreadable file must not be reported as ready: {body}"
    );
    let reason = body["extraction_reason"]
        .as_str()
        .expect("the response must carry the reason for the composer to show");
    assert!(!reason.is_empty());

    let (status, stored_reason) = stored_status(state, body["id"].as_str().unwrap()).await;
    assert_eq!(status, "no_text");
    assert_eq!(stored_reason.as_deref(), Some(reason));
}

#[tokio::test]
async fn a_file_that_cannot_be_read_is_stored_as_failed() {
    let f = fresh_app().await;
    let (app, state) = (&f.app, &f.state);
    let token = make_user_and_token(state).await;

    // Extension says DOCX, content is not a zip container: the old
    // path turned this into an empty string with no trace.
    let (code, body) = upload(app, &token, "finto.docx", b"non e' uno zip").await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "failed", "{body}");
    assert!(
        body["extraction_reason"]
            .as_str()
            .is_some_and(|r| !r.is_empty()),
        "a failure without a reason is the defect we are fixing: {body}"
    );

    let (status, reason) = stored_status(state, body["id"].as_str().unwrap()).await;
    assert_eq!(status, "failed");
    assert!(reason.is_some());
}

#[tokio::test]
async fn an_unsupported_format_is_recorded_rather_than_accepted_silently() {
    let f = fresh_app().await;
    let (app, state) = (&f.app, &f.state);
    let token = make_user_and_token(state).await;

    let (code, body) = upload(app, &token, "archivio.zip", b"PK\x03\x04qualcosa").await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "failed", "{body}");
    // The stored reason is a canonical code the interface translates,
    // not a sentence: an Italian string in the database could not be
    // shown in the other five languages.
    assert_eq!(body["extraction_reason"], "unsupported_format", "{body}");

    let (status, _) = stored_status(state, body["id"].as_str().unwrap()).await;
    assert_eq!(status, "failed");
}

#[tokio::test]
async fn a_csv_is_stored_as_ready_and_keeps_its_rows() {
    // CSV matters on its own: the tabular workflows parse the rows,
    // and the library's format detector does not know the extension at
    // all — the boundary reads it as plain text.
    let f = fresh_app().await;
    let (app, state) = (&f.app, &f.state);
    let token = make_user_and_token(state).await;

    let csv = "Voce;Importo
Canone locazione;1200,50
Spese condominiali;180,00
";
    let (code, body) = upload(app, &token, "prospetto.csv", csv.as_bytes()).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "ready", "{body}");
    assert_eq!(body["file_type"], "csv", "{body}");

    let doc_id = body["id"].as_str().unwrap();
    let path: (Option<String>,) =
        sqlx::query_as("SELECT extracted_text_path FROM documents WHERE id = ?")
            .bind(doc_id)
            .fetch_one(&state.db)
            .await
            .expect("row");
    let storage = cove_studio::storage::make_storage().expect("storage");
    let text = String::from_utf8_lossy(
        &storage.get(&path.0.expect("text path")).await.expect("cached text"),
    )
    .into_owned();
    // Separators and row order are the document: nothing may be
    // reordered, and nothing may be inserted in front.
    assert!(text.starts_with("Voce;Importo"), "{text}");
    assert!(text.contains("Canone locazione;1200,50"), "{text}");
    assert!(!text.contains("Introduzione"), "{text}");
}

#[tokio::test]
async fn legacy_office_extensions_are_classified_rather_than_other() {
    // `.doc` used to be offered in the composer while nothing could
    // read it; `.ppt` was not offered at all. Both are readable now, so
    // both must be classified — `other` would send them down the
    // "unsupported" path in every later branch.
    let f = fresh_app().await;
    let (app, state) = (&f.app, &f.state);
    let token = make_user_and_token(state).await;

    for (name, expected) in [("relazione.doc", "doc"), ("slide.ppt", "ppt")] {
        // Not valid binary Office content, so extraction fails — what
        // this asserts is the classification, not the parse.
        let (code, body) = upload(app, &token, name, b"contenuto non valido").await;
        assert_eq!(code, StatusCode::OK, "{body}");
        assert_eq!(body["file_type"], expected, "{name}: {body}");
        assert_ne!(
            body["status"], "ready",
            "{name} could not be parsed, so it must not claim to be ready: {body}"
        );
    }
}

#[tokio::test]
async fn a_real_pdf_keeps_its_page_markers_in_the_cached_text() {
    let f = fresh_app().await;
    let (app, state) = (&f.app, &f.state);
    let token = make_user_and_token(state).await;

    let bytes = std::fs::read("tests/medical/CARTELLA_TEST_001.pdf").expect("fixture");
    let (code, body) = upload(app, &token, "cartella.pdf", &bytes).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "ready", "{body}");

    // The cached text is what later chat turns read; the markers in it
    // are what citations resolve against.
    let doc_id = body["id"].as_str().unwrap();
    let path: (Option<String>,) =
        sqlx::query_as("SELECT extracted_text_path FROM documents WHERE id = ?")
            .bind(doc_id)
            .fetch_one(&state.db)
            .await
            .expect("row");
    let key = path.0.expect("cache upload must record the text path");
    let storage = cove_studio::storage::make_storage().expect("storage");
    let text = String::from_utf8_lossy(&storage.get(&key).await.expect("cached text")).into_owned();
    assert!(text.contains("[Page 1]"), "no page marker in cached text");
}
