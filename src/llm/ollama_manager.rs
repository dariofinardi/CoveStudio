//! Plug-and-play Ollama lifecycle for the "Modalità sicura locale" flow.
//!
//! In secure mode the local OpenAI-compatible provider in [`super::local`]
//! refuses any base URL that isn't loopback and any model that isn't in
//! the local-models catalogue (`config/local-models/ollama.json`, see
//! [`crate::presets::local_models`]). This module makes that catalogue
//! usable end to end without a terminal:
//!
//!   * [`heartbeat`] — is Ollama running on loopback?
//!   * [`list_installed`] — which models are already present?
//!   * [`ensure`] — idempotent install: pull the base model if missing,
//!     then create the derivation described in the catalogue, streaming
//!     progress as [`EnsureEvent`].
//!   * [`uninstall`] — remove a derivation (the base model stays).
//!   * [`find_legacy_installs`] / [`migrate_legacy`] / [`remove_legacy`] —
//!     carry derivations created under names from earlier releases over
//!     to the current names. Migration copies the model (no download, the
//!     layers are shared); removing the old names is a separate step the
//!     UI performs only after the user agrees.
//!
//! No model name is written in this module: every name comes from the
//! catalogue.

use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use ollama_rs::generation::completion::request::GenerationRequest;
use ollama_rs::models::create::CreateModelRequest;
use ollama_rs::models::pull::PullModelStatus;
use ollama_rs::models::ModelOptions;
use ollama_rs::Ollama;
use serde::Serialize;

use crate::presets::local_models::{self, LocalModel, NameMigration};

/// Loopback URL used in secure mode. Fixed on purpose: secure mode is the
/// mode that forbids pointing the local provider anywhere else.
pub const SECURE_BASE_URL: &str = "http://localhost:11434";

fn client() -> Ollama {
    Ollama::new("http://localhost".to_string(), 11434)
}

/// Is Ollama serving on the loopback port?
pub async fn heartbeat() -> bool {
    client().list_local_models().await.is_ok()
}

/// Names of the models present on this Ollama instance, as Ollama lists
/// them (usually with a `:latest` tag).
pub async fn list_installed() -> Result<Vec<String>> {
    let models = client()
        .list_local_models()
        .await
        .context("ollama list_local_models")?;
    Ok(models.into_iter().map(|m| m.name).collect())
}

/// True when `name` is present in `installed`, ignoring the `:latest` tag.
pub fn is_installed(installed: &[String], name: &str) -> bool {
    installed.iter().any(|i| local_models::same_model_name(i, name))
}

/// Progress of [`ensure`], forwarded as SSE by the Settings route.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "phase")]
pub enum EnsureEvent {
    Started { model_id: String },
    Pulling {
        status: String,
        completed_bytes: u64,
        total_bytes: u64,
    },
    Creating { model_id: String },
    Ready { model_id: String },
    Error { message: String },
}

/// Brings the catalogue model `id` to a usable state: pulls the base model
/// if missing, then creates the derivation if missing.
pub fn ensure(id: String) -> impl futures_util::Stream<Item = EnsureEvent> + Send + 'static {
    async_stream::stream! {
        let Some(entry) = local_models::find(&id) else {
            yield EnsureEvent::Error { message: format!("Modello non in catalogo: {id}") };
            return;
        };
        yield EnsureEvent::Started { model_id: entry.id.clone() };

        let ollama = client();
        let installed: Vec<String> = match ollama.list_local_models().await {
            Ok(v) => v.into_iter().map(|m| m.name).collect(),
            Err(e) => {
                yield EnsureEvent::Error { message: format!("Ollama non raggiungibile: {e}") };
                return;
            }
        };

        if !is_installed(&installed, &entry.base_model) {
            let mut pull = match ollama.pull_model_stream(entry.base_model.clone(), false).await {
                Ok(s) => s,
                Err(e) => {
                    yield EnsureEvent::Error { message: format!("ollama pull avvio fallito: {e}") };
                    return;
                }
            };
            while let Some(chunk) = pull.next().await {
                match chunk {
                    Ok(PullModelStatus { message, total, completed, .. }) => {
                        yield EnsureEvent::Pulling {
                            status: message,
                            completed_bytes: completed.unwrap_or(0),
                            total_bytes: total.unwrap_or(0),
                        };
                    }
                    Err(e) => {
                        yield EnsureEvent::Error { message: format!("ollama pull stream interrotto: {e}") };
                        return;
                    }
                }
            }
        }

        if !is_installed(&installed, &entry.id) {
            yield EnsureEvent::Creating { model_id: entry.id.clone() };
            let mut create = match ollama.create_model_stream(build_create_request(&entry)).await {
                Ok(s) => s,
                Err(e) => {
                    yield EnsureEvent::Error { message: format!("ollama create avvio fallito: {e}") };
                    return;
                }
            };
            while let Some(chunk) = create.next().await {
                if let Err(e) = chunk {
                    yield EnsureEvent::Error { message: format!("ollama create stream errore: {e}") };
                    return;
                }
            }
        }

        yield EnsureEvent::Ready { model_id: entry.id.clone() };
    }
}

/// Removes the derivation `id`; the base model stays installed.
pub async fn uninstall(id: &str) -> Result<()> {
    let entry = local_models::find(id).ok_or_else(|| anyhow!("Modello non in catalogo: {id}"))?;
    client()
        .delete_model(entry.id)
        .await
        .context("ollama delete_model")
}

/// Loads the model into memory so the first chat doesn't pay the cold start.
pub async fn warm_up(id: &str) -> Result<()> {
    client()
        .generate(GenerationRequest::new(id.to_string(), " ".to_string()))
        .await
        .context("ollama warm-up generation")?;
    Ok(())
}

/// Builds the Ollama create request from the catalogue entry.
fn build_create_request(entry: &LocalModel) -> CreateModelRequest {
    let spec = &entry.modelfile;
    let mut options = ModelOptions::default();
    if let Some(t) = spec.temperature {
        options = options.temperature(t);
    }
    if let Some(n) = spec.num_predict {
        options = options.num_predict(n);
    }
    if !spec.stop.is_empty() {
        options = options.stop(spec.stop.clone());
    }

    let mut request = CreateModelRequest::new(entry.id.clone())
        .from_model(entry.base_model.clone())
        .parameters(options);
    if let Some(template) = &spec.template {
        request = request.template(template.clone());
    }
    if let Some(system) = &spec.system {
        request = request.system(system.clone());
    }
    request
}

// ---------------------------------------------------------------------------
// Names from earlier releases
// ---------------------------------------------------------------------------

/// A model installed under a legacy name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LegacyInstall {
    /// Name that replaces it.
    pub current_name: String,
    /// Name as listed by Ollama (e.g. `old-name:latest`).
    pub installed_name: String,
    /// Whether the current name is already installed too.
    pub current_installed: bool,
}

/// Every legacy name of `migrations` found in `installed`.
pub fn find_legacy_installs(migrations: &[NameMigration], installed: &[String]) -> Vec<LegacyInstall> {
    migrations
        .iter()
        .flat_map(|migration| {
            let current_installed = is_installed(installed, &migration.current);
            installed
                .iter()
                .filter(|name| local_models::same_model_name(name, &migration.legacy))
                .map(move |name| LegacyInstall {
                    current_name: migration.current.clone(),
                    installed_name: name.clone(),
                    current_installed,
                })
        })
        .collect()
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct MigrationReport {
    /// Derivations now available under the current name.
    pub migrated: Vec<RenamedModel>,
    /// Derivations that could not be copied: they must be installed again.
    pub failed: Vec<FailedMigration>,
    /// Legacy names still present in Ollama, removable after the user agrees.
    pub leftovers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenamedModel {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FailedMigration {
    pub from: String,
    pub to: String,
    pub error: String,
}

/// Copies every legacy derivation to its current name when the current
/// one is missing. Nothing is deleted here.
pub async fn migrate_legacy() -> Result<MigrationReport> {
    let installed = list_installed().await?;
    let legacy = find_legacy_installs(&local_models::name_migrations(), &installed);
    let ollama = client();
    let mut report = MigrationReport::default();
    let mut done: std::collections::HashSet<String> = std::collections::HashSet::new();

    for item in legacy {
        if item.current_installed || done.contains(&item.current_name) {
            report.leftovers.push(item.installed_name);
            continue;
        }
        match ollama.copy_model(item.installed_name.clone(), item.current_name.clone()).await {
            Ok(()) => {
                tracing::info!(
                    "[ollama] migrated local model {} → {}",
                    item.installed_name,
                    item.current_name
                );
                done.insert(item.current_name.clone());
                report.migrated.push(RenamedModel {
                    from: item.installed_name.clone(),
                    to: item.current_name,
                });
                report.leftovers.push(item.installed_name);
            }
            Err(e) => {
                tracing::warn!(
                    "[ollama] could not migrate {} → {}: {e}",
                    item.installed_name,
                    item.current_name
                );
                report.failed.push(FailedMigration {
                    from: item.installed_name,
                    to: item.current_name,
                    error: e.to_string(),
                });
            }
        }
    }
    Ok(report)
}

/// Deletes the given legacy names from Ollama. Only names listed as legacy
/// in the catalogue are accepted; anything else is refused before Ollama
/// is contacted. Returns the names actually removed.
pub async fn remove_legacy(names: &[String]) -> Result<Vec<String>> {
    let migrations = local_models::name_migrations();
    if let Some(unknown) = names
        .iter()
        .find(|n| !migrations.iter().any(|m| local_models::same_model_name(n, &m.legacy)))
    {
        return Err(anyhow!("`{unknown}` non è un nome precedente di un modello in catalogo"));
    }
    let ollama = client();
    let mut removed = Vec::with_capacity(names.len());
    for name in names {
        ollama
            .delete_model(name.clone())
            .await
            .with_context(|| format!("ollama delete_model {name}"))?;
        removed.push(name.clone());
    }
    Ok(removed)
}

/// Thinking-suppression preamble prepended to the system prompt in secure
/// mode, as a second line of defence behind the catalogue derivations.
pub fn no_think_preamble() -> &'static str {
    "[Modalità sicura locale] Rispondi sempre in modo diretto e conciso. \
     Non includere ragionamento esplicito tra <think>, <thinking>, <reasoning> \
     o blocchi simili. Vai direttamente alla risposta finale.\n\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::local_models::ModelfileSpec;

    fn model(id: &str, legacy: &[&str], spec: ModelfileSpec) -> LocalModel {
        LocalModel {
            id: id.into(),
            legacy_ids: legacy.iter().map(|s| s.to_string()).collect(),
            base_model: "base:q4".into(),
            display_name: id.into(),
            approx_size_gb: 1.0,
            min_ram_gb: 4,
            modelfile: spec,
        }
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn create_request_carries_every_catalogue_field() {
        let entry = model(
            "new-a",
            &[],
            ModelfileSpec {
                template: Some("{{ .Content }}/no_think".into()),
                system: Some("sii diretto".into()),
                stop: names(&["<think>"]),
                temperature: Some(0.5),
                num_predict: Some(2048),
            },
        );
        let req = build_create_request(&entry);
        assert_eq!(req.model_name, "new-a");
        assert_eq!(req.from_model.as_deref(), Some("base:q4"));
        assert!(req.template.as_deref().unwrap().contains("/no_think"));
        assert_eq!(req.system.as_deref(), Some("sii diretto"));
        let params = serde_json::to_value(req.parameters.as_ref().unwrap()).unwrap();
        assert_eq!(params["stop"][0], "<think>");
        assert_eq!(params["num_predict"], 2048);
    }

    fn migration(legacy: &str, current: &str) -> NameMigration {
        NameMigration { legacy: legacy.into(), current: current.into() }
    }

    #[test]
    fn legacy_installs_are_found_with_or_without_tag() {
        let migrations = vec![
            migration("old-a", "new-a"),
            migration("old-b", "new-b"),
            migration("old-c:ctx8k", "new-c:ctx8k"),
        ];
        let installed = names(&["old-a:latest", "new-b:latest", "old-b", "old-c:ctx8k", "unrelated:latest"]);
        let found = find_legacy_installs(&migrations, &installed);
        assert_eq!(
            found,
            vec![
                LegacyInstall { current_name: "new-a".into(), installed_name: "old-a:latest".into(), current_installed: false },
                LegacyInstall { current_name: "new-b".into(), installed_name: "old-b".into(), current_installed: true },
                LegacyInstall { current_name: "new-c:ctx8k".into(), installed_name: "old-c:ctx8k".into(), current_installed: false },
            ]
        );
    }

    #[test]
    fn nothing_to_migrate_on_a_clean_setup() {
        let migrations = vec![migration("old-a", "new-a")];
        assert!(find_legacy_installs(&migrations, &names(&["new-a:latest"])).is_empty());
        assert!(find_legacy_installs(&migrations, &[]).is_empty());
    }

    #[test]
    fn installed_check_ignores_latest_tag() {
        let installed = names(&["x:latest", "base:q4"]);
        assert!(is_installed(&installed, "x"));
        assert!(is_installed(&installed, "base:q4"));
        assert!(!is_installed(&installed, "base"));
    }

    #[test]
    fn no_think_preamble_marks_itself() {
        let preamble = no_think_preamble();
        assert!(preamble.contains("[Modalità sicura locale]"));
        assert!(preamble.ends_with("\n\n"));
    }
}
