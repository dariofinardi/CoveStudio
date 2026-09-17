# Authoritative legal corpora

Plan and per-source survey for ingesting public legal sources (EU + national) into Cove Studio's RAG store, configurable per-corpus from **Impostazioni → Documenti locali / EUR-Lex / …**.

## Goals

1. Let the user opt-in to one or more authoritative corpora — they're large, so default-off.
2. Per-corpus controls:
   - **Enabled** toggle (powers sync on/off).
   - **Reference language** picker (multilingual corpora only — EUR-Lex). Falls back to English if a document isn't available in the chosen language.
   - **Search by law number / identifier** — e.g. "Regolamento UE 2016/679", "art. 1322 c.c.", "Loi n° 78-17", "BGB §433", "Lov nr. 502 af 23/05/2018".
   - **Search by keyword** — full-text search routed to the source's native search endpoint (not RAG).
3. Ingestion is opt-in *per document*: search returns hits, user picks which to add to their personal index. We don't bulk-mirror national gazettes — that's gigabytes and most of it is irrelevant to any one user.
4. Ingested documents land in the same `sqlite-vec` partition as folder-synced docs (scope = `global` by default), so retrieval treats them uniformly.

## Source survey

Ranked by ease of programmatic ingestion (best → worst).

### 1. Denmark — Retsinformation
- **Site**: https://www.retsinformation.dk
- **API**: documented JSON. `GET https://www.retsinformation.dk/api/document/{eli}` for a single act; `/api/search?q=…` for full-text. Used by the `marklok/danishmike` fork as reference.
- **Languages**: Danish only.
- **Search**: ELI / document number / full-text.
- **Formats**: HTML body, PDF, partial XML/ELI metadata.
- **License**: Danish public-sector information — free reuse.
- **Adapter shape**: thin REST client; map ELI → `documents` row + `doc_chunks` after extraction.

### 2. France — Légifrance via PISTE
- **Site**: https://www.legifrance.gouv.fr ; portal: https://piste.gouv.fr
- **API**: REST/JSON, OAuth2. Base: `https://api.piste.gouv.fr/dila/legifrance/lf-engine-app/`. Endpoints: `/search`, `/consult/code`, `/consult/lawDecree`, `/consult/jorf`. Daily bulk dumps (LEGI / JORF / KALI XML) at https://echanges.dila.gouv.fr.
- **Languages**: French.
- **Search**: NOR identifier, code articles, full-text.
- **Formats**: LEGI XML (custom, pre-AKN), JSON via API, PDF, HTML.
- **License**: Open License 2.0 (Etalab) — commercial use OK with attribution.
- **Adapter shape**: needs OAuth2 token refresh, otherwise straightforward.

### 3. EU — EUR-Lex (priority)
- **Site**: https://eur-lex.europa.eu
- **APIs**:
  - REST/SOAP web service at `https://eur-lex.europa.eu/EURLexWebService` (free registration).
  - SPARQL endpoint at `https://publications.europa.eu/webapi/rdf/sparql`.
  - Cellar OData / RDF bulk dumps for offline mirroring.
- **Languages**: all 24 EU official languages.
- **Search**: CELEX number lookup; "Expert Query" SOAP interface; full-text + metadata.
- **Formats**: Formex 4 XML, Akoma Ntoso (newer), HTML, PDF, RDF.
- **License**: Commission Decision 2011/833/EU — free reuse including commercial, attribution required, no red flags.
- **Adapter shape**: SOAP for search/lookup; CELEX → URL pattern for direct document fetch in N languages with EN fallback.

#### Reference-language fallback (EUR-Lex specific)

A multilingual EUR-Lex document is identified by its CELEX, e.g. `32016R0679` (GDPR). The same CELEX has up to 24 language variants. The user picks a primary language (say Italian) and an automatic fallback (English).

Resolution algorithm:

1. Build URL for primary: `https://eur-lex.europa.eu/legal-content/IT/TXT/?uri=CELEX:{celex}`.
2. If 200 → use it.
3. Else → retry with English: `…/EN/TXT/?uri=CELEX:{celex}`.
4. Else → mark as `unavailable` and skip.

Document-level metadata (CELEX, EuroVoc descriptors, ELI, date) stored once; full text stored per language actually fetched.

### 4. Spain — BOE
- **Site**: https://www.boe.es
- **APIs**:
  - Open Data REST: `https://www.boe.es/datosabiertos/api/`.
  - Daily summary XML: `https://www.boe.es/diario_boe/xml.php?id=BOE-S-YYYYMMDD`.
  - Per-document URL: `…/buscar/act.php?id={id}`.
- **Languages**: Spanish; partial co-official languages.
- **Search**: identifier (`BOE-A-YYYY-NNNNN`), date range; full-text via HTML form.
- **Formats**: XML (custom + ELI/RDF pilot), PDF, HTML.
- **License**: BOE reuse permitted with attribution; commercial OK.
- **Note**: CENDOJ (case law) has no API and ToS forbid scraping → not implemented.

### 5. Germany — Gesetze im Internet (BMJ)
- **Site**: https://www.gesetze-im-internet.de
- **API**: NO REST. Bulk TOC at `https://www.gesetze-im-internet.de/gii-toc.xml` lists every law as a per-law ZIP containing custom GII XML + PDF. Same pattern at `rechtsprechung-im-internet.de` for case law.
- **Languages**: German (selected English translations available for major codes).
- **Search**: none server-side — list everything, search locally after ingestion.
- **Formats**: GII XML (custom DTD), HTML, PDF.
- **License**: "Free use for personal / non-commercial". Commercial reuse requires permission — **red flag for SaaS**, OK for personal desktop use.
- **Adapter shape**: scheduled bulk-pull of TOC → diff against local index → fetch new/changed ZIPs.

### 6. Italy — Normattiva (direct: low-priority; via HF dataset: viable)
- **Site**: https://www.normattiva.it
- **API**: **none**. URN-based deep links (`urn:nir:stato:legge:2003-06-30;196` →
  `https://www.normattiva.it/uri-res/N2Ls?urn:nir:stato:legge:2003-06-30;196`). Akoma Ntoso XML downloadable per act from the page UI but not via a documented API.
- **Languages**: Italian.
- **Search**: HTML form only.
- **Formats**: Akoma Ntoso XML, HTML, PDF.
- **License (direct)**: Normattiva ToS forbid systematic harvesting / republication — **legal red flag for direct ingestion**.
- **Recommendation (direct path)**: ship a **manual import** only — let the user paste a Normattiva URN, fetch that one document, store it. No background sync.

#### Italy via HuggingFace `dossier-legal/italian-legal-corpus` (recommended path)

A third-party CC-BY-4.0 dataset on HuggingFace re-publishes a comprehensive Italian legal corpus that bypasses our Normattiva ToS exposure (the upstream dataset author handled redistribution; we consume a CC-BY-licensed derivative).

- **URL**: https://huggingface.co/datasets/dossier-legal/italian-legal-corpus
- **License**: CC-BY-4.0 — commercial use OK with attribution to the dataset.
- **Size**: ~366 000 documents, 1.74 GB compressed (Parquet).
- **Sources**:
  - Normattiva — ~300K Italian national legislation acts
  - Corte Costituzionale — ~18K constitutional-court decisions
  - OpenGA — ~100K administrative-justice rulings
  - EUR-Lex (Italian) — ~50K EU acts with Italian text (overlaps with our EUR-Lex V1)
- **Schema** (per row): `id`, `source`, `doc_type`, `title`, `date`, `text` (cleaned full text), `authority`, `number`, `year`, `ecli`, `language` (`"it"`).
- **Date range**: 1861 → present (spans Italian unification to 2026).
- **Access**: ungated; downloadable as Parquet via HuggingFace CDN, no token required.

##### Adapter design (planned)

The dataset is too large to mirror in full into `sqlite-vec` (~5 GB of vectors at 768 dims × 5 chunks/doc), so the adapter would not bulk-embed. Instead:

1. **Lazy bulk download** — first time the user enables the corpus, fetch the Parquet shards into `data/corpora/italian-legal/`. ~1.74 GB one-shot download, resumable.
2. **Local FTS index** — build a SQLite FTS5 virtual table over `(title, number, year, authority, doc_type, text)` so search-by-number and search-by-keyword run locally without ever touching Normattiva. ~30 min build on first import.
3. **Selective ingestion** — searches return hits; the user picks which ones to push into their personal RAG pool. This gives the same "find act → add to my index" UX as EUR-Lex V1 with no bulk-embedding overhead.
4. **Reuse the chat-cache layout** — each ingested document goes through the same `cache/<sha256>.txt` flow used by EUR-Lex and chat-attached docs, so retrieval is uniform.

##### Implementation status (V1, shipped)
- ✅ Adapter `src/corpora/italian_legal.rs` — bulk Parquet importer + HF `/rows` fetcher.
- ✅ Parquet reader via `parquet = "53"` + `arrow-array` with column projection (we pull metadata only — no `text` body — so the import stays at ~80 MB).
- ✅ FTS5 virtual table on `(title, authority, number)` plus B-tree indexes on `source / doc_type / year`.
- ✅ Migration `0016_italian_legal.sql` adds `italian_corpus`, `italian_corpus_fts`, `italian_corpus_meta`, `italian_corpus_settings`.
- ✅ Routes `src/routes/italian_legal.rs`: config, import, import-status, search, fetch (single doc → cache + embed), documents list, delete, resync.
- ✅ UI page `/account/italia-legale` with enable toggle, sources checklist, bulk import + progress bar, filtered FTS5 search, multi-select results → bulk-indicizza, indexed-docs list with the same status badges (`indicizzato` / `in sync N%` / `in attesa` / `interrotto`) and `Riavvia` recovery button as EUR-Lex.
- ✅ Sources covered: **Normattiva** (~69K) + **Corte Costituzionale** (~22K) = ~91K rows.

##### Sources NOT yet covered (V2 / V3 roadmap)

These are the ones surfaced as "in arrivo" in the UI. Each row below is the concrete work item to make that source real.

| Source | Status | Volume | What's needed |
|---|---|---|---|
| **OpenGA** (TAR + Consiglio di Stato) | 🟡 in dataset, opt-in mancante | ~125K | Quasi gratis: aggiungere `"openga"` a `DEFAULT_SOURCES` in `src/corpora/italian_legal.rs`, esporre la checkbox di import nella UI come opt-in (volumi grandi → import iniziale ~+150 MB). Doc types: `decision`, `order`, `decree`, `opinion`. `authority` = nome del TAR / Consiglio. |
| **Cassazione** (civile + penale + sezioni unite) | 🔴 fuori dataset HF | sconosciuto, indicativamente >1M | Servirà un adapter dedicato. Sorgenti possibili: (a) snapshot annuali CSV/XML da italgiure.giustizia.it se accessibili sotto licenza (verificare ToS) — (b) dataset terzo eventualmente pubblicato su HuggingFace (es. cerca `cassazione` con filtro CC-BY) — (c) fetcher per ECLI singoli con politica di consumo controllato. Sotto la trait `LegalCorpusAdapter` il design è già pronto, manca la sorgente. |
| **Aggiornamenti live Normattiva (post-2026-03-01)** | 🔴 placeholder | incrementale | Il dataset HF è frozen. Per atti pubblicati dopo lo snapshot serve un client che risolva URN (`urn:nir:stato:legge:YYYY;NNN`) → URL Normattiva → scraping del singolo atto in Akoma Ntoso. ToS Normattiva vieta harvesting sistematico ma consente fetch puntuale → uso ammesso solo come "single-URN import" su richiesta utente, non come crawler periodico. |
| **Leggi regionali** | 🔴 placeholder | ~varia per regione | 20 sistemi diversi (BUR + sito Consiglio Regionale per ogni regione). Approccio realistico: adapter generico "Akoma Ntoso URL → import" che lascia all'utente la mappatura regione-per-regione (o un seed di URL preconfigurati per le 5-6 regioni più importanti: Lombardia, Lazio, Veneto, Emilia-Romagna, Toscana, Sicilia). |
| **Gazzetta Ufficiale** (atti correnti) | 🔴 placeholder | ~daily diff | Sumario XML quotidiano: `https://www.boe.es/diario_boe/...` no, in Italia `https://www.gazzettaufficiale.it/`. Sovrappone ~80% con Normattiva live; utile come secondo canale per atti molto recenti che Normattiva consolida con qualche giorno di ritardo. |
| **Decreti ministeriali / Circolari** | 🔴 fuori dataset | ~varia | Un sottoinsieme è in Normattiva; il resto sui siti dei singoli ministeri. Adapter per-ministero non realistico — meglio un "import da URL" che accetta una pagina ufficiale e ne scarica il PDF/HTML, lasciando la curatela all'utente. |

##### Caveats note
- Il dataset HF mostra qualche `year` futuro (es. 2026-10) per metadati di pubblicazione programmata. Filtriamo `year <= current_year` solo nella UI di ricerca, ma la riga resta nell'indice.
- Bulk import ~80 MB / ~3 minuti — la UI lo dichiara prima di partire.
- Attribuzione CC-BY-4.0 al dataset autore (`dossier-legal`) prevista nel footer di `Italia legale` (TODO se vuoi che la inserisca).
- Il dataset è uno snapshot unico (commit pinned a `e503a93f...`). Per restare attuali serve la live-fetch row sopra.

### Out of scope

- **CENDOJ** (Spanish case law) — ToS-blocked.
- **Bundesgesetzblatt** — paywall / registered access.
- **Conseil d'État ArianeWeb** (French case law) — no API; partially in JADE fund via Légifrance.

## UI plan

Settings sidebar reorganisation:

```
Impostazioni
├── Generale
├── Modelli LLM
├── Server MCP
├── Documenti locali           ← was "Sincronizzazione"
└── Corpora giuridici
    ├── EUR-Lex                 (24 lingue, fallback EN)
    ├── Italia (Normattiva)     (manual-import only)
    ├── Francia (Légifrance)
    ├── Germania (GII)          (commercial-use red flag)
    ├── Spagna (BOE)
    └── Danimarca (Retsinformation)
```

Each corpus page renders the same `LegalCorpusPanel` component with corpus-specific config:

- **Toggle**: "Sincronizzazione attiva" — when off, no background fetches and search is disabled.
- **Lingua di riferimento** (EUR-Lex only): dropdown of supported languages; fallback to EN if a doc is missing in the chosen language.
- **Cerca per identificatore**: free-text input — corpus-specific identifier (CELEX / NOR / BOE id / law number).
- **Cerca per parole chiave**: free-text input — full-text search routed to the source's native endpoint.
- **Risultati**: list of hits, each with an "Aggiungi al mio indice" button.
- **Documenti indicizzati**: list of corpus docs already in the user's `documents` table, with remove/refresh.

## Backend adapter contract

Each corpus implements a Rust trait:

```rust
#[async_trait]
pub trait LegalCorpusAdapter: Send + Sync {
    fn id(&self) -> &'static str;             // "eurlex", "retsinformation", …
    fn languages(&self) -> &[&'static str];   // ["it","en","fr",...] or single

    async fn search_by_id(
        &self,
        identifier: &str,
        language: Option<&str>,
    ) -> Result<Vec<CorpusHit>>;

    async fn search_by_keyword(
        &self,
        query: &str,
        language: Option<&str>,
        limit: usize,
    ) -> Result<Vec<CorpusHit>>;

    async fn fetch(
        &self,
        hit: &CorpusHit,
        language: Option<&str>,
    ) -> Result<CorpusDocument>;
}

pub struct CorpusHit {
    pub identifier: String,                    // CELEX / NOR / ELI / law-number
    pub title: String,
    pub date: Option<chrono::NaiveDate>,
    pub url: String,
    pub languages_available: Vec<String>,
}

pub struct CorpusDocument {
    pub identifier: String,
    pub title: String,
    pub language: String,                       // actual language fetched
    pub fetched_with_fallback: bool,            // true if user wanted X, got EN
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub source_url: String,
}
```

Once `fetch` returns, the routes layer:

1. Computes SHA-256 of the bytes → reuses the same hash-keyed cache layout as chat-attachments (see [CACHE.md](CACHE.md)).
2. Inserts a `documents` row with `corpus_id`, `corpus_identifier`, `corpus_language`, `fetched_with_fallback`, `chat_id = NULL`, `project_id = NULL` (a future migration adds those columns).
3. Schedules embedding via the existing `EmbeddingService::index_document` path.

The user's RAG queries automatically include corpus docs in the global partition; per-corpus filtering can be exposed later via a scope dropdown if needed.

## Implementation status

| Component | Status |
|---|---|
| UI: rename "Sincronizzazione" → "Documenti locali" | ✅ |
| UI: EUR-Lex page with toggle / language / fallback / CELEX fetch | ✅ |
| UI: per-corpus pages for IT/FR/DE/ES/DK | 🔲 planned |
| Schema: migration 0015 (`documents.corpus_*` + `corpus_settings` table) | ✅ |
| Backend trait `LegalCorpusAdapter` | ✅ ([src/corpora/mod.rs](../src/corpora/mod.rs)) |
| Adapter: **EUR-Lex** V1 — CELEX fetch via HTML scraping, EN fallback | ✅ ([src/corpora/eurlex.rs](../src/corpora/eurlex.rs)) |
| Adapter: EUR-Lex V2 — SOAP CWS keyword search | 🔲 needs registration ([EURLEX_REGISTRATION.md](EURLEX_REGISTRATION.md)) |
| Adapter: Retsinformation | 🔲 planned |
| Adapter: Légifrance (PISTE) | 🔲 planned |
| Adapter: BOE | 🔲 planned |
| Adapter: Gesetze im Internet | 🔲 personal-use disclaimer required |
| Adapter: Normattiva | 🔲 manual-import only (ToS) |
| Adapter: HuggingFace `dossier-legal/italian-legal-corpus` (Normattiva + Corte Cost) | ✅ shipped — see §Italy via HuggingFace |
| Adapter: OpenGA (TAR + CdS) — opt-in del dataset HF | 🔲 ~30 min: aggiungere `openga` a `DEFAULT_SOURCES` + checkbox UI |
| Adapter: Cassazione (civile + penale + sez. unite) | 🔲 V2 — sorgente da identificare (italgiure.giustizia.it / dataset terzo) |
| Adapter: Live Normattiva (post-snapshot) | 🔲 V2 — client URN, single-fetch on user demand |
| Adapter: Leggi regionali (20 BUR + Consigli) | 🔲 V3 — adapter generico Akoma Ntoso da URL |
| Adapter: Gazzetta Ufficiale (sumario XML quotidiano) | 🔲 V3 — secondo canale per atti recentissimi |
| Adapter: Decreti ministeriali / circolari (per-ministero) | 🔲 V3 — import da URL con curatela utente |

EUR-Lex V1 ships using the public legal-content endpoint (no auth). The SOAP CWS for keyword search is V2 — see [EURLEX_REGISTRATION.md](EURLEX_REGISTRATION.md) for the registration path. Other adapters fit the same trait.
