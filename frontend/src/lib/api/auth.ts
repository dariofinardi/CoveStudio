// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { api } from './client'
import type { AuthStatus, AuthSuccess, BiometricAvailability, SessionUser } from '$lib/types/auth'

/** Wrappers for `src/routes/auth.rs`. */
export const authApi = {
  /** First-run detection + cold-start rehydration. No auth. */
  status: () => api<AuthStatus>('/auth/status', { noAuth: true }),

  /** Create the single local profile. No auth. */
  setup: (payload: { username: string; pin: string; display_name?: string }) =>
    api<AuthSuccess>('/auth/setup', { method: 'POST', body: payload, noAuth: true }),

  /** PIN login. No auth. */
  unlock: (pin: string) =>
    api<AuthSuccess>('/auth/unlock', { method: 'POST', body: { pin }, noAuth: true }),

  /** Windows Hello / Touch ID login (no body — backend reads the profile). */
  unlockBiometric: () =>
    api<AuthSuccess>('/auth/unlock-biometric', { method: 'POST', noAuth: true }),

  /** Hardware support + per-profile opt-in state. No auth. */
  biometricAvailable: () =>
    api<BiometricAvailability>('/auth/biometric-available', { noAuth: true }),

  /** Opt this profile in to biometric unlock (requires biometric verify). Auth. */
  biometricEnable: () =>
    api<{ ok: boolean; enabled: boolean }>('/auth/biometric-enable', { method: 'POST' }),

  /** Opt out. Auth. */
  biometricDisable: () =>
    api<{ ok: boolean; enabled: boolean }>('/auth/biometric-disable', { method: 'POST' }),

  /** Change the PIN. Auth. */
  changePin: (current_pin: string, new_pin: string) =>
    api<{ ok: boolean }>('/auth/change-pin', {
      method: 'POST',
      body: { current_pin, new_pin },
    }),

  /** Reset the PIN by proving identity with biometrics (forgot-PIN path). Auth. */
  changePinBiometric: (new_pin: string) =>
    api<{ ok: boolean }>('/auth/change-pin-biometric', {
      method: 'POST',
      body: { new_pin },
    }),

  /** Revoke all sessions for the current user. Auth. */
  logout: () => api<{ ok: boolean }>('/auth/logout', { method: 'POST' }),

  /** Validate the cached token + rehydrate the profile. Auth (401 if stale). */
  me: () => api<SessionUser>('/auth/me'),
}
