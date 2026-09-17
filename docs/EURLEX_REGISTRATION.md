# EUR-Lex — registrazione e modalità di accesso

Cove Studio V1 supporta EUR-Lex **senza alcuna registrazione**: il connettore scarica il testo HTML direttamente dall'endpoint pubblico `https://eur-lex.europa.eu/legal-content/{LANG}/TXT/HTML/?uri=CELEX:{celex}`. Questo basta per indicizzare un atto specifico tramite il suo **codice CELEX** (es. `32016R0679` per il GDPR) in una qualsiasi delle 24 lingue UE, con fallback automatico sull'inglese se l'atto non è disponibile nella lingua scelta.

## Quando serve registrarsi

La registrazione al **EUR-Lex Web Service (CWS)** — il SOAP service ufficiale — diventa necessaria solo per V2 quando vorremo:

- **ricerca full-text** ("trova tutti gli atti UE che contengono *trattamento dati personali*")
- **filtri avanzati** per intervallo di date, settore, descrittore EuroVoc, autore istituzionale
- **export di metadati** (CELEX ↔ ELI ↔ riferimenti incrociati)
- **volumi superiori** ai ~5 000 documenti/giorno (oltre quel volume lo scraping HTML diventa fragile)

Per la sola consultazione di atti puntuali via CELEX, V1 basta e avanza.

## Come registrarsi al Web Service (CWS)

1. **Ottieni un account EU Login** se non lo hai già:  
   👉 [https://webgate.ec.europa.eu/cas/](https://webgate.ec.europa.eu/cas/)  
   La registrazione è gratuita; richiede un'email valida e una password. È lo stesso sistema usato da tutti i portali Commissione Europea.

2. **Compila il modulo di registrazione al Web Service** (autenticato con EU Login):  
   👉 [https://eur-lex.europa.eu/protected/web-service-registration.html](https://eur-lex.europa.eu/protected/web-service-registration.html)  
   Ti chiederanno:
   - finalità d'uso (ricerca legale, automazione documentale, etc.)
   - volume previsto di richieste (puoi indicare *fino a 1 000 query/giorno* — il limite tecnico è 10 000)
   - se l'uso è personale o aziendale (per uso aziendale serve indicare la ragione sociale)

3. **Approvazione manuale**: tipicamente 1-5 giorni lavorativi. Le credenziali ti arrivano via email; sono `username` + `password` separati dall'EU Login, da usare in WS-Security UsernameToken nelle chiamate SOAP.

4. **Limiti**: 10 000 query/giorno, max 100 risultati per pagina. Nessun costo.

## Endpoint principali

| Cosa | URL | Auth |
|---|---|---|
| Lookup CELEX → HTML (V1) | `https://eur-lex.europa.eu/legal-content/{LANG}/TXT/HTML/?uri=CELEX:{celex}` | nessuna |
| Lookup CELEX → PDF | `https://eur-lex.europa.eu/legal-content/{LANG}/TXT/PDF/?uri=CELEX:{celex}` | nessuna |
| Web Service SOAP (V2) | `https://eur-lex.europa.eu/EURLexWebService` (WSDL: `?wsdl`) | EU Login + creds CWS |
| SPARQL pubblico | `https://publications.europa.eu/webapi/rdf/sparql` | nessuna |
| Cellar REST | `https://publications.europa.eu/resource/celex/{celex}` | nessuna |

Il SPARQL endpoint Cellar è utile per l'arricchimento di metadati (riferimenti incrociati, descrittori EuroVoc, ELI) **senza registrazione**, e potrà essere il primo passo verso V2 prima di toccare il SOAP CWS.

## Lingue supportate

Tutte le 24 lingue UE ufficiali. Il connettore EUR-Lex le elenca automaticamente nel selettore di lingua del pannello:

`bg cs da de el en es et fi fr ga hr hu it lt lv mt nl pl pt ro sk sl sv`

Per atti adottati dopo l'adesione del rispettivo Stato membro, tutte le lingue sono di norma disponibili contestualmente. Per atti più datati o per opinioni preparatorie della Corte di giustizia, potrebbe mancare qualche lingua: in quel caso il connettore ricade automaticamente sull'inglese (se l'opzione è attiva nel pannello).

## Riferimento normativo sull'uso

L'uso e il riuso dei contenuti EUR-Lex è disciplinato dalla **Decisione 2011/833/UE** della Commissione: riuso libero, anche commerciale, **con attribuzione della fonte**. Nessuna licenza separata da accettare. Vedi:  
👉 [https://eur-lex.europa.eu/legal-content/IT/TXT/?uri=CELEX:32011D0833](https://eur-lex.europa.eu/legal-content/IT/TXT/?uri=CELEX:32011D0833)

## Roadmap

- ✅ V1 — fetch CELEX-based con HTML scraping, fallback EN, indicizzazione nel pool RAG globale dell'utente.
- 🔲 V2 — registrazione al SOAP CWS, ricerca full-text + filtri EuroVoc, paginazione risultati.
- 🔲 V2.5 — arricchimento metadati via SPARQL (ELI, riferimenti incrociati, gerarchia normativa).
- 🔲 V3 — sincronizzazione differenziale: l'utente sottoscrive una "watch list" CELEX e Cove Studio ricontrolla periodicamente le versioni consolidate.
