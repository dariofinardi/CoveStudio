//! Mistral La Plateforme (`api.mistral.ai`) — dedicated provider.
//!
//! Split from `src/llm/local.rs` (OpenAI-compat path) in v0.6.0
//! because Mistral has provider-specific knobs that matter for the
//! legal-target product and would be wrong to send to a generic
//! OpenAI-shape endpoint:
//!
//! * **`parallel_tool_calls: false`** — Mistral defaults to parallel
//!   tool execution; legal workflows are sequential (tabular cell
//!   extraction, citation lookups in order, …) and benefit from the
//!   predictability of one-call-at-a-time semantics.
//!
//! * **`safe_prompt: false`** — Mistral's safety wrapper false-flags
//!   legitimate legal content (sentences on criminal cases, sensitive
//!   medical reports). Sent explicit-OFF here; the user-facing toggle
//!   lands with migration 0033 in v0.6.0 Commit B.
//!
//! * **`prompt_cache_key`** — Mistral charges only 10% of normal
//!   token price on cache hits. We derive a stable per-chat key
//!   (`covestudio_chat_{chat_id}`) so the system prompt + attached
//!   documents prefix is cached across turns. On a typical 20-turn
//!   document-heavy legal conversation the cache hit rate is 80-90%,
//!   for an effective cost reduction in the same range.
//!
//! Plus Mistral-specific error mapping: 401 invalid key, 403 quota
//! or guardrail block, 422 validation error, 429 rate limit with
//! `Retry-After` header parsing.
//!
//! ZDR (Zero Data Retention) is NOT a per-request header — it
//! requires a manual support ticket at help.mistral.ai (confirmed
//! 2026-06 against the official help center). The card UI surfaces
//! this as info text + a link, not as a checkbox.

use anyhow::{anyhow, Result};
use futures_util::stream;
use serde::Deserialize;
use serde_json::json;
use std::collections::VecDeque;
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore};

use super::local::{parse_sse_line_opt, to_wire_messages};
use super::types::{StreamEvent, StreamParams};
use crate::llm::BoxStream;

/// Process-global concurrency cap for Mistral cloud requests.
///
/// Why a hard cap of 1: Mistral's free Experiment tier defaults to
/// 1 request per second per workspace. The chat composer in this
/// product routinely fires several Mistral calls per turn (main
/// chat + title gen + HyDE retrieval + per-cell tabular
/// extraction) — without serialisation they all race and 429 the
/// instant they overlap. Retry-with-backoff (added in v0.6.1)
/// helps with one-off spikes but doesn't fix the underlying
/// concurrency: 8 cell-extractors retrying simultaneously after
/// 1s still produce 7 failures, then 6 after 2s, etc.
///
/// The semaphore lives at module scope so it spans the whole
/// Cove Studio process — chat / tabular / HyDE / title gen all
/// queue through the same gate. The cap of 1 is safe for
/// Experiment tier (1 RPS) and only mildly underutilises paid
/// Scale tier (typically 4-8 RPS); paid users rarely 429 anyway,
/// so the conservative default is the right product trade-off.
///
/// If a user is on Scale and feels the throughput hit, the next
/// step is to expose a per-user override (settings.json /
/// MISTRAL_CONCURRENCY env var) — tracked as a follow-up, not
/// shipped in v0.6.2 because the audience is small enough to
/// support manually.
const MISTRAL_MAX_CONCURRENT: usize = 1;

static MISTRAL_GATE: LazyLock<Semaphore> =
    LazyLock::new(|| Semaphore::new(MISTRAL_MAX_CONCURRENT));

/// Minimum wall-clock spacing between two consecutive Mistral calls.
///
/// Why a spacing on top of the v0.6.2 semaphore: the semaphore caps
/// CONCURRENCY (no two requests in flight at once) but doesn't cap
/// THROUGHPUT. If call A takes 0.3s, call B starts at t=0.3s and
/// finishes at t=0.6s — that's two calls in the same wall-clock
/// second, and Mistral Experiment tier (1 RPS via token bucket)
/// 429s the second one.
///
/// v0.6.2 worked for slow calls (Mistral Large 3 typically takes
/// 1-3s per turn) but broke down for fast title-generation calls
/// to Ministral 3B (~0.3-0.5s) or whenever the user's tabular
/// review burned through many cheap cells in a row.
///
/// 1100ms (vs the nominal 1000ms) gives ~100ms of safety margin
/// against Mistral's bucket-refill granularity. Slows worst-case
/// throughput from 1.00 RPS to 0.91 RPS — acceptable for the
/// product, drops the 429 rate to near zero on Experiment.
const MISTRAL_MIN_INTERVAL: Duration = Duration::from_millis(1100);

/// Wall-clock timestamp of the most recently *issued* Mistral
/// request. Read and written under the semaphore, so only one
/// task touches it at a time. `Instant::now()` at startup so the
/// first call doesn't pay an initial pause.
static MISTRAL_LAST_CALL: LazyLock<Mutex<Instant>> =
    LazyLock::new(|| Mutex::new(Instant::now() - MISTRAL_MIN_INTERVAL));

/// Max retry attempts on HTTP 429 before surfacing the error to the
/// caller. Three is enough to ride out the typical RPS hiccup on
/// Mistral's Experiment tier (1 req/s) without making the user wait
/// pathologically long if the quota is genuinely exhausted.
const MAX_429_RETRIES: u32 = 3;

/// Hard cap on `Retry-After` honour. Mistral has been observed to
/// occasionally send very large retry-after values when a workspace's
/// monthly quota resets; we cap so the chat composer doesn't hang
/// for half an hour on what looks like a network freeze. After the
/// cap the call surfaces the 429 error and the user can retry
/// manually.
const MAX_BACKOFF_SECS: u64 = 30;

/// Decide how long to wait before retrying a 429. Pure function so
/// the retry policy is testable without sleeping.
///
/// Strategy:
///   1. If Mistral sent a `Retry-After` header parseable as integer
///      seconds, honour it (capped at MAX_BACKOFF_SECS).
///   2. Otherwise fall back to exponential backoff:
///      attempt 0 → 1s, attempt 1 → 2s, attempt 2 → 4s, …
pub(crate) fn next_backoff(attempt: u32, retry_after: Option<&str>) -> Duration {
    if let Some(ra) = retry_after {
        if let Ok(secs) = ra.trim().parse::<u64>() {
            return Duration::from_secs(secs.min(MAX_BACKOFF_SECS));
        }
        // Mistral has historically sent integer seconds; if we ever
        // get the HTTP-date format ("Mon, 01 Jan 2026 00:00:00 GMT")
        // we don't bother parsing it — fall through to exponential.
    }
    Duration::from_secs(1u64 << attempt.min(6)) // cap shift to avoid overflow
}

/// POST to Mistral with automatic 429 retry. Returns the first
/// non-429 response (or the final 429 after `MAX_429_RETRIES`
/// attempts so the caller can surface it via `mistral_error`).
///
/// We retry only on 429 — other status codes (401 invalid key, 403
/// quota or guardrail, 422 validation, 5xx server) are surfaced
/// immediately because they're either authoritative refusals or
/// distinct enough to be worth user attention without our retry
/// loop hiding them.
async fn post_with_retry(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    body: &serde_json::Value,
) -> Result<reqwest::Response> {
    // Acquire the process-global concurrency permit. With
    // MISTRAL_MAX_CONCURRENT = 1 this serialises every Mistral call
    // (chat / tabular / HyDE / title gen) so we never have more
    // than one Mistral request in flight at a time — keeping us
    // under Experiment tier's 1 RPS limit.
    //
    // `acquire` is fair (FIFO) so the order callers fire matches
    // the order their results come back; useful for tabular cell
    // extraction where row-by-row output makes more sense than
    // arbitrary reorder.
    let _permit = MISTRAL_GATE
        .acquire()
        .await
        .map_err(|e| anyhow!("Mistral semaphore poisoned: {e}"))?;

    // v0.6.4 — enforce minimum spacing between consecutive Mistral
    // requests. The semaphore alone is concurrency control; this
    // converts it into a true rate limiter (≤ 1 RPS) by sleeping
    // until at least MISTRAL_MIN_INTERVAL has passed since the
    // previous request was issued. The mutex is held only briefly
    // — under the semaphore's mutual exclusion the lock acquisition
    // is uncontended in steady state.
    {
        let mut last = MISTRAL_LAST_CALL.lock().await;
        let elapsed = last.elapsed();
        if elapsed < MISTRAL_MIN_INTERVAL {
            let wait = MISTRAL_MIN_INTERVAL - elapsed;
            tracing::debug!(
                "[llm/mistral] rate-limit spacing: sleeping {wait:?} (since last call: {elapsed:?})"
            );
            tokio::time::sleep(wait).await;
        }
        *last = Instant::now();
    }

    for attempt in 0..MAX_429_RETRIES {
        let resp = client
            .post(url)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await?;

        // Last attempt: caller handles whatever came back.
        if attempt + 1 == MAX_429_RETRIES {
            return Ok(resp);
        }

        // 429 → back off and retry. Anything else → surface now.
        if resp.status().as_u16() != 429 {
            return Ok(resp);
        }

        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        // Drain the body so the connection can be reused — without
        // this reqwest may hold it open until drop.
        let _ = resp.bytes().await;

        let backoff = next_backoff(attempt, retry_after.as_deref());
        tracing::warn!(
            "[llm/mistral] 429 on attempt {} of {}, backing off {:?}{}",
            attempt + 1,
            MAX_429_RETRIES,
            backoff,
            retry_after
                .as_deref()
                .map(|r| format!(" (Retry-After: {r})"))
                .unwrap_or_default(),
        );
        tokio::time::sleep(backoff).await;
    }
    // Unreachable — the loop returns on the last iteration. Keep an
    // explicit error to satisfy the type checker without `unreachable!`
    // (cleaner stack if the constant ever drifts).
    Err(anyhow!("post_with_retry: exhausted retries unexpectedly"))
}

/// La Plateforme endpoint. Hardcoded — Mistral is a managed cloud
/// provider; the regional Azure-AI / AWS-Bedrock deployments need
/// different auth flows and are not modelled here. If a user wants
/// to point at one of those, they should use the `openai:` provider
/// path (which accepts a free-form `local_base_url`).
const MISTRAL_BASE_URL: &str = "https://api.mistral.ai/v1";

/// Tighter than the OpenAI / Ollama 1.0 default — citation lists,
/// structured JSON and tool calls all benefit from less random
/// sampling in this product. Same value `local.rs` ships.
const DEFAULT_TEMPERATURE: f32 = 0.5;

/// 8192 — long enough to fit the citation block legal answers
/// typically emit (which gets truncated at 4096 — see v0.5.1
/// history). Same value `local.rs` ships.
const DEFAULT_MAX_TOKENS: u32 = 8192;

fn resolve_credentials(params: &StreamParams) -> Result<(String, String)> {
    let cfg = params.local_config.as_ref().ok_or_else(|| {
        anyhow!(
            "Mistral: API key non configurata. Imposta la chiave in \
             Settings → Modelli LLM → Mistral AI."
        )
    })?;
    let api_key = cfg.api_key.as_deref().unwrap_or("").trim();
    if api_key.is_empty() {
        return Err(anyhow!(
            "Mistral: API key vuota. Inseriscila in \
             Settings → Modelli LLM → Mistral AI."
        ));
    }
    let model = if cfg.model.is_empty() {
        super::strip_model_prefix(&params.model).to_string()
    } else {
        cfg.model.clone()
    };
    if model.is_empty() {
        return Err(anyhow!(
            "Mistral: model id mancante. Esempi validi: \
             mistral-large-latest, mistral-medium-latest, mistral-small-latest."
        ));
    }
    Ok((api_key.to_string(), model))
}

/// Derive the cache key. Only emit when we have a chat id —
/// one-shot calls (title generation, HyDE, summarisation) have no
/// repeated-prefix structure to cache.
pub(crate) fn cache_key_for(params: &StreamParams) -> Option<String> {
    params
        .chat_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|cid| format!("{}_chat_{cid}", crate::product::SLUG))
}

pub(crate) fn build_body(params: &StreamParams, model: &str, stream: bool) -> serde_json::Value {
    let messages = to_wire_messages(&params.full_system(), &params.messages);
    // Per-user overrides (Commit B). When `mistral_opts` is None
    // (one-shot callers, or chat callers before migration 0033 has
    // run on their data folder) both flags fall back to false —
    // matching the Commit A hard-coded behaviour.
    let opts = params.mistral_opts.as_ref();
    let parallel = opts.map(|o| o.parallel_tool_calls).unwrap_or(false);
    let safe = opts.map(|o| o.safe_prompt).unwrap_or(false);
    let mut body = json!({
        "model": model,
        "messages": messages,
        "stream": stream,
        "max_tokens": DEFAULT_MAX_TOKENS,
        "temperature": DEFAULT_TEMPERATURE,
        // Mistral default is `true`. We default to `false` (sequential
        // tool execution for predictable legal workflows); the user
        // can flip it via `mistral_parallel_tools` in Settings.
        "parallel_tool_calls": parallel,
        // Mistral default is also `false`; explicit-OFF unless the
        // user opted IN via `mistral_safe_prompt` in Settings, which
        // applies the upstream safety wrapper.
        "safe_prompt": safe,
    });
    if !params.tools.is_empty() {
        if let Ok(tools) = serde_json::to_value(&params.tools) {
            body["tools"] = tools;
        }
    }
    // The 10%-of-price cache key. Only emit when we have a chat
    // anchor — one-shot calls don't benefit and the key would be
    // wasted on unique prefixes.
    if let Some(key) = cache_key_for(params) {
        body["prompt_cache_key"] = json!(key);
    }
    body
}

/// Map a non-success HTTP response into a user-readable
/// `anyhow::Error`. Italian-first because that's the product
/// audience; the chat UI surfaces the message verbatim.
pub(crate) fn mistral_error(
    status: reqwest::StatusCode,
    body: &str,
    retry_after: Option<&str>,
) -> anyhow::Error {
    match status.as_u16() {
        401 => anyhow!(
            "Mistral 401: API key non valida o scaduta. \
             Verifica in Settings → Modelli LLM → Mistral AI."
        ),
        403 => anyhow!(
            "Mistral 403: richiesta rifiutata (quota esaurita o filtro \
             di sicurezza attivato). Dettagli: {body}"
        ),
        422 => anyhow!(
            "Mistral 422: payload non valido. Probabile model id \
             errato o messaggio malformato. Dettagli: {body}"
        ),
        429 => {
            let ra = retry_after
                .map(|s| format!(" (riprova fra ~{s}s)"))
                .unwrap_or_default();
            anyhow!("Mistral 429: rate limit raggiunto{ra}.")
        }
        500..=599 => anyhow!("Mistral {status}: errore lato server. Dettagli: {body}"),
        _ => anyhow!("Mistral {status}: {body}"),
    }
}

pub async fn stream(params: StreamParams) -> Result<BoxStream> {
    let (api_key, model) = resolve_credentials(&params)?;
    tracing::info!(
        "[llm/mistral] stream → model={model}, chat_id={:?}, cache_key={}",
        params.chat_id.as_deref(),
        cache_key_for(&params).as_deref().unwrap_or("<none>"),
    );
    let client = reqwest::Client::new();
    let body = build_body(&params, &model, true);

    let url = format!("{MISTRAL_BASE_URL}/chat/completions");
    let resp = post_with_retry(&client, &url, &api_key, &body).await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let text = resp.text().await.unwrap_or_default();
        return Err(mistral_error(status, &text, retry_after.as_deref()));
    }

    let byte_stream = resp.bytes_stream();
    let event_stream = stream::unfold(
        (byte_stream, String::new(), VecDeque::<StreamEvent>::new()),
        |(mut bs, mut buf, mut pending)| async move {
            use futures_util::StreamExt;
            loop {
                if let Some(ev) = pending.pop_front() {
                    return Some((Ok(ev), (bs, buf, pending)));
                }
                match bs.next().await {
                    None => return None,
                    Some(Ok(bytes)) => {
                        buf.push_str(&String::from_utf8_lossy(&bytes));
                        while let Some(idx) = buf.find('\n') {
                            let line = buf[..idx].trim().to_string();
                            buf.drain(..=idx);
                            if line.is_empty() {
                                continue;
                            }
                            if let Some(ev) = parse_sse_line_opt(&line) {
                                pending.push_back(ev);
                            }
                        }
                    }
                    Some(Err(e)) => {
                        return Some((
                            Err(anyhow!("Mistral stream error: {e}")),
                            (bs, buf, pending),
                        ));
                    }
                }
            }
        },
    );

    Ok(Box::pin(event_stream))
}

pub async fn complete(params: StreamParams) -> Result<String> {
    let (api_key, model) = resolve_credentials(&params)?;
    let client = reqwest::Client::new();
    let body = build_body(&params, &model, false);

    let url = format!("{MISTRAL_BASE_URL}/chat/completions");
    let resp = post_with_retry(&client, &url, &api_key, &body).await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let text = resp.text().await.unwrap_or_default();
        return Err(mistral_error(status, &text, retry_after.as_deref()));
    }

    // Local types to avoid leaking Mistral wire shape into the rest
    // of the crate — the non-streaming response payload uses the
    // OpenAI shape and we only care about the first choice's text.
    #[derive(Deserialize)]
    struct CompleteResponse {
        choices: Vec<Choice>,
    }
    #[derive(Deserialize)]
    struct Choice {
        message: Msg,
    }
    #[derive(Deserialize)]
    struct Msg {
        content: Option<String>,
    }

    let parsed: CompleteResponse = resp.json().await?;
    Ok(parsed
        .choices
        .into_iter()
        .next()
        .and_then(|c| c.message.content)
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::{LocalConfig, Message};

    fn params_with(
        api_key: &str,
        model: &str,
        chat_id: Option<&str>,
    ) -> StreamParams {
        StreamParams {
            model: format!("mistral:{model}"),
            system_prompt: "system test".into(),
            system_volatile: String::new(),
            messages: vec![Message::user("hi".to_string())],
            tools: vec![],
            max_iterations: 1,
            enable_thinking: false,
            local_config: Some(LocalConfig {
                base_url: MISTRAL_BASE_URL.to_string(),
                api_key: if api_key.is_empty() {
                    None
                } else {
                    Some(api_key.to_string())
                },
                model: model.to_string(),
                secure_mode: false,
            }),
            claude_api_key: None,
            gemini_api_key: None,
            gemini_region: None,
            chat_id: chat_id.map(String::from),
            mistral_opts: None,
        }
    }

    #[test]
    fn resolve_credentials_rejects_missing_key() {
        let p = params_with("", "mistral-large-latest", None);
        let err = resolve_credentials(&p).unwrap_err();
        assert!(err.to_string().contains("vuota"));
    }

    #[test]
    fn resolve_credentials_rejects_missing_local_config() {
        let mut p = params_with("k", "mistral-large-latest", None);
        p.local_config = None;
        let err = resolve_credentials(&p).unwrap_err();
        assert!(err.to_string().contains("non configurata"));
    }

    #[test]
    fn resolve_credentials_accepts_valid_key_and_model() {
        let p = params_with("sk-test", "mistral-large-latest", None);
        let (key, model) = resolve_credentials(&p).unwrap();
        assert_eq!(key, "sk-test");
        assert_eq!(model, "mistral-large-latest");
    }

    #[test]
    fn cache_key_emitted_for_chat_scoped_call() {
        let p = params_with("k", "mistral-large-latest", Some("abc-123"));
        assert_eq!(
            cache_key_for(&p).as_deref(),
            Some(format!("{}_chat_abc-123", crate::product::SLUG).as_str()),
        );
    }

    #[test]
    fn cache_key_omitted_for_one_shot_calls() {
        let p = params_with("k", "mistral-large-latest", None);
        assert!(cache_key_for(&p).is_none());
    }

    #[test]
    fn cache_key_omitted_for_empty_chat_id() {
        let p = params_with("k", "mistral-large-latest", Some(""));
        assert!(cache_key_for(&p).is_none());
    }

    #[test]
    fn build_body_disables_parallel_tool_calls() {
        let p = params_with("k", "mistral-large-latest", Some("x"));
        let body = build_body(&p, "mistral-large-latest", true);
        // Both literally `false`, not omitted — we want explicit OFF
        // so a future Mistral default change doesn't quietly turn
        // them back on.
        assert_eq!(body["parallel_tool_calls"], json!(false));
        assert_eq!(body["safe_prompt"], json!(false));
    }

    #[test]
    fn build_body_includes_cache_key_when_chat_scoped() {
        let p = params_with("k", "mistral-large-latest", Some("chat-xyz"));
        let body = build_body(&p, "mistral-large-latest", true);
        assert_eq!(body["prompt_cache_key"], json!(format!("{}_chat_chat-xyz", crate::product::SLUG)));
    }

    #[test]
    fn build_body_omits_cache_key_for_one_shot() {
        let p = params_with("k", "mistral-large-latest", None);
        let body = build_body(&p, "mistral-large-latest", true);
        assert!(body.get("prompt_cache_key").is_none());
    }

    #[test]
    fn build_body_has_sensible_sampling_defaults() {
        let p = params_with("k", "mistral-large-latest", None);
        let body = build_body(&p, "mistral-large-latest", true);
        assert_eq!(body["temperature"], json!(0.5));
        assert_eq!(body["max_tokens"], json!(8192));
        assert_eq!(body["stream"], json!(true));
    }

    #[test]
    fn build_body_propagates_stream_flag_off_for_complete() {
        let p = params_with("k", "mistral-large-latest", None);
        let body = build_body(&p, "mistral-large-latest", false);
        assert_eq!(body["stream"], json!(false));
    }

    #[test]
    fn build_body_honours_mistral_opts_override_to_true() {
        let mut p = params_with("k", "mistral-large-latest", None);
        p.mistral_opts = Some(crate::llm::types::MistralOpts {
            safe_prompt: true,
            parallel_tool_calls: true,
        });
        let body = build_body(&p, "mistral-large-latest", true);
        assert_eq!(body["safe_prompt"], json!(true));
        assert_eq!(body["parallel_tool_calls"], json!(true));
    }

    #[test]
    fn build_body_falls_back_to_false_when_mistral_opts_is_none() {
        let mut p = params_with("k", "mistral-large-latest", None);
        p.mistral_opts = None;
        let body = build_body(&p, "mistral-large-latest", true);
        assert_eq!(body["safe_prompt"], json!(false));
        assert_eq!(body["parallel_tool_calls"], json!(false));
    }

    #[test]
    fn mistral_error_401_mentions_api_key() {
        let e = mistral_error(reqwest::StatusCode::UNAUTHORIZED, "{}", None);
        let s = e.to_string();
        assert!(s.contains("401"));
        assert!(s.contains("API key"));
    }

    #[test]
    fn mistral_error_429_extracts_retry_after() {
        let e = mistral_error(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            "{}",
            Some("12"),
        );
        let s = e.to_string();
        assert!(s.contains("429"));
        assert!(s.contains("~12s"));
    }

    #[test]
    fn mistral_error_429_without_retry_after_still_clear() {
        let e = mistral_error(reqwest::StatusCode::TOO_MANY_REQUESTS, "{}", None);
        let s = e.to_string();
        assert!(s.contains("429"));
        assert!(s.contains("rate limit"));
    }

    #[test]
    fn next_backoff_honours_retry_after_seconds() {
        // Mistral typically sends Retry-After as a plain integer
        // number of seconds. Honour it verbatim.
        assert_eq!(next_backoff(0, Some("5")), Duration::from_secs(5));
        assert_eq!(next_backoff(2, Some("12")), Duration::from_secs(12));
    }

    #[test]
    fn next_backoff_caps_retry_after_at_thirty_seconds() {
        // Mistral has been observed to occasionally return a very
        // large Retry-After when a monthly quota resets — without a
        // cap the chat composer would hang for half an hour. Cap is
        // explicit so users see a "still rate-limited" error
        // promptly instead of a frozen UI.
        assert_eq!(
            next_backoff(0, Some("9999")),
            Duration::from_secs(30),
        );
    }

    #[test]
    fn next_backoff_exponential_when_header_missing() {
        // 1s, 2s, 4s — total ~7s for the default 3-attempt budget.
        assert_eq!(next_backoff(0, None), Duration::from_secs(1));
        assert_eq!(next_backoff(1, None), Duration::from_secs(2));
        assert_eq!(next_backoff(2, None), Duration::from_secs(4));
    }

    #[test]
    fn next_backoff_ignores_bogus_retry_after_format() {
        // If we ever get HTTP-date format (RFC 7231) instead of
        // integer seconds, parsing fails and we fall back to
        // exponential — better than panic or zero wait.
        let b = next_backoff(2, Some("Mon, 01 Jan 2026 00:00:00 GMT"));
        assert_eq!(b, Duration::from_secs(4));
    }

    #[test]
    fn mistral_min_interval_is_at_least_one_second() {
        // v0.6.4 — Mistral Experiment is nominally 1 RPS via token
        // bucket; we ship 1100ms (10% safety margin) so brief bursts
        // don't 429. Pin the constant so a well-meaning future PR
        // doesn't accidentally drop below 1s and re-introduce the
        // 429 cascade.
        assert!(
            MISTRAL_MIN_INTERVAL.as_millis() >= 1000,
            "MISTRAL_MIN_INTERVAL must be >= 1000ms — Mistral Experiment tier is 1 RPS"
        );
    }

    #[test]
    fn mistral_concurrency_cap_is_one() {
        // The semaphore is the actual fix for the Experiment-tier
        // 429 storm — retry-with-backoff alone doesn't help when 8
        // tabular cells fire simultaneously, they just all retry in
        // parallel and burn through the budget. The cap of 1 is the
        // product trade-off: safe on Experiment, slightly under
        // Scale's 4-8 RPS but paid users rarely 429 anyway.
        assert_eq!(MISTRAL_MAX_CONCURRENT, 1);
    }

    #[tokio::test]
    async fn mistral_gate_serialises_acquirers() {
        // Verify the semaphore actually blocks a second acquirer
        // while the first holds the permit. We use try_acquire
        // (non-blocking) so the test doesn't depend on timing.
        let first = MISTRAL_GATE.acquire().await.unwrap();
        let second = MISTRAL_GATE.try_acquire();
        assert!(
            second.is_err(),
            "second acquirer must block while the first holds the permit (cap = 1)"
        );
        drop(first);
        // After drop, a fresh acquire succeeds.
        let third = MISTRAL_GATE.try_acquire();
        assert!(third.is_ok());
    }

    #[test]
    fn next_backoff_handles_whitespace_in_header() {
        // Some load balancers add whitespace; the trim() in
        // next_backoff handles it.
        assert_eq!(next_backoff(0, Some("  3  ")), Duration::from_secs(3));
    }

    #[test]
    fn mistral_error_500_surfaces_body() {
        let e = mistral_error(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "{\"message\":\"internal\"}",
            None,
        );
        let s = e.to_string();
        assert!(s.contains("500"));
        assert!(s.contains("internal"));
    }
}
