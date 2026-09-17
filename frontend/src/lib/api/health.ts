// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { api } from './client'
import type { HealthReport } from '$lib/types/health'

/** `GET /healthz` — unauthenticated liveness/readiness probe. */
export const healthApi = {
  get: () => api<HealthReport>('/healthz', { noAuth: true }),
}
