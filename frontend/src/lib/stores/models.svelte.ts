// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { modelsApi } from '$lib/api/models'
import { userApi } from '$lib/api/user'
import type { CatalogueProvider, ModelCatalogue } from '$lib/types/model'
import type { LlmSettings } from '$lib/types/user'

/**
 * LLM model catalogue (read-only, from /models) + the user's persisted
 * LlmSettings (from /user/llm-settings). The Settings → Models section
 * edits a local copy and saves a patch.
 */
function createModelsStore() {
  let catalogue = $state<ModelCatalogue | null>(null)
  let settings = $state<LlmSettings>({})
  let loading = $state<boolean>(false)
  let saving = $state<boolean>(false)
  let error = $state<string | null>(null)

  function providerById(id: string): CatalogueProvider | undefined {
    return catalogue?.providers.find((p) => p.id === id)
  }

  return {
    get catalogue() {
      return catalogue
    },
    get settings() {
      return settings
    },
    get loading() {
      return loading
    },
    get saving() {
      return saving
    },
    get error() {
      return error
    },

    providerById,

    /** Every model across all providers — for the role dropdowns. */
    get allModels() {
      return (catalogue?.providers ?? []).flatMap((p) =>
        p.models.map((m) => ({ provider: p.display_name, providerId: p.id, ...m })),
      )
    },

    async load() {
      loading = true
      error = null
      try {
        const [cat, llm] = await Promise.all([
          modelsApi.catalogue(),
          userApi.getLlmSettings(),
        ])
        catalogue = cat
        settings = llm
      } catch (e) {
        error = (e as Error).message
      } finally {
        loading = false
      }
    },

    /** Persist a patch; merges the result into the local settings. */
    async save(patch: Partial<LlmSettings>) {
      saving = true
      try {
        await userApi.updateLlmSettings(patch)
        settings = { ...settings, ...patch }
      } finally {
        saving = false
      }
    },
  }
}

export const modelsStore = createModelsStore()
