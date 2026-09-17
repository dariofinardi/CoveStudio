// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
//! Removes tool-result JSON that a model pasted into its own answer.
//!
//! The document tools (`generate_docx`, `generate_xlsx`, `edit_document`,
//! `read_document`) return a small JSON object; the UI turns it into a
//! download card and the prose is supposed to describe the document in
//! words. Weaker models — small local ones above all — instead echo the
//! raw object (`{"doc_id":"doc-3","filename":"…"}`) as visible text, so
//! the user sees machine output and, if the card is missing for any
//! reason, believes copy/paste is the only way to get the file.
//!
//! The system prompt forbids this, but a prompt rule is a request, not a
//! guarantee: this pass is the deterministic half. It strips whole JSON
//! objects that look like a tool result, keeping every other line as is.

use std::borrow::Cow;

/// Placeholder left where an object was removed, so [`tidy`] can drop a
/// line that held nothing else. Never reaches the user.
const MARK: char = '\u{0}';

/// Keys that identify our tool results. A JSON object is dropped only if
/// it carries at least one of these *and* nothing but known keys, so a
/// snippet the user actually asked for (a config sample, an API payload)
/// survives.
const RESULT_KEYS: &[&str] = &["doc_id", "document_id", "edits_applied", "download_url"];

/// Keys a tool result may legitimately contain besides [`RESULT_KEYS`].
const ALLOWED_KEYS: &[&str] = &[
    "filename",
    "doc_label",
    "file_type",
    "unresolved_placeholders",
    "warning",
    "note",
    "unit",
    "total",
    "first",
    "last",
    "returned_from",
    "returned_to",
    "complete",
    "next_page_from",
    "match_count",
    "matches",
    "query",
    "text",
    "pages",
    "page",
    "size_bytes",
    "workflow_id",
    "title",
    "find",
    "replace",
    "hits",
];

/// Strips echoed tool-result objects from an answer. Returns
/// [`Cow::Borrowed`] when there was nothing to remove, so the caller can
/// skip the `content_replace` round-trip.
pub fn strip_echoed_tool_results(text: &str) -> Cow<'_, str> {
    if !text.contains('{') {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut stripped = false;
    while let Some(start) = rest.find('{') {
        let (head, tail) = rest.split_at(start);
        out.push_str(head);
        match object_end(tail) {
            Some(end) if is_tool_result(&tail[..end]) => {
                // The marker lets `tidy` tell a line that held only the
                // object (drop it) from one that had prose around it.
                out.push(MARK);
                stripped = true;
                rest = &tail[end..];
            }
            // Not a tool result (or an unterminated brace): keep the
            // brace and carry on from the next character.
            _ => {
                out.push('{');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    if !stripped {
        return Cow::Borrowed(text);
    }
    Cow::Owned(tidy(&out))
}

/// Byte offset just past the `}` matching the `{` at the start of `s`,
/// ignoring braces inside JSON strings.
fn object_end(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (i, b) in bytes.iter().enumerate() {
        if in_string {
            match b {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// True when `candidate` parses as one of our tool results.
fn is_tool_result(candidate: &str) -> bool {
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(candidate)
    else {
        return false;
    };
    if map.is_empty() {
        return false;
    }
    let has_marker = map.keys().any(|k| RESULT_KEYS.contains(&k.as_str()));
    let only_known = map
        .keys()
        .all(|k| RESULT_KEYS.contains(&k.as_str()) || ALLOWED_KEYS.contains(&k.as_str()));
    has_marker && only_known
}

/// Cleans up what the removal leaves behind: a line that held only the
/// object, the code fence that wrapped it, runs of blank lines, trailing
/// spaces. Removed objects are marked with [`MARK`].
fn tidy(text: &str) -> String {
    let mut kept: Vec<String> = Vec::new();
    for line in text.lines() {
        if !line.contains(MARK) {
            kept.push(line.to_string());
            continue;
        }
        let rest = line.replace(MARK, "");
        // Nothing but the object was on this line: the line goes too.
        if !rest.trim().is_empty() {
            kept.push(rest);
        }
    }

    // A fence pair that now wraps nothing at all.
    let mut lines: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < kept.len() {
        if kept[i].trim().starts_with("```") {
            let mut j = i + 1;
            while j < kept.len() && kept[j].trim().is_empty() {
                j += 1;
            }
            if j < kept.len() && kept[j].trim() == "```" {
                i = j + 1;
                continue;
            }
        }
        lines.push(kept[i].clone());
        i += 1;
    }

    let mut out = String::with_capacity(text.len());
    let mut blank_run = 0usize;
    for line in lines {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            blank_run += 1;
            if blank_run > 1 || out.is_empty() {
                continue;
            }
        } else {
            blank_run = 0;
        }
        out.push_str(trimmed);
        out.push('\n');
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_a_pasted_docx_result() {
        let text = "Ho creato il documento.\n\n{\"doc_id\":\"doc-3\",\"filename\":\"Contratto.docx\"}\n\nLo trovi qui sopra.";
        let out = strip_echoed_tool_results(text);
        assert!(!out.contains("doc_id"), "{out}");
        assert!(out.contains("Ho creato il documento."));
        assert!(out.contains("Lo trovi qui sopra."));
    }

    #[test]
    fn strips_a_fenced_result_and_its_fences() {
        let text = "Fatto.\n\n```json\n{\"doc_id\": \"doc-1\", \"filename\": \"A.docx\", \"note\": \"call read_document\"}\n```\n\nSegue la sintesi.";
        let out = strip_echoed_tool_results(text);
        assert!(!out.contains("doc_id"), "{out}");
        assert!(!out.contains("```"), "{out}");
        assert!(out.contains("Segue la sintesi."));
    }

    #[test]
    fn strips_an_edit_result() {
        let text = "{\"doc_id\":\"doc-2\",\"document_id\":\"0b9f\",\"filename\":\"B.docx\",\"edits_applied\":[{\"find\":\"a\",\"replace\":\"b\",\"hits\":1}]}";
        assert_eq!(strip_echoed_tool_results(text), "");
    }

    #[test]
    fn keeps_unrelated_json_the_user_asked_for() {
        let text = "Esempio di configurazione:\n\n{\"base_url\":\"http://localhost:11434/v1\",\"model\":\"qwen3\"}\n";
        assert!(matches!(strip_echoed_tool_results(text), Cow::Borrowed(_)));
    }

    #[test]
    fn keeps_prose_with_braces() {
        let text = "Usa la sintassi {nome} nel modello, non { da solo.";
        assert!(matches!(strip_echoed_tool_results(text), Cow::Borrowed(_)));
    }

    #[test]
    fn keeps_a_json_object_that_only_looks_similar() {
        // Carries a marker key but also unknown ones: the user is
        // probably discussing an API payload.
        let text = "{\"doc_id\":\"x\",\"user_id\":\"u1\",\"acl\":[\"read\"]}";
        assert!(matches!(strip_echoed_tool_results(text), Cow::Borrowed(_)));
    }

    #[test]
    fn leaves_untouched_text_borrowed() {
        let text = "Nessun JSON qui.";
        assert!(matches!(strip_echoed_tool_results(text), Cow::Borrowed(_)));
    }

    #[test]
    fn strips_several_objects_in_one_answer() {
        let text = "A\n{\"doc_id\":\"doc-1\",\"filename\":\"A.docx\"}\nB\n{\"doc_id\":\"doc-2\",\"filename\":\"B.docx\"}\nC";
        let out = strip_echoed_tool_results(text);
        assert_eq!(out, "A\nB\nC");
    }
}
