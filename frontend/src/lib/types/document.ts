// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import type { Domain } from './domain'

/** Document metadata from `GET /document` (src/routes/documents.rs). */
export interface DocumentMeta {
  id: string
  filename: string
  file_type: string
  size_bytes: number
  status: string
  domain: Domain
  created_at: string
  /** Owning project id, or null for docs that aren't attached to a
   *  project (synced-folder docs, standalone uploads). The tabular
   *  doc picker uses this to scope its list to the current review's
   *  project plus optionally globals. */
  project_id?: string | null
  /** Folder within the project (`GET /document`). null = project root. */
  project_folder_id?: string | null
  /** Per-chat accept/reject state (migration 0029). Defaults to
   *  `accepted` for legacy rows. */
  decision?: 'accepted' | 'rejected'
  decision_reason?: string | null
  decision_summary?: string | null
}
