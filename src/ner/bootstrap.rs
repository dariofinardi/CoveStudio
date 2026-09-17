//! GLiNER2 model bootstrap — manual hf-hub download with per-byte
//! progress. Bypasses `gliner2_inference::Gliner2Engine::from_
//! pretrained` (which uses `hf_hub` internally and exposes no
//! progress hooks) so we can publish a real `Downloading` state
//! the chat banner can render alongside the embedding-model
//! download bar.
//!
//! Architecture mirrors `src/audio/bootstrap.rs`:
//!   - process-wide singleton (`bootstrap()`)
//!   - serialised first-use via a tokio mutex
//!   - `Arc<RwLock<NerStatus>>` for non-blocking status reads
//!
//! Cache layout: `<HF_HOME>/models/<sanitized_repo>/<variant>/<file>`.
//! Side-by-side with the hf-hub cache (`~/.cache/huggingface/...`)
//! the gliner2 crate would otherwise use, but in our own flat
//! folder so `Gliner2Engine::new(Gliner2Config { models_dir })`
//! finds the files directly without snapshot/blob plumbing.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, RwLock};

use super::engine::{NerStatus, PII_MODEL_ID, PII_MODEL_VARIANT};

/// Files the V2 engine (fp16_v2 variant) needs. Mirror of the list
/// in `gliner2_inference::lib_v2::Gliner2EngineV2::from_pretrained`,
/// adapted for the IOBinding-friendly suffixes used on non-Apple
/// platforms. `tokenizer.json` is always there; the eight ONNX
/// shards depend on the suffix.
fn v2_file_list() -> Vec<String> {
    let is_apple =
        std::env::consts::OS == "macos" || std::env::consts::OS == "ios";
    let no_iobinding = std::env::var("GLINER2_NO_IOBINDING").is_ok();
    let suffix = if is_apple || no_iobinding {
        "_fp16.onnx"
    } else {
        "_fp16_iobinding.onnx"
    };
    let mut files = vec!["tokenizer.json".to_string()];
    for base in [
        "encoder",
        "token_gather",
        "span_rep",
        "schema_gather",
        "count_pred_argmax",
        "count_lstm_fixed",
        "scorer",
        "classifier",
    ] {
        files.push(format!("{base}{suffix}"));
    }
    files
}

/// HF resolve URL for a single file under a repo + subfolder.
fn resolve_url(repo_id: &str, subfolder: &str, filename: &str) -> String {
    format!("https://huggingface.co/{repo_id}/resolve/main/{subfolder}/{filename}")
}

/// Our cache dir. Lives under HF_HOME so users who already
/// redirected that env var (we do, in `lib.rs::ensure_hf_cache_dir`)
/// keep the heavy artefacts grouped. Sanitised repo id (`/` → `--`)
/// to keep the path Windows-safe.
fn cache_dir(repo_id: &str, variant: &str) -> Result<PathBuf> {
    let base = std::env::var("HF_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(default_data_dir)
        .ok_or_else(|| anyhow!("no HF_HOME nor home dir for gliner2 cache"))?;
    // Earlier releases used `mikerust-gliner2`: moved so the weights are
    // not downloaded again.
    let dir = crate::product::migrate_entry(&base, "models", &["mikerust-gliner2"])
        .join(repo_id.replace('/', "--"))
        .join(variant);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating gliner2 cache dir at {}", dir.display()))?;
    Ok(dir)
}

fn default_data_dir() -> Option<PathBuf> {
    crate::product::home_dir()?;
    Some(crate::product::data_subdir("gliner2"))
}

/// Bootstrap singleton — handle to status + serialised download.
pub struct NerBootstrap {
    status: RwLock<NerStatus>,
    download_lock: Mutex<()>,
}

impl NerBootstrap {
    fn new() -> Self {
        Self {
            status: RwLock::new(NerStatus::Idle),
            download_lock: Mutex::new(()),
        }
    }

    /// Public reader used by `/sync/ner-status`.
    pub async fn status(&self) -> NerStatus {
        self.status.read().await.clone()
    }

    /// Internal mutators — package-private to `ner::engine` so
    /// `ensure_engine` can mark the engine as Loading / Ready /
    /// Failed once the download has completed and the ONNX
    /// sessions start initialising.
    pub(super) async fn set_status(&self, s: NerStatus) {
        *self.status.write().await = s;
    }

    /// Ensure every file the V2 engine needs is on disk. Returns
    /// the absolute models_dir path to hand to
    /// `Gliner2Engine::new(Gliner2Config { models_dir, … })`.
    ///
    /// Serialised by `download_lock`: two concurrent first-callers
    /// collapse into one download; the second waiter re-checks
    /// existence under the lock and short-circuits.
    pub async fn ensure_files(&self, repo_id: &str, variant: &str) -> Result<PathBuf> {
        let dir = cache_dir(repo_id, variant)?;
        let files = v2_file_list();

        tracing::info!(
            "[ner] ensure_files entry — repo={repo_id} variant={variant} dir={} \
             files_needed={} HF_HOME={:?} GLINER2_NO_IOBINDING={:?}",
            dir.display(),
            files.len(),
            std::env::var("HF_HOME").ok(),
            std::env::var("GLINER2_NO_IOBINDING").ok(),
        );
        for f in &files {
            let p = dir.join(f);
            tracing::info!(
                "[ner]   pre-check: {} → {}",
                p.display(),
                if p.exists() { "EXISTS" } else { "missing" }
            );
        }

        // Fast path: every file already present.
        if files.iter().all(|f| dir.join(f).exists()) {
            tracing::info!("[ner] cache hit — all {} files present, skipping download", files.len());
            *self.status.write().await = NerStatus::Loading;
            return Ok(dir);
        }
        tracing::info!("[ner] cache miss — entering download path under lock");

        let _g = self.download_lock.lock().await;

        // Re-check under the lock; a peer may have just finished.
        if files.iter().all(|f| dir.join(f).exists()) {
            tracing::info!("[ner] re-check under lock — peer completed, skipping download");
            *self.status.write().await = NerStatus::Loading;
            return Ok(dir);
        }
        tracing::info!("[ner] confirmed under lock — proceeding to download phase");

        let client = reqwest::Client::builder()
            .user_agent(crate::product::user_agent(None))
            .build()
            .context("reqwest client")?;

        // First pass: HEAD every missing file to total up the byte
        // budget. Some HF servers return Content-Length on HEAD
        // even for LFS blobs (they redirect to a signed URL but
        // the Content-Length comes through); if HEAD is rejected,
        // we still report progress per-file without a grand total.
        let mut total_budget: u64 = 0;
        let mut head_known = true;
        let mut missing: Vec<(String, u64)> = Vec::new();
        for f in &files {
            let target = dir.join(f);
            if target.exists() {
                continue;
            }
            let url = resolve_url(repo_id, variant, f);
            match client.head(&url).send().await {
                Ok(r) => {
                    let len = r
                        .headers()
                        .get(reqwest::header::CONTENT_LENGTH)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);
                    if len == 0 {
                        head_known = false;
                    }
                    total_budget += len;
                    missing.push((f.clone(), len));
                }
                Err(_) => {
                    head_known = false;
                    missing.push((f.clone(), 0));
                }
            }
        }
        if !head_known {
            // Without a precise total the UI shows an indeterminate
            // bar via `total = None` in the status payload.
            total_budget = 0;
        }
        tracing::info!(
            "[ner] downloading {} file(s) ({} MB total) → {}",
            missing.len(),
            total_budget / 1_048_576,
            dir.display()
        );

        let total_opt = if head_known && total_budget > 0 {
            Some(total_budget)
        } else {
            None
        };
        *self.status.write().await = NerStatus::Downloading {
            downloaded: 0,
            total: total_opt,
            file: missing
                .first()
                .map(|(n, _)| n.clone())
                .unwrap_or_default(),
        };

        let mut downloaded_global: u64 = 0;
        for (fname, _expected_len) in &missing {
            let target = dir.join(fname);
            // Per-file stream into <name>.part, atomic rename on
            // success. Mirrors whisper bootstrap.
            let url = resolve_url(repo_id, variant, fname);
            let part = target.with_extension("part");
            let _ = tokio::fs::remove_file(&part).await;

            tracing::info!("[ner]   → {fname}");
            let resp = client
                .get(&url)
                .send()
                .await
                .with_context(|| format!("GET {url}"))?
                .error_for_status()
                .with_context(|| format!("HF rejected {url}"))?;

            // Update status to point at this filename — useful when
            // a single file is several hundred MB (the encoder).
            {
                let mut g = self.status.write().await;
                if let NerStatus::Downloading { ref mut file, .. } = *g {
                    *file = fname.clone();
                }
            }

            let mut sink = tokio::fs::File::create(&part)
                .await
                .with_context(|| format!("create {}", part.display()))?;
            let mut stream = resp.bytes_stream();
            use futures_util::StreamExt;
            const TICK_BYTES: u64 = 1 << 20; // 1 MB
            let mut next_tick = downloaded_global + TICK_BYTES;
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.context("chunk read")?;
                sink.write_all(&chunk).await.context("part write")?;
                downloaded_global += chunk.len() as u64;
                if downloaded_global >= next_tick {
                    next_tick = downloaded_global + TICK_BYTES;
                    let mut g = self.status.write().await;
                    if let NerStatus::Downloading {
                        ref mut downloaded, ..
                    } = *g
                    {
                        *downloaded = downloaded_global;
                    }
                }
            }
            sink.flush().await.context("part flush")?;
            drop(sink);
            tokio::fs::rename(&part, &target)
                .await
                .with_context(|| format!("rename {} → {}", part.display(), target.display()))?;
        }

        tracing::info!("[ner] all files cached at {}", dir.display());
        *self.status.write().await = NerStatus::Loading;
        Ok(dir)
    }
}

/// Process-wide singleton accessor.
pub fn bootstrap() -> Arc<NerBootstrap> {
    static INST: OnceLock<Arc<NerBootstrap>> = OnceLock::new();
    INST.get_or_init(|| Arc::new(NerBootstrap::new())).clone()
}

/// Convenience: ensure the default model + variant are on disk and
/// return the directory to hand to `Gliner2Engine::new`.
pub async fn ensure_default_model() -> Result<PathBuf> {
    bootstrap()
        .ensure_files(PII_MODEL_ID, PII_MODEL_VARIANT)
        .await
        .with_context(|| {
            format!(
                "ensure_default_model({} / {})",
                PII_MODEL_ID, PII_MODEL_VARIANT
            )
        })
}

#[allow(dead_code)]
pub fn is_file(p: &Path) -> bool {
    p.is_file()
}
