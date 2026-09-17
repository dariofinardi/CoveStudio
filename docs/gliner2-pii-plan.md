# GLiNER2 PII — integration plan

> **Note (September 2026).** This plan dates from May 2026. The
> `SemplificaAI/gliner2-rs` crate and the `SemplificaAI` model it
> references are being replaced by a fork of the crate and a new ONNX
> conversion of the base model published under the maintainer's account.

**Status**: design + scaffolding lands on `main`; route + UI follow once
the foundation is callable.

## 1. Scope

**In scope (Phase 1, this branch direction)**

- Bring `gliner2_inference` (`SemplificaAI/gliner2-rs`, v0.5.0) in
  as an optional dependency behind the `ner-pii` feature flag.
- Lazy-load a `Gliner2Engine` singleton in `src/ner/` and expose a
  `pub async fn extract_entities(text, labels) -> Vec<Entity>` that
  any caller (route handler, MCP tool, etc.) can use.
- Wire `ort::init()` and `HF_HOME` at server startup so the engine
  cooperates with the existing fastembed/ort runtime and stashes
  model weights under the same user-data tree the rest of the app
  uses.

**In scope (Phase 1.5, follow-up commit)**

- `GET /document/:id/entities` route that runs the engine on the
  document's joined chunk text and returns the entity list.
- Entity badges in the document viewer (no editing, just visibility).

**Out of scope (Phase 2)**

- Automatic PII redaction before sending to a cloud LLM provider.
  Same `extract_entities` entry point reused; only the call site
  changes (chat send path).
- Custom label-set editor in the UI.
- Confidence-threshold slider.

## 2. Library

- Repo: <https://github.com/SemplificaAI/gliner2-rs>
- Crate: `gliner2_inference` v0.5.0
- License: Apache-2.0
- ort: pinned to `=2.0.0-rc.9` — exact match with Cove Studio's existing
  pin, no resolver drama, cargo dedupes
- Tokenizer: `tokenizers 0.19.1` — already transitive via fastembed
- Default cache: `~/.cache/huggingface/hub/` via `hf-hub`

Required init the parent app owns (the crate does NOT do this for
you):

```rust
ort::init().with_name("GLiNER2_Engine").commit()?;
```

## 3. Dependency block

```toml
[features]
ner-pii = ["dep:gliner2_inference", "dep:half"]

[dependencies]
gliner2_inference = { git = "https://github.com/SemplificaAI/gliner2-rs", tag = "v0.5.0", optional = true }
half               = { version = "2.4", optional = true }
```

The crate isn't on crates.io yet (or wasn't at v0.5.0 — check before
shipping). Pulling from git with a tag avoids surprise upstream
changes. `half` is a small Rust f16/bf16 crate; gliner2 needs it
for the FP16 tensor flow.

## 4. Model + cache

Default model: `SemplificaAI/gliner2-privacy-filter-PII-multi`,
variant `fp16_v2`. Resolved at runtime via `Gliner2Engine::from_
pretrained(...)`; `hf-hub` handles the download + cache.

We redirect the HF cache to `~/cove-studio-data/gliner2/` at startup
by setting `HF_HOME` (same pattern as `FASTEMBED_CACHE_DIR`):

- One folder per heavy on-disk artefact, all under
  `~/cove-studio-data/`
- Tauri watcher never sees the model files
- Power users can override with their own `HF_HOME` env var

## 5. Label set

The PII model is multi-label across European personal-data taxonomies.
For Phase 1 we ship a default label list in `src/ner/labels.rs`
matching the GDPR + Italian fiscal context the rest of the app
targets:

- `person_name`, `email`, `phone`, `address`
- `fiscal_code` (codice fiscale), `vat_number` (partita IVA)
- `iban`, `credit_card`
- `date_of_birth`, `ip_address`
- `license_plate`

The exact labels the model exposes need confirmation against the
model card on HF; if the names differ, the constants in
`labels.rs` are the single point to fix.

## 6. API surface

```rust
// src/ner/mod.rs
pub use engine::{extract_entities, Entity, EntityKind};

// src/ner/engine.rs
pub struct Entity { pub start: usize, pub end: usize,
                    pub label: String, pub score: f32,
                    pub text: String }

pub async fn extract_entities(
    text: &str,
    labels: Option<&[&str]>,
) -> anyhow::Result<Vec<Entity>>;
```

`labels = None` → use the default PII set. `Some(&[...])` →
caller-specified subset (e.g. only `["fiscal_code", "iban"]` for a
contract-redaction workflow).

Internally: singleton via `OnceLock<Arc<Gliner2Engine>>`, lazy-loaded
on first call. Heavy CPU work runs on `tokio::task::spawn_blocking`.

## 7. Build prerequisites

Same as whisper: bindgen builds vendored ONNX Runtime FFI bits
through the `ort-sys` rc.9 crate (no new C++ compile — `ort` itself
is pure-Rust via `load-dynamic`). The vendored `onnxruntime.dll`
1.20.0 already lives under `libs/onnxruntime/<platform>/` and the
gliner engine uses it through the same `ORT_DYLIB_PATH` we set for
fastembed. No new native deps.

## 8. Implementation order

1. Cargo deps + `ner-pii` feature flag (`cargo check` no feature
   stays clean).
2. `src/ner/{mod.rs, engine.rs, labels.rs}` + `ort::init()` call
   + `HF_HOME` redirect at startup.
3. `GET /document/:id/entities` route — runs the engine on the
   joined chunk text, returns JSON `{ entities: [...] }`.
4. Frontend: surface entities in the document viewer (Phase 1.5).
5. Cloud-call redaction pre-filter (Phase 2).
