// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { tabularApi } from '$lib/api/tabular'
import type { CreateTabularReviewBody, TabularReview } from '$lib/types/tabular'

function createTabularStore() {
  let items = $state<TabularReview[]>([])
  let loading = $state<boolean>(false)
  let error = $state<string | null>(null)
  // Cross-route signal — set by callers (e.g. ProjectDetail) that
  // need the Tabular screen to open straight on a specific review
  // after `router.go('tabular')`. Tabular.svelte consumes and clears
  // it via the `consumePendingDetailId` method below.
  let pendingDetailId = $state<string | null>(null)

  return {
    get items() {
      return items
    },
    get loading() {
      return loading
    },
    get error() {
      return error
    },

    /** Schedule the Tabular screen to drill straight into this review
     *  the next time it mounts. Use with `router.go('tabular')`. */
    selectDetail(id: string) {
      pendingDetailId = id
    },

    /** Read-and-clear the cross-route navigation signal. */
    consumePendingDetailId(): string | null {
      const id = pendingDetailId
      pendingDetailId = null
      return id
    },

    async refresh() {
      loading = true
      error = null
      try {
        items = await tabularApi.list()
      } catch (e) {
        error = (e as Error).message
      } finally {
        loading = false
      }
    },

    async create(body: CreateTabularReviewBody) {
      const res = await tabularApi.create(body)
      await this.refresh()
      return res
    },

    async remove(id: string) {
      await tabularApi.remove(id)
      items = items.filter((r) => r.id !== id)
    },
  }
}

export const tabularStore = createTabularStore()
