//! Named-entity recognition + PII detection via GLiNER2.
//!
//! Optional. Compiled in only when the `ner-pii` feature is enabled —
//! without it this module is empty and downstream callers fall
//! through to "feature not compiled" branches.
//!
//! Pipeline (full spec: `docs/gliner2-pii-plan.md`):
//!
//! ```text
//! text + Option<&[label]> + threshold
//!   → engine::mask_pii
//!         SpanEngine (gliner2-rs 0.9.6), lazy singleton, fp16 on the
//!         same onnxruntime DLL fastembed already loads
//!       · one SchemaTask per trained PII group + our extras
//!       · chunked in words, spans remapped and merged at the seams
//!       · privacy::redact by offset, then every remaining literal
//!         occurrence of a detected value
//!   → redacted String
//! ```
//!
//! The detection threshold comes from the user's setting
//! (`user_settings.pii_threshold`, migration 0035): lower masks more.
//! The label vocabulary is the model's own (`gliner2_rs::privacy`), so
//! there is no local label list to keep in sync any more.

#![cfg(feature = "ner-pii")]

pub mod bootstrap;
pub mod engine;

pub use engine::{extract_entities, mask_pii, status, Entity, NerStatus, ProgressFn};
