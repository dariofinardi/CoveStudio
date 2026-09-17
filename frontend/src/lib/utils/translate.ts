// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

/**
 * Bulk translation helper. The backend `/workflow/translate` endpoint
 * translates one text per request; a workflow or DOCX template has many
 * fields, so translating them one-by-one is unbearably slow. This runs
 * the requests through a small concurrency pool and guards each one with
 * a timeout so a single stuck call can never hang the whole operation.
 */

import { workflowsApi } from '$lib/api/workflows'
import type { Locale } from '$lib/types/user'

export interface TranslateJob {
  /** Current text to translate. */
  text: string
  /** Write the translated text back into the model. */
  apply: (translated: string) => void
}

/** Max requests in flight at once — fast without tripping rate limits. */
const POOL_SIZE = 4
/** Per-request ceiling; a call slower than this is abandoned as failed. */
const REQUEST_TIMEOUT_MS = 90_000

function withTimeout<T>(p: Promise<T>, ms: number): Promise<T> {
  return Promise.race([
    p,
    new Promise<never>((_, reject) =>
      setTimeout(() => reject(new Error(`translation timed out after ${ms / 1000}s`)), ms),
    ),
  ])
}

/**
 * Translate every non-empty job into `locale`, concurrently. Individual
 * failures are collected, not thrown — every other field still gets
 * translated. Returns the first error encountered, or `null` on success.
 * `onProgress` fires after each job with `(done, total)`.
 */
export async function translateAll(
  jobs: TranslateJob[],
  locale: Locale,
  onProgress?: (done: number, total: number) => void,
): Promise<Error | null> {
  const pending = jobs.filter((j) => j.text.trim().length > 0)
  const total = pending.length
  let done = 0
  let firstError: Error | null = null
  onProgress?.(0, total)
  if (total === 0) return null

  let next = 0
  async function worker(): Promise<void> {
    while (next < total) {
      const job = pending[next++]
      try {
        const res = await withTimeout(workflowsApi.translate(job.text, locale), REQUEST_TIMEOUT_MS)
        job.apply(res.text)
      } catch (e) {
        if (!firstError) firstError = e as Error
      }
      done++
      onProgress?.(done, total)
    }
  }

  await Promise.all(Array.from({ length: Math.min(POOL_SIZE, total) }, () => worker()))
  return firstError
}
