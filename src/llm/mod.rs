pub mod types;
pub mod claude;
pub mod gemini;
pub mod local;
pub mod ollama_manager;
pub mod mistral;
pub mod builtin_tools;
pub mod structured;
pub mod summarize;
pub mod context_window;
pub mod hyde;

pub use types::*;

use anyhow::Result;
use futures_util::Stream;
use std::pin::Pin;

pub type BoxStream = Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>;

pub fn provider_for_model(model: &str) -> Provider {
    // Explicit prefixes set by the model picker for user-configured
    // providers. As of v0.6.0 Mistral has a dedicated provider path
    // (parallel_tool_calls / safe_prompt / prompt_cache_key) and no
    // longer rides the generic OpenAI-compat shim.
    if model.starts_with("mistral:") {
        return Provider::Mistral;
    }
    if model.starts_with("openai:") || model.starts_with("local:") {
        return Provider::OpenAI;
    }
    if model.starts_with("claude") {
        return Provider::Claude;
    }
    if model.starts_with("gemini") {
        return Provider::Gemini;
    }
    // Legacy fallback: env-configured names or "localllm-*" prefix.
    let main = std::env::var("VLLM_MAIN_MODEL").unwrap_or_default();
    let light = std::env::var("VLLM_LIGHT_MODEL").unwrap_or_default();
    if (!main.is_empty() && model == main)
        || (!light.is_empty() && model == light)
        || model.starts_with("localllm")
    {
        return Provider::OpenAI;
    }
    Provider::Gemini
}

/// Strip the `openai:` / `local:` / `mistral:` prefix from a model id
/// when present.
pub fn strip_model_prefix(model: &str) -> &str {
    model
        .strip_prefix("openai:")
        .or_else(|| model.strip_prefix("local:"))
        .or_else(|| model.strip_prefix("mistral:"))
        .unwrap_or(model)
}

/// Best-effort detection of models that handle a long `tools` schema
/// reliably. Used by the chat dispatcher to decide whether to inject
/// **MCP** tool schemas alongside the always-on builtin tools.
///
/// Gating here is conservative on purpose:
///   - Big-3 cloud models (Claude / Gemini / GPT) — yes. Their
///     function-calling implementations all handle 20-30 tool
///     schemas without confusion.
///   - `openai:` BYO endpoints — yes. The user pointed us at an
///     OpenAI-shaped server they trust; assume it speaks tools.
///   - `local:` endpoints — no. These are typically small Ollama /
///     llama.cpp models (3B-13B) that get distracted by long tool
///     schemas; we observed gemma3 and llama3.2:3b in particular
///     emit malformed JSON when given >5 tools. Power users can
///     opt in via the `COVE_FORCE_MCP_TOOLS=1` env override.
///   - Unknown / unconfigured — no, fail closed.
///
/// The system prompt always summarises MCP servers as text (see
/// `build_mcp_system_prompt`) regardless of this gate, so a model
/// that doesn't get the tool schemas still knows the servers exist
/// and can ask the user to invoke them.
pub fn supports_mcp_tools(model: &str) -> bool {
    if crate::product::env_var("FORCE_MCP_TOOLS")
        .map(|v| matches!(v.trim(), "1" | "true" | "yes"))
        .unwrap_or(false)
    {
        return true;
    }
    let raw = model.to_ascii_lowercase();
    if raw.starts_with("local:") {
        return false;
    }
    if raw.starts_with("openai:") || raw.starts_with("mistral:") {
        // Mistral's function-calling (mistral-large / -small) handles
        // the MCP tool-schema set as reliably as the big-3 clouds.
        return true;
    }
    let m = strip_model_prefix(&raw);
    m.starts_with("claude")
        || m.starts_with("gemini")
        || m.starts_with("gpt-")
}

/// Best-effort detection of multimodal (vision-capable) models.
/// Used to decide whether to send rendered PDF pages to the LLM.
pub fn is_vision_capable(model: &str) -> bool {
    let m = strip_model_prefix(model).to_lowercase();
    m.contains("gemma3")
        || m.contains("gemma-3")
        || m.contains("gpt-4o")
        || m.contains("gpt-4-vision")
        || m.contains("gpt-4-turbo")
        || m.contains("claude")
        || m.contains("gemini")
        || m.contains("llava")
        || m.contains("llama3.2-vision")
        || m.contains("qwen2-vl")
        || m.contains("qwen2.5-vl")
        || m.contains("pixtral")
        || m.contains("vision")
}

pub async fn stream_chat(params: StreamParams) -> Result<BoxStream> {
    match provider_for_model(&params.model) {
        Provider::Claude => claude::stream(params).await,
        Provider::OpenAI => local::stream(params).await,
        Provider::Gemini => gemini::stream(params).await,
        Provider::Mistral => mistral::stream(params).await,
    }
}

pub async fn complete_text(model: &str, system: Option<&str>, user: &str) -> Result<String> {
    let params = StreamParams {
        model: model.to_string(),
        system_prompt: system.unwrap_or("").to_string(),
        system_volatile: String::new(),
        messages: vec![Message::user(user.to_string())],
        tools: vec![],
        max_iterations: 1,
        enable_thinking: false,
        local_config: None,
        claude_api_key: None,
        gemini_api_key: None,
        gemini_region: None,
        chat_id: None,
        mistral_opts: None,
        response_schema: None,
    };
    match provider_for_model(model) {
        Provider::Claude => claude::complete(params).await,
        Provider::OpenAI => local::complete(params).await,
        Provider::Gemini => gemini::complete(params).await,
        Provider::Mistral => mistral::complete(params).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_routing_explicit_prefixes() {
        assert_eq!(provider_for_model("openai:gpt-4o"), Provider::OpenAI);
        assert_eq!(provider_for_model("local:llama3"), Provider::OpenAI);
    }

    #[test]
    fn provider_routing_claude_family() {
        assert_eq!(provider_for_model("claude-opus-4-7"), Provider::Claude);
        assert_eq!(provider_for_model("claude-3-haiku"), Provider::Claude);
    }

    #[test]
    fn provider_routing_gemini_family() {
        assert_eq!(provider_for_model("gemini-2.5-pro"), Provider::Gemini);
        assert_eq!(provider_for_model("gemini-3-flash-preview"), Provider::Gemini);
        assert_eq!(provider_for_model("gemini-3.5-flash"), Provider::Gemini);
    }

    #[test]
    fn provider_routing_unknown_falls_back_to_gemini() {
        // SAFETY: tests run sequentially per default test binary, but we
        // use unique env-var names so we don't fight other tests.
        unsafe { std::env::remove_var("VLLM_MAIN_MODEL") };
        unsafe { std::env::remove_var("VLLM_LIGHT_MODEL") };
        assert_eq!(provider_for_model("foobar"), Provider::Gemini);
    }

    #[test]
    fn strip_model_prefix_handles_known_prefixes() {
        assert_eq!(strip_model_prefix("openai:gpt-4o"), "gpt-4o");
        assert_eq!(strip_model_prefix("local:llama3"), "llama3");
        // No prefix → unchanged.
        assert_eq!(strip_model_prefix("claude-opus-4-7"), "claude-opus-4-7");
    }

    #[test]
    fn supports_mcp_tools_capability_table() {
        // All three groups mutate the process-global COVE_FORCE_MCP_TOOLS
        // env var, so they live in one test: split across separate #[test]s
        // they race under the parallel test runner.

        // Force override ON: even local/unknown models report true.
        unsafe { std::env::set_var(crate::product::env_var_name("FORCE_MCP_TOOLS"), "1") };
        assert!(supports_mcp_tools("local:gemma3"));
        assert!(supports_mcp_tools("foobar-7b"));
        unsafe { std::env::remove_var(crate::product::env_var_name("FORCE_MCP_TOOLS")) };

        // Default table: known cloud models report true.
        assert!(supports_mcp_tools("claude-opus-4-7"));
        assert!(supports_mcp_tools("claude-sonnet-4-6"));
        assert!(supports_mcp_tools("gemini-2.5-pro"));
        assert!(supports_mcp_tools("gemini-2.5-flash"));
        assert!(supports_mcp_tools("gemini-3.5-flash"));
        assert!(supports_mcp_tools("gpt-4o"));
        assert!(supports_mcp_tools("gpt-4.1"));
        assert!(supports_mcp_tools("openai:gpt-4o"));

        // Default table: local: prefix and bare unknown / vLLM names are
        // OFF until the user opts in.
        assert!(!supports_mcp_tools("local:llama3.2:3b"));
        assert!(!supports_mcp_tools("local:gemma3"));
        assert!(!supports_mcp_tools("localllm-main"));
        assert!(!supports_mcp_tools("foobar-7b"));
    }

    #[test]
    fn vision_capability_known_models() {
        assert!(is_vision_capable("gpt-4o"));
        assert!(is_vision_capable("claude-opus-4-7"));
        assert!(is_vision_capable("gemini-2.5-pro"));
        assert!(is_vision_capable("gemini-3.5-flash"));
        assert!(is_vision_capable("gemma-3-27b"));
        assert!(is_vision_capable("local:gemma3:4b"));
        assert!(is_vision_capable("openai:gpt-4-turbo"));
    }

    #[test]
    fn vision_capability_text_only_models() {
        assert!(!is_vision_capable("llama3:7b"));
        assert!(!is_vision_capable("mistral-small"));
        assert!(!is_vision_capable("gpt-4")); // pre-vision
    }
}
