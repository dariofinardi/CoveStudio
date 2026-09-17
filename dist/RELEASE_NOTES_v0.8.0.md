# Cove Studio v0.8.0 — MikeRust is renamed

> **MikeRust is now Cove Studio.** Same maintainer (Dario Finardi), now
> managing the project directly as an individual, in a new repository:
> [github.com/dariofinardi/CoveStudio](https://github.com/dariofinardi/CoveStudio).
> The installer replaces an existing MikeRust installation in place and
> **your data is migrated automatically on first start** — nothing to
> export, nothing to reinstall.

> **Cumulative changelog since 0.6.7.** The last published installer was
> **v0.6.7**: the 0.7.x line never shipped as an MSI. These notes
> therefore cover everything from v0.7.0 to v0.8.0, so anyone upgrading
> from 0.6.7 or earlier sees the full set of changes. For the
> version-by-version detail see [`HISTORY.md`](../HISTORY.md).

Beyond the name, 0.8.0 addresses the three problems that showed up most
in real use: documents too long for the model, download cards that
sometimes did not appear, and citations of generated documents that
would not open. The 0.7.x line, included here, had added the Italian tax
sector and workflows valid across several sectors.

---

## 🏷️ Renaming and migration

* Product name, desktop identifier (`app.covestudio`), executable, log
  file, user agent and environment-variable prefix (`COVE_`).
* Project archives are written as `.coveprj`; existing `.mikeprj` files
  can still be imported.
* On first start these move by themselves: the data folder
  (`mikerust-data` → `cove-studio-data`), the database (`mike.db` →
  `cove-studio.db`, with its `-wal`/`-shm` files), the embedding and PII
  model caches, and the interface preferences. No extra download.
* `MRUST_*` environment variables are still read.
* Ollama models installed under the previous names are copied to the new
  names and the settings that used them are updated; removing the old
  names happens **only after you confirm** in Settings → Models.

## 📄 Long documents

* **Context window detected per model.** For local Ollama servers the
  value comes from the server itself (loaded model, Modelfile `num_ctx`,
  model maximum); for the others from the model catalogue. If the server
  rejects a request for overflow, the real limit is learnt from that
  error. Settings shows the available window, and where the number came
  from, under each model picker.
* **Attachments over the budget are no longer truncated blindly**: each
  document gets a share proportional to its size and, when it exceeds
  that share, is reduced to the passages relevant to the question (BM25
  ranking). The reply states which documents were excerpted.
* **`read_document` reads by page** (`page_from`/`page_to`, with totals
  and where to continue), so the assistant can walk through a long
  document instead of losing its tail.
* **Gemini 3.7 Flash** and **Gemini 3.8 Flash** added to the catalogue
  (1,048,576 input / 65,536 output tokens).

## 🩹 Word documents — fixes

* A document **generated during the conversation** now gets a citable
  handle, so its citations open the real document. The handle used to
  stay unresolved and the viewer reported a "removed source" for a
  document that did exist.
* **`edit_document` produces a download card**, so an edited document is
  reachable without regenerating it. A document edited more than once in
  the same turn shows a single card.
* When the model pastes a tool's **internal JSON** (`{"doc_id":…}`) into
  its answer, that block is stripped afterwards: the download card
  already carries the information. JSON you actually asked for is left
  untouched. This mostly affected small local models.
* `find_in_document` can no longer break on text with multi-byte
  characters.

## 🔒 Dependencies and security

* **SheetJS** updated to 0.20.3 and vendored in the repository: the npm
  distribution is stuck at 0.18.5, which carries two known
  vulnerabilities (prototype pollution and ReDoS) with no npm release to
  upgrade to.
* npm dependencies updated (postcss, js-yaml, undici, brace-expansion,
  dompurify, vitest, svelte, vite, typescript): `pnpm audit` reports no
  known vulnerabilities. On the Rust side `serde_with` 3.20 → 3.22.
* Still open: an advisory on `thrift` 0.17, pulled in by `parquet` 53.
  `parquet` drops thrift only from version 59, a major upgrade of the
  whole arrow stack, planned separately.
* Windows artefacts are signed with Azure Trusted Signing.

## 🧱 Independence from the original code

* The assistant's base instructions were **rewritten from scratch**, with
  a reply-in-the-user's-language rule first, and moved to
  `config/system-prompts/base.md`.
* README, NOTICE and `docs/UPSTREAM_SYNC.md` state precisely what is
  original work and what still derives from Will Chen's initial code
  (built-in tool schemas, legal-domain presets, some interface strings).
  The licence remains AGPL-3.0-only.

---

# The 0.7.x line — never published as an installer

Upgrading from 0.6.7 brings all of this as well.

## 🧾 New "Tax" sector (v0.7.0)

A twelfth professional vertical, alongside `finance`: finance covers
analysis (financial statements, company valuation, distress), tax covers
compliance and advice (VAT, returns, direct and indirect taxes,
voluntary correction, assessment, litigation). Italian tax law.

* A dedicated system prompt **in 6 languages**, with the **2024 Italian
  tax reforms** already reflected, so the model does not cite repealed
  rules: abolition of *reclamo-mediazione* (from 4 January 2024), the new
  penalty regime under D.Lgs. 87/2024 (late payment 30% → 25%), and the
  generalised prior-hearing requirement.
* **8 workflow presets**: tax opinion, voluntary correction, assessment
  notice analysis, VAT reconciliation, flat-rate regime check, RW form
  (IVIE/IVAFE), indirect taxes on deeds, F24 payment schedule.
* **9 column presets**: taxable base, rate, tax due, withholding,
  penalty, interest, legal basis, deadline, tax code.

## 📊 Financial statements read for tax purposes (v0.7.2)

Three workflows that read the statements through a tax lens rather than
a civil-law one: **tax analysis of the financial statements** (derivation
under art. 83 TUIR, tax-relevant items, IRES estimate, deferred tax
under OIC 25), **civil-to-tax reconciliation** in RF-form style (one row
per adjustment, with the TUIR article and the form line) and the **IRAP
taxable base** (art. 5, D.Lgs. 446/97). The tax sector reaches 11
workflows.

## 🇮🇹 Legal sector in Italian (v0.7.1)

The **27 presets** of the legal sector were translated — the last
English-only content in the catalogue: 14 workflows and 13 column
presets, in Italian legal register. Concepts were **adapted** to Italian
law where needed (for example tenancy protection under L. 392/1978, ISTAT
rent indexation); market terms were left untranslated (SOFR, EURIBOR,
carried interest…).

## 🔗 Workflows valid across sectors (v0.7.3 and v0.7.5)

A workflow can appear in the picker of **several sectors** without
duplicating its definition. First for built-in presets (v0.7.3), then
for **user-created workflows** too (v0.7.5), with a tag picker in the
editor. Existing workflows are unaffected and stay single-sector.

---

## Installation

| Architecture | File |
|---|---|
| Windows x86_64 | `Cove Studio_0.8.0_x64.msi` |
| Windows ARM64 (Snapdragon X Elite) | `Cove Studio_0.8.0_arm64.msi` |

The installer replaces MikeRust and keeps your data (same *upgrade
code*): do not uninstall the previous version, and do not export
anything first. After the first start the `mikerust-data` folder is gone
— it became `cove-studio-data`, with the database named
`cove-studio.db`.

Each MSI carries the native libraries for its architecture
(`onnxruntime.dll` 1.20.0 and `pdfium.dll`): nothing else to install.

### Signature and verification

The application binary and the installer are signed with Azure Trusted
Signing. Windows shows **Jugaad srl** as the publisher: Jugaad provides
the signing infrastructure (see *Acknowledgements* in the README), while
the copyright and the maintenance of the project are Dario Finardi's. To
check:

```powershell
Get-AuthenticodeSignature ".\Cove Studio_0.8.0_arm64.msi" |
  Select-Object Status, @{n='Publisher';e={$_.SignerCertificate.Subject}}
```

Expected: `Status = Valid` and
`CN=Jugaad srl, O=Jugaad srl, L=Montecchio Emilia, S=Reggio Emilia, C=IT`.
The signature is timestamped (RFC 3161), so it stays valid after the
signing certificate itself expires.

## Models are downloaded on first use

The installer contains no model weights, and nothing is downloaded at
startup. Each model is fetched the first time it is actually needed,
with a progress bar in the chat:

| Model | When | Size |
|---|---|---|
| Embeddings, `multilingual-e5-base` INT8 | first indexing or search over documents | ~283 MB |
| GLiNER2 PII | first use of PII protection | ~1.1 GB |
| Local Ollama models | only in secure local mode, after you confirm | varies |
| Corpora (parquet datasets, EUR-Lex…) | only when you add that corpus | varies |

On a machine without internet access the app still starts and chats
work, but document search stays unavailable until the embedding model is
downloaded. Upgrading from MikeRust downloads nothing: the existing
caches are renamed, not re-fetched.

## Notes for people upgrading

* **API keys and PIN** stay where they are: the migration moves the
  database, it does not rewrite it.
* **Exported projects**: `.mikeprj` files remain importable; new exports
  are `.coveprj`.
* **Environment variables**: `MRUST_*` still works, but the new prefix is
  `COVE_*`; `.env.example` is up to date.
* **Orphan chunks**: if you once indexed documents that were later moved
  or deleted, the log reports "orphan KB chunk dropped". Those chunks are
  ignored in answers and can be purged for good with
  `POST /sync/cleanup-orphans`.

## Known limitations in this release

* On some Windows ARM64 configurations the DirectML provider fails to
  register and embeddings run on the CPU: slower, same results.
* `gemini-3.7-flash` may reject requests against an undocumented
  32,768-token ceiling that does not match `countTokens` (reported
  upstream to Google). If it happens, use `gemini-3.8-flash` or
  `gemini-3.5-flash`.
* The `thrift` 0.17 advisory inherited from `parquet` 53 is still open
  (excessive allocation on malformed parquet files): it concerns reading
  third-party parquet files, and fixing it requires the major arrow
  upgrade.

## Provenance and licence

Cove Studio is free software under **AGPL-3.0-only**. The original work
is © 2026 Dario Finardi; the parts still derived from
[`willchen96/mike`](https://github.com/willchen96/mike) — built-in tool
schemas, legal-domain presets, some interface strings — belong to their
authors under the same licence. What derives from what is set out in
`README.md` (section *Provenance and independence from Mike*) and in
`docs/UPSTREAM_SYNC.md`.

The names **Cove Studio** and **MikeRust** and the logo are not covered
by the code licence: see `NOTICE.md`.
