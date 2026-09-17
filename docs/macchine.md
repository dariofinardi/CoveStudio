# Automazione Validazione Documentazione Tecnica — Brief di Progetto

> **Destinatario:** Collega / Redattore prompt
> **Scopo:** Documento unico di riferimento. Contiene tutto il contesto, le regole operative, i workflow, le specifiche tool e i prompt pronti all'uso per una GenAI che processerà e produrrà contenuti di conformità normativa per macchine industriali.
> **Stato:** Perimetro definito — pronto per la fase di prompt engineering
> **Normativa:** Dir. 2006/42/CE · Reg. 2023/1230 · CRA 2024/2847/UE · RED 2014/53/UE · NIS2 2022/2555 · D.Lgs. 138/2024

---

## Indice

1. [Contesto Aziendale](#1-contesto-aziendale)
2. [Obiettivo del Progetto](#2-obiettivo-del-progetto)
3. [Perimetro Normativo](#3-perimetro-normativo)
4. [Architettura dell'Ecosistema Digitale](#4-architettura-delecosistema-digitale)
5. [Classificazione Livelli Documentali](#5-classificazione-livelli-documentali)
6. [Struttura dei Workflow](#6-struttura-dei-workflow)
7. [Checklist Normative di Riferimento](#7-checklist-normative-di-riferimento)
8. [Regole di Analisi Automatica](#8-regole-di-analisi-automatica)
9. [SBOM — Specifiche Tecniche e Pipeline CVE](#9-sbom--specifiche-tecniche-e-pipeline-cve)
10. [Specifiche Tool — Function Calling / MCP](#10-specifiche-tool--function-calling--mcp)
11. [Template Excel per il Workflow di Validazione](#11-template-excel-per-il-workflow-di-validazione)
12. [Prompt Pronti per i Workflow](#12-prompt-pronti-per-i-workflow)
    - Prompt A — Classificazione documento
    - Prompt B — Checklist RES
    - Prompt C — Coerenza multi-documento
    - Prompt D — Espansione istruzioni assemblaggio
    - Prompt E — Calcolo strutturale
    - Prompt F — Audit rapido fascicolo tecnico
    - Prompt G — Scadenze archivio CE
    - Prompt H — SBOM e CVE
    - Prompt I — Audit CRA completo
    - Prompt J — Audit RED
    - Prompt K — Analisi NIS2 supply chain
    - Prompt L — Audit integrato safety + cyber
13. [Assunzioni di Progetto](#13-assunzioni-di-progetto)
14. [Linee Guida per la Scrittura dei Prompt](#14-linee-guida-per-la-scrittura-dei-prompt)
15. [Priorità di Implementazione](#15-priorità-di-implementazione)
16. [Glossario](#16-glossario)

---

## 1. Contesto Aziendale

Siamo **esperti progettisti di macchine industriali**, con specializzazione in attrezzatura per autoofficina (avvitatori, carri ponte, sollevatori, accessori di sollevamento, ecc.).

Offriamo ai clienti un **ecosistema completo** che comprende:

- **Macchine fisiche** — sia modelli legacy (in Direttiva Macchine 2006/42/CE) sia nuove versioni (in Regolamento Macchine 2023/1230)
- **Accessori di sollevamento** — fascicoli tecnici dedicati soggetti ai requisiti §4.x Dir. 2006/42/CE
- **Componenti IoT e sensori** integrati nelle macchine, con connettività e telemetria
- **Piattaforma di telemetria** su cloud Microsoft Azure
- **Software gestionale di officina** — pianificazione, magazzino, amministrazione (SaaS, on-premise o ibrido)

I clienti sono prevalentemente officine (PMI e grandi reti/flotte), potenzialmente operanti in settori rilevanti per NIS2 (trasporti, infrastrutture).

---

## 2. Obiettivo del Progetto

Automatizzare i processi di **validazione, analisi, generazione e confronto** della documentazione tecnica regolatoria tramite un set strutturato di prompt per GenAI su piattaforma **mikerust**.

**Formati supportati in input da Cove Studio:**

| Formato | Supportato |
|---------|-----------|
| PDF | ✅ |
| DOCX | ✅ |
| XLSX | ✅ |
| TXT | ✅ |
| RTF | ✅ |
| MD | ✅ |

Le **lingue di lavoro primarie** sono italiano e inglese. Le altre lingue (per manuali destinati ad altri mercati UE) sono sempre derivate da questi master.

Il RAG di Cove Studio contiene già i testi completi di tutte le normative di riferimento, le FAQ ufficiali e le guidance degli organismi notificati. I prompt non devono allegare le norme ma devono referenziarle esplicitamente per articolo e allegato.

---

## 3. Perimetro Normativo

### 3.1 Normative Applicabili

| Normativa | Ambito nel Progetto | Stato |
|-----------|---------------------|-------|
| **Direttiva Macchine 2006/42/CE** | Macchine e accessori di sollevamento legacy — validazione documentazione esistente | In vigore fino al 20/01/2027 |
| **Regolamento Macchine 2023/1230/UE** | Macchine nuove — fascicolo tecnico (Allegato IV), manuale d'uso (Allegato III), dichiarazione di conformità | Obbligatorio dal 20/01/2027 |
| **CRA — Cyber Resilience Act 2024/2847/UE** | Prodotti con elementi digitali: firmware, IoT, software gestionale — SBOM, gestione vulnerabilità, aggiornamenti sicurezza | Fase transitoria fino a fine 2027 |
| **RED — Radio Equipment Directive 2014/53/UE** | Moduli radio/wireless integrati nelle macchine (sensori IoT, interfacce wireless) | Già attivo |
| **NIS2 — Direttiva 2022/2555/UE** | Sicurezza supply chain digitale; obblighi come fornitore di sistemi connessi. Recepita in Italia con D.Lgs. 138/2024 | Già attiva |

### 3.2 Sovrapposizioni Normative

Su una stessa macchina connessa coesistono più normative contemporaneamente. I prompt devono gestire questa sovrapposizione e segnalare i punti di intersezione con il flag `[MULTI-NORMA]`.

```
Macchina connessa
├── Macchina in sé                  → Reg. Macchine 2023/1230 (o Dir. 2006/42/CE se legacy)
├── Accessorio di sollevamento      → Dir. 2006/42/CE §4.x / Reg. Macchine Allegato I §4
├── Modulo IoT / radio              → RED 2014/53/UE
├── Firmware aggiornabile da remoto → CRA 2024/2847
└── Ecosistema digitale connesso    → NIS2 2022/2555
```

### 3.3 Transizione Direttiva → Regolamento Macchine

Il Regolamento 2023/1230 entra in vigore il **20 gennaio 2027**. Il workflow di validazione deve:

- Classificare ogni fascicolo tecnico come redatto sotto Direttiva o Regolamento
- Per i fascicoli in Direttiva, segnalare se il prodotto sarà commercializzato dopo il 20/01/2027 (richiede riallineamento)
- Evidenziare le differenze rilevanti: il Regolamento introduce requisiti espliciti per cybersecurity (Art. 9), sistemi AI integrati e sostenibilità

### 3.4 Separazione NIS2 / CRA per ambito

| Ambito di validazione | Normativa applicabile |
|-----------------------|-----------------------|
| Validazione del **prodotto** | Normativa macchine, CRA, RED |
| Validazione della **supply chain e processi interni** | NIS2 |

Per i prodotti con elementi digitali connessi, il fascicolo tecnico deve includere una sezione cybersecurity che documenti: vettori di attacco identificati nella valutazione dei rischi, misure di sicurezza implementate nel firmware/software, procedura di notifica degli incidenti.

---

## 4. Architettura dell'Ecosistema Digitale

```
[Macchina fisica]
     │
     ├── Sensori IoT integrati
     │        └── Telemetria → Microsoft Azure (cloud)
     │
     ├── Firmware (aggiornabile da remoto)       → perimetro CRA
     │
     └── Interfacce di comunicazione             → perimetro RED

[Software Gestionale di Officina]
     ├── Pianificazione
     ├── Magazzino
     └── Amministrazione
         (SaaS / on-premise / ibrido)            → perimetro CRA + NIS2
```

I protocolli specifici di comunicazione sono da considerarsi variabili nei prompt — non vincolati a uno standard specifico.

---

## 5. Classificazione Livelli Documentali

Prima di qualsiasi analisi, il sistema deve classificare il documento in ingresso. La classificazione determina quale checklist applicare e quale normativa di riferimento usare.

| Livello | Tipo documento | Norma principale | Sufficiente per CE autonoma |
|---------|---------------|------------------|-----------------------------|
| **L1** | Scheda tecnica prodotto | Nessuna specifica | ❌ No |
| **L2** | Manuale d'uso e manutenzione | Dir. 2006/42/CE — Allegato I §1.7.4 / Reg. 2023/1230 — Allegato III | ❌ No (parte del fascicolo) |
| **L3** | Fascicolo tecnico completo | Dir. 2006/42/CE — Allegato VII / Reg. 2023/1230 — Allegato IV | ✅ Sì, se completo |
| **L4** | Fascicolo tecnico accessorio di sollevamento | Dir. 2006/42/CE — Allegato I §4.x / Reg. 2023/1230 — Allegato I §4 | ✅ Sì, se completo |

**Regola per i prompt:** classificare il documento prima di analizzarlo. Se il documento non è classificabile in un livello, elencare i requisiti mancanti per raggiungere il livello superiore più vicino.

---

## 6. Struttura dei Workflow

I workflow sono suddivisi in tre categorie, ognuna con un tipo di output diverso.

---

### CATEGORIA A — Workflow di Analisi

Output: **testo strutturato Markdown** con sezioni, evidenziazione gap e anomalie.

| ID | Nome Workflow | Descrizione |
|----|--------------|-------------|
| A1 | Analisi fascicolo tecnico (L3) | Verifica contenuto vs. Reg. 2023/1230 Allegato IV e Dir. 2006/42/CE Allegato VII |
| A2 | Analisi manuale d'uso e manutenzione (L2) | Verifica contenuto vs. Allegato I §1.7.4 Dir. 2006/42/CE / Allegato III Reg. 2023/1230 |
| A3 | Analisi fascicolo accessorio di sollevamento (L4) | Verifica vs. Dir. 2006/42/CE §4.x incluso calcolo strutturale e marcatura |
| A4 | Analisi documentazione cyber | Verifica vs. requisiti essenziali CRA 2024/2847 |
| A5 | Analisi documentazione radio/wireless | Verifica vs. requisiti essenziali RED 2014/53/UE |
| A6 | Analisi documentazione sicurezza supply chain | Verifica vs. obblighi NIS2 come fornitore di sistemi connessi |
| A7 | Analisi SBOM esistente | Verifica completezza e conformità CRA di una SBOM in formato XML/JSON |
| A8 | Estrazione SBOM da sorgenti | Derivazione SBOM da documenti tecnici, distinte base, sorgenti firmware |
| A9 | Interrogazione CVE su componenti SBOM | Verifica vulnerabilità via pipeline CVE (vedere §9) |
| A10 | Confronto fascicolo tecnico vs manuale d'uso | Identificazione incoerenze, omissioni, contraddizioni tra L3 e L2 |
| A11 | Confronto dichiarazione di conformità vs fascicolo | Verifica allineamento tra dichiarato e documentato |
| A12 | Coerenza multi-documento | Verifica coerenza di codici, valori, fabbricante tra N documenti dello stesso prodotto |
| A13 | Analisi valutazione dei rischi | Verifica struttura e completezza del risk assessment per fase del ciclo di vita |
| A14 | Analisi relazione di calcolo strutturale | Verifica metodo, coefficienti, coerenza tra carichi statici e dinamici (accessori sollevamento) |

---

### CATEGORIA B — Workflow Tabular (Checklist Binarie)

Output: **tabella strutturata** (Markdown o CSV, formato nativo di Cove Studio).

**Schema colonne standard per tutte le checklist B:**

| Colonna | Tipo | Descrizione |
|---------|------|-------------|
| `ID_Requisito` | string | Identificativo univoco del requisito normativo |
| `Riferimento_Normativo` | string | Articolo / Allegato / Punto esatto della norma |
| `Descrizione_Requisito` | string | Testo sintetico del requisito |
| `Presente` | enum | ✅ Sì / ❌ No / ⚠️ Parziale |
| `Sezione_Documento` | string | Dove è stato trovato nel documento analizzato |
| `Tipo_GAP` | enum | GAP_CRITICO / WARNING / INFO / — |
| `Note` | string | Osservazioni specifiche |

| ID | Nome Checklist | Normativa di Riferimento |
|----|---------------|--------------------------|
| B1 | Checklist fascicolo tecnico L3 | Dir. 2006/42/CE — Allegato VII / Reg. 2023/1230 — Allegato IV |
| B2 | Checklist manuale d'uso L2 | Dir. 2006/42/CE — Allegato I §1.7.4 / Reg. 2023/1230 — Allegato III |
| B3 | Checklist fascicolo accessorio sollevamento L4 | Dir. 2006/42/CE — Allegato I §4.x |
| B4 | Checklist RES (Requisiti Essenziali di Sicurezza) | Dir. 2006/42/CE — Allegato I / Reg. 2023/1230 — Allegato III |
| B5 | Checklist requisiti essenziali CRA | CRA 2024/2847 — Art. 13 + Allegato I |
| B6 | Checklist requisiti essenziali RED | RED 2014/53/UE — Art. 3 |
| B7 | Checklist obblighi NIS2 supply chain | NIS2 2022/2555 — Art. 21 |
| B8 | Checklist SBOM — completezza CRA | CycloneDX 1.5+ / SPDX 2.3+ spec + CRA Art. 13 |

---

### CATEGORIA C — Template Generazione DOCX

Output: **bozza documento Word (.docx)** marcata "BOZZA — richiede revisione Responsabile Tecnico/CE".

| ID | Template | Documento generato | Input sorgente |
|----|----------|--------------------|----------------|
| C1 | Genera manuale d'uso e manutenzione | Bozza manuale completo conforme §1.7.4 / Allegato III | Fascicolo tecnico L3 |
| C2 | Espandi istruzioni di assemblaggio | Step assemblaggio conformi §1.7.4 con DPI, attrezzi, verifiche | Schede montaggio (qualsiasi formato) |
| C3 | Genera dichiarazione di conformità CE | Bozza DoC | Fascicolo tecnico + output checklist B1/B4 |
| C4 | Genera valutazione del rischio (safety) | Bozza risk assessment strutturato per fase del ciclo di vita | Fascicolo tecnico / descrizione macchina |
| C5 | Genera valutazione del rischio cyber | Bozza cybersecurity risk assessment | Documentazione tecnica + SBOM |
| C6 | Genera report anomalie qualitative | Report gap e difetti con riferimenti normativi | Output di qualsiasi workflow A |
| C7 | Genera SBOM CycloneDX | File SBOM XML/JSON standard CycloneDX 1.5+ | Output workflow A8 |
| C8 | Genera registro GAP | Tabella GAP con priorità, responsabile, scadenza | Output di qualsiasi workflow A o B |

---

## 7. Checklist Normative di Riferimento

Questa sezione elenca i requisiti specifici che i workflow B devono verificare voce per voce. Ogni elemento mancante genera un record GAP con riferimento normativo esatto.

### 7.1 Checklist L2 — Manuale d'uso (Dir. 2006/42/CE Allegato I §1.7.4)

- [ ] Ragione sociale e indirizzo completo del fabbricante / mandatario
- [ ] Denominazione della macchina e numero di serie
- [ ] Anno di fabbricazione
- [ ] Dichiarazione di conformità CE o copia
- [ ] Descrizione generale della macchina
- [ ] Disegni, schemi, descrizioni per uso, manutenzione e riparazione
- [ ] Descrizione del posto/i di lavoro previsti per gli operatori
- [ ] Descrizione dell'uso previsto della macchina
- [ ] Avvertenze sull'uso non consentito (misuse prevedibile)
- [ ] Istruzioni di montaggio, installazione e collegamento
- [ ] Istruzioni di messa in servizio e uso
- [ ] Istruzioni di regolazione e manutenzione
- [ ] Istruzioni di smontaggio, smaltimento, messa fuori servizio
- [ ] DPI indicati per ogni operazione
- [ ] Caratteristiche degli utensili da usare con la macchina
- [ ] Condizioni in cui la macchina soddisfa i requisiti di stabilità
- [ ] Istruzioni per la riduzione di rumore e vibrazioni (se applicabile)

### 7.2 Checklist L3 — Fascicolo Tecnico (Dir. 2006/42/CE Allegato VII)

- [ ] Descrizione generale della macchina
- [ ] Disegno d'insieme e schemi dei circuiti di comando
- [ ] Disegni dettagliati con note di calcolo dei componenti critici per la sicurezza
- [ ] Risultati della valutazione dei rischi
- [ ] Elenco delle norme applicate (armonizzate e non)
- [ ] Relazioni tecniche o certificati di prove
- [ ] Copia delle istruzioni (manuale d'uso)
- [ ] Copia della dichiarazione CE di incorporazione (se applicabile)
- [ ] Copia della dichiarazione di conformità CE

### 7.3 Checklist L4 — Fascicolo Accessorio di Sollevamento (Dir. 2006/42/CE §4.x)

Tutti i punti di L3, più i seguenti specifici per gli accessori di sollevamento:

- [ ] Calcolo strutturale con metodo dichiarato esplicitamente
- [ ] Coefficienti di sicurezza conformi a §4.1.2.3 Dir. 2006/42/CE
- [ ] Verifica a fatica per il numero di cicli attesi nella vita utile dichiarata
- [ ] Considerazione del carico dinamico (non solo peso statico)
- [ ] Carico massimo di utilizzo coerente tra calcolo, marcatura e documentazione
- [ ] Limitazioni d'uso riportate sia nel corpo del documento sia nella marcatura fisica
- [ ] Anno di produzione nella marcatura (non solo anno marcatura CE)
- [ ] Numero di serie o lotto nella marcatura
- [ ] Procedura di scarto con soglie dimensionali misurabili e strumento di misura indicato
- [ ] Registro delle ispezioni periodiche con modello compilabile allegato

### 7.4 Checklist RES — Requisiti Essenziali di Sicurezza

Per ogni RES applicabile della Direttiva / Regolamento Macchine, verificare la presenza di:

- Dichiarazione di applicabilità (Sì / No / N/A) motivata
- Modalità di soddisfacimento documentata nel fascicolo
- Norma armonizzata applicata (se esistente e disponibile)
- Documento di riferimento nel fascicolo che attesta la conformità

**Principio whitelist obbligatorio:** un RES è conforme solo se la copertura è esplicitamente documentata nel fascicolo. Non inferire la conformità per assenza di elementi negativi. Non accettare sezioni generiche come sostituto di una valutazione puntuale per RES.

---

## 8. Regole di Analisi Automatica

Queste regole devono essere incorporate in tutti i prompt di analisi (categoria A). Rappresentano i gap più frequenti nella documentazione tecnica per macchine industriali e accessori di sollevamento.

### Regola 1 — Valori di prestazione ambigui o multipli

Se nel documento compaiono più valori per la stessa grandezza dichiarativa (es. carico massimo di utilizzo, portata, velocità) ricavati con metodi o coefficienti diversi, segnalare il conflitto. I valori possono essere entrambi corretti per scopi diversi (es. calcolo strutturale vs. marcatura) ma non devono coesistere senza distinzione esplicita e senza che sia chiaro quale valore si applica a quale contesto d'uso.

**Tipo GAP:** `GAP_CRITICO`
**Riferimento normativo:** Dir. 2006/42/CE §4.1.2.3 / Reg. 2023/1230 Allegato I §4.1.2.3

### Regola 2 — Limitazioni d'uso non riportate in marcatura

Ogni limitazione operativa critica presente nel testo del documento (es. condizioni ambientali, configurazioni non ammesse, restrizioni di utilizzo) deve essere verificata anche nella marcatura CE del prodotto. Se una limitazione è presente nel testo ma assente dalla marcatura, generare un avviso.

**Tipo GAP:** `WARNING`
**Riferimento normativo:** Dir. 2006/42/CE Allegato I §1.7.3 / Reg. 2023/1230 Allegato I §1.7.3

### Regola 3 — Scadenza dell'obbligo di archiviazione

Estrarre la data di emissione del documento e calcolare la scadenza dell'obbligo di conservazione (10 anni dalla data di cessazione della produzione, ex Dir. 2006/42/CE Art. 5). Segnalare i documenti in scadenza entro 12 mesi.

**Tipo GAP:** `ALERT`
**Riferimento normativo:** Dir. 2006/42/CE Art. 5(3)

### Regola 4 — Coerenza del soggetto dichiarante nella DoC

Verificare che il soggetto firmatario della Dichiarazione di Conformità CE coincida con il fabbricante o il mandatario indicato nel fascicolo tecnico. Qualsiasi discrepanza tra i soggetti deve essere segnalata come gap critico.

**Tipo GAP:** `GAP_CRITICO`
**Riferimento normativo:** Dir. 2006/42/CE Allegato II / Reg. 2023/1230 Allegato V

### Regola 5 — Coerenza multi-documento

Quando si analizzano più documenti relativi allo stesso prodotto, verificare la coerenza dei seguenti campi tra tutti i documenti: denominazione e codice modello del prodotto, valori di prestazione dichiarati (portata, carico, velocità, ecc.), materiali e specifiche tecniche, fabbricante / mandatario dichiarato, versione e data di revisione. Generare un record di incoerenza per ogni difformità rilevata, indicando i due documenti in conflitto, il campo, i due valori e la gravità.

**Gravità CRITICA:** incoerenze nei valori di prestazione o nella denominazione del fabbricante.
**Gravità MEDIA:** incoerenze in codici modello, date, riferimenti a norme.

### Regola 6 — Struttura della valutazione dei rischi

Verificare che il risk assessment contenga tutti i seguenti elementi obbligatori:

1. Identificazione dei pericoli per ciascuna fase del ciclo di vita (trasporto, montaggio, messa in servizio, uso, manutenzione, smontaggio, smaltimento)
2. Stima del rischio con metodo dichiarato esplicitamente (es. HRN, matrice probabilità × gravità, norma EN ISO 12100)
3. Misure adottate collegate a ciascun pericolo identificato, secondo la gerarchia: eliminazione → protezione → informazione
4. Rischio residuo con valore numerico o classe dopo l'applicazione delle misure
5. Collegamento esplicito tra le misure adottate e i RES pertinenti della Direttiva / Regolamento

Sezioni con sole regole comportamentali per l'operatore, senza struttura formale di risk assessment secondo EN ISO 12100, non sono conformi.

**Tipo GAP:** `GAP_CRITICO` se assente; `WARNING` se parzialmente strutturata.

### Regola 7 — Relazione di calcolo strutturale (accessori di sollevamento)

Per i documenti L4, verificare i seguenti punti della relazione di calcolo e restituire per ciascuno CONFORME / NON CONFORME / DA VERIFICARE con motivazione e valori numerici estratti:

1. Il metodo di calcolo è dichiarato esplicitamente (es. trave isostatica, FEM, norma di riferimento)?
2. I coefficienti di sicurezza adottati sono conformi a Dir. 2006/42/CE §4.1.2.3?
3. Il carico di calcolo è coerente con il carico massimo di utilizzo dichiarato in marcatura?
4. Il calcolo considera sia i carichi statici sia i carichi dinamici (inerzia, impatti da manovra)?
5. Le ipotesi semplificative sono dichiarate esplicitamente?
6. È presente la verifica a fatica per il numero di cicli attesi nella vita utile dichiarata?

### Regola 8 — Requisiti essenziali di sicurezza informatica CRA (prodotti con elementi digitali)

Applicabile a tutti i prodotti che integrano firmware aggiornabile, connettività, sensori IoT o software gestionale. Verificare che la documentazione contenga e dimostri conformità a tutti i seguenti elementi.

**Requisiti di sicurezza del prodotto (CRA Allegato I, Parte I):**

- [ ] Assenza di vulnerabilità note sfruttabili al momento dell'immissione sul mercato
- [ ] Configurazione sicura di default: nessuna credenziale generica o universale
- [ ] Protezione da accessi non autorizzati con meccanismi di autenticazione e controllo accessi appropriati
- [ ] Protezione della riservatezza dei dati trattati: cifratura dei dati in transito e a riposo dove rilevante
- [ ] Protezione dell'integrità dei dati trattati, dei comandi e delle configurazioni
- [ ] Minimizzazione della superficie di attacco: porte, servizi e interfacce non necessari disabilitati
- [ ] Riduzione dell'impatto di un incidente: capacità di isolamento dei componenti compromessi
- [ ] Registrazione e monitoraggio delle attività rilevanti per la sicurezza (logging)
- [ ] Aggiornamenti di sicurezza distribuibili: meccanismo di aggiornamento sicuro e autenticato
- [ ] Classificazione del prodotto dichiarata: Default / Importante / Critico

**Requisiti di gestione delle vulnerabilità (CRA Allegato I, Parte II):**

- [ ] SBOM presente, aggiornata e in formato standard (CycloneDX o SPDX)
- [ ] Politica di divulgazione coordinata delle vulnerabilità (CVD policy) documentata
- [ ] Canale di segnalazione delle vulnerabilità dichiarato e accessibile
- [ ] Politica di supporto con aggiornamenti di sicurezza per almeno 5 anni dalla commercializzazione (o per la durata del ciclo di vita se inferiore)
- [ ] Procedura di gestione e risposta agli incidenti documentata
- [ ] Impegno a non introdurre vulnerabilità intenzionali

**Tipo GAP:** `GAP_CRITICO` per requisiti Parte I assenti; `GAP_CRITICO` per SBOM assente; `WARNING` per requisiti incompleti o non documentati.
**Riferimento normativo:** CRA 2024/2847 Art. 13, Allegato I Parte I e Parte II

---

### Regola 9 — Requisiti essenziali RED (moduli radio e wireless)

Applicabile a qualsiasi componente che utilizza frequenze radio: moduli WiFi, Bluetooth, Zigbee, LoRa, interfacce cellulari (4G/5G), interfacce wireless proprietarie. Verificare la documentazione per i seguenti requisiti.

**Requisiti generali (RED Art. 3.1):**

- [ ] Il prodotto non causa interferenze dannose alle reti e ai servizi radio
- [ ] Il prodotto accetta le interferenze indesiderate inevitabili
- [ ] Il prodotto è conforme alle norme armonizzate applicabili per le frequenze utilizzate

**Requisiti di sicurezza (RED Art. 3.3 — obbligatori per prodotti connessi a internet e indossabili):**

- [ ] Protezione della rete: il prodotto non compromette la rete o i servizi a cui è connesso
- [ ] Protezione dei dati personali e della privacy degli utenti
- [ ] Protezione da frodi: meccanismi di autenticazione per l'accesso a servizi a pagamento

**Documentazione richiesta:**

- [ ] Dichiarazione di conformità RED con elenco delle frequenze e potenze utilizzate
- [ ] Elenco delle norme armonizzate EN applicate (es. EN 300 328, EN 301 489, EN 62368)
- [ ] Documentazione dell'organismo notificato (se applicabile per Art. 3.3)
- [ ] Registro delle apparecchiature radio (se notifica preventiva richiesta per bande specifiche)

**Nota sulla sovrapposizione CRA/RED:** per prodotti connessi che usano interfacce radio, entrambe le normative si applicano contemporaneamente. Segnalare con `[MULTI-NORMA: CRA + RED]`.

**Tipo GAP:** `GAP_CRITICO` per requisiti Art. 3.1 assenti (impedisce la marcatura CE); `WARNING` per requisiti Art. 3.3 incompleti.
**Riferimento normativo:** RED 2014/53/UE Art. 3.1, Art. 3.2, Art. 3.3

---

### Regola 10 — Obblighi NIS2 come fornitore di sistemi connessi

NIS2 si applica all'organizzazione, non al singolo prodotto. Tuttavia il fornitore di sistemi connessi ha obblighi documentali verso i propri clienti che possono essere soggetti NIS2 (operatori di servizi essenziali e importanti). Verificare che la documentazione destinata ai clienti contenga quanto segue.

**Misure di sicurezza della supply chain (NIS2 Art. 21.2.d e Art. 21.2.e):**

- [ ] Descrizione delle misure di sicurezza adottate nello sviluppo e manutenzione del prodotto connesso
- [ ] Politica di gestione delle vulnerabilità e dei tempi di risposta documentata e comunicata ai clienti
- [ ] Informazioni sulle dipendenze software di terze parti (SBOM o equivalente) disponibili su richiesta
- [ ] Procedura di notifica degli incidenti che coinvolgono il prodotto comunicata ai clienti

**Continuità operativa (NIS2 Art. 21.2.c):**

- [ ] Documentazione delle misure di resilienza del prodotto connesso (failsafe, degraded mode, recovery)
- [ ] Tempi di ripristino dichiarati in caso di aggiornamento fallito o incidente di sicurezza

**Sicurezza della catena di approvvigionamento (NIS2 Art. 21.2.d):**

- [ ] Dichiarazione sulle pratiche di sicurezza dei fornitori di componenti critici
- [ ] Politica di valutazione dei fornitori di software e hardware integrati nel prodotto

**Crittografia e comunicazioni sicure (NIS2 Art. 21.2.h):**

- [ ] Utilizzo di protocolli crittografici aggiornati per le comunicazioni tra prodotto e infrastruttura cloud (es. TLS 1.2+)
- [ ] Documentazione del meccanismo di autenticazione tra macchina e piattaforma telemetrica

**Nota:** per i clienti che operano in settori NIS2 essenziali o importanti (trasporti, energia, acque, infrastrutture digitali), il fornitore deve essere in grado di rispondere alle richieste di audit della supply chain. Documentare preventivamente le misure adottate riduce il rischio di non conformità a cascata.

**Tipo GAP:** `WARNING` per elementi mancanti comunicati ai clienti; `GAP_CRITICO` se il fornitore stesso rientra tra i soggetti essenziali o importanti NIS2 e non ha misure proprie documentate.
**Riferimento normativo:** NIS2 2022/2555 Art. 21, Art. 23 / D.Lgs. 138/2024

---

### Regola 11 — Coerenza tra documentazione safety e documentazione cyber

Quando un prodotto ha sia un fascicolo tecnico (L3/L4) sia documentazione cyber (CRA/RED), verificare la coerenza tra i due domini su questi punti specifici:

- La valutazione dei rischi safety (EN ISO 12100) include anche i pericoli derivanti da attacchi cyber o malfunzionamenti software (es. perdita di controllo della macchina per comando remoto non autorizzato)?
- Le misure di sicurezza informatica adottate non introducono nuovi rischi safety non valutati (es. un aggiornamento remoto che modifica parametri di sicurezza della macchina)?
- Le limitazioni d'uso safety (es. "non utilizzare in ambienti con interferenze radio") sono coerenti con le specifiche radio del modulo IoT dichiarate nella documentazione RED?
- Il fascicolo tecnico dichiara esplicitamente quali funzioni di sicurezza della macchina dipendono da software o firmware (safety-related software) e come sono gestite in caso di aggiornamento?

**Tipo GAP:** `GAP_CRITICO` se i rischi cyber con impatto safety non sono valutati nel risk assessment; `WARNING` per incoerenze tra i due domini.
**Riferimento normativo:** Reg. 2023/1230 Art. 9 / CRA 2024/2847 Considerando 34 / EN ISO 13849 (safety-related software)

---

## 9. SBOM — Specifiche Tecniche e Pipeline CVE

### 9.1 Formati Supportati

| Formato | Standard | Preferenza |
|---------|----------|-----------|
| **CycloneDX 1.5+** | ECMA / OWASP | Raccomandato da CRA — preferito per ambito industriale/IoT |
| **SPDX 2.3+** | ISO/IEC 5962 | Alternativo — più diffuso in ambito software puro |

Il sistema deve supportare entrambi come input e output, con capacità di conversione bidirezionale.

### 9.2 Scenari di Input

**Scenario A — SBOM già esistente (CycloneDX o SPDX):**

```
parse_document
→ detect_sbom_format
→ validate_document
→ validate_sbom           (conformità CRA)
→ extract_components
→ normalize_identifiers
→ get_cve                 (batch NVD query)
→ enrich_cve
→ check_patch_status
→ filter_cve
→ generate_cve_report
```

**Scenario B — SBOM da derivare da documento tecnico o distinta base:**

```
parse_document
→ detect_sbom_format      (→ risultato: "raw")
→ extract_components      (estrazione da testo non strutturato)
→ normalize_identifiers   (risoluzione CPE/purl)
→ validate_sbom           (gap CRA sulla SBOM derivata)
→ get_cve
→ enrich_cve
→ check_patch_status
→ filter_cve
→ generate_cve_report     (output_format="docx")
```

### 9.3 Database e Fonti CVE

| Fonte | Scopo |
|-------|-------|
| **NVD — NIST** | Database primario CVE con CVSS v3.1 e v4.0 |
| **EPSS** | Probabilità di exploit nei 30 giorni (0.0–1.0) |
| **CISA KEV** | Known Exploited Vulnerabilities — vulnerabilità attivamente sfruttate |

### 9.4 Schema Colonne Report CVE

| Colonna | Tipo | Descrizione |
|---------|------|-------------|
| `Componente` | string | Nome e versione del componente |
| `CPE / purl` | string | Identificatore canonico |
| `CVE_ID` | string | Es. CVE-2024-12345 |
| `CVSS_Score` | number | Score v3.1 (o v4.0 se disponibile) |
| `Severity` | string | CRITICAL / HIGH / MEDIUM / LOW |
| `EPSS` | number | Probabilità exploit 30gg (0.0–1.0) |
| `KEV` | boolean | In CISA Known Exploited Vulnerabilities |
| `Stato_Patch` | string | patched / open / not_applicable |
| `Fix_Version` | string | Prima versione che risolve la CVE |
| `Azione` | string | Remediation consigliata |
| `Riferimento` | string | URL advisory principale |

---

## 10. Specifiche Tool — Function Calling / MCP

**Modalità di invocazione:** function calling nativo (Anthropic/OpenAI style) o MCP plugin esterno. Il LLM decide autonomamente quali tool invocare e in quale ordine in base al documento ricevuto — non è necessario specificare ogni tool nel prompt.

### Architettura a 4 Layer

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

---

### `parse_document` — Layer 1

Legge e normalizza il documento sorgente in testo strutturato utilizzabile dal LLM e dai tool successivi.

```json
{
  "name": "parse_document",
  "description": "Legge un documento sorgente (PDF, DOCX, XLSX, TXT, RTF, MD) e restituisce il contenuto testuale strutturato con metadati.",
  "parameters": {
    "file_path":      { "type": "string",  "required": true },
    "extract_tables": { "type": "boolean", "default": true },
    "language_hint":  { "type": "string",  "enum": ["it","en","auto"], "default": "auto" }
  },
  "returns": {
    "content":  "string — testo estratto e normalizzato",
    "tables":   "array  — tabelle estratte come array di oggetti",
    "metadata": {
      "format":     "string  — pdf|docx|xlsx|txt|rtf|md",
      "language":   "string  — lingua rilevata",
      "page_count": "integer",
      "title":      "string|null"
    }
  }
}
```

---

### `detect_sbom_format` — Layer 1

Determina se il documento è una SBOM, il formato, la versione schema e i tipi di identificatori usati.

```json
{
  "name": "detect_sbom_format",
  "description": "Analizza un documento per determinare se è una SBOM e ne identifica formato, versione schema e tipi di identificatori componente.",
  "parameters": {
    "content":   { "type": "string" },
    "file_path": { "type": "string" }
  },
  "returns": {
    "is_sbom":          "boolean",
    "format":           "string  — cyclonedx|spdx|raw|unknown",
    "schema_version":   "string  — es. '1.5' per CycloneDX, '2.3' per SPDX",
    "identifier_types": "array<string> — cpe22|cpe23|purl|swid|hash",
    "component_count":  "integer",
    "confidence":       "number  — 0.0-1.0"
  }
}
```

---

### `validate_document` — Layer 1

Valida struttura e integrità del documento. Per SBOM CycloneDX/SPDX valida anche la conformità allo schema normativo.

```json
{
  "name": "validate_document",
  "description": "Valida struttura e integrità del documento. Per SBOM valida la conformità allo schema CycloneDX o SPDX.",
  "parameters": {
    "file_path":       { "type": "string", "required": true },
    "expected_format": { "type": "string", "enum": ["cyclonedx","spdx","auto"], "default": "auto" },
    "expected_hash": {
      "type": "object",
      "properties": {
        "algorithm": { "type": "string", "enum": ["sha256","sha512","md5"] },
        "value":     { "type": "string" }
      }
    }
  },
  "returns": {
    "valid":          "boolean",
    "schema_errors":  "array<string>",
    "hash_match":     "boolean|null — null se hash non fornito",
    "computed_hash":  "string — SHA-256 del file",
    "warnings":       "array<string>"
  }
}
```

---

### `extract_components` — Layer 2

Estrae la lista completa dei componenti dalla SBOM o da testo tecnico non strutturato.

```json
{
  "name": "extract_components",
  "description": "Estrae tutti i componenti software/firmware da una SBOM (CycloneDX, SPDX) o da testo tecnico non strutturato.",
  "parameters": {
    "content":            { "type": "string",  "required": true },
    "format":             { "type": "string",  "enum": ["cyclonedx","spdx","raw","auto"], "default": "auto" },
    "include_transitive": { "type": "boolean", "default": true }
  },
  "returns": {
    "components": [
      {
        "name":        "string",
        "version":     "string",
        "type":        "string — library|firmware|operating-system|container|device|file",
        "supplier":    "string|null",
        "cpe":         "string|null — CPE 2.3 se presente",
        "purl":        "string|null — Package URL se presente",
        "hashes":      "object|null",
        "licenses":    "array<string>",
        "is_transitive": "boolean"
      }
    ],
    "total_count":            "integer",
    "extraction_confidence":  "number — 0.0-1.0 (rilevante per estrazione da raw)"
  }
}
```

---

### `normalize_identifiers` — Layer 2

> **Nota critica:** senza CPE 2.3 canonici l'NVD restituisce zero risultati anche per componenti noti. Questo tool non deve mai essere omesso dalla pipeline.

Converte gli identificatori componente nel formato canonico CPE 2.3 e purl, risolvendo ambiguità di naming e versioning.

```json
{
  "name": "normalize_identifiers",
  "description": "Normalizza identificatori componente (CPE, purl, SWID, nomi liberi) nel formato canonico CPE 2.3 e purl per l'interrogazione NVD.",
  "parameters": {
    "components": {
      "type": "array",
      "items": {
        "name":    "string (required)",
        "version": "string",
        "cpe":     "string",
        "purl":    "string"
      }
    }
  },
  "returns": {
    "normalized": [
      {
        "original":                 "object — componente originale",
        "cpe23":                    "string|null — es. cpe:2.3:a:openssl:openssl:3.0.7:*:*:*:*:*:*:*",
        "purl":                     "string|null — es. pkg:npm/lodash@4.17.21",
        "normalization_confidence": "number",
        "normalization_notes":      "string|null"
      }
    ],
    "unresolved": "array — componenti per cui non è stato possibile produrre un identificatore canonico"
  }
}
```

---

### `validate_sbom` — Layer 2

Verifica completezza e conformità della SBOM rispetto ai requisiti CRA e agli standard CycloneDX/SPDX.

```json
{
  "name": "validate_sbom",
  "description": "Verifica completezza e conformità di una SBOM rispetto a CRA 2024/2847 (Art. 13, Allegato I) e alle specifiche CycloneDX 1.5+ / SPDX 2.3+.",
  "parameters": {
    "content":           { "type": "string", "required": true },
    "format":            { "type": "string", "enum": ["cyclonedx","spdx","auto"], "default": "auto" },
    "compliance_target": { "type": "string", "enum": ["cra","spec_only","both"], "default": "both" }
  },
  "returns": {
    "compliant": "boolean",
    "cra_gaps": [
      {
        "requirement": "string — es. 'Art. 13(1): identificazione univoca del prodotto'",
        "status":      "string — missing|incomplete|ok",
        "detail":      "string"
      }
    ],
    "spec_gaps":      "array  — gap rispetto allo schema CycloneDX/SPDX",
    "coverage_score": "number — 0.0-1.0",
    "recommendations": "array<string>"
  }
}
```

---

### `get_cve` — Layer 3 ⭐ (tool principale)

Interroga NVD in modalità batch per recuperare le CVE associate ai componenti forniti. Preferire CPE 2.3 per firmware e OS, purl per librerie software.

```json
{
  "name": "get_cve",
  "description": "Interroga NVD (NIST) in modalità batch per recuperare CVE associate ai componenti. Supporta CPE 2.3 e purl. Restituisce CVSS v3.1 e v4.0, CWE, stato.",
  "parameters": {
    "components": {
      "type": "array",
      "items": {
        "cpe23":   "string — preferito per firmware e OS",
        "purl":    "string — preferito per librerie software",
        "name":    "string (required)",
        "version": "string (required)"
      }
    },
    "cvss_min_score":              { "type": "number",  "default": 0.0 },
    "include_rejected":            { "type": "boolean", "default": false },
    "max_results_per_component":   { "type": "integer", "default": 50 }
  },
  "returns": {
    "results": [
      {
        "component_name":    "string",
        "component_version": "string",
        "cve_id":            "string",
        "description":       "string",
        "cvss_v31": { "score": "number", "severity": "string", "vector": "string" },
        "cvss_v40": { "score": "number|null", "severity": "string|null", "vector": "string|null" },
        "cwe":           "array<string>",
        "published":     "string — ISO 8601",
        "last_modified": "string — ISO 8601",
        "status":        "string — ANALYZED|AWAITING_ANALYSIS|UNDERGOING_ANALYSIS|MODIFIED|REJECTED",
        "references":    "array<string>"
      }
    ],
    "summary": {
      "total_cve": "integer",
      "critical":  "integer",
      "high":      "integer",
      "medium":    "integer",
      "low":       "integer",
      "components_with_cve": "integer",
      "components_clean":    "integer"
    },
    "api_timestamp": "string — data/ora interrogazione NVD",
    "errors":        "array  — componenti per cui la query ha fallito con motivo"
  }
}
```

---

### `filter_cve` — Layer 3

Filtra e prioritizza i risultati CVE in base a soglia CVSS, vettore di attacco e stato patch.

```json
{
  "name": "filter_cve",
  "description": "Filtra e prioritizza CVE in base a soglia CVSS, vettore di attacco, stato patch.",
  "parameters": {
    "cve_results":    "array (required) — output di get_cve campo 'results'",
    "min_cvss_score": {
      "type": "number", "default": 4.0,
      "note": "4.0 per report operativi; 0.0 per audit completo CRA"
    },
    "attack_vector":  { "type": "array", "items": "NETWORK|ADJACENT|LOCAL|PHYSICAL" },
    "exclude_status": { "type": "array", "default": ["REJECTED"] },
    "only_with_patch": { "type": "boolean", "default": false }
  },
  "returns": {
    "filtered":       "array — CVE che soddisfano i criteri",
    "excluded_count": "integer",
    "filter_summary": "object — conteggi per criterio di esclusione"
  }
}
```

---

### `enrich_cve` — Layer 3

Arricchisce i dati CVE con EPSS score, flag CISA KEV e advisory vendor ufficiali.

```json
{
  "name": "enrich_cve",
  "description": "Arricchisce CVE con EPSS score (probabilità exploit 30gg), flag CISA KEV, advisory vendor.",
  "parameters": {
    "cve_ids": { "type": "array<string>", "required": true }
  },
  "returns": {
    "enriched": [
      {
        "cve_id":           "string",
        "epss_score":       "number|null — 0.0-1.0",
        "epss_percentile":  "number|null",
        "in_cisa_kev":      "boolean",
        "kev_due_date":     "string|null",
        "vendor_advisories": "array<string>",
        "exploit_public":   "boolean"
      }
    ]
  }
}
```

---

### `check_patch_status` — Layer 3

Per ogni coppia (componente, CVE), determina se esiste una versione patchata e fornisce il percorso di remediation.

```json
{
  "name": "check_patch_status",
  "description": "Verifica per ogni (componente, CVE) se esiste una versione patchata e fornisce il percorso di remediation.",
  "parameters": {
    "items": {
      "type": "array",
      "items": {
        "component_name":    "string (required)",
        "component_version": "string (required)",
        "cpe23":             "string",
        "purl":              "string",
        "cve_id":            "string (required)"
      }
    }
  },
  "returns": {
    "patch_status": [
      {
        "component_name":     "string",
        "cve_id":             "string",
        "status":             "string — patched|open|not_applicable|unknown",
        "fixed_in_version":   "string|null",
        "remediation_action": "string",
        "workaround_available": "boolean"
      }
    ]
  }
}
```

---

### `generate_cve_report` — Layer 4

Genera il report finale aggregando tutti i dati dei layer precedenti. Produce output tabellare per i workflow B o DOCX per i workflow C.

```json
{
  "name": "generate_cve_report",
  "description": "Genera il report di vulnerabilità finale aggregando i risultati di get_cve, enrich_cve, check_patch_status. Output tabellare (Markdown/CSV) per workflow B o DOCX per workflow C.",
  "parameters": {
    "cve_data":                "object (required) — output aggregato dei layer 3",
    "sbom_metadata":           "object — nome prodotto, versione, data, produttore",
    "output_format":           { "type": "string", "enum": ["markdown_table","csv","docx","json"], "default": "markdown_table" },
    "include_clean_components": { "type": "boolean", "default": false, "note": "true per audit completo CRA" },
    "language":                { "type": "string",  "enum": ["it","en"], "default": "it" }
  },
  "returns": {
    "report_content": "string",
    "file_path":      "string|null — percorso file generato (per docx)",
    "summary": {
      "product_name":          "string",
      "report_date":           "string",
      "total_components":      "integer",
      "vulnerable_components": "integer",
      "critical_count":        "integer",
      "high_count":            "integer",
      "medium_count":          "integer",
      "low_count":             "integer",
      "patched_available":     "integer",
      "open_unpatched":        "integer"
    }
  }
}
```

---

## 11. Template Excel per il Workflow di Validazione

Il workbook **"Validazione Fascicolo Tecnico"** è strutturato in 6 fogli. Può essere generato dal workflow C8 o alimentato manualmente con i risultati delle analisi.

### Foglio 1 — Registro documenti

| Campo | Tipo | Note |
|-------|------|------|
| `ID_documento` | Stringa | Codice univoco del documento |
| `Tipo_documento` | Dropdown | L1 / L2 / L3 / L4 |
| `Prodotto` | Stringa | Nome e modello |
| `Versione` | Stringa | Rev. X.X |
| `Data_emissione` | Data | |
| `Data_scadenza_archivio` | Formula | `=Data_emissione + 3650` |
| `Fabbricante` | Stringa | |
| `Firmatario_DdC` | Stringa | Deve corrispondere al fabbricante |
| `Norma_applicabile` | Dropdown | Dir. 2006/42/CE / Reg. 2023/1230 |
| `Stato_validazione` | Dropdown | Da validare / In corso / Validato / Scaduto |
| `Note` | Testo libero | |

### Foglio 2 — Checklist RES

Una riga per ogni RES applicabile della Direttiva / Regolamento Macchine:

| `RES_ID` | `Descrizione_RES` | `Applicabile` | `Modalità_soddisfacimento` | `Norma_armonizzata` | `Documento_riferimento` | `Esito` | `GAP` |
|----------|-------------------|---------------|---------------------------|---------------------|------------------------|---------|-------|
| 1.1.2 | Principi di integrazione della sicurezza | Sì / No / NA | Testo libero | EN XXXXX | ID_documento | OK / GAP / NA | Descrizione gap se presente |

### Foglio 3 — Registro GAP

| `GAP_ID` | `Data_rilevamento` | `Documento` | `Tipo_GAP` | `Descrizione` | `Articolo_normativo` | `Priorità` | `Responsabile` | `Scadenza_chiusura` | `Stato` |
|----------|--------------------|-------------|------------|---------------|----------------------|------------|----------------|---------------------|---------|

Valori `Tipo_GAP`: GAP_CRITICO / WARNING / INFO / ALERT
Valori `Priorità`: Alta / Media / Bassa
Valori `Stato`: Aperto / In lavorazione / Chiuso / Accettato con rischio

### Foglio 4 — Matrice rischi

| `Pericolo_ID` | `Fase_ciclo_vita` | `Descrizione_pericolo` | `Probabilità_1-5` | `Gravità_1-5` | `RPN_iniziale` | `Misura_adottata` | `RES_collegato` | `Probabilità_residua` | `Gravità_residua` | `RPN_residuo` | `Accettabile` |
|---------------|-------------------|------------------------|-------------------|---------------|----------------|-------------------|-----------------|-----------------------|-------------------|---------------|---------------|

### Foglio 5 — Tracciabilità norme

| `Norma` | `Edizione` | `Tipo` | `Applicabile_a` | `Stato` | `Data_verifica` | `Note` |
|---------|------------|--------|-----------------|---------|-----------------|--------|
| Dir. 2006/42/CE | 2006 | Direttiva | Macchine legacy | In vigore fino al 20/01/2027 | | Sostituita da Reg. 2023/1230 |
| Reg. 2023/1230 | 2023 | Regolamento | Nuovi prodotti | In vigore dal 20/01/2027 | | Periodo transitorio in corso |
| CRA 2024/2847 | 2024 | Regolamento | Prodotti con elementi digitali | Transitorio fino a fine 2027 | | |

### Foglio 6 — Dashboard KPI

Formule Excel da implementare:

- Totale documenti per stato di validazione (tabella pivot)
- Percentuale GAP chiusi / aperti / in scadenza
- Documenti con scadenza archivio nei prossimi 180 giorni
- Numero di RES con GAP critici aperti
- Numero di CVE aperti per severità (CRITICAL / HIGH / MEDIUM / LOW)

---

## 12. Prompt Pronti per i Workflow

Tutti i prompt seguono la struttura: `[RUOLO] → [CONTESTO] → [ISTRUZIONI] → [VINCOLI] → [FLAG OPERATIVI] → [OUTPUT ATTESO]`.

---

### Prompt A — Classificazione documento (prerequisito per tutti i workflow)

```
Sei un esperto di conformità CE per macchine industriali ai sensi della
Direttiva 2006/42/CE e del Regolamento Macchine 2023/1230/UE.

Analizza il documento allegato e:
1. Classificalo come:
   - L1: Scheda Tecnica
   - L2: Manuale d'uso e manutenzione
   - L3: Fascicolo Tecnico completo
   - L4: Fascicolo Tecnico Accessorio di Sollevamento
2. Indica la norma applicabile (Dir. 2006/42/CE o Reg. 2023/1230) e segnala
   se il prodotto sarà commercializzato dopo il 20/01/2027
3. Elenca i 3 elementi più critici mancanti per raggiungere il livello
   superiore (se non già L3 o L4 completo)
4. Indica se il documento è sufficiente per la marcatura CE autonoma

Rispondi SOLO in JSON con questa struttura:
{
  "livello": "L1|L2|L3|L4",
  "norma_applicabile": "Dir. 2006/42/CE | Reg. 2023/1230",
  "sufficiente_CE": true|false,
  "motivazione_CE": "stringa",
  "gap_principali": ["gap1", "gap2", "gap3"],
  "alert_transizione_2027": true|false,
  "motivazione_transizione": "stringa"
}
```

---

### Prompt B — Checklist RES completa (workflow B4)

```
Sei un valutatore di conformità CE specializzato in macchine industriali.

Analizza il documento allegato e verifica la presenza dei Requisiti Essenziali
di Sicurezza (RES) applicabili ai sensi di:
[specificare: Dir. 2006/42/CE Allegato I | Reg. 2023/1230 Allegato III]

Per ogni RES applicabile restituisci una tabella con le colonne:
RES_ID | Descrizione | Presente (Sì/No/Parziale) | Dove nel documento |
Tipo_GAP | Note

Regole obbligatorie:
- Indica sempre il numero esatto di paragrafo della norma
- Approccio whitelist: un RES è conforme SOLO se la copertura è
  esplicitamente documentata nel testo
- Non inferire la conformità per assenza di elementi negativi
- Se un requisito non è esplicitamente coperto, marcarlo ASSENTE
- Classificare ogni gap come: GAP_CRITICO / WARNING / INFO
- Non raggruppare requisiti distinti in una sola riga
```

---

### Prompt C — Analisi coerenza multi-documento (workflow A12)

```
Sono stati caricati N documenti relativi allo stesso prodotto.
Verifica la coerenza tra tutti i documenti sui seguenti campi:
1. Denominazione del prodotto e codici modello
2. Valori di prestazione dichiarati (portata, carico, velocità, pressione, ecc.)
3. Materiali e specifiche tecniche
4. Fabbricante / mandatario dichiarato
5. Versione e data di revisione del documento

Per ogni incoerenza trovata genera un record con:
INCOERENZA_ID | Documento_A | Documento_B | Campo | Valore_A | Valore_B | Gravità

Classificazione della gravità:
- CRITICA: incoerenze nei valori di prestazione o nella denominazione del fabbricante
- MEDIA: incoerenze in codici modello, date, riferimenti a norme
- BASSA: differenze di formattazione o denominazioni equivalenti

Al termine indica il documento più aggiornato e completo come riferimento
master per ciascun campo in conflitto.
```

---

### Prompt D — Espansione istruzioni di assemblaggio (workflow C2)

```
Hai ricevuto una scheda di assemblaggio con istruzioni visive e testo ridotto.
Espandi ciascuno step portandolo alla conformità con §1.7.4 della
Direttiva 2006/42/CE (o Allegato III Reg. 2023/1230).

Per ogni step aggiungi:
- Numero minimo di operatori necessari
- DPI obbligatori (con riferimento alla norma EN applicabile se disponibile)
- Attrezzi e utensili necessari con specifiche
- Avvertenza di sicurezza specifica (solo se effettivamente necessaria per quel passo)
- Criterio di verifica oggettivo: come l'operatore sa che il passo
  è eseguito correttamente

Formato da rispettare per ogni step:
[N] Titolo operazione
- Operatori necessari: [numero minimo]
- DPI obbligatori: [lista con norme EN]
- Attrezzi/utensili: [lista con specifiche]
- Avvertenza di sicurezza: [testo | "nessuna avvertenza specifica"]
- Descrizione testuale: [testo]
- Verifica: [criterio oggettivo misurabile]

Non inventare dati tecnici non presenti nel documento originale.
Segnalare con [DA VERIFICARE CON UFFICIO TECNICO] le informazioni
che richiedono conferma da parte dei progettisti.
```

---

### Prompt E — Analisi relazione di calcolo strutturale (workflow A14)

```
Sei un ingegnere meccanico esperto di accessori di sollevamento ai sensi
della Direttiva 2006/42/CE §4.x.

Analizza la relazione di calcolo strutturale presente nel documento e verifica:
1. Il metodo di calcolo adottato è dichiarato esplicitamente?
2. I coefficienti di sicurezza sono conformi a Dir. 2006/42/CE §4.1.2.3?
3. Il carico di calcolo è coerente con il carico massimo di utilizzo in marcatura?
4. Il calcolo considera sia carichi statici sia carichi dinamici
   (inerzia, impatti da manovra di sollevamento)?
5. Le ipotesi semplificative sono dichiarate esplicitamente?
6. È presente la verifica a fatica per il numero di cicli attesi
   nella vita utile dichiarata?

Per ogni punto restituisci:
CONFORME / NON CONFORME / DA VERIFICARE + motivazione + valori numerici estratti

Segnala con GAP_CRITICO qualsiasi incoerenza tra i valori di carico nel calcolo,
nella marcatura e nelle istruzioni d'uso.
```

---

### Prompt F — Audit rapido fascicolo tecnico (workflow A1/A3)

```
Sei un esperto di conformità CE per macchine industriali.

Esegui un audit del fascicolo tecnico allegato rispetto a:
[specificare: Dir. 2006/42/CE Allegato VII | Reg. 2023/1230 Allegato IV]

Produci un report strutturato con:
1. SEMAFORO GENERALE:
   🟢 Conforme — nessun gap critico
   🟡 Gap minori — presenti warning ma nessun gap critico
   🔴 Gap critici — presenti uno o più GAP_CRITICO
2. Stima percentuale di completezza del fascicolo
3. Lista GAP ordinata per gravità (GAP_CRITICO → WARNING → INFO → ALERT)
   Per ogni GAP: descrizione, riferimento normativo esatto, azione correttiva
4. Sintesi degli elementi conformi
5. Prossimi passi consigliati

Applica obbligatoriamente le seguenti regole di analisi:
- Verifica la coerenza di tutti i valori di prestazione dichiarati (Regola 1)
- Verifica che le limitazioni d'uso siano presenti anche in marcatura (Regola 2)
- Estrai la data di emissione e calcola la scadenza archivio a 10 anni (Regola 3)
- Verifica il soggetto firmatario della DoC vs fabbricante nel fascicolo (Regola 4)
- Per L4: verifica la relazione di calcolo strutturale (Regola 7)
- Se il prodotto ha elementi digitali: applica Regola 8 (CRA) e Regola 11 (coerenza safety/cyber)
- Se il prodotto ha interfacce radio: applica Regola 9 (RED), segnala [MULTI-NORMA: CRA + RED]
```

---

### Prompt G — Verifica scadenze archivio CE (workflow su XLSX/CSV)

```
Hai ricevuto un registro documenti in formato XLSX o CSV.

Per ogni documento:
1. Calcola la data di scadenza dell'obbligo di archiviazione
   (data emissione + 10 anni, ex Dir. 2006/42/CE Art. 5(3))
2. Classifica in:
   - SCADUTO
   - IN SCADENZA entro 3 mesi
   - IN SCADENZA entro 6 mesi
   - IN SCADENZA entro 12 mesi
   - OK
3. Genera una tabella ordinata per urgenza decrescente
4. Per i documenti in scadenza entro 6 mesi, genera un template
   di notifica email pre-compilata con: destinatario (responsabile indicato),
   oggetto, lista documenti, scadenza, azione richiesta

Output: tabella Markdown delle scadenze + testo email per ogni documento urgente.
```

---

### Prompt H — Analisi SBOM e CVE (workflow A7 + A9)

```
Sei un esperto di cybersecurity per prodotti industriali connessi ai sensi
del CRA 2024/2847/UE.

[Scenario A — se è stata fornita una SBOM strutturata]
Analizza la SBOM allegata in formato CycloneDX o SPDX e:
1. Verifica la conformità ai requisiti CRA Art. 13 e Allegato I
2. Per ogni componente, interroga NVD per CVE attivi
3. Arricchisci i risultati con EPSS score e flag CISA KEV
4. Verifica lo stato di patch per ogni CVE trovato

[Scenario B — se è stato fornito un documento tecnico o distinta base]
Estrai i componenti software/firmware dal documento, normalizza gli
identificatori in CPE 2.3 / purl, quindi procedi come Scenario A.

Output richiesto:
- Tabella CVE con colonne: Componente | CPE/purl | CVE_ID | CVSS_Score |
  Severity | EPSS | KEV | Stato_Patch | Fix_Version | Azione | Riferimento
- Riepilogo: totale componenti, vulnerabili, CVE critici/alti/medi/bassi,
  patch disponibili, CVE aperti senza patch
- Gap CRA: elementi mancanti nella documentazione rispetto a Art. 13

Soglia minima CVSS: [specificare: 0.0 per audit completo | 4.0 per report operativo]
Lingua output: [it | en]
```

---

### Prompt I — Audit CRA completo (workflow A4 + B5)

```
Sei un esperto di conformità al Cyber Resilience Act (CRA 2024/2847/UE)
per prodotti industriali connessi.

Analizza il documento allegato e verifica la conformità ai requisiti
essenziali di sicurezza informatica del CRA.

PARTE 1 — Requisiti di sicurezza del prodotto (Allegato I, Parte I)
Per ogni requisito restituisci una riga della tabella:
Requisito_ID | Descrizione | Presente (Sì/No/Parziale) | Dove nel documento |
Tipo_GAP | Note

Requisiti da verificare (Allegato I Parte I):
- Assenza di vulnerabilità note sfruttabili
- Configurazione sicura di default (no credenziali generiche)
- Controllo accessi e autenticazione
- Riservatezza dei dati (cifratura in transito e a riposo)
- Integrità di dati, comandi e configurazioni
- Minimizzazione della superficie di attacco
- Capacità di isolamento in caso di incidente
- Logging delle attività rilevanti per la sicurezza
- Meccanismo di aggiornamento sicuro e autenticato
- Classificazione del prodotto dichiarata (Default/Importante/Critico)

PARTE 2 — Gestione delle vulnerabilità (Allegato I, Parte II)
Stessa struttura tabellare per:
- SBOM presente e in formato standard (CycloneDX o SPDX)
- CVD policy documentata e canale di segnalazione dichiarato
- Politica di supporto per almeno 5 anni
- Procedura di gestione incidenti
- Impegno a non introdurre vulnerabilità intenzionali

PARTE 3 — Gap complessivo CRA
Al termine: semaforo generale, % completezza, prossimi passi.

Applica la Regola 8 e la Regola 11 (coerenza safety/cyber).
Segnala con [MULTI-NORMA: CRA + RED] se il prodotto ha interfacce radio.
```

---

### Prompt J — Audit RED (workflow A5 + B6)

```
Sei un esperto di conformità alla Radio Equipment Directive (RED 2014/53/UE).

Analizza il documento allegato relativo a un prodotto che integra
una o più interfacce di comunicazione radio (WiFi, Bluetooth, Zigbee,
LoRa, 4G/5G, o altra interfaccia wireless).

Verifica la presenza e la conformità ai seguenti requisiti:

REQUISITI ART. 3.1 (obbligatori per tutti i prodotti radio):
1. Non interferenza con reti e servizi radio esistenti
2. Conformità alle norme armonizzate per le frequenze utilizzate
3. Elenco completo delle frequenze e potenze di emissione dichiarate

REQUISITI ART. 3.2 (sicurezza elettrica e EMC):
4. Conformità alle norme EMC applicabili
5. Conformità alle norme di sicurezza elettrica applicabili

REQUISITI ART. 3.3 (obbligatori per prodotti connessi a internet):
6. Protezione della rete da compromissione
7. Protezione dei dati personali e della privacy
8. Protezione da frodi per servizi a pagamento (se applicabile)

DOCUMENTAZIONE:
9. Dichiarazione di conformità RED con norme armonizzate EN citate
10. Identificazione dell'organismo notificato (se Art. 3.3 applicato)
11. Registro notifica preventiva per bande soggette a restrizione (se applicabile)

Per ogni punto: CONFORME / NON CONFORME / N/A / DA VERIFICARE
+ riferimento normativo esatto + eventuale azione correttiva.

Se il prodotto rientra anche nel perimetro CRA, segnalare
con [MULTI-NORMA: CRA + RED] i requisiti sovrapposti.
```

---

### Prompt K — Analisi NIS2 supply chain (workflow A6 + B7)

```
Sei un esperto di conformità alla Direttiva NIS2 (2022/2555/UE),
recepita in Italia con D.Lgs. 138/2024, con specifico riferimento
agli obblighi del fornitore di sistemi connessi verso i clienti
che operano come soggetti essenziali o importanti.

Analizza il documento allegato (policy, documentazione tecnica del
prodotto connesso, contratto di fornitura, o documentazione di sicurezza)
e verifica la presenza dei seguenti elementi:

MISURE DI SICUREZZA DELLA SUPPLY CHAIN (Art. 21.2.d e 21.2.e):
- [ ] Descrizione delle misure di sicurezza nel ciclo di sviluppo del prodotto
- [ ] Politica di gestione delle vulnerabilità e tempi di risposta comunicata
- [ ] Informazioni sulle dipendenze software di terze parti (SBOM o equivalente)
- [ ] Procedura di notifica degli incidenti verso i clienti

CONTINUITÀ OPERATIVA (Art. 21.2.c):
- [ ] Misure di resilienza del prodotto (failsafe, degraded mode, recovery)
- [ ] Tempi di ripristino dichiarati in caso di incidente o aggiornamento fallito

SICUREZZA DELLA CATENA DI APPROVVIGIONAMENTO (Art. 21.2.d):
- [ ] Dichiarazione sulle pratiche di sicurezza dei fornitori di componenti critici
- [ ] Politica di valutazione dei fornitori di software e hardware integrati

CRITTOGRAFIA E COMUNICAZIONI SICURE (Art. 21.2.h):
- [ ] Protocolli crittografici aggiornati per le comunicazioni prodotto-cloud (TLS 1.2+)
- [ ] Autenticazione documentata tra macchina e piattaforma telemetrica

Per ogni elemento: PRESENTE / ASSENTE / PARZIALE + Tipo_GAP + Note
Indica sempre l'articolo NIS2 / D.Lgs. 138/2024 pertinente.
Al termine: valutazione complessiva del livello di maturità NIS2
come fornitore (Base / Intermedio / Avanzato) con motivazione.
```

---

### Prompt L — Audit integrato safety + cyber (workflow A4 + A1/A3, Regola 11)

```
Sei un esperto di conformità normativa con competenze trasversali
su sicurezza funzionale delle macchine (Dir. 2006/42/CE / Reg. 2023/1230)
e sicurezza informatica (CRA 2024/2847/UE).

Hai ricevuto due o più documenti relativi allo stesso prodotto connesso:
il fascicolo tecnico (L3 o L4) e la documentazione cyber (CRA).

Esegui un'analisi di coerenza tra i due domini verificando:

1. RISCHI CYBER CON IMPATTO SAFETY
   La valutazione dei rischi safety (EN ISO 12100) include i seguenti scenari?
   - Perdita di controllo della macchina per comando remoto non autorizzato
   - Modifica non autorizzata di parametri di sicurezza via aggiornamento firmware
   - Malfunzionamento della macchina per corruzione dei dati di controllo
   - Indisponibilità delle funzioni di sicurezza per attacco DoS sul sistema connesso
   Se assenti: segnalare come GAP_CRITICO con riferimento a Reg. 2023/1230 Art. 9

2. MISURE CYBER CHE POSSONO INTRODURRE RISCHI SAFETY
   Le seguenti misure di sicurezza informatica sono state valutate
   per l'impatto sulla sicurezza della macchina?
   - Aggiornamenti remoti del firmware: esiste un meccanismo di rollback sicuro?
   - Autenticazione per accesso ai comandi: non introduce ritardi pericolosi?
   - Cifratura delle comunicazioni: non impatta i tempi di risposta delle funzioni safety?

3. SAFETY-RELATED SOFTWARE (Reg. 2023/1230 Art. 9)
   Il fascicolo tecnico identifica esplicitamente le funzioni di sicurezza
   della macchina che dipendono da software o firmware?
   Sono documentate le misure per mantenerle integre durante gli aggiornamenti?

4. COERENZA DELLE LIMITAZIONI D'USO
   Le limitazioni ambientali dichiarate nel fascicolo tecnico (es. condizioni
   di temperatura, umidità, interferenze elettromagnetiche) sono coerenti con
   le specifiche operative dei moduli IoT/radio dichiarate nella documentazione RED/CRA?

Per ogni punto: CONFORME / NON CONFORME / DA VERIFICARE + motivazione.
Classifica ogni gap con il tipo (GAP_CRITICO / WARNING) e il riferimento
normativo esatto (indicare sempre sia la norma safety sia quella cyber pertinente).
Segnala le aree di sovrapposizione con il flag [MULTI-NORMA].
```

---

## 13. Assunzioni di Progetto

Quando si scrivono i prompt, le seguenti condizioni sono sempre da considerarsi soddisfatte:

1. **Il RAG di Cove Studio** contiene i testi completi di tutte le normative elencate in §3, incluse FAQ ufficiali e guidance degli organismi notificati e degli organismi di normazione.

2. **I tool disponibili** includono tutti quelli descritti in §10, più tool base per lettura, scrittura e conversione di file.

3. **I formati di input** sono sempre uno o più tra: PDF, DOCX, XLSX, TXT, RTF, MD.

4. **La lingua** del documento in input è italiano o inglese. I prompt devono rispondere nella stessa lingua del documento analizzato, salvo indicazione contraria esplicita nel prompt.

5. **I workflow sono separati e autonomi** — ogni prompt funziona indipendentemente, salvo dove esplicitamente indicato (es. C7 dipende dall'output di A8; C8 dipende dall'output di qualsiasi workflow A o B).

6. **Il LLM decide autonomamente** quali tool invocare e in quale ordine — non è necessario elencarli nel prompt. È sufficiente indicare obiettivo finale e formato di output atteso.

7. **La classificazione L1/L2/L3/L4** è un prerequisito per tutti i workflow di analisi. Se non è già nota, eseguire prima il Prompt A.

---

## 14. Linee Guida per la Scrittura dei Prompt

### 14.1 Struttura Raccomandata

```
[RUOLO]
"Sei un esperto di [dominio specifico] ai sensi di [norma specifica]..."

[CONTESTO]
- Tipo di documento in input (livello L1/L2/L3/L4 se noto)
- Normativa di riferimento attiva (Dir. 2006/42/CE o Reg. 2023/1230)
- ID workflow (A/B/C + numero)
- Lingua attesa in output

[ISTRUZIONI]
- Passi da eseguire in modo ordinato e numerato
- Per workflow B: specificare esattamente le colonne della tabella di output
- Per workflow C: specificare la struttura del documento da generare
- Per workflow A: specificare quali Regole di Analisi (§8) applicare

[VINCOLI]
- Non inventare requisiti non presenti nella norma
- Non omettere requisiti anche se non trovati nel documento
- Non inferire la conformità per assenza di elementi negativi (approccio whitelist)
- Segnalare con [DA VERIFICARE] i dati mancanti, non ometterli silenziosamente

[FLAG OPERATIVI]
- norma_applicabile: Dir. 2006/42/CE | Reg. 2023/1230
- livello_documento: L1 | L2 | L3 | L4
- lingua_output: it | en
- segnala_dato_mancante: true | false
- includi_riferimento_normativo: true | false
- soglia_gravita_gap: tutti | solo_media_e_critica | solo_critica
- cvss_min_score: 0.0 | 4.0 (solo per workflow CVE)
- include_clean_components: true | false (solo per workflow CVE)

[OUTPUT ATTESO]
Formato (Markdown / tabella / JSON / DOCX), lingua, livello di dettaglio.
```

### 14.2 Regole Generali

- Fare sempre riferimento esplicito all'articolo o allegato normativo pertinente, mai a parafrasi generiche.
- Per le checklist (B): ogni riga deve corrispondere a un requisito normativo identificabile — non raggruppare requisiti distinti in un'unica voce.
- Per i DOCX (C): marcare chiaramente ogni documento generato come "BOZZA — richiede revisione da parte del Responsabile Tecnico / CE".
- Per le analisi (A): distinguere sempre tra assenza di un elemento (gap documentale) e inadeguatezza di un elemento presente (difetto qualitativo).
- Per SBOM e CVE: classificare sempre con CVSS score e indicare lo stato (patched / open / under analysis).
- Per sovrapposizioni normative: segnalare con il flag `[MULTI-NORMA]` e indicare tutte le normative coinvolte.
- Approccio whitelist per i RES: un RES è conforme solo se la copertura è esplicitamente documentata nel fascicolo tecnico.

### 14.3 Formato Output per Categoria

| Categoria | Formato output | Note |
|-----------|---------------|------|
| A — Analisi | Testo strutturato Markdown | Heading per sezione, gap evidenziati con tipo e riferimento normativo |
| B — Tabular | Tabella Markdown o CSV | Colonne fisse come da §6 — Categoria B |
| C — DOCX | Documento Word (.docx) | Struttura conforme allo standard del documento target |
| CVE Report | Tabella Markdown / CSV / DOCX | Schema colonne come da §9.4 |

---

## 15. Priorità di Implementazione

Ordine suggerito in base a urgenza normativa e utilità operativa immediata:

1. **Prompt A + B0** — Classificazione documento: prerequisito per tutti i workflow
2. **B1 + B2 + B3** — Checklist fascicolo tecnico, manuale d'uso, accessori sollevamento
3. **B4** — Checklist RES completa
4. **A1 + A2 + A3** — Analisi qualitativa dei tre livelli documentali principali
5. **C1** — Generazione bozza manuale d'uso da fascicolo tecnico
6. **C2** — Espansione istruzioni assemblaggio
7. **A12 + A13 + A14** — Coerenza multi-documento, valutazione rischi, calcolo strutturale
8. **A8 + C7** — Estrazione e generazione SBOM (obblighi CRA)
9. **A9** — Pipeline CVE completa (§9)
10. **B5 + A4** — Checklist e analisi CRA
11. **A10 + A11** — Confronto incrociato tra documenti
12. **B6 + B7** — Checklist RED e NIS2
13. **C3 + C4 + C5** — Generazione DoC, risk assessment safety e cyber
14. **C6 + C8** — Report anomalie qualitative aggregato, registro GAP

---

## 16. Glossario

| Termine | Definizione |
|---------|-------------|
| **L1 / L2 / L3 / L4** | Livelli documentali: Scheda Tecnica / Manuale d'uso / Fascicolo Tecnico / Fascicolo Accessorio Sollevamento |
| **Fascicolo Tecnico** | Documentazione tecnica completa richiesta da Dir. 2006/42/CE Allegato VII / Reg. 2023/1230 Allegato IV |
| **Manuale d'uso e manutenzione** | Documento per l'utilizzatore finale — Dir. 2006/42/CE Allegato I §1.7.4 / Reg. 2023/1230 Allegato III |
| **DoC** | Declaration of Conformity — Dichiarazione di Conformità CE |
| **RES** | Requisiti Essenziali di Sicurezza — Dir. 2006/42/CE Allegato I / Reg. 2023/1230 Allegato III |
| **GAP_CRITICO** | Gap che impedisce la conformità CE o comporta rischio diretto per la sicurezza |
| **WARNING** | Gap che richiede correzione ma non impedisce immediatamente la conformità |
| **ALERT** | Notifica di scadenza imminente o condizione da monitorare |
| **Whitelist approach** | Un RES è conforme solo se la copertura è esplicitamente documentata. Non si inferisce la conformità per assenza di elementi negativi |
| **SBOM** | Software Bill of Materials — inventario completo di tutti i componenti software/firmware |
| **CycloneDX** | Formato SBOM standard ECMA/OWASP — raccomandato dal CRA per ambito industriale/IoT |
| **SPDX** | Software Package Data Exchange — formato SBOM ISO/IEC 5962, diffuso in ambito software |
| **CPE 2.3** | Common Platform Enumeration — identificatore canonico per interrogare NVD |
| **purl** | Package URL — identificatore standard per librerie software |
| **CVE** | Common Vulnerabilities and Exposures — identificativo standard per vulnerabilità note |
| **NVD** | National Vulnerability Database — database NIST, fonte primaria per CVE lookup |
| **CVSS** | Common Vulnerability Scoring System — scala di severità vulnerabilità (v3.1 e v4.0) |
| **EPSS** | Exploit Prediction Scoring System — probabilità di exploit nei 30 giorni (0.0–1.0) |
| **CISA KEV** | Known Exploited Vulnerabilities — lista CISA di vulnerabilità attivamente sfruttate |
| **RAG** | Retrieval-Augmented Generation — knowledge base normativo integrato in Cove Studio |
| **Cove Studio** | Piattaforma GenAI per l'esecuzione dei workflow documentali |
| **Function calling** | Modalità di invocazione tool da parte del LLM (Anthropic/OpenAI style) |
| **MCP** | Model Context Protocol — protocollo per plugin tool esterni |
| **HRN** | Hazard Rating Number — metodo quantitativo di stima del rischio |
| **EN ISO 12100** | Norma di riferimento per la valutazione e riduzione dei rischi nelle macchine |
| **IoT** | Internet of Things — sensori e dispositivi connessi integrati nelle macchine |
| **RED** | Radio Equipment Directive 2014/53/UE — apparecchiature radio e wireless |
| **CRA** | Cyber Resilience Act 2024/2847/UE — prodotti con elementi digitali |
| **NIS2** | Network and Information Security Directive 2022/2555/UE — sicurezza reti e supply chain |
| **CVD policy** | Coordinated Vulnerability Disclosure — politica di divulgazione coordinata delle vulnerabilità richiesta da CRA Allegato I Parte II |
| **Safety-related software** | Software che implementa funzioni di sicurezza della macchina — soggetto a requisiti specifici del Reg. 2023/1230 Art. 9 e EN ISO 13849 |
| **TLS** | Transport Layer Security — protocollo crittografico per le comunicazioni sicure prodotto-cloud (versione minima 1.2 raccomandata) |
| **Soggetto essenziale / importante** | Classificazione NIS2 degli operatori soggetti agli obblighi della direttiva (Art. 3) — impatta gli obblighi dei fornitori nella supply chain |
| **Degraded mode** | Modalità operativa ridotta ma sicura del prodotto in caso di malfunzionamento parziale — rilevante per requisiti di resilienza NIS2 |
| **EN ISO 13849** | Norma di riferimento per parti dei sistemi di comando legate alla sicurezza (safety-related) — include requisiti per software safety |
| **DoS** | Denial of Service — attacco informatico che rende indisponibile un servizio o sistema — rilevante per valutazione rischi cyber con impatto safety |
| **Failsafe** | Comportamento sicuro del sistema in caso di guasto o perdita di comunicazione — requisito di progettazione per macchine connesse |
| **D.Lgs. 138/2024** | Recepimento italiano della NIS2 |

---

*Versione: 2.2 — Maggio 2026*
*Normativa: Dir. 2006/42/CE · Reg. 2023/1230 · CRA 2024/2847 · RED 2014/53/UE · NIS2 2022/2555 · D.Lgs. 138/2024*
*Piattaforma: Cove Studio — function calling nativo o MCP plugin*
*Database CVE: NVD NIST — nvd.nist.gov/developers/vulnerabilities*
*Standard SBOM: CycloneDX 1.5+ / SPDX 2.3+*