# Test fixtures

Files the onboarding tests read. They are here, in the repository,
because a test that needs a file the maintainer happens to have on one
machine is a test nobody else can run.

| File | What it is for |
|---|---|
| `scan_es.pdf` | A scanned PDF with **no text layer**: the case where extraction must report `scanned_pdf` rather than an empty string, and where the vision fallback renders pages as images. |
| `scan_offuscato.pdf` | A PDF whose text layer does **not** match what is printed (tampered `cmap`/`ToUnicode`). Exercises the defaced pre-check — for legal documents, extracted text that says something the reader never sees is the dangerous case. |
| `office.doc` | Real legacy Word (OLE container), not a renamed `.docx`. Unreadable before the onboarding funnel. |
| `office.ppt` | Real legacy PowerPoint, same reason. |

Origin: the test corpus of `pageindex-rs`
(github.com/dariofinardi/pageindex-rs, Apache-2.0, same author), copied
here so this repository's tests are self-contained.

`tests/medical/*.pdf` are separate: text-layer PDFs already used by the
citation and ingestion tests.
