// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { api } from './client'
import { apiBase } from '$lib/stores/api-base.svelte'
import { authStore } from '$lib/stores/auth.svelte'
import type { Domain } from '$lib/types/domain'
import type { CreateTabularReviewBody, TabularReview } from '$lib/types/tabular'
import type { WorkflowColumn } from '$lib/types/workflow'

// `type` (not interface) — assignable to the client's query Record.
export type TabularFilter = {
  project_id?: string
  domain?: Domain
}

/** Body for `PATCH /tabular-review/{id}`. */
export interface PatchTabularBody {
  title?: string
  columns_config?: WorkflowColumn[]
  /** Reconciles the review's attached documents to exactly this set. */
  document_ids?: string[]
}

/** Wrappers for `src/routes/tabular_reviews.rs`. All require auth. */
export const tabularApi = {
  /** GET /tabular-review — returns a bare array. */
  list: (filter?: TabularFilter) =>
    api<TabularReview[]>('/tabular-review', { query: filter }),

  /** GET /tabular-review/{id} — review metadata + document rows + cells. */
  get: (id: string) => api<TabularReview>(`/tabular-review/${encodeURIComponent(id)}`),

  create: (body: CreateTabularReviewBody) =>
    api<{ id: string; title: string; domain: Domain }>('/tabular-review', {
      method: 'POST',
      body,
    }),

  /** Update title / columns / attached documents. Returns the fresh review. */
  patch: (id: string, body: PatchTabularBody) =>
    api<TabularReview>(`/tabular-review/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      body,
    }),

  remove: (id: string) =>
    api<{ ok: boolean }>(`/tabular-review/${encodeURIComponent(id)}`, { method: 'DELETE' }),

  /** Regenerate a single cell; returns its new status + content. */
  regenerateCell: (id: string, row_id: string, column_key: string) =>
    api<{ row_id: string; column_key: string; status: string; content: string }>(
      `/tabular-review/${encodeURIComponent(id)}/regenerate-cell`,
      { method: 'POST', body: { row_id, column_key } },
    ),

  /** Reset cells to pending — all rows, or just the given ones. */
  clearCells: (id: string, row_ids?: string[]) =>
    api<{ ok: boolean }>(`/tabular-review/${encodeURIComponent(id)}/clear-cells`, {
      method: 'POST',
      body: { row_ids },
    }),

  /** Import a spreadsheet — one review per worksheet. */
  importXlsx: async (
    file: File,
  ): Promise<{ reviews: { id: string; title: string }[] }> => {
    const res = await fetch(
      new URL('/tabular-review/import', apiBase.url || 'http://127.0.0.1:3001'),
      {
        method: 'POST',
        headers: { Authorization: `Bearer ${authStore.token ?? ''}` },
        body: await file.arrayBuffer(),
      },
    )
    if (!res.ok) {
      let detail = `import failed (${res.status})`
      try {
        detail = ((await res.json()) as { detail?: string }).detail ?? detail
      } catch {
        /* keep the generic message */
      }
      throw new Error(detail)
    }
    return res.json() as Promise<{ reviews: { id: string; title: string }[] }>
  },

  /** Download the review grid as an .xlsx blob. */
  exportXlsx: async (id: string): Promise<Blob> => {
    const res = await fetch(
      new URL(
        `/tabular-review/${encodeURIComponent(id)}/export`,
        apiBase.url || 'http://127.0.0.1:3001',
      ),
      { headers: { Authorization: `Bearer ${authStore.token ?? ''}` } },
    )
    if (!res.ok) throw new Error(`export failed (${res.status})`)
    return res.blob()
  },
}

export interface GenerateCallbacks {
  onCell: (rowId: string, columnKey: string, status: string, content: string) => void
  onError: (message: string) => void
  onDone: () => void
}

// ---------------------------------------------------------------------------
// v0.6.3 — Frontend rate-limit retry loop for Mistral 429 cells.
//
// Backend layers already in place:
//   * v0.6.1 — Mistral retry-with-backoff (1s/2s/4s, total ~7s) on
//     POST /chat/completions.
//   * v0.6.2 — Process-global Semaphore (1 permit) so Cove Studio never
//     issues two Mistral calls concurrently.
//
// Those two protect a SINGLE Mistral call against transient 429.
// They don't help when a tabular review queues 24 cells through
// the semaphore: cell #1 succeeds in ~1s, cell #2 in ~2s, … cell
// #24 has been waiting 24s by then. If Mistral's quota state
// shifts during that window (per-minute reset, monthly quota cliff)
// the backend's own retry budget exhausts on the bottom cells and
// the user sees them as red exclamations.
//
// This frontend layer wraps the picture: every cell that lands as
// `status: "error"` with a 429-ish content gets:
//   1. UI flipped to `rate_limited` (hourglass icon, not red X).
//   2. Scheduled retry via POST /regenerate-cell after a delay
//      that grows linearly: attempt N → N × 5s.
//   3. After MAX_RATE_LIMIT_RETRIES (10) attempts (~50s last wait,
//      ~275s cumulative) we give up and surface the original error.
// ---------------------------------------------------------------------------

const RATE_LIMIT_RETRIES = new Map<string, { attempts: number; timer?: number }>()
const MAX_RATE_LIMIT_RETRIES = 10
const BACKOFF_BASE_MS = 5000

function cellRetryKey(reviewId: string, rowId: string, columnKey: string): string {
  return `${reviewId}/${rowId}/${columnKey}`
}

/** Heuristic match against the Italian backend error string + the
 *  raw status code. Matches both `Mistral 429:` and "rate limit". */
function isRateLimitError(content: string): boolean {
  return /\b429\b|rate[\s_]?limit/i.test(content)
}

function scheduleRateLimitRetry(
  reviewId: string,
  rowId: string,
  columnKey: string,
  cb: GenerateCallbacks,
): void {
  const key = cellRetryKey(reviewId, rowId, columnKey)
  const state = RATE_LIMIT_RETRIES.get(key) ?? { attempts: 0 }
  state.attempts++

  if (state.attempts > MAX_RATE_LIMIT_RETRIES) {
    // Final give-up — surface the error pill and clear state.
    console.warn('[tabular] cell rate-limit exhausted', {
      reviewId,
      rowId,
      columnKey,
      attempts: state.attempts,
    })
    RATE_LIMIT_RETRIES.delete(key)
    cb.onCell(
      rowId,
      columnKey,
      'error',
      `Rate limit non risolto dopo ${MAX_RATE_LIMIT_RETRIES} tentativi.`,
    )
    return
  }

  const delayMs = BACKOFF_BASE_MS * state.attempts
  console.warn('[tabular] cell rate-limited, scheduling retry', {
    reviewId,
    rowId,
    columnKey,
    attempt: `${state.attempts}/${MAX_RATE_LIMIT_RETRIES}`,
    delayMs,
  })

  // UI: hourglass + countdown text. Re-emitted on every retry so
  // the user sees the attempt counter tick up.
  cb.onCell(
    rowId,
    columnKey,
    'rate_limited',
    `Tentativo ${state.attempts}/${MAX_RATE_LIMIT_RETRIES} fra ${Math.round(
      delayMs / 1000,
    )}s`,
  )

  state.timer = window.setTimeout(async () => {
    try {
      const r = await tabularApi.regenerateCell(reviewId, rowId, columnKey)
      const newStatus = String(r.status ?? '')
      const newContent = String(r.content ?? '')
      if (newStatus === 'error' && isRateLimitError(newContent)) {
        // Still rate-limited — loop with growing backoff.
        scheduleRateLimitRetry(reviewId, rowId, columnKey, cb)
      } else {
        // Either success or a different (permanent) error — flush
        // the retry state and let the UI render the result.
        RATE_LIMIT_RETRIES.delete(key)
        cb.onCell(rowId, columnKey, newStatus, newContent)
      }
    } catch (e) {
      RATE_LIMIT_RETRIES.delete(key)
      cb.onCell(rowId, columnKey, 'error', (e as Error).message)
    }
  }, delayMs) as unknown as number

  RATE_LIMIT_RETRIES.set(key, state)
}

/** Cancel every scheduled rate-limit retry for a given review.
 *  Call from the host when the user clicks "Interrompi" or starts
 *  a fresh "Genera" — otherwise the stale timers fire against the
 *  new run and produce confused per-cell updates. */
export function cancelRateLimitRetries(reviewId: string): void {
  for (const [key, state] of RATE_LIMIT_RETRIES.entries()) {
    if (!key.startsWith(`${reviewId}/`)) continue
    if (state.timer != null) window.clearTimeout(state.timer)
    RATE_LIMIT_RETRIES.delete(key)
  }
}

/**
 * Stream a review run. POSTs to `/tabular-review/{id}/generate` and
 * parses the `data: {type}` SSE stream — `cell_update` events update
 * one cell, `done` ends the run. Returns an AbortController.
 */
export function streamGenerate(id: string, cb: GenerateCallbacks): AbortController {
  const ctrl = new AbortController()

  void (async () => {
    let res: Response
    try {
      res = await fetch(
        new URL(
          `/tabular-review/${encodeURIComponent(id)}/generate`,
          apiBase.url || 'http://127.0.0.1:3001',
        ),
        {
          method: 'POST',
          headers: {
            Accept: 'text/event-stream',
            Authorization: `Bearer ${authStore.token ?? ''}`,
          },
          signal: ctrl.signal,
        },
      )
    } catch (e) {
      const err = e as Error
      if (err.name !== 'AbortError') {
        // v0.6.2 diagnostic hook: surface tabular cell errors in
        // the DevTools console too. The tabular UI renders a small
        // red exclamation per failed cell which doesn't carry the
        // backend error string; the console.warn is where the user
        // can triage 429 storms vs auth failures vs network drops.
        console.warn('[tabular] generate stream failed:', err.message, {
          reviewId: id,
        })
        cb.onError(err.message)
      }
      cb.onDone()
      return
    }

    if (!res.ok || !res.body) {
      let detail = `stream failed (${res.status})`
      try {
        const j = (await res.json()) as { detail?: string }
        if (j.detail) detail = j.detail
      } catch {
        /* keep status */
      }
      cb.onError(detail)
      cb.onDone()
      return
    }

    const reader = res.body.getReader()
    const decoder = new TextDecoder()
    let buf = ''
    const dispatch = (chunk: string) => {
      for (const line of chunk.split('\n')) {
        const l = line.replace(/^\s+/, '')
        if (!l.startsWith('data:')) continue
        const data = l.slice(5).trim()
        if (!data || data === '[DONE]') continue
        let ev: Record<string, unknown>
        try {
          ev = JSON.parse(data)
        } catch {
          continue
        }
        if (ev.type === 'cell_update') {
          const rowId = String(ev.row_id ?? '')
          const columnKey = String(ev.column_key ?? '')
          const status = String(ev.status ?? '')
          const content = String(ev.content ?? '')
          // v0.6.3 diagnostic: every per-cell error gets logged so
          // the DevTools console matches the visual cell pills. This
          // is the path silently missed by v0.6.2's hook — cell
          // errors come through cell_update events, not the
          // stream-level `error` event.
          if (status === 'error') {
            console.warn('[tabular] cell error:', content, {
              reviewId: id,
              rowId,
              columnKey,
            })
          }
          // v0.6.3: detect 429-class errors and rewrite the status
          // to `rate_limited`, then schedule a frontend-side retry
          // with growing backoff (5s × attempt — 5s, 10s, 15s…).
          // This sits ON TOP of the backend's 3-attempt retry
          // (v0.6.1) + 1-permit semaphore (v0.6.2); together the
          // three layers handle Experiment-tier 429 storms
          // gracefully even on big tabular reviews.
          if (status === 'error' && isRateLimitError(content)) {
            scheduleRateLimitRetry(id, rowId, columnKey, cb)
            return
          }
          cb.onCell(rowId, columnKey, status, content)
        } else if (ev.type === 'error') {
          // v0.6.2 diagnostic hook: log the backend SSE error event
          // verbatim with the review id so the user can correlate
          // failed cell pills with their backend cause (typically
          // Mistral 429 cascade or auth failure).
          console.warn('[tabular] stream error event:', ev.message, {
            reviewId: id,
            raw: ev,
          })
          cb.onError(String(ev.message ?? 'stream error'))
        }
      }
    }
    try {
      for (;;) {
        const { value, done } = await reader.read()
        if (done) break
        buf += decoder.decode(value, { stream: true })
        let idx: number
        while ((idx = buf.indexOf('\n\n')) >= 0) {
          dispatch(buf.slice(0, idx))
          buf = buf.slice(idx + 2)
        }
      }
      if (buf.trim()) dispatch(buf)
    } catch (e) {
      if ((e as Error).name !== 'AbortError') cb.onError((e as Error).message)
    }
    cb.onDone()
  })()

  return ctrl
}
