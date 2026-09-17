// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

/**
 * Light / dark / system theme (plan §4).
 *
 * The chosen mode maps to a class on <html> consumed by app.css:
 *   light  → .theme-light  (no overrides)
 *   dark   → .theme-dark   (always dark)
 *   system → .theme-system (dark only when the OS prefers dark)
 *
 * Persistence uses localStorage on purpose: theme is a device-local
 * display preference (a user may want dark on a laptop, light on a
 * desktop), not portable account data — so it does not belong on a
 * /user/* endpoint. The value is non-sensitive.
 */
import { readLocalPreference, writeLocalPreference } from '$lib/product'

export type ThemeMode = 'light' | 'dark' | 'system'

const PREFERENCE_NAME = 'theme'
const MODES: ThemeMode[] = ['light', 'dark', 'system']

function isThemeMode(v: unknown): v is ThemeMode {
  return typeof v === 'string' && (MODES as string[]).includes(v)
}

function createThemeStore() {
  let mode = $state<ThemeMode>('system')

  function applyClass() {
    const html = document.documentElement
    html.classList.remove('theme-light', 'theme-dark', 'theme-system')
    html.classList.add(`theme-${mode}`)
  }

  return {
    get mode() {
      return mode
    },

    /** Read the persisted choice and apply it. Call once at startup. */
    init() {
      const saved = readLocalPreference(PREFERENCE_NAME)
      if (isThemeMode(saved)) mode = saved
      applyClass()
    },

    set(next: ThemeMode) {
      mode = next
      applyClass()
      writeLocalPreference(PREFERENCE_NAME, next)
    },
  }
}

export const themeStore = createThemeStore()
