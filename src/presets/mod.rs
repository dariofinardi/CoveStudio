//! JSON-driven preset registries for system-shipped workflows and
//! column shortcuts.
//!
//! The two registries are loaded once at startup from on-disk directories
//! (`workflow-presets/<domain>/*.json` and `column-presets/<domain>/*.json`)
//! and held in `AppState`. They are NEVER written to the DB — the
//! `/workflow` list endpoint merges them with the user's own rows at
//! response time, so adding a new preset is as simple as dropping a
//! JSON file into the right directory and restarting the app.
//!
//! Built-in workflows are non-editable from the UI: the existing
//! workflow update/delete handlers use `WHERE user_id = ? AND id = ?`
//! filters, so a row with `user_id IS NULL` is effectively read-only
//! through the API. The frontend additionally surfaces `is_system:
//! true` to grey out the edit/delete affordances.
//!
//! Mirrors the corpus-plugin pattern in `crate::corpora::plugin` —
//! same ancestor-walking directory resolution, same fail-soft
//! parsing (one broken JSON does not stop the rest from loading).

pub mod column;
pub mod docx_template;
pub mod local_models;
pub mod model;
pub mod system_prompt;
pub mod workflow;

use std::path::{Path, PathBuf};

/// Resolve the on-disk directory for a preset family. Lookup order:
///
///   1. `COVE_<KIND>_PRESETS_DIR` env var (absolute path).
///   2. Walk ancestors from CWD looking for `config/<kind>-presets/`.
///   3. Walk ancestors from the current executable's path.
///   4. Fallback to `./config/<kind>-presets`.
///
/// `kind` is the lowercase prefix used both in the env var and in the
/// directory name — `"workflow"` resolves to `COVE_WORKFLOW_PRESETS_DIR`
/// and `config/workflow-presets/`; `"column"` resolves to
/// `COVE_COLUMN_PRESETS_DIR` and `config/column-presets/`. All
/// JSON-driven config families live under the single `config/` root
/// at the repository top level (alongside `config/corpora-plugins/`).
pub fn presets_dir(kind: &str) -> PathBuf {
    let env_name = format!("{}_PRESETS_DIR", kind.to_ascii_uppercase());
    let dir_name = format!("{kind}-presets");

    if let Some(dir) = crate::product::env_var(&env_name) {
        return PathBuf::from(dir);
    }
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(found) = walk_ancestors_for(&cwd, &dir_name) {
            return found;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(found) = walk_ancestors_for(&exe, &dir_name) {
            return found;
        }
    }
    PathBuf::from(format!("./config/{dir_name}"))
}

/// Resolve any subdirectory under `config/` — used for registries
/// whose folder name doesn't follow the `<kind>-presets` convention.
/// Same ancestor-walking lookup as `presets_dir`. Today's only caller
/// is the docx-template registry (`config/docx-templates/`).
///
/// Lookup order:
///   1. `COVE_<NAME>_DIR` env var (absolute path), where `<NAME>` is
///      `dir_name` upper-cased with `-` rewritten to `_`.
///   2. Walk ancestors from CWD for `config/<dir_name>/`.
///   3. Walk ancestors from the current executable's path.
///   4. Fallback to `./config/<dir_name>`.
pub fn config_subdir(dir_name: &str) -> PathBuf {
    let env_name = format!("{}_DIR", dir_name.to_ascii_uppercase().replace('-', "_"));
    if let Some(dir) = crate::product::env_var(&env_name) {
        return PathBuf::from(dir);
    }
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(found) = walk_ancestors_for(&cwd, dir_name) {
            return found;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(found) = walk_ancestors_for(&exe, dir_name) {
            return found;
        }
    }
    PathBuf::from(format!("./config/{dir_name}"))
}

fn walk_ancestors_for(start: &Path, dir_name: &str) -> Option<PathBuf> {
    for anc in start.ancestors() {
        let candidate = anc.join("config").join(dir_name);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

/// Collect every `.json` file under `root`, recursing into subdirectories
/// one level deep so domain-organised layouts like
/// `workflow-presets/legal/*.json` and `workflow-presets/insurance/*.json`
/// work without configuration. Hidden directories (starting with `.`)
/// and any non-`.json` files are skipped.
pub fn collect_json_files(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out: Vec<PathBuf> = Vec::new();
    if !root.is_dir() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
            out.push(path);
        } else if path.is_dir() {
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if name.starts_with('.') {
                continue;
            }
            // One-level recursion is enough for the `<domain>/<slug>.json`
            // layout we ship today. Deeper trees can be added later if a
            // jurisdiction needs sub-folders (e.g. country → sector).
            for sub in std::fs::read_dir(&path)? {
                let sub = sub?;
                let sub_path = sub.path();
                if sub_path.is_file()
                    && sub_path.extension().and_then(|s| s.to_str()) == Some("json")
                {
                    out.push(sub_path);
                }
            }
        }
    }
    out.sort();
    Ok(out)
}
