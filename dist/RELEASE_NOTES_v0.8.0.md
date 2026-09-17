# Cove Studio v0.8.0 — MikeRust cambia nome

> **MikeRust si chiama ora Cove Studio.** Stesso manutentore (Dario
> Finardi), gestione che passa direttamente alla persona fisica, nuovo
> repository: [github.com/dariofinardi/CoveStudio](https://github.com/dariofinardi/CoveStudio).
> L'installer sostituisce in luogo un'installazione MikeRust esistente e
> **i dati vengono migrati automaticamente al primo avvio**: non serve
> esportare né reinstallare nulla.

> **Changelog cumulativo dalla 0.6.7.** L'ultimo setup pubblicato è la
> **v0.6.7**: la linea 0.7.x non è mai uscita come installer. Questa nota
> raccoglie quindi tutto il lavoro da v0.7.0 a v0.8.0, così chi aggiorna
> da 0.6.7 (o da prima) vede l'intero set di novità. Il dettaglio
> versione per versione è in [`HISTORY.md`](../HISTORY.md).

Oltre al nome, la 0.8.0 affronta i tre problemi che si vedevano di più
nell'uso reale: i documenti troppo lunghi per il modello, le schede di
download che a volte non comparivano, e le citazioni dei documenti
generati che non si aprivano. La linea 0.7.x, inclusa qui, aveva aggiunto
il settore fiscale e i workflow validi su più settori.

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

---

# Linea 0.7.x — mai pubblicata come setup

Chi aggiorna dalla 0.6.7 trova anche tutto questo.

## 🧾 Nuovo settore «Fiscale» (v0.7.0)

Dodicesimo verticale professionale, accanto a `finance`: finance copre
l'analisi (bilanci, valutazione d'azienda, crisi), fiscale la
tax-compliance e la consulenza (IVA, dichiarazioni, imposte dirette e
indirette, ravvedimento, accertamento, contenzioso).

* System prompt dedicato **in 6 lingue**, con le **riforme fiscali 2024**
  già recepite, così il modello non cita istituti superati: abrogazione
  del reclamo-mediazione (dal 4.1.2024), nuovo regime sanzionatorio
  D.Lgs. 87/2024 (omesso versamento 30% → 25%), contraddittorio
  preventivo generalizzato.
* **8 workflow preset**: parere tributario, ravvedimento operoso,
  analisi dell'avviso di accertamento, riconciliazione IVA, verifica
  forfettario, quadro RW (IVIE/IVAFE), imposte indirette su atti,
  scadenzario F24.
* **9 preset di colonna**: imponibile, aliquota, imposta dovuta,
  ritenuta, sanzione, interessi, norma, scadenza, codice tributo.

## 📊 Analisi del bilancio ai fini fiscali (v0.7.2)

Tre workflow che leggono il bilancio in chiave fiscale, non
civilistica: **analisi fiscale del bilancio** (derivazione ex art. 83
TUIR, voci tax-relevant, stima IRES, fiscalità differita OIC 25),
**riconciliazione civilistico-fiscale** in stile Quadro RF (una riga per
variazione, con articolo TUIR e rigo) e **base imponibile IRAP** (art. 5
D.Lgs. 446/97). Il settore fiscale arriva a 11 workflow.

## 🇮🇹 Settore legale in italiano (v0.7.1)

Tradotti i **27 preset** del settore legale — l'ultimo contenuto ancora
in inglese del catalogo: 14 workflow e 13 preset di colonna, in registro
giuridico italiano. I concetti sono stati **adattati** all'ordinamento
italiano dove necessario (per esempio la tutela della locazione ex
L. 392/1978, l'indicizzazione ISTAT dei canoni); i termini di mercato
sono rimasti non tradotti (SOFR, EURIBOR, carried interest…).

## 🔗 Workflow validi su più settori (v0.7.3 e v0.7.5)

Un workflow può comparire nel selettore di **più settori** senza
duplicarne la definizione. Prima solo per i preset di sistema (v0.7.3),
poi anche per i **workflow creati dall'utente** (v0.7.5), con un
selettore a etichette nell'editor. Gli aggiornamenti non cambiano il
comportamento dei workflow esistenti, che restano su un solo settore.

---

## Installazione

| Architettura | File |
|---|---|
| Windows x86_64 | `Cove Studio_0.8.0_x64.msi` |
| Windows ARM64 (Snapdragon X Elite) | `Cove Studio_0.8.0_arm64.msi` |

L'installer sostituisce MikeRust mantenendo i dati (stesso *upgrade
code*): non disinstallare la versione precedente, né esportare nulla.
Dopo il primo avvio la cartella `mikerust-data` non esiste più, è
diventata `cove-studio-data`; il database è `cove-studio.db`.

Ogni MSI include le librerie native corrispondenti all'architettura
(`onnxruntime.dll` 1.20.0 e `pdfium.dll`): non serve installare altro.

### Firma e verifica

Eseguibile e installer sono firmati con Azure Trusted Signing. Windows
mostra come editore **Jugaad srl**, che fornisce l'infrastruttura di
firma (vedi *Ringraziamenti* nel README); copyright e manutenzione del
progetto restano di Dario Finardi. Per controllare:

```powershell
Get-AuthenticodeSignature ".\Cove Studio_0.8.0_arm64.msi" |
  Select-Object Status, @{n='Publisher';e={$_.SignerCertificate.Subject}}
```

Atteso: `Status = Valid` e
`CN=Jugaad srl, O=Jugaad srl, L=Montecchio Emilia, S=Reggio Emilia, C=IT`.
La firma è marcata temporalmente (RFC 3161), quindi resta valida anche
dopo la scadenza del certificato di firma.

## Note per chi aggiorna

* **Modelli locali (Ollama).** Al primo avvio i modelli installati con i
  nomi precedenti (`mike-…`, `mikerust-…`) vengono copiati sui nuovi
  nomi e le impostazioni aggiornate. La copia non cancella nulla: la
  rimozione dei vecchi nomi va confermata in Impostazioni → Modelli.
* **Chiavi API e PIN.** Restano dov'erano: la migrazione sposta il
  database, non lo riscrive.
* **Progetti esportati.** I file `.mikeprj` continuano a essere
  importabili; i nuovi export sono `.coveprj`.
* **Variabili d'ambiente.** `MRUST_*` funziona ancora, ma il nome nuovo è
  `COVE_*`; `.env.example` è aggiornato.
* **Chunk orfani.** Se in passato hai indicizzato documenti poi spostati
  o cancellati, il log segnala «orphan KB chunk dropped»: i chunk vengono
  ignorati nella risposta e si eliminano definitivamente con
  `POST /sync/cleanup-orphans`.

## Limiti noti in questa versione

* Su alcune configurazioni Windows ARM64 il provider DirectML non si
  registra e gli embedding girano su CPU: più lento, senza differenze di
  risultato.
* `gemini-3.7-flash` può rifiutare richieste contro un limite di 32.768
  token non documentato e non coerente con `countTokens` (difetto
  segnalato a Google). Se capita, usa `gemini-3.8-flash` o
  `gemini-3.5-flash`.
* Resta aperta la vulnerabilità `thrift` 0.17 ereditata da `parquet` 53
  (allocazione eccessiva su file parquet malformati): riguarda la lettura
  di file parquet di terzi, e la correzione richiede l'aggiornamento
  maggiore dello stack arrow.

## Provenienza e licenza

Cove Studio è software libero sotto **AGPL-3.0-only**. Il lavoro
originale è © 2026 Dario Finardi; le parti che derivano ancora da
[`willchen96/mike`](https://github.com/willchen96/mike) — schemi dei tool
built-in, preset del dominio legale, alcune stringhe d'interfaccia — sono
dei rispettivi autori sotto la stessa licenza. Il dettaglio di cosa
deriva da cosa è in `README.md` (sezione *Provenance and independence
from Mike*) e in `docs/UPSTREAM_SYNC.md`.

I nomi **Cove Studio** e **MikeRust** e il logo non sono coperti dalla
licenza del codice: vedi `NOTICE.md`.
