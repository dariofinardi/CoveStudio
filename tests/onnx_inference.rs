//! Integration tests for the ONNX inference path.
//!
//! These tests load `multilingual-e5-base` via `fastembed` (which wraps
//! the `ort` ONNX Runtime) and exercise the actual embedding pipeline +
//! sqlite-vec virtual table end-to-end. The first run downloads ~280 MB
//! of model weights to fastembed's cache directory; subsequent runs are
//! offline.
//!
//! Because the download is heavy and the model load takes seconds, all
//! tests in this file are marked `#[ignore]` so a vanilla `cargo test`
//! stays fast. To run them explicitly:
//!
//!     cargo test --features rag --test onnx_inference -- --ignored --nocapture
//!
//! The tests are also feature-gated behind `rag` so a slim build can
//! compile without `fastembed` / `sqlite-vec` / `libsqlite3-sys`.

#![cfg(feature = "rag")]

use cove_studio::embeddings::{register_sqlite_vec_auto_extension, EmbeddingService, SearchScope};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::str::FromStr;

/// Local copy of the private `service::vec_to_blob`. Integration tests
/// can't reach `pub(crate)` items, so we duplicate the trivial 4-byte
/// little-endian packing used by the embedding service.
fn vec_to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

/// Spin up an in-memory SQLite pool with the doc_chunks vec0 table
/// declared. We don't run the full migration suite — just the slice
/// the embedding service needs.
async fn setup_pool() -> sqlx::SqlitePool {
    register_sqlite_vec_auto_extension();
    // `:memory:` per-pool would give us multiple disconnected DBs once
    // the pool reconnects; we use `mode=memory&cache=shared` so all
    // sqlx connections in this pool see the same schema.
    let opts = SqliteConnectOptions::from_str("sqlite::memory:")
        .unwrap()
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1) // single conn → schema is visible everywhere
        .connect_with(opts)
        .await
        .unwrap();
    sqlx::query(
        "CREATE VIRTUAL TABLE doc_chunks USING vec0(
            user_id      text partition key,
            project_id   text partition key,
            embedding    float[768],
            +document_id text,
            +source_path text,
            +chunk_index integer,
            +text        text,
            +page        integer
        )",
    )
    .execute(&pool)
    .await
    .expect("create vec0 table");
    pool
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "downloads ~280MB ONNX model on first run; run with --ignored"]
async fn embed_query_returns_768_dimensional_vector() {
    let pool = setup_pool().await;
    let svc = EmbeddingService::new(pool);
    let v = svc.embed_query("Cosa dice il contratto sulla recessione?").await.unwrap();
    assert_eq!(v.len(), 768, "multilingual-e5-base produces 768-dim vectors");
    // Vectors must contain finite values.
    assert!(v.iter().all(|f| f.is_finite()));
    // Not all zeros — the model produced something.
    assert!(v.iter().any(|&f| f.abs() > 1e-6));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "downloads ~280MB ONNX model on first run; run with --ignored"]
async fn embed_passages_handles_batches() {
    let pool = setup_pool().await;
    let svc = EmbeddingService::new(pool);
    let texts = vec![
        "Il presente contratto è regolato dalla legge italiana.".to_string(),
        "The agreement is governed by Italian law.".to_string(),
        "Le contrat est régi par le droit italien.".to_string(),
    ];
    let vecs = svc.embed_passages(&texts).await.unwrap();
    assert_eq!(vecs.len(), 3);
    for v in &vecs {
        assert_eq!(v.len(), 768);
        assert!(v.iter().all(|f| f.is_finite()));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "downloads ~280MB ONNX model on first run; run with --ignored"]
async fn empty_passage_batch_is_a_noop() {
    let pool = setup_pool().await;
    let svc = EmbeddingService::new(pool);
    let v = svc.embed_passages(&[]).await.unwrap();
    assert!(v.is_empty(), "empty input must skip model load and return empty vec");
}

/// Cosine similarity helper for assertions.
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (na * nb)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "downloads ~280MB ONNX model on first run; run with --ignored"]
async fn semantically_similar_texts_are_close() {
    let pool = setup_pool().await;
    let svc = EmbeddingService::new(pool);
    let vs = svc.embed_passages(&[
        "Il contratto prevede la clausola di recessione anticipata.".to_string(),
        "Risoluzione anticipata del contratto: clausola contrattuale.".to_string(),
        "La pizza margherita ha pomodoro e mozzarella.".to_string(),
    ]).await.unwrap();

    let sim_close = cosine(&vs[0], &vs[1]);
    let sim_far = cosine(&vs[0], &vs[2]);
    // Two paraphrases of the same legal concept should be more similar
    // than the legal text vs. a pizza recipe. Expecting at least a 0.05
    // cosine gap (very lenient — typical gap on e5-base is ≥0.15).
    assert!(
        sim_close > sim_far + 0.05,
        "expected close > far by 0.05; got close={sim_close}, far={sim_far}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "downloads ~280MB ONNX model on first run; run with --ignored"]
async fn query_and_passage_prefixes_produce_different_geometry() {
    // E5 expects different prefixes for queries vs passages. We exercise
    // both code paths and verify they don't accidentally collapse to the
    // same vector for the same source text.
    let pool = setup_pool().await;
    let svc = EmbeddingService::new(pool);
    let q = svc.embed_query("contratto recessione").await.unwrap();
    let p = svc.embed_passages(&["contratto recessione".to_string()])
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(q.len(), 768);
    assert_eq!(p.len(), 768);
    // The prefixes differ ("query: " vs "passage: ") so the embeddings
    // must differ too — but they should still be close (similar topic).
    let sim = cosine(&q, &p);
    assert!(sim > 0.8, "query vs passage embedding for same text should be close, got {sim}");
    assert!(q != p, "different prefixes must yield different vectors");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "downloads ~280MB ONNX model on first run; run with --ignored"]
async fn end_to_end_index_and_search_returns_relevant_chunk() {
    let pool = setup_pool().await;
    let svc = EmbeddingService::new(pool);

    let n = svc.index_document(
        "user-1",
        None,
        "doc-1",
        "/fake/contract.txt",
        "Articolo 5: Recessione anticipata. Il contratto può essere risolto con \
         preavviso di 30 giorni.\n\n\
         Articolo 6: Pagamenti. I corrispettivi sono dovuti entro fine mese.\n\n\
         Articolo 7: Foro competente. Per ogni controversia è competente il \
         tribunale di Milano.",
    ).await.unwrap();
    assert!(n >= 1, "expected at least one chunk indexed, got {n}");

    let hits = svc.search(
        "user-1",
        SearchScope::Global,
        "come si risolve il contratto in anticipo?",
        3,
    ).await.unwrap();

    assert!(!hits.is_empty(), "should retrieve at least one chunk");
    // The top hit should mention the recessione clause.
    let top = &hits[0];
    let normalized = top.text.to_lowercase();
    assert!(
        normalized.contains("recess") || normalized.contains("articolo 5"),
        "top hit should reference the recessione article, got: {}", top.text,
    );
    assert!(top.distance >= 0.0 && top.distance <= 2.0, "cosine distance ∈ [0, 2]");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "downloads ~280MB ONNX model on first run; run with --ignored"]
async fn search_scope_isolates_strict_projects() {
    let pool = setup_pool().await;
    let svc = EmbeddingService::new(pool);

    // Index one global doc and one strict project doc.
    svc.index_document(
        "user-1",
        None,
        "doc-global",
        "/g.txt",
        "La capitale della Francia è Parigi.",
    ).await.unwrap();
    svc.index_document(
        "user-1",
        Some("proj-strict"),
        "doc-proj",
        "/p.txt",
        "Il segreto del cliente Acme è proprietario.",
    ).await.unwrap();

    // Strict scope on a *different* project must see nothing.
    let hits = svc.search(
        "user-1",
        SearchScope::ProjectStrict("proj-other"),
        "Acme",
        5,
    ).await.unwrap();
    assert!(hits.is_empty(), "strict scope on proj-other must not see proj-strict's chunks");

    // Strict scope on the right project sees only its own chunks.
    let hits = svc.search(
        "user-1",
        SearchScope::ProjectStrict("proj-strict"),
        "Acme",
        5,
    ).await.unwrap();
    assert!(hits.iter().all(|h| h.document_id == "doc-proj"));

    // Shared scope on the right project sees both global and own.
    let hits = svc.search(
        "user-1",
        SearchScope::ProjectShared("proj-strict"),
        "Parigi",
        5,
    ).await.unwrap();
    assert!(
        hits.iter().any(|h| h.document_id == "doc-global"),
        "shared scope must include global chunks"
    );

    // Global scope sees only global chunks.
    let hits = svc.search(
        "user-1",
        SearchScope::Global,
        "Acme",
        5,
    ).await.unwrap();
    assert!(
        hits.iter().all(|h| h.document_id == "doc-global"),
        "global scope must not leak project chunks"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "downloads ~280MB ONNX model on first run; run with --ignored"]
async fn rescope_moves_chunks_without_reindex() {
    let pool = setup_pool().await;
    let svc = EmbeddingService::new(pool);

    svc.index_document(
        "user-1",
        None,
        "doc-r",
        "/r.txt",
        "Una sentenza della Cassazione del 2023 ha chiarito il punto.",
    ).await.unwrap();

    // Initially in the global pool.
    let hits = svc.search("user-1", SearchScope::Global, "Cassazione", 5).await.unwrap();
    assert!(hits.iter().any(|h| h.document_id == "doc-r"));

    // Move to project pool.
    svc.rescope_document("doc-r", Some("proj-x")).await.unwrap();

    // No longer visible in global.
    let hits = svc.search("user-1", SearchScope::Global, "Cassazione", 5).await.unwrap();
    assert!(hits.iter().all(|h| h.document_id != "doc-r"));

    // Visible in the project's strict scope.
    let hits = svc.search("user-1", SearchScope::ProjectStrict("proj-x"), "Cassazione", 5).await.unwrap();
    assert!(hits.iter().any(|h| h.document_id == "doc-r"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "downloads ~280MB ONNX model on first run; run with --ignored"]
async fn delete_document_removes_all_chunks() {
    let pool = setup_pool().await;
    let svc = EmbeddingService::new(pool.clone());

    svc.index_document(
        "user-1",
        None,
        "doc-del",
        "/d.txt",
        "Testo da indicizzare e poi cancellare.",
    ).await.unwrap();

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM doc_chunks WHERE document_id = ?")
        .bind("doc-del")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(count >= 1);

    svc.delete_document("user-1", "doc-del").await.unwrap();

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM doc_chunks WHERE document_id = ?")
        .bind("doc-del")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "downloads ~280MB ONNX model on first run; run with --ignored"]
async fn reindex_is_idempotent() {
    let pool = setup_pool().await;
    let svc = EmbeddingService::new(pool.clone());

    let body = "A. Primo articolo.\n\nB. Secondo articolo.\n\nC. Terzo articolo.";
    let n1 = svc.index_document("u", None, "doc-i", "/i.txt", body).await.unwrap();
    let n2 = svc.index_document("u", None, "doc-i", "/i.txt", body).await.unwrap();
    assert_eq!(n1, n2);

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM doc_chunks WHERE document_id = ?")
        .bind("doc-i")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count as usize, n2, "re-index must not accumulate duplicates");
}

// ---------------------------------------------------------------------------
// Tests below run *without* loading the ONNX model. Kept here to exercise
// the surrounding infrastructure (vec0 table, BLOB packing, scope SQL).
// They run as part of the normal `cargo test --features rag` invocation.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vec_to_blob_size_for_768_dim_vector() {
    let v: Vec<f32> = vec![0.5_f32; 768];
    assert_eq!(vec_to_blob(&v).len(), 768 * 4);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vec0_table_accepts_768_dim_blob() {
    let pool = setup_pool().await;
    // Fabricate a deterministic 768-dim "embedding" without invoking ONNX.
    let v: Vec<f32> = (0..768).map(|i| (i as f32) * 0.001).collect();
    let blob = vec_to_blob(&v);
    sqlx::query(
        "INSERT INTO doc_chunks (embedding, user_id, project_id, document_id, source_path, chunk_index, text, page) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&blob[..])
    .bind("user-1")
    .bind("__global__")
    .bind("doc-1")
    .bind("/x.txt")
    .bind(0_i64)
    .bind("hello")
    .bind(Option::<i64>::None)
    .execute(&pool)
    .await
    .unwrap();

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM doc_chunks").fetch_one(&pool).await.unwrap();
    assert_eq!(count, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "downloads ~280MB ONNX model on first run; run with --ignored"]
async fn pdf_pages_propagate_through_index_and_search() {
    // Mimic the sync scanner's PDF output: each page prefixed with
    // `[Page N]\n` and separated by `\n\n`. We index this, then issue
    // a query that semantically targets page 2's content. The retrieved
    // chunk must carry `page = Some(2)` so the DocPanel can scroll
    // straight there.
    let pool = setup_pool().await;
    let svc = EmbeddingService::new(pool);

    // Each page must exceed the chunker's target_chars (default 1600)
    // so paragraph-packing can't merge pages 1+2 into a single chunk.
    // Inflate the prose with realistic Italian filler so KNN still
    // ranks the recessione paragraph at the top.
    let filler_p1 = "Definizioni preliminari del contratto, parti contraenti e oggetto. ".repeat(40);
    let filler_p2 = "La recessione anticipata richiede preavviso scritto di 30 giorni a mezzo PEC. ".repeat(40);
    let filler_p3 = "Foro competente è il tribunale di Milano per ogni controversia. ".repeat(40);

    let body = format!(
        "[Page 1]\n{filler_p1}\n\n[Page 2]\n{filler_p2}\n\n[Page 3]\n{filler_p3}",
    );

    let n = svc.index_document("u1", None, "doc-pages", "/c.txt", &body).await.unwrap();
    assert!(n >= 3, "expected at least one chunk per page, got {n}");

    let hits = svc.search(
        "u1",
        SearchScope::Global,
        "come si recede dal contratto in anticipo?",
        3,
    ).await.unwrap();

    assert!(!hits.is_empty());
    let top = &hits[0];
    assert!(
        top.text.to_lowercase().contains("recess"),
        "top hit should be the recessione chunk, got: {}", top.text
    );
    assert_eq!(
        top.page,
        Some(2),
        "the chunk holding page-2 content must report page = Some(2); got {:?}",
        top.page
    );
}
