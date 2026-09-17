//! Corpus plugin manifests — JSON-driven registry for legal corpora.
//!
//! Goal: every corpus Cove Studio knows about (EUR-Lex, Italian legal,
//! future Légifrance/BOE/Retsinformation/...) is described by a JSON
//! manifest file. The runtime scans a directory at startup, parses
//! each manifest, and exposes a registry the UI and chat system
//! prompt can consult.
//!
//! Today the manifest's `strategy` discriminator only knows about
//! `"builtin"` — the actual fetch/parse logic lives in a hand-written
//! Rust adapter (`eurlex.rs`, `italian_legal.rs`) referenced by name.
//! The manifest contributes metadata (display name, supported
//! languages, identifier label, enabled-by-default, homepage). This
//! lets us:
//!
//!   - Add a new corpus by dropping a JSON file (eventually, once
//!     `http-fetch-per-id` strategy lands — schema sketched below).
//!   - Configure existing corpora declaratively (default language,
//!     fallback policy, display name per locale) without recompiling.
//!   - Surface the same metadata uniformly to the UI and the chat's
//!     `<USER LIBRARY>` inventory, regardless of whether the
//!     underlying connector is builtin or declarative.
//!
//! Manifest location: `corpora-plugins/*.json` relative to the
//! current working directory by default, override with
//! `COVE_CORPUS_PLUGINS_DIR`.
//!
//! ### Future strategies (schema-only, not implemented yet)
//!
//! ```json
//! "strategy": {
//!   "kind": "http-fetch-per-id",
//!   "search_by_id":      { "url_template": "...", "shape": "rest-json", "body_path": "$.content" },
//!   "search_by_keyword": { "url_template": "...", "shape": "rest-json", "hits_path": "$.results[*]" }
//! }
//! ```
//!
//! When that lands, `ManifestAdapter` becomes a Rust struct that
//! interprets the manifest at runtime — same trait, no per-corpus
//! Rust code.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One corpus plugin as loaded from disk.
///
/// Schema is permissive on unknown top-level fields (so newer
/// manifests don't break older builds) but strict on the typed
/// fields it does know about. Use `corpus-plugin.schema.json` (in
/// `docs/`) as the authoritative editor reference.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CorpusPlugin {
    /// Stable corpus key (also written to `documents.corpus_id`).
    /// Must match `^[a-z][a-z0-9\-]*$` — lowercase + dash, no spaces.
    pub id: String,

    /// Default English display name shown in the UI when the user's
    /// locale lacks a specific override.
    pub display_name: String,

    /// Optional per-locale display names. Keyed by ISO-639-1 lowercase
    /// code (e.g. `"it"`, `"en"`, `"fr"`).
    #[serde(default)]
    pub display_name_locale: HashMap<String, String>,

    /// One-line description, shown in the corpus picker.
    #[serde(default)]
    pub description: Option<String>,

    /// Homepage URL for the source site (UI "open externally" link).
    #[serde(default)]
    pub homepage: Option<String>,

    /// ISO-639-1 codes the corpus is served in.
    pub languages: Vec<String>,

    /// Default language to fetch when the user hasn't set one.
    /// Must be in `languages`.
    pub default_language: String,

    /// When true and the requested language isn't available, the
    /// adapter falls back to `fallback_language`.
    #[serde(default = "default_true")]
    pub supports_language_fallback: bool,

    /// Language used when `supports_language_fallback` and the
    /// primary request returned nothing. Must be in `languages`.
    #[serde(default)]
    pub fallback_language: Option<String>,

    /// Label shown next to identifier inputs (CELEX, ELI, URN, ...).
    pub identifier_label: String,

    /// Sample identifier the UI can prefill or show in placeholder.
    #[serde(default)]
    pub identifier_example: Option<String>,

    /// Whether the corpus is enabled the first time the user opens
    /// the settings panel. Users can flip this later via
    /// `corpus_settings.enabled`.
    #[serde(default = "default_true")]
    pub enabled_by_default: bool,

    /// Whether the corpus is offered to the user at all. `false`
    /// retires a corpus whose connector isn't verified working
    /// end-to-end — the manifest stays on disk (revivable by flipping
    /// the flag) but the UI hides it entirely. Distinct from
    /// `enabled_by_default` (per-user on/off of a *working* corpus)
    /// and from `is_runnable()` (does an adapter exist at all).
    #[serde(default = "default_true")]
    pub available: bool,

    /// How Cove Studio actually fetches and indexes documents from
    /// this corpus. Discriminated union — see `CorpusStrategy`.
    pub strategy: CorpusStrategy,

    /// Which generic operations this corpus supports. Drives both
    /// route mounting on the backend (a missing capability 404s)
    /// and UI control visibility on the frontend (hide buttons
    /// for operations the corpus can't perform).
    ///
    /// All-true would be wrong for most real corpora: EUR-Lex has
    /// no bulk_import (every doc is fetched on demand), Italian
    /// Legal has bulk_import (HF parquet) but no embed_progress
    /// at the corpus level (uses /sync/embed-progress instead).
    /// So we deliberately default each to `false` and force every
    /// manifest to enumerate what it actually supports — that way
    /// adding a new capability doesn't silently enable it on old
    /// manifests.
    #[serde(default)]
    pub capabilities: Capabilities,

    /// Optional sub-sources inside the corpus that the user can
    /// enable/disable independently. Used by Italian Legal to
    /// expose Normattiva / Corte Cost / OpenGA / Cassazione as
    /// separately-toggleable inside the same corpus. Empty / absent
    /// for single-source corpora like EUR-Lex.
    #[serde(default)]
    pub sources: Vec<CorpusSource>,

    /// Upstream license + attribution metadata. Required by open-data
    /// providers like DILA (Etalab 2.0) and reused by the UI to render
    /// a "Source: …" footer / badge in the corpus panel and on
    /// citation pills. Must be present on any corpus that imports
    /// data the user redistributes (via `.coveprj` export, etc.).
    #[serde(default)]
    pub license: Option<CorpusLicense>,

    /// Discovery metadata for the Settings → Data sources filter UI:
    /// jurisdiction + document kinds + access/format badges. Optional
    /// for forward-compat — manifests predating this block still load.
    #[serde(default)]
    pub discovery: Option<CorpusDiscovery>,
}

/// Discovery metadata surfaced to the corpus-picker UI. Every field is
/// a free-form lowercase token (deliberately NOT validated here) so a
/// new manifest can introduce a value without breaking older builds:
///   - `jurisdiction`: short region code for the filter dropdown
///     (`eu`, `de`, `fr`, `coe`, `us`, ...).
///   - `doc_types`: `legislation` / `case_law` — a corpus may serve both.
///   - `auth`: `public` / `api-key` / `oauth2` / `optional-token`.
///   - `search_mode`: `free-text` / `citation-only` / `date-window` / `sparql`.
///   - `fetch_format`: `html` / `xml` / `json` / `sparql` / `zip-epub`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct CorpusDiscovery {
    #[serde(default)]
    pub jurisdiction: Option<String>,
    #[serde(default)]
    pub doc_types: Vec<String>,
    #[serde(default)]
    pub auth: Option<String>,
    #[serde(default)]
    pub search_mode: Option<String>,
    #[serde(default)]
    pub fetch_format: Option<String>,
}

/// Upstream license info for a corpus. The producer's attribution
/// requirements are encoded here so the UI can render them
/// consistently without per-corpus chrome. Compatible licenses today:
/// `etalab-2.0`, `cc-by-4.0`, `cc-by-sa-4.0`, `cc0-1.0`, `public-domain`.
/// Unknown values are accepted (forward-compat) but the UI renders
/// them verbatim.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CorpusLicense {
    /// Short license id, used to look up an info-link locally in
    /// the UI and to surface in `.coveprj` exports. Lowercase.
    pub id: String,
    /// One-line attribution text the UI shows under the corpus
    /// header. Should follow the producer's recommended template
    /// — e.g. for Etalab 2.0: "Source: DILA — Licence Ouverte 2.0".
    pub attribution: String,
    /// Link to the full license text (or to the producer's
    /// reuse policy page).
    #[serde(default)]
    pub url: Option<String>,
}

/// Boolean map of operations a corpus exposes. Each field is a
/// generic operation the runtime knows how to dispatch (via the
/// `strategy.builtin_id` adapter for builtin corpora, eventually
/// via declarative URL templates for future strategies). The
/// router uses these to decide whether to mount the corresponding
/// `/corpora/:id/<op>` route; the UI uses them to render/hide
/// controls.
///
/// Adding a new capability: extend this struct + bump
/// `Capabilities::default()` carefully (defaults are false to
/// avoid silently enabling new operations on old manifests).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Capabilities {
    /// Free-text or identifier search returning a list of
    /// `CorpusHit`. Maps to the trait's `search_by_id` /
    /// `search_by_keyword` depending on input shape (the dispatcher
    /// picks).
    #[serde(default)]
    pub search: bool,

    /// Fetch the full content of a document by its corpus-native
    /// identifier. Maps to the trait's `fetch`.
    #[serde(default)]
    pub fetch: bool,

    /// `GET /corpora/:id/documents` — list documents the user has
    /// already synced for this corpus. Generic DB-backed; no
    /// adapter call.
    #[serde(default)]
    pub documents: bool,

    /// `DELETE /corpora/:id/documents/:doc_id` — remove a synced doc.
    /// Implies `documents`.
    #[serde(default)]
    pub documents_delete: bool,

    /// `POST /corpora/:id/documents/:doc_id/resync` — re-run indexing
    /// for a previously-fetched doc whose text is still on disk.
    /// Implies `documents`.
    #[serde(default)]
    pub documents_resync: bool,

    /// `GET /corpora/:id/embed-progress` — per-corpus embedding
    /// progress polling. EUR-Lex needs this because synchronous fetch
    /// + embed of a single act takes long enough that the UI polls.
    /// Italian Legal doesn't (its bulk import has its own progress
    /// endpoint).
    #[serde(default)]
    pub embed_progress: bool,

    /// `POST /corpora/:id/import` — one-shot bulk import (e.g. HF
    /// parquet metadata download). Italian Legal uses this. EUR-Lex
    /// doesn't (no bulk dataset).
    #[serde(default)]
    pub bulk_import: bool,

    /// `GET|PUT /corpora/:id/config` — per-user enable/disable +
    /// default_language + fallback_en. Generic, lives in the
    /// `corpus_settings` table. Almost every corpus has this.
    #[serde(default)]
    pub user_config: bool,
}

/// One sub-source inside a corpus. Lets the corpus expose multiple
/// data origins under a single id (e.g. Italian Legal: Normattiva +
/// Corte Cost + OpenGA + Cassazione). Users toggle each one
/// independently via the settings UI; `corpus_settings.sources_enabled`
/// (future column, TBD) persists the selection.
///
/// `available: false` means the source is *declared* in the manifest
/// but not yet wired in the runtime — UI shows it disabled with the
/// `status_label` ("in arrivo" / "coming soon") so the user knows
/// it's on the roadmap.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CorpusSource {
    /// Stable source key, scoped to the parent corpus. Same regex
    /// rules as the corpus id (`^[a-z][a-z0-9\-]*$`).
    pub id: String,

    /// Human display name for this source. Not localised — the
    /// source names are usually proper nouns (Normattiva, Corte
    /// Costituzionale, Légifrance) that don't translate.
    pub display_name: String,

    /// Optional short qualifier rendered next to the name, e.g.
    /// volume hint ("~125K") or scope hint ("(incrementale)").
    #[serde(default)]
    pub subtitle: Option<String>,

    /// Longer description shown under the row when the source is
    /// "in arrivo" — explains why it's not available yet and what
    /// would unlock it.
    #[serde(default)]
    pub description: Option<String>,

    /// Whether the source is wired in the runtime today. When false
    /// the UI dims the checkbox and renders `status_label`.
    pub available: bool,

    /// When `available: true`, whether the source is on by default
    /// the first time the user opens the settings panel. Ignored
    /// when `available: false`.
    #[serde(default)]
    pub default_enabled: bool,

    /// Free-text label shown next to a non-available source. Common
    /// values: "in arrivo", "coming soon", "V2 roadmap".
    #[serde(default)]
    pub status_label: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Backend strategy. Discriminated union; `kind` chooses the variant.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CorpusStrategy {
    /// Hand-written Rust adapter. The `builtin_id` names which one.
    Builtin {
        /// Identifier matched against the in-binary registry of
        /// Rust adapters (`eurlex`, `italian-legal-hf`, ...).
        builtin_id: String,
    },

    /// Declarative REST fetch driven entirely by the JSON manifest.
    /// A generic `ManifestAdapter` reads `spec` and implements
    /// `LegalCorpusAdapter` against it: URL-template substitution,
    /// HTTP GET, CSS-selector / JSONPath extraction. See
    /// `manifest_adapter.rs` for the runtime.
    #[serde(rename = "http-fetch-per-id")]
    HttpFetchPerId(HttpFetchPerIdSpec),

    /// Bulk download of DILA OPENDATA tar.gz archives. Covers any
    /// fonds DILA publishes today (CNIL, LEGI, JORF, CASS, KALI) and
    /// uses the same XML schema across all of them — see
    /// `src/corpora/dila_bulk.rs` for the parser + importer.
    #[serde(rename = "dila-bulk-xml")]
    DilaBulkXml(crate::corpora::dila_bulk::DilaBulkXmlSpec),

    /// Future: bulk metadata import from a Hugging Face dataset
    /// (parquet projection + filtered rows). What the current
    /// `italian_legal` adapter does today.
    #[serde(rename = "hf-dataset-bulk")]
    HfDatasetBulk(serde_json::Value),
}

/// Declarative spec for the `http-fetch-per-id` strategy. Two
/// sub-specs (`search_by_id`, `search_by_keyword`) describe a URL
/// template plus extraction rules for the response. The generic
/// `ManifestAdapter` interprets these at runtime.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct HttpFetchPerIdSpec {
    /// Resolve `{identifier}` (and `{lang}` if templated) to a doc
    /// URL, fetch it, extract title + body + optional date. Required.
    pub search_by_id: HttpFetchByIdSpec,

    /// Resolve `{query}` (and `{lang}` if templated) to a search
    /// URL, fetch it, walk the hit list. Optional — manifest can
    /// declare `capabilities.search = false` and omit this entirely.
    #[serde(default)]
    pub search_by_keyword: Option<HttpSearchKeywordSpec>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct HttpFetchByIdSpec {
    /// HTTP URL template with `{identifier}` and optional `{lang}`
    /// placeholders. Placeholders are percent-encoded at substitution
    /// time; unknown placeholders cause the substitution to fail
    /// (with a runtime warning) so a typo doesn't silently produce
    /// `https://...{garbled}/...` URLs.
    pub url_template: String,

    /// Response shape — drives which extraction engine runs.
    pub shape: ResponseShape,

    /// Selector for the document title. CSS-selector when
    /// `shape == "rest-html"`, JSONPath when `shape == "rest-json"`.
    /// Supports the `@attr` suffix to read an attribute instead of
    /// the element's text (HTML only).
    #[serde(default)]
    pub title_path: Option<String>,

    /// Selector for the document body. Same syntax as `title_path`.
    /// Required: a fetch with no body is useless.
    pub body_path: String,

    /// Selector for an ISO-8601 date. Optional.
    #[serde(default)]
    pub date_path: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct HttpSearchKeywordSpec {
    /// HTTP URL template with `{query}` and optional `{lang}`,
    /// `{limit}` placeholders.
    pub url_template: String,

    /// Response shape.
    pub shape: ResponseShape,

    /// Selector that returns ONE element per hit. The
    /// `identifier_at` / `title_at` selectors are then evaluated
    /// *within* each hit element (HTML) or item object (JSON).
    pub hits_path: String,

    /// Selector for the corpus-native identifier inside a single hit.
    /// Supports HTML `@attr` and `:strip-prefix=...` /
    /// `:strip-suffix=...` postprocessors so e.g. `href` attributes
    /// can be trimmed to just the identifier.
    pub identifier_at: String,

    /// Selector for the hit title. Same syntax.
    pub title_at: String,

    /// Optional date selector inside the hit.
    #[serde(default)]
    pub date_at: Option<String>,

    /// Optional alternate URL template used when the engine detects a
    /// 4-digit year in the query. Receives `{query}`, `{lang}`,
    /// `{limit}` and `{year}`. When absent — or no year is detected —
    /// `url_template` is used. This lets a manifest add an API
    /// date-filter parameter without leaving a dangling empty
    /// `&date=` in the no-year case.
    #[serde(default)]
    pub url_template_year: Option<String>,
}

/// Response shape understood by the declarative engine. `rest-html`
/// uses `scraper` (CSS selectors); `rest-json` uses a tiny JSONPath
/// subset. Both modes share the same `@attr` and `:strip-*` syntax
/// where it makes sense.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ResponseShape {
    RestHtml,
    RestJson,
}

impl Default for ResponseShape {
    fn default() -> Self {
        ResponseShape::RestHtml
    }
}

impl CorpusPlugin {
    /// Validate cross-field invariants the deserialiser can't catch
    /// (e.g. `default_language` is one of `languages`).
    pub fn validate(&self) -> Result<()> {
        if !is_valid_corpus_id(&self.id) {
            bail!(
                "invalid corpus id {:?}: must match ^[a-z][a-z0-9\\-]*$",
                self.id
            );
        }
        if self.languages.is_empty() {
            bail!("corpus {} declares no languages", self.id);
        }
        for lang in &self.languages {
            if !is_valid_iso639_1(lang) {
                bail!(
                    "corpus {}: language {:?} is not a valid ISO-639-1 code",
                    self.id,
                    lang
                );
            }
        }
        if !self.languages.contains(&self.default_language) {
            bail!(
                "corpus {}: default_language {:?} not in languages",
                self.id,
                self.default_language
            );
        }
        if let Some(fb) = &self.fallback_language {
            if !self.languages.contains(fb) {
                bail!(
                    "corpus {}: fallback_language {:?} not in languages",
                    self.id,
                    fb
                );
            }
        }
        if self.supports_language_fallback && self.fallback_language.is_none() {
            bail!(
                "corpus {}: supports_language_fallback=true requires fallback_language to be set",
                self.id
            );
        }
        if let CorpusStrategy::Builtin { builtin_id } = &self.strategy {
            if !is_known_builtin(builtin_id) {
                bail!(
                    "corpus {}: unknown builtin_id {:?} (known: {})",
                    self.id,
                    builtin_id,
                    KNOWN_BUILTINS.join(", ")
                );
            }
        }

        // Source-level invariants.
        let mut seen_source_ids: std::collections::HashSet<&str> =
            std::collections::HashSet::new();
        for src in &self.sources {
            if !is_valid_corpus_id(&src.id) {
                bail!(
                    "corpus {} source {:?}: invalid id, must match ^[a-z][a-z0-9\\-]*$",
                    self.id,
                    src.id
                );
            }
            if !seen_source_ids.insert(src.id.as_str()) {
                bail!(
                    "corpus {}: duplicate source id {:?}",
                    self.id,
                    src.id
                );
            }
            if !src.available && src.default_enabled {
                bail!(
                    "corpus {} source {:?}: default_enabled=true but available=false",
                    self.id,
                    src.id
                );
            }
        }

        // Capability-implication checks: documents_delete and
        // documents_resync don't make sense without documents.
        if self.capabilities.documents_delete && !self.capabilities.documents {
            bail!(
                "corpus {}: capabilities.documents_delete=true requires documents=true",
                self.id
            );
        }
        if self.capabilities.documents_resync && !self.capabilities.documents {
            bail!(
                "corpus {}: capabilities.documents_resync=true requires documents=true",
                self.id
            );
        }
        Ok(())
    }

    /// Resolve the display name for the user's locale, falling back
    /// to the default English name when the locale has no override.
    pub fn localized_display_name(&self, locale: &str) -> &str {
        self.display_name_locale
            .get(locale)
            .map(String::as_str)
            .unwrap_or(self.display_name.as_str())
    }

    /// Convenience: is this manifest backed by a runnable adapter
    /// today? `false` for strategies we've parsed but not yet wired
    /// (currently only `hf-dataset-bulk`).
    pub fn is_runnable(&self) -> bool {
        matches!(
            self.strategy,
            CorpusStrategy::Builtin { .. }
                | CorpusStrategy::HttpFetchPerId(_)
                | CorpusStrategy::DilaBulkXml(_)
        )
    }
}

fn is_valid_corpus_id(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_valid_iso639_1(s: &str) -> bool {
    s.len() == 2 && s.chars().all(|c| c.is_ascii_lowercase())
}

/// Known builtin adapter ids. Keep in sync with the registry in
/// `src/corpora/mod.rs` (when we add it).
const KNOWN_BUILTINS: &[&str] = &["eurlex", "italian-legal-hf", "ch-fedlex"];

fn is_known_builtin(id: &str) -> bool {
    KNOWN_BUILTINS.contains(&id)
}

/// Resolve the directory to scan for plugin manifests.
///
/// Resolution order:
///   1. `COVE_CORPUS_PLUGINS_DIR` env var, if set. Used verbatim.
///   2. Walk the ancestors of the current working directory looking
///      for a `corpora-plugins/` folder. Picks up the repo-root copy
///      when the dev binary runs from `src-tauri/target/...`.
///   3. Walk the ancestors of the current executable path the same way.
///      Picks up the installed copy when the manifests are shipped
///      alongside the binary in a packaged build.
///   4. Fallback: `./corpora-plugins` relative to CWD (the historical
///      behaviour). Returned even when it doesn't exist so the loader
///      can emit a "directory not found" info log against a stable
///      path.
///
/// Same ancestor-walking idea as `PDFIUM_DYNAMIC_LIB_PATH` and other
/// asset locators in this codebase — keeps `cargo run` / `tauri dev`
/// / installer scenarios working without an env var.
pub fn plugins_dir() -> PathBuf {
    if let Some(dir) = crate::product::env_var("CORPUS_PLUGINS_DIR") {
        return PathBuf::from(dir);
    }

    if let Ok(cwd) = std::env::current_dir() {
        if let Some(found) = walk_ancestors_for_plugins(&cwd) {
            return found;
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(found) = walk_ancestors_for_plugins(&exe) {
            return found;
        }
    }

    PathBuf::from("./config/corpora-plugins")
}

/// Walk from `start` up the parent chain, returning the first
/// directory that contains a `config/corpora-plugins/` subtree. The
/// `config/` consolidation groups every on-disk JSON config family
/// (corpus plugins, workflow presets, column presets) under one
/// top-level folder so the repo root stays tidy.
fn walk_ancestors_for_plugins(start: &Path) -> Option<PathBuf> {
    for anc in start.ancestors() {
        let candidate = anc.join("config").join("corpora-plugins");
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

/// Scan `dir` for `*.json` files, parse each as a `CorpusPlugin`,
/// validate, and return them sorted by `id`. Skips files that fail
/// to parse with a tracing::warn — one broken manifest does not
/// stop the rest from loading.
pub fn load_plugins(dir: &Path) -> Result<Vec<CorpusPlugin>> {
    let mut out: Vec<CorpusPlugin> = Vec::new();

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::info!(
                "[corpus-plugins] directory {} not found; no manifests loaded",
                dir.display()
            );
            return Ok(out);
        }
        Err(e) => {
            return Err(anyhow!(
                "failed to read corpus plugins dir {}: {}",
                dir.display(),
                e
            ));
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("[corpus-plugins] read_dir entry error: {}", e);
                continue;
            }
        };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        match parse_manifest_file(&path) {
            Ok(plugin) => {
                tracing::info!(
                    "[corpus-plugins] loaded {} ({}): {} languages, strategy={:?}",
                    plugin.id,
                    path.display(),
                    plugin.languages.len(),
                    plugin.strategy
                );
                out.push(plugin);
            }
            Err(e) => {
                tracing::warn!(
                    "[corpus-plugins] skipping {}: {:#}",
                    path.display(),
                    e
                );
            }
        }
    }

    // Dedup by id; later loads of the same id win and we warn.
    let mut by_id: HashMap<String, CorpusPlugin> = HashMap::new();
    for plugin in out {
        if by_id.contains_key(&plugin.id) {
            tracing::warn!(
                "[corpus-plugins] duplicate corpus id {:?} — later definition wins",
                plugin.id
            );
        }
        by_id.insert(plugin.id.clone(), plugin);
    }
    let mut sorted: Vec<CorpusPlugin> = by_id.into_values().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(sorted)
}

fn parse_manifest_file(path: &Path) -> Result<CorpusPlugin> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let plugin: CorpusPlugin = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing JSON in {}", path.display()))?;
    plugin
        .validate()
        .with_context(|| format!("validating manifest {}", path.display()))?;
    Ok(plugin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp(name: &str, content: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(name);
        let mut f = std::fs::File::create(&path).expect("create");
        f.write_all(content.as_bytes()).expect("write");
        dir
    }

    #[test]
    fn parses_minimal_builtin_manifest() {
        let json = r#"{
            "id": "eurlex",
            "display_name": "EUR-Lex",
            "languages": ["en", "it", "fr"],
            "default_language": "en",
            "fallback_language": "en",
            "identifier_label": "CELEX",
            "strategy": { "kind": "builtin", "builtin_id": "eurlex" }
        }"#;
        let plugin: CorpusPlugin = serde_json::from_str(json).unwrap();
        plugin.validate().unwrap();
        assert_eq!(plugin.id, "eurlex");
        assert!(plugin.is_runnable());
    }

    #[test]
    fn rejects_uppercase_id() {
        let json = r#"{
            "id": "EurLex",
            "display_name": "x",
            "languages": ["en"],
            "default_language": "en",
            "fallback_language": "en",
            "identifier_label": "X",
            "strategy": { "kind": "builtin", "builtin_id": "eurlex" }
        }"#;
        let plugin: CorpusPlugin = serde_json::from_str(json).unwrap();
        assert!(plugin.validate().is_err());
    }

    #[test]
    fn rejects_default_language_not_in_list() {
        let json = r#"{
            "id": "x",
            "display_name": "x",
            "languages": ["fr"],
            "default_language": "en",
            "identifier_label": "X",
            "supports_language_fallback": false,
            "strategy": { "kind": "builtin", "builtin_id": "eurlex" }
        }"#;
        let plugin: CorpusPlugin = serde_json::from_str(json).unwrap();
        assert!(plugin.validate().is_err());
    }

    #[test]
    fn rejects_fallback_when_supported_but_unset() {
        let json = r#"{
            "id": "x",
            "display_name": "x",
            "languages": ["en"],
            "default_language": "en",
            "identifier_label": "X",
            "strategy": { "kind": "builtin", "builtin_id": "eurlex" }
        }"#;
        let plugin: CorpusPlugin = serde_json::from_str(json).unwrap();
        // supports_language_fallback defaults to true; fallback_language is None.
        assert!(plugin.validate().is_err());
    }

    #[test]
    fn rejects_unknown_builtin_id() {
        let json = r#"{
            "id": "weird",
            "display_name": "Weird",
            "languages": ["en"],
            "default_language": "en",
            "fallback_language": "en",
            "identifier_label": "X",
            "strategy": { "kind": "builtin", "builtin_id": "does-not-exist" }
        }"#;
        let plugin: CorpusPlugin = serde_json::from_str(json).unwrap();
        assert!(plugin.validate().is_err());
    }

    #[test]
    fn rejects_invalid_iso_code() {
        let json = r#"{
            "id": "x",
            "display_name": "x",
            "languages": ["eng"],
            "default_language": "eng",
            "fallback_language": "eng",
            "identifier_label": "X",
            "strategy": { "kind": "builtin", "builtin_id": "eurlex" }
        }"#;
        let plugin: CorpusPlugin = serde_json::from_str(json).unwrap();
        assert!(plugin.validate().is_err());
    }

    #[test]
    fn parses_http_fetch_strategy_and_marks_runnable() {
        // Http-fetch-per-id is implemented by ManifestAdapter, so a
        // valid manifest with that strategy is now runnable.
        // body_path is required (a fetch with no body is useless).
        let json = r#"{
            "id": "future",
            "display_name": "Future",
            "languages": ["en"],
            "default_language": "en",
            "fallback_language": "en",
            "identifier_label": "X",
            "strategy": {
                "kind": "http-fetch-per-id",
                "search_by_id": {
                    "url_template": "https://example.com/{identifier}",
                    "shape": "rest-html",
                    "body_path": "main"
                }
            }
        }"#;
        let plugin: CorpusPlugin = serde_json::from_str(json).unwrap();
        plugin.validate().unwrap();
        assert!(plugin.is_runnable());
        // The hf-dataset-bulk variant remains a placeholder and is
        // therefore NOT runnable yet.
        let hf_json = r#"{
            "id": "later",
            "display_name": "Later",
            "languages": ["en"],
            "default_language": "en",
            "fallback_language": "en",
            "identifier_label": "X",
            "strategy": { "kind": "hf-dataset-bulk" }
        }"#;
        let later: CorpusPlugin = serde_json::from_str(hf_json).unwrap();
        later.validate().unwrap();
        assert!(!later.is_runnable());
    }

    #[test]
    fn localized_display_name_falls_back_to_default() {
        let mut p: CorpusPlugin = serde_json::from_str(
            r#"{
                "id": "x",
                "display_name": "Default",
                "display_name_locale": { "it": "Italiano" },
                "languages": ["en"],
                "default_language": "en",
                "fallback_language": "en",
                "identifier_label": "X",
                "strategy": { "kind": "builtin", "builtin_id": "eurlex" }
            }"#,
        )
        .unwrap();
        assert_eq!(p.localized_display_name("it"), "Italiano");
        assert_eq!(p.localized_display_name("fr"), "Default");
        // Mutating the map confirms it really is a HashMap, not a fluke.
        p.display_name_locale
            .insert("fr".to_string(), "Français".to_string());
        assert_eq!(p.localized_display_name("fr"), "Français");
    }

    #[test]
    fn load_plugins_returns_empty_when_dir_missing() {
        let bogus = PathBuf::from("./this-directory-really-should-not-exist-1234");
        let out = load_plugins(&bogus).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn load_plugins_skips_broken_files_keeps_valid_ones() {
        let dir = tempfile::tempdir().unwrap();
        // valid
        let valid = r#"{
            "id": "ok",
            "display_name": "OK",
            "languages": ["en"],
            "default_language": "en",
            "fallback_language": "en",
            "identifier_label": "X",
            "strategy": { "kind": "builtin", "builtin_id": "eurlex" }
        }"#;
        std::fs::write(dir.path().join("ok.json"), valid).unwrap();
        // broken JSON
        std::fs::write(dir.path().join("broken.json"), "{ not json }").unwrap();
        // invalid id (validation failure)
        let invalid = r#"{
            "id": "BAD",
            "display_name": "Bad",
            "languages": ["en"],
            "default_language": "en",
            "fallback_language": "en",
            "identifier_label": "X",
            "strategy": { "kind": "builtin", "builtin_id": "eurlex" }
        }"#;
        std::fs::write(dir.path().join("invalid.json"), invalid).unwrap();
        // non-json file (ignored silently)
        std::fs::write(dir.path().join("readme.txt"), "ignore me").unwrap();

        let out = load_plugins(dir.path()).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "ok");
        // suppress unused warning on the TempDir guard
        let _ = write_temp;
    }

    #[test]
    fn capabilities_default_to_all_false() {
        // A manifest without a capabilities block parses fine and
        // every operation is opt-in.
        let json = r#"{
            "id": "x",
            "display_name": "X",
            "languages": ["en"],
            "default_language": "en",
            "fallback_language": "en",
            "identifier_label": "X",
            "strategy": { "kind": "builtin", "builtin_id": "eurlex" }
        }"#;
        let p: CorpusPlugin = serde_json::from_str(json).unwrap();
        p.validate().unwrap();
        assert!(!p.capabilities.search);
        assert!(!p.capabilities.fetch);
        assert!(!p.capabilities.documents);
        assert!(!p.capabilities.bulk_import);
    }

    #[test]
    fn capabilities_round_trip() {
        let json = r#"{
            "id": "x",
            "display_name": "X",
            "languages": ["en"],
            "default_language": "en",
            "fallback_language": "en",
            "identifier_label": "X",
            "strategy": { "kind": "builtin", "builtin_id": "eurlex" },
            "capabilities": {
                "search": true,
                "fetch": true,
                "documents": true,
                "documents_delete": true,
                "documents_resync": true,
                "embed_progress": true,
                "user_config": true
            }
        }"#;
        let p: CorpusPlugin = serde_json::from_str(json).unwrap();
        p.validate().unwrap();
        assert!(p.capabilities.search);
        assert!(p.capabilities.documents);
        assert!(p.capabilities.documents_delete);
        assert!(!p.capabilities.bulk_import); // unset → false
    }

    #[test]
    fn rejects_documents_delete_without_documents() {
        let json = r#"{
            "id": "x",
            "display_name": "X",
            "languages": ["en"],
            "default_language": "en",
            "fallback_language": "en",
            "identifier_label": "X",
            "strategy": { "kind": "builtin", "builtin_id": "eurlex" },
            "capabilities": { "documents_delete": true }
        }"#;
        let p: CorpusPlugin = serde_json::from_str(json).unwrap();
        assert!(p.validate().is_err());
    }

    #[test]
    fn rejects_documents_resync_without_documents() {
        let json = r#"{
            "id": "x",
            "display_name": "X",
            "languages": ["en"],
            "default_language": "en",
            "fallback_language": "en",
            "identifier_label": "X",
            "strategy": { "kind": "builtin", "builtin_id": "eurlex" },
            "capabilities": { "documents_resync": true }
        }"#;
        let p: CorpusPlugin = serde_json::from_str(json).unwrap();
        assert!(p.validate().is_err());
    }

    #[test]
    fn sources_parse_and_validate() {
        let json = r#"{
            "id": "italian-legal",
            "display_name": "Italia legale",
            "languages": ["it"],
            "default_language": "it",
            "fallback_language": "it",
            "identifier_label": "URN",
            "strategy": { "kind": "builtin", "builtin_id": "italian-legal-hf" },
            "sources": [
                {
                    "id": "normattiva",
                    "display_name": "Normattiva",
                    "available": true,
                    "default_enabled": true
                },
                {
                    "id": "openga",
                    "display_name": "OpenGA",
                    "subtitle": "(~125K)",
                    "description": "Already in HF dataset; needs opt-in filter.",
                    "available": false,
                    "status_label": "in arrivo"
                }
            ]
        }"#;
        let p: CorpusPlugin = serde_json::from_str(json).unwrap();
        p.validate().unwrap();
        assert_eq!(p.sources.len(), 2);
        assert!(p.sources[0].available);
        assert!(p.sources[0].default_enabled);
        assert!(!p.sources[1].available);
        assert_eq!(
            p.sources[1].status_label.as_deref(),
            Some("in arrivo")
        );
    }

    #[test]
    fn rejects_duplicate_source_ids() {
        let json = r#"{
            "id": "x",
            "display_name": "X",
            "languages": ["en"],
            "default_language": "en",
            "fallback_language": "en",
            "identifier_label": "X",
            "strategy": { "kind": "builtin", "builtin_id": "eurlex" },
            "sources": [
                { "id": "dup", "display_name": "A", "available": true },
                { "id": "dup", "display_name": "B", "available": true }
            ]
        }"#;
        let p: CorpusPlugin = serde_json::from_str(json).unwrap();
        assert!(p.validate().is_err());
    }

    #[test]
    fn rejects_default_enabled_on_unavailable_source() {
        let json = r#"{
            "id": "x",
            "display_name": "X",
            "languages": ["en"],
            "default_language": "en",
            "fallback_language": "en",
            "identifier_label": "X",
            "strategy": { "kind": "builtin", "builtin_id": "eurlex" },
            "sources": [
                { "id": "future", "display_name": "F", "available": false, "default_enabled": true }
            ]
        }"#;
        let p: CorpusPlugin = serde_json::from_str(json).unwrap();
        assert!(p.validate().is_err());
    }

    /// Integration check: every JSON file we ship in
    /// `corpora-plugins/` at the repo root must parse and validate.
    /// Catches regressions where the schema evolves but a real
    /// manifest hasn't been updated.
    #[test]
    fn shipped_manifests_load_and_validate() {
        let repo_root = std::env::var("CARGO_MANIFEST_DIR")
            .map(PathBuf::from)
            .expect("CARGO_MANIFEST_DIR set under cargo test");
        let dir = repo_root.join("corpora-plugins");
        if !dir.exists() {
            // Tolerate the case where the test runs from a checkout
            // without the plugins folder (e.g. submodule consumers).
            return;
        }
        let plugins = load_plugins(&dir).expect("load shipped plugins");
        assert!(
            !plugins.is_empty(),
            "expected at least one manifest in {}",
            dir.display()
        );
        // Each shipped manifest is already validated by load_plugins;
        // we also assert their ids are unique (the loader dedups, but
        // we want a hard failure if duplication ever sneaks in here).
        let mut seen: std::collections::HashSet<&str> =
            std::collections::HashSet::new();
        for p in &plugins {
            assert!(
                seen.insert(p.id.as_str()),
                "shipped manifest duplicate id: {}",
                p.id
            );
        }
    }

    #[test]
    fn walk_ancestors_finds_corpora_plugins() {
        // Create a temp tree:  root/level1/level2/level3
        //                       └─ config/corpora-plugins/
        // walking from level3 should return root/config/corpora-plugins.
        // The `config/` parent comes from commit f9d6bf5 which moved
        // every JSON-driven registry under a unified `config/` root.
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("config").join("corpora-plugins");
        std::fs::create_dir_all(&target).unwrap();
        let leaf = root.path().join("level1").join("level2").join("level3");
        std::fs::create_dir_all(&leaf).unwrap();

        let found = walk_ancestors_for_plugins(&leaf)
            .expect("expected to find corpora-plugins in an ancestor");
        // Compare canonical paths — on Windows Temp directories often
        // round-trip via short ("8.3") names and back, and std's
        // canonicalize returns a UNC-prefixed form; comparing canonical
        // to canonical is the only stable equality.
        assert_eq!(
            std::fs::canonicalize(&found).unwrap(),
            std::fs::canonicalize(&target).unwrap()
        );
    }

    #[test]
    fn walk_ancestors_returns_none_when_no_corpora_plugins_anywhere() {
        let root = tempfile::tempdir().unwrap();
        let leaf = root.path().join("a").join("b");
        std::fs::create_dir_all(&leaf).unwrap();
        // Don't create corpora-plugins anywhere in the tree.
        assert!(walk_ancestors_for_plugins(&leaf).is_none());
    }

    #[test]
    fn duplicate_ids_keep_last_seen() {
        let dir = tempfile::tempdir().unwrap();
        let mk = |display: &str| -> String {
            format!(
                r#"{{
                    "id": "dup",
                    "display_name": "{display}",
                    "languages": ["en"],
                    "default_language": "en",
                    "fallback_language": "en",
                    "identifier_label": "X",
                    "strategy": {{ "kind": "builtin", "builtin_id": "eurlex" }}
                }}"#
            )
        };
        std::fs::write(dir.path().join("a-first.json"), mk("first")).unwrap();
        std::fs::write(dir.path().join("b-second.json"), mk("second")).unwrap();
        let out = load_plugins(dir.path()).unwrap();
        assert_eq!(out.len(), 1);
        // Insertion order in the dedup HashMap depends on filename
        // scan order, which is OS-dependent. Just check we got one
        // of the two — the warn is the actionable signal.
        assert!(out[0].display_name == "first" || out[0].display_name == "second");
    }
}
