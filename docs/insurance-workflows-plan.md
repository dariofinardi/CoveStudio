# Piano Workflow Assicurativi — Cove Studio

## Obiettivo

Aggiungere workflow built-in di tipo `tabular` per il dominio `insurance`,
pensati per confrontare polizze tra diversi assicuratori.

---

## Fase 1 — Workflow da creare subito

| ID | Titolo | Practice | Tipo | Colonne totali |
|---|---|---|---|---|
| `builtin-insurance-rcp` | RC Professionale Review | Others — Insurance | `tabular` | 24 |
| `builtin-insurance-rcd` | RC Prodotti Review | Others — Insurance | `tabular` | 24 |
| `builtin-insurance-dno` | D&O Review | Others — Insurance | `tabular` | 24 |

---

## Fase 2 — Workflow da creare in seguito

| ID | Titolo | Priorità | Motivo |
|---|---|---|---|
| `builtin-insurance-cyber` | Cyber Insurance Review | Alta | Mercato in forte crescita; struttura first party + third party molto diversa dalle RC tradizionali |
| `builtin-insurance-rcg` | RC Generale Review | Alta | Copertura base per qualsiasi impresa; frequente negli allegati assicurativi di due diligence M&A |
| `builtin-insurance-property` | Property / All Risks Review | Media | Confronto polizze danni a beni aziendali; colonne proprie (valori assicurati, coassicurazione, business interruption) |
| `builtin-insurance-rcmed` | RC Medica Review | Media | Verticale specifico; struttura simile a RC Professionale ma con colonne dedicate (colpa grave, struttura vs singolo professionista) |
| `builtin-insurance-dd` | Due Diligence Assicurativa | Media | Workflow trasversale su portafoglio polizze di una società target in M&A; output: gap analysis e rischi non coperti |
| `builtin-insurance-keyman` | Polizza Vita / Key Man Review | Bassa | Utile in ambito corporate per verificare coperture su figure chiave |

---

## Blocco A — Colonne comuni a tutti i workflow (16 colonne)

Presenti in tutti i workflow, nello stesso ordine (index 0–15).

| Index | Nome | Format | Note |
|---|---|---|---|
| 0 | Assicuratore | `text` | Nome completo della compagnia e paese di sede |
| 1 | Contraente / Assicurato | `text` | Chi stipula e chi è coperto; distinguere se diversi |
| 2 | Data decorrenza | `date` | Inizio copertura |
| 3 | Data scadenza | `date` | Fine copertura |
| 4 | Premio annuo | `monetary_amount` | Premio lordo; indicare se rateizzato e con quale frequenza |
| 5 | Massimale per sinistro | `monetary_amount` | Limite di indennizzo per singolo evento/sinistro |
| 6 | Massimale aggregato annuo | `monetary_amount` | Tetto totale per anno di polizza |
| 7 | Franchigia / SIR | `monetary_amount` | Importo a carico dell'assicurato per sinistro |
| 8 | Tipo franchigia | `tag` (assoluta / relativa / SIR) | Come si applica la franchigia |
| 9 | Territorialità | `text` | Ambito geografico della copertura; esclusioni geografiche rilevanti |
| 10 | Condizioni di rinnovo | `text` | Tacito rinnovo, preavviso di disdetta, eventuali variazioni di premio al rinnovo |
| 11 | Recesso infrannuale | `text` | Possibilità di recedere prima della scadenza; effetti economici (rimborso pro-rata, penali) |
| 12 | Liquidazione sinistri | `text` | Claims handling interno vs esterno; termini massimi di liquidazione indicati in polizza |
| 13 | Subrogazione | `yes_no` | L'assicuratore subentra nei diritti dell'assicurato verso terzi responsabili? Note su eventuali limitazioni |
| 14 | Legge applicabile | `text` | Governing law della polizza |
| 15 | Foro competente / ADR | `text` | Tribunale competente o meccanismo arbitrale/mediazione |

---

## Blocco B1 — Colonne specifiche RC Professionale (8 colonne, index 16–23)

| Index | Nome | Format | Note |
|---|---|---|---|
| 16 | Attività assicurata | `text` | Professioni e attività coperte; esclusioni di attività |
| 17 | Trigger di copertura | `tag` (claims-made / loss-occurrence / mixed) | Come scatta la copertura |
| 18 | Retroattività | `text` | Data retroattiva o "full prior acts"; "Non previsto" se assente |
| 19 | Ultrattività / run-off | `text` | Durata e condizioni della copertura post-scadenza per sinistri pregressi |
| 20 | Estensione a collaboratori | `yes_no` | Dipendenti, praticanti, collaboratori inclusi; eventuale sub-limit per collaboratore |
| 21 | Copertura spese legali | `text` | Within limits vs outside limits; advance of defence costs |
| 22 | Copertura sanzioni disciplinari | `yes_no` | Sanzioni di ordini professionali o authority coperte; eventuali esclusioni |
| 23 | Esclusioni principali | `bulleted_list` | Dolo, colpa grave, attività non dichiarate, sanzioni penali, ecc. |

---

## Blocco B2 — Colonne specifiche RC Prodotti (8 colonne, index 16–23)

| Index | Nome | Format | Note |
|---|---|---|---|
| 16 | Prodotti assicurati | `text` | Categorie merceologiche coperte; esclusioni di prodotto |
| 17 | Copertura ritiro prodotti | `yes_no` | Product recall incluso o escluso |
| 18 | Massimale recall | `monetary_amount` | Limite specifico per costi di ritiro; "Non previsto" se assente |
| 19 | Danni a prodotti incorporati | `yes_no` | Cover su prodotti finiti che incorporano il prodotto assicurato |
| 20 | Esclusione difetti di progettazione | `yes_no` | Design defect escluso dalla copertura? |
| 21 | Copertura USA / Canada | `yes_no` | Territorio nordamericano incluso o espressamente escluso |
| 22 | Copertura completed operations | `yes_no` | Danni manifestatisi dopo la consegna o l'installazione coperti |
| 23 | Esclusioni principali | `bulleted_list` | Dolo, ritiri già avviati, prodotti difettosi noti, danni ambientali, ecc. |

---

## Blocco B3 — Colonne specifiche D&O (8 colonne, index 16–23)

| Index | Nome | Format | Note |
|---|---|---|---|
| 16 | Side A | `yes_no` | Copertura personale degli amministratori non indennizzati dalla società |
| 17 | Side B | `yes_no` | Rimborso alla società per indennizzo corrisposto agli amministratori |
| 18 | Side C / Entity coverage | `yes_no` | Copertura diretta della società per securities claims |
| 19 | Run-off / copertura post-mandato | `text` | Durata del run-off; condizioni di attivazione |
| 20 | Esclusione condotte fraudolente | `text` | Formulazione; presenza della "final adjudication" clause |
| 21 | Esclusione profitto illecito | `text` | Formulazione; presenza della "final adjudication" clause |
| 22 | Spese di difesa | `text` | Advance of defence costs; within limits vs outside limits; obbligo di rimborso se escluso |
| 23 | Esclusioni principali | `bulleted_list` | Insider trading, inquinamento, sanzioni, condotte dolose, shadow directors, ecc. |

---

## Struttura `prompt_md` (postura per ogni workflow)

Ogni workflow avrà un `prompt_md` breve con questo schema:

```
## Obiettivo
Stai analizzando una polizza [tipo] per confrontarla con altre polizze
dello stesso tipo. Per ogni colonna estrai l'informazione richiesta
dalla polizza allegata.

## Stile
- Risposte concise; cita il numero di articolo o clausola tra parentesi.
- Se la polizza non tratta una colonna, rispondi "Non previsto" — non inventare.
- Per importi, indica sempre la valuta.
- Per date, usa il formato ISO (YYYY-MM-DD).
```

---

## Prossimi passi

- [x] **Step 1** — Revisione del piano: colonne aggiornate, fase 2 documentata
- [ ] **Step 2** — Scrivere il prompt di ogni colonna (seguendo lo stile dei built-in legali: preciso, con istruzione sul formato atteso e su cosa scrivere se l'informazione è assente)
- [ ] **Step 3** — Scrivere il `prompt_md` definitivo per ciascun workflow
- [ ] **Step 4** — Generare il TypeScript completo da incollare in `builtinWorkflows.ts`
- [ ] **Step 5** — Test su 2–3 polizze reali; iterare sui prompt delle colonne
- [ ] **Step 6** — Creare i workflow di fase 2 (Cyber e RC Generale come priorità alta)

---

## Note implementative

- Il campo `domain` sarà `"insurance"` per tutti (il dominio è già presente nell'enum di Cove Studio).
- Il campo `practice` sarà `"Others — Insurance"` in attesa di aggiungere practice specifiche in `practices.ts`.
- Gli ID seguono la convenzione `builtin-` dei workflow esistenti.
- Il `prompt_md` di postura è breve perché il lavoro pesante è nei prompt delle singole colonne — coerente con lo stile dei built-in legali tabular (es. `builtin-nda`, `builtin-spa`).
- Le colonne totali sono 24 per tutti e tre i workflow di fase 1: 16 comuni (Blocco A) + 8 specifiche (Blocco B).
