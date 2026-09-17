// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

//! Effective context window of the model used for a chat turn.
//!
//! Lookup order:
//!
//!   1. for `local:` models served by Ollama, the server itself —
//!      `GET /api/ps` gives the `context_length` of a loaded model (the
//!      value the runner actually uses); otherwise `POST /api/show` gives
//!      the Modelfile `num_ctx`, when set, and the model's maximum;
//!   2. a context-overflow error previously returned by the server
//!      (llama.cpp style `n_ctx`), learnt so the next turn uses it;
//!   3. the model catalogue (`config/model.json`, `context_window`);
//!   4. the name-based table in [`super::summarize::context_window_tokens`].
//!
//! Server results are cached per server and model for a few minutes.
//! Non-Ollama local endpoints (vLLM, LM Studio, …) skip step 1.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use super::types::LocalConfig;
use crate::presets::model::ModelCatalogue;

/// How long a server value stays valid before asking again.
const PROBE_TTL: Duration = Duration::from_secs(600);

/// Timeout of each probe request: a slow or absent server must not delay
/// the chat turn noticeably.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Where the window value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowSource {
    /// Reported by the Ollama server (`/api/ps` or `/api/show`).
    Server,
    /// Read from a context-overflow error returned by the server.
    ServerError,
    /// `context_window` of the model in `config/model.json`.
    Catalogue,
    /// Name-based estimate.
    Default,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ContextWindow {
    pub tokens: usize,
    pub source: WindowSource,
}

impl ContextWindow {
    /// True when the value is declared or measured rather than guessed.
    pub fn is_known(&self) -> bool {
        self.source != WindowSource::Default
    }
}

/// Details for the Settings page.
#[derive(Debug, Clone, Serialize)]
pub struct WindowReport {
    #[serde(flatten)]
    pub window: ContextWindow,
    /// Whether an Ollama server answered for this model.
    pub server_reachable: bool,
    /// Whether the model is currently loaded on the server.
    pub loaded: bool,
    /// `num_ctx` set in the Modelfile, if any.
    pub modelfile_num_ctx: Option<usize>,
    /// Maximum context the model supports, as reported by the server.
    pub model_max: Option<usize>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct ProbeDetails {
    reachable: bool,
    loaded_context: Option<usize>,
    modelfile_num_ctx: Option<usize>,
    model_max: Option<usize>,
}

impl ProbeDetails {
    fn runtime_window(&self) -> Option<usize> {
        self.loaded_context.or(self.modelfile_num_ctx)
    }
}

struct CacheEntry {
    window: ContextWindow,
    stored_at: Instant,
}

fn cache() -> &'static Mutex<HashMap<String, CacheEntry>> {
    static CACHE: std::sync::OnceLock<Mutex<HashMap<String, CacheEntry>>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Server root (`http://host:port`) and model name for a `local:` model,
/// or `None` for cloud models.
fn local_target(model: &str, local: Option<&LocalConfig>) -> Option<(String, String)> {
    let bare = model.strip_prefix("local:")?;
    let cfg = local?;
    let base = if cfg.secure_mode {
        super::ollama_manager::SECURE_BASE_URL.to_string()
    } else {
        cfg.base_url.trim().to_string()
    };
    if base.is_empty() {
        return None;
    }
    let with_scheme = if base.starts_with("http://") || base.starts_with("https://") {
        base
    } else {
        format!("http://{base}")
    };
    let root = with_scheme
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .trim_end_matches('/')
        .to_string();
    let name = if cfg.model.trim().is_empty() { bare.to_string() } else { cfg.model.clone() };
    Some((root, name))
}

fn cache_key(root: &str, model: &str) -> String {
    format!("{root}|{}", model.strip_suffix(":latest").unwrap_or(model))
}

/// `context_window` of `model` in the catalogue. The dispatch prefix
/// (`local:`, `openai:`, `mistral:`) selects the provider; other ids are
/// looked up in every provider.
pub fn catalogue_window(catalogue: &ModelCatalogue, model: &str) -> Option<usize> {
    let (provider, id) = match model.split_once(':') {
        Some((p @ ("local" | "openai" | "mistral"), rest)) => (Some(p), rest),
        _ => (None, model),
    };
    let id_bare = id.strip_suffix(":latest").unwrap_or(id);
    catalogue
        .providers
        .iter()
        .filter(|p| provider.is_none_or(|wanted| p.id == wanted))
        .flat_map(|p| p.models.iter())
        .find(|m| m.id == id || m.id == id_bare)
        .and_then(|m| m.context_window)
        .map(|n| n as usize)
}

fn fallback(model: &str, catalogue: Option<&ModelCatalogue>) -> ContextWindow {
    match catalogue.and_then(|c| catalogue_window(c, model)) {
        Some(tokens) => ContextWindow { tokens, source: WindowSource::Catalogue },
        None => ContextWindow {
            tokens: super::summarize::context_window_tokens(model),
            source: WindowSource::Default,
        },
    }
}

/// The context window to plan the turn with.
pub async fn resolve(
    model: &str,
    local: Option<&LocalConfig>,
    catalogue: Option<&ModelCatalogue>,
) -> ContextWindow {
    let Some((root, name)) = local_target(model, local) else {
        return fallback(model, catalogue);
    };
    let key = cache_key(&root, &name);
    if let Some(window) = cache().lock().ok().and_then(|c| {
        c.get(&key)
            .filter(|e| e.stored_at.elapsed() < PROBE_TTL)
            .map(|e| e.window)
    }) {
        return window;
    }
    let details = probe_ollama(&root, &name).await;
    match details.runtime_window() {
        Some(tokens) => {
            let window = ContextWindow { tokens, source: WindowSource::Server };
            tracing::info!("[context-window] {name} on {root}: {tokens} tokens (from server)");
            store(&key, window);
            window
        }
        None => {
            let window = fallback(model, catalogue);
            tracing::info!(
                "[context-window] {name} on {root}: not reported by the server, using {} ({:?})",
                window.tokens,
                window.source
            );
            window
        }
    }
}

/// Fresh details for the Settings page (bypasses the cache and refreshes it).
pub async fn describe(
    model: &str,
    local: Option<&LocalConfig>,
    catalogue: Option<&ModelCatalogue>,
) -> WindowReport {
    let Some((root, name)) = local_target(model, local) else {
        return WindowReport {
            window: fallback(model, catalogue),
            server_reachable: false,
            loaded: false,
            modelfile_num_ctx: None,
            model_max: None,
        };
    };
    let details = probe_ollama(&root, &name).await;
    let window = match details.runtime_window() {
        Some(tokens) => {
            let window = ContextWindow { tokens, source: WindowSource::Server };
            store(&cache_key(&root, &name), window);
            window
        }
        None => cache()
            .lock()
            .ok()
            .and_then(|c| c.get(&cache_key(&root, &name)).map(|e| e.window))
            .filter(|w| w.source == WindowSource::ServerError)
            .unwrap_or_else(|| fallback(model, catalogue)),
    };
    WindowReport {
        window,
        server_reachable: details.reachable,
        loaded: details.loaded_context.is_some(),
        modelfile_num_ctx: details.modelfile_num_ctx,
        model_max: details.model_max,
    }
}

/// Learns the window from a server error such as
/// `request (38839 tokens) exceeds the available context size (32768 tokens)`.
/// Returns the learnt value.
pub fn learn_from_error(model: &str, local: Option<&LocalConfig>, error: &str) -> Option<usize> {
    let tokens = parse_context_from_error(error)?;
    let (root, name) = local_target(model, local)?;
    store(&cache_key(&root, &name), ContextWindow { tokens, source: WindowSource::ServerError });
    tracing::info!("[context-window] {name} on {root}: learnt {tokens} tokens from a server error");
    Some(tokens)
}

fn store(key: &str, window: ContextWindow) {
    if let Ok(mut c) = cache().lock() {
        c.insert(key.to_string(), CacheEntry { window, stored_at: Instant::now() });
    }
}

async fn probe_ollama(root: &str, model: &str) -> ProbeDetails {
    let mut details = ProbeDetails::default();
    let Ok(client) = reqwest::Client::builder().timeout(PROBE_TIMEOUT).build() else {
        return details;
    };

    if let Ok(resp) = client.get(format!("{root}/api/ps")).send().await {
        if let Ok(body) = resp.json::<Value>().await {
            details.reachable = body.get("models").is_some();
            details.loaded_context = loaded_context_length(&body, model);
        }
    }

    if let Ok(resp) = client
        .post(format!("{root}/api/show"))
        .json(&serde_json::json!({ "model": model }))
        .send()
        .await
    {
        if let Ok(body) = resp.json::<Value>().await {
            let (num_ctx, max) = show_context(&body);
            details.reachable |= body.get("model_info").is_some() || body.get("parameters").is_some();
            details.modelfile_num_ctx = num_ctx;
            details.model_max = max;
        }
    }
    details
}

fn same_name(a: &str, b: &str) -> bool {
    a.strip_suffix(":latest").unwrap_or(a) == b.strip_suffix(":latest").unwrap_or(b)
}

/// `context_length` of `model` in an `/api/ps` response.
fn loaded_context_length(body: &Value, model: &str) -> Option<usize> {
    body.get("models")?
        .as_array()?
        .iter()
        .find(|m| {
            ["name", "model"]
                .iter()
                .filter_map(|k| m.get(*k).and_then(Value::as_str))
                .any(|n| same_name(n, model))
        })?
        .get("context_length")?
        .as_u64()
        .filter(|n| *n > 0)
        .map(|n| n as usize)
}

/// `(num_ctx parameter, model maximum)` from an `/api/show` response.
fn show_context(body: &Value) -> (Option<usize>, Option<usize>) {
    let num_ctx = body
        .get("parameters")
        .and_then(Value::as_str)
        .and_then(|params| {
            params.lines().find_map(|line| {
                let mut parts = line.split_whitespace();
                (parts.next() == Some("num_ctx"))
                    .then(|| parts.next()?.parse::<usize>().ok())
                    .flatten()
            })
        });
    let max = body.get("model_info").and_then(Value::as_object).and_then(|info| {
        info.iter()
            .find(|(k, _)| k.ends_with(".context_length"))
            .and_then(|(_, v)| v.as_u64())
            .map(|n| n as usize)
    });
    (num_ctx, max)
}

/// Context size reported in an overflow error: llama.cpp's `n_ctx` field,
/// or the "available context size (N tokens)" wording.
fn parse_context_from_error(error: &str) -> Option<usize> {
    let digits_after = |marker: &str| -> Option<usize> {
        let start = error.find(marker)? + marker.len();
        let digits: String = error[start..]
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(|c| c.is_ascii_digit())
            .collect();
        digits.parse().ok()
    };
    digits_after("n_ctx").or_else(|| digits_after("available context size"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg(base: &str, secure: bool) -> LocalConfig {
        LocalConfig { base_url: base.into(), api_key: None, model: String::new(), secure_mode: secure }
    }

    fn catalogue() -> ModelCatalogue {
        serde_json::from_value(json!({
            "providers": [
                {"id": "anthropic", "display_name": "A", "auth": {"kind": "api_key"},
                 "models": [{"id": "claude-x", "display_name": "X", "context_window": 200000}]},
                {"id": "local", "display_name": "L", "auth": {"kind": "none"},
                 "models": [{"id": "alias:ctx16k", "display_name": "Alias", "context_window": 16384}]}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn target_strips_v1_and_adds_scheme() {
        let c = cfg("192.168.1.10:11434/v1/", false);
        assert_eq!(
            local_target("local:qwen3:14b", Some(&c)),
            Some(("http://192.168.1.10:11434".into(), "qwen3:14b".into()))
        );
        assert_eq!(local_target("claude-sonnet-4-6", Some(&c)), None);
        assert_eq!(local_target("local:x", None), None);
    }

    #[test]
    fn secure_mode_probes_loopback() {
        let c = cfg("http://ignored:1/v1", true);
        let (root, _) = local_target("local:m", Some(&c)).unwrap();
        assert_eq!(root, "http://localhost:11434");
    }

    #[test]
    fn catalogue_lookup_follows_the_dispatch_prefix() {
        let c = catalogue();
        assert_eq!(catalogue_window(&c, "claude-x"), Some(200_000));
        assert_eq!(catalogue_window(&c, "local:alias:ctx16k"), Some(16_384));
        assert_eq!(catalogue_window(&c, "openai:claude-x"), None);
        assert_eq!(fallback("claude-x", Some(&c)).source, WindowSource::Catalogue);
        assert_eq!(fallback("unknown-model", Some(&c)).source, WindowSource::Default);
    }

    #[test]
    fn ps_context_length_matches_with_or_without_latest() {
        let body = json!({"models": [
            {"name": "other:latest", "model": "other:latest", "context_length": 4096},
            {"name": "qwen3-30b-instruct:latest", "model": "qwen3-30b-instruct:latest", "context_length": 32768}
        ]});
        assert_eq!(loaded_context_length(&body, "qwen3-30b-instruct"), Some(32768));
        assert_eq!(loaded_context_length(&body, "missing"), None);
    }

    #[test]
    fn show_reads_num_ctx_and_model_maximum() {
        let body = json!({
            "parameters": "stop \"<|im_end|>\"\nnum_ctx                        65536\ntemperature 0.7",
            "model_info": {"qwen3moe.context_length": 262144, "general.architecture": "qwen3moe"}
        });
        assert_eq!(show_context(&body), (Some(65536), Some(262144)));
        assert_eq!(show_context(&json!({"model_info": {}})), (None, None));
    }

    #[test]
    fn runtime_window_prefers_the_loaded_model() {
        let d = ProbeDetails { reachable: true, loaded_context: Some(32768), modelfile_num_ctx: Some(2048), model_max: Some(262144) };
        assert_eq!(d.runtime_window(), Some(32768));
        let d = ProbeDetails { loaded_context: None, ..d };
        assert_eq!(d.runtime_window(), Some(2048));
    }

    #[test]
    fn overflow_errors_report_the_context_size() {
        let llama = r#"Local LLM error 400 Bad Request: {"error":{"message":"{\"error\":{\"code\":400,\"message\":\"request (38839 tokens) exceeds the available context size (32768 tokens), try increasing it\",\"type\":\"exceed_context_size_error\",\"n_prompt_tokens\":38839,\"n_ctx\":32768}}"}}"#;
        assert_eq!(parse_context_from_error(llama), Some(32768));
        assert_eq!(parse_context_from_error("exceeds the available context size (16384 tokens)"), Some(16384));
        assert_eq!(parse_context_from_error("rate limited"), None);
    }

    #[test]
    fn learnt_value_is_served_from_cache() {
        let c = cfg("http://127.0.0.9:1/v1", false);
        let model = "local:learn-test-model";
        assert_eq!(learn_from_error(model, Some(&c), "\"n_ctx\":12345"), Some(12345));
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let window = runtime.block_on(resolve(model, Some(&c), None));
        assert_eq!(window, ContextWindow { tokens: 12345, source: WindowSource::ServerError });
    }
}
