<p align="left">
  <img src="src/assets/mikerust_logo_3x3.svg" alt="MikeRust logo" width="120" height="120">
</p>

# MikeRust

Sovereign local AI document assistant — Rust+axum backend, SQLite, local filesystem storage, ONNX-based embeddings, Tauri shell, clean-room Svelte 5 frontend (forked from [`willchen96/mike`][upstream] upstream).

Designed to run entirely on the user's machine: no cloud database, no external auth provider, no S3 bucket. Optional LLM API keys are stored locally and never leave the box except to call the model provider the user explicitly configured.

Maintained by **Semplifica s.r.l.** — [semplifica.ai](https://semplifica.ai). The code is AGPL-3.0; the Semplifica wordmark and logo are trademarks, see [NOTICE.md](NOTICE.md) for the brand-vs-code separation.

### Built in the open — please contribute

MikeRust is meant to be a **collaborative** project. Fixes, new corpus
plugins, translations, jurisdiction-specific feedback, design ideas,
half-formed proposals — all welcome, and a small idea filed as an issue
tends to land faster than a full feature held back until it's "ready".
You don't have to write the patch yourself: open an
[issue](https://github.com/SemplificaAI/MikeRust/issues) to discuss a
direction, send a pull request when you have something concrete, or
reach out at [git@semplifica.ai](mailto:git@semplifica.ai)
if a public thread is the wrong place.

## Lineage

MikeRust derives from the open-source **Mike** project by Will Chen
([`willchen96/mike`][upstream]) — an AGPL-3.0 AI legal assistant with
a TypeScript / Express / Supabase / S3 / LibreOffice stack. MikeRust
keeps the *product* (chat with citations, document viewer, workflows,
tabular reviews, corpora) but rebuilds both halves:

  * the **backend** becomes a Rust+axum implementation that uses SQLite
    (via [`sqlite-vec`](https://github.com/asg017/sqlite-vec)) instead of
    Supabase + pgvector, embeds locally with ONNX (INT8-quantized
    multilingual-e5-base via fastembed + optional DirectML / QNN execution
    providers), extracts PDF / DOCX / RTF / XLSX in pure Rust — no
    LibreOffice process spawn — and ships as a Tauri desktop app with no
    server-side dependency;
  * the **frontend** is a clean-room rewrite in **Svelte 5 + Vite +
    Tailwind CSS v4**, replacing the original Next.js / React frontend —
    see *Frontend* below.

Both halves have now been rebuilt and the repository contains **no
source code from the original Mike project**. The Rust backend was
original from the start; the legacy React frontend — the only
Mike-derived code — has been **removed from the repository** and
replaced by the clean-room Svelte rewrite in `frontend/`. MikeRust
remains a fork of an AGPL-3.0 project and ships under AGPL-3.0 (see
*License*), but no Mike source survives in the tree.

For the original cloud-native upstream, see
[github.com/willchen96/mike][upstream]. For a different sister fork
specialised on Danish law, see
[github.com/marklok/danishmike](https://github.com/marklok/danishmike).

[upstream]: https://github.com/willchen96/mike

## Interface

A desktop window (Tauri) wrapping the Svelte frontend; the embedded axum
backend runs in the same process. A few views to set expectations
(screenshots are of the current Svelte UI):

### The workspace

![MikeRust Assistant home — left sidebar with Assistant, Projects, Tabular reviews, Workflows, DOCX templates and a recent-chats list; a 'Hello, Dario' greeting; a composer with attachment buttons, a per-conversation model picker and the AI disclaimer](docs/images/ui_main.png)

The sidebar holds the Assistant, Projects, Tabular reviews, Workflows and
DOCX templates, plus the recent-chats list and Settings. Light / system /
dark theme toggle sits in the top bar; the AI-disclaimer is always shown
under the composer.

### Chat with citations and inline document viewer

![Chat answer with numbered citation pills next to the document viewer, which is open on the right showing the cited PDF page](docs/images/ui_pdf.png)

Numeric citation pills (`[1]`, `[2]`, …) and `[gN]`/`[pN]` KB tags open the
source document in a resizable side panel. The viewer renders PDF / DOCX /
spreadsheets / plain text in-browser; PDF.js text-search highlights the exact
quote the model cited. Re-opening a chat re-renders all pills from persisted
annotations.

### DOCX generation from templates

![A generated 'Relazione di stima d'azienda' Word document rendered in the document viewer beside the chat](docs/images/ui_docx.png)

Attach a DOCX template, ask for the document, and the assistant drafts the
body and renders a print-ready `.docx` through the pure-Rust docx engine —
returned as a download card and previewable in the viewer.

### DOCX template editor

![The full-page DOCX template editor showing the IMS-procedure template — identity fields, per-locale display names, primary-domain and automation-level selectors, also-applicable-to checkboxes, and the start of the layout/margins section](docs/images/ui_docx_templates.png)

Every field of a template is editable on a single page — identity, layout,
typography, styles and the authoring contract. System templates open
read-only and can be **duplicated** into editable user templates, saved as
JSON under `config/docx-templates/user/`.

### Model providers

![Settings → LLM models — active-provider toggle across Anthropic, Google, OpenAI, Mistral and Local; per-provider API-key fields; a 'key set' badge on Gemini; Gemini model and serving-region selectors](docs/images/ui_models.png)

Settings → LLM models picks the active provider — Anthropic, Google, OpenAI,
Mistral, or a local OpenAI-compatible endpoint — and, for Gemini, the serving
region. Only providers with a saved API key are selectable.

### Internationalisation

Every user-facing string goes through a small runes-based i18n store
([`frontend/src/lib/stores/i18n.svelte.ts`](frontend/src/lib/stores/i18n.svelte.ts));
the catalogues live as plain JSON in
[`frontend/locales/`](frontend/locales/). Currently shipped: **Italian
(`it.json`), English (`en.json`), French (`fr.json`), German (`de.json`),
Spanish (`es.json`), Portuguese (`pt.json`)** — six locales, identical
key tree on each (no missing strings). All six dictionaries are bundled
statically; English is the canonical locale and the fallback for any key
absent from another catalogue, so a new key is safe to ship before its
translations land.

> **Key parity tool.** [`frontend/scripts/fill-i18n.mjs`](frontend/scripts/fill-i18n.mjs)
> carries a `T` table of translations and *adds* any key a locale is
> missing, then asserts all six catalogues hold the identical key set.
> Adding UI strings: put the new keys in the `T` table with all six
> languages and run `node scripts/fill-i18n.mjs`.

> ⚠️ Development and screenshots use the UI in **Italian** — that's the
> source-of-truth surface for visual review and copy iteration. **Never
> hardcode user-facing strings**, always resolve them through an
> `i18n.t('Namespace.key')` call.

## Frontend

The frontend was rewritten from scratch in **Svelte 5 + Vite + Tailwind
CSS v4**, replacing the original Next.js / React frontend.

The rewrite was done **blind**: from a written specification of UI
*behaviour* — what each screen does, which backend endpoints it calls,
what the user sees and on which interaction — never by porting,
translating or reading the React source across line by line. The point
was to carry **no code dependency** from the original Mike project into
this frontend. `frontend/` is therefore original, independent work, not
a derivative of the upstream React code.

Svelte was chosen because it is a **compiler, not a runtime framework**:
it emits markedly more **compact** code and the running UI is **leaner
and more reactive**. MikeRust's interface is form- and panel-heavy —
editors, modals, side panels, settings, tables — the kind of UI that
gains little from React's virtual-DOM diffing; Svelte's compiled,
fine-grained reactivity updates exactly the nodes that changed, with no
vDOM layer and far less shipped JavaScript.

### Code independence

The Rust backend was original from the start — Mike's backend is an
Express / TypeScript stack, so nothing was carried over. The React
frontend was the only Mike-derived code in the project, and it has now
been **removed from the repository**. The Svelte `frontend/` that
replaced it was verified independent before that removal:

- it shared **no byte-identical file** with the old React tree — a
  SHA-256 comparison of every source file in both trees found zero
  matches;
- the formats were disjoint — 106 `.tsx` React components versus 68
  `.svelte` components — so nothing could have been copied across;
- every commit that touches `frontend/` is authored by **Dario Finardi**.

The repository therefore now holds **no source code from the original
Mike project** — only original Rust (`src/`, `src-tauri/`) and the
original, blind-rewritten Svelte frontend (`frontend/`).

## Quick start

### Supported platforms

MikeRust currently ships **Windows-only**: x86_64 + ARM64 MSI
installers (the latter native on Snapdragon X Elite). **macOS** is on
the roadmap — work hasn't started yet, but the codebase already
compiles to `aarch64-apple-darwin` and the Tauri / Webview backends
are macOS-ready, so the gating items are signing / notarisation and
the equivalent of `Windows Hello` via Touch ID. **Linux is not
supported and there are no plans to add it.** Any Linux-specific
advisories that show up in the dependency graph through Tauri's GTK
chain (`gtk`, `glib`, `atk`, `webkit2gtk`, …) are therefore not
reachable in any shipped artefact — they compile on Linux but are
inert on Windows and macOS, which use `webview2-com` and `WKWebView`
respectively.

### Install a pre-built Windows release

Each tagged release ships pre-built MSI installers for Windows x86_64
and ARM64. They bundle the binary plus the matching `onnxruntime.dll`
(1.20.0) and `pdfium.dll` under `<install>/libs/<lib>/win-<arch>/`, so
the only post-install requirement is double-clicking the installer.

```
dist/MikeRust_<version>_x64.msi    # Windows x86_64
dist/MikeRust_<version>_arm64.msi  # Windows ARM64 (Snapdragon X Elite)
```

Builds are produced by `scripts/build-release.ps1` and attached to
the matching tag on
[GitHub Releases](https://github.com/SemplificaAI/MikeRust/releases).
Runtime logs land in `<home>/mikerust-data/mike-tauri.log` (see
v0.2.2 entry in [HISTORY.md](HISTORY.md) for the rationale).

### Build from source

```bash
# 1. pdfium (PDF extraction)
# Download from https://github.com/bblanchon/pdfium-binaries/releases
# Place pdfium.dll / libpdfium.so / libpdfium.dylib in libs/pdfium/

# 2. onnxruntime (embeddings via the RAG feature)
# Download the variant matching your hardware (CPU / DirectML / CUDA / CoreML / …)
# from https://github.com/microsoft/onnxruntime/releases and place the
# onnxruntime.dll / libonnxruntime.so / libonnxruntime.dylib under the
# matching libs/onnxruntime/<platform>/ subfolder. The full recipe per
# variant is in libs/onnxruntime/README.md. ort is built with
# `load-dynamic` — no statically-linked runtime, no system DLL fallback.

# 3. Backend env
cp .env.example .env
# Edit .env: set JWT_SECRET. STORAGE_PATH, DATABASE_URL etc. have sensible defaults.

# 4. Install frontend deps + Tauri CLI (one-shot)
# The Svelte frontend uses pnpm.
cd frontend && pnpm install && cd ..

# 5. Run dev (Tauri shell + axum backend + Svelte/Vite frontend)
# Preferred on Windows: use the repo-local Tauri CLI binary installed
# by pnpm under frontend/node_modules/ (version pinned by package.json,
# no global cargo-tauri required).
.\frontend\node_modules\.bin\tauri.cmd dev --config src-tauri/tauri.svelte.conf.json

# Optional alternative (only if cargo-tauri is installed globally):
# cargo tauri dev --config src-tauri/tauri.svelte.conf.json

# Or backend only (axum on 127.0.0.1:$PORT, no Tauri shell):
cargo run --features rag
```

The first run will:
- create `data/db/mike.db` (SQLite) and apply all migrations
- create `data/storage/` (uploads, chat cache)
- download `multilingual-e5-base` ONNX weights (~280 MB) into `%USERPROFILE%/mikerust-data/fastembed/` on first scan / first chat with attachments

## Architecture

```
Browser / Tauri webview (Svelte + Vite :5173)
       │  HTTP + SSE
       ▼
axum backend (127.0.0.1:<random>)   ← OS-assigned high port; the Tauri
                                      shell publishes it to the frontend
                                      via the `api_base_url` invoke
                                      command at boot.
                                      Override with PORT=3001 for the
                                      standalone-backend dev story.
   ├── SQLite          mike.db          (schema, vector store, settings)
   ├── sqlite-vec      doc_chunks       (768-dim embeddings, partition-keyed)
   ├── fastembed/ort   multilingual-e5-base ONNX  (CPU / DirectML / QNN)
   ├── pdfium-render   PDF text extraction + page rendering
   ├── quick-xml+zip   DOCX extraction (incl. redline detection — see below)
   ├── rtf-parser      RTF text extraction
   ├── calamine        XLSX/XLS/XLSB/ODS extraction
   ├── Local storage   ./data/storage/{documents,cache}
   ├── LLM             Anthropic / Gemini / OpenAI / vLLM / Ollama
   └── MCP             any HTTP/SSE MCP server, including localhost
```

## Key features

### RAG: local folder sync
Configure folders under **Impostazioni → Documenti locali**. The scanner walks the tree (honouring `.gitignore`-style patterns), extracts text per format, chunks at ~800 tokens with 200-token overlap, and embeds with INT8-quantized `multilingual-e5-base` (Xenova mirror, 768 dims, ~265 MB on disk). Embeddings live in `sqlite-vec` virtual tables in the same `mike.db`. Search queries use cosine over the partitioned vector index; partitions are keyed by `(user_id, project_id_or_global)` so cross-tenant retrieval is impossible.

Supported formats:
- **PDF** — pdfium-render native text. Per-page extraction; pages stamped with `[Page N]` markers so chunks can carry locality metadata. Scanned PDFs (no embedded text) are skipped unless a vision LLM is configured.
- **DOCX** — pure Rust ZIP+XML. Detects tracked deletions (`<w:del>`) and strike-through formatting (`<w:strike/>`/`<w:dstrike/>`); both are surfaced inline as `[removed by author: …]` markers. See [docs/DOCX.md](docs/DOCX.md).
- **RTF** — `rtf-parser` body text (control words, font tables, pictures, fields stripped).
- **XLSX / XLS / XLSB / ODS** — `calamine` per-sheet flattening.
- **TXT / MD / CSV** — UTF-8 lossy decode.
- **Images / scanned PDFs** — surfaced via the chat composer's vision path when the selected model is vision-capable; not indexed for RAG.

### RAG: hardware acceleration
ONNX Runtime execution providers compiled in via opt-in features:

```bash
cargo build --features rag-directml   # Windows GPU (DX12 device, no extra SDK)
cargo build --features rag-qnn        # Qualcomm Snapdragon NPU (X Elite / 8 Gen 3)
```

The service tries the configured EP → CPU, silently skipping providers whose DLLs aren't loadable.

**Critical fix — ort/onnxruntime ABI version match (Windows 11 ARM64 / Qualcomm Snapdragon X Elite).**
The vendored `onnxruntime.dll` in `libs/onnxruntime/<platform>/` **must match the exact version** the current `ort` crate was compiled against. ABI drift even across a single minor — e.g. 1.20.0 ↔ 1.24.x — silently deadlocks `TextEmbedding::try_new_from_user_defined` because the function-pointer table ort builds at startup references symbols (IO bindings, plugin-EP APIs) that don't exist on the other side. No error, no log, the spawn_blocking just never returns. The current pin is `ort = "=2.0.0-rc.9"` / `fastembed = "=4.9.1"`, which targets **onnxruntime 1.20.0** (the rc.12 / 1.24.2 line was reverted as a preparatory step for the upcoming data-security / privacy feature — see [`HISTORY.md`](HISTORY.md) entry for 2026-05-20).

We chased this bug across three failed attempts before isolating it to ABI mismatch. The fix:

1. **Vendor onnxruntime 1.20.0** (the version `ort 2.0.0-rc.9` actually links against — verifiable with `Select-String -Path target\debug\mike-tauri.exe -Pattern 'branch=rel-\d+\.\d+\.\d+'`). Drop the matching `onnxruntime.dll` for each platform under `libs/onnxruntime/win-{arm64,x64}/`; see [`libs/onnxruntime/README.md`](libs/onnxruntime/README.md) for the fetch recipe. With the right DLL, `try_new_from_user_defined` returns in ≈2.3 s on ARM64 native and ≈3.2 s on x64 under Windows' Prism emulation — equivalent to the static-link timing.
2. **`ort` runs in `load-dynamic` mode** (cf. `Cargo.toml`: `ort/load-dynamic` + `fastembed/ort-load-dynamic`), so the runtime DLL is a distributable artifact users can swap independently of the Rust toolchain. `ensure_onnxruntime_dylib_path()` resolves the path at startup and exports `ORT_DYLIB_PATH` before any embedding code touches the runtime.
3. **Default model: `Xenova/multilingual-e5-base` INT8-dynamic** (~265 MB) instead of intfloat FP32 (~1.1 GB). Cross-model FP32-vs-INT8 cosine drift stays ≥ 0.97 on a curated Italian legal/insurance corpus and top-1 retrieval is preserved (see `tests/embedding_perf.rs::quality_fp32_vs_int8`). The INT8 path is also ~1.8× faster on batch indexing and ~3.7× lighter in RAM, so it is now the default on every platform.

**When upgrading `ort`:** rebuild, run the `Select-String` probe above on the new `mike-tauri.exe`, then bump every vendored `onnxruntime.dll` to match the new version. Cross-minor drift is a silent deadlock, not a build error or a runtime panic — there is no fail-fast.

### Chat with attachments — hash-keyed cache
Documents attached via the chat composer (the **+** button) land in `data/storage/cache/` keyed by SHA-256 of the binary, and are pre-extracted to plain text at upload time:

```
data/storage/cache/<hash>.<ext>     # original file
data/storage/cache/<hash>.txt       # extracted plain text
```

Effects:
- **Dedup across chats** — the same file uploaded in different chats reuses the same on-disk pair (multiple `documents` rows reference one hash).
- **No filename collisions** — two different chats can both have a file named `contratto.pdf`; the hash determines the path, not the user-facing filename.
- **Auto re-extract on edit** — modifying a docx changes its hash, so the next upload generates a fresh `.txt` instead of stale text.
- **Chat-delete cleanup** — when a chat is deleted, the backend ref-counts each `content_hash` of its linked docs and removes the on-disk binary + text only when no other doc still references that hash.

See [docs/CACHE.md](docs/CACHE.md) for the storage contract and migration history.

### Citations
Assistant responses inline numeric markers (`[1]`, `[2]`) plus a trailing `<CITATIONS>` JSON block. The frontend parses both:
- **Per-marker pills** — `renderMessageHtml` in [`frontend/src/lib/utils/citations.ts`](frontend/src/lib/utils/citations.ts) maps each `[n]` to the matching annotation by `ref` (numeric) or `doc_id` (alphanumeric `[g1]`/`[p1]` from KB hits).
- **DocPanel jump** — clicking a pill opens the cited doc in the in-app viewer, scrolling to the cited page (PDFs) or section (DOCX).

Annotations are persisted on `messages.annotations` (migration 0012) so re-opening a chat re-renders all pills correctly. Page-marker contamination (`[Page N]` leaking into citation quotes) is sanitised on both write and read paths so PDF.js text-layer highlights work.

When the model skips the `<CITATIONS>` block (some providers do), `synthesise_kb_citations_from_markers` rebuilds citations from RAG hits keyed by `(g|p)<n>` tags in the response.

### Local-folder sync
**Impostazioni → Documenti locali** (formerly "Sincronizzazione"). Add a folder, optionally scope to a project, hit *Scansiona ora*. Scanner emits per-file progress over the `/sync/folders/:id/status` endpoint with coarse pipeline stages (`extracting`, `embedding`) so the user sees motion during the slow first PDF.

The embedding model state — including the one-shot ~280 MB download — is reported via `/sync/model-status`; the UI renders an amber progress bar above the folder list while in `downloading` or `loading` state.

### Authoritative legal corpora — plugin system

**The intent.** The medium-term goal for this project is a **plugin
system for downloading legal documents locally**: a contributor (or the
user) describes a new public source — Légifrance, BOE, Bundesgesetzblatt,
a regional bulletin — in a small JSON manifest, and MikeRust handles the
rest (sidebar entry, importer, bulk-snapshot ingestion, search, fetch,
embed). No Rust patch, no rebuild, no per-corpus bespoke UI. The user
keeps a fully offline mirror of the parts of public law they care about,
under their own AGPL-licensed copy of MikeRust.

The first implementation lives in [`config/corpora-plugins/`](config/corpora-plugins/)
and is documented in [docs/CORPUS_PLUGINS.md](docs/CORPUS_PLUGINS.md).
Today three strategies are supported:

- `builtin` — corpus served by a hand-written Rust adapter (EUR-Lex,
  Italian Legal). The manifest only carries metadata (display name,
  language list, license attribution).
- `dila-bulk-xml` — fully declarative: point at a DILA OPENDATA archive
  index URL, pick a *fonds* (CNIL / LEGI / JORF / CASS / KALI), and the
  generic importer takes care of download → tar walk → XML parse →
  `corpus_documents` insert → FTS5 index. **Proof of concept: CNIL**,
  ~26 000 délibérations indexed locally from an ~18 MB tar.gz, Etalab 2.0
  license, zero anti-bot exposure.
- `http-fetch-per-id` — fully declarative keyword search + single-
  document fetch driven by URL templates and CSS/JSONPath extraction.
  Carries per-source discovery metadata (jurisdiction, doc type, auth
  mode, search mode, fetch format) and an optional year-filtered search
  endpoint. This is the live engine behind the international connectors
  (e-Gov, eCFR, CourtListener, OpenLegalData, BOE, …). Sources behind a
  JS anti-bot challenge (Cloudflare/WAF) are detected and fail loudly —
  for those the connector must target the source's JSON API, never its
  website.

**Alternative under evaluation — MCP-driven backend.** A second design
is on the table: instead of (or alongside) the JSON-plugin path, expose
legal-source ingestion as an **MCP backend**. Each public source becomes
an MCP server with tools like `search`, `fetch`, `bulk_import`,
`list_indexed`; MikeRust calls those tools the same way it already calls
any other MCP server. The trade-off is roughly:

| | JSON plugins (current) | MCP backend (evaluating) |
|---|---|---|
| Author-time cost | one JSON file | one MCP server (Rust/Python/TS) |
| Run-time cost | in-process, zero extra deps | extra long-lived process |
| Reach | bounded by the manifest schema | unbounded — arbitrary code |
| Sovereignty | data stays in `data/db/mike.db` | depends on the server's policy |
| Reuse outside MikeRust | none | usable from Claude / any MCP host |

Neither path forecloses the other. The JSON plugin is shipping now
because it's the smallest possible footprint; the MCP backend is being
weighed for the connectors that can't be expressed declaratively
(arbitrary auth flows, multi-step session protocols, jurisdiction-
specific quirks like Légifrance's PISTE OAuth or the Bundesanzeiger's
TOC-then-ZIP pattern). Feedback welcome — pick your favourite source and
tell us which path would be less painful for it.

| Corpus | Strategy | Languages | Status |
|---|---|---|---|
| **EUR-Lex** (EU) | builtin (REST/SOAP + SPARQL + Cellar) | 24 EU languages | ✅ V1 — CELEX fetch via public HTML, EN fallback |
| **Italia legale** (HF dataset) | builtin (HF datasets-server + Parquet bulk) | Italian | ✅ V1 — Normattiva (~69K) + Corte Costituzionale (~22K) |
| **CNIL** (France, via DILA OPENDATA) | `dila-bulk-xml` plugin | French | ✅ V1 — délibérations + recommandations + avis (~26K docs, Etalab 2.0) |
| Italian: OpenGA (TAR + Consiglio di Stato) | Same HF dataset, source filter | Italian | 🔲 in dataset, opt-in mancante |
| Italian: Cassazione (civile/penale/sez. unite) | da identificare | Italian | 🔲 V2 — sorgente fuori dataset HF |
| Italian: Normattiva post-snapshot live | URN single-fetch | Italian | 🔲 V2 — atti dopo 2026-03-01 |
| Italian: Leggi regionali (20 BUR) | per-regione | Italian | 🔲 V3 |
| Italian: Gazzetta Ufficiale (sumario quotidiano) | XML feed | Italian | 🔲 V3 |
| Italian: Decreti ministeriali / circolari | per-ministero | Italian | 🔲 V3 (import da URL) |
| French: LEGI / JORF / CASS / KALI (DILA bulk) | `dila-bulk-xml` plugin | French | 🔲 — same strategy as CNIL; one manifest each |
| **Légifrance** (France, via PISTE) | candidate for MCP backend | French | 🔲 OAuth2 REST — JSON plugin or MCP TBD |
| **Retsinformation** (Denmark) | JSON `/api/document/{eli}` + `/api/search` | Danish | planned |
| **BOE** (Spain) | Open Data API + daily XML sumarios | Spanish | planned |
| **Gesetze im Internet** (Germany) | TOC XML → per-law ZIP | German | scraping-only |
| **Normattiva** (Italy, direct) | none — HTML / Akoma Ntoso URN deep links | Italian | sostituito dal connettore HF; resta utile come V2 live-fetch |

### Sovereign data
Everything that contains user data lives under the workspace:
- `data/db/mike.db` — schema, embeddings, settings, all chats, all documents metadata
- `data/storage/` — uploads (`documents/`), chat cache (`cache/`)
- `%USERPROFILE%/mikerust-data/fastembed/` — ONNX weights (out-of-tree to avoid the Tauri watcher)
- `data/.mikeprj` envelopes — AES-256-GCM-encrypted project bundles, key derived via Argon2id from a recipient email

No telemetry, no remote logging, no anonymous metrics. Outbound traffic only when the user explicitly invokes a remote LLM (Anthropic / Gemini / OpenAI) or a remote MCP server they configured themselves.

## Environment variables

See `.env.example` for the full reference.

| Variable | Required | Default |
|---|---|---|
| `JWT_SECRET` | **yes** | — |
| `DATABASE_URL` | no | `sqlite:<USERPROFILE>/mikerust-data/mike.db` |
| `STORAGE_PATH` | no | `%USERPROFILE%/mikerust-data/storage` |
| `FASTEMBED_CACHE_DIR` | no | `%USERPROFILE%/mikerust-data/fastembed` |
| `PDFIUM_DYNAMIC_LIB_PATH` | no | walks ancestors of cwd / exe for `libs/pdfium/` |
| `ORT_DYLIB_PATH` | no | walks ancestors for `libs/onnxruntime/<platform>/` (see [`libs/onnxruntime/README.md`](libs/onnxruntime/README.md)) |
| `PORT` | no | `0` (OS picks a free high port — see Architecture) |
| `VLLM_BASE_URL` | for local LLM | — |
| `VLLM_API_KEY` | no | `local` |
| `ANTHROPIC_API_KEY` | for Claude | — |
| `GEMINI_API_KEY` | for Gemini | — |
| `MCP_SERVERS` | no | `[]` |

## Implementation status

| Area | Status |
|---|---|
| Auth (PIN/Argon2id + Windows Hello biometric + opaque sessions) | ✅ |
| SQLite + migrations (0001 → 0031) | ✅ |
| Local storage (filesystem) | ✅ — the historical S3/R2 fallback (`s3-storage` feature) was removed in v0.5.2. The AWS SDK chain it pulled in pinned a vulnerable `rustls 0.21.12` / `rustls-webpki 0.101.7`, and the feature was never wired into `make_storage` to begin with. |
| PDF extraction (pdfium) + scanned-PDF detection | ✅ |
| DOCX extraction with redline detection | ✅ |
| RTF / XLSX / TXT / MD / CSV extraction | ✅ |
| RAG: scanner, chunker, sqlite-vec, fastembed CPU | ✅ |
| RAG: DirectML / QNN execution providers | ✅ opt-in |
| LLM: Anthropic / Gemini / OpenAI / vLLM / Ollama | ✅ |
| MCP client (HTTP/SSE) — synchronous tools | ✅ |
| MCP client — multi-step async flows (request → poll → fetch) | ⚠️ partial — see note below |
| Routes: auth, user, chat, documents, projects, workflows, docx-templates, sync, tabular-review, corpora | ✅ |
| Project: documents (PDF/DOCX/RTF/XLSX) + folders + versions + rename | ✅ |
| Project: chats list, tabular-reviews list, owner/shared visibility | ✅ |
| Project: URL `?tab=` deep-linking (documents / assistant / reviews) | ✅ |
| Project: `.mikeprj` AES-256-GCM export + import | ✅ |
| Chat citations with persistence | ✅ |
| Chat-attachment hash cache + ref-counted cleanup | ✅ |
| **Accept / Reject decision on generated docx** (migration 0029) — per-chat decision (`accepted` / `rejected`) on every docx the model emits; rejection requires a user motive and triggers a one-shot LLM summary; subsequent chat turns inject the reason + summary in place of the rejected body so the model can correct itself without re-seeing the vetoed bytes. A read-only **"Vedi riassunto"** modal re-opens the archived reason + summary after the reject modal closes; flipping back to Accept restores the original document while keeping the audit trail. | ✅ |
| **Chat-files popover** in the composer footer — surfaces all five categories the chat ever touched: uploaded attachments, tool-generated docs, rejected docs (strikethrough + red `Rifiutato` badge), project-inherited docs (`chats.project_id` → `documents.project_id`) and KB / corpora docs cited via `messages.annotations`. Per-format icon colours (Excel green / Word blue / PDF red / PowerPoint orange / Markdown text-primary); origin tag chips colour-coded by category. Reads exclusively from `GET /chat/:id/documents`. | ✅ |
| **App version badge** next to "MikeRust" in the sidebar + **License panel** in Settings → Licenza (SPDX `AGPL-3.0-only`, plain-language summary, full bundled LICENSE text) | ✅ |
| **HyDE — Hypothetical Document Embeddings** (migration 0030, v0.5.0) — opt-in toggle in Settings → Recupero documenti. When ON, `retrieve_kb_chunks` drafts a domain-aware pseudo-answer (anchored on the legal/medical/finance/… prologue), embeds it, runs a second KNN, and merges the two rankings via Reciprocal Rank Fusion (k=60) before the usual top-K + 0.75 distance threshold + PII filter. Default OFF (adds one LLM call per turn). | ✅ |
| **DirectML execution provider** compiled into the MSI (Windows DX12 GPU). `ort` tries DirectML at runtime; falls back to CPU automatically on machines without a DX12 adapter. No knob — transparent acceleration. | ✅ |
| **Citation pipeline normalisers (v0.5.1)** — model-independent post-processors that survive the variability of mid-tier LLMs: hybrid bracket splitter (`[c1, c2, FILE.pdf, p.4, doc-7]` → clean per-citation pills, stops at `<CITATIONS>`), cross-message `[cN]` lookup (re-resolves a marker against earlier assistant turns when the current turn forgot to re-emit it), plus five new explicit CITATION QUALITY RULES in `MRUST_SYSTEM_PROMPT` (omit empty quotes, ranges only with `[[PAGE_BREAK]]`, prefer per-passage + attached-doc citations, re-emit cross-turn `[cN]`). | ✅ |
| **Cross-provider determinism (v0.5.1)** — `temperature = 0.5` on Anthropic / OpenAI / local-OpenAI-compatible; `max_tokens` 4096 → 8192 on Claude + local for trailing `<CITATIONS>` JSON headroom. **Gemini sampler rolled back to its API default in v0.5.1b** for versatile heterogeneous-document analysis — tightening `gemini-2.5-flash` to 0.5 triggered a long-context white-out (model loops on whitespace until the stream limit). | ✅ |
| **Orphan KB cleanup (v0.5.1)** — `retrieve_kb_chunks` probes each chunk's `source_path` and drops missing files at chat-time; new `POST /sync/cleanup-orphans` cascades through `documents` + `doc_chunks` + `synced_files`; viewer surfaces a "Pulisci sorgenti rimosse" modal when a citation source 404s. | ✅ |
| **DocxView A4 fit + reflow toggle (v0.5.1)** — preserves A4 page geometry (width + height + margins, `breakPages: true`) and auto-scales via CSS `zoom = containerWidth / pageWidth` (clamped `[0.4, 1.5]`) through a `ResizeObserver` when the user drags the side-panel divider; top-right toggle flips to a reflow mode for narrow side-panel reading. | ✅ |
| **Tabular-review preset FK hotfix (v0.5.4)** — migration 0031 rebuilds `tabular_reviews` dropping the residual `workflow_id` REFERENCES `workflows(id)` FK that blocked every preset-based "Nuova revisione" with `(code: 787) FOREIGN KEY constraint failed`. Built-in workflows live as JSON manifests in `config/workflow-presets/` and intentionally never own a DB row; the same pattern was already fixed for `workflow_hidden` in migration 0022. No behaviour change for any existing review. | ✅ |
| **Tabular doc-upload from disk + extraction fixes (v0.5.4)** — biggest item in the release. The new picker `Upload` button in the "Aggiungi documenti" modal of a tabular review lets the user upload **directly from the filesystem**, no longer forcing them to first attach a doc to a chat or a project before it could be analysed. **Filesystem-uploaded docs used to be silently broken** because they went through the legacy `cache=false` storage path (`documents/<uid>/<doc_id>`, no extension, no extracted-text sidecar) and `load_document_text` couldn't (a) resolve the relative storage key to an absolute path pdfium can open and (b) match the catch-all "format not supported" dispatch branch caused by the missing extension — every cell came back "Document text unavailable". Both legs fixed: absolute-path resolution via `STORAGE_PATH` / `default_storage_path`, and an extension-suffixed sibling materialised on disk from the documents row's `file_type` so pdfium and the dispatcher both see a recognisable `.pdf` / `.docx` / … path. Exhaustive `[tabular][doc-text]` tracing added at every branch (sidecar hit, sidecar miss, `storage.get` failure, dispatch result, skip reason) so the next regression surfaces in the log directly. | ✅ |
| **Tabular row dedup on re-upload (v0.5.4)** — adding a document that is byte-identical to one already extracted in the same review (match key: filename + SHA-256 `content_hash`) now inherits the earlier row's cells + status instead of re-running the LLM. Hits log as `[tabular][dedup] review=… new_doc=… inherits cells …`. Prereq lit alongside this: the legacy `cache=false` upload path now also computes the SHA-256 `content_hash` (~100 ms on 50 MB, dwarfed by upload bandwidth) so the dedup join can match every upload regardless of which path delivered it. | ✅ |
| **Tabular detail full-width layout (v0.5.4)** — `TabularDetail.svelte` drops the historical `max-w-6xl` cap on header and grid. The table now fills whatever horizontal space the window provides; the side document-viewer panel resizes/collapses independently and the grid shrinks back when that panel is open. | ✅ |
| **Workflow picker labels Tabellare vs Assistente (v0.5.4)** — `PickerModal` gains an optional `badge` field per item, rendered as a right-aligned pill via the existing `Badge` component (reusing the brand-audit `tabular` purple and `assistant` blue tones). The chat composer's "Allega un workflow" modal populates it from `workflow.type`. | ✅ |
| **Import Project button on the Projects list (v0.5.4)** — secondary "Importa progetto" button next to "Nuovo progetto", powered by a hidden `<input type="file" accept=".mikeprj">` that reuses the existing email-prompt modal previously reachable only via drag-and-drop of a `.mikeprj` onto the page. Backend endpoint and i18n catalogue were already complete since v0.4.x. | ✅ |
| **"Nuova revisione" inside a project + clickable review rows (v0.5.4)** — parity with the existing "Nuova chat" button on the Conversazioni tab. The new button opens an inline modal that lists tabular workflows scoped to the project's own domain, inherits `project_id` + the project's domain on submit, and drills straight into the new review's extraction grid without leaving the project shell. Existing review rows on the project's Revisioni tabellari tab are also clickable now and open inline. | ✅ |
| **Scope-correct doc picker in tabular reviews (v0.5.4)** — the "Aggiungi documenti" modal of a tabular review now filters its list by `domain == review.domain` (always) plus a project-aware scope rule that mirrors the project's *Ambito di recupero* dropdown: **rigoroso** ("solo progetto") shows only docs whose `project_id` matches the review's project, **condiviso** ("globale + progetto") adds global docs (`project_id IS NULL`), and a review without a project shows only globals. Previously the picker surfaced every document the user had ever uploaded anywhere — including cross-domain chat attachments. Backend `GET /document` now also returns each doc's `project_id` so the filter can run client-side; the picker's Upload affordance inherits the review's project on uploads so a doc added through a strict-scope picker stays visible there afterwards. | ✅ |
| **Generic router back-stack — drill-downs always return to the originating screen (v0.5.4)** — the `router` store gains a small back stack with `goWithReturn(target, ctx, entry)`, `popBack()` and `consumePending(): NavContext`. Drill-downs (e.g. opening a chat from inside a project) push a back entry so the destination screen surfaces a contextual back arrow (e.g. "Torna a 'Studio 2026'") that pops the stack and restores the originating screen's nested state — not the destination's flat sidebar parent. Standard `router.go()` clears the stack (sidebar navigation is "switch context", not "drill"). A reactive `navTick` re-fires destination `$effect`s on re-navigation to the same route (so clicking a different project in the sidebar accordion while already on Projects updates the open project detail), and every mutating router method runs inside `untrack(…)` so calling `router.go(…)` from inside an `$effect` body doesn't accidentally subscribe the caller via the read-modify-write under `navTick++` / `backStack.length`. First consumers wired: Chat opened from a project returns to that exact project. The mechanism extends to any future drill-down by adding `NavContext` fields and a `goWithReturn` call-site. New i18n key `Nav.backToProject` localised in all six locales. | ✅ |
| **Sidebar "Progetti recenti" accordion (v0.5.4)** — between the tool nav and the existing "Chat recenti" accordion. Lists the top 5 projects by `updated_at` DESC so the user can jump between a few active projects without bouncing through the Projects screen. Click → `router.go('projects', { projectId: p.id })` and the consumePending mechanism drills straight into that project's detail. Refreshed once on Shell mount so the list is populated even before the user visits the Projects screen. New i18n keys `Sidebar.recentProjects` + `Sidebar.noProjects` localised in all six locales. | ✅ |
| **PickerModal select-all / deselect-all (v0.5.4)** — multi-select pickers (the tabular doc picker is the main consumer) gain a small toggle bar between the search input and the list. Visible only in multi mode and only when the visible set has more than one row. Scope is the **visible** (search + filter applied) set so the user can select-all on a search-filtered subset without disturbing selections outside it; the label flips to "Deseleziona tutti" once every visible row is in the selection. A live "{n} selezionati" counter on the same row gives feedback after each click. New i18n keys `PickerModal.{selectAll,deselectAll,selectedCount}` localised in all six locales. | ✅ |
| **Per-row Export button on the Projects list (v0.5.4)** — Download icon between the rename and delete affordances on every project row. Opens the existing email-prompt + include-chats modal previously only reachable from inside a project's detail page; reuses the entire `POST /project/:id/export` chain (`mikeprj::io::build_payload` → ZIP → AES-256-GCM via Argon2id-derived key from the recipient email, file format documented in [`src/mikeprj/mod.rs`](src/mikeprj/mod.rs)). The `.mikeprj` payload includes project metadata, document binaries, tabular-review column configurations (no extracted cells), custom workflow prompts, and chats when opt-in — see HISTORY for the per-field rundown. | ✅ |
| **`.mikeprj` export-shape gaps closed (v0.5.5)** — `ProjectRecord` gains `domain` + `isolation_mode`, `DocumentRecord` gains `domain` + `project_folder_id` (path hint) + accept/reject decision tuple, `TabularReviewRecord` and `WorkflowRecord` gain `domain`; the workflows exporter stops hard-coding `type = "assistant"` and discarding `columns_config` so custom tabular workflows finally travel intact. All new fields are `Option<…>` with `#[serde(default)]` so older archives still deserialise; two new back-compat tests pin the behaviour. The importer rehydrates `documents.content_hash` from the manifest's `sha256` so tabular dedup-by-hash works on the recipient's first re-upload. | ✅ |
| **Tabular reviews per-row rename (v0.5.5)** — the Revisioni tabellari list view gains a Pencil icon between the domain Badge and the Trash on every row — parity with the Projects-list rename affordance. Opens a small inline modal with the current title pre-filled (Enter submits, Escape cancels, Save disabled when empty or unchanged). Reuses the existing `PATCH /tabular-review/:id` endpoint. New i18n keys `TabularReviews.renameReview` + `TabularReviews.renamedToast` localised in all six locales. | ✅ |
| **Doc-picker project scope (v0.5.6)** — the chat composer's "Sfoglia tutti" picker inside a project-scoped chat now restricts to the project's documents via the existing `?project_id=…` query param on `documents.rs::list_documents`. Standalone chats keep the global picker unchanged. Previously every project-scoped chat surfaced the entire user library, including documents from other projects and from standalone-chat attachments. | ✅ |
| **New-chat-in-project confirm modal (v0.5.6)** — clicking `+` in the sidebar while a project-scoped chat is active now opens a confirm modal ("Stai lavorando dentro un progetto. Vuoi mantenere il progetto associato alla nuova chat?") with two action buttons (*Chat indipendente* / *Sì, mantieni il progetto*) plus implicit cancel via X / Esc / backdrop click. Also fixes the long-standing "chip persists silently" bug — the project chip used to silently survive the new-chat action because the composer's `$effect` early-returned on null `activeProjectId` instead of clearing the chip. A new monotonic `chatStore.clearProjectTick` lets the modal's "Chat indipendente" branch reset the chip without race conditions. | ✅ |
| **Experimental local-only LLM mode (v0.5.6, opt-in)** — new toggle in Settings → Modelli LLM, off by default. When on, locks the local provider to `http://localhost:11434` and collapses the chat picker to two curated Ollama Modelfile derivations (Qwen 3.5 4B `q4_K_M` + `/no_think` chat-template injection, Gemma 4 E2B IT GGUF `Q4_K_M` + `<think>` / `<thinking>` / `<reasoning>` stop sequences). Backend lives in [`src/llm/ollama_manager.rs`](src/llm/ollama_manager.rs) (wraps [`ollama-rs`](https://crates.io/crates/ollama-rs) 0.3); migration 0032 adds `user_settings.local_secure_mode`. Treated as a **preview for the v0.6.x line** — mechanism is feature-complete and tested (13 new unit tests, 15/15 + 6/6 green) but UX around model discovery, context-window tuning, and cross-platform Ollama detection is still being refined. | 🔲 preview |
| **Mistral first-class — dedicated provider with prompt caching + sequential tools (v0.6.0)** — Mistral La Plateforme promoted from generic OpenAI-compat reuse to a [dedicated provider module](src/llm/mistral.rs) with three Mistral-specific knobs baked in: `parallel_tool_calls: false` (sequential semantics for legal workflows), `safe_prompt: false` (avoids false-positive refusals on legitimate legal content), and `prompt_cache_key = "mike_chat_{chat_id}"` (Mistral charges 10% of normal token price on cache hits — 80-90% effective cost reduction on long document-heavy chats). New `enum Provider::Mistral` routes the `mistral:` prefix to its own `stream` / `complete`; one-shot callers (HyDE, summarisation, title gen, doc summary, translation) pass `chat_id: None` and skip caching. Mistral-specific error mapping (401/403/422/429 with `Retry-After` parsing, 5xx) with Italian-first messages. Two new per-user toggles in the Mistral card (`mistral_safe_prompt`, `mistral_parallel_tools`) backed by migration 0033 — both default OFF. EU hosting + ZDR info paragraph + link to the official Mistral help-center article. 17 new unit tests; 105/105 across `llm::` green. | ✅ |
| **Mistral 429 retry-with-backoff (v0.6.1)** — `POST /v1/chat/completions` is wrapped in a 3-attempt retry that honours Mistral's `Retry-After` header when present (parsed as integer seconds, capped at 30s) and falls back to exponential backoff (1s→2s→4s, total ~7s) otherwise. Only 429 triggers a retry; 401/403/422/5xx surface immediately because they're authoritative refusals or distinct failure modes. The user-perceptible behaviour: transient 429s (the common case on the free Experiment tier at 1 req/s when MikeRust fires several Mistral calls in quick succession — main chat + title gen + HyDE + tabular cell) now resolve themselves in 1-7s instead of failing the chat turn. Pure-function `next_backoff(attempt, retry_after_header)` so the policy is testable without sleeping; 5 new unit tests pin Retry-After honouring, 30s cap, exponential fallback, bogus-format fallback, whitespace tolerance. 22/22 `llm::mistral` tests green; 110/110 across `llm::`. | ✅ |
| **Mistral concurrency semaphore + JS console diagnostics (v0.6.2)** — root-cause fix on top of v0.6.1's retry. A process-global `tokio::sync::Semaphore` with 1 permit gates every Mistral request (chat / tabular / HyDE / title gen) so MikeRust never has more than one Mistral request in flight regardless of how many subsystems call it concurrently. Combined with v0.6.1's backoff this keeps the free Experiment tier (1 RPS) usable even during tabular extraction storms. Cap is fair (FIFO) so tabular cells process in worker-pool order. Two `console.warn` hooks added (`[chat] LLM error` and `[tabular] stream error event`) so users can triage 429 storms vs auth vs network failures in the DevTools console alongside the existing UI banners. 24/24 `llm::mistral` tests green; 112/112 across `llm::`. | ✅ |
| **Tabular per-cell rate-limit retry + hourglass UI (v0.6.3)** — third 429-protection layer on top of v0.6.1 backend retry and v0.6.2 semaphore. The earlier two layers protect a single Mistral call but don't help when 24+ tabular cells queue through the semaphore in sequence and Mistral's quota window shifts during the wait. v0.6.3 adds a **frontend-side retry loop**: cells whose `cell_update` SSE event carries `status: "error"` + a 429-class content are flipped to a new `rate_limited` transient state (lucide `Hourglass` icon in amber, distinct from the red `AlertCircle` for permanent failures) and re-issued via `POST /tabular-review/{id}/regenerate-cell` after a linear backoff of N × 5s (attempt 1 = 5s, attempt 2 = 10s, …) up to 10 attempts (~275s cumulative). On 429 again → loop; on success → cell flips to `done`; on permanent error → red pill. `cancelRateLimitRetries(reviewId)` is called from the "Interrompi" / "Genera" actions so stale timers don't fire against a fresh run. Also: every `cell_update` with `status === "error"` now logs to the DevTools console (was previously silent — v0.6.2's hook only covered stream-level error events). | ✅ |
| **Nuovo settore «Fiscale» — tax/commercialista italiano (v0.7.0)** — twelfth professional vertical (`fiscale`), complementary to `finance` (analysis) on the tax-compliance + advisory side. Registered end-to-end: `src/domain.rs`, `frontend/.../domain.ts`, six-locale `Domains.values.fiscale` labels. Ships 6-language system prompts ([`config/system-prompts/*/fiscale.md`](config/system-prompts/it/fiscale.md)) that bake in the 2024 tax reforms (reclamo-mediazione repealed 4.1.2024 per D.Lgs. 220/2023; new penalty regime D.Lgs. 87/2024 from 1.9.2024 with omitted-payment penalty 30%→25%; generalised prior adversarial procedure D.Lgs. 219/2023). 8 workflow presets ([`config/workflow-presets/fiscale/`](config/workflow-presets/fiscale/)) — 3 assistant (parere tributario, ravvedimento operoso calc, analisi avviso accertamento) + 5 tabular (riconciliazione IVA, verifica forfettario, quadro RW IVIE/IVAFE, imposte indirette su atti, scadenzario F24). 9 column presets (imponibile, aliquota, imposta dovuta, ritenuta, sanzione, interessi, norma, scadenza, codice tributo). Descriptive plan in [`docs/piano_settore_fiscale.md`](docs/piano_settore_fiscale.md) including public/open norm + case-law databases (Normattiva, def.finanze.it, Sentenze Web Cassazione, Giustizia Tributaria DGT). No schema migration; 40/40 preset-loader tests green. | ✅ |
| **Cross-domain workflow registration** (`also_applicable_to`, v0.7.3 + v0.7.5) — a single workflow surfaces in more than one sector's picker without duplicating its JSON. Built-in presets declare the extra domains in their manifest (validated at load); **user-created workflows** set them in the workflow/tabular editor via a free-text **Additional domains** tag picker with autocomplete (migration 0034), sanitised server-side. Shipped cross-domain pair: *analisi cespiti* (`fiscale` ⇄ `finance`) + *controlli libri contabili* (`finance` ⇄ `fiscale`). | ✅ |
| Authoritative-corpus framework (`LegalCorpusAdapter` trait) | ✅ |
| EUR-Lex V1 (CELEX-based fetch + 24-language picker + EN fallback) | ✅ |
| EUR-Lex V2 (full-text search via SOAP CWS) | 🔲 [registration required](docs/EURLEX_REGISTRATION.md) |
| Italia legale V1 (Normattiva + Corte Cost via HF dataset) | ✅ |
| Italia legale V2 (OpenGA opt-in, Cassazione, live Normattiva) | 🔲 see [CORPORA.md](docs/CORPORA.md) |
| Italia legale V3 (regional laws, GU, ministerial decrees) | 🔲 |
| **JSON-manifest plugin system** (`config/corpora-plugins/*.json`) | ✅ schema + loader + adapter registry + generic /corpora routes + discovery metadata, per-corpus enable/disable, unified year-filter search, dev hot-reload |
| **`dila-bulk-xml` strategy** (download tar.gz → walk XML → FTS5) | ✅ end-to-end test + live import |
| CNIL via DILA OPENDATA (declarative plugin) | ✅ — first proof-of-concept consumer of the plugin system |
| Other DILA fondi (LEGI, JORF, CASS, KALI) as plugins | 🔲 — same strategy, one manifest each |
| MCP-backend alternative for ingestion (Légifrance / Bundesanzeiger / …) | 🔲 design phase — see "Authoritative legal corpora" |
| Other corpus ingestors (Retsinformation, BOE, …) | 🔲 planned |
| **Professional-domain column** across workflows / tabular_reviews / projects / documents (migration 0018) | ✅ 12 canonical domains (`legal`, `medical`, `finance`, `fiscale`, `real_estate`, `hr`, `insurance`, `ip`, `compliance`, `gdpr`, `pa`, `others`), validated at API boundary, filter chips in list views |
| **Per-user `default_domain`** preference (migration 0019, Account → Generali UI) | ✅ pre-selects in every create / picker modal |
| **Per-user `enabled_domains`** toggle (migration 0027, Impostazioni → Domini) | ✅ persists subset of visible verticals server-side; NULL = all enabled; downstream filtering of pickers is a follow-up |
| **Per-file PII redaction** — GLiNER2 zero-shot multilingual NER behind the `ner-pii` feature; per-file checkbox in the chat composer (`( PII [☐] filename ✕ )`) + per-chat disclaimer modal (Enter / Esc keybinds, link to Omissis via system browser); 2000-char chunked extraction with `[LABEL]` redaction over the *extracted text* before it reaches the cloud LLM; manual HF bootstrap with byte-level download progress in the same banner as the embedding model. The whole `stream_chat` pipeline runs inside a single `tokio::spawn` so `Sse::new` returns immediately — `doc_extract_*` and `pii_redact_*` SSE events render a "Estrazione testo → Anonimizzazione PII `n / N`" progress strip inside the assistant turn the moment each stage fires (no more silent wait on long PDFs) | ⚠️ **EXPERIMENTAL — recall depends on the zero-shot model**; with the multilingual PII variant and threshold 0.2 the redactor reliably catches dates / addresses / contacts / IBANs but may miss unusual first names (zero-shot training gaps). Pure-GLiNER pipeline by design (no regex fallback); pair with [Omissis](https://edito-pdf.com) for production-grade audited redaction. Performance on Snapdragon X Elite CPU: 22-35 s per 2000-char chunk |
| **JSON-driven workflow & column-preset registries** (`config/workflow-presets/` + `config/column-presets/`) | ✅ in-memory loaders, no DB seed, merged into `/workflow` list; replaces the legacy TS constants |
| Built-in workflows shipped (legal vertical) | ✅ 14 — Generate CP Checklist, NDA Review, SPA Review, Credit Agreement Review/Summary, Commercial Agreement/Lease/Supply Review, LPA, Shareholder Agreement Review/Summary, Change of Control Review, E-Discovery Review, Employment Agreement Review |
| Built-in workflows shipped (insurance vertical) | ✅ 6 — 3 tabular comparison (RC Professionale, RC Prodotti, D&O, 24 cols each) + 3 assistant (Riassunto copertura, Due Diligence assicurativa, Inventario beni assicurati) |
| Insurance phase-2 workflows (Cyber / RC Generale / Property review / RC Medica / Key Man) | 🔲 see `docs/insurance-workflows-plan.md` |
| **Built-in workflows shipped (medical-legal vertical)** | ✅ 11 — 7 tabular (inventario documenti, timeline cronologica, diagnosi strumentali con flag DIRETTA/INDIRETTA/ESCLUSIVA, ITT, IP-RC SIMLA, MIP INAIL, invalidità civile DM 1992) + 4 assistant (diagnosi ingresso, diagnosi dimissione, nesso causale 6.1→6.6, quality check 10 punti). Map dei 7 moduli di `docs/piano_toolkit_medico_legale.md` + DOCX template `it/relazione-medico-legale` per la stesura finale. |
| **Built-in workflows shipped (finance / commercialista vertical)** | ✅ 22 — 17 tabular (inventario, scadenzario fiscale annuale, portafoglio clienti, quality-check pre-invio, riclassificazione bilanci pluriennale, indicatori economico-finanziari, metodi valutativi, contestazioni accertamento, analisi bancaria Art. 32, rideterminazione reddito, indicatori di crisi CCII, stato passivo per rango, confronto piano vs liquidatoria, cash-flow previsionale, checklist DD documenti, rischi DD con semaforo, esposizione fiscale anno×tributo, rilievi-controdeduzioni, scadenze processuali, checklist redditi PF) + 5 assistant (relazione stima d'azienda, relazione CTU tributaria, attestazione Art. 33 CCII, report due diligence, ricorso tributario D.Lgs. 546/92). Map delle 6 aree di `docs/piano_toolkit_commercialista.md` + 3 DOCX template (`it/relazione-stima-valore`, `it/attestazione-art-33-ccii`, `it/ricorso-tributario`). |
| Column-preset shortcuts (auto-suggest column prompt+format from name match) | ✅ 13 legal + 17 insurance, domain-scoped auto-match |
| Picker modals with on-the-fly domain switch | ✅ workflow / template / tabular-template / column-preset pickers all expose a `DomainSelect` combo, pre-seeded with the user's default at every open |
| **LLM model catalogue** (`config/model.json` + `GET /models`) | ✅ 4 providers (Anthropic, Google Gemini, OpenAI, Mistral), Gemini 30-region matrix, `preview`/`legacy` flags drive auto-snap to global + UI dimming |
| Settings → Modelli LLM: catalogue-driven combos | ✅ model and region dropdowns populated from `/models`; "Provider attivo" buttons gated to providers with a saved API key (lock icon + tooltip for the rest) |
| Chat model picker filters out unconfigured providers | ✅ ModelToggle hides Anthropic / OpenAI / Gemini until their API key is saved, matching the Settings page gating |
| **Six UI locales** (`it` / `en` / `fr` / `de` / `es` / `pt`) | ✅ full catalogues, identical key tree, English fallback for any missing key via the i18n store |
| Language picker | ✅ in Settings → Profile |
| **DOCX templates screen** — list, detail, generate, apply-to-chat | ✅ filter by domain / locale / free-text; detail modal renders the auto-generated authoring contract; "Apply to chat" opens a fresh conversation with the template attached |
| **DOCX template editor** (full-page) | ✅ edits every `DocxTemplate` field — identity, layout, typography, styles, authoring contract; system templates open read-only and **duplicate** into editable user templates; user templates persist as JSON under `config/docx-templates/user/` via `POST /docx-templates/save` |
| Hide / unhide DOCX templates | ✅ per-user, with Tutti / Predefiniti / Personalizzati / Nascosti tabs (migration 0024) |
| **Prompt translation** | ✅ the workflow and DOCX-template editors translate their free-text fields into a language chosen in a modal; runs through a bounded-concurrency pool with a per-request timeout and live progress |
| **Prompt caching + window-aware summarization** | ✅ the stable system prefix is sent with Anthropic `cache_control` (Gemini gets implicit caching); conversation history is compressed once the whole prompt — system prefix + attached docs + history — passes 80% of the model's context window |
| Frontend unit tests (vitest) | ✅ citation / highlight / markdown utilities + a DOCX-template-editor mount test |

### Note: MCP client — async multi-step flows

The MCP client successfully discovers servers and dispatches **synchronous
tool calls** (a tool that returns its real result on the same call). What
is **not yet reliably handled** is the multi-step async pattern that
human-in-the-loop MCP servers use — Edge / Semplifica.Edge being the
canonical case:

```
Mike → request_pseudonymized_documents       → {session_id, status:"pending"}
        (Edge waits for the human to approve in its GUI)
Mike →   get_pseudonymized_documents          → list of pseudonymised files
Mike →     count entities / download text     → next tools in the chain
```

Today Mike's auto-chain wrapper covers the immediate `request_*` →
`get_*` hop (with a 300 s timeout, configurable via
`MCP_CALL_TIMEOUT_SECS`), but a third-step or beyond — "list the
pseudonymised files, count their entities, download the pseudonymised
text" as separate tool invocations within the same conversational turn —
is not driven correctly by the dispatcher yet. The user has to nudge the
chat to perform each subsequent step manually.

Tracked as work-in-progress; do **not** modify `MRUST_SYSTEM_PROMPT` or
`build_mcp_system_prompt` when fixing this — the prompt structure is
preserved, only the dispatcher mechanics are in scope. See
[`src/routes/chat.rs`](src/routes/chat.rs) `dispatch_mcp_tool_with_async_chain`.

## Documentation

- [docs/MANUAL.md](docs/MANUAL.md) — operator manual (running, troubleshooting, recovery)
- [docs/DOCX.md](docs/DOCX.md) — DOCX extraction details (tracked changes, strikes, namespaces)
- [docs/CACHE.md](docs/CACHE.md) — chat-attachment cache layout + ref-counting
- [docs/CORPORA.md](docs/CORPORA.md) — EUR-Lex + national legal-corpora plan + API survey
- [docs/EURLEX_REGISTRATION.md](docs/EURLEX_REGISTRATION.md) — EUR-Lex V1 (no auth) + V2 SOAP registration steps
- [docs/SESSION_RECAP.md](docs/SESSION_RECAP.md) — historical session notes
- [docs/UPSTREAM_SYNC.md](docs/UPSTREAM_SYNC.md) — policy + audit log for syncing fixes from upstream `willchen96/mike`
- [docs/CORPUS_PLUGINS.md](docs/CORPUS_PLUGINS.md) — JSON-manifest plugin system for legal corpora (schema, strategies, add-a-corpus guide)
- [docs/WORKFLOWS.md](docs/WORKFLOWS.md) — user manual for Workflows, Tabular Reviews, and Assistant injection. Includes 7 non-legal examples (medical, finance, real estate, HR, insurance, IP, compliance) for designing your own templates.
- [HISTORY.md](HISTORY.md) — release notes / changelog, date-grouped
- For the pristine upstream README, see [`willchen96/mike`][upstream] directly

## License

MikeRust is a fork of the AGPL-3.0 [`willchen96/mike`][upstream] project and ships under **AGPL-3.0**. The backend (`src/`, `src-tauri/`) is original Rust and the `frontend/` is an original clean-room Svelte rewrite; both ship under the same license for consistency. See `LICENSE`.

**Brand assets are not AGPL.** The wordmark **Semplifica**, the corporate
name **Semplifica s.r.l.**, and the Semplifica logo shipped under
[`frontend/public/semplifica/`](frontend/public/semplifica/) are
trademarks of [Semplifica s.r.l.](https://semplifica.ai) and are reserved
separately from the code license. The **MikeRust** name is also reserved
as the identifier of this upstream (MikeRust ships without its own logo
for now — only the Semplifica mark is present in the UI). Forks with
substantive changes are asked to drop the Semplifica wordmark/logo and
rename the binary; see [NOTICE.md](NOTICE.md) for the full policy and
the precedent (GitLab CE, Mastodon, Nextcloud, Element, Plausible, …).
