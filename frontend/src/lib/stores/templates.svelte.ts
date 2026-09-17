// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { templatesApi } from '$lib/api/templates'
import type { DocxTemplate } from '$lib/types/template'

function createTemplateStore() {
  let items = $state<DocxTemplate[]>([])
  let hidden = $state<Set<string>>(new Set())
  let loading = $state<boolean>(false)
  let error = $state<string | null>(null)

  /** Distinct locale codes present in the loaded set (for the filter). */
  const locales = $derived([...new Set(items.map((t) => t.locale))].sort())

  /** Templates the user has not hidden. */
  const visible = $derived(items.filter((t) => !hidden.has(t.id)))

  return {
    get items() {
      return items
    },
    get visible() {
      return visible
    },
    get hidden() {
      return hidden
    },
    get locales() {
      return locales
    },
    get loading() {
      return loading
    },
    get error() {
      return error
    },

    isHidden(id: string) {
      return hidden.has(id)
    },

    async refresh() {
      loading = true
      error = null
      try {
        const [res, hiddenIds] = await Promise.all([
          templatesApi.list(),
          templatesApi.listHidden(),
        ])
        items = res.docx_templates
        hidden = new Set(hiddenIds)
      } catch (e) {
        error = (e as Error).message
      } finally {
        loading = false
      }
    },

    async hide(id: string) {
      await templatesApi.hide(id)
      hidden = new Set([...hidden, id])
    },

    async unhide(id: string) {
      await templatesApi.unhide(id)
      const next = new Set(hidden)
      next.delete(id)
      hidden = next
    },
  }
}

export const templateStore = createTemplateStore()
