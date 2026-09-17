use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq)]
pub enum Provider {
    Claude,
    Gemini,
    /// Any OpenAI-compatible endpoint — OpenAI cloud, local Ollama,
    /// vLLM, Infomaniak, etc. Routed through `src/llm/local.rs`.
    OpenAI,
    /// Mistral La Plateforme (`api.mistral.ai`). Split from OpenAI
    /// path because Mistral has provider-specific knobs we want to
    /// always send: `parallel_tool_calls: false` (sequential tool
    /// calling for predictable legal-workflow runs),
    /// `prompt_cache_key` (charges 10% of normal token price on
    /// cached prefixes — huge on long conversations), plus
    /// Mistral-specific error mapping (401/403/422/429 with
    /// retry-after parsing). Routed through `src/llm/mistral.rs`.
    Mistral,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    /// Optional list of `data:image/png;base64,...` URLs to attach.
    /// Only honored when the target model is vision-capable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
    /// For `assistant` messages that requested tool calls in a previous turn.
    /// Replayed to the model as `tool_calls` in OpenAI-compatible format.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// For `tool` messages: the id of the call this result belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// For `tool` messages: the name of the invoked function. OpenAI keys
    /// tool results by id, Gemini keys them by function name — so we keep both.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            images: vec![],
            tool_calls: vec![],
            tool_call_id: None,
            tool_name: None,
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            images: vec![],
            tool_calls: vec![],
            tool_call_id: None,
            tool_name: None,
        }
    }
    /// Synthetic system-role message — used by the summarizer to inject
    /// the compressed-history block. Most providers map this onto a
    /// `system` role, but Gemini doesn't have one; the gemini.rs
    /// adapter folds system content into the first user message.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            images: vec![],
            tool_calls: vec![],
            tool_call_id: None,
            tool_name: None,
        }
    }
    pub fn tool_result(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            images: vec![],
            tool_calls: vec![],
            tool_call_id: Some(tool_call_id.into()),
            tool_name: Some(tool_name.into()),
        }
    }
    pub fn assistant_tool_calls(calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content: String::new(),
            images: vec![],
            tool_calls: calls,
            tool_call_id: None,
            tool_name: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    #[serde(rename = "type")]
    pub kind: String, // "function"
    pub function: ToolFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
    /// Opaque per-part token Gemini 2.5+ emits on `functionCall` parts
    /// generated during a thinking pass. The Gemini API requires the
    /// client to echo this back verbatim on the corresponding
    /// `functionCall` part when re-submitting the conversation for the
    /// next turn, or the request is rejected with
    /// `400 INVALID_ARGUMENT: Function call is missing a
    /// thought_signature in functionCall parts`. Always `None` for
    /// providers other than Gemini.
    /// See https://ai.google.dev/gemini-api/docs/thought-signatures
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub content: String,
}

#[derive(Debug)]
pub enum StreamEvent {
    ContentDelta(String),
    ReasoningDelta(String),
    ReasoningEnd,
    ToolCallStart { name: String },
    ToolCalls(Vec<ToolCall>),
    Done,
}

/// Per-user OpenAI-compatible endpoint config (Ollama / vLLM / Cloud Run / etc.)
/// When set, supersedes VLLM_BASE_URL / VLLM_API_KEY / VLLM_MAIN_MODEL env vars.
#[derive(Debug, Clone)]
pub struct LocalConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    /// "Modalità sicura locale" — when true, the local provider
    /// refuses any base_url that isn't loopback and any model id that
    /// isn't in the local-models catalogue
    /// (`config/local-models/ollama.json`). It also
    /// prepends a no-thinking preamble to the system prompt as a
    /// belt-and-braces safety net for any model that wasn't created
    /// via the catalogue Modelfile derivation. Default false on installs
    /// without the v0.5.6 migration (the migration ALTER TABLE pins
    /// the default to 0).
    pub secure_mode: bool,
}

/// Per-user Mistral provider options. Surfaced from the
/// `user_settings.mistral_*` columns (migration 0033). Only the
/// Mistral provider path reads this; other providers ignore it.
/// `None` on the params means the Mistral defaults apply (both
/// fields false — the Commit A behaviour).
#[derive(Debug, Clone, Default)]
pub struct MistralOpts {
    /// Mistral's `safe_prompt` request parameter. Default false
    /// (matches Mistral's API default). Toggle in
    /// Settings → Modelli LLM → Mistral AI.
    pub safe_prompt: bool,
    /// Mistral's `parallel_tool_calls`. Mistral's API default is
    /// `true`; this product overrides to `false` for sequential
    /// tool execution in legal workflows. The toggle re-enables
    /// the upstream parallel behaviour for power users who want it.
    pub parallel_tool_calls: bool,
}

pub struct StreamParams {
    pub model: String,
    /// Stable system-prompt prefix — identical across the turns of a chat
    /// (model instructions, attached-document text, tool descriptions).
    /// Sent as a cacheable block so providers that support prompt caching
    /// (Anthropic explicit, Gemini implicit) don't re-bill it every turn.
    pub system_prompt: String,
    /// Volatile system-prompt tail — content that changes per turn (today:
    /// per-query knowledge-base retrieval). Kept out of the cached prefix
    /// so it never invalidates the cache. Empty for non-chat callers.
    pub system_volatile: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSchema>,
    pub max_iterations: u32,
    pub enable_thinking: bool,
    pub local_config: Option<LocalConfig>,
    pub claude_api_key: Option<String>,
    pub gemini_api_key: Option<String>,
    /// Optional Google Cloud region (e.g. "europe-west1", "us-central1") for
    /// Gemini API calls. None or "global" → public endpoint. Preview models
    /// always force global; the `gemini::base_url` builder enforces this.
    pub gemini_region: Option<String>,
    /// Chat row id when the call is part of an actual conversation
    /// (Some(uuid) on the user-driven /chat path), None for one-shot
    /// callers like title generation, HyDE, or summarisation. Used by
    /// the Mistral provider to derive a stable `prompt_cache_key`
    /// (`covestudio_chat_{chat_id}`) — Mistral charges 10% of normal token
    /// price on cache hits, which on long legal conversations with
    /// stable system prompt + attached documents is a 70-90% cost
    /// reduction. None disables caching for that call.
    pub chat_id: Option<String>,
    /// Per-user Mistral options. Populated by `build_mistral_opts`
    /// in chat.rs only when the call is routed to the Mistral
    /// provider; other provider paths leave it None and Mistral's
    /// `build_body` falls back to the Commit A hard-coded defaults
    /// (`safe_prompt: false`, `parallel_tool_calls: false`).
    pub mistral_opts: Option<MistralOpts>,
}

impl StreamParams {
    /// The full system prompt — stable prefix plus volatile tail — for
    /// providers that send it as one opaque string (Gemini, local).
    pub fn full_system(&self) -> String {
        if self.system_volatile.is_empty() {
            self.system_prompt.clone()
        } else if self.system_prompt.is_empty() {
            self.system_volatile.clone()
        } else {
            format!("{}\n\n---\n\n{}", self.system_prompt, self.system_volatile)
        }
    }
}
