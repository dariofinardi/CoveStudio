# MikeRust v0.7.5 — Cross-dominio nell'editor dei workflow

Estende il meccanismo cross-dominio `also_applicable_to` — già
disponibile per i preset di sistema in v0.7.3 — anche ai workflow
**creati dall'utente**. Ora l'editor di workflow e tabellari permette
di registrare un workflow in più settori tramite un selettore a tag
con testo libero.

## Selettore «Domini aggiuntivi»

Nuovo componente a tag con autocompletamento sui domini disponibili:

* chip per ogni dominio aggiuntivo scelto (rimovibili con ×);
* campo a testo libero che filtra i domini rimanenti in un menù a
  tendina; frecce ↑↓ per scorrere, Invio per aggiungere, Backspace
  per togliere l'ultimo;
* il dominio primario è escluso dai suggerimenti;
* presente sia nella creazione (modale) sia nell'editor con
  salvataggio automatico. I preset di sistema mostrano i domini
  aggiuntivi come badge in sola lettura.

Un workflow utile a più settori (es. analisi cespiti → `fiscale` e
`finance`) si registra una volta sola e compare in tutti i picker
applicabili.

## Backend (migrazione 0034)

* Nuova colonna `also_applicable_to TEXT NOT NULL DEFAULT '[]'` sulla
  tabella `workflows`. Le righe esistenti restano a settore singolo —
  nessun cambiamento di comportamento all'aggiornamento.
* Il campo viaggia in SELECT / INSERT / UPDATE; create e update
  sanificano l'elenco lato server (solo domini canonici, scarta il
  primario ridondante, deduplica).
* Il filtro `GET /workflow?domain=` corrisponde a una riga quando il
  target è il dominio primario **oppure** uno dei domini aggiuntivi
  (spostato da SQL a Rust, come per i preset).

## Note

* `docs/WORKFLOWS.md` aggiornato: il campo `also_applicable_to` non è
  più «solo preset».
* Verifiche: `svelte-check` 0 errori · 42/42 test preset-loader ·
  `cargo check` pulito.

## Download

- `MikeRust_0.7.5_x64.msi` — Windows x86_64
- `MikeRust_0.7.5_arm64.msi` — Windows ARM64, Snapdragon X Elite

Sostituzione drop-in per v0.7.3.

## Licenza

AGPL-3.0-only. Il marchio e il logo Semplifica sono marchi registrati.
