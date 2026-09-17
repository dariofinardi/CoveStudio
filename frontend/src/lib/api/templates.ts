// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { api } from './client'
import type { Domain } from '$lib/types/domain'
import type { DocxTemplate, TemplateDescription } from '$lib/types/template'

// `type` (not interface) so it stays assignable to the client's
// Record<string, …> query parameter.
export type TemplateFilter = {
  domain?: Domain
  locale?: string
}

/** Wrappers for `src/routes/docx_templates.rs`. All require auth. */
export const templatesApi = {
  /** List DOCX templates, optionally filtered by domain + locale prefix. */
  list: (filter?: TemplateFilter) =>
    api<{ docx_templates: DocxTemplate[] }>('/docx-templates', { query: filter }),

  /** Full authoring contract (auto-generated prompt_md + sidecar). */
  describe: (template_id: string, locale?: string) =>
    api<TemplateDescription>('/docx-templates/describe', {
      method: 'POST',
      body: { template_id, locale },
    }),

  /**
   * Render a template to a .docx blob plus the placeholders the backend
   * could not resolve (from the `X-Unresolved-Placeholders` header). A
   * direct fetch is used so the response header stays readable.
   */
  render: async (payload: {
    template_id: string
    body_md: string
    metadata?: Record<string, string>
    filename?: string
  }): Promise<{ blob: Blob; unresolved: string[] }> => {
    const { apiBase } = await import('$lib/stores/api-base.svelte')
    const { authStore } = await import('$lib/stores/auth.svelte')
    const res = await fetch(
      new URL('/docx-templates/render', apiBase.url || 'http://127.0.0.1:3001'),
      {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${authStore.token ?? ''}`,
        },
        body: JSON.stringify(payload),
      },
    )
    if (!res.ok) {
      let detail = `HTTP ${res.status}`
      try {
        const j = (await res.json()) as { detail?: string }
        if (j.detail) detail = j.detail
      } catch {
        /* keep status */
      }
      throw new Error(detail)
    }
    const header = res.headers.get('X-Unresolved-Placeholders') ?? ''
    const unresolved = header
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean)
    return { blob: await res.blob(), unresolved }
  },

  /**
   * Create or update a user template. The `id` must carry the `user/`
   * prefix; the backend writes a JSON file under
   * `config/docx-templates/user/` and re-saving an existing id replaces it.
   */
  save: (template: DocxTemplate) =>
    api<{ saved: boolean; template: DocxTemplate }>('/docx-templates/save', {
      method: 'POST',
      body: template,
    }),

  /** Delete a user template by id. System templates are rejected. */
  remove: (template_id: string) =>
    api<{ deleted: boolean }>('/docx-templates/delete', {
      method: 'POST',
      body: { template_id },
    }),

  /** Ids of templates the user has hidden — a bare string[]. */
  listHidden: () => api<string[]>('/docx-templates/hidden'),

  /** Hide a template (system or user) from the listing. */
  hide: (template_id: string) =>
    api<{ ok: boolean }>('/docx-templates/hidden', {
      method: 'POST',
      body: { template_id },
    }),

  /** Restore a previously hidden template. */
  unhide: (template_id: string) =>
    api<{ ok: boolean }>('/docx-templates/unhide', {
      method: 'POST',
      body: { template_id },
    }),
}
