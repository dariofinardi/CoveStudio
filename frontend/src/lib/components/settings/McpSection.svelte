<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Settings → MCP servers. List of per-user MCP server configs with a
  simplified add/edit form (name + URL + API key, per plan Q9 — the
  transport is auto-detected by the probe). Connection test surfaces
  the discovered transport and tool/prompt/resource counts.
-->
<script lang="ts">
  import Card from '$lib/components/ui/Card.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import IconButton from '$lib/components/ui/IconButton.svelte'
  import Input from '$lib/components/ui/Input.svelte'
  import Checkbox from '$lib/components/ui/Checkbox.svelte'
  import Toggle from '$lib/components/ui/Toggle.svelte'
  import Badge from '$lib/components/ui/Badge.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import Modal from '$lib/components/ui/Modal.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import ConfirmDialog from '$lib/components/ui/ConfirmDialog.svelte'
  import { mcpStore } from '$lib/stores/mcp.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { ApiError } from '$lib/types/error'
  import type { McpServer } from '$lib/types/user'
  import type { McpProbeResult } from '$lib/api/user'
  import { Pencil, Trash2 } from 'lucide-svelte'

  $effect(() => {
    void mcpStore.refresh()
  })

  let formOpen = $state(false)
  let editingName = $state<string | null>(null) // non-null = edit (name locked)
  let fName = $state('')
  let fUrl = $state('')
  let fKey = $state('')
  let fEnabled = $state(true)
  let saving = $state(false)
  let formError = $state<string | null>(null)

  let probing = $state(false)
  let probeResult = $state<McpProbeResult | null>(null)
  let probeError = $state<string | null>(null)

  let deleteTarget = $state<string | null>(null)

  const canSave = $derived(
    fName.trim().length > 0 && fUrl.trim().length > 0 && !saving
  )

  function openAdd() {
    editingName = null
    fName = ''
    fUrl = ''
    fKey = ''
    fEnabled = true
    formError = null
    probeResult = null
    probeError = null
    formOpen = true
  }

  function openEdit(s: McpServer) {
    editingName = s.name
    fName = s.name
    fUrl = s.url ?? ''
    fKey = s.api_key ?? ''
    fEnabled = s.enabled
    formError = null
    probeResult = null
    probeError = null
    formOpen = true
  }

  async function runProbe() {
    probing = true
    probeResult = null
    probeError = null
    try {
      probeResult = await mcpStore.probe(fUrl.trim(), fKey.trim() || undefined)
    } catch (e) {
      probeError = e instanceof ApiError ? e.detail : (e as Error).message
    } finally {
      probing = false
    }
  }

  async function save() {
    saving = true
    formError = null
    try {
      await mcpStore.upsert({
        name: fName.trim(),
        url: fUrl.trim(),
        api_key: fKey.trim() || undefined,
        enabled: fEnabled,
      })
      toastStore.success(
        editingName ? i18n.t('Settings.serverUpdated') : i18n.t('Settings.serverAdded')
      )
      formOpen = false
    } catch (e) {
      formError = e instanceof ApiError ? e.detail : (e as Error).message
    } finally {
      saving = false
    }
  }

  async function toggleEnabled(s: McpServer, next: boolean) {
    try {
      await mcpStore.upsert({
        name: s.name,
        transport: s.transport,
        url: s.url,
        api_key: s.api_key,
        enabled: next,
      })
    } catch (e) {
      toastStore.danger(i18n.t('Settings.serverUpdateError'), { detail: (e as Error).message })
    }
  }

  async function confirmDelete() {
    if (!deleteTarget) return
    try {
      await mcpStore.remove(deleteTarget)
      toastStore.info(i18n.t('Settings.serverRemoved'))
    } catch (e) {
      toastStore.danger(i18n.t('Settings.serverRemoveError'), { detail: (e as Error).message })
    } finally {
      // open is passed one-way (open={deleteTarget !== null}); clearing
      // the target here is what actually closes the dialog.
      deleteTarget = null
    }
  }
</script>

<Card>
  {#snippet header()}
    <h3 class="text-sm font-semibold text-(--color-text-primary)">{i18n.t('Settings.mcpServers')}</h3>
    <Button size="sm" onclick={openAdd}>{i18n.t('Settings.addServer')}</Button>
  {/snippet}

  {#if mcpStore.loading}
    <div class="flex items-center gap-2 text-sm text-(--color-text-secondary) py-6 justify-center">
      <Spinner size="sm" />
      {i18n.t('Settings.loadingServers')}
    </div>
  {:else if mcpStore.error}
    <EmptyState title={i18n.t('Settings.loadServersError')} description={mcpStore.error}>
      {#snippet action()}
        <Button size="sm" variant="secondary" onclick={() => mcpStore.refresh()}>
          {i18n.t('Common.retry')}
        </Button>
      {/snippet}
    </EmptyState>
  {:else if mcpStore.servers.length === 0}
    <EmptyState
      title={i18n.t('Settings.noServers')}
      description={i18n.t('Settings.noServersHint')}
    />
  {:else}
    <ul class="flex flex-col gap-2">
      {#each mcpStore.servers as s (s.name)}
        <li class="flex items-center gap-3 px-3 py-2.5 border border-(--color-surface-200) rounded-(--radius-md)">
          <div class="flex-1 min-w-0">
            <div class="flex items-center gap-2">
              <span class="text-sm font-medium text-(--color-text-primary) truncate">{s.name}</span>
              <Badge tone="neutral" size="xs">{s.transport}</Badge>
            </div>
            {#if s.url}
              <p class="text-xs text-(--color-text-secondary) font-mono truncate">{s.url}</p>
            {/if}
          </div>
          <Toggle
            size="sm"
            checked={s.enabled}
            onchange={(v) => toggleEnabled(s, v)}
          />
          <IconButton label={i18n.t('Settings.editServer')} size="sm" onclick={() => openEdit(s)}>
            <Pencil size={14} />
          </IconButton>
          <IconButton
            label={i18n.t('Settings.removeServer')}
            size="sm"
            variant="danger"
            onclick={() => (deleteTarget = s.name)}
          >
            <Trash2 size={14} />
          </IconButton>
        </li>
      {/each}
    </ul>
  {/if}
</Card>

<Modal
  bind:open={formOpen}
  title={editingName ? i18n.t('Settings.editServerTitle') : i18n.t('Settings.addServerTitle')}
  size="md"
>
  <div class="space-y-3">
    <Input label={i18n.t('Common.name')} bind:value={fName} disabled={!!editingName} placeholder="my-server" />
    <Input label={i18n.t('Settings.url')} bind:value={fUrl} placeholder="https://example.com/mcp" />
    <Input
      label={i18n.t('Settings.apiKeyOptional')}
      type="password"
      bind:value={fKey}
      autocomplete="off"
    />
    <Checkbox label={i18n.t('Settings.enabled')} bind:checked={fEnabled} />

    <div class="flex items-center gap-2 pt-1">
      <Button
        size="sm"
        variant="secondary"
        loading={probing}
        disabled={fUrl.trim().length === 0 || probing}
        onclick={runProbe}
      >
        {i18n.t('Settings.testConnection')}
      </Button>
      <span class="text-xs text-(--color-text-secondary)">
        {i18n.t('Settings.transportHint')}
      </span>
    </div>

    {#if probeError}
      <p class="text-sm text-(--color-danger-500)">{probeError}</p>
    {:else if probeResult}
      <div class="text-xs rounded-(--radius-md) bg-(--color-surface-50) border border-(--color-surface-200) p-3 space-y-1">
        {#if probeResult.ok}
          <p class="text-(--color-success-500) font-medium">
            {i18n.t('Settings.probeConnected', { transport: probeResult.transport_detected ?? '' })}
          </p>
          <p class="text-(--color-text-secondary)">
            {i18n.t('Settings.probeCounts', {
              tools: probeResult.tool_count ?? 0,
              prompts: probeResult.prompt_count ?? 0,
              resources: probeResult.resource_count ?? 0,
            })}
          </p>
          {#if probeResult.suggested_url}
            <p class="text-(--color-text-secondary)">
              {i18n.t('Settings.probeDiscoveredPath')}
              <span class="font-mono">{probeResult.suggested_url}</span>
            </p>
          {/if}
        {:else}
          <p class="text-(--color-warning-500) font-medium">
            {i18n.t('Settings.probeTransport', { transport: probeResult.transport_detected ?? '' })}
          </p>
          {#if probeResult.hint}<p class="text-(--color-text-secondary)">{probeResult.hint}</p>{/if}
        {/if}
      </div>
    {/if}

    {#if formError}
      <p class="text-sm text-(--color-danger-500)">{formError}</p>
    {/if}
  </div>

  {#snippet footer()}
    <Button variant="ghost" onclick={() => (formOpen = false)}>{i18n.t('Common.cancel')}</Button>
    <Button loading={saving} disabled={!canSave} onclick={save}>
      {editingName ? i18n.t('Common.save') : i18n.t('Settings.addServer')}
    </Button>
  {/snippet}
</Modal>

<ConfirmDialog
  open={deleteTarget !== null}
  title={i18n.t('Settings.removeServerConfirmTitle')}
  message={i18n.t('Settings.removeServerConfirmBody', { name: deleteTarget ?? '' })}
  confirmLabel={i18n.t('Settings.remove')}
  danger
  onconfirm={confirmDelete}
  oncancel={() => (deleteTarget = null)}
/>
