// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
//! GLiNER2 engine lifecycle + the PII masking pass.
//!
//! The engine lives on **one dedicated OS thread** for the life of the
//! process, and callers talk to it by message. That is not a style
//! choice: `SpanEngine` is not `Send` (it holds an ONNX Runtime
//! `MemoryInfo` pointer), so it can neither sit in a shared static nor
//! cross into `spawn_blocking`. Owning it on a single thread also
//! serialises inference for free — `extract_with` takes `&mut self` —
//! and keeps a panic inside the model from poisoning a lock the rest of
//! the application would then trip over.
//!
//! Redaction happens in two steps, deliberately:
//!
//! 1. `gliner2_rs::privacy::redact` replaces each detected span by
//!    offset, resolving overlaps in favour of the highest score (so
//!    `Giuseppe Verdi` becomes `[FULL_NAME]`, not
//!    `[FIRST_NAME] [LAST_NAME]`).
//! 2. Our own pass then replaces **every remaining literal occurrence**
//!    of each detected value. The model routinely tags a name on its
//!    first appearance and misses the fourth; offset-only redaction
//!    would leave that fourth one in clear text. For a tool whose
//!    purpose is not leaking, masking a few false positives is the
//!    cheaper mistake. The user tunes detection breadth with the
//!    threshold in Settings → Sicurezza; this pass is not optional.

use std::sync::mpsc;
use std::sync::{Arc, OnceLock, RwLock};

use anyhow::{anyhow, Result};
use gliner2_rs::{
    chain::ExecutionMode,
    chunker::{self, Chunker},
    privacy::{self, Group},
    InferenceParams, SchemaTask, SpanConfig, SpanEngine, SpanOutput,
};
use tokio::sync::Mutex;

/// Groups of the model's own PII vocabulary that we ask for. Labels
/// compete *inside* a group, so one `SchemaTask` per group scores far
/// better than one flat list of all 42 labels — that is the upstream
/// recommendation, not a guess.
///
/// `DigitalIdentity`, `Secrets` and `SensitiveDates` are left out: they
/// cover credentials, API keys and event dates, which in professional
/// documents produce more noise than protection (a contract is made of
/// dates). A caller that needs them passes explicit labels.
const PII_GROUPS: &[Group] = &[
    Group::Person,
    Group::Contact,
    Group::GovernmentId,
    Group::Banking,
];

/// Labels outside the model's trained vocabulary that matter for
/// Italian professional documents. They stay in a task of their own:
/// zero-shot detection still works, but mixing out-of-distribution
/// labels into a trained group degrades that group's own scores.
const EXTRA_LABELS: &[&str] = &["codice fiscale", "partita IVA", "targa", "patient name"];

/// Chunk geometry, in **words** — the engine counts words, not
/// characters. The encoder has 512 positions and the schema markers
/// share them with the text, so the window must leave room for the
/// labels we send, and we send five tasks. 256/48 is the geometry the
/// library documents for a schema this size; the previous 2000/200
/// *characters* were tuned against a different engine.
const CHUNK_WORDS: usize = 256;
const CHUNK_OVERLAP_WORDS: usize = 48;

/// Where the engine is in its lifecycle. Mirrors what
/// `/sync/ner-status` renders, so the UI can show a real progress bar
/// during the first download instead of an opaque spinner.
#[derive(Debug, Clone)]
pub enum NerStatus {
    /// No call has hit `ensure_engine` yet.
    Idle,
    /// Manual HF resolve of the model shards is in flight. `total` is
    /// `None` when HEAD didn't surface Content-Length (rare; the UI
    /// falls back to an indeterminate bar). `file` is the shard being
    /// streamed — `encoder` runs for minutes, the rest are short.
    Downloading {
        downloaded: u64,
        total: Option<u64>,
        file: String,
    },
    /// Files are cached and the ort sessions are being built.
    Loading,
    /// Engine is in memory; later calls jump straight to inference.
    Ready,
    /// Loading raised an error. The next call retries.
    Failed { error: String },
}

/// HF model id and variant for the privacy / PII task. We keep the
/// original repository rather than the library's new default
/// (`jugaadsrl/…-onnx`): the weights already on disk came from here,
/// the file names are identical, and switching would re-download
/// 1.1 GB for nothing.
pub(super) const PII_MODEL_ID: &str = "SemplificaAI/gliner2-privacy-filter-PII-multi";
pub(super) const PII_MODEL_VARIANT: &str = "fp16_v2";

/// Public reader used by the `/sync/ner-status` route. Kept `async` for
/// its callers' sake; the lock itself is synchronous because the engine
/// thread — which lives outside the tokio runtime — writes it.
pub async fn status() -> NerStatus {
    read_status()
}

fn read_status() -> NerStatus {
    status_cell()
        .read()
        .map(|g| g.clone())
        // A poisoned lock means some thread panicked while writing the
        // status, not that the engine is unusable; reporting Idle lets
        // the next call retry instead of wedging the feature.
        .unwrap_or(NerStatus::Idle)
}

fn set_status(next: NerStatus) {
    if let Ok(mut g) = status_cell().write() {
        *g = next;
    }
}

fn status_cell() -> &'static RwLock<NerStatus> {
    static CELL: OnceLock<RwLock<NerStatus>> = OnceLock::new();
    CELL.get_or_init(|| RwLock::new(NerStatus::Idle))
}

/// A single detected span. Carries real offsets now — the previous
/// engine exposed token indices only, which is why redaction used to
/// work on literal text alone.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Entity {
    pub label: String,
    pub score: f32,
    pub text: String,
    /// Byte offsets into the text that was passed in, half-open.
    pub start: usize,
    pub end: usize,
}

/// Progress callback: `(current_chunk, total_chunks)`, called from the
/// engine thread as each chunk completes. The chat send path turns
/// these into `pii_redact_progress` SSE events.
pub type ProgressFn = Arc<dyn Fn(usize, usize) + Send + Sync + 'static>;

/// Tasks handed to the model: one per trained group, plus our
/// out-of-distribution extras. `labels = Some(...)` replaces the whole
/// set with a single caller-scoped task.
fn tasks_for(labels: Option<Vec<String>>) -> Vec<SchemaTask> {
    if let Some(custom) = labels {
        return vec![SchemaTask::Entities(custom)];
    }
    let mut tasks: Vec<SchemaTask> = PII_GROUPS.iter().map(|g| g.task()).collect();
    tasks.push(SchemaTask::Entities(
        EXTRA_LABELS.iter().map(|s| s.to_string()).collect(),
    ));
    tasks
}

fn params_for(threshold: f32) -> InferenceParams {
    InferenceParams {
        threshold,
        // `flat_ner: false` keeps overlapping labels alive; the
        // redaction pass resolves them by score. `overlap_policy` stays
        // at its default, which matches the reference implementation.
        flat_ner: false,
        ..Default::default()
    }
}

/// One inference request for the engine thread.
struct Job {
    text: String,
    tasks: Vec<SchemaTask>,
    threshold: f32,
    progress: Option<ProgressFn>,
    reply: tokio::sync::oneshot::Sender<Result<SpanOutput>>,
}

/// Handle on the thread that owns the engine.
#[derive(Clone)]
struct EngineHandle {
    jobs: mpsc::Sender<Job>,
}

impl EngineHandle {
    async fn run(
        &self,
        text: String,
        tasks: Vec<SchemaTask>,
        threshold: f32,
        progress: Option<ProgressFn>,
    ) -> Result<SpanOutput> {
        let (reply, reply_rx) = tokio::sync::oneshot::channel();
        self.jobs
            .send(Job {
                text,
                tasks,
                threshold,
                progress,
                reply,
            })
            .map_err(|_| anyhow!("ner engine thread is gone"))?;
        reply_rx
            .await
            .map_err(|_| anyhow!("ner engine dropped the request without answering"))?
    }
}

/// Detected spans, without redacting. `labels = None` uses the PII
/// groups above.
pub async fn extract_entities(
    text: &str,
    labels: Option<&[&str]>,
    threshold: f32,
) -> Result<Vec<Entity>> {
    let engine = ensure_engine().await?;
    let tasks = tasks_for(labels.map(|l| l.iter().map(|s| s.to_string()).collect()));
    let output = engine.run(text.to_string(), tasks, threshold, None).await?;
    Ok(output
        .entities
        .into_iter()
        .map(|e| Entity {
            label: e.label,
            score: e.score,
            text: e.text,
            start: e.char_start,
            end: e.char_end,
        })
        .collect())
}

/// Masks every detected value in `text` and returns the redacted copy.
///
/// `threshold` is the user's setting (`user_settings.pii_threshold`,
/// migration 0035): lower masks more. `labels = None` uses the PII
/// groups; pass a subset to narrow the pass.
///
/// Long documents are chunked transparently, and the work happens on
/// the engine thread, so a multi-MB document never stalls the runtime.
pub async fn mask_pii(
    text: &str,
    labels: Option<&[&str]>,
    threshold: f32,
    progress: Option<ProgressFn>,
) -> Result<String> {
    let engine = ensure_engine().await?;
    let tasks = tasks_for(labels.map(|l| l.iter().map(|s| s.to_string()).collect()));
    let started_at = std::time::Instant::now();
    let output = engine
        .run(text.to_string(), tasks, threshold, progress)
        .await?;
    tracing::info!(
        "[ner] PII pass done — {} spans in {:?} (threshold {threshold:.2})",
        output.entities.len(),
        started_at.elapsed()
    );
    // Step 1: offset-accurate redaction, overlaps resolved by score.
    // `privacy::redact` would upper-case the label as it stands, which
    // turns our multi-word extras into `[CODICE FISCALE]` next to the
    // model's `[FULL_NAME]`. One shape for all of them.
    let redacted = privacy::redact_with(text, &output.entities, |label| placeholder_for(label));
    // Step 2: the occurrences the model did not tag.
    Ok(mask_remaining_occurrences(redacted, &output.entities))
}

/// The placeholder that replaces a detected value: the label in
/// upper case, with spaces and hyphens folded to underscores so the
/// model's own labels (`full_name`) and our multi-word extras
/// (`codice fiscale`) read the same way in the redacted document.
fn placeholder_for(label: &str) -> String {
    let mut out = String::with_capacity(label.len() + 2);
    out.push('[');
    for ch in label.chars() {
        match ch {
            ' ' | '-' => out.push('_'),
            c => out.extend(c.to_uppercase()),
        }
    }
    out.push(']');
    out
}

/// A detected value has to be at least this long before we mask it
/// document-wide. Names, emails, IBANs and tax codes all clear it;
/// initials and stray fragments do not, and replacing those everywhere
/// would shred unrelated words.
const MIN_GLOBAL_MASK_CHARS: usize = 4;

/// Replaces every literal occurrence of a detected value that survived
/// step 1, longest value first so `Mario Rossi` is masked before the
/// bare `Mario` can claim part of it.
fn mask_remaining_occurrences(mut text: String, entities: &[gliner2_rs::Entity]) -> String {
    use std::collections::HashSet;

    let mut seen: HashSet<(&str, &str)> = HashSet::new();
    let mut values: Vec<(&str, &str)> = Vec::new();
    for e in entities {
        let value = e.text.trim();
        if value.chars().count() < MIN_GLOBAL_MASK_CHARS {
            continue;
        }
        if seen.insert((value, e.label.as_str())) {
            values.push((value, e.label.as_str()));
        }
    }
    values.sort_by_key(|(v, _)| std::cmp::Reverse(v.len()));

    let mut replaced = 0usize;
    for (value, label) in values {
        if !text.contains(value) {
            continue;
        }
        let placeholder = placeholder_for(label);
        text = text.replace(value, &placeholder);
        replaced += 1;
    }
    if replaced > 0 {
        tracing::info!(
            "[ner] over-masking pass replaced further occurrences of {replaced} value(s)"
        );
    }
    text
}

/// Runs the model over `text`, chunking when needed. Reproduces
/// `SpanEngine::extract_long_with` so we can tick `progress` between
/// chunks — the library's own loop has no hook — while reusing its
/// public `remap` / `merge`, because hand-rolling seam handling is how
/// entities get lost.
fn run_pass(
    engine: &mut SpanEngine,
    text: &str,
    tasks: &[SchemaTask],
    threshold: f32,
    progress: Option<ProgressFn>,
) -> Result<SpanOutput> {
    let params = params_for(threshold);
    let chunker = Chunker::new(CHUNK_WORDS, CHUNK_OVERLAP_WORDS)
        .map_err(|e| anyhow!("chunker {CHUNK_WORDS}/{CHUNK_OVERLAP_WORDS}: {e:?}"))?;
    let chunks = chunker
        .split(text)
        .map_err(|e| anyhow!("chunking failed: {e:?}"))?;
    let total = chunks.len().max(1);

    if chunks.len() <= 1 {
        if let Some(cb) = &progress {
            cb(1, 1);
        }
        return engine
            .extract_with(text, tasks, &params)
            .map_err(|e| anyhow!("gliner2 extract failed: {e:?}"));
    }

    tracing::info!(
        "[ner] PII pass — {total} chunk(s) of {CHUNK_WORDS} words (overlap {CHUNK_OVERLAP_WORDS}) over {} chars",
        text.len()
    );
    let mut parts = Vec::with_capacity(chunks.len());
    for (i, chunk) in chunks.iter().enumerate() {
        // Tick before inference so the UI shows "1/N" immediately
        // rather than after the first chunk has finished.
        if let Some(cb) = &progress {
            cb(i + 1, total);
        }
        let started_at = std::time::Instant::now();
        let mut part = engine
            .extract_with(chunk.slice(text), tasks, &params)
            .map_err(|e| anyhow!("gliner2 extract failed on chunk {}/{total}: {e:?}", i + 1))?;
        // Offsets come back relative to the chunk; remap them onto the
        // full text before merging.
        chunker::remap(&mut part, chunk, text);
        tracing::debug!(
            "[ner] chunk {}/{total} ✓ {} spans in {:?}",
            i + 1,
            part.entities.len(),
            started_at.elapsed()
        );
        parts.push(part);
    }
    Ok(chunker::merge(parts))
}

/// Starts the engine thread on first use and hands back a handle;
/// later calls reuse it. A failed load is not cached, so the next call
/// retries — which matters when the failure was a half-finished
/// download.
async fn ensure_engine() -> Result<EngineHandle> {
    static CELL: OnceLock<Mutex<Option<EngineHandle>>> = OnceLock::new();
    let cell = CELL.get_or_init(|| Mutex::new(None));
    let mut guard = cell.lock().await;
    if let Some(handle) = guard.as_ref() {
        set_status(NerStatus::Ready);
        return Ok(handle.clone());
    }
    tracing::info!(
        "[ner] loading GLiNER2 engine — model={PII_MODEL_ID} variant={PII_MODEL_VARIANT}"
    );

    // Phase 1 — download, through our own resolver so the UI gets real
    // bytes/total (the library's downloader only prints to stderr).
    let models_dir = match super::bootstrap::ensure_default_model().await {
        Ok(d) => d,
        Err(e) => {
            set_status(NerStatus::Failed {
                error: format!("download failed: {e:#}"),
            });
            return Err(e);
        }
    };

    // Phase 2 — build the sessions on the thread that will own them.
    //
    // Same hazard the embedding path documents: with `ORT_DYLIB_PATH`
    // unset, `ort` resolves "onnxruntime.dll" by name and finds
    // Windows' own copy in System32 (ORT 1.17.x, shipped with Windows
    // ML), which rc.13 then rejects as `BadVersion`. The application
    // sets the variable at startup, but this module must not depend on
    // who called it first.
    #[cfg(feature = "rag")]
    crate::embeddings::service::ensure_onnxruntime_dylib_path();
    #[cfg(not(feature = "rag"))]
    if std::env::var_os("ORT_DYLIB_PATH").is_none() {
        tracing::warn!(
            "[ner] ORT_DYLIB_PATH is unset and this build has no `rag` feature to resolve it —              ort will look for onnxruntime.dll by name and may load the system copy"
        );
    }
    set_status(NerStatus::Loading);
    let (jobs_tx, jobs_rx) = mpsc::channel::<Job>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
    std::thread::Builder::new()
        .name("cove-ner".to_string())
        .spawn(move || engine_thread(models_dir, jobs_rx, ready_tx))
        .map_err(|e| anyhow!("spawning the ner engine thread: {e}"))?;

    match ready_rx.await {
        Ok(Ok(())) => {
            let handle = EngineHandle { jobs: jobs_tx };
            *guard = Some(handle.clone());
            set_status(NerStatus::Ready);
            Ok(handle)
        }
        Ok(Err(msg)) => {
            set_status(NerStatus::Failed { error: msg.clone() });
            Err(anyhow!(msg))
        }
        Err(_) => {
            let msg = "ner engine thread died while loading the model".to_string();
            set_status(NerStatus::Failed { error: msg.clone() });
            Err(anyhow!(msg))
        }
    }
}

/// Body of the engine thread: build once, then serve jobs until every
/// handle is dropped, which in practice means process shutdown.
fn engine_thread(
    models_dir: std::path::PathBuf,
    jobs: mpsc::Receiver<Job>,
    ready: tokio::sync::oneshot::Sender<Result<(), String>>,
) {
    // Execution mode is explicit on purpose. Left alone, the library
    // reads `GLINER2_DEVICE` as "auto", concludes a device-memory
    // provider is available and registers CUDA, then binds tensors to a
    // device this machine may not have; ONNX Runtime falls back to CPU
    // for compute while the binding is meaningless, and we would be
    // relying on an error path to recover. Cove Studio runs its models
    // on the CPU by design, so we say so.
    let config = SpanConfig::new(&models_dir).with_execution(ExecutionMode::Standard);
    let mut engine = match SpanEngine::new(config) {
        Ok(e) => {
            let _ = ready.send(Ok(()));
            e
        }
        Err(e) => {
            let _ = ready.send(Err(format!(
                "building GLiNER2 sessions from {}: {e:#}",
                models_dir.display()
            )));
            return;
        }
    };

    while let Ok(job) = jobs.recv() {
        let Job {
            text,
            tasks,
            threshold,
            progress,
            reply,
        } = job;
        let result = run_pass(&mut engine, &text, &tasks, threshold, progress);
        // A caller that gave up (disconnected client) simply drops the
        // receiver; that is not worth logging loudly.
        let _ = reply.send(result);
    }
    tracing::debug!("[ner] engine thread exiting — no handles left");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(text: &str, label: &str, start: usize) -> gliner2_rs::Entity {
        gliner2_rs::Entity {
            text: text.to_string(),
            label: label.to_string(),
            score: 0.9,
            char_start: start,
            char_end: start + text.len(),
            word_start: 0,
            word_end: 0,
            slot: 0,
            task: "entities".to_string(),
        }
    }

    #[test]
    fn over_masking_catches_untagged_repeats() {
        // The model tagged the first occurrence only; the others must
        // not survive.
        let redacted = "[FULL_NAME] firmò. Mario Rossi confermò. Scritto da Mario Rossi.";
        let out = mask_remaining_occurrences(
            redacted.to_string(),
            &[entity("Mario Rossi", "full_name", 0)],
        );
        assert!(!out.contains("Mario Rossi"), "{out}");
        assert_eq!(out.matches("[FULL_NAME]").count(), 3);
    }

    #[test]
    fn longest_value_is_masked_first() {
        let out = mask_remaining_occurrences(
            "Mario Rossi e Mario".to_string(),
            &[
                entity("Mario", "first_name", 0),
                entity("Mario Rossi", "full_name", 0),
            ],
        );
        assert_eq!(out, "[FULL_NAME] e [FIRST_NAME]");
    }

    #[test]
    fn very_short_values_are_left_alone() {
        // "MR" as initials would otherwise shred every word containing
        // those letters.
        let out =
            mask_remaining_occurrences("MR MRI MRS".to_string(), &[entity("MR", "person", 0)]);
        assert_eq!(out, "MR MRI MRS");
    }

    #[test]
    fn placeholders_have_one_shape() {
        assert_eq!(placeholder_for("full_name"), "[FULL_NAME]");
        assert_eq!(placeholder_for("codice fiscale"), "[CODICE_FISCALE]");
        assert_eq!(placeholder_for("date-of-birth"), "[DATE_OF_BIRTH]");
        // Accented labels are not expected, but must not be dropped.
        assert_eq!(placeholder_for("città"), "[CITTÀ]");
    }

    #[test]
    fn nothing_to_mask_leaves_text_untouched() {
        let out = mask_remaining_occurrences("Testo pulito".to_string(), &[]);
        assert_eq!(out, "Testo pulito");
    }

    #[test]
    fn default_tasks_cover_the_trained_groups_plus_extras() {
        assert_eq!(tasks_for(None).len(), PII_GROUPS.len() + 1);
    }

    #[test]
    fn custom_labels_replace_the_whole_schema() {
        assert_eq!(tasks_for(Some(vec!["iban".to_string()])).len(), 1);
    }
}
