// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

//! Product identity: display name, per-user data folder, environment
//! variable prefix, exported project format and network user agent.
//!
//! This is the single place to edit when the product is renamed. Every
//! previous value goes into the matching `LEGACY_*` list so existing
//! installations keep working: the data folder, the database and the
//! model caches are moved on first start, old environment variables are
//! still honoured and old project files can still be imported.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Name shown to users (dialogs, archive readme, MCP client info).
pub const NAME: &str = "Cove Studio";

/// Lowercase ASCII identifier used in user agents, file and model names.
pub const SLUG: &str = "covestudio";

/// Per-user data folder, created under the home directory.
pub const DATA_DIR_NAME: &str = "cove-studio-data";

/// Data folder names used by previous releases, newest first.
pub const LEGACY_DATA_DIR_NAMES: &[&str] = &["mikerust-data"];

/// SQLite database file inside the data folder.
pub const DATABASE_FILE_NAME: &str = "cove-studio.db";

/// Database file names used by previous releases, newest first.
pub const LEGACY_DATABASE_FILE_NAMES: &[&str] = &["mike.db"];

/// Log file written by the desktop shell inside the data folder.
pub const SHELL_LOG_FILE_NAME: &str = "cove-studio.log";

/// Prefix of every product-specific environment variable.
pub const ENV_PREFIX: &str = "COVE_";

/// Prefixes used by previous releases, still honoured as a fallback.
pub const LEGACY_ENV_PREFIXES: &[&str] = &["MRUST_"];

/// Extension (without the dot) of encrypted project export files.
pub const PROJECT_FILE_EXTENSION: &str = "coveprj";

/// Project file extensions from previous releases, still importable.
pub const LEGACY_PROJECT_FILE_EXTENSIONS: &[&str] = &["mikeprj"];

/// `User-Agent` for outgoing HTTP requests: `<slug>/<version>`, plus an
/// optional purpose, e.g. `covestudio/0.7.5 (italian-legal-corpus importer)`.
pub fn user_agent(purpose: Option<&str>) -> String {
    let base = format!("{SLUG}/{}", env!("CARGO_PKG_VERSION"));
    match purpose {
        Some(p) => format!("{base} ({p})"),
        None => base,
    }
}

/// Name for a scratch file in the system temp folder, prefixed so the
/// product's leftovers are recognisable.
pub fn temp_file_name(suffix: &str) -> String {
    format!("{SLUG}-{suffix}")
}

/// `<name>.<extension>` for an exported project archive.
pub fn project_file_name(stem: &str) -> String {
    format!("{stem}.{PROJECT_FILE_EXTENSION}")
}

/// True when `file_name` carries the current or a legacy project extension.
pub fn is_project_file_name(file_name: &str) -> bool {
    let Some((_, ext)) = file_name.rsplit_once('.') else { return false };
    std::iter::once(PROJECT_FILE_EXTENSION)
        .chain(LEGACY_PROJECT_FILE_EXTENSIONS.iter().copied())
        .any(|known| ext.eq_ignore_ascii_case(known))
}

/// Full name of a product environment variable, e.g. `COVE_MODEL_CATALOGUE`.
pub fn env_var_name(name: &str) -> String {
    format!("{ENV_PREFIX}{name}")
}

/// Value of a product environment variable, looked up under the current
/// prefix first and then under each legacy prefix.
pub fn env_var(name: &str) -> Option<String> {
    std::iter::once(ENV_PREFIX)
        .chain(LEGACY_ENV_PREFIXES.iter().copied())
        .find_map(|prefix| std::env::var(format!("{prefix}{name}")).ok())
}

/// The user's home directory (`USERPROFILE` on Windows, `HOME` elsewhere).
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// Per-user data folder. Resolved once per process: on first call a
/// folder left by a previous release is moved to the current name.
/// Without a home directory it falls back to a folder in the working
/// directory. The folder itself is not created here.
pub fn data_dir() -> PathBuf {
    static RESOLVED: OnceLock<PathBuf> = OnceLock::new();
    RESOLVED
        .get_or_init(|| {
            let home = home_dir().unwrap_or_else(|| PathBuf::from("."));
            migrate_entry(&home, DATA_DIR_NAME, LEGACY_DATA_DIR_NAMES)
        })
        .clone()
}

/// A subfolder of the data folder, e.g. `data_subdir("storage")`.
pub fn data_subdir(name: &str) -> PathBuf {
    data_dir().join(name)
}

/// SQLite database path. On first call a database left under a legacy
/// file name is renamed together with its `-wal` and `-shm` companions,
/// so no committed transaction is lost.
pub fn database_path() -> PathBuf {
    static RESOLVED: OnceLock<PathBuf> = OnceLock::new();
    RESOLVED
        .get_or_init(|| {
            let dir = data_dir();
            let path = migrate_entry(&dir, DATABASE_FILE_NAME, LEGACY_DATABASE_FILE_NAMES);
            if path.file_name().and_then(|n| n.to_str()) == Some(DATABASE_FILE_NAME) {
                for suffix in ["-wal", "-shm"] {
                    let current = format!("{DATABASE_FILE_NAME}{suffix}");
                    let legacy: Vec<String> = LEGACY_DATABASE_FILE_NAMES
                        .iter()
                        .map(|name| format!("{name}{suffix}"))
                        .collect();
                    let legacy: Vec<&str> = legacy.iter().map(String::as_str).collect();
                    migrate_entry(&dir, &current, &legacy);
                }
            }
            path
        })
        .clone()
}

/// Picks the entry (file or folder) named `current` inside `parent`. When
/// it does not exist, the newest legacy entry found is renamed to it; if
/// the rename fails (in use, permissions) the legacy entry is returned as
/// it is, so no data is ever left behind. With nothing to migrate the
/// path of `current` is returned without creating anything.
pub fn migrate_entry(parent: &Path, current: &str, legacy: &[&str]) -> PathBuf {
    let target = parent.join(current);
    if target.exists() {
        return target;
    }
    let Some(previous) = legacy.iter().map(|name| parent.join(name)).find(|p| p.exists()) else {
        return target;
    };
    match std::fs::rename(&previous, &target) {
        Ok(()) => {
            tracing::info!(
                "[product] moved {} to {}",
                previous.display(),
                target.display()
            );
            target
        }
        Err(e) => {
            tracing::warn!(
                "[product] could not move {} to {} ({e}); keeping the previous name",
                previous.display(),
                target.display()
            );
            previous
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn current_entry_wins_and_legacy_is_left_alone() {
        let home = TempDir::new().unwrap();
        std::fs::create_dir(home.path().join("new-data")).unwrap();
        std::fs::create_dir(home.path().join("old-data")).unwrap();
        let dir = migrate_entry(home.path(), "new-data", &["old-data"]);
        assert_eq!(dir, home.path().join("new-data"));
        assert!(home.path().join("old-data").exists());
    }

    #[test]
    fn legacy_folder_is_moved_with_its_content() {
        let home = TempDir::new().unwrap();
        let old = home.path().join("old-data");
        std::fs::create_dir(&old).unwrap();
        std::fs::write(old.join("app.db"), b"data").unwrap();
        let dir = migrate_entry(home.path(), "new-data", &["older-data", "old-data"]);
        assert_eq!(dir, home.path().join("new-data"));
        assert_eq!(std::fs::read(dir.join("app.db")).unwrap(), b"data");
        assert!(!old.exists());
    }

    #[test]
    fn legacy_file_is_moved() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("old.db"), b"rows").unwrap();
        let path = migrate_entry(dir.path(), "new.db", &["old.db"]);
        assert_eq!(std::fs::read(path).unwrap(), b"rows");
        assert!(!dir.path().join("old.db").exists());
    }

    #[test]
    fn fresh_install_gets_the_current_name() {
        let home = TempDir::new().unwrap();
        let dir = migrate_entry(home.path(), "new-data", &["old-data"]);
        assert_eq!(dir, home.path().join("new-data"));
        assert!(!dir.exists(), "the entry is created by its first user, not here");
    }

    #[test]
    fn project_file_names() {
        assert_eq!(project_file_name("Pratica"), format!("Pratica.{PROJECT_FILE_EXTENSION}"));
        assert!(is_project_file_name(&project_file_name("x")));
        assert!(is_project_file_name(&format!("X.{}", PROJECT_FILE_EXTENSION.to_uppercase())));
        for legacy in LEGACY_PROJECT_FILE_EXTENSIONS {
            assert!(is_project_file_name(&format!("vecchio.{legacy}")));
        }
        assert!(!is_project_file_name("x.zip"));
        assert!(!is_project_file_name("noext"));
    }

    #[test]
    fn user_agent_and_temp_names_use_the_slug() {
        assert!(user_agent(None).starts_with(&format!("{SLUG}/")));
        assert!(user_agent(Some("importer")).ends_with(" (importer)"));
        assert_eq!(temp_file_name("doc.pdf"), format!("{SLUG}-doc.pdf"));
    }

    #[test]
    fn env_var_reads_current_then_legacy_prefix() {
        let name = "PRODUCT_TEST_ONLY_VARIABLE";
        // SAFETY: the variable names are unique to this test.
        unsafe { std::env::set_var(env_var_name(name), "on") };
        assert_eq!(env_var(name).as_deref(), Some("on"));
        unsafe { std::env::remove_var(env_var_name(name)) };
        assert_eq!(env_var(name), None);
        if let Some(legacy) = LEGACY_ENV_PREFIXES.first() {
            unsafe { std::env::set_var(format!("{legacy}{name}"), "old") };
            assert_eq!(env_var(name).as_deref(), Some("old"));
            unsafe { std::env::remove_var(format!("{legacy}{name}")) };
        }
    }
}
