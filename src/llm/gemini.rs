/// Google Gemini — generateContent API with server-sent events streaming.
/// Supports function calling (tool-use) via `tools.function_declarations`.
use anyhow::{anyhow, Result};
use futures_util::{stream, StreamExt};
use serde_json::{json, Value};

use super::types::{Message, Role, StreamEvent, StreamParams, ToolCall};
use crate::llm::BoxStream;

fn api_key(params: &StreamParams) -> Result<String> {
    if let Some(k) = params.gemini_api_key.as_ref().filter(|s| !s.trim().is_empty()) {
        return Ok(k.clone());
    }
    std::env::var("GEMINI_API_KEY")
        .map_err(|_| anyhow!("Gemini API key not configured: set it in Account → Models, or set GEMINI_API_KEY"))
}

/// Resolve the Gemini endpoint URL.
///
/// The Generative Language API (the API-key-authenticated endpoint at
/// `generativelanguage.googleapis.com`) is *only* served globally — there
/// are no regional subdomains for it. Regional Gemini access requires
/// **Vertex AI**, which is a separate API using OAuth / service-account
/// credentials and a different URL pattern
/// (`<region>-aiplatform.googleapis.com/v1/projects/<proj>/locations/...`).
///
/// We accept and persist the user's region preference via
/// `params.gemini_region` so the choice survives Cove Studio restarts and
/// is ready for the future Vertex integration, but for now we route
/// every call to the global endpoint and log an info line when a
/// non-global region is requested.
fn base_url_with(model: &str, region: Option<&str>, suffix: &str) -> String {
    if let Some(r) = region.map(|s| s.trim()).filter(|r| !r.is_empty() && *r != "global") {
        tracing::info!(
            "[gemini] region '{r}' requested but Generative Language API is global-only — \
             routing to generativelanguage.googleapis.com (Vertex AI integration pending)"
        );
    }
    format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:{suffix}")
}

fn base_url(params: &StreamParams) -> String {
    base_url_with(
        &params.model,
        params.gemini_region.as_deref(),
        "streamGenerateContent",
    )
}

/// Convert Cove Studio messages into Gemini `contents` parts.
/// Roles: user → "user", assistant → "model".
/// Tool calls (assistant.tool_calls) → `model` part with `functionCall`.
/// Tool results (role=Tool) → `user` part with `functionResponse`.
fn to_wire_contents(messages: &[Message]) -> Vec<Value> {
    let mut out = Vec::new();
    for m in messages {
        match m.role {
            Role::System => continue, // system goes to systemInstruction, not contents
            Role::Tool => {
                // Gemini expects role:user with a functionResponse keyed by
                // the function NAME (not the call id). Prefer `tool_name`,
                // fall back to `tool_call_id` (which for OpenAI-compat models
                // happens to also be the function name in many cases).
                let name = m
                    .tool_name
                    .clone()
                    .or_else(|| m.tool_call_id.clone())
                    .unwrap_or_default();
                let response_value: Value = serde_json::from_str(&m.content)
                    .unwrap_or_else(|_| json!({ "result": m.content }));
                out.push(json!({
                    "role": "user",
                    "parts": [{
                        "functionResponse": {
                            "name": name,
                            "response": response_value
                        }
                    }]
                }));
            }
            Role::Assistant if !m.tool_calls.is_empty() => {
                let parts: Vec<Value> = m
                    .tool_calls
                    .iter()
                    .map(|c| {
                        // Gemini 3.5+ requires us to echo back the
                        // `thoughtSignature` that came with the original
                        // functionCall part; omitting it on replay is a
                        // hard 400 INVALID_ARGUMENT.
                        let mut part = json!({
                            "functionCall": {
                                "name": c.name,
                                "args": c.input
                            }
                        });
                        if let Some(sig) = &c.thought_signature {
                            part["thoughtSignature"] = Value::String(sig.clone());
                        }
                        part
                    })
                    .collect();
                out.push(json!({ "role": "model", "parts": parts }));
            }
            Role::User | Role::Assistant => {
                let role = if matches!(m.role, Role::Assistant) { "model" } else { "user" };
                out.push(json!({ "role": role, "parts": [{ "text": m.content }] }));
            }
        }
    }
    out
}

/// `safetySettings` payload turning OFF all four content filters.
///
/// Cove Studio is used on legal, insurance and PA workloads where the source
/// material legitimately contains references to violence (sentences,
/// claims), sexual content (employment-law / harassment cases), threats
/// (anti-corruption files) and hate-speech evidence (discrimination
/// litigation). Gemini's default `BLOCK_MEDIUM_AND_ABOVE` will silently
/// refuse or empty-respond on those — invisible until the first refused
/// case. The four categories Google currently exposes via the API
/// (matches the SDK example) all go to OFF.
fn safety_settings_off() -> Value {
    json!([
        { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "OFF" },
        { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "OFF" },
        { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "OFF" },
        { "category": "HARM_CATEGORY_HARASSMENT", "threshold": "OFF" },
    ])
}

/// Build the `thinkingConfig` payload appropriate to the model family.
///
/// Gemini 3.5 (and later) exposes a discrete `thinkingLevel` enum
/// (`OFF` / `LOW` / `MEDIUM` / `HIGH`). The default on Flash-class 3.5
/// is `HIGH`, which observed empirically tends to burn output budget
/// thinking instead of writing — implicated in the truncated NIS2
/// report. `MEDIUM` is the documented balanced setting.
///
/// Gemini 2.5 uses the older integer `thinkingBudget` (in tokens);
/// `-1` is the dynamic mode where the model chooses but stays within
/// model-published caps.
///
/// Returns `None` for families that don't expose a thinking knob
/// (e.g. legacy `gemini-1.5*`, `gemini-2.0*`).
fn thinking_config_for_model(model: &str) -> Option<Value> {
    let m = model.to_ascii_lowercase();
    if m.starts_with("gemini-3.5")
        || m.starts_with("gemini-3-flash")
        || m.starts_with("gemini-3-pro")
    {
        Some(json!({ "thinkingLevel": "MEDIUM" }))
    } else if m.starts_with("gemini-2.5") {
        Some(json!({ "thinkingBudget": -1 }))
    } else {
        None
    }
}

fn build_body(params: &StreamParams) -> Value {
    let mut body = json!({ "contents": to_wire_contents(&params.messages) });
    // Stable prefix first, volatile tail last: Gemini 2.5 implicit caching
    // matches the longest common prefix of the request automatically, so
    // keeping the per-query KB part at the end lets the stable prefix hit
    // the cache on follow-up turns without any explicit cache handle.
    let system = params.full_system();
    if !system.is_empty() {
        body["systemInstruction"] = json!({
            "parts": [{ "text": system }]
        });
    }
    if !params.tools.is_empty() {
        let function_declarations: Vec<Value> = params
            .tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.function.name,
                    "description": t.function.description,
                    "parameters": sanitize_schema_for_gemini(&t.function.parameters),
                })
            })
            .collect();
        body["tools"] = json!([{ "function_declarations": function_declarations }]);
    }
    body["safetySettings"] = safety_settings_off();
    // generationConfig is only attached when a per-model thinking knob is
    // needed. We deliberately do NOT set `temperature` on Gemini: the
    // cross-provider `0.5` we apply to Claude / local triggers a known
    // white-out failure mode on `gemini-2.5-flash` over long contexts —
    // the model gets stuck in a low-entropy whitespace loop and emits
    // hundreds of thousands of spaces before closing the stream
    // (observed 2026-05-26 on the medical-timeline workflow). Letting
    // Gemini fall back to its API default keeps the deterministic
    // tightening on the other providers without breaking Flash.
    if let Some(thinking) = thinking_config_for_model(&params.model) {
        body["generationConfig"] = json!({ "thinkingConfig": thinking });
    }
    body
}

/// Gemini's function-declaration schema is *almost* JSON-Schema but rejects
/// fields like `$schema`, `additionalProperties`, `title`, `default` and the
/// `format` keyword on strings. It also rejects a `required` entry that
/// names a property not present in `properties`. Strip / filter so a
/// permissive MCP schema doesn't trigger a 400 from Gemini.
fn sanitize_schema_for_gemini(v: &Value) -> Value {
    fn walk(v: &Value) -> Value {
        match v {
            Value::Object(map) => {
                let mut out = serde_json::Map::new();
                for (k, val) in map {
                    if matches!(
                        k.as_str(),
                        "$schema"
                            | "$id"
                            | "$ref"
                            | "$defs"
                            | "definitions"
                            | "additionalProperties"
                            | "title"
                            | "default"
                            | "examples"
                            | "const"
                            | "format"
                    ) {
                        continue;
                    }
                    out.insert(k.clone(), walk(val));
                }

                // Filter `required` to only names that exist in `properties`.
                if let (Some(Value::Array(req)), Some(Value::Object(props))) =
                    (out.get("required").cloned(), out.get("properties"))
                {
                    let prop_names: std::collections::HashSet<&str> =
                        props.keys().map(|s| s.as_str()).collect();
                    let filtered: Vec<Value> = req
                        .into_iter()
                        .filter(|r| match r.as_str() {
                            Some(name) => prop_names.contains(name),
                            None => false,
                        })
                        .collect();
                    if filtered.is_empty() {
                        out.remove("required");
                    } else {
                        out.insert("required".to_string(), Value::Array(filtered));
                    }
                } else if matches!(out.get("required"), Some(Value::Array(_)))
                    && out.get("properties").is_none()
                {
                    // `required` without `properties` is meaningless.
                    out.remove("required");
                }

                Value::Object(out)
            }
            Value::Array(arr) => Value::Array(arr.iter().map(walk).collect()),
            other => other.clone(),
        }
    }
    walk(v)
}

pub async fn stream(params: StreamParams) -> Result<BoxStream> {
    let key = api_key(&params)?;
    let client = reqwest::Client::new();
    let url = format!("{}?key={}&alt=sse", base_url(&params), key);

    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&build_body(&params))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("Gemini API error {status}: {text}"));
    }

    let byte_stream = resp.bytes_stream();
    let event_stream = stream::unfold(
        (byte_stream, String::new(), 0u64),
        |(mut bs, mut buf, mut tc_counter)| async move {
            loop {
                match bs.next().await {
                    None => {
                        if buf.trim().is_empty() { return None; }
                        let line = buf.trim().to_string();
                        buf.clear();
                        return Some((parse_gemini_sse(&line, &mut tc_counter), (bs, buf, tc_counter)));
                    }
                    Some(Err(e)) => {
                        return Some((Err(anyhow!("stream error: {e}")), (bs, buf, tc_counter)));
                    }
                    Some(Ok(bytes)) => {
                        buf.push_str(&String::from_utf8_lossy(&bytes));
                        while let Some(pos) = buf.find('\n') {
                            let line = buf[..pos].trim().to_string();
                            buf.drain(..=pos);
                            if let Some(ev) = parse_gemini_sse_opt(&line, &mut tc_counter) {
                                return Some((Ok(ev), (bs, buf, tc_counter)));
                            }
                        }
                    }
                }
            }
        },
    );

    Ok(Box::pin(event_stream))
}

fn parse_gemini_sse(line: &str, tc_counter: &mut u64) -> Result<StreamEvent> {
    parse_gemini_sse_opt(line, tc_counter).ok_or_else(|| anyhow!("empty SSE line"))
}

fn parse_gemini_sse_opt(line: &str, tc_counter: &mut u64) -> Option<StreamEvent> {
    if !line.starts_with("data: ") { return None; }
    let data = line[6..].trim();
    let v: Value = serde_json::from_str(data).ok()?;
    let parts = v
        .get("candidates")?
        .get(0)?
        .get("content")?
        .get("parts")?
        .as_array()?;

    // Function calls take priority — emit the whole batch as a ToolCalls event.
    let calls: Vec<ToolCall> = parts
        .iter()
        .filter_map(|p| {
            let fc = p.get("functionCall")?;
            *tc_counter += 1;
            let id = format!("gemini-fc-{tc_counter}");
            // `thoughtSignature` is the opaque token the model emits on
            // functionCall parts produced during a thinking pass; we
            // MUST echo it back when the conversation is replayed, or
            // Gemini 3.5+ rejects the request with 400 INVALID_ARGUMENT.
            let thought_signature = p
                .get("thoughtSignature")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Some(ToolCall {
                id,
                name: fc.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string(),
                input: fc.get("args").cloned().unwrap_or(json!({})),
                thought_signature,
            })
        })
        .collect();
    if !calls.is_empty() {
        return Some(StreamEvent::ToolCalls(calls));
    }

    // Fall back to text content delta.
    let text = parts
        .iter()
        .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
        .collect::<Vec<_>>()
        .join("");
    if !text.is_empty() {
        // Gemini 2.5 quirk: when AUTO mode decides to call a function, it
        // occasionally writes the call as Python-API prose (the same
        // syntax Google's documentation uses to illustrate tool-use) —
        // e.g. `tool_code print(default_api.generate_docx(body='…'))`
        // — instead of emitting a `functionCall` part. The official
        // SDKs strip it client-side; the raw REST path doesn't. We do
        // it here so the assistant turn actually dispatches the tool
        // instead of dumping the prose into the chat as a literal
        // markdown blob.
        if let Some((name, args)) = try_parse_tool_code_prose(&text) {
            *tc_counter += 1;
            let id = format!("gemini-fc-{tc_counter}");
            return Some(StreamEvent::ToolCalls(vec![ToolCall {
                id,
                name,
                input: args,
                // tool_code prose is text-mode output (the model
                // bypassed structured functionCalls), so there is no
                // signature to echo. Gemini accepts the absence here
                // because the original part wasn't a functionCall.
                thought_signature: None,
            }]));
        }
        return Some(StreamEvent::ContentDelta(text));
    }
    None
}

/// Detect Gemini's "tool_code prose" pattern (`tool_code print(default_api.NAME(kwarg='…'))`)
/// and convert it into a real ToolCall. Recognises every common envelope
/// the model produces:
///
///   - bare:          `default_api.NAME(arg='…')`
///   - print-wrapped: `print(default_api.NAME(arg='…'))`
///   - tool_code:     `tool_code print(default_api.NAME(arg='…'))`
///   - code-fenced:   ` ```python\ntool_code print(...)``` `
///   - triple-quoted body: `print(default_api.generate_docx(body='''line\n\nline'''))`
///   - prefix garbage:  Gemini sometimes leaks a "read_document\n" or
///     "thought\n" line before the call; we search forward for the
///     first `default_api.` anchor so those don't kill the match.
///
/// Returns `None` when the text doesn't match — the caller then falls
/// back to the normal `ContentDelta` path so regular prose is unaffected.
fn try_parse_tool_code_prose(text: &str) -> Option<(String, Value)> {
    // Anchor on the first `default_api.` occurrence anywhere in the
    // text. Gemini sometimes leaks an arbitrary leading line — a
    // previous step's tool name ("read_document\n"), a "thought\n"
    // header, code-fence noise — before the actual call. The earlier
    // strict prefix-stripping rejected those; anchoring on
    // `default_api.` accepts every wrapper the model can produce.
    // False-positive risk is small: regular prose almost never types
    // the literal `default_api.` token.
    let anchor = text.find("default_api.")?;
    let s = &text[anchor + "default_api.".len()..];

    // Parse "NAME(arg=val, …)".
    let paren = s.find('(')?;
    let name = s[..paren].trim();
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    let after = &s[paren + 1..];
    let args_inner = extract_balanced_paren_contents(after)?;
    let args = parse_python_kwargs(args_inner)?;
    Some((name.to_string(), args))
}

/// Given a slice that starts *after* an opening `(`, return the slice up
/// to its matching `)`. Tracks Python single/double-quoted string literals
/// (including triple-quoted `'''…'''` / `"""…"""` blocks that may span
/// lines and contain unescaped quotes of the same character) and nested
/// brackets so commas/parens inside strings don't fool it.
fn extract_balanced_paren_contents(s: &str) -> Option<&str> {
    #[derive(Clone, Copy)]
    enum StrMode {
        Single(char), // q
        Triple(char), // q (triple-quoted, only close on q q q)
    }
    let bytes = s.as_bytes();
    let mut depth: i32 = 1;
    let mut in_str: Option<StrMode> = None;
    let mut escaped = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if escaped {
            escaped = false;
            i += 1;
            continue;
        }
        match in_str {
            Some(StrMode::Triple(q)) => {
                // Only close on three consecutive q. Inside, `\` still
                // escapes the next char (Python triple strings honour
                // common escapes too).
                if c == b'\\' {
                    escaped = true;
                } else if c == q as u8
                    && i + 2 < bytes.len()
                    && bytes[i + 1] == q as u8
                    && bytes[i + 2] == q as u8
                {
                    in_str = None;
                    i += 3;
                    continue;
                }
            }
            Some(StrMode::Single(q)) => {
                if c == b'\\' {
                    escaped = true;
                } else if c == q as u8 {
                    in_str = None;
                }
            }
            None => match c {
                b'\'' | b'"' => {
                    let q = c;
                    if i + 2 < bytes.len() && bytes[i + 1] == q && bytes[i + 2] == q {
                        in_str = Some(StrMode::Triple(q as char));
                        i += 3;
                        continue;
                    }
                    in_str = Some(StrMode::Single(q as char));
                }
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&s[..i]);
                    }
                }
                _ => {}
            },
        }
        i += 1;
    }
    None
}

/// Parse a Python kwargs blob (`a='x', b=2, c={'k': 1}`) into a JSON
/// object. Permissive: missing values, unknown literal types, etc. fall
/// back to the raw substring. The aim is "do something sensible so the
/// dispatcher gets the bytes it needs", not "exactly mirror Python AST".
fn parse_python_kwargs(s: &str) -> Option<Value> {
    let mut obj = serde_json::Map::new();
    for part in split_top_level_commas(s) {
        let eq = part.find('=')?;
        let key = part[..eq].trim().to_string();
        if key.is_empty() {
            return None;
        }
        let val_str = part[eq + 1..].trim();
        let val = python_literal_to_json(val_str);
        obj.insert(key, val);
    }
    Some(Value::Object(obj))
}

/// Split a Python args string by commas at the top nesting level,
/// respecting string quotes (single, double, and triple) and nested
/// brackets. Triple-quoted strings are crucial for generate_docx
/// bodies, which Gemini emits as `body='''# Title\n\nbody'''`.
fn split_top_level_commas(s: &str) -> Vec<String> {
    #[derive(Clone, Copy)]
    enum StrMode {
        Single(char),
        Triple(char),
    }
    // Collect to Vec<char> so the 3-char triple-quote lookahead can
    // index forward safely on multibyte input (a body containing
    // accented characters like "Undómiel" was being corrupted when
    // this loop iterated bytes-as-chars).
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_str: Option<StrMode> = None;
    let mut escaped = false;
    let mut depth: i32 = 0;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if escaped {
            current.push(c);
            escaped = false;
            i += 1;
            continue;
        }
        match in_str {
            Some(StrMode::Triple(q)) => {
                if c == '\\' {
                    current.push(c);
                    escaped = true;
                } else if c == q
                    && i + 2 < chars.len()
                    && chars[i + 1] == q
                    && chars[i + 2] == q
                {
                    current.push(c);
                    current.push(c);
                    current.push(c);
                    in_str = None;
                    i += 3;
                    continue;
                } else {
                    current.push(c);
                }
            }
            Some(StrMode::Single(q)) => {
                if c == '\\' {
                    current.push(c);
                    escaped = true;
                } else {
                    if c == q {
                        in_str = None;
                    }
                    current.push(c);
                }
            }
            None => match c {
                '\'' | '"' => {
                    if i + 2 < chars.len() && chars[i + 1] == c && chars[i + 2] == c {
                        current.push(c);
                        current.push(c);
                        current.push(c);
                        in_str = Some(StrMode::Triple(c));
                        i += 3;
                        continue;
                    }
                    in_str = Some(StrMode::Single(c));
                    current.push(c);
                }
                '(' | '[' | '{' => {
                    depth += 1;
                    current.push(c);
                }
                ')' | ']' | '}' => {
                    depth -= 1;
                    current.push(c);
                }
                ',' if depth == 0 => {
                    let trimmed = current.trim().to_string();
                    if !trimmed.is_empty() {
                        out.push(trimmed);
                    }
                    current.clear();
                }
                _ => current.push(c),
            },
        }
        i += 1;
    }
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        out.push(trimmed);
    }
    out
}

/// Convert a Python literal to its JSON equivalent. Strings (single or
/// double quoted), numbers, booleans (`True` / `False`), and `None` map
/// naturally. Dict / list literals are normalised to JSON by swapping
/// single quotes for double quotes — good enough for the values Gemini
/// emits in tool_code prose. On any failure, returns a JSON string of
/// the raw substring so the dispatcher at least sees something.
fn python_literal_to_json(s: &str) -> Value {
    let trimmed = s.trim();
    if trimmed == "None" {
        return Value::Null;
    }
    if trimmed == "True" {
        return Value::Bool(true);
    }
    if trimmed == "False" {
        return Value::Bool(false);
    }
    if let Ok(n) = trimmed.parse::<i64>() {
        return json!(n);
    }
    if let Ok(n) = trimmed.parse::<f64>() {
        return json!(n);
    }
    // Triple-quoted string literal: '''…''' or """…""". Must come
    // before the single-quote check below or `'''x'''` is mistaken
    // for a single-quoted `''` + `x` + `''` mess.
    if trimmed.len() >= 6
        && (trimmed.starts_with("'''") && trimmed.ends_with("'''")
            || trimmed.starts_with("\"\"\"") && trimmed.ends_with("\"\"\""))
    {
        let inner = &trimmed[3..trimmed.len() - 3];
        return Value::String(decode_python_string_escapes(inner));
    }
    // String literal: ' or ".
    if trimmed.len() >= 2 {
        let first = trimmed.chars().next().unwrap();
        let last = trimmed.chars().last().unwrap();
        if (first == '\'' && last == '\'') || (first == '"' && last == '"') {
            let inner = &trimmed[1..trimmed.len() - 1];
            return Value::String(decode_python_string_escapes(inner));
        }
    }
    // Dict / list literal — try JSON after a naïve single→double quote swap.
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        let normalised = trimmed.replace('\'', "\"");
        if let Ok(v) = serde_json::from_str::<Value>(&normalised) {
            return v;
        }
    }
    // Last resort — keep the raw fragment so the model's intent isn't lost.
    Value::String(trimmed.to_string())
}

fn decode_python_string_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some('\'') => out.push('\''),
                Some('"') => out.push('"'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub async fn complete(params: StreamParams) -> Result<String> {
    let key = api_key(&params)?;
    let client = reqwest::Client::new();
    let url = format!(
        "{}?key={}",
        base_url_with(&params.model, params.gemini_region.as_deref(), "generateContent"),
        key,
    );

    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&build_body(&params))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("Gemini API error {status}: {text}"));
    }

    let v: Value = resp.json().await?;
    let text = v
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();

    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::StreamEvent;
    use serde_json::json;

    #[test]
    fn sanitize_drops_unsupported_keys() {
        let raw = json!({
            "$schema": "http://json-schema.org/draft-07/schema",
            "title": "X",
            "default": null,
            "additionalProperties": false,
            "type": "object",
            "properties": {
                "name": { "type": "string", "format": "uri" }
            },
            "required": ["name"]
        });
        let cleaned = sanitize_schema_for_gemini(&raw);
        let map = cleaned.as_object().unwrap();
        assert!(!map.contains_key("$schema"));
        assert!(!map.contains_key("title"));
        assert!(!map.contains_key("default"));
        assert!(!map.contains_key("additionalProperties"));
        assert_eq!(map["type"], "object");
        // `format` should be stripped from inner properties.
        let name = &map["properties"]["name"];
        assert!(!name.as_object().unwrap().contains_key("format"));
        assert_eq!(name["type"], "string");
        // `required` should remain since `name` exists in properties.
        assert_eq!(map["required"], json!(["name"]));
    }

    #[test]
    fn sanitize_filters_required_to_existing_props() {
        let raw = json!({
            "type": "object",
            "properties": { "a": { "type": "string" } },
            "required": ["a", "b", "ghost"]
        });
        let cleaned = sanitize_schema_for_gemini(&raw);
        // Only `a` exists, so `b` and `ghost` must be dropped.
        assert_eq!(cleaned["required"], json!(["a"]));
    }

    #[test]
    fn sanitize_removes_empty_required() {
        let raw = json!({
            "type": "object",
            "properties": { "a": { "type": "string" } },
            "required": ["b"]
        });
        let cleaned = sanitize_schema_for_gemini(&raw);
        assert!(cleaned.as_object().unwrap().get("required").is_none());
    }

    #[test]
    fn sanitize_drops_required_without_properties() {
        let raw = json!({
            "type": "object",
            "required": ["a"]
        });
        let cleaned = sanitize_schema_for_gemini(&raw);
        assert!(cleaned.as_object().unwrap().get("required").is_none());
    }

    fn empty_params(model: &str) -> StreamParams {
        StreamParams {
            model: model.to_string(),
            system_prompt: String::new(),
            system_volatile: String::new(),
            messages: vec![Message::user("hi".to_string())],
            tools: vec![],
            max_iterations: 1,
            enable_thinking: false,
            local_config: None,
            claude_api_key: None,
            gemini_api_key: None,
            gemini_region: None,
            chat_id: None,
            mistral_opts: None,
        }
    }

    #[test]
    fn build_body_always_attaches_safety_settings_off() {
        // Every Gemini call must carry the four safety categories set
        // to OFF — legal / insurance / PA workloads legitimately touch
        // violence / sex / threats / hate-speech material and the
        // default `BLOCK_MEDIUM_AND_ABOVE` would silently refuse.
        let body = build_body(&empty_params("gemini-3.5-flash"));
        let settings = body["safetySettings"].as_array().expect("array");
        assert_eq!(settings.len(), 4);
        let categories: std::collections::HashSet<&str> = settings
            .iter()
            .map(|s| s["category"].as_str().unwrap())
            .collect();
        for required in [
            "HARM_CATEGORY_HATE_SPEECH",
            "HARM_CATEGORY_DANGEROUS_CONTENT",
            "HARM_CATEGORY_SEXUALLY_EXPLICIT",
            "HARM_CATEGORY_HARASSMENT",
        ] {
            assert!(categories.contains(required), "missing category {required}");
        }
        for s in settings {
            assert_eq!(s["threshold"], "OFF", "category {} not OFF", s["category"]);
        }
    }

    #[test]
    fn build_body_thinking_config_shape_is_model_aware() {
        // Gemini 3.5 takes a discrete level enum; 2.5 takes the legacy
        // integer thinking_budget. Wrong shape → 400 from Google.
        let body_35 = build_body(&empty_params("gemini-3.5-flash"));
        assert_eq!(
            body_35["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "MEDIUM"
        );
        assert!(body_35["generationConfig"]["thinkingConfig"]
            .as_object()
            .unwrap()
            .get("thinkingBudget")
            .is_none());

        let body_25 = build_body(&empty_params("gemini-2.5-flash"));
        assert_eq!(
            body_25["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            -1
        );
        assert!(body_25["generationConfig"]["thinkingConfig"]
            .as_object()
            .unwrap()
            .get("thinkingLevel")
            .is_none());

        // gemini-2.5-pro and gemini-2.5-flash-lite ride the same 2.5
        // wire shape.
        assert_eq!(
            build_body(&empty_params("gemini-2.5-pro"))["generationConfig"]["thinkingConfig"]
                ["thinkingBudget"],
            -1
        );
    }

    #[test]
    fn build_body_omits_generation_config_on_legacy_families() {
        // Pre-2.5 models have no thinking knob, and we deliberately do
        // NOT set `temperature` on Gemini either (the cross-provider 0.5
        // we apply elsewhere triggers a white-out on gemini-2.5-flash
        // over long contexts — see build_body docstring). With nothing
        // to attach, generationConfig is omitted altogether on legacy
        // families.
        let body = build_body(&empty_params("gemini-1.5-pro"));
        assert!(
            body.get("generationConfig").is_none(),
            "generationConfig must be absent on legacy when no thinking knob applies"
        );

        let body = build_body(&empty_params("gemini-2.0-flash"));
        assert!(
            body.get("generationConfig").is_none(),
            "generationConfig must be absent on legacy when no thinking knob applies"
        );
    }

    #[test]
    fn build_body_still_carries_tools_and_system_instruction() {
        // Adding safety + generationConfig must NOT regress the other
        // payload sections. Smoke-test on a request with both tools
        // and a system prompt.
        let mut p = empty_params("gemini-3.5-flash");
        p.system_prompt = "you are the assistant".into();
        p.tools = vec![crate::llm::types::ToolSchema {
            kind: "function".into(),
            function: crate::llm::types::ToolFunction {
                name: "read_document".into(),
                description: "read a doc".into(),
                parameters: json!({"type": "object", "properties": {}, "required": []}),
            },
        }];
        let body = build_body(&p);
        assert!(body["systemInstruction"]["parts"][0]["text"]
            .as_str()
            .unwrap()
            .contains("you are the assistant"));
        let fns = body["tools"][0]["function_declarations"].as_array().unwrap();
        assert_eq!(fns[0]["name"], "read_document");
        // And the new fields are still there.
        assert!(body["safetySettings"].is_array());
        assert_eq!(
            body["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "MEDIUM"
        );
    }

    #[test]
    fn sanitize_recurses_into_arrays() {
        let raw = json!({
            "type": "array",
            "items": {
                "type": "object",
                "title": "ITEM",
                "properties": { "x": { "type": "number", "format": "float" } }
            }
        });
        let cleaned = sanitize_schema_for_gemini(&raw);
        let item = &cleaned["items"];
        assert!(!item.as_object().unwrap().contains_key("title"));
        assert!(!item["properties"]["x"].as_object().unwrap().contains_key("format"));
    }

    #[test]
    fn parse_sse_text_delta() {
        let mut counter = 0u64;
        let line = r#"data: {"candidates":[{"content":{"parts":[{"text":"ciao"}]}}]}"#;
        match parse_gemini_sse_opt(line, &mut counter) {
            Some(StreamEvent::ContentDelta(s)) => assert_eq!(s, "ciao"),
            other => panic!("expected ContentDelta, got {other:?}"),
        }
    }

    #[test]
    fn parse_sse_function_call_increments_counter() {
        let mut counter = 0u64;
        let line = r#"data: {"candidates":[{"content":{"parts":[{"functionCall":{"name":"read_document","args":{"doc_id":"doc-0"}}}]}}]}"#;
        match parse_gemini_sse_opt(line, &mut counter) {
            Some(StreamEvent::ToolCalls(calls)) => {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].name, "read_document");
                assert_eq!(calls[0].input["doc_id"], "doc-0");
                assert_eq!(calls[0].id, "gemini-fc-1");
            }
            other => panic!("expected ToolCalls, got {other:?}"),
        }
        // Subsequent call should produce id #2.
        let line2 = r#"data: {"candidates":[{"content":{"parts":[{"functionCall":{"name":"x","args":{}}}]}}]}"#;
        match parse_gemini_sse_opt(line2, &mut counter) {
            Some(StreamEvent::ToolCalls(calls)) => assert_eq!(calls[0].id, "gemini-fc-2"),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parse_sse_function_call_captures_thought_signature() {
        // Gemini 3.5+ tags functionCall parts produced during a thinking
        // pass with `thoughtSignature`; we must capture and echo it back.
        let mut counter = 0u64;
        let line = r#"data: {"candidates":[{"content":{"parts":[{"functionCall":{"name":"read_workflow","args":{"workflow_id":"x"}},"thoughtSignature":"opaque-token-abc"}]}}]}"#;
        match parse_gemini_sse_opt(line, &mut counter) {
            Some(StreamEvent::ToolCalls(calls)) => {
                assert_eq!(calls[0].thought_signature.as_deref(), Some("opaque-token-abc"));
            }
            other => panic!("expected ToolCalls, got {other:?}"),
        }
    }

    #[test]
    fn to_wire_contents_echoes_thought_signature_on_replay() {
        // Round-trip: an assistant turn carrying a Gemini tool call with
        // a thought_signature must surface it as `thoughtSignature` on
        // the wire, or Gemini 3.5+ rejects the request with
        // 400 INVALID_ARGUMENT on the next iteration.
        let msg = Message::assistant_tool_calls(vec![ToolCall {
            id: "gemini-fc-1".into(),
            name: "read_workflow".into(),
            input: json!({"workflow_id": "x"}),
            thought_signature: Some("opaque-token-abc".into()),
        }]);
        let wire = to_wire_contents(&[msg]);
        assert_eq!(wire[0]["role"], "model");
        let part = &wire[0]["parts"][0];
        assert_eq!(part["functionCall"]["name"], "read_workflow");
        assert_eq!(part["thoughtSignature"], "opaque-token-abc");

        // And the field is omitted when None (other providers, legacy
        // captures without a signature).
        let msg_nosig = Message::assistant_tool_calls(vec![ToolCall {
            id: "tool-1".into(),
            name: "x".into(),
            input: json!({}),
            thought_signature: None,
        }]);
        let wire_nosig = to_wire_contents(&[msg_nosig]);
        assert!(wire_nosig[0]["parts"][0].get("thoughtSignature").is_none());
    }

    #[test]
    fn parse_sse_ignores_garbage() {
        let mut counter = 0u64;
        assert!(parse_gemini_sse_opt("data: {}", &mut counter).is_none());
        assert!(parse_gemini_sse_opt("data: not json", &mut counter).is_none());
        assert!(parse_gemini_sse_opt("event: keepalive", &mut counter).is_none());
    }

    #[test]
    fn try_parse_tool_code_handles_full_wrapper() {
        // The exact shape observed in the wild: `tool_code print(default_api.NAME(arg='…'))`.
        let prose = "tool_code print(default_api.generate_docx(body='# Inventario\\n\\nrow 1'))";
        let (name, args) = try_parse_tool_code_prose(prose).expect("should parse");
        assert_eq!(name, "generate_docx");
        assert_eq!(args["body"], "# Inventario\n\nrow 1");
    }

    #[test]
    fn try_parse_tool_code_handles_print_only() {
        let prose = "print(default_api.read_document(doc_id='doc-0'))";
        let (name, args) = try_parse_tool_code_prose(prose).expect("should parse");
        assert_eq!(name, "read_document");
        assert_eq!(args["doc_id"], "doc-0");
    }

    #[test]
    fn try_parse_tool_code_handles_bare_default_api() {
        let prose = "default_api.list_docx_templates(domain='insurance')";
        let (name, args) = try_parse_tool_code_prose(prose).expect("should parse");
        assert_eq!(name, "list_docx_templates");
        assert_eq!(args["domain"], "insurance");
    }

    #[test]
    fn try_parse_tool_code_handles_python_fence() {
        let prose = "```python\ntool_code print(default_api.generate_docx(body='hi'))\n```";
        let (name, args) = try_parse_tool_code_prose(prose).expect("should parse");
        assert_eq!(name, "generate_docx");
        assert_eq!(args["body"], "hi");
    }

    #[test]
    fn try_parse_tool_code_handles_triple_quoted_multiline_body() {
        // Regression from the medical-anamnesis transcript: Gemini
        // emitted a multi-line body inside `'''…'''` with leading
        // "read_document\n" + "tool_code\n" garbage and a trailing
        // "thought\n…" rationale. The previous parser rejected this
        // (prefix garbage + triple-quote unhandled) and the prose
        // landed as a literal markdown blob in the chat.
        let prose = "read_document\ntool_code\nprint(default_api.generate_docx(body='''# Lettera di Dimissione\n\nDati Paziente\nNome: Arwen Und\u{00f3}miel\n\n## Diagnosi\nM23.2 \u{2014} Lesione complessa del menisco mediale ginocchio sinistro.\n''', metadata={'FILENAME': 'Lettera_Dimissioni.docx', 'LUOGO': 'Gran Burrone', 'DATA': '29/10/2025'}, template_id='it/lettera-dimissioni'))\nthought\nThe user asked to generate a DOCX document for the discharge letter.";
        let (name, args) = try_parse_tool_code_prose(prose).expect("should parse");
        assert_eq!(name, "generate_docx");
        let body = args["body"].as_str().expect("body is string");
        assert!(body.starts_with("# Lettera di Dimissione"));
        assert!(body.contains("Nome: Arwen Und\u{00f3}miel"));
        assert!(body.contains("M23.2"));
        assert_eq!(args["template_id"], "it/lettera-dimissioni");
        assert_eq!(args["metadata"]["FILENAME"], "Lettera_Dimissioni.docx");
        assert_eq!(args["metadata"]["LUOGO"], "Gran Burrone");
    }

    #[test]
    fn try_parse_tool_code_handles_multiple_kwargs() {
        // template_id + body + metadata dict.
        let prose = "tool_code print(default_api.generate_docx(template_id='it/diffida-messa-in-mora', body='# Letter', metadata={'DEBITORE': 'Tizio S.r.l.', 'IMPORTO': '1234'}))";
        let (name, args) = try_parse_tool_code_prose(prose).expect("should parse");
        assert_eq!(name, "generate_docx");
        assert_eq!(args["template_id"], "it/diffida-messa-in-mora");
        assert_eq!(args["body"], "# Letter");
        assert_eq!(args["metadata"]["DEBITORE"], "Tizio S.r.l.");
        assert_eq!(args["metadata"]["IMPORTO"], "1234");
    }

    #[test]
    fn try_parse_tool_code_rejects_plain_prose() {
        // No default_api., no print( wrapper — must not match.
        assert!(try_parse_tool_code_prose("Ecco il report richiesto:").is_none());
        // Looks like a function call but doesn't use the default_api namespace.
        assert!(try_parse_tool_code_prose("Foo(bar='baz')").is_none());
    }

    #[test]
    fn parse_sse_converts_tool_code_text_to_toolcalls() {
        // End-to-end: an SSE event carrying a text part with the tool_code
        // prose should surface as a ToolCalls event, not ContentDelta.
        let line = r#"data: {"candidates":[{"content":{"parts":[{"text":"tool_code print(default_api.generate_docx(body='# X'))"}]}}]}"#;
        let mut counter = 0u64;
        match parse_gemini_sse_opt(line, &mut counter) {
            Some(StreamEvent::ToolCalls(calls)) => {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].name, "generate_docx");
                assert_eq!(calls[0].input["body"], "# X");
            }
            other => panic!("expected ToolCalls, got {other:?}"),
        }
    }

    #[test]
    fn parse_sse_keeps_normal_text_as_contentdelta() {
        let line = r#"data: {"candidates":[{"content":{"parts":[{"text":"Ecco il documento generato."}]}}]}"#;
        let mut counter = 0u64;
        match parse_gemini_sse_opt(line, &mut counter) {
            Some(StreamEvent::ContentDelta(text)) => {
                assert_eq!(text, "Ecco il documento generato.");
            }
            other => panic!("expected ContentDelta, got {other:?}"),
        }
    }
}
