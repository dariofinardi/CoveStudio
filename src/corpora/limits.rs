//! Runtime hardening for the corpus-import pipeline.
//!
//! Today's only knob is `max_parquet_file_size_mb` — a hard byte cap
//! applied to any buffer before it reaches the `parquet` decoder.
//! See `config/corpora.json` for the rationale (Apache Thrift advisory,
//! unfixed upstream).
//!
//! Loader pattern mirrors [`crate::presets::model`]: an env override,
//! ancestor-walk discovery, and a permissive fallback when the JSON is
//! missing or malformed so the rest of the app keeps running.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 500 MB. Comfortably above any legitimate Parquet shard we currently
/// ingest (Cassazione shards run ~50-80 MB) and well below the size at
/// which a malicious Thrift footer would matter on a 16 GB workstation.
const DEFAULT_MAX_PARQUET_FILE_SIZE_MB: u64 = 500;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CorporaLimits {
    #[serde(rename = "$schema_version", default)]
    pub schema_version: u32,

    #[serde(default = "default_max_parquet_mb")]
    pub max_parquet_file_size_mb: u64,
}

fn default_max_parquet_mb() -> u64 {
    DEFAULT_MAX_PARQUET_FILE_SIZE_MB
}

impl Default for CorporaLimits {
    fn default() -> Self {
        Self {
            schema_version: 0,
            max_parquet_file_size_mb: DEFAULT_MAX_PARQUET_FILE_SIZE_MB,
        }
    }
}

impl CorporaLimits {
    /// Convenience: the cap in raw bytes.
    pub fn max_parquet_file_size_bytes(&self) -> u64 {
        self.max_parquet_file_size_mb.saturating_mul(1_024 * 1_024)
    }
}

/// Resolve `config/corpora.json`. Lookup order mirrors
/// [`crate::presets::model::catalogue_path`].
pub fn limits_path() -> PathBuf {
    if let Some(p) = crate::product::env_var("CORPORA_LIMITS") {
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
    PathBuf::from("./config/corpora.json")
}

fn walk_ancestors_for(start: &Path) -> Option<PathBuf> {
    for anc in start.ancestors() {
        let candidate = anc.join("config").join("corpora.json");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Load limits from disk; fall back to defaults (with a warning) on any
/// I/O or parse failure. Bulk loaders should call this once and pass
/// the result down — re-reading per shard is unnecessary and could mask
/// a runtime config edit racing with an in-flight import.
pub fn load_limits() -> CorporaLimits {
    let path = limits_path();
    match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<CorporaLimits>(&bytes) {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(
                    "[corpora-limits] failed to parse {}: {e} — using defaults",
                    path.display()
                );
                CorporaLimits::default()
            }
        },
        Err(e) => {
            tracing::warn!(
                "[corpora-limits] {} not readable ({e}) — using defaults",
                path.display()
            );
            CorporaLimits::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_500mb() {
        let l = CorporaLimits::default();
        assert_eq!(l.max_parquet_file_size_mb, 500);
        assert_eq!(l.max_parquet_file_size_bytes(), 500 * 1024 * 1024);
    }

    #[test]
    fn parses_real_config() {
        let json = r#"{
            "$schema_version": 1,
            "max_parquet_file_size_mb": 250
        }"#;
        let l: CorporaLimits = serde_json::from_str(json).unwrap();
        assert_eq!(l.schema_version, 1);
        assert_eq!(l.max_parquet_file_size_mb, 250);
    }

    #[test]
    fn missing_field_uses_default() {
        let json = r#"{ "$schema_version": 1 }"#;
        let l: CorporaLimits = serde_json::from_str(json).unwrap();
        assert_eq!(l.max_parquet_file_size_mb, 500);
    }

    #[test]
    fn byte_arithmetic_does_not_overflow() {
        let l = CorporaLimits {
            schema_version: 1,
            max_parquet_file_size_mb: u64::MAX,
        };
        // Should saturate, not panic.
        assert_eq!(l.max_parquet_file_size_bytes(), u64::MAX);
    }
}
