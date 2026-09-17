# Cove Studio v0.8.0 — MikeRust cambia nome

> **MikeRust si chiama ora Cove Studio.** Stesso manutentore (Dario
> Finardi), gestione che passa direttamente alla persona fisica, nuovo
> repository: [github.com/dariofinardi/CoveStudio](https://github.com/dariofinardi/CoveStudio).
> L'installer sostituisce in luogo un'installazione MikeRust esistente e
> **i dati vengono migrati automaticamente al primo avvio**: non serve
> esportare né reinstallare nulla.

Oltre al nome, questa versione affronta i tre problemi che si vedevano
di più nell'uso reale: i documenti troppo lunghi per il modello, le
schede di download che a volte non comparivano, e le citazioni dei
documenti generati che non si aprivano.

---

## 🏷️ Rinomina e migrazione

* Nome prodotto, identificativo desktop (`app.covestudio`), eseguibile,
  log, user agent e prefisso delle variabili d'ambiente (`COVE_`).
* Gli archivi di progetto si scrivono come `.coveprj`; i vecchi
  `.mikeprj` restano importabili.
* Al primo avvio vengono spostati da soli: cartella dati
  (`mikerust-data` → `cove-studio-data`), database (`mike.db` →
  `cove-studio.db`, con i file `-wal`/`-shm`), cache dei modelli di
  embedding e PII, preferenze d'interfaccia. Nessun download aggiuntivo.
* Le variabili `MRUST_*` continuano a essere lette.
* I modelli Ollama installati con i nomi precedenti vengono copiati sui
  nuovi nomi e le impostazioni che li usavano vengono aggiornate; la
  rimozione dei vecchi nomi avviene **solo dopo conferma** in
  Impostazioni.

## 📄 Documenti lunghi

* **Finestra di contesto rilevata per modello.** Per i server Ollama
  locali il valore arriva dal server stesso (modello caricato, `num_ctx`
  del Modelfile, massimo del modello); per gli altri dal catalogo. Se il
  server rifiuta una richiesta per superamento, il limite reale viene
  appreso dall'errore. In Impostazioni, sotto ogni selettore di modello,
  compare la finestra disponibile e da dove è stata ricavata.
* **Allegati oltre il budget non vengono più troncati alla cieca**: ogni
  documento riceve una quota proporzionale alla sua dimensione e, se la
  supera, viene ridotto ai passi pertinenti alla domanda (ranking BM25).
  La risposta dichiara quali documenti sono stati ridotti.
* **`read_document` legge per pagine** (`page_from`/`page_to`, con
  totali e punto in cui riprendere): l'assistente può percorrere un
  documento lungo invece di perderne la coda.
* Aggiunti al catalogo **Gemini 3.7 Flash** e **Gemini 3.8 Flash**
  (1.048.576 token in ingresso, 65.536 in uscita).

## 🩹 Documenti Word — correzioni

* Un documento **generato durante la conversazione** ora riceve
  un'etichetta citabile: le sue citazioni si aprono sul documento reale.
  Prima l'etichetta restava non risolta e il visualizzatore segnalava
  «sorgente rimossa» su un documento che invece esisteva.
* **`edit_document` produce la scheda di download**: dopo una modifica
  il documento è raggiungibile senza rigenerarlo. Un documento
  modificato più volte nello stesso turno mostra una sola scheda.
* Se il modello incolla nella risposta il **JSON interno di un tool**
  (`{"doc_id":…}`), quel blocco viene rimosso a valle: la scheda di
  download porta già quell'informazione. Un JSON che l'utente ha chiesto
  davvero resta intatto. Riguarda soprattutto i modelli locali piccoli.
* `find_in_document` non può più interrompersi su testi con caratteri
  multibyte.

## 🔒 Dipendenze e sicurezza

* **SheetJS** aggiornato alla 0.20.3 e incluso nel repository: su npm la
  distribuzione è ferma alla 0.18.5, che porta due vulnerabilità note
  (prototype pollution e ReDoS) senza alcuna versione npm a cui
  aggiornare.
* Dipendenze npm aggiornate (postcss, js-yaml, undici, brace-expansion,
  dompurify, vitest, svelte, vite, typescript): `pnpm audit` non segnala
  più vulnerabilità. Lato Rust `serde_with` 3.20 → 3.22.
* Resta aperta una segnalazione su `thrift` 0.17, che arriva da `parquet`
  53: `parquet` abbandona thrift solo dalla 59, un aggiornamento
  maggiore dell'intero stack arrow, pianificato a parte.
* Gli artefatti Windows sono firmati con Azure Trusted Signing.

## 🧱 Indipendenza dal codice originale

* Le istruzioni base dell'assistente sono state **riscritte da zero** in
  italiano (con la regola «rispondi nella lingua dell'utente» in testa) e
  spostate in `config/system-prompts/base.md`.
* README, NOTICE e `docs/UPSTREAM_SYNC.md` dichiarano con precisione cosa
  è lavoro originale e cosa deriva ancora dal codice iniziale di Will
  Chen (schemi dei tool built-in, preset del dominio legale, alcune
  stringhe d'interfaccia). La licenza resta AGPL-3.0-only.

---

## Installazione

| Architettura | File |
|---|---|
| Windows x86_64 | `CoveStudio_0.8.0_x64_en-US.msi` |
| Windows ARM64 (Snapdragon X Elite) | `CoveStudio_0.8.0_arm64_en-US.msi` |

L'installer sostituisce MikeRust mantenendo i dati. Dopo il primo avvio
la cartella `mikerust-data` non esiste più: è diventata
`cove-studio-data`.
