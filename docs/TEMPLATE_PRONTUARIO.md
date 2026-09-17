# Prontuario dei Template Professionali Italiani
### Analisi completa di layout, strutture e automazione
*Versione 1.0 — Maggio 2026*

---

## Nota metodologica

Questo documento raccoglie e sistematizza i template professionali più diffusi in Italia,
con tre obiettivi distinti ma complementari:

1. **Documentare** i layout (margini, font, interlinea) così come si trovano nella prassi
   consolidata e, dove esistono, nelle norme
2. **Strutturare** i template in modo che un collega possa configurarli in Word senza
   ambiguità
3. **Classificare** ogni template per potenziale di automazione, così da identificare dove
   investire nella creazione di sistemi documentali

Lo spirito è quello del *Prontuario* di Giovanna Panucci: il contenuto si genera dialogando
con l'AI, iterando, affinando. Il template entra in gioco alla fine, quando il contenuto
è pronto e va trasformato in un documento stampabile e professionale.

> *"Il Prontuario non serve per generare il contenuto. Quello si ottiene dialogando con
> Claude, iterando, affinando. Il Prontuario entra in gioco alla fine, quando il contenuto
> è pronto e devi trasformarlo in un documento stampabile."*
> — Giovanna Panucci, Gladiatori Digitali

Per la traduzione tecnica nel codice Cove Studio: questo documento è la **specifica
autoritativa** per i sidecar `config/docx-templates/<domain>/<slug>.json` e per i template
Word `.dotx` che li accompagnano. Ogni scheda qui sotto corrisponde a un template
shipped, e ogni `source_reference` nei sidecar punta alla scheda specifica.

---

## Parte I — Standard trasversali

Questi parametri si applicano come base di partenza a tutti i template della categoria
professionale/legale italiana, salvo indicazione contraria nelle schede singole.

### 1.1 Parametri di pagina

| Parametro | Standard professionale IT | Note |
|-----------|--------------------------|------|
| Formato | A4 (21 × 29,7 cm) | Unico formato accettato in tutti i contesti |
| Margine superiore | 2,5–3 cm | 3 cm per atti giudiziari |
| Margine inferiore | 2,5 cm | 1,17 cm solo per formato uso bollo |
| Margine sinistro | 3–3,5 cm | Maggiore per fascicolazione/rilegatura |
| Margine destro | 2–2,5 cm | 5,2 cm per formato uso bollo (timbri) |
| Orientamento | Verticale (Portrait) | |
| Fronte/retro | Solo per rogiti notarili e uso bollo | |

### 1.2 Tipografia

| Parametro | Ambito legale/forense | Ambito commerciale/PA | Ambito gestionale |
|-----------|----------------------|-----------------------|-------------------|
| Font corpo | Times New Roman | Arial / Calibri | Arial / Calibri |
| Dimensione testo | 12 pt | 11 pt | 11 pt |
| Dimensione note | 10 pt | 10 pt | 9 pt |
| Dimensione tabelle | 11 pt | 10–11 pt | 9–10 pt |
| Interlinea | 1,5 | 1,15 – singola | 1,15 – singola |
| Allineamento | Giustificato | Giustificato | Giustificato |

### 1.3 Elementi comuni a tutti i template

**Intestazione (header):** presente su ogni pagina dalla seconda in poi, contiene i dati
identificativi del professionista o dell'ente e il riferimento al documento.

**Data:** sempre a destra, prima del destinatario, nel formato esteso:
`Cremona, 14 maggio 2026`

**Oggetto:** sempre in grassetto, sintetico (max una riga), con etichetta esplicita
`Oggetto:` seguita dai riferimenti all'atto o alla pratica.

**Numerazione pagine:** in basso a destra, dal formato `Pag. X di Y` (atti giudiziari) al
semplice numero (lettere brevi). Non si numera la prima pagina nelle lettere.

**Firma:** a destra, con almeno tre righe: luogo e data, nome e cognome, qualifica.
Per atti notarili e ricorsi tributari è richiesta la firma digitale.

**Allegati:** sempre elencati in fondo, numerati progressivamente.

### 1.4 Regole tipografiche da rispettare

- **Mai righe vuote** nei documenti uso bollo (rogiti notarili, scritture legali) — ogni
  riga deve essere scritta o barrata
- **Grassetto** per titoli di sezione, termini chiave nelle conclusioni e riferimenti
  normativi
- **Corsivo** per citazioni giurisprudenziali, massime, brocardi latini
- **MAIUSCOLETTO o MAIUSCOLO** per i dati identificativi delle parti negli atti notarili
- **Rientro prima riga** (0,75–1 cm) negli atti difensivi forensi; assente (blocco
  americano) nelle comunicazioni PA e commerciali
- **Spaziatura tra paragrafi** (6 pt dopo) preferibile all'uso di righe vuote, soprattutto
  nei documenti PA

---

## Parte II — Schede template

### SCHEDA 1 — CTU Medico Legale → Tribunale

**Tipologia:** Relazione peritale depositata in cancelleria o trasmessa al giudice
designante.

**Fonte normativa:** Nessun vincolo formale; prassi consolidata dei fori italiani. Alcuni
tribunali (Milano, Bologna, Rimini) hanno adottato moduli quesito standardizzati che il
CTU deve rispettare nell'ordine di risposta.

#### Specifiche tecniche

| Parametro | Valore |
|-----------|--------|
| Formato | A4 |
| Font | Times New Roman |
| Dimensione testo | 12 pt |
| Dimensione note | 10 pt |
| Interlinea corpo | 1,5 |
| Interlinea citazioni/stralci | Singola, rientro sx 1,5 cm |
| Margine sinistro | 3,5 cm |
| Margine destro | 2 cm |
| Margine sup/inf | 2,5 cm |
| Intestazione | Nome, titolo, n. iscrizione albo CTU, recapiti |
| Piè di pagina | `Pag. X di Y` centrato |
| Note a piè di pagina | Times New Roman 10 pt, interlinea singola |

#### Struttura tipo

```
[INTESTAZIONE PERITO]
Dott. ___________  –  Medico Legale
Iscritto all'Albo CTU del Tribunale di ___________  al n. ___
Via ___________, tel. ___________, PEC ___________

────────────────────────────────────────────────────────────

TRIBUNALE ORDINARIO DI ___________
Sezione _____ Civile  –  R.G. n. ___________
Giudice dott./dott.ssa ___________

RELAZIONE DI CONSULENZA TECNICA D'UFFICIO

Causa:  ___________ (attore)  c.  ___________ (convenuto)
CTU nominato:  Dott. ___________, Specialista in ___________

────────────────────────────────────────────────────────────

1. PREMESSA E INCARICO RICEVUTO
2. DOCUMENTAZIONE ESAMINATA
3. QUESITI POSTI DAL GIUDICE
4. SVOLGIMENTO DELLE OPERAZIONI PERITALI
   4.1  Data, luogo e modalità della visita
   4.2  Esame della documentazione clinica
   4.3  Indagini tecniche eseguite
5. RISPOSTA AI QUESITI
   5.1  Quesito n. 1 – [titolo]
        [Il quesito riportato testualmente in corsivo]
        Risposta: [testo]
   5.2  Quesito n. 2 – [titolo]
        …
6. CONCLUSIONI
7. LIQUIDAZIONE DELLE COMPETENZE

                                   [Luogo], [Data]
                                   Il CTU
                                   [firma]

ALLEGATI
1. ___________
2. ___________
```

#### Campi variabili per automazione

`[TRIBUNALE]` `[SEZIONE]` `[RG]` `[GIUDICE]` `[ATTORE]` `[CONVENUTO]`
`[NOME_CTU]` `[SPECIALIZZAZIONE]` `[DATA_VISITA]` `[PERIZIANDO]`

#### Note pratiche

I titoli di sezione sono in grassetto maiuscolo numerati progressivamente. I quesiti del
giudice vengono riportati testualmente in corsivo prima della risposta. I referti della
visita vanno in rientro o box separato con interlinea singola per distinguerli dal
commento.

---

### SCHEDA 2 — Avvocato → Tribunale (Atto difensivo)

**Tipologia:** Comparsa di risposta, memoria ex art. 183 c.p.c., atto di citazione,
comparsa conclusionale.

**Fonte normativa:** Protocollo CNF/Cassazione 2015; D.M. 110/2023 e Protocollo
Ministero Giustizia n. 3761/2023 (sinteticità e chiarezza). I limiti di caratteri sono
vincolanti: 80.000 caratteri per atti introduttivi, 50.000 per memorie, 10.000 per note
udienza. La violazione non produce invalidità ma può essere valutata dal giudice.

#### Specifiche tecniche

| Parametro | Valore |
|-----------|--------|
| Formato | A4 |
| Font | Times New Roman (tradizionale) |
| Dimensione testo | 12 pt |
| Dimensione note | 10 pt |
| Interlinea | 1,5 |
| Margine sinistro | 3,5 cm |
| Margine destro | 2 cm |
| Margine sup/inf | 2,5–3 cm |
| Rientro prima riga | 0,75–1 cm |
| Intestazione | Studio legale, avvocato, C.F./P.IVA, PEC |
| Piè di pagina | Nome atto + R.G. + numero pagina |
| Indice navigabile | Obbligatorio; titoli come link interni (aumenta il compenso) |
| Limite caratteri | 80.000 atti introduttivi / 50.000 memorie / 10.000 note |

#### Note pratiche

Il separatore `* * *` o una linea tipografica tra le sezioni è convenzione diffusa nei
fori del Nord Italia. Le massime giurisprudenziali si citano in corsivo con rientro
1,5 cm. Le conclusioni sono numerate, con rientro a sinistra per gli elenchi subordinati.
L'indice navigabile (sommario Word con link interni) è fortemente consigliato e aumenta
il compenso liquidabile.

---

### SCHEDA 3 — Ente Pubblico → Tribunale / altri enti

**Tipologia:** Diffida istituzionale, autorizzazione, comunicazione ufficiale, delibera
trasmessa, richiesta di informazioni.

| Parametro | Valore |
|-----------|--------|
| Formato | A4 |
| Font | Arial 11 pt o Calibri 11 pt |
| Interlinea | Singola + 6 pt dopo paragrafo |
| Margini | 2,5 cm su tutti i lati |
| Intestazione | Logo ente + denominazione + indirizzo + PEC + codice IPA |
| Piè di pagina | `Prot. n. ___ del ___` + numero pagina |
| Oggetto | In grassetto, con etichetta `Oggetto:` |
| Classificazione | Codice titolario archivistico (es. VII/3) |
| Trasmissione | Via PEC obbligatoria verso altri enti e tribunali |

---

### SCHEDA 4 — Risposta ad Agenzia delle Entrate

**Tipologia:** Risposta ad avviso bonario, risposta a questionario, istanza di autotutela,
istanza di rimborso, risposta a invito al contraddittorio.

| Parametro | Valore |
|-----------|--------|
| Formato | A4 |
| Font | Arial 11 pt |
| Interlinea | Singola |
| Margini | 2,5 cm su tutti i lati |
| Allineamento | Blocco americano (tutto a sinistra, senza rientri) |
| Intestazione | Studio/professionista, P.IVA, PEC |
| Oggetto | Riferimento atto AdE + C.F. contribuente + anno imposta |

---

### SCHEDA 5 — Commercialista → Cliente / Terzi / Enti

**Tipologia:** Perizia di stima, parere professionale, relazione accompagnatoria al
bilancio, attestazione di conformità, dichiarazione sostitutiva.

| Parametro | Valore |
|-----------|--------|
| Formato | A4 |
| Font | Calibri o Arial (lettere) / Times New Roman 12 pt (perizie formali) |
| Dimensione testo | 11–12 pt |
| Interlinea | 1,15–1,5 |
| Margine sinistro | 3 cm per relazioni/perizie; 2,5 cm per lettere |
| Margine destro e inf/sup | 2–2,5 cm |
| Intestazione | Studio, iscrizione OdCEC, n. albo, P.IVA, PEC |
| Piè di pagina | Nome studio + pagina (documenti multipagina) |

---

### SCHEDA 6 — Notaio — Atto Notarile (Rogito / Istrumento Pubblico)

**Tipologia:** Compravendita immobiliare, costituzione di società, donazione, testamento,
procura speciale, verbale di assemblea societaria, mutuo ipotecario.

**Fonte normativa:** Legge Notarile n. 89/1913 (artt. 47–72) per i requisiti di
contenuto. Il formato uso bollo è una consuetudine giuridica consolidata, non un obbligo
assoluto nella forma digitale, ma ancora ampiamente rispettata.

#### Specifiche tecniche — Formato Uso Bollo

| Parametro | Valore |
|-----------|--------|
| Formato | A4 fronte/retro (4 facciate = 1 foglio uso bollo) |
| Font | Arial 12 pt (moderno) / Courier New 12 pt (classico) |
| Interlinea | **Esatta 28,35 pt** (25 righe per facciata, 100 righe per foglio) |
| Margine superiore | 2,7 cm |
| Margine inferiore | 1,17 cm |
| Margine sinistro | 2,8 cm |
| Margine destro | **5,2 cm** (spazio per marche da bollo e timbri) |
| Allineamento | Giustificato |
| Impostazione pagine | **Affiancate (specchio)** – i margini si alternano pari/dispari |
| Righe vuote | **Vietate** – ogni riga deve contenere testo |
| Sottoscrizione | In margine di ogni foglio (eccetto l'ultimo) da tutte le parti |

#### Note pratiche

I dati identificativi delle parti (nome, cognome, data/luogo di nascita, C.F., residenza)
vanno in MAIUSCOLETTO o grassetto per facilitare il controllo in cancelleria. I dati
catastali hanno una sezione dedicata quasi tabulare. Le dichiarazioni urbanistiche
(art. 46 D.P.R. 380/2001) e la conformità catastale sono sempre presenti per immobili.

---

### SCHEDA 7 — Template ad alto volume e alta automazione

#### 7a — Diffida / Messa in Mora ★★★★★

**Tipologia:** Lettera inviata per raccomandata A/R o PEC, preludio all'azione giudiziaria.

| Parametro | Valore |
|-----------|--------|
| Font | Arial o Calibri 11 pt |
| Interlinea | 1,15 |
| Margini | 2,5 cm su tutti i lati |
| Oggetto | Grassetto + parola DIFFIDA in maiuscolo |
| Termine | In grassetto corsivo |
| Lunghezza | 1 pagina (raramente 2) |

**Campi variabili chiave:** `[DEBITORE]` `[CF_PIVA]` `[INDIRIZZO]` `[PEC]`
`[DESCRIZIONE_INADEMPIMENTO]` `[IMPORTO]` `[TERMINE_GG]` `[SCADENZA_DATA]`
`[ALLEGATO_PROVA]`

#### 7b — Contratto di Locazione ★★★★★

**Tipologia:** Locazione abitativa (L. 431/1998): canone libero 4+4, canone concordato
3+2, contratto transitorio, uso foresteria. Locazione commerciale (artt. 27–42 L.
392/1978).

| Parametro | Valore |
|-----------|--------|
| Font | Times New Roman o Arial 11 pt |
| Interlinea | 1,5 |
| Margini | 2,5–3 cm sx; 2,5 cm altri lati |
| Articoli | In grassetto, numerati |
| Parti | In MAIUSCOLETTO nell'intestazione |

**Campi variabili chiave:** `[LOCATORE]` `[CF_LOCATORE]` `[CONDUTTORE]`
`[CF_CONDUTTORE]` `[INDIRIZZO_IMMOBILE]` `[DATI_CATASTALI]` `[CANONE_MENSILE]`
`[DURATA]` `[DATA_INIZIO]` `[DEPOSITO]` `[TIPO_CONTRATTO]` `[AGGIORNAMENTO_ISTAT]`

#### 7c — Verbale di Assemblea

**Tipologia:** Assemblea ordinaria/straordinaria di S.r.l., S.p.A., condominio,
associazione, cooperativa. Può richiedere forma notarile per delibere straordinarie di
SpA.

#### 7d — Istanza / Ricorso Amministrativo verso PA

**Tipologia:** Accesso agli atti (L. 241/90), FOIA, autotutela, istanza di rimborso verso
enti locali, ricorso a commissioni edilizie, SUAP, ecc.

#### 7e — Parcella / Notula Professionale ★★★★★

**Tipologia:** Pre-fattura emessa da avvocato, commercialista, CTU, perito, notaio per
riepilogare le prestazioni prima della fattura o come rendiconto analitico autonomo.

| Parametro | Valore |
|-----------|--------|
| Font | Arial o Calibri 11 pt |
| Interlinea | 1,15 |
| Margini | 2,5 cm su tutti i lati |
| Tabella importi | Bordi solo orizzontali (stile pulito) |
| Totale | In grassetto |
| Lunghezza | 1 pagina preferibilmente |

**Campi variabili chiave:** `[PROFESSIONISTA]` `[CLIENTE]` `[DESCRIZIONE_INCARICO]`
`[PERIODO_DA]` `[PERIODO_A]` `[VOCI_ONORARIO]` `[SPESE]`
`[CONTRIBUTO_INTEGRATIVO]` `[IMPONIBILE]` `[IVA]` `[TOTALE]` `[IBAN]`

---

### SCHEDA 8 — Procedura ISO 9001 / 14001 / 45001 (SGI) ★★★★★

**Tipologia:** Procedura di Sistema di Gestione Integrato per Qualità, Ambiente e
Sicurezza sul Lavoro.

**Fonte normativa:** UNI EN ISO 9001:2015 / 14001:2015 / 45001:2018. Nessun vincolo
normativo sulla forma del documento; le specifiche seguono le best practice dei sistemi
di gestione integrati.

#### Specifiche tecniche

| Parametro | Valore |
|-----------|--------|
| Formato | A4 |
| Font | Arial o Calibri |
| Dimensione testo | 11 pt |
| Dimensione tabelle e note | 9 pt |
| Interlinea | 1,15 o singola |
| Margini | 2,5 cm su tutti i lati |
| Allineamento | Giustificato |
| Numerazione pagine | In basso a destra |
| Numerazione sezioni | Gerarchica: `1.` / `1.1` / `1.1.1` |
| Intestazione | Logo aziendale · Codice procedura · N. revisione · Data |
| Piè di pagina | `[AZIENDA] – SGI – Procedura [CODICE] | Pagina X di Y` |

#### Note pratiche per l'automazione

Il codice procedura e il numero di revisione nell'intestazione si gestiscono con le
proprietà documento di Word (`{ DOCPROPERTY }`). La scheda processo della sezione 4.2.2
ha sempre gli stessi 15 campi e cambia solo il contenuto: è il blocco più adatto a un
sistema di generazione via form → Word o mail merge. Non ci sono vincoli normativi sulla
formattazione: è possibile brandizzare il template con colori e logo aziendali.

---

### SCHEDA 9 — Ricorso Tributario (Corte di Giustizia Tributaria)

**Tipologia:** Ricorso alla Corte di Giustizia Tributaria di primo grado avverso atti
dell'Agenzia delle Entrate, Agenzia delle Dogane, enti locali.

**Fonte normativa:** D.Lgs. 546/1992 come modificato dal D.Lgs. 220/2023. Il D.Lgs.
175/2024 (Testo Unico Giustizia Tributaria) è rinviato al 1° gennaio 2027 dal
Milleproroghe (D.L. 200/2025, art. 4).

#### Aggiornamenti normativi critici

| Novità | Impatto pratico sul documento |
|--------|-------------------------------|
| Rinomina in "Corte di Giustizia Tributaria" | Aggiornare obbligatoriamente l'intestazione |
| Reclamo-mediazione abolito (ricorsi dal 4/1/2024) | Termine deposito: **30 giorni** dalla notifica |
| Obbligo sinteticità art. 17-ter (dal 2/9/2024) | Motivi in sezioni **separate e numerate** |
| Contraddittorio preventivo obbligatorio art. 6-bis Statuto | Sezione dedicata se applicabile |
| Autotutela impugnabile (artt. 10-quater / 10-quinquies Statuto) | Nuovo tipo di atto |
| Processo telematico completo | Notifica via PEC, firma digitale, deposito PTT |

#### Note pratiche per l'automazione

Il campo `[TIPO_ATTO_IMPUGNATO]` deve essere una selezione multipla che genera
formulazioni diverse nelle conclusioni:

| Tipo atto | Formulazione automatica nelle conclusioni |
|-----------|------------------------------------------|
| Avviso di accertamento | "annullare l'avviso di accertamento n. ___ relativo al periodo ___" |
| Cartella di pagamento | "annullare la cartella di pagamento n. ___ e sospenderne l'esecuzione" |
| Diniego rimborso | "accertare il diritto al rimborso di Euro ___ e condannare l'Ufficio al pagamento" |
| Rifiuto autotutela obbligatoria | "accertare l'illegittimità del rifiuto ex art. 10-quater Statuto" |
| Rifiuto autotutela facoltativa | "accertare l'illegittimità del rifiuto ex art. 10-quinquies Statuto" |

---

## Parte III — Guida alla configurazione in Word

### 3.1 Impostare il template base in Word

Per ciascun template, la configurazione in Word segue questi passaggi nell'ordine:

**1. Impostazione pagina** (`Layout` → `Imposta pagina`)
- Formato: A4
- Margini: inserire i valori della scheda
- Per i template notarili: attivare `Pagine affiancate`

**2. Stili di carattere** (`Home` → `Stili`)
Definire almeno quattro stili personalizzati e salvarli nel template:

| Nome stile (italiano) | ID canonico (codice) | Font | Dimensione | Formattazione | Spaziatura dopo |
|------------|------|------------|---------------|-----------------|----|
| Corpo testo | `body_text` | (vedi scheda) | (vedi scheda) | Normale | 6 pt |
| Titolo sezione | `section_heading` | (vedi scheda) | +2 pt | Grassetto MAIUSCOLO | 12 pt prima, 6 pt dopo |
| Citazione | `citation` | (vedi scheda) | -1 pt | Corsivo, rientro 1,5 cm sx | 6 pt |
| Note piè pagina | `footnote` | (vedi scheda) | 10 pt | Normale | 3 pt |

L'ID canonico (colonna a destra) è quello usato dal renderer Cove Studio nelle chiavi
JSON; il nome italiano è quello scritto dentro lo styles.xml del `.dotx`.

**3. Intestazione e piè di pagina** (`Inserisci` → `Intestazione`)
- Attivare `Diversa per la prima pagina`
- Inserire i campi variabili come `{ DOCPROPERTY NomeProprietà }` per i dati che cambiano
- Per la numerazione: `{ PAGE } di { NUMPAGES }`

**4. Campi variabili** (`Inserisci` → `Parti rapide` → `Campo`)
I dati che cambiano per ogni documento devono essere campi Word, non testo fisso.
Sintassi adottata da Cove Studio: segnaposto `[NOME_CAMPO]` per mail merge / sostituzione
runtime.

**5. Salvare come `.dotx`** (`File` → `Salva con nome` → `Modello Word`)
Il file `.dotx` garantisce che ogni nuovo documento parta dal template senza modificare
il modello originale.

### 3.2 Campi variabili universali

Presenti in quasi tutti i template — formano l'`universal_metadata` ereditato in
automatico:

| Campo | Descrizione | Esempio |
|-------|-------------|---------|
| `[LUOGO]` | Città del professionista | Cremona |
| `[DATA]` | Data del documento | 14 maggio 2026 |
| `[MITTENTE]` | Nome/Studio | Avv. Mario Rossi |
| `[CF_MITTENTE]` | Codice fiscale mittente | RSSMRA70A01F205X |
| `[PIVA_MITTENTE]` | P.IVA studio | 01234567890 |
| `[PEC_MITTENTE]` | PEC del mittente | avv.rossi@pec.it |
| `[DESTINATARIO]` | Nome/ente destinatario | Tribunale di Cremona |
| `[INDIRIZZO_DEST]` | Indirizzo destinatario | Via ___ n. ___ |
| `[PEC_DEST]` | PEC destinatario | tribunale.cr@pec.giustizia.it |
| `[OGGETTO]` | Oggetto del documento | Ricorso avverso avviso n. ___ |
| `[RIF_PRATICA]` | Numero pratica interno | 2026/042 |

### 3.3 Livelli di automazione

| Livello | Descrizione | Adatti |
|---|---|---|
| **L1** — Mail merge semplice | Un file dati → N documenti identici nella struttura | Diffida, Parcella, Comunicazione PA, Istanza |
| **L2** — Rami condizionali | Il tipo di dato selezionato cambia sezioni del documento | Locazione (4+4/3+2/transitorio), Ricorso tributario, Verbale (ord./straord.) |
| **L3** — Blocchi ripetibili | Una sezione si moltiplica per N elementi | Procedura ISO (scheda processo), CTU (quesiti), Atto difensivo (motivi), Verbale (delibere), Parcella (voci) |
| **L4** — Integrazione gestionale | I dati arrivano da un database o CRM | Tutti i template con anagrafica parti, dati catastali per i rogiti, importi |

---

## Parte IV — Tabella riepilogativa completa

| # | Template | Vincolo normativo | Font | pt | Interlinea | Margine sx | Automazione | Priorità |
|---|----------|-------------------|------|----|------------|------------|-------------|----------|
| 1 | CTU Medico Legale | Nessuno (prassi foro) | Times NR | 12 | 1,5 | 3,5 cm | L2–L3 | ★★★ |
| 2 | Atto difensivo avvocato | Protocollo CNF/Cassazione + D.M. 110/2023 | Times NR | 12 | 1,5 | 3,5 cm | L1 | ★★ |
| 3 | Comunicazione PA | Nessuno (prassi) | Arial/Calibri | 11 | Singola | 2,5 cm | L1 | ★★★★ |
| 4 | Risposta Agenzia Entrate | Nessuno (prassi) | Arial | 11 | Singola | 2,5 cm | L2 | ★★★ |
| 5 | Relazione commercialista | Nessuno (prassi) | Calibri/Arial | 11–12 | 1,15–1,5 | 3 cm | L2 | ★★★ |
| 6 | Rogito notarile | L. 89/1913 | Arial/Courier | 12 | Esatta 28,35 pt | 2,8 cm | L4 | ★★★★ |
| 7a | Diffida/messa in mora | Nessuno | Arial/Calibri | 11 | 1,15 | 2,5 cm | **L1** | ★★★★★ |
| 7b | Contratto di locazione | L. 431/1998 / L. 392/1978 | Times NR/Arial | 11 | 1,5 | 3 cm | **L2** | ★★★★★ |
| 7c | Verbale di assemblea | C.c. / Statuto / L.N. | Arial/Calibri | 11 | 1,15 | 2,5 cm | L2–L3 | ★★★★ |
| 7d | Istanza/ricorso PA | L. 241/90 | Arial/Calibri | 11 | 1,15 | 2,5 cm | L1 | ★★★★ |
| 7e | Parcella professionale | Nessuno | Arial/Calibri | 11 | 1,15 | 2,5 cm | **L1–L3** | ★★★★★ |
| 8 | Procedura ISO 9001/14001/45001 | Nessuno (best practice) | Arial/Calibri | 11 | 1,15 | 2,5 cm | **L3–L4** | ★★★★★ |
| 9 | Ricorso tributario | D.Lgs. 546/1992 + 220/2023 | Times NR | 12 | 1,5 | 2,5 cm | L2–L3 | ★★★ |

---

*Documento a cura di analisi comparativa delle prassi forensi, notarili, commerciali e
della pubblica amministrazione italiana. Aggiornato a maggio 2026.*
*Fonti: prassi consolidata dei fori italiani, Protocollo CNF/Cassazione 2015,
D.M. 110/2023, D.Lgs. 546/1992 e 220/2023, UNI EN ISO 9001/14001/45001,
L. 89/1913, L. 431/1998, L. 241/1990.*
*Integrato con le schede 4 e 5 del Prontuario della Formattazione AI
di Giovanna Panucci – Gladiatori Digitali (2026).*
