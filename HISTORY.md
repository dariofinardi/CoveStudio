# History

Release notes for Cove Studio, formerly MikeRust. Entries up to v0.7.5
were written under the MikeRust name and describe the project as it was
at that date; statements they make about the relationship with Mike are
superseded by the README section *Provenance and independence from
Mike*. Tagged releases (`v0.1.0` and later) collect
the work into shippable semver bumps; the entries between tags are
ordered by the date the work landed on `main` (Europe/Rome), most recent
first. Each entry follows a light Keep-a-Changelog shape (Added /
Changed / Fixed / Docs / Removed) so contributors can skim by intent.

Commits referenced are short SHAs; run `git log <sha>` for the full
diff. For the relationship with `willchen96/mike` and the record of what was
taken from upstream, see [`docs/UPSTREAM_SYNC.md`](docs/UPSTREAM_SYNC.md).

---

## v0.10.1 — 2026-09-24 (a Word document can be opened, and a form can be handed out)

Two halves of the same job. Cove Studio could write a `.docx` and never
read one back; and the intake pipeline could merge several organisations'
forms into one schema with nowhere to put it.

### Added — `crates/docx-roundtrip`

A crate of its own, publishable separately: open a `.docx`, change it,
write it back **without losing what it does not understand**. A company
template carries text boxes, charts, fields and drawings no editor
models; this keeps each of them as the XML it was and puts it back where
it was, so a rich template survives an editor that understands half of
it.

Reimplemented in Rust from `QuoteDOCX` (a KeelOps plugin by the same
author), whose design it follows — the opaque strategy above all. One
difference: the original generates with a library and then re-opens the
package to swap a marker for each kept fragment, rewriting relationship
ids; writing the OOXML directly means the fragment goes straight to its
place, with no marker that could survive into a delivered document.

What the tests defend, because each was a way to lose a document: OOXML
toggles are tri-state (absent means *inherit*, not *off*); Word's
significant spaces survive; a `{{placeholder}}` split across runs — which
is what Word writes after any edit near the braces — is still found; an
unfilled placeholder is still a placeholder after a save; a tracked
deletion is not quietly accepted; two writes of the same document produce
the same bytes.

### Added — the collection form, as three artefacts

* **HTML**, self-contained, for the client to fill in: sticky section
  index with per-section progress, conditional details that appear on a
  "yes", amounts as text rather than number inputs (a number input
  refuses `1.200,50` or reinterprets it), a draft kept in the browser and
  nowhere else, and a banner saying in plain words that filling the form
  sends nothing — they generate a summary, copy it, and paste it into
  their reply. No native browser dialogs anywhere: in a sandboxed frame
  they are silently blocked, so clearing the form asks for a second click
  instead.
* **XLSX** both ways: the sheet handed out is the sheet read back, keyed
  on the field id, so a client who reorders rows or adds a column of
  their own notes still sends something readable.
* **DOCX** per recipient: each organisation's own questions in its own
  order with the answers filled in, plus a gaps document for the broker —
  what is missing and who requires it, and answers that say nothing
  (`test`, `da definire`) flagged rather than corrected. A questionnaire
  is a declaration, and the signature is the client's.

### Notes

* Verified: 725 tests in the workspace. The three generators are checked
  by **reading their output back** — the DOCX through the new crate, the
  XLSX through calamine — not by asserting on the bytes we just wrote.
* No model is called anywhere in the suite; the live pipeline test stays
  ignored by default.
* Plan and open risks: `docs/piano-editor-docx.md`.

---

## v0.10.0 — 2026-09-23 (asking a model for data, and checking what comes back)

Cove Studio could only ask models for prose. Everything it needed as
data — a table cell, a list of questions read out of a form — was a
free-text answer parsed on trust, with the expected shape written in the
prompt and hoped for. This release adds the contract, and the checks
that make an answer usable as data.

### Added

* **Structured output across every provider.** A request can carry a
  JSON Schema; Gemini takes it as `responseSchema`, Mistral and the
  OpenAI-compatible endpoints as a strict `response_format`, and Claude
  — which has no JSON mode — by being forced to call a tool whose
  arguments *are* the answer. `llm::structured::complete_json` returns a
  parsed value or an error naming what arrived instead.
* **`intake`**, the deterministic half of a new pipeline: read several
  organisations' forms, merge them into one, collect the answers once.
  It normalises what a model returns — duplicate questions, printed
  numbering, empty labels — and judges whether an extraction is
  plausible at all.
* **Three insurance prompts** (`config/workflow-presets/insurance/qst-*`)
  with the schema their answer must satisfy, for reading an underwriting
  questionnaire, merging several into one client form with provenance
  per insurer, and reporting what is missing or inconsistent before the
  broker sends anything. Workflow presets accept a new `extraction`
  kind for this.

### Fixed

* **The library's PDF fallback had never run.** The onboarding boundary
  maps the parser library's configuration onto ours, except for the one
  that mattered: the library looks for pdfium in `PAGEINDEX_PDFIUM_DIR`
  and we ship it elsewhere, so a PDF that needed the fallback failed
  with "pdfium cannot open the file" — which was not true, pdfium was
  never found. An AIG questionnaire encrypted with an *empty* password
  (readable by any viewer) was unreadable here because of it.
* **A PDF read in part passed for a PDF read.** On a 16-page
  questionnaire the parser returned 6.6k characters starting at page 15
  and reported success; pdfium returned all 16 pages and 44k characters.
  The boundary now checks how many pages the section tree accounts for
  and re-reads with pdfium when most of the document is missing, keeping
  whichever text is fuller.
* **Gemini's `finishReason` was never read**, so an answer cut off at
  the token budget came back as a complete one — well-formed, because a
  schema makes the model close the JSON early. Structured calls now ask
  for room (32k output tokens) and refuse anything that did not end
  cleanly.

### Notes

* Measured on three real underwriting questionnaires: the same model on
  the same document returned 118 questions on one run and 3 on the next,
  both valid. `intake` derives a floor from the document's own text
  (question marks, fill-in rules, label lines), retries below it, and
  fails loudly rather than handing on a form with most of it missing.
  When the whole-document read keeps failing it reads the document in
  pieces instead — which is slower and slightly less complete, so it is
  the safety net rather than the default.
* Verified: 628 Rust tests. The live pipeline test
  (`tests/qst_pipeline_live.rs`) is ignored by default: it needs an API
  key and the questionnaires, which stay outside the repository.

---

## v0.9.3 — 2026-09-23 (one funnel, and the verdict reaches the reader)

v0.9.1 built the funnel; this release makes everything use it and makes
the answer visible. The three extractors are gone: the folder scanner,
the chat attachment loader and the assistant's own tools all read
through `crate::ingest` now. The worst of the three was the tools' —
every failure became `unwrap_or_default()`, so `read_document` on a
password-protected PDF returned an empty string and the model could not
tell "this document is empty" from "this document could not be read".

### Added

* **The prompt states the verdict.** An attachment that yielded no
  usable text appears in the prompt under its own `doc-N` label with a
  sentence saying so and what to suggest — turn OCR on for a scan, ask
  for an unprotected copy of an encrypted PDF. The same applies to an
  image attached while the selected model cannot read images, which used
  to produce nothing but a line in the log.
* **The composer says it too**, in all six languages: a warning toast at
  upload time — while the user still has the file in hand and can act —
  and a marked chip for as long as the attachment is there. The stored
  value is a canonical code (`scanned_pdf`, `encrypted_pdf`, …); the
  interface translates it, and an unknown code still produces a true
  sentence rather than silence.
* **CSV, `.doc`, `.ppt`, `.pps` and `.xlsm`** are readable and declared
  everywhere they matter: the file picker, the upload classifier and the
  synced-folder extensions. `.doc` was previously offered in the picker
  while nothing could read it, and `.ppt` was not offered at all.

### Fixed

* **A scanned PDF was reported as readable.** The library returns a
  single section titled "PDF (nessun testo estraibile)" for a PDF with
  no text layer; counted as a heading, it declared the scan `ready` and
  handed the model that sentence as though it were the document's
  content. Found with a real scanned fixture, not by reading the code.
* Plain text arrived with its first line twice (the parser uses it as
  both the section title and the body), and CSV arrived under a heading
  called "Introduzione" that no author wrote. Both are suppressed; the
  structure keeps them, the text the model reads does not.
* `.csv` is unknown to the library's format detector, so it came back
  "unsupported" — it is read as plain text in the boundary, where our
  conventions belong. Reported upstream (pageindex-rs#1).

### Notes

* Verified: 544 unit tests, 18 functional tests over real files
  (including a scan, a legacy `.doc` and a legacy `.ppt`), 9 tests of the
  chat attachment loader — which had no coverage at all before this
  release — 7 upload tests, and 64 frontend tests.
* Fixtures for the scanned and legacy-Office cases now ship in
  `tests/fixtures/` with a README explaining what each one defends.
* A tampered-text-layer pre-check runs on PDF and DOCX through
  `chk_defaced`. It catches altered font tables; a PDF whose text layer
  is simply unusable garbage is a different case, and the answer to that
  one is OCR — deliberately not approximated with a threshold, because
  the library's own authors document why an absolute plausibility
  threshold misfires on table-heavy documents.

---

## v0.9.2 — 2026-09-18 (a document says whether it could be read)

Uploads used to be stored as `ready` whatever extraction produced. A
password-protected PDF, an image-only DOCX, a file whose extension
lied: the row said ready, the prompt received an empty block, and the
assistant answered with confidence about a document nobody had read.
The only trace was a line in the server log.

### Added

* `documents.status` now carries the verdict — `ready`, `no_text` or
  `failed` — and `extraction_reason` (migration 0036) the sentence that
  explains it, phrased for the person who attached the file rather than
  for the log. `POST /document` returns both, so the composer can say
  so immediately.
* A protected PDF is recognised as such and told what to do about it;
  an unsupported format, a corrupt container and an unreadable file are
  distinguished rather than collapsed into silence. An unrecognised
  failure keeps the original message: a technical sentence the user can
  paste into a report beats a tidy one that hides which file broke.

### Fixed

* **The cache key collided with itself.** For a `.txt` or `.md` upload
  the binary and the extracted text resolved to the *same* path
  (`cache/<hash>.txt`), so writing the binary made the text look
  already-extracted and the extraction step was skipped. Harmless while
  every row said `ready`; it surfaced the moment the verdict became
  real. The text now lands in `cache/<hash>.extracted.txt`, and older
  caches stay readable because every document carries its own
  `extracted_text_path`.
* Re-uploading content that was already extracted took the verdict on
  trust. It is now read back from the cached text: an empty extraction
  meant the file was unreadable then and still is.

### Notes

* Verified: 5 functional tests on `POST /document` (ready, no_text with
  a reason, failed, unsupported, and a real PDF whose cached text keeps
  its page markers), plus the full suite green in parallel — 535 unit
  tests and every integration suite.
* The tests for this route hold a lock while they run: they set
  `STORAGE_PATH`, which is process-global, and in parallel they wrote
  into each other's temporary directory.

---

## v0.9.1 — 2026-09-18 (one funnel for document onboarding)

Every file now enters Cove Studio through a single boundary,
`src/ingest/`, built on **`pageindex-rs`** (Apache-2.0). It replaces
three separate extractors keyed on different things and failing three
different ways: one returned a skip reason, one discarded the error, one
turned it into an empty string.

### Added

* **Format detection and extraction** for PDF (native text layer, with
  a pdfium fallback), DOCX, Markdown, plain text, and — new — the
  Office and legacy family: `.doc`, `.ppt`, `.rtf` and spreadsheets,
  converted through the library. `.doc` and `.ppt` were silently
  unsupported before.
* The **defaced pre-check**: a PDF whose text layer disagrees with what
  is printed (tampered `cmap`/`ToUnicode`, private-use or zero-width
  characters). For legal documents the extracted text saying something
  the reader never sees is a real risk, not a curiosity.
* A **section tree with page numbers**, from which the `[Page N]`
  markers are rendered. The markers now derive from structure instead
  of being pasted onto the text, and a test asserts they are monotonic
  and never duplicated — nothing guaranteed that before, and a citation
  could point at the wrong page unnoticed.

### Notes

* The library's archive (DuckDB with hybrid retrieval) is deliberately
  left out with `default-features = false`: vectors live in sqlite-vec.
  That split was made upstream for this integration.
* The boundary is the only place that knows the library: it maps its
  `PAGEINDEX_*` variables and working-directory model paths onto Cove
  Studio's own conventions, and absorbs its vocabulary — the library
  reports every spreadsheet as `sheet`, while `documents.file_type`
  distinguishes `xlsx` from `xls`, `xlsb` and `ods`, and the viewer and
  the tabular workflows branch on it.
* Dependency debug info is off in the dev profile: line tables across
  ~400 crates were enough to exceed the MSVC `.pdb` module limit
  (`LNK1140`). Our own crates keep theirs.
* Verified: 16 unit tests on the boundary, 13 functional tests on real
  files — the three PDFs in `tests/medical`, a DOCX written by Cove
  Studio's own writer, a spreadsheet, an RTF, Markdown, plain text, an
  empty file, a corrupt one, an unsupported one, a missing one.

---

## v0.9.0 — 2026-09-18 (one ONNX Runtime: ort rc.13, and PII rebuilt on gliner2-rs)

The process can host exactly one ONNX Runtime, and three things now want
it: the embedding model, the redesigned GLiNER2 crate and — next — the
OCR pipeline for scanned PDFs. This release moves the whole stack onto
the version they agree on.

### Changed — inference stack

* `ort` 2.0.0-rc.9 → **=2.0.0-rc.13**, `fastembed` 4.9.1 → **=5.17.4**
  (which declares `ort/api-24`), vendored `onnxruntime.dll` 1.20.0 →
  **1.28.2** for both Windows architectures. Release candidates 10 to 12
  were rejected upstream because models hung during session
  initialisation on ARM; rc.13 is the first since rc.9 that runs them,
  and it was validated on win-arm64 before this move.
* `ort::init().commit()` answers with a `bool` now (`false` = an
  environment was already installed), not a `Result`; the previous code
  logged that as an error.
* The execution-provider module was renamed (`execution_providers` →
  `ep`) and every type lost its suffix: `DirectMLExecutionProvider` →
  `DirectML` and fifteen others.

### Fixed

* **Windows ships its own `onnxruntime.dll`** (ORT 1.17.x, part of
  Windows ML, in System32). With `ORT_DYLIB_PATH` unset, `ort` loaded
  *that* — which is what the "silent deadlock" of earlier releases
  actually was. The variable was only set at application startup, so
  tests and any other entry point took the system copy. It is now
  resolved lazily on first model load as well, once per process, and
  rc.13 reports a mismatch instead of hanging.

### Changed — PII protection

* Rebuilt on **`gliner2-rs` 0.9.6** (Apache-2.0, from crates.io),
  replacing `gliner2_inference` 0.5.1. The engine lives on a dedicated
  thread because `SpanEngine` is not `Send`, which also serialises
  inference and keeps a panic in the model from poisoning a shared
  lock. The 1.1 GB of weights already on disk are reused as they are.
* The label vocabulary is the model's own, asked one group at a time,
  instead of eighteen hand-written labels; `codice fiscale`,
  `partita IVA`, `targa` and `patient name` stay as an extra group.
  Chunking is the library's, in words, with the seam de-duplication it
  ships.
* Redaction keeps **every literal occurrence** of a detected value,
  not only the spans the model marked: the library's offset-accurate
  pass runs first, then ours. A name caught once is not left in clear
  text three paragraphs later.
* **Masking caution** is now a slider in Settings → Sicurezza
  (`user_settings.pii_threshold`, migration 0035), labelled by effect
  rather than by number. The default is the model's calibrated 0.5:
  the previous hard-coded 0.2 was tuned against an engine whose prompt
  layout was defective, so its scores do not transfer. A document
  already redacted keeps its masking until it is processed again; the
  cache wins over the slider, and the log says so.

### Notes

* Verified: 535 unit tests, 11 real-inference embedding tests, 11
  real-model PII tests (masking, repetition, threshold monotonicity,
  offsets, chunk seams, multibyte text, clean text untouched), 8
  functional tests on the threshold endpoint, 55 frontend tests.

---

## v0.8.0 — 2026-09-17 (Cove Studio, renamed from MikeRust)

MikeRust is renamed **Cove Studio**. The maintainer is unchanged (Dario
Finardi); the repository moved from the `SemplificaAI` organisation to
[github.com/dariofinardi/CoveStudio](https://github.com/dariofinardi/CoveStudio)
and the project is managed directly by its maintainer. The minor bump
marks the new identity plus the features below; installations upgrade in
place, keeping their data.

### Changed — identity

* Product name, desktop identifier (`app.covestudio`), window title,
  crates (`cove-studio`, `cove-studio-desktop`), executable
  (`cove-studio`), log file (`cove-studio.log`), user agent
  (`covestudio/<version>`), environment variable prefix (`COVE_`).
* All identity values live in `src/product.rs` and
  `frontend/src/lib/product.ts`; translations use the `{product}` and
  `{projectExt}` placeholders.
* Project archives are written as `.coveprj` with magic `COVEPRJ\0`
  (module renamed `project_archive`).
* Local secure-mode models are named `covestudio-…-fast`; the context
  profile aliases are `covestudio-*:ctx*`.
* The Windows installer keeps the MikeRust upgrade code, so it replaces an
  existing MikeRust installation.
* Copyright headers now read "Copyright (c) 2026 Dario Finardi"; the
  Semplifica logo asset and brand references were removed.

### Added — migration of existing installations

* On first start `mikerust-data/` is moved to `cove-studio-data/`,
  `mike.db` (with `-wal` / `-shm`) to `cove-studio.db`, and the embedding
  and GLiNER2 model caches to their new folder names, without downloads.
* `MRUST_*` environment variables are still read; `.mikeprj` files and the
  `MIKEPRJ\0` magic are still accepted on import; interface preferences
  saved under `mikerust.*` keys are moved to `covestudio.*`.
* Ollama models installed under previous names are copied to the new
  names (`POST /user/local-secure/migrate`) and the settings that used
  them are updated; removing the previous names
  (`POST /user/local-secure/remove-legacy`) or reinstalling a model that
  could not be copied happens only after the user confirms in Settings.

### Changed — independence from Mike

* The assistant's base instructions were rewritten from scratch in
  Italian, with a reply-in-the-user's-language rule first, and moved to
  `config/system-prompts/base.md` (validated against the tokens the
  citation parser needs, embedded copy as fallback). They were previously
  adapted from Mike's `SYSTEM_PROMPT`.
* The project-document rename endpoint, ported from upstream commit
  `f39f175` on 13 May 2026, was rewritten: the extension now follows the
  stored file type and names are sanitised.
* The local-models catalogue moved out of the code into
  `config/local-models/ollama.json`.
* README, NOTICE, `docs/UPSTREAM_SYNC.md` and the frontend licence files
  now state precisely what is original and what still derives from Mike
  (built-in tool schemas, legal-domain presets, some interface strings).

### Changed — chat pipeline

* `src/routes/chat.rs` split into `chat/prompts.rs` (context blocks and
  service messages) and `chat/citation_resolution.rs`; fewer database
  queries per turn (citation indexes built once and only when the reply
  has citations), request parsed in a single pass, independent queries
  run concurrently.
* User-visible service notes (model without tool support, empty answer,
  tool-loop limit) follow the interface language.

### Added — long documents and context window

* Context window resolved per model before the prompt is assembled: for
  local Ollama servers from the server itself (`/api/ps` `context_length`,
  `/api/show` `num_ctx` and `<arch>.context_length`), otherwise from the
  model catalogue, with the value learnt back from an overflow error
  (`src/llm/context_window.rs`). `GET /models/context-window` exposes it
  and Settings shows it under each model picker.
* Oversized attachments are no longer sent whole: each document gets a
  share of the window proportional to its size, and a document over its
  share is cut to the segments that match the question, ranked with BM25
  (`src/routes/chat/attachment_budget.rs`, `src/document_segments.rs`).
  The reply says which documents were excerpted.
* `read_document` reads by page: `page_from` / `page_to` with the totals
  and where to continue, so the model can walk a long document instead of
  losing its tail.
* Gemini 3.7 Flash and Gemini 3.8 Flash in the model catalogue
  (1,048,576 input / 65,536 output tokens); the context-window table
  covers the whole `gemini-3.x` family.

### Fixed

* A document generated during a turn now receives a `doc-N` label, so the
  model can cite it and the citation resolves to the real document. Its
  citations used to carry an unresolved label, which made the viewer
  request `/document/doc-1/display`, get a 404 and report a removed
  source. The interface no longer treats a `doc-N` handle as a fetchable
  id either.
* `edit_document` results produce a download card, so an edited document
  is reachable without regenerating it; a document touched more than once
  in a turn gets a single card.
* Tool-result JSON that a model pastes into its own answer is stripped
  after the stream (`src/routes/chat/tool_echo.rs`): the download card
  carries that information, and a JSON snippet the user actually asked
  for is left alone. The base instructions forbid the echo as well.
* `find_in_document` could slice a UTF-8 boundary and panic on
  multi-byte text.
* Project documents listed in the prompt were numbered one lower than the
  labels resolved by the tools.
* The Gemma local-model derivation no longer forces Italian replies.

### Changed — dependencies

* SheetJS is vendored as `frontend/vendor/xlsx-0.20.3.tgz`, the official
  tarball: the npm copy stopped at 0.18.5, which carries the prototype
  pollution (GHSA-4r6h-8v6p-xvw6) and ReDoS (GHSA-5pgg-2g8v-p4x9)
  advisories with no npm release to upgrade to.
* npm dependencies updated within their ranges (postcss, js-yaml,
  undici, brace-expansion, dompurify, postcss-selector-parser, vitest,
  svelte, vite, typescript): `pnpm audit` reports no known
  vulnerabilities. `serde_with` 3.20 → 3.22 (GHSA-7gcf-g7xr-8hxj).
* Still open: `thrift` 0.17 via `parquet` 53 (GHSA-2f9f-gq7v-9h6m);
  `parquet` drops thrift only from version 59, which is a major upgrade
  of the arrow stack.

---

## v0.7.5 — 2026-06-14 (cross-dominio nell'editor dei workflow)

> Renumbered from v0.7.4 — a v0.7.4 release already exists on GitHub;
> this work ships as v0.7.5 to avoid the clash.

Extends the `also_applicable_to` cross-domain mechanism — shipped for
built-in presets in v0.7.3 — to **user-created** workflows, so the
workflow/tabular editor can tag any workflow as applicable to more
than one sector via a free-text tag picker.

### Added

* **`DomainPicker.svelte`** — a tag-style input with free-text
  autocomplete over the available domains: chips for the chosen extra
  domains (each removable), a filtered suggestion dropdown, arrow-key
  navigation, Enter to add, Backspace to remove the last. The primary
  domain is excluded from suggestions. Wired into both the create modal
  (`WorkflowModal.svelte`) and the auto-saving editor
  (`WorkflowEditor.svelte`); the editor also renders the extra domains
  as badges in read-only (preset) view.
* `also_applicable_to` on user workflows (migration **0034**): new
  `TEXT NOT NULL DEFAULT '[]'` column on `workflows`. Existing rows
  default to single-domain — no behaviour change on upgrade.
* 3 i18n keys (`Workflows.alsoApplicableTo*`) across all 6 locales.

### Changed

* `src/routes/workflows.rs`: `also_applicable_to` flows through
  SELECT / INSERT / UPDATE. Create and update **sanitise** the list
  server-side (canonical domains only, drop any redundant primary,
  dedup). The `GET /workflow?domain=` filter now matches a row when
  the target equals the primary `domain` **or** appears in its extra
  domains — moved from the SQL `WHERE` into Rust to mirror the preset
  `matches_domain` path.
* `docs/WORKFLOWS.md`: the `also_applicable_to` row no longer claims
  user workflows are single-domain; documents the editor tag picker.

### Notes

* Verified: `svelte-check` 0 errors · 42/42 preset-loader tests ·
  `cargo check` clean.

---

## v0.7.3 — 2026-06-13 (workflow cross-dominio + analisi cespiti e libri contabili)

Adds two commercialista workflows — fixed-asset analysis and
bookkeeping quadrature — and the **cross-domain registration**
mechanism the user asked for so a single preset can surface in more
than one sector's picker.

### Cross-domain preset support (`also_applicable_to`)

`WorkflowPreset` gains an optional `also_applicable_to: Vec<String>`
field, mirroring the DOCX-template mechanism of the same name. A
preset relevant to more than one vertical is registered **once** and
listed in every applicable domain's picker — no duplicate JSON.

* `src/presets/workflow.rs`: new field + `matches_domain(target)`
  method (primary `domain` OR any `also_applicable_to` entry, `None`
  ⇒ always). The loader sanitises the list — drops non-canonical
  entries and any redundant listing of the primary domain (warn,
  don't kill the preset). `to_api_json` exposes the field.
* `src/routes/workflows.rs`: the in-memory preset filter now uses
  `preset.matches_domain(Some(d))` instead of exact `domain == d`,
  so cross-domain presets slice into both pickers. User-created DB
  workflows stay single-domain (the column filter is unchanged).
* 2 new unit tests: `matches_domain_primary_also_and_none` and
  `shipped_cross_domain_commercialista_presets_in_both_pickers`.

### Two new workflows (both cross-domain `fiscale` ⇄ `finance`)

Grounded in authoritative sources (art. 16 DPR 600/73, art. 102/86
TUIR, DM 31/12/1988 coefficients, art. 5 D.Lgs. 446/97; verified
against current rules — super/iper-ammortamento is historical,
replaced by the Transizione 4.0/5.0 credito d'imposta; manutenzioni
plafond 5% per art. 102 c.6):

* **`builtin-fiscale-analisi-cespiti`** (tabular, primary `fiscale`,
  also `finance`) — fixed-asset register analysis: register↔bilancio
  reconciliation, amortisation deductibility, fiscal excess (→ IRES
  variation), movements + plus/minusvalenze. One row per asset/category.
* **`builtin-finance-controlli-libri-contabili`** (tabular, primary
  `finance`, also `fiscale`) — bookkeeping quadrature → anomaly
  register: double-entry balance, numbering progressivity, key-account
  balances (cassa never negative, bank reconciled), VAT registers ↔
  LIPE ↔ annual return, ritenute ↔ F24, customer/supplier squaring,
  year-end adjustments. One row per anomaly.

Both appear in the Fiscale **and** Finance workflow pickers.
docs/WORKFLOWS.md §2 documents the `also_applicable_to` field;
docs/piano_settore_fiscale.md §3 notes the cross-domain pair.

42/42 preset-loader tests green; no schema migration.

---

## v0.7.2 — 2026-06-13 (settore Fiscale — analisi del bilancio ai fini fiscali)

Fills a gap reported during testing: the Fiscale sector had no
balance-sheet workflow, forcing the user to reach into the
`finance` sector's "Riclassificazione bilanci" to analyse a bilancio.
Balance-sheet analysis legitimately exists in two sectors with
different, complementary angles — finance does the managerial/
valuation reading (reclassification, ratios, valuation, crisis);
fiscale needed the **tax reading** (civilistic result → taxable
income, IRAP base, deferred tax). v0.7.2 adds that reading.

### Three new `fiscale` workflows

* **`analisi-fiscale-bilancio`** (assistant) — reads the bilancio for
  tax purposes per the derivation principle (art. 83 TUIR): civilistic
  pre-tax result, the tax-relevant line items (ammortamenti art. 102,
  svalutazione crediti art. 106, spese di rappresentanza art. 108,
  interessi passivi/ROL art. 96, compensi amministratori art. 95 c.5,
  auto art. 164, plus/minusvalenze artt. 86-87, IMU/IRAP art. 99),
  an IRES estimate, an IRAP-base note, and deferred/anticipated tax
  (OIC 25) — with the mandatory professional disclaimer.
* **`riconciliazione-civilistico-fiscale`** (tabular) — Quadro RF-style:
  one row per variazione in aumento/diminuzione from the civilistic
  result to taxable income, with the TUIR article, amount, sign, and
  the RF rigo where it lands.
* **`base-imponibile-irap`** (tabular) — IRAP taxable base from the
  CE per art. 5 D.Lgs. 446/97: value-of-production components, the
  excluded cost lines (personnel B.9, write-downs B.10.d, provisions
  B.12-B.13), and the spettanti deductions (cuneo fiscale, deduzione
  integrale dipendenti a tempo indeterminato L. 190/2014).

The Fiscale sector now ships **11 workflows** (4 assistant + 7
tabular). `docs/piano_settore_fiscale.md` §3 updated with the new
entries and a note on the fiscal-vs-financial bilancio angle.

40/40 preset-loader tests green; no schema migration.

---

## v0.7.1 — 2026-06-09 (localizzazione italiana del verticale legale)

Translates the **legal vertical** workflow + column presets from
English to Italian, for the Italian-market product. The legal
presets were the last English-language content in the catalogue
(inherited from the upstream `willchen96/mike` international
templates); medical, finance, fiscale, insurance, PA and machinery-
compliance were already authored in Italian.

### Translated (27 files, all `domain: legal`)

* **14 workflow presets** (`config/workflow-presets/legal/`):
  coc-dd, commercial-agreement, commercial-lease, cp-checklist,
  credit-agreement, credit-summary, ediscovery, employment-agreement,
  lpa, nda, sha-summary, shareholder-agreement, spa, supply-agreement.
* **13 column presets** (`config/column-presets/legal/`):
  amendment, assignment, change-of-control, confidentiality,
  effective-date, force-majeure, governing-law, indemnity, parties,
  payment-fees, term, termination, warranties.

### What changed and what didn't

Translated to fluent Italian legal register: `title`, `practice`,
`prompt_md`, every column `name` + `prompt`, and `tags` DISPLAY
values (e.g. NDA `["Mutual","Unilateral"]` → `["Reciproco",
"Unilaterale"]`; employment `["Full Time","Part Time"]` →
`["Tempo pieno","Tempo parziale"]`).

Left byte-identical (canonical, never translated): `id`
(`builtin-nda` etc.), `type`, `domain`, every column `index` and
`format`, and the column-preset `match_pattern` / `match_flags`
(the regex is a matcher, not display text — it stays bilingual).

Jurisdiction-specific concepts were adapted to the Italian legal
system rather than translated literally where it mattered: e.g.
commercial-lease "security of tenure" → "tutela della stabilità del
rapporto (L. 392/1978)" replacing the England & Wales LTA 1954
reference; rent indexation generalised to ISTAT; "privileged" →
"coperto da segreto professionale". PE-fund terms of art that
Italian practitioners use untranslated (General Partner, carried
interest, hurdle, clawback, waterfall, locked box) were preserved
in the LPA preset; market terms (SOFR, EURIBOR, make-whole) kept in
the credit presets.

### Standby TODO

Integration of public Italian case-law / norm databases
(def.finanze.it, Sentenze Web Cassazione, Giustizia Tributaria DGT)
as in-chat consultable corpora is **explicitly parked** at the
user's request — annotated in `docs/piano_settore_fiscale.md` §6
+ roadmap Fase 3. It needs a per-source `LegalCorpusAdapter`, not a
config drop-in; deferred to a dedicated future workstream.

### Tests

`shipped_workflow_presets_all_load_cleanly` + the full preset-loader
suite (40 tests) stay green — every translated preset parses and
keeps a valid `domain` / `format`. No code change, no schema
migration.

---

## v0.7.0 — 2026-06-09 (nuovo settore «Fiscale» — tax/commercialista italiano)

Adds a twelfth professional vertical, **`fiscale`** (Italian tax /
commercialista compliance + advisory), end-to-end. It sits next to
`finance`: finance holds the analytical side (bilanci, valutazione
d'azienda, crisi d'impresa), fiscale the tax-compliance + advisory
side (IVA, dichiarazioni, imposte dirette/indirette, ravvedimento,
accertamento, contenzioso tributario).

### Domain registration

* `src/domain.rs` — `"fiscale"` added to `DOMAINS` (after
  `finance`). No migration needed; validation is API-boundary only.
* `frontend/src/lib/types/domain.ts` — `'fiscale'` added to the
  mirror `DOMAINS` const.
* All six locales gain `Domains.values.fiscale`: IT «Fiscale»,
  EN «Tax», FR «Fiscalité», DE «Steuern», ES/PT «Fiscal».

### System prompts (6 languages)

`config/system-prompts/{it,en,fr,de,es,pt}/fiscale.md`. The Italian
one is the richest; the others are operational translations that
keep the Italian statutory references (the subject matter is
national). All six bake in the **2024 tax-reform updates** so the
model never cites superseded institutes:

* Reclamo-mediazione (art. 17-bis D.Lgs. 546/92) **repealed** from
  4 Jan 2024 (D.Lgs. 220/2023) — appeals go straight to the Corte
  di Giustizia Tributaria di primo grado.
* New penalty regime **D.Lgs. 87/2024 from 1 Sep 2024** — omitted/
  late payment base penalty 30% → 25%; the model must check the
  violation date and state which regime it applies.
* Generalised prior adversarial procedure (art. 6-bis L. 212/2000,
  D.Lgs. 219/2023).

### Workflow presets — `config/workflow-presets/fiscale/` (8)

Assistant: `parere-tributario`, `ravvedimento-operoso` (calcolo con
regime D.Lgs. 87/2024 + interessi legali pro-rata + codici tributo
F24), `analisi-avviso-accertamento` (termini, vizi, confronto
adesione/autotutela/ricorso post-abrogazione reclamo).

Tabular: `riconciliazione-iva` (registri vs LIPE vs F24),
`verifica-forfettario` (requisiti L. 190/2014 art. 1 c. 54-89),
`quadro-rw-monitoraggio` (IVIE/IVAFE su attività estere),
`imposte-indirette-atto` (registro/bollo/ipo-catastali, DPR 131/86),
`scadenzario-versamenti-f24`.

### Column presets — `config/column-presets/fiscale/` (9)

`imponibile`, `aliquota`, `imposta-dovuta`, `ritenuta`, `sanzione`
(con avviso sul nuovo regime D.Lgs. 87/2024), `interessi` (tasso
legale pro-rata annuo), `norma-riferimento`, `scadenza`,
`codice-tributo`.

### Docs

* New `docs/piano_settore_fiscale.md` — descriptive sector plan:
  scope, distinction from finance, 2024 reform notes, the 8
  workflows + 9 columns, **public/open databases for norms and
  case-law** (Normattiva, def.finanze.it DEF, Sentenze Web
  Cassazione, Giustizia Tributaria DGT, EUR-Lex), and a 4-phase
  enrichment roadmap.
* `docs/WORKFLOWS.md` domain references updated from the stale "9
  domains" to the accurate 12, with `fiscale` flagged as the worked
  example of the add-a-domain flow.

### Tests

No new tests authored, but the existing preset-loader suite is the
gate: `shipped_workflow_presets_all_load_cleanly` (40 preset tests)
parses every shipped workflow including the 8 new fiscale ones and
would reject an invalid `domain` — all green. `cargo check` clean,
svelte-check 0 errors.

---

## v0.6.7 — 2026-06-08 (sidebar active-domain selector)

Adds a compact domain picker next to the MikeRust brand at the
top of the sidebar. Always visible, default-initialised from the
user's `default_domain` setting (set at sign-up time and
configurable in Settings → Generale), persisted server-side on
every change via the existing `setDefaultDomain` user-store
action.

### Why

The default domain is the global signal that drives every
domain-scoped UI:

  * The workflow + template pickers in the chat composer default
    to filtering by it.
  * "Nuovo progetto" / "Nuova revisione tabellare" pre-populate
    their domain dropdown to it.
  * The chat composer inherits it on chats that aren't inside a
    project.

Pre-v0.6.7 the only way to change the active domain was to dig
into Settings → Generale → Dominio predefinito + save. Most
real-world workflows (a lawyer who works across "Assicurazioni"
in the morning and "Compliance" in the afternoon) hit this knob
many times a day; making it a single click on the sidebar brand
takes a 6-click flow down to one.

### Implementation

* `Shell.svelte` brand snippet rebuilt as a flex row: Logo +
  "MikeRust" + version + `<Select>` aligned right (`ml-auto`,
  `max-w-[10rem]`).
* Options derived from `userStore.effectiveEnabledDomains` so
  domains the user has hidden in Settings → Generale stay
  hidden in the sidebar too.
* `onSidebarDomainChange(next)` calls `userStore.setDefaultDomain`
  (PUT /user/default-domain) on every change; reverts the
  visible select on a save failure by virtue of reading from
  the live `userStore.defaultDomain` getter.
* Single new i18n key `Sidebar.activeDomainTooltip` localised
  in all six locales (it / en / fr / de / es / pt).

No schema migration; no backend change; svelte-check 0 errors.

---

## v0.6.6 — 2026-06-08 (hotfix — Mistral profile picker auto-saves on click)

The Mistral profile picker added in v0.5.6 (Equilibrato / Premium)
and extended in v0.6.5 (Veloce) only mutated the local form state.
Persistence required the user to click "Salva modifiche" at the
bottom of the section — confusing because the highlight on the
picked profile *suggested* the choice was committed, but a
re-entry to Settings (or a fresh chat turn) would still hit the
previously-saved role models.

v0.6.6 turns `applyMistralProfile` into an async function that
immediately persists the three role assignments via
`modelsStore.save({ main_model, title_model, tabular_model })`
before showing the success toast. On a save failure (network drop,
backend 5xx) the form reverts to the previous snapshot so the
"Attivo" badge keeps pointing at what's actually persisted — no
lying about the state.

Side benefit: the existing `activeMistralProfile` derived state
already reads `form.main_model` etc., which on next component
mount initialises from `modelsStore.settings`. So with persistence
now happening on every click, the picker correctly highlights the
last-chosen profile when the user re-enters
Settings → Modelli LLM — even after a full app restart, because
the settings are read from the DB via GET /llm-settings on mount.

Single behaviour change, no schema migration, no API contract
change. svelte-check 0 errors. The three onclick handlers in the
profile picker grid now use `void applyMistralProfile(...)` to
acknowledge the unawaited promise (errors are caught inside the
function).

---

## v0.6.5 — 2026-06-08 (Mistral "Fast" profile preset)

Adds a third Mistral profile preset alongside "Equilibrato" and
"Premium" — surfaced in Settings → Modelli LLM → Mistral AI card
right above the two existing ones.

The new **"Veloce"** (Fast) profile assigns Mistral Small 4
(`mistral-small-latest`) to both main and tabular roles, keeping
Ministral 3B for titles. Rationale:

  * Mistral Small 4 is ~3-5× faster than Large 3 on typical
    legal queries — useful when the user is triaging documents
    or doing quick lookups.
  * Pricing-wise it's 5× cheaper on input ($0.1 vs $0.5 per Mtok)
    and 5× cheaper on output ($0.3 vs $1.5), so it's an easy
    choice for tabular runs where each cell is a focused
    extraction (filename, dates, parties) rather than deep
    reasoning.
  * Small 4 still has vision + function calling + the full 128K
    context window, so it doesn't downgrade the chat composer's
    capabilities — only its quality ceiling on complex multi-step
    reasoning.

Layout: the previous `grid-cols-2` becomes `grid-cols-3` so all
three buttons fit side-by-side on the Settings card. Order is
deliberate: Fast → Balanced → Premium (cheap-to-expensive,
left-to-right). The active profile detection uses the same
auto-derive logic that already handled Balanced ↔ Premium ↔
Custom transitions, so the user gets the brand-coloured "Attivo"
badge on whichever profile their current role assignments match.

Three new i18n keys: `mistralProfileFast`, `mistralProfileFastHint`,
`mistralProfileFastApplied` localised in all six locales
(it / en / fr / de / es / pt).

No backend changes; svelte-check 0 errors.

---

## v0.6.4 — 2026-06-08 (hotfix — Mistral 1-RPS spacing + global cursor-pointer)

Two issues addressed:

### 1. Mistral 429 cascade despite the v0.6.2 semaphore

The semaphore added in v0.6.2 caps **concurrency** (at most one
Mistral request in flight at a time) but doesn't cap
**throughput** — a fast call to Ministral 3B (~0.3s) followed
immediately by the next gates produces 2 RPS in the same wall-clock
second, and Mistral Experiment's token-bucket rate-limiter
(nominally 1 RPS) 429s the second one.

The user logs from a 24-cell tabular review on v0.6.3 confirmed
the failure mode: cells fired sequentially through the semaphore
but ~half of them still 429'd because the calls were spaced
~0.5s apart, not ~1s.

v0.6.4 adds a **minimum-spacing rate limiter** on top of the
v0.6.2 semaphore:

```rust
static MISTRAL_MIN_INTERVAL: Duration = Duration::from_millis(1100);
static MISTRAL_LAST_CALL: LazyLock<Mutex<Instant>> = ...;

async fn post_with_retry(...) {
    let _permit = MISTRAL_GATE.acquire().await?;
    {
        let mut last = MISTRAL_LAST_CALL.lock().await;
        let elapsed = last.elapsed();
        if elapsed < MISTRAL_MIN_INTERVAL {
            tokio::time::sleep(MISTRAL_MIN_INTERVAL - elapsed).await;
        }
        *last = Instant::now();
    }
    // ... existing retry loop ...
}
```

The 1100ms (vs nominal 1000ms) gives a 10% safety margin against
Mistral's bucket-refill granularity. Worst-case effective
throughput drops from 1.00 RPS to 0.91 RPS — irrelevant for the
chat composer (which already pays multi-second LLM latency) and
acceptable for tabular extraction (24 cells × 1.1s ≈ 26s vs
24 cells × 1.0s = 24s).

The mutex is held only briefly: the semaphore's mutual exclusion
guarantees zero lock contention in steady state. Combined with
the three frontend / backend retry layers (v0.6.1, v0.6.2, v0.6.3),
the Experiment-tier 429 rate should now be essentially zero
under normal use.

### 2. Cursor-pointer on every clickable element

Tailwind v4's preflight removed the default `cursor: pointer`
from `<button>` in some browser combinations. For a Tauri shell
aimed at non-technical legal users that's the wrong default —
the pointing-finger is the universal "this is clickable" signal.
v0.6.4 restores it globally in `frontend/src/app.css` via a
single rule:

```css
button:not(:disabled):not([aria-disabled="true"]),
a[href],
label[for],
[role="button"]:not([aria-disabled="true"]),
[role="link"]:not([aria-disabled="true"]),
[role="menuitem"]:not([aria-disabled="true"]),
[role="tab"]:not([aria-disabled="true"]),
[role="option"]:not([aria-disabled="true"]),
summary {
  cursor: pointer;
}
```

Covers all standard ARIA roles for clickable surfaces. Excluded:
`:disabled` and `[aria-disabled="true"]` keep their
`not-allowed` cursor.

### Tests

One new pinning test: `mistral_min_interval_is_at_least_one_second`
prevents a future PR from accidentally dropping below the 1 RPS
floor. 25/25 `llm::mistral` green. No frontend tests on the
cursor change (single CSS rule, manually verified across the
sidebar, model picker, tabs, file pills, attach menu, settings
toggles).

---

## v0.6.3 — 2026-06-08 (hotfix — tabular per-cell rate-limit retry loop + hourglass UI)

v0.6.2's process-global Mistral semaphore (1 permit) capped
concurrency to 1 RPS but didn't help when a tabular review queues
24+ cells: cell #24 has been waiting 24s by the time its turn
comes, and if Mistral's quota window shifts during that wait
(per-minute reset cliff, monthly quota approached, transient
upstream throttle) the backend's own retry budget exhausts and
the cell lands as a red exclamation pill — visually identical to
a permanent error.

This release adds a **frontend-side retry loop** on top of the
backend's two layers:

  1. v0.6.1 — backend retry-with-backoff (3 attempts, ~7s)
  2. v0.6.2 — process-global semaphore (1 permit, serialised)
  3. **v0.6.3 — frontend rate-limit retry (this release)**

When a cell_update SSE event arrives with `status: "error"` and the
content matches a 429-class signature (`\b429\b|rate[\s_]?limit`),
the frontend:

  * Rewrites the cell status to `rate_limited` (new transient state
    in `TabularCell.status` union) — distinct from the permanent
    `error` so the UI can render an **Hourglass icon** instead of
    the red AlertCircle.
  * Schedules a retry via `POST /tabular-review/{id}/regenerate-cell`
    after a **linearly-growing backoff**: attempt N → N × 5s.
  * On success → cell flips to `done` and the retry state is
    cleared.
  * Still 429 → loop with the next backoff (attempt 2 = 10s,
    3 = 15s, …).
  * After `MAX_RATE_LIMIT_RETRIES = 10` attempts (~275s cumulative
    wait) — give up and surface the original error as permanent.

A `cancelRateLimitRetries(reviewId)` helper is exposed and called
from the "Interrompi" / "Genera" actions in
`TabularDetail.svelte` so stale `setTimeout` callbacks don't fire
against a fresh run.

### Diagnostics

The v0.6.2 console.warn hook only covered stream-level `error`
events. Per-cell errors come through `cell_update` events with
`status: "error"` — that's why the user reported "non ho ricevuto
alcun errore" despite seeing red pills. v0.6.3 fixes the gap:
every `cell_update` with `status === "error"` now logs to the
DevTools console with `[tabular] cell error:` prefix + the
reviewId / rowId / columnKey context. The user can press F12 and
correlate visual pills with backend errors verbatim.

Additionally, every retry scheduled by the new loop emits
`[tabular] cell rate-limited, scheduling retry` + attempt count +
delay, so the user can watch the recovery in real time.

### UI

`TabularDetail.svelte` gains a fourth case in the cell-status
switch: `rate_limited` renders the lucide `Hourglass` icon in
the `--color-warning-700` tone, with the cell's content string
(e.g. "Tentativo 3/10 fra 15s") attached as the `title` tooltip
for hover-discoverability.

### Why linear (not exponential) backoff

User-driven product spec: the retry attempt × 5s gives the user a
predictable mental model ("attempt N waits roughly N × 5s, the
hourglass goes away after ~5min") that exponential would muddle
("waited 256s? 512s? do I keep waiting?"). For the cell-extraction
use case where ~10 attempts is the cap, linear gets us to ~50s
last wait, ~275s total — close enough to "a quick coffee".

### Tests

No new backend tests (this release is frontend-only). The 24/24
`llm::mistral` from v0.6.2 stay green; the frontend changes are
covered by svelte-check (0 errors).

---

## v0.6.2 — 2026-06-08 (hotfix — Mistral concurrency semaphore + JS console diagnostics)

The retry-with-backoff added in v0.6.1 helped one-off 429 spikes
but didn't address the root cause: the chat composer routinely
fires several Mistral calls in parallel (main chat + title gen +
HyDE retrieval + tabular cell extraction), and on the free
Experiment tier (1 req/s) they all 429 simultaneously. Retry
doesn't help when 8 callers retry at the same time — they just
race again on the next attempt.

v0.6.2 adds the proper fix: a **process-global concurrency cap**
on Mistral requests via a `tokio::sync::Semaphore` with 1 permit.
Every Mistral call (chat / tabular / HyDE / title gen) acquires
the permit before issuing the HTTP request and releases it on
completion. Combined with v0.6.1's retry-with-backoff, this
ensures we never exceed 1 RPS to Mistral regardless of how many
MikeRust subsystems try to call it concurrently.

The cap of 1 is the safe default for Experiment tier (1 RPS limit).
For paid Scale-tier users (4-8 RPS) this is mildly conservative
but those users rarely 429 anyway, so the simpler "always 1"
behaviour is the right trade-off. Future work: expose a
`MISTRAL_MAX_CONCURRENT` env var / per-user setting for power
users on paid tiers.

The semaphore lives in `src/llm/mistral.rs` at module scope as a
`LazyLock<Semaphore>` so it spans the entire MikeRust process
lifetime. Acquisition is fair (FIFO) so tabular cells process
row-by-row in the order the worker pool fires them — useful
because the per-row pill rendering reads better in deterministic
order than arbitrary reorder.

### JS console diagnostics

Two `console.warn` hooks added to surface LLM errors in the
DevTools console alongside the existing UI banners. The error
strings already appear in the chat error banner and the per-cell
red pill in tabular reviews, but the visual treatment is purposely
terse; the console hook lets the user triage the actual cause:

* `[chat] LLM error: …` with `{ activeModel, chatId }` context
  — fires from `chatStore`'s `onError` SSE callback.
* `[tabular] stream error event: …` with `{ reviewId, raw }`
  — fires from `streamGenerate`'s `cell_update` event-loop
  on `type === 'error'`. A second hook in the outer fetch
  catch logs network-level failures with the same prefix.

Press F12 → Console while reproducing a 429 storm to see what
Mistral is returning underneath the visual errors.

### Tests

Two new unit tests in `src/llm/mistral.rs`:
* `mistral_concurrency_cap_is_one` — asserts the constant
  hasn't drifted without intent.
* `mistral_gate_serialises_acquirers` — verifies the semaphore
  actually blocks a second acquirer while the first holds the
  permit (uses `try_acquire` to keep the test sync).

24/24 `llm::mistral` tests green; 112/112 across `llm::`.
svelte-check 0 errors.

---

## v0.6.1 — 2026-06-08 (hotfix — Mistral 429 retry-with-backoff)

Single-fix release on top of v0.6.0. The dedicated Mistral provider
landed in v0.6.0 surfaces an immediate error when Mistral returns
HTTP 429, which on the free Experiment tier (default 1 RPS) is easy
to trip when the chat composer fires multiple Mistral calls in
quick succession (main chat + title generation + HyDE + tabular
extraction). v0.6.1 wraps the POST to /chat/completions in a
3-attempt retry-with-backoff:

  1. Honour Mistral's `Retry-After` header when present (parsed as
     integer seconds, capped at 30s to avoid pathological waits on
     monthly-quota resets).
  2. Fall back to exponential backoff when the header is absent:
     1s → 2s → 4s. Total worst-case wait ~7s before surfacing the
     error.
  3. Only 429 triggers a retry — 401/403/422/5xx are surfaced
     immediately because they're either authoritative refusals
     (auth, quota, validation) or distinct failure modes that
     would be misleading to hide behind a retry.

New helper `next_backoff(attempt, retry_after_header)` is a pure
function (no I/O, no clock); the actual sleep happens in
`post_with_retry` around `tokio::time::sleep`. Both `stream` and
`complete` route through the same wrapper so SSE and non-SSE
paths share the policy.

5 new unit tests pin the backoff calculation (Retry-After
honoured, capped at 30s, exponential fallback, bogus-format
fallback, whitespace tolerance). 22/22 `llm::mistral` tests
green; 110/110 across the `llm::` tree.

No schema migration; no UI change; no API contract change beyond
the user-perceptible "transient 429s now resolve themselves in
~1-7s instead of failing the chat turn."

---

## v0.6.0 — 2026-06-08 (Mistral first-class: cloud provider with prompt caching + sequential tool calling)

Promotes Mistral La Plateforme (`api.mistral.ai`) from an
opportunistic OpenAI-compat reuse (rode through `local.rs` in
v0.5.x) to a **dedicated provider** with Mistral-specific
optimisations baked in. The headline benefits — addressed at the
legal-target product audience — are:

* **80-90% cost reduction on long document-heavy chats**. Mistral
  charges 10% of normal token price on cache hits; the new path
  derives a stable per-chat key (`mike_chat_{chat_id}`) so the
  system prompt + attached-documents prefix is cached across every
  turn of a conversation.
* **Predictable sequential tool execution**. Mistral's API default
  is `parallel_tool_calls: true`; legal workflows (tabular cell
  extraction, citation lookups in order) want sequential. We
  override to `false` by default; the user can re-enable parallel
  via the new toggle in Settings.
* **No spurious safety refusals on legal content**. The
  `safe_prompt` parameter — Mistral's safety wrapper — is sent
  explicitly OFF by default because criminal-case sentences and
  sensitive medical reports trip false positives. Available as a
  toggle for users who hit the opposite case (need stricter
  filtering).
* **EU data residency clarity**. New info paragraph in the
  Mistral card states "EU servers (France) · 30-day rolling abuse
  monitoring · never used for training on paid API plans". A link
  to the official Mistral help-center article explains how to
  request Zero Data Retention (manual support ticket — not a
  per-request header, confirmed against the docs 2026-06).

### Backend — `src/llm/mistral.rs`

New dedicated module (Commit A — 514 LOC, 17 unit tests). Routes
the `mistral:` prefix to its own `stream` / `complete` instead of
the generic `local::stream` OpenAI-compat path. What the module
sends that `local.rs` never did:

  * `parallel_tool_calls: false` (override Mistral's `true`
    default for sequential semantics).
  * `safe_prompt: false` (explicit OFF; was always implicit
    before).
  * `prompt_cache_key: "mike_chat_{chat_id}"` (stable per-chat
    key — only emitted when the call carries a chat_id; one-shot
    callers like title generation, HyDE, summarisation, doc
    summary and translation pass None and the key is omitted).

Mistral-specific error mapping with Italian-first messages:

  * 401 → "API key non valida o scaduta. Verifica in
    Settings → Modelli LLM → Mistral AI."
  * 403 → "richiesta rifiutata (quota esaurita o filtro di
    sicurezza attivato)" with body detail.
  * 422 → "payload non valido. Probabile model id errato o
    messaggio malformato."
  * 429 → parses `Retry-After` header into "(riprova fra ~Ns)".
  * 5xx → "errore lato server" with body detail.

### Plumbing — provider routing + chat_id

* `enum Provider` gains a `Mistral` variant. `provider_for_model("mistral:")`
  now returns `Provider::Mistral` (was `Provider::OpenAI`).
  `stream_chat` / `complete_text` dispatch to the new
  `mistral::stream` / `mistral::complete` functions. All five
  match-against-Provider sites updated (hyde, summarize,
  chat title gen, doc summarisation, workflow translation,
  tabular cell extraction).
* `StreamParams` gains `chat_id: Option<String>` — threaded
  through 11 construction sites so the Mistral cache key has a
  stable anchor.
* `to_wire_messages` + `parse_sse_line_opt` exposed `pub(crate)`
  in `local.rs` so `mistral.rs` reuses them — Mistral's SSE +
  message-wire format is OpenAI-identical, no point duplicating.

### Schema — migration 0033

Two new boolean columns on `user_settings`:

  * `mistral_safe_prompt INTEGER DEFAULT 0`
  * `mistral_parallel_tools INTEGER DEFAULT 0`

Both OFF by default — matches the Commit A hard-coded behaviour.
Existing installs upgrade transparently. `StreamParams` gains a
`mistral_opts: Option<MistralOpts>` field; the new
`crate::routes::chat::build_mistral_opts(model, settings)` helper
mirrors `build_local_config`'s shape and is called at every
StreamParams site where user settings are in scope (3 chat.rs
sites + tabular_reviews + workflows). One-shot callers
(hyde, summarize, docs summary, mod.rs complete_text) pass `None`
and inherit the false defaults.

### UI — Mistral card

`Settings → Modelli LLM → Mistral AI` grows two new sections,
inserted below the existing "Profilo modelli" picker (which was
already in v0.5.6):

  1. Two `Toggle` components for `mistral_safe_prompt` and
     `mistral_parallel_tools`, both with localised labels + hint
     text. Disabled when no API key is configured — toggling them
     without Mistral active is meaningless.
  2. Info paragraph + external link to the Mistral help-center
     ZDR article. Surfaced as text, not as a checkbox, because
     ZDR activation is a manual support ticket — verified against
     the official help center 2026-06.

### i18n — 5 new keys

`Settings.mistralSafePrompt` + Hint,
`Settings.mistralParallelTools` + Hint, `Settings.mistralEuHostingNote`,
`Settings.mistralZdrLink`. All localised in it/en/fr/de/es/pt.

### Migration notes

  * Existing chats keep their previous model selection — the
    routing change (`Provider::OpenAI` → `Provider::Mistral` for
    `mistral:` prefix) is transparent. The cost reduction kicks
    in immediately on the next chat turn that carries a chat_id.
  * Mistral users on v0.5.x can ignore Commit A entirely and
    keep using the previous defaults; the only behavioural
    difference is the new `prompt_cache_key` (auto-enabled, no
    config) which lowers their token bill by ~80-90% on long
    chats. The two toggles in the Settings card only matter if
    they're already a Mistral user.

### Tests

17 new unit tests across `src/llm/mistral.rs`:
  * credentials: missing key / missing config / valid
  * cache_key: chat-scoped / one-shot / empty-id
  * build_body: defaults / override / cache-key in/out /
    sampling defaults / stream flag propagation /
    mistral_opts override + fallback
  * error mapping: 401 / 429 with + without Retry-After / 500
    surfacing body

105/105 across the whole `llm::` tree green. `cargo check` clean
across the workspace.

### Not in this release (follow-up)

* `wiremock`-backed round-trip HTTP integration tests for
  Mistral. The 17 unit tests already cover the build_body and
  error-mapping logic; wiremock would add coverage of the wire
  shape end-to-end. Deferred to a v0.6.x point release.
* Vision support over Mistral Medium 3.5 / Large 3 — the API
  side works (we send `image_url` content parts) but the chat
  composer doesn't yet route PDF page renders to Mistral the same
  way it does to Gemini.

---

## v0.5.6 — 2026-06-07 (chat composer UX fixes + experimental local-only LLM mode)

Two bug fixes in the chat composer that surfaced during hands-on
testing of the v0.5.5 cycle, plus an opt-in experimental local-only
LLM mode for Ollama users (off by default, treated as a preview for
the v0.6.x line — see the closing section below).

### Doc picker — project scope

The "Sfoglia tutti" picker inside a project-scoped chat used to
show **every document the user had ever uploaded anywhere**,
including documents from other projects and from standalone chat
attachments. It now restricts to the current project's documents
via the existing `?project_id=…` query param on
`documents.rs::list_documents` — backend filter already in place
since the project work, only the frontend was passing nothing.

Standalone chats (Assistant tool, no project) keep the global
picker unchanged.

### New-chat-in-project confirm modal

Clicking `+` (new chat) in the sidebar while a project-scoped chat
was active used to silently carry the project chip into the new
chat — the user had no clear signal the next chat would be
project-bound until they glanced at the composer. The new flow:

* When the active chat is in a project, `+` opens a confirm modal:
  *"Stai lavorando dentro un progetto. Vuoi mantenere il progetto
  associato alla nuova chat?"* — two action buttons ("Chat
  indipendente" / "Sì, mantieni il progetto"), implicit cancel via
  X / Esc / backdrop click.
* When the active chat has no project, the flow is unchanged (no
  modal, instant new chat).

The fix also resolves the long-standing "chip persists silently"
bug from the original effect's early-return on null
`activeProjectId`. A new monotonic counter
`chatStore.clearProjectTick` lets the modal's "Chat indipendente"
branch reset the chip from outside the composer without race
conditions. New `Sidebar.*` i18n keys localised in all six
locales.

### Experimental — local-only LLM mode (opt-in, preview)

New toggle in Settings → Modelli LLM → "Modalità sicura locale".
**Off by default** so existing installs keep their custom Ollama
URL and free-form model id intact. When ON the local provider
locks to `http://localhost:11434` and the chat picker collapses
to two curated entries (Qwen 3.5 4B `q4_K_M` and Gemma 4 E2B IT
GGUF `Q4_K_M`) derived through Modelfiles MikeRust creates on
demand with thinking suppression baked in (Qwen `/no_think` token
in the chat template; Gemma `<think>` / `<thinking>` /
`<reasoning>` stop sequences + "rispondi direttamente" preamble).

Treated as a **preview for the v0.6.x line** — the mechanism is
feature-complete and tested but the UX around model discovery,
context-window tuning, and cross-platform Ollama detection is
still being refined. Power users can flip the toggle; everyone
else can ignore it — the rest of the local provider works exactly
as before.

Implementation notes (for reviewers):

* Backend module [`src/llm/ollama_manager.rs`](src/llm/ollama_manager.rs)
  wraps [`ollama-rs`](https://crates.io/crates/ollama-rs) 0.3 with
  `heartbeat()`, `list_installed()`, `ensure_curated(id)` (returns
  a Stream<EnsureEvent> covering `started → pulling{progress} →
  creating → ready` / `error`), and `uninstall_curated(id)`.
* `local.rs::resolve_endpoint` enforces secure mode: refuses any
  non-loopback URL, refuses any model id not in the curated
  allowlist (normalising the `:latest` Ollama tag suffix before
  the contains check), and prepends a no-thinking preamble to the
  system prompt.
* Four new routes under `/user/local-secure/*` for heartbeat /
  catalogue / install (SSE) / uninstall.
* Settings UI: auto-saves the toggle on flip, surfaces install
  progress + cancel button, refuses parallel installs via per-id
  AbortController registry, lives in a module-singleton store so
  it survives section unmount.
* Chat composer: when secure mode is on, the picker collapses to
  only the two curated entries (all cloud providers are hidden
  from the picker even if their API keys are configured).
* New direct deps: `ollama-rs = 0.3` (feature `stream`) and
  `async-stream = 0.3`. Both pure Rust.
* New schema migration **0032** — adds
  `user_settings.local_secure_mode INTEGER DEFAULT 0`.
* 13 new unit tests across `llm::local` and `llm::ollama_manager`
  (15/15 + 6/6 green).
---

## v0.5.5 — 2026-06-07 (stable consolidation of the v0.5.4 amendment cycle)

Stable promotion of everything that landed during the v0.5.4
hands-on testing cycle. The v0.5.4 tag was force-rewritten a few
times as fixes were appended (back-stack regressions, untrack
boundaries, export-shape gaps, tabular review rename); v0.5.5 marks
the end of that iteration with a clean, non-rewritten tag.

In addition to consolidating the v0.5.4 work (citation pipeline,
tabular UX bundle, generic navigation back-stack, sidebar
"Progetti recenti", PickerModal select-all, per-row project Export,
nav-tick + untrack fixes), v0.5.5 also includes the final polish
landed during the same cycle:

### Closed `.mikeprj` export-shape gaps

The v0.5.4 audit ([HISTORY v0.5.4 "Known gaps"](#v054--2026-06-05))
called out several fields the exporter wasn't reading or was
reading-and-discarding. All fixed in v0.5.5:

* [`ProjectRecord`](src/mikeprj/manifest.rs) gains `domain` and
  `isolation_mode`. The recipient lands on the same domain filter
  and retrieval-scope mode the source project had instead of falling
  back to defaults.
* [`DocumentRecord`](src/mikeprj/manifest.rs) gains `domain`,
  `project_folder_id` (as a path hint — folder-tree rebuild on the
  recipient side is its own change, tracked separately), and the
  accept/reject decision tuple from migration 0029
  (`decision`, `decision_reason`, `decision_summary`).
  `content_hash` is rehydrated on import from the manifest's
  `sha256` so the recipient's tabular dedup-by-hash works on the
  first re-upload, not only after a manual re-hash.
* [`TabularReviewRecord`](src/mikeprj/manifest.rs) gains `domain`
  (review-level — can diverge from the project's).
* [`WorkflowRecord`](src/mikeprj/manifest.rs) gains `domain`. The
  exporter also stops hard-coding `type = "assistant"` and
  discarding `columns_config` — a custom tabular workflow finally
  travels intact.

All new fields are `Option<…>` with `#[serde(default)]` so archives
produced by the v0.5.4 pre-amendment exporter still deserialise
cleanly; two new tests
([`document_record_roundtrips_pre_v054_archives`](src/mikeprj/manifest.rs),
[`project_record_roundtrips_pre_v054_archives`](src/mikeprj/manifest.rs))
pin the back-compat behaviour. 21/21 mikeprj tests green.

### Tabular reviews — per-row Pencil rename

The Revisioni tabellari list view ([`Tabular.svelte`](frontend/src/routes/Tabular.svelte))
gains a Pencil icon between the domain Badge and the Trash on every
row — parity with the Projects-list rename affordance. Opens a
small inline modal with the current title pre-filled, Enter
submits, Escape cancels, Save is disabled when the input is empty
or unchanged. Reuses the existing `PATCH /tabular-review/:id`
endpoint (which already accepted `title`). New i18n keys
`TabularReviews.renameReview` and `TabularReviews.renamedToast`
localised in all six locales.

### No new schema migration

Schema unchanged from v0.5.4 (still at migration 0031). All v0.5.5
changes are application-level. v0.5.4 installs upgrade to v0.5.5
by installing the new MSI on top — no DB step.

---

## v0.5.4 — 2026-06-05 (tabular-review FK hotfix + the UX upgrades and follow-up fixes from the same testing session)

The schema hotfix that triggered this release is the FK on
`tabular_reviews.workflow_id`; everything else listed below came out
of the same hands-on testing session and shipped together because
they touch the same screens.

### Schema hotfix — tabular_reviews FK to workflows blocked preset usage

Creating a new tabular review against any of the
49 built-in workflow presets shipped under
`config/workflow-presets/*.json` (the user surfaced it on
"Inventario documenti medico-legali" but every preset triggered the
same failure) returned

```
error returned from database: (code: 787) FOREIGN KEY constraint failed
```

directly in the "Nuova revisione" modal, blocking the creation.

The root cause was a residual FK from migration 0002:

```sql
CREATE TABLE tabular_reviews (
    ...
    workflow_id TEXT REFERENCES workflows(id) ON DELETE SET NULL,
    ...
);
```

Built-in workflows live as JSON manifests under
`config/workflow-presets/` and are merged into the registry in-memory
at startup — they intentionally never own a row in the `workflows`
table. We hit and fixed the same pattern on `workflow_hidden` in
migration 0022; `tabular_reviews` carried the same FK and the bug had
been latent since the JSON-preset registry shipped.

New [`migrations/0031_tabular_reviews_drop_workflow_fk.sql`](migrations/0031_tabular_reviews_drop_workflow_fk.sql)
rebuilds the table dropping the `workflow_id` FK while keeping the
`user_id` / `project_id` references intact and preserving all data
plus the `domain` index. After the migration runs once on first
launch of v0.5.4, `workflow_id` is a free text id that resolves
either against the DB `workflows` table (user-created workflows) or
the in-memory preset registry (built-in workflows).

No frontend / backend code change — the route handler was already
binding `workflow_id` as a free string. No DB seeding hack. No
behaviour change for any existing review.

### UX — workflow picker now labels Tabellare vs Assistente

[`PickerModal`](frontend/src/lib/components/ui/PickerModal.svelte)
gains an optional `badge` field per item, rendered as a right-aligned
pill via the existing `Badge` component (reusing the brand-audit
`tabular` purple and `assistant` blue tones — they were minted for
exactly this case but had never been surfaced). The chat composer's
"Allega un workflow" modal in
[`ChatInput.svelte`](frontend/src/lib/components/chat/ChatInput.svelte)
populates it from `workflow.type`. New i18n strings reuse the
existing `Workflows.typeTabular` / `Workflows.typeAssistant`
catalogue keys (no new translation work needed).

### UX — Upload affordance inside the doc picker

[`PickerModal`](frontend/src/lib/components/ui/PickerModal.svelte)
gains an optional `onUpload` prop. When set, the modal renders an
"Upload" button bottom-left (mirror of the upstream Mike layout).
[`TabularDetail.svelte`](frontend/src/lib/components/tabular/TabularDetail.svelte)
wires it to a hidden `<input type="file" multiple>` that uploads via
`documentsApi.upload` sequentially, refreshes the picker list, and
automatically links the new docs to the review via the same patch
endpoint the picker uses — the user does not have to re-tick the
files they just uploaded. New i18n key `Common.upload` added in all
six locales (it/en/fr/de/es/pt).

UX corollary: once an upload succeeds during a picker session the
"Conferma" button stays enabled even with zero items ticked
(`confirmAlwaysEnabled` flag toggled by the host), so the user has a
clean way out — Cancel implied undo of an action that had already
committed. New i18n key `TabularReviews.docUploaded` localised in
all six locales.

### UX — Import Project button on the Projects list

[`Projects.svelte`](frontend/src/routes/Projects.svelte) gains a
secondary "Importa progetto" button next to the existing
"Nuovo progetto", powered by a hidden `<input type="file"
accept=".mikeprj">` that reuses the existing email-prompt modal that
was previously reachable only by drag-and-dropping a `.mikeprj` file
onto the page. The backend `POST /project/import` endpoint and the
`ProjectImport.*` i18n catalogue were already complete since v0.4.x
and didn't need any new code.

### Follow-up — tabular cell extraction for legacy-path uploads

The new picker Upload affordance writes through
[`/document` `cache=false`](src/routes/documents.rs), the legacy
storage layout that uses a flat `documents/<user>/<doc_id>` key with
**no file extension**. Two cascading bugs surfaced once the user
clicked "Esegui":

1. `load_document_text` in
   [`tabular_reviews.rs`](src/routes/tabular_reviews.rs) was passing
   the *storage key* (relative, e.g. `documents/<uid>/<doc>`) to
   `extract_text_dispatch`. The dispatcher's PDF branch re-opens the
   file via pdfium — which takes a path, not bytes — so pdfium got a
   relative path and a missing extension, failed silently, and the
   row surfaced as "Document text unavailable" in the UI. Fix
   resolves the key to an absolute path under `STORAGE_PATH` /
   `default_storage_path` before dispatch.
2. The dispatcher dispatches on file extension. The legacy key has
   none, so the catch-all branch returned "format not supported"
   even after the absolute-path fix. The query now also reads
   `file_type` from the `documents` row and, when the key has no
   extension, materialises a sibling on disk with the right suffix
   (`.pdf`, `.docx`, …). Pdfium then opens it directly and the
   dispatcher's extension match hits the correct branch.

Exhaustive `[tabular][doc-text]` tracing lines were added at every
branch (sidecar hit / sidecar miss / storage.get / dispatch result)
so the next surprise surfaces in the log directly.

### Tabular detail — full-width layout

[`TabularDetail.svelte`](frontend/src/lib/components/tabular/TabularDetail.svelte)
no longer constrains itself to `max-w-6xl` (~1152 px). Header and
grid now fill the available container width, so wide monitors stop
wasting 60%+ of horizontal real estate. The side document-viewer
panel lives in a separate flex parent and resizes/collapses
independently — the grid shrinks back when that panel is open and
expands edge-to-edge when it isn't.

### Tabular row dedup on re-upload

Adding a document that is byte-identical to one already extracted
in the same review (match key: filename + SHA-256 content_hash) now
inherits the earlier row's cells + status instead of asking the LLM
to re-extract them. Rows still in `pending` are ignored — there is
nothing to copy from those yet. The match runs in the row-insert
loop of `patch_tabular_review` in
[`tabular_reviews.rs`](src/routes/tabular_reviews.rs); hits are
logged as `[tabular][dedup] review=… new_doc=… inherits cells …`.

Prereq lit alongside this feature: the legacy (`cache=false`)
upload path in
[`documents.rs`](src/routes/documents.rs) now also computes the
SHA-256 `content_hash`. Without it the dedup join couldn't match
anything coming through the new picker Upload affordance. Hashing
50 MB takes <100 ms — dwarfed by the upload bandwidth.

### "Nuova revisione" inside a project + clickable review rows

Parity with the existing "Nuova chat" button on the Conversazioni
tab. [`ProjectDetail.svelte`](frontend/src/lib/components/projects/ProjectDetail.svelte)
gains an inline create-review modal that lists tabular workflows
scoped to the project's own domain, inherits `project_id` + the
project's domain on submit, and drills straight into the new
review's extraction grid via a new `openReviewId` state. Existing
review rows on the project's *Revisioni tabellari* tab are also
clickable now and open inline. The standalone Tabular route keeps
its own `detailId` logic for use from the sidebar; the
project-originating path is owned by `ProjectDetail` so the back
button returns to the project, not to the standalone Revisioni
tabellari screen.

### Scope-correct doc picker in tabular reviews

User-reported on 2026-06-06: the "Aggiungi documenti" modal of a
tabular review listed every document the user had ever uploaded
anywhere — including chat attachments from other domains and other
projects.
[`TabularDetail`](frontend/src/lib/components/tabular/TabularDetail.svelte)
now filters its list against the review's own context:

  * `domain == review.domain` — always.
  * If the review is project-scoped, the project's `isolation_mode`
    drives the rest:
      - **rigoroso** (`strict`) → only docs whose `project_id`
        matches the review's project.
      - **condiviso** (`shared`) → project docs plus globals
        (`project_id IS NULL`, i.e. synced-folder uploads and
        standalone uploads).
  * Otherwise (no project on the review) → only globals.

Backend `GET /document` now returns each doc's `project_id` so the
filter can run client-side without an extra round-trip. The
picker's Upload affordance inherits the review's `project_id` on
new uploads so a doc added through a strict-scope picker stays
visible in that same picker afterwards instead of vanishing into
the global pool.

### Generic router back-stack — drill-downs return to where the user came from

Directive from 2026-06-06: "la coerenza di navigazione deve essere
applicata a tutti i contesti, chat, workflow, tabular, ecc." Back
from a drilled-in view must always return to the originating
screen, not to the destination's natural sidebar parent. The
TabularDetail-inside-ProjectDetail fix from earlier on the same
day addressed the tabular-from-project path inline; this generalises
the mechanism so any caller can add a return target with two lines.

[`router.svelte.ts`](frontend/src/lib/stores/router.svelte.ts) now
exposes:

  * `NavContext` — free-form per-route restoration data. Today the
    only field is `projectId`; new screens add fields without churn.
  * `BackEntry` — `{ route, context, label? }` on a stack.
  * `goWithReturn(target, ctx, entry)` — drill-down navigation;
    pushes an entry onto the stack so the destination screen has a
    way back.
  * `popBack()` — destination's back arrow calls this.
  * `consumePending(): NavContext` — destination reads it on mount
    and the call clears it so it isn't applied twice on rerenders.
  * `go()` now clears the back stack — sidebar entries (which call
    `go`) are "switch context", not "drill".

First consumer wired:
[`ProjectDetail.openChat`](frontend/src/lib/components/projects/ProjectDetail.svelte)
uses `goWithReturn('assistant', {}, { route: 'projects', context:
{ projectId: id }, label: t('Nav.backToProject', { name: project.name }) })`
instead of `router.go('assistant')`.
[`Assistant.svelte`](frontend/src/routes/Assistant.svelte) renders
a `← {router.back.label}` arrow at the top whenever `router.back`
is non-null.
[`Projects.svelte`](frontend/src/routes/Projects.svelte)'s mount
`$effect` consumes the pending context and, if it carries a
`projectId`, opens that project's detail directly — restoring the
exact place the user was.

New i18n key `Nav.backToProject` localised in all six locales.
Extending the mechanism to any future drill-down (Workflow editor,
DOCX template editor, …) is two lines per call-site.

#### Same-route re-navigation + Svelte 5 reactivity traps

Two follow-up fixes needed to make the back-stack robust:

1. **navTick** — clicking a *different* project in the new sidebar
   accordion while already on the Projects screen did nothing,
   because `router.go('projects', { projectId: X })` short-circuited
   on `current === 'projects'` and the destination `$effect` never
   re-fired. The router now exposes a monotonic `navTick` (bumped
   by `go` / `goWithReturn` / `popBack`). `Projects.svelte`
   dereferences it (`void router.navTick`) so the effect re-runs on
   every navigation, regardless of whether `current` changed.
2. **untrack on mutations** — wrapping `navTick++` and
   `backStack.length` checks inside an `$effect` body subscribed
   the caller to those signals via the read-modify-write, then the
   write re-triggered the caller. App.svelte's boot effect hit this
   immediately, producing `effect_update_depth_exceeded` before the
   unlock screen could render. Every mutating router method now
   wraps its body in `untrack(() => …)` so reads inside don't
   subscribe the caller; downstream consumers that explicitly read
   `navTick` (Projects.svelte) still subscribe and re-fire on
   navigation, because the write itself is observable across
   untrack boundaries.

### Sidebar "Progetti recenti" accordion

[`Shell.svelte`](frontend/src/routes/Shell.svelte) gains a
collapsible "Progetti recenti" section between the tool nav and the
existing "Chat recenti" accordion. Lists the top 5 projects by
`updated_at` DESC. Click → `router.go('projects', { projectId: p.id })`
and the consumePending mechanism drills straight into that project's
detail. The store is refreshed once on Shell mount so the accordion
is populated even before the user visits the Projects screen.

New i18n keys `Sidebar.recentProjects` and `Sidebar.noProjects`
localised in all six locales.

### PickerModal select-all / deselect-all toggle

[`PickerModal`](frontend/src/lib/components/ui/PickerModal.svelte)
in multi-select mode now exposes a small toggle bar between the
search input and the list. Visible only when there's more than one
visible row. Selection scope is the **visible** (search + filter
applied) set so the user can select-all on a search-filtered subset
without disturbing selections outside it; the label flips to
"Deseleziona tutti" once every visible row is in the selection — a
familiar three-state pattern without a dedicated indeterminate UI.
A live "{n} selezionati" counter sits on the left of the same row.

New i18n keys `PickerModal.{selectAll, deselectAll, selectedCount}`
localised in all six locales. The component's local `t` helper now
forwards a params record so the count interpolation works.

### Per-row Export button on the Projects list

[`Projects.svelte`](frontend/src/routes/Projects.svelte) gains a
Download icon between the rename and delete affordances on every
project row. Opens the existing email-prompt + include-chats modal
previously only reachable from inside a project's detail page.

Reuses the entire backend chain: `POST /project/:id/export` →
[`mikeprj::io::build_payload`](src/mikeprj/io.rs) → ZIP →
[`mikeprj::crypto::seal`](src/mikeprj/crypto.rs) (AES-256-GCM with
an Argon2id-derived key from the recipient email; file format
documented in [`src/mikeprj/mod.rs`](src/mikeprj/mod.rs)).

The `.mikeprj` payload includes:

* project metadata (name, cm_number, created_at);
* document binaries via `storage.get(storage_path)` plus the
  documents' metadata (filename, file_type, size_bytes, sha256,
  created_at);
* tabular reviews — **configuration only** (id, title,
  `columns_config`); the extracted cells do NOT travel and the
  recipient re-runs the extraction;
* custom user workflows (built-in presets resolve by id on the
  recipient side);
* chats with full message history when the export's
  `include_chats` toggle is on (default off).

NOT included by design: extracted-text caches, embeddings, the
chat-attachment hash pool, API keys, MCP server configurations.

**Known gaps in the current export shape** (intentional or oversight,
documented for transparency — fixes shipped in a follow-up): the
SELECT in `build_payload` doesn't read the project's `domain` or
`isolation_mode`, the workflows query hard-codes `type =
"assistant"` and discards `columns_config` (so tabular custom
workflows arrive monchi), `documents.project_folder_id` and the
per-chat decision state aren't carried, and `content_hash` isn't
re-emitted (the recipient's dedup-by-hash works only after a
re-upload). See the next entry once it lands.

---

## v0.5.3 — 2026-06-05 (hotfix — Gemini 3.5 parallel-tool-call accumulator)

Single-fix release. `gemini-3.5-flash` streams parallel function calls
across separate SSE chunks (one `functionCall` per `data:` line), each
chunk's parts array carrying its own `thoughtSignature` on the first
call. The chat dispatch loop in
[`src/routes/chat.rs`](src/routes/chat.rs) was **replacing** its
`iter_tool_calls` vec on every `StreamEvent::ToolCalls` event:

```rust
Ok(StreamEvent::ToolCalls(calls)) => {
    iter_tool_calls = calls;   // ← drops everything from earlier chunks
}
```

So when Gemini emitted three parallel calls in iteration 1, only the
last chunk's calls survived — the rest of the parallel batch vanished
before they were ever dispatched. Iteration 2's request to Gemini
then carried a truncated assistant turn that no longer matched the
model's internal record of which calls it had emitted, surfacing as:

```
Gemini API error 400 Bad Request:
  "Function call is missing a thought_signature in functionCall parts.
   Additional data, function call `default_api:read_document`, position 2."
```

(Reproduced on `gemini-3.5-flash` 2026-06-05 with the user testing
v0.5.2 + 4 uploaded documents.)

The fix is one line — `extend` instead of `=`:

```rust
Ok(StreamEvent::ToolCalls(calls)) => {
    iter_tool_calls.extend(calls);
}
```

`extend` is safe for the other providers too. Claude tool use never
emits `StreamEvent::ToolCalls` in the SSE path (Anthropic streams
deltas through a different channel); the OpenAI-compatible local
adapter emits a single batched event per turn, so `extend` of one vec
of N is identical to assignment of the same N.

Per the official Gemini docs
(<https://ai.google.dev/gemini-api/docs/thought-signatures>), only the
first function call in a parallel batch carries a `thoughtSignature`
and the others must not, so this fix is sufficient — the original
gemini.rs wire-format code (`to_wire_contents`) was already correct
for the single-call case.

No DB migration, no config change, no UX change. Everything from
v0.5.2 ships unchanged.

---

## v0.5.2 — 2026-05-26 (security hardening — drop S3 fallback + Parquet size cap + platform-support clarification)

Security-driven patch release. Three changes land together:

### Drop the `s3-storage` feature and the AWS SDK chain

`aws-sdk-s3` + `aws-config` are gone from `Cargo.toml`. Removing them
drops 70+ transitive crates from the lockfile, including:

- `rustls 0.21.12` + **`rustls-webpki 0.101.7`** — the Dependabot
  advisory that prompted this release. The AWS SDK chain was the
  only path that pulled in the vulnerable 0.21 line of rustls; the
  rest of the codebase (fastembed, sqlx, hf-hub) already uses
  `rustls 0.23` + `rustls-webpki 0.103.13`.
- `aws-smithy-*`, `aws-sigv4`, `rustls-native-certs`, `sct`, …
  the full SDK transitive graph.

The S3/R2 path was always feature-gated **OFF** by default and the
`s3-storage` feature was never actually wired into
[`src/storage/mod.rs`](src/storage/mod.rs)::`make_storage` — that
function has only ever returned `LocalStorage`. The trait stays for
the ergonomic win of a single `Box<dyn Storage>` handle and to keep
the door open for a sovereign-cloud backend on rustls 0.23 later.
Behaviour for end users is unchanged.

`local-storage` is kept as a no-op feature so anyone who pinned it
in their own manifest still resolves cleanly.

### Parquet shard size cap — Thrift advisory mitigation

The `parquet` crate (used by the Italian Legal Cassazione bulk
importer) transitively depends on `thrift 0.17.0`, which carries an
unfixed-upstream advisory: a crafted Parquet footer can declare an
allocation that the decoder honours before validating against the
actual stream length, causing a DoS-style OOM. The Apache Thrift
Rust bindings have not shipped a fix; even the latest `parquet 58`
still depends on `thrift ^0.17`.

Mitigation lands as a hard byte cap applied **before** the bytes ever
reach `parse_parquet_metadata`:

- New file [`config/corpora.json`](config/corpora.json) holds the
  knob `max_parquet_file_size_mb` (default **500 MB** — comfortably
  above any legitimate shard from the corpora we ingest, well below
  the threshold at which a malicious footer would matter on a 16 GB
  workstation).
- New module [`src/corpora/limits.rs`](src/corpora/limits.rs) is the
  loader. Same env-override + ancestor-walk pattern as
  `src/presets/model.rs` (`MRUST_CORPORA_LIMITS` env var). Falls back
  to defaults with a warning if the JSON is missing or malformed.
- [`src/corpora/italian_legal.rs`](src/corpora/italian_legal.rs)
  refuses to decode a shard above the cap with a clear bail message;
  the import job surfaces it through the normal `corpus_state`
  channel.
- [`docs/CORPUS_PLUGINS.md`](docs/CORPUS_PLUGINS.md) gains a
  **Security** section explaining the Thrift advisory, the
  `dila-bulk-xml` analogous concern (oversized XML / tar-walker
  hardening), and concrete guidance for plugin authors who add new
  corpus sources — pin URLs to official publishers, never accept
  user-supplied URLs verbatim, leave the cap at default unless you
  have a specific reason to raise it.

### Platform-support clarification (resolves the `glib 0.18.5` Dependabot finding)

[README.md](README.md) gains a "Supported platforms" section right at
the top of Quick start:

- **Windows** is the only currently shipping target (x86_64 + ARM64).
- **macOS** is on the roadmap but work hasn't started — codebase
  compiles to `aarch64-apple-darwin` already; gating items are
  signing / notarisation and a Touch-ID equivalent of the
  Windows-Hello unlock flow.
- **Linux is not supported and there are no plans to add it.**

The practical consequence for security scanners: the `gtk` / `glib` /
`atk` / `webkit2gtk` / `tao-linux` chain that Tauri pulls in for the
Linux WebView backend **is not present in any shipped MSI** —
`webview2-com` + `windows-rs` are what compile in on Windows.
Advisories on that chain (e.g. `glib 0.18.5` flagged 2026-05-26) are
therefore inert for end users and tracked as "not affected — Linux
support is not in scope".

### Lockfile + dep changes

- 70+ transitive crates removed via `cargo update` after deleting the
  AWS deps. Net `Cargo.lock` shrinks ~3 kB.
- No new direct dependencies. `parquet`, `arrow-*`, `thrift` versions
  unchanged (bumping `parquet 53 → 58` does not fix the Thrift
  advisory — even latest parquet still depends on `thrift ^0.17` —
  and would require a coordinated `arrow-array` / `arrow-schema`
  migration; deferred until either Thrift ships a fix or arrow-rs
  removes the dep).
- Schema version on `config/corpora.json` set to `1`. No DB
  migration.

### Tests

Existing 23 `llm::gemini` unit tests still green. Four new
`corpora::limits` unit tests pin the default / config / missing-field
/ overflow-safe arithmetic paths.

---

## v0.5.1b — 2026-05-26 (hotfix — Gemini sampler restored to default for versatile heterogeneous-document analysis)

Single-fix hotfix on top of v0.5.1. The cross-provider
`temperature = 0.5` introduced in v0.5.1 was making
`gemini-2.5-flash` lose expressiveness on long, heterogeneous
contexts — first reproduced on the "Timeline cronologica clinica"
workflow with three medical PDFs, where the model emitted the table
header (`| Data | Ora | Evento`) then locked into a whitespace loop
and emitted 155,010 characters of pure spaces before closing the
stream. A second reproduction with ten attachments showed
~1.96 million whitespace characters.

[`src/llm/gemini.rs`](src/llm/gemini.rs) `build_body` now omits
`temperature` entirely on every Gemini family. `generationConfig`
is attached only to carry `thinkingConfig` on the 2.5 family;
legacy 1.5 / 2.0 builds get no `generationConfig` at all. The new
regression test `build_body_omits_generation_config_on_legacy_families`
pins the behaviour.

Claude / OpenAI / local-OpenAI-compatible still get
`temperature = 0.5` from v0.5.1 — they do not exhibit the
white-out and benefit from the citation-output determinism. The
trade-off is intentional: Gemini gets a more **versatile** sampler
for varied-content tasks, the others stay tight for structured
output.

Everything else from v0.5.1 ships unchanged.

---

## v0.5.1 — 2026-05-26 (citation pipeline overhaul + orphan KB cleanup + A4 doc viewer)

Consolidated stable bundle covering an afternoon of iterative debug on the
v0.5.0 citation pipeline. The user tested four work-in-progress drops in
dev (formerly tagged v0.5.1 → v0.5.4 — those tags are recreated to point
at this consolidated commit; the WIP intermediates are not separately
releasable). Five distinct fix areas land together:

### Citation pipeline — model-independent post-processors

1. **Hybrid bracket splitter** ([`split_hybrid_citation_brackets`](src/routes/chat.rs)).
   Decomposes mixed-content brackets the model occasionally emits
   (`[c1, c2, FILE.pdf, p.4, doc-7, doc-8]`) into clean ones the
   frontend `MARKER_GROUP` regex can pill-ify — `[c1] [c2] [doc-id:
   FILE.pdf, page 4] [doc-id: doc-7] [doc-id: doc-8]`. Idempotent;
   stops at `<CITATIONS>` so it can't corrupt the trailing JSON block
   (regression test pins this).

2. **Cross-message citation lookup** ([`renderMessageHtml`](frontend/src/lib/utils/citations.ts)).
   When a `[cN]` in the current message has no matching annotation
   but an earlier assistant turn in the same chat did, the pill
   resolves to the older annotation. Catches the common case of
   models reusing `cN` labels across turns.

3. **Tightened CITATION QUALITY RULES** in `MRUST_SYSTEM_PROMPT`.
   Five new explicit rules attack the failure modes we measured:
   - Empty / short quotes → omit the citation.
   - Page ranges (`"1-3"`) RESERVED for sentences spanning
     `[[PAGE_BREAK]]`; otherwise `page` MUST be an integer.
   - Prefer per-passage citations over per-document citations.
   - Prefer attached docs (`doc-N`) over KB docs (`gN`/`pN`) when
     attached docs cover the claim.
   - Re-emit cross-turn `[cN]` annotations in the current turn's
     `<CITATIONS>` block.

### Cross-provider determinism + headroom

- **`temperature = 0.5`** on every provider (Claude / Gemini /
  OpenAI / local-OpenAI-compatible). Defaults were 1.0 across the
  board — too random for structured-output workloads. Set in
  [`src/llm/claude.rs`](src/llm/claude.rs),
  [`src/llm/gemini.rs`](src/llm/gemini.rs) and
  [`src/llm/local.rs`](src/llm/local.rs). **Rolled back on Gemini
  in [v0.5.1b](#v051b--2026-05-26-hotfix--gemini-sampler-restored-to-default-for-versatile-heterogeneous-document-analysis)**
  after a long-context white-out was reproduced on
  `gemini-2.5-flash`; see that entry for details.
- **`max_tokens` 4096 → 8192** on Claude + local. Doubles the
  headroom for trailing `<CITATIONS>` JSON on long answers. Gemini
  was already on its default (≥8192 on the 2.x family).

### Orphan KB chunks — chat-time filter + cleanup endpoint

User-reported: after removing a synced doc from the UI, its
embeddings outlived the file on disk. Every chat turn the cosine
retrieval surfaced those stale chunks, the model treated them as
sources, and one chat ended up emitting 12 citations all pointing
to the same dead PDF page 13.

- [`retrieve_kb_chunks`](src/routes/chat.rs) now probes each chunk's
  `source_path` on disk and drops the missing ones with a
  `[rag] orphan KB chunk dropped …` warning + per-turn summary.
- New endpoint **`POST /sync/cleanup-orphans`**
  ([`src/routes/sync.rs`](src/routes/sync.rs)) — per-user cascade
  delete of `documents` + `doc_chunks` + `synced_files` rows whose
  backing file is gone. Returns `{ scanned, orphans, deleted_docs,
  deleted_chunks, deleted_synced }`.
- **Frontend modal** ([`DocViewerPanel.svelte`](frontend/src/lib/components/documents/DocViewerPanel.svelte)):
  when a citation source 404s the viewer surfaces a warning panel
  with a `Pulisci sorgenti rimosse` button that calls the cleanup
  endpoint and toasts the row count.

### DocxView — A4 fit (default) + reflow toggle

[`DocxView.svelte`](frontend/src/lib/components/documents/DocxView.svelte)
now defaults to `fit` mode: preserves the document's native A4 page
geometry (width + height + margins, `breakPages: true`) and applies
CSS `zoom = containerWidth / pageWidth` (clamped to `[0.4, 1.5]`) via
a `ResizeObserver` so the page auto-scales when the user drags the
panel divider. A top-right toggle flips to `reflow` mode (drops
the page geometry, prose flows the full panel width) for narrow
side-panel reading. ResizeObserver is detached in reflow mode so
the responsive cost is zero off-path.

### Diagnostic logging

- `[rag][cite-diag] retrieve_kb_chunks …` now spells out HyDE on/off
  + locale + domain + top-K and notes the **base cosine retrieval
  always runs** regardless of HyDE (HyDE only adds a second pass —
  removes a recurring "but I turned HyDE off, why is the model still
  searching?" confusion).
- `[chat][cite-diag] …` events trace each step of the citation
  pipeline: final response shape (chars + bracket count + presence of
  `<CITATIONS>` markers), tail dump (last 800 chars), per-step
  outcome (`extract_citations_block` Some/None, source path,
  pre-enrichment entry count, FINAL SSE payload size).
- `[chat] <CITATIONS> block found but is not valid JSON` warning
  now dumps the head / mid / tail (300 chars each) of the offending
  payload so we can diagnose model malformations without re-running.

### E2E test scaffold

New [`tests/medical_citations_e2e.rs`](tests/medical_citations_e2e.rs)
bypasses the frontend entirely: places the 10 PDFs from
`tests/medical/` straight into the cache + documents rows, exercises
the real `POST /chat` handler via `tower::ServiceExt::oneshot`,
parses the SSE stream, and prints a structured JSON report of
citation quality (citation count, per-citation page-kind + quote
length, source distribution). Gated by `#[ignore]` + a
`GEMINI_API_KEY` env check; A/B switches via `E2E_HYDE` / `E2E_MODEL`.

Run with:

```pwsh
$env:GEMINI_API_KEY = "..."
cargo test --test medical_citations_e2e --features rag,pdf `
    -- --ignored --nocapture
```

### Tests

47 chat unit tests + 9 splitter unit tests + 49 frontend Vitest
tests all green. No new migrations (HyDE's `hyde_enabled` column
from v0.5.0 is still the latest schema change). 7 new i18n keys
for the cleanup modal (`DocViewer.cleanup.*`) + 4 for the DocxView
toggle (`DocxView.modeFit / modeReflow / switchToFit / switchToReflow`)
across all six locales — 1158 keys per locale.

---

## v0.5.0 — 2026-05-25 (HyDE retrieval + DirectML embeddings)

First minor bump since 0.4.0. Two RAG-layer changes that together raise the
ceiling of what the chat-time retrieval can find without changing the
on-disk vector index or the embedding model, plus a security pass on the
dependency graph and a citation-resolution fix for chats with many
attachments.

### Security pass

- **vite ≥ 6.4.2 / vitest 4.x** — closes GHSA-67mh-4wv8-2f99 (esbuild
  dev-server SSRF) and GHSA-4w7w-66w2-5vf9 (vite optimized-deps path
  traversal). `pnpm audit` clears both moderate advisories.
- **`cargo update`** — bumped 73 transitive crates to latest compatible
  versions. `ort 2.0-rc.9` and `fastembed 4.9.1` were pinned with `=`
  exact-version specs and were not moved (the e5-base / onnxruntime
  1.20.0 ABI match still holds — touching either would re-trigger the
  silent `try_new` deadlock documented in v0.3.x).
- **sqlx `default-features = false`** — explicit feature list
  (`["runtime-tokio", "sqlite", "migrate", "uuid", "chrono", "macros",
  "json", "tls-rustls-aws-lc-rs"]`). Drops the implicit `any` driver
  bundle that pulled `sqlx-mysql` + `sqlx-postgres` into the resolution
  graph for no runtime use.
- **xlsx 0.18.5 retained intentionally.** A swap to ExcelJS was tried
  (commit `2962f1c`) but lost in-app preview for `.xls` / `.xlsb` /
  `.ods` formats — reverted (`a204647`). The two `xlsx` CVEs (Prototype
  Pollution + ReDoS) are accepted as known issues; they're triggered
  only by malicious file content, and the doc-viewer's only input is
  a file the user has explicitly uploaded.
- Residual cargo-audit findings (rsa via sqlx-macros-core, 3×
  rustls-webpki via aws-* under the OFF-by-default `s3-storage`
  feature) are **not reachable in the shipped MSI** — both code paths
  are either build-time-only (sqlx-macros expansion of cross-backend
  query analyzers) or gated behind a feature flag that the default
  build doesn't enable.

### Citation resolver — filename fallback (chat with many attachments)

When >5 PDFs are attached to a chat, the LLM occasionally lifts the
filename straight from the inventory block in the system prompt and
emits it as the citation's `doc_id` instead of the canonical `doc-N`
handle. Both citation paths in [`src/routes/chat.rs`](src/routes/chat.rs)
now fall back to a reverse `filename → UUID` lookup over the chat's
attached docs before giving up — so the viewer no longer 404s on
`GET /document/<filename>/display`, and the inline `[doc-id: filename,
page N]` shape produces the right `[cN]` marker. Pre-existing bug,
not introduced by v0.5.0 — just easier to trip when more files share
one chat.

### HyDE — Hypothetical Document Embeddings (opt-in)

New behavioural toggle in **Settings → Recupero documenti** (default OFF).
When enabled, [`retrieve_kb_chunks`](src/routes/chat.rs) fires one extra
LLM call before vector search: the user's currently-active model is asked
to draft a short pseudo-answer (3–4 sentences, under 600 chars) anchored
on the **domain-aware prologue** introduced in v0.4.0 (legal / medical /
finance / …). Both the original query and the hypothesis are embedded;
the two KNN rankings are merged via **Reciprocal Rank Fusion** (k=60)
before the usual top-K + 0.75 distance threshold + PII filter apply.

Why this helps: e5-base cosine matches passages whose surface form looks
like the embedding input. An Italian legal question ("quanto dura il
preavviso?") is structurally unlike the passage that answers it ("Il
recesso è esercitato con preavviso non inferiore a sei mesi…"); HyDE
closes that gap by giving the embedder something shaped like a passage.

Cost: one extra LLM call per chat turn, billed on the user's active
provider (Anthropic / Gemini / OpenAI / local) — no local compute
overhead beyond a second sqlite-vec KNN. Errors in the HyDE call degrade
gracefully to vanilla cosine retrieval (logged, never silent).

### DirectML execution provider compiled in

`mike-tauri` now depends on `mike` with `features = ["pdf", "ner-pii",
"rag-directml"]`. At runtime `ort` tries to register the DirectML
execution provider (DX12 GPU); if no compatible adapter is present,
it falls back to CPU automatically. No setting, no user knob — just
faster embeddings on every Windows machine with a DX12 GPU
(integrated Iris / Adreno X1 / Radeon / GeForce). Hexagon NPU (QNN)
stays disabled because the default e5-base FP32 model deadlocks
session-init on the HTP backend — that incompatibility is a known
constraint pending an INT8 QDQ-quantised e5 variant.

### New behavioural-toggle policy

A project-wide rule lands with this release: **every new RAG / retrieval
/ reasoning behaviour must surface as an explicit on/off switch in
Settings → Recupero documenti**, with a one-line plain-language
description and a cost callout. HyDE is the first; adaptive top-K,
BM25+RRF fusion, MMR diversification etc. will all land in the same
panel.

### Files touched

- `migrations/0030_user_hyde_enabled.sql` — `user_settings.hyde_enabled` column.
- [`src/llm/hyde.rs`](src/llm/hyde.rs) — new module; domain-aware hypothesis generator.
- [`src/routes/chat.rs`](src/routes/chat.rs) — `retrieve_kb_chunks` wired to HyDE + RRF merge; new `reciprocal_rank_fuse` helper.
- [`src/routes/user.rs`](src/routes/user.rs) — `LlmSettings.hyde_enabled` + `GET/PUT /user/hyde-enabled`.
- [`src-tauri/Cargo.toml`](src-tauri/Cargo.toml) — `rag-directml` enabled by default.
- [`frontend/src/lib/components/settings/RetrievalSection.svelte`](frontend/src/lib/components/settings/RetrievalSection.svelte) — new Settings panel.
- [`frontend/src/routes/Settings.svelte`](frontend/src/routes/Settings.svelte) — new `retrieval` group between AI and About.
- [`frontend/src/lib/api/user.ts`](frontend/src/lib/api/user.ts) — `getHydeEnabled` / `updateHydeEnabled` wrappers.
- 10 new i18n keys (`Settings.retrieval*`) — all six locales now carry 1147 keys.

### Limitations / not in this release

- No runtime UI to inspect *which* EP is active (DirectML vs CPU). Available in logs only.
- Hexagon / QNN remains disabled pending an INT8 QDQ-quantised e5 variant.
- Other ORT EPs (CUDA, ROCm, OpenVINO, CoreML, TensorRT) are not compiled in — they require runtime SDKs we don't bundle.

---

## v0.4.7 — 2026-05-25 (version badge actually renders)

In v0.4.6 the version badge never appeared in the installed MSI
even though the wiring looked correct. Root cause: the store used
`@tauri-apps/api/app::getVersion()` to resolve the value into a
top-level `$state`, then exposed it through a getter. The Promise
resolved in dev, but in the bundled build the assignment inside
the async `.then` callback never propagated reactively to the
brand snippet — so `appVersion.value` stayed `null` and the
`{#if}` guard kept the chip mounted but empty.

### Fix

Replaced the runtime IPC call with a build-time import of
`frontend/package.json` (Vite's first-class JSON import). The
value is now a plain TypeScript string constant the templates can
interpolate directly — no Promise, no `$state`, no reactivity
race, no `{#if}` guard:

```ts
import pkg from '../../../package.json'
export const APP_VERSION: string = pkg.version
```

The release pipeline already bumps `package.json` alongside
`Cargo.toml` and `tauri.svelte.conf.json`, so a single source is
sufficient to label the running build.

### Files touched

- [`frontend/src/lib/stores/app-version.svelte.ts`](frontend/src/lib/stores/app-version.svelte.ts) — rewritten to export `APP_VERSION` const.
- [`frontend/src/routes/Shell.svelte`](frontend/src/routes/Shell.svelte) — drops the `{#if}` guard; renders `v{APP_VERSION}` unconditionally.
- [`frontend/src/lib/components/settings/LicenseSection.svelte`](frontend/src/lib/components/settings/LicenseSection.svelte) — same swap.

---

## v0.4.6 — 2026-05-25 (version label + License settings panel)

The shipped version was invisible to the user — neither the unlock
screen nor the post-login shell showed which build was running, and
there was no UI surface that named the licence the product is
distributed under.

### What changed

- **Sidebar brand**: `MikeRust` in [`Shell.svelte`](frontend/src/routes/Shell.svelte) now renders next to a tiny `v{version}` badge in muted secondary text. The version is pulled at runtime from the Tauri bundle via `@tauri-apps/api/app::getVersion`, so it always matches `tauri.svelte.conf.json` — not whatever `package.json` happens to show in a checked-out source tree.
- **New `app-version` store** ([`stores/app-version.svelte.ts`](frontend/src/lib/stores/app-version.svelte.ts)) caches the result of the single `getVersion()` call. Sync accessor returns `null` until resolved; callers render nothing in that window rather than block.
- **New Settings → License panel** ([`LicenseSection.svelte`](frontend/src/lib/components/settings/LicenseSection.svelte)) shows MikeRust + version, the SPDX identifier (`AGPL-3.0-only`), an introductory paragraph explaining the AGPL terms in plain language, and the full LICENSE file in a scrollable monospace block. The text is the canonical [`LICENSE`](LICENSE) at the repo root, inlined at build time via Vite's `?raw` import — no runtime fetch, no missing-file risk.
- **`vite.config.ts`**: `server.fs.allow: ['..']` lets the dev server resolve the LICENSE one directory above the frontend project root.

### Settings nav

A new `about` group sits between `ai` (LLM models / MCP) and `danger` so the licence panel reads as meta info rather than a destructive control. Five new i18n keys (`App.versionTooltip`, `Settings.license`, `Settings.licenseIntro`, `Settings.licenseSpdx`, `Settings.licenseFullText`) — all six locale bundles now carry 1137 keys.

---

## v0.4.5 — 2026-05-25 (chat-files popover — all five doc categories)

Closes the chat-document audit. The popover now enumerates **every**
document the chat has ever interacted with, not just the
chat-linked ones — matching the explicit "the chat archive must
retain its full document list" requirement:

1. **Uploaded** — composer attachments (`chat_id = ?` AND `content_hash IS NOT NULL`)
2. **Generated** — tool outputs like `generate_docx` (`chat_id = ?` AND `content_hash IS NULL`)
3. **Rejected** — preserved on the original row with `decision='rejected'`; surfaces with the original origin tag plus the existing strikethrough + red badge
4. **Project** — inherited from `chats.project_id` → `documents.project_id = ?`
5. **Referenced** — KB/corpora documents cited by the assistant, extracted from each assistant message's `annotations` JSON (`document_id` field) and looked up against `documents`

Backend precedence on doc-id overlap: chat-linked > project > referenced. A doc that's both uploaded AND cited stays "uploaded" so the most direct relationship wins.

### UI additions

New origin chips in [`ChatFilesPanel.svelte`](frontend/src/lib/components/chat/ChatFilesPanel.svelte):

- `Caricato` — neutral surface
- `Generato` — brand-50 tint
- `Progetto` — success-50 tint (calm green)
- `Citato` — info-50 tint (subtle blue)

Two new i18n keys (`ChatFiles.originProject`, `ChatFiles.originReferenced`) — all six locale bundles now carry 1132 keys.

### Backend

[`get_chat_documents`](src/routes/chat.rs) rewritten to:
- Fetch chat row + `project_id` in one round-trip
- Walk every assistant message's annotations JSON to collect cited document IDs
- UNION-style accumulation with insertion-ordered dedup (precedence enforced by first-write-wins)
- Single tracing log line summarising how many rows came from each branch

### Limitation noted

KB documents that exist only in `corpus_documents` (not mirrored into `documents` with a `corpus_id`) are not enumerable yet — those need a separate JOIN against `corpus_documents` once the mirror coverage is universal. Not a regression; the v0.4.4 popover didn't see them either.

---

## v0.4.4 — 2026-05-25 (chat-files popover — backend-sourced, survives reload)

v0.4.3 missed uploaded attachments after a chat reload, a chat
switch, or any other re-hydration: the popover walked
`chatStore.messages[*].files`, but `GET /chat/:id/messages` does
not echo back the `files` column, so the in-memory FileRef array
was empty for every message that came from the server. Only
generated docs (rehydrated from the persisted `doc_created`
events) survived. After a few turns the user could lose sight of
the 10 PDFs they had uploaded.

### Fix

New backend endpoint **`GET /chat/:id/documents`** in
[`src/routes/chat.rs`](src/routes/chat.rs) returns every row
satisfying `documents.chat_id = ? AND user_id = ?` — which means
both composer uploads (linked when the message is sent) and
`generate_docx` outputs (linked at create-time). Decision columns
ride along in the same row, so the popover paints
strikethrough + `Rifiutato` badge without an N+1 fan-out across
`GET /document/:id`.

[`ChatFilesPanel.svelte`](frontend/src/lib/components/chat/ChatFilesPanel.svelte)
now calls `chatApi.documents(activeId)` on each open. The
"uploaded vs generated" distinction is still derived locally by
intersecting the returned doc IDs with the assistant `steps` of
kind `'doc'` (rehydrated from persisted `doc_created` events);
anything not in that set is classified as uploaded. Result: the
popover correctly enumerates everything the chat has ever
touched, regardless of how many times the chat was reopened.

### Files touched

- [`src/routes/chat.rs`](src/routes/chat.rs) — new
  `get_chat_documents` handler + route registration.
- [`frontend/src/lib/api/chat.ts`](frontend/src/lib/api/chat.ts) —
  `chatApi.documents(id)` wrapper.
- [`frontend/src/lib/components/chat/ChatFilesPanel.svelte`](frontend/src/lib/components/chat/ChatFilesPanel.svelte) —
  uses the new endpoint; doc-id walk only used to label origin.

No new i18n keys; the existing `ChatFiles.hydrating` label still
applies (the call loads both the file list and decision state in
one round-trip).

---

## v0.4.3 — 2026-05-25 (chat-files shortcut)

After v0.4.1/v0.4.2 made the rejection summary discoverable, a
second gap surfaced: once the user closed the reject modal there
was no quick way back to the document itself — the `generate_docx`
chip lives in the scrollback, which gets buried as the chat grows.

This release adds a **chat-files popover** anchored to a new Files
button in the composer footer. It enumerates every document the
current chat has touched and exposes them in a flat, scannable
list:

- **Source-origin tags** — `Caricato` for user-attached files,
  `Generato` for `generate_docx` (and any future generator) outputs.
- **Per-format icon colour** — Excel green, Word blue, PDF red,
  PowerPoint orange, Markdown text-primary, anything else brand
  accent. The mapping was extracted from `ChatSteps.svelte` into
  the new [`fileIconColor`](frontend/src/lib/utils/file-icon.ts)
  utility so chat steps and the shortcut paint identically; one
  source of truth for the file palette.
- **Strikethrough + red `Rifiutato` badge** on rejected rows. The
  decision is fetched on open via `documentsApi.get` in parallel
  for every row, so the list reflects current state without needing
  the user to have opened each doc in the viewer first.
- **Click → opens in the existing doc-viewer side panel**, where
  Accept / Reject / Vedi riassunto / Apri in Word all already work.

### Files touched

- [`frontend/src/lib/utils/file-icon.ts`](frontend/src/lib/utils/file-icon.ts) — new shared helper.
- [`frontend/src/lib/components/chat/ChatFilesPanel.svelte`](frontend/src/lib/components/chat/ChatFilesPanel.svelte) — new popover.
- [`frontend/src/lib/components/chat/ChatInput.svelte`](frontend/src/lib/components/chat/ChatInput.svelte) — Files button + popover mount.
- [`frontend/src/lib/components/chat/ChatSteps.svelte`](frontend/src/lib/components/chat/ChatSteps.svelte) — now imports `fileIconColor` instead of inlining the switch.
- [`frontend/scripts/fill-i18n.mjs`](frontend/scripts/fill-i18n.mjs) — six new `ChatFiles.*` strings re-filled into the six locale bundles (now 1130 keys each).

Backend unchanged: the popover reads exclusively from the
already-streamed `chat.messages` array (user `files` + assistant
`steps`) plus per-doc `GET /document/:id` for decision state.

---

## v0.4.2 — 2026-05-25 (reject modal — step 2 transition fix)

Hotfix for v0.4.1. After a user clicked **Genera riassunto e rifiuta**
the backend correctly persisted the reject and returned the summary,
but the modal stayed pinned in step 1 (the reason textarea) instead
of transitioning to the step 2 preview. The user was left guessing
whether the action had landed.

### Root cause

`RejectReasonModal`'s `$effect` reset block tracked `initialReason`
reactively:

```svelte
$effect(() => {
  if (open) {
    reason = initialReason ?? ''  // ← tracked dependency
    summary = null
    busy = false
  }
})
```

When `generateAndPersist` resolved it called
`docViewer.setDecision(docId, 'rejected', res.reason, res.summary)`,
which mutated the store. The parent re-rendered the modal with a
fresh `initialReason` prop (the archived reason from the just-
confirmed reject), and the effect re-ran while `open` was still
true — clobbering `summary` back to `null` immediately after
`generateAndPersist` had set it. The `{#if summary === null}`
template branch then drew step 1 again.

### Fix

Read `initialReason` inside `svelte`'s `untrack` so the effect only
fires on the `open` transition, not when the parent updates the
archived reason mid-flow:

```svelte
$effect(() => {
  if (open) {
    reason = untrack(() => initialReason ?? '')
    summary = null
    busy = false
  }
})
```

Same reset behaviour on each modal open; no re-entry during the
await. The same race would have hit the very first reject if the
parent updated the prop while the API call was pending — the new
guard fixes both paths uniformly.

Single file touched:
[`frontend/src/lib/components/documents/RejectReasonModal.svelte`](frontend/src/lib/components/documents/RejectReasonModal.svelte).

---

## v0.4.1 — 2026-05-25 (rejection summary — persistent view)

Closes the UX gap exposed by v0.3.5's Accept/Reject flow: once
the user typed a motive and the backend generated an LLM summary,
the summary was visible only inside the modal that authored it.
Closing the modal lost the read surface, even though the summary
remained the *active* document substitute in every subsequent
chat turn (per [`chat::load_attached_docs`](src/routes/chat.rs)).

### What changed

- New **"Vedi riassunto"** button in the doc-viewer toolbar,
  visible exactly when `tab.decision === 'rejected'`. It opens a
  read-only modal showing both the user's reason and the
  archived summary — the same two strings the model sees in
  place of the rejected document.
- The new modal is purely a preview surface: no LLM call, no
  mutation, just a window on what the store already has after
  `hydrateDecision` or a freshly-confirmed reject.
- i18n: seven new keys across all six locales
  (`DocViewer.decision.viewSummary`, the matching tooltip, and
  five `DocViewer.summaryModal.*` strings).

### The canonical 8-step lifecycle, now fully observable

1. MikeRust generates a docx from a template.
2. The user can reject it and write a motive (≥10 chars).
3. Backend pulls the cached extracted text, runs a one-shot LLM
   summary anchored on the motive, and persists
   `decision='rejected' + decision_reason + decision_summary`.
4. From the next turn on, [`load_attached_docs`](src/routes/chat.rs)
   injects the reason + summary stub **in place of** the docx
   body.
5. The original generated docx is no longer sent to the model
   (the stub fully replaces it; the if-rejected branch returns
   before any text load).
6. The user can re-open the doc, click **Vedi riassunto** to
   re-read what's now active, then click **Accetta** to restore
   the document.
7. On re-accept the column flips to `accepted` — the doc goes
   back into chat context, the summary disappears from the
   active prompt but stays archived (`decision_reason` and
   `decision_summary` are intentionally preserved).
8. On re-reject the new motive + new summary overwrite the
   archived values via the same `UPDATE` path.

### Files touched

- [`frontend/src/lib/components/documents/ViewSummaryModal.svelte`](frontend/src/lib/components/documents/ViewSummaryModal.svelte) — new read-only modal.
- [`frontend/src/lib/components/documents/DocViewerPanel.svelte`](frontend/src/lib/components/documents/DocViewerPanel.svelte) — toolbar button + modal mount.
- [`frontend/scripts/fill-i18n.mjs`](frontend/scripts/fill-i18n.mjs) — seven new translation entries; the script re-fills the six locale bundles to 1124 keys.

No backend changes were necessary — `set_decision` (v0.3.5) and
`load_attached_docs` already implement steps 3–8 correctly; v0.4.1
just makes the persisted summary visible to the user.

---

## v0.4.0 — 2026-05-24 (domain-aware system-prompt prologue)

The chat handler now prepends a **domain-aware prologue** to every
turn's system prompt, giving the model an explicit role + working
language + presumed jurisdiction before the generic Mike rules
(tool-use, citations, SSE shape) kick in. The prologue is bundled as
editable `.md` files under `config/system-prompts/<locale>/<domain>.md`
so the operating organisation can tweak the persona without touching
the binary.

### How the prologue is composed

At `stream_chat` time the backend resolves:

- **Domain** — `project.domain` if the chat is in a project,
  otherwise `user_settings.default_domain`, otherwise `"others"`.
- **Locale** — `user_settings.locale` (set in Settings → General),
  otherwise `"it"` (MikeRust's primary user base).
- **Default country** — derived from the locale via a fixed map
  (`it → Italy`, `fr → France`, `de → Germany`, `es → Spain`,
  `pt → Portugal`, `en → "unspecified (ask the user)"`).

Then `crate::presets::system_prompt::resolve(locale, domain)` walks
the fallback chain:

```
1. <root>/<requested-locale>/<domain>.md
2. <root>/it/<domain>.md           (primary curated set)
3. <root>/en/<domain>.md           (universal fallback)
4. None — composer falls back to "generic professional"
```

`<root>` is found via the same env-var / cwd-ancestor / exe-ancestor
walker the other preset registries use, so dev (cwd = repo root) and
installed-MSI (`<install>/config/system-prompts/…`) both work without
configuration. Locale path-traversal (`../etc`) is hard-rejected by
`sanitize_segment` before any filesystem read.

### Country disambiguation

The wrapper text the composer wraps around the `.md` body ends with:

> Country disambiguation: If the user's request involves a country,
> regulation, or legal/medical/professional framework that does not
> match the default above, ASK the user which country / jurisdiction
> applies BEFORE giving jurisdiction-specific advice. Do not silently
> assume.

So a Spanish lawyer who pastes an Italian contract gets a polite
"which jurisdiction applies?" prompt instead of a wrong-country
answer, and an Italian medical-legal expert asking about a German
patient is steered to confirm the framework before applying SIMLA
tables.

### File set shipped

**66 files** under `config/system-prompts/`:

```
config/system-prompts/
├── it/   11 domain prompts — curated, ~30 lines each, primary set
├── en/   11 domain prompts — native English equivalents
├── fr/   11 domain prompts — French-jurisdiction adaptations
├── de/   11 domain prompts — German-jurisdiction adaptations
├── es/   11 domain prompts — Spanish-jurisdiction adaptations
└── pt/   11 domain prompts — Portuguese-jurisdiction adaptations
```

Each `.md` follows a five-section template:

1. Header (role + default country + working language)
2. Priority capabilities (4-7 bullets)
3. Operating constraints (3-4 bullets, includes the
   "never produce a conclusive opinion without disclaimer" rule)
4. Country / jurisdiction (default + ASK rule for cross-border cases)
5. Style (terminology, citation form, response structure)

### Implementation files

- `src/presets/system_prompt.rs` (new) — `resolve`,
  `assemble_prologue`, `default_country_for_locale`,
  `language_name_for_locale` + 8 unit tests covering locale
  fallback, missing domain, path-traversal rejection, prologue
  assembly with/without an `.md` body, country mapping.
- `src/presets/mod.rs` — exposes the new module alongside `column`,
  `docx_template`, `model`, `workflow`.
- `src/routes/chat.rs` — `stream_chat` now SELECTs
  `(locale, default_domain)` from `user_settings`, picks the
  effective domain (project > user > "others"), composes the
  prologue, prepends it to `MRUST_SYSTEM_PROMPT` in the
  `sections` vector.
- `scripts/build-release.ps1` — adds
  `../config/system-prompts/**/*.md` to `bundle.resources` so the
  installed MSI carries the 66 files at
  `<install>/config/system-prompts/<locale>/<domain>.md`.

### Tests

`cargo test --features ner-pii --lib presets::system_prompt` — 8/8
passing (locale fallback chain, missing domain, path-traversal
rejection, prologue wrapping, country mapping). `cargo check
--features ner-pii` clean. `pnpm exec svelte-check` 0/0.

### Installer artefacts

- `dist/MikeRust_0.4.0_x64.msi` — Windows x86_64
- `dist/MikeRust_0.4.0_arm64.msi` — Windows ARM64

---

## v0.3.6 — 2026-05-24 (generate_docx unblocked on multi-doc anamnesis)

Two related regressions surfaced while attempting the v0.3.5 docx
generation flow on a 10-cartella-clinica medical-anamnesis workflow
with Gemini 2.5 Flash:

### Fixed — Gemini `tool_code` leak with triple-quoted multi-line body

When Gemini decides to call `generate_docx` via its
"code-execution" output mode (a quirk of 2.5 Flash that the
official SDKs strip but our raw-REST stream doesn't), it emits the
call as Python prose:

```
read_document
tool_code
print(default_api.generate_docx(body='''# Lettera di Dimissione

Dati Paziente
Nome: …
''', metadata={…}, template_id='it/lettera-dimissioni'))
thought
The user asked to generate a DOCX…
```

The existing `try_parse_tool_code_prose` adapter caught the
single-line / single-quoted variants but blew up on this one
because (a) the body used Python triple quotes `'''…'''`, (b) the
prose was preceded by a leftover `read_document\ntool_code\n`
header from earlier in the turn, (c) the body contained Unicode
characters like `Undómiel` that a byte-level state machine
mangled. Net effect: the parser bailed, the prose landed as a
literal markdown blob in the chat, and the model never produced
the requested document.

- `try_parse_tool_code_prose` now anchors on the first
  `default_api.` substring anywhere in the text — every leading
  envelope shape (`tool_code`, `print(`, code-fence, prefix
  garbage) is tolerated implicitly.
- `extract_balanced_paren_contents` handles `'''…'''` and
  `"""…"""` triple-quoted strings: stays inside the string until
  three consecutive matching quote chars, ignores parens /
  brackets / single quotes inside.
- `split_top_level_commas` rewritten to iterate `chars()` (was
  walking bytes-as-chars, which corrupted any non-ASCII codepoint
  inside the body).
- `python_literal_to_json` recognises `'''…'''` / `"""…"""`
  before the single-quote branch so triple-quoted values aren't
  mistaken for a `''` + content + `''` sandwich.
- New regression test reproduces the full Gemini transcript
  shape (multi-line body, `read_document` prefix, accented
  Italian characters in the body, three kwargs including a
  nested `metadata` dict).

### Fixed — off-by-one between `CARTELLA_TEST_010.pdf` and `doc-10`

With 10 attachments named `CARTELLA_TEST_001..010.pdf` the model
reasoned about "the tenth document" and called the tools with
`doc-10`. The chat-local label map was 0-indexed (`doc-0..doc-9`),
so `read_document(doc_id="doc-10")` returned "document not found"
and the model apologised instead of finishing the workflow.

- `build_doc_system_prompt` and the `doc_label_map` constructor
  in `routes/chat.rs` now emit `doc-1..doc-N` labels.
- Tool schemas in `src/llm/builtin_tools.rs` updated to use
  `doc-1` / `doc-2` as example labels in the descriptions.
- The CITATIONS prompt's example block in `routes/chat.rs`
  updated to match.

### Installer artefacts

- `dist/MikeRust_0.3.6_x64.msi` — Windows x86_64
- `dist/MikeRust_0.3.6_arm64.msi` — Windows ARM64

---

## v0.3.5 — 2026-05-24 (docx Accept/Reject decision flow + Open-in-Word)

The DocViewerPanel's three buttons over a generated .docx — historically
a rendering preview toggle (`Tracked Change` / `Accept` / `Reject` of
the in-doc markup) — were doing nothing the user cared about: flipping
between them changed only what the panel rendered, not what the model
received on the next turn. The same rejected docx kept entering the
LLM payload because `load_attached_docs` auto-attaches every row
linked to the chat (`chat_id` foreign key), and the user had no way to
explain *why* a rejection happened.

This release turns those buttons into real operational decisions on
the per-chat lifecycle of a generated document.

### Schema — migration `0029_documents_decision`

- `documents.decision TEXT NOT NULL DEFAULT 'accepted'`
  CHECK in `('accepted', 'rejected')`.
- `documents.decision_reason TEXT` — free-text user explanation
  captured when the user clicks Reject.
- `documents.decision_summary TEXT` — one-shot LLM summary of the
  rejected version, generated server-side at reject-time and
  preserved across decision flips so a re-reject doesn't pay the
  summarisation cost twice.

Decision scope is **per-chat** by construction: every generated docx
already lives on a single `chat_id` (set by `generate_docx` at
creation), so a per-document column is implicitly per-chat. A docx
that wants to escape this scope is re-uploaded as a new attachment
in a different chat — that gets its own fresh decision row.

### Backend

- New endpoint `POST /document/:id/decision` with body
  `{ decision: "accepted" | "rejected", reason?: string }`. On
  reject the handler (a) refuses requests with a reason shorter
  than 10 chars, (b) reads the document's extracted text from the
  cached `extracted_text_path` (or runs a fresh
  `extract_text_dispatch` if no cache exists), (c) builds a
  rejection-aware summarisation prompt and calls the same LLM
  provider the user picked in Settings — Claude / OpenAI-compat /
  Gemini — via `crate::llm::summarize`'s `complete` per-provider
  path, (d) persists `decision`, `decision_reason`,
  `decision_summary`. On accept it just flips the column and
  returns the archived reason/summary so the UI can keep them
  available for a future re-reject without retyping.
- `GET /document/:id` extended to surface the three new fields.
- `routes/chat.rs::load_attached_docs` filters documents by
  `decision` per-turn: a rejected doc is substituted with a synth
  payload — `"[Documento rifiutato dall'utente]\nFilename: …\n
  Motivo del rifiuto: …\nRiassunto della versione rifiutata: …\n
  Indicazione operativa: non riprodurre questa versione…"` —
  rather than its full text, so the model on subsequent turns
  knows what was vetoed and why without re-seeing the vetoed
  bytes.

### Tauri shell — Open in Word

- New IPC command `open_external_path` in `src-tauri/src/lib.rs`.
  Accepts an absolute path; canonicalises it; rejects anything
  that doesn't sit under `<home>/mikerust-data/storage/`,
  `$STORAGE_PATH`, or the OS temp dir; hands the rest to
  `open::that` so Word / LibreOffice picks up the file with its
  native Track Changes UI. Frontend never fabricates the path:
  it asks the auth-gated `GET /document/:id/file_path` and
  passes the response string straight through.

### Frontend

- New `DocViewer.decision` segmented control inside
  `DocViewerPanel.svelte` with strong colour states: green fill +
  ✓ when `accepted`, red fill + ✗ when `rejected`, both visible
  against the toolbar background instead of the previous near-
  invisible `bg-surface-100`. ARIA `role="radiogroup"` + per-
  button `aria-checked` so screen readers and keyboard navigation
  surface the tri-state nature the old three separate `<button>`s
  obscured. A `Rejected: <reason>` chip appears beside the
  group when the doc is currently rejected so the why is visible
  without reopening the modal.
- New `RejectReasonModal.svelte` — two-step requester: (1) the
  user types the rejection reason (`min 10 chars`, validated
  inline); on confirm the modal calls
  `POST /document/:id/decision`, which fires the LLM
  summarisation; (2) the modal displays the summary it just
  received side-by-side with the user's reason for transparency,
  with a single "Close" button. The rejection is persisted at
  step 1 — step 2 is a preview, not a confirmation gate
  ("Reject means reject"). Re-fills from the archived reason
  every time it opens so a re-reject doesn't lose prior text.
- New "Apri in Word" toolbar button (lucide `<ExternalLink />`)
  replaces the old "Tracked Change" preview toggle. The
  preview-toggle semantics had become noise once the buttons
  stopped being decorative; the new behaviour hands the docx
  off to the user's preferred editor where they *can* actually
  accept/reject individual changes inline.
- `doc-viewer` store gains `decision`, `decisionReason`,
  `decisionSummary` per tab plus a `setDecision` action.
  `hydrateDecision` runs on tab open via `GET /document/:id`,
  so the toolbar reflects the current backend state.
- New i18n keys under `DocViewer.decision.*`,
  `DocViewer.openExternal.*`, `DocViewer.reject.*`,
  `DocViewer.accept.error` — 22 new strings × 6 locales (en +
  it canonical, fr/de/es/pt via `scripts/fill-i18n.mjs`'s
  translation table). i18n parity now at 1117 keys × 6 locales.

### Installer artefacts

- `dist/MikeRust_0.3.5_x64.msi` — Windows x86_64
- `dist/MikeRust_0.3.5_arm64.msi` — Windows ARM64

---

## v0.3.4 — 2026-05-24 (generate_docx — strip chat-citation markers)

### Fixed — exported .docx peppered with unresolvable `[c104]` pills

User report on the medical-anamnesis workflow: the produced .docx
contained inline citation markers like `[c95]`, `[c104]`, `[c168,
c169]` mixed into the prose. In the chat UI those are clickable
pills that jump to the source document; in a Word document opened
on a different machine they are unresolvable noise — the reader
sees `[c104]` and has no legend to map it back to a filename. The
model already lists source filenames in an "Allegati" / appendix
section the user is happy with; the inline markers were dead weight.

- New private helper inside `src/llm/builtin_tools.rs` strips
  `[c|g|p N]` and `[c|g|p N, c|g|p M, …]` patterns from the `body`
  before it reaches the docx renderer (template-driven branch *and*
  the legacy plain-markdown branch). Two follow-up sweeps clean up
  the artefacts a removed marker leaves behind: " ." → "." and
  "word  word" → "word word". Paragraph breaks (`\n`) are
  preserved intact.
- Compiled once via `LazyLock<Regex>` so the strip is amortised
  across all `generate_docx` calls in a process lifetime.
- The chat-UI flow is untouched: `[cN]` pills still render
  perfectly inside the assistant message, with their clickable
  doc-viewer jump behaviour. The strip only fires when the body is
  about to be baked into a binary docx that will likely outlive
  the chat session.

### Followups for a future release
- README update isn't needed (internal change). HISTORY entry +
  this note is the user-facing documentation.

### Installer artefacts
- `dist/MikeRust_0.3.4_x64.msi` — Windows x86_64
- `dist/MikeRust_0.3.4_arm64.msi` — Windows ARM64

---

## v0.3.2 — 2026-05-24

Three independent fixes batched into one release.

### Fixed — chat stops with "too many tool iterations" on multi-doc workflows

Symptom (Gemini 2.5 Flash, medical anamnesis workflow with 10 attached
PDFs): the model called `read_document` on a couple of files and then
the turn aborted with "stopped: too many tool iterations". The
`MAX_TOOL_ITERATIONS` cap in `src/routes/chat.rs` was 5 — a holdover
from early single-doc debug runs. A legitimate due-diligence /
medical-anamnesis flow needs to read 5–15 source documents before
composing the answer; the cap fired well before the model finished.

- `MAX_TOOL_ITERATIONS` bumped from `5` to `20`. Per-iteration cost is
  one LLM round-trip, so 20 bounds a runaway loop at ~20× the per-turn
  latency budget while comfortably covering ten-doc anamnesis flows.

### Fixed — Settings → "Modelli LLM" probe CORS-blocked

Symptom (`mike-tauri.log` + browser console):

```
Access to fetch at 'http://192.168.196.100:11434/v1/models'
  from origin 'http://tauri.localhost'
  has been blocked by CORS policy
```

The Settings page used to issue `fetch(${base}/models)` directly from
the WebView origin `http://tauri.localhost`. External Ollama / llama-
server / vLLM instances rarely advertise that origin in their
`Access-Control-Allow-Origin` header, so the browser blocked every
probe and the model dropdown stayed empty.

- New backend endpoint `GET /models/local/probe?base=…&api_key=…` does
  the server-to-server fetch (no Origin header involved, no CORS) and
  returns the upstream payload verbatim. Typed error responses
  (`upstream_status`, `error`) replace the opaque "Failed to fetch".
- `ModelsSection.svelte`'s `refreshLocalRuntimeModels` now calls the
  backend proxy instead of fetching the runtime directly.
- The chat path was never affected — chat-time LLM calls already go
  through the Rust `reqwest` client.

### Fixed — biometric unlock dialog used a hand-drawn fingerprint

The Windows Hello / Touch ID overlay (`BiometricPrompt.svelte`) drew
its fingerprint via seven inline SVG paths. The result looked off-
centre and hairline-thin against the brand-500 background and didn't
match the rest of the UI's lucide-svelte icon family.

- Swapped the inline SVG for `<Fingerprint size={40}
  class="text-(--color-brand-500)" />` from `lucide-svelte` (same
  package used by every other icon in the app). The `currentColor`
  inheritance preserves the brand-tint behaviour, so no styling
  change at the call site.

### Installer artefacts

- `dist/MikeRust_0.3.2_x64.msi` — Windows x86_64
- `dist/MikeRust_0.3.2_arm64.msi` — Windows ARM64

---

## v0.3.1 — 2026-05-24 (installed-MSI ships config registries)

### Fixed — empty workflow / column / docx-template picker on installed MSI

Symptom: opening the workflow picker in the installed app showed
"None"; the column-preset auto-match dropdown was empty; the
docx-template registry surfaced no system templates; and the model
catalogue dropdown only listed manually-saved providers. Backend
warnings in `mike-tauri.log` confirmed the loaders fell through to
the cwd-relative `./config/<dir>/` fallback after the ancestor
walker found nothing — the MSI shipped only the binary + `libs/`,
no `config/` tree.

- [scripts/build-release.ps1](scripts/build-release.ps1)'s
  `New-ResourcesOverlay` now bundles four extra JSON sources via
  glob patterns:
  - `config/workflow-presets/**/*.json` →
    `<install>/config/workflow-presets/…` (preserves the per-domain
    subfolders: compliance / finance / insurance / legal / medical / pa)
  - `config/column-presets/**/*.json` →
    `<install>/config/column-presets/…`
  - `config/docx-templates/**/*.json` →
    `<install>/config/docx-templates/…`
  - `config/model.json` → `<install>/config/model.json`
- The existing `crate::presets::presets_dir` ancestor walker now
  finds them at `<exe_dir>/config/<dir>/` without any code change —
  the resolver was already prepared for this layout, the MSI just
  wasn't shipping the files.
- `config/corpora-plugins/` is deliberately *not* bundled: the
  plugin system uses its own resolver and pre-built installs don't
  ship third-party plugins.

### Installer artefacts

- `dist/MikeRust_0.3.1_x64.msi` — Windows x86_64
- `dist/MikeRust_0.3.1_arm64.msi` — Windows ARM64

---

## v0.3.0 — 2026-05-23 (installed-MSI storage path)

### Fixed — "Could not load document" (Windows `os error 5`)

Symptom in the installed MSI: uploading a document, opening the
viewer, or reading the chat document cache surfaced
"Could not load document — Accesso negato. (os error 5)" with the
backend logging `ACCESS_DENIED` on file-system writes. Reproduced
from `<home>/mikerust-data/mike-tauri.log` (v0.2.2 file logger) —
the storage layer tried to `create_dir_all` `./data/storage/`, a
cwd-relative path. For a developer running `cargo run` from the
workspace root that resolves to `C:\Progetti\MikeRust\data\storage\`
and works. For an MSI launched from a Start-menu shortcut the cwd
is often `C:\Program Files\MikeRust\` (or `C:\Windows\System32` via
"Run"), neither of which is writable by a non-admin user. The
first `put` then failed with `os error 5` and `/document` /
`/upload` returned HTTP 500.

- [src/storage/mod.rs](src/storage/mod.rs) `LocalStorage::new()`
  falls back to a new helper `default_storage_path()` instead of
  `"./data/storage"`. The default now mirrors `db::default_db_url`:
  `<USERPROFILE | HOME>/mikerust-data/storage/`. Same user-writable
  directory the SQLite DB and the v0.2.5 PII cache already live
  under, so a full backup is one folder copy. The
  `STORAGE_PATH` env override is preserved for tests, fixtures,
  and the standalone-backend dev story.

### Installer artefacts

- `dist/MikeRust_0.3.0_x64.msi` — Windows x86_64
- `dist/MikeRust_0.3.0_arm64.msi` — Windows ARM64

---

## v0.2.7 — 2026-05-23 (patch — close the remaining PII bypasses)

The v0.2.6 fix closed the inline-attached redaction path for
follow-up turns but missed two parallel ways the LLM was still
receiving raw, unredacted document text. Confirmed on disk by
grepping the v0.2.5 cache file — every original license plate
**had** been masked to `[LICENSE_PLATE]` by GLiNER, yet the model
answer quoted all nine plates verbatim. They were leaking
through the KB-retrieval branch and (potentially) through the
read_document / find_in_document tools.

### Fixed — KB retrieval bypass

`retrieve_kb_chunks` returns raw indexed chunks of every
PII-protected document that semantically matched the user's
query. The inline-attached path was carefully redacting the
full text while the KB path served the same passages
unredacted, in parallel, on the same turn.

- `retrieve_kb_chunks` now consults
  `documents.pii_protected` per chunk and drops every chunk
  whose source document has the flag set. The user still gets
  the document content through `load_attached_docs`, which
  serves the anonymised `cache/pii/<doc_id>.txt`.
- The drop is logged at info level
  (`[rag] dropping KB chunk from PII-protected doc_id=…`) so
  the redacted retrieval is observable from
  `mike-tauri.log`.

### Fixed — read_document / find_in_document tool bypass

Both built-in tools resolved `documents.storage_path` and
read the raw bytes — the model could call them on a
PII-protected `doc-N` label and recover the full text
sidestepping every redaction layer.

- `resolve_doc` now also returns the real document id and the
  `pii_protected` column.
- New helper `read_doc_text_for_llm` substitutes the cached
  redacted text (`cache/pii/<doc_id>.txt`) for protected
  documents instead of running fresh extraction on the raw
  bytes. Cache miss is treated as a safety stop: the tool
  returns an error ("wait for the inline-attached pass to
  finish") rather than falling back to the raw text.
- `edit_document` refuses outright when targeting a
  PII-protected document — overwriting the raw docx and
  returning it as a download would re-expose every entity the
  redacted cache had carefully masked.

### Installer artefacts

- `dist/MikeRust_0.2.7_x64.msi` — Windows x86_64
- `dist/MikeRust_0.2.7_arm64.msi` — Windows ARM64

---

## v0.2.6 — 2026-05-23 (patch — PII leak fix)

### Fixed — PII protection lost on follow-up chat turns (CRITICAL)

User report: turn 1 with a PII-protected PDF redacted as expected
(model answer referenced `[LICENSE_PLATE]` etc.); turn 2 on the
same document (a plain text follow-up question) came back with the
*original* customer name and *every* unmasked license plate. The
leak was real — but it wasn't a GLiNER recall problem. The
`pii_protected` flag lived only on the message payload, so when the
follow-up text-only message arrived, the recovery code in
`stream_chat` re-attached every document linked to the chat (see
the `SELECT id FROM documents WHERE chat_id = ?` block) and
`load_attached_docs` ran without a flag for those auto-attached
docs — i.e. without any redaction at all. The full unredacted text
went to the LLM, which happily quoted it.

- New migration **`0028_documents_pii_protected.sql`** adds an
  `INTEGER NOT NULL DEFAULT 0` column on `documents`. The flag is
  now durable: once a user opts-in on a file, it stays protected
  for every later turn of every chat that references it.
- `stream_chat` UPSERTs `pii_protected = 1` on every document id
  the current request payload tagged. The write is one-way — the
  chat handler never resets the column to 0, only opt-in flips it.
- `load_attached_docs` extends its `SELECT` to include the new
  column and OR-merges it with the per-request `pii_protected_ids`
  set. Either source flips the doc into the redaction branch, so a
  follow-up text-only turn no longer needs the frontend to repeat
  the flag.
- The v0.2.5 `cache/pii/<doc_id>.txt` cache layer is now the
  steady-state behaviour: turn 1 pays full GLiNER inference and
  writes the cache; turn N reads the cache in milliseconds.

### Known follow-up — GLiNER recall on Italian PDFs

This release closes the leak from missing follow-up redaction, but
the same user report exposed a second, separate issue: even on the
first turn GLiNER missed `SOCIETA' A. TEC. S.R.L.` (an
`organization`) and most of the `license_plate` instances. That's a
zero-shot recall gap, not a wiring bug — tracked separately, will
likely need a lower threshold for Italian text or extra label
framings (`targa`, `azienda`).

### Installer artefacts

- `dist/MikeRust_0.2.6_x64.msi` — Windows x86_64
- `dist/MikeRust_0.2.6_arm64.msi` — Windows ARM64

---

## v0.2.5 — 2026-05-23 (patch)

### Added — persistent PII-redacted text cache

PII inference is the most expensive single step in
`load_attached_docs` (22-35 s per 2000-char chunk on CPU; a
9-page PDF is ~3-5 min). The previous release rerun the full
GLiNER pipeline on every chat turn that referenced a
PII-protected document, even if the *same* document had
already been anonymised seconds earlier in a previous turn of
the same chat. The user observation was textbook: send a
turn → wait 3 minutes for PII → send a follow-up question on
the same document → wait 3 minutes for PII *again*.

Because the label set
(`crate::ner::default_pii_labels`), the threshold
(`crate::ner::PII_THRESHOLD = 0.2`), and the chunking window
are all pinned at compile time, the redacted output is
deterministic for a given input — so the second pass
guarantees the same bytes as the first. We now persist that
result and consult it before any inference.

- `routes/chat.rs::pii_cache_key(doc_id)` resolves to
  `cache/pii/<doc_id>.txt` under the existing
  `crate::storage::make_storage()` LocalStorage (same root as
  the raw `extracted_text_path` cache, so a future
  garbage-collection sweep over deleted documents can prune
  both with a single prefix walk).
- `maybe_redact_pii` checks the cache before touching GLiNER.
  On hit it emits the same `pii_redact_start` / `pii_redact_done`
  SSE pair (with `total=1`) so the chat UI still shows the
  step transition consistently — the difference is the user
  sees it complete in milliseconds instead of minutes.
- On a successful (miss-then-redact) pass the result is
  written back via `storage.put()`. Failure to write is
  non-fatal: the user still gets a correctly anonymised
  payload this turn, only the *next* turn pays the inference
  cost again.

The key is `doc_id` alone for now. When the label set
eventually becomes a per-request input (e.g. a custom
profile), the key will pick up a labels hash so different
label combinations don't clash.

### Installer artefacts

- `dist/MikeRust_0.2.5_x64.msi` — Windows x86_64
- `dist/MikeRust_0.2.5_arm64.msi` — Windows ARM64

---

## v0.2.4 — 2026-05-23 (patch)

### Fixed — CORS reject of Windows Tauri WebView origin

With the v0.2.3 race-fix landed and the WebView finally reaching the
real backend URL, the boot fetch surfaced a different stall:

```
Access to fetch at 'http://127.0.0.1:59834/healthz'
  from origin 'http://tauri.localhost'
  has been blocked by CORS policy: No 'Access-Control-Allow-Origin'
  header is present on the requested resource.
```

The backend's CORS allowlist already carried `tauri://localhost`
(legacy custom-scheme variant) and `https://tauri.localhost` (the
macOS WRY shape) but was missing `http://tauri.localhost` — the
exact `Origin` header Windows / WebView2 sends in release builds.
The dev story (`tauri dev`) never hit this because Vite serves the
frontend at `http://localhost:5173`, an origin that *was* on the
list, so the regression was invisible until the MSI launch path
was actually exercised.

- `src/lib.rs` default CORS allowlist now includes
  `http://tauri.localhost` alongside the two existing Tauri shapes.
- The `MRUST_ALLOWED_ORIGINS` env override path is unchanged — a
  deployer who pins the list still gets exactly what they pinned.

### Installer artefacts

- `dist/MikeRust_0.2.4_x64.msi` — Windows x86_64
- `dist/MikeRust_0.2.4_arm64.msi` — Windows ARM64

---

## v0.2.3 — 2026-05-23 (patch)

### Fixed — frontend → backend race on cold MSI launch

Symptom: on a freshly installed MSI, opening MikeRust showed the Svelte
"Impossibile raggiungere il backend — Network error: Failed to fetch"
banner. The new v0.2.2 file logger
(`<home>/mikerust-data/mike-tauri.log`) made the diagnosis instant: the
backend actually started fine, bound `127.0.0.1:59209` and reported
the URL via `api_base_url`, but only after ~1 s of preset loading
(81 workflow presets + 30 column presets + 13 docx templates + 5
model providers + DB migrations + `ort::init`). The WebView booted
faster, fired its first `invoke('api_base_url')` while the backend
was still loading presets, got back an empty string, and fell
straight through to the hardcoded `http://127.0.0.1:3001` fallback —
which was guaranteed to refuse the connection.

- [frontend/src/lib/tauri/commands.ts](frontend/src/lib/tauri/commands.ts)
  `getApiBaseUrl()` now polls `invoke('api_base_url')` with
  exponential backoff (50 ms → 75 ms → 113 ms…, capped at 1 s) for
  up to 30 s. Once the embedded axum task fires its `port_tx`, the
  next `invoke` returns the real URL and the boot sequence
  continues normally. The old fallback to
  `VITE_API_BASE_URL` / `http://127.0.0.1:3001` still triggers when
  `invoke` outright throws (i.e. the frontend is being served by
  `vite dev` in a regular browser, not the Tauri WebView).
- Cold-launch overhead on the happy path: 50 ms — the first poll
  succeeds in the vast majority of cases, the loop only spins when
  the backend is genuinely slow.

### Installer artefacts

- `dist/MikeRust_0.2.3_x64.msi` — Windows x86_64
- `dist/MikeRust_0.2.3_arm64.msi` — Windows ARM64

---

## v0.2.2 — 2026-05-23 (patch)

Two patch-level fixes targeted at the installed MSI experience that
v0.2.1 unintentionally regressed.

### Fixed — silent backend on installed MSI

Symptom: launching the installed `mike-tauri.exe` showed the Svelte
"Impossibile raggiungere il backend / Network error: Failed to fetch"
banner with no further diagnostics. Root cause: the release build
sets `windows_subsystem = "windows"`, which detaches stdout/stderr,
so every `tracing::error!` from the embedded axum startup chain
(`AppState::new`, `run_migrations`, `ort::init`, the actual bind…)
went to a `NUL` device. The user could see *that* the backend was
down but had no way to know *why*.

- `tracing-appender = "0.2"` added to `mike-tauri`'s deps. The
  shell now installs an additional non-blocking file layer that
  writes every event to `<home>/mikerust-data/mike-tauri.log` —
  same directory as the SQLite DB so we know it's user-writable.
  The worker guard is `Box::leak`'d at startup so the writer
  keeps flushing for the lifetime of the process (a few KB of
  heap, freed at exit). Stdout/stderr layer stays on top for
  `tauri dev` / `cargo run` where stdout *is* attached.
- `[tauri] tracing → C:\Users\<name>\mikerust-data\mike-tauri.log`
  now prints at startup, so a user reporting a problem can paste
  the log path back without having to know the convention.
- `axum server error: …` rewritten to walk the full
  `std::error::Error::source()` chain and emit
  `axum server failed: <top> -> <middle> -> <root>` — previously
  only the topmost wrapper was visible, hiding the concrete cause
  (file-not-found, addr-in-use, sqlite-open-failed…).
- `eprintln!("[mikerust:fatal] …")` as a last-resort sink for
  catastrophic startup failures (tokio runtime build, axum
  bind/serve) — visible to anyone running the installed exe from
  a console even if the file logger itself failed to open.
- `ensure_data_dir()` helper creates `<home>/mikerust-data/` up
  front so the log file open never races the SQLite open.

### Changed — embedded axum picks an explicit random ephemeral port

Symptom: with several Tauri / Electron desktop apps that all bind
axum on localhost (e.g. the upstream `mike` and MikeRust running
side by side), there was a non-zero chance that the OS's port-0
ephemeral pool would hand MikeRust a port another desktop app had
freed milliseconds ago and was about to rebind.

- `rand = "0.9"` added to `mike-tauri`'s deps.
- `pick_free_random_port()` now picks a `u16` uniformly in
  `49152..=65535` (the IANA "dynamic / private ports" range),
  pre-binds it synchronously via `std::net::TcpListener::bind` to
  confirm it's free *right now*, and retries up to 20 times on
  collision. The final fallback is still `port = 0` so a
  vanishingly-unlikely 20-in-a-row miss doesn't kill startup.
- `[tauri] embedded axum will bind on 127.0.0.1:<port>` is logged
  before the actual bind, so the chosen port is visible in the
  same `mike-tauri.log` introduced above.
- The `PORT` env var override stays — tests, dev pinning, and the
  standalone-backend dev story (frontend on `localhost:3000`
  pointing at a fixed `NEXT_PUBLIC_API_BASE_URL`) keep working.

### Installer artefacts

- `dist/MikeRust_0.2.2_x64.msi` — Windows x86_64
- `dist/MikeRust_0.2.2_arm64.msi` — Windows ARM64

Same lean ~24 MB shape as v0.2.1 — the gliner2_inference v0.5.1
slim-ort fix carries over.

---

## v0.2.1 — 2026-05-23

First semver bump since `v0.1.0`. Twenty-nine commits between the two
tags, headlined by the entire **GLiNER2 PII redaction vertical** and a
**chat-stream live-progress refactor** that closes the long-PDF "silent
wait" UX gap. Also: lean MSI installers for x64 + arm64 (down from
161 MB to ~24 MB after fixing the `gliner2_inference v0.5.1` ort
default-feature cascade), and the Italian terminology of the PII rows
was corrected from *Redazione* to *Anonimizzazione*. The themed
entries below collect the per-topic detail.

### Installer artefacts

- `dist/MikeRust_0.2.1_x64.msi` — Windows x86_64
- `dist/MikeRust_0.2.1_arm64.msi` — Windows ARM64

Both ship the matching `onnxruntime.dll` 1.20.0 + `pdfium.dll` under
`resources/libs/{onnxruntime,pdfium}/win-<arch>/` (loaded dynamically
at runtime — see `libs/onnxruntime/README.md` for the version pin
rationale).

---

## 2026-05-23 — PII pipeline live progress — SSE streams during setup, not after

Closes the "silent wait" UX gap reported on long PDFs: after pressing
send the composer cleared and the chat sat frozen for the entire PDF
extraction + PII redaction window (multiple minutes on a 9-page
DOCX), with no spinner, step row or banner — only after the heavy
work finished did the assistant turn finally appear with the events
all collapsed at once. The fix is architectural plus a visible
text-extraction stage and an Italian terminology correction.

### Fixed — SSE response now starts streaming immediately

- `stream_chat` in `src/routes/chat.rs` previously awaited
  `load_attached_docs` (PDF text extraction → PII redaction) and
  `maybe_compress_history` (history summarizer) **before** returning
  `Sse::new(rx)`. Axum can't begin streaming the HTTP body until the
  handler returns, so every `doc_extract_*` / `pii_redact_*` event
  emitted during that window just buffered in the mpsc channel and
  flushed all at once when the response finally went out. The
  client's `fetch()` sat with no response start for the duration.
- The whole pipeline — `load_attached_docs` + join siblings
  (`discover_mcp_for_user`, `retrieve_kb_chunks`,
  `list_indexed_corpus_docs`), prompt composition, tool setup,
  summarizer, the existing LLM streaming loop — now lives inside
  a single `tokio::spawn(async move { … })`. The handler creates
  the channel, clones `state` and `chat_id`, dispatches the spawn,
  and returns `Sse::new(rx).keep_alive(KeepAlive::default())` on the
  very next line. KeepAlive pings keep the connection live during
  the heavy setup; every emitted event lands in the browser in real
  time.

### Added — doc_extract step in the assistant turn

- New `ChatStep` variant `{ kind: 'doc_extract', filename, chars,
  done }`. Backend emits `doc_extract_start` at the top of every
  attached-document load and `doc_extract_done` (with `chars`)
  after either the cached-text fast-path or the fallback
  `extract_text_dispatch` returns — same shape as `pii_redact_*`.
- `frontend/src/lib/components/chat/ChatSteps.svelte` renders the
  step exactly like the PII row: spinner + "Estrazione testo — file"
  while in flight, green check + "Testo estratto — file (N caratteri)"
  on completion. The PII step (when enabled) appears immediately
  after.
- `frontend/src/lib/stores/chat.svelte.ts` adds
  `onDocExtractStart` / `onDocExtractDone` callbacks; the start
  handler is idempotent (won't double-push if the same filename
  fires twice) and the done handler synthesises a step if no start
  was seen (defence against out-of-order SSE).
- `Assistant.stepDocExtractStarting` / `…Done` i18n keys across
  all 6 locales (en + it canonical, fr/de/es/pt via
  `scripts/fill-i18n.mjs`).

### Changed — Italian terminology: "Redazione" → "Anonimizzazione"

- In Italian the act of removing personal data from a document is
  **anonimizzazione**, not *redazione* (which translates closer to
  "drafting / editing"). Five strings updated in
  `frontend/locales/it.json`:
  - `Assistant.stepPiiRedactStarting/Progress/Done` → "Anonimizzazione PII…", "PII anonimizzata —"
  - `ChatInput.pii.omissisHintPrefix` → "Per un'anonimizzazione di livello produttivo…"
  - `ChatInput.pii.statusUnavailable` → "…il documento sarà inviato senza anonimizzazione."
- English / French / German / Spanish / Portuguese left unchanged —
  *redact / redaction / Schwärzung / redacción / redação* are the
  correct technical terms in those languages.

### What the user sees now

Pressing send on a chat with `( PII [✓] file.pdf )` produces, in
order, with each step appearing the moment it starts (no more
multi-minute silence):

1. (in browser console immediately) `[streamChat] request …`
2. **⏳ Estrazione testo — file.pdf** — spinner appears within a
   second of the request leaving the browser.
3. **✓ Testo estratto — file.pdf (N caratteri)** — green check
   when the cached or freshly extracted text is ready.
4. **⏳ Anonimizzazione PII — file.pdf (0 / N)** — counter advances
   as each 2000-char chunk completes.
5. **✓ PII anonimizzata — file.pdf** — green check.
6. (then the LLM begins streaming text deltas)

### Tests

`cargo check --features ner-pii` clean.
`pnpm typecheck` 0 errors. i18n parity OK at 1095 keys × 6 locales.

---

## 2026-05-23 — GLiNER2 PII redaction — end-to-end chat-attachment pipeline

Per-file PII protection for chat attachments. When the user toggles
the new "PII" checkbox on a file chip in the composer, the
document's extracted text is run through a GLiNER2 zero-shot
multilingual NER engine before being stuffed into the LLM payload —
every detected person / email / address / phone / IBAN / fiscal-
code / date span is replaced with `[LABEL]` so the cloud model
never sees the original entities.

### Pipeline

```
upload (docx/pdf/xlsx/rtf/txt)
  → extract_text_dispatch        (sync/scanner.rs — pure-Rust, no LLM)
  → cache <hash>.txt
  → chat send (checkbox PII on)
    → load_attached_docs
      → maybe_redact_pii         (routes/chat.rs)
        → crate::ner::mask_pii   (ner/engine.rs)
          → chunk_for_window(2000 chars, 200 overlap)
          → for each chunk: Gliner2Engine::extract(text, tasks, params)
          → dedupe + sort entities longest-first
          → source.replace(entity.text, "[LABEL]")
  → DocPayload with masked text   → LLM
```

The masking is text-based and **global**: if the model flags
"Mario" in one chunk, all occurrences of "Mario" in the document
are replaced — safer-by-default than relying on offset alignment
that gliner2-rs's `ExtractedEntity` doesn't currently expose at
char level.

### Added

- `src/ner/{mod,engine,labels,bootstrap}.rs` — feature-gated module
  behind the new `audio-transcription`-sibling `ner-pii` cargo
  feature. `mike-tauri` bakes it in so dev builds carry the engine
  out of the box.
- `gliner2_inference = { git = "SemplificaAI/gliner2-rs", tag = "v0.5.0" }` —
  the Rust binding around GLiNER2 ONNX inference, pinned to the
  same `ort = "=2.0.0-rc.9"` MikeRust already uses for embeddings.
  Cargo dedupes the runtime so a single onnxruntime DLL (1.20.0)
  powers both passes.
- `crate::ner::mask_pii(text, labels, progress)` — the public
  entry. `labels = None` → 17-label default set
  (`person`, `first name`, `last name`, `full name`,
  `patient name`, `email`, `phone`, `address`, `location`,
  `organization`, `date`, `date of birth`, `id_number`, `iban`,
  `credit_card`, `ip_address`, `license_plate`); a caller passes a
  subset to narrow the redaction. Threshold pinned at 0.2 for
  high recall.
- Chunking helper that splits at UTF-8 char boundaries (~2000
  chars with 200-char overlap), never mid-multibyte — emoji and
  accented characters survive intact.
- `Gliner2Engine` singleton per process via
  `OnceLock<Mutex<Option<Arc<…>>>>` — model loads once, every
  later call is a session-state copy + inference pass.
- Chat handler:
  `routes/chat.rs::maybe_redact_pii(text, protected, filename, sse_tx)`
  receives a per-document protected flag collected from
  `OutgoingMessage.files[].pii_protected` and bridges the
  blocking inference to the SSE stream for progress events.
- Cargo dep + `ner-pii` feature flag + `mike-tauri` pulls it via
  `features = ["pdf", "ner-pii"]`.

### Notes

- Adapted the engine wrapper to gliner2-rs v0.5's actual API after
  finding the README example used a different shape than the
  shipped code: `ExtractedEntity { text, label, score, start_tok,
  end_tok }` exposes token offsets (not char), and `extract`
  takes a third `Option<InferenceParams>` argument. Our `Entity`
  type drops start/end and relies on the literal text for
  global text replace.
- `ort::init().with_name("MikeRust").commit()` is called once at
  startup before any embedding or NER work — gliner2-rs requires
  it. The call is a no-op when both `rag` and `ner-pii` are off.
- The `HF_HOME` env var is redirected to
  `%USERPROFILE%/mikerust-data/gliner2/` so the model weights live
  next to the embedding cache and the Tauri watcher never sees
  them.

---

## 2026-05-23 — PII model bootstrap — manual HF download with byte progress

`gliner2-rs::Gliner2Engine::from_pretrained` uses `hf-hub`
internally and exposes no byte-level download progress. The chat
banner would sit at "Loading PII model…" for minutes on first run
with nothing to show the user that 586 MB was actually moving.

Replaced with our own HF resolver that streams each shard from
`https://huggingface.co/<repo>/resolve/main/<variant>/<file>` and
publishes `downloaded/total/file` ticks to a `Downloading` state
the UI renders identically to the fastembed download bar.

### Added

- `src/ner/bootstrap.rs` (mirror of `src/audio/bootstrap.rs`):
  - Singleton `NerBootstrap` with `Arc<RwLock<NerStatus>>` for
    non-blocking status reads + `Mutex<()>` to serialise racing
    first-callers (the second waiter sees the freshly-cached
    files on re-check).
  - `ensure_files(repo_id, variant)` HEAD-passes for the total
    byte budget, then streams 8 ONNX shards + `tokenizer.json`
    into `<HF_HOME>/mikerust-gliner2/<repo>--<repo>/<variant>/`
    with 1 MB tick granularity and `.part` → atomic rename on
    success.
  - Variant auto-detection: respects `GLINER2_NO_IOBINDING=1`
    env var (forces the non-iobinding ONNX variant the runtime
    1.20.0 is happier with).
- `NerStatus::Downloading { downloaded, total, file }` variant +
  `/sync/ner-status` route extension serializing the bytes.
- `EmbeddingBanner.svelte` extended to also poll
  `/sync/ner-status` every 600 ms and render
  *"Download modello PII — encoder_fp16.onnx (123.4/502.1 MB)"*
  during the first run.

### Why bypass `from_pretrained`?

The crate's `from_pretrained` is sync, doesn't expose progress
callbacks, and downloads into hf-hub's snapshot/blob/refs cache
layout (not a flat directory). Replicating that layout from
outside is fragile, and the user would have no visibility into
the multi-minute first-time download. Our flat layout
(`<dir>/<file>`) works directly with `Gliner2Engine::new(Gliner2Config { models_dir, … })`,
which autodetects V1 vs V2 from the file presence — same engine,
clean download UX.

---

## 2026-05-23 — PII redaction UX — chip checkbox, disclaimer modal, per-chunk progress

Front-to-back UX for the user-controlled PII redaction. Every
file chip in the chat composer gains a per-file PII toggle; a
one-shot disclaimer modal warns about the blackbox nature of the
detector the first time the user enables it in a chat; and a
per-chunk progress step reports `(n / N)` inside the assistant's
own response while the redaction is running.

### Added — composer chip

- `Badge` for each attached file now reads
  `( PII [☐] filename ✕ )` — checkbox inserted before the
  filename; clicking it toggles `FileRef.piiProtected`.
- `OutgoingMessage.files[].pii_protected` (snake_case on the
  wire) carries the per-file flag to the backend.

### Added — disclaimer modal

- First PII toggle in each chat opens a modal:
  > "La protezione PII usa un modello AI blackbox che analizza il
  > testo estratto dal file e sostituisce i dati personali (nomi,
  > email, codici fiscali, IBAN, …) con segnaposto prima che il
  > documento sia inviato al modello. Il rilevamento può essere
  > impreciso: alcune entità potrebbero sfuggire al filtro e
  > raggiungere comunque l'LLM."
  >
  > "Per una redazione di livello produttivo e auditata
  > consigliamo Omissis, disponibile su edito-pdf.com."
- Acknowledgement is per-chat (resets on every chat switch via
  a `$effect` on `chatStore.activeId` — no localStorage). Cycling
  through chats keeps the user honest about the blackbox caveat
  on each new conversation.
- Keyboard: **Enter** confirms (= applies the toggle + marks
  acked); **Esc** cancels and visually un-checks the checkbox
  (the browser's pre-intercept click would otherwise leave a
  ghost-checked state while `piiProtected` stays false). Modal
  listener wired only while open, so Enter in the textarea still
  sends the message as before.
- Link to <https://edito-pdf.com> opens in the system browser
  via the existing `openExternal` Tauri command — not in the
  embedded WebView.

### Added — per-chunk progress

- SSE events `pii_redact_start { filename, total }`,
  `pii_redact_progress { filename, current, total }`,
  `pii_redact_done { filename }` flow through the same chat
  stream the rest of the turn uses. To make this work the
  `(tx, rx)` channel had to be hoisted *above* the
  `tokio::join!(load_attached_docs, …)` block: PII redaction
  happens during that join, before the spawn task that normally
  owns `tx`.
- A bridge channel (`mpsc::channel<(usize, usize)>(16)`) carries
  ticks from the `spawn_blocking` worker through to a forwarder
  task that translates them into SSE events on the chat stream.
  `try_send` on the worker side so a slow consumer never blocks
  the inference.
- `ChatStep` gains a `pii_redact` variant `{ filename, current,
  total, done }`. Rendered by `ChatSteps.svelte` as a tool step
  with spinner + "Redazione PII — file.pdf (3 / 17)" label;
  flips to ✓ on `done`.
- Status banner above the composer (the same component that
  shows the embedding-model download) renders the PII bootstrap:
  *"Download modello PII — encoder_fp16.onnx (123.4 / 502.1 MB)"*
  while bytes are flowing, then *"Caricamento modello PII —
  inizializzazione sessioni ONNX"* during the ort session
  build, then disappears.

### i18n

- 13 new keys across all six locales via `fill-i18n.mjs`:
  - `ChatInput.pii.tooltip/disclaimerTitle/disclaimerBody/omissisHintPrefix/omissisHintSuffix/acknowledge/statusLoading/statusFailed/statusUnavailable`
  - `Assistant.stepPiiRedactStarting/stepPiiRedactProgress/stepPiiRedactDone`
  - `NerStatus.downloading/loadingModel/failed`

---

## 2026-05-23 — PII tuning — labels, threshold, diagnostic logging

A first round of empirical tuning after the pipeline started
returning results on Italian medical text. Threshold dropped,
labels broadened, every layer of the pipeline gained explicit
tracing so future "PII didn't fire" reports can be debugged from
log lines alone.

### Changed

- **Labels** moved from compound forms (`person_name`,
  `fiscal_code`, `vat_number`) to natural-language GLiNER
  conventions (`person`, `first name`, `last name`, `full
  name`, `patient name`, `email`, `phone`, `id_number`).
  The compound forms produced ~0 hits per chunk; the natural
  forms surface several entities per paragraph. 17 labels in
  the default set, with multiple framings of "person" so the
  zero-shot model has more ways to find a given name.
- **Threshold** lowered from `gliner2-rs` default 0.5 → 0.3 →
  0.2. The safer-by-default redaction prefers over-masking
  ("Madre" / "Padre" tagged as person false-positives) to
  leaking ("ARWEN" missed because score < 0.3 → person name
  sent unredacted to LLM).
- `InferenceParams { threshold: 0.2, flat_ner: false }` passed
  explicitly to every `engine.extract` call — previously the
  None argument meant gliner2-rs picked its own default.

### Added — observability across the pipeline

- `[startup] features compiled in: rag=… pdf=… ner-pii=… audio-transcription=…`
  one-shot at boot so "ner-pii didn't fire" can be triaged from
  the dev log without reading cargo args.
- `[chat] payload parsed — attachments=N pii_protected=M (ner-pii built-in: …)`
  per request — confirms the per-file flag reaches the backend.
- `[chat] maybe_redact_pii(filename) — protected=… ner-pii-built-in=…`
  per attachment — confirms the redaction branch was entered.
- `[ner] ensure_files entry — repo=… variant=… files_needed=…
  HF_HOME=… GLINER2_NO_IOBINDING=…` per bootstrap call +
  per-file `pre-check: <path> → EXISTS/missing` lines so a
  cache-miss / cache-hit decision is fully transparent.
- `[ner] PII pass started/done` framing every full redaction
  pass with timing.
- `[ner] chunk N/M → extracting (… chars)` and
  `[ner] chunk N/M ✓ K entities in …ms` per chunk.
- `[ner]     · entity label=… score=… text=…` for every entity
  the model emits — so a "ARWEN missed but UNDÓMIEL caught"
  triage is a one-grep job.
- Frontend `[ChatInput] PII checkbox click`, `[ChatInput] PII
  toggle <file> → <bool>`, `[ChatInput] send() called …`,
  `[streamChat] request { piiProtectedFiles: N }` so the
  toggle → payload → SSE chain is visible from the JS console
  without backend access.

### Considered and rejected — regex pre-pass

Briefly shipped a deterministic regex pre-pass for the obvious
high-precision patterns (email / fiscal code / IBAN / phone /
IPv4 / date) and reverted it the same day at user direction:
the pipeline is to remain **pure GLiNER zero-shot**, no
hand-coded patterns. The trade-off accepted: gliner recall
gaps on unusual names (e.g. fictional first names, foreign
spellings, atypical fiscal layouts) may slip through to the
LLM. The threshold drop to 0.2 + the expanded "person" label
family widen the recall net within the model's own
zero-shot framing.

### Tests

`cargo test --features ner-pii --lib transcript_tests chunk_tests` —
all green (chunking + audio transcript helpers unaffected by
ner work).

`cargo check --features ner-pii` clean; `pnpm typecheck` 0/0/0.

### Known caveats (documented inline)

- Performance: 22-35 s per 2000-char chunk on Snapdragon X Elite
  ARM64 CPU with the FP16 model. An 8-chunk file is ~3-5 min.
  The QNN execution provider isn't compatible with the FP16
  model (needs QDQ INT8); CPU is the baseline. Performance lap
  back to QNN tracked separately.
- Substring overmatch: `source.replace` is non-overlapping
  left-to-right. Sorting longest-first prevents the obvious
  pathological case ("Mario Rossi" before "Mario") but doesn't
  rule out short tokens hitting unrelated substrings. Phase 2
  may add tokenizer-aligned char offsets if substring leaks
  become a real problem.

---

## 2026-05-22 — New `gdpr` canonical domain (11 verticals)

Added a dedicated `gdpr` professional vertical, distinct from
`compliance`. Domain identifiers are English snake_case and travel
over the wire as-is; UI labels are localised per market acronym
(`GDPR` it/en, `RGPD` fr/es/pt, `DSGVO` de).

### Added

- `gdpr` in [src/domain.rs](src/domain.rs) `DOMAINS` slice and in
  [frontend/src/lib/types/domain.ts](frontend/src/lib/types/domain.ts)
  — placed between `compliance` and `pa` (related but distinct: a
  workflow on a privacy-impact assessment is GDPR, a workflow on
  NIS2 incident-notification policy is compliance).
- `Domains.values.gdpr` key across all six locales.

### Behaviour with existing data

- Users with `enabled_domains = NULL` (no explicit preference) see
  GDPR enabled automatically — the NULL sentinel means "every shipped
  domain", and the new one rides along.
- Users with an explicit subset (e.g. `["legal","compliance"]`) keep
  their list; GDPR is opt-in via Impostazioni → Domini.
- No migration required — schema accepts arbitrary string under
  `domain` columns, validation is at the API boundary against
  `DOMAINS`. This is the documented add-a-domain procedure in
  `src/domain.rs`.

The first batch of `gdpr` workflow presets is queued — guidance
material to follow.

---

## 2026-05-22 — Citation pipeline: corpus URL → local path remap (write + read)

Citation pills pointing to corpus documents (EUR-Lex specifically,
but applies to any corpus that fetches an upstream URL) opened with
404 in the document viewer. Cause: the citation's `path` field
carried the upstream URL (`https://eur-lex.europa.eu/legal-content/...`),
which `/sync/kb-doc` rejected because it does `std::fs::read(path)`
and can't take a URL. Older `doc_chunks` rows persisted the URL as
`source_path`; the fix in
[`src/routes/eurlex.rs`](src/routes/eurlex.rs)
(2026-05-20) used the local cache file going forward but didn't
rewrite existing rows.

### Fixed

- **Write-path (live streaming)** — in
  [`src/routes/chat.rs`](src/routes/chat.rs) the citation builder
  now pre-fetches a `document_id → absolute_local_path` map from
  `documents` (joined with `STORAGE_PATH`) at the start of each
  turn. When emitting a citation whose `kb.source_path` starts with
  `http://` or `https://`, the value is substituted with the local
  cache path. Log: `[chat] remapping URL source_path → local
  storage path for doc <uuid>`. If no mapping exists (corpus doc
  never persisted to `documents`), the original URL flows through
  and the warning `citation source_path is a URL but no local
  storage_path is registered` makes the diagnosis explicit.
- **Read-path (chat reload)** — new helper
  `remap_url_annotation_paths` mirrors the same logic in
  `get_messages`. Annotations persisted in the old shape (URL in
  `path`) are rewritten on load, so reopening an old chat shows
  working citation pills without a destructive migration.

### Not changed (deliberate)

- No schema migration. The on-disk JSON in `messages.annotations`
  keeps the URL; only the API response substitutes it. Next time
  citations are generated for the same doc, the write-path remap
  kicks in and persists the local path — the URL form fades out
  organically.

---

## 2026-05-22 — Citation viewer: hallucinated-quote safety net + highlight hardening

When the model emits a `<CITATIONS>` block but the `quote` field
isn't an actual passage of the cited document — most often by citing
one of its own report's section headings, or by paraphrasing — the
viewer used to silently sit at the middle of the document with no
highlight. Two layers of defence shipped today.

### Backend safety net — `src/routes/chat.rs`

Right after `strip_page_markers` on the model's quote, the citation
builder now validates the quote against the chunk text we actually
retrieved (`kb.text`):

- Computes the letters-only-and-ASCII-lower-cased projection of
  both sides via the new `letters_only` helper (mirror of the
  frontend's `onlyLetters` in `highlight.ts`).
- If the projection of the quote isn't a substring of the
  projection of the chunk (`needle.len() >= 4` guard against
  trivial matches), the model hallucinated — replace the quote
  with the first 200 chars of the chunk text (trimmed at a UTF-8
  char boundary). Log: `[chat] citation quote not found in chunk
  for tag …; substituting first N chars of chunk text`.

The viewer now lands on the actual cited passage instead of an
invented header. Old chats that already persisted hallucinated
quotes are not retroactively rewritten — they were generated
before the validator existed.

### Frontend highlight hardening — `frontend/src/lib/utils/highlight.ts` + `TextView.svelte`

- **`onlyLetters` is now NFD-aware**: `s.normalize('NFD').replace(/[̀-ͯ]/g, '')`
  before stripping non-alphanumerics. Italian quotes ("perché",
  "città") no longer fail to match a document whose accents use a
  different normalisation form than the model's emitted quote.
- **Fallback prefix lengths extended** to `[…, 24, 16, 12, 8]`.
  Short references like "Settimane 1-4" used to fall below the
  previous floor of 32; they now get at least one shot.
- **TextView normalises line endings on input**: CR / CRLF / lone
  CR collapse to LF, leading BOM stripped. Visually consistent
  across Windows-saved TXT (CRLF), legacy Mac (CR), and Unix (LF)
  sources.
- **No-match fallback**: when `highlightCitation` returns null the
  viewer scrolls the container to the top instead of sitting
  silently in the middle, and logs a console warning with the
  first 80 chars of the quote — the user sees the document opened
  cleanly and the developer has a discoverable hook for diagnosing
  hallucinations from the browser console.

### Tests

`pnpm exec vitest run src/lib/utils/highlight.test.ts` →
**13/13** green (no regressions from the NFD addition).

`cargo check` clean.

---

## 2026-05-22 — `run.ps1` — debug-mode launcher at the repo root

A convenience entry point at the repo root: `./run.ps1` defaults to
`RUST_LOG=mike=debug,info` and forwards to
[`scripts/dev.ps1`](scripts/dev.ps1) — no need to remember the
`-LogTrace` flag every time, no duplication of the actual launch
logic.

### Added

- [run.ps1](run.ps1) — three parameters: `-Quiet` skips the
  RUST_LOG export; `-RustLog <expr>` overrides the level (e.g.
  `mike=trace,info,hyper=warn`); default invocation runs in debug
  mode out of the box.
- Parse-checked via
  `[System.Management.Automation.Language.Parser]::ParseFile()`.
  The actual `tauri dev` invocation still lives in
  `scripts/dev.ps1` — `run.ps1` is purely a wrapper so dev-story
  changes have a single source of truth.

---

## 2026-05-21 — Settings reorganised: Domains toggle + hairline-grouped sub-nav

The Settings page accumulated seven sections over the spring (Profile,
Security, LLM models, MCP servers, Data sources, **Domains** added
today, Danger zone). A flat alphabetical-ish list stopped scaling:
unrelated functions sat next to each other and the eye had no
landmarks. Replaced by a four-group layout with hairline separators —
no group labels, no extra chrome, just enough visual cadence to make
"same family / different family" obvious at a glance.

### Added

- **`Settings.domains` UI** — toggle each of the ten professional
  verticals (`legal`, `medical`, `finance`, `real_estate`, `hr`,
  `insurance`, `ip`, `compliance`, `pa`, `others`) on or off. Lives at
  [frontend/src/lib/components/settings/DomainsSection.svelte](frontend/src/lib/components/settings/DomainsSection.svelte).
  Saves per-toggle (no Save button); the last enabled toggle is
  locked with a warning toast — at least one domain must remain
  active.
- **Backend persistence** — new `enabled_domains TEXT` column on
  `user_settings` (migration
  [`0027_user_enabled_domains.sql`](migrations/0027_user_enabled_domains.sql))
  storing a JSON array of canonical IDs. NULL = no explicit
  preference = every domain enabled (backwards compatible). New
  routes `GET / PUT /user/enabled-domains` in
  [src/routes/user.rs](src/routes/user.rs); the PUT collapses an
  explicit "all ten enabled" payload back to NULL so a user reset
  doesn't leave a permanent snapshot of the shipped domain set.
- **Frontend store + API**: `userStore.enabledDomains`,
  `userStore.effectiveEnabledDomains` (always a concrete list),
  `userStore.isDomainEnabled(d)` helper. Hydrated post-unlock alongside
  locale and default-domain.
- **5 new i18n keys** across all six locales via
  [`fill-i18n.mjs`](frontend/scripts/fill-i18n.mjs):
  `Settings.domains`, `Settings.domainsHint`,
  `Settings.enabledDomainsSaved`, `Settings.enabledDomainsError`,
  `Settings.atLeastOneDomain`.

### Changed

- **Settings sub-nav groups + hairline separators** in
  [frontend/src/routes/Settings.svelte](frontend/src/routes/Settings.svelte).
  Sections gain a `group: 'account' | 'content' | 'ai' | 'danger'`
  field; a 1-px divider styled with `bg-(--color-surface-200)`,
  `my-1.5 mx-3` is drawn whenever two consecutive entries' groups
  differ. The visual order is now:
  ```
  Profilo · Sicurezza
  ──────────
  Domini · Fonti dati
  ──────────
  Modelli LLM · Server MCP
  ──────────
  Zona pericolosa
  ```
  Rationale: 1) who you are, 2) what the app shows, 3) how the AI
  reasons, 4) destructive actions deliberately isolated. No group
  labels — low visual noise.

### Not changed (deliberate)

- **Downstream filtering is a follow-up.** The toggle persists the
  preference and the store exposes `isDomainEnabled`, but the
  default-domain dropdown in Profilo, the domain filters in
  Workflows / Projects / Templates / Tabular, and the create-modal
  pickers still show the full ten-domain list. Wiring those is a
  separate pass — best done after the user has a chance to discover
  the new toggle.

`pnpm typecheck` → 0 errors / 0 warnings. `cargo check` clean.

---

## 2026-05-21 — Chat reload: generated-doc cards survive, Italian paren citations get pills

Two independent regressions in the chat-history replay path landed in
the same pass because they share the same loader code.

### Fixed — doc cards no longer disappear on chat reload

A chat with one PDF upload, one generated DOCX and one generated XLSX
came back from `get_messages` with all three references plain-text:
the `[c1]` pills resolved, but the green/blue download cards that the
live stream rendered for `doc_created` events were gone. Cause: the
backend persisted `messages.events` in 0020 but the SELECT in
`get_messages` never read the column, so the frontend had nothing to
replay.

- **[src/routes/chat.rs](src/routes/chat.rs)** — `get_messages` SELECT
  now pulls `events` alongside `annotations`; the response shape
  includes a parsed JSON array. Empty / NULL → `[]`.
- **[frontend/src/lib/api/chat.ts](frontend/src/lib/api/chat.ts)** —
  `messages.events?: unknown[]` typed.
- **[frontend/src/lib/stores/chat.svelte.ts](frontend/src/lib/stores/chat.svelte.ts)**
  — `selectChat` filters `doc_created` events and maps them into the
  assistant message's `steps[]` as `{ kind: 'doc', filename,
  documentId, downloadUrl }` so the existing card renderer picks them
  up unchanged.

### Fixed — Italian parenthesised citations now become pills

Generated reports cite documents as `(doc-3, pag. 12-15)` in
Italian-language prose, not `[doc-id: …]`. The existing rewriter only
recognised the bracketed English form, so the Italian pattern stayed
plain text. Extended the manual scanner in
[`src/routes/chat.rs`](src/routes/chat.rs)
(`extract_inline_docid_refs` + sibling Italian variant) to also
recognise `(doc-N, pag. N[-M])` / `(doc-N, p. N-M)` / variations with
multiple whitespace, rewriting them into the canonical `[cN]` form
ahead of the citation block.

### Tests

`cargo test -p mike --lib` — green; new tests cover the paren-form
extractor and the `events` round-trip through `get_messages`.

---

## 2026-05-21 — Activity-pulse choreography: 7-state wave over the full 3×3 grid

The Mike logo doubles as the "AI thinking" indicator. The animation
went through three iterations today, ending on a wave that includes
every cell of the grid.

### Changed

- **Final shape** — seven-state choreography over a 3.2 s cycle in
  [frontend/src/app.css](frontend/src/app.css):
  ```
  state 0%   tutti                        all nine cells full size
  state 14%  shrink corner                cell 9 small
  state 28%  shrink quad                  cells 5, 6, 8, 9 small
  state 42%  shrink rest = full 3×3 small all nine cells small
  state 56%  restore rest                 1, 2, 3, 4, 7 large; quad+corner still small
  state 70%  restore quad                 5, 6, 8 large; corner still small
  state 84%  restore corner               back to tutti
  state 100% loop
  ```
- **New keyframe `mike-logo-pulse-rest`** drives cells 1, 2, 3, 4, 7
  — previously static. They share the same ease-in-out timing as the
  corner and quad waves so the three keyframes interlock without
  beating.
- **[Logo.svelte](frontend/src/lib/components/ui/Logo.svelte)** —
  doc comment rewritten to describe the three concentric waves
  (corner outermost, quad middle, rest innermost) rather than the
  earlier three-phase model.

### Iteration log

Commits in order (all 2026-05-21):

- `b5da3ea` — three-phase pulse (5689 → 9 → all → loop). User
  feedback: "looks like a flash, not a pulse".
- `d183341` — five-state shrink-and-restore with smooth ease-in-out
  over 2.4 s, no flash.
- (uncommitted today) — extended to seven states / 3.2 s, full 3×3
  beat in the middle.

`prefers-reduced-motion` continues to disable the animation entirely.

---

## 2026-05-21 — Spreadsheet viewer: break the read+write effect loop on XLSX open

Opening a generated Excel from the chat sometimes crashed the viewer
with `effect_update_depth_exceeded`. Cause: the loader effect called
`renderSheet()` directly inside `load()`, which read `sheetNames` /
`activeSheet` / `quote` — making them tracked dependencies of the
outer effect — and then `load()` wrote back to those same fields in
the same run. Svelte 5 detected the read-then-write loop and bailed.

### Fixed

- **[SheetView.svelte](frontend/src/lib/components/documents/SheetView.svelte)**
  — `load()` no longer calls `renderSheet` directly; it only parses
  the workbook and stamps `sheetNames` / `activeSheet`. A dedicated
  render-only `$effect` reads the workbook on the `loading: true →
  false` transition and also re-fires on `activeSheet` / `revision` /
  `quote` changes — read-only, never writes the state it depends on,
  so the feedback path is broken.

Commit: `ea5ebe5`.

---

## 2026-05-21 — Corpus UX: plain-text preview modal + eye button on indexed docs

Source documents in the global / project corpus were only browsable
through search hits — there was no way to inspect the full text of a
document without running a query that happened to match it. Added a
preview path end-to-end.

### Added

- **`GET /corpora/:id/preview` route** — strategy-dispatched plain-text
  preview. EUR-Lex returns the cached HTML stripped to text;
  `dila-bulk-xml` returns the `<content>` payload; `http-fetch-per-id`
  re-extracts on demand. Cap at ~250 KB so the modal doesn't choke
  on a 6 000-page omnibus.
- **`<CorpusPreviewModal>` component** — scrollable monospace body,
  Ctrl+F native browser search (no custom search widget — Chrome's
  text-find honours the live DOM). Eye button next to every result in
  the search hits *and* on every indexed document row.
- **Four new i18n keys** across all six locales.

### Changed

- **Badge contrast on grey surfaces** — tones bumped from `-50/-700`
  to `-100/-800` ([Badge.svelte](frontend/src/lib/components/ui/Badge.svelte)).
  The greens and blues used for the EUR-Lex / DILA badges were
  illegible against the surface-50 background of the corpus list;
  the +1 weight on both bg and text restores AA contrast.
- **Tooltip on truncated corpus titles** — long EU law titles
  (`"Regulation (EU) 2024/1689 …"` + 200 chars) were ellipsis-truncated
  with no way to see the full string. `title=` attribute on the row
  shows the full text on hover.

Commits: `2f83925`, `1b373f3`, `10e9ef4`.

---

## 2026-05-21 — `generate_docx`: self-correct `body=` vs `body_md=` and reset empty-retry on tool use

The DOCX-generation tool kept getting called with `body_md="…"` instead
of the documented `body="…"` parameter — Gemini 3.5 Flash and Claude
both invented the alias from context, ignoring the JSON-schema
parameter name. Server-side returned an empty-body error; the model
then retried with the *same* wrong key, sometimes looping until the
chat ran out of patience.

### Fixed

- **[src/llm/builtin_tools.rs](src/llm/builtin_tools.rs) —
  `exec_generate_docx`** now accepts `body` *or* `body_md` (and a few
  other obvious variants) as input keys. On the unhappy path — both
  absent — the error message names the *exact* expected key so the
  model corrects on the next turn rather than guessing again.
- **Empty-retry counter resets on a successful tool invocation.**
  Previously, three earlier rejected calls left the counter near the
  abort threshold; a successful call later in the same turn could
  push the *next* (unrelated) empty response over the limit and end
  the stream early. The counter now resets every time the dispatcher
  observes a non-empty tool result.

Commit: `9d91f4a`.

---

## 2026-05-21 — Compliance vertical brief: `docs/macchine.md`

Added the operational spec for the **compliance** product vertical —
validation, analysis, generation and comparison of regulatory
technical documentation for industrial machinery (autoofficina,
lifting accessories, connected IoT machinery).

### Added

- **[docs/macchine.md](docs/macchine.md)** (1544 lines, v2.2). Six EU
  regulations in scope: Dir. 2006/42/CE · Reg. 2023/1230 (in vigore
  20/01/2027) · CRA 2024/2847 · RED 2014/53 · NIS2 2022/2555 ·
  D.Lgs. 138/2024. Defines: 4 document levels (L1 scheda · L2 manuale
  · L3 fascicolo · L4 accessori di sollevamento), 30 workflows
  (A1-A14 analysis, B1-B8 binary checklists, C1-C8 DOCX generation),
  11 analysis rules with standardised GAP types, a 4-layer SBOM/CVE
  pipeline with 10 function-calling tools (NVD + EPSS + CISA KEV),
  the "Validazione Fascicolo Tecnico" XLSX workbook in 6 sheets, and
  12 ready prompts (A-L) with the `[RUOLO][CONTESTO][ISTRUZIONI]
  [VINCOLI][FLAG OPERATIVI][OUTPUT]` skeleton.
- **First preset of the vertical** — [`macchine-classify-doc.json`](config/workflow-presets/compliance/macchine-classify-doc.json)
  (Prompt A, L1/L2/L3/L4 classifier; assistant workflow returning
  pure JSON). It is the prerequisite for every subsequent analysis
  in the brief.

### Notes

- The brief is **distinct from the roadmap item "compliance-aware
  model evaluation"**. That item is about scoring LLMs on per-provider
  compliance metadata (serving region, DPA, EULA). The brief is a
  product vertical that *uses* MikeRust to validate customer
  documentation. Different work; don't conflate.
- Two issues flagged during the analysis on 2026-05-21:
  "Prompt B0" referenced in §15 but never defined (typo); the archive
  deadline formula in Sheet 1 (`Data_emissione + 3650`) is inconsistent
  with Regola 3 ("10 anni dalla *cessazione produzione*", Dir.
  2006/42/CE Art. 5(3)).
- The remaining 13 workflows + 3 DOCX templates from the priority-1
  list of §15 are deferred to the next pass.

---

## 2026-05-20 — Auto-fetch native DLLs + bundle them into the MSI

End-to-end: a clean checkout can now do `pnpm --dir frontend install`
followed by `./scripts/build-release.ps1` and end up with two MSIs in
`dist/` that **install with all native runtime DLLs already next to
the binary**. Operator zero-touch.

### Added

- **`scripts/fetch-native-libs.ps1`** — downloads the pinned versions
  of `onnxruntime` and `pdfium` for both Windows architectures and
  places them under `libs/<lib>/win-<arch>/`. Pinned to:
  - `onnxruntime 1.20.0` (matches `ort = "=2.0.0-rc.9"` per
    `libs/onnxruntime/README.md`; cross-version DLL deadlocks
    `TextEmbedding::try_new_from_user_defined` silently).
  - `pdfium chromium/7834` (recent bblanchon prebuilt; pdfium-render
    0.8+ binds dynamically through the stable `FPDF_*` C API).
  Idempotent — existing DLLs are kept unless `-Force` is passed.
  HEAD-checked every URL is alive (onnxruntime ≈ 62 MB / arch,
  pdfium ≈ 3.5 MB / arch).

### Changed

- **`scripts/build-release.ps1`** auto-invokes the fetch script when a
  target arch's DLLs are missing, then injects a per-arch
  `bundle.resources` overlay via a second `--config` argument. Each
  MSI bundles only the matching arch's DLLs (no double-arch payload).
- **`src/pdf/mod.rs::load_pdfium`** primary lookup is now per-arch
  (`libs/pdfium/win-x64/pdfium.dll` / `libs/pdfium/win-arm64/...`)
  mirroring the onnxruntime layout. Legacy flat `libs/pdfium/pdfium.dll`
  is kept as a fallback so pre-existing developer checkouts don't
  break before they run the fetch script.
- **`src/pdf/mod.rs` + `src/embeddings/service.rs`** lookups also
  walk `<exe_dir>/resources/` — Tauri MSI installs land
  `bundle.resources` files at `<install>/resources/<dest>`, so adding
  that directory to the candidate bases means the existing
  walker finds the bundled DLLs without any platform-specific
  branching.

### Install layout post-MSI

```
C:\Program Files\MikeRust\
├── mike-tauri.exe
└── resources\
    └── libs\
        ├── pdfium\win-<arch>\pdfium.dll
        └── onnxruntime\win-<arch>\onnxruntime.dll
```

Both loaders now find the DLLs at first boot of the installed
application — no manual operator step.

### Tests

`cargo test -p mike --lib` → **381/381** green (no test regressions
from the loader changes; the existing
`onnxruntime_subdir_and_filename_matches_compile_target`,
`find_onnxruntime_dylib_*` and `build_execution_providers_*` cover
the surface that moved).

PowerShell scripts parse-checked via
`[System.Management.Automation.Language.Parser]::ParseFile()`.

---

## 2026-05-20 — Build & dev launch scripts (`scripts/build-release.ps1`, `scripts/dev.ps1`)

Two PowerShell scripts that replace the ad-hoc command lines we have
been pasting into terminals all session.

### Added

- **`scripts/build-release.ps1`** — drives `tauri build --bundles msi`
  twice, once per Windows target triple (`x86_64-pc-windows-msvc` and
  `aarch64-pc-windows-msvc`), and collects the produced `.msi` files
  into the top-level `dist/` directory. Each MSI is renamed with a
  short `_x64` / `_arm64` suffix so the two architectures coexist
  without clobbering each other. Parameters: `-Target` (`x64` /
  `arm64` / `both`, default `both`), `-Clean` (wipe the per-triple
  bundle dir first), `-FrontendInstall` (run `pnpm install
  --frozen-lockfile` before the Rust build — useful in CI). Verifies
  that the rustc targets are actually installed before kicking off a
  long build.
- **`scripts/dev.ps1`** — wraps `tauri dev` with the right config
  pre-selected and the local `@tauri-apps/cli` resolved out of
  `frontend/node_modules`. Optional `-LogTrace` switch sets
  `RUST_LOG=mike=debug,info` for a richer backend trace.
- **`dist/`** is now a tracked directory (carries its own
  `.gitignore` that excludes everything but itself) so the build
  script can rely on its existence and `git status` stays clean
  after a build.

### Caveat documented in the script header

The runtime still needs `onnxruntime.dll` (matching the `ort` version
pin) and `pdfium.dll` next to the installed binary for the RAG and
PDF paths to work. Today these are **not** bundled via
`tauri.bundle.resources` in `src-tauri/tauri.svelte.conf.json`, so the
MSI installs only the application binary; post-install the operator
has to drop the matching DLLs into the install folder by hand. The
script's docstring points at `libs/onnxruntime/README.md` for the
right version pairing.

---

## 2026-05-20 — PDF viewer: serialize citation highlight against page rasterisation

Opening a citation on a **long** source PDF often landed the user on
the top of the document instead of on the cited page, with no
highlight. Cause: a race between the page-by-page render loop and the
highlight effect. `loading` only covers the initial PDF fetch; once
that resolves, the per-page rasterisation (canvas + text layer) keeps
running asynchronously for seconds. The highlight effect fired the
moment the user-supplied `quote` / `page` props arrived, scanned an
empty or partial `pagesEl`, found nothing, and silently no-op'd.

### Fixed

- **New `renderInProgress` reactive flag** in
  [PdfView.svelte](frontend/src/lib/components/documents/PdfView.svelte).
  `renderAll` flips it `true` at the start and `false` in a `finally`
  block — only the latest render (`token === renderToken`) clears it,
  so an aborted older pass can't race the new one. The flag is the
  promise-based "document fully painted" signal Svelte already had in
  hand (every `pageObj.render(...).promise` and `TextLayer.render()`
  return promises the loop awaits); we just expose its derivative as
  reactive state.
- **Highlight effect gates on both `loading` AND `renderInProgress`**.
  The effect re-fires automatically on the `true → false` transition,
  so an early citation click is honoured the moment the document is
  actually paintable — no `setTimeout`, no polling.
- **`renderAll` no longer calls `applyHighlight` directly** at its end.
  Removing that direct invocation lets the reactive effect be the
  single source of truth: same wake-up signal, no duplicate scrolls.
- **`applyHighlight` falls back to a page-scroll even without a quote
  match.** Citations that carry only `page` (KB chunks where verbatim
  text was lost in re-flow / page-break) used to silently do nothing;
  now they at least scroll the viewport to the right page.

`pnpm typecheck` → 0 errors / 0 warnings.

---

## 2026-05-20 — Gemini wire payload: explicit `safetySettings` + per-model `thinkingConfig`

Our `gemini.rs` was sending neither `safetySettings` nor a
`generationConfig.thinkingConfig`, falling back to Google's defaults
on both. Two consequences:

1. **Silent content-filter refusals.** Default safety is
   `BLOCK_MEDIUM_AND_ABOVE` across the four harm categories. Legal,
   insurance and PA workflows legitimately handle sentences citing
   violence, harassment investigations, anti-corruption files and
   discrimination cases — Gemini would empty-respond or refuse without
   any error surface in our logs.
2. **Default `thinkingLevel = HIGH` on Gemini 3.5.** The model burns
   output budget thinking before writing, which is implicated in the
   truncated NIS2 report we fixed earlier in the parser.

### Added

- **`safety_settings_off()`** in [src/llm/gemini.rs](src/llm/gemini.rs)
  builds the four-category payload with `threshold: "OFF"` — matches
  the canonical SDK example shape and the same toggles available in
  AI Studio.
- **`thinking_config_for_model(model)`** returns the right shape for
  the model family:
  - `gemini-3.5*` / `gemini-3-flash*` / `gemini-3-pro*` →
    `{"thinkingLevel": "MEDIUM"}` (3.5 API)
  - `gemini-2.5*` → `{"thinkingBudget": -1}` (2.5 API, dynamic
    thinking)
  - everything else (1.5, 2.0, future names) → `None`, omit the
    field — the older API rejects it with 400.
- Both are wired into `build_body`: every request now carries
  `safetySettings`, and Gemini 2.5+ requests also carry
  `generationConfig.thinkingConfig`.

### Not changed (deliberate)

- **`maxOutputTokens`** — left unset. Setting it to 65 535 is one
  token below the model cap, so it would be a no-op disguised as a
  change. Lower values risk truncation; higher are clamped.
- **`temperature`**, **`topP`** — already the Gemini defaults
  (1.0 and 0.95). Explicit values would be cargo-cult.
- **`seed`** — DELIBERATELY omitted in chat. A fixed seed would
  pin every response to the same output for the same prompt — useful
  for deterministic test fixtures, not for live chat.

### Tests

Five new tests in `llm::gemini`:
- `build_body_always_attaches_safety_settings_off`
- `build_body_thinking_config_shape_is_model_aware` (3.5 → enum
  `thinkingLevel`; 2.5 → integer `thinkingBudget: -1`; cross-checks
  that neither shape leaks into the other model family)
- `build_body_omits_thinking_config_on_legacy_families`
  (gemini-1.5 / 2.0)
- `build_body_still_carries_tools_and_system_instruction`
  (regression guard: new fields don't displace tools / system prompt)
- plus `empty_params` helper.

`cargo test -p mike --lib` → **381/381** green (was 377 +4).

---

## 2026-05-20 — `<CITATIONS>` block parser: tolerate truncated output

A NIS2 report generation produced **31** `[cN]` markers in prose plus
a fully-written `<CITATIONS>` JSON array of 31 entries — but the
response ran out of output tokens **before emitting the closing
`</CITATIONS>` tag**. `extract_citations_block` required that closing
tag and returned `None`, so all 31 citations were dropped silently
(`parsed 0 citations from response`) and every pill rendered as plain
text. The user saw `[c1] [c2] … [c31]` with no clickable behaviour.

### Fixed

- **`extract_citations_block`** now treats the closing tag as
  optional: if missing, it takes everything from the opening tag to
  end-of-text. This recovers the common "model ran out of tokens"
  case where the JSON itself is syntactically complete.
- **New `recover_truncated_citations_array`** — last-resort safety
  net for genuinely truncated JSON arrays (cut mid-entry). Walks the
  prefix as JSON respecting string scope (a `}` inside a quoted
  value doesn't confuse the depth tracker), remembers the offset of
  the most recent top-level `}` that closed an entry, and rebuilds
  `[ entry₁, …, entryₖ ]` as a valid prefix. Logs a warn so we know
  when this path fires.
- **`get_messages` retroactive recovery** — when the persisted
  `annotations` column is `NULL` (older turn or a turn the live
  pass silently dropped) but the message body still contains the
  `<CITATIONS>` block, the loader re-parses the content with the
  smarter extractor. Chats that broke silently in earlier builds
  now render pills again on reload.

### Tests

Four new tests in `routes::chat`:
- `unclosed_block_still_parses_when_inner_is_valid_json`
- `recovers_block_without_closing_tag` (the exact shape from the
  NIS2 report turn).
- `recovers_truncated_array_with_partial_last_entry`.
- `truncation_recovery_handles_quote_with_brace_inside` (security:
  a `}` inside a string must not be mistaken for an entry-close).
- `recovers_block_without_closing_tag_via_escape_repair` (the
  truncation tolerance composes with the existing `\'` repair).

`cargo test -p mike --lib` → **377/377** green (was 373 +4).

---

## 2026-05-20 — Rewrite free-form `[doc-id: …]` references into clickable `[cN]` pills

Verbose `generate_docx` follow-up descriptions occasionally bypassed the
`[cN]` + `<CITATIONS>` contract and emitted free-form bracketed
references such as `[doc-id: cdbe5ce0-…, page 1]` or `[doc-id: doc-0,
pages 2-3]` inline in the prose. These rendered as plain text — the
file and page were visible but not clickable. Fixed end-to-end.

### Added

- **`src/routes/chat.rs::extract_inline_docid_refs`** — manual scanner
  that finds every `[doc-id: <handle>[, page[s] <N|N-M>]]` occurrence
  in the assistant's prose. Tolerates variable whitespace, `page` vs
  `pages`, capital `Doc-ID:` / `DOC-ID:`, and the form without a page.
  Returns matches in order with byte offsets so the rewriter can do
  in-place substitution.
- **`src/routes/chat.rs::rewrite_inline_docid_citations`** — given the
  scanned references and a `resolve(handle) -> Option<(uuid, filename)>`
  closure (security: never reveals an arbitrary UUID through the
  viewer), assigns sequential `c1, c2, …` refs, replaces each
  occurrence in the prose, and produces a `<CITATIONS>`-compatible
  JSON array. Two references with the same `(uuid, page)` share one
  ref so the block stays compact.
- **`content_replace` SSE event** — the chat handler streams the
  rewritten body to the live view immediately after the rewrite, so
  pills render on the current turn without waiting for a reload.
  The frontend (`api/chat.ts` + `stores/chat.svelte.ts::onContentReplace`)
  swaps `message.content` wholesale on receipt.

### Changed

- **Chat handler** rewrites the body BEFORE persistence and BEFORE
  `extract_citations_block`, so the DB row also has `[cN]` markers and
  a synthesised citations array — reload of the chat shows the same
  clickable pills the live turn did.
- **`MRUST_SYSTEM_PROMPT`** picked up an explicit anti-pattern rule
  banning free-form `[doc-id: …]` style references in prose. The
  rewriter is a safety net; the prompt is the prevention.

### Tests

Seven new tests in `routes::chat` pin the scanner and the rewriter:
- `extract_inline_docid_refs_picks_up_the_observed_pattern` — the
  exact NIS2-turn shape (UUID + `page N` and `page N-M`).
- `extract_inline_docid_refs_handles_doc_n_and_pages_and_no_page`.
- `extract_inline_docid_refs_ignores_malformed_and_unrelated_brackets`.
- `rewrite_inline_docid_citations_collapses_repeats_into_one_ref`.
- `rewrite_inline_docid_citations_distinct_pages_get_distinct_refs`.
- `rewrite_inline_docid_citations_leaves_unresolved_handles_alone`
  (the security guarantee).
- `rewrite_inline_docid_citations_returns_none_when_nothing_resolves`.

`cargo test -p mike --lib` → **373/373** green (was 366 +7).
Frontend `pnpm typecheck` → 0 errors / 0 warnings.

---

## 2026-05-20 — Citation viewer: resolve corpus-inventory `doc_id` via canonical key

Clicking a citation pill could return `Impossibile caricare il documento
— Not Found` when the model emitted the inventory-line form of a corpus
document id (e.g. `[italian-legal] corte_costituzionale_1990_241`,
bracket + space included) instead of the `[gN]`/`[pN]` KB tag. The
backend resolver fell through every existing fallback, the frontend
hit `GET /document/<the-broken-string>/display` → 404.

### Fixed

- **`src/routes/chat.rs`** — new `canonical_corpus_key()` helper that
  strips every non-alphanumeric character and lowercases. A new
  per-request `library_corpus_index` (canonical-key → `(uuid,
  filename)`) is built from one cheap query against the user's full
  corpus library (`WHERE corpus_id IS NOT NULL AND corpus_identifier
  IS NOT NULL`), so it works even when the turn produced **zero** KB
  chunks (the existing `corpus_ref_to_tag` map then stays empty and
  doesn't help). The resolver's last-resort branch now canonicalises
  the model's `doc_id`, looks it up in that index, and stamps
  `document_id` + `filename` directly. The hallucinated `page` (if
  any) is dropped so the viewer falls back to text-searching the
  quote rather than scrolling to a fake page.

### Tests

`canonical_corpus_key_collapses_inventory_variants_onto_one_key`
pins five sloppy variants the model produces in practice — all
collapse to the same canonical string. Full suite:
`cargo test -p mike --lib` → **366/366** green (was 365 +1).

---

## 2026-05-20 — Pubblica Amministrazione domain + Fase-1 workflow pack

Introduced `pa` as a new canonical professional vertical and shipped
the seven workflow presets specified in `docs/pa-prompts.md` with full
prompt templates (the Fase-1 four-pack plus determina, RUP checklist
and PNRR milestone). All other workflows from blocks 1–5 that the spec
sketches with only a one-line description are deferred to a follow-up
pack — they don't have an authoring prompt yet.

### Added — domain registration

- **`src/domain.rs`** — `pa` appended to `DOMAINS` (between `compliance`
  and `others`). Schema-default validation (`is_valid`) accepts it
  automatically.
- **`frontend/src/lib/types/domain.ts`** — `pa` appended to the TS
  mirror, so the picker dropdown lists it.
- **`src/llm/builtin_tools.rs`** — `list_docx_templates` schema enum
  description extended with `'pa'` so the model can filter on it.
- **`frontend/locales/{it,en,fr,de,es,pt}.json`** — `Domains.values.pa`
  added across all six locales:
  - it: Pubblica Amministrazione · en: Public Administration
  - fr: Administration publique · de: Öffentliche Verwaltung
  - es: Administración pública · pt: Administração pública

### Added — workflows (`config/workflow-presets/pa/`)

Block 1 — Atti amministrativi:
- **`pa-delibera`** (assistant) — analisi delibera: competenza, motivazione (art. 3 L. 241/1990), iter procedimentale, copertura finanziaria (art. 183 TUEL), vizi rilevati, termini di impugnazione (TAR / PR / autotutela).
- **`pa-determina`** (assistant) — estrazione tabellare di 12 campi obbligatori della determina dirigenziale (numero, oggetto, norma attributiva, CIG, importo, capitolo, visti, pareri, pubblicazione, esecutività) con conformità sì/no/da verificare.

Block 2 — Appalti pubblici (D.Lgs. 36/2023):
- **`pa-appalto-review`** (assistant) — review clausola-per-clausola con focus obbligatorio su 10 aree (penali, SAL, subappalto, recesso, ADR, garanzie, varianti, riserve, revisione prezzi, clausole sociali) + sintesi rischi con semaforo 🔴🟡🟢.
- **`pa-rup-checklist`** (tabular) — 1 riga = 1 documento del fascicolo gara; 8 colonne mappano la fase (programmazione/progettazione/affidamento/esecuzione/chiusura), l'adempimento RUP, la norma D.Lgs. 36/2023 e l'esito.

Block 3 — Procedimento (L. 241/1990):
- **`pa-241-check`** (assistant) — verifica dei 7 controlli L. 241/90 (responsabile, comunicazione avvio, termine, preavviso rigetto, motivazione, partecipazione, accesso) con mini-scheda per punto e tabella riassuntiva esito+rischio+sanatoria.

Block 4 — PNRR e fondi UE:
- **`pa-pnrr-milestone`** (tabular) — 1 riga = 1 documento probatorio; 10 colonne riconducono il doc a Missione/Componente/Investimento, milestone/target ID, scadenza UE, documentazione richiesta vs disponibile, spese rendicontate, rischio (🔴🟡🟢), azione raccomandata.

Block 5 — Trasparenza e anticorruzione:
- **`pa-ptpct`** (assistant) — analisi del PTPCT in 6 sezioni (struttura · analisi del rischio · misure di prevenzione · obblighi di trasparenza D.Lgs. 33/2013 · RPCT · gap+raccomandazioni con scala Obbligatorio-Mancante/Presente-carente/Raccomandato-Mancante).

### Added — tests

- **`src/presets/workflow.rs::shipped_pa_workflows_present_and_typed`**
  anchors the seven PA preset ids, asserts each carries `domain = "pa"`
  and the spec's prescribed kind (assistant/tabular), each `practice`
  starts with `PA — `, and tabular presets have non-empty
  `columns_config` while assistants don't carry one.

### Tests

`cargo test -p mike --lib` → **365/365** green (was 364 +1).
Frontend `pnpm typecheck` → 0 errors / 0 warnings.

### Deferred

The fourteen workflows from `docs/pa-prompts.md` that are sketched
with only a one-line description (pa-ordinanza, pa-parere, pa-bando,
pa-collaudo, pa-variante, pa-silenzio, pa-autotutela, pa-accesso,
pa-rendiconto, pa-audit, pa-irregolarita, pa-foia, pa-conflitto,
pa-dati-aperti) require new authoring prompts and are tracked as a
follow-up batch.

---

## 2026-05-20 — NIS2 compliance pack: docx template + assistant workflow + tabular review

Translated `docs/nis2-prompts.md` into three ready-to-ship MikeRust
artefacts, all anchored to the **compliance** domain and to the Italian
NIS2 transposition (D.Lgs. 138/2024).

### Added

- **`config/docx-templates/compliance/nis2-audit-readiness-report.json`**
  — DOCX layout sidecar (`id: compliance/nis2-audit-readiness-report`,
  A4 portrait, L1, square-bracket placeholders). Section skeleton
  matches the spec one-to-one: title block, intestazione metadata,
  fixed 10-domain scoring table (`1. Governance` → `10. HR & accessi`)
  with TOTAL row + conformity semaphore, top-3 sanction risks under
  art. 38, three 4-week remediation phases, free-text final notes, and
  the signature block for valutatore + organo di gestione (art. 20).
  Required metadata: `ORGANIZZAZIONE`, `DATA_ASSESSMENT`, `VALUTATORE`,
  `CLASSIFICAZIONE_SOGGETTO` (`ESSENZIALE`/`IMPORTANTE`),
  `APPROVATORE_GESTIONE`, `DATA_APPROVAZIONE` (+ optional
  `FATTURATO_GLOBALE` to compute the 2% / 1,4% sanction cap in euro).
- **`config/workflow-presets/compliance/nis2-audit-readiness.json`**
  — `assistant`-type workflow (`builtin-compliance-nis2-audit-readiness`),
  practice "NIS2 — Cybersecurity & resilience". Walks the model
  through the full report (sections 1–4 of the spec), enforces the
  fixed 10-domain ordering / dicitura, requires evidence citations
  per score, and links to the docx via
  `default_output_template: "compliance/nis2-audit-readiness-report"`
  so `read_workflow` auto-bundles the template's authoring contract
  and the closing instruction calls `generate_docx` with the right
  template_id.
- **`config/workflow-presets/compliance/nis2-policy-inventory.json`**
  — `tabular`-type workflow (`builtin-compliance-nis2-policy-inventory`)
  for inventorying multiple policy/procedure documents against the
  ten NIS2 domains. Nine columns: title, type (policy/procedura/
  piano/registro/delibera/evidenza_tecnica), NIS2 domains covered,
  version+date, art. 20 approval, NIS2 controls present, gaps,
  maturity score 0–3, evaluator notes. Designed as the pre-step to
  the audit-readiness assistant workflow.
- **`src/presets/workflow.rs::tests`** — two new tests:
  - `shipped_workflow_presets_all_load_cleanly`: strongly-typed
    loader must accept every JSON file in `config/workflow-presets/`,
    every id must be unique, every `domain` canonical, every `kind`
    in `{assistant, tabular}`. Future-proofs against silent
    typo-induced startup drops.
  - `shipped_compliance_nis2_presets_present_and_wired`: anchors the
    three NIS2 ids and asserts the assistant workflow points at the
    matching docx template id, and that the tabular has the expected
    column shape.

### Tests

`cargo test -p mike --lib` → **364/364** green (was 360 +4: the two
new workflow-preset tests above, plus the two thought_signature tests
from the Gemini 3.5 entry).

The dev binary auto-loads presets at startup, so a tauri-dev restart
exposes "Audit readiness NIS2" in the Workflow picker and "Inventario
policy NIS2" in the Tabular reviews picker.

---

## 2026-05-20 — Gemini 3.5 Flash (GA) + thought_signature plumbing

Google released **Gemini 3.5 Flash** as GA at Google I/O '26 (model id
`gemini-3.5-flash`, 1M input / 64K output context, vision, tools,
agentic thinking, no region restrictions). Registered the new model
and promoted it to the default chat model. The release also surfaced
a hard requirement that broke our tool-use loop on day one and is now
fixed.

### Added

- **`config/model.json`** — `gemini-3.5-flash` registered at the top
  of the Google model list (`standard` tier, `1_048_576` context,
  `65_536` output, vision + tools + thinking, `preview: false`).
  Released 2026-05-19 GA, available globally.
- **`src/llm/types.rs`** — `ToolCall::thought_signature: Option<String>`.
  Gemini 2.5+ tags `functionCall` parts produced during a thinking
  pass with an opaque `thoughtSignature`; clients are required to echo
  it back on replay, or Gemini 3.5+ rejects the next turn with
  `400 INVALID_ARGUMENT: Function call is missing a thought_signature
  in functionCall parts`. The field is serialised only when set, so
  non-Gemini providers keep emitting a clean shape.
- **`src/llm/gemini.rs`** — parse `thoughtSignature` from incoming SSE
  `functionCall` parts; echo it back in `to_wire_contents` when
  replaying assistant tool-calls; two new unit tests pin both halves
  of the round-trip.

### Changed

- **`src/llm/summarize.rs`** — `context_window_tokens` recognises
  `gemini-3.5-flash` as a 1M-token model.
- **`src/llm/mod.rs`** — tests extended to cover `gemini-3.5-flash`
  on `provider_for_model` / `supports_mcp_tools` /
  `is_vision_capable` (the runtime predicates already match via
  prefix / `contains`).
- **`src/routes/chat.rs`** — the five default-model literals
  (`gemini-3-flash-preview`) replaced with `gemini-3.5-flash`. New
  chats, the generate-title fallback and the "first configured
  provider" fallback all land on the GA model.
- **`config/model.json`** — `gemini-3-flash-preview` marked
  `legacy: true` with a `_legacy_note` pointing users at the new
  3.5 entry; pinned-preview settings keep working but new picks see
  the GA model first.

### Fixed

- **Gemini 3.5 Flash tool-use** — the first `read_workflow` call in
  the running app failed with `400 INVALID_ARGUMENT: Function call
  is missing a thought_signature in functionCall parts. ... Additional
  data, function call default_api:read_workflow, position 6`. Plumbing
  the `thoughtSignature` through `ToolCall` and back into the wire
  payload on replay clears it; covered by
  `parse_sse_function_call_captures_thought_signature` and
  `to_wire_contents_echoes_thought_signature_on_replay`.

### Tests

`cargo test -p mike --lib llm::` → **66/66** green, including the
two new thought-signature round-trip tests.

---

## 2026-05-20 — ONNX Runtime downgrade preparatory to data-privacy work

Downgraded the embedding stack from `ort 2.0.0-rc.12` / `fastembed 5.13.4`
(onnxruntime 1.24.2) back to `ort 2.0.0-rc.9` / `fastembed 4.9.1`
(onnxruntime 1.20.0). The downgrade is **propedeutic to a new
data-security and privacy feature**: that work relies on the smaller,
audited 1.20.0 runtime surface and on the set of execution providers
that shipped with it.

### Changed

- **`Cargo.toml`** — `ort` pinned at `=2.0.0-rc.9` (was `=2.0.0-rc.12`)
  and `fastembed` pinned at `=4.9.1` (was `5`). The `std` feature flag
  on `ort` is dropped — it never existed on rc.9. The `image-models`
  feature on `fastembed` is dropped — it was a 5.x addition we never
  used.
- **EP type names** — rc.9 exposes execution providers only via
  `ort::execution_providers::<Name>ExecutionProvider`; the short
  `ort::ep::<Name>` aliases (`QNN`, `CUDA`, `DirectML`, …) were
  introduced in a later rc and do not exist here. Every EP push in
  `embeddings/service.rs::build_execution_providers` was renamed to
  the long form. Vitis specifically is `VitisAIExecutionProvider`.
- **`UserDefinedEmbeddingModel` construction** — fastembed 4.9.1 marks
  the struct `#[non_exhaustive]`; the explicit struct literal we used
  on 5.x no longer compiles. Now built through `::new(onnx, tokenizer)
  .with_pooling(Pooling::Mean).with_quantization(QuantizationMode::None)`.
  The `external_initializers` and `output_key` fields fastembed 5.x
  exposes simply don't exist on this version — we never relied on
  them, but they will need re-adding when we eventually bump back.

### Removed

- **`rag-webgpu`** and **`rag-azure`** cargo features and their EP
  pushes in `build_execution_providers` — neither EP exists in
  onnxruntime 1.20.0. Removed from the platform packs
  (`rag-accel-windows`/`linux`/`macos`) and from `rag-accel-all`.
  Will return when the runtime moves forward again.

### Docs

- **`libs/onnxruntime/README.md`** — version note flipped from
  "must be exactly 1.24.2 for rc.12" to "must be exactly 1.20.0 for
  rc.9", with the strings-probe pattern updated. New "Why we are
  deliberately on the rc.9 line" subsection points back here.

### Migration note for contributors

Re-vendor the matching DLL before relaunching the dev binary:
`libs/onnxruntime/<platform>/onnxruntime.dll` (or `.so`/`.dylib`) MUST
be a **v1.20.0** Microsoft build. Keeping the 1.24.2 DLL while running
rc.9 silently deadlocks `TextEmbedding::try_new_from_user_defined`
during embedding-model load — the failure mode that triggered the
original "lock to one version" rule. The README has a one-liner
PowerShell snippet that fetches and drops the right file.

### Tests

All **360 mike lib tests pass** with default features. The
`embeddings::service::` block (45 tests covering vector packing,
dylib discovery walking, execution-provider construction with every
EP enabled, …) is green both under default features and under
`--features rag-accel-all`. The end-to-end ONNX inference path is
only exercised when the dev binary loads the matched 1.20.0 DLL —
contributors should re-run `tauri dev` and trigger an embedding to
validate that side of the migration on their machine.

---

## 2026-05-19 — Declarative legal-corpus connectors + data-sources UX overhaul

Brought the JSON-manifest corpus system from scaffold to a working
declarative connector engine, reworked the Settings → Data sources
panel, and cleared a long tail of QA findings on the corpus workflow.

### Added

- **Corpus discovery metadata** — `jurisdiction`, `doc_types`, `auth`,
  `search_mode`, `fetch_format` added to the manifest schema and to all
  28 manifests; surfaced through `GET /corpora`.
- **Data-sources filters** — Jurisdiction and Type dropdowns over the
  source tabs, per-source badges (document type, auth, search mode,
  fetch format), and a search-mode hint under the search box.
- **Per-corpus enable/disable** — generic `GET|PUT /corpora/:id/config`
  (shares the existing `corpus_settings` table, no migration), with an
  enable toggle on every source panel — not just EUR-Lex.
- **Unified search box — year filter** — the engine extracts a 4-digit
  year from the query and routes to a date-filtered endpoint via the
  new optional `url_template_year` manifest field (wired for US/eCFR
  and US/CourtListener; available to any manifest).
- **Manifest hot-reload (dev)** — a background watcher re-reads
  `config/corpora-plugins/` and swaps the plugin + adapter registry
  in-process, so connector edits no longer need a restart. Debug builds
  only; a packaged app ships frozen manifests.
- **Per-hit indexing queue** — multiple "Index" clicks now queue up,
  each hit showing its own state (queued → running → done/error) with
  a retry, instead of only the last click showing a progress bar.
- `PLAN_FONTI_INTERNAZIONALI.md` — a matrix of the 28 SuzieLaw-derived
  international legal sources (download mode, auth, format, doc_id).

### Changed

- The `http-fetch-per-id` `ManifestAdapter` is now the live engine for
  the declarative corpora (search + fetch via URL templates and
  CSS/JSONPath extraction) — previously scaffold-only.
- CNIL, EUR-Lex and Italian-Legal are no longer hidden from the Data
  sources panel — an over-broad exclusion list left EUR-Lex's dedicated
  panel unreachable.
- Display-name cleanups: `CNIL` → `FR / CNIL`,
  `Italian Legal Corpus` → `IT / Italian-Legal`; the jurisdiction
  filter shows localized names ("Italiana", "Tedesca", …) in all six
  locales.
- `http-fetch-per-id` search: a keyword search that returns nothing now
  falls back to an identifier probe — one box accepts free text and
  corpus-native identifiers.
- Connector manifests corrected against the live APIs: **DE/OpenLegalData**
  (search + fetch through the JSON API by numeric id — the website
  pages are Cloudflare-gated, the API is not), **JP/e-Gov** (keyword
  search added via the v2 API), **US/eCFR** (section-level result
  headings), **US/CourtListener** (year filter).
- **IT/Normattiva** set to citation-only: the portal is a JS app with
  no search API — free-text Italian search lives in IT/Italian-Legal
  (local FTS index).
- Indexed-document rows now show the stored `.txt` size.

### Fixed

- JSON `null` no longer leaks into search results as the literal text
  `"null"` — a `null` field is treated as absent.
- "Import already in progress" is now an informational toast, not a red
  error; the import progress bar tracks the real `downloading` /
  `importing` job states (it previously watched a `running` string the
  backend never emits) and resumes polling when the panel is reopened
  mid-import.

---

## 2026-05-18 — KB citations open in viewer + formal verification trace

Implemented the frontend wiring needed to open/download knowledge-base
citations (`[gN]` / `[pN]`) from chat directly in the document viewer,
and recorded a formal verification step so the change can be
re-checked manually.

### Fixed

- KB citation payload `path` is now preserved in the frontend citation
  model (`kbPath`) and routed through the viewer tab state (`source:
  kb`) instead of being treated like `/document/{id}` only.
- Viewer fetch path now switches by source:
  - `document` -> `/document/{id}/display` and `/document/{id}/download`
  - `kb` -> `/sync/kb-doc?path=...` (for both open and download)
- DOCX viewer now supports tracked-change mode with per-tab policy
  `show | accept | reject`, surfaced in the header controls.
- `Accept` / `Reject` are implemented as renderer-level visual policies
  (non-destructive): no backend persistence/mutation is performed in
  this step.
- Chat token strategy switched to quality-first: frontend compaction is
  now emergency-only (very large payloads), while normal context
  reduction is delegated to backend model-aware summarization (80%
  context-window trigger). Emergency compaction preserves first user
  anchor + recent window + selected older structured user turns.
- Emergency compaction budgets are now model-aware on the frontend:
  they read `context_window` from the model catalogue when available,
  with dedicated fallback for local/OpenAI-compatible endpoints
  (including Ollama/vLLM patterns) to avoid over-trimming on large local
  context models.
- `config/model.json` now includes a dedicated `local` provider catalogue
  aligned to a 24GB-VRAM target: professional quantized shortlist only
  (Qwen 3.6 27B, Qwen 3.5 27B, Qwen 2.5 32B, Mistral Small 3.1 24B),
  with 70B entries removed.
- Added `scripts/recommend-context-window.ps1` to auto-profile local host
  resources (RAM/VRAM), read installed Ollama models via `ollama list`
  + `ollama show`, and output a per-model recommended `context_window`
  ready for `config/model.json` (`text`, `json`, or `markdown` formats).
- Added `scripts/ollama-context-profiles.json` with Snapdragon X Elite
  tuned profiles (`qwen3.5:4b` @ 16K, `qwen3.5:9b` @ 8K,
  `gemma4:e2b` @ 8K), plus two operational scripts:
  `scripts/apply-ollama-context-profiles.ps1` (dry-run by default,
  `-Apply` to create aliases) and
  `scripts/verify-ollama-context-profiles.ps1` (checks effective context
  via `ollama show`).
- `config/model.json` local provider now includes tuned alias IDs
  aligned to those profiles:
  `mikerust-qwen35-4b:ctx16k`, `mikerust-qwen35-9b:ctx8k`,
  `mikerust-gemma4-e2b:ctx8k`.
- Fixed `src-tauri/tauri.svelte.conf.json` dev/build commands pathing
  for repository-root launches: `pnpm --dir ../frontend ...` ->
  `pnpm --dir ./frontend ...`, avoiding accidental resolution to
  `C:\Progetti\frontend`.
- Fixed Settings → Model roles provider-filter logic so local models are
  visible in role dropdowns when a local endpoint is configured
  (`local_base_url` set). This unblocks selection of local tuned aliases
  like `mikerust-qwen35-4b:ctx16k` as `main_model`.
- Fixed broader Settings provider/model coherence to prevent cross-provider
  mismatches (e.g. local model accidentally sent to Gemini):
  - Active provider chips are now toggle-style multi-select (non-exclusive),
    with enabled state tied to actually configured providers.
  - Role-model dropdowns now show only models from configured + toggled
    providers.
  - Local models in role dropdowns are now emitted with `local:` prefix,
    so backend provider routing is unambiguous.
  - Local provider role options are filtered against the runtime
    OpenAI-compatible `/models` list when available, so only truly
    reachable local models are shown.
  - Save path normalizes role model ids and persists a coherent
    `active_provider` from toggled providers.
- Updated `README.md` Quick start run command to the working Windows
  repo-local Tauri CLI invocation:
  - `.\frontend\node_modules\.bin\tauri.cmd dev --config src-tauri/tauri.svelte.conf.json`
  - Added the global `cargo tauri dev ...` form as optional fallback only.
- Updated `config/model.json` local provider catalogue for GPU-server
  deployment, adding balanced runtime profiles discovered on the remote
  Ollama endpoint:
  - `rtx4090-qwen36:balanced`
  - `rtx4090-qwen35-9b:balanced`
  - `rtx4090-gemma4-26b:balanced`
  - `rtx4090-gemma4-e4b:balanced`
  - `rtx4090-gemma4-e2b:balanced`
  and refreshed the local-provider quality note accordingly.
- Fixed local OpenAI-compatible response handling for models that emit
  reasoning text instead of assistant `content`:
  - streaming parser now falls back to `delta.reasoning` /
    `delta.reasoning_content` when `delta.content` is empty;
  - non-stream completion parser now falls back to
    `message.reasoning` / `message.reasoning_content`.
  This prevents empty assistant turns with some `*balanced` profiles.
- Fixed local model resolution in `build_local_config`: when the selected
  model is explicitly prefixed (e.g. `local:rtx4090-qwen36:balanced`),
  the backend now uses that requested model id instead of silently
  overriding it with `local_model` from saved provider defaults. This
  removes cross-model mismatches and `502` errors on `/generate-title`.
- Refined local-output behavior to avoid exposing model thinking traces in
  chat UI: `llm/local` now emits only assistant `content` as user-visible
  text (stream + non-stream paths), ignoring `reasoning` fields that some
  OpenAI-compatible backends include separately.
- Added explicit server-console diagnostics for OpenAI-compatible upstream
  payloads (`llm/local`):
  - log each incoming SSE `data:` line from Ollama/vLLM as
    `[llm/local] upstream_sse ...` (truncated preview);
  - log non-stream completion raw body as
    `[llm/local] upstream_complete_body ...` (truncated preview).
- Forced non-thinking mode on OpenAI-compatible local requests
  (`llm/local` stream + complete now send `think: false`) to reduce
  pathological outputs where `content` was a single character (e.g. `S`)
  while long reasoning text consumed the completion budget.
- Fixed `llm/local` SSE parser buffering logic: when multiple upstream
  `data:` lines arrive in the same buffer, the parser now drains and
  queues all parseable events instead of returning only the first and
  dropping the rest. This addresses truncated one-character replies seen
  in chat despite richer upstream payloads.
- Fixed `/chat/:id/generate-title` empty-title persistence: when the title
  model returns empty `content`, backend now derives a deterministic
  fallback from the first user message (up to 5 words), and finally
  falls back to `New chat` if that is also empty. This prevents chats
  remaining `Untitled`.
- Updated Settings → Local provider UX:
  - swapped field order so `API key (optional)` appears before `Model`;
  - added a manual refresh button next to `Base URL` to query
    `<base>/models` on demand;
  - model roles (`Main`, `Title`, `Tabular`) now take local choices
    strictly from the runtime `/models` response (when local provider is
    active), so users can only pick actually available local models.
- Finalized Local provider constraint: `Model` is now a runtime-backed
  dropdown (not free text), populated only from `<base>/models` after
  refresh, so users cannot save arbitrary local model IDs.
- Follow-up per UX request: removed Local `Model` selector entirely from
  the Local provider card. Users now choose runtime local models only in
  Model roles (`Main`/`Title`/`Tabular`), with `Main` auto-defaulted to
  the first runtime local model after refresh when unset/invalid.
- Tightened global chat response-style instructions in
  `MRUST_SYSTEM_PROMPT` to improve formatting quality:
  - answer in the user's language;
  - keep responses concise and structured;
  - avoid repeating the same filename and duplicate document lists;
  - avoid verbose meta-reasoning unless explicitly requested.
- Added Recent Chats rename UX in the sidebar:
  - pencil button added immediately left of the trash icon;
  - inline title edit mode with Enter / blur save and Escape cancel;
  - rename persisted through `PATCH /chat/:id` and reflected in local
    chat list state.
- New-chat UX tweak: creating a fresh chat now closes the right-side
  document viewer panel (`docViewer.closeAll()`), so the new conversation
  starts with a clean workspace.
- Chat-rename UX polish: entering inline rename now autofocuses the input
  and places the caret at the end of the current title text.
- Settings → Active providers persistence: multi-selected provider chips
  are now restored on reopen (filtered to currently configured providers)
  via localStorage, so combinations like `Google + Local` survive page
  reopen; disabling `Local` and saving leaves only `Google` active.
- Fixed provider-toggle persistence timing: active-provider chips now
  write to localStorage immediately on toggle (not only on Save), so
  `Google + Local` remains selected after reopen even before a full
  settings submit.

### Tests

- Formal validation command executed:
  - `pnpm --dir frontend typecheck`
- Result:
  - `svelte-check found 0 errors and 0 warnings`
- Added unit coverage for citation mapping of backend `path` ->
  frontend `kbPath` in `frontend/src/lib/types/citation.test.ts`.
- Formal validation re-run after tracked-change implementation:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`
- Formal validation re-run after chat context-compaction update:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`
- Formal validation re-run after quality-first compaction strategy update:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`
- Formal validation re-run after model-aware budget update:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`
- Formal validation after local model-catalogue update:
  - `cargo check -p mike`
  - `Finished dev profile` (compilation OK; non-blocking warnings only)
- Formal validation after 24GB local shortlist alignment (Qwen 27B variants):
  - `cargo check -p mike`
  - `Finished dev profile` (compilation OK; non-blocking warnings only)
- Formal validation after context-window recommendation script:
  - `pwsh -File .\scripts\recommend-context-window.ps1 -Format json`
  - Exit code `0`; JSON report generated with model max context + recommended context.
- Formal validation after profile scripts and tuned aliases update:
  - `pwsh -File .\scripts\apply-ollama-context-profiles.ps1`
  - Exit code `0`; dry-run generated valid `ollama create` commands.
  - `pwsh -File .\scripts\verify-ollama-context-profiles.ps1`
  - Exit code `0`; aliases currently missing until explicit apply step.
  - `cargo check -p mike`
  - Compilation OK (`Finished dev profile`; non-blocking warnings only).
- Formal validation after Tauri Svelte config fix:
  - `.\frontend\node_modules\.bin\tauri.cmd dev --config src-tauri/tauri.svelte.conf.json`
  - Vite dev server started on `http://127.0.0.1:5173/` and Tauri DevCommand started successfully (running background process).
- Formal validation after local-role dropdown fix:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`.
- Formal validation after provider/model coherence fix:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`.
- Formal validation after README command update:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`.
- Formal validation after GPU-server balanced catalogue update:
  - `cargo check -p mike`
  - Compilation OK (`Finished dev profile`; non-blocking warnings only).
- Runtime validation after balanced catalogue update:
  - `.\frontend\node_modules\.bin\tauri.cmd dev --config src-tauri/tauri.svelte.conf.json`
  - Vite dev server started on `http://127.0.0.1:5173/`; Tauri DevCommand running.
- Formal validation after local-balanced runtime fixes:
  - `cargo check -p mike`
  - Compilation OK (`Finished dev profile`; non-blocking warnings only).
- Formal validation after thinking-visibility refinement:
  - `cargo check -p mike`
  - Compilation OK (`Finished dev profile`; non-blocking warnings only).
- Formal validation after upstream-payload logging enhancement:
  - `cargo check -p mike`
  - Compilation OK (`Finished dev profile`; non-blocking warnings only).
- Formal validation after forcing `think: false` on local requests:
  - `cargo check -p mike`
  - Compilation OK (`Finished dev profile`; non-blocking warnings only).
- Formal validation after SSE buffering/parser fix:
  - `cargo check -p mike`
  - Compilation OK (`Finished dev profile`; non-blocking warnings only).
- Formal validation after non-empty title fallback:
  - `cargo check -p mike`
  - Compilation OK (`Finished dev profile`; non-blocking warnings only).
- Formal validation after Local provider settings UX/runtime-model binding:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`.
- Formal validation after Local model field dropdown lock:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`.
- Formal validation after removing Local model selector and defaulting
  `Main` from runtime refresh:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`.
- Formal validation after response-style prompt hardening:
  - `cargo check -p mike`
  - Compilation OK (`Finished dev profile`; non-blocking warnings only).
- Formal validation after Recent Chats rename feature:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`.
- Formal validation after new-chat doc-viewer auto-close:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`.
- Formal validation after rename autofocus/caret UX polish:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`.
- Formal validation after active-providers persistence:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`.
- Formal validation after immediate provider-toggle persistence:
  - `pnpm --dir frontend typecheck`
  - `svelte-check found 0 errors and 0 warnings`.

### Process

- Per user request, every future development step must include at least
  one formal technical validation (typecheck/build/targeted tests), and
  both `HISTORY.md` and `PLAN_CODEX.md` must be updated with the
  executed command(s) and result(s) for manual re-verification.

## 2026-05-17 — Clean-room frontend rewrite (React → Svelte 5)

The AGPL Next.js UI has been replaced by a clean-room Svelte 5 + Vite +
Tailwind v4 application. The rewrite keeps the Rust backend contracts
intact while rebuilding every screen in a new component system and store
architecture.

### Added — Svelte 5 foundations

- clean-room scaffold + bootable Tauri shell; new design-system
  primitives and layout shell (`Fase 0–1`). ([`b86fe5f`](#),
  [`a6b7f3f`](#), [`9f20c21`](#))
- API layer + Svelte runes stores + auth flow (`Fase 2–3`).
  ([`5b45e71`](#))
- Routed shell + Workflows screen (`Fase 4`). ([`2e42c92`](#))
- Theme toggle (light/dark/system) and i18n store; locale bundles back
  to 6 languages. ([`70e75d9`](#), [`43251fb`](#), [`501f58d`](#))

### Added — Core product screens

- Assistant chat with streaming responses, attachments, and model
  selection. ([`c92f2a0`](#))
- Projects screen (list/create/edit/delete) with project detail workflow.
  ([`6ca253e`](#))
- Tabular reviews screen. ([`0b8fe49`](#))
- Workflow creation modal + editor path. ([`def3757`](#))
- Templates (DOCX) screen. ([`4e1f6bd`](#))
- Settings screens: profile/security, LLM models, MCP servers.
  ([`f8a7700`](#), [`3c6863e`](#), [`5f0b4ea`](#))
- Sidebar refinements (chat list, sticky settings). ([`9593df8`](#))

### Removed

- Legacy React/Next.js frontend removed from the repository.
  ([`0a9bbcf`](#))

### Docs

- README reframed to explain the blind Svelte rewrite and code
  independence; screenshots refreshed. ([`6342131`](#), [`46de002`](#))
- Svelte rewrite gap-analysis document added. ([`def85a1`](#))

## 2026-05-14 — Toolkit commercialista (22 workflows + 3 DOCX templates)

Mirror del toolkit medico-legale shipped earlier today, this time for
the Italian commercialista / dottore commercialista vertical. Maps
the 6 operational areas of `docs/piano_toolkit_commercialista.md`
under the canonical `finance` domain.

### Added — Workflow tabular (17)

**Trasversale (Fase 1 del piano, working life dello studio):**
- `commerc-inventario-documenti` (8 cols) — classificazione
  documentale universale per tutte le aree, ancorata a `DOC-NN`.
- `commerc-scadenzario-annuale` (7 cols) — calendario adempimenti
  fiscali italiani con responsabile interno.
- `commerc-portafoglio-clienti-stato` (10 cols) — dashboard
  semaforica per cliente su tutti gli adempimenti dell'anno.
- `commerc-quality-check-preinvio` (6 cols) — checklist quality
  pre-invio dichiarazioni (Mod. Redditi / IVA / 770).

**Area 1 — Perizie e stime di valore:**
- `commerc-riclassificazione-bilanci` (8 cols) — riclassificazione
  CE pluriennale a valore aggiunto.
- `commerc-indicatori-econ-finanz` (8 cols) — ROE/ROI/EBITDA
  margin/PFN-EBITDA/Current/DSO/DPO con benchmark di settore.
- `commerc-metodi-valutativi` (7 cols) — patrimoniale / reddituale
  / DCF / multipli ponderati → valore finale.

**Area 2 — CTU tributaria:**
- `commerc-contestazioni-ufficio` (7 cols) — estrazione rilievi
  da avviso di accertamento / PVC / cartella.
- `commerc-analisi-bancaria` (8 cols) — mappa movimenti c/c ex
  Art. 32 DPR 600/73 con classificazione Cass. SS.UU. 24823/2015.
- `commerc-rideterminazione-reddito` (6 cols) — confronto importi
  ufficio vs rideterminati.

**Area 3 — Crisi d'impresa e procedure concorsuali (CCII):**
- `commerc-indicatori-crisi` (7 cols) — DSCR / PFN-EBITDA / PN /
  liquidità / CCN con soglie di allerta.
- `commerc-stato-passivo` (8 cols) — classificazione creditori per
  rango (privilegiati / chirografari / postergati Art. 2467 c.c.).
- `commerc-confronto-piano-liquidatoria` (6 cols) — best interest
  of creditors test ex Art. 88 c.4 CCII.
- `commerc-cashflow-previsionale` (7 cols) — FCF previsionale
  5 anni con DSCR di piano.

**Area 4 — Due diligence fiscale e societaria:**
- `commerc-checklist-dd-documenti` (6 cols) — tracking documenti
  per area (fiscale / societario / previdenziale / giuslavoristico).
- `commerc-rischi-dd-semaforo` (10 cols) — risk map con semaforo
  🔴 / 🟡 / 🟢 + azione SPA consigliata (accantonamento / escrow /
  monitoraggio).
- `commerc-esposizione-fiscale-anno-tributo` (11 cols) — matrice
  anno × tributo (IRES / IRAP / IVA / Ritenute / INPS+INAIL) +
  sanzioni min/max + interessi → esposizione worst case.

**Area 5 — Contenzioso tributario (D.Lgs. 546/92):**
- `commerc-rilievi-controdeduzioni` (10 cols) — matrice
  rilievo × tesi difensiva × giurisprudenza × DOC probanti.
- `commerc-scadenze-processuali` (6 cols) — termini D.Lgs. 546/92
  (60 gg ricorso, 90 gg reclamo, 30 gg costituzione, 20 gg memoria,
  appello, Cassazione) con giorni residui.

**Area 6 — Adempimenti periodici:**
- `commerc-checklist-redditi-pf` (6 cols) — documenti per
  dichiarazione persone fisiche (CU, oneri detraibili / deducibili
  con soglie normative).

### Added — Workflow assistant (5)

Narrative output che chiude ciascuna area con la relazione
professionale finale:

- `commerc-relazione-stima` (Area 1) — relazione di stima d'azienda
  per cessione / conferimento / fusione / asseverata / Art. 2343 c.c.
- `commerc-relazione-ctu-tributaria` (Area 2) — relazione CTU /
  perizia di parte tributaria.
- `commerc-relazione-att-33-ccii` (Area 3) — attestazione del
  professionista indipendente ex Art. 33 CCII (D.Lgs. 14/2019).
- `commerc-report-due-diligence` (Area 4) — executive summary +
  raccomandazioni SPA.
- `commerc-ricorso-tributario` (Area 5) — ricorso tributario di
  primo grado ex Art. 18 D.Lgs. 546/92.

### Added — DOCX templates (3)

A4 portrait, Calibri 11pt justify. `also_applicable_to` cross-domain
per le perizie e contenziosi che attraversano legal / insurance /
compliance:

- `it/relazione-stima-valore` — Area 1, 9 sezioni dal Premessa agli
  Allegati. `also_applicable_to: [legal, real_estate, insurance,
  compliance]`.
- `it/attestazione-art-33-ccii` — Area 3, 11 sezioni con la formula
  finale ATTESTAZIONE POSITIVA / NEGATIVA in grassetto centrato.
  `also_applicable_to: [legal, compliance]`.
- `it/ricorso-tributario` — Area 5, 8 sezioni conformi a D.Lgs.
  546/92: intestazione → fatto → motivi → richieste → valore →
  istanza cautelare opzionale → firma → allegati.
  `also_applicable_to: [legal]`.

### Verified at boot

  55 workflow-preset(s) loaded (was 31, +22 finance + +2 ribilanciamenti)
  9 docx-template(s) loaded (was 6, +3 finance)

Backend Tauri dev ha auto-rilevato ciascun file e ricaricato senza
manual intervention. Test `presets::*` continuano a passare.

---

## 2026-05-14 — Medico-legale toolkit + chat-history persistence + generated-doc cleanup

Three converging streams shipped on the same day, all addressing the
"will my work still be here tomorrow?" question that the assistant
chat keeps raising:

### Added — Medico-legale toolkit (11 workflow-presets + 1 DOCX template)

Maps the 7 operational modules of `docs/piano_toolkit_medico_legale.md`
into MikeRust's preset registry under the canonical `medical` domain.
All assets load automatically at the next backend boot. Designed to
chain: each workflow references the `DOC-NN` codes minted by the
inventario, so the entire perizia flows from one tabella inventario
through timeline → diagnosi → ITT / IP → relazione finale without
re-numbering or re-attaching documents.

- **Tabular** (`config/workflow-presets/medical/`):
  `medlegal-inventario-documenti` (Mod. 1, 8 cols),
  `medlegal-timeline-cronologica` (Mod. 2, 8 cols),
  `medlegal-diagnosi-strumentali` (Mod. 3b, 9 cols — the workflow
  that classifies each reperto DIRETTA / INDIRETTA / ESCLUSIVA /
  DA VALUTARE for the nesso causale),
  `medlegal-postumi-temporanei-itt` (Mod. 4, 7 cols, includes the
  equivalente-ITT-100% formula in the prompt),
  `medlegal-postumi-permanenti-rc` (Mod. 5a, 9 cols — covers SIMLA
  2016 / 2025 + micropermanenti Art. 138-139 D.Lgs. 209/2005 +
  Balthazard),
  `medlegal-danno-biologico-inail` (Mod. 5b, 6 cols — D.Lgs. 38/2000
  + DM 12 luglio 2000 / DM 9 aprile 2008, soglie capitale / rendita),
  `medlegal-invalidita-civile` (Mod. 5c, 6 cols — DM 5 febbraio
  1992, soglie 33% / 46% / 67% / 74% / accompagnamento).
- **Assistant** (narrative): `medlegal-diagnosi-ingresso` (Mod. 3a,
  scheda strutturata da verbale PS), `medlegal-diagnosi-dimissione`
  (Mod. 3c, scheda strutturata da lettera di dimissione),
  `medlegal-nesso-causale` (Sez. 6 della relazione, sei sotto-sezioni
  cronologico / topografico / continuità / esclusione / concausa /
  conclusione), `medlegal-quality-check` (Mod. 7, 10-point checklist
  pre-firma).
- **DOCX template** (`config/docx-templates/it/relazione-medico-legale.json`):
  A4 portrait, Calibri 11pt justify, 11 sezioni dal Premessa agli
  Allegati. `also_applicable_to: [legal, insurance]` perché CTU e
  perizie assicurative la usano trasversalmente. 8 required_metadata
  (PERITO_NOME, PERITO_QUALIFICA, INCARICO_TIPO, DATA_EVENTO,
  QUESITI, REGIME_APPLICABILE, IP_TOTALE, ITT_GIORNI_EQUIV).

Registry totals at boot: **31 workflow-presets** (was 20, +11
medical) and **6 DOCX templates** (was 5, +1 medical-legal).

### Fixed — Chat reopen no longer loses uploaded files / workflow chip / template chip

Migration `0021_messages_user_metadata.sql` plus surgical updates to
`stream_chat` and `get_chat` close the symmetric gap left by
migration 0020:

- New nullable JSON columns on `messages`: `files`, `workflow`,
  `template`. Same pattern as `annotations` (0012) and `events`
  (0020) — one JSON blob per non-text aspect of a message.
- `stream_chat` now also fixes a related bug: the user message
  used to be stored with the marker-augmented content
  (`[Workflow: …]` / `[Template: …]` prefix injected for the LLM),
  which surfaced as literal text on chat reopen. Stored content is
  now the raw user-typed text; markers stay in the LLM input only.
- `get_chat` reads the three new columns and attaches them to user-
  role entries in the response, matching the wire shape the
  frontend's `getChat` already expects — no frontend change needed.
- `tests/chat_history_smoke.rs` is new: two round-trip cases
  exercising the full files/workflow/template/events path against
  a tempdir SQLite.

### Fixed — Generated `.docx` now linked to its originating chat (cascade cleanup)

Pre-existing loophole: `exec_generate_docx` inserted into `documents`
without `chat_id`, so deleting the chat orphaned the file on disk.
Three pieces:

- `builtin_tools::dispatch` gains a `chat_id: Option<&str>`
  parameter forwarded to `exec_generate_docx`. The single chat call
  site passes `Some(&chat_id_clone)`; `None` is kept legal for a
  future REST-only tool surface.
- `INSERT INTO documents` in `exec_generate_docx` now binds
  `chat_id` so the FK cascade on chat deletion catches the row.
- `delete_chat` cleanup loop gains a second branch for
  non-hashed (generated) documents: each has a unique
  `documents/<user_id>/<doc_id>` storage path with no possible
  aliasing, so the file is freed unconditionally. Tracing now logs
  the split between cache-hash deletions and generated-doc
  deletions.

Net effect on the user-visible side: the toolkit produces relazioni
medico-legali whose DOC-NN inventario, timeline, diagnosi, ITT,
IP, nesso, and final `.docx` all persist together for the lifetime
of the chat, then disappear cleanly when the chat is deleted. No
orphans on either end.

---

## 2026-05-14 — ort load-dynamic resolved: onnxruntime 1.24.2 ABI match (critical RAG fix)

After three failed attempts that diagnosed the wrong cause, the
load-dynamic embedding path is finally **stable on Windows 11 ARM64
(Snapdragon X Elite) and x64**. The root cause was *not* the DLL
binary (we had Microsoft's official stock release), but an **ABI
version mismatch**: `ort 2.0.0-rc.12` statically compiles against
**onnxruntime 1.24.2**, while the vendored DLL we were shipping was
1.20.0. Four releases of drift in the function-pointer table caused
`TextEmbedding::try_new_from_user_defined` to deadlock silently at
the first missing symbol lookup — no error, no log, the
`spawn_blocking` just never returned.

### Fixed — RAG hang on first chat (Snapdragon X Elite)

- **Vendored DLLs bumped to onnxruntime 1.24.2** for `win-arm64`
  (SHA256 `D4C4D939…`) and `win-x64` (SHA256 `114947D6…`). Source:
  Microsoft's official `v1.24.2` release on GitHub. With matching
  versions, session init returns in **2269 ms ARM64 native** /
  **3248 ms x64 under Prism emulation** — equivalent to the
  previously-working static-link timing.
- **`ort/load-dynamic` re-enabled** (cf. commit `c781d57`,
  reversing the revert from `56ab9a1`). The runtime DLL is now a
  distributable artifact users can swap independently of the Rust
  toolchain. The earlier reverts had concluded "load-dynamic
  fundamentally hangs on ARM64" — that diagnosis was wrong; the
  fault was downstream of the load itself.
- **`ensure_onnxruntime_dylib_path()` call restored** in
  `src/lib.rs::run_server_with_channels()`, pre-AppState so the
  env-var is visible before EmbeddingService constructs its session.
- **Step 1/4 → 4/4 logging restored** in
  `src/embeddings/service.rs::ensure_model_ready()`. The cleanup
  commit `b8c25b8` had removed these as "diagnostic noise"; without
  them, the second hang attempt produced zero observable signal
  inside `spawn_blocking` (cargo test buffers stdout) and required
  `Get-Process` to confirm the binary was even alive. They stay in
  as the canonical diagnostic for any future version-drift regression.

### Added — diagnostic recipe

Knowing the **exact** onnxruntime version `ort-sys` linked against
is the only way to choose the right vendored DLL. The string
`branch=rel-1.24.2, git-commit=...` is baked into the compiled
`mike-tauri.exe` by pyke's CI build — extract it with:

```powershell
Select-String -Path target\debug\mike-tauri.exe `
  -Pattern 'branch=rel-\d+\.\d+\.\d+' -Encoding Default
```

This must be checked **every time `ort` / `fastembed` / `ort-sys` is
bumped**. ABI drift across more than one minor (1.20 → 1.24 is the
worst observed so far) is a silent deadlock, not a build error or a
runtime panic — there is no fail-fast.

### Verified

| metric | ARM64 native | x64 emulated (Prism) |
|---|---|---|
| `try_new_from_user_defined` | 2269 ms | 3248 ms |
| Cold embed (1 query) | 26 ms | 131 ms |
| Warm embed | 5 ms | 26 ms |
| Batch-16 passages | 81 ms (5 ms/passage) | 238 ms (14 ms/passage) |
| FP32 quality drift mean | 0.984 | — |
| Top-1 ranking agreement | 4/4 | — |

Test suite green on the new build: `embedding_perf` 3/3
(`perf_fp32_intfloat`, `perf_int8_xenova`, `quality_fp32_vs_int8`)
+ `workflows_smoke` 6/6 + `service::tests::find_onnxruntime_dylib`
6/6, plus the x64 cross-compiled `embedding_perf` executed under
Windows 11's Prism translation layer.

### Docs

- [`README.md`](README.md) — "Critical fix — ort/onnxruntime ABI
  version match" subsection in "RAG: hardware acceleration", listing
  the three changes that restored the pipeline and the upgrade
  protocol.
- [`libs/onnxruntime/README.md`](libs/onnxruntime/README.md) —
  "Version: must be exactly 1.24.2 for ort 2.0.0-rc.12" subsection
  + `branch=rel-` `Select-String` recipe + every download URL bumped
  from 1.20.x to 1.24.2.

### Changed — Cargo.toml

- `fastembed`: `default-features = false`, features `[hf-hub-native-tls,
  image-models, ort-load-dynamic]`.
- `ort = "=2.0.0-rc.12"`: `default-features = false`, features
  `[std, load-dynamic]`.

Both back to the shape commit `b565e08` originally introduced — the
revert that intervened (`56ab9a1`) was based on the misdiagnosis. The
Cargo.toml comment block documents both prior attempts so the next
person tempted to "ship the DLL separately" reads about the
*real* failure mode (ABI drift) instead of repeating the chase.

---

## 2026-05-14 — DOCX template wiring (Phase 1.A.2 — LLM tools + HTTP endpoint)

Glue between the renderer (Phase 1.A.1) and the rest of the system:
LLM tools to discover and use templates, an HTTP endpoint for the
frontend to download `.docx` files directly, and workflow → template
wiring so a workflow can declare "I produce a Diffida" and the
authoring contract flows through to the model in one round-trip.

### Added — LLM tools

- **`list_docx_templates(domain?, locale?)`** — returns every loaded
  template with id, display_name, category, domain, automation_level,
  required_metadata, and source_reference. Filters by canonical
  domain enum or locale prefix. First step of the workflow:
  "user wants to produce a structured doc → discover what's
  available".
- **`describe_docx_template(template_id)`** — returns the full
  authoring contract: the auto-generated `prompt_md` (composed
  programmatically from sidecar fields per `docs/TEMPLATE_PRONTUARIO.md`
  Parte V) plus the raw sidecar for introspection. Second step:
  "I picked the Diffida — how do I write it?". The model injects
  the returned `prompt_md` into its working context and then writes
  the body following the section_skeleton.
- **`generate_docx(body_md, template_id?, metadata?, title?)`**
  extended — when `template_id` is supplied, routes through the new
  `src/docx::render` pipeline with `metadata` as the `[PLACEHOLDER]`
  bag; when omitted, falls back to the legacy
  `markdown_to_docx(title, body)` path for backwards compat. Response
  includes `unresolved_placeholders` when any `[TOKEN]` was left
  unfilled, so the LLM can surface the gap to the user instead of
  pretending the document is complete.

### Added — `DocxTemplate::auto_generated_prompt_md(locale)`

Composes the closing-formatter contract entirely from structured
sidecar fields — layout (paper, margins, typography, footnotes),
placeholder syntax, required_metadata with per-field guidance from
`field_prompts`, the full section_skeleton with `[REPEATING BLOCK]`
markers for L3 templates, character_limits for atti difensivi, and
the author override `prompt_md_extra` if present. Closes with the
canonical "call `generate_docx(template_id="...", ...)`" instruction.

Means: editing the sidecar JSON propagates to the LLM prompt at the
next restart. Zero drift between what the engine renders and what
the LLM thinks it should write. Two unit tests anchor the output to
the shipped Diffida (lists DEBITORE/IMPORTO/TERMINE_GG, mentions
the canonical "DIFFIDA E METTE IN MORA" formula) and Parcella
(marks `voci_onorario` as `[REPEATING BLOCK]`).

### Added — Workflow → Template wiring

- `WorkflowPreset.default_output_template: Option<String>` — opt-in
  field on the workflow sidecar. When set on an `assistant`-type
  workflow, the chat handler doesn't need to pre-load anything —
  the existing `read_workflow` tool now bundles the linked template
  in its response when invoked.
- `read_workflow` extended: short-circuits to the preset registry
  first (previously DB-only — preset workflows would 404), and when
  the workflow has a `default_output_template`, attaches the
  template's id + display_name + automation_level + required_metadata
  + auto-generated prompt_md as `default_output_template.*` in the
  same JSON response. A `closing_instruction` field explicitly tells
  the LLM "this workflow ends with a `generate_docx` call".

### Added — `POST /docx-templates/{describe,render}`

Frontend-facing HTTP endpoints (LLM-tool-equivalent but for the UI):

- **`POST /docx-templates/describe`** — body `{ template_id, locale? }`,
  returns the same payload as `describe_docx_template` tool. Powers
  the workflow editor's "Preview authoring contract" affordance.
- **`POST /docx-templates/render`** — body `{ template_id, body_md,
  metadata, filename? }`, returns the rendered `.docx` bytes with
  `Content-Disposition: attachment` and a custom
  `X-Unresolved-Placeholders` header listing any `[TOKEN]` still
  present. The UI uses this for "anteprima" / "scarica subito" flows
  without persisting the doc in the user's documents list.

Template ids contain `/` (e.g. `it/diffida-messa-in-mora`) so we
pass them in the JSON body instead of the URL path — sidesteps
URL-encoding pitfalls in clients.

### Tests

- `auto_generated_prompt_md_contains_layout_and_skeleton` —
  anchors the prompt output to the real shipped Diffida sidecar.
- `auto_generated_prompt_md_marks_repeating_blocks` — verifies
  `[REPEATING BLOCK]` markup for L3 templates (Parcella).
- Updated `schemas_have_required_fields` to expect 7 tools (was 5).
- Updated `is_builtin_recognises_each_tool` to include the two new
  tool names.

Full crate test count: 296/296 green (the 2 new + the 294 baseline).

### What's still deferred to later

- The `read_workflow` extension only walks the preset registry for
  the workflow itself; the DB branch can't carry
  `default_output_template` yet because the SQL `workflows` table
  doesn't have a corresponding column. Phase 2 will add a migration
  + expose the field in the workflow editor.
- Frontend `/account/templates` page (preview / list / link from
  workflows) — Phase 2.

---

## 2026-05-14 — DOCX renderer (Phase 1.A.1 — end-to-end rendering)

The other half of the template subsystem: the actual pipeline that
turns a sidecar JSON + LLM-produced Markdown + a metadata bag into a
print-ready `.docx`. No `.dotx` intermediate — every template ships
as JSON only and the renderer builds styles.xml and document.xml
from scratch at request time. Aligns with the architectural
decision recorded yesterday: the tool is the closing formatter, not
a Word template management system.

### Added — `src/docx/` module

Five sub-modules, all pure-function, all zero-I/O on the hot path:

- **`it_helpers.rs`** — Italian-locale formatters: `format_italian_date`
  ("Cremona, 14 maggio 2026"), `format_italian_amount` ("€ 1.234,56"
  with dot-thousands / comma-decimal), `format_protocol_block`
  ("Prot. n. 12345/2026<TAB>Cremona, 14 maggio 2026" for PA letters).
- **`placeholders.rs`** — `[NAME]` substitution against a HashMap bag.
  Pure-text, UTF-8-safe (regression test guards against
  `bytes[i] as char` corrupting multi-byte sequences), with grammar
  `[A-Z0-9_.]+` so `[label](url)` Markdown links and lowercase
  brackets are correctly ignored. `find_remaining_tokens` reports
  the unfilled fields back to the renderer.
- **`styles_xml.rs`** — builds `word/styles.xml` from the sidecar's
  `typography` + `style_map_baseline` + `style_map`. Every conversion
  in one place: points → half-points, cm → twips
  (`cm_to_twips(3.5) == 1984` matches Word's own value),
  line-spacing multiplier → 240-base, alignment → `<w:jc>`. Style
  IDs are PascalCase ASCII (Word's grammar); `w:name` carries the
  localised Italian text. The 4 baseline styles (`BodyText`,
  `SectionHeading` with `<w:b/>` + `<w:caps/>` and +2pt bump,
  `Citation` with italic + 1.5cm indent, `Footnote` 10pt single)
  are always emitted; extra per-template styles inherit from
  `BodyText`.
- **`document_xml.rs`** — `pulldown-cmark` events → WML paragraphs.
  Headings 1/2/3 → `SectionHeading`, paragraphs → `BodyText`,
  ordered/unordered lists with manual marker (`•` / `1.`), bold +
  italic + code spans. Page size from `paper`, margins from
  `margins_cm` via `cm_to_twips`. Tables / footnotes / blockquotes /
  page-break directive deferred to Phase 2 (the 4 shipped templates
  don't need them yet).
- **`package.rs`** — zip the result with the canonical OOXML layout:
  `[Content_Types].xml`, `_rels/.rels`, `word/_rels/document.xml.rels`,
  `word/document.xml`, `word/styles.xml`. The three boilerplate
  fragments are const strings (they never vary).
- **`mod.rs`** — public API: `render(template, body_md, metadata) →
  RenderOutcome` with `bytes` + `unresolved_placeholders`. Pipeline
  is: substitute on the Markdown source (BEFORE the parser, so
  pulldown-cmark doesn't consume `[X]` as a CommonMark
  shortcut-reference link) → render body XML → wrap with page setup
  → build styles.xml → package zip. Missing required_metadata fields
  log a warning but the render proceeds anyway — gaps surface as
  `[UNFILLED_TOKEN]` in the document, which is the loudest possible
  proofread cue. The Markdown parser's text-event escape pass writes
  every text run with `xml_escape`, so values with `&`, `<`, `>`,
  apostrophe end up as proper XML entities (`&amp;`, `&lt;`, …).

### Tests

- 58 unit tests across the 5 sub-modules: every public function has
  at least one focused test for its contract; cross-cutting
  invariants (XML well-formedness, ASCII style IDs, UTF-8 round-trip,
  alignment-to-OOXML mapping) covered by dedicated tests.
- 3 integration tests in `src/docx/mod.rs::tests` exercising the
  full render against the shipped `it/diffida-messa-in-mora`
  template: end-to-end zip output with the canonical 5 parts,
  XML-special-char escaping in metadata values, and the
  unresolved-placeholder reporting on missing fields. All 294 crate
  tests green (the 58 new + the 236 pre-existing).
- Drive-by fix to `corpora::plugin::tests::walk_ancestors_finds_corpora_plugins`
  whose test fixture predated commit f9d6bf5 (which moved
  `corpora-plugins/` under `config/`).

### What's still deferred to later sub-phases

- Phase 2 in module: tables (GFM), footnotes (`[^N]`), blockquotes
  (`> testo` → `Citation` style), `---PAGE---` directive, YAML
  front-matter parser for cover-page metadata.
- Phase 1.A.2 (next commit): wire `generate_docx` builtin tool +
  `list_docx_templates` + `describe_docx_template` so the LLM can
  discover and use the registry; the `POST /docx-templates/:id/render`
  HTTP endpoint that streams the bytes.

---

## 2026-05-14 — DOCX template registry (Phase 1.A foundation)

First lap of the structured-output system. The DOCX template registry
becomes a first-class entity alongside workflows / column-presets /
model catalogue, with sidecar JSON files describing layout, authoring
contract, and automation level for each shipped template. Loaded at
startup, exposed via `GET /docx-templates`, ready for the renderer
pipeline (Phase 1.A.1, next lap).

### Added — DocxTemplate registry

- **`docs/TEMPLATE_PRONTUARIO.md`** — authoritative Italian-professional
  template specification (Versione 1.0, May 2026): 9 schede + 5 sotto-
  schede ad alto volume covering CTU, Atto difensivo, Comunicazione PA,
  Risposta AdE, Commercialista, Rogito notarile, Diffida, Locazione,
  Verbale, Istanza PA, Parcella, Procedura ISO, Ricorso tributario.
  Each scheda lists paper size, margins, typography, structure, layout
  conventions, automation level (L1-L4) and priority (★).
- **`src/presets/docx_template.rs`** — schema parser mirroring the
  workflow / column-preset registry pattern. Full type system:
  `Paper` (with `format: standard | uso_bollo`), `UsoBollo` (special
  notarial-deed constraints), `MarginsCm`, `Typography`, `Footnotes`,
  `SectionSkeletonEntry` (with `repeating: bool` for L3 automation),
  `CharacterLimits` (D.M. 110/2023 — atti difensivi),
  `FewShotExample`, `DocxTemplate` root. Style-map baseline (4
  canonical styles: `body_text`, `section_heading`, `citation`,
  `footnote`) hard-coded as default — every template inherits, can
  override. English snake_case canonical IDs everywhere (memory feedback),
  Word style names embedded inside the `.dotx` files stay localised.
- **`AppState.docx_templates`** populated at startup from
  `config/docx-templates/<domain>/<slug>.json`. Same fail-soft policy
  as the other registries: a broken template logs a warning and the
  rest still load.
- **`config_subdir(name)`** helper in `src/presets/mod.rs` for
  resolving any config subfolder that doesn't follow the
  `<kind>-presets` naming. Used by docx-templates (`config/docx-templates/`);
  generalises cleanly to future registries.
- **`GET /docx-templates`** route with optional `?domain=` and
  `?locale=` filter query params. Returns each template's full
  sidecar JSON plus synthesised `is_system: true` / `is_owner: false`.

### Added — first 4 shipped templates (★★★★★)

The four highest-priority templates from the Prontuario, all on disk
at `config/docx-templates/`:

- **`it/diffida-messa-in-mora`** (legal, L1) — Calibri 11 / 1.15 /
  margini 2.5 cm, 1 pagina, formula `DIFFIDA E METTE IN MORA` con
  termine perentorio in grassetto corsivo. 7 campi richiesti
  (`DEBITORE`, `CF_PIVA`, `INDIRIZZO`, `DESCRIZIONE_INADEMPIMENTO`,
  `IMPORTO`, `TERMINE_GG`, `ALLEGATO_PROVA`).
- **`it/parcella-professionale`** (finance, L3) — Calibri 11 / 1.15 /
  blocco americano. Voci di onorario come blocco ripetibile (L3),
  contributo integrativo 4% (avvocati) o 2% (commercialisti), IVA 22%
  con esenzione per regime forfetario, IBAN obbligatorio.
- **`it/contratto-locazione`** (real_estate, L2) — TNR 11 / 1.5 /
  margine sx 3 cm. Rami condizionali su `TIPO_CONTRATTO` ("Canone
  libero 4+4" / "Canone concordato 3+2" / "Contratto transitorio")
  che cambiano artt. 2 (durata) e 3 (aggiornamento ISTAT). 11 articoli
  fissi, parti in MAIUSCOLETTO, foro inderogabile.
- **`compliance/procedura-iso-sgi`** (compliance, L3) — Calibri 11 /
  1.15 / margini 2.5 cm uniformi. Header con `[AZIENDA] · CODICE ·
  REVISIONE · DATA`. Sezione 4.2.2 "Scheda processo" come blocco
  ripetibile L3 con 15 campi fissi (Nome, Responsabile, Documenti,
  Vincoli normativi, Input/Output, Indicatori, Rischi, Punti di forza,
  ecc.). Domain `compliance` estende il sistema oltre il legal-tech.

### Tests

- 9 unit test in `src/presets/docx_template.rs`:
  schema parsing (minimal + character_limits + display_name fallback),
  validation (domain enum, automation_level enum, uso_bollo block
  presence, placeholder_syntax enum), API-shape projection, and
  `shipped_templates_all_load_cleanly` — integration test that opens
  the real `config/docx-templates/` tree and verifies the 4 shipped
  templates are all parseable + valid. All green.

### Next

Phase 1.A.1 (next commit): the actual rendering pipeline —
`src/docx/` module that maps Markdown + YAML front-matter into the
companion `.dotx`, binds `[PLACEHOLDERS]`, and emits print-ready
`.docx`. Then the LLM tool `generate_docx` learns to accept
`template_id` and pull `prompt_md` from the sidecar.

---

## 2026-05-14 — ONNX Runtime: load-dynamic + every execution provider

The embeddings pipeline switches from the statically-baked
onnxruntime that fastembed used to ship to a fully **load-dynamic**
setup, with all 18 execution providers ort exposes now available
behind cargo feature flags.

### Changed — ONNX Runtime loading

- **`ort/load-dynamic` everywhere** — the onnxruntime native library
  is no longer downloaded by fastembed at build time and no longer
  statically linked into the binary. Instead the runtime DLL lives in
  `libs/onnxruntime/<platform>/` (vendored locally to the project, the
  same pattern already used for pdfium) and is loaded at process
  start via `ORT_DYLIB_PATH`. Three motivations:
  *(a)* reproducibility — the runtime version we test against is the
  one we ship; *(b)* sovereignty — no implicit dependency on whatever
  `onnxruntime.dll` lives in `system32`; *(c)* security — no
  `LoadLibrary` against the system search path means no DLL-hijack
  surface where a poisoned onnxruntime on PATH could be picked up
  before ours.
- **`ensure_onnxruntime_dylib_path()`** in
  [`src/embeddings/service.rs`](src/embeddings/service.rs) — the
  startup hook called from `run_server_with_channels()` before
  AppState is constructed. Walks cwd and exe ancestors looking for
  `libs/onnxruntime/<platform>/{onnxruntime.dll | libonnxruntime.so
  | libonnxruntime.dylib}` and exports the absolute path via
  `ORT_DYLIB_PATH`. Mirrors the pdfium discovery walk so dev runs
  (cwd = workspace root) and bundled runs (cwd = `src-tauri/`) both
  work without extra config.
- **`fastembed`** is now pulled in with `default-features = false`
  and only the `hf-hub-native-tls`, `image-models`,
  `ort-load-dynamic` features turned on — the previous default
  brought in `ort/download-binaries` which baked a CPU-only
  onnxruntime into every release build.

### Added — every ort execution provider as a cargo feature

Each ONNX Runtime execution provider exposed by ort 2.0.0-rc.12 now
has its own `rag-<ep>` cargo feature. Enabling a feature only
toggles the Rust API surface — the actual EP code lives in the
runtime DLL the user drops into `libs/onnxruntime/<platform>/`, so a
binary built with `--features rag-accel-all` works on a machine that
only has the CPU build of onnxruntime (ort silently skips providers
whose backend DLLs aren't loadable).

- **NPU class:** `rag-qnn` (Qualcomm Hexagon), `rag-cann`
  (Huawei Ascend), `rag-nnapi` (Android NN API), `rag-rknpu`
  (Rockchip), `rag-vitis` (AMD/Xilinx FPGA).
- **GPU class:** `rag-tensorrt`, `rag-cuda` (NVIDIA),
  `rag-migraphx`, `rag-rocm` (AMD), `rag-directml` (DX12 on
  Windows), `rag-coreml` (Apple Silicon ANE/GPU),
  `rag-webgpu` (cross-platform), `rag-openvino` (Intel
  CPU/iGPU/VPU).
- **Optimised-CPU class:** `rag-onednn` (Intel), `rag-acl` (ARM
  Compute Library), `rag-xnnpack` (mobile-class kernels),
  `rag-tvm` (Apache TVM).
- **Service class:** `rag-azure` (Cognitive Services off-load).
- **Convenience umbrellas:** `rag-accel-windows`, `rag-accel-linux`,
  `rag-accel-macos`, and `rag-accel-all` (every EP at once).

The `build_execution_providers()` helper in `src/embeddings/service.rs`
now registers every configured provider in the canonical
NPU → GPU → optimised-CPU → service order. ArmNN was retired
(removed from upstream ONNX Runtime — use ACL / XNNPACK / the
Kleidi-optimised CPU EP instead).

### Added — `libs/onnxruntime/` directory layout

- `libs/onnxruntime/{win-x64, win-arm64, linux-x64, linux-aarch64,
  macos-x64, macos-arm64}/` — one subdir per platform. Tracked in
  git as empty (`.gitkeep`) so the structure is discoverable on a
  fresh clone, but the runtime libraries themselves are
  **gitignored** (`*.dll`, `*.so`, `*.dylib`, `DirectML.dll`,
  `onnxruntime_providers_*.dll`, `Qnn*.dll`, archives) — each
  contributor fetches the variant matching their hardware.
- `libs/onnxruntime/README.md` — full layout reference + per-EP
  download recipes (CPU / DirectML / CUDA / TensorRT / OpenVINO /
  QNN / CoreML / ROCm) with PowerShell quick-fetch examples.
- README quickstart updated with a new "step 2" pointing at the
  onnxruntime fetch flow, alongside the existing pdfium step.

---

## 2026-05-13 — Plugin system, French locale, project refactor, dynamic port, migration auto-heal, domain column, JSON-preset registries, insurance vertical

The big day for declarative ingestion. The medium-term goal of the
project ("download legal documents locally via a plugin manifest") goes
from prose to running code: a JSON schema, a generic loader, a generic
HTTP-driven and a generic DILA-bulk-XML strategy, and a first
proof-of-concept consumer — CNIL — that imports ~26 000 délibérations
from the French Open Data archive into a local FTS5 index in a single
click. The same day brings a project-page refactor mirrored from
upstream `willchen96/mike`, the French locale, a UX polish pass on the
new corpus panel, two pieces of platform hardening in the afternoon
(the axum backend no longer binds to a hardcoded port, and sqlx
migration checksum drift now self-heals at startup instead of crashing
the app), and — in the late afternoon — a second wave that lifts the
workflow / column-preset built-ins out of TypeScript constants into
JSON files, adds a professional-vertical `domain` column across the
four config tables, ships the insurance vertical with 6 workflows
plus 17 column-presets, and polishes the picker modals so the user
can switch domain on the fly.

### Added — plugin system

- **JSON-manifest plugin schema v1** for legal corpora
  (`corpora-plugins/*.json`): each manifest declares `id`, `display_name`
  (with optional per-locale overrides), `homepage`, `languages`,
  `identifier_label`, `license`, and a `strategy` discriminator. Loader
  walks `corpora-plugins/` from the workspace root or the binary's
  ancestors so both `cargo run` and `cargo tauri dev` find the manifests
  ([`f777405`](#), [`dd1c3c2`](#)).
- **Schema v2** — adds explicit `capabilities` (`search`, `fetch`,
  `documents`, `documents_delete`, `documents_resync`, `embed_progress`,
  `bulk_import`, `user_config`) and a list of `sources` so a single
  corpus can ship multiple sub-toggles (e.g. CNIL → délibérations /
  recommandations / avis / guides pratiques). Sources are surfaced in
  the UI with available / coming-soon grouping ([`c123400`](#)).
- **`http-fetch-per-id` strategy** — declarative single-document fetcher
  driven by a URL template, with CSS / JSONPath extractors for body and
  metadata, automatic anti-bot detection (Cloudflare, AWS WAF, Akamai)
  and a structured `ManifestAdapter` registry on `AppState`
  ([`dcb5e43`](#), [`cfc38b9`](#)).
- **`dila-bulk-xml` strategy** — generic importer for DILA OPENDATA
  archives (CNIL / LEGI / JORF / CASS / KALI share the same XML schema):
  scrape the Apache directory index for the latest `Freemium_*` tarball,
  download, walk the tar in `tokio::task::spawn_blocking`, SAX-parse each
  XML entry (`quick-xml`), batch-insert into a new `corpus_documents`
  table backed by an FTS5 virtual table with `unicode61 +
  remove_diacritics`. Idempotent: skip the import if the same
  archive_ts is already on record ([`87f6907`](#), [`c388beb`](#)).
- **CNIL** — first plugin to exercise the bulk strategy end-to-end. ~18 MB
  archive, ~26 000 délibérations indexed locally, Etalab 2.0 licence
  attribution wired through to the UI footer. Zero anti-bot exposure
  (vs. the abandoned Légifrance attempt earlier the same morning).
- **Generic routes**: `/corpora`, `/corpora/:id`, `/corpora/:id/search`,
  `/fetch`, `/documents`, `/documents/:doc_id` (DELETE),
  `/import`, `/import-status`, `/import-progress`. Dispatch picks
  `dila-bulk-xml` short-circuit, `ManifestAdapter` registry, or 501
  with a hint for builtin corpora that still ride their dedicated
  routes ([`812638f`](#)).
- **`migrations/0017_corpus_documents.sql`** — composite-key table
  `(corpus_id, identifier)` plus FTS5 virtual table indexing
  `titre / titre_full / numero / body`, plus a per-corpus
  `corpus_imports` snapshot row so the UI can render
  "Snapshot from YYYY-MM-DD · N documents indexed".
- **End-to-end test** for the DILA bulk path: insert a real
  CNIL XML fixture into a fresh DB, run `search_local_index`, assert FTS5
  matches. Pinned a production bug: FTS5 `MATCH` clauses must reference
  the bare table name, not an alias ([`f21c2db`](#)).
- **Data-driven sidebar** — the Account → Documents & sources section
  now renders from `/corpora`; runnable corpora are clickable, declared-
  but-not-yet-wired corpora are dimmed with the manifest description as
  tooltip. New corpora appear in the sidebar the moment their JSON ships
  ([`a83d2a4`](#)).
- **Generic corpus settings page** (`/account/corpora/[id]`) — renders
  capabilities, sources, search box, bulk-import button + live progress
  bar, indexed-docs list with trash icon, license footer with deep link
  to the original source opened in the OS default browser ([`9c444b6`](#),
  [`f78c8bb`](#)).
- **Per-row indeterminate progress strip** under each search hit while
  its fetch is in flight, so the user sees motion during the
  multi-second embedding step ([`f78c8bb`](#)).
- **Concurrent syncs** — `syncing` state is now a `Set<string>` with
  functional `setState`, so multiple "Indicizza" clicks each keep their
  own spinner + strip and the backend `POST /corpora/:id/fetch` calls
  run in parallel ([`f78c8bb`](#)).
- **Re-indicizza state** — the index-button in the search list switches
  to "Re-indicizza" when the hit's identifier is already in the local
  index (cross-referenced via Set) ([`f78c8bb`](#)).

### Added — projects refactor (upstream sync)

- **`PATCH /project/:id/document/:docId/rename`** — port from upstream
  `willchen96/mike@f39f175`: rename a document inside a project, with
  filename uniqueness check and `updated_at` bump. Wired to a
  `RowActions` UI in the Documents tab ([`ca4073b`](#), [`0ea5161`](#)).
- Frontend `ProjectPage` decomposed into focused components — layout
  helpers + `DocVersionHistory` moved to `ProjectPageParts`,
  `ProjectPageSkeleton` extracted, Assistant tab and Reviews tab become
  their own files. Same behaviour, smaller diffs from now on
  ([`b60feda`](#), [`a967dab`](#), [`41ea283`](#)).

### Added — internationalisation

- **French locale** (`fr.json`) shipping with 658-key parity to `en` and
  `it`. Locale list goes from `["it", "en"]` to `["it", "en", "fr"]`;
  `LanguageSwitcher` picks up a `Français` button; backend
  `PUT /user/locale` whitelist accepts `"fr"`.
- New i18n namespace `Corpora` (with sub-namespace `Corpora.bulk`)
  feeds the generic corpus settings page; EUR-Lex page stragglers
  (`Sync…` / `Indicizzato` / `Indicizza`) are now translated through
  `useTranslations` too.

### Added — platform hardening (afternoon)

- **Dynamic port for the embedded axum server.** Default mode is now
  `port = 0` (OS picks a free high port); the actual port is reported
  back to the Tauri shell via a `tokio::oneshot` channel, stashed in
  `Arc<Mutex<Option<String>>>` managed state, and exposed to the
  frontend through a new `api_base_url` Tauri command. The frontend
  has a tiny `lib/apiBase.ts` module that calls `invoke("api_base_url")`
  once at boot and caches the result; all 22 existing `API_BASE`
  consumers (login, signup, AuthContext, UserProfileContext, account
  pages, components, hooks, the central `apiRequest` wrapper) now read
  through `apiBase()` instead of a build-time env constant. Pre-mount
  fetches (`AuthContext`, login probes) await `apiBaseReady` to avoid
  racing the IPC handshake. Outside Tauri (plain Next.js dev in a
  browser), discovery fails fast and falls back to
  `NEXT_PUBLIC_API_BASE_URL` ([`src/lib.rs`](src/lib.rs),
  [`src-tauri/src/lib.rs`](src-tauri/src/lib.rs),
  [`frontend/src/lib/apiBase.ts`](frontend/src/lib/apiBase.ts)).
  `.env.example` updated to leave `PORT` commented out by default.
- **Migration auto-heal at startup.** When sqlx detects checksum drift
  on a previously-applied migration (the dev edited a migration file
  in place after first run), `AppState::run_migrations` now catches the
  error, asks sqlx itself for the expected checksum of every bundled
  migration, rewrites the `_sqlx_migrations` rows in-process, and
  re-runs. Safe because every migration in the repo uses
  `CREATE … IF NOT EXISTS`. Self-contained — no external `sqlite3`
  binary, no manual `DELETE FROM _sqlx_migrations` SQL, no second
  helper executable. Surfaces `WARN [migrations] checksum drift
  detected — auto-healing` then `INFO [migrations] checksum drift
  healed; resume normal startup` in the log
  ([`src/db/mod.rs`](src/db/mod.rs)).

### Added — professional domain column (late afternoon)

- **`domain` column on 4 tables** (migration 0018) — `workflows`,
  `tabular_reviews`, `projects`, `documents` each gain a
  `TEXT NOT NULL DEFAULT 'legal'` column with a `(user_id, domain)`
  composite index. Mike-inherited rows backfill to `legal` (the
  upstream tool was law-firm-focused). Canonical 9-domain set
  validated at the API boundary (`crate::domain::DOMAINS`): `legal`,
  `medical`, `finance`, `real_estate`, `hr`, `insurance`, `ip`,
  `compliance`, `others`. No SQL CHECK — adding a future domain is a
  one-line edit to the Rust constant + the frontend `Domain` union +
  one i18n label per locale.
- **Backend filters** — list endpoints accept `?domain=…`; create
  endpoints accept an optional `domain` body field (normalised to
  `legal` when missing/invalid); update endpoints are strict (reject
  unknown values with 400, treat omitted as "leave unchanged"). The
  document upload path inherits from the parent project at INSERT
  time when `project_id` is set; standalone uploads accept the
  `domain` form field.
- **Frontend wiring** — `Domain` union + `DOMAINS` array constant in
  `shared/types.ts`; `MikeWorkflow` / `MikeProject` / `MikeDocument`
  / `TabularReview` gain an optional `domain` field. New
  `DomainSelect` + `DomainFilter` shared components,
  i18n-backed via the `Domains.values.*` namespace (it/en/fr, 9
  labels per locale). All 14 built-in workflows tagged
  `domain: "legal"`. Domain filter chip in the Workflows list page;
  domain dropdown in the new-workflow modal (above the practice
  chips).
- **Per-user default domain** (migration 0019) —
  `user_settings.default_domain TEXT` lets the user pick a
  preferred vertical once in Account → Generali (next to the
  language switcher). The `NewWorkflowModal`, `AddNewTRModal`,
  `AddColumnModal` and `WFEditColumnModal` all pre-select this
  default on every open instead of hard-coding `legal`. Persisted
  via `PUT /user/default-domain`; cached in `UserProfileContext`.

### Added — JSON-driven workflow & column-preset registries (late afternoon)

- **System-shipped workflow templates moved out of TypeScript** —
  the 14 built-in workflows that used to live as a 1244-line
  `BUILT_IN_WORKFLOWS` constant in
  `frontend/.../builtinWorkflows.ts` are now per-file JSONs under
  `config/workflow-presets/<domain>/<slug>.json`. The backend
  loads them once at startup into `AppState.workflow_presets`
  (mirror of the corpus-plugin pattern: ancestor-walking directory
  resolution, fail-soft parsing, override via
  `MIKE_WORKFLOW_PRESETS_DIR`). The `/workflow` list endpoint
  merges them with user-owned DB rows at response time
  (`is_system: true`, `user_id: null` for presets). The
  `/workflow/:id` handler short-circuits the lookup to the in-memory
  registry before falling through to the DB so the detail page
  works for built-ins too. Built-ins remain immutable from the API:
  update/delete handlers keep their `WHERE user_id = ?` filter, so
  a row with `user_id NULL` never matches.
- **Column-preset registry** — same pattern. The 13 column
  shortcuts that lived as a `PROMPT_PRESETS` array in
  `columnPresets.ts` move to `config/column-presets/<domain>/*.json`
  and serve over a new `/column-presets` endpoint. Regex pattern
  is split into `match_pattern` + `match_flags` so the JSON is
  portable; the frontend rebuilds `new RegExp(...)` at use time in
  `AddColumnModal` / `WFEditColumnModal`. Both consumers fetch on
  open and scope the auto-match (column-name → preset) to the
  active domain so a legal preset doesn't fire when the user is
  filling an insurance review.
- **`config/` consolidation** — three preset families that used to
  sit at the repo root (`corpora-plugins/`, `workflow-presets/`,
  `column-presets/`) now live together under `config/`. The
  ancestor-walking loaders look for `config/<dir-name>/` instead
  of `<dir-name>/`. Repo root stays tidy, future config families
  inherit the convention.
- **Picker modals — on-the-fly domain switch** — `AssistantWorkflowModal`
  (chat composer's "Aggiungi workflow"), `AddNewTRModal` (tabular
  template picker), `AddColumnModal` and `WFEditColumnModal`
  (column presets popover) each render a `DomainSelect` inline
  (full 9-domain set, sempre visibile). Selection persists during
  the modal session, resets to the user's saved default on every
  next open. Empty-slice case is allowed (no entries shown) so
  the user can scout other verticals freely.

### Added — insurance vertical (late afternoon)

First non-legal vertical shipped with real, useful content (driven
by the user's `docs/insurance-workflows-plan.md`):

- **3 tabular comparison workflows** — `RC Professionale Review`,
  `RC Prodotti Review`, `D&O Review`. Each has **24 columns**: 16
  common (Blocco A — assicuratore, contraente, decorrenza/scadenza,
  premio, massimali, franchigia/SIR, territorialità, condizioni di
  rinnovo, recesso infrannuale, liquidazione sinistri, subrogazione,
  legge applicabile, foro/ADR) plus 8 type-specific (RCP: trigger
  di copertura, retroattività, ultrattività, copertura sanzioni
  disciplinari, …; RCD: product recall, completed operations,
  copertura USA/Canada, …; D&O: Side A/B/C, esclusione condotte
  fraudolente con final-adjudication clause, …). Column prompts
  in Italian, style guard: cite article, "Non previsto" fallback,
  ISO dates, currency always specified.
- **3 assistant-type workflows** for the chat composer —
  `Riassunto copertura polizza` (single-policy 1-page Markdown
  brief in 8 sections), `Due Diligence assicurativa`
  (portfolio-level M&A review with inventory table, per-policy
  analysis, gap list prioritised 🔴/🟡/🟢, suggested
  rep&warranties for the SPA), `Inventario beni assicurati`
  (Property/All-Risks asset extraction → 9-column Markdown table
  with category, descrizione, identificativo, ubicazione, valore,
  criterio valutazione, sottolimite, note, plus a "Totale
  assicurato" sum check and red-flag section).
- **17 column-presets** under `config/column-presets/insurance/` —
  the 16 Blocco-A columns extracted as shortcuts plus "Esclusioni
  principali" (recurring across all three specific blocks). Regex
  patterns accept Italian and English variants
  (`insurer`/`assicuratore`, `deductible`/`franchigia`, …) so
  international users get the suggestion when typing in either
  language.

### Changed (late afternoon)

- Workflow list "Origine" column renamed `Mike` → `MikeRust` for
  system-shipped rows. The two sibling labels `Myself` / `Shared`
  that were also hardcoded got i18n'd to
  `Workflows.originSelf` / `originShared` in it/en/fr.

### Removed (late afternoon)

- `frontend/src/app/components/workflows/builtinWorkflows.ts`
  (1244 lines) and `frontend/src/app/components/tabular/columnPresets.ts`
  (104 lines) — single source of truth is now the JSON files on
  disk under `config/`.

### Security — defensive cleanup around the runtime DB (late evening)

Forensic verification of the GitHub remote (every branch, every
historical blob, `git log -p -S "AIzaSy"` over all refs) confirmed
**no `.db` file or API-key fragment has ever been pushed** — the
runtime DB lives outside the project tree by design
(`<user-home>/mikerust-data/mike.db`) and `.gitignore` had already
been excluding `*.db` and `data/`. Three defensive tidy-ups landed
anyway so the safeguard is explicit and survives future refactors:

- **Legacy `data/mike.db` removed from the working tree.** The stub
  was a leftover from the pre-MikeRust upstream layout (only migration
  0001 applied, no user data), but a SQLite file sitting in the
  project root is a `git add .` accident waiting to happen.
- **`.gitignore` annotated with a "DO NOT REMOVE" rationale** above
  the DB exclusion block, calling out that the runtime DB stores user
  API keys in plaintext and that `*.db` / `*.sqlite` / `data/` are
  the primary defence against accidental commits.
- **`.env.example` no longer suggests an in-repo DB path.** The
  `DATABASE_URL=sqlite://mike.db` line that shipped from upstream
  Mike would have landed the DB next to the executable (i.e. inside
  the project tree if run from the repo root). Now the variable is
  commented out so the code's secure default
  (`<user-home>/mikerust-data/mike.db`) wins by default; same
  treatment for `STORAGE_PATH`.

### Added — German, Spanish, Portuguese locales (evening)

- **`de.json`, `es.json`, `pt.json`** — full UI catalogues translated
  from the English source (767 lines each, every key present, no
  missing or extra strings). Lawyer-appropriate formal register: "Sie"
  in German, "usted" in Spanish, European Portuguese with formal
  pronouns. Practice-area labels and professional-domain values
  translated to natural local conventions (e.g. "Corporate" →
  "Gesellschaftsrecht" / "Mercantil" / "Direito Societário").
- **English fallback chain** — `src/i18n/request.ts` now deep-merges
  the active locale onto the English catalogue, so any string that
  hasn't been translated in a non-English locale silently falls back
  to English instead of rendering the literal key. Makes future
  catalogue additions safe: drop a new key into `en.json` and the UI
  works in all six locales immediately while translations catch up.
- **`/user/locale` whitelist** expanded to accept `de` / `es` / `pt`
  alongside the existing `it` / `en` / `fr`.
- **LanguageSwitcher** redesigned as a Radix dropdown with inline SVG
  flags for each locale. Emoji flags fall back to the bare regional
  indicator letters on Windows (no built-in flag font), so the
  component ships its own minimal hand-rolled SVGs (Italy, simplified
  Union Jack, France, Germany, Spain, Portugal) that render identically
  on every supported OS.

### Changed — Account profile layout (evening)

- All four Profile fields (Nome utente, Nome visualizzato, Lingua,
  Settore) now share a single layout pattern: block label above + flex
  row with the control at `flex-1` and a phantom 80px slot mirroring
  the Save button used by Nome visualizzato. Result: all controls end
  at the same x-coordinate inside the `max-w-md` container, so the
  panel looks ordered even though only Nome visualizzato actually
  carries a Save action.

### Fixed — chat model picker (evening)

- The chat ModelToggle was surfacing Anthropic models even when the
  user had no Claude API key saved, because the custom-model branch
  fired on the saved `claudeModel` field unconditionally. Same picker
  also fell back to the full preset list when `profile.llm` hadn't
  loaded yet. Both gated now — the picker only shows providers with a
  saved API key, mirroring the gating already applied to the Settings
  page provider toggle.

### Added — LLM model catalogue (evening)

- **`config/model.json`** — single source of truth for the LLM
  provider/model/region catalogue. Four providers (Anthropic, Google
  Gemini, OpenAI, Mistral) with their model lists, capability flags
  (`supports_vision`, `supports_tools`, `supports_prompt_cache`, …),
  and Gemini's 30-region matrix (11 Europe, 6 US, 6 Asia, plus
  Canada, South America, Oceania and the Middle East). Preview models
  carry a `preview: true` flag so consumers can force `region=global`
  automatically (preview deployments are global-only by spec across
  all three vendors).
- **`src/presets/model.rs` + `GET /models`** — typed loader and
  read-only route serving the catalogue. Same fail-soft policy as the
  workflow / column-preset registries: a missing or malformed
  `model.json` logs a warning and falls back to an empty catalogue
  rather than blocking startup.
- **Settings → Modelli LLM rewrite** — text inputs for model and
  region replaced with catalogue-driven `<select>` dropdowns. Picking
  a preview Gemini model auto-snaps the region back to "global" and
  disables the region selector. A "Custom" option preserves any
  pre-existing model id that isn't in the catalogue so configuration
  isn't silently lost on a catalogue refresh.
- **Gated active-provider toggle** — the four provider buttons in the
  "Provider attivo" group are now disabled (greyed out, lock icon,
  tooltip explaining why) for providers that don't have an API key
  saved. The local provider stays enabled as long as the base URL is
  configured. Clearing a saved key for the currently-active provider
  automatically falls back to the local provider so the chat picker
  doesn't try to use a credentialless cloud endpoint on the next turn.

### Added — branding & docs

- **`NOTICE.md`** — trademark policy alongside AGPL-3.0: the Semplifica
  wordmark, corporate name, and logo are reserved trademarks of
  Semplifica s.r.l. Forks with substantive changes are asked to drop the
  marks and rename the binary. Same pattern as GitLab CE, Mastodon,
  Nextcloud, Element, Plausible ([`0709f6d`](#)).
- **README rewrite** of the "Authoritative legal corpora" section:
  reframed around the stated intent of a JSON-manifest plugin system,
  with an explicit trade-off table comparing it against an MCP-backend
  alternative that is under evaluation for connectors the manifest
  cannot express declaratively (PISTE OAuth, multi-step approval flows,
  etc.). Maintainer attribution to Semplifica s.r.l. + a collaboration
  invite (GitHub issues / PRs / email) added near the top
  ([`42ad45f`](#)).
- **`docs/CORPUS_PLUGINS.md`** — full schema reference and "how to add
  a corpus" walk-through ([`f777405`](#)).
- **`docs/UPSTREAM_SYNC.md`** — policy + audit log for the periodic
  upstream-sync passes against `willchen96/mike` ([`74eab2b`](#),
  [`c94199e`](#), [`606cd50`](#)).
- **`docs/WORKFLOWS.md`** — end-user manual for Workflows, Tabular
  Reviews, and Assistant injection. Twelve sections covering the
  mental model, column-format reference (9 formats), prompt-writing
  guide, step-by-step authoring flow, and 7 worked examples *outside*
  the legal domain (medical records review, M&A IC memo, real-estate
  inspection, HR resume screening, insurance claims triage, patent
  landscape, compliance audit register). Built-ins remain legal-focused
  but the framework is domain-agnostic.
- **`HISTORY.md`** — release-notes index introduced; this very file.

### Changed

- Sidebar user-pill dropdown now carries a Semplifica brand link to
  `https://semplifica.ai`; the link opens the user's OS default browser
  via Tauri's `open_external_url` command (avoids navigating the
  in-app WebView). Logo image dropped per design feedback; the asset
  stays under `frontend/public/semplifica/` for future use
  ([`e94c178`](#), [`0709f6d`](#)).
- Bulk-import progress bar no longer renders 100% green when the
  snapshot is already current (backend emits `phase=done` /
  `total=0`); the green text line stays but the misleading "26K docs
  indexed now" bar is hidden ([`f78c8bb`](#)).

### Fixed

- `ProjectsOverview` fetch is now gated on auth state and gracefully
  surfaces load errors instead of throwing ([`e9f4f4a`](#)).
- `generic_search` short-circuits to `dila-bulk-xml` *before* the
  `corpus_adapters` registry lookup, so DILA-backed corpora actually
  serve queries instead of 501-ing because they have no `ManifestAdapter`
  ([`812638f`](#)).
- CNIL routing heuristic: prefer search-first endpoints over deep-link
  identifier fetches (CNIL/Légifrance HTML lookups returned anti-bot
  pages on direct URN access); detector now scans Cloudflare / AWS WAF
  / Akamai signatures and surfaces a descriptive error
  ([`3a59276`](#), [`cfc38b9`](#)).
- DILA `tar::Archive` walks are wrapped in `tokio::task::spawn_blocking`
  so the iterator's non-`Send` state doesn't trip an `.await` in the
  importer ([`c388beb`](#)).

### Removed

- `frontend/src/app/components/projects/ProjectPage.tsx` shrunk by ~600
  lines as layout helpers, `DocVersionHistory`, the skeleton and the
  Assistant / Reviews tabs moved to their own files (see Refactor above).

---

## 2026-05-09 — EUR-Lex robustness, MCP async hardening, UX docs

Stabilisation day on top of the initial bootstrap: the EUR-Lex
connector learns to detect partial responses, the MCP client gets a
visible "waiting on server" indicator for slow human-in-the-loop tools,
and the README gets its first screenshot pair.

### Added — MCP

- **Auto-chain `request_*` → `get_*`** for the Edge / Semplifica.Edge
  async pattern: when an MCP tool returns `{status: "pending",
  session_id: ...}`, the dispatcher polls the matching `get_*` tool
  until completion or timeout, transparently to the model
  ([`2f83a7a`](#)).
- **5-minute dispatch timeout** for MCP tool calls (was the
  Tower default), configurable via `MCP_CALL_TIMEOUT_SECS`. Short
  result bodies are logged at info level for diagnosis ([`a6b78d5`](#)).
- **Per-user MCP discovery cache** so the tool catalogue isn't
  re-fetched on every chat message; cache invalidates on MCP-config
  change. Silent tool-injection failures now log the exact reason
  ([`5d526f8`](#)).
- **Phase-by-phase dispatch logging** + SSE heartbeats so the chat
  channel doesn't look dead during a long MCP roundtrip
  ([`813a8e4`](#)).
- **Live wait indicator** in the chat composer — when a tool call is in
  flight on the MCP server side (e.g. a human has to click "Approve" in
  Edge's UI), the assistant message shows a pulsing "waiting" affordance
  with a hint that the result will arrive as soon as the server releases
  it ([`72e1f21`](#)).

### Added — EUR-Lex

- **Cellar REST fallback** as a fourth path after the public HTML / SOAP
  / SPARQL routes. Activates when the prior three return AWS WAF
  challenge markers ([`d9408d1`](#)).
- **Phantom-row guard** — refuse to persist a fetched document under
  1 KB. The page often comes back as a stub when EUR-Lex's CDN gets
  testy, and a 0-chunk row was getting marked `ready` ([`b6099bd`](#),
  [`c691e57`](#)).
- **Retry-with-backoff** in the fetch path; threshold raised from 400 to
  2 000 characters so genuinely short acts (one-page decisions) still
  pass but actual stubs are caught.

### Added — citations / viewer

- Numeric citation pills (`[1]`, `[2]`) and KB tags (`[gN]` / `[pN]`)
  now open EUR-Lex / corpus-document sources in the side panel with
  the cited quote highlighted on the page (previously only chat-
  attached PDFs worked) ([`3da46f0`](#)).

### Added — docs

- README gains the chat-with-citations and EUR-Lex-sync screenshot pair
  ([`6de8073`](#)) and a frank note that MCP async multi-step flows are
  partial ([`aa174da`](#)).
- New `docs/` diagrams: local-MCP-gatekeeper flow and the MikeRust
  architectural-stack overview ([`d093369`](#), [`615a4dc`](#),
  [`01dd8cc`](#)).

### Fixed

- `ChatView.upsertTab` assigned the same id to multiple tabs on rapid
  open / close cycles — fixed by deriving the id from a monotonic
  counter ([`82fa2af`](#)).
- `isDocxFilename` no longer panics on `null` / `undefined` input
  ([`10f8a74`](#)).
- `.taurignore` widened so editing docs or i18n catalogues doesn't
  trigger a Tauri dev-binary restart ([`56d832d`](#)).

---

## 2026-05-08 — Initial release: MikeRust fork from `willchen96/mike`

The starting point. MikeRust forks `willchen96/mike` (an AGPL-3.0
TypeScript / Express / Supabase / S3 / LibreOffice cloud-native AI
legal assistant), keeps the Next.js frontend largely intact, and
replaces the backend with a Rust + axum implementation designed to run
**entirely on the user's machine**.

### Added — sovereign backend

- **Rust + axum** API on `:3001`, single-binary Tauri shell wrapping the
  Next.js frontend on `:3000`.
- **SQLite** via `sqlite-vec` for vector storage (no Supabase, no
  pgvector). Schema covers users, chats, projects, documents,
  tabular reviews, workflows, MCP servers, settings.
- **Local filesystem storage** — `data/db/mike.db` + `data/storage/`
  for uploads and chat cache. Optional S3 trait scaffolded for later.
- **Auth** — PIN with Argon2id, opaque sessions, Windows Hello biometric
  unlock path.
- **`.mikeprj` envelope** — AES-256-GCM-encrypted project bundles with
  Argon2id-derived key, keyed to a recipient email so a colleague can
  import a project the sender explicitly shared with them.

### Added — local document pipeline

- **PDF** extraction via `pdfium-render` with per-page text and scanned-
  PDF detection.
- **DOCX** extraction in pure Rust (ZIP + XML) with tracked-deletion
  (`<w:del>`) and strike-through detection, surfaced inline as
  `[removed by author: …]` markers (see `docs/DOCX.md`).
- **RTF** via `rtf-parser`; **XLSX / XLS / XLSB / ODS** via `calamine`;
  **TXT / MD / CSV** as UTF-8 lossy decode.
- **ONNX embeddings** with `multilingual-e5-base` (768 dims) via
  `fastembed` + `ort`. Hardware acceleration is opt-in: `rag-directml`
  for Windows GPU, `rag-qnn` for Qualcomm NPU. Service tries
  QNN → DirectML → CPU, silently skipping providers whose DLLs aren't
  loadable.
- **Chunker + scanner** — recursive folder walk with `.gitignore`-style
  patterns, ~800-token chunks with 200-token overlap, embeddings stored
  in `sqlite-vec` virtual tables partitioned by
  `(user_id, project_id_or_global)` for cross-tenant isolation.
- **Hash-keyed cache** for chat attachments: documents land under
  `data/storage/cache/<sha256>.<ext>` with a pre-extracted
  `<sha256>.txt`. Multiple `documents` rows can reference the same
  hash, and ref-counted cleanup deletes the on-disk pair only when no
  doc still references it.

### Added — LLMs and tools

- **LLM providers**: Anthropic, Gemini, OpenAI, vLLM, Ollama. API keys
  stored locally in `data/db/mike.db`, never sent anywhere except the
  provider the user explicitly configured.
- **MCP client** (HTTP / SSE) — synchronous tool calls dispatch end-to-
  end; configured servers come from `MCP_SERVERS` env or the in-app
  MCP settings page.
- **MCP tool-schema injection** for tool-capable models so the assistant
  knows which tools are available without re-fetching per turn
  ([`c043a3b`](#)).

### Added — authoritative corpus framework

- **`LegalCorpusAdapter` trait** + builtin EUR-Lex V1 connector
  (CELEX-based fetch via public HTML, 24-language picker, English
  fallback when a doc isn't published in the requested language).
- **Italian Legal Corpus V1** — Normattiva (~69 K acts) + Corte
  Costituzionale (~22 K decisions) via the HuggingFace
  `datasets-server` `/rows` endpoint with Parquet bulk for the offline
  snapshot.

### Added — frontend

- Forked Mike's Next.js UI: chat with citation pills, document viewer
  with PDF.js, tabular-review workflow editor, projects + workflows +
  members + sharing.
- Bilingual i18n via `next-intl` — Italian primary, English mirror.

### Notes

- License: AGPL-3.0 (inherited from `willchen96/mike`); backend code
  ships under the same licence for consistency.
- Telemetry: none. No remote logging, no anonymous metrics. Outbound
  traffic only when the user explicitly invokes a remote LLM or a
  remote MCP server they configured themselves.
