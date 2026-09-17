# Cove Studio — Development Plan

> **Last updated:** May 2026 — consolidated document (the former `PLAN.md`
> and `PLAN_MISSING.md` were merged here).
>
> **Purpose.** Cove Studio (formerly MikeRust) is a local desktop
> assistant that started from the
> [willchen96/mike](https://github.com/willchen96/mike) project and is geared
> toward **local and sovereign** use: no cloud, no mandatory external service,
> a single desktop executable. What is original and what still derives from
> Mike is listed in the README section *Provenance and independence from
> Mike*. This document describes what Cove Studio is, how it
> is built, how it starts, and maintains the **functional specification** area
> by area together with its progress status.
>
> **How to read the functional sections (9 onward).** Each section describes
> the expected *behaviour* — what the user sees, what happens on each
> interaction, which backend endpoints get called and with what semantics. It
> is a specification: whoever implements it reproduces the behaviour by writing
> new code. The **Current state** block of each section is a snapshot of the
> *initial gap analysis*; the **real, up-to-date status** is in the summary
> table of section 9. The HTTP endpoint paths and SSE event names are the
> network contract of the Rust backend and must be quoted verbatim.

---

## 1. What Cove Studio is

A desktop-first, fully local AI assistant for documents. A single Tauri
executable bundles: the axum backend, the SQLite database, the Svelte
frontend. LLM calls go to providers configured by the user (Claude, Gemini,
OpenAI, Mistral, or a local OpenAI-compatible endpoint such as Ollama/vLLM);
everything else — auth, storage, RAG, indexes, corpora — lives on the user's
machine.

Main use cases: assistant with verifiable citations, tabular document review
(structured extraction), generation of `.docx` documents from templates,
management of projects with isolated knowledge bases.

---

## 2. Architecture

```text
cove-studio.exe  (single executable)
├── Tauri webview       ← shows the Svelte frontend (static build in frontend/dist/)
└── tokio thread        ← axum on 127.0.0.1:<dynamic port> (loopback only)
        ├── SQLite   (cove-studio.db — zero setup, migrations applied automatically at startup)
        ├── Auth     (Argon2id PIN + Windows Hello / Touch ID, opaque-token sessions)
        ├── Storage  (local filesystem, canonicalized path)
        ├── RAG      (ONNX embeddings via ort, local indexes)
        ├── PDF/DOCX (pdfium-render for extraction; docx writer for generation)
        ├── LLM      (Claude / Gemini / OpenAI / Mistral / local OpenAI-compat)
        └── MCP      (JSON-RPC client to any HTTP/SSE MCP server)
```

The backend port is **dynamic**: the backend binds to `127.0.0.1:0`, the
system picks a free port, the Tauri shell receives it and injects it into the
webview as `api_base_url`. In standalone development it can be fixed via
`PORT`.

---

## 3. Workspace structure

```text
cove-studio/
├── Cargo.toml          ← workspace (members: "." and "src-tauri"), edition 2024
├── src/                ← crate `cove-studio` (library + standalone bin)
│   ├── lib.rs          ← run_server(port); exposes the axum app
│   ├── main.rs         ← standalone bin (cargo run)
│   ├── auth/           ← Argon2id PIN, biometrics, middleware, sessions, rate-limit
│   ├── db/             ← AppState, SQLite pool, migration runner
│   ├── routes/         ← auth, user, chat, projects, documents, workflows,
│   │                     tabular_reviews, docx_templates, presets, models,
│   │                     corpora, eurlex, italian_legal, sync, health
│   ├── llm/            ← claude, gemini, local (OpenAI-compat), summarize, builtin_tools
│   ├── mcp/            ← MCP JSON-RPC client (HTTP/SSE)
│   ├── pdf/            ← PDF extraction (pdfium-render) and DOCX (ZIP+XML)
│   ├── docx/           ← .docx generation (package/document_xml/styles_xml)
│   ├── presets/        ← workflow, column, docx_template (loaded from config/)
│   ├── corpora/        ← EUR-Lex, Italian Legal, manifest plugin adapter
│   ├── sync/           ← local folder scanner → RAG index
│   ├── embeddings/     ← ONNX model, sessions
│   ├── project_archive/ ← encrypted project export/import (.coveprj)
│   └── storage/        ← local filesystem
├── src-tauri/          ← crate `cove-studio-desktop` (desktop shell; depends on `cove-studio`)
│   ├── tauri.conf.json ← window, bundle, resources
│   └── src/lib.rs      ← starts the axum thread + Tauri::Builder
├── frontend/           ← Svelte 5 + Vite 6 + Tailwind v4 frontend (TS strict)
│   └── src/
│       ├── routes/     ← Boot, Unlock, Setup, Shell, Assistant, Workflows,
│       │                 Tabular, Projects, Templates, Settings, Playground
│       ├── lib/components/ ← auth, chat, documents, docx, projects, settings,
│       │                     tabular, workflow, layout, ui, domain
│       ├── lib/stores/  ← runes-based ($state): chat, projects, tabular, etc.
│       ├── lib/api/     ← typed fetch wrappers for the backend endpoints
│       └── lib/i18n/    ← 6-language bundle (it/en/fr/de/es/pt)
├── migrations/         ← 0001 … 0023 (SQL, applied in order at startup)
├── config/             ← versioned presets: workflow-presets/, column-presets/,
│                          docx-templates/, corpora-plugins/, model.json
└── libs/pdfium/        ← pdfium.dll / .so / .dylib
```

---

## 4. Startup and build

### Development — Tauri (recommended)

```bash
cargo tauri dev          # builds backend + shell, starts Vite, opens the window
```

### Development — backend and frontend separately

```bash
cargo run                # standalone axum backend (uses PORT if set)
cd frontend && pnpm dev   # Vite on :5173 — the package manager is pnpm, not npm
```

### Desktop build

```bash
cd frontend && pnpm build   # svelte-check + vite build → frontend/dist/
cargo tauri build           # builds cove-studio.exe + installer
```

### Checks

```bash
cd frontend && pnpm typecheck   # svelte-check (must report 0 errors)
cd frontend && pnpm test        # vitest (frontend unit tests)
cargo test --workspace          # Rust tests (unit + doc + integration)
```

---

## 5. Environment variables

### Backend

| Variable | Required | Default | Notes |
|---|---|---|---|
| `DATABASE_URL` | No | `<home>/cove-studio-data/cove-studio.db` | |
| `STORAGE_PATH` | No | `<home>/cove-studio-data/storage` | canonicalized at startup |
| `PORT` | No | `0` (dynamic) | fix only for standalone dev |
| `ANTHROPIC_API_KEY` / `GEMINI_API_KEY` / etc. | For LLM | — | normally keys are saved via `/user/llm-settings` |
| `VLLM_BASE_URL` | For local LLM | — | OpenAI-compatible endpoint |
| `COVE_FORCE_MCP_TOOLS` | No | — | forces MCP tools to be enabled even on local models |

### Frontend

In Tauri mode the backend URL is injected by the shell (`api_base_url`). In
standalone dev the frontend points to the backend via the known port.

---

## 6. Project conventions (always to be respected)

- Every user-visible string goes through the i18n system (6 languages: it/en/fr/de/es/pt). Never hard-coded text.
- Schema identifiers (enum values, JSON keys, route parameters) stay in English snake_case; only visible labels are localized.
- User preferences are saved server-side via the `/user/*` endpoints, not in `localStorage`.
- The "AI can make mistakes / not legal advice" disclaimer is mandatory beneath the chat composer.
- Commits go directly on `main` and do not include a `Co-Authored-By` trailer.

---

## 7. Test status

| Suite | Status |
|---|---|
| `cargo test --workspace` | 358 passed, 0 failed, 14 ignored (3 perf + 11 ONNX that require models) |
| Rust doctests | green |
| `svelte-check` | 0 errors, 0 warnings (~3500 files) |
| `vite build` | green |
| `vitest` (frontend) | unit tests present for pure utilities (`highlight`, `citations`, etc.) |

---

## 8. Differences from the original Mike

| Aspect | Mike (original) | Cove Studio |
|---|---|---|
| Backend | Express + TypeScript | **Rust axum** |
| Auth | Supabase Auth | **Argon2id PIN + Windows Hello / Touch ID** |
| Database | Supabase Postgres | **SQLite (zero setup)** |
| Storage | Cloudflare R2 | **Local filesystem** |
| Frontend | Next.js | **Svelte 5 + Vite** |
| Deploy | Web (OpenNext) | **Tauri desktop (single exe)** |
| LLM | Claude + Gemini | Claude + Gemini + **OpenAI + Mistral + local** |
| MCP | No | **Yes** |
| Cloud dependencies | Supabase, R2, OpenNext | **None** |

---

## 9. Feature status (updated summary)

Real status as of May 2026. Build, typecheck and the test suite are green;
where marked "Implemented" the components exist and compile, but a complete
functional QA pass has not been re-run in this revision.

| Area | Status | Notes |
|---|---|---|
| 1. Assistant — interactive citations | ✅ Implemented | pills, tooltip, click → viewer |
| 2. Assistant — tool/document events | ✅ Implemented | all SSE events emitted by the backend are covered (`citations`, `content_delta`, `tool_call_*`, `doc_created`, `error`); the `reasoning_*`/`doc_read`/`doc_find`/`doc_edited`/`doc_replicate`/`workflow_applied` events remain a future spec, not yet emitted by the backend |
| 3. Assistant — model, auto title, tracked changes | 🟡 Partial | model selector and title generation present; tracked-change card (accept/reject) still to be completed |
| 4. Multi-format document viewer | ✅ Implemented | PDF / DOCX / XLSX / text, tabbed panel |
| 5. Workflows — editor, edit, delete | ✅ Implemented | full-page editor |
| 6. Tabular review — grid, run, cells, chat | ✅ Implemented | `TabularDetail` |
| 7. Projects — detail, documents, folders, versions | ✅ Implemented | `ProjectDetail` |
| 8. DOCX templates — detail, generation, **editor** | ✅ Implemented | detail/describe/render/apply-to-chat + full-page editor (all fields) with persistence to `config/docx-templates/user/` |
| 9. Documents — upload, indexing status | ✅ Implemented | |
| 10. Settings — Data sources (sync, EUR-Lex, corpora) | ✅ Implemented | `SyncSection`/`EurlexSection`/`CorpusSection` |
| 11. Embedding status banner | ✅ Implemented | `EmbeddingBanner` |
| 12. MCP tool approval in chat | 🟠 Open area | asynchronous dialog still not fully resolved |
| 13. i18n regression in Settings | ✅ Resolved | strings extracted into i18n keys |

> The sections that follow keep the detailed functional specification. The
> **Current state** block of each reflects the initial gap analysis; the table
> above is the source of truth for the current status.

---

## 1. Assistant — Interactive citations

### Current state
The chat SSE stream already receives a `citations` event, but the client
registers no handler for it: the event is silently discarded. Assistant
messages are rendered as Markdown with no handling of citation markers.
Result: the raw citation block can appear as text in the response.

### Expected behaviour

**Markers in the text.** The model inserts citation markers in square brackets
in the response text:
- `[1]`, `[2]`, … → citation of a document attached to the conversation;
- `[g1]`, `[g2]`, … → fragment from the global knowledge base;
- `[p1]`, `[p2]`, … → fragment from the project knowledge base;
- comma-separated groups are allowed: `[1, 2]`, `[g1, p3]`.

Numbers that are **not** citations (e.g. amounts, 4-digit years) must be
ignored: recognize as a citation only tokens that exactly match the pattern
above and that have a resolvable citation in the received list.

**Machine block.** At the end of the response the model appends a delimited
`<CITATIONS> … </CITATIONS>` block (structured machine-readable content). This
block **must never be shown**: it must be removed from the text before
rendering it as Markdown. The removal must also work if the response arrives
truncated during streaming (the block may be partial).

**`citations` event.** The list of citations arrives as a final SSE event.
Each entry contains at least: a reference (the marker index, e.g. `1`, `g2`),
the document or fragment identifier, the source label, an optional page
(single number or a textual range such as `"41-42"` for citations straddling a
page break), and the cited text. The handler must store this list on the last
assistant message.

**Visual rendering.** Each resolvable marker becomes a small numbered
superscript "pill", clickable, colored differently by origin:
- gray → citation of an attached document;
- green → global knowledge base;
- blue → project knowledge base.

On hover it shows a tooltip with page/source and the cited text.

**Click.** Clicking a pill opens the document viewer (section 4) on the cited
document, with the cited passage highlighted and scrolled into view. If the
citation indicates a page range, the cited text may contain a page-break
separator: it must be split into two highlighted portions, one per page.

### To do
1. Add a `Citation` type to the chat types and an optional citation-list field on the message type.
2. Register the `citations` SSE event handler in the chat send flow and save it on the last assistant message.
3. Cleanup function that removes `<CITATIONS>…` (even partial) before Markdown rendering.
4. "Citation pill" component with the three colors, tooltip, and click callback.
5. Replace the markers in the rendered text with the interactive pills.

---

## 2. Assistant — Tool, document-read and document-creation events

### Current state
The SSE stream defines callbacks for `tool_call_start` and `doc_created` but
the client does not wire them up: they are discarded. No progress indicator,
nor card for generated documents.

### Expected behaviour

The assistant message is not just text: it is an **ordered sequence of typed
events** arriving from the SSE stream. Beyond the text (`content_delta` /
`content_done`), the following must be handled:

- **`reasoning_delta` / `reasoning_block_end`** — the model's "reasoning". It
  must be rendered as a collapsible "thought process" block; during streaming
  it shows an animated indicator with a rotating label ("Thinking…",
  "Reasoning…", …); at the end of the block it stays collapsed, expandable on
  demand.
- **`tool_call_start`** — a tool invocation has begun. Shows a row "Running
  {tool}…" with a spinner.
- **`tool_call_progress`** — periodic tick (~every 5 s) that updates the
  elapsed-seconds counter on the in-progress tool row. After ~10 s it shows a
  hint "this is taking longer than expected" (useful when an external tool is
  waiting for a manual approval).
- **`doc_read_start` / `doc_read`** — the assistant is reading a document:
  "Reading {file}…" then "Read {file}" (with a green dot). The "Read" file name
  is clickable and opens the document in the side panel if a citation exists
  for it.
- **`doc_find_start` / `doc_find`** — text search inside a document:
  "Searching '{query}'…" then "Found '{query}' (N occurrences) in {file}".
- **`doc_created_start` / `doc_created` / `doc_download`** — the assistant
  generated a document; ends with a downloadable/openable card.
- **`doc_edited_start` / `doc_edited`** — the assistant edited a DOCX producing
  tracked changes; ends with an edit card + download card.
- **`doc_replicate_start` / `doc_replicated`** — a document was cloned N times.
- **`workflow_applied`** — a workflow was applied; "Applied workflow {title}",
  clickable to open the workflow.
- **`error`** — shows a red error block in the message.

**Grouping.** Consecutive non-text events must be grouped in a single
collapsible "steps" container, which minimizes when text follows. Between one
real event and the next, a transient placeholder ("Thinking…") must be shown so
the activity indicator never seems stuck. Above each assistant message there is
a status icon (in progress / idle / completed / error).

**Document card.** Generated or edited documents are rendered as download
cards: file name, file-type label, optional version badge, download button. If
the document is persisted as a first-class document, clicking the card opens it
in the side panel. The download must happen with the authorization token and
accept **only** URLs relative to the backend (external URLs must be rejected so
the token is not exposed).

**Cancellation.** Interrupting the stream mid-way adds a "cancelled by user"
note. Every assistant message has a "copy" button that copies both rich HTML
and plain text to the clipboard.

### To do
1. Extend the assistant message type to an ordered list of typed "blocks".
2. Wire up all the SSE callbacks listed in the chat stream state.
3. Components: collapsible reasoning block, event row (tool / read / search), document card, collapsible "steps" container, status icon.
4. Grouping logic and transient placeholder.

---

## 3. Assistant — Model selector, automatic title, tracked changes

### Current state
- The chat send payload accepts an optional `model` field, but the client
  never sets it: no per-conversation model selector.
- The chat-rename call exists but is never invoked: no automatic title
  generation.
- No handling of tracked changes (accept/reject).

### Expected behaviour

**Model selector in the composer.** A dropdown that groups models by provider.
**Only** the models of providers the user has configured (API key saved, or
base URL set for the local provider) must appear. Models without a usable key
show a red alert icon and are not selectable for sending. If the user tries to
send with an unavailable model, a "missing API key" window opens. The chosen
model is **persisted as a user preference** server-side. The chosen model is
included in the chat send payload.

**Automatic title.** After the **first** message of a new chat completes, the
client calls `POST /chat/{id}/generate-title` passing a summary of the message
(including the name of any attached workflows/templates/files) and renames the
chat with the returned title. Chats remain manually renamable
(`PATCH /chat/{id}`) and deletable (`DELETE /chat/{id}`).

**Tracked changes (accept/reject).** When the assistant edits a DOCX, each
change becomes an **edit card** below the response. A single change → a single
card; multiple changes → a section grouping them with a summary ("N tracked
changes across M documents"), **Accept all** / **Reject all** buttons
(sequential, with a progress counter) and a collapsible list of per-change
cards. Each card has a **View** button (opens the change in the side panel) and
Accept/Reject buttons.

Accept/Reject calls `POST /single-documents/{docId}/edits/{editId}/accept` or
`.../reject`. The UI applies the change to the rendered document immediately
(shows/hides the change) for instant feedback, and rolls back if the call
fails. Resolution produces a new document version: the download-card URL and
the version badge update. Resolving a change from any surface (card, bulk bar,
buttons in the side panel) must sync the state across all surfaces.

### To do
1. Model selector in the composer, fed by the model catalog and configured providers; preference persistence.
2. Title generation logic after the first message.
3. Single edit-card and multi-change section components with bulk accept/reject.
4. Optimistic update of the rendered document and cross-surface sync.

---

## 4. Multi-format document viewer (side panel)

### Current state
Entirely absent. The document components folder is empty. It is one of the
largest missing features.

### Expected behaviour

> **License note.** This feature is specific to Cove Studio. It
> must be built with JS rendering libraries only (e.g. `pdf.js`), **without**
> system plugins.

**Structure.** A resizable panel that slides in from the right side
(drag handle for resizing; "x" to close a single tab and a "close all"
command). It hosts **browser-style tabs**, one per open document; each tab
keeps its own scroll position and viewer state. Tabs open by clicking: a
citation pill, the "View" of an edit card, a download card, or a "Read {file}"
event.

**Tab header** (varies by what triggered the open):
- *Citation mode*: a "Citation" card with the cited text and the page label,
  plus a Download button.
- *Tracked-change mode*: a "Tracked change" card with the diff (inserted text
  in green, deleted text struck through in red), an optional rationale line,
  Accept/Reject buttons, and Download.
- *Plain document mode*: only file name, version badge, Download.

**Body — renderer selection by file type:**

- **PDF.** Rendered page by page on a canvas, with an overlaid **selectable
  text layer**. Bottom-left a page counter (`current/total`); bottom-right zoom
  controls (+ buttons and a percentage value). Trackpad pinch zoom and
  ctrl+wheel zoom must work. The cited sentence must be searched in the text
  layer and highlighted; if not found on the suggested page, all pages must be
  scanned. The first highlight must be brought to the vertical center of the
  view.

- **DOCX / DOC.** Rendered in-browser. Tracked changes (insertions /
  deletions) are shown with colored style (struck through / underlined). Pages
  auto-scale to the panel width. The cited sentence must be searched in the
  text and highlighted; a target tracked change must be brought into view and
  briefly flashed. For non-blocking errors a dismissible warning banner must be
  shown at top-left.

- **Markdown / TXT / RTF.** Rendered as readable formatted text, with
  selectable text; the cited sentence highlighted and scrolled into view.

- **XLSX / spreadsheets.** Rendered as a navigable table/grid, selectable text.

**Byte retrieval.** For first-class documents a viewable representation is
requested via `GET /single-documents/{id}/display` (returns PDF bytes if a PDF
rendering exists, otherwise DOCX bytes → DOCX renderer). For knowledge-base
citations retrieval is via `GET /sync/kb-doc?path=…`.

**Selection and copy.** In all formats the rendered text must be selectable and
copyable, so the user can paste it into the assistant chat.

**Cross-source open behaviour.** Opening the same document from a different
source (citation, card, "Read" event) must reuse the existing tab if already
open, only updating its highlight/position.

### To do
1. Resizable side panel with a multi-tab manager and per-tab state.
2. `pdf.js`-based PDF renderer with text layer, zoom, page counter, highlight and text search.
3. In-browser DOCX renderer with tracked-change rendering and highlighting.
4. Renderer for Markdown/TXT/RTF and for spreadsheets (XLSX).
5. Header cards for the three modes (citation / change / plain document).
6. Cited-passage highlight logic, including the page-range case.
7. Authenticated download accepting only backend-relative URLs.

---

## 5. Workflows — Edit, delete, full-page editor

### Current state
Working: list (DB + presets), tabbed filters (All / Built-in / Personal /
Hidden), domain filter, hide/show presets, **creation** modal (assistant type
with a Markdown prompt editor, tabular type with a column editor). Missing:
**edit**, **delete** from the interface, the full-page editor, the detail view,
sharing.

### Expected behaviour

**Full-page editor** (`/workflows/{id}` conceptually). Header with a breadcrumb
path and an inline-renamable title. A save-state indicator ("Saving…" /
"Saved"). Built-in / non-editable workflows show a "Read-only" badge.

- **Assistant-type workflow.** A WYSIWYG rich-text editor that produces
  Markdown — toolbar with H1/H2/H3, bold, italic, bullet and numbered lists.
  Changes **autosave** with a ~800 ms debounce (`PATCH /workflow/{id}`
  updating the prompt).
- **Tabular-type workflow.** A table of columns. Each row is a column with
  Title, Format (free text, bullet list, number, percentage, monetary amount,
  currency, yes/no, date, tag — each with an icon), and an extraction Prompt.
  An "Add column" button opens the column modal; clicking a row edits the
  column; checkbox + actions menu for multi-delete; "x" to delete a single
  column. Column changes save immediately. Built-in tabular workflows are
  read-only (clicking a row opens the column modal read-only).

**Deletion.** Personal workflows can be deleted (`DELETE /workflow/{id}`);
multi-delete deletes the personal ones and hides the built-in ones. A delete
command must be exposed on the list rows (missing today, only hide/show
exists).

**Detail view.** Clicking a list row opens a workflow view modal (read-only for
built-in ones).

**Sharing** (personal workflows only). A modal that lets the owner add
recipient emails (with an "allow edit" flag) and list/revoke existing shares
(`POST /workflows/{id}/share`, `GET /workflows/{id}/shares`,
`DELETE /workflows/{id}/shares/{shareId}`). Shared workflows appear in the
recipients' lists; those with "allow edit" are editable, the others read-only.
The list "Source" column shows the name of whoever shared it.

### To do
1. Full-page route/editor with renamable title and save indicator.
2. WYSIWYG → Markdown editor with debounced autosave for assistant workflows.
3. Column editor with immediate save for tabular workflows.
4. Delete command (single and multi) on the list.
5. Read-only detail modal.
6. Sharing modal and share management.

---

## 6. Tabular review — Grid, run, cells, review chat

### Current state
Only working: list, creation modal (title, domain chosen first, selection of a
tabular workflow of that domain from which it inherits the columns), delete with
confirmation. **The whole core of the feature is missing**: no grid, no run, no
cell handling, no detail view.

### Expected behaviour

**Review detail view.** Header with breadcrumb + renamable title, a search box
(filters the document rows), a share button (standalone review), an **Export**
button (downloads the grid as an Excel file) and a **Run** button. A toolbar
with: an "Assistant in the review" toggle (opens the review chat panel), an
Actions menu (when rows are selected: Clear results, Delete documents), Add
documents, Add columns.

**The grid.** Rows are documents, columns are the extraction questions; the
first column is the document file name. Both the checkbox column and the
document column stay "sticky" during horizontal scroll. Column headers have an
inline edit menu. A "+" header cell and a "+" button in the toolbar add
columns.

**The cells.** Each cell shows the AI answer for that (document, column) pair:
- `pending` → empty; `generating` → an animated skeleton (shimmer);
  `error` → a red alert icon.
- `done` → the first line of the answer, truncated. A small colored
  flag-dot in the corner (green / gray / yellow / red) signals a cell rating.
  Clicking the cell opens an inline overlay with the full answer; the overlay
  has a "View details" action.
- Answers are Markdown with two special inline elements: **citation pills**
  (numbered superscripts; clicking them scrolls the grid to the cited cell and
  highlights it) and **tag pills** (colored chips).

**Running the review.** "Run" opens a `POST /tabular-review/{id}/generate`
stream. Cells are optimistically set to `generating` (skipping those already
`done`), then one `cell_update` SSE event per cell updates its content and
status in real time. It requires ≥ 1 column and ≥ 1 document, and an available
model (the "review model" saved by the user); otherwise the "missing API key"
window opens.

**Cell detail panel.** Opening a cell (or clicking a citation in a cell)
scrolls a panel into view. It has an info column with the column name, document
name, the flag badge, the formatted **Results** and the **Reasoning**, with
previous/next navigation between columns and a **Regenerate** button
(`POST /tabular-review/{id}/regenerate-cell`). When a citation is clicked, the
document viewer (section 4) also appears on the left side with the sentence
highlighted.

**Document and column management.** Add documents (`PATCH` with the new
document identifiers); columns added/edited/deleted from the column modal with
immediate save (`PATCH` with the column configuration); "Clear results"
(`POST .../clear-cells`) sets the cells of the selected rows back to `pending`.

**"Add column" modal.** Allows adding one or more columns together. Each draft
has: a name field (typing a name auto-suggests a preset configuration when it
matches a known **column preset** by regular expression), a preset dropdown
(filterable by domain, retrieved from `/column-presets`), a Format selector, a
tag editor (when the format is "tag" — chips added with Enter / comma), and a
multi-line prompt with an **Auto-generate prompt** button
(`POST /tabular-review/prompt`, which returns a prompt from preset / LLM /
fallback). In edit mode it acts on a single column and can delete it.

**Review creation modal.** To be extended from the current one: beyond title
and workflow-template, it must have a "create under a project" toggle (then a
project menu) and a **document selector** (a directory-style list of standalone
documents and projects with their folders; in project mode only the ready
documents of that project). An upload button adds new documents inline.

**Review chat panel.** A resizable panel on the left inside the review. The
user asks free questions about the review; the AI answers with streaming
content, reasoning blocks and "Read {file}" steps. Answers carry **tabular
citations** — numbered pills that, when clicked, scroll the grid to the
referenced cell. A "clock" icon lists previous review chats (searchable); a "+"
starts a new chat; a trash can deletes the current one; the panel has its own
model selector. Streaming on `POST /tabular-review/{id}/chat`; SSE events
including `chat_id`, `chat_title`,
`reasoning_delta`/`reasoning_block_end`, `content_delta`, `doc_read_start`/`doc_read`,
`citations`. Chats persist (`GET /tabular-review/{id}/chats`,
`.../chats/{chatId}/messages`, `DELETE …`).

### To do
1. Review detail view with header, toolbar, search.
2. Grid component with columns/cells, sticky columns, editable headers.
3. Cell states (pending/generating/error/done), flag-dot, inline overlay.
4. Run via the `generate` stream with optimistic update and `cell_update` handling.
5. Cell detail panel with navigation, regeneration, and document-viewer hook-up.
6. "Add column" modal with presets, auto-suggestion, prompt generation, tag editor.
7. Extension of the creation modal (project + document selector + inline upload).
8. Review chat panel with history, model, tabular citations.
9. Grid export to Excel.

---

## 7. Projects — Detail, documents, folders, versions, export/import

### Current state
Working: list with search and domain filter, create/edit/delete (name,
description, domain). **Missing**: detail view (clicking a row does nothing),
document management, isolation modes, `.coveprj` export/import, project chat,
project review.

### Expected behaviour

**Project detail view.** Header: breadcrumb + renamable title (with an optional
case number as a suffix), search, a members/people button, a **RAG isolation
toggle** (owner only — switches between "shared" mode, in which the project
chats see global + project documents, and "strict" mode, in which they see only
project documents), an **export** button (opens the export modal), and the
"+ Chat" / "+ Tabular review" buttons.

Three tabs (linkable via a query parameter):

**Documents tab.** A folder tree: subfolders and documents nested freely. Each
document row shows a file-type icon (or a spinner while `pending`/`processing`,
or a red alert on `error`), the file name (inline renamable), and an expandable
section for the version history. Drag and drop to move documents into folders
and reorder subfolders into other folders (with cycle protection). Context menu
(right-click) and toolbar offer: Add subfolder, Add documents (upload or browse
modal), rename/delete folder (cascading). Multi-actions on selected documents:
Download (single file or a zip archive via
`POST /single-documents/download-zip`), Remove from subfolder, Delete (reserved
for the owner). Clicking a document opens it in the view modal/panel.

**Document versions.** Each document can have multiple versions. Expanding the
history (`GET /single-documents/{id}/versions`) lists the versions with number,
origin and a renamable display name. The user can upload a new version
(`POST .../versions`, via a modal) and rename versions
(`PATCH .../versions/{vid}`).

**Assistant tab.** Lists the project chats (`GET /projects/{id}/chats`), each
renamable/deletable. "+ Chat" creates a project-scoped chat and opens it. The
project chat behaves exactly like the global Assistant but its RAG scope is the
project (according to the isolation mode).

**Review tab.** Lists the project's tabular reviews; "+ Tabular review" creates
one with project scope (requires ≥ 1 ready document).

**Project export.** A modal that asks for a recipient email and an "include the
chats" checkbox. `POST /project/{id}/export` returns the encrypted `.coveprj`
binary, which is downloaded. The file is cryptographically bound to the
recipient's email — only that person can import it.

**Project import via drag & drop.** Dragging a `.coveprj` file onto the
projects page shows a drop overlay; on drop, a confirmation window asks for the
recipient email (the file is encrypted, bound to that email).
`POST /project/import` imports it and navigates to the new project.

**Project create/edit modal.** To be extended with the case number and the
editability of the isolation mode (currently absent from the modal).

### To do
1. Project detail route/view with header and three tabs.
2. Folder-based document tree with drag & drop, cycle protection, context menu, multi-actions.
3. Version history: list, upload new version, rename.
4. Assistant tab with project chat at project RAG scope.
5. Review tab with project-scoped reviews.
6. RAG isolation toggle (owner only).
7. `.coveprj` export modal and drag-&-drop import flow with email confirmation.
8. Extension of the project modal (case number, isolation mode).

---

## 8. DOCX templates — Detail, generation, apply-to-chat, **editor**

### Current state
List, detail modal (`describe`), apply-to-chat and the render window (`render`)
are implemented. **In development**: the template editor, which lets the user
create and edit their own DOCX templates.

### Expected behaviour

**Detail modal.** Clicking a row opens a modal that retrieves the full template
definition and its auto-generated authoring prompt
(`POST /docx-templates/describe`). The modal shows: localized name, id, domain
badge (plus the "also applicable to" domains), automation-level badge, "system"
marker, the list of required metadata fields.

**Apply to chat.** The detail modal has an "Apply to chat" action that opens a
new Assistant chat with the template already attached as a chip. A direct link
from the Templates page that opens the Assistant with the template
pre-attached must also be possible.

**Generation/render.** The backend exposes `POST /docx-templates/render`
(returns a `.docx` blob) and `POST /docx-templates/describe`. A render window
must be built that collects the values of the required metadata fields and
produces the document; the response header that signals unresolved placeholders
must be handled, showing them to the user.

### DOCX template editor

**Persistence model.** System templates live in `config/docx-templates/` as
read-only JSON files. User templates live in `config/docx-templates/user/` as
**writable** JSON files: creating or editing a template means writing a JSON
file in that folder. The read endpoints (`list`, `describe`, `render`) merge
the two sources; system templates are never editable or deletable from the
interface.

**Backend write endpoints.**
- `POST /docx-templates` — creates a new user template; the body is the full
  JSON definition; the backend assigns an id under `user/`, validates and
  writes the file. Returns the saved definition.
- `PUT /docx-templates/{id}` — updates an existing user template; rejects with
  an error if the id belongs to a system template.
- `DELETE /docx-templates/{id}` — deletes a user template; rejects system
  templates.
After every write the in-memory template state (`state.docx_templates`) must be
realigned so that `list`/`describe`/`render` see the changes immediately.

**Full-page editor ("core + complete layout" scope).** Covers **all** fields of
the `DocxTemplate` model, not just the authoring ones:
- *Identity*: localized name (for the 6 languages), category, domain and "also
  applicable to" domains, locale, automation level, placeholder syntax, origin
  reference.
- *Layout and typography*: paper format, stamp-duty use, margins,
  typography/font, footnotes, style map (baseline + overrides), supported
  directives, header and footer block, section numbering.
- *Content*: section skeleton, per-field prompts, required metadata, character
  limits, few-shot examples, extra prompt text.
System templates open in the editor read-only, with the ability to **duplicate**
them into a new editable user template.

### To do
1. ✅ Detail modal with `describe` and metadata fields.
2. ✅ "Apply to chat" action and direct template → Assistant link.
3. ✅ Render window that collects the metadata, calls `render`, downloads the `.docx`.
4. ✅ Backend write endpoints (`POST /docx-templates/save` and `/delete`) to `config/docx-templates/user/` with read-side merge.
5. ✅ Full-page editor with complete field coverage and duplicate-from-system.

---

## 9. Documents — Upload, indexing status, conversion

### Current state
No dedicated screen. Documents are reachable only as chat attachments. There is
no upload, no list, no display of the indexing status.

### Expected behaviour

**Upload.** Uploads accept pdf/docx/doc/rtf/xlsx/xls/xlsb/ods/csv/txt/md and
images. A standalone upload goes to `POST /single-documents`; project uploads to
`POST /projects/{id}/documents`.

**Document status.** After upload a document has a status
(`pending` → `processing` → `ready`, or `error`): the backend converts the
formats and builds embeddings/indexes. The status must be shown visually
(spinner while processing, alert on error, ready when processing is finished).

**URL and byte resolution.** Document URLs resolve via
`GET /single-documents/{id}/url`; the bytes for viewing via the `/display` and
`/docx` endpoints (see section 4).

### To do
1. File upload component (native picker) used by chat, projects and reviews.
2. Reusable document status indicator (pending/processing/ready/error).
3. Optional standalone document screen/list if required later.

---

## 10. Settings — Data sources (local sync, EUR-Lex, corpora)

### Current state
The "Data sources" section is a disabled entry that renders a "coming soon"
placeholder. No UI for local sync, EUR-Lex, Italian legal corpus. Working well,
instead: Profile, Security, Models, MCP.

### Expected behaviour

The Settings sub-navigation includes a "Documents and sources" group with:
Local documents/sync, plus an entry for each corpus registered by the backend
(e.g. EUR-Lex, Italian Legal). The corpora list is obtained from `GET /corpora`;
corpora not yet wired up appear dimmed with a "coming soon" suffix.

### 10.1 Local document sync
Indexes local filesystem folders into the RAG knowledge base. An "add folder"
form (path, label, recursive checkbox, and a scope selector — global or a
specific project). The folder list shows, for each, the scope, the time of the
last scan, a **Scan** button and a remove button. During a scan, a live
progress section shows processed/total, indexed/skipped/failed counts, a
progress bar, and the current file with the pipeline phase
(`extracting`/`embedding`). At the top of the page a banner shows the
download/loading progress of the embedding model. Expanding a folder lists the
synced files with a per-file status (ready/skipped/failed) and fragment count.
Polling: `GET /sync/folders/{id}/status` (~1.5 s while scanning) and
`GET /sync/model-status` (~0.7 s).
API: `GET/POST /sync/folders`, `POST /sync/folders/{id}/scan`, `DELETE`, `GET .../files`.

### 10.2 EUR-Lex corpus
Searches and indexes EU legal documents into the RAG knowledge base. An
"enabled" toggle, a language selector (24 EU languages) with an "English
fallback" option; the configuration auto-saves (debounced) via
`PUT /eurlex/config`. A **smart search** box accepts a CELEX identifier, a
natural reference, or keywords; `POST /eurlex/search` returns the results. Each
result shows title, CELEX identifier, available languages, an "open on EUR-Lex"
link (opens the system browser), and a **Sync** button that fetches and indexes
the document (`POST /eurlex/fetch`). An "indexed documents" section lists the
already-synced documents with a status badge (indexed / syncing with a live % /
queued / interrupted / "no fragments"), a **resync** command for the failed
ones (`POST .../resync`) and deletion. During the sync it polls
`GET /eurlex/embed-progress` and shows a per-document embedding progress bar.
API: `GET /eurlex/config`, `GET /eurlex/documents`, `DELETE /eurlex/documents/{id}`.

### 10.3 Italian Legal corpus (and generic corpora)
A corpus-specific page that follows the same scheme as EUR-Lex/sync: a bulk
import flow with phased progress (`POST /corpora/{id}/import`,
`GET .../import-status`, `GET .../import-progress`), search/fetch, and document
management — driven by the capabilities declared by the corpus. Corpora not
wired up fall back to a generic corpus page rendered from the metadata of
`GET /corpora/{id}`.

### To do
1. Enable the "Data sources" section with sub-navigation fed by `GET /corpora`.
2. Local Sync page: add folder, list, scan, live progress, polling, file list.
3. EUR-Lex page: auto-saved config, search, sync, indexed documents, resync, embedding polling.
4. Italian Legal corpus page and generic corpus page.
5. Dedicated API modules and stores for sync and eurlex/corpora (absent today).

---

## 11. Embedding status banner (Assistant)

### Current state
Absent.

### Expected behaviour
Above the chat composer, a banner appears **only** when the chat is waiting for
a response **and** the embedding subsystem is busy: "downloading the model"
(one-time, with MB progress), "loading the model" (a 5–10 s session build after
a restart), or "computing embeddings N/M" during bulk indexing. It is invisible
at steady state. It polls `GET /sync/model-status` and
`GET /eurlex/embed-progress` (~500 ms, with auto-throttling at rest). It never
blocks typing.

### To do
1. Banner component with the three states, self-throttling polling, visible only when the subsystem is busy.

---

## 12. MCP tool approval in chat

### Current state
Absent. (See also the memory note about the still-unresolved MCP asynchronous
dialog: a request/retrieve pattern with auto-chaining + 300 s timeout +
progress ticker already shipped, but residual failure modes remain.)

### Expected behaviour
When an external/MCP tool requires a manual approval during a response, the
in-progress tool row must show the "this is taking longer than expected" hint
after ~10 s (see section 2, `tool_call_progress`). Any approval dialog must be
integrated with care: **do not modify the model's system prompt**.

### To do
1. Align the tool event row with the progress ticker.
2. Evaluate with the user whether/how to expose an explicit approval dialog (still an open area).

---

## 13. i18n regression in Settings *(defect to fix)*

### Current state
The Profile, Security, Models, MCP and Danger zone sections contain
**English hard-coded strings** (labels, toast messages). The biometric
verification overlay also has hard-coded strings. This violates the project
rule that every visible string goes through the i18n system.

### To do
1. Extract all visible strings of the Settings sections and the biometric overlay into i18n keys.
2. Add the keys in all 6 languages (it/en/fr/de/es/pt) keeping bundle parity.
3. Verify at build that key parity is maintained.

---

## Suggested implementation order

The features are interdependent. Suggested sequence:

1. **Settings i18n fix (sec. 13)** — defect, quick, unblocks compliance.
2. **Multi-format document viewer (sec. 4)** — foundation of citations and cells.
3. **Assistant citations (sec. 1)** + **tool/document events (sec. 2)** — they complete the chat and depend on the viewer.
4. **Model selector + automatic title + tracked changes (sec. 3)**.
5. **Documents: upload and status (sec. 9)** — prerequisite for reviews and projects.
6. **Complete tabular review (sec. 6)** — large, depends on the viewer and documents.
7. **Projects: detail and tabs (sec. 7)** — large, depends on documents, chat and reviews.
8. **Workflows: full-page editor + edit/delete/sharing (sec. 5)**.
9. **DOCX templates: detail and generation (sec. 8)**.
10. **Settings → Data sources (sec. 10)** + **embedding banner (sec. 11)**.
11. **MCP tool approval (sec. 12)** — to be discussed, open area.

The `documents/`, `domain/`, `tabular/` component folders are currently empty:
they will be populated respectively by sections 9, by the domain selectors
already used elsewhere, and by section 6.
