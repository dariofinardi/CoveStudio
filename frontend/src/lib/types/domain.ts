// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { i18n } from '$lib/stores/i18n.svelte'

/**
 * Professional verticals. Canonical English snake_case IDs — these are
 * the values that travel over the wire and live in the DB. Mirror of
 * `crate::domain::DOMAINS`. UI labels are localised via the i18n
 * `Domains.*` namespace; never translate the IDs themselves.
 */
export const DOMAINS = [
  'legal',
  'medical',
  'finance',
  'fiscale',
  'real_estate',
  'hr',
  'insurance',
  'ip',
  'compliance',
  'gdpr',
  'pa',
  'others',
] as const

export type Domain = (typeof DOMAINS)[number]

/** Per-row schema default (migration 0018) and frontend fallback. */
export const DEFAULT_DOMAIN: Domain = 'legal'

export function isDomain(value: unknown): value is Domain {
  return typeof value === 'string' && (DOMAINS as readonly string[]).includes(value)
}

/**
 * Localised display label for a domain id, via the i18n `Domains.values.*`
 * namespace. Canonical IDs never change; only the rendered label does.
 */
export function domainLabel(domain: string): string {
  return i18n.t(`Domains.values.${domain}`)
}
