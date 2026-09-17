// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import type { Domain } from './domain'

/** Types mirroring `src/routes/projects.rs`. */

/** RAG retrieval scope for chats inside a project. */
export type IsolationMode = 'shared' | 'strict'

/** Shape from `GET /project` (list). */
export interface Project {
  id: string
  name: string
  description: string | null
  domain: Domain
  created_at: string
  updated_at: string
}

/** `GET /project/{id}` adds the isolation mode. */
export interface ProjectDetail extends Project {
  isolation_mode: IsolationMode
}

export interface CreateProjectBody {
  name: string
  description?: string
  domain?: Domain
}

export interface UpdateProjectBody {
  name?: string
  description?: string
  isolation_mode?: IsolationMode
  domain?: Domain
}
