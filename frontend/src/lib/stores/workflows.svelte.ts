// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { workflowsApi } from '$lib/api/workflows'
import type { Domain } from '$lib/types/domain'
import type { Workflow, WorkflowFilter } from '$lib/types/workflow'

function createWorkflowStore() {
  let items = $state<Workflow[]>([])
  let hidden = $state<Set<string>>(new Set())
  let loading = $state<boolean>(false)
  let error = $state<string | null>(null)

  /** Workflows the user has not hidden. */
  const visible = $derived(items.filter((w) => !hidden.has(w.id)))

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
    get loading() {
      return loading
    },
    get error() {
      return error
    },

    isHidden(id: string) {
      return hidden.has(id)
    },

    async refresh(filter?: WorkflowFilter) {
      loading = true
      error = null
      try {
        const [list, hiddenIds] = await Promise.all([
          workflowsApi.list(filter),
          workflowsApi.listHidden(),
        ])
        items = list.workflows
        hidden = new Set(hiddenIds)
      } catch (e) {
        error = (e as Error).message
      } finally {
        loading = false
      }
    },

    async hide(id: string) {
      await workflowsApi.hide(id)
      hidden = new Set([...hidden, id])
    },

    async unhide(id: string) {
      await workflowsApi.unhide(id)
      const next = new Set(hidden)
      next.delete(id)
      hidden = next
    },

    async remove(id: string) {
      await workflowsApi.remove(id)
      items = items.filter((w) => w.id !== id)
    },

    countByDomain(domain: Domain): number {
      return items.filter((w) => w.domain === domain).length
    },
  }
}

export const workflowStore = createWorkflowStore()
