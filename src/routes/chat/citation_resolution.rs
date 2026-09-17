// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

//! Resolution of the citations emitted by the model: each entry of the
//! `<CITATIONS>` block is traced back to a real document (attachment,
//! project document or knowledge-base chunk) and enriched with the fields
//! the viewer needs (`document_id`, `filename`, `path`,
//! `page`, `source`, …).
//!
//! The database indexes are built only once, and only if the
//! reply contains citations: one query for the labelled file names and
//! one for the corpus library (aliases, canonical keys, local paths).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde_json::{Map, Value};

use super::{canonical_corpus_key, letters_only, strip_page_markers, RetrievedKbEntry};
use crate::AppState;

/// Row of the user's corpus library.
struct CorpusRow {
    id: String,
    filename: String,
    corpus_id: String,
    identifier: Option<String>,
    storage_path: Option<String>,
}

pub(super) struct CitationResolver<'a> {
    kb_by_tag: &'a HashMap<String, RetrievedKbEntry>,
    doc_label_map: &'a HashMap<String, String>,
    /// Document UUID → file name, for the `doc-N` labels.
    name_by_id: HashMap<String, String>,
    /// Alias of a corpus identifier (e.g. `eurlex_32016R0679`) → KB tag.
    corpus_ref_to_tag: HashMap<String, String>,
    /// Canonical key (lowercase alphanumerics only) → (UUID, file name).
    library_corpus_index: HashMap<String, (String, String)>,
    /// Corpus document UUID → absolute path of the cached file.
    corpus_local_path_by_docid: HashMap<String, String>,
    /// KB chunk text normalised for matching against citations,
    /// computed the first time it is needed.
    normalised_kb_text: OnceLock<Vec<(&'a str, String)>>,
    /// Distinct documents among the turn's KB chunks.
    kb_document_count: usize,
}

impl<'a> CitationResolver<'a> {
    pub(super) async fn load(
        state: &AppState,
        user_id: &str,
        doc_label_map: &'a HashMap<String, String>,
        kb_by_tag: &'a HashMap<String, RetrievedKbEntry>,
    ) -> CitationResolver<'a> {
        let name_by_id = fetch_filenames(state, user_id, doc_label_map.values()).await;
        let library = fetch_corpus_library(state, user_id).await;

        let kb_doc_ids: HashSet<&str> =
            kb_by_tag.values().map(|e| e.document_id.as_str()).collect();
        let mut tag_by_doc: HashMap<&str, &str> = HashMap::new();
        // First tag per document in retrieval order: g1, g2, …, g10.
        let mut sorted_tags: Vec<&RetrievedKbEntry> = kb_by_tag.values().collect();
        sorted_tags.sort_by_key(|e| {
            let (scope, n) = e.tag.split_at(e.tag.len().min(1));
            (scope.to_string(), n.parse::<u32>().unwrap_or(u32::MAX))
        });
        for entry in sorted_tags {
            tag_by_doc.entry(entry.document_id.as_str()).or_insert(entry.tag.as_str());
        }

        let storage_root = std::path::PathBuf::from(
            std::env::var("STORAGE_PATH").unwrap_or_else(|_| "./data/storage".to_string()),
        );

        let mut corpus_ref_to_tag = HashMap::new();
        let mut library_corpus_index = HashMap::new();
        let mut corpus_local_path_by_docid = HashMap::new();
        for row in library {
            if let Some(sp) = &row.storage_path {
                let abs = storage_root.join(sp.replace('/', std::path::MAIN_SEPARATOR_STR));
                corpus_local_path_by_docid
                    .insert(row.id.clone(), abs.to_string_lossy().into_owned());
            }
            let Some(ident) = &row.identifier else { continue };

            // Alias to the KB tag, only for documents retrieved in this turn.
            if kb_doc_ids.contains(row.id.as_str())
                && let Some(tag) = tag_by_doc.get(row.id.as_str())
            {
                let corpus = &row.corpus_id;
                let ident_lower = ident.to_ascii_lowercase();
                let corpus_lower = corpus.to_ascii_lowercase();
                for key in [
                    ident.clone(),
                    ident_lower.clone(),
                    format!("{corpus}_{ident}"),
                    format!("{corpus_lower}_{ident_lower}"),
                    format!("{corpus}:{ident}"),
                    format!("{corpus_lower}:{ident_lower}"),
                    format!("{corpus}/{ident}"),
                    format!("{corpus_lower}/{ident_lower}"),
                ] {
                    corpus_ref_to_tag.entry(key).or_insert_with(|| tag.to_string());
                }
            }

            // Canonical keys over the whole library: bare identifier and
            // identifier prefixed with the corpus.
            for source in [ident.as_str(), &format!("{} {ident}", row.corpus_id)] {
                let canon = canonical_corpus_key(source);
                if !canon.is_empty() {
                    library_corpus_index
                        .entry(canon)
                        .or_insert_with(|| (row.id.clone(), row.filename.clone()));
                }
            }
        }
        tracing::info!(
            "[chat] citation indexes: {} labelled filenames, {} corpus aliases, {} canonical keys, {} local paths",
            name_by_id.len(),
            corpus_ref_to_tag.len(),
            library_corpus_index.len(),
            corpus_local_path_by_docid.len()
        );

        CitationResolver {
            kb_by_tag,
            doc_label_map,
            name_by_id,
            corpus_ref_to_tag,
            library_corpus_index,
            corpus_local_path_by_docid,
            normalised_kb_text: OnceLock::new(),
            kb_document_count: kb_doc_ids.len(),
        }
    }

    /// Resolves and enriches every entry of the citations array.
    pub(super) fn resolve_all(&self, citations: Value) -> Vec<Value> {
        let Value::Array(entries) = citations else { return Vec::new() };
        entries.into_iter().map(|c| self.resolve_one(c)).collect()
    }

    fn is_known(&self, label: &str) -> bool {
        self.kb_by_tag.contains_key(label) || self.doc_label_map.contains_key(label)
    }

    fn resolve_one(&self, citation: Value) -> Value {
        let mut obj = match citation {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        obj.insert("type".into(), Value::String("citation_data".into()));

        let original = obj
            .get("doc_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let mut label = original.clone();
        if !self.is_known(&label) {
            label = self.recover_label(&original, &mut obj);
            if label != original {
                obj.insert("doc_id".into(), Value::String(label.clone()));
            }
        }

        match self.kb_by_tag.get(&label) {
            Some(kb) => self.enrich_kb(&mut obj, &label, kb),
            None => self.enrich_attached(&mut obj, &label),
        }
        Value::Object(obj)
    }

    /// Recovers the label when the model didn't use a valid tag or
    /// `doc-N`. May remove `page` when the label was
    /// guessed and the page is therefore unreliable.
    fn recover_label(&self, original: &str, obj: &mut Map<String, Value>) -> String {
        let normalised = original
            .trim()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_ascii_lowercase();

        if self.is_known(&normalised) {
            return normalised;
        }
        if let Some(tag) = self
            .corpus_ref_to_tag
            .get(original)
            .or_else(|| self.corpus_ref_to_tag.get(&normalised))
        {
            tracing::info!(
                "[chat] citation doc_id {original:?} resolved via corpus alias to KB tag {tag:?}"
            );
            return tag.clone();
        }
        if !normalised.is_empty() && normalised.chars().all(|c| c.is_ascii_digit()) {
            let mut g_tags = self.kb_by_tag.keys().filter(|k| k.starts_with('g'));
            if let (Some(only), None) = (g_tags.next(), g_tags.next()) {
                tracing::info!(
                    "[chat] citation doc_id {original:?} is a bare number; mapping to sole KB tag {only:?}"
                );
                return only.clone();
            }
            let candidate = format!("g{normalised}");
            if self.kb_by_tag.contains_key(&candidate) {
                return candidate;
            }
        }

        if let Some(tag) = obj
            .get("quote")
            .and_then(Value::as_str)
            .and_then(|q| self.match_quote_to_kb(q))
        {
            tracing::info!(
                "[chat] citation doc_id {original:?} resolved by quote match to KB tag {tag:?}"
            );
            return tag.to_string();
        }

        // All of the turn's chunks come from the same document: the
        // citation almost certainly refers to it.
        if self.kb_document_count == 1 {
            let mut keys: Vec<&String> = self.kb_by_tag.keys().collect();
            keys.sort();
            if let Some(tag) = keys.iter().find(|k| k.starts_with('g')).or(keys.first()) {
                tracing::info!(
                    "[chat] citation doc_id {original:?} unresolvable; all KB chunks share one document — routing to {tag:?}"
                );
                obj.remove("page");
                return (*tag).clone();
            }
        }
        original.to_string()
    }

    /// Finds the KB chunk that contains the start of the quote
    /// (at least 25 characters, whitespace collapsed, lowercase).
    fn match_quote_to_kb(&self, quote: &str) -> Option<&'a str> {
        let needle: String = compact_lower(quote).chars().take(120).collect();
        if needle.chars().count() < 25 {
            return None;
        }
        let normalised = self.normalised_kb_text.get_or_init(|| {
            self.kb_by_tag
                .iter()
                .map(|(tag, kb)| (tag.as_str(), compact_lower(&kb.text)))
                .collect()
        });
        normalised
            .iter()
            .find(|(_, hay)| hay.contains(&needle))
            .map(|(tag, _)| *tag)
    }

    fn enrich_kb(&self, obj: &mut Map<String, Value>, label: &str, kb: &RetrievedKbEntry) {
        // `[Page N]` markers exist only in the extracted text, not in the PDF.
        if let Some(q) = obj.get("quote").and_then(Value::as_str) {
            let cleaned = strip_page_markers(q);
            if cleaned != q {
                obj.insert("quote".into(), Value::String(cleaned));
            }
        }
        // Invented quote: replace it with the start of the chunk
        // so the viewer at least lands on the right passage.
        if let Some(q) = obj.get("quote").and_then(Value::as_str) {
            let chunk_clean = strip_page_markers(&kb.text);
            let needle = letters_only(q);
            if needle.len() >= 4 && !letters_only(&chunk_clean).contains(&needle) {
                let trimmed = chunk_clean.trim();
                let mut end = 200.min(trimmed.len());
                while end < trimmed.len() && !trimmed.is_char_boundary(end) {
                    end += 1;
                }
                let fallback = trimmed[..end].to_string();
                tracing::warn!(
                    "[chat] citation quote not found in chunk for tag {label:?} (doc {:?}, chunk {}): model emitted {:?}; substituting first {} chars of chunk text",
                    kb.document_id,
                    kb.chunk_index,
                    q.chars().take(80).collect::<String>(),
                    fallback.len()
                );
                obj.insert("quote".into(), Value::String(fallback));
            }
        }

        obj.insert("source".into(), Value::String("kb".into()));
        obj.insert("scope".into(), Value::String(kb.scope_label.into()));

        // `/sync/kb-doc` reads from disk: a URL path must be remapped to the
        // local cached file.
        let mut path = kb.source_path.clone();
        if path.starts_with("http://") || path.starts_with("https://") {
            match self.corpus_local_path_by_docid.get(&kb.document_id) {
                Some(local) => {
                    tracing::info!(
                        "[chat] remapping URL source_path → local storage path for doc {:?} (was {path:?})",
                        kb.document_id
                    );
                    path = local.clone();
                }
                None => tracing::warn!(
                    "[chat] citation source_path is a URL ({path:?}) but no local storage_path is registered for doc_id {:?} — viewer will 404",
                    kb.document_id
                ),
            }
        }
        obj.insert("path".into(), Value::String(path));
        obj.insert("chunk_index".into(), Value::Number(kb.chunk_index.into()));
        obj.insert("document_id".into(), Value::String(kb.document_id.clone()));
        let basename = std::path::Path::new(&kb.source_path)
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| kb.source_path.clone());
        obj.insert("filename".into(), Value::String(basename));

        // The page given by the model is more precise than the chunk's start
        // page; the chunker's page remains the fallback.
        let model_page_ok = obj
            .get("page")
            .is_some_and(|v| v.is_i64() || v.is_string());
        if !model_page_ok && let Some(p) = kb.page {
            obj.insert("page".into(), Value::Number(p.into()));
        }
    }

    fn enrich_attached(&self, obj: &mut Map<String, Value>, label: &str) {
        obj.insert("source".into(), Value::String("attached".into()));
        let mut uuid = self.doc_label_map.get(label).cloned();
        let mut filename = uuid
            .as_ref()
            .and_then(|u| self.name_by_id.get(u))
            .cloned()
            .unwrap_or_default();

        // The model used the file name instead of `doc-N`.
        if uuid.is_none()
            && let Some((id, fname)) = self.name_by_id.iter().find(|(_, f)| f.as_str() == label)
        {
            tracing::info!(
                "[chat] citation doc_id {label:?} resolved via attached-filename match"
            );
            uuid = Some(id.clone());
            filename = fname.clone();
        }

        // The model copied a line from the library listing.
        if uuid.is_none() {
            let canon = canonical_corpus_key(label);
            if !canon.is_empty()
                && let Some((corp_uuid, corp_filename)) = self.library_corpus_index.get(&canon)
            {
                tracing::info!(
                    "[chat] citation doc_id {label:?} resolved to corpus document {corp_filename:?} via canonical-key match"
                );
                uuid = Some(corp_uuid.clone());
                if filename.is_empty() {
                    filename = corp_filename.clone();
                }
                obj.remove("page");
            }
        }

        if let Some(uuid) = uuid {
            obj.insert("document_id".into(), Value::String(uuid));
        }
        if !filename.is_empty() {
            obj.insert("filename".into(), Value::String(filename));
        }
    }
}

/// Whitespace collapsed and lowercased, to compare quotes and chunks.
fn compact_lower(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for word in s.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out.to_lowercase()
}

async fn fetch_filenames<'s>(
    state: &AppState,
    user_id: &str,
    ids: impl Iterator<Item = &'s String>,
) -> HashMap<String, String> {
    let ids: Vec<&String> = ids.collect::<HashSet<_>>().into_iter().collect();
    if ids.is_empty() {
        return HashMap::new();
    }
    let sql = format!(
        "SELECT id, filename FROM documents WHERE user_id = ? AND id IN ({})",
        vec!["?"; ids.len()].join(",")
    );
    let mut query = sqlx::query_as::<_, (String, String)>(&sql).bind(user_id);
    for id in ids {
        query = query.bind(id);
    }
    query
        .fetch_all(&state.db)
        .await
        .map(|rows| rows.into_iter().collect())
        .unwrap_or_default()
}

async fn fetch_corpus_library(state: &AppState, user_id: &str) -> Vec<CorpusRow> {
    sqlx::query_as::<_, (String, String, String, Option<String>, Option<String>)>(
        "SELECT id, filename, corpus_id, corpus_identifier, storage_path FROM documents \
         WHERE user_id = ? AND corpus_id IS NOT NULL",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map(|rows| {
        rows.into_iter()
            .map(|(id, filename, corpus_id, identifier, storage_path)| CorpusRow {
                id,
                filename,
                corpus_id,
                identifier,
                storage_path,
            })
            .collect()
    })
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::compact_lower;

    #[test]
    fn compact_lower_collapses_whitespace() {
        assert_eq!(compact_lower("  Art.\n 35   GDPR\t"), "art. 35 gdpr");
        assert_eq!(compact_lower(""), "");
    }
}
