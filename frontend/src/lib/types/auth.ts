// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

/**
 * Types mirroring `src/routes/auth.rs`. The backend is a single-user
 * local app: exactly one profile row exists at a time.
 */

/** The `{ id, username, display_name }` shape returned by setup/unlock/me. */
export interface SessionUser {
  id: string
  username: string
  display_name: string | null
}

/** `GET /auth/status` — first-run detection + cold-start rehydration. */
export type AuthStatus =
  | { setup_required: true }
  | {
      setup_required: false
      user: SessionUser
      biometric_enrolled: boolean
    }

/** Success body of `POST /auth/setup`, `/auth/unlock`, `/auth/unlock-biometric`. */
export interface AuthSuccess {
  token: string
  user: SessionUser
}

/** `GET /auth/biometric-available`. */
export interface BiometricAvailability {
  /** Hardware + OS support present (Windows Hello / Touch ID). */
  available: boolean
  /** This profile has opted in to biometric unlock. */
  enabled: boolean
}

/** PIN must be 4–8 digits (backend `validate_pin_format`). */
export const PIN_MIN_LENGTH = 4
export const PIN_MAX_LENGTH = 8

export function isValidPinFormat(pin: string): boolean {
  return /^[0-9]{4,8}$/.test(pin)
}
