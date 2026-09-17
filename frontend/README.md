# Cove Studio frontend

Svelte 5 desktop interface of Cove Studio (formerly MikeRust), served
inside the Tauri shell.

- **Stack:** Tauri 2 · Svelte 5 (runes) · TypeScript · Tailwind CSS v4 · Vite
- **Licence:** AGPL-3.0-only (see [LICENSE](LICENSE) and [../NOTICE.md](../NOTICE.md))
- **History:** written from 15 May 2026 following
  [../docs/mikerust-ui-rewrite-plan.md](../docs/mikerust-ui-rewrite-plan.md);
  it replaced the Next.js frontend forked from Mike on 17 May 2026.

## Develop

From the repository root:

```pwsh
# 1. Install dependencies (once)
pnpm --dir frontend install

# 2. Launch the desktop app with the Svelte frontend
.\frontend\node_modules\.bin\tauri.cmd dev --config src-tauri/tauri.svelte.conf.json
```

## Scripts (`pnpm <script>` inside this directory)

| Script           | Action                                  |
|------------------|-----------------------------------------|
| `dev`            | Vite dev server on `127.0.0.1:5173`     |
| `build`          | Type-check + production build to `dist` |
| `preview`        | Serve `dist` for local preview          |
| `typecheck`      | `svelte-check`                          |
| `lint`           | ESLint on `src/`                        |
| `format`         | Prettier write                          |
| `test`           | Vitest unit suite                       |
| `test:watch`     | Vitest watch mode                       |
| `test:e2e`       | Playwright suite                        |
| `license-audit`  | Reject non-permissive transitive deps   |

## Layout

```
frontend/
├── src/
│   ├── lib/
│   │   ├── api/        ← HTTP wrappers for the backend routes
│   │   ├── components/ ← UI primitives and feature components
│   │   ├── stores/     ← Svelte 5 runes state (one file per resource)
│   │   ├── tauri/      ← invoke wrappers for the Tauri commands
│   │   ├── types/      ← TypeScript mirrors of the Rust serde structs
│   │   ├── utils/      ← citations, highlight, markdown, sse, download, …
│   │   └── product.ts  ← product name, project file format, preference keys
│   ├── routes/         ← Boot / Setup / Unlock / Assistant / Projects / …
│   ├── App.svelte
│   ├── app.css
│   └── main.ts
├── locales/            ← i18n catalogues (en canonical + it/fr/de/es/pt)
├── public/             ← static assets bundled as-is
└── tests/              ← unit (Vitest) + e2e (Playwright)
```

## Conventions

- Every user-facing string goes through `i18n.t('Namespace.key')`, with the
  key present in all six catalogues; `scripts/fill-i18n.mjs` checks parity.
  Translations can use `{product}` and `{projectExt}`, filled from
  `src/lib/product.ts`.
- User preferences are saved through the `/user/*` endpoints; `localStorage`
  is used only for device-local display preferences.
- Schema identifiers (enum values, JSON keys, route parameters) stay in
  English; only display labels are localised.
- `xlsx` (SheetJS) is vendored: `vendor/xlsx-0.20.3.tgz`, the official
  tarball from `https://cdn.sheetjs.com/xlsx-0.20.3/xlsx-0.20.3.tgz`. The
  npm registry copy stopped at 0.18.5, which carries the prototype
  pollution (GHSA-4r6h-8v6p-xvw6, fixed in 0.19.3) and ReDoS
  (GHSA-5pgg-2g8v-p4x9, fixed in 0.20.2) advisories with no npm release to
  upgrade to. To move to a newer SheetJS, download the new tarball into
  `vendor/`, point the dependency at it and delete the old one.

## Backend contract

The frontend is an HTTP client of the axum backend at
`http://127.0.0.1:<port>`, discovered at startup through the
`api_base_url` Tauri command. The other Tauri commands only cover what a
web page cannot do: `open_external_url`, `open_external_path` and
`pick_folder`.
