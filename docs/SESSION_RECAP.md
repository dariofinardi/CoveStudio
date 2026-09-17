# MikeRust — riepilogo sessione

> **Historical record.** This recap describes MikeRust as of 8 May 2026,
> when the project still used the Next.js frontend forked from Mike. That
> frontend was replaced by the Svelte application on 17 May 2026 and the
> project is now Cove Studio. For the current state see the README.

> Cronistoria tecnica di quanto sviluppato in questa sessione, organizzata per area
> con riferimenti puntuali ai file. Pensato come documento di "consegna" che
> chiunque (o io stesso al prossimo giro) possa leggere senza ricostruire la
> conversazione. Per le voci ancora aperte vedi [TODO.md](../TODO.md).

---

## 1. Internazionalizzazione (next-intl, IT/EN)

### Setup
- Installato `next-intl` ([package.json](../frontend/package.json)).
- Configurazione `withNextIntl` in [frontend/next.config.ts](../frontend/next.config.ts).
- Cataloghi: [messages/it.json](../frontend/messages/it.json) (default) e [messages/en.json](../frontend/messages/en.json).
- Wrapper `NextIntlClientProvider` in [frontend/src/app/layout.tsx](../frontend/src/app/layout.tsx) con `<html lang>` dinamico.
- Helper di config: [frontend/src/i18n/config.ts](../frontend/src/i18n/config.ts), [request.ts](../frontend/src/i18n/request.ts), [actions.ts](../frontend/src/i18n/actions.ts).

### Persistenza locale
- Cookie `mike_locale` letto da `getRequestConfig` (SSR).
- Persistito **lato backend** in `user_settings.locale` (migrazione `0005_user_locale.sql`) via `GET/PUT /user/locale`. Lo switcher [LanguageSwitcher.tsx](../frontend/src/app/components/shared/LanguageSwitcher.tsx) scrive sia il cookie sia la riga DB.

### Surfaces tradotte (high-impact)
Login, signup, account general/models/mcp/sync, sidebar, error/not-found, projects overview, project page, project export modal + import drag&drop, workflow list, tabular reviews list, AddNewTRModal, NewProjectModal, NewWorkflowModal, ChatInput, InitialView, ModelToggle, ApiKeyMissingModal, DeleteChatsModal, OwnerOnlyModal, CreditsExhaustedModal, AssistantMessage (thinking-phrases ruotanti, Reading/Found/Creating/Replicating/Editing/etc.), AssistantWorkflowModal, AddDocButton, EditCard, DocPanel headers, AssistantSidePanel, useAssistantChat (cancelText/genericError).

### Memoria persistente
- [feedback_ui_i18n.md](C:/Users/df/.claude/projects/c--Progetti-MikeRust/memory/feedback_ui_i18n.md): regola "ogni modifica UI deve includere chiavi i18n" salvata.

### Surfaces tradotte (medium/low-impact, batch successivo)
- [DisplayWorkflowModal.tsx](../frontend/src/app/components/workflows/DisplayWorkflowModal.tsx): Select project, No projects found, Workflow Prompt, Columns, No columns defined, Tags, Prompt, _No prompt defined._.
- [ShareWorkflowModal.tsx](../frontend/src/app/components/workflows/ShareWorkflowModal.tsx): breadcrumb People, Add by email, Allow editing, Read-only/Can edit, Share/Sharing.
- [WorkflowPromptEditor.tsx](../frontend/src/app/components/workflows/WorkflowPromptEditor.tsx): toolbar tooltips (Heading 1-3, Bold, Italic, Bullet/Numbered list).
- [WFEditColumnModal.tsx](../frontend/src/app/components/workflows/WFEditColumnModal.tsx) e [AddColumnModal.tsx](../frontend/src/app/components/tabular/AddColumnModal.tsx): Column name, Column presets, Add tag, prompt placeholder (key condivise sotto `WorkflowColumns`).
- [TREditColumnMenu.tsx](../frontend/src/app/components/tabular/TREditColumnMenu.tsx): Edit Column, Label, Format, Add tags…, Prompt, Auto-generate, Delete, Save/Saving.
- [TabularReviewView.tsx](../frontend/src/app/components/tabular/TabularReviewView.tsx): breadcrumb Projects/Tabular Reviews/Add Documents, Search documents…, People with access tooltip, Export to Excel/Export, Run/Running…, Assistant in Tabular Review, Actions, Clear results, Delete, Add Documents/Columns, Untitled Review fallback.
- [TRSidePanel.tsx](../frontend/src/app/components/tabular/TRSidePanel.tsx): Regenerate tooltip, Flag/Results/Reasoning headers, citation tooltip "Page N: …" parametrizzata.
- [TabularCell.tsx](../frontend/src/app/components/tabular/TabularCell.tsx): citation tooltip + See details.
- [TRTable.tsx](../frontend/src/app/components/tabular/TRTable.tsx): Document column header, empty state ("Tabular Review" + emptyHint), `+ Add Columns`, `Add Documents`.
- [TRChatPanel.tsx](../frontend/src/app/components/tabular/TRChatPanel.tsx): Ask a question…, Search chats, Chat history, New chat, Delete chat, Close.
- [PeopleModal.tsx](../frontend/src/app/components/shared/PeopleModal.tsx): Add by email…, Add member, Remove access, alreadyHasAccess/isTheOwner messaggi parametrizzati, Owner badge.
- [UploadNewVersionModal.tsx](../frontend/src/app/components/shared/UploadNewVersionModal.tsx) + [AddDocumentsModal.tsx](../frontend/src/app/components/shared/AddDocumentsModal.tsx) + [AddProjectDocsModal.tsx](../frontend/src/app/components/shared/AddProjectDocsModal.tsx): header con filename param, version name placeholder, Cancel/Save/Saving, search placeholder.
- [SelectAssistantProjectModal.tsx](../frontend/src/app/components/assistant/SelectAssistantProjectModal.tsx): breadcrumb (Assistant > Start Chat in a Project), Cancel, Continue.
- [ProjectExplorer.tsx](../frontend/src/app/components/projects/ProjectExplorer.tsx): "Folder name" placeholder.

### Nuovi namespace e key
- **Tabular** (nuovo): `flag`, `results`, `reasoning`, `citationTitle` (parametrizzato `{page}/{quote}`), `document`, `tabularReview`, `emptyHint`, `addColumns`, `addDocuments`, `seeDetails`.
- **WorkflowColumns** (esteso): `editColumn`, `label`, `format`, `addTagsPlural`, `autoGenerate`.
- **TabularReviews** (esteso): `searchDocsPlaceholder`, `peopleWithAccess`, `exportToExcel`, `export`, `run`, `assistantInReview`, `actions`, `clearResults`, `addDocuments`, `addColumns`, `breadcrumbAddDocuments`.
- **Projects** (esteso): `folderName`.
- **Assistant**: array `thinkingPhrases.0..4` (5 frasi rotanti) + `regenerate`, `closePanel`.

### Aperti
- Pluralizzazione corretta sui counter ("1 file" vs "5 file") via ICU MessageFormat di next-intl — non ancora applicata.
- `RenameableTitle.tsx` non ha stringhe utente proprie: nessun intervento richiesto.

---

## 2. Storage / data residency

### Spostamento dati fuori dal repo
Il rebuild-loop di Tauri dev era causato da SQLite WAL/SHM scritti dentro `src-tauri/`. Sistemato:
- `.env` aggiornato con `DATABASE_URL=sqlite:C:/Users/df/mikerust-data/mike.db` e `STORAGE_PATH=C:/Users/df/mikerust-data/storage` (forward slashes obbligatori per sqlx URI).
- Fallback nel codice: [src/db/mod.rs](../src/db/mod.rs) `default_db_url()` → `%USERPROFILE%/mikerust-data/mike.db`. Crea la cartella parent se manca.
- `load_dotenv()` in [src/lib.rs](../src/lib.rs) cammina dalla cwd e dal path dell'eseguibile fino a trovare `.env`. Prima `dotenvy::dotenv()` falliva silenziosamente perché la cwd di `mike-tauri.exe` è `src-tauri/`.
- [.taurignore](../.taurignore) al workspace root: ignora `frontend/.next/`, `**/*.log`, `**/*.db*`, `data/`, `target/`. Risolve il loop su `next-development.log`.
- Migrazione iniziale dei dati esistenti da `src-tauri/mike.db*` e `data/mike.db` al nuovo path.

### Memoria persistente
- [feedback_storage_policy.md](C:/Users/df/.claude/projects/c--Progetti-MikeRust/memory/feedback_storage_policy.md): preferire backend SQLite a localStorage per portabilità + sicurezza.

### Migrazione localStorage → backend
- API keys: cache localStorage rimosso. La pagina [account/models/page.tsx](../frontend/src/app/(pages)/account/models/page.tsx) usa input *uncontrolled* (DOM-only), mostra chip verde "Chiave salvata" senza esporre il valore in React state.
- Modello selezionato: persistito in `user_settings.title_model` via `UserProfileContext.setSelectedModel`. Hook [useSelectedModel.ts](../frontend/src/app/hooks/useSelectedModel.ts) riscritto.
- Region Gemini: migrazione `0006_user_gemini_region.sql`.
- Modello Gemini: migrazione `0007_user_gemini_model.sql`.

---

## 3. Auth & sessioni

### Display name persistente
- Frontend [UserProfileContext.tsx](../frontend/src/contexts/UserProfileContext.tsx) `updateDisplayName` ora chiama davvero `PUT /user/profile` (prima aggiornava solo lo stato in memoria → perdita al riavvio).
- Backend [src/routes/auth.rs](../src/routes/auth.rs) `/auth/status`, `/auth/unlock`, `/auth/unlock-biometric` aggiornati per restituire `display_name`.

### Token 401 globale
- [mikeApi.ts](../frontend/src/app/lib/mikeApi.ts) `apiRequest` su 401 → pulisce `mike_auth_token`/`mike_auth_user` da localStorage e redirect a `/login`. Risolve il caso "token orfano" da DB precedenti.
- Stesso comportamento in [account/models/page.tsx](../frontend/src/app/(pages)/account/models/page.tsx) `saveSettings`.

### Patch semantics su user_settings
[src/routes/user.rs](../src/routes/user.rs) `update_llm_settings` ora usa `INSERT OR IGNORE` + `UPDATE … SET col = COALESCE(?, col)`. Permette di salvare un singolo campo (es. region) senza riscrivere la API key.

---

## 4. Modelli LLM e Gemini

### ModelToggle
[frontend/src/app/components/assistant/ModelToggle.tsx](../frontend/src/app/components/assistant/ModelToggle.tsx):
- Aggiunti **Gemini 2.5 Pro** + **Gemini 2.5 Flash** stable.
- Marcate `gemini-3.1-pro-preview` e `gemini-3-flash-preview` come `(preview)`.
- Default: `gemini-2.5-flash`.
- Helper `isGlobalOnlyGeminiModel(id)` per la UI delle regioni.

### Region picker
- Selettore regione in [account/models/page.tsx](../frontend/src/app/(pages)/account/models/page.tsx) (Globale + 8 regioni IT/EU/US/Asia, incluso Milano `europe-west8`).
- Backend [src/llm/gemini.rs](../src/llm/gemini.rs) `base_url_with(model, region, suffix)`: la Generative Language API è solo globale (l'endpoint `<region>-generativelanguage.googleapis.com` non esiste). La regione viene memorizzata come **preferenza per la futura integrazione Vertex AI** ma le chiamate vanno sempre a `generativelanguage.googleapis.com`. Hint UI aggiornato a riflettere onestamente il vincolo.
- Region propagata in `StreamParams.gemini_region` end-to-end.

---

## 5. Bug fix chat / drip loop

- [useAssistantChat.ts](../frontend/src/app/hooks/useAssistantChat.ts) tick del drip ora avvolto in `startTransition()` (priorità interrompibile). Disinnesca il guard "Maximum update depth exceeded" che si attivava quando il drip a 60 fps superava la velocità di commit di React.
- Cleanup useEffect non aborta più il fetch (StrictMode causava cancellazioni spurie con messaggio "Cancelled by user" automatico).

---

## 6. RAG end-to-end

### Architettura concordata: 3 livelli di scope

```
TIER 1 — global pool      (synced_files.project_id IS NULL)
TIER 2 — project pool     (synced_files.project_id = <X>)
   - isolation_mode='shared'  → chat vede globale + proprio (default)
   - isolation_mode='strict'  → chat vede solo proprio
TIER 3 — attached         (paperclip per turn, full-text in system prompt)
```

### Backend
- **Vector store**: sqlite-vec (NON Lance — protoc-free; stesso `mike.db` per tutto).
- **Embedder**: `multilingual-e5-base` via `fastembed` (download HF + cache su prima invocazione).
- **Chunker**: [src/embeddings/chunker.rs](../src/embeddings/chunker.rs) — paragrafi → frasi → hard-cut con overlap. Abbreviazioni IT/EN (art./n./avv./i.e./e.g.) non rompono la frase. Test inclusi.
- **Service**: [src/embeddings/service.rs](../src/embeddings/service.rs) — `EmbeddingService` con `SearchScope::{Global, ProjectShared(pid), ProjectStrict(pid)}`. Prefissi `passage:`/`query:` corretti per E5. `index_document` idempotente (re-embed sostituisce). `vec_to_blob` per serializzare f32 little-endian.
- **Migrazione**: [migrations/0009_rag_scopes.sql](../migrations/0009_rag_scopes.sql) — `project_id` su `sync_folders`/`synced_files`, `projects.isolation_mode TEXT NOT NULL DEFAULT 'shared'`, virtual table `doc_chunks USING vec0(embedding float[768], +metadata...)`.
- **Bootstrap extension**: [src/db/mod.rs](../src/db/mod.rs) registra `sqlite3_auto_extension(sqlite3_vec_init)` via `libsqlite3-sys` PRIMA che sqlx apra la pool. Una sola volta per processo (`std::sync::Once`).
- **Folder scanner**: [src/sync/scanner.rs](../src/sync/scanner.rs) — `walkdir` + `ignore` (rispetta `.gitignore` + `.mikesyncignore`), sha256 idempotente, dispatch verso estrattori esistenti. Skip espliciti per scanned-PDF e formati non testuali con `skip_reason` mostrato in UI.
- **Routes**: [src/routes/sync.rs](../src/routes/sync.rs) — `GET /sync/folders`, `POST /sync/folders` (con `project_id` opzionale), `DELETE /sync/folders/{id}`, `POST /sync/folders/{id}/scan`, `GET /sync/folders/{id}/status`, `GET /sync/folders/{id}/files`, `GET /sync/kb-doc?path=...` (con allowlist sui path indicizzati).
- **Project isolation**: `PUT /project/{id}` accetta `isolation_mode: 'shared'|'strict'`, `GET` lo restituisce ([src/routes/projects.rs](../src/routes/projects.rs)).
- **Hook chat**: [src/routes/chat.rs](../src/routes/chat.rs) `retrieve_kb_chunks()` invocato in `tokio::join!` parallelamente a `load_attached_docs`/`discover_mcp_for_user`. Soglia distance `≤ 0.6`. Embed solo dell'**ultimo** user message (no history). `build_kb_system_prompt` rende il blocco `<KNOWLEDGE BASE>` con istruzioni al modello (è excerpt, non full doc, citalo come `[g1]` con `source:kb`).
- **Citation parser esteso**: model emette `[g1]`/`[p1]` → entry citation finale ha `source: "kb"`, `scope`, `path`, `chunk_index`, `document_id`, `filename`.

### Frontend
- **Pannello "Sincronizzazione locale"**: [account/sync/page.tsx](../frontend/src/app/(pages)/account/sync/page.tsx) — form aggiunta cartella (path + label + ricorsivo + dropdown scope Globale/Progetto), lista cartelle con badge scope, scan button, **barra di progresso live** (poll 1.5 s solo quando una scansione è in `running`), drill-down file con stato (Indicizzato/Ignorato/Errore) + motivo skip + numero chunk.
- **Toggle isolation_mode** in header progetto: icona `Lock`/`Share2` (owner-only) accanto a People button.
- **Pillole citation differenziate**: [AssistantMessage.tsx](../frontend/src/app/components/assistant/AssistantMessage.tsx) — verde per globali, blu per project, grigio per attached. Tooltip mostra scope + filename + chunk index.
- **DocPanel kb integrato**: catena completa kbPath → useFetchDocxBytes/useFetchSingleDoc/DocxView/DocView/DocPanel/AssistantSidePanelTab. Click su `[g1]` apre nella stessa side-tab degli attached, con highlight dei passaggi citati e download via `/sync/kb-doc?path=...`.

### Prestazioni (CPU-only)
| Operazione | Tempo |
|-----------|-------|
| 1 query embedding | 50–150 ms |
| Batch 10 chunk index | 200–400 ms |
| Brute-force search 10k vettori | 5–10 ms |
| Brute-force search 100k vettori | 50–100 ms |

E5-base FP32: ~280 MB RAM. INT8 disponibile in fastembed se serve.

---

## 7. Conversation summarization buffer

[src/llm/summarize.rs](../src/llm/summarize.rs):
- `estimate_tokens(text)`: euristica char/4.
- `context_window_tokens(model)`: tabella per Claude 4.6/4.7 (1M), Gemini 2.5 Pro (2M), Gemini 2.5/3 Flash (1M), GPT-4o (128k), local default (8k).
- `should_summarize`: trigger quando `tokens(history) > 0.7 × window` E ci sono almeno KEEP_RECENT_TURNS×2+2 messaggi.
- `summarize_old_turns`: mantiene gli ultimi 4 turn user/assistant verbatim, comprime il resto in **un singolo messaggio system** "EARLIER CONVERSATION SUMMARY". La chiamata di compressione riusa lo stesso modello dell'utente con prompt italiano focalizzato su nomi/date/decisioni/aperti.
- `maybe_compress_history`: failing-open — errori non bloccano la chat, fallback alla history originale.

[src/llm/types.rs](../src/llm/types.rs): aggiunti `Message::system()` e `Message::assistant()`.

Hook in [src/routes/chat.rs](../src/routes/chat.rs) subito prima del `tokio::spawn` SSE.

---

## 8. Project export/import (`.mikeprj`)

### Crypto envelope
[src/mikeprj/crypto.rs](../src/mikeprj/crypto.rs):
- Header binario: magic `MIKEPRJ\0` + version + flags + SHA-256(email) + salt + nonce.
- Payload: AES-256-GCM con chiave derivata via Argon2id da `normalize_email(recipient_email)` + salt random.
- `seal()` / `open()` con verifica fingerprint prima del decrypt.
- Sharing model "weak email pinning": chiunque conosca l'email può aprire il file. Documentato come limitazione intenzionale; futuro v2 con OTP via canale fuori-banda.

### Schema versionato
[src/mikeprj/manifest.rs](../src/mikeprj/manifest.rs): `Manifest`, `ProjectRecord`, `DocumentRecord`, `TabularReviewRecord`, `WorkflowRecord`, `ChatRecord`. `SCHEMA_VERSION = 1`.

### Build/parse
[src/mikeprj/io.rs](../src/mikeprj/io.rs): `build_payload()` legge dalla DB + storage; `zip_payload()` serializza ZIP (deflate + README); `unzip_payload()` parsea.

### Endpoints
[src/routes/projects.rs](../src/routes/projects.rs):
- `POST /project/{id}/export` body `{ recipient_email, include_chats? }` → binary `.mikeprj` con `Content-Disposition` corretto. `sanitize_filename` per il nome del progetto.
- `POST /project/import` multipart `file` + `recipient_email` → `crypto::open` valida, `unzip_payload` scompatta, crea project con UUID nuovo, copia documenti in storage, ricrea reviews/workflows/chats con UUID nuovi.

### UI
- [ProjectExportModal.tsx](../frontend/src/app/components/projects/ProjectExportModal.tsx): icona `Package` nel header progetto (owner-only) → modal con email + checkbox "Includi chat" + download blob URL.
- Drag&drop in [ProjectsOverview.tsx](../frontend/src/app/components/projects/ProjectsOverview.tsx): overlay full-page su drag, dialog di conferma con email proposta = email utente loggato, gestione "wrong email" tradotta, redirect a `/projects/{new_id}` su successo.

### Decisioni concordate
- Chat: opt-in (default OFF).
- Tabular review: solo configurazione (no celle).
- Cifratura: ON di default con email pinning.
- Scope: tabular review configurazione + workflow custom + documents bytes; built-in workflow esclusi (matching id).

---

## 9. TODO architettura — Document loader pluggable

Annotato in [TODO.md](../TODO.md) come refactor da pianificare:

```rust
pub trait DocumentLoader: Send + Sync {
    fn name(&self) -> &'static str;
    fn supports(&self, ext: &str, mime: Option<&str>) -> bool;
    fn extract_text(&self, bytes: &[u8], path: Option<&Path>) -> Result<ExtractResult>;
    fn supports_vision(&self) -> bool { false }
    fn render_pages_as_images(&self, _bytes: &[u8]) -> Result<Vec<Vec<u8>>> { ... }
}
```

Vantaggio: `chat.rs::load_attached_docs` e `sync/scanner.rs::extract_text_dispatch` (oggi due `match` paralleli su estensione) si unificano. Nuovi formati (`eml`, `pptx`, `rtf`, `epub`, `html`) = un file in `document_loader/`. **Da fare DOPO** mikeprj/i18n perché ha alto blast radius.

---

## 10. Persistenza dati e memoria del progetto

### Migrazioni create
| # | File | Scopo |
|---|------|-------|
| 0005 | `0005_user_locale.sql` | `user_settings.locale` |
| 0006 | `0006_user_gemini_region.sql` | `user_settings.gemini_region` |
| 0007 | `0007_user_gemini_model.sql` | `user_settings.gemini_model` |
| 0008 | `0008_sync_folders.sql` | `sync_folders` + `synced_files` |
| 0009 | `0009_rag_scopes.sql` | `project_id` su sync, `projects.isolation_mode`, virtual `doc_chunks` (vec0) |

### Cargo deps aggiunte
- RAG (feature `rag`, default ON): `fastembed`, `sqlite-vec`, `libsqlite3-sys` (con `bundled`), `zerocopy`, `walkdir`, `ignore`.
- mikeprj: `aes-gcm`, `sha2` (già usato da migrazione del progetto export).

### Memoria utente persistente
1. [MEMORY.md](C:/Users/df/.claude/projects/c--Progetti-MikeRust/memory/MEMORY.md) (indice).
2. [feedback_ui_i18n.md](C:/Users/df/.claude/projects/c--Progetti-MikeRust/memory/feedback_ui_i18n.md) — ogni edit UI deve passare per i18n.
3. [feedback_storage_policy.md](C:/Users/df/.claude/projects/c--Progetti-MikeRust/memory/feedback_storage_policy.md) — preferire backend SQLite a localStorage.

---

## 11. Verifiche utente in attesa

> **Non testato dall'utente in questa sessione**, lasciato come prima azione del prossimo giro:

1. Smoke test pipeline RAG end-to-end:
   - Riavviare MikeRust → migrazioni `0005`–`0009` partono.
   - Account → "Sincronizzazione locale" → aggiungi cartella reale, scope Globale.
   - "Scansiona ora" → primo run scarica e5-base (~280 MB) da HuggingFace.
   - Verificare nei log backend `[rag] loading multilingual-e5-base` e `[rag] sqlite-vec auto-extension registered`.
   - Chat con query attinente → log `[chat] ... kb_chunks=N`.
   - Click su pillola `[g1]` → DocPanel apre il file con highlight.
2. Toggle isolation `shared`/`strict` su un progetto + verifica che chat strict non veda chunks globali.
3. Export `.mikeprj` con email collega → import su seconda istanza con stessa email → verificare che project, documents, reviews, workflow vengano replicati.

---

## 12. Quick reference — comandi e percorsi

```
# Build
cargo check                                    # full (rag feature)
cargo check --no-default-features \
            --features local-storage           # slim, no fastembed/sqlite-vec
npx tsc --noEmit                               # frontend types

# Dati persistenti
%USERPROFILE%/mikerust-data/mike.db            # SQLite (DB + vettori sqlite-vec)
%USERPROFILE%/mikerust-data/storage/           # bytes documenti
%USERPROFILE%/mikerust-logs/tauri-dev.log      # log dev

# Modello e5 cache (fastembed)
%LOCALAPPDATA%/fastembed/                      # ~280 MB dopo primo download
```

### Endpoint nuovi (riepilogo)
```
GET  /user/locale                              i18n persistence
PUT  /user/locale                              { locale: "it"|"en" }

GET  /sync/folders                             list configured folders
POST /sync/folders                             { path, label?, recursive?, project_id? }
DELETE /sync/folders/{id}
POST /sync/folders/{id}/scan                   kicks off background scan
GET  /sync/folders/{id}/status                 { status, total, processed, indexed, skipped, failed, current_file }
GET  /sync/folders/{id}/files                  drill-down on indexed/skipped
GET  /sync/kb-doc?path=...                     stream bytes of indexed file (allowlisted)

PUT  /project/{id}                             body now also accepts { isolation_mode: "shared"|"strict" }
POST /project/{id}/export                      { recipient_email, include_chats? } → binary .mikeprj
POST /project/import                           multipart { file, recipient_email } → { project_id }
```

---

_Ultima modifica: chiusura sessione i18n high-impact + recap doc._
