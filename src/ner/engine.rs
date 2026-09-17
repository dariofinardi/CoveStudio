//! GLiNER2 engine singleton + entity extraction entry point.
//!
//! Pattern mirrors `crate::audio::transcribe::get_or_load_context`:
//! a process-wide `OnceLock<Mutex<Option<Arc<Gliner2Engine>>>>` so
//! the first call pays the ~500 MB model load + the FP16
//! WhisperContext-equivalent warmup, and every later call jumps
//! straight to inference. Heavy work runs on
//! `tokio::task::spawn_blocking` so the tokio worker pool stays
//! responsive while long documents are scanned.
//!
//! Cache layout: `hf-hub` (gliner2-rs transitive dep) drops the
//! weights under `$HF_HOME/...`. We set `HF_HOME` to
//! `%USERPROFILE%/cove-studio-data/gliner2/` at server startup so the
//! ~500 MB model lives next to the other heavy artefacts (fastembed,
//! whisper) and the Tauri watcher never sees it.

use anyhow::{anyhow, Context, Result};
use std::sync::{Arc, OnceLock};
use tokio::sync::{Mutex, RwLock};

use gliner2_inference::{
    ExtractedEntity, Gliner2Engine, InferenceParams, ModelType, SchemaTask,
};

/// Lower-than-default threshold for the GLiNER2 zero-shot scorer.
/// gliner2-rs ships with 0.5; with multilingual PII labels on
/// Italian medical text that suppresses most hits. 0.3 is the
/// "show me everything reasonable, I'll over-mask rather than leak"
/// trade-off the safer-by-default redaction needs.
const PII_THRESHOLD: f32 = 0.2;

use super::labels::default_pii_labels;

/// Snapshot of where the GLiNER2 engine is in its lifecycle. We
/// route through our own hf-hub-style downloader (`bootstrap.rs`)
/// so the `Downloading` state can carry real bytes/total — the
/// gliner2-rs `from_pretrained` is opaque on that front.
#[derive(Debug, Clone)]
pub enum NerStatus {
    /// No call has hit `ensure_engine` yet.
    Idle,
    /// Manual HF resolve of the V2 model shards is in flight.
    /// `total` is `None` when HEAD didn't surface Content-Length
    /// (rare; the UI falls back to an indeterminate progress bar).
    /// `file` is the current shard being streamed — `encoder` runs
    /// for several minutes at 500 Mb/s WAN, every other shard is
    /// short.
    Downloading {
        downloaded: u64,
        total: Option<u64>,
        file: String,
    },
    /// Files are cached on disk and `Gliner2Engine::new` is
    /// building the ort sessions. ~1-3 s on CPU.
    Loading,
    /// Engine is in memory; subsequent `mask_pii` calls jump
    /// straight to inference.
    Ready,
    /// Loading raised an error. UI shows the message; the next
    /// `ensure_engine` call resets to Loading and tries again.
    Failed { error: String },
}

/// Default HF model id for the privacy / PII task. Exported so
/// `super::bootstrap` can resolve files against the same repo as
/// `ensure_engine`. Pinned here so a future variant swap is a
/// one-line change.
pub(super) const PII_MODEL_ID: &str = "SemplificaAI/gliner2-privacy-filter-PII-multi";
pub(super) const PII_MODEL_VARIANT: &str = "fp16_v2";

/// Public reader used by the `/sync/ner-status` route.
pub async fn status() -> NerStatus {
    status_cell().read().await.clone()
}

fn status_cell() -> &'static RwLock<NerStatus> {
    static CELL: OnceLock<RwLock<NerStatus>> = OnceLock::new();
    CELL.get_or_init(|| RwLock::new(NerStatus::Idle))
}

/// A single PII / named-entity span. We expose only `label`, `score`
/// and the literal extracted `text` because gliner2-rs v0.5.0
/// `ExtractedEntity` carries **token** offsets (`start_tok` /
/// `end_tok`), not byte/char offsets — re-aligning tokens to char
/// positions would require holding the tokenizer alongside the
/// engine. For PII redaction (the only consumer in Phase 1) the
/// text is sufficient: we do a global string replace below. A future
/// char-offset API can come back via a tokenizer alignment pass.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Entity {
    pub label: String,
    pub score: f32,
    pub text: String,
}

/// Top-level extraction entry point. `labels = None` uses the
/// canonical PII set in `labels.rs`; `Some(&[...])` lets the caller
/// scope detection (e.g. only banking identifiers for a contract
/// redaction workflow).
pub async fn extract_entities(
    text: &str,
    labels: Option<&[&str]>,
) -> Result<Vec<Entity>> {
    let engine = ensure_engine().await?;
    let owned_labels: Vec<String> = labels
        .map(|l| l.iter().map(|s| s.to_string()).collect())
        .unwrap_or_else(|| {
            default_pii_labels()
                .iter()
                .map(|s| s.to_string())
                .collect()
        });
    let text_owned = text.to_string();

    tokio::task::spawn_blocking(move || run_pass(engine, &text_owned, owned_labels))
        .await
        .map_err(|e| anyhow!("ner task join: {e:?}"))?
}

/// GLiNER2's context window (characters). Documents longer than this
/// would be silently truncated by the model — we chunk client-side
/// and stitch entity spans back into a single redaction pass over
/// the original full text.
pub const GLINER2_WINDOW_CHARS: usize = 2000;

/// Overlap between adjacent chunks. Catches entities that straddle a
/// chunk boundary (e.g. a name split in two by an unlucky cut). A
/// 200-char overlap is generous given typical PII spans (5-60 chars)
/// and survives most boundary cases without inflating the inference
/// cost meaningfully.
pub const GLINER2_OVERLAP_CHARS: usize = 200;

/// Progress callback: `(current_chunk, total_chunks)`. Called inside
/// the blocking worker every time a chunk inference completes; the
/// chat send path uses it to emit `pii_redact_progress` SSE events
/// so the UI can render `n / N` against a long document.
/// `Send + Sync + 'static` to cross the `spawn_blocking` boundary.
pub type ProgressFn = Arc<dyn Fn(usize, usize) + Send + Sync + 'static>;

/// Convenience pipeline: extract PII spans across every chunk of the
/// input text, dedupe across overlap regions, then run the upstream
/// `mask_pii_text` once on the *original* text so the offsets stay
/// authoritative. Output: a redacted copy of `text` with every span
/// replaced by `[LABEL]` (e.g. `[PERSON]`, `[EMAIL]`).
///
/// `labels = None` uses the canonical PII set; pass a custom subset
/// to narrow the redaction (e.g. `&["fiscal_code","iban"]` only).
///
/// Long documents (PDFs of dozens of pages, audio transcripts of an
/// hour) safely route through this entry point — chunking is
/// transparent. The whole pass runs on `spawn_blocking` so the tokio
/// runtime stays responsive while a several-MB document is processed.
pub async fn mask_pii(
    text: &str,
    labels: Option<&[&str]>,
    progress: Option<ProgressFn>,
) -> Result<String> {
    let engine = ensure_engine().await?;
    let owned_labels: Vec<String> = labels
        .map(|l| l.iter().map(|s| s.to_string()).collect())
        .unwrap_or_else(|| {
            default_pii_labels()
                .iter()
                .map(|s| s.to_string())
                .collect()
        });
    let text_owned = text.to_string();

    tokio::task::spawn_blocking(move || {
        let chunks = chunk_for_window(
            &text_owned,
            GLINER2_WINDOW_CHARS,
            GLINER2_OVERLAP_CHARS,
        );
        if chunks.len() > 1 {
            tracing::info!(
                "[ner] chunking {} chars into {} windows of {} (overlap {})",
                text_owned.len(),
                chunks.len(),
                GLINER2_WINDOW_CHARS,
                GLINER2_OVERLAP_CHARS,
            );
        }
        let total = chunks.len();
        let tasks = vec![SchemaTask::Entities(owned_labels)];
        let mut all_entities: Vec<ExtractedEntity> = Vec::new();
        let pass_started_at = std::time::Instant::now();
        tracing::info!(
            "[ner] PII pass started — {} chunk(s) over {} chars",
            total,
            text_owned.len()
        );
        for (i, chunk) in chunks.iter().enumerate() {
            // Tick BEFORE the inference so the UI shows "1/N starting"
            // immediately rather than after the first chunk finishes
            // (per-chunk latency can be hundreds of ms each).
            if let Some(cb) = &progress {
                cb(i + 1, total);
            }
            let chunk_text = &text_owned[chunk.start..chunk.end];
            let chunk_started_at = std::time::Instant::now();
            tracing::info!(
                "[ner] chunk {}/{} → extracting ({} chars)",
                i + 1,
                total,
                chunk.end - chunk.start
            );
            // gliner2-rs `extract` takes (text, tasks, Option<params>).
            // We pass `None` so the engine uses its default
            // InferenceParams (threshold 0.5, flat_ner false). Future
            // work: expose a per-call threshold in the public API.
            let (entities, _r, _c) = engine
                .extract(
                    chunk_text,
                    &tasks,
                    Some(InferenceParams {
                        threshold: PII_THRESHOLD,
                        flat_ner: false,
                    }),
                )
                .map_err(|e| anyhow!("gliner2 extract failed on chunk: {e:?}"))?;
            for e in &entities {
                tracing::info!(
                    "[ner]     · entity label={:?} score={:.3} text={:?}",
                    e.label,
                    e.score,
                    e.text
                );
            }
            tracing::info!(
                "[ner] chunk {}/{} ✓ {} entities in {:?}",
                i + 1,
                total,
                entities.len(),
                chunk_started_at.elapsed()
            );
            all_entities.extend(entities);
        }
        tracing::info!(
            "[ner] PII pass done — {} entities total in {:?}",
            all_entities.len(),
            pass_started_at.elapsed()
        );
        Ok(redact_by_text(&text_owned, &all_entities))
    })
    .await
    .map_err(|e| anyhow!("ner mask task join: {e:?}"))?
}

/// Globally replace every entity's literal text in `source` with
/// `[LABEL]` (uppercase). Sort by length descending so a longer
/// entity ("Mario Rossi") is masked before the shorter prefix
/// ("Mario") that could otherwise hit the residual leftover. Dedup
/// `(text, label)` pairs so the same span found in multiple
/// overlapping chunks doesn't trigger redundant work. This is
/// safer-by-default than offset-based replacement: if the same
/// person name appears 5 times in the document and the model tagged
/// only one occurrence, we still mask all 5 — the alternative
/// (leaking 4 of them) is the wrong default for a redaction tool.
///
/// Limit: substring overmatch is possible (entity "Mar" hitting
/// "Marathon"). For Phase 1 PII labels — person names, emails,
/// fiscal codes, IBANs, phone numbers — this is rare. Phase 2 can
/// add tokenizer-aligned char offsets for stricter span control.
fn redact_by_text(source: &str, entities: &[ExtractedEntity]) -> String {
    use std::collections::HashSet;
    let mut seen = HashSet::<(String, String)>::new();
    let mut unique: Vec<(&str, &str)> = Vec::new();
    for e in entities {
        if e.text.trim().is_empty() {
            continue;
        }
        let key = (e.text.clone(), e.label.clone());
        if seen.insert(key) {
            unique.push((e.text.as_str(), e.label.as_str()));
        }
    }
    // Longest-first so "Mario Rossi" gets the mask before "Mario"
    // would catch it.
    unique.sort_by_key(|(t, _)| std::cmp::Reverse(t.len()));
    let mut out = source.to_string();
    for (text, label) in unique {
        let placeholder = format!("[{}]", label.to_uppercase());
        // String::replace is a non-overlapping left-to-right pass;
        // good enough for our single-pass redaction.
        out = out.replace(text, &placeholder);
    }
    out
}

/// One chunk window, in **byte** offsets into the source text.
/// `start..end` is always on UTF-8 char boundaries and the chunk
/// length never exceeds `window` chars.
#[derive(Debug, Clone, Copy)]
struct Chunk {
    start: usize,
    end: usize,
}

/// Split `text` into chunks of at most `window` characters with
/// `overlap` characters of slide between adjacent chunks. We split
/// at the nearest UTF-8 char boundary so a multi-byte character is
/// never cut in half — `engine.extract` would otherwise panic on
/// an invalid borrow.
///
/// For single-window inputs the returned slice is `[(0, text.len())]`
/// and the caller skips the stitching path entirely.
fn chunk_for_window(text: &str, window: usize, overlap: usize) -> Vec<Chunk> {
    debug_assert!(window > overlap, "overlap must be strictly less than window");
    if text.is_empty() {
        return Vec::new();
    }
    if text.chars().count() <= window {
        return vec![Chunk { start: 0, end: text.len() }];
    }

    // Walk char boundaries in steps of (window - overlap). Each
    // chunk covers `window` chars from its start, until we run out
    // of text. We track char positions and convert to byte offsets
    // by walking `char_indices` — keeps everything boundary-safe.
    let stride = window - overlap;
    let mut out = Vec::new();
    let total_chars = text.chars().count();
    let mut start_char = 0usize;
    while start_char < total_chars {
        let end_char = (start_char + window).min(total_chars);
        let start_byte = char_pos_to_byte(text, start_char);
        let end_byte = char_pos_to_byte(text, end_char);
        out.push(Chunk { start: start_byte, end: end_byte });
        if end_char == total_chars {
            break;
        }
        start_char += stride;
    }
    out
}

/// Convert a char index into the corresponding byte offset. Linear
/// scan — fine at our chunk sizes (~2000 chars), well below any
/// hot path.
fn char_pos_to_byte(text: &str, char_pos: usize) -> usize {
    if char_pos == 0 {
        return 0;
    }
    text.char_indices()
        .nth(char_pos)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

#[cfg(test)]
mod chunk_tests {
    use super::*;

    #[test]
    fn short_text_single_chunk() {
        let text = "Mario Rossi, mario@example.com";
        let chunks = chunk_for_window(text, 2000, 200);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].start, 0);
        assert_eq!(chunks[0].end, text.len());
    }

    #[test]
    fn long_text_chunks_with_overlap() {
        let text = "a".repeat(5000);
        let chunks = chunk_for_window(&text, 2000, 200);
        // 5000 chars, stride 1800: starts at 0, 1800, 3600 → 3 chunks.
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].start, 0);
        assert_eq!(chunks[0].end, 2000);
        assert_eq!(chunks[1].start, 1800);
        assert_eq!(chunks[1].end, 3800);
        assert_eq!(chunks[2].start, 3600);
        assert_eq!(chunks[2].end, 5000);
    }

    #[test]
    fn respects_utf8_boundaries() {
        // 4-byte emoji at every position — char_count == 1000,
        // byte_len == 4000. Window of 500 chars produces chunks
        // aligned on char boundaries, never on a byte mid-emoji.
        let emoji = "🚀".repeat(1000);
        let chunks = chunk_for_window(&emoji, 500, 50);
        for c in &chunks {
            assert!(emoji.is_char_boundary(c.start));
            assert!(emoji.is_char_boundary(c.end));
        }
    }

    #[test]
    fn empty_text_returns_no_chunks() {
        assert!(chunk_for_window("", 2000, 200).is_empty());
    }
}

fn run_pass(
    engine: Arc<Gliner2Engine>,
    text: &str,
    labels: Vec<String>,
) -> Result<Vec<Entity>> {
    // Chunk so long inputs don't get silently truncated by the
    // model's ~2000-char context window. Mirror of the `mask_pii`
    // codepath above; we just collect entities here instead of
    // running the text-replace pass.
    let chunks = chunk_for_window(text, GLINER2_WINDOW_CHARS, GLINER2_OVERLAP_CHARS);
    let tasks = vec![SchemaTask::Entities(labels)];
    let mut out: Vec<Entity> = Vec::new();
    for chunk in &chunks {
        let chunk_text = &text[chunk.start..chunk.end];
        let (entities, _r, _c) = engine
            .extract(
                chunk_text,
                &tasks,
                Some(InferenceParams {
                    threshold: PII_THRESHOLD,
                    flat_ner: false,
                }),
            )
            .map_err(|e| anyhow!("gliner2 extract failed: {e:?}"))?;
        for e in entities {
            out.push(Entity {
                label: e.label,
                score: e.score,
                text: e.text,
            });
        }
    }
    Ok(out)
}

/// Lazy-loaded process-wide engine. Wraps the underlying
/// `Gliner2Engine` in an `Arc` so the worker closure can move a
/// clone into `spawn_blocking` without holding the mutex across the
/// await. The mutex itself only protects the *creation* path; once
/// the engine exists, every call goes through a cheap read.
async fn ensure_engine() -> Result<Arc<Gliner2Engine>> {
    static CELL: OnceLock<Mutex<Option<Arc<Gliner2Engine>>>> = OnceLock::new();
    let cell = CELL.get_or_init(|| Mutex::new(None));
    let mut guard = cell.lock().await;
    if let Some(engine) = guard.as_ref() {
        // Already loaded by a previous call — make sure the status
        // reflects that even if the lifecycle code was edited later.
        *status_cell().write().await = NerStatus::Ready;
        return Ok(engine.clone());
    }
    tracing::info!(
        "[ner] loading GLiNER2 engine — model={} variant={}",
        PII_MODEL_ID,
        PII_MODEL_VARIANT
    );

    // Phase 1 — download. Our bootstrap publishes bytes/total so
    // the UI can render a real progress bar (gliner2-rs's own
    // `from_pretrained` is opaque). On a warm cache this returns
    // immediately and sets status to Loading.
    let models_dir = match super::bootstrap::ensure_default_model().await {
        Ok(d) => d,
        Err(e) => {
            let msg = format!("download failed: {e:#}");
            *status_cell().write().await =
                NerStatus::Failed { error: msg.clone() };
            return Err(e);
        }
    };

    // Phase 2 — session build. `Gliner2Engine::new` autodetects V1
    // vs V2 from the file names already on disk in models_dir.
    *status_cell().write().await = NerStatus::Loading;
    let config = gliner2_inference::Gliner2Config {
        models_dir: models_dir.to_string_lossy().to_string(),
        max_width: 8,
        model_type: ModelType::HuggingFace,
    };
    let result = Gliner2Engine::new(config).with_context(|| {
        format!(
            "building GLiNER2 sessions from {}",
            models_dir.display()
        )
    });
    match result {
        Ok(engine) => {
            let arc = Arc::new(engine);
            *guard = Some(arc.clone());
            *status_cell().write().await = NerStatus::Ready;
            Ok(arc)
        }
        Err(e) => {
            let msg = format!("{e:#}");
            *status_cell().write().await =
                NerStatus::Failed { error: msg.clone() };
            Err(e)
        }
    }
}
