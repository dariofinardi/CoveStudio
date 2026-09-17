# DOCX extraction

Cove Studio extracts text from `.docx` files in pure Rust — no LibreOffice, no Pandoc, no external process. The extractor lives at [src/pdf/mod.rs](../src/pdf/mod.rs) (`extract_docx_text`) and is reused by:

1. The folder scanner (`src/sync/scanner.rs::extract_text_dispatch`) when indexing `.docx` files into the embedding store.
2. The chat-upload pipeline (`src/routes/documents.rs::upload_document`, cache path) when an attached docx is hashed and pre-extracted to `data/storage/cache/<hash>.txt`.
3. The `read_document` builtin tool (`src/llm/builtin_tools.rs`) when the LLM asks for the full text of an attached doc by label.

## Why pure Rust

The upstream `willchen96/mike` and its forks (e.g. `marklok/danishmike`) shell out to LibreOffice via `libreoffice-convert` and parse via `mammoth`. That's three problems for a sovereign desktop app:

- **External binary** — LibreOffice is ~300 MB, doesn't bundle, requires PATH wiring.
- **Per-extraction process spawn** — each docx is a fork+exec with ~1 s startup overhead.
- **Mammoth loses redline information** — its HTML output drops `<w:del>` and visual strikes silently.

`zip` + `quick-xml` give us streaming ZIP entry access and a low-allocation XML reader. The extractor stays well under 200 lines and runs in microseconds for typical contracts.

## Output format

The extractor emits **plain text with paragraph newlines** plus inline `[removed by author: …]` markers around content the author indicated as removed. Two distinct OOXML signals trigger the marker:

### 1. Tracked deletions (`<w:del>`)

When a document is edited with track-changes on, deleted runs are wrapped:

```xml
<w:del w:id="1" w:author="Alice" w:date="2024-01-01T00:00:00Z">
  <w:r><w:delText xml:space="preserve">old text</w:delText></w:r>
</w:del>
```

`<w:delText>` is the same as `<w:t>` for our purposes — text that was there before the deletion. The extractor enters "removed" mode on `<w:del>`, exits on `</w:del>`, and wraps everything inside in `[removed by author: old text]`.

### 2. Strike-through formatting (`<w:strike/>` / `<w:dstrike/>`)

Authors sometimes simulate "removed" without enabling tracked changes by applying strike-through formatting to a run:

```xml
<w:r>
  <w:rPr><w:strike/></w:rPr>
  <w:t>visually struck</w:t>
</w:r>
```

`<w:dstrike/>` is double-strike, treated identically. The extractor flips a `current_run_struck` flag when it sees either inside `<w:rPr>`, applies the marker for that run's `<w:t>` content, and clears the flag at `</w:r>`.

### Marker output

Both signals collapse into the same surface form so downstream consumers (LLM prompts, embedding chunks) don't have to distinguish:

```
The contract clauses are: clause 1, [removed by author: clause 2], clause 3.
```

When a struck or deleted segment fans out across multiple sibling runs the marker spans them; the bracket closes at:
- end of `<w:del>` block (for tracked deletions)
- end of `<w:r>` for strike-through-only runs (since strike scope is run-local)
- end of `<w:p>` (paragraph) — never spans paragraphs, even if a malformed doc would have it.

## Paragraph and inline structure

| OOXML element | Output |
|---|---|
| `<w:p>` open | newline (if previous content) |
| `<w:p>` close | flushes any open removal bracket |
| `<w:br/>` | newline |
| `<w:tab/>` | single space |
| `<w:t>` text | emitted verbatim (XML-unescaped) |
| Anything else | ignored |

After the walk, internal whitespace is collapsed to single spaces but newlines are preserved, then leading/trailing whitespace is trimmed.

## What we don't extract (yet)

The extractor focuses on `word/document.xml` only. Out of scope at the moment:

- **Headers / footers** (`word/header*.xml`, `word/footer*.xml`) — usually boilerplate (page numbers, "Confidential" labels). Adding them is a one-line change but bloats embeddings with no signal.
- **Footnotes / endnotes** (`word/footnotes.xml`) — relevant for academic / legal citations; planned.
- **Comments** (`word/comments.xml`) — distinct from track-changes deletions; planned, would emit as `[comment by author: …]`.
- **Tracked insertions** (`<w:ins>`) — currently rendered as plain text, which matches what a human reader would see in the "all markup accepted" view. Could be marked as `[inserted by author: …]` symmetrically with deletions if redline analysis becomes a priority.

## Caching and re-extraction

Chat-attached docx files are extracted once at upload and persisted at `data/storage/cache/<sha256>.txt`. A file edit changes the SHA-256, so the cache transparently regenerates on the next upload. See [CACHE.md](CACHE.md) for the full storage contract.

Existing cache entries from before redline detection landed are stale. To force regeneration:

```bash
rm -r data/storage/cache/*
# next upload re-hashes and re-extracts
```

(or just upload a one-byte-different version, which produces a new hash.)

## Tests

[src/pdf/mod.rs](../src/pdf/mod.rs) `mod docx_tests` covers:

- Plain paragraphs round-trip unchanged.
- `<w:del>` tracked deletion produces `[removed by author: …]`.
- `<w:strike/>` formatted run produces the same marker.
- `<w:dstrike/>` double-strike treated identically.
- Removal brackets never span paragraph boundaries.
- Plain runs without strike formatting stay plain (no false positives).
- Empty documents return empty string.

Run with:

```bash
cargo test --features rag --lib pdf::docx_tests
```
