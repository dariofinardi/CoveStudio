//! Embedding + vector store service (sqlite-vec backend).
//!
//! Same SQLite file as the rest of Cove Studio (`cove-studio.db`). Vectors live
//! in the `doc_chunks` virtual table created by the sqlite-vec extension
//! (see migration 0009). Atomic transactions, single-file backup,
//! shared connection pool with the rest of the app — no separate store,
//! no native deps beyond what sqlx already brings in.
//!
//! The INT8-quantized `multilingual-e5-base` ONNX weights (Xenova
//! mirror, ~265 MB) are downloaded by `fastembed` to its cache
//! directory the first time `embed_passages` or `embed_query` is
//! called, then loaded once per process.

use anyhow::{anyhow, Context, Result};
use fastembed::{
    InitOptionsUserDefined, Pooling, QuantizationMode, TextEmbedding, TokenizerFiles,
    UserDefinedEmbeddingModel,
};
use futures_util::StreamExt;
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, OnceCell, RwLock};

use super::chunker::{chunk_text, ChunkConfig};

/// Vector dimension produced by `multilingual-e5-base`. Hard-coded
/// because the migration's `vec0(embedding float[768])` table size
/// must match.
pub const EMBEDDING_DIM: usize = 768;

/// Sentinel value for the `project_id` partition when a chunk belongs
/// to the global pool (no project scope).
///
/// sqlite-vec partition keys must always be filtered with strict
/// equality (`=`) — no `IS NULL`, no `OR`. Storing global rows as
/// NULL would make them unreachable by any KNN query, so we encode
/// "global" as this string. The leading underscores make it
/// vanishingly unlikely to collide with a real project UUID.
pub const GLOBAL_PARTITION: &str = "__global__";

#[derive(Debug, thiserror::Error)]
pub enum RagError {
    #[error("model not loaded: {0}")]
    ModelLoad(String),
    #[error("vector store error: {0}")]
    Store(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("other: {0}")]
    Other(#[from] anyhow::Error),
}

/// One per process. Holds the lazy-loaded e5 model and a clone of the
/// app-wide sqlx pool so it can issue vector queries without opening a
/// new connection.
pub struct EmbeddingService {
    db: SqlitePool,
    model: OnceCell<Mutex<TextEmbedding>>,
    /// Live status of the model load — observable via the public
    /// `/rag/model-status` endpoint so the frontend can render a
    /// progress bar for the (potentially long) one-shot download +
    /// the subsequent ONNX init.
    status: Arc<RwLock<ModelStatus>>,
    /// Live progress of any document currently being chunk+embed'd.
    /// `None` between jobs; populated for the duration of a single
    /// `embed_passages_with_progress` call. Read by the
    /// `/eurlex/embed-progress` endpoint to drive the per-row
    /// progress bar in the EUR-Lex settings panel.
    pub active_embed: Arc<RwLock<Option<EmbedProgress>>>,
}

impl EmbeddingService {
    /// Read-only snapshot of the model-load status, exposed so callers
    /// outside this module (notably the `/healthz` handler) can render
    /// the current state without owning the internal `RwLock`. Clones
    /// the variant so the lock guard stays inside the function.
    pub async fn status_snapshot(&self) -> ModelStatus {
        self.status.read().await.clone()
    }
}

/// Live snapshot of the in-flight chunk → embed work. Updated after
/// every batch so the UI progress bar can advance smoothly.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EmbedProgress {
    pub document_id: String,
    pub current: usize,
    pub total: usize,
}

/// Snapshot of where the embedding model is in its lifecycle.
#[derive(Debug, Clone)]
pub enum ModelStatus {
    /// Model has never been touched.
    Idle,
    /// At least one of the required files is being fetched from
    /// HuggingFace. `total` may be `None` when the server omits the
    /// Content-Length header (rare for HF). `file` is the remote path
    /// (e.g. `onnx/model.onnx`, `tokenizer.json`) so the UI can label
    /// which artefact it is.
    Downloading {
        downloaded: u64,
        total: Option<u64>,
        file: String,
    },
    /// Files are on disk, ONNX Runtime is materialising the session.
    /// Fast (1–2 s on CPU) but worth surfacing so the user sees the
    /// progress bar transition smoothly into "Loading…" instead of
    /// freezing at 100 %.
    Loading,
    /// Session is built and ready to embed.
    Ready,
    /// Something went wrong; the message goes straight into the UI.
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct RetrievedChunk {
    pub document_id: String,
    pub source_path: String,
    pub text: String,
    pub chunk_index: i32,
    /// 1-based PDF page number this chunk belongs to. `None` for
    /// formats without page markers (DOCX, XLSX, MD, TXT, CSV) or for
    /// chunks above the first `[Page N]` marker.
    pub page: Option<i64>,
    /// Lower is better (cosine distance, range 0..2 with 0 == identical).
    pub distance: f32,
}

/// Scope of a retrieval query. Mirrors the three-tier model documented
/// in `mod.rs`.
#[derive(Debug, Clone, Copy)]
pub enum SearchScope<'a> {
    /// Global pool only (no project context — e.g. the standalone
    /// `/assistant` chat).
    Global,
    /// Project chats with `isolation_mode = 'shared'`: see global + own.
    ProjectShared(&'a str),
    /// Project chats with `isolation_mode = 'strict'`: see only own.
    ProjectStrict(&'a str),
}

impl EmbeddingService {
    pub fn new(db: SqlitePool) -> Self {
        Self {
            db,
            model: OnceCell::new(),
            status: Arc::new(RwLock::new(ModelStatus::Idle)),
            active_embed: Arc::new(RwLock::new(None)),
        }
    }

    /// Public read of the active-embed snapshot, used by the
    /// `/eurlex/embed-progress` route.
    pub async fn embed_progress(&self) -> Option<EmbedProgress> {
        self.active_embed.read().await.clone()
    }

    /// Cheap read of the live model lifecycle state — used by the
    /// `/rag/model-status` route to power the frontend progress bar.
    pub async fn status(&self) -> ModelStatus {
        self.status.read().await.clone()
    }

    async fn ensure_model(&self) -> Result<&Mutex<TextEmbedding>> {
        // Pre-download the model files explicitly with progress
        // updates, then hand the bytes to fastembed via the
        // `try_new_from_user_defined` entry point. This avoids
        // fastembed's transitive `hf_hub` download (which only logs to
        // stderr) and gives us a real progress bar.
        let status = self.status.clone();
        self.model
            .get_or_try_init(|| async move {
                let cache_root = resolve_fastembed_cache_dir();
                let model_dir =
                    crate::product::migrate_entry(&cache_root, CACHE_SUBDIR, LEGACY_CACHE_SUBDIRS);
                if let Err(e) = tokio::fs::create_dir_all(&model_dir).await {
                    let msg = format!("create cache dir {}: {e}", model_dir.display());
                    *status.write().await = ModelStatus::Failed(msg.clone());
                    return Err(anyhow!(msg));
                }

                let files = match download_model_files(&model_dir, &status).await {
                    Ok(f) => f,
                    Err(e) => {
                        *status.write().await = ModelStatus::Failed(e.to_string());
                        return Err(e);
                    }
                };

                *status.write().await = ModelStatus::Loading;
                let providers = build_execution_providers();
                let provider_label = if providers.is_empty() {
                    "CPU".to_string()
                } else {
                    "EP → CPU fallback".to_string()
                };
                tracing::info!(
                    "[rag] building ONNX session ({} MB onnx, providers: {}, \
                     ORT_DYLIB_PATH={:?})",
                    files.onnx.len() / (1024 * 1024),
                    provider_label,
                    std::env::var("ORT_DYLIB_PATH").ok(),
                );

                let init_start = std::time::Instant::now();
                let onnx_mb = files.onnx.len() / (1024 * 1024);
                tracing::info!("[rag] step 1/4: assembling UserDefinedEmbeddingModel struct");
                let model_result = tokio::task::spawn_blocking(move || {
                    let blocking_start = std::time::Instant::now();
                    tracing::info!(
                        "[rag] step 2/4: inside spawn_blocking, building struct ({} MB onnx)",
                        onnx_mb
                    );
                    // fastembed 4.9.1 marks UserDefinedEmbeddingModel
                    // `#[non_exhaustive]` (fields like `external_initializers`
                    // and `output_key` only landed in 5.x) — use the
                    // builder API. E5 family needs mean pooling over the
                    // last hidden state to keep indexing- and query-time
                    // vector geometry consistent.
                    let model = UserDefinedEmbeddingModel::new(
                        files.onnx,
                        TokenizerFiles {
                            tokenizer_file: files.tokenizer,
                            config_file: files.config,
                            special_tokens_map_file: files.special_tokens_map,
                            tokenizer_config_file: files.tokenizer_config,
                        },
                    )
                    .with_pooling(Pooling::Mean)
                    .with_quantization(QuantizationMode::None);
                    let opts = InitOptionsUserDefined::new()
                        .with_max_length(512)
                        .with_execution_providers(providers);
                    tracing::info!(
                        "[rag] step 3/4: calling TextEmbedding::try_new_from_user_defined \
                         (after {} ms in spawn_blocking)",
                        blocking_start.elapsed().as_millis()
                    );
                    let session_start = std::time::Instant::now();
                    let res = TextEmbedding::try_new_from_user_defined(model, opts);
                    tracing::info!(
                        "[rag] step 3/4 RETURNED: try_new_from_user_defined took {} ms, ok={}",
                        session_start.elapsed().as_millis(),
                        res.is_ok(),
                    );
                    res
                })
                .await;
                tracing::info!(
                    "[rag] step 4/4: spawn_blocking joined (total {} ms)",
                    init_start.elapsed().as_millis(),
                );

                let model = match model_result {
                    Ok(Ok(m)) => m,
                    Ok(Err(e)) => {
                        let msg = format!("model init: {e}");
                        tracing::error!("[rag] model init failed: {msg}");
                        *status.write().await = ModelStatus::Failed(msg.clone());
                        return Err(anyhow!(msg));
                    }
                    Err(e) => {
                        let msg = format!("spawn_blocking failed: {e}");
                        tracing::error!("[rag] spawn_blocking failed: {msg}");
                        *status.write().await = ModelStatus::Failed(msg.clone());
                        return Err(anyhow!(msg));
                    }
                };

                *status.write().await = ModelStatus::Ready;
                tracing::info!(
                    "[rag] model ready ({} ms)",
                    init_start.elapsed().as_millis()
                );
                Ok::<_, anyhow::Error>(Mutex::new(model))
            })
            .await
    }

    /// E5 expects each document chunk to be prefixed with `passage: `.
    /// fastembed exposes a single `embed` call so we add the prefix
    /// ourselves to keep the geometry correct vs. queries.
    ///
    /// Politeness measures, in order of impact:
    ///   1. **Small batch (16)**. fastembed's default 256 OOMs on
    ///      laptops with multilingual-e5-base — we cap at 16. Override
    ///      via `EMBED_BATCH_SIZE`.
    ///   2. **Lower thread priority** during embed. Without this, ORT
    ///      saturates all CPU cores at NORMAL priority and the desktop
    ///      becomes unresponsive. We drop to BELOW_NORMAL on Windows
    ///      so the OS scheduler hands CPU back to the foreground app
    ///      whenever it needs it.
    ///   3. **Yield between batches**. Each batch is a multi-second
    ///      blocking call; in between we let other tokio tasks run
    ///      (e.g. HTTP responses, the chat handler) and sleep briefly
    ///      so the OS can run UI redraws. The added latency is in
    ///      milliseconds per batch — invisible to the user.
    pub async fn embed_passages(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let prefixed: Vec<String> = texts.iter().map(|t| format!("passage: {t}")).collect();
        let n = prefixed.len();
        let model_already_loaded = self.model.initialized();
        if !model_already_loaded {
            tracing::info!(
                "[rag] first embed call ({} chunks) — model not yet loaded, this may take a minute on first run",
                n
            );
        }
        let mu = self.ensure_model().await?;
        let mut guard = mu.lock().await;
        let batch_size = std::env::var("EMBED_BATCH_SIZE")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|&n| n > 0 && n <= 256)
            .unwrap_or(16);

        let total = prefixed.len();
        let mut all_vectors: Vec<Vec<f32>> = Vec::with_capacity(total);
        let t = std::time::Instant::now();

        // We chunk manually instead of letting fastembed's internal
        // batching iterate inside one blocking call — that way the
        // tokio runtime stays responsive between batches and we can
        // log progress on long indexing runs.
        for (batch_idx, batch_texts) in prefixed.chunks(batch_size).enumerate() {
            let batch_vec: Vec<String> = batch_texts.to_vec();
            let bs = batch_size;
            let batch_result = tokio::task::block_in_place(|| {
                with_low_thread_priority(|| guard.embed(batch_vec, Some(bs)))
            });
            let batch_vectors = batch_result.map_err(|e| anyhow!("embed: {e}"))?;
            all_vectors.extend(batch_vectors);
            // Update the shared progress snapshot if any caller has
            // populated it — the route layer reads this via
            // `/eurlex/embed-progress` to drive the UI bar.
            if let Some(p) = self.active_embed.write().await.as_mut() {
                p.current = all_vectors.len();
                p.total = total;
            }
            // Periodically log so a 30-batch run doesn't go silent
            // for a minute.
            if (batch_idx + 1) % 8 == 0 || (batch_idx + 1) * batch_size >= total {
                tracing::info!(
                    "[rag] embed progress: {}/{} chunks ({}ms elapsed)",
                    all_vectors.len(),
                    total,
                    t.elapsed().as_millis()
                );
            }
            // Yield + brief sleep so the UI thread / HTTP responses
            // get scheduling time. 20ms × ~16 batches = ~320ms total
            // overhead on a 255-chunk doc — invisible to the user.
            tokio::task::yield_now().await;
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        tracing::info!(
            "[rag] embedded {} passage chunks in {}ms (batch_size={}, low-prio)",
            total,
            t.elapsed().as_millis(),
            batch_size,
        );
        Ok(all_vectors)
    }

    // (helpers defined as free functions below — see with_low_thread_priority)

    /// E5 expects retrieval-time embeddings to use the `query: ` prefix
    /// — without it the cosine geometry between queries and passages is
    /// off and recall degrades.
    pub async fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        let mu = self.ensure_model().await?;
        let mut guard = mu.lock().await;
        let prefixed = format!("query: {text}");
        let vectors = tokio::task::block_in_place(|| guard.embed(vec![prefixed], None))
            .map_err(|e| anyhow!("embed query: {e}"))?;
        vectors
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("empty embedding result"))
    }

    /// Idempotent: chunk → embed → upsert. Re-running on the same
    /// `document_id` first deletes the old rows so we don't accumulate
    /// duplicates on rescan / re-edit.
    ///
    /// `project_id = None` → global pool. `Some(id)` → project pool.
    pub async fn index_document(
        &self,
        user_id: &str,
        project_id: Option<&str>,
        document_id: &str,
        source_path: &str,
        text: &str,
    ) -> Result<usize> {
        let chunks = chunk_text(text, ChunkConfig::default());
        if chunks.is_empty() {
            return Ok(0);
        }
        let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();

        // Stamp the active-embed snapshot before the long blocking
        // embed call so the `/eurlex/embed-progress` poll has a doc
        // id to display from the very first frame. The clearing
        // happens in a guard so it runs even on error.
        *self.active_embed.write().await = Some(EmbedProgress {
            document_id: document_id.to_string(),
            current: 0,
            total: texts.len(),
        });
        let vectors_result = self.embed_passages(&texts).await;
        // Always clear the active-embed before returning.
        *self.active_embed.write().await = None;
        let vectors = vectors_result?;

        let mut tx = self.db.begin().await?;

        // Drop any prior chunks for this doc.
        sqlx::query("DELETE FROM doc_chunks WHERE document_id = ?")
            .bind(document_id)
            .execute(&mut *tx)
            .await?;

        // Encode the "no project" case as the GLOBAL_PARTITION sentinel
        // — partition-key columns in vec0 must never be NULL if we want
        // them to be reachable by KNN queries.
        let partition_pid: &str = project_id.unwrap_or(GLOBAL_PARTITION);

        for (chunk, vec) in chunks.iter().zip(vectors.iter()) {
            if vec.len() != EMBEDDING_DIM {
                return Err(anyhow!(
                    "embedding dim mismatch: expected {EMBEDDING_DIM}, got {}",
                    vec.len()
                ));
            }
            let blob = vec_to_blob(vec);
            let page: Option<i64> = chunk.page.map(|p| p as i64);
            sqlx::query(
                "INSERT INTO doc_chunks \
                 (embedding, user_id, project_id, document_id, source_path, chunk_index, text, page) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&blob[..])
            .bind(user_id)
            .bind(partition_pid)
            .bind(document_id)
            .bind(source_path)
            .bind(chunk.index as i64)
            .bind(&chunk.text)
            .bind(page)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(chunks.len())
    }

    /// Find top-K chunks for `query` under the given scope.
    pub async fn search(
        &self,
        user_id: &str,
        scope: SearchScope<'_>,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<RetrievedChunk>> {
        let qv = self.embed_query(query).await?;
        let qblob = vec_to_blob(&qv);
        let k = top_k as i64;

        // sqlite-vec's MATCH returns results ordered by distance.
        // The auxiliary columns (user_id, project_id, …) are filterable
        // with standard WHERE clauses; vec0 honours them by passing the
        // predicates down to the inner KNN search.
        // sqlite-vec's KNN query planner requires:
        //  * `k = ?` (or `LIMIT N` with N literal) to size the result;
        //  * partition-key WHERE clauses to use `=`, never `IS NULL` or
        //    `OR`. We side-step both by using a sentinel ("__global__")
        //    for the global pool, written at INSERT time, and by issuing
        //    two separate KNN queries for the "shared" scope (one over
        //    the global partition, one over the project's), then
        //    merging client-side and re-sorting by distance.
        // Row tuple: (document_id, source_path, text, chunk_index, page, distance)
        type Row = (String, String, String, i64, Option<i64>, f32);
        let rows: Vec<Row> = match scope {
            SearchScope::Global => {
                sqlx::query_as(
                    "SELECT document_id, source_path, text, chunk_index, page, distance \
                     FROM doc_chunks \
                     WHERE user_id = ? \
                       AND project_id = ? \
                       AND embedding MATCH ? \
                       AND k = ? \
                     ORDER BY distance",
                )
                .bind(user_id)
                .bind(GLOBAL_PARTITION)
                .bind(&qblob[..])
                .bind(k)
                .fetch_all(&self.db)
                .await?
            }
            SearchScope::ProjectStrict(pid) => {
                sqlx::query_as(
                    "SELECT document_id, source_path, text, chunk_index, page, distance \
                     FROM doc_chunks \
                     WHERE user_id = ? \
                       AND project_id = ? \
                       AND embedding MATCH ? \
                       AND k = ? \
                     ORDER BY distance",
                )
                .bind(user_id)
                .bind(pid)
                .bind(&qblob[..])
                .bind(k)
                .fetch_all(&self.db)
                .await?
            }
            SearchScope::ProjectShared(pid) => {
                // Two KNN queries (one per partition), merged + re-sorted.
                let global: Vec<Row> = sqlx::query_as(
                    "SELECT document_id, source_path, text, chunk_index, page, distance \
                     FROM doc_chunks \
                     WHERE user_id = ? \
                       AND project_id = ? \
                       AND embedding MATCH ? \
                       AND k = ? \
                     ORDER BY distance",
                )
                .bind(user_id)
                .bind(GLOBAL_PARTITION)
                .bind(&qblob[..])
                .bind(k)
                .fetch_all(&self.db)
                .await?;
                let proj: Vec<Row> = sqlx::query_as(
                    "SELECT document_id, source_path, text, chunk_index, page, distance \
                     FROM doc_chunks \
                     WHERE user_id = ? \
                       AND project_id = ? \
                       AND embedding MATCH ? \
                       AND k = ? \
                     ORDER BY distance",
                )
                .bind(user_id)
                .bind(pid)
                .bind(&qblob[..])
                .bind(k)
                .fetch_all(&self.db)
                .await?;
                let mut combined = Vec::with_capacity(global.len() + proj.len());
                combined.extend(global);
                combined.extend(proj);
                combined.sort_by(|a, b| {
                    a.5.partial_cmp(&b.5).unwrap_or(std::cmp::Ordering::Equal)
                });
                combined.truncate(top_k);
                combined
            }
        };

        Ok(rows
            .into_iter()
            .map(|(d, p, t, ci, page, dist)| RetrievedChunk {
                document_id: d,
                source_path: p,
                text: t,
                chunk_index: ci as i32,
                page,
                distance: dist,
            })
            .collect())
    }

    /// Drop every chunk belonging to `document_id` (any scope). Called
    /// when the source file disappears from disk during a rescan.
    pub async fn delete_document(&self, _user_id: &str, document_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM doc_chunks WHERE document_id = ?")
            .bind(document_id)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    /// Move all chunks of a document from one scope to another
    /// (e.g. when the user re-tags a sync folder from global to a
    /// project, or vice versa). sqlite-vec doesn't allow `UPDATE` on
    /// partition-key columns, so we read the rows, delete them, and
    /// re-insert into the new partition. Vectors are scope-agnostic so
    /// we keep the existing embeddings — no model re-invocation needed.
    pub async fn rescope_document(
        &self,
        document_id: &str,
        new_project_id: Option<&str>,
    ) -> Result<()> {
        let new_partition: &str = new_project_id.unwrap_or(GLOBAL_PARTITION);

        let mut tx = self.db.begin().await?;

        let rows: Vec<(Vec<u8>, String, String, i64, String, Option<i64>)> = sqlx::query_as(
            "SELECT embedding, user_id, source_path, chunk_index, text, page \
             FROM doc_chunks WHERE document_id = ?",
        )
        .bind(document_id)
        .fetch_all(&mut *tx)
        .await?;

        if rows.is_empty() {
            return Ok(());
        }

        sqlx::query("DELETE FROM doc_chunks WHERE document_id = ?")
            .bind(document_id)
            .execute(&mut *tx)
            .await?;

        for (embedding, user_id, source_path, chunk_index, text, page) in rows {
            sqlx::query(
                "INSERT INTO doc_chunks \
                 (embedding, user_id, project_id, document_id, source_path, chunk_index, text, page) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&embedding[..])
            .bind(user_id)
            .bind(new_partition)
            .bind(document_id)
            .bind(source_path)
            .bind(chunk_index)
            .bind(text)
            .bind(page)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }
}

/// Bytes of every model artefact we need to build a
/// `UserDefinedEmbeddingModel` for E5 base.
struct ModelFiles {
    onnx: Vec<u8>,
    tokenizer: Vec<u8>,
    config: Vec<u8>,
    special_tokens_map: Vec<u8>,
    tokenizer_config: Vec<u8>,
}

/// Where the model bytes live on disk. Honours `FASTEMBED_CACHE_DIR`
/// (set by `lib::ensure_fastembed_cache_dir` at startup so it points
/// at `<userdata>/cove-studio-data/fastembed/`), falling back to a sane
/// per-user default. Never returns a path inside the workspace tree.
fn resolve_fastembed_cache_dir() -> PathBuf {
    if let Ok(p) = std::env::var("FASTEMBED_CACHE_DIR") {
        return PathBuf::from(p);
    }
    crate::product::data_subdir("fastembed")
}

/// Files we need from `Xenova/multilingual-e5-base` on HuggingFace.
/// Listed in the order they should be downloaded so the big
/// `model_quantized.onnx` (~265 MB) comes last, after the tiny
/// tokenizer metadata is on disk — that way a user who cancels
/// mid-onnx-download still ends up with a coherent partial cache
/// and only has to retry the heavy file on the next run.
///
/// We use the Xenova INT8-dynamic mirror (vs intfloat FP32) because
/// on a curated Italian legal/insurance corpus it is ~1.8× faster
/// on batch indexing, ~3.7× smaller in RAM, 4× smaller on disk,
/// and the FP32-vs-INT8 cosine drift stays ≥ 0.97 with top-1
/// ranking preserved on every query tested (see
/// `tests/embedding_perf.rs::quality_fp32_vs_int8`). The retrieval
/// geometry is indistinguishable for our use case.
const E5_BASE_FILES: &[(&str, &str)] = &[
    ("config.json", "config.json"),
    ("special_tokens_map.json", "special_tokens_map.json"),
    ("tokenizer_config.json", "tokenizer_config.json"),
    ("tokenizer.json", "tokenizer.json"),
    ("onnx/model_quantized.onnx", "model_quantized.onnx"),
];

const HF_REPO: &str = "Xenova/multilingual-e5-base";

/// Cache subdirectory under FASTEMBED_CACHE_DIR. Distinct from the
/// FP32 `e5-base/` so the two can coexist on disk (lets devs
/// run `tests/embedding_perf.rs::quality_fp32_vs_int8` without
/// re-downloading either).
const CACHE_SUBDIR: &str = "e5-base-quantized";

/// Name of `CACHE_SUBDIR` in earlier releases; moved on first use so the
/// ~275 MB model is not downloaded again.
const LEGACY_CACHE_SUBDIRS: &[&str] = &["mike-e5-base-quantized"];

/// Ensure every required E5 file is on disk under `dir`, downloading
/// any that are missing and updating the shared `status` as bytes
/// stream in. Returns the file *bytes* once everything is ready so we
/// can hand them to fastembed's `UserDefinedEmbeddingModel`.
async fn download_model_files(
    dir: &Path,
    status: &Arc<RwLock<ModelStatus>>,
) -> Result<ModelFiles> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60 * 30))
        .build()
        .context("build reqwest client")?;

    for (remote, local) in E5_BASE_FILES {
        let target = dir.join(local);
        if target.exists() {
            let len = tokio::fs::metadata(&target).await?.len();
            if len > 0 {
                tracing::info!(
                    "[rag] cached {} ({} bytes) — skipping download",
                    local,
                    len
                );
                continue;
            }
            // Zero-byte file from a prior failed write — wipe and retry.
            let _ = tokio::fs::remove_file(&target).await;
        }
        download_one(&client, remote, &target, status).await?;
    }

    // Read everything back. They're at most ~280 MB total so loading
    // into memory is fine — we hand them to fastembed which would do
    // it anyway.
    Ok(ModelFiles {
        config: tokio::fs::read(dir.join("config.json")).await?,
        special_tokens_map: tokio::fs::read(dir.join("special_tokens_map.json")).await?,
        tokenizer_config: tokio::fs::read(dir.join("tokenizer_config.json")).await?,
        tokenizer: tokio::fs::read(dir.join("tokenizer.json")).await?,
        onnx: tokio::fs::read(dir.join("model_quantized.onnx")).await?,
    })
}

/// Stream a single file from the HuggingFace mirror to disk, updating
/// the shared `status` after every chunk so the frontend progress bar
/// can move smoothly. Writes to `<target>.part` then atomic-renames
/// on success — partial downloads never look "complete" to the
/// `len > 0` check on the next run.
async fn download_one(
    client: &reqwest::Client,
    remote: &str,
    target: &Path,
    status: &Arc<RwLock<ModelStatus>>,
) -> Result<()> {
    let url = format!("https://huggingface.co/{HF_REPO}/resolve/main/{remote}");
    tracing::info!("[rag] downloading {url}");

    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("non-success on {url}"))?;
    let total = resp.content_length();

    let part = target.with_extension("part");
    let mut file = tokio::fs::File::create(&part)
        .await
        .with_context(|| format!("create {}", part.display()))?;
    let mut downloaded: u64 = 0;
    let mut last_pct_logged: i64 = -1;

    *status.write().await = ModelStatus::Downloading {
        downloaded,
        total,
        file: remote.to_string(),
    };

    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.with_context(|| format!("read chunk from {url}"))?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        *status.write().await = ModelStatus::Downloading {
            downloaded,
            total,
            file: remote.to_string(),
        };
        // Throttle log lines so big files don't spam: log only on each
        // 10 % milestone of total (when known).
        if let Some(t) = total {
            if t > 0 {
                let pct = ((downloaded * 100) / t) as i64;
                if pct >= last_pct_logged + 10 {
                    last_pct_logged = pct;
                    tracing::info!(
                        "[rag] {remote}: {downloaded}/{t} bytes ({pct}%)"
                    );
                }
            }
        }
    }

    file.flush().await?;
    drop(file);
    tokio::fs::rename(&part, target)
        .await
        .with_context(|| format!("rename {} -> {}", part.display(), target.display()))?;
    tracing::info!("[rag] {remote}: download complete ({downloaded} bytes)");
    Ok(())
}

/// Assemble the ONNX Runtime execution-provider list, in fallback
/// order, based on which hardware-accel features were compiled in.
///
/// Order is best-to-worst-fallback: NPU-class providers first, then
/// GPU, then optimised-CPU, with the implicit CPU EP picking up the
/// remainder. ort calls each provider's `supported_by_platform()` at
/// `register()` time and silently skips any whose backend DLLs aren't
/// loadable — so it's safe to ship a binary built with every EP
/// enabled and let the host machine self-select.
///
/// Empty vec means "CPU only" (the ort default).
#[cfg(feature = "rag")]
fn build_execution_providers()
-> Vec<ort::execution_providers::ExecutionProviderDispatch> {
    #[allow(unused_mut)]
    let mut out: Vec<ort::execution_providers::ExecutionProviderDispatch> = Vec::new();

    // ── NPU class ─────────────────────────────────────────────────
    #[cfg(feature = "rag-qnn")]
    {
        // Hexagon NPU on Snapdragon X Elite / 8 Gen 3 / etc.
        // `QnnHtp.dll` must be reachable; fp16 halves memory and ~2x
        // throughput vs fp32.
        out.push(
            ort::execution_providers::QNNExecutionProvider::default()
                .with_backend_path("QnnHtp.dll")
                .with_htp_fp16_precision(true)
                .build(),
        );
    }
    #[cfg(feature = "rag-cann")]
    {
        // Huawei Ascend NPU.
        out.push(ort::execution_providers::CANNExecutionProvider::default().build());
    }
    #[cfg(feature = "rag-nnapi")]
    {
        // Android Neural Networks API — no-op on desktop targets, but
        // harmless to leave compiled in.
        out.push(ort::execution_providers::NNAPIExecutionProvider::default().build());
    }
    #[cfg(feature = "rag-rknpu")]
    {
        // Rockchip RK3588 / RK3568 NPU.
        out.push(ort::execution_providers::RKNPUExecutionProvider::default().build());
    }
    #[cfg(feature = "rag-vitis")]
    {
        // AMD/Xilinx Vitis AI FPGA.
        out.push(ort::execution_providers::VitisAIExecutionProvider::default().build());
    }

    // ── GPU class ─────────────────────────────────────────────────
    #[cfg(feature = "rag-tensorrt")]
    {
        // NVIDIA TensorRT — graph optimiser on top of CUDA. Listed
        // before plain CUDA so ort prefers it when both are available.
        out.push(ort::execution_providers::TensorRTExecutionProvider::default().build());
    }
    #[cfg(feature = "rag-cuda")]
    {
        out.push(ort::execution_providers::CUDAExecutionProvider::default().build());
    }
    #[cfg(feature = "rag-migraphx")]
    {
        // AMD MIGraphX — graph optimiser on top of ROCm.
        out.push(ort::execution_providers::MIGraphXExecutionProvider::default().build());
    }
    #[cfg(feature = "rag-rocm")]
    {
        out.push(ort::execution_providers::ROCmExecutionProvider::default().build());
    }
    #[cfg(feature = "rag-directml")]
    {
        // DirectML — any DX12 GPU on Windows (Adreno X1, Iris, Radeon,
        // GeForce). Picks up the most capable adapter automatically.
        out.push(ort::execution_providers::DirectMLExecutionProvider::default().build());
    }
    #[cfg(feature = "rag-coreml")]
    {
        // Apple Silicon ANE / GPU. Best perf on M-series.
        out.push(ort::execution_providers::CoreMLExecutionProvider::default().build());
    }
    // NOTE: WebGPU dropped together with the rc.12 → rc.9 downgrade;
    // ORT 1.20.0 predates the WebGPU EP. Will return when we bump
    // the runtime back up.
    #[cfg(feature = "rag-openvino")]
    {
        // Intel CPU / iGPU / Movidius VPU.
        out.push(ort::execution_providers::OpenVINOExecutionProvider::default().build());
    }

    // ── Optimised CPU class ───────────────────────────────────────
    #[cfg(feature = "rag-onednn")]
    {
        // Intel oneDNN — CPU-side optimised kernels.
        out.push(ort::execution_providers::OneDNNExecutionProvider::default().build());
    }
    #[cfg(feature = "rag-acl")]
    {
        // ARM Compute Library.
        out.push(ort::execution_providers::ACLExecutionProvider::default().build());
    }
    // NOTE: Arm NN was removed from upstream ONNX Runtime — use ACL,
    // XNNPACK, or the Kleidi-optimised CPU EP instead. The `rag-armnn`
    // cargo feature has been retired accordingly.
    #[cfg(feature = "rag-xnnpack")]
    {
        // Google XNNPACK — mobile / low-end CPU optimisation.
        out.push(ort::execution_providers::XNNPACKExecutionProvider::default().build());
    }
    #[cfg(feature = "rag-tvm")]
    {
        // Apache TVM.
        out.push(ort::execution_providers::TVMExecutionProvider::default().build());
    }

    // NOTE: the Azure EP was dropped together with the rc.12 → rc.9
    // downgrade. Cognitive-services off-load will return when the
    // runtime is bumped back to a version that ships it.

    out
}

#[cfg(not(feature = "rag"))]
#[allow(dead_code)]
fn build_execution_providers() -> Vec<()> {
    Vec::new()
}

/// Resolve the on-disk path to `onnxruntime.dll` (Windows) /
/// `libonnxruntime.{so,dylib}` (Linux/macOS) and export it via
/// `ORT_DYLIB_PATH` so the `ort` crate's `load-dynamic` mode picks it
/// up. Mirrors the discovery walk used for pdfium so dev runs (cwd =
/// workspace root) and bundled runs (cwd = src-tauri/) both work.
///
/// Search order (first hit wins):
///   1. `$ORT_DYLIB_PATH` already set (explicit override — respected).
///   2. `<exe_dir>/libs/onnxruntime/<platform>/`
///   3. `<cwd>/libs/onnxruntime/<platform>/`
///   4. Each ancestor of `<cwd>` ending in
///      `libs/onnxruntime/<platform>/`, up to filesystem root.
///   5. Each ancestor of `<exe_dir>` ending in same.
///
/// `<platform>` is one of `win-x64`, `win-arm64`, `linux-x64`,
/// `linux-aarch64`, `macos-x64`, `macos-arm64`. We pick the one
/// matching the current process's compile target.
///
/// On failure: logs a loud warning and returns without setting the
/// env var. Subsequent ort calls will then fail with a clear "library
/// not found" error pointing at the README, which is better than a
/// silent fallback to a system DLL.
///
/// Safety note: we keep the DLL *inside* the project tree on purpose
/// — no PATH search, no `LoadLibrary` against the system32 onnxruntime,
/// no surprise version mismatches across machines.
#[cfg(feature = "rag")]
pub fn ensure_onnxruntime_dylib_path() {
    if std::env::var_os("ORT_DYLIB_PATH").is_some() {
        tracing::info!(
            "[rag] ORT_DYLIB_PATH already set — honouring explicit override"
        );
        return;
    }

    let (dir, file) = onnxruntime_subdir_and_filename();
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|x| x.to_path_buf()));
    let cwd = std::env::current_dir().ok();

    // Tauri MSI installs land at `<install>/cove-studio.exe` with
    // `bundle.resources` files staged under `<install>/resources/`.
    // Including that directory as an additional start makes the
    // walker find `<install>/resources/libs/onnxruntime/<sub>/<file>`
    // without any platform-specific branching.
    let exe_resources =
        exe_dir.as_ref().map(|d| d.join("resources"));
    let starts: Vec<PathBuf> = exe_dir
        .clone()
        .into_iter()
        .chain(exe_resources.into_iter())
        .chain(cwd.into_iter())
        .collect();

    match find_onnxruntime_dylib(&starts, dir, file) {
        Some(path) => {
            tracing::info!(
                "[rag] loading onnxruntime from {} (load-dynamic)",
                path.display()
            );
            // SAFETY: single-threaded process startup before the ort
            // crate observes the env var — no concurrent reads of
            // std::env::set_var to race with.
            unsafe {
                std::env::set_var("ORT_DYLIB_PATH", &path);
            }
        }
        None => {
            tracing::warn!(
                "[rag] onnxruntime DLL not found in libs/onnxruntime/{}/{} \
                 (searched cwd + exe ancestors). \
                 The first embed call will fail. See libs/onnxruntime/README.md.",
                dir,
                file,
            );
        }
    }
}

/// Pure search helper for the onnxruntime dynamic library. Given a
/// list of `starts` (typically the executable's directory and the
/// process cwd) plus the target subdirectory and filename, returns
/// the first matching path found by:
///   1. Direct lookup at `<start>/libs/onnxruntime/<sub>/<file>` for
///      every start.
///   2. Ancestor walk: for every ancestor of every start, check
///      `<ancestor>/libs/onnxruntime/<sub>/<file>`.
///
/// No env-var access, no `current_exe()` / `current_dir()` calls —
/// fully deterministic from the inputs, so tests can drive it with
/// any directory layout.
#[cfg(feature = "rag")]
pub fn find_onnxruntime_dylib(
    starts: &[PathBuf],
    sub: &str,
    file: &str,
) -> Option<PathBuf> {
    fn candidate(base: &std::path::Path, sub: &str, file: &str) -> PathBuf {
        base.join("libs").join("onnxruntime").join(sub).join(file)
    }

    // Phase 1: direct lookup at every start.
    for base in starts {
        let c = candidate(base, sub, file);
        if c.is_file() {
            return Some(c);
        }
    }
    // Phase 2: ancestor walk.
    for base in starts {
        for anc in base.ancestors() {
            let c = candidate(anc, sub, file);
            if c.is_file() {
                return Some(c);
            }
        }
    }
    None
}

#[cfg(not(feature = "rag"))]
#[allow(dead_code)]
pub fn ensure_onnxruntime_dylib_path() {}

/// Resolve `(subdirectory, filename)` for the bundled onnxruntime DLL
/// based on the current build target. Subdirectories are kept separate
/// so `libs/onnxruntime/` can carry multiple platform builds side-by-
/// side (useful for cross-compiled distributions and bisecting EP
/// support between vendor variants).
#[cfg(feature = "rag")]
fn onnxruntime_subdir_and_filename() -> (&'static str, &'static str) {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        return ("win-x64", "onnxruntime.dll");
    }
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    {
        return ("win-arm64", "onnxruntime.dll");
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        return ("linux-x64", "libonnxruntime.so");
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        return ("linux-aarch64", "libonnxruntime.so");
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        return ("macos-x64", "libonnxruntime.dylib");
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        return ("macos-arm64", "libonnxruntime.dylib");
    }
    #[allow(unreachable_code)]
    ("unknown", "onnxruntime")
}

/// Pack `Vec<f32>` as little-endian bytes — sqlite-vec's BLOB format.
/// Faster and more compact than the JSON-array alternative.
pub(crate) fn vec_to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

/// Run a closure with the calling thread's priority lowered, then
/// restore the previous priority. Used to keep ONNX-heavy embedding
/// work from saturating the desktop scheduler.
///
/// Windows: `SetThreadPriority(THREAD_PRIORITY_BELOW_NORMAL)`.
/// Other platforms: no-op for now (Linux nice() requires raising the
/// priority back which needs CAP_SYS_NICE, so we leave it alone and
/// rely on batching + yielding instead).
#[cfg(target_os = "windows")]
fn with_low_thread_priority<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    use windows::Win32::System::Threading::{
        GetCurrentThread, GetThreadPriority, SetThreadPriority,
        THREAD_PRIORITY_BELOW_NORMAL, THREAD_PRIORITY,
    };
    unsafe {
        let h = GetCurrentThread();
        // Snapshot the previous priority so we can restore it. If the
        // get-call fails we just default to NORMAL on restore.
        let prev = GetThreadPriority(h);
        let _ = SetThreadPriority(h, THREAD_PRIORITY_BELOW_NORMAL);
        let result = f();
        let _ = SetThreadPriority(h, THREAD_PRIORITY(prev));
        result
    }
}

#[cfg(not(target_os = "windows"))]
fn with_low_thread_priority<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    f()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec_to_blob_packs_little_endian() {
        // 1.0_f32 → 0x3F800000 LE → [0, 0, 0x80, 0x3F]
        let bytes = vec_to_blob(&[1.0_f32]);
        assert_eq!(bytes, vec![0, 0, 0x80, 0x3F]);
    }

    #[test]
    fn vec_to_blob_handles_empty() {
        assert!(vec_to_blob(&[]).is_empty());
    }

    #[test]
    fn vec_to_blob_size_matches_f32_count() {
        let v: Vec<f32> = (0..768).map(|i| i as f32).collect();
        let bytes = vec_to_blob(&v);
        assert_eq!(bytes.len(), 768 * 4);
    }

    #[test]
    fn vec_to_blob_roundtrips_via_from_le_bytes() {
        let v = vec![0.0_f32, -1.5, 3.5, f32::MAX, f32::MIN];
        let bytes = vec_to_blob(&v);
        let mut back: Vec<f32> = Vec::new();
        for chunk in bytes.chunks_exact(4) {
            back.push(f32::from_le_bytes(chunk.try_into().unwrap()));
        }
        assert_eq!(back, v);
    }

    #[test]
    fn embedding_dim_is_768() {
        assert_eq!(EMBEDDING_DIM, 768);
    }

    #[test]
    fn search_scope_variants_are_distinguishable() {
        let g = SearchScope::Global;
        let s = SearchScope::ProjectShared("proj-1");
        let st = SearchScope::ProjectStrict("proj-1");
        // Sanity: the enum compiles and the project_id getter is the
        // only field we rely on at the SQL level. Pattern-match
        // ergonomics is what we're really testing here.
        match g { SearchScope::Global => {}, _ => panic!("global mismatch") }
        match s { SearchScope::ProjectShared(p) => assert_eq!(p, "proj-1"), _ => panic!() }
        match st { SearchScope::ProjectStrict(p) => assert_eq!(p, "proj-1"), _ => panic!() }
    }

    // ─────────────────────────────────────────────────────────────────
    // ONNX Runtime load-dynamic plumbing
    // ─────────────────────────────────────────────────────────────────
    //
    // We test the pure `find_onnxruntime_dylib` helper rather than the
    // env-mutating `ensure_onnxruntime_dylib_path()` wrapper — the
    // latter relies on `current_exe()` / `current_dir()` / `set_var`
    // which we can't safely manipulate from a parallel test runner
    // without races. The pure helper is fed an explicit `starts`
    // slice, so each test owns its own tempdir and there's no shared
    // state to coordinate.

    #[test]
    fn onnxruntime_subdir_and_filename_matches_compile_target() {
        let (sub, file) = onnxruntime_subdir_and_filename();
        // The subdir must be one we ship in libs/onnxruntime/.
        let valid_subs = [
            "win-x64",
            "win-arm64",
            "linux-x64",
            "linux-aarch64",
            "macos-x64",
            "macos-arm64",
        ];
        assert!(
            valid_subs.contains(&sub),
            "unexpected platform subdir: {sub}"
        );
        // The filename must match the conventional dynamic-library name
        // for the host OS — anything else and the runtime loader will
        // miss the file we shipped.
        #[cfg(target_os = "windows")]
        assert_eq!(file, "onnxruntime.dll");
        #[cfg(target_os = "linux")]
        assert_eq!(file, "libonnxruntime.so");
        #[cfg(target_os = "macos")]
        assert_eq!(file, "libonnxruntime.dylib");
    }

    #[test]
    fn find_onnxruntime_dylib_returns_none_when_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // Tempdir is empty — no libs/onnxruntime/ tree at all.
        let got = find_onnxruntime_dylib(
            &[tmp.path().to_path_buf()],
            "win-x64",
            "onnxruntime.dll",
        );
        assert!(got.is_none(), "expected None, got {:?}", got);
    }

    #[test]
    fn find_onnxruntime_dylib_finds_dll_at_start_level() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dll_dir = tmp.path().join("libs").join("onnxruntime").join("win-x64");
        std::fs::create_dir_all(&dll_dir).expect("create dirs");
        let dll = dll_dir.join("onnxruntime.dll");
        std::fs::write(&dll, b"fake dll").expect("write fake dll");

        let got = find_onnxruntime_dylib(
            &[tmp.path().to_path_buf()],
            "win-x64",
            "onnxruntime.dll",
        );
        assert_eq!(got.as_deref(), Some(dll.as_path()));
    }

    #[test]
    fn find_onnxruntime_dylib_walks_ancestors() {
        // Simulate the Tauri-dev layout: workspace root has
        // libs/onnxruntime/, but the process is "running" from a
        // deep subfolder like target/debug/. The function should walk
        // up the tree and find the DLL at the workspace root.
        let tmp = tempfile::tempdir().expect("tempdir");
        let workspace = tmp.path();
        let dll_dir = workspace
            .join("libs")
            .join("onnxruntime")
            .join("linux-x64");
        std::fs::create_dir_all(&dll_dir).expect("create dirs");
        let dll = dll_dir.join("libonnxruntime.so");
        std::fs::write(&dll, b"fake so").expect("write fake so");

        // "Start" from a deeply nested folder several levels under
        // the workspace root — mimicking the path of an executable
        // bundled under target/debug/.
        let deep = workspace
            .join("target")
            .join("debug")
            .join("bundle");
        std::fs::create_dir_all(&deep).expect("create deep dir");

        let got = find_onnxruntime_dylib(
            &[deep],
            "linux-x64",
            "libonnxruntime.so",
        );
        assert_eq!(got.as_deref(), Some(dll.as_path()));
    }

    #[test]
    fn find_onnxruntime_dylib_prefers_direct_start_over_ancestor() {
        // If both the start directory AND a higher ancestor have a
        // matching DLL, the direct hit at the start level wins — we
        // assume the caller chose `starts` deliberately (e.g. exe_dir
        // before cwd), so an ancestor-walked match should only kick
        // in when no start-level hit exists.
        let tmp = tempfile::tempdir().expect("tempdir");
        let outer = tmp.path();
        let inner = outer.join("subproject");
        std::fs::create_dir_all(&inner).expect("inner");

        let outer_dll_dir = outer.join("libs").join("onnxruntime").join("win-x64");
        std::fs::create_dir_all(&outer_dll_dir).expect("outer dirs");
        std::fs::write(
            outer_dll_dir.join("onnxruntime.dll"),
            b"outer dll",
        )
        .expect("write outer");

        let inner_dll_dir = inner.join("libs").join("onnxruntime").join("win-x64");
        std::fs::create_dir_all(&inner_dll_dir).expect("inner dirs");
        let inner_dll = inner_dll_dir.join("onnxruntime.dll");
        std::fs::write(&inner_dll, b"inner dll").expect("write inner");

        let got = find_onnxruntime_dylib(
            &[inner.clone()],
            "win-x64",
            "onnxruntime.dll",
        );
        assert_eq!(
            got.as_deref(),
            Some(inner_dll.as_path()),
            "direct-level hit should win over ancestor"
        );
    }

    #[test]
    fn find_onnxruntime_dylib_ignores_wrong_platform_subdir() {
        // A linux-x64 DLL placed in the tree must not match a search
        // for win-x64. Otherwise cross-built distributions would pick
        // the wrong library at runtime.
        let tmp = tempfile::tempdir().expect("tempdir");
        let dll_dir = tmp.path().join("libs").join("onnxruntime").join("linux-x64");
        std::fs::create_dir_all(&dll_dir).expect("create dirs");
        std::fs::write(dll_dir.join("libonnxruntime.so"), b"lx").expect("write");

        let got = find_onnxruntime_dylib(
            &[tmp.path().to_path_buf()],
            "win-x64",
            "onnxruntime.dll",
        );
        assert!(got.is_none(), "must not cross-match platforms");
    }

    #[test]
    fn find_onnxruntime_dylib_rejects_directory_with_matching_name() {
        // Defensive: if somebody creates a *directory* named
        // `onnxruntime.dll` at the expected path (e.g. by extracting
        // an archive incorrectly), `is_file()` must reject it instead
        // of returning a "match" that can't actually be loaded.
        let tmp = tempfile::tempdir().expect("tempdir");
        let bogus = tmp
            .path()
            .join("libs")
            .join("onnxruntime")
            .join("win-x64")
            .join("onnxruntime.dll");
        std::fs::create_dir_all(&bogus).expect("create dir-as-file");

        let got = find_onnxruntime_dylib(
            &[tmp.path().to_path_buf()],
            "win-x64",
            "onnxruntime.dll",
        );
        assert!(got.is_none(), "directory must not be accepted as a DLL");
    }

    #[test]
    fn build_execution_providers_default_is_cpu_only() {
        // Default `rag` build (no extra `rag-*` features) has no EPs
        // registered explicitly — ort falls back to the implicit CPU
        // provider. This is the contract every accel feature builds
        // on, so guard it explicitly.
        #[cfg(not(any(
            feature = "rag-acl",
            feature = "rag-azure",
            feature = "rag-cann",
            feature = "rag-coreml",
            feature = "rag-cuda",
            feature = "rag-directml",
            feature = "rag-migraphx",
            feature = "rag-nnapi",
            feature = "rag-onednn",
            feature = "rag-openvino",
            feature = "rag-qnn",
            feature = "rag-rknpu",
            feature = "rag-rocm",
            feature = "rag-tensorrt",
            feature = "rag-tvm",
            feature = "rag-vitis",
            feature = "rag-webgpu",
            feature = "rag-xnnpack",
        )))]
        {
            let eps = build_execution_providers();
            assert!(eps.is_empty(), "expected zero EPs in plain rag build");
        }
    }
}
