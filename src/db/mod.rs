use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, RwLock};

use crate::auth::SessionStore;

#[cfg(feature = "rag")]
use crate::embeddings::EmbeddingService;
#[cfg(feature = "rag")]
use crate::sync::scanner::ScanProgressHandle;

/// Resolve the SQLite path to a location *outside* the project tree.
///
/// Tauri dev's file watcher rebuilds whenever any file under `src-tauri/`
/// changes. SQLite in WAL mode constantly rewrites `.db-wal` and
/// `.db-shm`, so a DB anywhere under the project triggers an infinite
/// rebuild loop. Default location is the product database inside the
/// per-user data folder (see `crate::product::database_path`)
/// — overridable via `DATABASE_URL` for tests / CI.
fn default_db_url() -> String {
    let path = crate::product::database_path();
    // SQLite URI on Windows requires forward slashes after `sqlite:`.
    format!("sqlite:{}", path.display().to_string().replace('\\', "/"))
}

/// Request sent through the biometric channel.
/// The Tauri side receives it, shows the OS dialog with the correct HWND,
/// and sends back Ok(true/false) or Err(message).
pub type BiometricRequest = (String, oneshot::Sender<Result<bool, String>>);

/// How long an MCP discovery snapshot stays valid before we re-run the
/// `initialize → tools/list → prompts/list` handshake. Five minutes
/// matches the typical horizon at which an MCP server might cycle a
/// session id; before this cache, every chat turn paid the full
/// handshake cost on every configured server. Configurable via env
/// override `MCP_CACHE_TTL_SECS` for tuning / tests.
pub fn mcp_cache_ttl() -> std::time::Duration {
    std::env::var("MCP_CACHE_TTL_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(std::time::Duration::from_secs)
        .unwrap_or_else(|| std::time::Duration::from_secs(300))
}

/// How long to wait for an MCP `tools/call` response before giving up.
/// Default 5 minutes — pseudonymization, OCR, RAG-summary tools can
/// realistically take 60-120 s on a non-trivial doc, and the previous
/// 60 s default tripped over them: every long call returned an opaque
/// `{"error":"network: timeout"}` string and the model would tell the
/// user "communication error" instead of waiting. Override via env
/// `MCP_CALL_TIMEOUT_SECS` for shops that prefer to fail faster.
pub fn mcp_call_timeout_secs() -> u64 {
    std::env::var("MCP_CALL_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&n| n > 0 && n <= 1800)
        .unwrap_or(300)
}

/// Stored per-user. We carry the discovery payload as opaque JSON so
/// `db/mod.rs` doesn't depend on the routes layer's `McpDiscovered`
/// type — the chat handler serialises into this on insert and
/// deserialises on read. Cheap (a handful of MCP servers per user;
/// each one a few hundred bytes of JSON).
#[derive(Clone)]
pub struct McpDiscoveryCacheEntry {
    pub stored_at: std::time::Instant,
    /// JSON-encoded `Vec<McpDiscovered>`. Kept as a string to keep
    /// this module dependency-free.
    pub payload_json: String,
}

impl McpDiscoveryCacheEntry {
    pub fn is_fresh(&self, ttl: std::time::Duration) -> bool {
        self.stored_at.elapsed() < ttl
    }
}

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub sessions: SessionStore,
    /// Send a biometric verification request to the Tauri window process.
    /// None when running outside Tauri (e.g. standalone server).
    pub biometric_tx: Option<mpsc::Sender<BiometricRequest>>,
    /// Cache of model identifiers that the upstream LLM provider has rejected
    /// for `tools=[…]` (e.g. Ollama Gemma3 returns "does not support tools").
    /// Avoids paying the round-trip on every chat request.
    pub no_tools_models: Arc<RwLock<HashSet<String>>>,

    /// Per-user MCP discovery cache. Avoids re-running the
    /// `initialize → notifications/initialized → tools/list → prompts/list`
    /// handshake on every chat turn — without this every user message
    /// hammered every configured MCP server with a fresh session id.
    /// TTL-based; entries older than `MCP_CACHE_TTL` are re-discovered
    /// on the next chat. Manually invalidated when the user updates
    /// MCP server settings (POST/PUT/DELETE on /user/mcp).
    pub mcp_discovery_cache:
        Arc<RwLock<HashMap<String, McpDiscoveryCacheEntry>>>,

    /// Process-wide embedding service (loads multilingual-e5-base once
    /// on first use and reuses it). `None` when the `rag` feature is
    /// disabled at compile time, OR when STORAGE_PATH wasn't configured
    /// (which we need as the on-disk root for per-user Lance dbs).
    #[cfg(feature = "rag")]
    pub embeddings: Option<Arc<EmbeddingService>>,

    /// In-memory map of in-flight scan progress, keyed by `sync_folders.id`.
    /// Populated when `/sync/folders/{id}/scan` kicks off a job; read by
    /// the status endpoint. Cleared when the user removes the folder.
    #[cfg(feature = "rag")]
    pub scans: Arc<RwLock<HashMap<String, ScanProgressHandle>>>,

    /// JSON-driven corpus plugin registry, loaded once at startup from
    /// `COVE_CORPUS_PLUGINS_DIR` (or walks ancestors for `corpora-plugins`
    /// by default). Read by the `/corpora` endpoint and by the chat
    /// library-inventory builder. Empty when no manifest directory
    /// exists — the hardcoded EUR-Lex / Italian routes still work,
    /// the registry is purely metadata for discovery and UI.
    /// Behind a `RwLock` so the dev manifest hot-reloader can swap the
    /// registry in-process when a `config/corpora-plugins/*.json` file
    /// changes — no restart needed.
    pub corpus_plugins:
        Arc<std::sync::RwLock<Vec<crate::corpora::plugin::CorpusPlugin>>>,

    /// Per-corpus runtime adapter for declarative corpora
    /// (`strategy.kind == "http-fetch-per-id"`). Keyed by corpus id.
    /// Built once at startup from `corpus_plugins`. The generic
    /// `/corpora/:id/{search,fetch}` routes look up this map to
    /// dispatch HTTP fetch + extraction without per-corpus Rust code.
    ///
    /// Builtin corpora (EUR-Lex, Italian Legal) are NOT in this
    /// registry today — their existing `/eurlex/*` /
    /// `/italian-legal/*` routes call their adapters directly. They
    /// migrate here when we converge on generic routes.
    pub corpus_adapters:
        Arc<std::sync::RwLock<crate::corpora::manifest_adapter::AdapterRegistry>>,

    /// Live progress for in-flight bulk imports, keyed by corpus id.
    /// Spawned by POST `/corpora/:id/import`, polled by GET
    /// `/corpora/:id/import-progress`. A `phase=="error"` entry
    /// sticks until the next import overwrites it so the UI can
    /// surface the message after the user looks away.
    pub corpus_import_progress: Arc<
        RwLock<HashMap<String, Arc<RwLock<crate::corpora::dila_bulk::ImportProgress>>>>,
    >,

    /// System-shipped workflow templates loaded from
    /// `workflow-presets/<domain>/*.json` at startup. Merged into the
    /// `/workflow` list response with `is_system: true` so the UI
    /// greys out edit/delete affordances. Read-only: the underlying
    /// JSON files are the single source of truth. Use the existing
    /// `user_hidden_workflows` table to scope per-user hiding.
    pub workflow_presets: Arc<Vec<crate::presets::workflow::WorkflowPreset>>,

    /// Column shortcut catalogue loaded from
    /// `column-presets/<domain>/*.json`. The AddColumnModal queries
    /// this via `/column-presets` to suggest a name/format/prompt
    /// triple when the user starts typing a column title.
    pub column_presets: Arc<Vec<crate::presets::column::ColumnPreset>>,

    /// LLM provider/model/region catalogue loaded from
    /// `config/model.json`. Served as-is via `GET /models` to drive
    /// the Settings → Modelli LLM page. Empty when the file is missing
    /// or malformed — the page will just show empty dropdowns.
    pub model_catalogue: Arc<crate::presets::model::ModelCatalogue>,

    /// DOCX template registry — sidecar JSON + companion `.dotx`
    /// files under `config/docx-templates/<domain>/<slug>.{json,dotx}`.
    /// Drives the closing-formatter pipeline that turns
    /// LLM-produced Markdown into print-ready Word documents
    /// styled per Italian professional conventions (see
    /// `docs/TEMPLATE_PRONTUARIO.md`). Served via `GET /docx-templates`.
    pub docx_templates:
        Arc<Vec<crate::presets::docx_template::DocxTemplate>>,
}

impl AppState {
    pub async fn new() -> Result<Self> {
        // Register sqlite-vec as a SQLite auto-extension BEFORE we open
        // any connection. This way every connection sqlx creates
        // (including the one running migrations) gets the `vec0`
        // virtual-table module loaded — required by migration 0009 and
        // by every embedding query later on.
        //
        // The cast goes through `*const ()` because libsqlite3-sys'
        // `sqlite3_auto_extension` expects a generic init function
        // pointer, while sqlite-vec exposes a specifically-typed one.
        // Both ABIs match what SQLite calls at extension load time.
        #[cfg(feature = "rag")]
        {
            crate::embeddings::register_sqlite_vec_auto_extension();
            tracing::info!("[rag] sqlite-vec auto-extension registered");
        }

        let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| default_db_url());

        // SQLite won't auto-create the parent directory; do it explicitly
        // so `<user-home>/cove-studio-data/` exists on first run.
        if let Some(file_path) = db_url.strip_prefix("sqlite:") {
            // Strip query string if any (e.g. ?mode=rwc) before mkdir.
            let raw = file_path.split('?').next().unwrap_or(file_path);
            // Tolerate both `/` and `\` in the URL.
            let pb = PathBuf::from(raw.replace('/', std::path::MAIN_SEPARATOR_STR));
            if let Some(parent) = pb.parent() {
                if !parent.as_os_str().is_empty() {
                    let _ = std::fs::create_dir_all(parent);
                }
            }
        }
        tracing::info!("[db] using DATABASE_URL={db_url}");

        let opts = SqliteConnectOptions::from_str(&db_url)?
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

        let db = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(opts)
            .await?;

        let sessions = SessionStore::new(db.clone());

        // Bootstrap the RAG embedding service. The vector store lives
        // in the same SQLite file via the sqlite-vec virtual table —
        // we just hand the pool to the service. The migration adds the
        // `doc_chunks` vec0 table; if that already ran we're ready.
        #[cfg(feature = "rag")]
        let embeddings: Option<Arc<EmbeddingService>> =
            Some(Arc::new(EmbeddingService::new(db.clone())));

        // Load corpus plugin manifests from disk. Failures are
        // non-fatal: we log and continue with an empty registry — the
        // hardcoded EUR-Lex / Italian routes still serve their requests
        // regardless of what's in the registry.
        let plugins_dir = crate::corpora::plugin::plugins_dir();
        let corpus_plugins = match crate::corpora::plugin::load_plugins(&plugins_dir) {
            Ok(p) => {
                tracing::info!(
                    "[corpus-plugins] {} manifest(s) loaded from {}",
                    p.len(),
                    plugins_dir.display()
                );
                p
            }
            Err(e) => {
                tracing::warn!(
                    "[corpus-plugins] load failed from {}: {:#}",
                    plugins_dir.display(),
                    e
                );
                Vec::new()
            }
        };
        // Build the runtime adapter registry for declarative corpora.
        // Builtin corpora are intentionally NOT inserted here yet —
        // see comment on AppState::corpus_adapters.
        let corpus_adapters =
            crate::corpora::manifest_adapter::build_adapter_registry(&corpus_plugins);
        tracing::info!(
            "[corpus-adapters] {} declarative adapter(s) registered",
            corpus_adapters.len()
        );
        let corpus_plugins =
            Arc::new(std::sync::RwLock::new(corpus_plugins));
        let corpus_adapters =
            Arc::new(std::sync::RwLock::new(corpus_adapters));
        // Dev convenience: watch config/corpora-plugins/ and hot-reload
        // manifests in-process so connector edits don't need a restart.
        // Debug builds only — a packaged app ships frozen manifests.
        if cfg!(debug_assertions) {
            crate::corpora::manifest_adapter::spawn_manifest_reloader(
                plugins_dir.clone(),
                corpus_plugins.clone(),
                corpus_adapters.clone(),
            );
        }

        // Workflow + column preset registries. Same fail-soft policy as
        // corpus plugins: a broken JSON or missing directory logs a
        // warning and the registry stays empty rather than blocking
        // startup. Both are read-only at runtime — the JSON files on
        // disk are the single source of truth.
        let workflow_presets_dir = crate::presets::presets_dir("workflow");
        let workflow_presets = crate::presets::workflow::load_workflow_presets(
            &workflow_presets_dir,
        )
        .unwrap_or_else(|e| {
            tracing::warn!(
                "[workflow-presets] load failed from {}: {:#}",
                workflow_presets_dir.display(),
                e
            );
            Vec::new()
        });
        tracing::info!(
            "[workflow-presets] {} preset(s) loaded from {}",
            workflow_presets.len(),
            workflow_presets_dir.display()
        );

        let column_presets_dir = crate::presets::presets_dir("column");
        let column_presets =
            crate::presets::column::load_column_presets(&column_presets_dir)
                .unwrap_or_else(|e| {
                    tracing::warn!(
                        "[column-presets] load failed from {}: {:#}",
                        column_presets_dir.display(),
                        e
                    );
                    Vec::new()
                });
        tracing::info!(
            "[column-presets] {} preset(s) loaded from {}",
            column_presets.len(),
            column_presets_dir.display()
        );

        // DOCX template registry. Mirrors the workflow/column-preset
        // pattern: drop a sidecar JSON (+ companion `.dotx`) into
        // `config/docx-templates/<domain>/`, restart, ready. Same
        // fail-soft policy.
        let docx_templates_dir = crate::presets::config_subdir("docx-templates");
        let docx_templates =
            crate::presets::docx_template::load_docx_templates(&docx_templates_dir)
                .unwrap_or_else(|e| {
                    tracing::warn!(
                        "[docx-templates] load failed from {}: {:#}",
                        docx_templates_dir.display(),
                        e
                    );
                    Vec::new()
                });
        tracing::info!(
            "[docx-templates] {} template(s) loaded from {}",
            docx_templates.len(),
            docx_templates_dir.display()
        );

        // LLM model catalogue. Same fail-soft policy: a missing/broken
        // `config/model.json` logs a warning and falls back to an empty
        // catalogue. The Settings → Modelli LLM page will render empty
        // dropdowns rather than fail to mount.
        let model_catalogue_path = crate::presets::model::catalogue_path();
        let model_catalogue = match crate::presets::model::load_catalogue(
            &model_catalogue_path,
        ) {
            Ok(c) => {
                tracing::info!(
                    "[model-catalogue] loaded {} provider(s) from {}",
                    c.providers.len(),
                    model_catalogue_path.display()
                );
                c
            }
            Err(e) => {
                tracing::warn!(
                    "[model-catalogue] load failed from {}: {:#}",
                    model_catalogue_path.display(),
                    e
                );
                crate::presets::model::ModelCatalogue::empty()
            }
        };

        Ok(Self {
            db,
            sessions,
            biometric_tx: None,
            no_tools_models: Arc::new(RwLock::new(HashSet::new())),
            mcp_discovery_cache: Arc::new(RwLock::new(HashMap::new())),
            #[cfg(feature = "rag")]
            embeddings,
            #[cfg(feature = "rag")]
            scans: Arc::new(RwLock::new(HashMap::new())),
            corpus_plugins,
            corpus_adapters,
            corpus_import_progress: Arc::new(RwLock::new(HashMap::new())),
            workflow_presets: Arc::new(workflow_presets),
            column_presets: Arc::new(column_presets),
            model_catalogue: Arc::new(model_catalogue),
            docx_templates: Arc::new(docx_templates),
        })
    }

    /// Invalidate any cached MCP discovery for this user. Called from
    /// the /user/mcp settings endpoints whenever the user adds, edits
    /// or removes a server, so the next chat re-runs discovery instead
    /// of using a stale (possibly broken) tool list.
    pub async fn invalidate_mcp_cache_for_user(&self, user_id: &str) {
        let mut g = self.mcp_discovery_cache.write().await;
        g.remove(user_id);
    }

    pub async fn run_migrations(&self) -> Result<()> {
        // First attempt: run migrations normally. The common case.
        let first = sqlx::migrate!("./migrations").run(&self.db).await;
        match first {
            Ok(()) => {}
            Err(e) => {
                // Checksum drift recovery. Sqlx refuses to start when a
                // migration file was edited after first apply (its
                // computed checksum no longer matches the `_sqlx_migrations`
                // row). Standard advice is "drop the tracking row + re-run",
                // but that needs an external sqlite3 binary and assumes the
                // dev knows the version number. We do it in-process instead:
                // ask sqlx itself for the expected checksum of every bundled
                // migration, UPDATE the tracking rows to match, then re-run.
                //
                // Safety: all migrations in this repo use
                // `CREATE ... IF NOT EXISTS`, so the second pass is a no-op
                // for schema and merely rewrites the checksum row. If a
                // future migration is destructive on re-apply, gate this
                // recovery behind a feature flag or an env var.
                //
                // We match on the error message because sqlx::migrate::MigrateError
                // wraps the version-mismatch case in a string; there is no
                // structured variant we can pattern-match against in 0.8.
                let msg = e.to_string();
                if !msg.contains("was previously applied but has been modified") {
                    return Err(e.into());
                }
                tracing::warn!(
                    "[migrations] checksum drift detected — auto-healing: {msg}"
                );
                self.heal_migration_checksums().await?;
                sqlx::migrate!("./migrations").run(&self.db).await?;
                tracing::info!("[migrations] checksum drift healed; resume normal startup");
            }
        }
        self.sessions.purge_expired().await?;
        Ok(())
    }

    /// Rewrite every `_sqlx_migrations` row's `checksum` column to match
    /// the checksum sqlx computes for the bundled migration file on disk.
    /// Used by `run_migrations` to recover from checksum drift without
    /// requiring an external sqlite3 binary.
    async fn heal_migration_checksums(&self) -> Result<()> {
        let migrator = sqlx::migrate!("./migrations");
        for migration in migrator.iter() {
            let res = sqlx::query(
                "UPDATE _sqlx_migrations SET checksum = ? WHERE version = ?",
            )
            .bind(migration.checksum.as_ref())
            .bind(migration.version as i64)
            .execute(&self.db)
            .await?;
            if res.rows_affected() > 0 {
                tracing::info!(
                    "[migrations] checksum rewritten for version {} ({})",
                    migration.version,
                    migration.description,
                );
            }
        }
        Ok(())
    }
}
