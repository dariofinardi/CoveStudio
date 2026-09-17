//! Embedding-pipeline diagnostic: load each cached model variant
//! and measure the time taken by every step of the pipeline.
//!
//! The user reported indefinite hang on chat after the
//! `[rag] building ONNX session` log. Both the FP32 intfloat model
//! and the INT8-dynamic Xenova model exhibited the same symptom on
//! a Snapdragon X Elite ARM64 setup after swapping in a clean base
//! CPU onnxruntime.dll. The chat-handler pathway doesn't surface
//! WHERE exactly it hangs because the stall is inside the
//! `spawn_blocking` that wraps fastembed's `try_new_from_user_defined`.
//!
//! This test bypasses the chat handler and exercises the same
//! fastembed call directly, with `eprintln!`-style timing at every
//! sub-step. The output identifies which model variant works on
//! this machine and how long each phase takes — concrete data to
//! replace guesswork.
//!
//! How to run:
//!   cargo test --test embedding_perf -- --ignored --nocapture
//!
//! Ignored by default because each variant takes 5-30 s and loads
//! the full 1.1 GB / 275 MB ONNX into RAM — too heavy for routine
//! CI. Run on-demand when diagnosing embedding-pipeline regressions.

#![cfg(feature = "rag")]

use std::path::PathBuf;
use std::time::Instant;

use fastembed::{
    InitOptionsUserDefined, Pooling, QuantizationMode, TextEmbedding, TokenizerFiles,
    UserDefinedEmbeddingModel,
};

/// Resolve a cached model directory under the product data folder
/// (`fastembed/<subdir>/`). Skips the test (via `Result<…, &'static str>`)
/// when the directory is missing so the test is a no-op on machines
/// that haven't downloaded the model yet.
fn cache_dir(subdir: &str) -> Result<PathBuf, &'static str> {
    cove_studio::product::home_dir().ok_or("no USERPROFILE or HOME env var")?;
    let p: PathBuf = cove_studio::product::data_subdir("fastembed").join(subdir);
    if !p.is_dir() {
        return Err("cache dir not present — run the app once to download the model");
    }
    Ok(p)
}

struct ModelFiles {
    onnx: Vec<u8>,
    tokenizer: Vec<u8>,
    config: Vec<u8>,
    special_tokens_map: Vec<u8>,
    tokenizer_config: Vec<u8>,
}

fn load_files(dir: &PathBuf, onnx_filename: &str) -> std::io::Result<ModelFiles> {
    Ok(ModelFiles {
        onnx: std::fs::read(dir.join(onnx_filename))?,
        tokenizer: std::fs::read(dir.join("tokenizer.json"))?,
        config: std::fs::read(dir.join("config.json"))?,
        special_tokens_map: std::fs::read(dir.join("special_tokens_map.json"))?,
        tokenizer_config: std::fs::read(dir.join("tokenizer_config.json"))?,
    })
}

/// Resolve and export `ORT_DYLIB_PATH` so fastembed/ort can find
/// the vendored runtime DLL even when run from cargo test (which
/// doesn't go through `lib::run_server_with_channels`, where the
/// service ordinarily sets this).
fn ensure_ort_dylib() {
    if std::env::var("ORT_DYLIB_PATH").is_ok() {
        eprintln!("[ort] ORT_DYLIB_PATH already set");
        return;
    }
    // Walk ancestors of the workspace looking for libs/onnxruntime/<sub>/<file>.
    let (sub, file) = if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
        ("win-arm64", "onnxruntime.dll")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        ("win-x64", "onnxruntime.dll")
    } else if cfg!(target_os = "macos") {
        ("macos-arm64", "libonnxruntime.dylib")
    } else {
        ("linux-x64", "libonnxruntime.so")
    };
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let candidate = manifest_dir
        .join("libs")
        .join("onnxruntime")
        .join(sub)
        .join(file);
    if candidate.is_file() {
        let bytes = std::fs::metadata(&candidate)
            .map(|m| m.len() / (1024 * 1024))
            .unwrap_or(0);
        eprintln!("[ort] setting ORT_DYLIB_PATH={} ({} MB)", candidate.display(), bytes);
        // SAFETY: single-threaded test setup before any ort call.
        unsafe {
            std::env::set_var("ORT_DYLIB_PATH", &candidate);
        }
    } else {
        eprintln!("[ort] WARNING: {} not found — ort will fail to load", candidate.display());
    }
}

/// Heart of the diagnostic: load files, build the fastembed model,
/// time every step. Prints to stderr (visible with `--nocapture`).
/// Returns the embed query time so it can be compared across variants.
fn time_pipeline(label: &str, dir: &PathBuf, onnx_filename: &str) {
    eprintln!("\n===== {label} =====");
    ensure_ort_dylib();
    eprintln!("[t=0] cache dir: {}", dir.display());

    let t_io = Instant::now();
    let files = match load_files(dir, onnx_filename) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[SKIP] cannot read files: {e}");
            return;
        }
    };
    eprintln!(
        "[t={:>6} ms] file read complete (onnx={} MB, tokenizer={} KB, config={} B)",
        t_io.elapsed().as_millis(),
        files.onnx.len() / (1024 * 1024),
        files.tokenizer.len() / 1024,
        files.config.len(),
    );

    let t_struct = Instant::now();
    let model = UserDefinedEmbeddingModel {
        onnx_file: files.onnx,
        external_initializers: vec![],
        tokenizer_files: TokenizerFiles {
            tokenizer_file: files.tokenizer,
            config_file: files.config,
            special_tokens_map_file: files.special_tokens_map,
            tokenizer_config_file: files.tokenizer_config,
        },
        pooling: Some(Pooling::Mean),
        quantization: QuantizationMode::None,
        output_key: None,
    };
    let opts = InitOptionsUserDefined::new()
        .with_max_length(512)
        .with_execution_providers(vec![]);
    eprintln!(
        "[t={:>6} ms] UserDefinedEmbeddingModel + InitOptions assembled (now calling try_new)",
        t_struct.elapsed().as_millis(),
    );

    let t_session = Instant::now();
    let mut embedding = match TextEmbedding::try_new_from_user_defined(model, opts) {
        Ok(e) => {
            eprintln!(
                "[t={:>6} ms] try_new_from_user_defined: OK (session built)",
                t_session.elapsed().as_millis(),
            );
            e
        }
        Err(err) => {
            eprintln!(
                "[t={:>6} ms] try_new_from_user_defined: FAILED — {err}",
                t_session.elapsed().as_millis(),
            );
            return;
        }
    };

    // ── First-shot embed (single query, what the chat does on every turn).
    let t_q1 = Instant::now();
    let q1 = embedding.embed(vec!["query: polizza Allianz".to_string()], Some(1));
    match q1 {
        Ok(v) => {
            assert_eq!(v.len(), 1, "expected one vector");
            assert_eq!(v[0].len(), 768, "e5-base produces 768-d vectors");
            eprintln!(
                "[t={:>6} ms] first embed_query (cold ort path): OK, {} dims",
                t_q1.elapsed().as_millis(),
                v[0].len(),
            );
        }
        Err(e) => {
            eprintln!(
                "[t={:>6} ms] first embed_query: FAILED — {e}",
                t_q1.elapsed().as_millis(),
            );
            return;
        }
    }

    // ── Second-shot embed — measures warm/cached cost.
    let t_q2 = Instant::now();
    let _ = embedding.embed(vec!["query: contraente A.TEC. S.r.l.".to_string()], Some(1));
    eprintln!(
        "[t={:>6} ms] second embed_query (warm): OK",
        t_q2.elapsed().as_millis(),
    );

    // ── Batch embed of 16 passages — what indexing pays per chunk.
    let t_batch = Instant::now();
    let passages: Vec<String> = (0..16)
        .map(|i| format!("passage: clausola n. {i} della polizza Allianz applicazioni"))
        .collect();
    let _ = embedding.embed(passages, Some(16));
    eprintln!(
        "[t={:>6} ms] batch embed 16 passages: OK ({} ms per passage avg)",
        t_batch.elapsed().as_millis(),
        t_batch.elapsed().as_millis() / 16,
    );
}

/// Build a fastembed model from a cache directory, returning None on any
/// failure (skipped cache, IO error, session build failure). Shared by
/// the timing test and the quality test.
fn build_model(dir: &PathBuf, onnx_filename: &str) -> Option<TextEmbedding> {
    ensure_ort_dylib();
    let files = load_files(dir, onnx_filename).ok()?;
    let model = UserDefinedEmbeddingModel {
        onnx_file: files.onnx,
        external_initializers: vec![],
        tokenizer_files: TokenizerFiles {
            tokenizer_file: files.tokenizer,
            config_file: files.config,
            special_tokens_map_file: files.special_tokens_map,
            tokenizer_config_file: files.tokenizer_config,
        },
        pooling: Some(Pooling::Mean),
        quantization: QuantizationMode::None,
        output_key: None,
    };
    let opts = InitOptionsUserDefined::new()
        .with_max_length(512)
        .with_execution_providers(vec![]);
    TextEmbedding::try_new_from_user_defined(model, opts).ok()
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na * nb)
}

#[test]
#[ignore = "loads heavy models; run on-demand with --ignored --nocapture"]
fn perf_fp32_intfloat() {
    let dir = match cache_dir("e5-base") {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[SKIP FP32] {e}");
            return;
        }
    };
    time_pipeline("FP32 (intfloat/multilingual-e5-base, model.onnx, ~1.1 GB)", &dir, "model.onnx");
}

#[test]
#[ignore = "loads heavy models; run on-demand with --ignored --nocapture"]
fn perf_int8_xenova() {
    let dir = match cache_dir("e5-base-quantized") {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[SKIP INT8] {e}");
            return;
        }
    };
    time_pipeline(
        "INT8 dynamic (Xenova/multilingual-e5-base, model_quantized.onnx, ~275 MB)",
        &dir,
        "model_quantized.onnx",
    );
}

/// FP32-vs-INT8 quality comparison. For a curated set of Italian
/// legal/insurance text, this measures:
///
///   1. **Cross-model drift** — for each identical text, the cosine
///      between its FP32 embedding and its INT8 embedding. A clean
///      INT8 dynamic quantization should leave drift ≥ 0.97; below
///      0.95 means quantization is materially degrading the semantic
///      geometry.
///
///   2. **Ranking preservation** — for several realistic queries,
///      whether the top-1 passage selected by INT8 matches the one
///      selected by FP32. This is the metric that actually predicts
///      retrieval quality in the RAG pipeline; same-text drift can
///      be high while top-1 still agrees, and conversely a low drift
///      doesn't guarantee correct ranking if it's biased.
///
/// Loads both models simultaneously (~1.3 GB RAM peak), so it lives
/// behind `--ignored` like the per-model timing tests.
#[test]
#[ignore = "loads both FP32 and INT8 models (~1.3 GB RAM); --ignored --nocapture"]
fn quality_fp32_vs_int8() {
    let fp32_dir = match cache_dir("e5-base") {
        Ok(d) => d,
        Err(e) => { eprintln!("[SKIP QUALITY] FP32 cache: {e}"); return; }
    };
    let int8_dir = match cache_dir("e5-base-quantized") {
        Ok(d) => d,
        Err(e) => { eprintln!("[SKIP QUALITY] INT8 cache: {e}"); return; }
    };

    eprintln!("\n===== Quality: FP32 vs INT8 (multilingual-e5-base) =====");
    let t = Instant::now();
    let mut fp32 = match build_model(&fp32_dir, "model.onnx") {
        Some(m) => m,
        None => { eprintln!("[SKIP QUALITY] FP32 model build failed"); return; }
    };
    eprintln!("[t={:>5} ms] FP32 model ready", t.elapsed().as_millis());

    let t2 = Instant::now();
    let mut int8 = match build_model(&int8_dir, "model_quantized.onnx") {
        Some(m) => m,
        None => { eprintln!("[SKIP QUALITY] INT8 model build failed"); return; }
    };
    eprintln!("[t={:>5} ms] INT8 model ready", t2.elapsed().as_millis());

    // ── Curated Italian legal/insurance corpus. Queries use the e5
    // "query:" prefix; passages use "passage:" — required by the
    // asymmetric retrieval training of multilingual-e5-base.
    let queries: Vec<(&str, &str)> = vec![
        ("query: chi è il contraente della polizza?", "Q_PARTY"),
        ("query: qual è la franchigia per danni materiali?", "Q_DEDUCT"),
        ("query: qual è il foro competente?", "Q_VENUE"),
        ("query: l'IVA è inclusa nel premio?", "Q_TAX"),
    ];
    let passages: Vec<(&str, &str)> = vec![
        ("passage: il contraente della polizza è A.TEC. S.r.l., con sede legale in Milano.", "P_PARTY"),
        ("passage: la franchigia per danni materiali ammonta a € 500 per sinistro.", "P_DEDUCT"),
        ("passage: per ogni controversia il foro competente è il Tribunale di Milano.", "P_VENUE"),
        ("passage: il premio annuale è di € 3.200, IVA esclusa.", "P_TAX"),
        ("passage: la durata della polizza è triennale con tacito rinnovo annuale.", "P_DURATION_DISTRACTOR"),
    ];

    let all_texts: Vec<String> = queries.iter().chain(passages.iter()).map(|(t, _)| t.to_string()).collect();
    let labels: Vec<&str>      = queries.iter().chain(passages.iter()).map(|(_, l)| *l).collect();

    let fp32_vecs = fp32.embed(all_texts.clone(), Some(16)).expect("fp32 batch embed");
    let int8_vecs = int8.embed(all_texts.clone(), Some(16)).expect("int8 batch embed");
    assert_eq!(fp32_vecs.len(), int8_vecs.len());

    // ── 1. Cross-model drift
    eprintln!("\n— Cross-model drift on identical text (cosine FP32, INT8) —");
    let mut drifts = Vec::with_capacity(fp32_vecs.len());
    for i in 0..fp32_vecs.len() {
        let c = cosine(&fp32_vecs[i], &int8_vecs[i]);
        drifts.push(c);
        eprintln!("  [{:<22}] cosine = {:.6}", labels[i], c);
    }
    let mean: f32 = drifts.iter().sum::<f32>() / drifts.len() as f32;
    let min  = drifts.iter().cloned().fold(1.0f32, f32::min);
    let max  = drifts.iter().cloned().fold(0.0f32, f32::max);
    eprintln!("  → mean = {:.6}   min = {:.6}   max = {:.6}", mean, min, max);

    // ── 2. Ranking preservation
    eprintln!("\n— Ranking preservation per query (top-1 passage) —");
    let qn = queries.len();
    let pn = passages.len();
    let mut agree_count = 0usize;
    for qi in 0..qn {
        let q_fp = &fp32_vecs[qi];
        let q_q8 = &int8_vecs[qi];
        let mut rank_fp: Vec<(usize, f32)> = (0..pn).map(|pi| (pi, cosine(q_fp, &fp32_vecs[qn + pi]))).collect();
        let mut rank_q8: Vec<(usize, f32)> = (0..pn).map(|pi| (pi, cosine(q_q8, &int8_vecs[qn + pi]))).collect();
        rank_fp.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        rank_q8.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_fp = passages[rank_fp[0].0].1;
        let top_q8 = passages[rank_q8[0].0].1;
        let agree = top_fp == top_q8;
        if agree { agree_count += 1; }
        eprintln!(
            "  [{}] {:<8} FP32 → {:<22} ({:.4})   INT8 → {:<22} ({:.4})",
            if agree { "MATCH" } else { "DIFF " },
            queries[qi].1,
            top_fp, rank_fp[0].1,
            top_q8, rank_q8[0].1,
        );
    }
    eprintln!("  → top-1 agreement: {}/{} queries", agree_count, qn);

    // ── 3. Soft assertions so this surfaces regressions without
    //      requiring a manual eyeball every run.
    assert!(min >= 0.90, "INT8 drift too high (min={:.4} < 0.90) — quantization may have corrupted weights", min);
    assert!(agree_count >= qn - 1, "INT8 top-1 disagrees on {} queries (allow at most 1)", qn - agree_count);
}
