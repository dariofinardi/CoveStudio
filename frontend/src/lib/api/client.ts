// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { apiBase } from '$lib/stores/api-base.svelte'
import { authStore } from '$lib/stores/auth.svelte'
import { ApiError } from '$lib/types/error'

export interface RequestOptions {
  method?: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE'
  /** JSON body — serialized automatically. Ignored when `multipart` is set. */
  body?: unknown
  query?: Record<string, string | number | boolean | undefined | null>
  signal?: AbortSignal
  /** Return the raw Blob instead of parsing JSON. */
  asBlob?: boolean
  /** Send a FormData body (multipart upload) instead of JSON. */
  multipart?: FormData
  /** Skip attaching the bearer token (used by unauthenticated probes). */
  noAuth?: boolean
}

function buildUrl(path: string, query?: RequestOptions['query']): string {
  const base = apiBase.url || 'http://127.0.0.1:3001'
  const url = new URL(path, base)
  if (query) {
    for (const [k, v] of Object.entries(query)) {
      if (v !== undefined && v !== null && v !== '') {
        url.searchParams.set(k, String(v))
      }
    }
  }
  return url.toString()
}

/**
 * Typed HTTP client for the axum backend. Centralises auth header
 * injection, the uniform `{ detail }` error shape, and 401 handling
 * (drops the cached token so the router falls back to Unlock).
 */
export async function api<T>(path: string, opts: RequestOptions = {}): Promise<T> {
  const headers: Record<string, string> = { Accept: 'application/json' }
  if (!opts.noAuth && authStore.token) {
    headers.Authorization = `Bearer ${authStore.token}`
  }

  let body: BodyInit | undefined
  if (opts.multipart) {
    body = opts.multipart // browser sets the multipart Content-Type + boundary
  } else if (opts.body !== undefined) {
    headers['Content-Type'] = 'application/json'
    body = JSON.stringify(opts.body)
  }

  let res: Response
  try {
    res = await fetch(buildUrl(path, opts.query), {
      method: opts.method ?? 'GET',
      headers,
      body,
      signal: opts.signal,
    })
  } catch (e) {
    if ((e as Error).name === 'AbortError') throw e
    throw new ApiError(0, `Network error: ${(e as Error).message}`)
  }

  if (res.status === 401 && !opts.noAuth) {
    authStore.invalidate()
    throw new ApiError(401, 'Session expired')
  }

  if (!res.ok) {
    let detail = res.statusText || `HTTP ${res.status}`
    try {
      const j = (await res.json()) as { detail?: string }
      if (j.detail) detail = j.detail
    } catch {
      // non-JSON error body — keep statusText
    }
    const retryHeader = res.headers.get('Retry-After')
    const retryAfter = retryHeader ? Number(retryHeader) : undefined
    throw new ApiError(res.status, detail, Number.isNaN(retryAfter) ? undefined : retryAfter)
  }

  if (opts.asBlob) return (await res.blob()) as T
  if (res.status === 204) return undefined as T

  const text = await res.text()
  if (!text) return undefined as T
  return JSON.parse(text) as T
}
