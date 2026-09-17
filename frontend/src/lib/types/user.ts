// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

/** Types mirroring `src/routes/user.rs`. */

/** `GET /user/profile`. */
export interface UserProfile {
  id: string
  username: string
  display_name: string | null
  created_at: string
}

/** Supported UI locales. English is canonical (plan §14, decision Q8). */
export const LOCALES = ['en', 'it', 'fr', 'de', 'es', 'pt'] as const
export type Locale = (typeof LOCALES)[number]
export const DEFAULT_LOCALE: Locale = 'en'

export function isLocale(value: unknown): value is Locale {
  return typeof value === 'string' && (LOCALES as readonly string[]).includes(value)
}

/** Native names of the supported locales — for language pickers. */
export const LOCALE_LABELS: Record<Locale, string> = {
  en: 'English',
  it: 'Italiano',
  fr: 'Français',
  de: 'Deutsch',
  es: 'Español',
  pt: 'Português',
}

/** Active LLM provider. */
export type LlmProvider = 'anthropic' | 'google' | 'openai' | 'mistral' | 'local'

/**
 * `GET/PUT /user/llm-settings`. Every field is optional: PUT has patch
 * semantics (absent/null = unchanged; empty string = explicit clear).
 */
export interface LlmSettings {
  main_model?: string | null
  title_model?: string | null
  tabular_model?: string | null
  claude_api_key?: string | null
  gemini_api_key?: string | null
  gemini_region?: string | null
  gemini_model?: string | null
  openai_api_key?: string | null
  openai_model?: string | null
  local_base_url?: string | null
  local_api_key?: string | null
  local_model?: string | null
  active_provider?: LlmProvider | null
  mistral_api_key?: string | null
  mistral_model?: string | null
  /** v0.5.6: "Modalità sicura locale" toggle. ON → local provider
   *  pins to loopback + models of the local-models catalogue only. */
  local_secure_mode?: boolean
  /** v0.6.0: Mistral `safe_prompt` request param. OFF by default;
   *  ON applies Mistral's upstream safety wrapper. */
  mistral_safe_prompt?: boolean
  /** v0.6.0: Mistral `parallel_tool_calls`. OFF by default
   *  (sequential tool execution for predictable legal workflows);
   *  ON re-enables Mistral's upstream parallel default. */
  mistral_parallel_tools?: boolean
  /** Same field on PUT — kept Partial<LlmSettings> compatible. */
  hyde_enabled?: boolean
}

/** One row of the v0.5.6 curated-models catalogue (GET
 *  /user/local-secure/models). Mirrors `local_secure_models` in
 *  `src/routes/user.rs`. */
export interface CuratedModelEntry {
  /** Catalogue id (`config/local-models/ollama.json`). This is what
   *  gets written to `user_settings.local_model`. */
  id: string
  /** Upstream tag Ollama needs to pull (`qwen2.5:3b-instruct-q4_K_M`
   *  or `hf.co/…:Q4_K_M`). Shown in the Settings UI as the source. */
  base_model: string
  display_name: string
  /** Approximate on-disk footprint in GB. Shown next to the entry. */
  approx_size_gb: number
  /** Minimum recommended RAM in GB. */
  min_ram_gb: number
  /** Is the catalogue derivation present? Drives the
   *  Installato / Installa decision in the UI. */
  ready: boolean
  /** Is the BASE model already on disk? Lets the UI show
   *  "wrapper missing — fast install" vs "pulling X GB". */
  base_present: boolean
  /** Names from earlier releases under which this model is still
   *  installed in Ollama (e.g. `old-name:latest`). */
  legacy_installed: string[]
}

/** Result of POST /user/local-secure/migrate. */
export interface LocalModelMigrationReport {
  /** Models now available under the current name. */
  migrated: { from: string; to: string }[]
  /** Models that could not be copied: they must be installed again. */
  failed: { from: string; to: string; error: string }[]
  /** Previous names still present in Ollama, removable after consent. */
  leftovers: string[]
}

/** SSE event yielded by `POST /user/local-secure/ensure/{id}`. The
 *  variants mirror `EnsureEvent` in `src/llm/ollama_manager.rs` —
 *  serde tag = "phase". */
export type LocalSecureEnsureEvent =
  | { phase: 'started'; model_id: string }
  | {
      phase: 'pulling'
      status: string
      completed_bytes: number
      total_bytes: number
    }
  | { phase: 'creating'; model_id: string }
  | { phase: 'ready'; model_id: string }
  | { phase: 'error'; message: string }

/** MCP server config — mirror of `McpServerOut` in user.rs. */
export type McpTransport = 'http' | 'sse' | 'stdio'

export interface McpServer {
  name: string
  transport: McpTransport
  url?: string
  command?: string
  args: string[]
  env: Record<string, unknown>
  headers: Record<string, unknown>
  api_key?: string
  enabled: boolean
}
