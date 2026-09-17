<p align="left">
  <img src="src/assets/cove-studio-logo.svg" alt="Cove Studio logo" width="120" height="120">
</p>

# Cove Studio

**A private, local AI assistant for professional document work** —
legal, tax, finance, medical-legal, insurance, compliance and public
administration. Rust + axum backend, SQLite, local filesystem storage,
on-device embeddings (ONNX), Tauri desktop shell, Svelte 5 frontend.

Cove Studio runs on the user's own machine: no cloud database, no
external identity provider, no object storage. API keys for cloud
models stay in the local database and are used only to call the
provider the user configured. With the local secure mode the assistant
can run entirely offline against a local Ollama server.

> **Formerly MikeRust.** Cove Studio is the new name of the project
> previously published as **MikeRust**. The codebase, its history and
> its maintainer are the same; see [Renaming](#renaming-from-mikerust)
> for what changed and how existing installations are migrated.

## Maintainer

Cove Studio is maintained by **Dario Finardi**, the author of the
project since its first commit. Until release 0.7.5 the repository was
published under the `SemplificaAI` GitHub organisation; from this
release the project is managed directly by its maintainer as an
individual, at
[github.com/dariofinardi/CoveStudio](https://github.com/dariofinardi/CoveStudio).

Contributions are welcome: bug reports, corpus plugins, translations,
jurisdiction-specific feedback and design proposals. Open an
[issue](https://github.com/dariofinardi/CoveStudio/issues) to discuss a
direction, or a pull request when you have something concrete.

## Renaming from MikeRust

| | MikeRust (≤ 0.7.5) | Cove Studio |
|---|---|---|
| Application name | MikeRust | Cove Studio |
| Desktop app identifier | `com.mikerust.app` | `app.covestudio` |
| Per-user data folder | `<home>/mikerust-data/` | `<home>/cove-studio-data/` |
| Database file | `mike.db` | `cove-studio.db` |
| Environment variable prefix | `MRUST_` | `COVE_` |
| Project export files | `.mikeprj` (magic `MIKEPRJ\0`) | `.coveprj` (magic `COVEPRJ\0`) |
| Local secure-mode models | `mike-…-fast` | `covestudio-…-fast` |
| Rust crates | `mike`, `mike-tauri` | `cove-studio`, `cove-studio-desktop` |

Existing installations keep working:

- on first start the data folder, the database (with its `-wal` / `-shm`
  files) and the model caches are moved to the new names, so nothing is
  downloaded again and no data is left behind;
- `MRUST_*` environment variables are still read when the `COVE_*`
  variable is not set;
- `.mikeprj` files exported by MikeRust can still be imported;
- local models installed under the old names are copied to the new names
  inside Ollama (no download); removing the old names is proposed in
  Settings and happens only after the user confirms;
- the Windows installer uses the same upgrade code as MikeRust, so it
  replaces an existing MikeRust installation.

Interface preferences stored inside the desktop webview (such as the
theme) belong to the app identifier and start from their defaults after
the change; settings saved in the database are kept.

## Provenance and independence from Mike

Cove Studio started in May 2026 from the open-source project **Mike** by
Will Chen ([`willchen96/mike`][upstream]), an AGPL-3.0 legal AI assistant
built on TypeScript, Express, Supabase and Next.js. This section states
precisely what is original work and what still derives from Mike.

### Original work, independent of Mike

| Area | Location | Notes |
|---|---|---|
| Backend | `src/` | Rust + axum, written from the first commit (8 May 2026) against the HTTP contract the Mike frontend consumed, without Mike's backend code. The HTTP routes, response shapes and persistence differ (SQLite instead of Supabase/Postgres, singular route names, local-only auth). |
| Desktop shell | `src-tauri/` | Tauri 2 application, biometric unlock, native dialogs. |
| Frontend | `frontend/` | Svelte 5 application written from 15 May 2026 following the rewrite plan in [`docs/mikerust-ui-rewrite-plan.md`](docs/mikerust-ui-rewrite-plan.md) and the behavioural specification consolidated in `PLAN.md`; completed on 17 May 2026, when the forked Next.js frontend was removed (commit `0a9bbcf`). |
| Database schema | `migrations/` | SQLite schema, 34 migrations. |
| Retrieval and citations | `src/embeddings/`, `src/routes/chat*` | Local embeddings, HyDE retrieval, citation resolution and normalisers. |
| Corpora | `src/corpora/`, `config/corpora-plugins/` | EUR-Lex, Italian legal corpus, DILA bulk import, declarative connectors. |
| Document processing | `src/pdf/`, `src/docx/`, `src/sync/` | PDF, DOCX (with tracked changes), RTF, spreadsheets; DOCX generation engine. |
| Privacy features | `src/ner/`, `src/llm/ollama_manager.rs` | Personal-data redaction before cloud calls; local secure mode with the model catalogue in `config/local-models/`. |
| Project archives | `src/project_archive/` | Encrypted `.coveprj` export/import. |
| Assistant instructions | `config/system-prompts/` | Base instructions (`base.md`, rewritten in September 2026) and the domain prologues for all twelve verticals. |
| Presets for non-legal domains | `config/workflow-presets/`, `config/column-presets/`, `config/docx-templates/` | Tax, finance, medical-legal, insurance, compliance, public administration and the other verticals, plus all DOCX templates. |

### Still derived from Mike

The following parts are derived from Mike and remain under Mike's
AGPL-3.0 licence; the copyright in them belongs to Will Chen and the
Mike contributors, and they are kept with that attribution:

| Part | Location | Origin |
|---|---|---|
| Built-in tool schemas and descriptions of the assistant (`read_document`, `find_in_document`, `read_workflow`, `generate_docx`, `edit_document`, …) | `src/llm/builtin_tools.rs` | Mirrors the tool declarations in Mike's `backend/src/lib/chatTools.ts`. |
| Legal-domain workflows and column presets (NDA, SPA, LPA, credit agreement, CP checklist, …) | `config/workflow-presets/legal/`, `config/column-presets/legal/` | Based on Mike's built-in workflows; translated into Italian in v0.7.1. |
| Some interface strings | `frontend/locales/` | About thirty English texts originally written for Mike's interface. |

Compatibility with parts of Mike's HTTP contract (for example the
`/single-documents` route alias) is an interface choice, not shared code.

The git history before 17 May 2026 contains the Next.js frontend forked
from Mike, under its AGPL-3.0 licence; the first installable release
(v0.1.0, 21 May 2026) already shipped the Svelte frontend.

[upstream]: https://github.com/willchen96/mike

## Interface

A desktop window (Tauri) wraps the Svelte frontend; the axum backend
runs inside the same process. The screenshots show the Italian interface.

![Assistant home — sidebar with Assistant, Projects, Tabular reviews, Workflows, DOCX templates and recent chats; composer with attachments, model picker and the AI disclaimer](docs/images/ui_main.png)

![A chat answer with citation pills beside the document viewer open on the cited PDF page](docs/images/ui_pdf.png)

![A generated Word document shown in the document viewer beside the chat](docs/images/ui_docx.png)

![The full-page DOCX template editor](docs/images/ui_docx_templates.png)

![Settings → LLM models with the providers and their API keys](docs/images/ui_models.png)

## Features

### Assistant and citations

- Chat with documents attached from disk, from a project or from the
  indexed library; scanned pages go to vision-capable models as images.
- Citations: `[cN]` markers for attached and project documents, `[gN]` /
  `[pN]` tags for passages retrieved from the library, plus a trailing
  `<CITATIONS>` JSON block. A deterministic post-processor repairs the
  shapes models commonly get wrong before anything is stored.
- Clicking a citation opens the source in the side viewer (PDF, DOCX,
  spreadsheets, text) and highlights the quoted passage.
- The assistant answers in the language of the user's message; its
  instructions live in `config/system-prompts/` and can be edited without
  rebuilding.
- Long conversations are compressed once the prompt exceeds 80% of the
  model's context window; the stable part of the prompt is sent as a
  cacheable prefix.

### Workflows, tabular reviews and documents

- Twelve professional domains: `legal`, `medical`, `finance`, `fiscale`,
  `real_estate`, `hr`, `insurance`, `ip`, `compliance`, `gdpr`, `pa`,
  `others`.
- Built-in assistant and tabular workflows defined as JSON presets, with
  cross-domain registration; user workflows are editable in the app.
- Tabular reviews extract structured data from many documents into a
  grid, with Excel import/export and per-cell retry on rate limits.
- DOCX generation from templates through a pure-Rust engine; a full-page
  template editor; accept/reject decisions on generated documents.

### Retrieval and corpora

- Local folder sync: text extraction, chunking, INT8
  `multilingual-e5-base` embeddings stored in `sqlite-vec`.
- Optional HyDE retrieval (a drafted hypothetical answer is embedded and
  fused with the direct query through Reciprocal Rank Fusion), behind a
  Settings toggle.
- Authoritative corpora: EUR-Lex, an Italian legal corpus (Normattiva and
  Constitutional Court), CNIL via DILA bulk XML, and declarative
  connectors for further national sources. See
  [docs/CORPUS_PLUGINS.md](docs/CORPUS_PLUGINS.md).

### Models and tools

- Providers: Anthropic, Google Gemini, OpenAI, Mistral and any local
  OpenAI-compatible endpoint (Ollama, vLLM).
- **Local secure mode**: the local provider is locked to loopback and to
  the models listed in `config/local-models/ollama.json`, installed and
  configured from Settings.
- MCP client for HTTP/SSE servers, with automatic follow-up of
  `request_*` → `get_*` asynchronous tools.

### Privacy

- Optional personal-data redaction of attachments (GLiNER2, `ner-pii`
  feature) before text reaches a cloud model. The detector is
  zero-shot: it is an aid, not an audited anonymisation.
- No telemetry, no remote logging. Outbound traffic happens only when
  the user calls a cloud model, a remote MCP server or a corpus source.

## Platforms

Windows x86_64 and ARM64 (native on Snapdragon X Elite). macOS is on the
roadmap. Linux is not supported.

## Installation

Each release provides MSI installers for Windows x86_64 and ARM64. They
bundle the application with the matching `onnxruntime.dll` (1.20.0) and
`pdfium.dll`. Releases are published on
[GitHub Releases](https://github.com/dariofinardi/CoveStudio/releases).

The installers are signed with Azure Trusted Signing, so Windows shows a
verified publisher instead of an unknown-publisher warning. The
certificate belongs to **Jugaad srl**, which provides the signing
infrastructure (see *Acknowledgements*); the publisher name you see at
install time is therefore Jugaad srl, while the copyright and the
maintenance of the project are Dario Finardi's.

## Building from source

```bash
# 1. Native libraries
#    pdfium:      https://github.com/bblanchon/pdfium-binaries/releases → libs/pdfium/<platform>/
#    onnxruntime: 1.20.0 from https://github.com/microsoft/onnxruntime/releases
#                 → libs/onnxruntime/<platform>/ (see libs/onnxruntime/README.md)
#    scripts/fetch-native-libs.ps1 downloads both on Windows.

# 2. Optional environment overrides
cp .env.example .env

# 3. Frontend dependencies (pnpm)
cd frontend && pnpm install && cd ..

# 4. Desktop app in development mode
.\frontend\node_modules\.bin\tauri.cmd dev --config src-tauri/tauri.svelte.conf.json

# Backend only (axum on 127.0.0.1:$PORT)
cargo run --features rag
```

Release installers are built with `scripts/build-release.ps1`.

**Code signing.** Windows artefacts are signed with Azure Trusted
Signing, so no private key is stored anywhere: `scripts/sign-windows.ps1`
asks the service for a short-lived certificate, authenticating with the
current `az login` session. It needs the Windows SDK signing tools, the
`Microsoft.Trusted.Signing.Client` NuGet package expanded under
`%LOCALAPPDATA%\TrustedSigningClient` (the script prints the two commands
if it is missing), and an identity holding the *Trusted Signing
Certificate Profile Signer* role on the signing account. The dlib is
x64-only, so the script picks the x64 `signtool` even on an ARM64 host.
`bundle.windows.signCommand` calls the script during bundling, so the
application binary is signed before it goes into the MSI, and the MSI is
signed too.

Two failure modes are worth knowing, because both look like a missing
role:

* **`403 Forbidden` at signing time.** The client authenticates with
  `DefaultAzureCredential`, which tries Visual Studio, VS Code and Azure
  PowerShell *before* the Azure CLI; if one of those is signed in with
  another account, the service receives a valid token for an identity
  without the signer role. The script's metadata therefore excludes
  every credential except `AzureCliCredential`, tying signing to
  `az login`.
* **The first attempt after a pause fails.** The Azure CLI token is
  renewed inside the client and the call in flight is lost; the script
  retries three times with a growing pause.

**ONNX Runtime version.** `ort` is built in `load-dynamic` mode and the
vendored `onnxruntime.dll` must match the version `ort` was compiled
against (currently `ort 2.0.0-rc.9` / `fastembed 4.9.1` → onnxruntime
1.20.0). A mismatch does not fail: model loading hangs silently. After
upgrading `ort`, check the linked version with
`Select-String -Path target\debug\cove-studio.exe -Pattern 'branch=rel-\d+\.\d+\.\d+'`
and vendor the matching DLL.

## Architecture

```
Tauri webview (Svelte 5 + Vite)
       │  HTTP + SSE
       ▼
axum backend (127.0.0.1:<port chosen by the OS>)
   ├── SQLite + sqlite-vec   cove-studio.db   schema, settings, chats, embeddings
   ├── fastembed / ort       multilingual-e5-base INT8 (CPU, DirectML)
   ├── pdfium-render         PDF text and page rendering
   ├── quick-xml + zip       DOCX extraction and generation
   ├── calamine, rtf-parser  spreadsheets, RTF
   ├── GLiNER2 (optional)    personal-data redaction
   ├── LLM providers         Anthropic, Gemini, OpenAI, Mistral, local
   └── MCP client            HTTP/SSE servers, local or remote
```

The Tauri shell passes the backend port to the frontend at startup.

## Data locations

Everything is stored under `<home>/cove-studio-data/`:

| Path | Content |
|---|---|
| `cove-studio.db` | Database: settings, API keys, projects, chats, document metadata, embeddings |
| `storage/documents/`, `storage/cache/` | Uploaded files and hash-keyed chat attachments with extracted text |
| `fastembed/` | Embedding model |
| `gliner2/` | Personal-data redaction model |
| `cove-studio.log` | Desktop app log |

## Configuration

Defaults work without any environment variable. See `.env.example`.

| Variable | Purpose |
|---|---|
| `DATABASE_URL` | SQLite URL (default: the database in the data folder) |
| `STORAGE_PATH` | Document storage root |
| `FASTEMBED_CACHE_DIR`, `HF_HOME` | Model caches |
| `PDFIUM_DYNAMIC_LIB_PATH`, `ORT_DYLIB_PATH` | Native library locations |
| `PORT` | Fixed backend port for standalone runs |
| `COVE_ALLOWED_ORIGINS` | Extra CORS origins |
| `COVE_SYSTEM_PROMPTS_DIR`, `COVE_WORKFLOW_PRESETS_DIR`, `COVE_COLUMN_PRESETS_DIR`, `COVE_CORPUS_PLUGINS_DIR`, `COVE_MODEL_CATALOGUE`, `COVE_CORPORA_LIMITS` | Alternative configuration locations |
| `COVE_FORCE_MCP_TOOLS` | Send MCP tool schemas to every model |
| `MCP_CALL_TIMEOUT_SECS`, `MCP_CACHE_TTL_SECS` | MCP call timeout and discovery cache |
| `VLLM_BASE_URL`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`, `MCP_SERVERS` | Legacy fallbacks when nothing is configured in Settings |

Every `COVE_` variable is also read under the former `MRUST_` prefix.

## Known limitations

- **Multi-step asynchronous MCP flows.** Tools that return a pending
  session are followed by their `get_*` companion automatically, but
  chains of three or more asynchronous steps in one turn are not driven
  yet.
- **Personal-data redaction** depends on a zero-shot model and can miss
  uncommon names.
- **Local secure mode** is a preview: model discovery and context tuning
  are still being refined.

## Documentation

- [HISTORY.md](HISTORY.md) — release notes
- [NOTICE.md](NOTICE.md) — licensing, names and third-party trademarks
- [docs/UPSTREAM_SYNC.md](docs/UPSTREAM_SYNC.md) — relationship with the Mike project
- [docs/MANUAL.md](docs/MANUAL.md) — operator and user manual
- [docs/WORKFLOWS.md](docs/WORKFLOWS.md) — workflows, tabular reviews and presets
- [docs/CORPUS_PLUGINS.md](docs/CORPUS_PLUGINS.md), [docs/CORPORA.md](docs/CORPORA.md) — corpora
- [docs/DOCX.md](docs/DOCX.md), [docs/CACHE.md](docs/CACHE.md) — DOCX extraction, attachment cache
- [docs/mikerust-ui-rewrite-plan.md](docs/mikerust-ui-rewrite-plan.md) — the May 2026 frontend rewrite plan, kept unchanged as a historical record

## Acknowledgements

Thanks to **Jugaad srl** for DevOps support and for the code-signing
infrastructure behind the Windows installers.

Thanks to **Will Chen** and the contributors to
[`willchen96/mike`](https://github.com/willchen96/mike) for the work the
parts listed in *Provenance and independence from Mike* come from.

## License

Cove Studio is free software under the **GNU Affero General Public
License v3.0 only** ([LICENSE](LICENSE)). Copyright © 2026 Dario Finardi
for the original work; the parts derived from Mike listed above remain
copyright of Will Chen and the Mike contributors under the same licence.

The names **Cove Studio** and **MikeRust** and the logo are not covered
by the code licence; see [NOTICE.md](NOTICE.md).
