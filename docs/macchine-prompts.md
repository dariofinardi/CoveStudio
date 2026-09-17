# Macchine industriali — mapping brief → preset Cove Studio

> **Fonte**: [`docs/macchine.md`](macchine.md) v2.2 (1544 righe, 2026-05-21).
> Questo file mappa le sezioni del brief operativo ai preset
> effettivamente shippati come workflow e DOCX template sotto
> `config/workflow-presets/compliance/` e
> `config/docx-templates/compliance/`. Quando il brief evolve, i preset
> seguono — le righe ❌ qui sotto sono lo stato dell'implementazione,
> non un giudizio sul brief.

## Convenzioni di naming

- Workflow ID: `builtin-compliance-macchine-<slug>` (kebab-case)
- File workflow: `config/workflow-presets/compliance/macchine-<slug>.json`
- File DOCX template: `config/docx-templates/compliance/macchine-<slug>.json`
- Tutti i preset appartengono al **dominio `compliance`** e usano la
  pratica `Macchine industriali — <categoria>` come sotto-etichetta
  per il raggruppamento UI.

## Categorie del brief

Il brief §6 distingue tre categorie:

- **A — Analisi**: output Markdown libero con sezioni, gap evidenziati.
- **B — Tabular**: output tabella strutturata con schema fisso (7 colonne
  ID_Requisito / Riferimento_Normativo / Descrizione_Requisito / Presente /
  Sezione_Documento / Tipo_GAP / Note).
- **C — Generazione DOCX**: bozza documento prodotta tramite template.

In Cove Studio **tutte le categorie B sono implementate come workflow
`assistant`** (non come workflow `tabular`), perché lo schema "1 riga
= 1 requisito" del brief è l'inverso del tabular Cove Studio (che ha
"1 riga = 1 documento"). L'assistant emette la tabella nel corpo
della risposta.

## Mapping completo

### Prerequisito

| Brief | Preset Cove Studio | Tipo | Stato |
|---|---|---|---|
| Prompt A — Classificazione documento | `macchine-classify-doc` | assistant | ✅ |

### Categoria A — Analisi

| Brief | Preset Cove Studio | Tipo | Template DOCX | Stato |
|---|---|---|---|---|
| A1 — Analisi fascicolo tecnico (L3) | `macchine-audit-fascicolo` | assistant | `macchine-audit-fascicolo-report` | ✅ |
| A2 — Analisi manuale d'uso (L2) | *(coperto da B2 + audit fascicolo)* | — | — | 🟡 parziale |
| A3 — Analisi fascicolo accessorio L4 | `macchine-audit-fascicolo` *(con LIVELLO=L4)* | assistant | `macchine-audit-fascicolo-report` | ✅ |
| A4 — Analisi documentazione cyber (CRA) | *(coperto da B5)* | — | — | 🟡 parziale |
| A5 — Analisi documentazione RED | *(coperto da B6)* | — | — | 🟡 parziale |
| A6 — Analisi documentazione NIS2 | *(coperto da B7)* | — | — | 🟡 parziale |
| A7 — Analisi SBOM esistente | *(coperto da B8)* | — | — | 🟡 parziale |
| A8 — Estrazione SBOM da sorgenti | *(integrato nel workflow SBOM+CVE)* | — | — | 🟡 parziale |
| A9 — Interrogazione CVE su SBOM | `macchine-sbom-cve` | assistant | `macchine-sbom-cve-report` | ✅ |
| A10 — Confronto fascicolo vs manuale | *(coperto da A12)* | — | — | 🟡 parziale |
| A11 — Confronto DoC vs fascicolo | *(coperto da A12)* | — | — | 🟡 parziale |
| A12 — Coerenza multi-documento | `macchine-coerenza-multidoc` | assistant | — | ✅ |
| A13 — Analisi valutazione rischi | 🔲 *(deferito — coperto in parte da `audit-fascicolo` via Regola 6)* | — | — | 🔲 |
| A14 — Calcolo strutturale | `macchine-calcolo-strutturale` | assistant | — | ✅ |
| Prompt L — Audit integrato safety+cyber (Regola 11) | `macchine-audit-integrato` | assistant | — | ✅ |

### Categoria B — Checklist tabellari

| Brief | Preset Cove Studio | Tipo | Stato |
|---|---|---|---|
| B1 — Fascicolo tecnico L3 | `macchine-fascicolo-l3-checklist` | assistant | ✅ |
| B2 — Manuale d'uso L2 | `macchine-manuale-l2-checklist` | assistant | ✅ |
| B3 — Accessorio sollevamento L4 | `macchine-accessorio-l4-checklist` | assistant | ✅ |
| B4 — RES (Requisiti Essenziali di Sicurezza) | `macchine-res-checklist` | assistant | ✅ |
| B5 — Requisiti essenziali CRA | `macchine-cra-checklist` | assistant | ✅ |
| B6 — Requisiti essenziali RED | `macchine-red-checklist` | assistant | ✅ |
| B7 — Obblighi NIS2 supply chain | `macchine-nis2-supplychain-checklist` | assistant | ✅ |
| B8 — SBOM completezza CRA | `macchine-sbom-completeness` | assistant | ✅ |

### Categoria C — Generazione DOCX

| Brief | Preset Cove Studio | Tipo | Template DOCX | Stato |
|---|---|---|---|---|
| C1 — Genera manuale d'uso | `macchine-genera-manuale` | assistant | `macchine-manuale-uso-bozza` | ✅ |
| C2 — Espandi istruzioni assemblaggio | `macchine-espandi-assemblaggio` | assistant | — | ✅ |
| C3 — Genera Dichiarazione di Conformità CE | 🔲 *(deferito)* | — | — | 🔲 |
| C4 — Genera risk assessment safety | 🔲 *(deferito)* | — | — | 🔲 |
| C5 — Genera risk assessment cyber | 🔲 *(deferito)* | — | — | 🔲 |
| C6 — Report anomalie qualitative | 🔲 *(parzialmente coperto da `audit-fascicolo`)* | — | — | 🔲 |
| C7 — Genera SBOM CycloneDX | 🔲 *(richiede tool function-calling esterno)* | — | — | 🔲 |
| C8 — Genera registro GAP (XLSX) | 🔲 *(richiede pipeline xlsx generativa)* | — | — | 🔲 |

## Pipeline SBOM/CVE — tool function-calling (§10 del brief)

Il workflow `macchine-sbom-cve` orchestra la pipeline a 4 layer:

```
LAYER 1 — INGESTION
  parse_document · detect_sbom_format · validate_document
LAYER 2 — SBOM PROCESSING
  extract_components · normalize_identifiers · validate_sbom
LAYER 3 — CVE LOOKUP + ENRICHMENT
  get_cve · filter_cve · enrich_cve · check_patch_status
LAYER 4 — OUTPUT
  generate_cve_report
```

Stato dei 10 tool:

- ✅ `parse_document` — copre già la ingestion di PDF/DOCX/XLSX/TXT/RTF/MD
- 🔲 Gli altri 9 tool richiedono client HTTP verso NVD + EPSS + CISA KEV
  e parser CycloneDX/SPDX dedicati. Implementazione deferita.

Finché i tool 2-10 non esistono, il workflow `macchine-sbom-cve` funziona
in modalità degradata: il modello fa del suo meglio leggendo la SBOM
come testo e dichiarando esplicitamente quando non può accedere a NVD.
Documentare esplicitamente la limitazione all'utente.

## Regole di analisi automatica (§8 del brief)

Le 11 regole sono incorporate nei prompt dei workflow `audit-fascicolo`,
`calcolo-strutturale`, `coerenza-multidoc`, `audit-integrato`, `cra-checklist`
e `red-checklist`. Tabella di copertura:

| Regola | Descrizione | Workflow che la implementano |
|---|---|---|
| R1 | Conflitti valori di prestazione | `audit-fascicolo`, `accessorio-l4-checklist`, `coerenza-multidoc` |
| R2 | Limitazioni d'uso fuori marcatura | `audit-fascicolo`, `manuale-l2-checklist`, `accessorio-l4-checklist` |
| R3 | Scadenza obbligo archiviazione | `audit-fascicolo` |
| R4 | Coerenza firmatario DoC | `audit-fascicolo`, `accessorio-l4-checklist` |
| R5 | Coerenza multi-documento | `coerenza-multidoc` |
| R6 | Struttura valutazione rischi | `audit-fascicolo` |
| R7 | Relazione di calcolo strutturale (L4) | `calcolo-strutturale`, `accessorio-l4-checklist` |
| R8 | Requisiti CRA Parte I + II | `cra-checklist`, `audit-fascicolo`, `audit-integrato` |
| R9 | Requisiti essenziali RED | `red-checklist`, `audit-fascicolo` |
| R10 | Obblighi NIS2 supply chain | `nis2-supplychain-checklist` |
| R11 | Coerenza safety/cyber | `audit-integrato`, `cra-checklist` |

## Problemi noti nel brief (segnalati 2026-05-21)

- **§15** cita "Prompt B0" come prerequisito — voce non definita altrove
  nel brief. Trattato come refuso: il prerequisito è Prompt A
  (classificatore L1-L4) → workflow `macchine-classify-doc`.
- **Regola 3** dice "10 anni dalla **cessazione produzione**" (Art. 5(3)
  Dir. 2006/42/CE — corretto) ma il **Foglio 1 del workbook XLSX**
  calcola `Data_emissione + 3650`. Sono due cose diverse: la norma
  parte dall'ultima unità immessa sul mercato, non dall'emissione del
  documento. Il workflow `audit-fascicolo` segnala entrambe le date
  e raccomanda l'allineamento del workbook.

## Backlog (priorità decrescente)

1. **A13 — Analisi valutazione dei rischi** come workflow dedicato
   (oggi solo come Regola 6 dentro `audit-fascicolo`)
2. **C3 — Dichiarazione di Conformità CE** + template DOCX
3. **C4/C5 — Risk assessment safety/cyber** come template DOCX
4. **C8 — Workbook XLSX "Validazione Fascicolo Tecnico"** (6 fogli §11
   del brief) — richiede pipeline xlsx generativa dedicata
5. **Tool function-calling 2-10** della pipeline SBOM/CVE — richiede
   network policy NVD/EPSS/KEV e parser CycloneDX/SPDX
