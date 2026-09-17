// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { api } from './client'
import type { Workflow, WorkflowFilter } from '$lib/types/workflow'

/** Wrappers for `src/routes/workflows.rs`. All endpoints require auth. */
export const workflowsApi = {
  /** List DB workflows + system presets, AND-filtered by type/domain. */
  list: (filter?: WorkflowFilter) =>
    api<{ workflows: Workflow[] }>('/workflow', { query: filter }),

  get: (id: string) => api<Workflow>(`/workflow/${encodeURIComponent(id)}`),

  create: (payload: Partial<Workflow>) =>
    api<Workflow>('/workflow', { method: 'POST', body: payload }),

  update: (id: string, payload: Partial<Workflow>) =>
    api<Workflow>(`/workflow/${encodeURIComponent(id)}`, { method: 'PUT', body: payload }),

  remove: (id: string) =>
    api<{ ok: boolean }>(`/workflow/${encodeURIComponent(id)}`, { method: 'DELETE' }),

  /** Hidden built-in workflow ids — backend returns a bare string[]. */
  listHidden: () => api<string[]>('/workflow/hidden'),

  hide: (workflow_id: string) =>
    api<{ ok: boolean }>('/workflow/hidden', { method: 'POST', body: { workflow_id } }),

  unhide: (workflow_id: string) =>
    api<{ ok: boolean }>(`/workflow/hidden/${encodeURIComponent(workflow_id)}`, {
      method: 'DELETE',
    }),

  /** Translate a prompt into the target locale, preserving Markdown. */
  translate: (text: string, target_locale: string) =>
    api<{ text: string }>('/workflow/translate', {
      method: 'POST',
      body: { text, target_locale },
    }),
}
