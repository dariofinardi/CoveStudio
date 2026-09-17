# Corpus plugin manifests

Cove Studio discovers legal-corpus connectors through **JSON manifest
files** in [`config/corpora-plugins/`](../corpora-plugins). One file per
corpus. The runtime parses them at startup, validates them, and
exposes the registry to the UI (`GET /corpora`) and the chat's
`<USER LIBRARY>` system prompt block.

The manifest is intentionally **declarative and read-only**: it does
not execute code. Today every shipped manifest has
`"strategy": { "kind": "builtin", "builtin_id": "…" }`, which says
"use the named hand-written Rust adapter for this corpus". This
unifies how corpora are listed and configured without forcing the
complex existing adapters (EUR-Lex's multi-URL fallback +
AWS-WAF detection + retry-with-backoff, Italian Legal's parquet bulk
import + HF row fetch) into a JSON DSL.

When `http-fetch-per-id` and `hf-dataset-bulk` strategies land later,
adding a new corpus becomes: drop a JSON file in `config/corpora-plugins/`.

## Layout

```
config/corpora-plugins/
├── eurlex.json
├── italian-legal.json
└── …
```

By default the runtime scans `./config/corpora-plugins/` relative to the
working directory. Override with the `MIKE_CORPUS_PLUGINS_DIR`
environment variable (useful for tests or alternative installations).

## Schema

### Top-level fields

| Field | Type | Required | Description |
|---|---|---|---|
| `id` | string | ✅ | Stable corpus key. Matches `^[a-z][a-z0-9\-]*$`. Persisted in `documents.corpus_id`. |
| `display_name` | string | ✅ | Default English name shown in the UI when the user's locale has no override. |
| `display_name_locale` | `{ [locale]: string }` | ❌ | Per-locale override map. Locale keys are ISO-639-1 lowercase. The content of the manifest (`display_name`, `description`, source labels) is the source-of-truth — UI i18n only covers chrome ("Search", "Sync", section headings). |
| `description` | string | ❌ | One-line description for the corpus picker. |
| `homepage` | string | ❌ | Source-site URL. UI renders "open externally" link. |
| `languages` | `string[]` | ✅ | ISO-639-1 lowercase codes the corpus is served in. **Not** localised — codes are data; UI maps them to native language names via a universal table. |
| `default_language` | string | ✅ | Initial language when the user opens the panel. Must be in `languages`. |
| `supports_language_fallback` | bool | ❌ (default `true`) | When true and primary language returns nothing, the adapter tries `fallback_language`. |
| `fallback_language` | string | ❌ (required if `supports_language_fallback`) | Must be in `languages`. |
| `identifier_label` | string | ✅ | Label shown next to identifier inputs (CELEX, ELI, URN, BOE-A-…). |
| `identifier_example` | string | ❌ | Example for placeholder text or onboarding hints. |
| `enabled_by_default` | bool | ❌ (default `true`) | First-time enabled state in user settings. Users can toggle later. |
| `strategy` | object | ✅ | Discriminated union; see [Strategies](#strategies). |
| `capabilities` | object | ❌ (default all false) | Which generic operations this corpus supports; see [Capabilities](#capabilities). |
| `sources` | array | ❌ | Sub-sources inside this corpus that the user can toggle independently; see [Sources](#sources). |

### Capabilities

A flat boolean map declaring which generic operations the corpus exposes.
The backend mounts (or 404s) the `/corpora/:id/<op>` route accordingly;
the frontend hides UI controls for operations set to `false`. **Every
flag defaults to `false`** so that adding a new capability later doesn't
silently activate it on legacy manifests.

| Field | Maps to (when `strategy.kind == "builtin"`) | Notes |
|---|---|---|
| `search` | `LegalCorpusAdapter::search_by_id` or `search_by_keyword` (dispatcher picks based on input shape) | Free-text and identifier-lookup both go through this single endpoint. |
| `fetch` | `LegalCorpusAdapter::fetch` | Single-doc download by identifier. |
| `documents` | generic — DB-backed listing on `documents` filtered by `corpus_id` | No adapter call. Required if you want any of the next two. |
| `documents_delete` | generic — DB `DELETE` + ref-counted cache cleanup | Validation rejects manifest if `true` while `documents == false`. |
| `documents_resync` | generic — re-runs indexing from on-disk cached text | Validation rejects manifest if `true` while `documents == false`. |
| `embed_progress` | adapter-specific — usually wired to `active_embed` state | Used for long sync embeddings (EUR-Lex). Italian Legal uses `/sync/embed-progress` globally instead. |
| `bulk_import` | adapter-specific — one-shot bulk metadata download (e.g. HF parquet projection) | Only Italian Legal today. |
| `user_config` | generic — `corpus_settings` table | `GET|PUT /corpora/:id/config`. Almost every corpus has this. |

For future declarative strategies (`http-fetch-per-id`), capabilities
become objects with URL templates rather than booleans. See
[Strategies](#strategies).

### Sources

Optional array of sub-categories the user can enable/disable inside a
single corpus. Italian Legal uses this to expose Normattiva,
Corte Costituzionale, OpenGA, Cassazione as independently-toggleable
slices of the same HF dataset. Empty/absent for single-source corpora
like EUR-Lex.

| Field | Type | Required | Description |
|---|---|---|---|
| `id` | string | ✅ | Scoped to the parent corpus. Same regex as the corpus id. |
| `display_name` | string | ✅ | Not localised — source names are usually proper nouns. |
| `subtitle` | string | ❌ | Short qualifier rendered next to the name ("~125K", "(incrementale)"). |
| `description` | string | ❌ | Longer description shown when the source is `available: false`. |
| `available` | bool | ✅ | Whether the source is wired in the runtime today. |
| `default_enabled` | bool | ❌ (default `false`) | First-time enabled state. Must be `false` when `available: false` (validation). |
| `status_label` | string | ❌ | Free-text label shown next to a non-available source ("in arrivo", "coming soon", "V2 roadmap"). |

### Validation rules

The loader rejects a manifest with a `tracing::warn!` and continues
loading the others if any of these fail:

- `id` doesn't match the regex (uppercase, spaces, punctuation forbidden).
- `languages` empty, or contains a non-ISO-639-1 code.
- `default_language` not in `languages`.
- `fallback_language` set but not in `languages`.
- `supports_language_fallback: true` but `fallback_language` missing.
- `strategy.kind == "builtin"` with `builtin_id` not in the known list.

## Strategies

The `strategy` field is a discriminated union keyed on `kind`. Today
only `builtin` is honored by the runtime; the others parse but the
corpus is marked as not-runnable.

### `builtin`

Points to a hand-written Rust adapter compiled into the binary.

```json
"strategy": {
    "kind": "builtin",
    "builtin_id": "eurlex"
}
```

Known `builtin_id` values:

| `builtin_id` | Rust module | What it does |
|---|---|---|
| `eurlex` | [`src/corpora/eurlex.rs`](../src/corpora/eurlex.rs) | CELEX/ELI fetch from public EUR-Lex, 4-URL fallback (TXT/HTML/ALL/Cellar), AWS-WAF detection, retry-with-backoff, HTML longest-match extraction. |
| `italian-legal-hf` | [`src/corpora/italian_legal.rs`](../src/corpora/italian_legal.rs) | Bulk metadata import (parquet projection, pinned commit SHA) + on-demand `/rows` fetch from the `dossier-legal/italian-legal-corpus` HF dataset. Filters Normattiva + Corte Costituzionale. |

Adding a new builtin: implement `LegalCorpusAdapter`, register the
`builtin_id` in `KNOWN_BUILTINS` in
[`src/corpora/plugin.rs`](../src/corpora/plugin.rs), ship a manifest
that points at it.

### `http-fetch-per-id`

Declarative REST corpora. The JSON manifest carries the URL templates
and extraction rules; the runtime `ManifestAdapter` interprets them
and implements `LegalCorpusAdapter` generically. **No per-corpus
Rust code.** This is how CNIL is wired today.

```json
"strategy": {
    "kind": "http-fetch-per-id",
    "search_by_id": {
        "url_template": "https://api.example.com/doc/{identifier}/{lang}",
        "shape": "rest-json",
        "title_path": "$.title",
        "body_path":  "$.content.text"
    },
    "search_by_keyword": {
        "url_template": "https://api.example.com/search?q={query}",
        "shape": "rest-json",
        "hits_path":     "$.results[*]",
        "identifier_at": "$.cid",
        "title_at":      "$.title"
    }
}
```

Template placeholders (percent-encoded at substitution):
  - `{identifier}` — corpus-native id (CELEX, ref-CNIL, BOE-A-…)
  - `{query}`      — user-typed free-text
  - `{lang}`       — ISO-639-1 code from corpus settings
  - `{limit}`      — search-result cap (server picks default 20)

Unresolved placeholders after substitution fail with a clear error —
typos don't produce `https://...{garbled}/...` URLs.

Response shapes:
  - `rest-html` — CSS selectors via `scraper` crate
  - `rest-json` — tiny JSONPath subset: `$.a.b`, `$.a[0]`, `$.a[*].b`

Selector-syntax extensions (work in both shapes where it makes sense):
  - `selector@attr` — read an HTML attribute instead of element text
  - `selector:strip-prefix=PFX` — trim a literal prefix
  - `selector:strip-suffix=SFX` — trim a literal suffix

Postprocessors stack: `a@href:strip-prefix=/fr/deliberation/` is
"select the `<a>`, read `href`, strip the URL prefix". Used in the
CNIL manifest to recover bare identifiers from `href` URLs.

Generic routes that consume these strategies:

| Verb / Path | Capability | What it does |
|---|---|---|
| `POST /corpora/:id/search` | `search` | Calls `search_by_id` if input has no whitespace and is short, else `search_by_keyword`. |
| `POST /corpora/:id/fetch`  | `fetch`  | Calls `search_by_id.url_template`, extracts `title_path`/`body_path`, stores via hash-keyed cache, inserts a `documents` row, runs RAG indexing. |
| `GET /corpora/:id/documents` | `documents` | Filtered list from `documents` where `corpus_id = :id`. |

Builtin corpora (EUR-Lex, Italian Legal) bypass these routes today —
their dedicated `/eurlex/*` / `/italian-legal/*` endpoints continue
to work. The generic router returns `501 Not Implemented` for
builtin corpus ids until the migration to a single set of routes
finishes.

### `hf-dataset-bulk` *(reserved, not yet implemented)*

Generalises what `italian-legal-hf` does today: bulk metadata import
from a HuggingFace dataset (pinned revision, projected columns) +
on-demand `/rows` fetch.

## A non-Italian example — CNIL (France)

Shows how to describe a French corpus with multiple sources and a
declarative-future strategy. Marked `runnable: false` today (the
runtime only honours `builtin` strategies), but the manifest is the
real shape the JSON will take once `http-fetch-per-id` lands.

[`config/corpora-plugins/cnil.json`](../config/corpora-plugins/cnil.json):

```json
{
    "id": "cnil",
    "display_name": "CNIL",
    "display_name_locale": {
        "fr": "CNIL",
        "it": "CNIL — Garante francese",
        "en": "CNIL (France)"
    },
    "description": "Publications de la Commission nationale de l'informatique et des libertés…",
    "homepage": "https://www.cnil.fr",
    "languages": ["fr"],
    "default_language": "fr",
    "supports_language_fallback": false,
    "identifier_label": "Référence CNIL",
    "identifier_example": "SAN-2024-013",
    "enabled_by_default": false,
    "strategy": {
        "kind": "http-fetch-per-id",
        "search_by_id": {
            "url_template": "https://www.cnil.fr/fr/deliberation/{identifier}",
            "shape": "rest-html",
            "title_path": "h1.title",
            "body_path": "article.node--type-deliberation .field--name-body"
        },
        "search_by_keyword": {
            "url_template": "https://www.cnil.fr/fr/search/site/{query}?f%5B0%5D=type%3Adeliberation",
            "shape": "rest-html",
            "hits_path": "ol.search-results li"
        }
    },
    "capabilities": {
        "search": true, "fetch": true,
        "documents": true, "documents_delete": true, "documents_resync": true,
        "embed_progress": true, "bulk_import": false, "user_config": true
    },
    "sources": [
        { "id": "deliberations",  "display_name": "Délibérations (sanctions, mises en demeure)", "available": false, "status_label": "à venir" },
        { "id": "recommandations","display_name": "Recommandations et lignes directrices",      "available": false, "status_label": "à venir" },
        { "id": "guides-pratiques","display_name": "Guides pratiques et fiches méthodologiques", "available": false, "status_label": "à venir" },
        { "id": "avis-textes",    "display_name": "Avis sur projets de textes",                  "available": false, "status_label": "à venir" }
    ]
}
```

Two things to notice:

- `display_name_locale` carries FR/IT/EN overrides so an Italian user
  sees "CNIL — Garante francese" while a French user sees "CNIL".
- The strategy's URL templates use `{identifier}` and `{query}`
  placeholders. CSS selectors in `*_path` fields target the body and
  title in the rendered HTML. When the `http-fetch-per-id` runtime
  ships, the generic adapter will read these templates instead of
  calling a Rust trait method.

### EUR-Lex

[`config/corpora-plugins/eurlex.json`](../config/corpora-plugins/eurlex.json):

```json
{
    "id": "eurlex",
    "display_name": "EUR-Lex",
    "display_name_locale": { "it": "EUR-Lex", "en": "EUR-Lex" },
    "description": "Official portal for European Union law…",
    "homepage": "https://eur-lex.europa.eu",
    "languages": ["bg", "cs", "da", "…", "sv"],
    "default_language": "en",
    "supports_language_fallback": true,
    "fallback_language": "en",
    "identifier_label": "CELEX",
    "identifier_example": "32016R0679",
    "enabled_by_default": true,
    "strategy": { "kind": "builtin", "builtin_id": "eurlex" }
}
```

### Italian Legal Corpus

[`config/corpora-plugins/italian-legal.json`](../config/corpora-plugins/italian-legal.json):

```json
{
    "id": "italian-legal",
    "display_name": "Italian Legal Corpus",
    "display_name_locale": {
        "it": "Italia legale",
        "en": "Italian Legal Corpus"
    },
    "languages": ["it"],
    "default_language": "it",
    "supports_language_fallback": false,
    "identifier_label": "URN",
    "identifier_example": "urn:nir:stato:legge:2023-12-29;213",
    "enabled_by_default": true,
    "strategy": { "kind": "builtin", "builtin_id": "italian-legal-hf" }
}
```

## Adding a new corpus

### Path A — backed by a builtin Rust adapter

1. Implement `LegalCorpusAdapter` for the corpus in `src/corpora/<name>.rs`.
2. Add the builtin id to `KNOWN_BUILTINS` in
   [`src/corpora/plugin.rs`](../src/corpora/plugin.rs).
3. Drop a manifest in `config/corpora-plugins/<id>.json` with
   `"strategy": { "kind": "builtin", "builtin_id": "<your-id>" }`.
4. Restart Cove Studio. `GET /corpora` will include the new entry,
   and the settings panel will list it.

### Path B — pure JSON (when declarative strategies land)

Once `http-fetch-per-id` ships, this is the entire flow:

1. Identify the corpus's public endpoints (no auth or BYOK).
2. Map response paths to `title_path` / `body_path` (JSONPath or CSS
   selector depending on `shape`).
3. Save the JSON, restart. The corpus becomes available without a
   recompile.

## Registry semantics

- **Read-only at runtime**: hot reload is not supported. To pick up
  manifest changes, restart Cove Studio. (The hook would be cheap to add
  if needed — file-watcher on the plugins directory — but it changes
  the failure mode of misconfigured manifests, so we're keeping it
  explicit for now.)
- **Failures are isolated**: a broken manifest is logged and skipped;
  it does not stop the rest from loading.
- **Duplicate ids**: last manifest wins, with a `tracing::warn!`. The
  filesystem-scan order is OS-dependent so don't rely on it.
- **Per-user enable state** is NOT in the manifest. `enabled_by_default`
  only sets the initial state; the live state lives in `corpus_settings`
  rows in the DB (see `/eurlex/config`, `/italian-legal/config`).

## ⚠️ Security — only point plugins at sources you trust

A corpus manifest is a routing instruction: it tells Cove Studio where to
fetch bytes from and how to parse them. **Two of the supported parsers
have known unfixed-upstream advisories that only manifest under
attacker-controlled input**, and a malicious manifest is the easiest
path to that input:

- **`hf-dataset-bulk` (Parquet shards)** — the `parquet` crate pulls in
  `thrift 0.17.0`, which has a moderate-severity *Memory Allocation
  with Excessive Size Value* advisory. A crafted Parquet footer can
  declare an oversized allocation that the decoder honours before
  validating against the actual stream length, causing process OOM.
  The Apache Thrift Rust bindings have not shipped a fix; even the
  latest `parquet 58` still depends on `thrift ^0.17`. We mitigate it
  with a hard byte cap on every shard before it reaches the decoder
  (`max_parquet_file_size_mb` in
  [`config/corpora.json`](../config/corpora.json), default 500 MB)
  — see [`src/corpora/limits.rs`](../src/corpora/limits.rs) and the
  pre-decode bail in
  [`src/corpora/italian_legal.rs`](../src/corpora/italian_legal.rs).
  The cap mitigates the most obvious DoS path; it does not make
  untrusted Parquet input safe in general.

- **`dila-bulk-xml` (XML inside tar.gz)** — uses `quick-xml` against
  arbitrary tar contents. The parser is hardened against entity
  expansion, but a malicious archive could still serve oversized
  records or path-traversal entries the importer's tar walker is
  expected to reject. We don't expose a per-archive size knob today;
  the safe practice is to point this strategy only at official DILA
  endpoints (`echanges.dila.gouv.fr/OPENDATA/`).

**Concrete guidance for plugin authors:**

1. Pin `base_url` (and any per-strategy URL fields) to an official
   government / academic / well-known publisher endpoint. Never accept
   user-supplied URLs verbatim, and never point a `hf-dataset-bulk`
   strategy at a HuggingFace dataset you do not control or trust.
2. Treat third-party HF datasets as untrusted by default — review the
   shard sizes and footer structure before enabling the plugin, and
   leave the `max_parquet_file_size_mb` cap at its default unless you
   have a specific reason to raise it.
3. The runtime refuses to decode a shard above the cap with a
   `[italian-legal] shard … above the max_parquet_file_size_mb cap`
   error — that is the intended failure mode, not a bug to work around.
4. The `builtin` strategy is the safest option: the Rust adapter
   chooses every URL and parser configuration; the manifest only
   surfaces display metadata.

## Surfacing in chat

When the chat handler builds the `<USER LIBRARY>` block of the system
prompt, it reads documents persisted under each `corpus_id` from the
`documents` table. The corpus plugin registry is the metadata source
for the inventory header (display name, identifier label, source
homepage); the data source is still the `documents` table.

See [`src/routes/chat.rs::build_library_inventory_prompt`](../src/routes/chat.rs).

## See also

- [`docs/CORPORA.md`](CORPORA.md) — high-level plan + API survey per corpus
- [`docs/UPSTREAM_SYNC.md`](UPSTREAM_SYNC.md) — relationship with the Mike project and policy for upstream changes
- [`src/corpora/mod.rs`](../src/corpora/mod.rs) — `LegalCorpusAdapter` trait
- [`src/corpora/plugin.rs`](../src/corpora/plugin.rs) — manifest parser + registry
