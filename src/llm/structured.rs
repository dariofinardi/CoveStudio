// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
//! Asking a model for **data** instead of prose.
//!
//! Every provider can constrain its answer to a JSON Schema — Gemini
//! through `generationConfig.responseSchema`, Mistral and the
//! OpenAI-compatible endpoints through `response_format`, Claude by
//! forcing a tool call. They disagree on the wire; this module is where
//! that stops mattering: a caller hands over a schema and gets back a
//! parsed value, or an error that says what arrived instead.
//!
//! Why it exists: the tabular extractor asks for a value in prose and
//! keeps whatever comes back, and that is survivable for one cell. The
//! work this module was built for — reading a hundred questions out of
//! an insurer's questionnaire, each with a type, a label and a
//! provenance — is not survivable that way. One malformed answer in a
//! hundred is a broken run, and "reply with only JSON" in a prompt does
//! not prevent it.
//!
//! **A schema is a request, not a guarantee.** A model can still return
//! an object that satisfies the shape and says something false. This
//! module guarantees the shape; whether the content is right is the
//! caller's problem, and usually the reviewer's.

use anyhow::{anyhow, Result};
use serde_json::Value;

use super::types::{Message, StreamParams};
use super::{claude, gemini, local, mistral, provider_for_model, Provider};

/// Everything a structured call needs beyond the schema itself.
///
/// Credentials are per-user in this product, so they travel with the
/// request rather than living in a global.
#[derive(Default, Clone)]
pub struct StructuredCreds {
    pub claude_api_key: Option<String>,
    pub gemini_api_key: Option<String>,
    pub gemini_region: Option<String>,
    pub local_config: Option<super::types::LocalConfig>,
    pub mistral_opts: Option<super::types::MistralOpts>,
}

/// Asks `model` for a value matching `schema`.
///
/// `system` frames the task, `user` carries the material. The answer is
/// parsed before it is returned: a caller never sees a string it has to
/// guess about.
pub async fn complete_json(
    model: &str,
    system: &str,
    user: &str,
    schema: Value,
    creds: &StructuredCreds,
) -> Result<Value> {
    let params = StreamParams {
        model: model.to_string(),
        system_prompt: system.to_string(),
        system_volatile: String::new(),
        messages: vec![Message::user(user.to_string())],
        tools: vec![],
        max_iterations: 1,
        enable_thinking: false,
        local_config: creds.local_config.clone(),
        claude_api_key: creds.claude_api_key.clone(),
        gemini_api_key: creds.gemini_api_key.clone(),
        gemini_region: creds.gemini_region.clone(),
        // One-shot: no chat to anchor a prompt cache to.
        chat_id: None,
        mistral_opts: creds.mistral_opts.clone(),
        response_schema: Some(schema),
    };

    let started = std::time::Instant::now();
    let raw = match provider_for_model(model) {
        Provider::Claude => claude::complete(params).await,
        Provider::OpenAI => local::complete(params).await,
        Provider::Gemini => gemini::complete(params).await,
        Provider::Mistral => mistral::complete(params).await,
    }?;

    let value = parse_json_answer(&raw)?;
    tracing::info!(
        "[llm/structured] {model} answered in {:?} ({} chars)",
        started.elapsed(),
        raw.len()
    );
    Ok(value)
}

/// Parses what a provider returned when a schema was in force.
///
/// Even with structured output enabled, models wrap the object in a
/// Markdown fence often enough to be worth handling — and a fence is
/// unambiguous, so peeling it is repair, not guesswork. Anything beyond
/// that is an error: silently "fixing" a malformed answer is how a run
/// ends up with data nobody can explain.
pub fn parse_json_answer(raw: &str) -> Result<Value> {
    let text = raw.trim();
    if text.is_empty() {
        return Err(anyhow!("the model returned nothing"));
    }
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        return Ok(v);
    }
    if let Some(inner) = strip_code_fence(text) {
        if let Ok(v) = serde_json::from_str::<Value>(inner.trim()) {
            return Ok(v);
        }
    }
    // The excerpt is deliberately included: a caller reading the log
    // needs to see what came back, not just that it was wrong.
    let excerpt: String = text.chars().take(300).collect();
    Err(anyhow!(
        "the model did not return valid JSON despite a schema being set; got: {excerpt}"
    ))
}

/// Returns the body of a ```…``` fence, if the text is exactly one.
fn strip_code_fence(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("```")?;
    // Skip an optional language tag on the opening line.
    let body = match rest.find('\n') {
        Some(i) => &rest[i + 1..],
        None => return None,
    };
    body.rfind("```").map(|end| &body[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_plain_object() {
        let v = parse_json_answer(r#"{"campi": [{"id": "piva"}]}"#).expect("parses");
        assert_eq!(v["campi"][0]["id"], "piva");
    }

    #[test]
    fn peels_a_markdown_fence() {
        // Models add these even when told not to; the fence is
        // unambiguous, so removing it is repair rather than guessing.
        let v = parse_json_answer("```json\n{\"n\": 3}\n```").expect("parses");
        assert_eq!(v["n"], 3);
    }

    #[test]
    fn peels_a_fence_without_a_language_tag() {
        let v = parse_json_answer("```\n[1, 2]\n```").expect("parses");
        assert_eq!(v[1], 2);
    }

    #[test]
    fn an_empty_answer_is_an_error_not_an_empty_object() {
        let err = parse_json_answer("   ").unwrap_err().to_string();
        assert!(err.contains("nothing"), "{err}");
    }

    #[test]
    fn prose_fails_and_the_error_shows_what_arrived() {
        // The excerpt is the point: without it the log says "invalid
        // JSON" and the reader still has to reproduce the call to learn
        // anything.
        let err = parse_json_answer("Certo! Ecco le domande che ho trovato: ...")
            .unwrap_err()
            .to_string();
        assert!(err.contains("Ecco le domande"), "{err}");
    }

    #[test]
    fn half_an_object_is_not_repaired_into_something_plausible() {
        // A truncated answer means the budget ran out. Guessing the
        // closing braces would hand the caller data the model never
        // produced.
        assert!(parse_json_answer(r#"{"campi": [{"id": "piva""#).is_err());
    }
}
