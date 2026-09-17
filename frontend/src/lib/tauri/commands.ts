// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { invoke } from '@tauri-apps/api/core'

/**
 * Discover the axum backend base URL.
 *
 * The Tauri shell spawns the backend on a free port at startup and reports
 * it back via the `api_base_url` command. The first `invoke` can race the
 * backend startup: the WebView boots in ~100ms but the backend's AppState (81
 * workflow presets + 30 column presets + 13 docx templates + 5 model
 * providers + DB migrations + ort init) takes ~1s to settle before
 * `port_tx` fires. In that window the command returns an empty string and
 * the old code immediately fell through to the `:3001` fallback, never
 * retrying — that's the "Network error: Failed to fetch" the user saw on
 * cold launch even though the backend was about to start a few hundred
 * ms later.
 *
 * Now we poll with exponential backoff up to 30 s: 50 ms, 75 ms, 113 ms…
 * capped at 1 s. The backend almost always reports its port within the
 * first second, so on the happy path this adds one or two extra IPC
 * round-trips. If we're not running inside Tauri (e.g. `vite dev`
 * opened in a regular browser for component dev), the very first
 * `invoke` throws — we bail out of the loop immediately to the
 * `VITE_API_BASE_URL` env var, then to the canonical localhost
 * default.
 */
export async function getApiBaseUrl(): Promise<string> {
  const START = Date.now()
  const MAX_WAIT_MS = 30_000
  let delay = 50
  while (Date.now() - START < MAX_WAIT_MS) {
    let u: string
    try {
      u = await invoke<string>('api_base_url')
    } catch {
      // Not in Tauri context — no point polling.
      break
    }
    if (u) return u
    await new Promise((resolve) => setTimeout(resolve, delay))
    delay = Math.min(Math.round(delay * 1.5), 1000)
  }
  return import.meta.env.VITE_API_BASE_URL ?? 'http://127.0.0.1:3001'
}

/**
 * Open an external http(s) URL in the user's default browser instead of
 * letting the Tauri WebView intercept it. Throws on any non-http(s) input.
 */
export async function openExternal(url: string): Promise<void> {
  if (!/^https?:\/\//.test(url)) {
    throw new Error('openExternal: only http(s) URLs allowed')
  }
  await invoke('open_external_url', { url })
}

/**
 * Open a *file path* with the OS's default associated application.
 * Backed by the `open_external_path` Tauri command, which validates
 * the path against the user's Cove Studio storage root before launching
 * — see src-tauri/src/lib.rs for the security model. Used by the
 * DocViewerPanel "Open in Word" button so the user can run Word's
 * native track-changes accept/reject workflow on a model-generated
 * docx. The path must be the absolute path returned by the backend
 * (`GET /document/:id/file_path`); fabricating one client-side will
 * fail the prefix check on the Rust side.
 */
export async function openExternalPath(path: string): Promise<void> {
  await invoke('open_external_path', { path })
}

/**
 * Open the native folder picker. Returns the chosen absolute path, or
 * null if the user cancelled or we are not running inside Tauri.
 */
export async function pickFolder(): Promise<string | null> {
  try {
    return (await invoke<string | null>('pick_folder')) ?? null
  } catch {
    return null
  }
}
