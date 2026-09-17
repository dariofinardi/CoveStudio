// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

/**
 * i18n store (plan §14). English is the canonical locale and the
 * fallback for any key missing from another locale.
 *
 * All six dictionaries are imported statically and bundled — for a
 * desktop app the ~35 KB/locale cost is negligible and it avoids
 * async locale-loading flicker. The translation files themselves are
 * reused from the previous Cove Studio frontend (frontendMike/messages),
 * which is original work of the project owner (plan §14.1).
 */

import { PRODUCT_PLACEHOLDERS } from '$lib/product'
import { DEFAULT_LOCALE, isLocale, type Locale } from '$lib/types/user'
import en from '../../../locales/en.json'
import it from '../../../locales/it.json'
import fr from '../../../locales/fr.json'
import de from '../../../locales/de.json'
import es from '../../../locales/es.json'
import pt from '../../../locales/pt.json'

type Dict = Record<string, unknown>

const DICTS: Record<Locale, Dict> = {
  en: en as Dict,
  it: it as Dict,
  fr: fr as Dict,
  de: de as Dict,
  es: es as Dict,
  pt: pt as Dict,
}

/** Resolve a dotted key path (e.g. `Domains.values.legal`) to a string. */
function resolve(dict: Dict, path: string): string | undefined {
  let cur: unknown = dict
  for (const seg of path.split('.')) {
    if (cur && typeof cur === 'object') {
      cur = (cur as Dict)[seg]
    } else {
      return undefined
    }
  }
  return typeof cur === 'string' ? cur : undefined
}

function createI18n() {
  let locale = $state<Locale>(DEFAULT_LOCALE)
  let dict = $state<Dict>(DICTS[DEFAULT_LOCALE])
  const fallback = DICTS.en

  return {
    get locale() {
      return locale
    },

    setLocale(loc: Locale) {
      locale = loc
      dict = DICTS[loc] ?? DICTS.en
    },

    /** Pick the best initial locale from a BCP-47 tag (e.g. navigator.language). */
    detectFrom(tag: string | undefined) {
      const short = (tag ?? '').slice(0, 2).toLowerCase()
      if (isLocale(short)) this.setLocale(short)
    },

    /**
     * Translate a dotted key. Missing keys fall back to English, then
     * to the raw key itself. `{name}`-style placeholders are filled
     * from `params`.
     */
    t(key: string, params?: Record<string, string | number>): string {
      let raw = resolve(dict, key) ?? resolve(fallback, key) ?? key
      for (const [k, v] of Object.entries({ ...PRODUCT_PLACEHOLDERS, ...params })) {
        raw = raw.replaceAll(`{${k}}`, String(v))
      }
      return raw
    },
  }
}

export const i18n = createI18n()
