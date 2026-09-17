# Relationship with the Mike project

Cove Studio (formerly MikeRust) started in May 2026 from
[`willchen96/mike`](https://github.com/willchen96/mike), the AGPL-3.0 legal
AI assistant by Will Chen. This document records how the two projects
relate today, the policy for following upstream changes, and the history
of what was taken from upstream.

## Current state

| Layer | Status |
|---|---|
| Backend (`src/`) | Original Rust implementation, written from the first commit (8 May 2026) against the HTTP contract that Mike's frontend consumed. The one endpoint ported from an upstream commit (see the 13 May 2026 audit below) was rewritten independently in September 2026. |
| Frontend (`frontend/`) | Original Svelte 5 application, completed on 17 May 2026; the Next.js frontend forked from Mike was removed in commit `0a9bbcf`. |
| Assistant instructions (`config/system-prompts/base.md`) | Rewritten in September 2026; until then the base prompt was adapted from Mike's `SYSTEM_PROMPT`. |
| Built-in tool schemas (`src/llm/builtin_tools.rs`) | **Derived from Mike** — they mirror the tool declarations in Mike's `chatTools.ts`. AGPL-3.0, copyright Will Chen and Mike contributors. |
| Legal-domain presets (`config/workflow-presets/legal/`, `config/column-presets/legal/`) | **Derived from Mike** — based on Mike's built-in workflows, translated into Italian in v0.7.1. AGPL-3.0, copyright Will Chen and Mike contributors. |
| Interface strings (`frontend/locales/`) | About thirty English strings **derived from Mike's interface**; the rest is original. |

The git history before 17 May 2026 contains the forked Mike frontend under
its AGPL-3.0 licence.

## Policy for upstream changes

No source code is copied from upstream anymore. Upstream activity is
reviewed only as a source of information:

- **Security fixes** — read the advisory or the commit message, identify
  the threat, and check whether the equivalent surface exists in Cove
  Studio. If it does, fix it with an independent implementation.
- **Bug reports and UX changes** — treat them as behaviour reports:
  reproduce the behaviour in Cove Studio and implement a fix in its own
  code. Do not port diffs.
- **Everything else** (Express routes, Postgres/Supabase schema, S3/R2,
  Cloudflare/OpenNext deployment, billing and sign-up flows) does not
  apply to a local desktop application and is skipped.

When a review leads to a change, the commit message names the upstream
commit that prompted it and states that the implementation is original.

## History

### 13 May 2026 — review of upstream up to `2e8eafc` (PR #64)

At that date the project still used the forked Next.js frontend.

| Upstream | Outcome at the time | Status today |
|---|---|---|
| `e261d2e` fix(security): scope tabular-review `document_ids` by access (CWE-639) | Not applicable: the vulnerable routes did not exist in MikeRust. | Tabular reviews now have their own ownership checks. |
| `7062a30` fix project folder boundary checks | Not applicable: no equivalent folder routes. | — |
| `f39f175` sync deployment and project page fixes (PR #64) | **Ported**: a refactor of the forked `ProjectPage.tsx` (commits `b60feda`, `a967dab`, `41ea283`, `e9f4f4a`, `0ea5161`) and the backend endpoint `PATCH /project/:id/documents/:doc_id` with its filename helper (commit `ca4073b`). | The React code was removed with the forked frontend on 17 May 2026. The endpoint was rewritten independently in September 2026 (extension taken from the stored file type, filename sanitisation). |
| `bef75b0` add OpenAI model support | Already available in MikeRust. | — |
| `91d0c2a` update Next and Cloudflare deps | Skipped: not used. | — |
| `625bca4` JSONB `shared_with` and path-style S3 | Skipped: Postgres and S3 specific. | — |
| `eb44140` fix(security): HMAC secret fail-fast | Skipped: download-URL HMAC not used. | — |
| `ba6f771`, `7f5dd21` security and backend profile updates | Skipped: Express middleware. | — |
| `af5691e` remove app legal pages | Skipped: hosting concern. | — |
| `a84c1cc` improve setup guidance | Skipped: upstream deployment docs. | — |

No further upstream review has been recorded since.
