use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response, Sse},
    response::sse::Event,
    routing::get,
    Json, Router,
};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc};
use tokio_stream::wrappers::ReceiverStream;

use crate::{
    auth::middleware::AuthUser,
    llm::{
        self, builtin_tools, LocalConfig, Message, Role, StreamEvent, StreamParams, ToolCall,
        ToolFunction, ToolSchema,
    },
    routes::user::{fetch_llm_settings, fetch_mcp_servers, read_jsonrpc_response, McpServerOut},
    storage::make_storage,
    AppState,
};
use std::collections::{HashMap, HashSet};

mod attachment_budget;
mod citation_resolution;
mod prompts;
mod tool_echo;

/// Build the OpenAI-compatible `LocalConfig` for a model id carrying a
/// `local:`, `openai:` or `mistral:` prefix. Returns `None` for native
/// cloud models (Claude / Gemini) or when the selected provider has no
/// endpoint / key configured.
///
/// Mistral and OpenAI have fixed public endpoints; `local:` reads the
/// user's BYO base URL. The model name is the per-provider stored model
/// field, falling back to the id with its prefix stripped.
pub fn build_local_config(
    model: &str,
    settings: Option<&crate::routes::user::LlmSettings>,
) -> Option<LocalConfig> {
    let s = settings?;
    let requested_model = llm::strip_model_prefix(model).trim().to_string();
    let (base, key, stored_model) = if model.starts_with("openai:") {
        (
            s.openai_api_key
                .as_ref()
                .map(|_| "https://api.openai.com/v1".to_string())
                .unwrap_or_default(),
            s.openai_api_key.clone(),
            s.openai_model.clone(),
        )
    } else if model.starts_with("mistral:") {
        (
            s.mistral_api_key
                .as_ref()
                .map(|_| "https://api.mistral.ai/v1".to_string())
                .unwrap_or_default(),
            s.mistral_api_key.clone(),
            s.mistral_model.clone(),
        )
    } else if model.starts_with("local:") {
        (
            s.local_base_url.clone().unwrap_or_default(),
            s.local_api_key.clone(),
            s.local_model.clone(),
        )
    } else {
        return None;
    };
    if base.trim().is_empty() {
        return None;
    }
    Some(LocalConfig {
        base_url: base,
        api_key: key.filter(|k| !k.trim().is_empty()),
        model: if requested_model.is_empty() {
            stored_model
                .filter(|m| !m.trim().is_empty())
                .unwrap_or_default()
        } else {
            requested_model
        },
        // Secure mode only applies to the `local:` provider — the
        // openai: and mistral: cloud branches don't read this flag.
        // Setting it on those is a no-op (the enforcement guards in
        // src/llm/local.rs only fire when the request is actually
        // routed to a local Ollama endpoint).
        secure_mode: model.starts_with("local:") && s.local_secure_mode,
    })
}

/// Build the per-request Mistral options snapshot from the saved
/// `user_settings.mistral_*` columns. Returns `None` when the call
/// isn't routed to Mistral (the Mistral provider path is the only
/// consumer; OpenAI / local / Claude / Gemini ignore this field on
/// `StreamParams`). The two flags default to `false` server-side
/// via the migration 0033 DEFAULT clause, matching the v0.6.0
/// Commit A hard-coded behaviour.
pub fn build_mistral_opts(
    model: &str,
    settings: Option<&crate::routes::user::LlmSettings>,
) -> Option<crate::llm::types::MistralOpts> {
    if !model.starts_with("mistral:") {
        return None;
    }
    let s = settings?;
    Some(crate::llm::types::MistralOpts {
        safe_prompt: s.mistral_safe_prompt,
        parallel_tool_calls: s.mistral_parallel_tools,
    })
}

// ---------------------------------------------------------------------------
// MCP capability discovery — surfaces configured servers to the chat model
// ---------------------------------------------------------------------------

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct McpDiscovered {
    config_name: String,
    server_name: Option<String>,
    server_version: Option<String>,
    instructions: Option<String>,
    tools: Vec<(String, String)>,    // (name, description) — for system prompt rendering
    /// Full tool schemas (incl. inputSchema) ready to be passed to the LLM.
    tool_schemas: Vec<ToolSchema>,
    prompts: Vec<(String, String)>,  // (name, description)
    /// Coordinates needed to dispatch a `tools/call` later.
    url: Option<String>,
    api_key: Option<String>,
    extra_headers: serde_json::Map<String, serde_json::Value>,
    session_id: Option<String>,
}

async fn discover_one_mcp(server: McpServerOut) -> Option<McpDiscovered> {
    if server.transport == "stdio" {
        return Some(McpDiscovered {
            config_name: server.name,
            server_name: None,
            server_version: None,
            instructions: Some(format!(
                "(Configured as stdio: command={} args={:?}; runtime spawning is not yet wired in this build.)",
                server.command.as_deref().unwrap_or(""),
                server.args
            )),
            tools: vec![],
            tool_schemas: vec![],
            prompts: vec![],
            url: None,
            api_key: None,
            extra_headers: serde_json::Map::new(),
            session_id: None,
        });
    }
    let url = server.url.as_ref()?.clone();
    if url.trim().is_empty() {
        return None;
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().ok()?);
    headers.insert(
        "Accept",
        "application/json, text/event-stream".parse().ok()?,
    );
    if let Some(k) = server.api_key.as_ref().filter(|k| !k.trim().is_empty()) {
        if let Ok(v) = format!("Bearer {k}").parse() {
            headers.insert("Authorization", v);
        }
    }
    for (k, v) in &server.headers {
        if let Some(s) = v.as_str() {
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                s.parse::<reqwest::header::HeaderValue>(),
            ) {
                headers.insert(name, value);
            }
        }
    }

    // 1) initialize → capture session id
    let init_resp = client
        .post(&url)
        .headers(headers.clone())
        .json(&json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": crate::product::NAME, "version": env!("CARGO_PKG_VERSION") }
            }
        }))
        .send()
        .await
        .ok()?;

    if !init_resp.status().is_success() {
        tracing::warn!("[mcp/discover] {}: initialize {}", server.name, init_resp.status());
        return None;
    }

    let session_id: Option<String> = init_resp
        .headers()
        .get("mcp-session-id")
        .or_else(|| init_resp.headers().get("Mcp-Session-Id"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let init_value = read_jsonrpc_response(init_resp, 1, 10).await.ok()?;
    let server_name = init_value["result"]["serverInfo"]["name"]
        .as_str()
        .map(|s| s.to_string());
    let server_version = init_value["result"]["serverInfo"]["version"]
        .as_str()
        .map(|s| s.to_string());
    let instructions = init_value["result"]["instructions"]
        .as_str()
        .map(|s| s.to_string());

    // 2) Build session-aware headers
    let mut session_headers = headers.clone();
    if let Some(sid) = &session_id {
        if let Ok(v) = sid.parse() {
            session_headers.insert("Mcp-Session-Id", v);
        }
    }

    // 3) notifications/initialized handshake completion (best-effort)
    let _ = client
        .post(&url)
        .headers(session_headers.clone())
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        }))
        .send()
        .await;

    // 4) tools/list — keep the full inputSchema for tool-use, plus a
    // (name, description) summary for the system prompt rendering.
    let raw_tools: Vec<Value> = match client
        .post(&url)
        .headers(session_headers.clone())
        .json(&json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}))
        .send()
        .await
    {
        Ok(r) => read_jsonrpc_response(r, 2, 8)
            .await
            .ok()
            .and_then(|v| v["result"]["tools"].as_array().cloned())
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    let tools: Vec<(String, String)> = raw_tools
        .iter()
        .map(|t| (
            t["name"].as_str().unwrap_or("").to_string(),
            t["description"].as_str().unwrap_or("").to_string(),
        ))
        .collect();
    let tool_schemas: Vec<ToolSchema> = raw_tools
        .iter()
        .map(|t| ToolSchema {
            kind: "function".to_string(),
            function: ToolFunction {
                name: t["name"].as_str().unwrap_or("").to_string(),
                description: t["description"].as_str().unwrap_or("").to_string(),
                parameters: t["inputSchema"].clone(),
            },
        })
        .collect();

    // 5) prompts/list
    let prompts = match client
        .post(&url)
        .headers(session_headers.clone())
        .json(&json!({"jsonrpc":"2.0","id":3,"method":"prompts/list","params":{}}))
        .send()
        .await
    {
        Ok(r) => read_jsonrpc_response(r, 3, 8)
            .await
            .ok()
            .and_then(|v| v["result"]["prompts"].as_array().cloned())
            .map(|arr| {
                arr.into_iter()
                    .map(|p| {
                        (
                            p["name"].as_str().unwrap_or("").to_string(),
                            p["description"].as_str().unwrap_or("").to_string(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    Some(McpDiscovered {
        config_name: server.name,
        server_name,
        server_version,
        instructions,
        tools,
        tool_schemas,
        prompts,
        url: Some(url.clone()),
        api_key: server.api_key,
        extra_headers: server.headers,
        session_id,
    })
}

/// Dispatch a tool call to the right MCP server using its session id.
/// Returns a string suitable for `tool` role message content.
///
/// Verbose phase-by-phase logging: every line carries the elapsed-ms
/// since dispatch start so the user can see *exactly* where time
/// goes — useful when an MCP tool requires interactive approval on
/// the server side and the call appears to "hang".
async fn dispatch_mcp_tool(
    servers: &[McpDiscovered],
    tool_name: &str,
    arguments: &Value,
) -> String {
    let dispatch_start = std::time::Instant::now();
    macro_rules! mtrace {
        ($fmt:literal $(, $arg:expr)* $(,)?) => {
            tracing::info!(
                concat!("[mcp/dispatch] tool={} +{}ms — ", $fmt),
                tool_name,
                dispatch_start.elapsed().as_millis()
                $(, $arg)*
            )
        };
    }

    let Some(srv) = servers.iter().find(|s| {
        s.tool_schemas.iter().any(|t| t.function.name == tool_name)
    }) else {
        tracing::warn!(
            "[mcp/dispatch] tool={} +0ms — no MCP server provides this tool (known servers: {:?})",
            tool_name,
            servers.iter().map(|s| s.config_name.as_str()).collect::<Vec<_>>()
        );
        return json!({"error": format!("No MCP server provides tool '{tool_name}'")}).to_string();
    };
    let Some(url) = &srv.url else {
        return json!({"error": "tool's MCP server has no URL"}).to_string();
    };

    let timeout_secs = crate::db::mcp_call_timeout_secs();
    mtrace!(
        "routing to server={} url={} session_id={} timeout={}s",
        srv.config_name,
        url,
        srv.session_id.as_deref().unwrap_or("(none)"),
        timeout_secs
    );

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
    {
        Ok(c) => c,
        Err(e) => return json!({"error": e.to_string()}).to_string(),
    };

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::CONTENT_TYPE, "application/json".parse().unwrap());
    headers.insert(reqwest::header::ACCEPT, "application/json, text/event-stream".parse().unwrap());
    if let Some(k) = srv.api_key.as_ref().filter(|k| !k.trim().is_empty()) {
        if let Ok(v) = format!("Bearer {k}").parse() {
            headers.insert(reqwest::header::AUTHORIZATION, v);
        }
    }
    for (k, v) in &srv.extra_headers {
        if let Some(s) = v.as_str() {
            if let (Ok(name), Ok(val)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                s.parse::<reqwest::header::HeaderValue>(),
            ) {
                headers.insert(name, val);
            }
        }
    }
    if let Some(sid) = &srv.session_id {
        if let Ok(v) = sid.parse() {
            headers.insert("Mcp-Session-Id", v);
        }
    }

    let body = json!({
        "jsonrpc": "2.0",
        "id": 100,
        "method": "tools/call",
        "params": {
            "name": tool_name,
            "arguments": arguments,
        }
    });
    let body_bytes = body.to_string().len();
    mtrace!(
        "POST {} (body {} bytes, {} args, headers: {:?})",
        url,
        body_bytes,
        arguments
            .as_object()
            .map(|m| m.len())
            .unwrap_or(0),
        headers
            .keys()
            .map(|k| k.as_str())
            .filter(|k| !k.eq_ignore_ascii_case("authorization")) // never log Bearer tokens
            .collect::<Vec<_>>()
    );

    let resp = match client.post(url).headers(headers).json(&body).send().await {
        Ok(r) => {
            mtrace!(
                "POST returned: status={} content-type={:?}",
                r.status(),
                r.headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|h| h.to_str().ok())
            );
            r
        }
        Err(e) => {
            mtrace!("POST failed: {}", e);
            return json!({"error": format!("network: {e}")}).to_string();
        }
    };

    mtrace!("reading response body / SSE stream (timeout {}s)", timeout_secs);
    // Reader timeout matches the wire-level timeout — otherwise the
    // SSE stream reader could give up earlier than the HTTP client
    // and we'd lose a long but legitimate tool response (e.g. Edge
    // pseudonymising a multi-MB document, or a tool that requires
    // interactive human approval before releasing the response).
    let val = match read_jsonrpc_response(resp, 100, timeout_secs).await {
        Ok(v) => {
            mtrace!("body decoded as JSON-RPC, ~{} chars", v.to_string().len());
            v
        }
        Err(e) => {
            mtrace!("body read failed: {}", e);
            return json!({"error": format!("read: {e}")}).to_string();
        }
    };

    if let Some(rpc_err) = val.get("error") {
        mtrace!("JSON-RPC error in response: {}", rpc_err);
        return json!({"error": rpc_err}).to_string();
    }

    // MCP tools/call result is `{content: [{type:"text", text:"…"}, …], isError?:bool}`
    let content = &val["result"]["content"];
    if let Some(arr) = content.as_array() {
        let joined: Vec<String> = arr
            .iter()
            .filter_map(|c| c["text"].as_str().map(|s| s.to_string()))
            .collect();
        if !joined.is_empty() {
            mtrace!(
                "DONE — returning {} text chunk(s), {} total chars",
                joined.len(),
                joined.iter().map(|s| s.len()).sum::<usize>()
            );
            return joined.join("\n");
        }
    }
    let fallback = val["result"].to_string();
    mtrace!(
        "DONE — content array empty, returning result-as-string ({} chars)",
        fallback.len()
    );
    fallback
}

/// Dispatch an MCP tool, then transparently auto-chain a follow-up
/// `get_*` call when the server returns the async-pending pattern.
///
/// Pattern detection (Edge's pseudonymise flow is the canonical
/// example):
///
///   1. Model calls `request_pseudonymized_documents(ids=[…])`
///   2. Edge returns `{session_id, status:"pending", doc_count:N}`
///      — the actual documents aren't ready yet because Edge wants
///      a human to click "Conferma" in its UI first.
///   3. Without auto-chain, the model receives the pending envelope
///      as the tool result, almost always declares the job done,
///      and never fetches the real documents.
///
/// Auto-chain bridges step 3 by:
///
///   * recognising the `{session_id, status:"pending"}` shape;
///   * deriving the companion tool name (`request_X` → `get_X`);
///   * checking the same MCP server actually exposes that companion;
///   * dispatching it with `{session_id, wait_for_approval: true,
///     wait_timeout_seconds: <our timeout>}` so the long-poll
///     completes server-side;
///   * substituting the get_* result for the original.
///
/// Generic enough to fit any MCP server that uses the same naming
/// convention. Tools that don't follow the pattern (or that already
/// return their full result inline) are unaffected — the function
/// degrades to a passthrough.
async fn dispatch_mcp_tool_with_async_chain(
    servers: &[McpDiscovered],
    tool_name: &str,
    arguments: &Value,
) -> String {
    let primary = dispatch_mcp_tool(servers, tool_name, arguments).await;

    // Only the "request_*" tools can ever trigger a chain — short-
    // circuit otherwise so we don't pay the JSON parse for every
    // tool result (most are already final).
    let companion_name = match tool_name.strip_prefix("request_") {
        Some(rest) => format!("get_{rest}"),
        None => return primary,
    };

    // Try to parse the response as JSON. If it isn't JSON, or the
    // shape doesn't match the pending pattern, just return as-is.
    let parsed: Value = match serde_json::from_str(&primary) {
        Ok(v) => v,
        Err(_) => return primary,
    };
    let session_id = parsed
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let status = parsed
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let is_pending = matches!(
        status,
        "pending" | "queued" | "in_review" | "awaiting_approval"
    );
    let (Some(session_id), true) = (session_id, is_pending) else {
        return primary;
    };

    // The companion tool must exist on the same server that handled
    // the request — calling it on a different server would land in
    // the wrong session-id namespace.
    let server_has_companion = servers.iter().any(|s| {
        s.tool_schemas
            .iter()
            .any(|t| t.function.name == tool_name)
            && s.tool_schemas
                .iter()
                .any(|t| t.function.name == companion_name)
    });
    if !server_has_companion {
        tracing::info!(
            "[mcp/dispatch] auto-chain skipped: {} returned pending session_id={} \
             but companion {} not found on the same server — passing the pending \
             envelope to the model so it can decide what to do",
            tool_name,
            session_id,
            companion_name
        );
        return primary;
    }

    let timeout_secs = crate::db::mcp_call_timeout_secs();
    let chain_args = json!({
        "session_id": session_id,
        // Edge's flag — long-poll until the human clicks Conferma.
        // Other MCP servers using the same naming pattern may
        // ignore this kwarg, which is fine.
        "wait_for_approval": true,
        "wait_timeout_seconds": timeout_secs,
    });
    tracing::info!(
        "[mcp/dispatch] auto-chain {} → {} with session_id={} \
         (wait_for_approval=true, timeout={}s)",
        tool_name,
        companion_name,
        session_id,
        timeout_secs
    );

    let chained = dispatch_mcp_tool(servers, &companion_name, &chain_args).await;
    tracing::info!(
        "[mcp/dispatch] auto-chain done: {} → {} returned {} chars",
        tool_name,
        companion_name,
        chained.len()
    );
    chained
}

async fn discover_mcp_for_user(state: &AppState, user_id: &str) -> Vec<McpDiscovered> {
    let ttl = crate::db::mcp_cache_ttl();

    // Cache hit: deserialise and return without touching the network.
    {
        let cache = state.mcp_discovery_cache.read().await;
        if let Some(entry) = cache.get(user_id) {
            if entry.is_fresh(ttl) {
                if let Ok(parsed) =
                    serde_json::from_str::<Vec<McpDiscovered>>(&entry.payload_json)
                {
                    tracing::info!(
                        "[mcp/discover] cache hit for user={}: {} servers ({} sec old, ttl {}s)",
                        user_id,
                        parsed.len(),
                        entry.stored_at.elapsed().as_secs(),
                        ttl.as_secs(),
                    );
                    return parsed;
                }
                tracing::warn!(
                    "[mcp/discover] cache entry deserialise failed for user={}, re-discovering",
                    user_id
                );
            }
        }
    }

    // Cache miss / stale: do the full handshake.
    let servers = match fetch_mcp_servers(&state.db, user_id).await {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let enabled: Vec<McpServerOut> =
        servers.into_iter().filter(|s| s.enabled).collect();
    if enabled.is_empty() {
        // Drop any prior cached entry — the user just disabled all servers.
        state.mcp_discovery_cache.write().await.remove(user_id);
        return vec![];
    }
    use futures_util::future::join_all;
    let futs = enabled.into_iter().map(discover_one_mcp);
    let discovered: Vec<McpDiscovered> =
        join_all(futs).await.into_iter().flatten().collect();
    tracing::info!(
        "[mcp/discover] cache miss for user={}: discovered {} servers via fresh handshake",
        user_id,
        discovered.len()
    );

    // Store in cache for next request.
    if let Ok(payload_json) = serde_json::to_string(&discovered) {
        let mut g = state.mcp_discovery_cache.write().await;
        g.insert(
            user_id.to_string(),
            crate::db::McpDiscoveryCacheEntry {
                stored_at: std::time::Instant::now(),
                payload_json,
            },
        );
    }

    discovered
}

/// Reduce a corpus identifier (or any model-emitted `doc_id` variant)
/// to its alphanumeric-only, lowercase canonical form. Used by the
/// citation resolver as a last-resort lookup key against the user's
/// full corpus library so that bracket/space/separator/case variants
/// the model produces (e.g. `[italian-legal] corte_costituzionale_1990_241`,
/// `Italian-Legal_corte_costituzionale_1990_241`, or even
/// `italianlegal:cortecostituzionale1990/241`) all collapse onto the
/// same key as the canonical `<corpus_id><corpus_identifier>` we index.
fn canonical_corpus_key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Repair the invalid backslash escapes LLMs routinely emit inside the
/// `<CITATIONS>` JSON. The model copies verbatim quotes and over-escapes
/// them — most commonly an apostrophe as `\'`, which is NOT a legal JSON
/// escape and makes `serde_json` reject the whole block. JSON only
/// allows `\` before `" \ / b f n r t u`; for any other follower the
/// backslash is spurious, so we drop it and keep the character. This
/// only ever turns an unparseable block into a parseable one.
fn repair_json_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some(&n) if matches!(n, '"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' | 'u') => {
                out.push('\\');
                out.push(n);
                chars.next();
            }
            // Spurious escape (e.g. `\'`): drop the backslash, keep the char.
            Some(&n) => {
                out.push(n);
                chars.next();
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Extract the JSON inside a `<CITATIONS>...</CITATIONS>` block at the end
/// of the assistant response. Tolerant of:
/// * surrounding whitespace and `` ```json `` code fences,
/// * the invalid backslash escapes LLMs commonly emit (`\'` etc.),
/// * a **missing closing tag** — the model ran out of output tokens
///   before writing `</CITATIONS>` (observed when emitting 30+ citations
///   for a full report). In that case we take everything from the open
///   tag to end-of-text.
/// * a **truncated JSON array** — if the array itself was cut mid-entry,
///   recover the longest complete prefix (`[ {…}, {…}, …, {…} ]`) so
///   we surface the entries the model managed to finish.
///
/// Returns the parsed `Value` (an array) or `None`.
pub(crate) fn extract_citations_block(text: &str) -> Option<Value> {
    let lower = text.to_lowercase();
    let open = lower.rfind("<citations>")?;
    let after_open = open + "<citations>".len();
    let inner_raw = if let Some(close_rel) = lower[after_open..].find("</citations>") {
        text[after_open..after_open + close_rel].trim()
    } else {
        // No closing tag — model output was truncated before it
        // finished. Take everything that came through.
        text[after_open..].trim()
    };
    // Strip optional Markdown fences like ```json … ```
    let inner = inner_raw
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim();
    let inner = inner.trim_end_matches("```").trim();
    if let Ok(v) = serde_json::from_str::<Value>(inner) {
        return Some(v);
    }
    // Clean parse failed — most often an over-escaped apostrophe (`\'`).
    // Retry once with the escapes repaired.
    let repaired = repair_json_escapes(inner);
    if repaired != inner {
        if let Ok(v) = serde_json::from_str::<Value>(&repaired) {
            tracing::info!(
                "[chat] <CITATIONS> block parsed after repairing invalid JSON escapes"
            );
            return Some(v);
        }
    }
    // Last resort: truncation recovery. Walk the (repaired) prefix as
    // a JSON array, keep every complete top-level entry, drop the
    // partial one at the tail, close with `]`.
    if let Some(recovered) = recover_truncated_citations_array(&repaired) {
        let n = recovered.as_array().map(|a| a.len()).unwrap_or(0);
        tracing::warn!(
            "[chat] <CITATIONS> block was truncated — recovered first {n} entries from the JSON prefix"
        );
        return Some(recovered);
    }
    // DIAGNOSTIC (v0.5.2+): we found a <CITATIONS> block but every
    // parse path failed. Dump the first / middle / last 300 chars of
    // the offending payload so we can see what the model actually
    // emitted (and whether the splitter, a stray escape, a mid-array
    // truncation, or some other shape is at fault). Truncating to
    // 300/segment keeps the log line readable on a 50k-char response.
    let inner_chars = inner.chars().count();
    let head = inner.chars().take(300).collect::<String>();
    let tail_skip = inner_chars.saturating_sub(300);
    let tail = inner.chars().skip(tail_skip).collect::<String>();
    let mid_start = inner_chars / 2;
    let mid = inner
        .chars()
        .skip(mid_start.saturating_sub(150))
        .take(300)
        .collect::<String>();
    tracing::warn!(
        "[chat] <CITATIONS> block found but is not valid JSON — citations dropped \
         (inner_chars={inner_chars})\nHEAD:\n{head}\nMID:\n{mid}\nTAIL:\n{tail}"
    );
    None
}

/// Recover the longest valid `[…]` prefix from a truncated citations
/// JSON array. The input is the body that should have been a full
/// array; we walk it character-by-character respecting string scope
/// (so a quote-contained `}` doesn't fool us) and remember the offset
/// of the most recent `}` that closed a top-level array entry. Cutting
/// there and appending `]` gives a syntactically valid prefix.
///
/// Returns `None` when the prefix doesn't even start with `[` or no
/// complete entry was emitted.
fn recover_truncated_citations_array(inner: &str) -> Option<Value> {
    let s = inner.trim();
    if !s.starts_with('[') {
        return None;
    }
    let bytes = s.as_bytes();
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut esc = false;
    let mut last_top_level_entry_end: Option<usize> = None;
    for (i, &b) in bytes.iter().enumerate() {
        let c = b as char;
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' | '[' => depth += 1,
            '}' | ']' => {
                if c == '}' && depth == 2 {
                    // Closes an entry inside the outer array.
                    last_top_level_entry_end = Some(i);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    let cut = last_top_level_entry_end?;
    let recovered = format!("{}]", &s[..=cut]);
    serde_json::from_str::<Value>(&recovered).ok()
}

/// Result of processing one attached document.
pub struct DocPayload {
    pub filename: String,
    /// Extracted plain text (None when only images are usable, e.g. scanned PDF).
    pub text: Option<String>,
    /// `data:image/png;base64,...` URLs for vision-capable models.
    pub images: Vec<String>,
    /// Set when `text` holds only excerpts because the whole document did
    /// not fit the model's context window.
    pub excerpt: Option<attachment_budget::ExcerptInfo>,
}

const MAX_PDF_IMAGE_PAGES: usize = 8;
const PDF_RENDER_DPI: f32 = 200.0;

#[cfg(feature = "pdf")]
fn pages_to_data_urls(pngs: Vec<Vec<u8>>) -> Vec<String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    pngs.into_iter()
        .map(|bytes| format!("data:image/png;base64,{}", STANDARD.encode(&bytes)))
        .collect()
}

/// Read attached documents from storage and extract their text and/or images.
/// `vision_ok` lets scanned PDFs fall back to rendered page images.
/// Emit a `doc_extract_*` SSE event so the chat UI can render a
/// "Estraendo testo da X" step before the PII pass kicks in. Without
/// it the user sees a multi-second block on long PDFs with no
/// feedback. `start = true` for the leading event, `false` for the
/// terminal one (carries `chars` so the UI can report doc size).
fn emit_doc_extract(
    tx: &tokio::sync::mpsc::Sender<Result<axum::response::sse::Event, std::convert::Infallible>>,
    filename: &str,
    chars: Option<usize>,
    done: bool,
) {
    use axum::response::sse::Event;
    let payload = if done {
        serde_json::json!({
            "type": "doc_extract_done",
            "filename": filename,
            "chars": chars.unwrap_or(0),
        })
    } else {
        serde_json::json!({
            "type": "doc_extract_start",
            "filename": filename,
        })
    };
    // try_send: a stuck client mustn't block the loader. Dropping a
    // progress tick is acceptable; the chat continues regardless.
    let _ = tx.try_send(Ok(Event::default().data(payload.to_string())));
}

async fn load_attached_docs(
    state: &AppState,
    user_id: &str,
    document_ids: &[String],
    vision_ok: bool,
    pii_protected_ids: &std::collections::HashSet<String>,
    sse_tx: &tokio::sync::mpsc::Sender<Result<axum::response::sse::Event, std::convert::Infallible>>,
) -> Vec<DocPayload> {
    let mut out = Vec::new();
    for doc_id in document_ids {
        let row: Option<(String, String, Option<String>, Option<String>, i64, String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT filename, file_type, storage_path, extracted_text_path, pii_protected, \
                    decision, decision_reason, decision_summary \
             FROM documents WHERE id = ? AND user_id = ?",
        )
        .bind(doc_id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();

        let Some((
            filename,
            file_type,
            Some(storage_path),
            extracted_text_path,
            persisted_pii,
            decision,
            decision_reason,
            decision_summary,
        )) = row
        else {
            continue;
        };

        // Rejected docs (per-chat decision, migration 0029) never load
        // their full text into the prompt. Instead we synthesise a
        // short note from the user-provided reason and the one-shot
        // LLM summary captured at reject-time, so the model on a
        // subsequent turn knows what was rejected and why — without
        // re-seeing the bytes the user already vetoed.
        if decision == "rejected" {
            let reason = decision_reason.as_deref().unwrap_or("(motivo non registrato)");
            let summary = decision_summary
                .as_deref()
                .unwrap_or("(riassunto non disponibile)");
            let stub = format!(
                "[Documento rifiutato dall'utente]\n\
                 Filename: {filename}\n\
                 Motivo del rifiuto: {reason}\n\
                 Riassunto della versione rifiutata: {summary}\n\
                 Indicazione operativa: non riprodurre questa versione \
                 così com'è; correggi tenendo conto del motivo sopra."
            );
            tracing::info!(
                "[chat] doc {filename} (id={doc_id}) rejected — substituting full \
                 text with reason+summary stub ({} chars)",
                stub.len()
            );
            out.push(DocPayload {
                filename: filename.clone(),
                text: Some(stub),
                images: Vec::new(),
                excerpt: None,
            });
            continue;
        }

        // Effective protection = persisted column (set by an earlier
        // opt-in turn) OR the per-request set (set by this turn). The
        // OR-logic guarantees a follow-up text-only turn still
        // redacts a document the user opted-in to earlier, even if
        // the current payload doesn't repeat the flag.
        let pii_on = persisted_pii != 0 || pii_protected_ids.contains(doc_id);

        // Emit the leading step event so the UI shows "Estraendo
        // testo — file" the moment we start touching the file —
        // before any storage read, before PII. Closes via
        // `doc_extract_done` either in the cached fast path or
        // after the per-format extractor runs.
        emit_doc_extract(sse_tx, &filename, None, false);

        let storage = match make_storage() {
            Ok(s) => s,
            Err(_) => continue,
        };

        // Cache fast path: if the upload pipeline already extracted
        // plain text to data/storage/cache/<hash>.txt, prefer it.
        //  - Text-bearing formats (docx, rtf, xlsx, txt/md/csv): use
        //    the cache directly and skip the per-format dispatch and
        //    even the binary read.
        //  - PDFs: use the cache if non-empty (native PDFs); fall
        //    through if empty (scanned PDFs needing vision rendering).
        //  - Images: never use the cache — they need the binary
        //    base64-encoded for the model.
        let is_image_format = matches!(
            file_type.as_str(),
            "png" | "jpeg" | "jpg" | "tiff" | "tif"
        );
        let mut cached_text: Option<String> = None;
        if !is_image_format {
            if let Some(txt_key) = extracted_text_path.as_ref() {
                if let Ok(txt_bytes) = storage.get(txt_key).await {
                    let text = String::from_utf8_lossy(&txt_bytes).into_owned();
                    if !text.is_empty() {
                        cached_text = Some(text);
                    }
                }
            }
        }
        if let Some(text) = cached_text.take() {
            if file_type != "pdf" || !text.trim().is_empty() {
                let chars = text.len();
                tracing::info!(
                    "[chat] using cached text for {filename}: {} chars",
                    chars
                );
                emit_doc_extract(sse_tx, &filename, Some(chars), true);
                let final_text = maybe_redact_pii(
                    text,
                    pii_on,
                    doc_id,
                    &filename,
                    sse_tx,
                )
                .await;
                out.push(DocPayload {
                    filename: filename.clone(),
                    text: Some(final_text),
                    images: Vec::new(),
                    excerpt: None,
                });
                continue;
            }
        }

        let bytes = match storage.get(&storage_path).await {
            Ok(b) => b,
            Err(_) => continue,
        };

        let mut payload = DocPayload {
            filename: filename.clone(),
            text: None,
            images: Vec::new(),
            excerpt: None,
        };

        match file_type.as_str() {
            "docx" => {
                payload.text = crate::pdf::extract_docx_text(&bytes).ok();
            }
            "rtf" => {
                let raw = String::from_utf8_lossy(&bytes);
                payload.text = rtf_parser::RtfDocument::try_from(raw.as_ref())
                    .map(|d| d.get_text())
                    .ok();
            }
            "xlsx" | "xls" | "xlsb" | "ods" => {
                payload.text = crate::pdf::extract_xlsx_text(&bytes).ok();
            }
            "txt" | "md" | "csv" => {
                payload.text = Some(String::from_utf8_lossy(&bytes).to_string());
            }
            "png" => {
                if vision_ok {
                    use base64::{engine::general_purpose::STANDARD, Engine as _};
                    payload.images.push(format!(
                        "data:image/png;base64,{}",
                        STANDARD.encode(&bytes)
                    ));
                } else {
                    tracing::warn!(
                        "[chat] {filename}: PNG attached but selected model is not vision-capable"
                    );
                }
            }
            "jpeg" | "jpg" => {
                if vision_ok {
                    use base64::{engine::general_purpose::STANDARD, Engine as _};
                    payload.images.push(format!(
                        "data:image/jpeg;base64,{}",
                        STANDARD.encode(&bytes)
                    ));
                } else {
                    tracing::warn!(
                        "[chat] {filename}: JPEG attached but selected model is not vision-capable"
                    );
                }
            }
            "tiff" | "tif" => {
                if vision_ok {
                    match crate::pdf::convert_tiff_to_jpegs(&bytes) {
                        Ok(jpegs) => {
                            tracing::info!(
                                "[chat] {filename}: TIFF converted to {} JPEG frame(s)",
                                jpegs.len()
                            );
                            use base64::{engine::general_purpose::STANDARD, Engine as _};
                            for j in jpegs {
                                payload.images.push(format!(
                                    "data:image/jpeg;base64,{}",
                                    STANDARD.encode(&j)
                                ));
                            }
                        }
                        Err(e) => {
                            tracing::warn!("[chat] {filename}: TIFF conversion failed: {e}");
                        }
                    }
                } else {
                    tracing::warn!(
                        "[chat] {filename}: TIFF attached but selected model is not vision-capable"
                    );
                }
            }
            "pdf" => {
                #[cfg(feature = "pdf")]
                {
                    let tmp = std::env::temp_dir().join(crate::product::temp_file_name(&format!("{doc_id}.pdf")));
                    if std::fs::write(&tmp, &bytes).is_ok() {
                        let pages = crate::pdf::extract_text(&tmp).ok();
                        if let Some(pages) = pages {
                            let scanned = crate::pdf::is_scanned_pdf(&pages);
                            let mut full_text = String::new();
                            for p in &pages {
                                full_text.push_str(&format!("[Page {}]\n{}\n", p.page, p.text));
                            }
                            if !scanned {
                                payload.text = Some(full_text);
                            } else if vision_ok {
                                tracing::info!(
                                    "[chat] {filename}: scanned PDF detected, rendering up to {MAX_PDF_IMAGE_PAGES} pages at {PDF_RENDER_DPI} DPI"
                                );
                                match crate::pdf::render_pdf_pages(
                                    &tmp,
                                    PDF_RENDER_DPI,
                                    MAX_PDF_IMAGE_PAGES,
                                ) {
                                    Ok(pngs) => {
                                        payload.images = pages_to_data_urls(pngs);
                                    }
                                    Err(e) => {
                                        tracing::warn!("[chat] render PDF pages failed: {e}");
                                    }
                                }
                            } else {
                                tracing::warn!(
                                    "[chat] {filename}: scanned PDF but the selected model is not vision-capable; sending what little text was extracted"
                                );
                                payload.text = Some(full_text);
                            }
                        }
                        let _ = std::fs::remove_file(&tmp);
                    }
                }
                #[cfg(not(feature = "pdf"))]
                {
                    tracing::warn!("[chat] PDF document {doc_id} skipped: pdf feature not enabled");
                }
            }
            _ => {
                tracing::warn!("[chat] unsupported file_type={file_type} for {filename}");
            }
        }

        // Close the doc_extract step with the raw character count
        // BEFORE we apply PII redaction. The UI then transitions
        // into the pii_redact step (if protected) — two distinct
        // visual phases instead of one undifferentiated wait.
        let pre_redact_chars =
            payload.text.as_deref().map(|t| t.len()).unwrap_or(0);
        emit_doc_extract(sse_tx, &filename, Some(pre_redact_chars), true);

        // Apply PII redaction before the chars count log so the
        // log line reflects what actually goes to the LLM.
        if let Some(t) = payload.text.take() {
            payload.text = Some(
                maybe_redact_pii(
                    t,
                    pii_on,
                    doc_id,
                    &filename,
                    sse_tx,
                )
                .await,
            );
        }
        let chars = payload.text.as_deref().map(|t| t.len()).unwrap_or(0);
        tracing::info!(
            "[chat] loaded doc {filename}: text={} chars, images={}",
            chars,
            payload.images.len()
        );
        out.push(payload);
    }
    out
}

/// If `protected` is true AND the `ner-pii` feature is built in,
/// run `text` through GLiNER2 and return the redacted copy.
/// Emits `pii_redact_*` SSE events on `sse_tx` so the chat UI can
/// render a tool step with `n / N` chunk progress while a long
/// document is being processed (the engine is single-threaded per
/// session and a 100-page PDF can take 10-30 s).
/// On failure (model load, inference) the original text flows
/// through and the failure is logged — the user already saw the
/// blackbox disclaimer; breaking the chat over an inference glitch
/// would be worse.
/// Storage key for the cached PII-anonymised text of a document.
///
/// PII inference is the most expensive single step in `load_attached_docs`
/// (22-35 s per 2000-char chunk on CPU; a 9-page PDF is ~3-5 min). Rerunning
/// it on every chat turn for the same document is wasteful — once a
/// `[LABEL]`-redacted copy exists, the same masking is byte-for-byte stable
/// (the labels and threshold are pinned in `crate::ner::default_pii_labels`
/// / `crate::ner::PII_THRESHOLD`). We persist the redacted text alongside
/// the raw extracted text and consult it before any inference. The key
/// space is flat under `cache/pii/` so a future garbage-collection sweep
/// over deleted documents is a single prefix walk.
fn pii_cache_key(doc_id: &str) -> String {
    format!("cache/pii/{doc_id}.txt")
}

async fn maybe_redact_pii(
    text: String,
    protected: bool,
    doc_id: &str,
    filename: &str,
    #[allow(unused_variables)] sse_tx: &tokio::sync::mpsc::Sender<
        Result<axum::response::sse::Event, std::convert::Infallible>,
    >,
) -> String {
    tracing::info!(
        "[chat] maybe_redact_pii({filename}, doc_id={doc_id}) — protected={} ner-pii-built-in={}",
        protected,
        cfg!(feature = "ner-pii")
    );
    if !protected {
        return text;
    }

    // Cache fast-path. The redacted output is deterministic for a given
    // input + fixed labels + fixed threshold, so a previous turn's
    // result is reusable. We still emit the start/done SSE pair so the
    // UI stays consistent (and the user sees the "cache hit" is fast).
    let cache_key = pii_cache_key(doc_id);
    if let Ok(storage) = crate::storage::make_storage() {
        if let Ok(bytes) = storage.get(&cache_key).await {
            let cached = String::from_utf8_lossy(&bytes).into_owned();
            if !cached.is_empty() {
                tracing::info!(
                    "[chat] PII cache hit for {filename} (doc_id={doc_id}): {} chars",
                    cached.len()
                );
                use axum::response::sse::Event;
                // Two-event burst: start (with total=1 so the UI doesn't
                // try to render N/N progress) immediately followed by done.
                // try_send because the events are best-effort; if the
                // client already disconnected we shouldn't pay anything.
                let _ = sse_tx.try_send(Ok(Event::default().data(
                    json!({
                        "type": "pii_redact_start",
                        "filename": filename,
                        "total": 1,
                    })
                    .to_string(),
                )));
                let _ = sse_tx.try_send(Ok(Event::default().data(
                    json!({
                        "type": "pii_redact_done",
                        "filename": filename,
                    })
                    .to_string(),
                )));
                return cached;
            }
        }
    }

    #[cfg(feature = "ner-pii")]
    {
        use axum::response::sse::Event;
        // Bridge channel: spawn_blocking worker → tokio task that
        // forwards each (current, total) tick as a pii_redact_progress
        // SSE event. Bounded(16) so a stuck client never bloats memory.
        let (prog_tx, mut prog_rx) =
            tokio::sync::mpsc::channel::<(usize, usize)>(16);
        let filename_owned = filename.to_string();
        let sse_tx_clone = sse_tx.clone();
        let forwarder = tokio::spawn(async move {
            // First tick that arrives carries `total`; we emit the
            // `start` event from it (we don't know the total until
            // chunks are computed inside the blocking pass).
            let mut sent_start = false;
            while let Some((current, total)) = prog_rx.recv().await {
                if !sent_start {
                    let _ = sse_tx_clone
                        .send(Ok(Event::default().data(
                            json!({
                                "type": "pii_redact_start",
                                "filename": filename_owned,
                                "total": total,
                            })
                            .to_string(),
                        )))
                        .await;
                    sent_start = true;
                }
                let _ = sse_tx_clone
                    .send(Ok(Event::default().data(
                        json!({
                            "type": "pii_redact_progress",
                            "filename": filename_owned,
                            "current": current,
                            "total": total,
                        })
                        .to_string(),
                    )))
                    .await;
            }
            // Final done event whatever the outcome — even if no
            // progress arrived (e.g. spawn_blocking failed before
            // the first chunk), so the UI doesn't leave a dangling
            // "loading" step.
            let _ = sse_tx_clone
                .send(Ok(Event::default().data(
                    json!({
                        "type": "pii_redact_done",
                        "filename": filename_owned,
                    })
                    .to_string(),
                )))
                .await;
        });

        let progress_cb: crate::ner::ProgressFn = std::sync::Arc::new(
            move |current: usize, total: usize| {
                // try_send so a slow consumer never blocks the
                // blocking worker; dropped progress ticks aren't
                // critical — the UI smooths over them.
                let _ = prog_tx.try_send((current, total));
            },
        );
        let result = crate::ner::mask_pii(&text, None, Some(progress_cb)).await;
        // Drop the closure (and with it `prog_tx`) so the forwarder
        // sees EOF and emits the `done` event.
        let _ = forwarder.await;

        match result {
            Ok(masked) => {
                tracing::info!(
                    "[chat] PII redaction applied to {filename}: {} → {} chars",
                    text.len(),
                    masked.len()
                );
                // Persist for the next chat turn that references the
                // same document. Failure here is non-fatal — the user
                // still gets a correctly anonymised payload this turn;
                // only the *next* turn pays the inference cost again.
                if let Ok(storage) = crate::storage::make_storage() {
                    if let Err(e) = storage
                        .put(&cache_key, masked.as_bytes(), "text/plain")
                        .await
                    {
                        tracing::warn!(
                            "[chat] failed to write PII cache for {filename} ({cache_key}): {e:#}"
                        );
                    } else {
                        tracing::info!(
                            "[chat] PII cache stored for {filename} → {cache_key}"
                        );
                    }
                }
                return masked;
            }
            Err(e) => {
                tracing::warn!(
                    "[chat] PII redaction failed for {filename}: {e:#} — \
                     sending ORIGINAL text to LLM"
                );
            }
        }
    }
    #[cfg(not(feature = "ner-pii"))]
    {
        tracing::warn!(
            "[chat] {filename} flagged for PII protection but `ner-pii` feature \
             is not compiled — sending ORIGINAL text to LLM"
        );
    }
    text
}

/// One retrieved KB chunk plus the citation tag it was rendered with so
/// the response post-processor can map the model's `[g1]`/`[p1]` text
/// references back to the source path + chunk index.
#[derive(Debug, Clone)]
pub struct RetrievedKbEntry {
    /// Tag used in the system prompt: "g1", "g2", "p1", ... — used by
    /// the citation parser to look the entry up.
    pub tag: String,
    /// "global" | "project". Surfaced in the prompt and copied into
    /// the citation JSON.
    pub scope_label: &'static str,
    pub source_path: String,
    pub document_id: String,
    pub chunk_index: i32,
    pub text: String,
    /// 1-based page number authoritative from the chunker (PDFs only).
    /// `None` for non-PDF formats. Forwarded into the citation JSON so
    /// the DocPanel can scroll directly to the right page instead of
    /// falling back to text-search.
    pub page: Option<i64>,
}

/// Maximum cosine distance accepted for a chunk to be included. Values
/// above this threshold are noise rather than relevant context — but
/// 0.6 was too aggressive for cross-lingual queries (e.g. asking in
/// English about an Italian-language GDPR), where multilingual-e5
/// similarities cluster ~0.05-0.10 lower than monolingual. With an
/// English question against an Italian corpus doc we observed valid
/// matches falling around 0.62-0.68 and getting culled, leading to
/// "no relevant passages found" answers despite the doc being
/// retrievable in principle. 0.75 still excludes cosine-distant
/// noise while admitting cross-lingual paraphrases.
#[cfg(feature = "rag")]
const KB_DISTANCE_THRESHOLD: f32 = 0.75;

/// Reciprocal Rank Fusion (Cormack et al. 2009). Merge two ranked
/// chunk lists by `sum(1 / (k + rank))`, deduping on
/// `(document_id, chunk_index)`. Returns a single list ordered by
/// fused score, truncated to `target`. Each survivor keeps the
/// *minimum* distance across the two source lists so the downstream
/// `KB_DISTANCE_THRESHOLD` filter still has a meaningful value
/// (semantically: "this chunk was a good hit on at least one ranker").
#[cfg(feature = "rag")]
fn reciprocal_rank_fuse(
    primary: Vec<crate::embeddings::service::RetrievedChunk>,
    secondary: Vec<crate::embeddings::service::RetrievedChunk>,
    target: usize,
) -> Vec<crate::embeddings::service::RetrievedChunk> {
    use crate::embeddings::service::RetrievedChunk;
    const RRF_K: f32 = 60.0;

    // Insertion-ordered: keep the first occurrence's RetrievedChunk
    // but track minimum distance across both rankings.
    let mut by_key: std::collections::HashMap<(String, i32), (RetrievedChunk, f32)> =
        std::collections::HashMap::new();
    let mut score: std::collections::HashMap<(String, i32), f32> =
        std::collections::HashMap::new();

    for (rank, c) in primary.into_iter().enumerate() {
        let key = (c.document_id.clone(), c.chunk_index);
        let s = 1.0 / (RRF_K + (rank as f32) + 1.0);
        *score.entry(key.clone()).or_insert(0.0) += s;
        let entry = by_key
            .entry(key)
            .or_insert((c.clone(), c.distance));
        if c.distance < entry.1 {
            entry.1 = c.distance;
        }
    }
    for (rank, c) in secondary.into_iter().enumerate() {
        let key = (c.document_id.clone(), c.chunk_index);
        let s = 1.0 / (RRF_K + (rank as f32) + 1.0);
        *score.entry(key.clone()).or_insert(0.0) += s;
        let entry = by_key
            .entry(key)
            .or_insert((c.clone(), c.distance));
        if c.distance < entry.1 {
            entry.1 = c.distance;
        }
    }

    let mut merged: Vec<(f32, RetrievedChunk)> = by_key
        .into_iter()
        .map(|(key, (mut chunk, min_dist))| {
            // Reuse the min distance so the downstream threshold sees
            // the better of the two scorers' confidence.
            chunk.distance = min_dist;
            let s = *score.get(&key).unwrap_or(&0.0);
            (s, chunk)
        })
        .collect();
    merged.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    merged.into_iter().take(target).map(|(_, c)| c).collect()
}

/// Run vector retrieval against the user's library and return the
/// chunks ready to be rendered into the system prompt. The scope is
/// inferred from the chat's project_id + the project's isolation_mode.
/// Returns an empty vec when:
///  - the rag feature isn't compiled in
///  - the embedding service isn't initialised
///  - the user has no indexed documents in the relevant pool
///  - all retrieved chunks are above the distance threshold
///
/// When `user_settings.hyde_enabled = 1` (migration 0030), the
/// function also fires a one-shot LLM call to draft a domain-aware
/// hypothesis (see `crate::llm::hyde`) and runs a second KNN against
/// that hypothesis; the two result sets are fused via Reciprocal Rank
/// Fusion before the distance threshold + PII filter are applied.
#[cfg(feature = "rag")]
async fn retrieve_kb_chunks(
    state: &AppState,
    user_id: &str,
    chat_id: &str,
    user_query: &str,
    top_k_target: usize,
) -> Vec<RetrievedKbEntry> {
    let Some(svc) = state.embeddings.as_ref() else {
        return Vec::new();
    };
    if user_query.trim().is_empty() {
        return Vec::new();
    }

    // Resolve scope: chat → project_id → isolation_mode.
    let project_row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT project_id FROM chats WHERE id = ?",
    )
    .bind(chat_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    let project_id: Option<String> = project_row.and_then(|(p,)| p);

    // Pull the three knobs we need from user_settings in one round-trip:
    // - hyde_enabled  → whether to fire the HyDE expansion pass
    // - locale        → drives the domain prologue language
    // - default_domain → drives which domain.md preset is loaded
    let prefs: Option<(i64, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT hyde_enabled, locale, default_domain FROM user_settings WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    let (hyde_enabled, user_locale, user_default_domain) = prefs
        .map(|(h, l, d)| (h != 0, l, d))
        .unwrap_or((false, None, None));
    tracing::info!(
        "[rag][cite-diag] retrieve_kb_chunks user={user_id}: \
         hyde={}, locale={:?}, domain={:?}, top_k_target={top_k_target} — \
         BASE cosine retrieval ALWAYS runs (whether HyDE is on or off); HyDE only adds a SECOND pass",
        if hyde_enabled { "ON" } else { "OFF" },
        user_locale.as_deref().unwrap_or(""),
        user_default_domain.as_deref().unwrap_or(""),
    );

    use crate::embeddings::service::SearchScope;
    // Resolve scope once. `is_strict_project` is needed because the
    // borrow rules around the SearchScope<'_> lifetime make it
    // simpler to keep the variant inline than capture in a closure.
    let is_strict_project = match project_id.as_deref() {
        None => false,
        Some(pid) => {
            let mode: Option<(String,)> = sqlx::query_as(
                "SELECT isolation_mode FROM projects WHERE id = ?",
            )
            .bind(pid)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
            mode.as_ref().map(|(m,)| m.as_str()) == Some("strict")
        }
    };

    // Build the scope inline at each call site. A closure would be
    // tidier but Rust's lifetime inference for `impl Fn() ->
    // SearchScope<'_>` is too narrow here (the borrowed `pid` outlives
    // each individual call but the closure signature flattens that).
    let scope_for_primary = match project_id.as_deref() {
        None => SearchScope::Global,
        Some(p) if is_strict_project => SearchScope::ProjectStrict(p),
        Some(p) => SearchScope::ProjectShared(p),
    };

    // Primary pass — vanilla embedding of the user's query.
    let primary_result = svc
        .search(user_id, scope_for_primary, user_query, top_k_target)
        .await;

    // Optional HyDE pass — draft a domain-aware hypothesis, embed it,
    // run a second KNN. Errors here are non-fatal: log and degrade to
    // the primary-only ranking. We deliberately don't gate the primary
    // pass on HyDE so a flaky LLM call never hides relevant chunks.
    let hyde_result = if hyde_enabled {
        let locale = user_locale.as_deref().unwrap_or("it");
        // Project domain wins over user default, mirroring how the
        // domain prologue is composed in stream_chat. We re-resolve
        // here rather than threading a parameter to avoid changing the
        // function signature (the call site already passes only the
        // raw text).
        let project_domain: Option<String> = if let Some(pid) = project_id.as_deref() {
            let r: Option<(Option<String>,)> = sqlx::query_as(
                "SELECT domain FROM projects WHERE id = ?",
            )
            .bind(pid)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
            r.and_then(|(d,)| d)
        } else {
            None
        };
        let domain = project_domain
            .or(user_default_domain.clone())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "others".to_string());

        // Reuse the chat-side credentials path so HyDE rides whatever
        // model + provider the user has currently configured.
        let user_settings = crate::routes::user::fetch_llm_settings(&state.db, user_id)
            .await
            .ok();
        let raw_model = user_settings
            .as_ref()
            .and_then(|s| s.main_model.clone())
            .unwrap_or_else(|| "gemini-2.5-flash".to_string());
        let local_config = build_local_config(&raw_model, user_settings.as_ref());
        let creds = crate::llm::hyde::HydeCreds {
            local_config,
            claude_api_key: user_settings.as_ref().and_then(|s| s.claude_api_key.clone()),
            gemini_api_key: user_settings.as_ref().and_then(|s| s.gemini_api_key.clone()),
            gemini_region: user_settings.as_ref().and_then(|s| s.gemini_region.clone()),
        };

        match crate::llm::hyde::generate_hypothesis(
            user_query,
            locale,
            &domain,
            &raw_model,
            &creds,
        )
        .await
        {
            Ok(hypothesis) if !hypothesis.trim().is_empty() => {
                tracing::info!(
                    "[rag] HyDE on user={} domain={} locale={} → hypothesis {} chars",
                    user_id,
                    domain,
                    locale,
                    hypothesis.chars().count(),
                );
                let scope_for_hyde = match project_id.as_deref() {
                    None => SearchScope::Global,
                    Some(p) if is_strict_project => SearchScope::ProjectStrict(p),
                    Some(p) => SearchScope::ProjectShared(p),
                };
                Some(
                    svc.search(user_id, scope_for_hyde, &hypothesis, top_k_target)
                        .await,
                )
            }
            Ok(_) => {
                tracing::warn!(
                    "[rag] HyDE returned empty hypothesis user={} — falling back to primary-only",
                    user_id,
                );
                None
            }
            Err(e) => {
                tracing::warn!(
                    "[rag] HyDE call failed user={}: {e:#} — falling back to primary-only",
                    user_id,
                );
                None
            }
        }
    } else {
        None
    };

    // Fuse the rankings. Without HyDE: just the primary list. With
    // HyDE: Reciprocal Rank Fusion with k=60 (the conventional value
    // from Cormack et al. 2009), deduplicated by `(document_id,
    // chunk_index)`. The fused score is `sum(1 / (k + rank))` across
    // both lists; we then re-sort by score descending and synthesise
    // a `distance` value as the minimum across the two lists so the
    // downstream KB_DISTANCE_THRESHOLD filter still meaningfully
    // applies (any chunk that survived a vanilla KNN's threshold once
    // is kept).
    let chunks = match (primary_result, hyde_result) {
        (Ok(primary), None) => primary,
        (Err(e), None) => {
            tracing::warn!("[rag] retrieval failed: {e}");
            return Vec::new();
        }
        (Ok(primary), Some(Ok(hyde_chunks))) => {
            tracing::info!(
                "[rag] RRF merge: primary={} hyde={} chunks before fusion",
                primary.len(),
                hyde_chunks.len(),
            );
            reciprocal_rank_fuse(primary, hyde_chunks, top_k_target)
        }
        (Ok(primary), Some(Err(e))) => {
            tracing::warn!("[rag] HyDE KNN failed: {e} — using primary only");
            primary
        }
        (Err(e), Some(Ok(hyde_chunks))) => {
            tracing::warn!("[rag] primary KNN failed: {e} — using HyDE only");
            hyde_chunks
        }
        (Err(ep), Some(Err(eh))) => {
            tracing::warn!("[rag] both retrievals failed: primary={ep} hyde={eh}");
            return Vec::new();
        }
    };

    // Filter by distance + label per-chunk based on whether the row had
    // project_id NULL (global) or a value (project). We can't know the
    // raw project_id from the public RetrievedChunk; instead, we look
    // it up in synced_files via the document_id — cheap and accurate.
    // We ALSO drop any chunk whose source document is flagged
    // `pii_protected = 1` (migration 0028). KB chunks come from the
    // raw indexed text, not the redacted PII cache — leaving them in
    // would re-expose the very entities the inline-attached path is
    // careful to mask. The user still gets the document content via
    // the parallel `load_attached_docs` path, which serves the
    // anonymised cache from `cache/pii/<doc_id>.txt`. Citations
    // referencing those passages move with the doc, so the answer
    // can still ground itself on the redacted version.
    let mut out: Vec<RetrievedKbEntry> = Vec::new();
    let mut g_idx = 0u32;
    let mut p_idx = 0u32;
    let mut orphan_count = 0u32;
    for c in chunks.into_iter().filter(|c| c.distance <= KB_DISTANCE_THRESHOLD) {
        // Orphan-source filter (v0.5.4+). When the user removes a file
        // from a synced folder (or from the UI's local-documents
        // settings panel) the on-disk file disappears but the
        // `doc_chunks` embeddings + `documents` row sometimes
        // outlive it — the file watcher / soft-delete path missed
        // it. Surfacing orphan chunks into the system prompt then
        // makes the model hallucinate that document as a source on
        // every turn, leading to "12 citations all pointing to the
        // same 404 page" syndrome the user reported in the medical-
        // legal chat. We probe the source_path here; URL-shaped
        // source_paths (EUR-Lex / DILA / italian-legal) are remapped
        // to their local cache equivalent further down anyway, so
        // we use the same map for the existence check.
        let probe_path: String = if c.source_path.starts_with("http://")
            || c.source_path.starts_with("https://")
        {
            // Will be remapped below; trust for now.
            c.source_path.clone()
        } else {
            c.source_path.clone()
        };
        let is_url = probe_path.starts_with("http://") || probe_path.starts_with("https://");
        if !is_url && !std::path::Path::new(&probe_path).exists() {
            orphan_count += 1;
            tracing::warn!(
                "[rag] orphan KB chunk dropped: source_path={:?} missing on disk \
                 (doc_id={}, chunk_index={}) — run /sync/cleanup-orphans to purge",
                probe_path,
                c.document_id,
                c.chunk_index,
            );
            continue;
        }

        // Per-doc PII-protection lookup. Cheap because the chunk batch
        // typically points to at most a handful of distinct documents,
        // and SQLite has a 100 ns hot-cache lookup.
        let prot: Option<(i64,)> = sqlx::query_as(
            "SELECT pii_protected FROM documents WHERE id = ?",
        )
        .bind(&c.document_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
        if prot.map(|(p,)| p != 0).unwrap_or(false) {
            tracing::info!(
                "[rag] dropping KB chunk from PII-protected doc_id={} \
                 (chunk_index={}, source={}) — content available via \
                 redacted cache instead",
                c.document_id,
                c.chunk_index,
                c.source_path,
            );
            continue;
        }

        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT project_id FROM synced_files WHERE document_id = ?",
        )
        .bind(&c.document_id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
        let is_global = row.and_then(|(p,)| p).is_none();
        let (tag, scope_label) = if is_global {
            g_idx += 1;
            (format!("g{g_idx}"), "global")
        } else {
            p_idx += 1;
            (format!("p{p_idx}"), "project")
        };
        out.push(RetrievedKbEntry {
            tag,
            scope_label,
            source_path: c.source_path,
            document_id: c.document_id,
            chunk_index: c.chunk_index,
            text: c.text,
            page: c.page,
        });
    }
    if orphan_count > 0 {
        tracing::warn!(
            "[rag] {orphan_count} orphan KB chunk(s) dropped this turn — \
             these are stale embeddings whose source file is missing on disk. \
             Call POST /sync/cleanup-orphans to purge them permanently."
        );
    }
    out
}

#[cfg(not(feature = "rag"))]
async fn retrieve_kb_chunks(
    _state: &AppState,
    _user_id: &str,
    _chat_id: &str,
    _user_query: &str,
    _top_k_target: usize,
) -> Vec<RetrievedKbEntry> {
    Vec::new()
}

/// Lightweight description of a doc in the user's authoritative-corpus
/// library — enough to render the "you have these documents indexed"
/// section of the system prompt without dragging the full text in.
struct CorpusInventoryEntry {
    corpus_id: String,
    identifier: String,
    title: String,
    language: String,
    status: String,
}

/// Pull the list of corpus-sourced documents the user has indexed.
/// Used to seed the library-inventory section of the system prompt
/// so the model orients itself even when the user's question doesn't
/// trigger a semantic-retrieval hit on those documents.
async fn list_indexed_corpus_docs(
    state: &AppState,
    user_id: &str,
) -> Vec<CorpusInventoryEntry> {
    let rows: Vec<(String, String, String, Option<String>, String)> = sqlx::query_as(
        "SELECT corpus_id, corpus_identifier, filename, corpus_language, status \
         FROM documents \
         WHERE user_id = ? AND corpus_id IS NOT NULL AND corpus_identifier IS NOT NULL \
         ORDER BY created_at DESC \
         LIMIT 50",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    rows.into_iter()
        .map(|(corpus_id, identifier, title, language, status)| CorpusInventoryEntry {
            corpus_id,
            identifier,
            title,
            language: language.unwrap_or_default(),
            status,
        })
        .collect()
}

/// Remove the `[Page N]` markers our PDF scanner prepends to each
/// extracted page when it concatenates them. The model often copies
/// these markers verbatim into citation quotes (because they appear
/// inside the chunk text it was given), but they aren't actually
/// present in the underlying PDF — leaving them in breaks the
/// PDF.js text-search highlight in the DocPanel viewer.
///
/// Strategy: drop standalone `[Page N]` tokens (with surrounding
/// whitespace), then collapse any double-spaces / leading newlines
/// the removal might leave behind. Quotes that don't contain a marker
/// pass through unchanged.
/// Compact a string to its ASCII-alphanumeric, lower-cased projection.
/// Counterpart of `frontend/src/lib/utils/highlight.ts::onlyLetters`
/// (the frontend version additionally NFD-normalises to handle the
/// case where the document on disk and the persisted quote use
/// different accent encodings; server-side both come from the same
/// retrieval pipeline so we skip that step). Used by the citation
/// validator below to decide whether the model's emitted quote is a
/// substring of the chunk text we actually retrieved — if not, the
/// quote is a hallucination and we replace it with the chunk's real
/// opening before persisting the citation.
fn letters_only(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        }
    }
    out
}

fn strip_page_markers(quote: &str) -> String {
    let mut out = String::with_capacity(quote.len());
    let bytes = quote.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        // Detect `[Page <digits>]` at byte i.
        if bytes[i] == b'[' && bytes.get(i..i + 6) == Some(b"[Page ") {
            let num_start = i + 6;
            let mut j = num_start;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > num_start && bytes.get(j) == Some(&b']') {
                // Skip the marker and a single trailing whitespace
                // character (newline or space) if present.
                i = j + 1;
                if i < bytes.len() && (bytes[i] == b'\n' || bytes[i] == b' ') {
                    i += 1;
                }
                continue;
            }
        }
        out.push(quote[i..].chars().next().unwrap());
        i += quote[i..].chars().next().unwrap().len_utf8();
    }
    // Trim and collapse the most common leftover artefact (leading
    // newline that remained when the marker was at the very start).
    out.trim_start().to_string()
}

/// Walk a citations JSON array and rewrite each entry's `quote` field
/// through `strip_page_markers`. Used by the chat-history loader so
/// citations persisted before the strip-on-write fix still render
/// without literal `[Page N]` contamination.
fn sanitise_annotations_quotes(value: Value) -> Value {
    let Value::Array(items) = value else {
        return value;
    };
    let cleaned = items
        .into_iter()
        .map(|item| {
            let Value::Object(mut obj) = item else {
                return item;
            };
            if let Some(q) = obj.get("quote").and_then(|v| v.as_str()) {
                let stripped = strip_page_markers(q);
                if stripped != q {
                    obj.insert("quote".into(), Value::String(stripped));
                }
            }
            Value::Object(obj)
        })
        .collect();
    Value::Array(cleaned)
}

/// Fallback path that synthesises citation entries from the inline
/// `[gN]`/`[pN]` markers in the assistant's response when the model
/// forgot to emit the trailing `<CITATIONS>` JSON block. Each unique
/// tag found in `text` that resolves to a `kb_by_tag` entry produces a
/// `{"doc_id": "<tag>", "quote": "..."}` shape that the downstream
/// resolver then enriches with `source: "kb"`, `path`, `page`, etc.
///
/// Returns `None` when `text` has no resolvable KB markers — caller
/// should treat that as "no citations" and ship an empty array.
fn synthesise_kb_citations_from_markers(
    text: &str,
    kb_by_tag: &HashMap<String, RetrievedKbEntry>,
) -> Option<Value> {
    use std::collections::BTreeSet;
    let re_iter = text.match_indices('[');
    let mut tags = BTreeSet::<String>::new();
    for (i, _) in re_iter {
        // Simple state machine: after `[` we accept `g|p` then digits then `]`.
        let bytes = text.as_bytes();
        if let Some(&b) = bytes.get(i + 1) {
            if b == b'g' || b == b'p' || b == b'G' || b == b'P' {
                let mut j = i + 2;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j > i + 2 && bytes.get(j) == Some(&b']') {
                    let tag = text[i + 1..j].to_ascii_lowercase();
                    if kb_by_tag.contains_key(&tag) {
                        tags.insert(tag);
                    }
                }
            }
        }
    }
    if tags.is_empty() {
        return None;
    }
    let arr: Vec<Value> = tags
        .into_iter()
        .map(|tag| {
            // Use a short prefix of the chunk text as the synthesized
            // quote so the DocPanel still has something to highlight.
            // The resolver further down stamps the authoritative page
            // and source path so the click-to-open path still works.
            let quote = kb_by_tag
                .get(&tag)
                .map(|e| {
                    let t = e.text.trim();
                    let cap = 200.min(t.len());
                    let mut end = cap;
                    while end < t.len() && !t.is_char_boundary(end) {
                        end -= 1;
                    }
                    t[..end].to_string()
                })
                .unwrap_or_default();
            json!({ "doc_id": tag, "quote": quote })
        })
        .collect();
    tracing::info!(
        "[chat] no <CITATIONS> block in response — synthesised {} citation(s) from inline KB markers",
        arr.len()
    );
    Some(Value::Array(arr))
}

/// One inline `[doc-id: <handle>, page <N>]`-style reference found in
/// the assistant's prose. `handle` is either a `doc-N` chat-local label
/// or a 36-char UUID; `page` is the digit (or `N-M` range) the model
/// emitted (may be `None` for a `[doc-id: <handle>]` without page).
#[derive(Debug, Clone, PartialEq, Eq)]
struct InlineDocIdRef {
    start: usize,
    end: usize,
    handle: String,
    page: Option<String>,
}

/// Scan `text` for every `[doc-id: <handle>[, page[s] <N|N-M>]]`
/// occurrence the model emits as a free-form citation when it ignores
/// the `[cN]` + `<CITATIONS>` contract (most common with verbose
/// `generate_docx` follow-up descriptions). Returns the matches in
/// order of appearance — the caller assigns sequential `cN` refs and
/// rewrites the prose. Tolerant of variable whitespace, `page` vs
/// `pages`, capital `Doc-ID:` / `DOC-ID:`, and missing-page form.
fn extract_inline_docid_refs(text: &str) -> Vec<InlineDocIdRef> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let lower = text.to_ascii_lowercase();
    let needle = "[doc-id:";
    let mut search_from = 0;
    while let Some(off) = lower[search_from..].find(needle) {
        let start = search_from + off;
        let after_prefix = start + needle.len();
        let mut i = after_prefix;
        // skip whitespace after the colon
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        // read the handle: UUID hex+dash characters or doc-\d+
        let handle_start = i;
        while i < bytes.len() {
            let c = bytes[i] as char;
            if c.is_ascii_alphanumeric() || c == '-' {
                i += 1;
            } else {
                break;
            }
        }
        let handle = text[handle_start..i].to_string();
        if handle.is_empty() {
            search_from = after_prefix;
            continue;
        }
        // optional `, page[s] N` clause
        let mut page: Option<String> = None;
        let mut j = i;
        while j < bytes.len() && (bytes[j] as char).is_whitespace() {
            j += 1;
        }
        if j < bytes.len() && bytes[j] == b',' {
            j += 1;
            while j < bytes.len() && (bytes[j] as char).is_whitespace() {
                j += 1;
            }
            // accept `page` or `pages`, case-insensitive
            let rest_lower = &lower[j..];
            let page_word = if rest_lower.starts_with("pages") {
                Some("pages")
            } else if rest_lower.starts_with("page") {
                Some("page")
            } else {
                None
            };
            if let Some(w) = page_word {
                j += w.len();
                while j < bytes.len() && (bytes[j] as char).is_whitespace() {
                    j += 1;
                }
                let digits_start = j;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j > digits_start {
                    // optional `-N` range
                    let mut k = j;
                    if k < bytes.len() && bytes[k] == b'-' {
                        k += 1;
                        let r = k;
                        while k < bytes.len() && bytes[k].is_ascii_digit() {
                            k += 1;
                        }
                        if k > r {
                            j = k;
                        }
                    }
                    page = Some(text[digits_start..j].to_string());
                    while j < bytes.len() && (bytes[j] as char).is_whitespace() {
                        j += 1;
                    }
                }
            }
        }
        if j < bytes.len() && bytes[j] == b']' {
            out.push(InlineDocIdRef {
                start,
                end: j + 1,
                handle,
                page,
            });
            search_from = j + 1;
        } else {
            // Not a well-formed marker — keep scanning from after the colon.
            search_from = after_prefix;
        }
    }
    out
}

/// Decompose **hybrid citation brackets** the model occasionally emits
/// when answering with many sources. The frontend marker renderer
/// (`MARKER_GROUP` in `frontend/src/lib/utils/citations.ts`) requires
/// every token inside a `[...]` to match `[gcp]\d+`. The model
/// sometimes concatenates valid refs with filename + page hints and
/// `doc-N` labels into a single bracket — e.g.
/// `[c1, c2, c18, c34, CARTELLA_TEST_002.pdf, p.4, doc-7, doc-8]` —
/// which fails the regex and leaves the whole bracket as plain text,
/// dragging the legitimate `cN` refs down with it.
///
/// This normaliser splits any such bracket into a sequence of clean
/// brackets the rest of the pipeline understands:
///   - `cN` / `gN` / `pN`  → `[cN]` (frontend pill marker)
///   - `doc-N`              → `[doc-id: doc-N]`
///                            (rewrite_inline_docid_citations resolves it)
///   - `FILENAME.ext` + optional `p.N` / `pag N` / `page N`
///                          → `[doc-id: FILENAME.ext, page N]`
///   - anything else        → preserved verbatim, parenthesised
///
/// A bracket whose every token is already a clean `[gcp]\d+` is left
/// untouched (the renderer handles `[c1, c2, c3]` natively — the
/// regex permits comma-separated tokens as long as they all match).
///
/// The function is **idempotent**: clean input passes through unchanged.
/// Model-independent post-processing — runs after the LLM stream
/// completes regardless of provider (Claude / Gemini / OpenAI /
/// local). See `feedback_model_independent_normalization.md` in the
/// project memory.
pub fn split_hybrid_citation_brackets(text: &str) -> std::borrow::Cow<'_, str> {
    // Fast path — no `[` at all means nothing to rewrite. Avoids
    // allocating a String for the common short-answer case.
    if !text.contains('[') {
        return std::borrow::Cow::Borrowed(text);
    }

    // CRITICAL: stop processing at `<CITATIONS>`. The trailing JSON
    // block contains `[…]` arrays whose nested `quote` strings often
    // include square brackets the model copies verbatim from the
    // prose (e.g. `"art. 32 [3] del decreto"`). Without this guard
    // the splitter's first-`]` scan would land inside that quote,
    // truncate the JSON array, and silently kill every citation —
    // exactly the v0.5.1 regression that produced "0 entries" SSE
    // events on otherwise-well-formed model output.
    let prose_end = {
        let lower = text.to_ascii_lowercase();
        lower.find("<citations>").unwrap_or(text.len())
    };
    let (prose, tail) = text.split_at(prose_end);

    enum Tok<'a> {
        /// `c1`, `g12`, `p3` — pill-renderable on the frontend.
        Ref(&'a str),
        /// `doc-7` — needs `[doc-id: doc-7]` shape for the inline rewriter.
        DocLabel(&'a str),
        /// Filename with extension. Page may follow as a separate token.
        Filename(&'a str),
        /// `p.4`, `pag 4`, `page 4`, `pp. 4-6`. Carries the digit substring.
        PageHint(&'a str),
        /// Anything we don't recognise — kept verbatim.
        Unknown(&'a str),
    }

    fn classify(tok: &str) -> Tok<'_> {
        let t = tok.trim();
        if t.is_empty() {
            return Tok::Unknown(tok);
        }
        // Ref shape: single letter g/c/p + 1..3 digits, lowercase.
        let bytes = t.as_bytes();
        if matches!(bytes[0], b'g' | b'c' | b'p' | b'G' | b'C' | b'P')
            && t.len() >= 2
            && t.len() <= 4
            && bytes[1..].iter().all(|b| b.is_ascii_digit())
        {
            return Tok::Ref(t);
        }
        // doc-N shape (the model's alternate handle).
        if let Some(rest) = t.strip_prefix("doc-")
            && !rest.is_empty()
            && rest.bytes().all(|b| b.is_ascii_alphanumeric())
        {
            return Tok::DocLabel(t);
        }
        // PageHint: any of `p.N`, `p N`, `pag.N`, `pag N`, `pagina N`,
        // `page N`, `pages N`, `pp.N-M`.
        let lower = t.to_ascii_lowercase();
        for prefix in &[
            "pp.", "pp ", "pag.", "pagina ", "pag ", "pages ", "page ", "p.", "p ",
        ] {
            if let Some(rest) = lower.strip_prefix(prefix) {
                let rest_trim = rest.trim_start();
                if !rest_trim.is_empty()
                    && rest_trim
                        .chars()
                        .next()
                        .map(|c| c.is_ascii_digit())
                        .unwrap_or(false)
                {
                    return Tok::PageHint(t);
                }
            }
        }
        // Filename: contains `.` and at least one alphanumeric char.
        // Reject pure decimals like `2.5` to avoid catching numeric
        // tokens — require ≥3 chars after the dot OR an extension
        // longer than 1 char.
        if let Some(dot) = t.rfind('.')
            && dot > 0
            && dot + 1 < t.len()
        {
            let ext = &t[dot + 1..];
            if ext.len() >= 2 && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
                return Tok::Filename(t);
            }
        }
        Tok::Unknown(tok)
    }

    fn extract_page_digit(hint: &str) -> Option<String> {
        // Pull the first decimal number out of a PageHint (handles
        // both `p.4` and `pages 12-14` — the rewriter only needs the
        // first page number).
        let mut buf = String::new();
        let mut started = false;
        for c in hint.chars() {
            if c.is_ascii_digit() {
                buf.push(c);
                started = true;
            } else if started && c == '-' {
                buf.push(c);
            } else if started {
                break;
            }
        }
        if buf.is_empty() {
            None
        } else {
            Some(buf)
        }
    }

    let mut out = String::with_capacity(text.len() + text.len() / 8);
    let mut cursor = 0usize;
    while cursor < prose.len() {
        let Some(rel_open) = prose[cursor..].find('[') else {
            out.push_str(&prose[cursor..]);
            break;
        };
        let open = cursor + rel_open;
        let Some(rel_close) = prose[open + 1..].find(']') else {
            // No closing bracket — flush the rest verbatim and stop.
            out.push_str(&prose[cursor..]);
            break;
        };
        let close = open + 1 + rel_close;
        let inner = &prose[open + 1..close];
        out.push_str(&prose[cursor..open]);

        // Idempotency guard: `[doc-id: <handle>[, page N]]` is the
        // already-canonical inline shape that
        // `rewrite_inline_docid_citations` consumes. Splitting it
        // again would re-prefix the filename with another `doc-id:`,
        // producing `[doc-id: doc-id: FILE.pdf, page 3]`. Pass through.
        if inner.trim_start().to_ascii_lowercase().starts_with("doc-id:") {
            out.push('[');
            out.push_str(inner);
            out.push(']');
            cursor = close + 1;
            continue;
        }

        // Tokenise on top-level commas. (Filenames and refs don't
        // contain commas, so a flat split is enough.)
        let raw_tokens: Vec<&str> = inner.split(',').collect();
        let classified: Vec<Tok<'_>> = raw_tokens.iter().map(|t| classify(t)).collect();

        // Detection of "needs splitting": at least one Ref AND at
        // least one non-Ref token. Pure-Ref brackets are passed
        // through (the renderer accepts `[c1, c2, c3]` natively).
        // Unknown tokens DO trigger a split because their presence
        // breaks the frontend regex — `[c1, c2, foo]` matches no
        // pills today, so we must decompose.
        let any_ref = classified
            .iter()
            .any(|t| matches!(t, Tok::Ref(_)));
        let any_non_ref = classified.iter().any(|t| !matches!(t, Tok::Ref(_)));
        // Also split if the bracket contains a DocLabel, Filename or
        // page hint on its own (e.g. `[doc-7, doc-8]` won't render as
        // pills — needs to go through `rewrite_inline_docid_citations`).
        let any_docish = classified.iter().any(|t| {
            matches!(t, Tok::DocLabel(_) | Tok::Filename(_) | Tok::PageHint(_))
        });

        if (any_ref && any_non_ref) || (!any_ref && any_docish) {
            // Decompose. Build the replacement.
            let mut pieces: Vec<String> = Vec::new();
            let mut i = 0;
            let mut last_filename: Option<&str> = None;
            while i < classified.len() {
                match &classified[i] {
                    Tok::Ref(r) => {
                        pieces.push(format!("[{r}]"));
                    }
                    Tok::DocLabel(label) => {
                        pieces.push(format!("[doc-id: {label}]"));
                    }
                    Tok::Filename(name) => {
                        last_filename = Some(name);
                        // Peek next token: if a PageHint, fuse.
                        let next_page = classified
                            .get(i + 1)
                            .and_then(|t| match t {
                                Tok::PageHint(h) => extract_page_digit(h),
                                _ => None,
                            });
                        if let Some(page) = next_page {
                            pieces.push(format!("[doc-id: {name}, page {page}]"));
                            i += 1; // consume the PageHint
                        } else {
                            pieces.push(format!("[doc-id: {name}]"));
                        }
                    }
                    Tok::PageHint(h) => {
                        // Bare page hint — bind to the last filename we
                        // saw inside this bracket, if any.
                        if let (Some(name), Some(page)) =
                            (last_filename, extract_page_digit(h))
                        {
                            pieces.push(format!("[doc-id: {name}, page {page}]"));
                        }
                        // No filename context → drop the bare page; it
                        // can't be turned into a pill without a target.
                    }
                    Tok::Unknown(raw) => {
                        let trimmed = raw.trim();
                        if !trimmed.is_empty() {
                            // Preserve as parenthesised remainder so
                            // the prose still reads.
                            pieces.push(format!("({trimmed})"));
                        }
                    }
                }
                i += 1;
            }
            out.push_str(&pieces.join(" "));
        } else {
            // Either a pure-ref bracket or a bracket with no
            // citation-relevant content — pass through unchanged.
            out.push('[');
            out.push_str(inner);
            out.push(']');
        }
        cursor = close + 1;
    }

    // Re-attach the untouched `<CITATIONS>…</CITATIONS>` segment (and
    // anything after it) verbatim, so the trailing JSON survives even
    // when the prose was rewritten.
    out.push_str(tail);

    std::borrow::Cow::Owned(out)
}

/// Rewrite an assistant response that cites attached documents through
/// the free-form `[doc-id: <handle>, page <N>]` pattern into the
/// canonical `[cN]` markers + `<CITATIONS>` block format. `resolve`
/// returns `Some((document_uuid, filename))` for handles that point to
/// a real document the user can access — handles that resolve to
/// `None` are left untouched in the prose (they continue to render as
/// plain text, which is the safest fallback).
///
/// Returns `Some((rewritten_text, citations_array))` when at least one
/// reference was successfully rewritten; `None` means the text had no
/// such references (or none resolved). Two references with the same
/// `(uuid, page)` share a `cN` ref so the `<CITATIONS>` block stays
/// compact.
/// Second-shape scanner: the model also writes free-form citations in
/// parenthesised prose using the Italian convention
/// `(... doc-N, pag. <N>[-<M>])`, with no surrounding `[doc-id: …]`
/// brackets and "pag" instead of "page". Observed in the wild on the
/// `Inventario beni assicurati` workflow's docx-description follow-up.
///
/// Only the `doc-N, pag X` substring is captured — the prose context
/// (e.g. `(Polizza n. 449435502/39,`) stays untouched, so the rewriter
/// can fold the marker into a `[cN]` pill without eating meaningful
/// content from the user's view.
///
/// Recognises: `doc-N`, optional comma + whitespace, then a page word
/// (`pag`, `pag.`, `pagina`, `pagine`, `page`, `pages`, `p.`), then
/// digits with optional `-`/`–` range.
fn extract_inline_paren_doc_refs(text: &str) -> Vec<InlineDocIdRef> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let lower = text.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(off) = lower[search_from..].find("doc-") {
        let start = search_from + off;
        let after_doc = start + 4;
        // Read the digit suffix: `doc-1`, `doc-12`, …
        let mut k = after_doc;
        while k < bytes.len() && bytes[k].is_ascii_digit() {
            k += 1;
        }
        if k == after_doc {
            search_from = after_doc;
            continue;
        }
        let handle_end = k;

        // Optional `, ` between handle and the page word.
        let mut j = handle_end;
        if j < bytes.len() && bytes[j] == b',' {
            j += 1;
        }
        while j < bytes.len() && (bytes[j] as char).is_whitespace() {
            j += 1;
        }

        // Page word, case-insensitive. Order matters: longer prefixes
        // before shorter ones so `pagine` isn't truncated to `pag`.
        let rest = &lower[j..];
        let page_word_len = if rest.starts_with("pagine") { 6 }
            else if rest.starts_with("pagina") { 6 }
            else if rest.starts_with("pages") { 5 }
            else if rest.starts_with("pag.") { 4 }
            else if rest.starts_with("page") { 4 }
            else if rest.starts_with("pag") { 3 }
            else if rest.starts_with("p.") { 2 }
            else { 0 };
        if page_word_len == 0 {
            search_from = handle_end;
            continue;
        }
        j += page_word_len;
        while j < bytes.len() && (bytes[j] as char).is_whitespace() {
            j += 1;
        }

        // Digits + optional range `-N` or `–N` (em-dash).
        let page_digits_start = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j == page_digits_start {
            search_from = handle_end;
            continue;
        }
        let mut end = j;
        if end < bytes.len() {
            let c = bytes[end] as char;
            let dash_len = if c == '-' {
                Some(1usize)
            } else if text[end..].starts_with('–') {
                Some('–'.len_utf8())
            } else {
                None
            };
            if let Some(d) = dash_len {
                let after_dash = end + d;
                let mut r = after_dash;
                while r < bytes.len() && bytes[r].is_ascii_digit() {
                    r += 1;
                }
                if r > after_dash {
                    end = r;
                }
            }
        }

        out.push(InlineDocIdRef {
            start,
            end,
            handle: text[start..handle_end].to_string(),
            page: Some(text[page_digits_start..end].to_string()),
        });
        search_from = end;
    }
    out
}

fn rewrite_inline_docid_citations<F>(text: &str, mut resolve: F) -> Option<(String, Value)>
where
    F: FnMut(&str) -> Option<(String, String)>,
{
    // Both shapes feed the same rewriter. The bracketed `[doc-id: …]`
    // form and the parenthesised `... doc-N, pag …` form are gathered
    // into a single list, then sorted by start offset so substitution
    // happens in left-to-right document order — and the rewrite loop
    // applies them in reverse so earlier byte offsets stay valid.
    let mut refs = extract_inline_docid_refs(text);
    refs.extend(extract_inline_paren_doc_refs(text));
    refs.sort_by_key(|r| r.start);
    // Drop overlapping captures (e.g. an inner paren scan landing on
    // bytes already inside an outer `[doc-id: …]` bracket capture).
    let mut deduped: Vec<InlineDocIdRef> = Vec::with_capacity(refs.len());
    for r in refs {
        if let Some(prev) = deduped.last() {
            if r.start < prev.end {
                continue;
            }
        }
        deduped.push(r);
    }
    let refs = deduped;
    if refs.is_empty() {
        return None;
    }
    let mut citations: Vec<Value> = Vec::new();
    let mut key_to_ref: HashMap<(String, Option<String>), String> = HashMap::new();
    let mut rewrites: Vec<(usize, usize, String)> = Vec::new();
    for r in &refs {
        let Some((uuid, filename)) = resolve(&r.handle) else {
            continue; // unresolved → keep the original text as-is
        };
        let key = (uuid.clone(), r.page.clone());
        let ref_id = match key_to_ref.get(&key) {
            Some(id) => id.clone(),
            None => {
                let id = format!("c{}", citations.len() + 1);
                let mut obj = serde_json::Map::new();
                obj.insert("ref".into(), Value::String(id.clone()));
                // Keep both `doc_id` and `document_id` — the downstream
                // citation enrichment looks at both names.
                obj.insert("doc_id".into(), Value::String(uuid.clone()));
                obj.insert("document_id".into(), Value::String(uuid.clone()));
                obj.insert("filename".into(), Value::String(filename.clone()));
                if let Some(p) = &r.page {
                    if let Ok(n) = p.parse::<i64>() {
                        obj.insert("page".into(), Value::Number(n.into()));
                    } else {
                        obj.insert("page".into(), Value::String(p.clone()));
                    }
                }
                obj.insert("source".into(), Value::String("attached".to_string()));
                citations.push(Value::Object(obj));
                key_to_ref.insert(key, id.clone());
                id
            }
        };
        rewrites.push((r.start, r.end, ref_id));
    }
    if citations.is_empty() {
        return None;
    }
    // Rewrite in REVERSE order so earlier byte offsets stay valid as
    // later replacements change the string length.
    let mut out = text.to_string();
    for (start, end, ref_id) in rewrites.iter().rev() {
        out.replace_range(*start..*end, &format!("[{ref_id}]"));
    }
    Some((out, Value::Array(citations)))
}

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn err(status: StatusCode, msg: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"detail": msg})))
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list_chats).post(post_chat_root))
        .route("/{id}", get(get_chat).patch(patch_chat).delete(delete_chat))
        .route("/{id}/messages", get(get_messages))
        .route("/{id}/documents", get(get_chat_documents))
        .route("/{id}/message", axum::routing::post(post_message))
        .route("/{id}/generate-title", axum::routing::post(generate_title))
}

// ---------------------------------------------------------------------------
// GET /chat  — list chats for user
// ---------------------------------------------------------------------------
async fn list_chats(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    let rows: Vec<(String, String, Option<String>, Option<String>, String)> =
        sqlx::query_as(
            "SELECT id, user_id, project_id, title, updated_at \
             FROM chats WHERE user_id = ? ORDER BY updated_at DESC",
        )
        .bind(&auth.user_id)
        .fetch_all(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let chats: Vec<Value> = rows
        .into_iter()
        .map(|(id, user_id, project_id, title, updated_at)| {
            json!({
                "id": id,
                "user_id": user_id,
                "project_id": project_id,
                "title": title,
                "updated_at": updated_at,
            })
        })
        .collect();

    Ok(Json(json!({ "chats": chats })))
}

// ---------------------------------------------------------------------------
// POST /chat — dispatched by body shape
//   - { messages: [...], chat_id?, model? }     → SSE streaming
//   - { project_id?, title? } (no messages)    → create chat record (JSON)
// ---------------------------------------------------------------------------
async fn post_chat_root(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<Value>,
) -> Response {
    let has_messages = body
        .get("messages")
        .and_then(|v| v.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    tracing::info!("[chat] POST / dispatch: has_messages={has_messages}, user={}", auth.username);

    if has_messages {
        return stream_chat_root(state, auth, body).await;
    }
    create_chat_record(state, auth, body).await
}

async fn create_chat_record(
    state: Arc<AppState>,
    auth: AuthUser,
    body: Value,
) -> Response {
    let project_id = body.get("project_id").and_then(|v| v.as_str()).map(|s| s.to_string());
    let title = body.get("title").and_then(|v| v.as_str()).map(|s| s.to_string());

    let id = uuid::Uuid::new_v4().to_string();
    if let Err(e) = sqlx::query(
        "INSERT INTO chats (id, user_id, project_id, title) VALUES (?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .bind(&project_id)
    .bind(&title)
    .execute(&state.db)
    .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"detail": e.to_string()})),
        )
            .into_response();
    }

    (StatusCode::OK, Json(json!({ "id": id }))).into_response()
}

/// SSE for `POST /chat`.
/// Body: { messages: [{role, content, files?, workflow?, template?}], chat_id?, model? }
/// Emits `data: {type: ...}` events read by the chat client.
type SseSender = tokio::sync::mpsc::Sender<Result<Event, Infallible>>;

/// Sends an SSE event. `false` if the client has disconnected.
async fn emit(tx: &SseSender, payload: &Value) -> bool {
    tx.send(Ok(Event::default().data(payload.to_string())))
        .await
        .is_ok()
}

/// Appends text to the reply and shows it in the chat right away.
async fn emit_visible_text(tx: &SseSender, full_response: &mut String, text: &str) {
    full_response.push_str(text);
    emit(tx, &json!({ "type": "content_delta", "text": text })).await;
}

fn sql_placeholders(n: usize) -> String {
    vec!["?"; n].join(",")
}

/// The user's last message as it is stored: original text (without the
/// model-facing markers) and serialised structured metadata.
struct LastUserMessage {
    content: String,
    files: Option<String>,
    workflow: Option<String>,
    template: Option<String>,
}

/// `/chat` payload parsed in a single pass.
#[derive(Default)]
struct ParsedChatRequest {
    messages: Vec<Message>,
    /// Attached documents, in order of first appearance.
    doc_ids: Vec<String>,
    /// Attachments for which the user requested personal-data protection.
    pii_protected_ids: HashSet<String>,
    last_user: Option<LastUserMessage>,
}

fn parse_chat_request(body: &Value) -> ParsedChatRequest {
    let mut out = ParsedChatRequest::default();
    let Some(items) = body.get("messages").and_then(Value::as_array) else {
        return out;
    };
    out.messages.reserve(items.len());

    for m in items {
        if let Some(files) = m.get("files").and_then(Value::as_array) {
            for f in files {
                let Some(id) = f.get("document_id").and_then(Value::as_str) else { continue };
                if !out.doc_ids.iter().any(|x| x == id) {
                    out.doc_ids.push(id.to_string());
                }
                if f.get("pii_protected").and_then(Value::as_bool).unwrap_or(false) {
                    out.pii_protected_ids.insert(id.to_string());
                }
            }
        }

        let Some(role_name) = m.get("role").and_then(Value::as_str) else { continue };
        let content = m.get("content").and_then(Value::as_str).unwrap_or("");
        let role = match role_name {
            "user" => Role::User,
            "assistant" => Role::Assistant,
            "tool" => Role::Tool,
            _ => continue,
        };

        let content = if matches!(role, Role::User) {
            out.last_user = Some(LastUserMessage {
                content: content.to_string(),
                files: m
                    .get("files")
                    .filter(|v| v.as_array().is_some_and(|a| !a.is_empty()))
                    .map(Value::to_string),
                workflow: m.get("workflow").filter(|v| v.is_object()).map(Value::to_string),
                template: m.get("template").filter(|v| v.is_object()).map(Value::to_string),
            });
            with_selection_markers(m, content)
        } else {
            content.to_string()
        };

        out.messages.push(Message {
            role,
            content,
            images: vec![],
            tool_calls: vec![],
            tool_call_id: None,
            tool_name: None,
        });
    }
    out
}

/// The workflow and DOCX template picked in the composer arrive as JSON fields;
/// the model recognises them as markers at the start of the message (see
/// `config/system-prompts/base.md`, sezione 7).
fn with_selection_markers(message: &Value, content: &str) -> String {
    let marker = |key: &str, kind: &str| {
        let obj = message.get(key)?;
        let id = obj.get("id").and_then(Value::as_str)?;
        let title = obj.get("title").and_then(Value::as_str).unwrap_or("");
        Some(format!("[{kind}: {title} (id: {id})]\n"))
    };
    let workflow = marker("workflow", "Workflow");
    let template = marker("template", "Template");
    if workflow.is_none() && template.is_none() {
        return content.to_string();
    }
    let mut out = String::with_capacity(content.len() + 128);
    out.push_str(workflow.as_deref().unwrap_or(""));
    out.push_str(template.as_deref().unwrap_or(""));
    out.push('\n');
    out.push_str(content);
    out
}

/// The user's interface language and default domain.
async fn fetch_locale_and_default_domain(state: &AppState, user_id: &str) -> (String, Option<String>) {
    let row: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT locale, default_domain FROM user_settings WHERE user_id = ?")
            .bind(user_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
    let (locale, default_domain) = row.unwrap_or((None, None));
    let locale = locale
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("it")
        .to_string();
    (locale, default_domain)
}

async fn stream_chat_root(
    state: Arc<AppState>,
    auth: AuthUser,
    body: Value,
) -> Response {
    let model_request = body.get("model").and_then(Value::as_str).map(str::to_string);

    // Existing chat: a single query checks ownership and reads the project.
    let (chat_id, is_new_chat, chat_project_id) =
        match body.get("chat_id").and_then(Value::as_str) {
            Some(id) => {
                let row: Option<(Option<String>,)> =
                    sqlx::query_as("SELECT project_id FROM chats WHERE id = ? AND user_id = ?")
                        .bind(id)
                        .bind(&auth.user_id)
                        .fetch_optional(&state.db)
                        .await
                        .ok()
                        .flatten();
                let Some((project_id,)) = row else {
                    return (StatusCode::NOT_FOUND, Json(json!({"detail": "Chat not found"})))
                        .into_response();
                };
                (id.to_string(), false, project_id)
            }
            None => {
                let id = uuid::Uuid::new_v4().to_string();
                if let Err(e) = sqlx::query(
                    "INSERT INTO chats (id, user_id, project_id, title) VALUES (?, ?, NULL, NULL)",
                )
                .bind(&id)
                .bind(&auth.user_id)
                .execute(&state.db)
                .await
                {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"detail": e.to_string()})),
                    )
                        .into_response();
                }
                (id, true, None)
            }
        };

    let ParsedChatRequest { messages, mut doc_ids, pii_protected_ids, last_user } =
        parse_chat_request(&body);
    tracing::info!(
        "[chat] payload parsed — attachments={} pii_protected={} (ner-pii built-in: {})",
        doc_ids.len(),
        pii_protected_ids.len(),
        cfg!(feature = "ner-pii"),
    );

    // Personal-data protection stays on the document (migration
    // 0028): later turns without the attachment still honour it.
    if !pii_protected_ids.is_empty() {
        let sql = format!(
            "UPDATE documents SET pii_protected = 1 WHERE user_id = ? AND id IN ({})",
            sql_placeholders(pii_protected_ids.len())
        );
        let mut q = sqlx::query(&sql).bind(&auth.user_id);
        for id in &pii_protected_ids {
            q = q.bind(id);
        }
        if let Err(e) = q.execute(&state.db).await {
            tracing::warn!("[chat] failed to persist pii_protected flags: {e}");
        }
    }

    // Link new cached attachments to the chat, so deleting the
    // chat removes their files (migration 0013). Only documents not yet
    // linked (`chat_id IS NULL`) and uploaded to the cache (`content_hash`):
    // project documents must not follow the chat.
    if !doc_ids.is_empty() {
        let sql = format!(
            "UPDATE documents SET chat_id = ? \
             WHERE user_id = ? AND chat_id IS NULL AND content_hash IS NOT NULL \
               AND id IN ({})",
            sql_placeholders(doc_ids.len())
        );
        let mut q = sqlx::query(&sql).bind(&chat_id).bind(&auth.user_id);
        for id in &doc_ids {
            q = q.bind(id);
        }
        match q.execute(&state.db).await {
            Ok(res) => tracing::info!(
                "[chat] linked {}/{} attached cache doc(s) to chat {chat_id}",
                res.rows_affected(),
                doc_ids.len()
            ),
            Err(e) => tracing::warn!("[chat] failed to link attached docs to chat {chat_id}: {e}"),
        }
    }

    // Independent queries in parallel: attachments from earlier turns,
    // project documents (labelled and read on demand, not loaded
    // in full) and project identity.
    let (earlier_docs, project_documents, project_meta) = tokio::join!(
        async {
            sqlx::query_as::<_, (String,)>(
                "SELECT id FROM documents WHERE chat_id = ? AND user_id = ? ORDER BY created_at ASC",
            )
            .bind(&chat_id)
            .bind(&auth.user_id)
            .fetch_all(&state.db)
            .await
            .unwrap_or_default()
        },
        async {
            let Some(pid) = &chat_project_id else { return Vec::new() };
            sqlx::query_as::<_, (String, String)>(
                "SELECT id, filename FROM documents \
                 WHERE project_id = ? AND user_id = ? AND status = 'ready' \
                 ORDER BY created_at ASC",
            )
            .bind(pid)
            .bind(&auth.user_id)
            .fetch_all(&state.db)
            .await
            .unwrap_or_default()
        },
        async {
            let Some(pid) = &chat_project_id else { return None };
            sqlx::query_as::<_, (String, String)>(
                "SELECT name, domain FROM projects WHERE id = ? AND user_id = ?",
            )
            .bind(pid)
            .bind(&auth.user_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten()
        },
    );
    // Appended at the end: doc-N labels already assigned don't change.
    for (id,) in earlier_docs {
        if !doc_ids.contains(&id) {
            doc_ids.push(id);
        }
    }
    let project_documents: Vec<(String, String)> = project_documents
        .into_iter()
        .filter(|(id, _)| !doc_ids.contains(id))
        .collect();

    // Store the original text, without markers, with the metadata that
    // rebuilds the composer pills when the chat is reopened (migration 0021).
    if let Some(msg) = last_user.filter(|m| !m.content.trim().is_empty()) {
        let _ = sqlx::query(
            "INSERT INTO messages (id, chat_id, role, content, files, workflow, template) \
             VALUES (?, ?, 'user', ?, ?, ?, ?)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&chat_id)
        .bind(&msg.content)
        .bind(&msg.files)
        .bind(&msg.workflow)
        .bind(&msg.template)
        .execute(&state.db)
        .await;
    }

    let user_settings = fetch_llm_settings(&state.db, &auth.user_id).await.ok();
    let raw_model = model_request
        .or_else(|| user_settings.as_ref().and_then(|s| s.main_model.clone()))
        .unwrap_or_else(|| "gemini-3.5-flash".to_string());
    let local_config = build_local_config(&raw_model, user_settings.as_ref());
    let vision_ok = llm::is_vision_capable(&raw_model);

    // Retrieval uses only the last question: the full history
    // mixes different topics and worsens similarity.
    let last_user_query = messages
        .iter()
        .rev()
        .find(|m| matches!(m.role, Role::User))
        .map(|m| m.content.clone())
        .unwrap_or_default();
    let kb_top_k = if doc_ids.is_empty() { 8 } else { 6 };

    // The heavy work (text extraction, personal-data redaction,
    // history summarisation) runs in the task, so the client receives the
    // progress events as they happen instead of all together at the end.
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(64);
    tokio::spawn(run_chat_turn(
        ChatTurn {
            state,
            user_id: auth.user_id,
            chat_id,
            is_new_chat,
            messages,
            doc_ids,
            pii_protected_ids,
            project_documents,
            project_meta,
            user_settings,
            raw_model,
            local_config,
            vision_ok,
            last_user_query,
            kb_top_k,
        },
        tx,
    ));

    Sse::new(ReceiverStream::new(rx))
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

/// Everything needed to run a chat turn in the background task.
struct ChatTurn {
    state: Arc<AppState>,
    user_id: String,
    chat_id: String,
    is_new_chat: bool,
    messages: Vec<Message>,
    doc_ids: Vec<String>,
    pii_protected_ids: HashSet<String>,
    project_documents: Vec<(String, String)>,
    project_meta: Option<(String, String)>,
    user_settings: Option<crate::routes::user::LlmSettings>,
    raw_model: String,
    local_config: Option<LocalConfig>,
    vision_ok: bool,
    last_user_query: String,
    kb_top_k: usize,
}

/// Maximum loop iterations for a turn (each tool call
/// uses one). 20 covers analysis across a dozen documents
/// and still bounds a stuck loop.
const MAX_TOOL_ITERATIONS: u32 = 20;
/// Nudges to the model when it ends the turn with no text and no tools
/// (happens mostly with Gemini right after a tool result).
const MAX_EMPTY_ANSWER_RETRIES: u32 = 2;

async fn run_chat_turn(turn: ChatTurn, tx: SseSender) {
    let ChatTurn {
        state,
        user_id,
        chat_id,
        is_new_chat,
        messages,
        doc_ids,
        pii_protected_ids,
        project_documents,
        project_meta,
        user_settings,
        raw_model,
        local_config,
        vision_ok,
        last_user_query,
        kb_top_k,
    } = turn;

    if is_new_chat {
        emit(&tx, &json!({ "type": "chat_id", "chatId": &chat_id })).await;
    }

    // Parallel loading. The library listing lets the model
    // know which sources the user has even when semantic search
    // doesn't surface them.
    let (attached_docs, mcp_servers, kb_chunks, library_inventory, (locale, default_domain)) = tokio::join!(
        load_attached_docs(&state, &user_id, &doc_ids, vision_ok, &pii_protected_ids, &tx),
        discover_mcp_for_user(&state, &user_id),
        retrieve_kb_chunks(&state, &user_id, &chat_id, &last_user_query, kb_top_k),
        list_indexed_corpus_docs(&state, &user_id),
        fetch_locale_and_default_domain(&state, &user_id),
    );
    drop(pii_protected_ids);

    // The project domain takes precedence over the user's default one.
    let domain = project_meta
        .as_ref()
        .map(|(_, d)| d.clone())
        .or(default_domain)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "others".to_string());

    // Prefix stable across turns, sent as a cacheable block. The knowledge
    // base passages change with every question and travel separately
    // (`system_volatile`) so they don't invalidate the cache.
    // Local Ollama models report their real window; cloud models use the
    // catalogue or the name-based table.
    let context_window = llm::context_window::resolve(
        &raw_model,
        local_config.as_ref(),
        Some(state.model_catalogue.as_ref()),
    )
    .await;

    let prologue = crate::presets::system_prompt::assemble_prologue(&locale, &domain);
    let base = crate::presets::system_prompt::base_instructions();
    let inventory_block = prompts::build_library_inventory_prompt(&library_inventory);
    let project_block = project_meta
        .as_ref()
        .map(|(name, d)| prompts::build_project_context_prompt(name, d))
        .unwrap_or_default();
    let project_docs_block = prompts::build_project_docs_prompt(doc_ids.len(), &project_documents);
    let mcp_block = prompts::build_mcp_system_prompt(&mcp_servers);
    let system_volatile = prompts::build_kb_system_prompt(&kb_chunks);
    drop(library_inventory);

    // Attachments get what the rest of the request leaves of 90% of the
    // window. With a local model whose window the server has not reported
    // yet they are sent whole: a guessed window would cut them needlessly,
    // and an overflow error teaches the real value for the next turn.
    let tools_tokens = llm::summarize::estimate_tokens(
        &serde_json::to_string(&builtin_tools::schemas()).unwrap_or_default(),
    );
    let fixed_tokens = [&prologue, &base, &inventory_block, &project_block, &project_docs_block, &mcp_block, &system_volatile]
        .iter()
        .map(|s| llm::summarize::estimate_tokens(s))
        .sum::<usize>()
        + llm::summarize::estimate_messages_tokens(&messages)
        + tools_tokens
        + llm::summarize::REPLY_RESERVE_TOKENS;
    let usable_window = context_window.tokens * 9 / 10;
    let mut attached_docs = attached_docs;
    let excerpted = if context_window.is_known() || !raw_model.starts_with("local:") {
        attachment_budget::fit(
            &mut attached_docs,
            &last_user_query,
            usable_window.saturating_sub(fixed_tokens),
        )
    } else {
        Vec::new()
    };

    let system_prompt = [
        prologue,
        base,
        inventory_block,
        prompts::build_doc_system_prompt(&attached_docs),
        project_block,
        project_docs_block,
        mcp_block,
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join(prompts::SECTION_SEPARATOR);

    // Budget for each `read_document` result: half of what the prompt
    // leaves, so a couple of reads fit; never below a useful minimum.
    let read_budget_chars = {
        let prompt_tokens = llm::summarize::estimate_tokens(&system_prompt)
            + llm::summarize::estimate_tokens(&system_volatile)
            + llm::summarize::estimate_messages_tokens(&messages)
            + tools_tokens
            + llm::summarize::REPLY_RESERVE_TOKENS;
        (usable_window.saturating_sub(prompt_tokens) / 2 * 4).max(8_000)
    };
    let excerpt_note = (!excerpted.is_empty()).then(|| {
        let items: Vec<(String, attachment_budget::ExcerptInfo)> = excerpted
            .iter()
            .filter_map(|&i| attached_docs[i].excerpt.map(|e| (attached_docs[i].filename.clone(), e)))
            .collect();
        prompts::attachments_excerpted_note(&locale, &items)
    });

    let attached_count = attached_docs.len();
    let images: Vec<String> = if vision_ok {
        attached_docs.into_iter().flat_map(|d| d.images).collect()
    } else {
        Vec::new()
    };
    let image_count = images.len();
    let mut messages = messages;
    if !images.is_empty()
        && let Some(last_user) = messages.iter_mut().rev().find(|m| matches!(m.role, Role::User))
    {
        // Page images go on the message the model is replying to.
        last_user.images = images;
    }

    tracing::info!(
        "[chat] turn: chat_id={chat_id}, model={raw_model}, vision_ok={vision_ok}, local_config_present={}, docs={attached_count}, mcp_servers={}, kb_chunks={} (sys_prompt={} chars cacheable + {} volatile, images={image_count})",
        local_config.is_some(),
        mcp_servers.len(),
        kb_chunks.len(),
        system_prompt.len(),
        system_volatile.len(),
    );

    // Tools: built-in ones first, then MCP ones only for models
    // that handle many schemas well (`llm::supports_mcp_tools`).
    let mut all_tools: Vec<ToolSchema> = builtin_tools::schemas();
    let mcp_tools_enabled = llm::supports_mcp_tools(&raw_model);
    let mcp_tool_count: usize = mcp_servers.iter().map(|s| s.tool_schemas.len()).sum();
    if mcp_tools_enabled {
        all_tools.reserve(mcp_tool_count);
        for srv in &mcp_servers {
            all_tools.extend(srv.tool_schemas.iter().cloned());
        }
    }

    // doc-N labels (from 1) → UUID: attachments first, then project
    // documents, as in the prompt blocks.
    // Grows during the tool loop: a document generated mid-turn gets the
    // next label, so the model can cite it and the citation resolver can
    // trace it back to a real row.
    let mut doc_label_map: HashMap<String, String> = doc_ids
        .iter()
        .chain(project_documents.iter().map(|(id, _)| id))
        .enumerate()
        .map(|(i, id)| (format!("doc-{}", i + 1), id.clone()))
        .collect();
    drop(project_documents);

    tracing::info!(
        "[chat] tool-use: {} total tools (builtin + {mcp_tool_count} MCP, mcp_enabled={mcp_tools_enabled}), labels={}",
        all_tools.len(),
        doc_label_map.len()
    );
    if mcp_tool_count > 0 {
        if mcp_tools_enabled {
            let names: Vec<&str> = mcp_servers
                .iter()
                .flat_map(|s| s.tool_schemas.iter().map(|t| t.function.name.as_str()))
                .collect();
            tracing::info!("[chat] MCP tools shipped to model: {names:?}");
        } else {
            let servers: Vec<&str> = mcp_servers.iter().map(|s| s.config_name.as_str()).collect();
            tracing::info!(
                "[chat] MCP servers discovered ({mcp_tool_count} tools total) but NOT shipped — model {raw_model:?} not in supports_mcp_tools allowlist. Servers: {servers:?}. Set COVE_FORCE_MCP_TOOLS=1 to override."
            );
        }
    }

    let claude_key = user_settings.as_ref().and_then(|s| s.claude_api_key.clone());
    let gemini_key = user_settings.as_ref().and_then(|s| s.gemini_api_key.clone());
    let gemini_region = user_settings.as_ref().and_then(|s| s.gemini_region.clone());
    let mistral_opts = build_mistral_opts(&raw_model, user_settings.as_ref());

    // Compress the oldest turns beyond 80% of the context window,
    // counting the system prompt too. If summarisation fails
    // the messages stay unchanged.
    let summarizer_creds = llm::summarize::SummarizerCreds {
        local_config: local_config.clone(),
        claude_api_key: claude_key.clone(),
        gemini_api_key: gemini_key.clone(),
        gemini_region: gemini_region.clone(),
    };
    let system_overhead = llm::summarize::estimate_tokens(&system_prompt)
        + llm::summarize::estimate_tokens(&system_volatile);
    let mut current_messages = llm::summarize::maybe_compress_history(
        messages,
        &raw_model,
        &summarizer_creds,
        system_overhead,
        context_window.tokens,
    )
    .await;
    drop(summarizer_creds);

    let mut full_response = String::new();
    if let Some(note) = &excerpt_note {
        emit_visible_text(&tx, &mut full_response, note).await;
    }

    // Compression only shortens the history: attached documents in the
    // system prompt can still overflow a small window. Warn before sending
    // when the window is known from the server.
    let estimated_tokens = system_overhead
        + llm::summarize::estimate_messages_tokens(&current_messages)
        + llm::summarize::REPLY_RESERVE_TOKENS;
    if context_window.is_known() && estimated_tokens > context_window.tokens {
        tracing::warn!(
            "[chat] estimated request ≈{estimated_tokens} tokens exceeds the {} token window of {raw_model}",
            context_window.tokens
        );
        let note = prompts::context_overflow_note(&locale, estimated_tokens, context_window.tokens);
        emit_visible_text(&tx, &mut full_response, &note).await;
    }

    // Events to store with the message (today `doc_created`, for the download
    // card on reopen), in the same shape sent while streaming.
    let mut persistent_events: Vec<Value> = Vec::new();
    // Documents that already got a download card this turn.
    let mut carded_documents: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut empty_answer_retries: u32 = 0;
    let mut iteration: u32 = 0;
    let mut errored = false;

    // Some models reject the `tools` parameter: this is found out on the first
    // call and remembered in AppState so the attempt isn't repeated.
    let already_known_unsupported = state.no_tools_models.read().await.contains(&raw_model);
    let mut tools_supported = !all_tools.is_empty() && !already_known_unsupported;
    let mut tool_warning_emitted = false;
    if !all_tools.is_empty() && already_known_unsupported {
        let warning = prompts::unsupported_tools_warning(&locale, &raw_model, mcp_servers.len());
        emit_visible_text(&tx, &mut full_response, &warning).await;
        tool_warning_emitted = true;
    }

    loop {
        iteration += 1;
        let params = StreamParams {
            model: raw_model.clone(),
            system_prompt: system_prompt.clone(),
            system_volatile: system_volatile.clone(),
            messages: current_messages.clone(),
            tools: if tools_supported { all_tools.clone() } else { vec![] },
            max_iterations: 1,
            enable_thinking: false,
            local_config: local_config.clone(),
            claude_api_key: claude_key.clone(),
            gemini_api_key: gemini_key.clone(),
            gemini_region: gemini_region.clone(),
            // Used by Mistral for a stable prompt_cache_key.
            chat_id: Some(chat_id.clone()),
            mistral_opts: mistral_opts.clone(),
        };

        let mut stream = match llm::stream_chat(params).await {
            Ok(s) => s,
            Err(e) => {
                let msg = e.to_string();
                // Only an explicit tool rejection disables
                // tool use; a generic 400 is almost always a malformed schema.
                let lower = msg.to_lowercase();
                let unsupported = [
                    "does not support tools",
                    "tools not supported",
                    "does not support tool use",
                    "tool use is not supported",
                    "functioncalling is not supported",
                    "function calling is not supported",
                ]
                .iter()
                .any(|needle| lower.contains(needle));
                if tools_supported && unsupported {
                    tracing::warn!(
                        "[chat] model {raw_model}: tools rejected — retrying without tool-use. Original error: {}",
                        msg.chars().take(500).collect::<String>()
                    );
                    state.no_tools_models.write().await.insert(raw_model.clone());
                    tools_supported = false;
                    if !tool_warning_emitted {
                        let warning = prompts::unsupported_tools_warning(&locale, &raw_model, mcp_servers.len());
                        emit_visible_text(&tx, &mut full_response, &warning).await;
                        tool_warning_emitted = true;
                    }
                    iteration -= 1;
                    continue;
                }
                tracing::error!("[chat] stream_chat error (iter {iteration}): {msg}");
                // A context-overflow error carries the server's real window:
                // remember it so the next turn plans with it.
                if let Some(window) =
                    llm::context_window::learn_from_error(&raw_model, local_config.as_ref(), &msg)
                {
                    let note = prompts::context_window_learnt_note(&locale, window);
                    emit_visible_text(&tx, &mut full_response, &note).await;
                }
                emit(&tx, &json!({ "type": "error", "message": msg })).await;
                errored = true;
                break;
            }
        };

        let mut iter_text = String::new();
        let mut iter_tool_calls: Vec<ToolCall> = Vec::new();
        let mut got_done = false;
        let mut got_err: Option<String> = None;
        while let Some(event) = stream.next().await {
            match event {
                Ok(StreamEvent::ContentDelta(text)) => {
                    iter_text.push_str(&text);
                    full_response.push_str(&text);
                    if !emit(&tx, &json!({ "type": "content_delta", "text": text })).await {
                        break;
                    }
                }
                // Accumulate: Gemini sends parallel calls in separate chunks,
                // each with its own thoughtSignature, and they must all be
                // returned on the next turn.
                Ok(StreamEvent::ToolCalls(calls)) => iter_tool_calls.extend(calls),
                // The model's reasoning has its own event and doesn't
                // go into the reply.
                Ok(StreamEvent::ReasoningDelta(text)) => {
                    if !emit(&tx, &json!({ "type": "reasoning_delta", "text": text })).await {
                        break;
                    }
                }
                Ok(StreamEvent::ReasoningEnd) => {
                    emit(&tx, &json!({ "type": "reasoning_done" })).await;
                }
                Ok(StreamEvent::Done) => {
                    got_done = true;
                    break;
                }
                Err(e) => {
                    got_err = Some(e.to_string());
                    break;
                }
                _ => {}
            }
        }
        drop(stream);
        tracing::info!(
            "[chat] iter {iteration}: text={}, tool_calls={}, done={got_done}, err={got_err:?}",
            iter_text.len(),
            iter_tool_calls.len(),
        );

        if iter_tool_calls.is_empty() {
            if iter_text.trim().is_empty()
                && full_response.trim().is_empty()
                && empty_answer_retries < MAX_EMPTY_ANSWER_RETRIES
            {
                empty_answer_retries += 1;
                tracing::warn!(
                    "[chat] empty answer at iter {iteration}; nudging model (retry {empty_answer_retries}/{MAX_EMPTY_ANSWER_RETRIES})"
                );
                current_messages.push(Message::user(prompts::EMPTY_ANSWER_NUDGE));
                continue;
            }
            if full_response.trim().is_empty() {
                emit_visible_text(&tx, &mut full_response, prompts::empty_answer_note(&locale)).await;
            }
            break;
        }
        if iteration >= MAX_TOOL_ITERATIONS {
            tracing::warn!("[chat] hit MAX_TOOL_ITERATIONS, stopping");
            emit(&tx, &json!({ "type": "content_delta", "text": prompts::tool_loop_limit_note(&locale) })).await;
            break;
        }
        // A tool call proves the model is active: the
        // empty-reply nudges start again from zero.
        empty_answer_retries = 0;

        current_messages.push(Message::assistant_tool_calls(iter_tool_calls.clone()));
        for call in &iter_tool_calls {
            emit(&tx, &json!({ "type": "tool_call_start", "name": call.name })).await;

            // A `tool_call_progress` every 5 s: slow MCP tools (for
            // example with human approval) stay visibly in progress.
            let started = std::time::Instant::now();
            let progress_name = call.name.clone();
            let progress_tx = tx.clone();
            let progress_task = tokio::spawn(async move {
                let mut ticker = tokio::time::interval(std::time::Duration::from_secs(5));
                ticker.tick().await;
                loop {
                    ticker.tick().await;
                    let payload = json!({
                        "type": "tool_call_progress",
                        "name": progress_name,
                        "elapsed_secs": started.elapsed().as_secs(),
                    });
                    if !emit(&progress_tx, &payload).await {
                        return;
                    }
                }
            });

            let result = if builtin_tools::is_builtin(&call.name) {
                tracing::info!("[chat] dispatching builtin tool: {}", call.name);
                builtin_tools::dispatch(
                    &state,
                    &user_id,
                    Some(&chat_id),
                    &doc_label_map,
                    &call.name,
                    &call.input,
                    read_budget_chars,
                )
                .await
            } else {
                tracing::info!("[chat] dispatching MCP tool: {}", call.name);
                // `request_*` calls with a pending session continue
                // automatically with the matching `get_*`.
                dispatch_mcp_tool_with_async_chain(&mcp_servers, &call.name, &call.input).await
            };
            progress_task.abort();
            emit(&tx, &json!({ "type": "tool_call_done", "name": call.name })).await;

            if result.len() <= 200 {
                tracing::info!("[chat] tool {} result ({} chars): {result}", call.name, result.len());
            } else {
                tracing::info!("[chat] tool {} result: {} chars", call.name, result.len());
            }

            // A document created mid-turn joins the `doc-N` labels: the
            // model needs a label to cite it with (the raw UUID is not a
            // citable handle) and the citation resolver needs the label
            // to find the row. Without this the model invents `doc-1`
            // and the viewer asks the backend for a label.
            let result = match register_generated_document(&call.name, &result, &mut doc_label_map) {
                Some(annotated) => annotated,
                None => result,
            };

            if let Some(event) = tool_step_event(&call.name, &result) {
                // One card per document, however many times the turn
                // touched it: two edits in a row on the same file would
                // otherwise look like two different documents, and the
                // download link always serves the latest bytes anyway.
                let duplicate_card = event["type"] == "doc_created"
                    && event
                        .get("document_id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| !carded_documents.insert(id.to_string()));
                if !duplicate_card {
                    emit(&tx, &event).await;
                    if event["type"] == "doc_created" {
                        let mut stored = event;
                        stored["isStreaming"] = Value::Bool(false);
                        persistent_events.push(stored);
                    }
                }
            }
            current_messages.push(Message::tool_result(&call.id, &call.name, &result));
        }
    }
    drop(current_messages);
    drop(all_tools);

    tracing::info!(
        "[chat] stream finished: chars={}, errored={errored}",
        full_response.len()
    );

    // Model-independent normalisation: a tool result the model pasted
    // into its own answer never belongs in the prose — the download card
    // and the step list already carry it.
    if let std::borrow::Cow::Owned(cleaned) = tool_echo::strip_echoed_tool_results(&full_response) {
        tracing::info!(
            "[chat] tool-result echo stripped from response: {} → {} chars",
            full_response.len(),
            cleaned.len()
        );
        full_response = cleaned;
        emit(&tx, &json!({ "type": "content_replace", "text": &full_response })).await;
    }

    // Model-independent normalisation: `[c1, c2, FILE.pdf, p.4]`
    // is split into valid markers before any other analysis.
    let pre_split_len = full_response.len();
    if let std::borrow::Cow::Owned(split) = split_hybrid_citation_brackets(&full_response) {
        full_response = split;
        tracing::info!(
            "[chat] hybrid-citation-bracket splitter rewrote response: {pre_split_len} → {} chars",
            full_response.len()
        );
        emit(&tx, &json!({ "type": "content_replace", "text": &full_response })).await;
    }

    {
        let lower = full_response.to_ascii_lowercase();
        let tail = full_response
            .char_indices()
            .rev()
            .nth(800)
            .map(|(i, _)| &full_response[i..])
            .unwrap_or(full_response.as_str());
        tracing::info!(
            "[chat][cite-diag] final response: {} chars, {} '[' total, <CITATIONS>={}, </CITATIONS>={}",
            full_response.chars().count(),
            full_response.matches('[').count(),
            lower.contains("<citations>"),
            lower.contains("</citations>")
        );
        tracing::info!("[chat][cite-diag] tail (last 800 chars):\n{tail}");
    }

    // Free-form references `[doc-id: …, page N]` rewritten as `[cN]` with
    // their citations, before saving.
    let model_block = extract_citations_block(&full_response);
    let mut prebuilt_citations: Option<Value> = None;
    if model_block.is_none()
        && let Some((new_body, citations)) =
            rewrite_inline_references(&state, &user_id, &doc_label_map, &full_response).await
    {
        full_response = new_body;
        prebuilt_citations = Some(citations);
        emit(&tx, &json!({ "type": "content_replace", "text": &full_response })).await;
    }

    // The turn is saved even without text if it produced persistent events
    // (e.g. only generate_docx), otherwise the download card
    // would disappear when the chat is reopened.
    let asst_msg_id: Option<String> = if !full_response.is_empty() || !persistent_events.is_empty() {
        let id = uuid::Uuid::new_v4().to_string();
        let events_json = (!persistent_events.is_empty())
            .then(|| Value::Array(std::mem::take(&mut persistent_events)).to_string());
        let _ = sqlx::query(
            "INSERT INTO messages (id, chat_id, role, content, events) VALUES (?, ?, 'assistant', ?, ?)",
        )
        .bind(&id)
        .bind(&chat_id)
        .bind(&full_response)
        .bind(&events_json)
        .execute(&state.db)
        .await;
        let _ = sqlx::query("UPDATE chats SET updated_at = datetime('now') WHERE id = ?")
            .bind(&chat_id)
            .execute(&state.db)
            .await;
        Some(id)
    } else {
        None
    };

    // Citation source, in order: rewrite of free-form references,
    // the model's <CITATIONS> block, synthesis from [gN]/[pN] markers.
    let kb_by_tag: HashMap<String, RetrievedKbEntry> =
        kb_chunks.into_iter().map(|e| (e.tag.clone(), e)).collect();
    let (citations_source, citations_json) = if let Some(prebuilt) = prebuilt_citations {
        ("inline-docid-rewriter", Some(prebuilt))
    } else if let Some(parsed) = model_block {
        ("model-emitted-block", Some(parsed))
    } else if let Some(synth) = synthesise_kb_citations_from_markers(&full_response, &kb_by_tag) {
        ("kb-marker-synthesis", Some(synth))
    } else {
        tracing::warn!(
            "[chat][cite-diag] NO citations from any source — no <CITATIONS> block, no inline doc-id refs, no `g`/`p` markers matching the kb_by_tag map (size={})",
            kb_by_tag.len()
        );
        ("none", None)
    };
    tracing::info!(
        "[chat][cite-diag] resolution source = {citations_source}, pre-enrichment entries = {}",
        citations_json.as_ref().and_then(|v| v.as_array()).map_or(0, Vec::len)
    );

    let citations: Vec<Value> = match citations_json {
        Some(v) if v.as_array().is_some_and(|a| !a.is_empty()) => {
            citation_resolution::CitationResolver::load(&state, &user_id, &doc_label_map, &kb_by_tag)
                .await
                .resolve_all(v)
        }
        _ => Vec::new(),
    };
    for (i, c) in citations.iter().enumerate() {
        tracing::info!(
            // `document_id` is what the viewer will fetch: logging it
            // separates "the model cited doc-1" from "doc-1 resolved to
            // a real row", which look identical without it.
            "[chat]   citation #{i}: doc_id={:?} document_id={:?} source={:?} page={:?} ref={:?}",
            c.get("doc_id").and_then(|v| v.as_str()),
            c.get("document_id").and_then(|v| v.as_str()),
            c.get("source").and_then(|v| v.as_str()),
            c.get("page"),
            c.get("ref"),
        );
    }
    let citation_count = citations.len();
    let citations = Value::Array(citations);

    // Annotations stay on the message for when the chat is reopened.
    if let Some(id) = &asst_msg_id {
        let annotations_json = (citation_count > 0).then(|| citations.to_string());
        match sqlx::query("UPDATE messages SET annotations = ? WHERE id = ?")
            .bind(&annotations_json)
            .bind(id)
            .execute(&state.db)
            .await
        {
            Ok(r) => tracing::info!(
                "[chat] annotations persisted on message id={id} rows_affected={} payload_bytes={}",
                r.rows_affected(),
                annotations_json.as_ref().map_or(0, String::len),
            ),
            Err(e) => tracing::error!("[chat] FAILED to persist annotations on id={id}: {e}"),
        }
    }

    tracing::info!(
        "[chat][cite-diag] FINAL citations SSE payload: {citation_count} entr{}, source={citations_source}",
        if citation_count == 1 { "y" } else { "ies" }
    );
    emit(&tx, &json!({ "type": "citations", "citations": citations })).await;
}

/// Gives a document just created by a tool the next `doc-N` label and
/// tells the model about it. Returns the annotated tool result, or
/// `None` when the call created nothing (wrong tool, error result,
/// already-labelled document).
///
/// The label is what makes a generated document citable: the prompt
/// asks for `doc_id` in the `<CITATIONS>` block to be a `doc-N` handle,
/// and the resolver only maps handles it knows. A generated document
/// that never gets one leaves the model guessing `doc-1` and the viewer
/// requesting `/document/doc-1/display`.
fn register_generated_document(
    tool_name: &str,
    result: &str,
    doc_label_map: &mut HashMap<String, String>,
) -> Option<String> {
    if !matches!(tool_name, "generate_docx" | "generate_xlsx") {
        return None;
    }
    let mut rv: Value = serde_json::from_str(result).ok()?;
    if rv.get("error").is_some() {
        return None;
    }
    let doc_id = rv.get("doc_id").and_then(Value::as_str)?.to_string();
    if doc_id.is_empty() || doc_id.starts_with("doc-") {
        return None;
    }
    let label = doc_label_map
        .iter()
        .find(|(_, id)| **id == doc_id)
        .map(|(label, _)| label.clone())
        .unwrap_or_else(|| {
            let label = format!("doc-{}", doc_label_map.len() + 1);
            doc_label_map.insert(label.clone(), doc_id.clone());
            label
        });
    let map = rv.as_object_mut()?;
    map.insert("doc_label".into(), json!(label));
    let note = map
        .get("note")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_default();
    map.insert(
        "note".into(),
        json!(format!(
            "{note} Cite this document as \"{label}\" in the <CITATIONS> block; \
             read_document accepts either \"{label}\" or the doc_id."
        )
        .trim()
        .to_string()),
    );
    tracing::info!("[chat] generated document {doc_id} labelled {label} for citations");
    Some(rv.to_string())
}

/// Progress event for built-in tools that have a dedicated rendering
/// in the UI. Based on the tool name rather than on the
/// result shape: read_document also returns doc_id and filename.
fn tool_step_event(tool_name: &str, result: &str) -> Option<Value> {
    let rv: Value = serde_json::from_str(result).ok()?;
    if rv.get("error").is_some() {
        return None;
    }
    let s = |k: &str| rv.get(k).and_then(Value::as_str).unwrap_or("");
    match tool_name {
        "generate_docx" | "generate_xlsx" => {
            let (doc_id, filename) = (s("doc_id"), s("filename"));
            (!doc_id.is_empty() && !filename.is_empty()).then(|| {
                json!({
                    "type": "doc_created",
                    "filename": filename,
                    "download_url": format!("/document/{doc_id}/download"),
                    "document_id": doc_id,
                })
            })
        }
        // An edited document is a document the user wants to download
        // too: without a card the only visible trace of the edit is the
        // prose, which is what pushes weaker models to paste the raw
        // tool JSON instead.
        "edit_document" => {
            let (doc_id, filename) = (s("document_id"), s("filename"));
            (!doc_id.is_empty() && !filename.is_empty()).then(|| {
                json!({
                    "type": "doc_created",
                    "filename": filename,
                    "download_url": format!("/document/{doc_id}/download"),
                    "document_id": doc_id,
                })
            })
        }
        "read_document" => Some(json!({
            "type": "doc_read",
            "doc_id": s("doc_id"),
            "filename": s("filename"),
        })),
        "find_in_document" => Some(json!({
            "type": "doc_find",
            "doc_id": s("doc_id"),
            "filename": s("filename"),
            "query": s("query"),
            "match_count": rv.get("match_count").and_then(Value::as_u64).unwrap_or(0),
        })),
        "read_workflow" => Some(json!({
            "type": "workflow_applied",
            "workflow_id": s("workflow_id"),
            "title": s("title"),
        })),
        _ => None,
    }
}

/// Rewrites free-form references `[doc-id: <handle>, page N]` (and similar)
/// as `[cN]` markers, resolving doc-N labels, UUIDs and file
/// names against the user's documents with a single query.
async fn rewrite_inline_references(
    state: &AppState,
    user_id: &str,
    doc_label_map: &HashMap<String, String>,
    text: &str,
) -> Option<(String, Value)> {
    let inline_refs = extract_inline_docid_refs(text);
    if inline_refs.is_empty() {
        return None;
    }
    let handles: HashSet<&str> = inline_refs.iter().map(|r| r.handle.as_str()).collect();
    let uuids: HashSet<&str> = handles
        .iter()
        .filter_map(|h| match doc_label_map.get(*h) {
            Some(uuid) => Some(uuid.as_str()),
            None if h.len() == 36 && h.matches('-').count() == 4 => Some(*h),
            None => None,
        })
        .collect();

    let mut filename_by_uuid: HashMap<String, String> = HashMap::new();
    if !uuids.is_empty() {
        let sql = format!(
            "SELECT id, filename FROM documents WHERE user_id = ? AND id IN ({})",
            sql_placeholders(uuids.len())
        );
        let mut query = sqlx::query_as::<_, (String, String)>(&sql).bind(user_id);
        for u in &uuids {
            query = query.bind(*u);
        }
        if let Ok(rows) = query.fetch_all(&state.db).await {
            filename_by_uuid.extend(rows);
        }
    }
    // The model sometimes uses the file name as a handle.
    let uuid_by_filename: HashMap<&str, &str> = filename_by_uuid
        .iter()
        .map(|(uuid, fname)| (fname.as_str(), uuid.as_str()))
        .collect();
    let handle_to_doc: HashMap<&str, (String, String)> = handles
        .iter()
        .filter_map(|h| {
            let uuid = doc_label_map
                .get(*h)
                .map(String::as_str)
                .or_else(|| uuid_by_filename.get(h).copied())
                .unwrap_or(*h);
            let filename = filename_by_uuid.get(uuid)?;
            Some((*h, (uuid.to_string(), filename.clone())))
        })
        .collect();

    let (new_body, citations) =
        rewrite_inline_docid_citations(text, |h| handle_to_doc.get(h).cloned())?;
    tracing::info!(
        "[chat] rewrote {} inline [doc-id: …] reference(s) to [cN] markers",
        citations.as_array().map_or(0, Vec::len)
    );
    Some((new_body, citations))
}

// ---------------------------------------------------------------------------
// GET /chat/:id
// ---------------------------------------------------------------------------
async fn get_chat(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let row: Option<(String, String, Option<String>, Option<String>, String)> =
        sqlx::query_as(
            "SELECT id, user_id, project_id, title, updated_at \
             FROM chats WHERE id = ? AND user_id = ?",
        )
        .bind(&id)
        .bind(&auth.user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let (chat_id, user_id, project_id, title, updated_at) =
        row.ok_or_else(|| err(StatusCode::NOT_FOUND, "Chat not found"))?;

    type MsgRow = (
        String,         // id
        String,         // role
        Option<String>, // content
        String,         // created_at
        Option<String>, // annotations (assistant)
        Option<String>, // events (assistant)
        Option<String>, // files (user)
        Option<String>, // workflow (user)
        Option<String>, // template (user)
    );
    let msg_rows: Vec<MsgRow> = sqlx::query_as(
        "SELECT id, role, content, created_at, annotations, events, files, workflow, template \
         FROM messages WHERE chat_id = ? ORDER BY created_at ASC",
    )
    .bind(&chat_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let with_annot = msg_rows
        .iter()
        .filter(|r| r.1 == "assistant" && r.4.is_some())
        .count();
    let with_events = msg_rows
        .iter()
        .filter(|r| r.1 == "assistant" && r.5.is_some())
        .count();
    let with_files = msg_rows
        .iter()
        .filter(|r| r.1 == "user" && r.6.is_some())
        .count();
    tracing::info!(
        "[chat] GET /chat/{}: {} messages total, \
         {} assistant rows w/ annotations, {} w/ persistent events, \
         {} user rows w/ persisted files",
        chat_id,
        msg_rows.len(),
        with_annot,
        with_events,
        with_files,
    );

    let messages: Vec<Value> = msg_rows
        .into_iter()
        .map(|(mid, role, content, created_at, annotations, events, files, workflow, template)| {
            let content_value = if role == "assistant" {
                let mut arr = vec![json!({
                    "type": "content",
                    "text": content.unwrap_or_default(),
                })];
                // Append persisted non-text events (today: `doc_created`)
                // so the frontend's getChat path picks them up exactly
                // like the live SSE stream — see mikeApi.ts where the
                // events array is derived from `m.content`.
                if let Some(stored) = events.as_deref()
                    && let Ok(Value::Array(items)) = serde_json::from_str::<Value>(stored)
                {
                    arr.extend(items);
                }
                Value::Array(arr)
            } else {
                json!(content.unwrap_or_default())
            };
            // Hydrate annotations the same way the live SSE event does,
            // so the chat-history loader path delivers identical shape.
            // Re-apply `strip_page_markers` to each KB quote: rows
            // persisted before that fix landed contain the literal
            // `[Page N]` markers that PDF.js can't match — sanitising
            // on read makes old chats render correctly without a
            // destructive migration.
            let annotations_value = annotations
                .as_deref()
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                .map(sanitise_annotations_quotes)
                .unwrap_or_else(|| Value::Array(Vec::new()));
            // User-side metadata — files / workflow / template (see
            // migration 0021). Parse-on-read: if the stored JSON is
            // corrupt the field is dropped silently, the user message
            // still renders as plain text.
            let mut entry = json!({
                "id": mid,
                "role": role,
                "content": content_value,
                "created_at": created_at,
                "annotations": annotations_value,
            });
            if role == "user" {
                if let Some(v) = files
                    .as_deref()
                    .and_then(|s| serde_json::from_str::<Value>(s).ok())
                {
                    entry["files"] = v;
                }
                if let Some(v) = workflow
                    .as_deref()
                    .and_then(|s| serde_json::from_str::<Value>(s).ok())
                {
                    entry["workflow"] = v;
                }
                if let Some(v) = template
                    .as_deref()
                    .and_then(|s| serde_json::from_str::<Value>(s).ok())
                {
                    entry["template"] = v;
                }
            }
            entry
        })
        .collect();

    Ok(Json(json!({
        "chat": {
            "id": chat_id,
            "user_id": user_id,
            "project_id": project_id,
            "title": title,
            "updated_at": updated_at,
        },
        "messages": messages,
    })))
}

// ---------------------------------------------------------------------------
// PATCH /chat/:id  — update title
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct PatchChatBody {
    title: Option<String>,
}

async fn patch_chat(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<PatchChatBody>,
) -> ApiResult {
    let result = sqlx::query(
        "UPDATE chats SET title = COALESCE(?, title), updated_at = datetime('now') \
         WHERE id = ? AND user_id = ?",
    )
    .bind(&body.title)
    .bind(&id)
    .bind(&auth.user_id)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    if result.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "Chat not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// DELETE /chat/:id
// ---------------------------------------------------------------------------
async fn delete_chat(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    // Snapshot the cache-keyed paths of every doc linked to this chat
    // BEFORE the FK cascade (migration 0013) wipes the rows. We need
    // both storage_path (binary) and extracted_text_path so the
    // ref-count check can free the right files.
    let docs_to_check: Vec<(String, Option<String>, Option<String>, Option<String>)> =
        sqlx::query_as(
            "SELECT id, storage_path, extracted_text_path, content_hash \
             FROM documents WHERE chat_id = ? AND user_id = ?",
        )
        .bind(&id)
        .bind(&auth.user_id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

    let result = sqlx::query("DELETE FROM chats WHERE id = ? AND user_id = ?")
        .bind(&id)
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    if result.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "Chat not found"));
    }

    // FK cascade has already removed every documents row that pointed
    // at this chat. Two cleanup paths now:
    //
    //   - **Hash-keyed cache uploads** (`content_hash IS NOT NULL`,
    //     used by chat composer attachments) — ref-count check before
    //     deleting bytes: a hash shared with another chat keeps its
    //     files alive.
    //   - **Generated documents** (`content_hash IS NULL`, written by
    //     `exec_generate_docx`) — each has a unique
    //     `documents/<user_id>/<doc_id>` storage path, no dedup is
    //     possible, so the file is freed unconditionally.
    //
    // Without this second branch generated `.docx`s would be left
    // dangling on disk when the originating chat is deleted, slowly
    // bloating `data/storage/`.
    if !docs_to_check.is_empty() {
        if let Ok(storage) = make_storage() {
            let mut seen_hashes: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            let mut generated_swept = 0usize;
            for (doc_id, sp, txt, hash) in &docs_to_check {
                match hash.as_ref() {
                    Some(hash) => {
                        if !seen_hashes.insert(hash.clone()) {
                            continue;
                        }
                        let still_referenced: Option<(i64,)> = sqlx::query_as(
                            "SELECT 1 FROM documents WHERE content_hash = ? LIMIT 1",
                        )
                        .bind(hash)
                        .fetch_optional(&state.db)
                        .await
                        .unwrap_or(None);
                        if still_referenced.is_some() {
                            tracing::info!(
                                "[chat] keeping cache files for hash {} \
                                 (still referenced by another doc)",
                                hash
                            );
                            continue;
                        }
                        if let Some(key) = sp.as_ref() {
                            if let Err(e) = storage.delete(key).await {
                                tracing::warn!(
                                    "[chat] failed to delete cache binary {} (doc {}): {}",
                                    key,
                                    doc_id,
                                    e
                                );
                            }
                        }
                        if let Some(key) = txt.as_ref() {
                            if let Err(e) = storage.delete(key).await {
                                tracing::warn!(
                                    "[chat] failed to delete cache text {} (doc {}): {}",
                                    key,
                                    doc_id,
                                    e
                                );
                            }
                        }
                    }
                    None => {
                        // Generated doc — storage path is unique per
                        // doc_id, no other row points at it, free
                        // unconditionally.
                        if let Some(key) = sp.as_ref() {
                            if let Err(e) = storage.delete(key).await {
                                tracing::warn!(
                                    "[chat] failed to delete generated doc binary {} (doc {}): {}",
                                    key,
                                    doc_id,
                                    e
                                );
                            } else {
                                generated_swept += 1;
                            }
                        }
                    }
                }
            }
            tracing::info!(
                "[chat] delete chat={} swept {} doc row(s) \
                 ({} unique cache hash(es), {} generated doc(s))",
                id,
                docs_to_check.len(),
                seen_hashes.len(),
                generated_swept,
            );
        }
    }

    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// GET /chat/:id/messages
// ---------------------------------------------------------------------------
/// Back-fill `document_id`/`filename` on citation annotations whose
/// `doc_id` is a chat-local `doc-N` label but whose real document link
/// was never resolved at write time. This happens on follow-up turns of
/// a reopened chat: the frontend no longer carries the attachment in the
/// payload, so the live citation enrichment had no label→UUID map and
/// persisted `document_id: null`. The document itself is never lost —
/// it stays linked to the chat via `documents.chat_id` — so resolving it
/// on read makes the viewer work again. `chat_docs` is the chat's
/// attached documents ordered as `doc-0`, `doc-1`, ….
/// Rewrite annotations whose `path` is the upstream URL of a corpus
/// document back to the local cache-file path the viewer can fetch via
/// `/sync/kb-doc`. Older indexing runs (pre eurlex.rs:462 fix) stored
/// the EUR-Lex URL as `doc_chunks.source_path`; that URL got persisted
/// into `messages.annotations[].path`. The hot fix at write time
/// remaps new citations, but persisted ones from old chats still carry
/// the URL — so we apply the same remap on read.
///
/// `corpus_local_path_by_docid` maps a `document_id` to the absolute
/// on-disk path of its cached binary (storage root joined with the
/// row's `storage_path`). Built once per `get_messages` call from
/// `documents` rows that belong to a corpus.
fn remap_url_annotation_paths(
    mut value: Value,
    corpus_local_path_by_docid: &HashMap<String, String>,
) -> Value {
    if corpus_local_path_by_docid.is_empty() {
        return value;
    }
    let cits = if value.is_array() {
        value.as_array_mut()
    } else {
        value.get_mut("citations").and_then(|v| v.as_array_mut())
    };
    let Some(cits) = cits else {
        return value;
    };
    for c in cits.iter_mut() {
        let Some(obj) = c.as_object_mut() else { continue };
        let path = obj.get("path").and_then(|v| v.as_str()).unwrap_or("");
        if !(path.starts_with("http://") || path.starts_with("https://")) {
            continue;
        }
        let doc_uuid = obj
            .get("document_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if doc_uuid.is_empty() {
            continue;
        }
        if let Some(local) = corpus_local_path_by_docid.get(doc_uuid) {
            tracing::info!(
                "[chat] get_messages: remapping URL path → local storage path for doc {:?}",
                doc_uuid
            );
            obj.insert("path".to_string(), Value::String(local.clone()));
        }
    }
    value
}

fn enrich_doc_citations(mut value: Value, chat_docs: &[(String, String)]) -> Value {
    if chat_docs.is_empty() {
        return value;
    }
    // Annotations are persisted either as a bare array of citation
    // objects or as an object with a `citations` array — handle both.
    let cits = if value.is_array() {
        value.as_array_mut()
    } else {
        value.get_mut("citations").and_then(|v| v.as_array_mut())
    };
    let Some(cits) = cits else {
        return value;
    };
    for c in cits.iter_mut() {
        let Some(obj) = c.as_object_mut() else { continue };
        let resolved = obj
            .get("document_id")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty());
        if resolved {
            continue;
        }
        let idx = obj
            .get("doc_id")
            .and_then(|v| v.as_str())
            .and_then(|s| s.strip_prefix("doc-"))
            .and_then(|n| n.parse::<usize>().ok());
        let Some((doc_id, filename)) = idx.and_then(|i| chat_docs.get(i)) else {
            continue;
        };
        obj.insert("document_id".to_string(), Value::String(doc_id.clone()));
        let has_name = obj
            .get("filename")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty());
        if !has_name {
            obj.insert("filename".to_string(), Value::String(filename.clone()));
        }
    }
    value
}

async fn get_messages(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    // Verify ownership
    let exists: Option<(String,)> =
        sqlx::query_as("SELECT id FROM chats WHERE id = ? AND user_id = ?")
            .bind(&id)
            .bind(&auth.user_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    exists.ok_or_else(|| err(StatusCode::NOT_FOUND, "Chat not found"))?;

    // Documents attached to this chat, ordered as doc-0, doc-1, … so a
    // `doc-N` label maps back to a real document even on turns whose
    // citation links were never resolved at write time.
    let chat_docs: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, filename FROM documents WHERE chat_id = ? AND user_id = ? \
         ORDER BY created_at ASC",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    // Mapping `document_id → absolute local path` for the user's corpus
    // docs. Used to rewrite annotations whose persisted `path` is the
    // upstream URL — old chats stored those before the eurlex.rs fix.
    let mut corpus_local_path_by_docid: HashMap<String, String> = HashMap::new();
    {
        let storage_root = std::path::PathBuf::from(
            std::env::var("STORAGE_PATH")
                .unwrap_or_else(|_| "./data/storage".to_string()),
        );
        if let Ok(rows) = sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT id, storage_path FROM documents \
             WHERE user_id = ? AND corpus_id IS NOT NULL AND storage_path IS NOT NULL",
        )
        .bind(&auth.user_id)
        .fetch_all(&state.db)
        .await
        {
            for (doc_uuid, sp_opt) in rows {
                if let Some(sp) = sp_opt {
                    let abs = storage_root
                        .join(sp.replace('/', std::path::MAIN_SEPARATOR_STR));
                    corpus_local_path_by_docid
                        .insert(doc_uuid, abs.to_string_lossy().to_string());
                }
            }
        }
    }

    let rows: Vec<(
        String,         // id
        String,         // role
        Option<String>, // content
        String,         // created_at
        Option<String>, // annotations
        Option<String>, // events (assistant — persisted doc_created etc.)
    )> = sqlx::query_as(
        "SELECT id, role, content, created_at, annotations, events FROM messages \
         WHERE chat_id = ? ORDER BY created_at ASC",
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let with_annot = rows
        .iter()
        .filter(|(_, role, _, _, ann, _)| role == "assistant" && ann.is_some())
        .count();
    let with_events = rows
        .iter()
        .filter(|(_, role, _, _, _, ev)| role == "assistant" && ev.is_some())
        .count();
    tracing::info!(
        "[chat] GET /chat/{}/messages: {} rows total, {} w/ annotations, {} w/ events",
        id,
        rows.len(),
        with_annot,
        with_events,
    );

    let messages: Vec<Value> = rows
        .into_iter()
        .map(|(id, role, content, created_at, annotations, events)| {
            // Hydrate annotations from the stored JSON. When the column
            // is NULL — older turns from before migration 0012, or
            // turns where the live pass dropped the citations (e.g.
            // <CITATIONS> block truncated without `</CITATIONS>` before
            // the truncation-tolerant parser shipped) — re-parse the
            // persisted content with the current (smarter) extractor.
            // This retroactively re-renders pills on chats that broke
            // silently in earlier builds.
            let annotations_value = annotations
                .as_deref()
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                .or_else(|| {
                    content.as_deref().and_then(extract_citations_block)
                })
                .map(|v| enrich_doc_citations(v, &chat_docs))
                .map(|v| remap_url_annotation_paths(v, &corpus_local_path_by_docid))
                .unwrap_or_else(|| Value::Array(Vec::new()));
            // Persisted non-text events (today: `doc_created`). Without
            // hydrating them here the download cards for generated
            // docx/xlsx vanish on chat reload — they only existed in
            // the live SSE stream, never in the prose. Returning the
            // raw array lets the frontend turn them into steps in the
            // same shape the live `onDocCreated` callback produces.
            let events_value = events
                .as_deref()
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                .unwrap_or_else(|| Value::Array(Vec::new()));
            json!({
                "id": id,
                "role": role,
                "content": content,
                "created_at": created_at,
                "annotations": annotations_value,
                "events": events_value,
            })
        })
        .collect();

    Ok(Json(json!({ "messages": messages })))
}

// ---------------------------------------------------------------------------
// GET /chat/:id/documents
// ---------------------------------------------------------------------------
/// Enumerate **every** document a chat has interacted with, across five
/// categories the chat archive is expected to retain (per the explicit
/// "chat must persist its full document list" requirement):
///
/// 1. **Uploaded** — composer attachments. `documents.chat_id = ?` AND
///    `content_hash IS NOT NULL` (cache-keyed uploads always have a hash).
/// 2. **Generated** — `generate_docx` outputs. `chat_id = ?` AND
///    `content_hash IS NULL` (generated docs are not hash-deduped).
/// 3. **Rejected** — not a separate origin; it's any (1)/(2) row with
///    `decision='rejected'`. Stored on the same row so re-accept restores
///    the original document without re-uploading.
/// 4. **Referenced** — KB / corpora documents cited by the assistant.
///    Live in `documents` with `chat_id IS NULL` and a `corpus_id`; the
///    link to the chat is the doc-id buried in `messages.annotations`
///    (citation JSON). We parse those JSON blobs and look the IDs up.
/// 5. **Project** — when the chat is in a project (`chats.project_id IS
///    NOT NULL`), every doc with `documents.project_id = chats.project_id`
///    is reachable as context regardless of whether a specific message
///    pulled it in.
///
/// Origin precedence on overlap: chat_id (uploaded/generated) > project >
/// referenced. A doc that was both uploaded AND cited stays "uploaded";
/// a project doc that was also cited stays "project". This favours the
/// most permanent / direct relationship so the UI label matches the
/// user's mental model.
///
/// Decision columns ride along on every row so the popover can paint
/// strikethrough + `Rifiutato` badge without an N+1 fan-out across
/// `GET /document/:id`.
async fn get_chat_documents(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    // Ownership check + grab the chat's project_id in one round-trip so a
    // project-scoped chat can pull in inherited docs (category 5).
    let chat_row: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT id, project_id FROM chats WHERE id = ? AND user_id = ?",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let Some((_, project_id)) = chat_row else {
        return Err(err(StatusCode::NOT_FOUND, "chat not found"));
    };

    // Walk every assistant message's persisted annotations JSON and
    // collect any `document_id` that points outside the chat's own docs.
    // These are the KB / corpus citations the user saw rendered as
    // `[gN]` / `[pN]` pills — the chat must remember them even though
    // they live elsewhere in the table.
    let annot_rows: Vec<(Option<String>,)> = sqlx::query_as(
        "SELECT annotations FROM messages \
         WHERE chat_id = ? AND role = 'assistant' AND annotations IS NOT NULL",
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let mut referenced_ids: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for (raw,) in annot_rows {
        let Some(json_str) = raw else { continue };
        let Ok(val) = serde_json::from_str::<Value>(&json_str) else {
            continue;
        };
        // Annotations are persisted either as a bare array or an object
        // with a `citations` array — match `enrich_doc_citations` above.
        let cits = if val.is_array() {
            val.as_array().cloned().unwrap_or_default()
        } else {
            val.get("citations")
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default()
        };
        for c in cits {
            if let Some(doc_id) = c.get("document_id").and_then(|v| v.as_str()) {
                if !doc_id.is_empty() {
                    referenced_ids.insert(doc_id.to_string());
                }
            }
        }
    }

    // Insertion-ordered accumulator: chat_id rows first (creation order),
    // then project, then referenced. Skips any duplicate id so the
    // precedence rule above is enforced by first-write-wins.
    let mut out: Vec<Value> = Vec::new();
    let mut seen: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    // (1) + (2) + (3) — chat-linked rows. `content_hash` discriminates
    // uploaded (Some) from generated (None); decision is on the same row.
    let chat_rows: Vec<(
        String,         // id
        String,         // filename
        String,         // file_type
        String,         // decision
        Option<String>, // decision_reason
        Option<String>, // decision_summary
        Option<String>, // content_hash
    )> = sqlx::query_as(
        "SELECT id, filename, file_type, decision, decision_reason, decision_summary, content_hash \
         FROM documents \
         WHERE chat_id = ? AND user_id = ? \
         ORDER BY created_at ASC",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    for (doc_id, filename, file_type, decision, reason, summary, content_hash) in chat_rows {
        let origin = if content_hash.is_some() { "uploaded" } else { "generated" };
        seen.insert(doc_id.clone());
        out.push(json!({
            "id": doc_id,
            "filename": filename,
            "file_type": file_type,
            "decision": decision,
            "decision_reason": reason,
            "decision_summary": summary,
            "origin": origin,
        }));
    }

    // (5) — project-inherited. Only when the chat lives in a project.
    if let Some(pid) = project_id.as_deref() {
        let proj_rows: Vec<(
            String,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
        )> = sqlx::query_as(
            "SELECT id, filename, file_type, decision, decision_reason, decision_summary \
             FROM documents \
             WHERE project_id = ? AND user_id = ? \
             ORDER BY created_at ASC",
        )
        .bind(pid)
        .bind(&auth.user_id)
        .fetch_all(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

        for (doc_id, filename, file_type, decision, reason, summary) in proj_rows {
            if seen.contains(&doc_id) {
                continue; // already in as uploaded/generated
            }
            seen.insert(doc_id.clone());
            out.push(json!({
                "id": doc_id,
                "filename": filename,
                "file_type": file_type,
                "decision": decision,
                "decision_reason": reason,
                "decision_summary": summary,
                "origin": "project",
            }));
        }
    }

    // (4) — KB / corpora references gleaned from annotations. We bind
    // each id individually because sqlx::query_as doesn't expand a Vec
    // into placeholders, and SQLite's IN-list size is fine for any
    // realistic chat (chats with >999 distinct citations don't exist).
    let unseen_refs: Vec<String> = referenced_ids
        .into_iter()
        .filter(|i| !seen.contains(i))
        .collect();
    if !unseen_refs.is_empty() {
        let placeholders = unseen_refs.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, filename, file_type, decision, decision_reason, decision_summary \
             FROM documents \
             WHERE user_id = ? AND id IN ({placeholders})",
        );
        let mut q = sqlx::query_as::<_, (
            String,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
        )>(&sql)
        .bind(&auth.user_id);
        for rid in &unseen_refs {
            q = q.bind(rid);
        }
        let ref_rows = q
            .fetch_all(&state.db)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

        for (doc_id, filename, file_type, decision, reason, summary) in ref_rows {
            seen.insert(doc_id.clone());
            out.push(json!({
                "id": doc_id,
                "filename": filename,
                "file_type": file_type,
                "decision": decision,
                "decision_reason": reason,
                "decision_summary": summary,
                "origin": "referenced",
            }));
        }
    }

    tracing::info!(
        "[chat] GET /chat/{}/documents: {} rows ({} chat-linked, project={}, refs={})",
        id,
        out.len(),
        seen.len(),
        project_id.is_some(),
        out.iter()
            .filter(|d| d.get("origin").and_then(|v| v.as_str()) == Some("referenced"))
            .count(),
    );

    Ok(Json(json!({ "documents": out })))
}

// ---------------------------------------------------------------------------
// POST /chat/:id/message  — SSE streaming
// Body: { content, model?, system_prompt? }
// Response: text/event-stream with delta/done events
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct PostMessageBody {
    content: String,
    model: Option<String>,
    system_prompt: Option<String>,
}

async fn post_message(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(chat_id): Path<String>,
    Json(body): Json<PostMessageBody>,
) -> Response {
    // Verify ownership
    let exists: Option<(String,)> =
        sqlx::query_as("SELECT id FROM chats WHERE id = ? AND user_id = ?")
            .bind(&chat_id)
            .bind(&auth.user_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();

    if exists.is_none() {
        return (StatusCode::NOT_FOUND, Json(json!({"detail": "Chat not found"}))).into_response();
    }

    // Persist user message
    let user_msg_id = uuid::Uuid::new_v4().to_string();
    if let Err(e) = sqlx::query(
        "INSERT INTO messages (id, chat_id, role, content) VALUES (?, ?, 'user', ?)",
    )
    .bind(&user_msg_id)
    .bind(&chat_id)
    .bind(&body.content)
    .execute(&state.db)
    .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"detail": e.to_string()})),
        )
            .into_response();
    }

    // Load conversation history (last 50 messages)
    let history: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT role, content FROM messages WHERE chat_id = ? ORDER BY created_at ASC LIMIT 50")
            .bind(&chat_id)
            .fetch_all(&state.db)
            .await
            .unwrap_or_default();

    let messages: Vec<Message> = history
        .into_iter()
        .filter_map(|(role, content)| {
            let r = match role.as_str() {
                "user" => Role::User,
                "assistant" => Role::Assistant,
                "tool" => Role::Tool,
                _ => return None,
            };
            Some(Message { role: r, content: content.unwrap_or_default(), images: vec![], tool_calls: vec![], tool_call_id: None, tool_name: None })
        })
        .collect();

    // Resolve model from request or user settings
    let user_settings = fetch_llm_settings(&state.db, &auth.user_id)
        .await
        .ok();

    let raw_model = body.model.clone().unwrap_or_else(|| {
        user_settings
            .as_ref()
            .and_then(|s| s.main_model.clone())
            .unwrap_or_else(|| "gemini-3.5-flash".to_string())
    });
    let model = raw_model.clone();

    // Build per-provider config from saved settings.
    let local_config = build_local_config(&model, user_settings.as_ref());

    let system_prompt = body.system_prompt.unwrap_or_default();

    let params = StreamParams {
        model: model.clone(),
        system_prompt,
        system_volatile: String::new(),
        messages,
        tools: vec![],
        max_iterations: 1,
        enable_thinking: false,
        local_config,
        claude_api_key: user_settings.as_ref().and_then(|s| s.claude_api_key.clone()),
        gemini_api_key: user_settings.as_ref().and_then(|s| s.gemini_api_key.clone()),
        gemini_region: user_settings.as_ref().and_then(|s| s.gemini_region.clone()),
        // This is the non-chat endpoint (no Mistral cache key
        // benefit on one-shot calls); pass through the route's
        // chat_id anyway so future-Mistral can opt-in.
        chat_id: Some(chat_id.clone()),
        mistral_opts: build_mistral_opts(&model, user_settings.as_ref()),
    };

    // SSE stream
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(64);
    let state_clone = state.clone();
    let chat_id_clone = chat_id.clone();

    tokio::spawn(async move {
        let mut full_response = String::new();

        match llm::stream_chat(params).await {
            Err(e) => {
                let _ = tx
                    .send(Ok(Event::default().event("error").data(e.to_string())))
                    .await;
            }
            Ok(mut stream) => {
                while let Some(event) = stream.next().await {
                    match event {
                        Ok(StreamEvent::ContentDelta(text)) => {
                            full_response.push_str(&text);
                            let data = serde_json::to_string(&json!({ "delta": text }))
                                .unwrap_or_default();
                            if tx.send(Ok(Event::default().event("delta").data(data))).await.is_err() {
                                break;
                            }
                        }
                        Ok(StreamEvent::Done) | Err(_) => break,
                        _ => {}
                    }
                }

                // Persist assistant message
                let asst_msg_id = uuid::Uuid::new_v4().to_string();
                let _ = sqlx::query(
                    "INSERT INTO messages (id, chat_id, role, content) VALUES (?, ?, 'assistant', ?)",
                )
                .bind(&asst_msg_id)
                .bind(&chat_id_clone)
                .bind(&full_response)
                .execute(&state_clone.db)
                .await;

                // Update chat timestamp
                let _ = sqlx::query(
                    "UPDATE chats SET updated_at = datetime('now') WHERE id = ?",
                )
                .bind(&chat_id_clone)
                .execute(&state_clone.db)
                .await;

                let done_data = serde_json::to_string(&json!({ "message_id": asst_msg_id }))
                    .unwrap_or_default();
                let _ = tx.send(Ok(Event::default().event("done").data(done_data))).await;
            }
        }
    });

    let sse_stream = ReceiverStream::new(rx);
    Sse::new(sse_stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

// ---------------------------------------------------------------------------
// POST /chat/:id/generate-title — short title from first user message
// ---------------------------------------------------------------------------
async fn generate_title(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(chat_id): Path<String>,
) -> ApiResult {
    let owns: Option<(String,)> = sqlx::query_as("SELECT id FROM chats WHERE id = ? AND user_id = ?")
        .bind(&chat_id)
        .bind(&auth.user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    if owns.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "Chat not found"));
    }

    let first: Option<(String,)> = sqlx::query_as(
        "SELECT content FROM messages WHERE chat_id = ? AND role = 'user' \
         ORDER BY created_at ASC LIMIT 1",
    )
    .bind(&chat_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let Some((first_msg,)) = first else {
        return Ok(Json(json!({ "title": null })));
    };

    let user_settings = fetch_llm_settings(&state.db, &auth.user_id).await.ok();
    // Pick a model from user settings — prefer the active provider, then any
    // configured one. Falling back to Gemini default fails when the user only
    // has a Local/OpenAI key set, so try to match what the chat is actually using.
    //
    // Crucially every candidate model must have its endpoint/key configured —
    // otherwise we'd happily pick `local:llama3.2:3b` only to 502 because the
    // user never wrote a localBaseUrl.
    let is_usable = |m: &str, s: &crate::routes::user::LlmSettings| -> bool {
        if let Some(rest) = m.strip_prefix("local:") {
            return !rest.is_empty()
                && s.local_base_url
                    .as_deref()
                    .map(|x| !x.trim().is_empty())
                    .unwrap_or(false);
        }
        if let Some(rest) = m.strip_prefix("openai:") {
            return !rest.is_empty()
                && s.openai_api_key
                    .as_deref()
                    .map(|x| !x.trim().is_empty())
                    .unwrap_or(false);
        }
        if let Some(rest) = m.strip_prefix("mistral:") {
            return !rest.is_empty()
                && s.mistral_api_key
                    .as_deref()
                    .map(|x| !x.trim().is_empty())
                    .unwrap_or(false);
        }
        if m.starts_with("claude") {
            return s
                .claude_api_key
                .as_deref()
                .map(|x| !x.trim().is_empty())
                .unwrap_or(false);
        }
        if m.starts_with("gemini") {
            return s
                .gemini_api_key
                .as_deref()
                .map(|x| !x.trim().is_empty())
                .unwrap_or(false);
        }
        false
    };
    let title_model = user_settings
        .as_ref()
        .and_then(|s| s.title_model.clone().filter(|m| is_usable(m, s)))
        .or_else(|| {
            user_settings
                .as_ref()
                .and_then(|s| s.main_model.clone().filter(|m| is_usable(m, s)))
        })
        .or_else(|| {
            user_settings.as_ref().and_then(|s| match s.active_provider.as_deref() {
                // For local/openai also require the corresponding endpoint
                // / API key to be configured — otherwise we'd pick a model
                // that has no way to be reached and the title generation
                // would 502.
                Some("local") => match (&s.local_model, &s.local_base_url) {
                    (Some(m), Some(b)) if !b.trim().is_empty() => Some(format!("local:{m}")),
                    _ => None,
                },
                Some("openai") => match (&s.openai_model, &s.openai_api_key) {
                    (Some(m), Some(k)) if !k.trim().is_empty() => Some(format!("openai:{m}")),
                    _ => None,
                },
                Some("mistral") => match (&s.mistral_model, &s.mistral_api_key) {
                    (Some(m), Some(k)) if !k.trim().is_empty() => Some(format!("mistral:{m}")),
                    _ => None,
                },
                Some("claude") => s
                    .claude_api_key
                    .as_ref()
                    .filter(|k| !k.trim().is_empty())
                    .map(|_| "claude-sonnet-4-6".to_string()),
                Some("gemini") => s
                    .gemini_api_key
                    .as_ref()
                    .filter(|k| !k.trim().is_empty())
                    .map(|_| "gemini-3.5-flash".to_string()),
                _ => None,
            })
        })
        .or_else(|| {
            // No active_provider — pick first configured.
            let s = user_settings.as_ref()?;
            if let Some(m) = &s.local_model {
                if s.local_base_url.is_some() {
                    return Some(format!("local:{m}"));
                }
            }
            if let Some(m) = &s.openai_model {
                if s.openai_api_key.is_some() {
                    return Some(format!("openai:{m}"));
                }
            }
            if let Some(m) = &s.mistral_model {
                if s.mistral_api_key.is_some() {
                    return Some(format!("mistral:{m}"));
                }
            }
            if s.claude_api_key.is_some() { return Some("claude-sonnet-4-6".to_string()); }
            if s.gemini_api_key.is_some() { return Some("gemini-3.5-flash".to_string()); }
            None
        })
        .unwrap_or_else(|| "gemini-3.5-flash".to_string());

    tracing::info!("[chat] generate_title using model={title_model}");

    let local_config = build_local_config(&title_model, user_settings.as_ref());

    let prompt = prompts::title_prompt(&first_msg);

    let params = StreamParams {
        model: title_model.clone(),
        system_prompt: String::new(),
        system_volatile: String::new(),
        messages: vec![Message::user(prompt)],
        tools: vec![],
        max_iterations: 1,
        enable_thinking: false,
        local_config,
        claude_api_key: user_settings.as_ref().and_then(|s| s.claude_api_key.clone()),
        gemini_api_key: user_settings.as_ref().and_then(|s| s.gemini_api_key.clone()),
        gemini_region: user_settings.as_ref().and_then(|s| s.gemini_region.clone()),
        // Title generation is one-shot — no Mistral cache benefit.
        chat_id: None,
        mistral_opts: build_mistral_opts(&title_model, user_settings.as_ref()),
    };

    let title_text = match llm::provider_for_model(&title_model) {
        llm::Provider::Claude => llm::claude::complete(params).await,
        llm::Provider::OpenAI => llm::local::complete(params).await,
        llm::Provider::Gemini => llm::gemini::complete(params).await,
        llm::Provider::Mistral => llm::mistral::complete(params).await,
    }
    .map_err(|e| err(StatusCode::BAD_GATEWAY, &e.to_string()))?;

    let mut title: String = title_text
        .lines()
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| c == '"' || c == '\'' || c.is_whitespace())
        .chars()
        .take(80)
        .collect();

    if title.trim().is_empty() {
        let fallback_words: Vec<&str> = first_msg
            .split_whitespace()
            .take(5)
            .collect();
        let fallback = fallback_words.join(" ");
        title = fallback
            .trim_matches(|c: char| c == '"' || c == '\'' || c.is_whitespace())
            .chars()
            .take(80)
            .collect();
        if title.trim().is_empty() {
            title = "New chat".to_string();
        }
    }

    sqlx::query("UPDATE chats SET title = ?, updated_at = datetime('now') WHERE id = ?")
        .bind(&title)
        .bind(&chat_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({ "title": title })))
}

#[cfg(test)]
mod tests {
    use super::{
        canonical_corpus_key, enrich_doc_citations, extract_citations_block,
        extract_inline_docid_refs, extract_inline_paren_doc_refs,
        register_generated_document, rewrite_inline_docid_citations,
        sanitise_annotations_quotes, split_hybrid_citation_brackets, strip_page_markers,
        tool_step_event,
    };
    use serde_json::{json, Value};
    use std::collections::HashMap;

    #[test]
    fn generated_document_gets_the_next_label_and_is_citable() {
        let mut labels: HashMap<String, String> = HashMap::new();
        let result = json!({"doc_id": "1f4c", "filename": "Contratto.docx", "note": "Persisted."});
        let annotated =
            register_generated_document("generate_docx", &result.to_string(), &mut labels).unwrap();
        let rv: Value = serde_json::from_str(&annotated).unwrap();
        assert_eq!(rv["doc_label"], "doc-1");
        assert!(rv["note"].as_str().unwrap().contains("doc-1"));
        assert_eq!(labels.get("doc-1").map(String::as_str), Some("1f4c"));
    }

    #[test]
    fn generated_document_label_follows_the_attachments() {
        let mut labels: HashMap<String, String> =
            HashMap::from([("doc-1".to_string(), "aaa".to_string())]);
        let result = json!({"doc_id": "bbb", "filename": "B.docx"});
        let annotated =
            register_generated_document("generate_xlsx", &result.to_string(), &mut labels).unwrap();
        let rv: Value = serde_json::from_str(&annotated).unwrap();
        assert_eq!(rv["doc_label"], "doc-2");
        assert_eq!(labels.len(), 2);
    }

    #[test]
    fn generated_document_keeps_its_label_when_already_known() {
        let mut labels: HashMap<String, String> =
            HashMap::from([("doc-3".to_string(), "ccc".to_string())]);
        let result = json!({"doc_id": "ccc", "filename": "C.docx"});
        let annotated =
            register_generated_document("generate_docx", &result.to_string(), &mut labels).unwrap();
        let rv: Value = serde_json::from_str(&annotated).unwrap();
        assert_eq!(rv["doc_label"], "doc-3");
        assert_eq!(labels.len(), 1);
    }

    #[test]
    fn failed_generation_gets_no_label() {
        let mut labels: HashMap<String, String> = HashMap::new();
        let result = json!({"error": "storage write: disk full"});
        assert!(register_generated_document("generate_docx", &result.to_string(), &mut labels)
            .is_none());
        assert!(labels.is_empty());
        // Other tools never create a label either.
        let read = json!({"doc_id": "doc-1", "filename": "A.docx"});
        assert!(
            register_generated_document("read_document", &read.to_string(), &mut labels).is_none()
        );
        assert!(labels.is_empty());
    }

    #[test]
    fn edited_document_gets_a_download_card() {
        let result = json!({
            "doc_id": "doc-2",
            "document_id": "4a7e",
            "filename": "Contratto.docx",
            "edits_applied": [{"find": "a", "replace": "b", "hits": 1}],
        });
        let event = tool_step_event("edit_document", &result.to_string()).unwrap();
        assert_eq!(event["type"], "doc_created");
        assert_eq!(event["document_id"], "4a7e");
        assert_eq!(event["download_url"], "/document/4a7e/download");
    }

    #[test]
    fn sanitise_annotations_quotes_strips_each_entry() {
        let input = json!([
            { "doc_id": "g1", "quote": "[Page 1]\nFirst quote", "page": 1 },
            { "doc_id": "g2", "quote": "Plain quote", "page": 2 },
            { "doc_id": "g3", "quote": "[Page 3] Mid [Page 5] tail", "page": 3 },
        ]);
        let out = sanitise_annotations_quotes(input);
        let arr = out.as_array().expect("array");
        assert_eq!(arr[0]["quote"], "First quote");
        assert_eq!(arr[1]["quote"], "Plain quote");
        assert_eq!(arr[2]["quote"], "Mid tail");
    }

    #[test]
    fn sanitise_annotations_quotes_passes_non_array_through() {
        let v = json!({ "not": "array" });
        assert_eq!(sanitise_annotations_quotes(v.clone()), v);
    }

    #[test]
    fn sanitise_annotations_quotes_preserves_other_fields() {
        let input = json!([{
            "doc_id": "g1",
            "quote": "[Page 1]\ntext",
            "page": 1,
            "source": "kb",
            "scope": "global",
            "filename": "a.pdf",
        }]);
        let out = sanitise_annotations_quotes(input);
        let obj = out.as_array().unwrap()[0].as_object().unwrap();
        assert_eq!(obj["quote"], Value::String("text".to_string()));
        assert_eq!(obj["source"], "kb");
        assert_eq!(obj["scope"], "global");
        assert_eq!(obj["filename"], "a.pdf");
        assert_eq!(obj["page"], 1);
    }

    #[test]
    fn strip_page_markers_drops_leading_marker() {
        let q = "[Page 1]\nModello [2026] per la Valutazione…";
        assert_eq!(
            strip_page_markers(q),
            "Modello [2026] per la Valutazione…"
        );
    }

    #[test]
    fn strip_page_markers_drops_inline_marker() {
        let q = "qualcosa qui [Page 5] e qualcosa lì";
        assert_eq!(
            strip_page_markers(q),
            "qualcosa qui e qualcosa lì"
        );
    }

    #[test]
    fn strip_page_markers_handles_multi_digit() {
        let q = "[Page 123]\ntesto pagina centoventitré";
        assert_eq!(strip_page_markers(q), "testo pagina centoventitré");
    }

    #[test]
    fn strip_page_markers_preserves_other_brackets() {
        // Real document brackets like [2026] or [art. 5] must survive.
        let q = "Articolo [art. 5] del 2026 [2026]";
        assert_eq!(strip_page_markers(q), q);
    }

    #[test]
    fn strip_page_markers_preserves_non_marker_text() {
        let q = "Plain quote with no markers at all.";
        assert_eq!(strip_page_markers(q), q);
    }

    #[test]
    fn strip_page_markers_handles_multiple_markers() {
        let q = "[Page 1]\nfoo [Page 2]\nbar";
        assert_eq!(strip_page_markers(q), "foo bar");
    }

    #[test]
    fn extracts_plain_block() {
        let text = "Some answer.\n<CITATIONS>[{\"doc\":\"a\",\"page\":1}]</CITATIONS>";
        let v = extract_citations_block(text).unwrap();
        assert_eq!(v, json!([{"doc":"a","page":1}]));
    }

    #[test]
    fn extracts_block_with_code_fence() {
        let text = "Answer.\n<CITATIONS>\n```json\n[{\"x\":1}]\n```\n</CITATIONS>";
        let v = extract_citations_block(text).unwrap();
        assert_eq!(v, json!([{"x":1}]));
    }

    #[test]
    fn case_insensitive_tag() {
        let text = "<citations>[]</citations>";
        let v = extract_citations_block(text).unwrap();
        assert_eq!(v, json!([]));
    }

    #[test]
    fn returns_none_for_no_block() {
        assert!(extract_citations_block("plain text").is_none());
    }

    #[test]
    fn unclosed_block_still_parses_when_inner_is_valid_json() {
        // Truncation tolerance: a JSON array that arrived without its
        // closing tag must still parse — this is the recovery path the
        // citation-rendering UI depends on for long reports.
        let v = extract_citations_block("<CITATIONS>[1,2,3]").expect("parses");
        assert_eq!(v, json!([1, 2, 3]));
    }

    #[test]
    fn returns_none_for_invalid_json() {
        assert!(extract_citations_block("<CITATIONS>not json</CITATIONS>").is_none());
    }

    #[test]
    fn extract_inline_docid_refs_picks_up_the_observed_pattern() {
        // The exact shape the model emitted on the NIS2 report turn that
        // surfaced this bug: UUID + comma + `page N` (or `page N-M`).
        let text = "**Introduzione** [doc-id: cdbe5ce0-36f1-4574-a818-64e06826e632, page 1]. \
                    Continua [doc-id: cdbe5ce0-36f1-4574-a818-64e06826e632, page 1-2] eccetera.";
        let refs = extract_inline_docid_refs(text);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].handle, "cdbe5ce0-36f1-4574-a818-64e06826e632");
        assert_eq!(refs[0].page.as_deref(), Some("1"));
        assert_eq!(refs[1].handle, "cdbe5ce0-36f1-4574-a818-64e06826e632");
        assert_eq!(refs[1].page.as_deref(), Some("1-2"));
    }

    #[test]
    fn extract_inline_docid_refs_handles_doc_n_and_pages_and_no_page() {
        // Three legitimate variants: doc-N label, `pages` plural, and no
        // page at all (model cited the document without a page).
        let text = "A [doc-id: doc-0, page 3] B [doc-id: doc-1, pages 4] C [doc-id: doc-2] D";
        let refs = extract_inline_docid_refs(text);
        assert_eq!(refs.len(), 3);
        assert_eq!((refs[0].handle.as_str(), refs[0].page.as_deref()), ("doc-0", Some("3")));
        assert_eq!((refs[1].handle.as_str(), refs[1].page.as_deref()), ("doc-1", Some("4")));
        assert_eq!(refs[2].handle.as_str(), "doc-2");
        assert!(refs[2].page.is_none());
    }

    #[test]
    fn extract_inline_docid_refs_ignores_malformed_and_unrelated_brackets() {
        // Real bracket-rich prose must not produce false positives.
        let text = "Vedi [Page 3] e [art. 5] e [doc-id]: testo. \
                    Anche [doc-id: ] vuoto e [doc-id: foo, page abc] malformato.";
        let refs = extract_inline_docid_refs(text);
        assert!(refs.is_empty(), "expected no matches, got {refs:?}");
    }

    #[test]
    fn extract_inline_paren_doc_refs_picks_up_italian_paren_form() {
        // The exact shape from the Inventario beni assicurati workflow:
        // a parenthesised italian reference, no [doc-id:] bracket.
        let text = "Veicolo coperto (Polizza n. 449435502/39, doc-1, pag. 99-101). \
                    Cfr. anche doc-0, pag 5 e doc-2, pagina 12.";
        let refs = extract_inline_paren_doc_refs(text);
        assert_eq!(refs.len(), 3);
        assert_eq!((refs[0].handle.as_str(), refs[0].page.as_deref()), ("doc-1", Some("99-101")));
        assert_eq!((refs[1].handle.as_str(), refs[1].page.as_deref()), ("doc-0", Some("5")));
        assert_eq!((refs[2].handle.as_str(), refs[2].page.as_deref()), ("doc-2", Some("12")));
        // The match must NOT consume the closing paren or trailing
        // prose — only the `doc-N, pag …` substring.
        let m0 = &text[refs[0].start..refs[0].end];
        assert_eq!(m0, "doc-1, pag. 99-101");
    }

    #[test]
    fn extract_inline_paren_doc_refs_ignores_bare_doc_n_without_page_word() {
        // `doc-1` on its own is NOT a citation marker — many prompts
        // mention the label in passing. Must require a page word.
        let text = "Il file doc-1 contiene la polizza. Cfr. doc-1 per dettagli.";
        assert!(extract_inline_paren_doc_refs(text).is_empty());
    }

    #[test]
    fn rewrite_inline_docid_citations_handles_both_shapes_together() {
        // Mixed prose: one bracket shape + two paren shapes pointing at
        // the same doc on different pages. All three must collapse onto
        // distinct cN refs and rewrite in document order.
        let text = "Cfr. [doc-id: doc-0, page 1] e poi (Polizza, doc-0, pag. 7) \
                    nonché doc-1, pag. 3.";
        let (out, cits) = rewrite_inline_docid_citations(text, |h| match h {
            "doc-0" => Some(("uuid-A".into(), "polizza.pdf".into())),
            "doc-1" => Some(("uuid-B".into(), "schedule.pdf".into())),
            _ => None,
        })
        .expect("rewrites");
        assert_eq!(out, "Cfr. [c1] e poi (Polizza, [c2]) nonché [c3].");
        let arr = cits.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["page"], 1);
        assert_eq!(arr[1]["page"], 7);
        assert_eq!(arr[1]["filename"], "polizza.pdf");
        assert_eq!(arr[2]["page"], 3);
        assert_eq!(arr[2]["filename"], "schedule.pdf");
    }

    #[test]
    fn rewrite_inline_docid_citations_collapses_repeats_into_one_ref() {
        // Two references to the same (uuid, page) MUST share a single
        // c1 in the <CITATIONS> array — keeps the block compact.
        let text = "Vedi [doc-id: doc-0, page 1] e di nuovo [doc-id: doc-0, page 1].";
        let (out, cits) = rewrite_inline_docid_citations(text, |h| {
            if h == "doc-0" {
                Some(("uuid-abc".into(), "Report.docx".into()))
            } else {
                None
            }
        })
        .expect("at least one rewrite");
        assert_eq!(out, "Vedi [c1] e di nuovo [c1].");
        let arr = cits.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["ref"], "c1");
        assert_eq!(arr[0]["doc_id"], "uuid-abc");
        assert_eq!(arr[0]["document_id"], "uuid-abc");
        assert_eq!(arr[0]["filename"], "Report.docx");
        assert_eq!(arr[0]["page"], 1);
        assert_eq!(arr[0]["source"], "attached");
    }

    #[test]
    fn rewrite_inline_docid_citations_distinct_pages_get_distinct_refs() {
        let text = "Sez. A [doc-id: doc-0, page 1]; sez. B [doc-id: doc-0, page 2].";
        let (out, cits) = rewrite_inline_docid_citations(text, |h| {
            (h == "doc-0").then(|| ("uuid-abc".into(), "Report.docx".into()))
        })
        .expect("rewrite");
        assert_eq!(out, "Sez. A [c1]; sez. B [c2].");
        let arr = cits.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["page"], 1);
        assert_eq!(arr[1]["page"], 2);
    }

    #[test]
    fn rewrite_inline_docid_citations_leaves_unresolved_handles_alone() {
        // Security: handles that don't resolve through the user's
        // documents MUST NOT be rewritten — keep the model's text and
        // emit no citation for them, so we never reveal an arbitrary
        // UUID through the viewer.
        let text = "[doc-id: doc-0, page 1] e [doc-id: fake-uuid, page 2].";
        let (out, cits) = rewrite_inline_docid_citations(text, |h| {
            (h == "doc-0").then(|| ("uuid-abc".into(), "Report.docx".into()))
        })
        .expect("rewrite");
        assert_eq!(out, "[c1] e [doc-id: fake-uuid, page 2].");
        let arr = cits.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["ref"], "c1");
    }

    #[test]
    fn rewrite_inline_docid_citations_returns_none_when_nothing_resolves() {
        let text = "Nessun riferimento qui, solo prosa.";
        assert!(rewrite_inline_docid_citations(text, |_| None).is_none());
        // Even with a [doc-id: ...] marker — if every handle is unknown
        // we must return None and let the original body stand.
        let text = "[doc-id: ignoto, page 1]";
        assert!(rewrite_inline_docid_citations(text, |_| None).is_none());
    }

    // ── split_hybrid_citation_brackets ───────────────────────────────

    #[test]
    fn split_hybrid_passes_clean_brackets_through() {
        // Pure-ref brackets must be unchanged — the frontend renders
        // them natively. Idempotency for the common case.
        let text = "Vedi [c1] e [c2, c3, c4]. Anche [g5] e [p7].";
        let out = split_hybrid_citation_brackets(text);
        assert_eq!(out.as_ref(), text);
    }

    #[test]
    fn split_hybrid_passes_non_citation_brackets_through() {
        // Brackets that contain neither refs nor doc-ish tokens stay
        // verbatim — the rewriter must not rewrite `[3]` page numbers in
        // unrelated prose, square-bracketed paraphrase, etc.
        let text = "Art. 32 [3] del decreto, articolo [vedi nota].";
        let out = split_hybrid_citation_brackets(text);
        assert_eq!(out.as_ref(), text);
    }

    #[test]
    fn split_hybrid_decomposes_ref_plus_filename_with_page() {
        let text = "[c1, c2, c18, c34, CARTELLA_TEST_002.pdf, p.4]";
        let out = split_hybrid_citation_brackets(text);
        assert_eq!(
            out.as_ref(),
            "[c1] [c2] [c18] [c34] [doc-id: CARTELLA_TEST_002.pdf, page 4]"
        );
    }

    #[test]
    fn split_hybrid_handles_bare_page_after_seen_filename() {
        // Model sometimes emits `[c1, FILE.pdf, p.4, p.6]` with two
        // pages — both should bind to the preceding filename so each
        // becomes its own doc-id marker.
        let text = "[c1, CARTELLA_TEST_002.pdf, p.4, p.6]";
        let out = split_hybrid_citation_brackets(text);
        assert_eq!(
            out.as_ref(),
            "[c1] [doc-id: CARTELLA_TEST_002.pdf, page 4] [doc-id: CARTELLA_TEST_002.pdf, page 6]"
        );
    }

    #[test]
    fn split_hybrid_decomposes_doc_labels_into_doc_id_form() {
        let text = "[c1, c2, doc-6, doc-7, doc-8]";
        let out = split_hybrid_citation_brackets(text);
        assert_eq!(
            out.as_ref(),
            "[c1] [c2] [doc-id: doc-6] [doc-id: doc-7] [doc-id: doc-8]"
        );
    }

    #[test]
    fn split_hybrid_idempotent_on_already_split_output() {
        // Run the function twice — the second pass must be a no-op.
        // Critical for the post-processing pipeline; if we ever wire
        // it in twice (defensive paths), it must not corrupt clean text.
        let text = "[c1, c2, FILE.pdf, p.3]";
        let pass_one = split_hybrid_citation_brackets(text).into_owned();
        let pass_two = split_hybrid_citation_brackets(&pass_one);
        assert_eq!(pass_two.as_ref(), &pass_one);
    }

    #[test]
    fn split_hybrid_handles_real_world_long_inventory_ref() {
        // The exact failure case from a 10-attachment medical-legal
        // report: a long mixed bracket that the frontend's MARKER_GROUP
        // regex would refuse to match because of the .pdf token.
        let text = "[c1, c2, c3, c4, c5, c6, c7, c8, c9, c10, c11, c12, c13, c14, c15, c16, c17, c18, c34, CARTELLA_TEST_002.pdf, p.3]";
        let out = split_hybrid_citation_brackets(text);
        let s = out.as_ref();
        // Each ref now stands alone — render-ready as a pill.
        for i in [1, 2, 3, 14, 18, 34] {
            let needle = format!("[c{i}]");
            assert!(
                s.contains(&needle),
                "expected {} in split output: {}",
                needle,
                s
            );
        }
        assert!(s.contains("[doc-id: CARTELLA_TEST_002.pdf, page 3]"));
        // No more comma-with-leading-letter inside any bracket.
        assert!(
            !s.contains(", c"),
            "split output still contains comma-separated refs: {s}"
        );
    }

    #[test]
    fn split_hybrid_does_not_corrupt_trailing_citations_json_block() {
        // v0.5.1 regression: my splitter looked for the first `]` after
        // each `[`, and inside the trailing JSON block the model's
        // citation quotes occasionally contain `[N]` brackets copied
        // from the prose (e.g. "vedi art. 32 [3]"). The splitter then
        // landed on the nested `]` and truncated the JSON array,
        // which `extract_citations_block` silently rejected, killing
        // every annotation. This test pins the contract: the segment
        // from `<CITATIONS>` onwards must pass through verbatim.
        let body = r#"Vedi [c1] e [c2].

<CITATIONS>[{"ref":"c1","doc_id":"doc-0","quote":"art. 32 [3] del decreto","page":1},{"ref":"c2","doc_id":"doc-1","quote":"sez. II","page":4}]</CITATIONS>"#;
        let out = split_hybrid_citation_brackets(body);
        // The trailing JSON must be byte-identical.
        let cit_start = body.find("<CITATIONS>").unwrap();
        let expected_tail = &body[cit_start..];
        assert!(
            out.as_ref().ends_with(expected_tail),
            "CITATIONS block was corrupted by the splitter.\nGot tail:\n{}\nExpected tail:\n{}",
            &out.as_ref()[out.as_ref().len().saturating_sub(expected_tail.len())..],
            expected_tail
        );
        // Prose part should still be exact for clean refs.
        assert!(out.as_ref().contains("Vedi [c1] e [c2]."));
    }

    #[test]
    fn split_hybrid_preserves_unknown_tokens_in_parentheses() {
        // The model occasionally drops unstructured prose into a
        // bracket — we keep that text rather than swallow it, so the
        // answer doesn't lose meaning.
        let text = "[c1, c2, vedi anche allegato A]";
        let out = split_hybrid_citation_brackets(text);
        let s = out.as_ref();
        assert!(s.contains("[c1]"));
        assert!(s.contains("[c2]"));
        assert!(s.contains("(vedi anche allegato A)"));
    }

    #[test]
    fn canonical_corpus_key_collapses_inventory_variants_onto_one_key() {
        // The bug we are guarding against: the model copies the
        // `<USER LIBRARY>` line `[italian-legal] corte_costituzionale_1990_241`
        // (with bracket + space) as `doc_id` instead of the [gN] tag.
        // The canonical form must match whatever we index on the lookup
        // side — `<corpus_id> <corpus_identifier>` — and tolerate every
        // other punctuation / case variant.
        let canon = canonical_corpus_key("[italian-legal] corte_costituzionale_1990_241");
        assert_eq!(canon, "italianlegalcortecostituzionale1990241");
        // Every reasonable alternative form must collapse to the same key.
        for variant in [
            "italian-legal corte_costituzionale_1990_241",
            "Italian-Legal_corte_costituzionale_1990_241",
            "italianlegal:cortecostituzionale1990/241",
            "[ITALIAN-LEGAL] corte_costituzionale_1990_241",
        ] {
            assert_eq!(
                canonical_corpus_key(variant),
                canon,
                "variant {variant:?} did not collapse to the canonical key"
            );
        }
        // Sanity: bare ASCII passes through lowercase.
        assert_eq!(canonical_corpus_key("EurLex_32016R0679"), "eurlex32016r0679");
        // Empty / whitespace-only / punctuation-only inputs canonicalise
        // to the empty string, which the resolver must skip.
        assert_eq!(canonical_corpus_key(""), "");
        assert_eq!(canonical_corpus_key("   "), "");
        assert_eq!(canonical_corpus_key("[ ]"), "");
    }

    #[test]
    fn recovers_block_without_closing_tag() {
        // Exact shape observed in the wild: model wrote the JSON array
        // fully but ran out of output tokens before emitting the
        // `</CITATIONS>` tag. We must still surface the citations.
        let text = "Prose.\n<CITATIONS>\n[\n  {\"ref\":\"c1\",\"doc_id\":\"doc-0\",\"page\":1,\"quote\":\"hi\"},\n  {\"ref\":\"c2\",\"doc_id\":\"doc-0\",\"page\":2,\"quote\":\"bye\"}\n]";
        let v = extract_citations_block(text).expect("recovers without close tag");
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["ref"], "c1");
        assert_eq!(arr[1]["ref"], "c2");
    }

    #[test]
    fn recovers_truncated_array_with_partial_last_entry() {
        // The model's output was cut off mid-entry: the last
        // {"ref":"c3", … is missing its closing `}` and quote string.
        // We must recover c1 and c2 and drop the partial c3.
        let text = "<CITATIONS>\n[\n  {\"ref\":\"c1\",\"doc_id\":\"doc-0\",\"page\":1,\"quote\":\"hi\"},\n  {\"ref\":\"c2\",\"doc_id\":\"doc-0\",\"page\":2,\"quote\":\"bye\"},\n  {\"ref\":\"c3\",\"doc_id\":\"doc-0\",\"page\":3,\"quote\":\"truncated mid-stri";
        let v = extract_citations_block(text).expect("recovers truncated array");
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2, "expected first two complete entries");
        assert_eq!(arr[0]["ref"], "c1");
        assert_eq!(arr[1]["ref"], "c2");
    }

    #[test]
    fn truncation_recovery_handles_quote_with_brace_inside() {
        // A quote containing `}` must NOT trick the depth tracker — the
        // `}` inside the string is part of content, not structure.
        let text = "<CITATIONS>\n[\n  {\"ref\":\"c1\",\"quote\":\"value has } brace\"},\n  {\"ref\":\"c2\",\"quote\":\"second\"";
        let v = extract_citations_block(text).expect("recovers despite brace-in-string");
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["ref"], "c1");
    }

    #[test]
    fn recovers_block_without_closing_tag_via_escape_repair() {
        // Missing `</CITATIONS>` AND an over-escaped apostrophe inside
        // a quote — both must be tolerated, and the array must parse.
        let text = "<CITATIONS>\n[{\"ref\":\"c1\",\"quote\":\"SOCIETA\\' ALBA\"}]";
        let v = extract_citations_block(text).expect("parses after both repairs");
        let arr = v.as_array().unwrap();
        assert_eq!(arr[0]["quote"], "SOCIETA' ALBA");
    }

    #[test]
    fn repairs_over_escaped_apostrophe() {
        // LLMs copy verbatim quotes and emit `\'`, which is not legal
        // JSON — the block must still parse after the escape repair.
        let text = "Answer [c1].\n<CITATIONS>\n\
            [{\"ref\": \"c1\", \"doc_id\": \"doc-0\", \
              \"quote\": \"SOCIETA\\' ALBA LEASING S.P.A.\"}]\n\
            </CITATIONS>";
        let v = extract_citations_block(text).unwrap();
        assert_eq!(v[0]["ref"], "c1");
        assert_eq!(v[0]["quote"], "SOCIETA' ALBA LEASING S.P.A.");
    }

    #[test]
    fn repair_leaves_valid_escapes_intact() {
        // A clean block with legitimate \n / \" escapes must round-trip
        // unchanged (it parses on the first attempt, no repair applied).
        let text = "<CITATIONS>[{\"quote\":\"line one\\nsays \\\"hi\\\"\"}]</CITATIONS>";
        let v = extract_citations_block(text).unwrap();
        assert_eq!(v[0]["quote"], "line one\nsays \"hi\"");
    }

    #[test]
    fn picks_last_block_when_multiple() {
        // rfind on "<citations>" → last opening tag wins.
        let text = "<CITATIONS>[1]</CITATIONS> ... <CITATIONS>[2]</CITATIONS>";
        let v = extract_citations_block(text).unwrap();
        assert_eq!(v, json!([2]));
    }

    // ── enrich_doc_citations ────────────────────────────────────────

    #[test]
    fn enrich_backfills_missing_document_id_from_chat_docs() {
        let docs = vec![
            ("uuid-a".to_string(), "first.pdf".to_string()),
            ("uuid-b".to_string(), "second.pdf".to_string()),
        ];
        let input = json!({
            "citations": [
                { "ref": 1, "doc_id": "doc-0", "document_id": null, "source": "attached" },
                { "ref": 2, "doc_id": "doc-1", "filename": "kept.pdf" },
            ]
        });
        let out = enrich_doc_citations(input, &docs);
        let cits = out["citations"].as_array().unwrap();
        assert_eq!(cits[0]["document_id"], "uuid-a");
        assert_eq!(cits[0]["filename"], "first.pdf");
        // An already-present filename is not overwritten.
        assert_eq!(cits[1]["document_id"], "uuid-b");
        assert_eq!(cits[1]["filename"], "kept.pdf");
    }

    #[test]
    fn enrich_leaves_resolved_citations_untouched() {
        let docs = vec![("uuid-a".to_string(), "first.pdf".to_string())];
        let input = json!({
            "citations": [
                { "ref": 1, "doc_id": "doc-0", "document_id": "real-uuid" },
            ]
        });
        let out = enrich_doc_citations(input.clone(), &docs);
        assert_eq!(out["citations"][0]["document_id"], "real-uuid");
    }

    #[test]
    fn enrich_handles_bare_array_annotations() {
        // Annotations are persisted as a bare array, not a wrapper object.
        let docs = vec![("uuid-a".to_string(), "first.pdf".to_string())];
        let input = json!([{ "ref": 1, "doc_id": "doc-0", "page": 4 }]);
        let out = enrich_doc_citations(input, &docs);
        assert_eq!(out[0]["document_id"], "uuid-a");
        assert_eq!(out[0]["filename"], "first.pdf");
    }

    #[test]
    fn enrich_is_a_noop_without_chat_docs() {
        let empty: Vec<(String, String)> = vec![];
        let v = json!([{ "doc_id": "doc-0" }]);
        assert_eq!(enrich_doc_citations(v.clone(), &empty), v);
    }

    #[test]
    fn enrich_skips_doc_label_out_of_range() {
        let docs = vec![("uuid-a".to_string(), "first.pdf".to_string())];
        let input = json!({ "citations": [{ "doc_id": "doc-5" }] });
        let out = enrich_doc_citations(input, &docs);
        assert!(out["citations"][0].get("document_id").is_none());
    }
}
