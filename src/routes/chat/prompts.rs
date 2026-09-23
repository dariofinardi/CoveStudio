// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

//! Text the chat sends to the model: context blocks (documents,
//! project, library, knowledge base, MCP) and service messages.
//! The base instructions live in `config/system-prompts/base.md`
//! (`crate::presets::system_prompt::base_instructions`).
//!
//! The instructions are in Italian. The structural tokens read by the
//! post-processor (`[cN]`, `[gN]`/`[pN]`, `doc-N`, `<CITATIONS>`,
//! `[[PAGE_BREAK]]`, `[Page N]`, `[Workflow: … (id: …)]`,
//! `[Template: … (id: …)]`) stay unchanged: the citation parser and
//! the normalisers depend on their exact form.

use std::fmt::Write as _;

use super::{CorpusInventoryEntry, DocPayload, McpDiscovered, RetrievedKbEntry};

/// Separator between the sections of the system prompt.
pub(super) const SECTION_SEPARATOR: &str = "\n\n---\n\n";

/// What to tell the model about a document it cannot read. Keyed on the
/// canonical codes from `crate::ingest`.
///
/// The sentence matters more than it looks: without it the model gets an
/// empty block under a filename and answers anyway, which is how a
/// confident answer about an unread document happens. With it, the model
/// can say what is true — the document is there, its content is not.
fn unreadable_note(code: &str) -> &'static str {
    use crate::ingest::outcome::{failure, no_text};
    match code {
        no_text::SCANNED_PDF => {
            "NON LEGGIBILE: è una scansione o un'immagine, senza testo selezionabile. \
             Non hai il contenuto di questo documento: dillo all'utente e suggerisci di \
             attivare l'OCR nelle impostazioni o di usare un modello che legge le immagini"
        }
        no_text::IMAGE_ONLY => {
            "NON LEGGIBILE: il documento non contiene testo, probabilmente solo immagini. \
             Non hai il suo contenuto: dillo all'utente"
        }
        no_text::EMPTY_FILE => "NON LEGGIBILE: il file è vuoto",
        no_text::IMAGE_NEEDS_VISION_MODEL => {
            "NON LEGGIBILE: è un'immagine e il modello in uso non legge le immagini. Non hai              il suo contenuto: dillo all'utente e suggerisci di scegliere un modello              multimodale nelle impostazioni"
        }
        no_text::OFFICE_NO_TEXT => {
            "NON LEGGIBILE: dal file non è stato possibile estrarre testo. Non hai il suo \
             contenuto: dillo all'utente"
        }
        failure::ENCRYPTED_PDF => {
            "NON LEGGIBILE: il PDF è protetto da password. Non hai il suo contenuto: \
             dillo all'utente e chiedi una copia senza protezione"
        }
        failure::UNSUPPORTED_FORMAT => {
            "NON LEGGIBILE: formato non supportato. Non hai il suo contenuto: dillo \
             all'utente"
        }
        failure::CORRUPT_FILE => {
            "NON LEGGIBILE: il file è danneggiato o non è del formato che l'estensione \
             dichiara. Non hai il suo contenuto: dillo all'utente"
        }
        // Includes `read_failed` and anything a future version adds:
        // the wording stays true even when the cause is unknown.
        _ => {
            "NON LEGGIBILE: la lettura del file non è riuscita. Non hai il suo contenuto: \
             dillo all'utente"
        }
    }
}

/// Block with the documents attached to the chat: full text for the
/// readable ones, a header for those sent as images, and a stated
/// verdict for those that could not be read.
/// Labels start at `doc-1` and follow the order of `doc_ids`,
/// matching the label → UUID map built by the caller.
pub(super) fn build_doc_system_prompt(docs: &[DocPayload]) -> String {
    let text_docs = docs.iter().filter(|d| d.text.is_some());
    let image_docs = docs.iter().filter(|d| !d.images.is_empty());
    let unreadable_docs = docs
        .iter()
        .filter(|d| d.text.is_none() && d.images.is_empty() && d.unreadable.is_some());
    let n_text = text_docs.clone().count();
    let n_images = image_docs.clone().count();
    if n_text == 0 && n_images == 0 && unreadable_docs.clone().next().is_none() {
        return String::new();
    }

    let capacity: usize = docs
        .iter()
        .map(|d| d.text.as_ref().map_or(0, String::len) + d.filename.len() + 48)
        .sum::<usize>()
        + 256;
    let mut s = String::with_capacity(capacity);
    s.push_str(
        "DOCUMENTI ALLEGATI — l'utente ha allegato i documenti seguenti; usali per \
         rispondere. L'etichetta doc-N serve solo negli strumenti e nel campo \
         \"doc_id\" di <CITATIONS>: nel testo indica il nome del file.\n\n",
    );
    for (i, d) in text_docs.enumerate() {
        let label = i + 1;
        match d.excerpt {
            None => {
                let _ = write!(s, "=== doc-{label} (file: {}) ===\n", d.filename);
            }
            Some(e) => {
                let unit = match e.unit {
                    crate::document_segments::SegmentUnit::Page => "pagine",
                    crate::document_segments::SegmentUnit::Part => "parti",
                };
                let _ = write!(
                    s,
                    "=== doc-{label} (file: {}) — DOCUMENTO LUNGO: qui ci sono solo gli \
                     estratti più pertinenti alla domanda ({} {unit} su {}). Per il resto \
                     usa read_document con page_from/page_to oppure find_in_document; non \
                     dare per assente ciò che non vedi negli estratti. ===\n",
                    d.filename, e.kept, e.total
                );
            }
        }
        let _ = write!(s, "{}\n\n", d.text.as_deref().unwrap_or_default());
    }
    for (i, d) in image_docs.enumerate() {
        let _ = write!(
            s,
            "=== doc-{} (file: {}, inviato come {} immagini di pagina allegate sotto) ===\n\n",
            n_text + i + 1,
            d.filename,
            d.images.len()
        );
    }
    // Documents that reached us but carry nothing usable. They keep a
    // doc-N label: the user attached them and may ask about them by
    // name, and an answer that silently ignores one of three attached
    // files is worse than one that says which it could not read.
    for (i, d) in unreadable_docs.enumerate() {
        let code = d.unreadable.as_deref().unwrap_or_default();
        let _ = write!(
            s,
            "=== doc-{} (file: {}) — {} ===\n\n",
            n_text + n_images + i + 1,
            d.filename,
            unreadable_note(code)
        );
    }
    s
}

/// Block with the project documents, readable on demand through
/// `read_document` / `find_in_document` and not loaded in full.
/// `base` shifts the numbering past the attachments to avoid collisions.
pub(super) fn build_project_docs_prompt(base: usize, docs: &[(String, String)]) -> String {
    if docs.is_empty() {
        return String::new();
    }
    let mut s = String::from(
        "DOCUMENTI DEL PROGETTO — questa chat appartiene a un progetto e i documenti \
         elencati ne fanno già parte. Sono disponibili subito: aprili per intero con \
         `read_document` o cercaci dentro con `find_in_document`, usando l'etichetta. \
         Non chiedere mai all'utente di allegarli. Se ti chiede quali documenti contiene \
         il progetto, elenca esattamente questi:\n",
    );
    for (i, (_, filename)) in docs.iter().enumerate() {
        let _ = writeln!(s, "  - doc-{} : {}", base + i + 1, filename);
    }
    s
}

/// Block with the project name and domain, so the assistant doesn't infer
/// the project name from the title of an attached document.
pub(super) fn build_project_context_prompt(name: &str, domain: &str) -> String {
    format!(
        "CONTESTO DEL PROGETTO — questa chat appartiene al progetto \"{name}\", \
         dominio professionale \"{domain}\". Quando l'utente parla del \"progetto\" \
         (nome, oggetto, perimetro) si riferisce a questo. Il nome ufficiale è \
         \"{name}\": non dedurlo mai dal nome, dal titolo o dal contenuto di un documento."
    )
}

/// Block listing the official-source documents indexed for the
/// user. It is for orientation: the actual passages arrive in the
/// knowledge base. Documents not ready yet are listed separately.
pub(super) fn build_library_inventory_prompt(entries: &[CorpusInventoryEntry]) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let (ready, pending): (Vec<&CorpusInventoryEntry>, Vec<&CorpusInventoryEntry>) =
        entries.iter().partition(|e| e.status == "ready");

    let mut s = String::with_capacity(1_600 + entries.len() * 96);
    s.push_str(
        "<LIBRERIA UTENTE — documenti di fonti ufficiali indicizzati per l'utente>\n\
         Elenco informativo. I documenti indicizzati sono consultabili: quando la domanda \
         li riguarda, i passi pertinenti compaiono nella sezione <BASE DI CONOSCENZA> con \
         i tag [g1]/[g2]/[p1]/…\n\
         \n\
         SE <BASE DI CONOSCENZA> CONTIENE TAG [gN]/[pN]:\n\
           · usali e citali secondo le regole di quella sezione; i documenti dell'utente \
             sono la fonte di riferimento.\n\
         \n\
         SE <BASE DI CONOSCENZA> È ASSENTE O NON PERTINENTE:\n\
           · la ricerca semantica non ha superato la soglia: il documento NON manca. Non \
             dire che \"non è caricato\" o \"non è interrogabile\".\n\
           · puoi rispondere con le tue conoscenze se ne sei sicuro, dichiarando che la \
             risposta non si basa sulla libreria dell'utente e suggerendo di riformulare \
             la domanda o di allegare il documento per avere citazioni.\n\
         \n\
         VALORI AMMESSI PER doc_id IN <CITATIONS>:\n\
           · solo (a) i tag [gN]/[pN] della base di conoscenza oppure (b) le etichette \
             doc-N dei file effettivamente allegati alla chat;\n\
           · mai gli identificativi di questo elenco (ad esempio \"32016R0679\" o \
             \"eurlex_32016R0679\"): sono riferimenti del corpus, non etichette di citazione;\n\
           · mai etichette doc-N inventate quando non ci sono file allegati.\n\
         \n\
         Alle domande \"che documenti hai?\" o \"conosci X?\" rispondi in base a questo \
         elenco, senza citazioni.\n\n",
    );
    if !ready.is_empty() {
        s.push_str("Indicizzati e consultabili:\n");
        for e in &ready {
            let _ = writeln!(
                s,
                "  · [{}] {}: {} ({})",
                e.corpus_id,
                e.identifier,
                e.title,
                e.language.to_uppercase()
            );
        }
    }
    if !pending.is_empty() {
        s.push_str("\nIndicizzazione in corso o interrotta (non ancora consultabili):\n");
        for e in &pending {
            let _ = writeln!(
                s,
                "  · [{}] {}: {} — {}",
                e.corpus_id, e.identifier, e.title, e.status
            );
        }
    }
    s
}

/// Block with the passages retrieved from the knowledge base for the
/// current question. It changes every turn, so it travels as a volatile
/// tail and stays out of the cacheable prefix.
pub(super) fn build_kb_system_prompt(chunks: &[RetrievedKbEntry]) -> String {
    if chunks.is_empty() {
        return String::new();
    }
    let capacity = chunks
        .iter()
        .map(|c| c.text.len() + c.source_path.len() + 48)
        .sum::<usize>()
        + 2_400;
    let mut s = String::with_capacity(capacity);
    s.push_str(
        "<BASE DI CONOSCENZA — estratti recuperati (non documenti completi)>\n\
         Passi selezionati per somiglianza con la domanda dalla libreria indicizzata \
         dell'utente. Sono frammenti, non documenti integrali: se serve più contesto usa \
         lo strumento `search_kb` per recuperare altri passi vicini, oppure chiedi \
         all'utente di allegare il documento.\n\n",
    );
    for c in chunks {
        let basename = std::path::Path::new(&c.source_path)
            .file_name()
            .map(|f| f.to_string_lossy())
            .unwrap_or_else(|| c.source_path.as_str().into());
        let _ = write!(
            s,
            "[{}] {} · {} (frammento {}):\n«{}»\n\n",
            c.tag, c.scope_label, basename, c.chunk_index, c.text
        );
    }
    s.push_str(
        "COME CITARE QUESTI PASSI (obbligatorio):\n\
           1. Nel testo scrivi il tag esattamente com'è nel punto di riferimento, ad \
              esempio: \"Articolo 35 GDPR [g1]\".\n\
           2. Aggiungi la voce corrispondente nel blocco <CITATIONS> in fondo alla \
              risposta: questi passi sono fonti documentali a tutti gli effetti.\n\
           3. Nella voce imposta sia \"ref\" sia \"doc_id\" al tag usato nel testo \
              (\"g1\", \"p1\", …): non un numero, non \"doc-0\", non un nome di file.\n\
           4. \"quote\" deve essere una sottostringa esatta del testo tra «…»: niente \
              traduzioni, parafrasi, riassunti o correzioni tipografiche. Il visualizzatore \
              cerca queste parole nel PDF per evidenziarle, e ogni differenza rompe \
              l'evidenziazione. Se rispondi in un'altra lingua, traduci nel testo ma lascia \
              \"quote\" nella lingua originale.\n\n\
         Esempio (in italiano; nella risposta usa la lingua dell'utente):\n\
         Testo: \"L'articolo 35 GDPR richiede una valutazione d'impatto [g1].\"\n\
         <CITATIONS>\n\
         [\n  {\"ref\": \"g1\", \"doc_id\": \"g1\", \"quote\": \"...\"}\n]\n\
         </CITATIONS>\n\n\
         Se nel testo compare un tag [gN]/[pN], il blocco <CITATIONS> è obbligatorio: \
         senza, l'interfaccia non può mostrare il riferimento cliccabile.\n\
         </BASE DI CONOSCENZA>\n",
    );
    s
}

/// Block about the connected MCP servers. The tool definitions
/// travel in the `tools` parameter; here a short summary is enough, plus
/// the instruction not to offer tools unless the user asks for them.
pub(super) fn build_mcp_system_prompt(servers: &[McpDiscovered]) -> String {
    if servers.is_empty() {
        return String::new();
    }
    let mut s = String::from(
        "STRUMENTI MCP — di norma rispondi direttamente in base alla conversazione e ai \
         documenti allegati.\n\n\
         Hai a disposizione strumenti esterni opzionali forniti dai server MCP collegati \
         (dichiarati nel parametro `tools`). Invocali **solo quando l'utente lo chiede \
         esplicitamente** (\"usa lo strumento X\", \"chiama X\", \"esegui X su questo\"). \
         Per saluti e richieste generiche (\"ciao\", \"prova\", \"spiega\", \"riassumi\", \
         \"analizza questo\") rispondi normalmente, **senza elencare o proporre gli \
         strumenti**.\n\n\
         Server MCP collegati (non elencarli se non richiesto):\n",
    );
    for srv in servers {
        let display = srv.server_name.as_deref().unwrap_or(&srv.config_name);
        let summary: String = srv
            .instructions
            .as_deref()
            .and_then(|inst| inst.split(['.', '\n']).next())
            .map(|first| first.trim().chars().take(160).collect())
            .unwrap_or_default();
        let _ = write!(s, "- `{display}`");
        if let Some(v) = &srv.server_version {
            let _ = write!(s, " v{v}");
        }
        if !summary.is_empty() {
            let _ = write!(s, " — {summary}");
        }
        s.push('\n');
    }
    s.push('\n');
    s
}

/// Notice shown in chat when the selected model doesn't accept the
/// `tools` parameter, in the interface language (`locale`).
pub(super) fn unsupported_tools_warning(locale: &str, model: &str, n: usize) -> String {
    const MODELS: &str = "Claude, Gemini, GPT-4o, Qwen 2.5, Llama 3.1+, Mistral Small";
    let body = match locale {
        "it" => format!(
            "**Il modello selezionato non supporta l'uso di strumenti** (`{model}`). I {n} server \
             MCP configurati sono visibili nel mio contesto, ma non posso invocarne direttamente \
             gli strumenti. Per usarli scegli un modello compatibile: {MODELS}."
        ),
        "fr" => format!(
            "**Le modèle sélectionné ne prend pas en charge les outils** (`{model}`). Les {n} \
             serveurs MCP configurés sont visibles dans mon contexte, mais je ne peux pas appeler \
             leurs outils. Pour les utiliser, choisissez un modèle compatible : {MODELS}."
        ),
        "de" => format!(
            "**Das ausgewählte Modell unterstützt keine Tools** (`{model}`). Die {n} konfigurierten \
             MCP-Server sind in meinem Kontext sichtbar, aber ich kann ihre Tools nicht aufrufen. \
             Wählen Sie dafür ein kompatibles Modell: {MODELS}."
        ),
        "es" => format!(
            "**El modelo seleccionado no admite el uso de herramientas** (`{model}`). Los {n} \
             servidores MCP configurados son visibles en mi contexto, pero no puedo invocar sus \
             herramientas. Para usarlas, elige un modelo compatible: {MODELS}."
        ),
        "pt" => format!(
            "**O modelo selecionado não suporta o uso de ferramentas** (`{model}`). Os {n} \
             servidores MCP configurados estão visíveis no meu contexto, mas não posso invocar \
             as suas ferramentas. Para usá-las, escolha um modelo compatível: {MODELS}."
        ),
        _ => format!(
            "**The selected model does not support tool use** (`{model}`). The {n} configured MCP \
             servers are visible in my context, but I cannot call their tools. To use them, pick \
             a compatible model: {MODELS}."
        ),
    };
    format!("> ⚠️ {body}\n\n---\n\n")
}

/// Nudge sent to the model when it ends a turn with no text and no
/// tool calls. It restates the language so the reply isn't dragged
/// into Italian.
pub(super) const EMPTY_ANSWER_NUDGE: &str =
    "Non hai prodotto alcuna risposta. Completa ora la richiesta seguendo le \
     istruzioni: se ti serve il contenuto di un documento caricalo con \
     read_document, poi fornisci direttamente il risultato richiesto, nella \
     lingua dell'ultimo messaggio dell'utente (non in quella di questo sollecito).";

/// Note shown to the user when the reply is still empty after the
/// nudges run out.
pub(super) fn empty_answer_note(locale: &str) -> &'static str {
    match locale {
        "it" => "_(Il modello non ha prodotto una risposta. Riprova a inviare il messaggio.)_",
        "fr" => "_(Le modèle n'a produit aucune réponse. Réessayez d'envoyer le message.)_",
        "de" => "_(Das Modell hat keine Antwort erzeugt. Senden Sie die Nachricht erneut.)_",
        "es" => "_(El modelo no ha generado ninguna respuesta. Vuelve a enviar el mensaje.)_",
        "pt" => "_(O modelo não produziu nenhuma resposta. Tente enviar a mensagem novamente.)_",
        _ => "_(The model produced no answer. Please send the message again.)_",
    }
}

/// Warning shown before sending when the request is estimated to exceed
/// the model's context window.
pub(super) fn context_overflow_note(locale: &str, estimated: usize, window: usize) -> String {
    let body = match locale {
        "it" => format!(
            "La richiesta stimata (~{estimated} token) supera la finestra di contesto del \
             modello ({window} token). Riduci o togli qualche allegato, oppure aumenta il \
             contesto del modello sul server (per Ollama `num_ctx` o `OLLAMA_CONTEXT_LENGTH`)."
        ),
        "fr" => format!(
            "La requête estimée (~{estimated} jetons) dépasse la fenêtre de contexte du modèle \
             ({window} jetons). Réduisez ou retirez des pièces jointes, ou augmentez le contexte \
             du modèle sur le serveur (pour Ollama `num_ctx` ou `OLLAMA_CONTEXT_LENGTH`)."
        ),
        "de" => format!(
            "Die geschätzte Anfrage (~{estimated} Token) überschreitet das Kontextfenster des \
             Modells ({window} Token). Anhänge verkleinern oder entfernen oder den Kontext des \
             Modells auf dem Server erhöhen (bei Ollama `num_ctx` oder `OLLAMA_CONTEXT_LENGTH`)."
        ),
        "es" => format!(
            "La solicitud estimada (~{estimated} tokens) supera la ventana de contexto del modelo \
             ({window} tokens). Reduce o quita algunos adjuntos, o aumenta el contexto del modelo \
             en el servidor (en Ollama `num_ctx` u `OLLAMA_CONTEXT_LENGTH`)."
        ),
        "pt" => format!(
            "O pedido estimado (~{estimated} tokens) excede a janela de contexto do modelo \
             ({window} tokens). Reduza ou remova alguns anexos, ou aumente o contexto do modelo \
             no servidor (no Ollama `num_ctx` ou `OLLAMA_CONTEXT_LENGTH`)."
        ),
        _ => format!(
            "The estimated request (~{estimated} tokens) exceeds the model's context window \
             ({window} tokens). Remove or shorten some attachments, or increase the model's \
             context on the server (for Ollama `num_ctx` or `OLLAMA_CONTEXT_LENGTH`)."
        ),
    };
    format!("> ⚠️ {body}\n\n")
}

/// Note shown when some attachments were reduced to excerpts to fit the
/// model's context window.
pub(super) fn attachments_excerpted_note(
    locale: &str,
    items: &[(String, super::attachment_budget::ExcerptInfo)],
) -> String {
    use crate::document_segments::SegmentUnit;
    let list = items
        .iter()
        .map(|(name, e)| {
            let unit = match (locale, e.unit) {
                ("it", SegmentUnit::Page) => "pagine",
                ("it", SegmentUnit::Part) => "parti",
                ("fr", SegmentUnit::Page) => "pages",
                ("fr", SegmentUnit::Part) => "parties",
                ("de", SegmentUnit::Page) => "Seiten",
                ("de", SegmentUnit::Part) => "Teile",
                ("es", SegmentUnit::Page) => "páginas",
                ("es", SegmentUnit::Part) => "partes",
                ("pt", SegmentUnit::Page) => "páginas",
                ("pt", SegmentUnit::Part) => "partes",
                (_, SegmentUnit::Page) => "pages",
                (_, SegmentUnit::Part) => "parts",
            };
            format!("{name} ({}/{} {unit})", e.kept, e.total)
        })
        .collect::<Vec<_>>()
        .join(", ");
    let body = match locale {
        "it" => format!("Documenti troppo lunghi per la finestra del modello: ho inviato gli estratti più pertinenti di {list}; il resto viene letto a pagine quando serve."),
        "fr" => format!("Documents trop longs pour la fenêtre du modèle : les extraits les plus pertinents de {list} ont été envoyés ; le reste est lu par pages si nécessaire."),
        "de" => format!("Dokumente zu lang für das Modellfenster: Die relevantesten Auszüge aus {list} wurden gesendet; der Rest wird bei Bedarf seitenweise gelesen."),
        "es" => format!("Documentos demasiado largos para la ventana del modelo: se han enviado los extractos más pertinentes de {list}; el resto se lee por páginas cuando haga falta."),
        "pt" => format!("Documentos demasiado longos para a janela do modelo: foram enviados os excertos mais relevantes de {list}; o resto é lido por páginas quando necessário."),
        _ => format!("Documents too long for the model's window: the most relevant excerpts of {list} were sent; the rest is read page by page when needed."),
    };
    format!("> ℹ️ {body}\n\n")
}

/// Note shown when the server rejected the request for its size and its
/// real context window was learnt from the error.
pub(super) fn context_window_learnt_note(locale: &str, window: usize) -> String {
    let body = match locale {
        "it" => format!("Il server lavora con una finestra di {window} token: la richiesta era troppo grande. Dal prossimo messaggio la conversazione viene compressa di conseguenza; per gli allegati riducili o aumenta il contesto del modello."),
        "fr" => format!("Le serveur utilise une fenêtre de {window} jetons : la requête était trop grande. À partir du prochain message la conversation est compressée en conséquence ; pour les pièces jointes, réduisez-les ou augmentez le contexte du modèle."),
        "de" => format!("Der Server arbeitet mit einem Fenster von {window} Token: Die Anfrage war zu groß. Ab der nächsten Nachricht wird die Unterhaltung entsprechend komprimiert; Anhänge verkleinern oder den Kontext des Modells erhöhen."),
        "es" => format!("El servidor trabaja con una ventana de {window} tokens: la solicitud era demasiado grande. Desde el próximo mensaje la conversación se comprime en consecuencia; reduce los adjuntos o aumenta el contexto del modelo."),
        "pt" => format!("O servidor trabalha com uma janela de {window} tokens: o pedido era demasiado grande. A partir da próxima mensagem a conversa é comprimida em conformidade; reduza os anexos ou aumente o contexto do modelo."),
        _ => format!("The server runs the model with a {window} token window: the request was too large. From the next message the conversation is compressed accordingly; shorten the attachments or increase the model's context."),
    };
    format!("> ⚠️ {body}\n\n")
}

/// Note shown when the tool loop exceeds the limit.
pub(super) fn tool_loop_limit_note(locale: &str) -> &'static str {
    match locale {
        "it" => "\n\n_(interrotto: troppe chiamate consecutive agli strumenti)_",
        "fr" => "\n\n_(interrompu : trop d'appels d'outils consécutifs)_",
        "de" => "\n\n_(abgebrochen: zu viele aufeinanderfolgende Tool-Aufrufe)_",
        "es" => "\n\n_(interrumpido: demasiadas llamadas consecutivas a herramientas)_",
        "pt" => "\n\n_(interrompido: demasiadas chamadas consecutivas a ferramentas)_",
        _ => "\n\n_(stopped: too many consecutive tool calls)_",
    }
}

/// Prompt that generates the chat title from the first message.
pub(super) fn title_prompt(first_message: &str) -> String {
    let excerpt: String = first_message.chars().take(500).collect();
    format!(
        "Scrivi un titolo di 3-5 parole, senza virgolette né punteggiatura, per una chat \
         che inizia con il messaggio seguente. Usa la stessa lingua del messaggio e \
         rispondi solo con il titolo.\n\n{excerpt}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(name: &str, text: Option<&str>, images: usize) -> DocPayload {
        DocPayload {
            filename: name.to_string(),
            text: text.map(str::to_string),
            images: vec![String::new(); images],
            excerpt: None,
            unreadable: None,
        }
    }

    fn unreadable_doc(name: &str, code: &str) -> DocPayload {
        DocPayload {
            unreadable: Some(code.to_string()),
            ..doc(name, None, 0)
        }
    }

    #[test]
    fn an_unreadable_document_is_declared_to_the_model() {
        // The defect this fixes: the document appeared in the prompt
        // as a filename over an empty block, and the model answered
        // about it anyway.
        let docs = vec![unreadable_doc(
            "scansione.pdf",
            crate::ingest::outcome::no_text::SCANNED_PDF,
        )];
        let prompt = build_doc_system_prompt(&docs);
        assert!(prompt.contains("scansione.pdf"), "{prompt}");
        assert!(prompt.contains("NON LEGGIBILE"), "{prompt}");
        // The one case with an actionable answer must name it.
        assert!(prompt.contains("OCR"), "{prompt}");
        assert!(
            prompt.contains("dillo all'utente"),
            "the model must be told to say so: {prompt}"
        );
    }

    #[test]
    fn unreadable_documents_keep_a_label_after_the_readable_ones() {
        // Labels must not collide: the tools resolve doc-N, and a
        // duplicate would send read_document to the wrong file.
        let docs = vec![
            doc("contratto.txt", Some("testo"), 0),
            doc("scansione.pdf", None, 3),
            unreadable_doc(
                "protetto.pdf",
                crate::ingest::outcome::failure::ENCRYPTED_PDF,
            ),
        ];
        let prompt = build_doc_system_prompt(&docs);
        assert!(prompt.contains("doc-1 (file: contratto.txt)"), "{prompt}");
        assert!(prompt.contains("doc-2 (file: scansione.pdf"), "{prompt}");
        assert!(prompt.contains("doc-3 (file: protetto.pdf)"), "{prompt}");
        assert_eq!(prompt.matches("doc-3").count(), 1, "{prompt}");
        assert!(prompt.contains("password"), "{prompt}");
    }

    #[test]
    fn every_code_has_a_note_and_none_is_silent() {
        use crate::ingest::outcome::{failure, no_text};
        for code in [
            no_text::SCANNED_PDF,
            no_text::IMAGE_ONLY,
            no_text::EMPTY_FILE,
            no_text::OFFICE_NO_TEXT,
            failure::ENCRYPTED_PDF,
            failure::UNSUPPORTED_FORMAT,
            failure::CORRUPT_FILE,
            failure::READ_FAILED,
            // A code this build does not know yet must still produce a
            // true sentence rather than an empty one.
            "qualcosa_di_nuovo",
        ] {
            let note = unreadable_note(code);
            assert!(note.contains("NON LEGGIBILE"), "{code}: {note}");
        }
    }

    #[test]
    fn a_turn_with_only_unreadable_documents_still_builds_a_block() {
        // Otherwise the model would see no mention of the attachment at
        // all and answer as if the user had sent nothing.
        let docs = vec![unreadable_doc(
            "vuoto.txt",
            crate::ingest::outcome::no_text::EMPTY_FILE,
        )];
        assert!(!build_doc_system_prompt(&docs).is_empty());
    }

    #[test]
    fn user_visible_notes_fall_back_to_english() {
        assert!(empty_answer_note("en").contains("no answer"));
        assert!(empty_answer_note("ja").contains("no answer"));
        assert!(tool_loop_limit_note("it").contains("interrotto"));
        let w = unsupported_tools_warning("en", "local:gemma3", 2);
        assert!(w.starts_with("> ⚠️ **The selected model"));
        assert!(w.contains("`local:gemma3`") && w.contains("The 2 configured"));
    }

    #[test]
    fn doc_prompt_numbers_text_then_image_docs() {
        let docs = [doc("a.pdf", Some("AAA"), 0), doc("scan.pdf", None, 2), doc("b.docx", Some("BBB"), 0)];
        let p = build_doc_system_prompt(&docs);
        assert!(p.contains("=== doc-1 (file: a.pdf) ===\nAAA"));
        assert!(p.contains("=== doc-2 (file: b.docx) ===\nBBB"));
        assert!(p.contains("=== doc-3 (file: scan.pdf, inviato come 2 immagini"));
        assert!(build_doc_system_prompt(&[]).is_empty());
    }

    #[test]
    fn project_docs_prompt_offsets_labels() {
        let docs = vec![("id1".to_string(), "x.pdf".to_string())];
        assert!(build_project_docs_prompt(2, &docs).contains("doc-3 : x.pdf"));
        assert!(build_project_docs_prompt(0, &[]).is_empty());
    }
}
