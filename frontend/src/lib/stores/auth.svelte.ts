// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { authApi } from '$lib/api/auth'
import type { SessionUser } from '$lib/types/auth'

/**
 * Session state. Per plan decision Q10 the bearer token lives in memory
 * only — it is intentionally NOT persisted to localStorage/sessionStorage.
 * Opt-in encrypted persistence via tauri-plugin-stronghold is a later
 * task; until then a process restart means a fresh unlock.
 */
function createAuthStore() {
  let token = $state<string | null>(null)
  let user = $state<SessionUser | null>(null)
  let biometricEnrolled = $state<boolean>(false)

  const isAuthenticated = $derived(token !== null && user !== null)

  function adopt(t: string, u: SessionUser) {
    token = t
    user = u
  }

  return {
    get token() {
      return token
    },
    get user() {
      return user
    },
    get biometricEnrolled() {
      return biometricEnrolled
    },
    get isAuthenticated() {
      return isAuthenticated
    },

    setBiometricEnrolled(value: boolean) {
      biometricEnrolled = value
    },

    async setup(payload: { username: string; pin: string; display_name?: string }) {
      const res = await authApi.setup(payload)
      adopt(res.token, res.user)
      return res.user
    },

    async unlock(pin: string) {
      const res = await authApi.unlock(pin)
      adopt(res.token, res.user)
      return res.user
    },

    async unlockBiometric() {
      const res = await authApi.unlockBiometric()
      adopt(res.token, res.user)
      return res.user
    },

    /** Revoke server-side sessions and wipe local state. */
    async logout() {
      try {
        await authApi.logout()
      } catch {
        // best-effort: even if the call fails, drop the local token
      }
      token = null
      user = null
    },

    /** Called by the API client on any 401 — drops the dead token. */
    invalidate() {
      token = null
      user = null
    },
  }
}

export const authStore = createAuthStore()
