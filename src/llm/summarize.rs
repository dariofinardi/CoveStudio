//! Conversation-history summarization buffer.
//!
//! When the running chat history approaches the model's context window
//! we compress the oldest turns into a single synthetic system message
//! and forward only that + the most recent turns. The user's UI still
//! shows every message; only the payload to the LLM is compressed.
//!
//! Trigger: the *whole* prompt — system prefix (instructions + attached
//! documents) + volatile KB block + conversation history + reply
//! headroom — exceeds `0.8 × model_window`. Measuring only the history
//! would miss the dominant cost in a document-heavy chat, where the
//! attached-document text in the system prompt is far larger than the
//! turns.
//! Strategy: keep the last `KEEP_RECENT_TURNS` turns verbatim, compress
//! everything older with one LLM call, replace those turns with a
//! `Role::System`-style message tagged "EARLIER CONVERSATION SUMMARY".

use anyhow::Result;

use super::types::{Message, Role};

/// How many recent (user, assistant) turns we always keep verbatim,
/// regardless of token budget. Two pairs is enough for follow-up
/// pronouns and references ("now redo it with…", "what did you mean by
/// X?"); going lower starts to hurt coherence.
pub const KEEP_RECENT_TURNS: usize = 4;

/// Ratio of the model's context window we allow the whole prompt to
/// fill before compressing older turns. The estimate already includes
/// the system prefix, the volatile KB block and a reply reserve, so 0.8
/// is a real "80% full" line, not a history-only heuristic.
pub const TRIGGER_RATIO: f32 = 0.8;

/// Tokens kept free for the model's own reply. The window has to hold
/// the whole prompt *and* the answer; reserving this keeps a long answer
/// from overflowing a window the prompt already nearly filled.
pub const REPLY_RESERVE_TOKENS: usize = 4096;

/// Rough characters-per-token for European languages with the app's
/// typical legal text. e5/Llama-3 tokenizers land around 3.8–4.2 chars
/// per token; we use 4 as a portable heuristic. We could load the
/// actual tokenizer of the target model but the cost/complexity isn't
/// justified for a heuristic that's only used to decide *whether* to
/// summarize — the summarizer itself is bounded by KEEP_RECENT_TURNS.
const CHARS_PER_TOKEN: usize = 4;

/// Cheap token estimation. Don't use this for billing — only for the
/// "should we summarize?" decision.
pub fn estimate_tokens(text: &str) -> usize {
    (text.chars().count() + CHARS_PER_TOKEN - 1) / CHARS_PER_TOKEN
}

/// Total estimated tokens across a list of messages.
pub fn estimate_messages_tokens(msgs: &[Message]) -> usize {
    msgs.iter()
        .map(|m| {
            estimate_tokens(&m.content)
                + estimate_tokens(m.tool_name.as_deref().unwrap_or(""))
                + 8 // overhead per message (role markers, separators)
        })
        .sum()
}

/// Per-model context window in tokens. Numbers are deliberately
/// conservative — when in doubt, return a smaller value so we trigger
/// summarization slightly early rather than overflowing.
///
/// Patterns:
///  - `claude-opus-4-7` and `claude-sonnet-4-6` are 1M-context
///    variants; older Claude is 200k.
///  - Gemini 2.5 Pro is 2M; 2.5 Flash is 1M.
///  - GPT-4o family: 128k.
///  - Local / Ollama: highly variable but usually 4k–32k. 8k is a
///    safe default — users with bigger windows tend to use cloud.
pub fn context_window_tokens(model: &str) -> usize {
    let m = model.to_ascii_lowercase();
    let m = m.strip_prefix("openai:").unwrap_or(&m);
    let m = m.strip_prefix("local:").unwrap_or(m);

    // Claude
    if m.starts_with("claude-opus-4-7") || m.starts_with("claude-sonnet-4-6") {
        return 1_000_000;
    }
    if m.starts_with("claude-") {
        return 200_000;
    }

    // Gemini
    if m == "gemini-2.5-pro" || m.contains("gemini-3.1-pro") {
        return 1_000_000;
    }
    if m.starts_with("gemini-2.5-flash")
        || m.starts_with("gemini-3-flash")
        // Every Gemini 3.x Flash so far ships a 1M window (3.5, 3.6, 3.7,
        // 3.8); this keeps new minor versions working before the catalogue
        // lists them.
        || m.starts_with("gemini-3.")
    {
        return 1_000_000;
    }
    if m.starts_with("gemini-1.5-pro") {
        return 2_000_000;
    }
    if m.starts_with("gemini-") {
        return 32_000;
    }

    // OpenAI
    if m.starts_with("gpt-4o") || m.starts_with("gpt-4.1") {
        return 128_000;
    }
    if m.starts_with("gpt-4-turbo") {
        return 128_000;
    }
    if m.starts_with("gpt-4") {
        return 8_192;
    }

    // Local / unknown
    8_192
}

/// Should we summarize, given the running history, the target model,
/// and `system_overhead_tokens` — the estimated size of everything that
/// is NOT the message history (system prefix + volatile KB block). The
/// caller measures that because it owns those strings; here we add the
/// history and a reply reserve and compare against `0.8 × window`.
pub fn should_summarize(messages: &[Message], model: &str, system_overhead_tokens: usize) -> bool {
    should_summarize_within(messages, context_window_tokens(model), system_overhead_tokens)
}

/// Same as [`should_summarize`] with an explicit window, e.g. the one
/// reported by a local Ollama server (see `super::context_window`).
pub fn should_summarize_within(messages: &[Message], window: usize, system_overhead_tokens: usize) -> bool {
    let used =
        system_overhead_tokens + estimate_messages_tokens(messages) + REPLY_RESERVE_TOKENS;
    let trigger = (window as f32 * TRIGGER_RATIO) as usize;
    used > trigger && messages.len() > KEEP_RECENT_TURNS * 2 + 2
}

/// Split a message list into (older, newer) where `newer` is the last
/// `KEEP_RECENT_TURNS` user/assistant pairs (plus any trailing
/// non-pair message), and `older` is everything before that point.
fn split_at_recent_window(messages: &[Message]) -> (&[Message], &[Message]) {
    // Walk the tail looking for KEEP_RECENT_TURNS user messages; keep
    // everything from the earliest of those onwards as "newer".
    let mut user_seen = 0usize;
    let mut split_idx = messages.len();
    for (idx, msg) in messages.iter().enumerate().rev() {
        if matches!(msg.role, Role::User) {
            user_seen += 1;
            if user_seen >= KEEP_RECENT_TURNS {
                split_idx = idx;
                break;
            }
        }
    }
    (&messages[..split_idx], &messages[split_idx..])
}

/// Subset of credentials needed to fire a one-shot summarizer call —
/// pulled out as a small struct so the chat dispatcher can pass it in
/// without constructing a full `StreamParams` early.
#[derive(Debug, Clone, Default)]
pub struct SummarizerCreds {
    pub local_config: Option<super::types::LocalConfig>,
    pub claude_api_key: Option<String>,
    pub gemini_api_key: Option<String>,
    pub gemini_region: Option<String>,
}

/// Run the summarizer LLM call and return a single `system`-role
/// message that should replace the older turns in the prompt.
///
/// `model` is the model used for the *summarization* — we deliberately
/// reuse the user-selected model so the language and style match. If
/// the user prefers a cheaper summarizer they can wire `title_model`
/// here in a future revision.
pub async fn summarize_old_turns(
    older: &[Message],
    target_model: &str,
    creds: &SummarizerCreds,
) -> Result<Message> {
    // Render the older turns as a transcript the LLM can summarize.
    // Tools, citations, system prompts are dropped — only user/
    // assistant prose makes it in. This bounds the summarizer's own
    // context size (it never sees the full attached-doc system prompt).
    let mut transcript = String::with_capacity(2048);
    for m in older {
        let label = match m.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
            Role::Tool => continue,
            Role::System => continue,
        };
        if m.content.trim().is_empty() {
            continue;
        }
        transcript.push_str(label);
        transcript.push_str(": ");
        transcript.push_str(&m.content);
        transcript.push_str("\n\n");
    }

    let prompt = format!(
        "Riassumi in italiano il dialogo qui sotto in 1–3 paragrafi compatti, \
         preservando: nomi, date, decisioni prese, fatti accertati, e domande \
         lasciate aperte. Indica esplicitamente QUALI documenti e quali \
         sezioni/clausole (per nome o numero) sono stati discussi, così che \
         si possano ri-consultare se servono dettagli — ma NON includere il \
         testo dei documenti né le sezioni di tool-call. Scrivi in modo che \
         chi legge il riassunto possa continuare la conversazione \
         coerentemente.\n\n\
         === Dialogo ===\n{transcript}=== Fine dialogo ===",
    );

    // Reuse the credentials from the running request so we don't have
    // to re-fetch them. The summarizer call is non-streaming via
    // `complete` on the same model the user selected.
    let params = super::types::StreamParams {
        model: target_model.to_string(),
        system_prompt:
            "You are a concise legal-meeting note-taker. Output only the requested summary."
                .to_string(),
        system_volatile: String::new(),
        messages: vec![Message::user(prompt)],
        tools: vec![],
        max_iterations: 1,
        enable_thinking: false,
        local_config: creds.local_config.clone(),
        claude_api_key: creds.claude_api_key.clone(),
        gemini_api_key: creds.gemini_api_key.clone(),
        gemini_region: creds.gemini_region.clone(),
        // Summarisation is one-shot — no cache benefit.
        chat_id: None,
        mistral_opts: None,
        response_schema: None,
    };

    let summary = match super::provider_for_model(target_model) {
        super::Provider::Claude => super::claude::complete(params).await?,
        super::Provider::OpenAI => super::local::complete(params).await?,
        super::Provider::Gemini => super::gemini::complete(params).await?,
        super::Provider::Mistral => super::mistral::complete(params).await?,
    };

    Ok(Message::system(format!(
        "EARLIER CONVERSATION SUMMARY (compressed to fit context window):\n\n{}\n\n\
         This is a lossy summary of earlier turns. If the user asks for exact \
         wording, figures, dates or clauses that are not stated above, do NOT \
         answer from this summary — call `read_document` or `find_in_document` \
         on the relevant `doc-N` to re-read the source, then answer from it.",
        summary.trim()
    )))
}

/// Apply summarization if the trigger fires. Returns the (possibly
/// modified) message list to send to the LLM. The returned list is
/// always safe to use directly; on errors the original list is
/// returned untouched (failing-open is preferred to a hard 500 mid-
/// chat — the worst case is the model sees fewer turns or the request
/// truncates server-side).
pub async fn maybe_compress_history(
    messages: Vec<Message>,
    target_model: &str,
    creds: &SummarizerCreds,
    system_overhead_tokens: usize,
    context_window_tokens: usize,
) -> Vec<Message> {
    if !should_summarize_within(&messages, context_window_tokens, system_overhead_tokens) {
        return messages;
    }
    let (older, newer) = split_at_recent_window(&messages);
    if older.is_empty() {
        return messages;
    }
    let older_owned: Vec<Message> = older.to_vec();
    let newer_owned: Vec<Message> = newer.to_vec();

    tracing::info!(
        "[summarize] compressing {} older turns (≈{} tokens) for model {}",
        older_owned.len(),
        estimate_messages_tokens(&older_owned),
        target_model,
    );

    match summarize_old_turns(&older_owned, target_model, creds).await {
        Ok(summary_msg) => {
            let mut compressed = Vec::with_capacity(newer_owned.len() + 1);
            compressed.push(summary_msg);
            compressed.extend(newer_owned);
            compressed
        }
        Err(e) => {
            tracing::warn!("[summarize] failed: {e} — sending raw history");
            let mut original = Vec::with_capacity(older_owned.len() + newer_owned.len());
            original.extend(older_owned);
            original.extend(newer_owned);
            original
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_threshold_no_split() {
        let msgs = vec![
            Message::user("Ciao"),
            Message::assistant("Salve"),
        ];
        assert!(!should_summarize(&msgs, "gemini-2.5-flash", 0));
    }

    #[test]
    fn small_context_triggers() {
        let big = "x".repeat(60_000); // ~15k tokens
        let mut msgs = vec![];
        for _ in 0..6 {
            msgs.push(Message::user(big.clone()));
            msgs.push(Message::assistant("ok"));
        }
        // gpt-4 → 8k window → should trigger
        assert!(should_summarize(&msgs, "gpt-4", 0));
    }

    #[test]
    fn system_overhead_triggers_even_with_short_history() {
        // A short conversation but a huge attached-document system prompt:
        // history-only measurement would never fire, the overhead-aware
        // one must. 12 short turns clear the message-count floor; the
        // overhead (≈180k tokens) blows past 0.8 × 200k.
        let mut msgs = vec![];
        for i in 0..6 {
            msgs.push(Message::user(format!("domanda {i}")));
            msgs.push(Message::assistant(format!("risposta {i}")));
        }
        assert!(!should_summarize(&msgs, "claude-3-5-sonnet", 0));
        assert!(should_summarize(&msgs, "claude-3-5-sonnet", 180_000));
    }

    #[test]
    fn split_keeps_recent_pairs() {
        let mut msgs = vec![];
        for i in 0..10 {
            msgs.push(Message::user(format!("u{i}")));
            msgs.push(Message::assistant(format!("a{i}")));
        }
        let (older, newer) = split_at_recent_window(&msgs);
        // Should retain the last 4 user msgs (i.e. last 8 messages incl. assistants).
        let recent_users = newer.iter().filter(|m| matches!(m.role, Role::User)).count();
        assert_eq!(recent_users, KEEP_RECENT_TURNS);
        assert!(!older.is_empty());
    }

    #[test]
    fn estimate_tokens_handles_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn estimate_tokens_rounds_up() {
        // 1 char → 1 token (4-char rounding).
        assert_eq!(estimate_tokens("a"), 1);
        // 4 chars → 1 token.
        assert_eq!(estimate_tokens("abcd"), 1);
        // 5 chars → 2 tokens (rounded up).
        assert_eq!(estimate_tokens("abcde"), 2);
    }

    #[test]
    fn estimate_messages_tokens_includes_overhead() {
        let msgs = vec![Message::user("x")]; // 1 char content
        let est = estimate_messages_tokens(&msgs);
        // 1 (chars/4 ceil) + 0 (no tool_name) + 8 (overhead) = 9
        assert_eq!(est, 9);
    }

    #[test]
    fn context_window_claude_long() {
        assert_eq!(context_window_tokens("claude-opus-4-7"), 1_000_000);
        assert_eq!(context_window_tokens("claude-sonnet-4-6"), 1_000_000);
        assert_eq!(context_window_tokens("claude-3-5-sonnet"), 200_000);
    }

    #[test]
    fn context_window_gemini() {
        assert_eq!(context_window_tokens("gemini-2.5-pro"), 1_000_000);
        assert_eq!(context_window_tokens("gemini-2.5-flash"), 1_000_000);
        assert_eq!(context_window_tokens("gemini-3-flash-preview"), 1_000_000);
        assert_eq!(context_window_tokens("gemini-3.5-flash"), 1_000_000);
        assert_eq!(context_window_tokens("gemini-3.7-flash"), 1_000_000);
        assert_eq!(context_window_tokens("gemini-3.8-flash"), 1_000_000);
        assert_eq!(context_window_tokens("gemini-1.5-pro"), 2_000_000);
        assert_eq!(context_window_tokens("gemini-pro"), 32_000);
    }

    #[test]
    fn context_window_openai_and_legacy() {
        assert_eq!(context_window_tokens("gpt-4o-mini"), 128_000);
        assert_eq!(context_window_tokens("gpt-4.1"), 128_000);
        assert_eq!(context_window_tokens("gpt-4-turbo"), 128_000);
        assert_eq!(context_window_tokens("gpt-4"), 8_192);
    }

    #[test]
    fn context_window_local_default_is_8k() {
        assert_eq!(context_window_tokens("llama3:7b"), 8_192);
        // Stripped prefixes still resolve.
        assert_eq!(context_window_tokens("local:llama3"), 8_192);
        assert_eq!(context_window_tokens("openai:gpt-4o"), 128_000);
    }

    #[test]
    fn split_at_recent_window_with_few_messages() {
        // Less than KEEP_RECENT_TURNS users in the input. The function
        // never finds the Nth-from-end user message, so split_idx stays
        // at messages.len() — i.e. EVERYTHING goes into `older` and
        // `newer` is empty. In practice `maybe_compress_history` only
        // calls this when should_summarize() is true, and that gate
        // requires len > 10 messages, so the corner case is harmless.
        let msgs = vec![
            Message::user("u1"),
            Message::assistant("a1"),
            Message::user("u2"),
        ];
        let (older, newer) = split_at_recent_window(&msgs);
        assert_eq!(older.len(), 3);
        assert!(newer.is_empty());
    }

    #[test]
    fn split_at_recent_window_with_exactly_keep_recent_users() {
        // Exactly 4 users → the earliest user becomes the split point,
        // so older starts at index 0 (length 0) and newer is everything.
        let mut msgs = vec![];
        for i in 0..4 {
            msgs.push(Message::user(format!("u{i}")));
            msgs.push(Message::assistant(format!("a{i}")));
        }
        let (older, newer) = split_at_recent_window(&msgs);
        assert!(older.is_empty(), "no older content with exactly KEEP_RECENT_TURNS users");
        assert_eq!(newer.len(), msgs.len());
    }

    #[test]
    fn should_summarize_requires_minimum_message_count() {
        // Even with a tiny window, a 2-message conversation must not trigger.
        let big = "x".repeat(100_000);
        let msgs = vec![Message::user(big), Message::assistant("ok")];
        assert!(!should_summarize(&msgs, "gpt-4", 0));
    }

    #[test]
    fn maybe_compress_returns_unchanged_when_below_threshold() {
        // Exercise the failing-open path that requires no LLM call.
        let msgs = vec![
            Message::user("Ciao"),
            Message::assistant("Salve"),
        ];
        let creds = SummarizerCreds::default();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let out = runtime.block_on(maybe_compress_history(
            msgs.clone(),
            "gemini-2.5-flash",
            &creds,
            0,
            context_window_tokens("gemini-2.5-flash"),
        ));
        assert_eq!(out.len(), msgs.len());
    }
}
