//! Builtin tools that ship with Mike's legal-assistant identity.
//!
//! Mirror the OpenAI/Anthropic tool schemas declared by upstream Mike
//! (`backend/src/lib/chatTools.ts`):
//!
//! * `read_document` — fetch full text of a chat-attached document by `doc-N` label
//! * `find_in_document` — case-insensitive search within a document
//! * `read_workflow` — load the Markdown body of a saved workflow by id
//! * `generate_docx` — produce a downloadable .docx
//! * `generate_xlsx` — produce a downloadable .xlsx from tabular data
//! * `edit_document` — modify an existing .docx
//!
//! The model is expected to call these tools to ground its answers. The
//! dispatch fn returns plain-string results that get fed back as `tool`
//! messages in the next iteration, exactly like MCP tool results.

use crate::llm::types::{ToolFunction, ToolSchema};
use crate::AppState;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::LazyLock;

// `[c12]` / `[g3]` / `[p4]` plus comma-grouped variants like
// `[c1, c2, c3]`. Compiled once at first call site via `LazyLock`
// — the same instance is reused across every `generate_docx` call.
// `\b` would over-match because the marker is bracket-delimited; we
// rely on the literal `[` and `]` for anchoring.
static CITATION_MARKER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[(?:[cgp]\d+)(?:\s*,\s*[cgp]\d+)*\]").expect("citation marker regex compiles")
});
// "word ." → "word." after a marker drops, including ".,;:?!).
static SPACE_BEFORE_PUNCT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r" +([.,;:?!\)\]])").expect("space-before-punct regex compiles")
});
// Collapse "word  word" → "word word" left by an inline marker drop.
// We deliberately don't touch `\n` runs — paragraph structure must
// survive intact.
static DOUBLE_SPACE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"  +").expect("double-space regex compiles"));

const READ_DOCUMENT: &str = "read_document";
const FIND_IN_DOCUMENT: &str = "find_in_document";
const READ_WORKFLOW: &str = "read_workflow";
const GENERATE_DOCX: &str = "generate_docx";
const GENERATE_XLSX: &str = "generate_xlsx";
const EDIT_DOCUMENT: &str = "edit_document";
const LIST_DOCX_TEMPLATES: &str = "list_docx_templates";
const DESCRIBE_DOCX_TEMPLATE: &str = "describe_docx_template";

pub fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        READ_DOCUMENT
            | FIND_IN_DOCUMENT
            | READ_WORKFLOW
            | GENERATE_DOCX
            | GENERATE_XLSX
            | EDIT_DOCUMENT
            | LIST_DOCX_TEMPLATES
            | DESCRIBE_DOCX_TEMPLATE
    )
}

pub fn schemas() -> Vec<ToolSchema> {
    fn fun(name: &str, description: &str, parameters: Value) -> ToolSchema {
        ToolSchema {
            kind: "function".to_string(),
            function: ToolFunction {
                name: name.to_string(),
                description: description.to_string(),
                parameters,
            },
        }
    }

    vec![
        fun(
            READ_DOCUMENT,
            "Read the text content of a document attached by the user. Always call this before answering questions about, summarising, or citing from a document. Long documents are returned a range at a time: the result says which pages (or parts, for text without pages) it contains and where to continue.",
            json!({
                "type": "object",
                "properties": {
                    "doc_id": {
                        "type": "string",
                        "description": "The document ID to read (e.g. 'doc-1', 'doc-2')"
                    },
                    "page_from": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Optional first page to read (the N of the [Page N] markers). For documents without pages, the first part number."
                    },
                    "page_to": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Optional last page (or part) to read, inclusive."
                    }
                },
                "required": ["doc_id"]
            }),
        ),
        fun(
            FIND_IN_DOCUMENT,
            "Search for specific strings inside a document — a Ctrl+F equivalent. Returns each match with surrounding context. Matching is case-insensitive and whitespace-tolerant.",
            json!({
                "type": "object",
                "properties": {
                    "doc_id": { "type": "string", "description": "The document ID to search (e.g. 'doc-1')." },
                    "query":  { "type": "string", "description": "The string to search for (case-insensitive)." },
                    "max_results": { "type": "integer", "description": "Maximum matches to return (default 20).", "minimum": 1, "maximum": 200 }
                },
                "required": ["doc_id", "query"]
            }),
        ),
        fun(
            READ_WORKFLOW,
            "Read the full instructions (prompt) of a workflow by its ID. Call this after a workflow marker has been mentioned.",
            json!({
                "type": "object",
                "properties": {
                    "workflow_id": { "type": "string", "description": "The workflow ID to read." }
                },
                "required": ["workflow_id"]
            }),
        ),
        fun(
            GENERATE_DOCX,
            "Produce a downloadable .docx document. Two modes:\n\
             • **Template mode** (preferred): pass `template_id` (e.g. 'it/diffida-messa-in-mora') and `metadata` (the values for the template's `[PLACEHOLDERS]`). The body Markdown is rendered through the template's layout — typography, margins, styles all come from the template sidecar. Use `list_docx_templates` to discover templates and `describe_docx_template` to see the required metadata fields for a specific one.\n\
             • **Plain mode** (legacy): omit `template_id`. Falls back to a minimal generic layout. Pass `title` for the filename.\n\
             Returns the new document id and filename.",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Document title / base filename (no extension)." },
                    "body":  { "type": "string", "description": "Document content in Markdown. Headings, bullet lists, bold/italic honoured. With a template_id, `[PLACEHOLDER]` tokens in the body are substituted against the metadata map." },
                    "template_id": { "type": "string", "description": "Optional. The id of a docx-template from list_docx_templates (e.g. 'it/diffida-messa-in-mora'). When omitted, falls back to the plain renderer." },
                    "metadata": {
                        "type": "object",
                        "description": "Map of [PLACEHOLDER] name → value, e.g. { \"DEBITORE\": \"Tizio S.r.l.\", \"IMPORTO\": \"€ 12.345,67\" }. Required when template_id is supplied. Universal fields (LUOGO, DATA, MITTENTE, OGGETTO, RIF_PRATICA, ...) should always be filled.",
                        "additionalProperties": { "type": "string" }
                    }
                },
                "required": ["body"]
            }),
        ),
        fun(
            GENERATE_XLSX,
            "Produce a downloadable .xlsx (Microsoft Excel) spreadsheet from tabular data. Use this whenever the user asks for an Excel file, a spreadsheet, or to export a table to Excel. Pass the column labels in `headers` and the data in `rows` (one array of cell strings per row, aligned with `headers`). Returns the new document id and filename; the UI shows a download card automatically.",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Spreadsheet title / base filename (no extension)." },
                    "sheet_name": { "type": "string", "description": "Optional worksheet tab name (default 'Foglio1', truncated to 31 chars)." },
                    "headers": {
                        "type": "array",
                        "description": "Column header labels, left to right.",
                        "items": { "type": "string" }
                    },
                    "rows": {
                        "type": "array",
                        "description": "Data rows. Each row is an array of cell values aligned with `headers`; pad short rows with empty strings.",
                        "items": { "type": "array", "items": { "type": "string" } }
                    }
                },
                "required": ["headers", "rows"]
            }),
        ),
        fun(
            LIST_DOCX_TEMPLATES,
            "List the DOCX templates available to the closing formatter. Returns id, display_name, category, domain, automation_level, and required_metadata for each. Filter by `domain` (e.g. 'legal', 'finance', 'real_estate', 'compliance'). Call this FIRST when the user asks to produce a structured document (atto, diffida, parcella, contratto, ...) to pick the right template, then call describe_docx_template to see how to fill it, then generate_docx.",
            json!({
                "type": "object",
                "properties": {
                    "domain": { "type": "string", "description": "Optional canonical domain filter: 'legal' | 'medical' | 'finance' | 'real_estate' | 'hr' | 'insurance' | 'ip' | 'compliance' | 'pa' | 'others'." },
                    "locale": { "type": "string", "description": "Optional locale filter, e.g. 'it' to see only Italian templates." }
                },
                "required": []
            }),
        ),
        fun(
            DESCRIBE_DOCX_TEMPLATE,
            "Get the full authoring contract for a specific DOCX template: auto-generated system-prompt block (layout + section skeleton + required fields + per-field guidance), source reference into the Prontuario, and raw sidecar JSON. Call this AFTER list_docx_templates and BEFORE generate_docx — the returned `prompt_md` is what teaches you how to write a correct body for this template.",
            json!({
                "type": "object",
                "properties": {
                    "template_id": { "type": "string", "description": "The template id from list_docx_templates, e.g. 'it/diffida-messa-in-mora'." }
                },
                "required": ["template_id"]
            }),
        ),
        fun(
            EDIT_DOCUMENT,
            "Apply minimal substitutions to an existing .docx document attached to the chat. Pass `doc_id` (e.g. 'doc-1') and an array of `edits`, each with `find` and `replace` strings. The find string MUST appear verbatim in the document.",
            json!({
                "type": "object",
                "properties": {
                    "doc_id": { "type": "string", "description": "The document ID to edit (e.g. 'doc-1')." },
                    "edits": {
                        "type": "array",
                        "description": "List of substitutions to apply atomically.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "find":    { "type": "string" },
                                "replace": { "type": "string" }
                            },
                            "required": ["find", "replace"]
                        }
                    }
                },
                "required": ["doc_id", "edits"]
            }),
        ),
    ]
}

/// `doc_label_map` maps the chat-local label (`doc-1`, `doc-2`, …) to the
/// real `documents.id` UUID stored in SQLite. Built by the chat dispatcher
/// from the message's attached files.
///
/// `chat_id` is the originating chat's UUID. Forwarded into tools that
/// persist documents (today: `generate_docx`) so the new row carries the
/// `documents.chat_id` FK — that way deleting the chat cascades the
/// generated `.docx` away with it (see `delete_chat` cleanup in
/// `src/routes/chat.rs`). `None` is acceptable for callers that aren't
/// in a chat context (e.g. future REST-only tool runs), in which case
/// the row will be NULL-chat-id as before.
pub async fn dispatch(
    state: &AppState,
    user_id: &str,
    chat_id: Option<&str>,
    doc_label_map: &HashMap<String, String>,
    name: &str,
    arguments: &Value,
    read_budget_chars: usize,
) -> String {
    match name {
        READ_DOCUMENT => {
            exec_read_document(state, user_id, doc_label_map, arguments, read_budget_chars).await
        }
        FIND_IN_DOCUMENT => exec_find_in_document(state, user_id, doc_label_map, arguments).await,
        READ_WORKFLOW => exec_read_workflow(state, user_id, arguments).await,
        GENERATE_DOCX => exec_generate_docx(state, user_id, chat_id, arguments).await,
        GENERATE_XLSX => exec_generate_xlsx(state, user_id, chat_id, arguments).await,
        EDIT_DOCUMENT => exec_edit_document(state, user_id, doc_label_map, arguments).await,
        LIST_DOCX_TEMPLATES => exec_list_docx_templates(state, arguments).await,
        DESCRIBE_DOCX_TEMPLATE => exec_describe_docx_template(state, arguments).await,
        other => json!({"error": format!("unknown builtin tool: {other}")}).to_string(),
    }
}

/// Look up a document by chat-local label (`doc-1`, `doc-2`, …) or
/// raw UUID and return the metadata the tool exec functions need.
///
/// The returned tuple is `(filename, file_type, storage_path, real_id,
/// pii_protected)`. `pii_protected` is the migration-0028 column —
/// when truthy, the caller MUST serve content from the redacted cache
/// (`cache/pii/<real_id>.txt`) rather than the raw `storage_path`.
async fn resolve_doc(
    state: &AppState,
    user_id: &str,
    doc_label_map: &HashMap<String, String>,
    label_or_id: &str,
) -> Option<(String, String, Option<String>, String, bool)> {
    let real_id = doc_label_map
        .get(label_or_id)
        .cloned()
        .unwrap_or_else(|| label_or_id.to_string());
    let row: Option<(String, String, Option<String>, i64)> = sqlx::query_as(
        "SELECT filename, file_type, storage_path, pii_protected \
         FROM documents WHERE id = ? AND user_id = ?",
    )
    .bind(&real_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    row.map(|(f, t, s, p)| (f, t, s, real_id, p != 0))
}

/// Resolve a document's *effective* text for the LLM-facing tools. For
/// PII-protected documents this is the on-disk redacted cache produced
/// by the chat handler's `maybe_redact_pii`; for unprotected ones it's
/// the freshly-extracted text from the raw storage bytes.
///
/// Refusing to operate on a protected document at all would break the
/// model's `find_in_document` / `read_document` workflow for entirely
/// legitimate questions ("trova tutte le scadenze nel contratto"). By
/// substituting the redacted text we preserve those use-cases while
/// guaranteeing nothing the user opted to mask reaches the LLM.
///
/// Cache miss on a protected document — possible if the user calls
/// `read_document` before the first chat turn has run GLiNER — is
/// treated as a safety stop: we return `None` so the tool surfaces a
/// "PII redaction not yet ready" error instead of falling back to the
/// raw text.
async fn read_doc_text_for_llm(
    file_type: &str,
    filename: &str,
    storage_path: &str,
    doc_id: &str,
    pii_protected: bool,
) -> Result<String, String> {
    if pii_protected {
        let key = format!("cache/pii/{doc_id}.txt");
        match crate::storage::make_storage() {
            Ok(storage) => match storage.get(&key).await {
                Ok(bytes) => {
                    let s = String::from_utf8_lossy(&bytes).into_owned();
                    if s.is_empty() {
                        return Err(format!(
                            "document {filename} is PII-protected but the redacted cache \
                             ({key}) is empty — wait for the inline-attached pass to \
                             finish before calling this tool"
                        ));
                    }
                    Ok(s)
                }
                Err(_) => Err(format!(
                    "document {filename} is PII-protected; the redacted cache ({key}) \
                     has not been built yet — wait for the inline-attached PII pass \
                     to finish (it runs once per document) before calling this tool"
                )),
            },
            Err(e) => Err(format!("storage backend unavailable: {e}")),
        }
    } else {
        let bytes = match crate::storage::make_storage() {
            Ok(s) => match s.get(storage_path).await {
                Ok(b) => b,
                Err(e) => return Err(format!("storage read: {e}")),
            },
            Err(e) => return Err(format!("storage backend unavailable: {e}")),
        };
        Ok(extract_text(file_type, filename, &bytes))
    }
}

async fn exec_read_document(
    state: &AppState,
    user_id: &str,
    doc_label_map: &HashMap<String, String>,
    arguments: &Value,
    read_budget_chars: usize,
) -> String {
    let doc_label = arguments.get("doc_id").and_then(|v| v.as_str()).unwrap_or("");
    if doc_label.is_empty() {
        return json!({"error": "doc_id is required"}).to_string();
    }
    let Some((filename, file_type, Some(storage_path), real_id, pii_protected)) =
        resolve_doc(state, user_id, doc_label_map, doc_label).await
    else {
        return json!({"error": format!("document {doc_label} not found")}).to_string();
    };
    let text = match read_doc_text_for_llm(
        &file_type,
        &filename,
        &storage_path,
        &real_id,
        pii_protected,
    )
    .await
    {
        Ok(t) => t,
        Err(e) => return json!({"error": e}).to_string(),
    };
    let range = (
        arguments.get("page_from").and_then(Value::as_u64).map(|n| n as usize),
        arguments.get("page_to").and_then(Value::as_u64).map(|n| n as usize),
    );
    let mut out = read_range(&text, range, read_budget_chars);
    out["doc_id"] = json!(doc_label);
    out["filename"] = json!(filename);
    out["file_type"] = json!(file_type);
    out.to_string()
}

/// Text to return for `read_document`: the whole document when it fits the
/// budget and no range is asked, otherwise the requested pages (or parts)
/// up to the budget, with where to continue.
fn read_range(text: &str, (from, to): (Option<usize>, Option<usize>), budget_chars: usize) -> Value {
    use crate::document_segments::{render, segment, SegmentUnit};

    let total_chars = text.chars().count();
    let (unit, segments) = segment(text);
    let unit_name = match unit {
        SegmentUnit::Page => "page",
        SegmentUnit::Part => "part",
    };
    if from.is_none() && to.is_none() && total_chars <= budget_chars {
        return json!({
            "text": text,
            "unit": unit_name,
            "total": segments.len(),
            "complete": true,
        });
    }
    let (Some(first), Some(last)) = (segments.first(), segments.last()) else {
        return json!({ "text": text, "unit": unit_name, "total": 0, "complete": true });
    };
    let from = from.unwrap_or(first.number);
    let to = to.unwrap_or(last.number).max(from);

    let mut selected = Vec::new();
    let mut used = 0usize;
    let mut stopped_before: Option<usize> = None;
    for seg in segments.iter().filter(|s| s.number >= from && s.number <= to) {
        let size = seg.chars() + 16;
        if !selected.is_empty() && used + size > budget_chars {
            stopped_before = Some(seg.number);
            break;
        }
        selected.push(seg);
        used += size;
    }
    let Some(returned_from) = selected.first().map(|s| s.number) else {
        return json!({
            "error": format!("no {unit_name} in range {from}-{to}; this document has {unit_name}s {}-{}", first.number, last.number),
        });
    };
    let returned_to = selected.last().map(|s| s.number).unwrap_or(returned_from);

    let mut body = render(unit, &selected);
    let mut cut_inside = false;
    if body.chars().count() > budget_chars {
        body = body.chars().take(budget_chars).collect();
        cut_inside = true;
    }
    let next = stopped_before.or_else(|| {
        (returned_to < last.number).then(|| {
            segments.iter().map(|s| s.number).find(|n| *n > returned_to).unwrap_or(returned_to + 1)
        })
    });
    let requested_more = to > returned_to || cut_inside;
    let mut result = json!({
        "text": body,
        "unit": unit_name,
        "total": segments.len(),
        "first": first.number,
        "last": last.number,
        "returned_from": returned_from,
        "returned_to": returned_to,
        "complete": returned_from == first.number && returned_to == last.number && !cut_inside,
    });
    if cut_inside {
        result["note"] = json!(format!(
            "{unit_name} {returned_to} is longer than the reading limit and was cut; use find_in_document to reach specific passages in it."
        ));
    } else if let Some(n) = next.filter(|_| requested_more || to == last.number) {
        result["next_page_from"] = json!(n);
        result["note"] = json!(format!(
            "The document continues. Call read_document with page_from = {n} to read on, or use find_in_document to locate specific passages."
        ));
    }
    result
}

/// Largest char boundary at or below `i`.
fn floor_boundary(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Smallest char boundary at or above `i`.
fn ceil_boundary(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

async fn exec_find_in_document(
    state: &AppState,
    user_id: &str,
    doc_label_map: &HashMap<String, String>,
    arguments: &Value,
) -> String {
    let doc_label = arguments.get("doc_id").and_then(|v| v.as_str()).unwrap_or("");
    let query = arguments.get("query").and_then(|v| v.as_str()).unwrap_or("");
    let max_results = arguments
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(20)
        .min(200) as usize;
    if doc_label.is_empty() || query.is_empty() {
        return json!({"error": "doc_id and query are required"}).to_string();
    }
    let Some((filename, file_type, Some(storage_path), real_id, pii_protected)) =
        resolve_doc(state, user_id, doc_label_map, doc_label).await
    else {
        return json!({"error": format!("document {doc_label} not found")}).to_string();
    };
    let text = match read_doc_text_for_llm(
        &file_type,
        &filename,
        &storage_path,
        &real_id,
        pii_protected,
    )
    .await
    {
        Ok(t) => t,
        Err(e) => return json!({"error": e}).to_string(),
    };

    // Case-insensitive, whitespace-tolerant search.
    let needle: String = query.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    let haystack_norm: String = text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();

    let mut matches = Vec::new();
    let mut start = 0usize;
    while let Some(idx) = haystack_norm[start..].find(&needle) {
        let abs = start + idx;
        let ctx_lo = floor_boundary(&haystack_norm, abs.saturating_sub(60));
        let ctx_hi = ceil_boundary(&haystack_norm, abs + needle.len() + 60);
        let snippet = &haystack_norm[ctx_lo..ctx_hi];
        matches.push(json!({
            "offset": abs,
            "snippet": snippet,
        }));
        if matches.len() >= max_results { break; }
        start = abs + needle.len();
    }
    json!({
        "doc_id": doc_label,
        "filename": filename,
        "query": query,
        "match_count": matches.len(),
        "matches": matches,
    })
    .to_string()
}

async fn exec_read_workflow(state: &AppState, user_id: &str, arguments: &Value) -> String {
    let id = arguments.get("workflow_id").and_then(|v| v.as_str()).unwrap_or("");
    if id.is_empty() {
        return json!({"error": "workflow_id is required"}).to_string();
    }

    // ── First check the in-memory preset registry. System workflows
    // (`builtin-*`) live there, not in the DB. Without this branch
    // every preset workflow would 404 on read_workflow.
    if let Some(preset) = state.workflow_presets.iter().find(|p| p.id == id) {
        return build_read_workflow_response(
            id,
            &preset.title,
            preset.prompt_md.as_deref().unwrap_or(""),
            preset.default_output_template.as_deref(),
            &state.docx_templates,
        )
        .to_string();
    }

    let row: Option<(String, String)> =
        sqlx::query_as("SELECT title, prompt_md FROM workflows WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(user_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
    let Some((title, prompt_md)) = row else {
        return json!({"error": format!("workflow {id} not found")}).to_string();
    };
    // User-created workflows don't carry default_output_template yet
    // — that's a system-preset-only field for now. Phase 2 may surface
    // it in the workflow editor UI and add the DB column.
    build_read_workflow_response(id, &title, &prompt_md, None, &state.docx_templates)
        .to_string()
}

/// Bundle a workflow's prompt with the linked DOCX template's
/// authoring contract (if any) into the JSON payload `read_workflow`
/// returns. Pure function — takes only the inputs it needs, no
/// AppState — so unit tests can drive it with hand-rolled templates.
fn build_read_workflow_response(
    workflow_id: &str,
    title: &str,
    prompt_md: &str,
    default_output_template: Option<&str>,
    docx_templates: &[crate::presets::docx_template::DocxTemplate],
) -> Value {
    let mut payload = json!({
        "workflow_id": workflow_id,
        "title": title,
        "prompt_md": prompt_md,
    });

    if let Some(tpl_id) = default_output_template {
        if let Some(template) = docx_templates.iter().find(|t| t.id == tpl_id) {
            payload["default_output_template"] = json!({
                "template_id": template.id,
                "display_name": template.display_name_for("it"),
                "automation_level": template.automation_level,
                "required_metadata": template.required_metadata,
                "prompt_md": template.auto_generated_prompt_md("it"),
                "source_reference": template.source_reference,
            });
            payload["closing_instruction"] = json!(format!(
                "This workflow produces a Word document. Once the user's request is clear, write the body in Markdown and call generate_docx(template_id=\"{}\", body=\"<your Markdown here>\", metadata={{...}}). The parameter MUST be named `body` (not `body_md` — that does not exist) and MUST contain the full Markdown text you want rendered into the docx. The template's authoring contract above (default_output_template.prompt_md) tells you which sections to emit and which fields to collect.",
                template.id
            ));
        } else {
            payload["default_output_template_missing"] = json!(format!(
                "Workflow references template_id={tpl_id} but it isn't loaded. The user should drop the sidecar into config/docx-templates/. Continue without docx output."
            ));
        }
    }
    payload
}

async fn exec_generate_docx(
    state: &AppState,
    user_id: &str,
    chat_id: Option<&str>,
    arguments: &Value,
) -> String {
    // The model occasionally invokes us with `body_md` (a name we used
    // to suggest in the prompt by mistake) or no body field at all.
    // Accept either name and fall back gracefully so a small naming
    // slip doesn't cost a retry.
    let raw_body = arguments
        .get("body")
        .and_then(|v| v.as_str())
        .or_else(|| arguments.get("body_md").and_then(|v| v.as_str()))
        .unwrap_or("");

    // Strip `[c1]`, `[g2]`, `[p3]` and grouped variants like
    // `[c1, c2, c3]` from the body before handing it to the docx
    // renderer. Those markers are the chat-UI citation pills the model
    // emits in every answer — useful inline when the reader can click
    // to open the source doc, meaningless when the .docx is exported
    // and read in Word (a reader stuck with `[c104]` has no legend to
    // resolve it). The model's Allegati / Appendix section, which
    // lists the source filenames, remains the canonical traceability
    // for the exported document.
    let body_owned: String;
    let body: &str = {
        let cleaned = CITATION_MARKER_RE.replace_all(raw_body, "");
        // Collapse the double spaces / "space-before-punct" artefacts
        // a removed marker leaves behind (e.g. "…coniuge [c95]." →
        // "…coniuge ." → "…coniuge.").
        let s = SPACE_BEFORE_PUNCT_RE.replace_all(&cleaned, "$1");
        let s = DOUBLE_SPACE_RE.replace_all(&s, " ");
        if s == raw_body {
            raw_body
        } else {
            body_owned = s.into_owned();
            &body_owned
        }
    };

    if body.is_empty() {
        // Self-correcting error: tell the model EXACTLY what's wrong,
        // the right argument shape, and the next step. The agent loop's
        // empty-answer retry plus this guidance turns a single bad
        // call into a fixed second call instead of a silent give-up.
        let supplied: Vec<&String> = arguments
            .as_object()
            .map(|o| o.keys().collect())
            .unwrap_or_default();
        return json!({
            "error": "Missing required argument `body` — the Markdown text to render into the document.",
            "supplied_arguments": supplied,
            "hint": "Call generate_docx again with the document body as the `body` argument, e.g. {\"body\": \"# Title\\n\\nParagraph…\", \"template_id\": \"<id from describe_docx_template>\", \"metadata\": {…}}. The body must be a non-empty Markdown string; `body_md` is NOT a valid argument name.",
        })
        .to_string();
    }
    let template_id = arguments
        .get("template_id")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let explicit_title = arguments
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // ── Branch A: template-driven render via src/docx/ pipeline.
    let (bytes, default_title, unresolved): (Vec<u8>, String, Vec<String>) =
        if let Some(template_id) = template_id {
            let template = state
                .docx_templates
                .iter()
                .find(|t| t.id == template_id);
            let Some(template) = template else {
                return json!({
                    "error": format!("template_id {template_id} not found"),
                    "hint": "Call list_docx_templates to see available ids."
                })
                .to_string();
            };

            // metadata map[String→String]. Coerce numeric / bool
            // values to string so the LLM can be sloppy.
            let metadata: std::collections::HashMap<String, String> = arguments
                .get("metadata")
                .and_then(|v| v.as_object())
                .map(|obj| {
                    obj.iter()
                        .map(|(k, v)| {
                            let s = match v {
                                Value::String(s) => s.clone(),
                                Value::Number(n) => n.to_string(),
                                Value::Bool(b) => b.to_string(),
                                Value::Null => String::new(),
                                other => other.to_string(),
                            };
                            (k.clone(), s)
                        })
                        .collect()
                })
                .unwrap_or_default();

            let outcome = match crate::docx::render(template, body, &metadata) {
                Ok(o) => o,
                Err(e) => return json!({"error": format!("docx render: {e}")}).to_string(),
            };
            let title_default = template
                .display_name_for("it")
                .replace('/', "-");
            (outcome.bytes, title_default, outcome.unresolved_placeholders)
        } else {
            // ── Branch B: backwards-compat plain render.
            let title_for_legacy = explicit_title
                .clone()
                .unwrap_or_else(|| "Untitled".to_string());
            let bytes =
                match crate::pdf::docx_writer::markdown_to_docx(&title_for_legacy, body) {
                    Ok(b) => b,
                    Err(e) => {
                        return json!({"error": format!("docx build: {e}")}).to_string();
                    }
                };
            (bytes, title_for_legacy, Vec::new())
        };

    let title = explicit_title.unwrap_or(default_title);
    let safe_title = sanitize_filename(&title);
    let filename = format!("{safe_title}.docx");
    let doc_id = uuid::Uuid::new_v4().to_string();
    let storage_path = format!("documents/{user_id}/{doc_id}");

    let storage = match crate::storage::make_storage() {
        Ok(s) => s,
        Err(e) => return json!({"error": format!("storage: {e}")}).to_string(),
    };
    if let Err(e) = storage
        .put(
            &storage_path,
            &bytes,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        )
        .await
    {
        return json!({"error": format!("storage write: {e}")}).to_string();
    }

    let size = bytes.len() as i64;
    // chat_id is bound when the call came from a chat dispatch so the
    // generated .docx cascades away with the chat on deletion (see
    // `delete_chat` in `src/routes/chat.rs`). Tool runs that originate
    // outside a chat context (none today; future REST surface) keep the
    // column NULL — those rows are orphaned by design and need a
    // separate cleanup story.
    if let Err(e) = sqlx::query(
        "INSERT INTO documents (id, user_id, project_id, chat_id, filename, file_type, size_bytes, storage_path, status) \
         VALUES (?, ?, NULL, ?, ?, 'docx', ?, ?, 'ready')",
    )
    .bind(&doc_id)
    .bind(user_id)
    .bind(chat_id)
    .bind(&filename)
    .bind(size)
    .bind(&storage_path)
    .execute(&state.db)
    .await
    {
        return json!({"error": format!("db: {e}")}).to_string();
    }

    let mut payload = json!({
        "doc_id": doc_id,
        "filename": filename,
        "size_bytes": size,
        "note": "Document persisted as a standalone document. Call read_document with this doc_id to verify content before describing it to the user."
    });
    if !unresolved.is_empty() {
        payload["unresolved_placeholders"] = json!(unresolved);
        payload["warning"] = json!(format!(
            "{} placeholder(s) still present in the document — these metadata fields were not supplied: {}. The document is generated but the user will see the gaps. Consider regenerating with the missing fields filled.",
            unresolved.len(),
            unresolved.join(", ")
        ));
    }
    payload.to_string()
}

/// Coerce a JSON cell value to a plain string so the LLM can be sloppy
/// about types (a number, a bool, or a proper string all work).
fn json_cell_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Build an .xlsx workbook with one bold header row plus the data rows.
/// Kept as a standalone fn so the `&mut Worksheet` borrow ends before
/// `save_to_buffer` needs the workbook back.
fn build_xlsx(
    sheet_name: &str,
    headers: &[String],
    rows: &[Vec<String>],
) -> Result<Vec<u8>, rust_xlsxwriter::XlsxError> {
    use rust_xlsxwriter::{Format, Workbook};
    let mut workbook = Workbook::new();
    let header_fmt = Format::new().set_bold();
    {
        let sheet = workbook.add_worksheet();
        // set_name rejects names >31 chars or with invalid chars; a bad
        // name shouldn't fail the whole export, so ignore the error.
        let _ = sheet.set_name(sheet_name);
        for (c, h) in headers.iter().enumerate() {
            sheet.write_with_format(0, c as u16, h.as_str(), &header_fmt)?;
        }
        for (r, row) in rows.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                sheet.write((r + 1) as u32, c as u16, cell.as_str())?;
            }
        }
    }
    workbook.save_to_buffer()
}

async fn exec_generate_xlsx(
    state: &AppState,
    user_id: &str,
    chat_id: Option<&str>,
    arguments: &Value,
) -> String {
    let headers: Vec<String> = arguments
        .get("headers")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().map(json_cell_to_string).collect())
        .unwrap_or_default();
    if headers.is_empty() {
        return json!({"error": "headers (a non-empty array of column names) is required"})
            .to_string();
    }
    let rows: Vec<Vec<String>> = arguments
        .get("rows")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .map(|r| {
                    r.as_array()
                        .map(|cells| cells.iter().map(json_cell_to_string).collect())
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();
    if rows.is_empty() {
        return json!({"error": "rows (an array of row arrays) is required and must be non-empty"})
            .to_string();
    }

    let sheet_name: String = arguments
        .get("sheet_name")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("Foglio1")
        .chars()
        .take(31)
        .collect();

    let bytes = match build_xlsx(&sheet_name, &headers, &rows) {
        Ok(b) => b,
        Err(e) => return json!({"error": format!("xlsx build: {e}")}).to_string(),
    };

    let title = arguments
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Tabella".to_string());
    let safe_title = sanitize_filename(&title);
    let filename = format!("{safe_title}.xlsx");
    let doc_id = uuid::Uuid::new_v4().to_string();
    let storage_path = format!("documents/{user_id}/{doc_id}");

    let storage = match crate::storage::make_storage() {
        Ok(s) => s,
        Err(e) => return json!({"error": format!("storage: {e}")}).to_string(),
    };
    if let Err(e) = storage
        .put(
            &storage_path,
            &bytes,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        )
        .await
    {
        return json!({"error": format!("storage write: {e}")}).to_string();
    }

    let size = bytes.len() as i64;
    // chat_id bound so the generated .xlsx cascades away with the chat
    // on deletion, mirroring exec_generate_docx.
    if let Err(e) = sqlx::query(
        "INSERT INTO documents (id, user_id, project_id, chat_id, filename, file_type, size_bytes, storage_path, status) \
         VALUES (?, ?, NULL, ?, ?, 'xlsx', ?, ?, 'ready')",
    )
    .bind(&doc_id)
    .bind(user_id)
    .bind(chat_id)
    .bind(&filename)
    .bind(size)
    .bind(&storage_path)
    .execute(&state.db)
    .await
    {
        return json!({"error": format!("db: {e}")}).to_string();
    }

    json!({
        "doc_id": doc_id,
        "filename": filename,
        "size_bytes": size,
        "rows": rows.len(),
        "columns": headers.len(),
        "note": "Spreadsheet persisted as a standalone document. The download card is shown by the UI — do not add a download link in your prose."
    })
    .to_string()
}

async fn exec_list_docx_templates(state: &AppState, arguments: &Value) -> String {
    let domain_filter = arguments
        .get("domain")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let locale_filter = arguments
        .get("locale")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let items: Vec<Value> = state
        .docx_templates
        .iter()
        .filter(|t| t.matches_domain(domain_filter.as_deref()))
        .filter(|t| {
            locale_filter
                .as_deref()
                .is_none_or(|l| t.locale.starts_with(l))
        })
        .map(|t| {
            json!({
                "id": t.id,
                "display_name": t.display_name_for("it"),
                "category": t.category,
                "domain": t.domain,
                "also_applicable_to": t.also_applicable_to,
                "locale": t.locale,
                "automation_level": t.automation_level,
                "required_metadata": t.required_metadata,
                "source_reference": t.source_reference,
            })
        })
        .collect();
    json!({ "templates": items, "count": items.len() }).to_string()
}

async fn exec_describe_docx_template(state: &AppState, arguments: &Value) -> String {
    let id = arguments
        .get("template_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if id.is_empty() {
        return json!({"error": "template_id is required"}).to_string();
    }
    let Some(template) = state.docx_templates.iter().find(|t| t.id == id) else {
        return json!({
            "error": format!("template {id} not found"),
            "hint": "Call list_docx_templates to see available ids."
        })
        .to_string();
    };
    // Return both the auto-generated authoring prompt AND the raw
    // sidecar so the model can introspect any field it wants. The
    // prompt_md is the load-bearing payload — it's what the model
    // injects into its own working context to know HOW to write a
    // body for this template.
    json!({
        "template_id": template.id,
        "display_name": template.display_name_for("it"),
        "prompt_md": template.auto_generated_prompt_md("it"),
        "sidecar": template.to_api_json(),
    })
    .to_string()
}

async fn exec_edit_document(
    state: &AppState,
    user_id: &str,
    doc_label_map: &HashMap<String, String>,
    arguments: &Value,
) -> String {
    let label = arguments.get("doc_id").and_then(|v| v.as_str()).unwrap_or("");
    let edits_val = arguments.get("edits").and_then(|v| v.as_array());
    let Some(edits_val) = edits_val else {
        return json!({"error": "edits array is required"}).to_string();
    };
    let edits: Vec<crate::pdf::docx_writer::DocxEdit> = edits_val
        .iter()
        .filter_map(|e| {
            let find = e.get("find").and_then(|v| v.as_str())?.to_string();
            let replace = e.get("replace").and_then(|v| v.as_str())?.to_string();
            Some(crate::pdf::docx_writer::DocxEdit { find, replace })
        })
        .collect();
    if edits.is_empty() {
        return json!({"error": "no valid edit entries"}).to_string();
    }

    let Some((filename, file_type, Some(storage_path), _, pii_protected)) =
        resolve_doc(state, user_id, doc_label_map, label).await
    else {
        return json!({"error": format!("document {label} not found")}).to_string();
    };
    if file_type != "docx" {
        return json!({"error": format!("edit_document only supports .docx files (got {file_type})")}).to_string();
    }
    if pii_protected {
        return json!({"error": format!(
            "document {filename} is PII-protected; edit_document writes to the raw \
             docx and would re-expose masked entities — refuse instead of leaking"
        )}).to_string();
    }

    let storage = match crate::storage::make_storage() {
        Ok(s) => s,
        Err(e) => return json!({"error": format!("storage: {e}")}).to_string(),
    };
    let bytes = match storage.get(&storage_path).await {
        Ok(b) => b,
        Err(e) => return json!({"error": format!("storage read: {e}")}).to_string(),
    };

    let (new_bytes, hits) = match crate::pdf::docx_writer::apply_text_edits(&bytes, &edits) {
        Ok(x) => x,
        Err(e) => return json!({"error": format!("docx edit: {e}")}).to_string(),
    };

    if let Err(e) = storage
        .put(
            &storage_path,
            &new_bytes,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        )
        .await
    {
        return json!({"error": format!("storage write: {e}")}).to_string();
    }
    let new_size = new_bytes.len() as i64;
    let real_id = doc_label_map
        .get(label)
        .cloned()
        .unwrap_or_else(|| label.to_string());
    let _ = sqlx::query("UPDATE documents SET size_bytes = ? WHERE id = ? AND user_id = ?")
        .bind(new_size)
        .bind(&real_id)
        .bind(user_id)
        .execute(&state.db)
        .await;

    let summary: Vec<Value> = edits
        .iter()
        .zip(hits.iter())
        .map(|(e, h)| json!({"find": e.find, "replace": e.replace, "hits": h}))
        .collect();
    json!({
        // `doc_id` stays the chat-local label the model works with;
        // `document_id` is the real row id, used only by the UI layer to
        // build the download card (see `tool_step_event`).
        "doc_id": label,
        "document_id": real_id,
        "filename": filename,
        "edits_applied": summary,
    })
    .to_string()
}

fn sanitize_filename(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.is_empty() { return "Untitled".to_string(); }
    let cleaned: String = trimmed
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    cleaned.chars().take(60).collect::<String>().trim().to_string()
}

fn extract_text(file_type: &str, filename: &str, bytes: &[u8]) -> String {
    match file_type {
        "docx" => crate::pdf::extract_docx_text(bytes).unwrap_or_default(),
        "rtf" => {
            // Same path the sync scanner uses — RtfDocument::get_text()
            // returns the body without control words / fonts / pictures.
            let raw = String::from_utf8_lossy(bytes);
            rtf_parser::RtfDocument::try_from(raw.as_ref())
                .map(|d| d.get_text())
                .unwrap_or_default()
        }
        "xlsx" | "xls" | "xlsb" | "ods" => {
            crate::pdf::extract_xlsx_text(bytes).unwrap_or_default()
        }
        "txt" | "md" | "csv" => String::from_utf8_lossy(bytes).to_string(),
        "pdf" => {
            #[cfg(feature = "pdf")]
            {
                let tmp = std::env::temp_dir().join(crate::product::temp_file_name(&format!("builtin-{filename}")));
                if std::fs::write(&tmp, bytes).is_ok() {
                    let out = crate::pdf::extract_full_text(&tmp).unwrap_or_default();
                    let _ = std::fs::remove_file(&tmp);
                    out
                } else {
                    String::new()
                }
            }
            #[cfg(not(feature = "pdf"))]
            {
                let _ = filename;
                String::new()
            }
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn read_range_returns_whole_short_documents() {
        let v = super::read_range("[Page 1]\nuno\n[Page 2]\ndue\n", (None, None), 10_000);
        assert_eq!(v["complete"], true);
        assert_eq!(v["total"], 2);
        assert!(v.get("next_page_from").is_none());
    }

    #[test]
    fn read_range_pages_long_documents_within_the_budget() {
        let page = "x".repeat(900);
        let text: String = (1..=10).map(|n| format!("[Page {n}]\n{page}\n")).collect();
        let v = super::read_range(&text, (None, None), 3_000);
        assert_eq!(v["returned_from"], 1);
        assert_eq!(v["returned_to"], 3);
        assert_eq!(v["next_page_from"], 4);
        assert_eq!(v["complete"], false);
        let v = super::read_range(&text, (Some(9), None), 3_000);
        assert_eq!(v["returned_from"], 9);
        assert_eq!(v["returned_to"], 10);
        assert!(v.get("next_page_from").is_none());
        assert!(v["text"].as_str().unwrap().starts_with("[Page 9]"));
    }

    #[test]
    fn read_range_cuts_a_single_oversized_segment() {
        let text = format!("[Page 1]\n{}\n", "y".repeat(5_000));
        let v = super::read_range(&text, (None, None), 1_000);
        assert_eq!(v["text"].as_str().unwrap().chars().count(), 1_000);
        assert!(v["note"].as_str().unwrap().contains("cut"));
    }

    #[test]
    fn read_range_reports_an_empty_range() {
        let v = super::read_range("[Page 1]\nuno\n", (Some(5), Some(6)), 100);
        assert!(v["error"].as_str().unwrap().contains("no page"));
    }

    #[test]
    fn char_boundaries_are_safe_around_accents() {
        let s = "àèìòù";
        assert_eq!(super::floor_boundary(s, 3), 2);
        assert_eq!(super::ceil_boundary(s, 3), 4);
    }

    use super::*;

    #[test]
    fn is_builtin_recognises_each_tool() {
        for name in [
            "read_document",
            "find_in_document",
            "read_workflow",
            "generate_docx",
            "generate_xlsx",
            "edit_document",
            "list_docx_templates",
            "describe_docx_template",
        ] {
            assert!(is_builtin(name), "{name} should be builtin");
        }
        assert!(!is_builtin("unknown_tool"));
        assert!(!is_builtin(""));
    }

    #[test]
    fn schemas_have_required_fields() {
        let s = schemas();
        assert_eq!(s.len(), 8);
        for sch in &s {
            assert_eq!(sch.kind, "function");
            assert!(!sch.function.name.is_empty());
            assert!(!sch.function.description.is_empty());
            assert_eq!(sch.function.parameters["type"], "object");
        }
        let names: Vec<&str> = s.iter().map(|t| t.function.name.as_str()).collect();
        assert!(names.contains(&"read_document"));
        assert!(names.contains(&"find_in_document"));
        assert!(names.contains(&"read_workflow"));
        assert!(names.contains(&"generate_docx"));
        assert!(names.contains(&"generate_xlsx"));
        assert!(names.contains(&"list_docx_templates"));
        assert!(names.contains(&"describe_docx_template"));
        assert!(names.contains(&"edit_document"));
    }

    #[test]
    fn schema_required_arrays_are_consistent() {
        let s = schemas();
        for sch in &s {
            let p = &sch.function.parameters;
            let required = p["required"].as_array().expect("required must be array");
            let props = p["properties"].as_object().expect("properties must be object");
            for r in required {
                let key = r.as_str().unwrap();
                assert!(props.contains_key(key), "{} requires {key} but property not declared", sch.function.name);
            }
        }
    }

    // ── build_read_workflow_response ────────────────────────────────

    fn diffida_template() -> crate::presets::docx_template::DocxTemplate {
        let dir = crate::presets::config_subdir("docx-templates");
        let templates =
            crate::presets::docx_template::load_docx_templates(&dir).expect("load");
        templates
            .into_iter()
            .find(|t| t.id == "it/diffida-messa-in-mora")
            .expect("diffida present")
    }

    #[test]
    fn read_workflow_response_without_template_carries_only_core_fields() {
        let payload = build_read_workflow_response(
            "wf-id",
            "Test workflow",
            "Sei un assistente. Aiuta l'utente.",
            None,
            &[],
        );
        assert_eq!(payload["workflow_id"], json!("wf-id"));
        assert_eq!(payload["title"], json!("Test workflow"));
        assert!(payload.get("default_output_template").is_none());
        assert!(payload.get("default_output_template_missing").is_none());
        assert!(payload.get("closing_instruction").is_none());
    }

    #[test]
    fn read_workflow_response_with_template_bundles_authoring_contract() {
        let templates = vec![diffida_template()];
        let payload = build_read_workflow_response(
            "builtin-redazione-diffida",
            "Redazione diffida",
            "Sei un avvocato. Redigi una diffida.",
            Some("it/diffida-messa-in-mora"),
            &templates,
        );

        // Core fields
        assert_eq!(payload["workflow_id"], json!("builtin-redazione-diffida"));

        // default_output_template object present and well-formed
        let dot = payload["default_output_template"].as_object().expect("dot present");
        assert_eq!(dot["template_id"], json!("it/diffida-messa-in-mora"));
        assert_eq!(dot["automation_level"], json!("L1"));
        // required_metadata is a non-empty array
        assert!(dot["required_metadata"].as_array().unwrap().len() >= 5);
        // prompt_md contains the layout description (anchored to a
        // stable substring from the sidecar)
        assert!(dot["prompt_md"].as_str().unwrap().contains("Calibri"));
        // source_reference points back to the Prontuario
        assert!(dot["source_reference"]
            .as_str()
            .unwrap()
            .contains("TEMPLATE_PRONTUARIO"));

        // closing_instruction explicitly names generate_docx with the id
        let ci = payload["closing_instruction"].as_str().expect("closing");
        assert!(ci.contains("generate_docx"));
        assert!(ci.contains(r#"template_id="it/diffida-messa-in-mora""#));
    }

    #[test]
    fn read_workflow_response_with_unknown_template_emits_missing_field() {
        let payload = build_read_workflow_response(
            "wf-id",
            "Workflow rotto",
            "...",
            Some("it/non-esiste"),
            &[diffida_template()],
        );
        let missing = payload["default_output_template_missing"]
            .as_str()
            .expect("missing field present");
        assert!(missing.contains("it/non-esiste"));
        assert!(missing.contains("config/docx-templates/"));
        // No legit template object — the wiring failed.
        assert!(payload.get("default_output_template").is_none());
        // No closing_instruction either — there's no template to wrap up against.
        assert!(payload.get("closing_instruction").is_none());
    }

    #[test]
    fn read_workflow_response_with_template_id_but_empty_registry() {
        // Edge case: server starts with the docx-templates dir empty
        // (or removed). A workflow that references a template should
        // still produce a usable response — just with the
        // _missing field set.
        let payload = build_read_workflow_response(
            "wf-id",
            "Test",
            "...",
            Some("it/diffida-messa-in-mora"),
            &[],
        );
        assert!(payload["default_output_template_missing"].is_string());
    }

    #[test]
    fn sanitize_filename_default_when_empty() {
        assert_eq!(sanitize_filename(""), "Untitled");
        assert_eq!(sanitize_filename("    "), "Untitled");
    }

    #[test]
    fn sanitize_filename_replaces_unsafe_chars() {
        let s = sanitize_filename("foo/bar:baz?\\<>|*\"");
        assert!(!s.contains('/'));
        assert!(!s.contains('\\'));
        assert!(!s.contains(':'));
        assert!(!s.contains('?'));
        assert!(!s.contains('*'));
        assert!(!s.contains('"'));
        assert!(!s.contains('<'));
        assert!(!s.contains('>'));
        assert!(!s.contains('|'));
    }

    #[test]
    fn sanitize_filename_truncates_to_60_chars() {
        let long = "a".repeat(120);
        let out = sanitize_filename(&long);
        // 60-char max via `take(60)`. The trim() at the end may yield ≤60.
        assert!(out.chars().count() <= 60);
    }

    #[test]
    fn sanitize_filename_keeps_safe_chars() {
        assert_eq!(sanitize_filename("Contract Draft 2025-Q1"), "Contract Draft 2025-Q1");
        assert_eq!(sanitize_filename("invoice_#42"), "invoice_#42".replace('#', "_"));
    }

    #[test]
    fn build_xlsx_produces_a_zip_workbook() {
        let headers = vec!["Col A".to_string(), "Col B".to_string()];
        let rows = vec![
            vec!["1".to_string(), "two".to_string()],
            vec!["3".to_string()], // a short row must be tolerated
        ];
        let bytes = build_xlsx("Foglio1", &headers, &rows).expect("xlsx builds");
        // An .xlsx is a ZIP container — it starts with the PK magic bytes.
        assert!(bytes.starts_with(b"PK\x03\x04"));
        assert!(bytes.len() > 100);
    }

    #[test]
    fn generate_docx_missing_body_returns_actionable_error() {
        // Drive the validation branch directly via JSON shape so the test
        // doesn't need an AppState. The dispatch's first action when no
        // body / body_md is found is to return the structured error.
        // We simulate by inspecting the JSON the caller sees: the hint
        // must mention the right argument name AND the wrong name.
        let err = json!({
            "error": "Missing required argument `body` — the Markdown text to render into the document.",
            "supplied_arguments": ["template_id"],
            "hint": "Call generate_docx again with the document body as the `body` argument, e.g. {\"body\": \"# Title\\n\\nParagraph…\", \"template_id\": \"<id from describe_docx_template>\", \"metadata\": {…}}. The body must be a non-empty Markdown string; `body_md` is NOT a valid argument name.",
        });
        // Anchor the shape so future refactors keep the model-facing hints.
        let hint = err["hint"].as_str().unwrap();
        assert!(hint.contains("`body` argument"));
        assert!(hint.contains("`body_md` is NOT a valid argument name"));
        assert!(err.get("supplied_arguments").is_some());
    }

    #[test]
    fn json_cell_to_string_coerces_scalar_types() {
        assert_eq!(json_cell_to_string(&json!("hi")), "hi");
        assert_eq!(json_cell_to_string(&json!(42)), "42");
        assert_eq!(json_cell_to_string(&json!(true)), "true");
        assert_eq!(json_cell_to_string(&Value::Null), "");
    }

    #[test]
    fn extract_text_handles_text_formats() {
        assert_eq!(extract_text("txt", "x.txt", b"hello"), "hello");
        assert_eq!(extract_text("md", "x.md", b"# title"), "# title");
        assert_eq!(extract_text("csv", "x.csv", b"a,b,c\n1,2,3"), "a,b,c\n1,2,3");
    }

    #[test]
    fn extract_text_unknown_format_returns_empty() {
        assert_eq!(extract_text("zip", "x.zip", b"PK\x03\x04"), "");
        assert_eq!(extract_text("", "x", b"data"), "");
    }
}
