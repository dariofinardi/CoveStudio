<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Settings → Local document sync. Index local filesystem folders into
  the RAG knowledge base: add folders, scan them (with live progress)
  and inspect per-file results.
-->
<script lang="ts">
  import Card from '$lib/components/ui/Card.svelte'
  import Input from '$lib/components/ui/Input.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import IconButton from '$lib/components/ui/IconButton.svelte'
  import Checkbox from '$lib/components/ui/Checkbox.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import Progress from '$lib/components/ui/Progress.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import ConfirmDialog from '$lib/components/ui/ConfirmDialog.svelte'
  import { syncApi, type SyncFolder, type ScanStatus, type SyncedFile, type ModelStatus } from '$lib/api/data-sources'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { pickFolder } from '$lib/tauri/commands'
  import { Trash2, RefreshCw, ChevronDown, ChevronRight, FolderOpen } from 'lucide-svelte'

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)

  let folders = $state<SyncFolder[]>([])
  let loading = $state(true)
  let model = $state<ModelStatus>({ state: 'idle' })

  let fPath = $state('')
  let fLabel = $state('')
  let fRecursive = $state(true)
  let adding = $state(false)

  let scans = $state<Record<string, ScanStatus>>({})
  let expanded = $state<string | null>(null)
  let files = $state<SyncedFile[]>([])
  let filesLoading = $state(false)
  let removeTarget = $state<SyncFolder | null>(null)

  let pollTimer: ReturnType<typeof setInterval> | undefined

  async function refresh() {
    loading = true
    try {
      folders = await syncApi.listFolders()
    } catch {
      folders = []
    } finally {
      loading = false
    }
  }

  async function refreshModel() {
    try {
      model = await syncApi.modelStatus()
    } catch {
      model = { state: 'unavailable' }
    }
  }

  $effect(() => {
    void refresh()
    void refreshModel()
    return () => clearInterval(pollTimer)
  })

  async function browseFolder() {
    const picked = await pickFolder()
    if (picked) fPath = picked
  }

  async function addFolder() {
    if (!fPath.trim()) return
    adding = true
    try {
      await syncApi.addFolder({
        path: fPath.trim(),
        label: fLabel.trim() || undefined,
        recursive: fRecursive,
      })
      fPath = ''
      fLabel = ''
      fRecursive = true
      await refresh()
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    } finally {
      adding = false
    }
  }

  function startPolling() {
    clearInterval(pollTimer)
    pollTimer = setInterval(async () => {
      let anyRunning = false
      for (const f of folders) {
        try {
          const s = await syncApi.scanStatus(f.id)
          scans[f.id] = s
          if (s.status === 'running') anyRunning = true
        } catch {
          /* ignore */
        }
      }
      void refreshModel()
      if (!anyRunning) {
        clearInterval(pollTimer)
        void refresh()
      }
    }, 1500)
  }

  async function scan(f: SyncFolder) {
    try {
      await syncApi.startScan(f.id)
      scans[f.id] = { status: 'running' }
      startPolling()
    } catch (e) {
      toastStore.danger(t('Sync.scanFailed'), { detail: (e as Error).message })
    }
  }

  async function confirmRemove() {
    if (!removeTarget) return
    const id = removeTarget.id
    removeTarget = null
    try {
      await syncApi.deleteFolder(id)
      folders = folders.filter((f) => f.id !== id)
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    }
  }

  async function toggleFiles(f: SyncFolder) {
    if (expanded === f.id) {
      expanded = null
      return
    }
    expanded = f.id
    filesLoading = true
    files = []
    try {
      files = await syncApi.listFiles(f.id)
    } catch {
      files = []
    } finally {
      filesLoading = false
    }
  }

  const modelBanner = $derived.by(() => {
    if (model.state === 'downloading') {
      return t('Sync.modelDownloading', { file: model.file })
    }
    if (model.state === 'loading') return t('Sync.modelLoading')
    if (model.state === 'failed') return t('Sync.modelFailed', { error: model.error })
    return null
  })
</script>

<div class="space-y-4">
  {#if modelBanner}
    <div class="flex items-center gap-2 px-3 py-2 rounded-(--radius-md) bg-(--color-surface-100) text-xs text-(--color-text-secondary)">
      <Spinner size="sm" />
      <span>{modelBanner}</span>
    </div>
  {/if}

  <Card title={t('Sync.title')} subtitle={t('Sync.subtitle')}>
    <div class="space-y-3">
      <div class="flex items-end gap-2">
        <Input
          label={t('Sync.folderPath')}
          bind:value={fPath}
          placeholder={t('Sync.folderPathPlaceholder')}
          class="flex-1"
        />
        <Button variant="secondary" onclick={browseFolder}>
          <FolderOpen size={15} class="mr-1" />{t('Sync.browse')}
        </Button>
      </div>
      <div class="flex items-end gap-3">
        <Input label={t('Sync.label')} bind:value={fLabel} placeholder={t('Sync.labelPlaceholder')} class="flex-1" />
        <Button loading={adding} disabled={!fPath.trim()} onclick={addFolder}>{t('Sync.addFolder')}</Button>
      </div>
      <Checkbox label={t('Sync.recursive')} bind:checked={fRecursive} />
    </div>
  </Card>

  {#if loading}
    <div class="flex justify-center py-8"><Spinner size="sm" /></div>
  {:else if folders.length === 0}
    <EmptyState title={t('Sync.noFolders')} />
  {:else}
    <ul class="flex flex-col gap-2">
      {#each folders as f (f.id)}
        {@const s = scans[f.id]}
        <li class="bg-(--color-surface-0) border border-(--color-surface-200) rounded-(--radius-md) p-3 space-y-2">
          <div class="flex items-center gap-3">
            <div class="flex-1 min-w-0">
              <p class="text-sm font-medium text-(--color-text-primary) truncate">
                {f.label || f.path}
              </p>
              <p class="text-xs text-(--color-text-secondary) font-mono truncate">{f.path}</p>
            </div>
            <span class="text-xs text-(--color-text-secondary)">
              {t('Sync.lastScan')}: {f.last_scan_at ? new Date(f.last_scan_at).toLocaleString() : t('Sync.never')}
            </span>
            <Button
              size="sm"
              variant="secondary"
              loading={s?.status === 'running'}
              onclick={() => scan(f)}
            >
              <RefreshCw size={13} class="mr-1" />{t('Sync.scan')}
            </Button>
            <IconButton label={t('Sync.showFiles')} size="sm" onclick={() => toggleFiles(f)}>
              {#if expanded === f.id}<ChevronDown size={15} />{:else}<ChevronRight size={15} />{/if}
            </IconButton>
            <IconButton label={t('Sync.remove')} size="sm" variant="danger" onclick={() => (removeTarget = f)}>
              <Trash2 size={14} />
            </IconButton>
          </div>

          {#if s && s.status === 'running'}
            <div class="space-y-1">
              <Progress value={s.total ? (s.processed ?? 0) / s.total : null} />
              <p class="text-xs text-(--color-text-secondary)">
                {s.processed ?? 0}/{s.total ?? 0}
                · {t('Sync.indexed')} {s.indexed ?? 0}
                · {t('Sync.skipped')} {s.skipped ?? 0}
                · {t('Sync.failed')} {s.failed ?? 0}
                {#if s.current_file}· {s.current_file}{/if}
              </p>
            </div>
          {:else if s && (s.status === 'done' || s.status === 'failed')}
            <p class="text-xs text-(--color-text-secondary)">
              {s.status === 'done' ? t('Sync.scanDone') : t('Sync.scanFailed')}
              · {t('Sync.indexed')} {s.indexed ?? 0}
              · {t('Sync.skipped')} {s.skipped ?? 0}
              · {t('Sync.failed')} {s.failed ?? 0}
            </p>
          {/if}

          {#if expanded === f.id}
            <div class="border-t border-(--color-surface-100) pt-2">
              {#if filesLoading}
                <div class="flex justify-center py-3"><Spinner size="sm" /></div>
              {:else if files.length === 0}
                <p class="text-xs text-(--color-text-secondary) py-2">{t('Sync.files')}: 0</p>
              {:else}
                <ul class="flex flex-col gap-0.5 max-h-60 overflow-y-auto">
                  {#each files as file (file.path)}
                    <li class="flex items-center gap-2 text-xs">
                      <span class="flex-1 min-w-0 truncate font-mono text-(--color-text-secondary)">
                        {file.path}
                      </span>
                      <span class="text-(--color-text-disabled)">
                        {file.chunk_count} {t('Sync.chunks')}
                      </span>
                      <span
                        class={file.status === 'failed'
                          ? 'text-(--color-danger-500)'
                          : file.status === 'skipped'
                            ? 'text-(--color-text-disabled)'
                            : 'text-(--color-success-500)'}
                      >
                        {i18n.t(`Sync.fileStatus.${file.status}`)}
                      </span>
                    </li>
                  {/each}
                </ul>
              {/if}
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<ConfirmDialog
  open={removeTarget !== null}
  title={t('Sync.remove')}
  message={t('Sync.removeConfirm')}
  confirmLabel={t('Sync.remove')}
  danger
  onconfirm={confirmRemove}
  oncancel={() => (removeTarget = null)}
/>
