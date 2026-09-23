/// Anthropic Claude — Messages API with streaming (text/event-stream)
use anyhow::{anyhow, Result};
use futures_util::{stream, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};

use super::types::{Message, Role, StreamEvent, StreamParams};
use crate::llm::BoxStream;

fn api_key(params: &StreamParams) -> Result<String> {
    if let Some(k) = params.claude_api_key.as_ref().filter(|s| !s.trim().is_empty()) {
        return Ok(k.clone());
    }
    std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| anyhow!("Anthropic API key not configured: set it in Account → Models, or set ANTHROPIC_API_KEY"))
}

fn to_wire_messages(messages: &[Message]) -> Vec<Value> {
    messages
        .iter()
        .filter(|m| m.role != Role::System)
        .map(|m| {
            let role = match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "user",
                Role::System => "user",
            };
            json!({ "role": role, "content": m.content })
        })
        .collect()
}

pub async fn stream(params: StreamParams) -> Result<BoxStream> {
    let key = api_key(&params)?;
    let client = reqwest::Client::new();

    let wire_messages = to_wire_messages(&params.messages);
    let mut body = json!({
        "model": params.model,
        // 8192 doubles the headroom for the trailing `<CITATIONS>`
        // JSON block when a long answer cites many sources (a 10-PDF
        // medical-legal report easily emits 40+ annotations + ~3 KB
        // of prose). 4096 was truncating annotation arrays in
        // production — see v0.5.1 hotfix. Every catalogued Claude
        // model in `config/model.json` supports ≥8192 output.
        "max_tokens": 8192,
        // 0.5 (v0.5.3+) — tighter sampling than Anthropic's default
        // of 1.0. Citations + structured JSON blocks + tool calls
        // all benefit from less-random sampling. Project-wide
        // setting across all providers.
        "temperature": 0.5,
        "stream": true,
        "messages": wire_messages,
    });
    // Send `system` as content blocks so the stable prefix can carry a
    // `cache_control` breakpoint. Anthropic then caches that prefix for
    // ~5 min; follow-up turns of the same chat re-use it at ~10% of the
    // input-token cost and with a faster time-to-first-token. The
    // volatile tail (per-query KB retrieval) follows uncached so it
    // never invalidates the cached prefix. All Claude 3+ models support
    // this; below the cache minimum Anthropic just skips caching, no error.
    let mut system_blocks: Vec<Value> = Vec::new();
    if !params.system_prompt.is_empty() {
        system_blocks.push(json!({
            "type": "text",
            "text": params.system_prompt,
            "cache_control": { "type": "ephemeral" },
        }));
    }
    if !params.system_volatile.is_empty() {
        system_blocks.push(json!({ "type": "text", "text": params.system_volatile }));
    }
    if !system_blocks.is_empty() {
        body["system"] = json!(system_blocks);
    }

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("Claude API error {status}: {text}"));
    }

    let byte_stream = resp.bytes_stream();
    let event_stream = stream::unfold(
        (byte_stream, String::new()),
        |(mut bs, mut buf)| async move {
            loop {
                match bs.next().await {
                    None => {
                        if buf.trim().is_empty() { return None; }
                        let line = buf.trim().to_string();
                        buf.clear();
                        return Some((parse_claude_sse(&line), (bs, buf)));
                    }
                    Some(Err(e)) => {
                        return Some((Err(anyhow!("stream error: {e}")), (bs, buf)));
                    }
                    Some(Ok(bytes)) => {
                        buf.push_str(&String::from_utf8_lossy(&bytes));
                        while let Some(pos) = buf.find('\n') {
                            let line = buf[..pos].trim().to_string();
                            buf.drain(..=pos);
                            if let Some(ev) = parse_claude_sse_opt(&line) {
                                return Some((Ok(ev), (bs, buf)));
                            }
                        }
                    }
                }
            }
        },
    );

    Ok(Box::pin(event_stream))
}

fn parse_claude_sse(line: &str) -> Result<StreamEvent> {
    parse_claude_sse_opt(line).ok_or_else(|| anyhow!("empty SSE line"))
}

fn parse_claude_sse_opt(line: &str) -> Option<StreamEvent> {
    if !line.starts_with("data: ") { return None; }
    let data = line[6..].trim();
    let v: Value = serde_json::from_str(data).ok()?;
    let event_type = v.get("type")?.as_str()?;
    match event_type {
        "content_block_delta" => {
            let delta = v.get("delta")?;
            if delta.get("type")?.as_str()? == "text_delta" {
                let text = delta.get("text")?.as_str()?.to_string();
                return Some(StreamEvent::ContentDelta(text));
            }
            None
        }
        "message_start" => {
            // Surface prompt-cache effectiveness: `cache_read` > 0 means a
            // follow-up turn re-used the cached system prefix instead of
            // re-billing it. Logged only when caching actually engaged.
            if let Some(usage) = v.get("message").and_then(|m| m.get("usage")) {
                let read = usage
                    .get("cache_read_input_tokens")
                    .and_then(|x| x.as_u64())
                    .unwrap_or(0);
                let written = usage
                    .get("cache_creation_input_tokens")
                    .and_then(|x| x.as_u64())
                    .unwrap_or(0);
                if read > 0 || written > 0 {
                    let fresh = usage
                        .get("input_tokens")
                        .and_then(|x| x.as_u64())
                        .unwrap_or(0);
                    tracing::info!(
                        "[claude] prompt cache: {read} tokens read from cache, \
                         {written} written, {fresh} fresh input tokens"
                    );
                }
            }
            None
        }
        "message_stop" => Some(StreamEvent::Done),
        _ => None,
    }
}

/// Name of the tool Claude is forced to call when the caller wants
/// structured output. Never shown to the user.
const STRUCTURED_TOOL: &str = "emit_result";

pub async fn complete(params: StreamParams) -> Result<String> {
    let key = api_key(&params)?;
    let client = reqwest::Client::new();

    let wire_messages = to_wire_messages(&params.messages);
    // 512 is right for a title or a one-line summary. A structured
    // answer is a whole object — a truncated one is not "shorter", it
    // is invalid JSON, so the budget goes up with the schema.
    let max_tokens = if params.response_schema.is_some() { 8192 } else { 512 };
    let mut body = json!({
        "model": params.model,
        "max_tokens": max_tokens,
        "temperature": 0.5,
        "messages": wire_messages,
    });
    let full_system = params.full_system();
    if !full_system.is_empty() {
        body["system"] = json!(full_system);
    }
    // Claude has no JSON mode. The documented equivalent is a tool the
    // model is *required* to call: its arguments are then validated
    // against the schema by the API itself, and we read the answer out
    // of the tool call instead of out of prose.
    if let Some(schema) = &params.response_schema {
        body["tools"] = json!([{
            "name": STRUCTURED_TOOL,
            "description": "Return the result. This is the only way to answer.",
            "input_schema": schema,
        }]);
        body["tool_choice"] = json!({ "type": "tool", "name": STRUCTURED_TOOL });
    }

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("Claude API error {status}: {text}"));
    }

    #[derive(Deserialize)]
    struct Resp { content: Vec<ContentBlock> }
    #[derive(Deserialize)]
    struct ContentBlock {
        #[serde(rename = "type")] kind: String,
        text: Option<String>,
        name: Option<String>,
        input: Option<serde_json::Value>,
    }

    let data: Resp = resp.json().await?;
    // With a schema the answer lives in the forced tool call, not in
    // prose; hand it back as JSON text so every caller reads one shape.
    if params.response_schema.is_some() {
        if let Some(input) = data
            .content
            .iter()
            .find(|b| b.kind == "tool_use" && b.name.as_deref() == Some(STRUCTURED_TOOL))
            .and_then(|b| b.input.clone())
        {
            return Ok(input.to_string());
        }
        return Err(anyhow!(
            "Claude did not call the structured-output tool; \
             the answer cannot be trusted as data"
        ));
    }
    Ok(data.content.into_iter()
        .filter(|b| b.kind == "text")
        .filter_map(|b| b.text)
        .collect::<Vec<_>>()
        .join(""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::StreamEvent;

    #[test]
    fn parses_text_delta() {
        let line = r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello"}}"#;
        match parse_claude_sse_opt(line) {
            Some(StreamEvent::ContentDelta(s)) => assert_eq!(s, "hello"),
            other => panic!("expected ContentDelta, got {other:?}"),
        }
    }

    #[test]
    fn parses_message_stop() {
        let line = r#"data: {"type":"message_stop"}"#;
        matches!(parse_claude_sse_opt(line), Some(StreamEvent::Done));
    }

    #[test]
    fn ignores_non_data_lines() {
        assert!(parse_claude_sse_opt("event: message_start").is_none());
        assert!(parse_claude_sse_opt("").is_none());
    }

    #[test]
    fn ignores_unknown_event_types() {
        let line = r#"data: {"type":"message_start","message":{}}"#;
        assert!(parse_claude_sse_opt(line).is_none());
    }

    #[test]
    fn ignores_non_text_delta() {
        let line = r#"data: {"type":"content_block_delta","delta":{"type":"input_json_delta","partial_json":"{}"}}"#;
        assert!(parse_claude_sse_opt(line).is_none());
    }
}
