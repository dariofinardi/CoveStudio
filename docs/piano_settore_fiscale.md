# Settore «Fiscale» — Piano descrittivo

> Settore aggiunto in **v0.7.0**. Dominio canonico: `fiscale`. Etichette
> localizzate: IT «Fiscale», EN «Tax», FR «Fiscalité», DE «Steuern»,
> ES/PT «Fiscal».

## 1. Obiettivo e perimetro

Il settore **Fiscale** assiste commercialisti, consulenti tributari, CAF
e studi professionali italiani sul versante **adempimenti, imposte e
contenzioso tributario**. È pensato per il mercato e l'ambito italiano:
i prompt, le norme citate e i workflow sono tarati sulla normativa
tributaria nazionale e sulla prassi dell'Agenzia delle Entrate.

### Distinzione dal settore «Finanza»

I due settori sono complementari e volutamente distinti:

| Settore | Copre |
|---|---|
| **Finanza** (`finance`) | Riclassificazione bilanci, indicatori economico-finanziari, valutazione d'azienda, crisi d'impresa (CCII), CTU tributaria, perizie di stima |
| **Fiscale** (`fiscale`) | IVA, imposte dirette (IRPEF/IRES/IRAP), imposte indirette (registro/bollo/ipo-catastali), regimi agevolati, ravvedimento, monitoraggio fiscale, accertamento e processo tributario |

Un commercialista lavora su entrambi: il selettore di settore in alto
nella sidebar (v0.6.7) permette di passare dall'uno all'altro in un
click, filtrando workflow, template e ruoli di default.

## 2. Aggiornamenti normativi recepiti (verifica 2024)

Il system prompt e i workflow del settore recepiscono esplicitamente le
riforme tributarie 2024, perché citare istituti superati in un prodotto
fiscale è dannoso:

- **Reclamo-mediazione (art. 17-bis D.Lgs. 546/1992): ABROGATO** dal
  4 gennaio 2024 (art. 2 D.Lgs. 220/2023). Per i ricorsi notificati da
  tale data NON esiste più la fase obbligatoria di reclamo: il ricorso
  si propone direttamente alla **Corte di Giustizia Tributaria di primo
  grado**.
- **Nuovo regime sanzionatorio (D.Lgs. 87/2024) dal 1° settembre 2024:**
  per omesso/tardivo versamento la sanzione base scende dal 30% al
  **25%**; il ravvedimento si semplifica (riduzione a 1/7 in luogo di
  1/6 in alcuni casi) e il cumulo giuridico è esteso al ravvedimento.
  Lo spartiacque è la **data di commissione della violazione**:
  fino al 31.8.2024 vecchio regime, dal 1.9.2024 nuovo regime.
- **Contraddittorio preventivo generalizzato** (art. 6-bis L. 212/2000,
  introdotto dal D.Lgs. 219/2023 di riforma dello Statuto del
  contribuente): l'ufficio deve di norma instaurare il contraddittorio
  prima di emettere un atto impugnabile.
- Le ex Commissioni Tributarie sono dal 2023 le **Corti di Giustizia
  Tributaria** di primo e secondo grado (L. 130/2022).

## 3. Workflow inclusi (`config/workflow-presets/fiscale/`)

Undici preset pronti all'uso, mix assistant + tabular.

### Assistant (chat strutturata)

| Slug | Titolo | Funzione |
|---|---|---|
| `parere-tributario` | Parere tributario strutturato | Parere su quesito: fatti → norma → prassi/giurisprudenza → analisi → conclusione → disclaimer |
| `ravvedimento-operoso` | Ravvedimento operoso — calcolo guidato | Calcolo imposta + sanzione ridotta (regime D.Lgs. 87/2024) + interessi legali pro-rata, con codici tributo F24 |
| `analisi-avviso-accertamento` | Analisi avviso di accertamento + strategia difensiva | Termini di impugnazione, rilievi, vizi, confronto opzioni (adesione/autotutela/ricorso) post-abrogazione reclamo |
| `analisi-fiscale-bilancio` | Analisi fiscale del bilancio | Lettura del bilancio **ai fini delle imposte**: derivazione (art. 83 TUIR), poste a rilevanza fiscale, stima IRES/IRAP, fiscalità differita (OIC 25) |

### Tabular (estrazione multi-documento)

| Slug | Titolo | Una riga per… |
|---|---|---|
| `riconciliazione-iva` | Riconciliazione liquidazioni IVA periodiche | periodo di liquidazione (registri vs LIPE vs F24) |
| `verifica-forfettario` | Verifica requisiti regime forfettario | requisito/causa ostativa (L. 190/2014 art. 1 c. 54-89) |
| `quadro-rw-monitoraggio` | Monitoraggio fiscale — Quadro RW (IVIE/IVAFE) | attività estera (immobile/conto/partecipazione/cripto) |
| `imposte-indirette-atto` | Imposte indirette su atti | atto (registro/bollo/ipo-catastali, DPR 131/86) |
| `scadenzario-versamenti-f24` | Scadenzario versamenti F24 per cliente | scadenza di versamento (tributo/codice/periodo/importo) |
| `riconciliazione-civilistico-fiscale` | Riconciliazione civilistico-fiscale (variazioni IRES — Quadro RF) | variazione in aumento/diminuzione dal risultato civilistico al reddito imponibile (art. 83 TUIR) |
| `base-imponibile-irap` | Determinazione base imponibile IRAP da bilancio | voce/aggregato del valore della produzione netta (art. 5 D.Lgs. 446/97) |
| `analisi-cespiti` ⇄ | Analisi cespiti / registro beni ammortizzabili | cespite o categoria (riconciliazione registro↔bilancio, ammortamenti art. 102 TUIR, eccedenza fiscale, plus/minus) |

> **Workflow cross-dominio (⇄).** Due workflow del commercialista sono
> registrati una sola volta ma visibili in **due settori** tramite il
> campo `also_applicable_to`: `analisi-cespiti` (primario `fiscale`,
> anche `finance`) e `controlli-libri-contabili` (primario `finance`,
> anche `fiscale` — file in `config/workflow-presets/finance/`). Così
> l'analisi cespiti e i controlli di quadratura compaiono sia nel
> picker Fiscale sia in quello Finanza, senza duplicare il JSON. Vedi
> `docs/WORKFLOWS.md` §2 (campo `also_applicable_to`).

> **Bilancio: angolazione fiscale vs finanziaria.** L'analisi del bilancio
> esiste in due settori con tagli diversi e complementari: nel settore
> **Finanza** la lettura è gestionale/valutativa (riclassificazione,
> indici, valutazione d'azienda, crisi); nel settore **Fiscale** la
> lettura è tributaria (dal risultato civilistico al reddito imponibile,
> base IRAP, fiscalità differita). I tre workflow `analisi-fiscale-bilancio`,
> `riconciliazione-civilistico-fiscale` e `base-imponibile-irap` coprono
> questa seconda angolazione.

## 4. Column-preset inclusi (`config/column-presets/fiscale/`)

Nove colonne quick-insert per costruire rapidamente revisioni tabellari
fiscali su misura: `imponibile`, `aliquota`, `imposta-dovuta`,
`ritenuta`, `sanzione` (con avviso sul nuovo regime D.Lgs. 87/2024),
`interessi` (tasso legale pro-rata annuo), `norma-riferimento`,
`scadenza`, `codice-tributo`.

## 5. System prompt (`config/system-prompts/{lang}/fiscale.md`)

Sei lingue (it/en/fr/de/es/pt). Il prompt italiano è il più ricco; gli
altri sono traduzioni operative che mantengono i riferimenti normativi
italiani (la materia è nazionale). Vincoli chiave imposti al modello:

- Cita la norma puntuale al primo richiamo (TUIR, DPR 633/72, DPR
  600/73, D.Lgs. 546/92, D.Lgs. 472/97, L. 212/2000).
- Disclaimer professionale obbligatorio su ogni parere/calcolo.
- Separazione netta fatti / interpretazioni; «DATO NON DISPONIBILE» per
  i dati mancanti (mai inventare aliquote, imponibili, scadenze, codici
  tributo).
- Importi formato «€ 1.234,56»; date ISO «YYYY-MM-DD».

## 6. Banche dati pubbliche e open per norme e sentenze

Fonti gratuite e ufficiali utili per verificare norme e giurisprudenza
tributaria, da affiancare (in futuro) come corpora del settore:

| Fonte | URL | Contenuto |
|---|---|---|
| **Normattiva** | normattiva.it | Testi normativi vigenti e multivigenti (IPZS) — leggi, decreti, codici |
| **Documentazione Economica e Finanziaria (MEF)** | def.finanze.it | Banca dati gratuita: normativa tributaria, **prassi** (circolari/risoluzioni AdE), **giurisprudenza tributaria** (Cassazione e Corti di Giustizia Tributaria) |
| **Sentenze Web Cassazione** | sentenze.cortedicassazione.it | Massime e testi delle sentenze civili/tributarie della Corte di Cassazione |
| **Giustizia Tributaria (DGT-MEF)** | def.finanze.it/DocTribFrontend | Banca dati delle decisioni delle Corti di Giustizia Tributaria di primo e secondo grado |
| **EUR-Lex** | eur-lex.europa.eu | Direttiva IVA 2006/112/CE e normativa UE rilevante (già integrato in Cove Studio come corpus) |
| **Agenzia delle Entrate** | agenziaentrate.gov.it | Modelli, istruzioni, codici tributo, interpelli pubblicati |

> Nota di prodotto: l'integrazione di `def.finanze.it` e
> `sentenze.cortedicassazione.it` come corpora consultabili in chat è
> tracciata come lavoro futuro (vedi roadmap §8). Normattiva ed EUR-Lex
> sono già accessibili tramite gli adapter corpora esistenti.

## 7. Come usare il settore

1. Imposta il settore attivo su **Fiscale** (selettore in sidebar, o
   Settings → Generale → Dominio predefinito).
2. In **Workflow** trovi gli 8 preset `Fiscale` filtrati.
3. In **Revisioni tabellari** → «Nuova revisione» scegli un workflow
   tabular Fiscale; i documenti del progetto vengono ereditati.
4. Le colonne quick-insert Fiscale sono disponibili nel doc-picker delle
   revisioni tabellari per costruire schemi su misura.

## 8. Roadmap di arricchimento

- **Fase 1 (fatto, v0.7.0):** dominio + 8 workflow + 9 column-preset +
  system prompt 6 lingue + doc descrittivo.
- **Fase 2:** template DOCX fiscali (ricorso CGT primo grado, istanza di
  autotutela, istanza di adesione, F24 prospetto ravvedimento).
- **Fase 3:** corpora consultabili — integrazione `def.finanze.it`
  (prassi + giurisprudenza tributaria) e `sentenze.cortedicassazione.it`
  come adapter di ricerca in chat.
- **Fase 4:** calcoli assistiti automatici (tabelle aliquote IRPEF per
  anno, tasso legale per anno, codici tributo) come tool function-calling
  così il modello non deve «ricordare» valori volatili ma interrogare una
  tabella versionata.

## 9. Riferimenti normativi del settore

- **IVA:** DPR 633/1972; Direttiva 2006/112/CE.
- **Imposte dirette:** TUIR (DPR 917/1986); IRAP (D.Lgs. 446/1997).
- **Imposte indirette:** DPR 131/1986 (registro); D.Lgs. 347/1990 (ipo-catastali).
- **Regimi agevolati:** L. 190/2014 art. 1 c. 54-89 (forfettario).
- **Monitoraggio fiscale:** D.L. 167/1990 art. 4 (RW); L. 214/2011 (IVIE/IVAFE).
- **Accertamento:** DPR 600/1973.
- **Sanzioni:** D.Lgs. 471/1997 e D.Lgs. 472/1997; **riforma D.Lgs. 87/2024**.
- **Processo tributario:** D.Lgs. 546/1992; **riforma D.Lgs. 220/2023**; L. 130/2022 (Corti di Giustizia Tributaria).
- **Statuto del contribuente:** L. 212/2000; **riforma D.Lgs. 219/2023** (contraddittorio, autotutela).
