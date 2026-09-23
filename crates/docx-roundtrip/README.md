# docx-roundtrip

Open a `.docx`, edit it, write it back — **without losing what the library
does not understand**.

A company's Word template carries more than any editor models: text boxes,
charts, fields, content controls, drawings anchored in ways nobody wants to
reimplement. Three things can be done with that content, and only one of
them is honest:

- drop it — every save impoverishes the document;
- refuse the file — the editor is useless on the documents people have;
- **keep it verbatim and put it back where it was**.

This crate does the third. What it models becomes an editable tree; what it
does not becomes an *opaque* node holding the original XML and the
relationships that XML points at.

```rust
let bytes = std::fs::read("offerta.docx")?;
let mut opened = docx_roundtrip::open(&bytes)?;

// Fill a template's placeholders.
for field in opened.document.fields.values_mut() {
    if field.key.as_deref() == Some("cliente") {
        field.value = "Oxygen S.r.l.".into();
    }
}

let assets = docx_roundtrip::Assets::from_opened(&opened);
std::fs::write("offerta-compilata.docx", docx_roundtrip::write(&opened.document, &assets)?)?;
```

## What it promises

- Everything the model represents comes back as structure: paragraphs,
  runs and their formatting, tables (including merged cells), sections with
  their page setup and running heads, footnotes, styles with their
  inheritance resolved but their names kept, tracked changes, comments.
- Everything else comes back as the XML it was.
- `{{placeholders}}` are found even when Word splits them across runs — it
  does, on any edit near the braces — and an unfilled one is written back
  as a placeholder, so a template survives being saved.
- Writing the same document twice produces the same bytes.

## What it does not promise

- **A byte-identical re-save.** Attribute order, relationship ids and
  namespace declarations become this crate's once a document is written
  again. The verifiable contract is the one above.
- Rendering or layout: it says nothing about how a document *looks*, and
  does not paginate.
- Fonts: a document that uses fonts the reader has not installed will
  display with substitutes, exactly as it does in Word.

## Origin

Reimplemented in Rust from `QuoteDOCX` (KeelOps plugin, same author), whose
design — especially the opaque strategy — this follows. The JavaScript
original generates with a library and re-injects the kept fragments
afterwards through a marker; writing the OOXML directly makes that second
pass unnecessary.
