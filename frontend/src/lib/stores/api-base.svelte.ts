// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { getApiBaseUrl } from '$lib/tauri/commands'

function createApiBaseStore() {
  let url = $state<string>('')
  let resolved = $state<boolean>(false)

  return {
    get url() {
      return url
    },
    get resolved() {
      return resolved
    },
    async hydrate(): Promise<string> {
      url = await getApiBaseUrl()
      resolved = true
      return url
    },
  }
}

export const apiBase = createApiBaseStore()
