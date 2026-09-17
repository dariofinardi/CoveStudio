# Cove Studio — Dominio: Pubblica Amministrazione Italiana

> Piano workflow, tabelle di output e template di prompt per l'analisi documentale nella PA italiana.
> Struttura compatibile con l'architettura JSON `config/` di Cove Studio.

---

## 1. Premessa e perimetro

La pubblica amministrazione italiana è uno dei casi d'uso più forti per Cove Studio per ragioni strutturali: produce volumi enormi di atti eterogenei con struttura ricorrente, riferimenti normativi obbligatori e obbligo di tracciabilità documentale. Esattamente il problema che il modello "carica → seleziona workflow → aspetta" risolve senza complessità agentiche.

### Principi operativi

| Principio | Descrizione |
|---|---|
| **Tracciabilità** | Ogni dato estratto è collegato al documento sorgente con riferimento all'articolo o alla pagina |
| **Revisione professionale** | Il funzionario o il legale valida ogni output prima di procedere |
| **Neutralità valutativa** | L'AI presenta i dati e segnala le criticità; il professionista esprime il giudizio |
| **Aderenza normativa** | Ogni estrazione fa riferimento esplicito alla norma applicata |
| **Completezza documentale** | Il sistema segnala gap, incongruenze e atti mancanti nel fascicolo |

### Corpus da indicizzare

| Fonte | Tipo | Stato in Cove Studio |
|---|---|---|
| Normattiva | Leggi e decreti italiani | In pipeline (Italian Legal Corpus) |
| EUR-Lex | Direttive e regolamenti UE | ✅ Già disponibile (GDPR, AI Act, Direttive appalti) |
| ANAC delibere e PNA | Anticorruzione e appalti | Da aggiungere |
| MIT circolari appalti | D.Lgs. 36/2023 attuazione | Da aggiungere |
| Gazzetta Ufficiale incrementale | Atti recenti | In preparazione |

---

## 2. Architettura del dominio

```
┌─────────────────────────────────────────────────────────────┐
│                     INPUT DOCUMENTALE                        │
│  Delibere · Determine · Contratti · Verbali · Relazioni     │
│  Istanze · Pareri · Rendiconti · Bandi · Capitolati         │
└────────────────────────┬────────────────────────────────────┘
                         │
        ┌────────────────▼───────────────────┐
        │      CLASSIFICAZIONE DOCUMENTALE    │
        │  (tipo atto, ente, riferimento norm.)│
        └────────────────┬───────────────────┘
                         │
     ┌───────────────────┼────────────────────┐
     │         │         │         │          │
     ▼         ▼         ▼         ▼          ▼
  BLOCCO 1  BLOCCO 2  BLOCCO 3  BLOCCO 4  BLOCCO 5
  Atti amm. Appalti   Procedim. PNRR/EU   Traspar.
```

---

## 3. Blocco 1 — Atti Amministrativi

### 3.1 Workflow

| ID | Nome | Tipo | Descrizione |
|---|---|---|---|
| `pa-delibera` | Analisi delibera | Narrativo | Estrae presupposti, vizi di forma, congruità motivazionale, riferimenti normativi mancanti |
| `pa-determina` | Analisi determina dirigenziale | Tabular | Mappa RUP, importi, CIG, copertura finanziaria, basi normative |
| `pa-ordinanza` | Ordinanza — verifica struttura | Narrativo | Controlla competenza, motivazione, proporzionalità, termini impugnativi |
| `pa-parere` | Estrazione pareri tecnici | Tabular | Tabella pareri richiesti / ricevuti / obbligatori ma mancanti |

### 3.2 Template prompt — `pa-delibera`

```
Sei un esperto di diritto amministrativo italiano. Analizza la delibera allegata
e produci una relazione strutturata secondo i seguenti punti.

STRUTTURA RICHIESTA:
1. Organo deliberante e competenza
   · Verifica che l'atto rientri nelle attribuzioni dell'organo (art. 42-48 TUEL
     per enti locali, o norma speciale applicabile)
   · Segnala eventuali profili di incompetenza assoluta o relativa

2. Presupposti e motivazione (art. 3 L. 241/1990)
   · I presupposti di fatto sono indicati e documentati?
   · La motivazione è sufficiente o meramente formale?
   · Sono citati i pareri obbligatori (tecnico, contabile, di regolarità)?

3. Iter procedimentale
   · Convocazione regolare (termini, ordine del giorno)
   · Quorum costitutivo e deliberativo rispettati
   · Astensioni obbligatorie per conflitto di interessi

4. Copertura finanziaria (art. 183 TUEL se applicabile)
   · Capitolo di bilancio, impegno di spesa, visto del responsabile finanziario

5. Vizi rilevati
   · Per ogni vizio: tipo (forma/sostanza/competenza), gravità
     (annullabile/nullo/irregolare), norma violata, documentazione di riferimento

6. Termini per impugnazione
   · TAR: 60 giorni dalla notifica/pubblicazione (art. 29 c.p.a.)
   · Ricorso straordinario al Presidente della Repubblica: 120 giorni
   · Autotutela: segnala se i presupposti art. 21-nonies L. 241/1990 sono presenti

Per ogni punto indica il riferimento preciso nel documento allegato.
Segnala esplicitamente le informazioni non reperibili nel fascicolo.
```

### 3.3 Template prompt — `pa-determina` (tabular)

```
Sei un esperto di diritto amministrativo e contabilità pubblica. Analizza la
determina dirigenziale allegata ed estrai le informazioni in formato tabella.

COLONNE OBBLIGATORIE:
- Campo
- Valore estratto
- Riferimento (articolo/pagina)
- Conforme (sì/no/da verificare)
- Note

RIGHE OBBLIGATORIE:
- Numero e data determina
- Ufficio/dirigente responsabile
- Oggetto sintetico
- Norma attributiva del potere
- CIG (se presente)
- Importo a base d'asta / importo affidato
- Capitolo di bilancio e impegno
- Visto di regolarità tecnica
- Visto di regolarità contabile
- Pareri allegati
- Pubblicazione all'albo pretorio
- Termine efficacia / esecutività

Segnala eventuali campi obbligatori assenti nel documento.
```

---

## 4. Blocco 2 — Contratti e Appalti Pubblici (D.Lgs. 36/2023)

### 4.1 Workflow

| ID | Nome | Tipo | Descrizione |
|---|---|---|---|
| `pa-appalto-review` | Review contratto di appalto | Tabular | Clausole critiche: penali, SAL, subappalto, recesso, ADR |
| `pa-rup-checklist` | Checklist RUP | Tabular | Adempimenti per fase: programmazione, progettazione, affidamento, esecuzione |
| `pa-collaudo` | Analisi verbale di collaudo | Narrativo | Riserve, difformità, termini di garanzia, responsabilità residue |
| `pa-variante` | Analisi variante in corso d'opera | Narrativo | Presupposti art. 120 D.Lgs. 36/2023, impatto economico, autorizzazioni |
| `pa-bando` | Analisi bando di gara | Tabular | Requisiti di partecipazione, criteri di aggiudicazione, cause di esclusione |

### 4.2 Template prompt — `pa-appalto-review` (tabular)

```
Sei un esperto di contrattualistica pubblica (D.Lgs. 36/2023 e Allegati).
Analizza il contratto di appalto allegato ed estrai le informazioni in formato tabella.

COLONNE OBBLIGATORIE:
- Clausola (articolo e titolo)
- Contenuto sintetico
- Norma di riferimento (D.Lgs. 36/2023 / capitolato tipo MIT / altro)
- Criticità (sì / no / da approfondire)
- Descrizione criticità
- Azione raccomandata

FOCUS OBBLIGATORIO sulle seguenti aree:
· Penali e ritardi (entità, calcolo, tetto massimo)
· SAL e pagamenti (frequenza, termini, interessi moratori D.Lgs. 231/2002)
· Subappalto e limiti (% ammessa, obblighi di indicazione, responsabilità solidale)
· Recesso e risoluzione (art. 122-123 D.Lgs. 36/2023, clausola di rinegoziazione)
· Foro competente e ADR (accordo bonario, arbitrato, mediazione)
· Garanzie fideiussorie (definitiva, polizza decennale postuma se edilizia)
· Varianti e limiti di importo (art. 120 D.Lgs. 36/2023)
· Riserve e contenzioso (termini, forma, effetti)
· Revisione prezzi (art. 60 D.Lgs. 36/2023, indici ISTAT)
· Clausole sociali e ambientali (se presenti)

Al termine produci una sezione "Sintesi rischi" con semaforo:
🔴 Alto · 🟡 Medio · 🟢 Basso — per ciascuna area analizzata.
```

### 4.3 Tabella checklist RUP — output `pa-rup-checklist`

| Fase | Adempimento | Norma (D.Lgs. 36/2023) | Eseguito | Data | Documento | Note |
|---|---|---|---|---|---|---|
| Programmazione | Inserimento nel programma biennale acquisti | Art. 37 | | | | |
| Programmazione | Nomina RUP | Art. 15 | | | | |
| Progettazione | Verifica progetto (indipendente) | Art. 42 | | | | |
| Affidamento | Pubblicazione bando su BDNCP | Art. 84 | | | | |
| Affidamento | Verifica requisiti aggiudicatario | Art. 94-98 | | | | |
| Affidamento | Comunicazione esito gara | Art. 90 | | | | |
| Esecuzione | Consegna lavori/servizi | Art. 107 | | | | |
| Esecuzione | Approvazione SAL | Art. 113 | | | | |
| Esecuzione | Collaudo / CRE | Art. 116 | | | | |
| Esecuzione | Svincolo garanzie | Art. 117 | | | | |

---

## 5. Blocco 3 — Procedimento Amministrativo (L. 241/1990)

### 5.1 Workflow

| ID | Nome | Tipo | Descrizione |
|---|---|---|---|
| `pa-241-check` | Verifica procedimento L. 241/90 | Tabular | Responsabile, termini, comunicazioni, preavviso rigetto, silenzio |
| `pa-silenzio` | Analisi silenzio-inadempimento | Narrativo | Decorso termini, tipologia silenzio, rimedi processuali |
| `pa-autotutela` | Autotutela — presupposti | Narrativo | Annullamento d'ufficio vs revoca, interesse pubblico, affidamento, termine 12 mesi |
| `pa-accesso` | Analisi istanza di accesso | Tabular | Tipo accesso (documentale/FOIA/civico), esito, eccezioni applicabili |

### 5.2 Template prompt — `pa-241-check`

```
Sei un esperto di diritto amministrativo italiano. Analizza il procedimento
descritto nei documenti allegati e verifica la conformità alla L. 241/1990 e s.m.i.

STRUTTURA RICHIESTA (per ogni punto: norma specifica · esito · doc. di riferimento · rischio):

1. RESPONSABILE DEL PROCEDIMENTO (art. 5-6)
   · Nominato e comunicato all'interessato?
   · Indicato nelle comunicazioni ufficiali?

2. COMUNICAZIONE DI AVVIO (art. 7-8)
   · Dovuta (il provvedimento finale non è vincolato e incide su terzi)?
   · Se dovuta: inviata, con quali contenuti, ai soggetti corretti?
   · Eccezioni per urgenza motivata (art. 7 co. 2)?

3. TERMINE DI CONCLUSIONE (art. 2)
   · Norma che fissa il termine (regolamento, norma speciale, 30 gg. residuale)?
   · Data avvio — data conclusione — termine rispettato?
   · Decorso senza provvedimento: silenzio-assenso (art. 20) o inadempimento (art. 2)?

4. PREAVVISO DI RIGETTO (art. 10-bis)
   · Applicabile (procedimento a istanza di parte con esito negativo)?
   · Inviato? Controdeduzioni ricevute e valutate nella motivazione finale?

5. MOTIVAZIONE (art. 3)
   · Presupposti di fatto e di diritto esplicitati?
   · Motivazione per relationem: atti richiamati allegati o facilmente conoscibili?

6. PARTECIPAZIONE (art. 9-10)
   · Soggetti portatori di interessi diffusi ammessi?
   · Memorie e documenti depositati: valutati o espressamente disattesi?

7. ACCESSO AGLI ATTI (art. 22 ss.)
   · Istanze presentate: riscontrate nei termini (30 gg.)?
   · Dinieghi: motivati, con indicazione dell'impugnazione?

SCALA DI RISCHIO: Alto (vizio invalidante) · Medio (irregolarità sanabile) · Basso (formale)
```

---

## 6. Blocco 4 — PNRR e Fondi Europei

### 6.1 Workflow

| ID | Nome | Tipo | Descrizione |
|---|---|---|---|
| `pa-pnrr-milestone` | Verifica milestone PNRR | Tabular | Milestone, target, scadenza, documentazione probatoria, stato |
| `pa-rendiconto` | Analisi rendiconto fondi EU | Tabular | Spese ammissibili, documentazione, rischio disallineamento |
| `pa-audit` | Audit trail documentale | Narrativo | Ricostruzione catena documentale per verifica ispettiva (Corte dei Conti, UE) |
| `pa-irregolarita` | Rilevazione irregolarità | Tabular | Irregolarità rilevate, norma violata, misura correttiva, impatto finanziario |

### 6.2 Tabella milestone PNRR — output `pa-pnrr-milestone`

| Milestone / Target | Investimento | Scadenza | Documentazione richiesta | Documentazione disponibile | Gap | Rischio |
|---|---|---|---|---|---|---|
| | | | | | | 🔴/🟡/🟢 |

### 6.3 Template prompt — `pa-pnrr-milestone`

```
Sei un esperto di fondi europei e PNRR (Piano Nazionale di Ripresa e Resilienza).
Analizza i documenti allegati relativi all'intervento e verifica lo stato di
avanzamento rispetto agli obiettivi assegnati.

STRUTTURA RICHIESTA:
1. Identificazione intervento
   · Missione, Componente, Investimento/Riforma di riferimento
   · Codice CUP, soggetto attuatore, soggetto beneficiario

2. Milestone e target assegnati
   · Estrai in tabella: ID, descrizione, tipo (milestone/target), scadenza,
     indicatore di realizzazione, fonte di verifica prevista

3. Stato di avanzamento documentale
   · Per ogni milestone/target: documentazione probatoria disponibile,
     documentazione mancante, gap da colmare

4. Spese rendicontate
   · Importo, categoria di costo, periodo di riferimento
   · Verifica ammissibilità (spese sostenute dopo il 01/02/2020,
     conformità Reg. UE 2021/241 e linee guida RGS)

5. Rischi
   · Ritardi rispetto alle scadenze: impatto su tranche di finanziamento
   · Doppio finanziamento: verifica con altri fondi (FESR, FSE+, fondi nazionali)
   · Irregolarità: segnalare per eventuale comunicazione OLAF

6. Azioni raccomandate
   · Priorità, responsabile, termine
```

---

## 7. Blocco 5 — Trasparenza e Anticorruzione

### 7.1 Workflow

| ID | Nome | Tipo | Descrizione |
|---|---|---|---|
| `pa-ptpct` | Analisi PTPCT | Tabular | Misure obbligatorie, stato attuazione, gap rispetto al PNA ANAC vigente |
| `pa-foia` | Gestione istanza FOIA | Narrativo | Legittimità, eccezioni applicabili (art. 5-bis D.Lgs. 33/2013), termini |
| `pa-conflitto` | Conflitto di interessi | Narrativo | Segnali negli atti, obblighi di astensione, riferimenti ANAC |
| `pa-dati-aperti` | Verifica obblighi pubblicazione | Tabular | Obblighi D.Lgs. 33/2013, stato pubblicazione, aggiornamento, sezione sito |

### 7.2 Template prompt — `pa-ptpct`

```
Sei un esperto di prevenzione della corruzione e trasparenza nella PA italiana
(D.Lgs. 190/2012, D.Lgs. 33/2013, PNA ANAC vigente). Analizza il PTPCT allegato.

STRUTTURA RICHIESTA:
1. Struttura del documento
   · Presenza delle sezioni obbligatorie (analisi del contesto, mappatura processi,
     misure specifiche, misure di trasparenza, sezione formazione)

2. Analisi del rischio
   · Processi mappati: completi rispetto all'attività dell'ente?
   · Metodologia di valutazione del rischio coerente con PNA?
   · Aree a rischio generali e specifiche correttamente identificate?

3. Misure di prevenzione
   · Per ogni misura obbligatoria (PNA): prevista nel PTPCT sì/no,
     responsabile indicato, termine, indicatore di monitoraggio

4. Obblighi di trasparenza (D.Lgs. 33/2013)
   · Sezione "Amministrazione Trasparente": verifica presenza sottosezioni obbligatorie
   · Aggiornamento dati: frequenza rispettata?
   · Responsabile della Trasparenza nominato e indicato?

5. RPCT
   · Nominato con atto formale? Posizione in struttura (indipendenza adeguata)?
   · Relazione annuale pubblicata?

6. Gap e raccomandazioni
   · Misure assenti o incomplete
   · Riferimento PNA o delibera ANAC applicabile
   · Priorità di intervento

SCALA: Obbligatorio-Mancante · Obbligatorio-Presente · Raccomandato-Mancante
```

---

## 8. Tabelle di output trasversali

### Tabella A — Quadro normativo dell'atto

| Norma citata | Tipo | Vigente all'emanazione | Applicazione corretta | Note |
|---|---|---|---|---|
| | Legge / Decreto / Circolare | Sì / No / Da verificare | Sì / No / Parziale | |

### Tabella B — Vizi di legittimità

| Vizio | Categoria | Presente | Gravità | Norma violata | Fonte doc. |
|---|---|---|---|---|---|
| | Forma / Sostanza / Competenza | Sì / No / Dubbio | Nullo / Annullabile / Irregolare | | |

### Tabella C — Scadenzario procedimentale

| Fase | Termine (giorni) | Data avvio | Scadenza | Eseguita | Conseguenza del ritardo |
|---|---|---|---|---|---|

### Tabella D — Semaforo rischi (sintesi finale)

| Area | Rischio | Descrizione sintetica | Azione raccomandata | Priorità |
|---|---|---|---|---|
| | 🔴 Alto / 🟡 Medio / 🟢 Basso | | | Alta / Media / Bassa |

---

## 9. Roadmap implementativa

### Fase 1 — Core PA (priorità immediata)

I 4 workflow più richiesti, coprono il 70% dei casi:

- `pa-delibera` — atti degli organi collegiali
- `pa-241-check` — conformità procedimentale
- `pa-appalto-review` — contratti pubblici
- `pa-ptpct` — anticorruzione

### Fase 2 — Appalti avanzati

Blocco completo D.Lgs. 36/2023:

- `pa-rup-checklist`, `pa-bando`, `pa-collaudo`, `pa-variante`

### Fase 3 — PNRR e fondi europei

Dominio ad alta specializzazione, da sviluppare con corpus EUR-Lex già indicizzato:

- `pa-pnrr-milestone`, `pa-rendiconto`, `pa-audit`, `pa-irregolarita`

---

## 10. Riferimenti normativi

| Norma | Oggetto | Rilevanza |
|---|---|---|
| L. 241/1990 e s.m.i. | Procedimento amministrativo | Blocco 3 — trasversale |
| D.Lgs. 267/2000 (TUEL) | Ordinamento enti locali | Blocco 1 |
| D.Lgs. 36/2023 | Codice dei contratti pubblici | Blocco 2 |
| D.Lgs. 33/2013 | Trasparenza PA | Blocco 5 |
| D.Lgs. 190/2012 | Prevenzione corruzione | Blocco 5 |
| Reg. UE 2021/241 | Dispositivo RRF (PNRR) | Blocco 4 |
| D.Lgs. 104/2010 (c.p.a.) | Codice processo amministrativo | Blocco 1-3 |
| PNA ANAC vigente | Piano Nazionale Anticorruzione | Blocco 5 |
| D.Lgs. 231/2002 | Ritardi di pagamento | Blocco 2 |

---

*Documento per Cove Studio — Dominio PA Italiana v1.0*