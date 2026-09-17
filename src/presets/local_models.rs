// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

//! Catalogue of local models for the secure local mode, loaded from
//! `config/local-models/ollama.json`.
//!
//! Each entry describes an Ollama derivation the app creates on top of a
//! base model (template, system text, stop sequences, parameters). No model
//! name lives in the code: the allowlist, the Settings list, the chat
//! picker and the migration of names used by earlier releases
//! (`legacy_ids`, plus `renamed_aliases` for other Ollama aliases the app
//! documents, such as the context profiles in `config/model.json`) all
//! read this file. The copy embedded at build time is
//! used when the file on disk is missing or invalid.

use serde::{Deserialize, Serialize};

const CATALOGUE_FILE: &str = "ollama.json";
const EMBEDDED_CATALOGUE: &str = include_str!("../../config/local-models/ollama.json");

/// Tag Ollama appends to models created or pulled without one.
const DEFAULT_TAG: &str = ":latest";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalModel {
    /// Name of the derivation in Ollama; stored in the user's settings.
    pub id: String,
    /// Names the same derivation had in earlier releases.
    #[serde(default)]
    pub legacy_ids: Vec<String>,
    /// Upstream model pulled before the derivation is created.
    pub base_model: String,
    pub display_name: String,
    pub approx_size_gb: f32,
    pub min_ram_gb: u32,
    pub modelfile: ModelfileSpec,
}

/// Fields of the Ollama derivation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelfileSpec {
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub system: Option<String>,
    #[serde(default)]
    pub stop: Vec<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub num_predict: Option<i32>,
}

/// An Ollama alias renamed in a later release, outside the model list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AliasRename {
    pub from: String,
    pub to: String,
}

/// A name from an earlier release and the name that replaces it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NameMigration {
    pub legacy: String,
    pub current: String,
}

#[derive(Debug, Deserialize)]
struct CatalogueFile {
    models: Vec<LocalModel>,
    #[serde(default)]
    renamed_aliases: Vec<AliasRename>,
}

impl LocalModel {
    /// True when `name` (as stored in settings or listed by Ollama, with
    /// or without `:latest`) is this model's current id.
    pub fn is_current_name(&self, name: &str) -> bool {
        same_model_name(name, &self.id)
    }

    /// The legacy id matching `name`, if any.
    pub fn legacy_id_for(&self, name: &str) -> Option<&str> {
        self.legacy_ids
            .iter()
            .map(String::as_str)
            .find(|legacy| same_model_name(name, legacy))
    }
}

/// Compares Ollama model names ignoring the implicit `:latest` tag.
pub fn same_model_name(a: &str, b: &str) -> bool {
    a.strip_suffix(DEFAULT_TAG).unwrap_or(a) == b.strip_suffix(DEFAULT_TAG).unwrap_or(b)
}

/// The catalogue models: the file on disk when valid, otherwise the
/// embedded copy. Read on each call so edits apply without restarting.
pub fn catalogue() -> Vec<LocalModel> {
    load().models
}

/// Every rename to apply to an existing Ollama setup: the legacy ids of
/// the catalogue models and the renamed aliases.
pub fn name_migrations() -> Vec<NameMigration> {
    migrations_of(&load())
}

fn migrations_of(file: &CatalogueFile) -> Vec<NameMigration> {
    let from_models = file.models.iter().flat_map(|m| {
        m.legacy_ids.iter().map(|legacy| NameMigration {
            legacy: legacy.clone(),
            current: m.id.clone(),
        })
    });
    let from_aliases = file.renamed_aliases.iter().map(|a| NameMigration {
        legacy: a.from.clone(),
        current: a.to.clone(),
    });
    from_models.chain(from_aliases).collect()
}

fn load() -> CatalogueFile {
    let path = crate::presets::config_subdir("local-models").join(CATALOGUE_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => match parse(&text) {
            Ok(file) => return file,
            Err(e) => tracing::warn!(
                "[local-models] {} ignored ({e}); using the built-in catalogue",
                path.display()
            ),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!("[local-models] failed to read {}: {e}", path.display()),
    }
    parse(EMBEDDED_CATALOGUE).expect("embedded local-models catalogue must be valid")
}

/// The entry whose current id matches `name`.
pub fn find(name: &str) -> Option<LocalModel> {
    catalogue().into_iter().find(|m| m.is_current_name(name))
}

/// The entry whose current or legacy id matches `name`.
pub fn find_including_legacy(name: &str) -> Option<LocalModel> {
    catalogue()
        .into_iter()
        .find(|m| m.is_current_name(name) || m.legacy_id_for(name).is_some())
}

fn parse(text: &str) -> Result<CatalogueFile, String> {
    let file: CatalogueFile = serde_json::from_str(text).map_err(|e| e.to_string())?;
    validate(&file)?;
    Ok(file)
}

/// Every name, current or legacy, must be unique across models and
/// aliases, and a model must not share its base model's name.
fn validate(file: &CatalogueFile) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for alias in &file.renamed_aliases {
        for name in [&alias.from, &alias.to] {
            let bare = name.strip_suffix(DEFAULT_TAG).unwrap_or(name).to_string();
            if name.trim().is_empty() || !seen.insert(bare) {
                return Err(format!("empty or duplicate alias name `{name}`"));
            }
        }
    }
    for m in &file.models {
        if m.id.trim().is_empty() || m.base_model.trim().is_empty() {
            return Err("every model needs id and base_model".into());
        }
        for name in std::iter::once(&m.id).chain(&m.legacy_ids) {
            let bare = name.strip_suffix(DEFAULT_TAG).unwrap_or(name).to_string();
            if !seen.insert(bare) {
                return Err(format!("duplicate model name `{name}`"));
            }
            if same_model_name(name, &m.base_model) {
                return Err(format!("`{name}` must differ from its base model"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn embedded() -> Vec<LocalModel> {
        parse(EMBEDDED_CATALOGUE).unwrap().models
    }

    #[test]
    fn migrations_cover_legacy_ids_and_aliases() {
        let file = parse(EMBEDDED_CATALOGUE).unwrap();
        let migrations = migrations_of(&file);
        let expected = file.models.iter().map(|m| m.legacy_ids.len()).sum::<usize>()
            + file.renamed_aliases.len();
        assert_eq!(migrations.len(), expected);
        assert!(migrations.iter().all(|m| m.legacy != m.current));
    }

    #[test]
    fn embedded_catalogue_is_valid_and_not_empty() {
        assert!(!embedded().is_empty());
    }

    #[test]
    fn names_match_with_or_without_latest_tag() {
        assert!(same_model_name("a-model:latest", "a-model"));
        assert!(same_model_name("a-model", "a-model:latest"));
        assert!(!same_model_name("a-model:q4", "a-model"));
    }

    #[test]
    fn legacy_ids_resolve_to_their_entry() {
        for m in embedded() {
            for legacy in &m.legacy_ids {
                assert_eq!(m.legacy_id_for(&format!("{legacy}:latest")), Some(legacy.as_str()));
                assert!(!m.is_current_name(legacy));
            }
        }
    }

    #[test]
    fn duplicate_names_are_rejected() {
        let text = r#"{"models":[
            {"id":"x","legacy_ids":["old"],"base_model":"b","display_name":"X","approx_size_gb":1,"min_ram_gb":1,"modelfile":{}},
            {"id":"old:latest","base_model":"c","display_name":"Y","approx_size_gb":1,"min_ram_gb":1,"modelfile":{}}
        ]}"#;
        assert!(parse(text).unwrap_err().contains("duplicate"));
    }

    #[test]
    fn a_model_cannot_be_named_like_its_base() {
        let text = r#"{"models":[
            {"id":"b","base_model":"b","display_name":"X","approx_size_gb":1,"min_ram_gb":1,"modelfile":{}}
        ]}"#;
        assert!(parse(text).is_err());
    }

    #[test]
    fn modelfile_texts_do_not_force_a_reply_language() {
        for m in embedded() {
            let system = m.modelfile.system.unwrap_or_default().to_lowercase();
            assert!(!system.contains("in italiano"), "{}: system text forces Italian", m.id);
        }
    }
}
