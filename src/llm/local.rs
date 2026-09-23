/// OpenAI-compatible streaming endpoint (vLLM, Infomaniak AI Tools, etc.)
/// Mirrors the logic in the TypeScript localllm.ts.
use anyhow::{anyhow, Result};
use futures_util::stream;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::VecDeque;

use super::types::{Message, Role, StreamEvent, StreamParams, ToolCall};
use crate::llm::{strip_model_prefix, BoxStream};

/// True when `base` (already normalised via [`normalize_base`]) points
/// at one of the loopback aliases. Used by the secure-mode guard in
/// [`resolve_endpoint`] to refuse any base URL that could route off
/// the machine. We accept `localhost`, `127.0.0.1` and `[::1]`
/// (IPv6) so a user's `/etc/hosts` quirk or IPv6-only stack doesn't
/// trip the check — those are still loopback by definition.
fn is_loopback_url(base: &str) -> bool {
    let lc = base.to_ascii_lowercase();
    lc.starts_with("http://localhost")
        || lc.starts_with("http://127.0.0.1")
        || lc.starts_with("http://[::1]")
        || lc.starts_with("https://localhost")
        || lc.starts_with("https://127.0.0.1")
        || lc.starts_with("https://[::1]")
}

/// Normalize a base URL for OpenAI-compatible requests.
/// Accepts both "https://host" and "https://host/v1" forms — appends `/v1`
/// when the user-supplied URL doesn't already include a versioned suffix.
/// Adds `http://` when the user typed a host:port without scheme (typical
/// for local Ollama endpoints like `127.0.0.1:11434/v1`).
fn normalize_base(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/').to_string();
    let with_scheme = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed
    } else {
        format!("http://{trimmed}")
    };
    if with_scheme.ends_with("/v1") || with_scheme.contains("/v1/") {
        with_scheme
    } else {
        format!("{with_scheme}/v1")
    }
}

fn resolve_endpoint(params: &StreamParams) -> Result<(String, String, String)> {
    if let Some(cfg) = &params.local_config {
        let mut base = normalize_base(&cfg.base_url);
        let key = cfg.api_key.clone().unwrap_or_else(|| "local".to_string());
        let mut model = if cfg.model.is_empty() {
            strip_model_prefix(&params.model).to_string()
        } else {
            cfg.model.clone()
        };

        // Modalità sicura locale: refuse any base URL that isn't
        // loopback, and any model id that isn't on the curated
        // allowlist. The frontend Settings UI prevents this from
        // happening interactively (URL field is locked, model is a
        // select restricted to the curated entries) but a stale
        // local_config row in the DB could still slip past — fail
        // closed here.
        if cfg.secure_mode {
            if !is_loopback_url(&base) {
                return Err(anyhow!(
                    "Modalità sicura locale attiva: il provider locale può puntare \
                     solo a localhost (URL ricevuto: {base})."
                ));
            }
            // The model must be in the local-models catalogue, under its
            // current name or a name from an earlier release not yet
            // migrated. Ollama adds `:latest` to untagged models, so the
            // comparison ignores that tag.
            if crate::presets::local_models::find_including_legacy(&model).is_none() {
                return Err(anyhow!(
                    "Modalità sicura locale attiva: il modello `{model}` non è \
                     nell'allowlist dei modelli locali."
                ));
            }
            // Snap the URL to the canonical loopback form so logs /
            // traces don't show "127.0.0.1" vs "localhost" mismatches.
            base = format!("{}/v1", crate::llm::ollama_manager::SECURE_BASE_URL);
            // `model` already validated above; leave it as-is.
            let _ = &mut model; // (no-op, suppress unused_assignments)
        }
        return Ok((base, key, model));
    }
    // Legacy env-var path
    let base = std::env::var("VLLM_BASE_URL")
        .map_err(|_| anyhow!("Local model not configured: set it in Account → Models, or set VLLM_BASE_URL"))?;
    let base = normalize_base(&base);
    let key = std::env::var("VLLM_API_KEY").unwrap_or_else(|_| "local".to_string());
    let model = if params.model == "localllm-light" {
        std::env::var("VLLM_LIGHT_MODEL").unwrap_or_else(|_| params.model.clone())
    } else if params.model.starts_with("localllm") {
        std::env::var("VLLM_MAIN_MODEL").unwrap_or_else(|_| params.model.clone())
    } else {
        strip_model_prefix(&params.model).to_string()
    };
    Ok((base, key, model))
}

fn log_upstream_line(line: &str) {
    let t = line.trim();
    if !t.starts_with("data:") {
        return;
    }
    let one_line = t.replace('\n', "\\n");
    let preview: String = one_line.chars().take(1200).collect();
    tracing::info!("[llm/local] upstream_sse {preview}");
}

#[derive(Deserialize)]
struct StreamChunk {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    delta: Delta,
}

#[derive(Deserialize, Default)]
struct Delta {
    content: Option<String>,
    reasoning: Option<String>,
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Deserialize)]
struct ToolCallDelta {
    index: usize,
    id: Option<String>,
    function: Option<FunctionDelta>,
}

#[derive(Deserialize)]
struct FunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<serde_json::Value>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    think: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    /// 0.5 (v0.5.3+) — tighter sampling than the OpenAI / vLLM /
    /// Ollama default of 1.0. Determinism matters more than
    /// creativity in this product (citation lists, structured JSON
    /// blocks, tool calls all benefit from less-random sampling) —
    /// see the project memory on user preference for deterministic
    /// output across all providers.
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

/// Compose the effective system prompt for a local call. When the
/// user is in secure mode this prepends the no-think preamble before
/// `params.full_system()` so any model that wasn't created from the
/// local-models catalogue (older derivation, stale Ollama variant, …)
/// still gets the "no chain-of-thought" instruction at the top of its
/// system block. Catalogue derivations may get it twice (once from
/// their Modelfile, once from here) — harmless overlap.
fn effective_system(params: &StreamParams) -> String {
    let base = params.full_system();
    let secure = params
        .local_config
        .as_ref()
        .map(|c| c.secure_mode)
        .unwrap_or(false);
    if secure {
        format!(
            "{}{}",
            crate::llm::ollama_manager::no_think_preamble(),
            base
        )
    } else {
        base
    }
}

/// Shared OpenAI-compatible message-wire formatting. Used by both
/// the generic [`local::stream`] path and the Mistral-dedicated
/// [`super::mistral::stream`] — same JSON shape since Mistral's
/// `/chat/completions` endpoint mirrors OpenAI's. Exposed
/// `pub(crate)` so we don't duplicate the tool-call replay logic.
pub(crate) fn to_wire_messages(system: &str, messages: &[Message]) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    if !system.is_empty() {
        out.push(json!({ "role": "system", "content": system }));
    }
    for m in messages {
        let role = match m.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
            Role::System => "system",
        };
        // Tool result message — needs `tool_call_id`.
        if matches!(m.role, Role::Tool) {
            out.push(json!({
                "role": "tool",
                "tool_call_id": m.tool_call_id.clone().unwrap_or_default(),
                "content": m.content,
            }));
            continue;
        }
        // Assistant message that previously emitted tool_calls — replay them.
        if !m.tool_calls.is_empty() {
            let calls: Vec<serde_json::Value> = m.tool_calls.iter().map(|c| {
                json!({
                    "id": c.id,
                    "type": "function",
                    "function": {
                        "name": c.name,
                        "arguments": serde_json::to_string(&c.input).unwrap_or_else(|_| "{}".into()),
                    }
                })
            }).collect();
            let mut obj = serde_json::Map::new();
            obj.insert("role".into(), json!("assistant"));
            if !m.content.is_empty() {
                obj.insert("content".into(), json!(m.content));
            }
            obj.insert("tool_calls".into(), json!(calls));
            out.push(serde_json::Value::Object(obj));
            continue;
        }
        if m.images.is_empty() {
            out.push(json!({ "role": role, "content": m.content }));
        } else {
            // OpenAI vision content array: text + image_url parts.
            let mut parts: Vec<serde_json::Value> = Vec::new();
            if !m.content.is_empty() {
                parts.push(json!({ "type": "text", "text": m.content }));
            }
            for url in &m.images {
                parts.push(json!({
                    "type": "image_url",
                    "image_url": { "url": url }
                }));
            }
            out.push(json!({ "role": role, "content": parts }));
        }
    }
    out
}

pub async fn stream(
    params: StreamParams,
) -> Result<BoxStream> {
    let (base, api_key, model) = resolve_endpoint(&params)?;
    tracing::info!("[llm/local] stream → base={base}, model={model}, key_present={}", !api_key.is_empty() && api_key != "local");
    let client = reqwest::Client::new();

    let messages = to_wire_messages(&effective_system(&params), &params.messages);
    let tools = if params.tools.is_empty() {
        None
    } else {
        Some(serde_json::to_value(&params.tools)?)
    };

    let body = ChatRequest {
        model,
        messages,
        tools,
        stream: true,
        // Ollama reasoning-capable profiles may emit almost everything in
        // `reasoning` and only a tiny `content` fragment (e.g. "S").
        // Ask for non-thinking mode so user-visible `content` is complete.
        think: Some(false),
        // Ollama defaults to num_predict: 128 — too short for real answers.
        // OpenAI-compatible servers ignore this if their own limit is lower.
        // 8192 (up from 4096) for the same reason as the Claude path: a
        // long answer with many sources truncates the trailing
        // `<CITATIONS>` block at 4096 and the frontend ends up with
        // gappy pill rendering. See v0.5.1 hotfix.
        max_tokens: Some(8192),
        temperature: Some(0.5),
    };

    let resp = client
        .post(format!("{}/chat/completions", base.trim_end_matches('/')))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("Local LLM error {status}: {text}"));
    }

    // Parse SSE stream
    let byte_stream = resp.bytes_stream();
    let event_stream = stream::unfold(
        (byte_stream, String::new(), VecDeque::<StreamEvent>::new()),
        |(mut bs, mut buf, mut pending)| async move {
            loop {
                if let Some(ev) = pending.pop_front() {
                    return Some((Ok(ev), (bs, buf, pending)));
                }

                while let Some(pos) = buf.find('\n') {
                    let line = buf[..pos].trim().to_string();
                    buf.drain(..=pos);
                    log_upstream_line(&line);
                    if let Some(ev) = parse_sse_line_opt(&line) {
                        pending.push_back(ev);
                    }
                }

                if let Some(ev) = pending.pop_front() {
                    return Some((Ok(ev), (bs, buf, pending)));
                }

                use futures_util::StreamExt;
                match bs.next().await {
                    None => {
                        // Connection closed by upstream. If anything remains
                        // in the buffer, try to parse it as a final event;
                        // otherwise close the stream cleanly. Do NOT emit a
                        // synthetic error for unparseable trailing data —
                        // that would be reported back to the client as a
                        // failed stream when the response is actually fine.
                        let trimmed = buf.trim().to_string();
                        buf.clear();
                        if !trimmed.is_empty() {
                            for line in trimmed.lines() {
                                log_upstream_line(line);
                                if let Some(ev) = parse_sse_line_opt(line.trim()) {
                                    pending.push_back(ev);
                                }
                            }
                        }
                        if let Some(ev) = pending.pop_front() {
                            return Some((Ok(ev), (bs, buf, pending)));
                        }
                        return None;
                    }
                    Some(Err(e)) => {
                        return Some((Err(anyhow!("stream error: {e}")), (bs, buf, pending)));
                    }
                    Some(Ok(bytes)) => {
                        buf.push_str(&String::from_utf8_lossy(&bytes));
                    }
                }
            }
        },
    );

    Ok(Box::pin(event_stream))
}

fn parse_sse_line(line: &str) -> Result<StreamEvent> {
    parse_sse_line_opt(line)
        .ok_or_else(|| anyhow!("empty SSE line"))
}

/// Parse one SSE record body emitted by an OpenAI-shape chat
/// completions stream. Returns `Some` for content / tool / done
/// events; `None` for keep-alive comments, unparseable lines, or
/// empty deltas. Pub(crate) so [`super::mistral`] can reuse it —
/// Mistral's SSE format is byte-identical to OpenAI's.
pub(crate) fn parse_sse_line_opt(line: &str) -> Option<StreamEvent> {
    if !line.starts_with("data: ") { return None; }
    let data = line[6..].trim();
    if data == "[DONE]" { return Some(StreamEvent::Done); }
    let chunk: StreamChunk = serde_json::from_str(data).ok()?;
    let delta = chunk.choices.into_iter().next()?.delta;
    if let Some(text) = delta.content.filter(|t| !t.is_empty()) {
        return Some(StreamEvent::ContentDelta(text));
    }
    if let Some(tcs) = delta.tool_calls {
        let calls: Vec<ToolCall> = tcs
            .into_iter()
            .filter_map(|tc| {
                let f = tc.function?;
                Some(ToolCall {
                    id: tc.id.unwrap_or_else(|| format!("tool-{}", tc.index)),
                    name: f.name.unwrap_or_default(),
                    input: serde_json::from_str(f.arguments.as_deref().unwrap_or("{}"))
                        .unwrap_or(json!({})),
                    // Gemini-only field; OpenAI-compatible endpoints
                    // don't emit a thought signature.
                    thought_signature: None,
                })
            })
            .collect();
        if !calls.is_empty() {
            return Some(StreamEvent::ToolCalls(calls));
        }
    }
    None
}

pub async fn complete(params: StreamParams) -> Result<String> {
    let (base, api_key, model) = resolve_endpoint(&params)?;
    tracing::info!("[llm/local] complete → base={base}, model={model}, key_present={}", !api_key.is_empty() && api_key != "local");
    let client = reqwest::Client::new();

    let messages = to_wire_messages(&effective_system(&params), &params.messages);
    let mut body = json!({
        "model": model,
        "messages": messages,
        "stream": false,
        "think": false,
        // See the note in the Claude provider: a structured answer is
        // an object, and a truncated object is invalid, not short.
        "max_tokens": if params.response_schema.is_some() { 8192 } else { 512 },
        "temperature": 0.5,
    });
    if let Some(schema) = &params.response_schema {
        // OpenAI-compatible servers (including llama.cpp and vLLM) read
        // `response_format`; Ollama reads a bare `format`. Sending both
        // costs nothing and saves a per-backend branch we would have to
        // keep in step with whatever the user points us at.
        body["response_format"] = json!({
            "type": "json_schema",
            "json_schema": {
                "name": "cove_studio_result",
                "schema": schema,
                "strict": true,
            }
        });
        body["format"] = schema.clone();
    }

    let resp = client
        .post(format!("{}/chat/completions", base.trim_end_matches('/')))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        tracing::error!("[llm/local] complete non-success {status}: {text}");
        return Err(anyhow!("Local LLM error {status}: {text}"));
    }

    #[derive(Deserialize)]
    struct Resp { choices: Vec<RespChoice> }
    #[derive(Deserialize)]
    struct RespChoice { message: RespMessage }
    #[derive(Deserialize)]
    struct RespMessage {
        content: Option<String>,
        reasoning: Option<String>,
        reasoning_content: Option<String>,
    }

    let raw = resp.text().await
        .map_err(|e| { tracing::error!("[llm/local] complete read body: {e}"); anyhow!(e) })?;
    let preview: String = raw.replace('\n', "\\n").chars().take(2000).collect();
    tracing::info!("[llm/local] upstream_complete_body {preview}");
    let data: Resp = serde_json::from_str(&raw)
        .map_err(|e| {
            tracing::error!("[llm/local] complete parse error: {e} (body: {})", raw.chars().take(400).collect::<String>());
            anyhow!("Local LLM body parse error: {e}")
        })?;
    Ok(data.choices.into_iter().next()
        .and_then(|c| c.message.content)
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::StreamEvent;

    fn first_catalogue_model() -> String {
        crate::presets::local_models::catalogue()[0].id.clone()
    }

    #[test]
    fn secure_mode_accepts_a_legacy_name_not_yet_migrated() {
        let Some(legacy) = crate::presets::local_models::catalogue()
            .into_iter()
            .find_map(|m| m.legacy_ids.first().cloned())
        else {
            return;
        };
        let p = params_with_local("http://localhost:11434", &legacy, true);
        assert!(resolve_endpoint(&p).is_ok());
    }

    fn params_with_local(
        base_url: &str,
        model: &str,
        secure_mode: bool,
    ) -> StreamParams {
        StreamParams {
            model: model.to_string(),
            system_prompt: "you are the assistant".into(),
            system_volatile: String::new(),
            messages: vec![Message::user("hi".to_string())],
            tools: vec![],
            max_iterations: 1,
            enable_thinking: false,
            local_config: Some(crate::llm::types::LocalConfig {
                base_url: base_url.to_string(),
                api_key: None,
                model: model.to_string(),
                secure_mode,
            }),
            chat_id: None,
            mistral_opts: None,
            response_schema: None,
            claude_api_key: None,
            gemini_api_key: None,
            gemini_region: None,
        }
    }

    #[test]
    fn loopback_url_classification() {
        for ok in &[
            "http://localhost/v1",
            "http://127.0.0.1:11434/v1",
            "http://[::1]:11434/v1",
            "https://localhost:8443/v1",
        ] {
            assert!(is_loopback_url(ok), "expected loopback: {ok}");
        }
        for bad in &[
            "http://192.168.1.10:11434/v1",
            "http://10.0.0.5/v1",
            "https://ollama.example.com/v1",
            "http://my-laptop.local:11434/v1",
        ] {
            assert!(!is_loopback_url(bad), "expected NOT loopback: {bad}");
        }
    }

    #[test]
    fn secure_mode_rejects_non_loopback_url() {
        let p = params_with_local(
            "https://ollama.example.com",
            &first_catalogue_model(),
            true,
        );
        let err = resolve_endpoint(&p).unwrap_err();
        assert!(
            err.to_string().contains("Modalità sicura"),
            "secure-mode guard must mention itself in the error (got {err})"
        );
    }

    #[test]
    fn secure_mode_rejects_uncurated_model() {
        let p = params_with_local("http://localhost:11434", "qwen2.5:7b", true);
        let err = resolve_endpoint(&p).unwrap_err();
        assert!(err.to_string().contains("allowlist"), "got: {err}");
    }

    #[test]
    fn secure_mode_accepts_curated_model_on_loopback() {
        let p = params_with_local(
            "http://localhost:11434",
            &first_catalogue_model(),
            true,
        );
        let (base, _key, model) = resolve_endpoint(&p).unwrap();
        // Base URL is snapped to the canonical loopback form.
        assert!(base.starts_with("http://localhost:11434"));
        assert_eq!(model, first_catalogue_model());
    }

    #[test]
    fn secure_mode_accepts_curated_model_with_latest_suffix() {
        // Reproduces the 2026-06-07 false-positive rejection: the
        // free-form Settings refresh path saved `main_model =
        // local:<model>:latest` after reading Ollama's
        // tag-suffixed local-model list. Without the strip_suffix
        // normalisation the allowlist contains check failed and the
        // chat composer flagged the model as "non in allowlist".
        let p = params_with_local(
            "http://localhost:11434",
            &format!("{}:latest", first_catalogue_model()),
            true,
        );
        let (_base, _key, model) = resolve_endpoint(&p).unwrap();
        // We keep the original :latest on the way out — that's what
        // Ollama actually expects on the wire — but the allowlist
        // check normalises only for the membership test.
        assert_eq!(model, format!("{}:latest", first_catalogue_model()));
    }

    #[test]
    fn non_secure_mode_permits_any_url_and_model() {
        let p = params_with_local("http://192.168.1.5:11434", "qwen2.5:7b", false);
        let (base, _key, model) = resolve_endpoint(&p).unwrap();
        assert!(base.contains("192.168.1.5"));
        assert_eq!(model, "qwen2.5:7b");
    }

    #[test]
    fn effective_system_prepends_preamble_in_secure_mode() {
        let p = params_with_local(
            "http://localhost:11434",
            &first_catalogue_model(),
            true,
        );
        let s = effective_system(&p);
        assert!(s.starts_with("[Modalità sicura locale]"));
        assert!(s.contains("you are the assistant"));
    }

    #[test]
    fn effective_system_unchanged_off_secure_mode() {
        let p = params_with_local("http://localhost:11434", "anything", false);
        let s = effective_system(&p);
        assert_eq!(s, "you are the assistant");
    }

    #[test]
    fn parses_done_marker() {
        match parse_sse_line_opt("data: [DONE]") {
            Some(StreamEvent::Done) => {}
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn ignores_non_data_lines() {
        assert!(parse_sse_line_opt(": comment").is_none());
        assert!(parse_sse_line_opt("event: message").is_none());
        assert!(parse_sse_line_opt("").is_none());
    }

    #[test]
    fn parses_content_delta() {
        let line = r#"data: {"id":"x","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"content":"hello"},"finish_reason":null}]}"#;
        match parse_sse_line_opt(line) {
            Some(StreamEvent::ContentDelta(s)) => assert_eq!(s, "hello"),
            other => panic!("expected ContentDelta, got {other:?}"),
        }
    }

    #[test]
    fn parses_tool_calls() {
        let line = r#"data: {"id":"x","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"read_document","arguments":"{\"doc_id\":\"doc-0\"}"}}]},"finish_reason":null}]}"#;
        match parse_sse_line_opt(line) {
            Some(StreamEvent::ToolCalls(calls)) => {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].id, "call_1");
                assert_eq!(calls[0].name, "read_document");
                assert_eq!(calls[0].input["doc_id"], "doc-0");
            }
            other => panic!("expected ToolCalls, got {other:?}"),
        }
    }

    #[test]
    fn ignores_reasoning_only_delta() {
        let line = r#"data: {"id":"x","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"reasoning":"thinking text"},"finish_reason":null}]}"#;
        assert!(parse_sse_line_opt(line).is_none());
    }

    #[test]
    fn malformed_json_returns_none() {
        assert!(parse_sse_line_opt("data: not json").is_none());
    }

    #[test]
    fn empty_delta_returns_none() {
        let line = r#"data: {"id":"x","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{},"finish_reason":null}]}"#;
        assert!(parse_sse_line_opt(line).is_none());
    }
}
