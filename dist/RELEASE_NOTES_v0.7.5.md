# MikeRust v0.7.5 — Settore «Fiscale» + workflow cross-dominio

> **Changelog cumulativo dell'intera linea 0.7.x.** L'ultima release
> pubblicata è la **v0.6.7**; questa nota raccoglie tutto il lavoro da
> v0.7.0 a v0.7.5 in un'unica pubblicazione, così chi aggiorna da 0.6.7
> vede l'intero set di novità.

La linea 0.7.x introduce il **dodicesimo settore professionale —
«Fiscale»** (tax/commercialista italiano), porta in italiano l'ultimo
verticale ancora in inglese (legale), aggiunge l'analisi del bilancio
ai fini fiscali e il meccanismo di **registrazione cross-dominio** dei
workflow, esteso fino all'editor.

---

## 🆕 Nuovo settore «Fiscale» (v0.7.0)

Dodicesimo verticale professionale, `fiscale`, accanto a `finance`:
finance copre l'analisi (bilanci, valutazione d'azienda, crisi),
fiscale la parte tax-compliance + consulenza (IVA, dichiarazioni,
imposte dirette/indirette, ravvedimento, accertamento, contenzioso).

* **Registrazione dominio** end-to-end: `src/domain.rs`,
  `frontend/.../domain.ts`, e le etichette `Domains.values.fiscale` in
  tutte e 6 le lingue (IT «Fiscale», EN «Tax», FR «Fiscalité»,
  DE «Steuern», ES/PT «Fiscal»). Nessuna migrazione.
* **System prompt in 6 lingue** (`config/system-prompts/*/fiscale.md`)
  con le **riforme fiscali 2024** già recepite, così il modello non
  cita istituti superati:
  * reclamo-mediazione (art. 17-bis D.Lgs. 546/92) **abrogato** dal
    4.1.2024 (D.Lgs. 220/2023);
  * nuovo regime sanzionatorio **D.Lgs. 87/2024 dal 1.9.2024**
    (omesso/tardivo versamento 30% → 25%);
  * contraddittorio preventivo generalizzato (art. 6-bis L. 212/2000,
    D.Lgs. 219/2023).
* **8 workflow preset** (`config/workflow-presets/fiscale/`): 3
  assistant (parere tributario, ravvedimento operoso, analisi avviso
  di accertamento) + 5 tabellari (riconciliazione IVA, verifica
  forfettario, quadro RW IVIE/IVAFE, imposte indirette su atti,
  scadenzario F24).
* **9 column preset** (`config/column-presets/fiscale/`): imponibile,
  aliquota, imposta dovuta, ritenuta, sanzione, interessi, norma,
  scadenza, codice tributo.
* Nuovo piano descrittivo `docs/piano_settore_fiscale.md`.

## 📊 Analisi del bilancio ai fini fiscali (v0.7.2)

Il settore Fiscale non aveva un workflow di bilancio: l'analisi
civilistico-gestionale resta in `finance`, ma serviva la **lettura
fiscale** (risultato civilistico → reddito imponibile, base IRAP,
fiscalità differita). Aggiunti **3 workflow `fiscale`**:

* **`analisi-fiscale-bilancio`** (assistant) — lettura del bilancio
  per il principio di derivazione (art. 83 TUIR): voci tax-relevant
  (ammortamenti art. 102, svalutazione crediti art. 106, rappresentanza
  art. 108, interessi/ROL art. 96, compensi amministratori art. 95,
  auto art. 164, plus/minusvalenze artt. 86-87, IMU/IRAP art. 99),
  stima IRES, nota base IRAP, fiscalità differita/anticipata (OIC 25).
* **`riconciliazione-civilistico-fiscale`** (tabellare) — stile Quadro
  RF: una riga per variazione in aumento/diminuzione, con articolo
  TUIR, importo, segno e rigo RF.
* **`base-imponibile-irap`** (tabellare) — base IRAP dal CE per art. 5
  D.Lgs. 446/97, con costi esclusi e deduzioni spettanti.

Il settore Fiscale arriva così a **11 workflow** (4 assistant + 7
tabellari).

## 🇮🇹 Localizzazione italiana del verticale legale (v0.7.1)

Tradotti dall'inglese all'italiano i preset del **settore legale** —
l'ultimo contenuto ancora in inglese del catalogo (ereditato dai
template internazionali dell'upstream); medical, finance, fiscale,
insurance, PA ed edilizia-compliance erano già in italiano.

* **27 file** `domain: legal`: 14 workflow preset + 13 column preset.
* Tradotti in registro giuridico italiano `title`, `practice`,
  `prompt_md`, ogni `name`/`prompt` di colonna e i valori display dei
  `tags`. Restano invariati (canonici) `id`, `type`, `domain`,
  `index`, `format`, `match_pattern`/`match_flags`.
* Concetti giurisdizionali **adattati** al sistema italiano dove
  necessario (es. "security of tenure" → tutela ex L. 392/1978;
  indicizzazione canoni → ISTAT); termini di mercato e di prassi PE
  mantenuti non tradotti (SOFR, EURIBOR, General Partner, carried
  interest, waterfall…).

## 🔗 Workflow cross-dominio: preset (v0.7.3) → editor (v0.7.5)

Un singolo workflow può comparire nel picker di **più settori** senza
duplicare il JSON, tramite il campo `also_applicable_to` (stesso
meccanismo dei template DOCX).

**Preset di sistema (v0.7.3):**
* `WorkflowPreset` guadagna `also_applicable_to` + metodo
  `matches_domain` (dominio primario **oppure** un dominio aggiuntivo).
  Il loader valida le voci (scarta non-canoniche e il primario
  ridondante); il filtro `GET /workflow?domain=` usa `matches_domain`.
* Due nuovi workflow del commercialista, **entrambi `fiscale` ⇄
  `finance`**, da fonti autorevoli (art. 16 DPR 600/73, art. 102/86
  TUIR, coefficienti DM 31/12/1988, art. 5 D.Lgs. 446/97; norme
  verificate aggiornate — super/iper-ammortamento storico → credito
  d'imposta Transizione 4.0/5.0; plafond manutenzioni 5% art. 102 c.6):
  * **analisi cespiti / registro beni ammortizzabili** (tabellare) —
    riconciliazione registro↔bilancio, deducibilità ammortamenti,
    eccedenza fiscale, movimenti e plus/minusvalenze;
  * **controlli di quadratura libri contabili** (tabellare) — registro
    anomalie: partita doppia, progressività, saldi mastrini,
    riconciliazione IVA (registri ↔ LIPE ↔ dichiarazione), ritenute ↔
    F24, quadratura clienti/fornitori, scritture di assestamento.

**Editor (v0.7.5):** il meccanismo cross-dominio arriva ai workflow
**creati dall'utente**.
* Nuovo selettore **«Domini aggiuntivi»**: input a tag con
  autocompletamento sui domini disponibili (chip rimovibili, menù a
  tendina filtrato, frecce ↑↓, Invio per aggiungere, Backspace per
  togliere); il dominio primario è escluso dai suggerimenti. Presente
  sia nella creazione sia nell'editor con salvataggio automatico; i
  preset di sistema mostrano i domini aggiuntivi come badge in sola
  lettura.
* **Migrazione 0034**: nuova colonna `also_applicable_to TEXT NOT NULL
  DEFAULT '[]'` su `workflows`. Le righe esistenti restano a settore
  singolo — nessun cambiamento all'aggiornamento. Create/update
  sanificano l'elenco lato server; il filtro `GET /workflow?domain=`
  matcha primario **o** dominio aggiuntivo.
* 3 chiavi i18n × 6 locale.

---

## Note tecniche

* Una sola migrazione di schema nell'intera linea 0.7.x: **0034**
  (cross-dominio workflow utente). Tutto il resto è additivo
  (preset/prompt/colonne in JSON e Markdown).
* Verifiche: `svelte-check` 0 errori · 42/42 test preset-loader ·
  `cargo check` pulito.
* Documentazione: `docs/piano_settore_fiscale.md`, `docs/WORKFLOWS.md`
  (§2 `also_applicable_to`), README (12 domini + riga cross-dominio).

## Download

- `MikeRust_0.7.5_x64.msi` — Windows x86_64
- `MikeRust_0.7.5_arm64.msi` — Windows ARM64, Snapdragon X Elite

Sostituzione drop-in per la serie 0.6.x / 0.7.x.

## Licenza

AGPL-3.0-only. Il marchio e il logo Semplifica sono marchi registrati
di Semplifica s.r.l. (vedi NOTICE.md).
