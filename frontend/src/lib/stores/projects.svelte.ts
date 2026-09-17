// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { projectsApi } from '$lib/api/projects'
import type { CreateProjectBody, Project, UpdateProjectBody } from '$lib/types/project'

function createProjectStore() {
  let items = $state<Project[]>([])
  let loading = $state<boolean>(false)
  let error = $state<string | null>(null)

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

    async refresh() {
      loading = true
      error = null
      try {
        const res = await projectsApi.list()
        items = res.projects
      } catch (e) {
        error = (e as Error).message
      } finally {
        loading = false
      }
    },

    async create(body: CreateProjectBody) {
      const res = await projectsApi.create(body)
      await this.refresh()
      return res
    },

    async update(id: string, body: UpdateProjectBody) {
      await projectsApi.update(id, body)
      await this.refresh()
    },

    async remove(id: string) {
      await projectsApi.remove(id)
      items = items.filter((p) => p.id !== id)
    },
  }
}

export const projectStore = createProjectStore()
