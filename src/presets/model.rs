//! LLM model catalogue — loaded once at startup from `config/model.json`.
//!
//! Same pattern as the workflow / column preset registries: read JSON
//! from disk, hold it in `AppState`, expose via `/models`. The frontend
//! Settings → Modelli LLM page consumes the catalogue to drive the
//! model + region dropdowns and to gate the "active provider" toggle
//! to providers that have an API key configured.
//!
//! Drop-in extensibility: to add a new provider, model, or region, edit
//! `config/model.json` and restart — no Rust changes required.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Auth metadata for a provider. Today every entry is `api_key`-based;
/// the kind field is forward-looking for OAuth / Vertex-style flows.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderAuth {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_var: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format_hint: Option<String>,
}

/// Region (data-residency) selectable for a provider when
/// `supports_regions == true`. Today only Google Gemini uses this.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderRegion {
    pub id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_default: Option<bool>,
}

/// Single model entry. Optional fields stay optional on the wire so we
/// don't drown the frontend in flags it doesn't use. The `preview` flag
/// is load-bearing: when `true`, the frontend forces the region selector
/// to `global` because preview models are global-only by spec.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderModel {
    pub id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_vision: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_tools: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_streaming: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_prompt_cache: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_extended_thinking: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_thinking: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_reasoning: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy: Option<bool>,
}

/// One provider in the catalogue. Matches the JSON shape one-for-one
/// so the wire format is just `serde_json::to_value(provider)`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Provider {
    pub id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    pub auth: ProviderAuth,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub supports_regions: bool,
    #[serde(default)]
    pub regions: Vec<ProviderRegion>,
    pub models: Vec<ProviderModel>,
}

/// Root document. `$schema_version` is reserved for future migrations;
/// today we accept anything and warn on unknowns.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelCatalogue {
    #[serde(rename = "$schema_version", default)]
    pub schema_version: u32,
    pub providers: Vec<Provider>,
}

/// Resolve the on-disk path for `config/model.json`. Lookup order
/// mirrors the preset directories:
///   1. `COVE_MODEL_CATALOGUE` env var (absolute path).
///   2. Walk ancestors from CWD looking for `config/model.json`.
///   3. Walk ancestors from the current executable's path.
///   4. Fallback to `./config/model.json`.
pub fn catalogue_path() -> PathBuf {
    if let Some(p) = crate::product::env_var("MODEL_CATALOGUE") {
        return PathBuf::from(p);
    }
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(found) = walk_ancestors_for(&cwd) {
            return found;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(found) = walk_ancestors_for(&exe) {
            return found;
        }
    }
    PathBuf::from("./config/model.json")
}

fn walk_ancestors_for(start: &Path) -> Option<PathBuf> {
    for anc in start.ancestors() {
        let candidate = anc.join("config").join("model.json");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Parse `config/model.json` into a typed catalogue. Failures fall back
/// to an empty catalogue with a warning — the rest of the app continues
/// to work; the models page just shows an empty dropdown.
pub fn load_catalogue(path: &Path) -> Result<ModelCatalogue> {
    let bytes = std::fs::read(path)?;
    let cat: ModelCatalogue = serde_json::from_slice(&bytes)?;
    Ok(cat)
}

impl ModelCatalogue {
    /// Empty catalogue — used as the fallback when the JSON is missing
    /// or unparseable.
    pub fn empty() -> Self {
        Self {
            schema_version: 0,
            providers: Vec::new(),
        }
    }
}
