// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import type { Domain } from './domain'

/** Types mirroring `src/routes/workflows.rs`. */

export type WorkflowType = 'assistant' | 'tabular'

/**
 * Tabular-column output formats. Canonical snake_case ids — the value
 * stored in `columns_config[].format`; UI labels are localised.
 */
export const COLUMN_FORMATS = [
  'free_text',
  'bulleted_list',
  'number',
  'percentage',
  'monetary_amount',
  'currency',
  'yes_no',
  'date',
  'tags',
] as const
export type ColumnFormat = (typeof COLUMN_FORMATS)[number]

/**
 * Per-column config for `tabular` workflows. Shipped presets use
 * `name` for the column title and `text`/`tag` for some formats;
 * the create modal historically wrote `label`. The helpers below
 * paper over both shapes.
 */
export interface WorkflowColumn {
  key?: string
  /** Column title — preset shape. */
  name?: string
  /** Column title — legacy create-modal shape. */
  label?: string
  prompt?: string
  format?: string
  [extra: string]: unknown
}

/** Resolve a column's display title across the preset/legacy shapes. */
export function columnTitle(
  col: { name?: string; label?: string; key?: string },
  index = 0,
): string {
  return col.name || col.label || col.key || `#${index + 1}`
}

/** Map a raw preset format onto a canonical {@link ColumnFormat}. */
export function normalizeColumnFormat(raw: string | undefined): ColumnFormat {
  const v = (raw ?? '').trim().toLowerCase()
  if ((COLUMN_FORMATS as readonly string[]).includes(v)) return v as ColumnFormat
  const alias: Record<string, ColumnFormat> = {
    text: 'free_text',
    free_text: 'free_text',
    freetext: 'free_text',
    tag: 'tags',
    bullet_list: 'bulleted_list',
    list: 'bulleted_list',
    money: 'monetary_amount',
    boolean: 'yes_no',
  }
  return alias[v] ?? 'free_text'
}

export interface Workflow {
  id: string
  /** null for system presets, a user id for DB-stored workflows. */
  user_id: string | null
  title: string
  type: WorkflowType
  prompt_md: string | null
  columns_config: WorkflowColumn[]
  practice: string | null
  domain: Domain
  /** Extra domains this workflow also surfaces under (migration 0034). */
  also_applicable_to: Domain[]
  created_at: string
  /** true = shipped preset (config/workflows/*), not editable. */
  is_system: boolean
  /** true = the current user owns this DB row. */
  is_owner: boolean
}

// A `type` (not `interface`) so it stays assignable to the API client's
// `Record<string, …>` query parameter — interfaces lack an implicit
// index signature and TS rejects them there.
export type WorkflowFilter = {
  type?: WorkflowType
  domain?: Domain
}
