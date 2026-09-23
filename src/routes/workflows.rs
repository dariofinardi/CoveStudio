use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use crate::{
    auth::middleware::AuthUser,
    llm::{self, Message, StreamParams},
    routes::chat::build_local_config,
    routes::user::fetch_llm_settings,
    AppState,
};

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn err(status: StatusCode, msg: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"detail": msg})))
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list_workflows).post(create_workflow))
        .route("/hidden", get(list_hidden).post(hide_workflow))
        .route("/hidden/{id}", axum::routing::delete(unhide_workflow))
        .route("/translate", post(translate_prompt))
        .route(
            "/{id}",
            get(get_workflow)
                .put(update_workflow)
                .patch(update_workflow)
                .delete(delete_workflow),
        )
}

/// Tuple shape returned by the SELECT statements below. Wrapping it in a
/// type alias keeps the long row signatures readable.
type WorkflowRow = (
    String,         // id
    String,         // user_id
    String,         // title
    Option<String>, // prompt_md (NULL if blank string)
    String,         // type
    Option<String>, // practice
    String,         // columns_config (JSON text, parsed on the way out)
    String,         // created_at
    String,         // updated_at
    String,         // domain (added in migration 0018, defaults to 'legal')
    String,         // also_applicable_to (JSON array text, migration 0034, default '[]')
);

const SELECT_COLS: &str =
    "id, user_id, title, NULLIF(prompt_md, '') AS prompt_md, type, practice, columns_config, created_at, updated_at, domain, also_applicable_to";

/// Parse the stored `also_applicable_to` JSON text into a clean
/// `Vec<String>`: keep only canonical domains, drop a redundant
/// listing of the primary domain. Tolerant of malformed JSON
/// (treats it as empty) so a bad row never breaks the list.
fn parse_also_applicable(raw: &str, primary: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(raw)
        .unwrap_or_default()
        .into_iter()
        .filter(|d| d != primary && crate::domain::is_valid(d))
        .collect()
}

fn row_to_json(row: WorkflowRow, current_user: &str) -> Value {
    let (id, user_id, title, prompt_md, ty, practice, columns_config, created_at, _updated_at, domain, also_raw) =
        row;
    let cols: Value = serde_json::from_str(&columns_config).unwrap_or_else(|_| json!([]));
    let also = parse_also_applicable(&also_raw, &domain);
    let is_owner = user_id == current_user;
    json!({
        "id": id,
        "user_id": user_id,
        "title": title,
        "type": ty,
        "prompt_md": prompt_md,
        "columns_config": cols,
        "practice": practice,
        "domain": domain,
        "also_applicable_to": also,
        "created_at": created_at,
        "is_system": false,
        "is_owner": is_owner,
    })
}

/// Sanitise a client-supplied additional-domains list into the JSON
/// text we persist: keep only canonical domains, drop any redundant
/// listing of the primary domain, de-duplicate. Returns "[]" for
/// None/empty. Mirrors the preset loader's `also_applicable_to`
/// hygiene so user rows and built-in presets behave identically.
fn sanitise_also_applicable(list: Option<&Vec<String>>, primary: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    let clean: Vec<&String> = list
        .map(|v| v.as_slice())
        .unwrap_or(&[])
        .iter()
        .filter(|d| d.as_str() != primary && crate::domain::is_valid(d) && seen.insert((*d).clone()))
        .collect();
    serde_json::to_string(&clean).unwrap_or_else(|_| "[]".to_string())
}

// ---------------------------------------------------------------------------
// GET /workflow?type=assistant|tabular
// ---------------------------------------------------------------------------
async fn list_workflows(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult {
    // Optional filters: `type` (assistant | tabular) — set by every
    // frontend caller — and `domain` (legal | medical | …) added in
    // migration 0018. Both are AND-combined when present; both fall
    // through silently when absent or empty.
    let type_filter: Option<String> = params
        .get("type")
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let domain_filter: Option<String> = params
        .get("domain")
        .filter(|s| !s.is_empty() && crate::domain::is_valid(s))
        .map(|s| s.to_string());

    // Build the WHERE clause incrementally so we keep parameter bindings
    // positional and avoid runtime SQL string concatenation tricks.
    // NOTE: the `domain` filter is applied in Rust (not SQL) because a
    // row now matches on its primary `domain` OR any entry in the
    // `also_applicable_to` JSON array (migration 0034) — cross-domain
    // user workflows, mirroring the preset behaviour. The `type`
    // filter stays in SQL (cheap, exact).
    let mut sql = format!(
        "SELECT {SELECT_COLS} FROM workflows WHERE user_id = ?"
    );
    if type_filter.is_some() {
        sql.push_str(" AND type = ?");
    }
    sql.push_str(" ORDER BY updated_at DESC");

    let mut q = sqlx::query_as::<_, WorkflowRow>(&sql).bind(&auth.user_id);
    if let Some(t) = &type_filter {
        q = q.bind(t);
    }
    let rows: Vec<WorkflowRow> = q
        .fetch_all(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let mut workflows: Vec<Value> = rows
        .into_iter()
        .filter(|r| match &domain_filter {
            None => true,
            // r.9 = primary domain, r.10 = also_applicable_to JSON text.
            Some(d) => &r.9 == d || parse_also_applicable(&r.10, &r.9).iter().any(|x| x == d),
        })
        .map(|r| row_to_json(r, &auth.user_id))
        .collect();

    // Merge in-memory workflow presets (system-shipped from
    // workflow-presets/<domain>/*.json). They appear with
    // `is_system: true` and a null `user_id` so the existing
    // edit/delete gating on the frontend kicks in. Apply the same
    // `type` and `domain` filters used for the DB query so the
    // user's selection slices both registries consistently.
    for preset in state.workflow_presets.iter() {
        if let Some(ref t) = type_filter {
            if &preset.kind != t {
                continue;
            }
        }
        if let Some(ref d) = domain_filter {
            // Cross-domain aware: a preset surfaces under its primary
            // `domain` AND any `also_applicable_to` entry.
            if !preset.matches_domain(Some(d)) {
                continue;
            }
        }
        workflows.push(preset.to_api_json());
    }

    Ok(Json(json!({ "workflows": workflows })))
}

// ---------------------------------------------------------------------------
// POST /workflow
// Body: { title, type?, prompt_md?, practice?, columns_config? }
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct CreateWorkflowBody {
    title: String,
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    prompt_md: Option<String>,
    #[serde(default)]
    practice: Option<String>,
    #[serde(default)]
    columns_config: Option<Value>,
    /// Professional vertical — see `crate::domain::DOMAINS`. Falls back
    /// to `legal` (matching the schema default) when omitted or unknown.
    #[serde(default)]
    domain: Option<String>,
    /// Additional domains the workflow also surfaces under (migration
    /// 0034). Sanitised server-side: only canonical domains kept, the
    /// primary domain dropped if redundantly listed.
    #[serde(default)]
    also_applicable_to: Option<Vec<String>>,
}

async fn create_workflow(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<CreateWorkflowBody>,
) -> ApiResult {
    if body.title.trim().is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "Workflow title cannot be empty"));
    }
    let id = uuid::Uuid::new_v4().to_string();

    // Default to "assistant" when the client omits the type — the modal
    // always sends one, but other call sites (built-in promotion, future
    // import flows) might not.
    let ty = body.r#type.unwrap_or_else(|| "assistant".to_string());
    if ty != "assistant" && ty != "tabular" {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "type must be 'assistant' or 'tabular'",
        ));
    }

    // Empty-string sentinel for prompt_md keeps the original NOT NULL
    // constraint happy without requiring a destructive table rebuild.
    // The SELECT side rewrites empty strings back to NULL via NULLIF()
    // so the frontend sees a real Option<String>.
    let prompt_md = body.prompt_md.unwrap_or_default();

    let cols_text = body
        .columns_config
        .map(|v| v.to_string())
        .unwrap_or_else(|| "[]".to_string());

    let dom = crate::domain::normalise_or_default(body.domain.as_deref());
    let also_text = sanitise_also_applicable(body.also_applicable_to.as_ref(), dom);

    sqlx::query(
        "INSERT INTO workflows (id, user_id, title, prompt_md, type, practice, columns_config, domain, also_applicable_to) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .bind(body.title.trim())
    .bind(&prompt_md)
    .bind(&ty)
    .bind(&body.practice)
    .bind(&cols_text)
    .bind(dom)
    .bind(&also_text)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let row: WorkflowRow = sqlx::query_as(&format!(
        "SELECT {SELECT_COLS} FROM workflows WHERE id = ?"
    ))
    .bind(&id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(row_to_json(row, &auth.user_id)))
}

// ---------------------------------------------------------------------------
// GET /workflow/:id
// ---------------------------------------------------------------------------
async fn get_workflow(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    // Workflow presets are not stored in the DB — short-circuit the
    // lookup so the workflow detail page can fetch a built-in by id.
    if let Some(preset) = state.workflow_presets.iter().find(|p| p.id == id) {
        return Ok(Json(preset.to_api_json()));
    }

    let row: Option<WorkflowRow> = sqlx::query_as(&format!(
        "SELECT {SELECT_COLS} FROM workflows WHERE id = ? AND user_id = ?"
    ))
    .bind(&id)
    .bind(&auth.user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let row = row.ok_or_else(|| err(StatusCode::NOT_FOUND, "Workflow not found"))?;
    Ok(Json(row_to_json(row, &auth.user_id)))
}

// ---------------------------------------------------------------------------
// PUT|PATCH /workflow/:id
// Body: { title?, prompt_md?, practice?, columns_config? }
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct UpdateWorkflowBody {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    prompt_md: Option<String>,
    #[serde(default)]
    practice: Option<String>,
    #[serde(default)]
    columns_config: Option<Value>,
    #[serde(default)]
    domain: Option<String>,
    /// Additional domains (migration 0034). Some(list) replaces the
    /// stored set (empty array clears it); None leaves it unchanged.
    #[serde(default)]
    also_applicable_to: Option<Vec<String>>,
}

async fn update_workflow(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<UpdateWorkflowBody>,
) -> ApiResult {
    let cols_text: Option<String> = body.columns_config.map(|v| v.to_string());

    // Reject unknown domains on update (strict) — silently coercing
    // would mask client bugs. None/omitted = leave unchanged.
    if let Some(ref d) = body.domain {
        if !crate::domain::is_valid(d) {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "domain must be one of the canonical values (legal, medical, finance, real_estate, hr, insurance, ip, compliance, others)",
            ));
        }
    }

    // Sanitise against the effective primary domain: the one being set
    // in this same update, else "" (a no-op for the redundant-primary
    // filter — the SELECT path re-strips any stale primary on read).
    // Some(list) replaces the stored set (empty array clears it);
    // None leaves the column untouched via COALESCE.
    let also_text: Option<String> = body
        .also_applicable_to
        .as_ref()
        .map(|v| sanitise_also_applicable(Some(v), body.domain.as_deref().unwrap_or("")));

    let result = sqlx::query(
        "UPDATE workflows SET \
           title              = COALESCE(?, title), \
           prompt_md          = COALESCE(?, prompt_md), \
           practice           = COALESCE(?, practice), \
           columns_config     = COALESCE(?, columns_config), \
           domain             = COALESCE(?, domain), \
           also_applicable_to = COALESCE(?, also_applicable_to), \
           updated_at = datetime('now') \
         WHERE id = ? AND user_id = ?",
    )
    .bind(&body.title)
    .bind(&body.prompt_md)
    .bind(&body.practice)
    .bind(&cols_text)
    .bind(&body.domain)
    .bind(&also_text)
    .bind(&id)
    .bind(&auth.user_id)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    if result.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "Workflow not found"));
    }

    let row: WorkflowRow = sqlx::query_as(&format!(
        "SELECT {SELECT_COLS} FROM workflows WHERE id = ?"
    ))
    .bind(&id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(row_to_json(row, &auth.user_id)))
}

// ---------------------------------------------------------------------------
// DELETE /workflow/:id
// ---------------------------------------------------------------------------
async fn delete_workflow(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    let result = sqlx::query("DELETE FROM workflows WHERE id = ? AND user_id = ?")
        .bind(&id)
        .bind(&auth.user_id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    if result.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "Workflow not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// GET /workflow/hidden
// ---------------------------------------------------------------------------
async fn list_hidden(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> ApiResult {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT workflow_id FROM workflow_hidden WHERE user_id = ?")
            .bind(&auth.user_id)
            .fetch_all(&state.db)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let ids: Vec<String> = rows.into_iter().map(|(id,)| id).collect();
    Ok(Json(json!(ids)))
}

// ---------------------------------------------------------------------------
// POST /workflow/hidden  — Body: { workflow_id }
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct HideWorkflowBody {
    workflow_id: String,
}

async fn hide_workflow(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<HideWorkflowBody>,
) -> ApiResult {
    sqlx::query(
        "INSERT OR IGNORE INTO workflow_hidden (user_id, workflow_id) VALUES (?, ?)",
    )
    .bind(&auth.user_id)
    .bind(&body.workflow_id)
    .execute(&state.db)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// DELETE /workflow/hidden/:id
// ---------------------------------------------------------------------------
async fn unhide_workflow(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult {
    sqlx::query("DELETE FROM workflow_hidden WHERE user_id = ? AND workflow_id = ?")
        .bind(&auth.user_id)
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// POST /workflow/translate  { text, target_locale }
// Translates a prompt into the user's language, preserving Markdown.
// Used by the workflow editor: built-ins are duplicated, then translated.
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct TranslateBody {
    text: String,
    target_locale: Option<String>,
}

async fn translate_prompt(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<TranslateBody>,
) -> ApiResult {
    let text = body.text.trim();
    if text.is_empty() {
        return Ok(Json(json!({ "text": "" })));
    }
    let language = match body.target_locale.as_deref().unwrap_or("en") {
        "it" => "Italian",
        "fr" => "French",
        "de" => "German",
        "es" => "Spanish",
        "pt" => "Portuguese",
        _ => "English",
    };

    let settings = fetch_llm_settings(&state.db, &auth.user_id)
        .await
        .unwrap_or_default();
    let model = settings
        .title_model
        .clone()
        .filter(|m| !m.trim().is_empty())
        .or_else(|| settings.main_model.clone().filter(|m| !m.trim().is_empty()))
        .ok_or_else(|| {
            err(
                StatusCode::BAD_REQUEST,
                "No model configured. Set one in Settings → Models.",
            )
        })?;

    let system = format!(
        "Translate the user's text into {language}. Preserve all Markdown \
         formatting (headings, lists, bold, code) exactly. Do not translate \
         code, placeholders or tokens wrapped in braces/brackets. Output ONLY \
         the translation, with no preamble or explanation."
    );
    let params = StreamParams {
        model: model.clone(),
        system_prompt: system,
        system_volatile: String::new(),
        messages: vec![Message::user(text.to_string())],
        tools: vec![],
        max_iterations: 1,
        enable_thinking: false,
        local_config: build_local_config(&model, Some(&settings)),
        claude_api_key: settings.claude_api_key.clone(),
        gemini_api_key: settings.gemini_api_key.clone(),
        gemini_region: settings.gemini_region.clone(),
        // Translation is one-shot — no Mistral cache benefit.
        chat_id: None,
        mistral_opts: crate::routes::chat::build_mistral_opts(&model, Some(&settings)),
        response_schema: None,
    };
    let translated = match llm::provider_for_model(&model) {
        llm::Provider::Claude => llm::claude::complete(params).await,
        llm::Provider::OpenAI => llm::local::complete(params).await,
        llm::Provider::Gemini => llm::gemini::complete(params).await,
        llm::Provider::Mistral => llm::mistral::complete(params).await,
    }
    .map_err(|e| err(StatusCode::BAD_GATEWAY, &e.to_string()))?;

    Ok(Json(json!({ "text": translated.trim() })))
}
