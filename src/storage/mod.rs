use anyhow::{anyhow, Result};
use std::path::{Component, Path, PathBuf};
use tokio::fs;

/// Unified storage trait. Only `LocalStorage` ships in v0.5.2+; the
/// historical S3/R2 fallback was removed when its AWS SDK chain
/// transitively pinned a vulnerable rustls 0.21.12 / rustls-webpki
/// 0.101.7 and was never actually wired into `make_storage` (the S3
/// implementation lived only on a feature branch). The trait remains
/// for the small ergonomic win of a single `Box<dyn Storage>` handle
/// across the codebase and to keep the door open for a sovereign-cloud
/// backend later (rustls 0.23 native).
#[async_trait::async_trait]
pub trait Storage: Send + Sync {
    async fn put(&self, key: &str, data: &[u8], content_type: &str) -> Result<()>;
    async fn get(&self, key: &str) -> Result<Vec<u8>>;
    async fn delete(&self, key: &str) -> Result<()>;
    /// Returns a URL or file:// path usable for download
    async fn public_url(&self, key: &str) -> Result<String>;
}

// ---------------------------------------------------------------------------
// Local filesystem implementation
// ---------------------------------------------------------------------------

pub struct LocalStorage {
    base: PathBuf,
}

/// Default base directory for `LocalStorage` when `STORAGE_PATH` is
/// unset. Mirrors `db::default_db_url` and `lib::ensure_data_dir`:
/// everything we persist that isn't shipped read-only with the binary
/// lives under `<home>/cove-studio-data/`. The old default was
/// `./data/storage`, a cwd-relative path that worked in `cargo run`
/// (cwd = workspace root) but blew up the moment the user double-
/// clicked the installed MSI — Windows resolves the relative path
/// against the launch cwd (often `C:\Program Files\Cove Studio\` for a
/// shortcut, sometimes `C:\Windows\System32` when launched through
/// "Run"), neither of which is writable by a non-admin user. The
/// first storage `put` then failed with `os error 5` (ACCESS_DENIED)
/// and the WebView surfaced it as "Could not load document — Accesso
/// negato". Anchoring to `%USERPROFILE%` / `$HOME` puts the storage
/// next to the SQLite DB so a backup is one folder copy.
pub fn default_storage_path() -> String {
    crate::product::data_subdir("storage").display().to_string()
}

impl LocalStorage {
    pub fn new() -> Result<Self> {
        let base = PathBuf::from(
            std::env::var("STORAGE_PATH").unwrap_or_else(|_| default_storage_path()),
        );
        std::fs::create_dir_all(&base)?;
        // Resolve the base once so `safe_path_under` can prefix-check
        // against an unambiguous absolute form. If canonicalize fails
        // (rare — symlinks to deleted paths, exotic FS), keep the raw
        // base and let the per-call check fail gracefully.
        let base = std::fs::canonicalize(&base).unwrap_or(base);
        Ok(Self { base })
    }

    /// Resolve a storage key into an absolute path **guaranteed** to
    /// live under `self.base`, rejecting any input that tries to escape
    /// (via `..` segments, absolute paths, or symlinks). Returns `Err`
    /// when the key would resolve outside the base directory; callers
    /// then return a 4xx upstream instead of touching the file system.
    ///
    /// The previous implementation did `key.replace("..", "")`, which
    /// failed three ways: (a) `PathBuf::join` of an absolute key
    /// replaces the base entirely; (b) a single `.` segment plus
    /// platform path-separator games can still escape on Windows;
    /// (c) symlinks under `base` to outside-base files are not
    /// detected. Component-by-component validation closes all three.
    fn safe_path_under(&self, key: &str) -> Result<PathBuf> {
        let normalised = key.replace('\\', "/");
        let trimmed = normalised.trim_start_matches('/');
        let rel = Path::new(trimmed);

        let mut acc = self.base.clone();
        for component in rel.components() {
            match component {
                // Plain component — push and continue.
                Component::Normal(seg) => acc.push(seg),
                // Current-dir noise — ignore.
                Component::CurDir => continue,
                // Anything else (RootDir, Prefix, ParentDir) signals
                // an escape attempt. Refuse explicitly.
                Component::RootDir
                | Component::Prefix(_)
                | Component::ParentDir => {
                    return Err(anyhow!(
                        "rejecting storage key with non-relative component: {key:?}"
                    ));
                }
            }
        }

        // Final prefix check: if the file already exists, canonicalize
        // and verify it's still under `base` (catches symlink escapes
        // pointing outside). If not yet created (typical for put), use
        // the parent directory's canonical form.
        let canonical = match std::fs::canonicalize(&acc) {
            Ok(c) => c,
            Err(_) => {
                let parent = acc.parent().unwrap_or(&self.base);
                std::fs::create_dir_all(parent).ok();
                let parent_canon = std::fs::canonicalize(parent)
                    .unwrap_or_else(|_| parent.to_path_buf());
                let leaf = acc.file_name().ok_or_else(|| {
                    anyhow!("storage key has no filename component: {key:?}")
                })?;
                parent_canon.join(leaf)
            }
        };

        if !canonical.starts_with(&self.base) {
            return Err(anyhow!(
                "storage key escapes base dir: key={key:?} resolved={canonical:?}"
            ));
        }
        Ok(canonical)
    }
}

#[async_trait::async_trait]
impl Storage for LocalStorage {
    async fn put(&self, key: &str, data: &[u8], _content_type: &str) -> Result<()> {
        let path = self.safe_path_under(key)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&path, data).await?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>> {
        Ok(fs::read(self.safe_path_under(key)?).await?)
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let path = self.safe_path_under(key)?;
        if path.exists() {
            fs::remove_file(path).await?;
        }
        Ok(())
    }

    async fn public_url(&self, key: &str) -> Result<String> {
        // In local mode serve via /download/:key endpoint
        let api_base = std::env::var("API_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:3001".to_string());
        Ok(format!("{api_base}/download/{key}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_storage(tmp: &tempfile::TempDir) -> LocalStorage {
        let base = tmp.path().to_path_buf();
        std::fs::create_dir_all(&base).unwrap();
        let base = std::fs::canonicalize(&base).unwrap();
        LocalStorage { base }
    }

    #[test]
    fn safe_path_accepts_simple_relative_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let s = fresh_storage(&tmp);
        let p = s.safe_path_under("documents/user-1/abc").unwrap();
        assert!(p.starts_with(&s.base));
    }

    #[test]
    fn safe_path_strips_leading_slash() {
        let tmp = tempfile::tempdir().unwrap();
        let s = fresh_storage(&tmp);
        let p = s.safe_path_under("/documents/user-1/abc").unwrap();
        assert!(p.starts_with(&s.base));
    }

    #[test]
    fn safe_path_rejects_parent_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let s = fresh_storage(&tmp);
        assert!(s.safe_path_under("../etc/passwd").is_err());
        assert!(s.safe_path_under("documents/../../../etc/passwd").is_err());
        assert!(s.safe_path_under("/../etc/passwd").is_err());
    }

    #[test]
    fn safe_path_rejects_windows_drive_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let s = fresh_storage(&tmp);
        // A bare leading slash is *not* an attack — it's sanitised to
        // a relative path. The dangerous form is a key carrying a
        // platform-specific prefix that `PathBuf::join` honours as
        // absolute, replacing the base. On Windows that's the drive
        // letter (`C:\…`); we refuse it via the `Prefix` component.
        if cfg!(windows) {
            assert!(s.safe_path_under("C:\\Windows\\system32").is_err());
            assert!(s.safe_path_under("D:/etc/passwd").is_err());
        }
    }

    #[test]
    fn safe_path_strips_leading_slash_to_relative() {
        let tmp = tempfile::tempdir().unwrap();
        let s = fresh_storage(&tmp);
        // Confirm that an unintentionally-prefixed key (the call
        // sites add `/` for readability) lands cleanly under base.
        let p = s.safe_path_under("/absolute/path").unwrap();
        assert!(p.starts_with(&s.base));
        assert!(p.ends_with("path"));
    }

    #[test]
    fn safe_path_rejects_backslash_traversal_on_any_platform() {
        let tmp = tempfile::tempdir().unwrap();
        let s = fresh_storage(&tmp);
        // Backslashes are converted to `/` before splitting so even
        // on Unix the `..` segments are detected.
        assert!(s.safe_path_under("..\\..\\etc\\passwd").is_err());
    }

    #[test]
    fn safe_path_keeps_current_dir_noise() {
        let tmp = tempfile::tempdir().unwrap();
        let s = fresh_storage(&tmp);
        // `./foo/./bar` collapses to `foo/bar` without escaping.
        let p = s.safe_path_under("./foo/./bar").unwrap();
        assert!(p.starts_with(&s.base));
        assert!(p.ends_with("bar"));
    }
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

pub fn make_storage() -> Result<Box<dyn Storage>> {
    // Local filesystem only. The S3/R2 path was removed in v0.5.2 —
    // see the `Storage` trait docstring above for the rationale.
    Ok(Box::new(LocalStorage::new()?))
}
