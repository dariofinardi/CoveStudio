# Notice — licensing, names and trademarks

## Code licence

Cove Studio is distributed under the GNU Affero General Public License,
version 3 only (AGPL-3.0-only); see [LICENSE](LICENSE).

- **Original work** — Copyright © 2026 Dario Finardi.
- **Parts derived from Mike** — Copyright © Will Chen and the contributors
  of [`willchen96/mike`](https://github.com/willchen96/mike), AGPL-3.0.
  They are listed in the README section
  [Provenance and independence from Mike](README.md#provenance-and-independence-from-mike):
  the built-in tool schemas and descriptions (`src/llm/builtin_tools.rs`),
  the legal-domain workflow and column presets
  (`config/workflow-presets/legal/`, `config/column-presets/legal/`) and
  some interface strings in `frontend/locales/`.

Anyone may use, study, modify and redistribute the code under the terms of
the AGPL, including running it as a network service with the corresponding
source available.

## Project names and logo

The names **Cove Studio** and **MikeRust** (its former name) and the
project logo identify the upstream project maintained by Dario Finardi.
They are not licensed under the AGPL. If you redistribute a modified
version:

1. use a different name and logo for your distribution, so users do not
   mistake it for the upstream project;
2. keep the AGPL notices, the copyright lines and the attribution to Mike
   and to Cove Studio ("based on Cove Studio" is welcome).

Unmodified redistributions (mirrors, builds of an unchanged release) may
keep the name and logo.

For questions about the names, open an issue at
[github.com/dariofinardi/CoveStudio](https://github.com/dariofinardi/CoveStudio/issues).

## Third-party names

The following names appear in the source as references to external
systems the application integrates with. They belong to their respective
owners and are listed for clarity, not as a claim:

- **Mike** — Will Chen.
- **Anthropic / Claude**, **Google Gemini**, **OpenAI**, **Mistral AI**,
  **Ollama** — model providers and runtimes.
- **HuggingFace** — Hugging Face, Inc.
- **CNIL** — Commission nationale de l'informatique et des libertés.
- **DILA** — Direction de l'information légale et administrative.
- **Légifrance**, **Journal officiel** — French government.
- **EUR-Lex** — Publications Office of the European Union.
- **Normattiva**, **Corte Costituzionale**, **Gazzetta Ufficiale** — Italian
  State.
- **Omissis / Edito** — referenced from the personal-data disclaimer as an
  external redaction service.

Open-data corpora keep the licence of their source (for example Etalab 2.0
for DILA data, CC-BY-4.0 for the Italian legal dataset); the licence of
each corpus is recorded in its manifest under `config/corpora-plugins/`.
