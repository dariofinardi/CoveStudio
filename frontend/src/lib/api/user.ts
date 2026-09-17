// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { api } from './client'
import type { Domain } from '$lib/types/domain'
import type {
  CuratedModelEntry,
  LlmSettings,
  Locale,
  LocalSecureEnsureEvent,
  LocalModelMigrationReport,
  McpServer,
  McpTransport,
  UserProfile,
} from '$lib/types/user'

/** Input for upsertMcpServer — every field but `name` is optional. */
export interface McpServerInput {
  name: string
  transport?: McpTransport
  url?: string
  command?: string
  args?: string[]
  env?: Record<string, unknown>
  headers?: Record<string, unknown>
  api_key?: string
  enabled?: boolean
}

/** Wrappers for `src/routes/user.rs`. All endpoints require auth. */
export const userApi = {
  getProfile: () => api<UserProfile>('/user/profile'),

  updateProfile: (display_name: string | null) =>
    api<{ ok: boolean }>('/user/profile', { method: 'PUT', body: { display_name } }),

  getLocale: () => api<{ locale: Locale | null }>('/user/locale'),

  updateLocale: (locale: Locale) =>
    api<{ ok: boolean; locale: Locale }>('/user/locale', { method: 'PUT', body: { locale } }),

  getDefaultDomain: () => api<{ default_domain: Domain | null }>('/user/default-domain'),

  updateDefaultDomain: (default_domain: Domain) =>
    api<{ ok: boolean; default_domain: Domain }>('/user/default-domain', {
      method: 'PUT',
      body: { default_domain },
    }),

  /** `null` = no explicit preference (every domain enabled). */
  getEnabledDomains: () =>
    api<{ enabled_domains: Domain[] | null }>('/user/enabled-domains'),

  updateEnabledDomains: (enabled_domains: Domain[]) =>
    api<{ ok: boolean; enabled_domains: Domain[] | null }>('/user/enabled-domains', {
      method: 'PUT',
      body: { enabled_domains },
    }),

  /** Opt-in HyDE (Hypothetical Document Embeddings) for chat-time
   *  retrieval. See `src/routes/chat.rs::retrieve_kb_chunks` +
   *  `src/llm/hyde.rs`. Persisted in `user_settings.hyde_enabled`
   *  (migration 0030). Default `false`. */
  getHydeEnabled: () => api<{ hyde_enabled: boolean }>('/user/hyde-enabled'),

  updateHydeEnabled: (hyde_enabled: boolean) =>
    api<{ ok: boolean; hyde_enabled: boolean }>('/user/hyde-enabled', {
      method: 'PUT',
      body: { hyde_enabled },
    }),

  getLlmSettings: () => api<LlmSettings>('/user/llm-settings'),

  /** Patch semantics: omit fields to leave them unchanged. */
  updateLlmSettings: (patch: Partial<LlmSettings>) =>
    api<{ ok: boolean }>('/user/llm-settings', { method: 'PUT', body: patch }),

  // -------------------------------------------------------------------
  // v0.5.6 "Modalità sicura locale" plug-and-play endpoints. Backed by
  // the four /user/local-secure/* routes in src/routes/user.rs.
  // -------------------------------------------------------------------

  /** Is Ollama serving on the loopback port we'd use in secure mode? */
  localSecureHeartbeat: () =>
    api<{ ollama_running: boolean; base_url: string }>(
      '/user/local-secure/heartbeat',
    ),

  /** Curated catalogue + per-entry installed/ready flags. */
  localSecureModels: () =>
    api<{ models: CuratedModelEntry[] }>('/user/local-secure/models'),

  /**
   * Open an SSE stream against POST /user/local-secure/ensure/{id}.
   * Returns the underlying `EventSource` so the caller can listen for
   * messages and close it on unmount. We use a hand-built fetch +
   * stream reader rather than `EventSource` because EventSource only
   * supports GET, and the route is POST (the body would be empty
   * anyway, but the route shape matters for the auth middleware).
   *
   * Pass an `AbortSignal` (from a per-row `AbortController`) to cancel
   * the install mid-flight. Aborting closes the fetch → axum drops the
   * SSE stream → ollama-rs drops its `pull_model_stream` → the TCP
   * connection to Ollama closes → Ollama treats the pull as cancelled.
   * Parallel installs already worked because each call carries its own
   * fetch + reader + state; the abort signal is the missing piece that
   * makes them feel symmetric (start any time, stop any time).
   */
  localSecureEnsureStream: async (
    modelId: string,
    onEvent: (ev: LocalSecureEnsureEvent) => void,
    signal?: AbortSignal,
  ): Promise<void> => {
    // Reuse the same base + token handling as `api()` so the SSE call
    // hits the same backend instance and carries the auth cookie.
    const { apiBase } = await import('$lib/stores/api-base.svelte')
    const { authStore } = await import('$lib/stores/auth.svelte')
    const base = apiBase.url || 'http://127.0.0.1:3001'
    const headers: Record<string, string> = { Accept: 'text/event-stream' }
    if (authStore.token) headers.Authorization = `Bearer ${authStore.token}`
    const res = await fetch(
      `${base}/user/local-secure/ensure/${encodeURIComponent(modelId)}`,
      { method: 'POST', headers, signal },
    )
    if (!res.ok || !res.body) {
      throw new Error(`local-secure/ensure failed: HTTP ${res.status}`)
    }
    const reader = res.body.getReader()
    const decoder = new TextDecoder()
    let buf = ''
    while (true) {
      const { value, done } = await reader.read()
      if (done) break
      buf += decoder.decode(value, { stream: true })
      // Split on the SSE record separator (blank line). Each record is
      // a `data: …` line; the backend uses .json_data() so each is a
      // complete JSON object on a single data: line.
      let idx
      while ((idx = buf.indexOf('\n\n')) >= 0) {
        const record = buf.slice(0, idx)
        buf = buf.slice(idx + 2)
        for (const line of record.split('\n')) {
          const t = line.trim()
          if (!t.startsWith('data:')) continue
          try {
            const payload = JSON.parse(t.slice(5).trim()) as LocalSecureEnsureEvent
            onEvent(payload)
          } catch {
            // SSE keep-alive comments (`: keep-alive`) hit this path
            // — silent drop is intentional.
          }
        }
      }
    }
  },

  /** Remove a catalogue model's derivation (keeps the base model on disk). */
  localSecureUninstall: (modelId: string) =>
    api<{ ok: boolean }>(
      `/user/local-secure/uninstall/${encodeURIComponent(modelId)}`,
      { method: 'DELETE' },
    ),

  /** Copy models installed under previous names to the current names
   *  (no download, nothing deleted) and update the settings using them. */
  localSecureMigrate: () =>
    api<LocalModelMigrationReport>('/user/local-secure/migrate', { method: 'POST' }),

  /** Delete previous model names from Ollama. Call only after the user agreed. */
  localSecureRemoveLegacy: (names: string[]) =>
    api<{ removed: string[] }>('/user/local-secure/remove-legacy', {
      method: 'POST',
      body: { names },
    }),

  listMcpServers: () => api<{ servers: McpServer[] }>('/user/mcp-servers'),

  /**
   * Create or update a server. Only `name` is strictly required — the
   * backend `UpsertMcpBody` applies serde defaults for transport (http)
   * and enabled (true) and treats the rest as optional.
   */
  upsertMcpServer: (server: McpServerInput) =>
    api<{ ok: boolean; name: string }>('/user/mcp-servers', { method: 'POST', body: server }),

  deleteMcpServer: (name: string) =>
    api<{ ok: boolean }>(`/user/mcp-servers/${encodeURIComponent(name)}`, { method: 'DELETE' }),

  probeMcpServer: (payload: { url: string; api_key?: string; headers?: Record<string, unknown> }) =>
    api<McpProbeResult>('/user/mcp-servers/probe', { method: 'POST', body: payload }),

  /** Irreversible — CASCADE-deletes all user data. */
  deleteAccount: () => api<{ ok: boolean }>('/user/account', { method: 'DELETE' }),
}

/** Shape of `POST /user/mcp-servers/probe` (success or transport hint). */
export interface McpProbeResult {
  ok: boolean
  transport_detected?: 'http' | 'sse'
  suggested_url?: string | null
  hint?: string
  server_info?: unknown
  capabilities?: unknown
  instructions?: string | null
  tools?: { name: string; description: string }[]
  tool_count?: number
  prompts?: { name: string; description: string; arguments: unknown[] }[]
  prompt_count?: number
  resources?: { uri: string; name: string; description: string; mimeType: string }[]
  resource_count?: number
}
