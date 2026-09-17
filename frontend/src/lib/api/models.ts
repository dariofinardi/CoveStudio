// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { api } from './client'
import type { ModelCatalogue } from '$lib/types/model'

/** `GET /models` — read-only LLM provider/model/region catalogue. */
export const modelsApi = {
  catalogue: () => api<ModelCatalogue>('/models'),
}
