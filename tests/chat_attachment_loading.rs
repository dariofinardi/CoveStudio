//! Tests for the step where an attachment becomes prompt material.
//!
//! `load_attached_docs` is where three things live together: text
//! extraction through the onboarding funnel, images for vision-capable
//! models, and the verdict for files that yielded neither. Until this
//! file existed that function had no coverage at all — it needs a real
//! database and a real storage, which is exactly the excuse that leaves
//! a conversion like this unverified.
//!
//! What each case defends is the same property: **the prompt must never
//! stay silent about a file the user attached.** Silence is what made
//! the assistant answer confidently about documents nobody had read.

use cove_studio::routes::chat::{load_attached_docs, DocPayload};
use cove_studio::AppState;
use sqlx::sqlite::SqlitePoolOptions;
use std::collections::HashSet;
use std::sync::Arc;

/// `STORAGE_PATH` is process-global, so these tests run one at a time.
static STORAGE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Fixture {
    state: Arc<AppState>,
    user_id: String,
    _dir: tempfile::TempDir,
    _guard: std::sync::MutexGuard<'static, ()>,
}

async fn fixture() -> Fixture {
    let guard = STORAGE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().expect("tempdir");
    unsafe {
        std::env::set_var("STORAGE_PATH", dir.path().join("storage"));
    }
    let url = format!(
        "sqlite://{}?mode=rwc",
        dir.path()
            .join("chat.db")
            .display()
            .to_string()
            .replace('\\', "/")
    );

    #[cfg(feature = "rag")]
    cove_studio::embeddings::register_sqlite_vec_auto_extension();

    let pool = SqlitePoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("connect");
    sqlx::migrate!("./migrations").run(&pool).await.expect("migrate");

    let user_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO user_profiles (id, username, display_name, pin_hash) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(&user_id)
    .bind(format!("att-{}", &user_id[..8]))
    .bind("Att")
    .bind("dummy")
    .execute(&pool)
    .await
    .expect("user");

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
    Fixture {
        state: Arc::new(state),
        user_id,
        _dir: dir,
        _guard: guard,
    }
}

/// Stores a document the way an upload would: bytes in storage, row in
/// `documents`, no cached text — so the loader has to read it.
async fn attach(f: &Fixture, filename: &str, file_type: &str, bytes: &[u8]) -> String {
    let doc_id = uuid::Uuid::new_v4().to_string();
    let key = format!("documents/{}/{doc_id}", f.user_id);
    let storage = cove_studio::storage::make_storage().expect("storage");
    storage
        .put(&key, bytes, "application/octet-stream")
        .await
        .expect("put");
    sqlx::query(
        "INSERT INTO documents (id, user_id, filename, file_type, size_bytes, storage_path, status) \
         VALUES (?, ?, ?, ?, ?, ?, 'ready')",
    )
    .bind(&doc_id)
    .bind(&f.user_id)
    .bind(filename)
    .bind(file_type)
    .bind(bytes.len() as i64)
    .bind(&key)
    .execute(&f.state.db)
    .await
    .expect("insert document");
    doc_id
}

async fn load(f: &Fixture, ids: &[String], vision_ok: bool) -> Vec<DocPayload> {
    // The loader emits SSE progress events; a channel nobody drains is
    // what a disconnected client looks like, and it must not block.
    let (tx, _rx) = tokio::sync::mpsc::channel(64);
    load_attached_docs(
        &f.state,
        &f.user_id,
        ids,
        vision_ok,
        &HashSet::new(),
        0.5,
        &tx,
    )
    .await
}

#[tokio::test]
async fn a_readable_attachment_arrives_as_text() {
    let f = fixture().await;
    let id = attach(&f, "nota.txt", "txt", b"Il contratto scade il 31 dicembre.").await;

    let docs = load(&f, &[id], false).await;
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].filename, "nota.txt");
    assert!(
        docs[0]
            .text
            .as_deref()
            .is_some_and(|t| t.contains("31 dicembre")),
        "{:?}",
        docs[0].text
    );
    assert!(docs[0].unreadable.is_none());
}

#[tokio::test]
async fn a_pdf_arrives_with_its_page_markers() {
    let f = fixture().await;
    let bytes = std::fs::read("tests/medical/CARTELLA_TEST_001.pdf").expect("fixture");
    let id = attach(&f, "cartella.pdf", "pdf", &bytes).await;

    let docs = load(&f, &[id], false).await;
    let text = docs[0].text.as_deref().unwrap_or_default();
    assert!(text.contains("[Page 1]"), "no page marker: {text:.120}");
    assert!(docs[0].unreadable.is_none());
}

#[tokio::test]
async fn an_empty_attachment_carries_a_verdict_instead_of_silence() {
    let f = fixture().await;
    let id = attach(&f, "vuoto.txt", "txt", b"").await;

    let docs = load(&f, &[id], false).await;
    assert_eq!(docs.len(), 1, "the file must not vanish from the turn");
    assert!(docs[0].text.is_none());
    assert!(docs[0].images.is_empty());
    assert_eq!(docs[0].unreadable.as_deref(), Some("empty_file"));
}

#[tokio::test]
async fn a_corrupt_attachment_carries_a_verdict() {
    let f = fixture().await;
    let id = attach(&f, "finto.docx", "docx", b"non e' uno zip").await;

    let docs = load(&f, &[id], false).await;
    assert!(docs[0].text.is_none());
    assert!(
        matches!(
            docs[0].unreadable.as_deref(),
            Some("corrupt_file") | Some("read_failed")
        ),
        "{:?}",
        docs[0].unreadable
    );
}

#[tokio::test]
async fn an_unsupported_attachment_carries_a_verdict() {
    let f = fixture().await;
    let id = attach(&f, "archivio.zip", "other", b"PK\x03\x04qualcosa").await;

    let docs = load(&f, &[id], false).await;
    assert_eq!(docs[0].unreadable.as_deref(), Some("unsupported_format"));
}

#[tokio::test]
async fn an_image_reaches_a_vision_model_as_an_image() {
    let f = fixture().await;
    // Smallest valid PNG: 1x1, white.
    let png: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    let id = attach(&f, "foto.png", "png", png).await;

    let docs = load(&f, &[id], true).await;
    assert_eq!(docs[0].images.len(), 1, "the image must reach the model");
    assert!(
        docs[0].images[0].starts_with("data:image/png;base64,"),
        "{}",
        &docs[0].images[0][..40.min(docs[0].images[0].len())]
    );
    assert!(docs[0].unreadable.is_none());
    assert!(docs[0].text.is_none());
}

#[tokio::test]
async fn an_image_without_a_vision_model_says_so() {
    // The case that was still silent after the conversion: no text, no
    // image, and nothing in the prompt — so the model answered as if
    // the user had attached nothing at all.
    let f = fixture().await;
    let png: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52,
    ];
    let id = attach(&f, "foto.png", "png", png).await;

    let docs = load(&f, &[id], false).await;
    assert!(docs[0].images.is_empty());
    assert!(docs[0].text.is_none());
    assert_eq!(
        docs[0].unreadable.as_deref(),
        Some("image_needs_vision_model")
    );
}

#[tokio::test]
async fn several_attachments_keep_their_order_and_their_verdicts() {
    // Order is what the doc-N labels are built from: a reshuffle would
    // point read_document and every citation at the wrong file.
    let f = fixture().await;
    let a = attach(&f, "uno.txt", "txt", b"primo documento").await;
    let b = attach(&f, "vuoto.txt", "txt", b"").await;
    let c = attach(&f, "tre.txt", "txt", b"terzo documento").await;

    let docs = load(&f, &[a, b, c], false).await;
    assert_eq!(docs.len(), 3);
    assert_eq!(docs[0].filename, "uno.txt");
    assert_eq!(docs[1].filename, "vuoto.txt");
    assert_eq!(docs[2].filename, "tre.txt");
    assert!(docs[0].text.is_some());
    assert_eq!(docs[1].unreadable.as_deref(), Some("empty_file"));
    assert!(docs[2].text.is_some());
}

#[tokio::test]
async fn a_missing_storage_object_does_not_drop_the_rest_of_the_turn() {
    // A row pointing at bytes that are gone (cache cleaned, folder
    // moved). The turn must continue with the documents that are fine.
    let f = fixture().await;
    let ok = attach(&f, "presente.txt", "txt", b"contenuto").await;
    let gone = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO documents (id, user_id, filename, file_type, size_bytes, storage_path, status) \
         VALUES (?, ?, 'scomparso.txt', 'txt', 10, 'documents/nowhere/none', 'ready')",
    )
    .bind(&gone)
    .bind(&f.user_id)
    .execute(&f.state.db)
    .await
    .expect("insert");

    let docs = load(&f, &[gone, ok], false).await;
    assert!(
        docs.iter().any(|d| d.filename == "presente.txt"),
        "the readable document must still reach the prompt"
    );
}
