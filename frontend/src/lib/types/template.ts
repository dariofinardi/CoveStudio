// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import type { Domain } from './domain'

/**
 * Types mirroring `src/presets/docx_template.rs` (the `to_api_json()`
 * shape). The full sidecar is typed here because the template editor
 * needs to read and write every field; the index signature is kept so
 * any future field still round-trips untyped.
 */

export type AutomationLevel = 'L1' | 'L2' | 'L3' | 'L4'

export interface Paper {
  size: string
  orientation: string
  format: string
}

export interface UsoBollo {
  line_spacing_pt_exact: number
  lines_per_facciata: number
  facciate_per_foglio: number
  mirror_margins: boolean
  duplex: boolean
  forbid_empty_lines: boolean
  marginal_signature_required: boolean
  signature_exclude_last_page: boolean
}

export interface MarginsCm {
  top: number
  right: number
  bottom: number
  left: number
}

export interface Typography {
  body_font: string
  body_size_pt: number
  line_spacing: number
  paragraph_after_pt: number
  alignment: string
  first_line_indent_cm: number
}

export interface Footnotes {
  font: string
  size_pt: number
  line_spacing: number
}

export interface SectionSkeletonEntry {
  id: string
  title?: string
  render?: string
  guidance?: string
  repeating?: boolean
}

export interface FewShotExample {
  label: string
  path: string
}

export interface DocxTemplate {
  schema_version?: number
  id: string
  /** locale code → display name; pick via templateDisplayName(). */
  display_name: Record<string, string>
  category: string
  domain: Domain
  also_applicable_to: Domain[]
  locale: string
  automation_level: AutomationLevel
  placeholder_syntax: string
  source_reference?: string
  paper: Paper
  uso_bollo?: UsoBollo
  margins_cm: MarginsCm
  typography: Typography
  footnotes?: Footnotes
  style_map_baseline: Record<string, string>
  style_map: Record<string, string>
  directives_supported: string[]
  header_block?: string
  footer_block?: string
  section_numbering: string
  section_skeleton: SectionSkeletonEntry[]
  field_prompts: Record<string, string>
  required_metadata: string[]
  character_limits?: Record<string, number>
  few_shot_examples: FewShotExample[]
  prompt_md_extra?: string
  /** Synthesised by the backend: read-only system vs writable user. */
  is_system: boolean
  is_owner: boolean
  /** Any field not explicitly typed still round-trips. */
  [extra: string]: unknown
}

/** `POST /docx-templates/describe` response. */
export interface TemplateDescription {
  template_id: string
  display_name: string
  prompt_md: string
  sidecar: DocxTemplate
}

/** The six UI locales display names are edited for, in display order. */
export const TEMPLATE_NAME_LOCALES = ['it', 'en', 'fr', 'de', 'es', 'pt'] as const

/** The four placeholder-syntax values the backend accepts. */
export const PLACEHOLDER_SYNTAXES = ['square_brackets', 'docproperty', 'jinja'] as const

/**
 * Resolve a display name for the given UI locale. Falls back to the
 * `en` entry, then to any available entry, then to the raw id.
 */
export function templateDisplayName(t: DocxTemplate, locale: string): string {
  return (
    t.display_name[locale] ??
    t.display_name[locale.slice(0, 2)] ??
    t.display_name.en ??
    Object.values(t.display_name)[0] ??
    t.id
  )
}

/** A fresh, valid blank user template — defaults mirror the Rust loader. */
export function blankUserTemplate(): DocxTemplate {
  return {
    schema_version: 1,
    id: '',
    display_name: {},
    category: '',
    domain: 'legal' as Domain,
    also_applicable_to: [],
    locale: 'it-IT',
    automation_level: 'L1',
    placeholder_syntax: 'square_brackets',
    paper: { size: 'A4', orientation: 'portrait', format: 'standard' },
    margins_cm: { top: 2.5, right: 2.5, bottom: 2.5, left: 2.5 },
    typography: {
      body_font: 'Times New Roman',
      body_size_pt: 12,
      line_spacing: 1.5,
      paragraph_after_pt: 0,
      alignment: 'justify',
      first_line_indent_cm: 0,
    },
    style_map_baseline: {
      body_text: 'Corpo testo',
      section_heading: 'Titolo sezione',
      citation: 'Citazione',
      footnote: 'Note piè pagina',
    },
    style_map: {},
    directives_supported: [],
    section_numbering: 'manual',
    section_skeleton: [],
    field_prompts: {},
    required_metadata: [],
    few_shot_examples: [],
    is_system: false,
    is_owner: true,
  }
}

/** Default uso-bollo block (canonical 25-lines/facciata notarial layout). */
export function defaultUsoBollo(): UsoBollo {
  return {
    line_spacing_pt_exact: 28.35,
    lines_per_facciata: 25,
    facciate_per_foglio: 4,
    mirror_margins: true,
    duplex: true,
    forbid_empty_lines: true,
    marginal_signature_required: true,
    signature_exclude_last_page: true,
  }
}

/** Default footnotes block. */
export function defaultFootnotes(): Footnotes {
  return { font: 'Times New Roman', size_pt: 10, line_spacing: 1 }
}

/** The slug part after the `user/` prefix, or `''` for system ids. */
export function userSlug(id: string): string {
  return id.startsWith('user/') ? id.slice('user/'.length) : ''
}

/** True when `slug` is a backend-acceptable user-template identifier. */
export function isValidSlug(slug: string): boolean {
  return /^[a-z0-9][a-z0-9_-]*$/.test(slug) && slug.length <= 80
}
