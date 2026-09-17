// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

/**
 * Shape returned by `GET /healthz` (axum backend `src/routes/health.rs`).
 * Mirrors the Rust handler verbatim — keep in sync when fields change.
 */
export interface HealthReport {
  status: 'ok' | 'degraded'
  uptime_secs: number
  version: string
  db: {
    size: number
    idle: number
  }
  rag:
    | { status: 'disabled' }
    | { status: 'idle' }
    | { status: 'loading' }
    | { status: 'ready' }
    | {
        status: 'downloading'
        file?: string
        downloaded?: number
        total?: number
      }
    | { status: 'failed'; error: string }
  presets: {
    workflows: number
    columns: number
    docx_templates: number
    model_providers: number
  }
}
