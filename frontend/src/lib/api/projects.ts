// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { api } from './client'
import type { Domain } from '$lib/types/domain'
import type {
  CreateProjectBody,
  Project,
  ProjectDetail,
  UpdateProjectBody,
} from '$lib/types/project'

// `type` (not interface) — assignable to the client's query Record.
export type ProjectFilter = {
  domain?: Domain
}

/** A node of a project's document-folder tree. parent_id null = root. */
export interface ProjectFolder {
  id: string
  parent_id: string | null
  name: string
  created_at: string
}

/** Wrappers for `src/routes/projects.rs`. All require auth. */
export const projectsApi = {
  list: (filter?: ProjectFilter) =>
    api<{ projects: Project[] }>('/project', { query: filter }),

  get: (id: string) => api<ProjectDetail>(`/project/${encodeURIComponent(id)}`),

  create: (body: CreateProjectBody) =>
    api<{ id: string; name: string; domain: Domain }>('/project', {
      method: 'POST',
      body,
    }),

  update: (id: string, body: UpdateProjectBody) =>
    api<{ ok: boolean }>(`/project/${encodeURIComponent(id)}`, { method: 'PUT', body }),

  remove: (id: string) =>
    api<{ ok: boolean }>(`/project/${encodeURIComponent(id)}`, { method: 'DELETE' }),

  /** Rename a document inside a project. */
  renameDocument: (id: string, docId: string, filename: string) =>
    api<{ ok: boolean }>(
      `/project/${encodeURIComponent(id)}/documents/${encodeURIComponent(docId)}`,
      { method: 'PATCH', body: { filename } },
    ),

  // ── document folder tree ──────────────────────────────────────────
  listFolders: (id: string) =>
    api<{ folders: ProjectFolder[] }>(
      `/project/${encodeURIComponent(id)}/folders`,
    ),

  createFolder: (id: string, name: string, parent_id: string | null = null) =>
    api<{ id: string }>(`/project/${encodeURIComponent(id)}/folders`, {
      method: 'POST',
      body: { name, parent_id },
    }),

  /** Rename (`name`) and/or re-parent (`parent_id`, null = root) a folder. */
  updateFolder: (
    id: string,
    folderId: string,
    body: { name?: string; parent_id?: string | null },
  ) =>
    api<{ id: string }>(
      `/project/${encodeURIComponent(id)}/folders/${encodeURIComponent(folderId)}`,
      { method: 'PATCH', body },
    ),

  deleteFolder: (id: string, folderId: string) =>
    api<{ ok: boolean }>(
      `/project/${encodeURIComponent(id)}/folders/${encodeURIComponent(folderId)}`,
      { method: 'DELETE' },
    ),

  /** Move a document into a folder (`folder_id` null = project root). */
  moveDocument: (id: string, docId: string, folder_id: string | null) =>
    api<{ id: string }>(
      `/project/${encodeURIComponent(id)}/documents/${encodeURIComponent(docId)}/folder`,
      { method: 'PATCH', body: { folder_id } },
    ),

  /** Export to an encrypted project file blob. */
  exportProject: (id: string, recipient_email: string, include_chats = false) =>
    api<Blob>(`/project/${encodeURIComponent(id)}/export`, {
      method: 'POST',
      body: { recipient_email, include_chats },
      asBlob: true,
    }),

  /** Import a project file blob. */
  importProject: (file: File, recipient_email: string) => {
    const fd = new FormData()
    fd.append('file', file)
    fd.append('recipient_email', recipient_email)
    return api<{ ok: boolean; project_id: string; document_count: number; chat_count: number }>(
      '/project/import',
      { method: 'POST', multipart: fd },
    )
  },
}
