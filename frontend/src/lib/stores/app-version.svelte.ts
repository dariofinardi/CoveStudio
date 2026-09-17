// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

/**
 * The frontend bundle's semver, read from `frontend/package.json` at
 * build time via Vite's first-class JSON import. Synchronous — no
 * Tauri IPC, no Promise dance, no race with template render. The
 * release pipeline keeps this in lock-step with
 * `src-tauri/tauri.svelte.conf.json` and the two Cargo manifests, so
 * this single source is enough to label the running build.
 *
 * v0.4.6 tried `@tauri-apps/api/app::getVersion()` + `$state`. The
 * top-level promise resolved fine in dev, but the assignment inside
 * the async `.then` callback never propagated reactively to the
 * sidebar brand snippet in the installed MSI, so the badge stayed
 * invisible. A build-time constant has no such failure mode.
 */
import pkg from '../../../package.json'

export const APP_VERSION: string = pkg.version
