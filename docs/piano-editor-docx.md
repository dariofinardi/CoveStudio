# Editor .docx in Cove Studio — piano

**Stato**: in lavorazione dal 23/09/2026 (sessione notturna autonoma).
**Origine**: `QuoteDOCX` del repository privato `dariofinardi/keelops-Plugin`,
in produzione dal 22/09/2026, che a sua volta «deriva dall'analisi fatta per
MikeRust». Il codice è dell'autore, che ne ha autorizzato esplicitamente la
ripresa, la modifica e l'integrazione senza vincoli.

---

## 1. Che problema risolve

Oggi Cove Studio **scrive** documenti Word ma non li **riapre**. Genera un
`.docx` da Markdown più un modello descritto in JSON (`src/docx/`, ~2000
righe), e sa modificarne uno solo per sostituzione di testo dentro i run
(`apply_text_edits`): niente struttura, niente tabelle, niente stili, e la
stringa da cercare deve comparire letteralmente.

Chi lavora con documenti veri ha bisogno dell'altra direzione: prendere il
modello `.docx` dello studio o dell'azienda — con la sua carta intestata, i
suoi stili, le sue tabelle — scriverci dentro, rileggerlo, confrontare due
versioni. Un modello aziendale non si può ricostruire da uno schema JSON:
va **letto e restituito**.

## 2. Cosa si prende da QuoteDOCX

Quel plugin (Node + React) ha già risolto il problema difficile. Le idee che
valgono, in ordine di importanza:

**Gli opachi.** Ciò che il modello dati non rappresenta non viene perso: il
frammento OOXML originale si conserva nel documento (`opaque[id] = { kind,
xml, rels }`) e il generatore lo rimette dov'era. È questo che permette a un
modello aziendale ricco di sopravvivere a un editor che ne capisce metà —
senza, ogni salvataggio impoverirebbe il documento.

**Il modello di documento** (`MODELLO-DATI.md §2`): un albero JSON
versionato — pagina, stili, numerazioni, sezioni con intestazioni e piè di
pagina, blocchi (paragrafo, tabella, immagine, interruzione, opaco), inline
(testo, nota, campo, immagine, opaco), note a piè di pagina, campi,
commenti, revisioni. È la stessa forma che serve a noi.

**I segnaposto spezzati fra run.** Word spezza `{{nome}}` in più run alla
prima correzione ortografica; il parser ricompone il testo del paragrafo,
cerca i segnaposto e riscrive i run attorno. Senza questo, un modello reale
perde metà dei suoi campi.

**La disciplina delle prove**: 14 file di test che fissano il contratto
(round-trip, note, revisioni, immagini, traduzioni, confronto).

Restano fuori: le rotte HTTP e i permessi di KeelOps, le sue tabelle, il PDF
via Chromium (noi abbiamo pdfium), e l'interfaccia React — Cove Studio è
Svelte 5.

## 3. Il crate

`crates/docx-roundtrip/` — membro del workspace già esistente, e un
componente che può vivere per conto proprio (pubblicabile, riusabile in altri
progetti dell'autore).

> Il nome dice cosa fa: aprire un `.docx`, modificarlo, riscriverlo **senza
> perdere per strada ciò che non si è capito**. Se preferisci un altro nome,
> cambiarlo adesso costa una riga.

```
crates/docx-roundtrip/
  Cargo.toml
  README.md
  src/
    lib.rs          l'API pubblica e la documentazione d'ingresso
    model.rs        l'albero del documento (serde), versionato
    ooxml.rs        lettura/scrittura OOXML di basso livello (quick-xml, zip)
    parse/
      mod.rs        .docx → Document
      props.rs      proprietà di run, paragrafo, tabella, bordi
      styles.rs     styles.xml, catena basedOn risolta
      walker.rs     la visita del corpo, con la regola dell'opaco
      fields.rs     campi semplici e complessi, segnaposto fra run
    render/
      mod.rs        Document → .docx
      document_xml.rs, styles_xml.rs, package.rs
    diff.rs         confronto fra due versioni
  tests/
    roundtrip.rs    aprire → riscrivere → riaprire: il documento non cambia
    opaque.rs       ciò che non capiamo torna identico
    placeholders.rs segnaposto spezzati fra run
    fixtures/       .docx minimi generati dal crate stesso
```

**Dipendenze**: `zip`, `quick-xml`, `serde`, `serde_json`, `anyhow` — tutte
già nell'albero di Cove Studio. Nessuna dipendenza nuova.

**Differenza tecnica rispetto all'originale.** QuoteDOCX genera il `.docx`
con la libreria `docx` di npm e poi **riapre lo zip** per reinserire gli
opachi al posto di un marcatore (`%%KEELOPS-OPAQUE:id%%`), rinumerando le
relazioni. Noi scriviamo l'OOXML direttamente (lo facciamo già in
`src/docx/`), quindi l'opaco si scrive **al suo posto durante la
generazione**: niente marcatori, niente seconda passata, niente collisioni di
id da rinumerare. È il vantaggio di non avere una libreria di mezzo.

## 4. Fasi

| # | Cosa | Verifica |
|---|---|---|
| 1 | Crate, modello dati, OOXML di base | compila, test del modello |
| 2 | Parser: paragrafi, run, proprietà, stili | apre i `.docx` che generiamo noi |
| 3 | Parser: tabelle, sezioni, intestazioni, note | apre un modello reale |
| 4 | Opachi: conservazione e relazioni | round-trip byte-per-byte sui frammenti |
| 5 | Generatore: modello → `.docx` | riaperto da Word e da noi |
| 6 | Segnaposto e campi | modello con `{{…}}` spezzati |
| 7 | Confronto fra versioni | differenze su documenti noti |
| 8 | Innesto in Cove Studio | `edit_document` strutturale, non più cerca-e-sostituisci |
| 9 | Editor nell'interfaccia (Svelte) | fase a sé, da progettare |

Le fasi 1-7 sono il crate e non toccano Cove Studio: si possono fare e
verificare senza rischiare nulla di ciò che già funziona.

## 5. Punti da decidere (per te)

1. **Nome del crate**: `docx-roundtrip` è descrittivo ma non è un marchio.
2. **Licenza**: QuoteDOCX è marcato `LicenseRef-Jugaad-Commercial`; questo
   repository è **AGPL-3.0 e pubblico**. La riscrittura in Rust finirà quindi
   sotto AGPL, disponibile a chiunque. Ne hai tutti i diritti e puoi
   licenziarla anche diversamente altrove, ma è una scelta che va fatta
   sapendo cosa comporta — vedi §7.
3. **L'editor nell'interfaccia**: l'originale usa React e TipTap
   (ProseMirror). ProseMirror è indipendente dal framework e TipTap ha un
   involucro Svelte, quindi la conversione modello ↔ ProseMirror
   dell'originale resta un riferimento valido. Va deciso se l'editor vive
   nella chat (accanto al documento generato) o come pagina propria.

## 6. Come si innesta in Cove Studio

- `generate_docx` continua a funzionare com'è: il crate non lo sostituisce,
  gli affianca la lettura.
- `edit_document` diventa strutturale: oggi cerca una stringa nei run e la
  sostituisce; con il modello può cambiare un paragrafo, una cella, uno
  stile, e riscrivere il pacchetto conservando tutto il resto.
- Un documento `.docx` allegato a una chat diventa **apribile**: l'assistente
  può leggerne la struttura (non solo il testo), e l'utente correggerlo.
- Il modello JSON è anche il formato che l'editor dell'interfaccia userà.

## 7. Anomalie e rischi annotati

1. **Licenza incrociata (da confermare)**. Il codice di origine è marcato
   commerciale, la destinazione è AGPL pubblica. Ho la tua autorizzazione
   esplicita, quindi procedo; ma se un domani vuoi vendere questo componente
   con una licenza chiusa, il fatto che una sua riscrittura sia pubblicata
   sotto AGPL da te stesso non te lo impedisce (ne detieni i diritti) —
   semplicemente chiunque potrà usare *quella* versione alle condizioni AGPL.
2. **Materiale privato sul disco**. Il clone di `keelops-Plugin` sta in
   `C:\Progetti\keelops-Plugin`, **fuori** da questo repository, che è
   pubblico. Non va mai copiato dentro: vale la lezione di `rctop/`.
3. **Fedeltà del round-trip**. Un `.docx` riscritto non sarà mai identico
   byte per byte all'originale (l'ordine degli attributi, gli id delle
   relazioni, i namespace). Il contratto verificabile è più debole e va
   dichiarato: *ciò che il modello rappresenta si riapre uguale, ciò che non
   rappresenta torna con lo stesso XML*. I test misurano quello.
4. **Word è tollerante, noi no**. Word apre file che violano lo schema; se
   scriviamo un pacchetto leggermente sbagliato, Word spesso lo ripara in
   silenzio e il difetto si scopre su un'altra macchina. Le prove vanno fatte
   anche riaprendo i file con LibreOffice, che è più severo.
5. **Font**. L'originale gestisce un catalogo di font (9 MB) per l'anteprima
   fedele. Non lo portiamo in questa fase: un documento che usa font non
   installati si vedrà con i sostituti, come già accade in Word.
