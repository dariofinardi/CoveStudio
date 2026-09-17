// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

/**
 * v0.6.0 — Global state for the "Modalità sicura locale" plug-and-play
 * install / cancel flow.
 *
 * Why this lives in a singleton store instead of the Settings component
 * that drives the UI: the user can navigate away from Settings → Modelli
 * LLM while a 3 GB pull is in flight, then come back. If we kept the
 * `installProgress` map and the per-id `AbortController` registry in
 * component-local `$state`, the unmount would lose both: the React/Svelte
 * lifecycle has no way to know that "the user just hid the panel" is
 * different from "the user is done with this install". The visible
 * symptom (reported 2026-06-07): user clicks Install → leaves Settings
 * → comes back → "Installa" is clickable again → second click fires a
 * SECOND parallel ensure stream → when the original pull completes the
 * UI suddenly has two downloads racing.
 *
 * The fix: keep both maps at module scope. They survive component
 * mount/unmount because the module is loaded once for the app's
 * lifetime. The component just reads `secureInstall.progress` reactively
 * and forwards user clicks to `secureInstall.install()` / `.cancel()`.
 *
 * The store is intentionally toast-agnostic — it takes
 * `onCancelled` / `onCompleted` / `onError` callbacks rather than
 * importing toastStore + i18n directly. Two reasons: (a) lets the same
 * store be driven from a future test harness without DOM, (b) keeps the
 * localised message wording in the calling component where the rest of
 * the section's strings already live.
 */

import { userApi } from '$lib/api/user'
import type { LocalSecureEnsureEvent } from '$lib/types/user'

export interface InstallProgress {
  phase: LocalSecureEnsureEvent['phase']
  bytes?: number
  total?: number
  status?: string
  error?: string
}

/** Optional per-install callbacks. The store handles the streaming and
 *  the AbortController bookkeeping; these let the UI surface the outcome
 *  with localised toasts. */
export interface InstallCallbacks {
  /** Fired after a successful install (catalogue refresh runs *before*
   *  this so a `await refreshSecureCatalogue()` in the caller can show
   *  the new ready badge). */
  onCompleted?: () => void | Promise<void>
  /** Fired when the user clicked Cancel. */
  onCancelled?: () => void | Promise<void>
  /** Fired on any non-abort error. */
  onError?: (message: string) => void
}

function createStore() {
  // Per-id progress map — exposed reactively so the UI re-renders on
  // every Pulling chunk.
  let progress = $state<Record<string, InstallProgress>>({})
  // Per-id AbortController registry. NOT $state on purpose: (a) it
  // doesn't need to drive a re-render and (b) the calling effect would
  // re-trigger on every set/unset which becomes a cycle when the
  // install resolves and triggers `delete`.
  const aborts = new Map<string, AbortController>()

  function setProg(id: string, next: InstallProgress) {
    progress = { ...progress, [id]: next }
  }

  function clearProg(id: string) {
    const { [id]: _drop, ...rest } = progress
    void _drop
    progress = rest
  }

  return {
    /** Live, per-id snapshot. UI reads this reactively. */
    get progress() {
      return progress
    },

    /** Is there currently an in-flight install for this id?
     *  Returns true while the SSE stream is open (phases started /
     *  pulling / creating). Excludes the terminal `ready` / `error`
     *  states so the UI doesn't keep the disabled spinner once the
     *  stream resolves. */
    isInstalling(id: string): boolean {
      const p = progress[id]
      return !!p && p.phase !== 'ready' && p.phase !== 'error'
    },

    /**
     * Kick off (or no-op rejoin) the install of a curated model.
     * Idempotent: a second call while one is already in flight for the
     * same id is silently dropped — the bug that made re-mounting the
     * Settings panel queue a second pull (2026-06-07) was caused by
     * NOT having this guard.
     */
    async install(id: string, cbs?: InstallCallbacks): Promise<void> {
      if (aborts.has(id)) return
      const controller = new AbortController()
      aborts.set(id, controller)
      setProg(id, { phase: 'started' })
      try {
        await userApi.localSecureEnsureStream(
          id,
          (ev) => {
            if (ev.phase === 'pulling') {
              setProg(id, {
                phase: 'pulling',
                bytes: ev.completed_bytes,
                total: ev.total_bytes,
                status: ev.status,
              })
            } else if (ev.phase === 'error') {
              setProg(id, { phase: 'error', error: ev.message })
              cbs?.onError?.(ev.message)
            } else {
              setProg(id, { phase: ev.phase })
            }
          },
          controller.signal,
        )
        // Stream closed cleanly. If the backend emitted `ready` the
        // final progress[id] reflects that; drop the row so the UI
        // flips back to the catalogue badge.
        if (progress[id]?.phase === 'ready') clearProg(id)
        await cbs?.onCompleted?.()
      } catch (e) {
        const err = e as Error
        if (err.name === 'AbortError') {
          // User clicked Cancel — drop the progress row entirely.
          clearProg(id)
          await cbs?.onCancelled?.()
        } else {
          setProg(id, { phase: 'error', error: err.message })
          cbs?.onError?.(err.message)
        }
      } finally {
        aborts.delete(id)
      }
    },

    /** User clicked Cancel. Aborts the underlying fetch which propagates
     *  to ollama-rs dropping its pull_model_stream → Ollama treats the
     *  pull as cancelled. The partial download stays on disk and a
     *  later re-install resumes from the SHA-256 layer cache. */
    cancel(id: string): void {
      aborts.get(id)?.abort()
    },
  }
}

export const secureInstall = createStore()
