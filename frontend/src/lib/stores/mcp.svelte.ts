// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { userApi, type McpProbeResult } from '$lib/api/user'
import type { McpServer } from '$lib/types/user'

/** Per-user MCP server configurations (src/routes/user.rs). */
function createMcpStore() {
  let servers = $state<McpServer[]>([])
  let loading = $state<boolean>(false)
  let error = $state<string | null>(null)

  return {
    get servers() {
      return servers
    },
    get loading() {
      return loading
    },
    get error() {
      return error
    },

    async refresh() {
      loading = true
      error = null
      try {
        const res = await userApi.listMcpServers()
        servers = res.servers
      } catch (e) {
        error = (e as Error).message
      } finally {
        loading = false
      }
    },

    /** Create or update a server, then refresh the list. */
    async upsert(server: {
      name: string
      transport?: McpServer['transport']
      url?: string
      api_key?: string
      enabled: boolean
    }) {
      await userApi.upsertMcpServer(server)
      await this.refresh()
    },

    async remove(name: string) {
      await userApi.deleteMcpServer(name)
      servers = servers.filter((s) => s.name !== name)
    },

    /** Handshake + tools discovery against a URL (no persistence). */
    probe(url: string, api_key?: string): Promise<McpProbeResult> {
      return userApi.probeMcpServer({ url, api_key })
    },
  }
}

export const mcpStore = createMcpStore()
