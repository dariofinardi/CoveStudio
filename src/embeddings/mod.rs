//! RAG (Retrieval-Augmented Generation) layer for Cove Studio.
//!
//! Pipeline:
//! 1. **Scan** — walk the user-configured folder tree (`sync` module),
//!    producing a list of files with their sha256.
//! 2. **Extract** — pull plain text out of supported formats (PDF with
//!    embedded text, DOCX, XLSX, MD, TXT). Scanned PDFs and pure
//!    images are skipped per user requirement.
//! 3. **Chunk** — split each document into overlapping passages sized
//!    to fit the embedding model's context (e5-base = 512 tokens).
//! 4. **Embed** — run each chunk through ONNX Runtime via `fastembed`
//!    using INT8-quantized `multilingual-e5-base` (Xenova mirror,
//!    768-dim vectors).
//! 5. **Index** — upsert into a LanceDB table keyed by
//!    `(document_id, chunk_index)`.
//! 6. **Retrieve** — at chat time, embed the user query (with the
//!    `query:` E5 prefix) and search the user's collection for top-K
//!    similar chunks; inject them into the system prompt instead of
//!    the entire document.
//!
//! Heavy dependencies (`fastembed`, `lancedb`, ONNX Runtime native
//! libs) live behind the `rag` feature so a slim build can omit them.
//! When the feature is off the routes return 503 with a clear message.

pub mod chunker;

#[cfg(feature = "rag")]
pub mod service;

#[cfg(feature = "rag")]
pub use service::{EmbeddingService, RagError, RetrievedChunk, SearchScope};

/// Register sqlite-vec as a SQLite auto-extension before any pool is
/// opened. Idempotent (uses `Once` internally). Production code calls
/// this from `AppState::new`; integration tests can call it before
/// constructing their own ad-hoc pool.
#[cfg(feature = "rag")]
pub fn register_sqlite_vec_auto_extension() {
    use std::sync::Once;
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        unsafe {
            type ExtInit = unsafe extern "C" fn(
                *mut libsqlite3_sys::sqlite3,
                *mut *mut std::os::raw::c_char,
                *const libsqlite3_sys::sqlite3_api_routines,
            ) -> std::os::raw::c_int;
            let init: ExtInit =
                std::mem::transmute(sqlite_vec::sqlite3_vec_init as *const ());
            libsqlite3_sys::sqlite3_auto_extension(Some(init));
        }
    });
}
