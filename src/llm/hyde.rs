//! HyDE — Hypothetical Document Embeddings for chat-time retrieval.
//!
//! When `user_settings.hyde_enabled = 1` (migration 0030, surfaced in
//! Settings → Recupero documenti), `retrieve_kb_chunks` in chat.rs
//! invokes [`generate_hypothesis`] to ask the user's currently-active
//! LLM to draft a short pseudo-answer — a 3-4 sentence paragraph that
//! would plausibly answer the query *as if it had been extracted from
//! an authoritative document in the same domain*. Both the original
//! query and the hypothesis are then embedded; the two KNN result sets
//! are merged via Reciprocal Rank Fusion before the usual top-K +
//! distance threshold runs.
//!
//! Why this helps: e5-base similarities match passages whose surface
//! form looks like the embedding input. A user question like "quanto
//! dura il preavviso per recedere dal contratto?" is structurally
//! unlike the actual passages that answer it ("Il recesso è esercitato
//! con preavviso non inferiore a sei mesi…") — cosine drift can be
//! 0.05-0.10 lower than a paraphrased "passage". The hypothesis closes
//! that gap by giving the embedder something shaped like a passage.
//!
//! Domain-awareness: the prompt reuses the v0.4.0 domain prologue
//! (`crate::presets::system_prompt::resolve`) so the hypothesis is
//! drafted in the register of the chat's domain (legal / medical /
//! finance / …). A generic hypothesis would phrase things in everyday
//! Italian; a domain-aware one mirrors the corpus phrasing and improves
//! retrieval where it matters most.

use anyhow::Result;

use super::types::{Message, StreamParams};

/// Maximum characters returned. The hypothesis is only used as a
/// retrieval probe — there is no value in long completions, and Gemini
/// / Claude / OpenAI all bill by the token. ~600 chars ≈ 150 tokens is
/// enough for 3-4 sentences in any European language and keeps the call
/// cheap.
const HYPOTHESIS_MAX_CHARS: usize = 600;

/// Credentials needed to drive the one-shot HyDE call. Mirrors
/// [`super::summarize::SummarizerCreds`] so the caller can pass the
/// same struct it already builds for the rejection-summary path.
#[derive(Debug, Clone, Default)]
pub struct HydeCreds {
    pub local_config: Option<super::types::LocalConfig>,
    pub claude_api_key: Option<String>,
    pub gemini_api_key: Option<String>,
    pub gemini_region: Option<String>,
}

/// Draft a domain-aware pseudo-answer for `user_query`. Returns the
/// raw model output (trimmed). Bounded to `HYPOTHESIS_MAX_CHARS` via
/// a soft prompt request; if the model exceeds it the output is
/// truncated.
///
/// `domain` is the canonical English snake_case domain id (e.g.
/// `legal`, `medical`); `locale` is the user's UI locale. Both flow
/// through `crate::presets::system_prompt::resolve` so the hypothesis
/// is anchored on the appropriate vertical's domain.md preset.
pub async fn generate_hypothesis(
    user_query: &str,
    locale: &str,
    domain: &str,
    target_model: &str,
    creds: &HydeCreds,
) -> Result<String> {
    let domain_body = crate::presets::system_prompt::resolve(locale, domain).unwrap_or_default();
    let lang = crate::presets::system_prompt::language_name_for_locale(locale);

    // System prompt: domain prologue body (if any) + HyDE-specific
    // instructions. Kept compact so the call is fast and cheap.
    let mut system = String::with_capacity(1024);
    if !domain_body.is_empty() {
        system.push_str(&domain_body);
        system.push_str("\n\n---\n\n");
    }
    system.push_str(
        "You generate a SHORT hypothetical passage to seed semantic retrieval. \
         Your output will NOT be shown to the user — it is only embedded and \
         compared against a vector index of real documents. \
         \n\nRules:\n\
         - Write 3 to 4 short sentences (under 600 characters total).\n\
         - Phrase the answer as if it were lifted from an authoritative document \
         in the domain above — same register, same vocabulary the corpus would \
         use.\n\
         - Do NOT preface with \"the answer is\", \"in this case\", or any meta \
         commentary. Output ONLY the hypothetical passage.\n\
         - Do NOT invent specific names, dates, citations, statutes or articles. \
         Use generic placeholders (\"the relevant article\", \"the contract\", \
         \"the date specified\") if needed — the embedder cares about phrasing, \
         not facts.\n",
    );
    system.push_str(&format!(
        "- Write in {lang}.\n"
    ));

    let user_msg = format!(
        "User query (to expand into a hypothetical passage):\n\n{}",
        user_query.trim()
    );

    let params = StreamParams {
        model: target_model.to_string(),
        system_prompt: system,
        system_volatile: String::new(),
        messages: vec![Message::user(user_msg)],
        tools: vec![],
        max_iterations: 1,
        enable_thinking: false,
        local_config: creds.local_config.clone(),
        claude_api_key: creds.claude_api_key.clone(),
        gemini_api_key: creds.gemini_api_key.clone(),
        gemini_region: creds.gemini_region.clone(),
        // HyDE is one-shot — no cache benefit from chat-scoped keys.
        chat_id: None,
        mistral_opts: None,
        response_schema: None,
    };

    let raw = match super::provider_for_model(target_model) {
        super::Provider::Claude => super::claude::complete(params).await?,
        super::Provider::OpenAI => super::local::complete(params).await?,
        super::Provider::Gemini => super::gemini::complete(params).await?,
        super::Provider::Mistral => super::mistral::complete(params).await?,
    };

    let mut trimmed = raw.trim().to_string();
    if trimmed.chars().count() > HYPOTHESIS_MAX_CHARS {
        // Hard cap — protect downstream embedding pass from a runaway
        // model output. The boundary is char-based, not byte-based,
        // to keep multi-byte glyphs intact.
        trimmed = trimmed
            .chars()
            .take(HYPOTHESIS_MAX_CHARS)
            .collect::<String>();
    }

    Ok(trimmed)
}
