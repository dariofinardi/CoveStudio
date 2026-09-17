// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import type { Domain } from './domain'
import type { WorkflowColumn } from './workflow'

/** Types mirroring `src/routes/tabular_reviews.rs`. */

/** One extracted value for a (document, column) pair. */
export interface TabularCell {
  key: string
  /** `rate_limited` (v0.6.3) is the transient state of a cell whose
   *  last backend call returned 429; a frontend timer is scheduled
   *  to retry it via /regenerate-cell with linear backoff. Renders
   *  as an Hourglass icon, distinct from the red AlertCircle that
   *  signals a permanent failure. */
  status: 'pending' | 'generating' | 'done' | 'error' | 'rate_limited'
  content: string
}

/** A document row of a review, with its per-column cells. */
export interface TabularRow {
  id: string
  document_id: string | null
  filename: string | null
  status: string
  cells: TabularCell[]
}

export interface TabularReview {
  id: string
  title: string
  project_id: string | null
  workflow_id: string | null
  /** Column definitions, inherited from the source tabular workflow. */
  columns_config: WorkflowColumn[]
  domain: Domain
  created_at: string
  updated_at: string
  /** Document rows + cells — present on `GET /tabular-review/{id}`. */
  rows?: TabularRow[]
}

/** Body for `POST /tabular-review`. */
export interface CreateTabularReviewBody {
  title?: string
  project_id?: string
  workflow_id?: string
  columns_config?: WorkflowColumn[]
  domain?: Domain
}
