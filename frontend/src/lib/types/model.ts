// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

/**
 * Types mirroring `config/model.json`, served verbatim by GET /models.
 * Read-only catalogue — used to drive the Settings → LLM models cards.
 */

export interface CatalogueModel {
  id: string
  display_name: string
  family: string
  tier: string
  context_window: number
  max_output_tokens: number
  supports_vision: boolean
  supports_tools: boolean
  supports_streaming: boolean
  supports_prompt_cache: boolean
  supports_extended_thinking: boolean
  preview: boolean
}

export interface CatalogueRegion {
  id: string
  display_name: string
  is_default: boolean
}

export interface CatalogueProviderAuth {
  kind: string
  env_var: string
  format_hint?: string
}

export interface CatalogueProvider {
  id: string
  display_name: string
  homepage?: string
  docs?: string
  auth: CatalogueProviderAuth
  endpoint?: string
  supports_regions: boolean
  regions: CatalogueRegion[]
  models: CatalogueModel[]
}

export interface ModelCatalogue {
  providers: CatalogueProvider[]
}
