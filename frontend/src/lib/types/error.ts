// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

/**
 * Every axum error response carries `{ "detail": "<message>" }`.
 * `ApiError` normalises that plus the HTTP status; status 0 is reserved
 * for client-side network failures (fetch threw before any response).
 */
export class ApiError extends Error {
  constructor(
    public readonly status: number,
    public readonly detail: string,
    /** Seconds from a `Retry-After` header, when the server sent one. */
    public readonly retryAfter?: number,
  ) {
    super(detail)
    this.name = 'ApiError'
  }

  get isNetwork(): boolean {
    return this.status === 0
  }

  get isUnauthorized(): boolean {
    return this.status === 401
  }

  get isRateLimited(): boolean {
    return this.status === 429
  }
}
