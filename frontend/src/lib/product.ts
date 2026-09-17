// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

/**
 * Product identity for the UI: display name, exported project format and
 * the prefix of device-local storage keys. Mirrors `src/product.rs` on the
 * backend; this is the single place to edit when the product is renamed.
 * Previous values go into the `LEGACY_*` lists so old project files stay
 * importable and saved device preferences are carried over.
 */

export const PRODUCT_NAME = 'Cove Studio'

/** Extension (without the dot) of encrypted project export files. */
export const PROJECT_FILE_EXTENSION = 'coveprj'

/** Project file extensions from previous releases, still importable. */
export const LEGACY_PROJECT_FILE_EXTENSIONS: readonly string[] = ['mikeprj']

const STORAGE_KEY_PREFIX = 'covestudio'
const LEGACY_STORAGE_KEY_PREFIXES: readonly string[] = ['mikerust']

const PROJECT_FILE_EXTENSIONS = [PROJECT_FILE_EXTENSION, ...LEGACY_PROJECT_FILE_EXTENSIONS]

/** Value for `<input type="file" accept>` covering current and legacy files. */
export const PROJECT_FILE_ACCEPT = PROJECT_FILE_EXTENSIONS.map((ext) => `.${ext}`).join(',')

/** `<stem>.<extension>` for a project export. */
export function projectFileName(stem: string): string {
  return `${stem}.${PROJECT_FILE_EXTENSION}`
}

/** True when the file name carries the current or a legacy project extension. */
export function isProjectFileName(name: string): boolean {
  const lower = name.toLowerCase()
  return PROJECT_FILE_EXTENSIONS.some((ext) => lower.endsWith(`.${ext}`))
}

/**
 * Placeholders every translation may use without passing them explicitly,
 * e.g. "Sblocca {product}" or "un file .{projectExt}".
 */
export const PRODUCT_PLACEHOLDERS: Readonly<Record<string, string>> = {
  product: PRODUCT_NAME,
  projectExt: PROJECT_FILE_EXTENSION,
}

/**
 * Reads a device-local preference. A value saved under a legacy key
 * prefix is moved to the current key on first read. Returns null when
 * storage is unavailable or the key is unset.
 */
export function readLocalPreference(name: string): string | null {
  try {
    const key = `${STORAGE_KEY_PREFIX}.${name}`
    const current = localStorage.getItem(key)
    if (current !== null) return current
    for (const prefix of LEGACY_STORAGE_KEY_PREFIXES) {
      const legacyKey = `${prefix}.${name}`
      const legacy = localStorage.getItem(legacyKey)
      if (legacy !== null) {
        localStorage.setItem(key, legacy)
        localStorage.removeItem(legacyKey)
        return legacy
      }
    }
  } catch {
    // storage unavailable (private mode, blocked site data)
  }
  return null
}

/** Saves a device-local preference; failures are ignored. */
export function writeLocalPreference(name: string, value: string): void {
  try {
    localStorage.setItem(`${STORAGE_KEY_PREFIX}.${name}`, value)
  } catch {
    // quota exceeded or storage unavailable — the in-memory value still applies
  }
}
