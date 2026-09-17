# Cove Studio — Operator and User Manual

This manual describes how to set up and use Cove Studio as it is
implemented in this repository. For an overview of the project, its
architecture, build instructions, provenance and licensing, see the
[README](../README.md). Release notes are in [HISTORY.md](../HISTORY.md).

Interface labels are quoted in English. The interface is available in
Italian, English, French, German, Spanish and Portuguese, and follows the
language chosen in **Settings → Profile → Language**; if you use another
language, the labels on screen are the translations of the ones quoted
here.

## Contents

1. [Overview and concepts](#1-overview-and-concepts)
2. [First start, PIN and unlocking](#2-first-start-pin-and-unlocking)
3. [The main window](#3-the-main-window)
4. [Settings](#4-settings)
5. [Assistant](#5-assistant)
6. [Projects](#6-projects)
7. [Workflows](#7-workflows)
8. [Tabular reviews](#8-tabular-reviews)
9. [DOCX templates](#9-docx-templates)
10. [Data locations, backup and migration](#10-data-locations-backup-and-migration)
11. [Logs and troubleshooting](#11-logs-and-troubleshooting)
12. [Configuration files and environment variables](#12-configuration-files-and-environment-variables)

---

## 1. Overview and concepts

Cove Studio is a desktop application for a single local user. The window
(Tauri) hosts the interface; the backend runs inside the same process and
listens only on `127.0.0.1`, on a port chosen at start-up. Data stays in a
per-user folder on the machine (see [section 10](#10-data-locations-backup-and-migration)).
Network traffic happens when you call a cloud model, a remote MCP
server or a corpus source, and when a model is downloaded for the first
time.

| Concept | What it is |
|---|---|
| **Profile** | The one local user: username, optional display name and a numeric PIN. Only one profile can exist per installation. |
| **Document** | A file you upload, attach to a chat, index from a local folder or corpus, or that the assistant generates. |
| **Project** | A container for documents (organised in folders), chats and tabular reviews, with a domain and a retrieval scope. |
| **Chat** | A conversation with the assistant, either standalone or inside a project. |
| **Workflow** | A reusable instruction. *Assistant* workflows carry a prompt; *tabular* workflows carry a set of columns. Built-in workflows come from JSON presets; you can create your own. |
| **Tabular review** | A grid where each row is a document and each column is an extraction prompt run by the model. |
| **DOCX template** | A definition of a Word document (layout, typography, structure, authoring rules) that the assistant uses to generate `.docx` files. |
| **Domain** | A professional vertical that filters workflows and templates and selects the assistant's domain instructions. |

The twelve domains are: Legal, Medical, Finance, Tax, Real estate, HR,
Insurance, IP, Compliance, GDPR, Public Administration and Others
(identifiers `legal`, `medical`, `finance`, `fiscale`, `real_estate`,
`hr`, `insurance`, `ip`, `compliance`, `gdpr`, `pa`, `others`).

---

## 2. First start, PIN and unlocking

### 2.1 Start-up

When the application starts, a "Connecting to Cove Studio…" screen is
shown while the interface locates the embedded backend and checks that it
answers. If the backend cannot be reached, the screen shows "Cannot reach
the backend" with the error and a **Retry** button; see
[section 11](#11-logs-and-troubleshooting).

### 2.2 Creating the profile

On the first start no profile exists and the setup screen ("Welcome to
Cove Studio") appears:

| Field | Rules |
|---|---|
| Username | Required. |
| Display name (optional) | Used in greetings. |
| PIN / Confirm PIN | 4 to 8 digits; both fields must match. |

Click **Create profile**. There is no PIN recovery by e-mail or password:
as the setup screen says, keep the PIN somewhere safe. If Windows Hello is
enabled later, it can be used to set a new PIN (see 4.2).

The PIN is stored as an Argon2id hash in the database.

### 2.3 Unlocking

Every time the application starts, the unlock screen asks for the PIN;
the session is kept in memory only and is not reused across restarts.
Type the PIN and click **Unlock**.

If biometric unlock is enabled in Settings and Windows Hello is available
on the device, the unlock screen also shows **Use biometric unlock**.
The Windows Hello dialog is attached to the Cove Studio window; while it
is open the application shows "Follow the system prompt to continue."

Biometric unlock is implemented for Windows Hello only. macOS Touch ID is
not implemented.

### 2.4 Signing out

**Sign out**, in the top bar, ends all sessions of the profile and returns
to the unlock screen. There is no automatic lock after a period of
inactivity; close the application or sign out when you leave the
workstation.

---

## 3. The main window

The window opens at 1280 × 800 pixels and cannot be made smaller than
960 × 640.

### 3.1 Sidebar

From top to bottom:

- The product name, the version, and a **domain selector**. The domain
  chosen here is the active domain for the whole application: it is saved
  immediately as your default domain and pre-selects the domain in the
  workflow, template, project and review dialogs. It lists only the
  domains enabled in Settings.
- **Assistant**, with a **+** button that starts a new chat. If the
  current chat belongs to a project, a dialog asks whether to keep the
  project (**Yes, keep the project**) or start an **Independent chat**.
- **Projects**, **Tabular reviews**, **Workflows**, **DOCX templates**.
- **Recent projects**: the five most recently updated projects.
- **Recent chats**: all chats. Hovering a chat shows **Rename** and
  **Delete**. Deleting asks for confirmation; the confirmation states that
  the chat, its messages and the documents uploaded or generated in it are
  permanently deleted.
- **Settings**, pinned at the bottom.

If a reply is still being generated and you open or create another chat,
a dialog ("Interrupt the response?") asks for confirmation before the
reply is stopped.

### 3.2 Top bar

The top bar shows the current screen, a theme switch (light, dark or
system), your display name and **Sign out**. While a reply is streaming,
a pulsing dot appears next to your name even if you are on another
screen.

The theme choice is stored in the desktop webview on this device, not in
the database.

---

## 4. Settings

Settings is organised in sections: **Profile**, **Security**, **Domains**,
**Data sources**, **LLM models**, **MCP servers**, **Retrieval**,
**License**, **Danger zone**.

### 4.1 Profile

- **Username** (read-only) and creation date.
- **Display name**, saved with **Save**.
- **Language**: one of the six interface languages; saved immediately.
- **Default domain**: the same setting as the domain selector in the
  sidebar.

Before you unlock, the interface uses the operating-system language when
it is one of the six supported languages. After unlocking, it uses the
language saved in the profile, or English if none has been saved.

The language also selects the language of the assistant's domain
instructions (see 12.2).

### 4.2 Security

**PIN.** Enter **Current PIN**, **New PIN** and **Confirm new PIN**, then
**Change PIN**. The new PIN must be 4 to 8 digits.

If biometric unlock is enabled, the form also offers **Forgot your PIN?**:
it hides the current-PIN field and **Reset with biometrics** sets the new
PIN after a Windows Hello verification. **Use the current PIN instead**
returns to the normal form.

**Biometric unlock.** The section first checks the device. Without
Windows Hello it shows "No biometric hardware detected on this device".
Otherwise the **Unlock with biometric** switch appears; turning it on
requires a Windows Hello verification, turning it off does not.

### 4.3 Domains

A switch per domain. Domains that are switched off are hidden from the
selectors and filters across the application. At least one domain must
stay enabled. Changes are saved immediately.

### 4.4 Data sources

Data sources are the documents indexed into the local knowledge base,
which the assistant searches at chat time. The section has a filter bar
(name, **Jurisdiction**, **Type**) and a row of tabs: **Local documents**
first, then one tab per corpus.

#### Local documents

Index the files of a folder on disk.

1. Enter the **Folder path** or click **Browse…** to open the Windows
   folder picker.
2. Optionally enter a **Label**.
3. Leave **Include subfolders** checked to index the folder recursively.
4. Click **Add folder**.

Each configured folder shows the last scan time and these actions:

- **Scan now** starts indexing in the background. Progress shows the
  processed/total count, the indexed, skipped and failed counts and the
  current file. Unchanged files are not processed again on later scans.
- **Show files** lists each file with its number of passages ("chunks")
  and its status: **Indexed**, **Skipped** or **Error**.
- **Remove folder** removes the folder from the configuration. Files on
  disk are not touched.

Indexed formats: PDF with embedded text, DOCX, RTF, XLSX, XLS, XLSB, ODS,
CSV, TXT and MD. Scanned PDFs without a text layer and images are not
indexed. Text is split into passages and embedded locally with the
`multilingual-e5-base` model (INT8, on the CPU); the model is downloaded
on first use, and a banner shows the download and loading progress.

There is no automatic watcher: run **Scan now** again after the folder
changes.

#### Corpora

Each corpus tab corresponds to a manifest in
[`config/corpora-plugins/`](../config/corpora-plugins/). A tab appears only
when the manifest is marked available and has an adapter that can run.
With the manifests currently in the repository these are:

| Tab | Source | What the tab offers |
|---|---|---|
| **EUR-Lex** | EU legislation | Settings, search, indexed documents |
| **IT / Italian-Legal** | Normattiva and Constitutional Court, from a public dataset snapshot | Snapshot import, search, indexed documents |
| **FR / CNIL** | CNIL deliberations from the DILA open-data archives | Snapshot import, search, indexed documents |
| **CH / Fedlex** | Swiss federal legislation | Search, indexed documents |

The other manifests in the folder are declared but marked unavailable, so
they have no tab. See [docs/CORPUS_PLUGINS.md](CORPUS_PLUGINS.md) and
[docs/CORPORA.md](CORPORA.md) for the manifest format.

The release build script (`scripts/build-release.ps1`) does not copy
`config/corpora-plugins/` into the installer. An installed application
therefore shows corpus tabs only if that folder is present next to the
executable or its location is set with `COVE_CORPUS_PLUGINS_DIR`.

**EUR-Lex tab:**

- **Sync enabled** switch, **Reference language** (the 24 official EU
  languages) and **Fall back to English when the chosen language is not
  available**. Changes are saved automatically.
- **Search** accepts a CELEX number, an ELI or act reference
  (e.g. "Directive 2014/24/EU") or keywords. Each result has an open
  link (in your browser) and **Index**.
- **Indexed documents** lists what has been indexed, with identifier,
  date, language, passage count and status, plus buttons to open the
  source, re-index and **Remove from index**.

**Other corpus tabs** show, depending on the corpus:

- **Source enabled** switch.
- **Import the corpus index**: downloads and indexes the corpus snapshot
  locally (**Import now**, later **Update**), with progress.
- **Search**, with a hint when the source searches by citation only or
  within a date window. Each result can be previewed with **View text**
  (the preview supports Ctrl+F and **Open on source**) and indexed with
  **Index**; several index requests are queued and run one at a time, and
  a running one can be cancelled.
- **Indexed documents (n)** with **View text** and **Remove from
  index**.

The source disclaimers and some empty-list messages in these tabs are
currently shown in Italian regardless of the interface language.

#### Orphan passages

If a document viewer cannot load a file because it has been removed from
disk, it offers **Clean orphan sources**, which deletes the documents
whose file no longer exists together with their indexed passages.

### 4.5 LLM models

The section is driven by the provider and model catalogue in
[`config/model.json`](../config/model.json). Provider cards appear in
this order: Local, Mistral, Google, OpenAI, Anthropic.

**Active provider.** Chips for the five providers; a chip is enabled only
once the provider is configured (an API key, or a base URL for Local).
The selected chips determine which models are offered in **Model roles**.

**Local (OpenAI-compatible).**

- With **Secure local mode** off, enter a **Base URL** (for example
  `http://127.0.0.1:11434/v1` for Ollama) and an optional API key, then
  click **Refresh** to list the models served there. The list is fetched
  by the backend, not by the webview.
- **Secure local mode** is described in 4.6.

**Mistral AI.** API key and model. **Model profile (quick assignment)**
sets the three model roles in one click and saves them immediately:

| Profile | Main | Chat titles | Tabular review |
|---|---|---|---|
| Fast | Mistral Small | Ministral 3B | Mistral Small |
| Balanced | Mistral Large | Ministral 3B | Mistral Large |
| Premium | Mistral Medium | Mistral Small | Mistral Medium |

If the roles have been set by hand, a note says they were customised.
Two switches, both off by default: **Safety filter (safe_prompt)** and
**Run tool calls in parallel**.

**Google Gemini.** API key, model and **Region**. The backend calls the
global Generative Language endpoint; a region other than global is stored
but not used for routing (a warning is written to the log).

**OpenAI.** API key and model.

**Anthropic (Claude).** API key.

**Model roles.** Three selectors:

| Role | Used for |
|---|---|
| **Main** | Chat replies. Also used for the rejection summary of generated documents and for HyDE. |
| **Chat titles** | The title generated after the first exchange of a new chat. |
| **Tabular review** | Tabular review cells; if not set, the main model is used. |

Click **Save changes** to store the section. API keys are stored in the
local database file without additional encryption; protect backups of
the data folder accordingly.

**Tools and vision by model.** Two capabilities depend on the model name:

- MCP tools are offered to Anthropic (`claude…`), Gemini (`gemini…`),
  OpenAI and Mistral models, and not to local models, unless
  `COVE_FORCE_MCP_TOOLS=1` is set.
- Images and page images of scanned PDFs are sent only to models whose
  name matches a known vision pattern (for example `claude`, `gemini`,
  `gpt-4o`, `pixtral`, `llava`, `qwen2.5-vl`, or any name containing
  `vision`). For other models these attachments are skipped and a warning
  is written to the log.

### 4.6 Secure local mode

Secure local mode is a preview feature. When the switch is on (it is
saved immediately):

- the local provider may only use a loopback address; requests are sent
  to `http://localhost:11434/v1` (Ollama);
- only the models listed in
  [`config/local-models/ollama.json`](../config/local-models/ollama.json)
  are accepted;
- the assistant's instructions for local models ask for direct answers
  without explicit reasoning blocks;
- the model selector in the chat composer lists only the catalogue models
  and hides cloud models, even if API keys are configured.

The card shows the Ollama server address and the catalogue. If Ollama is
not running, it shows "Ollama not detected on port 11434." with a link to
`ollama.com/download` and **Retry**.

For each catalogue model the card shows its name, the base model, the
approximate size and the recommended RAM, and:

- **Install**: pulls the base model if missing ("Downloading…" with a
  percentage and a progress bar, cancellable with **Cancel**), then
  creates the configured derivation in Ollama ("Configuring…"). An install
  in progress continues if you leave Settings.
- **Remove** (for an installed model): deletes the derivation; the base
  model stays in Ollama.

The catalogue currently contains two models:

| Id | Base model | Approx. size | Recommended RAM |
|---|---|---|---|
| `covestudio-qwen35-4b-fast` | `qwen2.5:3b-instruct-q4_K_M` | 2.5 GB | 8 GB |
| `covestudio-gemma4-e2b-fast` | `hf.co/unsloth/gemma-4-E2B-it-GGUF:Q4_K_M` | 3.1 GB | 12 GB |

The chat request for a local model is built from the **Base URL** stored
for the Local provider, which is then replaced by the loopback address in
secure mode. If no base URL has ever been saved, the request falls back to
the `VLLM_BASE_URL` environment variable and fails without it. Save a
local base URL (for example `http://127.0.0.1:11434/v1`) with secure mode
off before turning it on.

#### Models installed under previous names

Earlier releases created the same models as `mike-…-fast`, and some
Ollama aliases as `mikerust-…`. The mapping from old to new names is in
the `legacy_ids` and `renamed_aliases` entries of the catalogue file.

When the catalogue is loaded and Ollama contains models under a previous
name:

1. The models are copied to their new names inside Ollama. Nothing is
   downloaded and nothing is deleted. Model settings that pointed at the
   old names are updated. A notice reports how many models were moved.
2. A dialog, "Remove models with the previous name?", proposes deleting
   the old names from Ollama. **Remove** deletes them; **Keep** leaves them.
   If a model could not be copied, the dialog proposes removing the old
   version and downloading the model again.

Until they are migrated, the previous names are still accepted in secure
mode.

### 4.7 MCP servers

MCP servers give the assistant additional tools. The list shows each
server's name, transport and URL, a switch to enable or disable it, and
edit and remove buttons.

**Add server** opens a form with **Name** (cannot be changed later),
**URL**, **API key (optional)** (sent as a bearer token) and **Enabled**.
**Test connection** detects the transport (HTTP or SSE) and reports the
number of tools, prompts and resources, and a suggested path if the
server answered on a different one.

Behaviour during chats:

- the list of tools of each server is cached for 5 minutes
  (`MCP_CACHE_TTL_SECS`);
- a tool call waits up to 300 seconds (`MCP_CALL_TIMEOUT_SECS`, maximum
  1800);
- after a tool has been running for 10 seconds, the chat shows a note
  that some MCP tools wait for a manual confirmation on the server side.

Servers configured with the STDIO transport (for example through the
legacy `MCP_SERVERS` variable) are not started by this build. Chains of
three or more asynchronous MCP steps in one turn are a known limitation
(see the README).

### 4.8 Retrieval

**HyDE — Hypothetical Document Embeddings**, off by default. When on,
each chat turn first asks the main model for a short hypothetical answer
in the register of the active domain, searches the knowledge base with
both the question and that answer, and merges the two rankings. It adds
one model call per turn, billed by the active provider. The change applies
from the next turn.

### 4.9 License

Shows the product name, the running version, the SPDX identifier
(AGPL-3.0-only) and the full licence text. For names, trademarks and the
parts derived from Mike, see the [README](../README.md#provenance-and-independence-from-mike)
and [NOTICE.md](../NOTICE.md).

### 4.10 Danger zone

**Delete account** deletes the profile after confirmation (**Delete
everything**) and returns to the setup screen. The database records
linked to the profile are deleted with it. Files already stored under the
data folder (`storage/`) are not deleted by this operation; remove the
data folder by hand if you need them gone (see section 10).

---

## 5. Assistant

### 5.1 Empty chat

A new chat shows a greeting and the composer. Below the composer the
application always shows: "AI can make mistakes. Answers are not legal
advice."

### 5.2 Composer

Type the message in the text area. **Enter** sends; **Shift+Enter** adds
a new line. While a reply is streaming, **Send** becomes **Stop**.

The buttons below the text area:

| Button | Action |
|---|---|
| Paperclip (**Attach documents**) | **Upload files** from disk, or **Browse all** to pick existing documents. In a chat that belongs to a project, **Browse all** lists only that project's documents. |
| **Attach a project** | Picks a project. On a new chat, sending the first message creates the chat inside that project. |
| **Attach a workflow** | Picks one workflow; the picker is filtered by domain (the project's domain in a project chat, otherwise your default domain) and the filter can be changed. |
| **Attach a template** | Picks one DOCX template, filtered by domain in the same way. |
| **Files in this chat** | Opens the list of documents of the chat (5.6). |
| Model selector | Lists the models of the configured providers (all catalogue models if no API key is set; only the local catalogue in secure mode). Choosing a model changes the saved **Main** model role, for all chats. |

Attachments appear as chips above the text area and can be removed
individually. Files, workflow and template are cleared after sending; a
project chip stays while you are in that project's chat.

Upload formats accepted by the file picker: PDF, DOCX, DOC, RTF, XLSX,
XLS, XLSB, ODS, CSV, TXT, MD, PNG, JPG/JPEG, TIFF. The upload limit is
100 MB per request.

What is sent to the model for each attachment:

- text-bearing formats (DOCX, RTF, spreadsheets, CSV, TXT, MD) and PDFs
  with a text layer: the extracted text;
- scanned PDFs: up to 8 pages rendered as images, only for vision-capable
  models;
- PNG, JPEG: the image; TIFF: each frame converted to JPEG; only for
  vision-capable models.

### 5.3 Personal-data protection (PII)

Each file chip has a **PII** checkbox ("Strip personal data … from this
file before it reaches the model"). The first time you tick it in a chat,
a dialog explains the limits of the feature and asks you to confirm
(**I understand, continue**).

When the box is ticked, the extracted text of that file is passed through
the GLiNER2 detector before it is sent, and personal-data spans (names,
e-mail addresses, phone numbers, addresses, dates, identifiers and
similar) are replaced with labels. Details:

- The model (about 500 MB) is downloaded on first use into the data
  folder; a progress bar is shown while it downloads and loads.
- A file once marked stays protected in later turns of the same chat,
  even if the box is not ticked again. The redacted text is cached, so
  later turns do not repeat the detection.
- Passages of a protected document are excluded from knowledge-base
  retrieval, so the unredacted indexed text does not reach the model
  through that path.
- The steps "PII redaction — file (n / total)" and "PII redacted — file"
  appear in the reply.

Limits you must take into account:

- The detector is a zero-shot model. It can miss entities, especially
  uncommon names. It is an aid, not an audited anonymisation.
- Only extracted **text** is redacted. Images, TIFF frames and page
  images of scanned PDFs are sent as they are.
- If redaction fails, the original text is sent and the failure is
  written to the log.
- If the application was built without the `ner-pii` feature, the
  composer warns that the document will be sent unredacted. The desktop
  build in this repository enables the feature.
- Redaction applies whichever model you use, local or cloud.

### 5.4 Replies, steps and citations

Above the text of a reply, the assistant shows the steps it performed:
text extraction ("Extracted text — file (n chars)"), PII redaction,
documents read ("Read file"), searches in a document ("Found … in file"),
workflows applied, running tools ("Running name…" with elapsed seconds),
and cards for generated documents with an open and a download button.
If the model returns reasoning, it is available under a collapsible
**Reasoning** section.

Citations appear as clickable pills in the text. Clicking one opens the
cited document in the viewer on the page and passage quoted by the model.
A citation that refers to a label introduced in an earlier reply of the
same chat is resolved against that earlier reply. While the model writes
its citation list, the reply shows "Gathering sources…".

When a reply is streaming and you scroll up, **Jump to latest** returns to
the end.

A chat is titled automatically with the **Chat titles** model after the
first exchange.

Long conversations are compressed: when the prompt exceeds 80% of the
model's context window, the oldest turns are summarised.

### 5.5 Document viewer

The viewer opens as a side panel with one tab per document (**Close tab**,
**Close all**) and can be collapsed and expanded.

| Format | Viewer |
|---|---|
| PDF | Page view, positioned on the cited page and passage |
| DOCX | Page view ("A4 fit") or reflowed text ("Reflow") |
| XLSX and other spreadsheets | Sheet view |
| MD, RTF, TXT | Text view |
| Other formats | "This file format cannot be previewed. Use Download to open it." |

Every tab has **Download**. For citations, the header shows the quoted
passage and the page.

For DOCX documents the header also shows:

- **Accept** and **Reject**. The decision is per document and reversible.
  **Reject** asks for a reason of at least 10 characters and then
  **Generate summary & reject**: the main model writes a summary of the
  document, and in later turns the model receives the summary and your
  reason instead of the full document. **View summary** shows what
  replaced the document. Switching back to **Accept** restores the
  document; the summary is kept.
- **Open in Word**: opens the file with the application associated with
  `.docx` in Windows. Only files inside the storage folder (or the system
  temporary folder) can be opened this way.

The assistant can generate Word and Excel files and edit documents with
tracked changes through its built-in tools (`generate_docx`,
`generate_xlsx`, `edit_document`, together with `read_document`,
`find_in_document`, `read_workflow`, `list_docx_templates` and
`describe_docx_template`).

### 5.6 Files in this chat

The **Files in this chat** panel lists every document the chat has used,
tagged by origin: **Uploaded**, **Generated**, **Project** (documents of
the chat's project) or **Cited** (knowledge-base documents cited by the
assistant). Rejected documents remain listed, struck through and marked
**Rejected**. Click a row to open it in the viewer.

### 5.7 Instructions and domain

Each turn the assistant receives the base instructions
([`config/system-prompts/base.md`](../config/system-prompts/base.md)) and
a domain prologue from `config/system-prompts/<language>/<domain>.md`.
The domain is the chat's project domain, or your default domain for a
standalone chat. The assistant answers in the language of your message.

---

## 6. Projects

### 6.1 Project list

The **Projects** screen lists projects with name, description (or
creation date) and domain, with a search box and a domain filter. Each row
has **Rename project** (opens the edit dialog), **Export project** and
**Delete project**.

**New project** asks for **Project name**, an optional description and
the domain (pre-selected from your default domain). The same dialog is
used to edit a project.

Deleting a project asks for confirmation. Based on the database schema,
the project's chats and folders are deleted with it, while its documents
and tabular reviews are kept and detached from the project.

### 6.2 Project detail

Click a project to open it. The header has **Export project** and the
**Retrieval scope** selector:

| Retrieval scope | What the project's chats can retrieve from the knowledge base |
|---|---|
| **Shared — global + project** | Indexed documents outside any project plus the project's documents |
| **Strict — project only** | Only the project's documents |

The same rule decides which documents can be added to a tabular review of
the project.

The detail has three tabs.

**Documents.** A folder tree:

- **New folder** at the root, or a subfolder inside a folder; folders can
  be renamed and deleted. Deleting a folder removes its subfolders and
  moves its documents to the project root.
- **Upload files** uploads into the project.
- Drag a document onto a folder (or onto the empty area for the root) to
  move it; folders can be moved the same way.
- Click a document to open it in the viewer; documents can be renamed and
  deleted.

**Chats.** The project's chats and **New chat**. A chat opened from here
shows a back link to the project above the conversation. Every document
of the project is available to its chats.

**Tabular reviews.** The project's reviews and **New review** (see
section 8). A review opened here stays inside the project page.

### 6.3 Exporting and importing projects

Projects can be exchanged between Cove Studio installations as encrypted
`.coveprj` files. Files exported by MikeRust (`.mikeprj`) can still be
imported.

**Export** (from the list or the project detail):

1. Enter the **Recipient email**.
2. Tick **Include chat history** only if the chats should be shared (off
   by default).
3. Click **Export**; the file `<project name>.coveprj` is downloaded.

The file contains the project record (including domain and retrieval
scope), its documents with their folder placement and accept/reject
decisions, the configuration of its tabular reviews (without cell
results), your custom workflows, and optionally the chats.

**Import** (from the list): click **Import project**, or drop the file on
the Projects screen. Enter the e-mail address the sender typed as
recipient and click **Import**. The project is created as a new project.

The encryption key is derived from the recipient e-mail address. Anyone
who has the file and knows that address can open it. This protects
against casual interception only; do not rely on it for confidential
material sent over untrusted channels.

---

## 7. Workflows

### 7.1 Workflow list

The **Workflows** screen shows a table with name, type (**Assistant** or
**Tabular**, with the column count), practice area, domain and source
(built-in or **Myself**). Tabs: **All**, **Built-in**, **Custom**,
**Hidden**. A domain filter narrows the list.

Built-in workflows can be hidden (**Hide**) and shown again from the
**Hidden** tab (**Unhide**). Custom workflows can be deleted.

### 7.2 Creating a workflow

**New workflow** asks for:

- **Workflow name** and type (**Assistant** or **Tabular**);
- **Domain** and **Additional domains** (the workflow also appears in
  those domains' pickers);
- **Practice Area** (optional);
- for an assistant workflow, the **Prompt** (Markdown);
- for a tabular workflow, the columns: each column has a name, a format
  (Free text, Bulleted list, Number, Percentage, Monetary amount,
  Currency, Yes / No, Date, Tags) and a prompt describing what to extract.

### 7.3 Editing a workflow

Click a workflow to open the editor.

- Built-in workflows are **Read-only**; **Duplicate** creates an editable
  copy.
- Custom workflows are saved automatically a moment after each change
  ("Saved").
- **Translate** translates the prompt, or the column prompts, into a
  language you choose, using the model.
- For assistant workflows the prompt has an edit and a preview mode.

### 7.4 Using a workflow

- **Assistant workflows**: attach them in the chat composer. The reply
  shows "Applied workflow …".
- **Tabular workflows**: choose them as the template when creating a
  tabular review.

Built-in workflows are defined in
[`config/workflow-presets/`](../config/workflow-presets/) and column
shortcuts in [`config/column-presets/`](../config/column-presets/); see
[docs/WORKFLOWS.md](WORKFLOWS.md). Custom workflows are stored in the
database.

---

## 8. Tabular reviews

### 8.1 Review list

The **Tabular reviews** screen lists all reviews, including those inside
projects, with column count and creation date, a domain filter, and
**Rename review** and **Delete review** on each row.

**New review** asks for a name, a domain (pre-selected from your default
domain) and a **Workflow Template**; the template list contains the
tabular workflows of the chosen domain, and the review takes its columns
from the template. A review created here does not belong to a project;
reviews inside a project are created from the project detail (6.2).

**Import Excel** accepts `.xlsx`, `.xls`, `.xlsb` and `.ods` files and
creates one review per worksheet: the first row becomes the column
headers, each following row becomes a row with its cells already filled.
The conversion is local and does not call a model. Imported rows are not
linked to documents and are shown by row number.

### 8.2 Working in a review

The toolbar has **Add Documents**, **Clear results**, **Export to Excel**
and **Run** (**Stop** while running).

- **Add Documents** opens a picker that also allows uploading. It lists
  only documents of the review's domain: for a standalone review,
  documents outside any project; for a project review, the documents
  allowed by the project's retrieval scope.
- **Run** fills the cells with the **Tabular review** model (or the main
  model). Cells already completed are skipped. Cells show a spinner while
  generating, the first line of the result when done, or an error icon.
- If a cell fails because of a rate limit (HTTP 429), it shows an
  hourglass and is retried automatically: up to 10 attempts, waiting 5 s,
  10 s, 15 s and so on. The tooltip shows the attempt count. After the
  last attempt the error is shown. **Stop** and a new **Run** cancel the
  scheduled retries.
- Click a cell to see the full result and **Regenerate** it.
- Click a document name to open it in the viewer.
- **Clear results** empties the cells.
- **Export to Excel** downloads the grid as `.xlsx`.

Columns cannot be added or edited inside a review; they come from the
workflow template.

---

## 9. DOCX templates

### 9.1 Template list

The **DOCX templates** screen lists templates with name, domain, the
additional domains and the number of required fields. Tabs: **All**,
**Built-in**, **Custom**, **Hidden**, with search, domain and locale
filters. Each row has **Edit** and **Hide** / **Unhide**.

Built-in templates are loaded from
[`config/docx-templates/`](../config/docx-templates/) at start-up.

### 9.2 Template detail

Click a template to see its document structure and:

- **Apply to chat**: starts a new chat with the template attached in the
  composer.
- **Generate document**: fill in the metadata fields, paste or write the
  **Document body (Markdown)** and click **Generate .docx**. The file is
  downloaded; if placeholders remain unresolved, a warning lists them.

### 9.3 Template editor

**New template**, or **Edit** on a template, opens the full-page editor.
Built-in templates open read-only; **Duplicate** creates an editable copy,
for which you must set a new identifier.

The editor sections are:

| Section | Fields |
|---|---|
| **Identity** | Identifier (lowercase letters, digits, `-` and `_`; becomes the file name), category, display names per language, primary domain, locale, additional domains, placeholder syntax (square brackets, Word DOCPROPERTY or Jinja) |
| **Layout & margins** | Paper size, orientation, paper format (standard or "uso bollo" for notarial deeds, with lines per side, sides per sheet, mirror margins, double-sided, forbid empty lines, marginal signature), margins in cm |
| **Typography** | Body font and size, line spacing, space after paragraph, alignment, first-line indent, footnote font and size |
| **Styles & structure** | Baseline style map and overrides, section numbering (manual or automatic), supported directives, header and footer blocks |
| **Authoring contract** | Sections (id, heading, literal render, guidance, repeating block), required metadata, field prompts, character limits, few-shot examples, additional author notes |

Saving requires a valid identifier, at least one display name and a
category. Custom templates are written as JSON files to
`config/docx-templates/user/` inside the configuration folder the
application uses, not to the data folder; include that folder in your
backups (section 10).

---

## 10. Data locations, backup and migration

### 10.1 The data folder

All user data is under `<home>\cove-studio-data\`, where `<home>` is
`%USERPROFILE%`:

| Path | Content |
|---|---|
| `cove-studio.db` (with `-wal` and `-shm`) | SQLite database in WAL mode: profile and PIN hash, settings and API keys, projects, folders, chats and messages, document metadata and decisions, workflows, reviews and cells, MCP servers, corpus settings, indexed passages and their embeddings |
| `storage\documents\` | Uploaded and generated files |
| `storage\cache\` | Chat attachments keyed by content hash, with their extracted text; `cache\pii\` holds redacted text |
| `fastembed\` | Embedding model |
| `gliner2\` | Personal-data detection model |
| `cove-studio.log` | Application log |

`DATABASE_URL`, `STORAGE_PATH`, `FASTEMBED_CACHE_DIR` and `HF_HOME`
override these locations (see 12.3).

Outside the data folder:

- custom DOCX templates in `config/docx-templates/user/` (section 9.3);
- any configuration file you edited (section 12);
- models installed in Ollama, which Ollama stores in its own folder;
- the theme preference, stored in the webview profile.

### 10.2 Backup

1. Close Cove Studio, so the database and its WAL file are consistent.
2. Copy the whole `cove-studio-data` folder.
3. Copy `config/docx-templates/user/` and any edited configuration files.

The backup contains API keys in clear and your documents: store it
accordingly.

### 10.3 Restore

1. Close Cove Studio.
2. Put the `cove-studio-data` folder back under the home folder of the
   Windows user, replacing the existing one.
3. Put back the user templates and edited configuration files.
4. Start Cove Studio and unlock with the PIN of the restored profile.

Biometric unlock is a flag in the database; on another machine it works
only if Windows Hello is available there.

### 10.4 Migration from MikeRust

On the first start of Cove Studio:

- if `cove-studio-data` does not exist and `mikerust-data` does, the
  folder is renamed;
- if the database is still named `mike.db`, it is renamed to
  `cove-studio.db` together with its `-wal` and `-shm` files.

If a rename fails (for example because a file is in use), the old name is
kept and used, and a warning is written to the log; close any program
using the folder and restart. The embedding and PII model caches are
inside the data folder and move with it.

Other effects of the rename are described in the
[README](../README.md#renaming-from-mikerust): `MRUST_*` variables are
still read, `.mikeprj` files can be imported, local models are migrated
as described in 4.6, and the theme returns to its default because the
webview profile belongs to the new application identifier.

---

## 11. Logs and troubleshooting

### 11.1 Log file

The application writes its log to `<home>\cove-studio-data\cove-studio.log`.
The file is appended to and not rotated. The level is set by the
`RUST_LOG` environment variable (default
`cove_studio=debug,cove_studio_desktop_lib=debug,tower_http=info`).

Useful lines at start-up:

- `[tauri] embedded axum will bind on 127.0.0.1:<port>` and
  `API listening on 127.0.0.1:<port>`;
- `[startup] features compiled in: rag=… pdf=… ner-pii=…`;
- `[env] loaded <path>` when a `.env` file was found;
- `[product] moved …` or `could not move …` for the data folder migration.

The log contains document file names and model names; review it before
sharing it.

### 11.2 Common problems

**"Cannot reach the backend" at start-up.** The embedded backend did not
start or did not report its port. Look for `[tauri] axum server failed:`
in the log; the line contains the cause (for example a database that
cannot be opened, or a port already in use). Click **Retry** or restart
the application. If `PORT` is set in the environment or in a `.env` file,
make sure that port is free.

**The embedding model never finishes loading.** The ONNX Runtime library
(`onnxruntime.dll`) must be exactly the version the application was built
against (currently 1.20.0). A different version does not produce an error:
loading hangs. The installer places the library under
`<install>\resources\libs\onnxruntime\win-<arch>\`. `ORT_DYLIB_PATH`
overrides its location; remove that variable if it points at another
version. See the README section on building for how to check the version.

**Embedding or PII model download.** Both models are downloaded on first
use: the embedding model when a folder is scanned or retrieval first runs,
the PII model the first time a protected file is sent. Network access is
needed once. The banner above the composer and the Data sources section
show the progress. A failure shows "Embedding model failed to load" or
"PII model failed: …" with the cause.

**"Ollama not detected on port 11434."** Secure local mode requires Ollama
running on the same machine on its default port. Install or start
Ollama, then click **Retry**.

**Local model rejected in secure mode.** Errors stating that the local
provider can only point to localhost, or that a model is not in the
local-models allowlist, mean the saved settings refer to a non-loopback
URL or to a model outside `config/local-models/ollama.json`. Select a
catalogue model in **Model roles** or in the composer.

**"Local model not configured".** A `local:` model was selected but no
local base URL is saved; see the note in 4.6.

**Images or scanned PDFs are ignored.** The selected model's name does not
match a vision pattern (4.5). The log shows `attached but selected model
is not vision-capable`.

**MCP tools are not used.** The selected model is not in the tool
allowlist (4.5); the log shows `MCP servers discovered … but NOT shipped`.

**Documents from removed files appear as sources.** Use **Clean orphan
sources** in the document viewer (4.4).

**Indexed documents stuck.** Documents that were being indexed when the
application closed are marked `interrupted` at the next start; re-index
them from their corpus tab.

---

## 12. Configuration files and environment variables

### 12.1 Where the configuration folder is

The application looks for each configuration folder in this order: the
matching environment variable; a `config/` folder in the working
directory or one of its parents; a `config/` folder next to the executable
or one of its parents. The installer copies these folders next to the
executable: `workflow-presets`, `column-presets`, `docx-templates`,
`model.json`, `local-models` and `system-prompts` (not `corpora-plugins`).

Editing files in an installation folder may require administrator rights,
and a reinstallation or upgrade may overwrite them; keep a copy of your
changes.

### 12.2 Editable files

| File or folder | Content | When changes apply |
|---|---|---|
| [`config/system-prompts/base.md`](../config/system-prompts/base.md) | Base instructions of the assistant. If the file is empty or lacks required markers, the built-in copy is used and a warning is logged. | Next message |
| [`config/system-prompts/<lang>/<domain>.md`](../config/system-prompts/) | Domain prologue per language. If missing, the Italian and then the English version is used. | Next message |
| [`config/local-models/ollama.json`](../config/local-models/ollama.json) | Secure-mode catalogue: models, base models, Ollama parameters, previous names. If invalid, the built-in copy is used. | Next use |
| [`config/model.json`](../config/model.json) | Providers, models and regions shown in Settings | Restart |
| [`config/workflow-presets/<domain>/*.json`](../config/workflow-presets/) | Built-in workflows | Restart |
| [`config/column-presets/<domain>/*.json`](../config/column-presets/) | Column shortcuts for tabular workflows | Restart |
| [`config/docx-templates/`](../config/docx-templates/) | Built-in DOCX templates; `user/` holds templates saved from the editor | Built-in: restart. `user/`: immediately |
| [`config/corpora-plugins/*.json`](../config/corpora-plugins/) | Corpus manifests | Restart |
| [`config/corpora.json`](../config/corpora.json) | Size limit for corpus import files | Restart |

A malformed file in a preset folder is skipped with a warning in the log;
the other files still load.

### 12.3 Environment variables

The variables are listed in the [README](../README.md#configuration) and
in [`.env.example`](../.env.example). Every `COVE_` variable is also read
under the former `MRUST_` prefix.

Variables can be set in the Windows environment or in a `.env` file. The
application looks for `.env` in the working directory and its parents,
then in the executable's folder and its parents, and loads the first one
found.

Additional variables not in the README table:

| Variable | Purpose |
|---|---|
| `RUST_LOG` | Log filter (11.1) |
| `VLLM_API_KEY`, `VLLM_MAIN_MODEL`, `VLLM_LIGHT_MODEL` | Legacy local-model fallback used with `VLLM_BASE_URL` |
| `COVE_DOCX_TEMPLATES_DIR`, `COVE_LOCAL_MODELS_DIR` | Alternative locations of the DOCX template and local-model catalogue folders |
