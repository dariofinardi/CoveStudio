<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Tabular-review detail: the extraction grid. Rows are documents,
  columns are the review's questions; "Run" streams the per-cell
  extraction. Cells open a detail panel with the full answer and a
  regenerate action.
-->
<script lang="ts">
  import Badge from '$lib/components/ui/Badge.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import Modal from '$lib/components/ui/Modal.svelte'
  import PickerModal from '$lib/components/ui/PickerModal.svelte'
  import type { PickerItem } from '$lib/components/ui/PickerModal.svelte'
  import { tabularApi, streamGenerate, cancelRateLimitRetries } from '$lib/api/tabular'
  import { documentsApi } from '$lib/api/documents'
  import { projectsApi } from '$lib/api/projects'
  import { docViewer } from '$lib/stores/doc-viewer.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { domainLabel } from '$lib/types/domain'
  import type { TabularReview, TabularRow, TabularCell } from '$lib/types/tabular'
  import { ArrowLeft, Play, Plus, Eraser, AlertCircle, Hourglass, RefreshCw, FileText, Download } from 'lucide-svelte'

  let { id, onback }: { id: string; onback: () => void } = $props()

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)

  let review = $state<TabularReview | null>(null)
  let loading = $state(true)
  let error = $state<string | null>(null)
  let running = $state(false)
  let abortCtrl: AbortController | null = null

  const columns = $derived(review?.columns_config ?? [])
  const rows = $derived(review?.rows ?? [])

  async function load() {
    loading = true
    error = null
    try {
      review = await tabularApi.get(id)
    } catch (e) {
      error = (e as Error).message
    } finally {
      loading = false
    }
  }

  let exporting = $state(false)
  async function exportXlsx() {
    exporting = true
    try {
      const blob = await tabularApi.exportXlsx(id)
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = `${review?.title ?? 'review'}.xlsx`
      a.click()
      URL.revokeObjectURL(url)
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    } finally {
      exporting = false
    }
  }

  $effect(() => {
    void id
    void load()
    return () => abortCtrl?.abort()
  })

  function colKey(col: Record<string, unknown>, i: number): string {
    return (col.key as string) ?? (col.label as string) ?? `col_${i + 1}`
  }
  function colLabel(col: Record<string, unknown>, i: number): string {
    return (
      (col.name as string) ||
      (col.label as string) ||
      (col.key as string) ||
      `#${i + 1}`
    )
  }
  function cellOf(row: TabularRow, key: string): TabularCell | undefined {
    return row.cells.find((c) => c.key === key)
  }

  // ── run ──────────────────────────────────────────────────────────
  function run() {
    if (running || !review) return
    running = true
    error = null
    abortCtrl = streamGenerate(id, {
      onCell: (rowId, key, status, content) => {
        const row = review?.rows?.find((r) => r.id === rowId)
        if (!row) return
        const existing = row.cells.find((c) => c.key === key)
        if (existing) {
          existing.status = status as TabularCell['status']
          existing.content = content
        } else {
          row.cells.push({ key, status: status as TabularCell['status'], content })
        }
      },
      onError: (msg) => {
        error = msg
        toastStore.danger(t('TabularReviews.runError'), { detail: msg })
      },
      onDone: () => {
        running = false
        abortCtrl = null
      },
    })
  }

  function stop() {
    abortCtrl?.abort()
    abortCtrl = null
    running = false
    // Cancel any pending frontend rate-limit retries — leaving
    // them armed would fire setTimeout callbacks against a
    // run the user already aborted, causing zombie cell updates.
    cancelRateLimitRetries(id)
  }

  // ── clear ────────────────────────────────────────────────────────
  async function clearResults() {
    try {
      await tabularApi.clearCells(id)
      await load()
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    }
  }

  // ── add documents ────────────────────────────────────────────────
  let pickerOpen = $state(false)
  let pickerItems = $state<PickerItem[]>([])
  let pickerLoading = $state(false)
  let uploadingDoc = $state(false)
  let uploadInputEl: HTMLInputElement | undefined = $state()
  // Once the user uploads anything during a picker session the changes
  // are already committed server-side, so Confirm should stay enabled
  // (even with nothing ticked) to give them a clean way out.
  let hasUploadedThisSession = $state(false)

  async function refreshPickerItems() {
    // Doc visibility policy in a tabular-review picker:
    //   1. domain must match the review's domain — a "medico" review
    //      never lists "legal" docs and vice versa.
    //   2. if the review is project-scoped:
    //        - rigoroso ("strict")  → only docs that belong to that
    //          project (`project_id === review.project_id`)
    //        - condiviso ("shared") → project docs + globals
    //          (`project_id IS NULL`, which is what synced-folder
    //          uploads and standalone uploads carry)
    //      The project's isolation_mode is what drives the choice;
    //      we fetch it once per picker open (cached via projectCache
    //      across this component's lifetime).
    //   3. if the review has no project, only globals show up.
    // This stops the picker from surfacing every doc the user has
    // ever attached to any chat in any domain — the previous
    // behaviour the user reported on 2026-06-06.
    const dom = review?.domain
    const r = await documentsApi.list(dom ? { domain: dom } : undefined)
    const reviewProjectId = review?.project_id ?? null

    let scopeMode: 'shared' | 'strict' = 'shared'
    if (reviewProjectId) {
      const p =
        projectCache[reviewProjectId] ??
        (await projectsApi.get(reviewProjectId).catch(() => null))
      if (p) {
        projectCache[reviewProjectId] = p
        scopeMode = p.isolation_mode ?? 'shared'
      }
    }

    const filtered = r.documents.filter((d) => {
      const dpid = d.project_id ?? null
      if (reviewProjectId) {
        if (scopeMode === 'strict') return dpid === reviewProjectId
        return dpid === reviewProjectId || dpid === null
      }
      return dpid === null
    })

    pickerItems = filtered.map((d) => ({
      id: d.id,
      label: d.filename,
      sublabel: d.file_type,
    }))
  }

  // Light per-instance cache so a back-to-back open of the picker
  // doesn't refetch the same project. Cleared with the component.
  const projectCache: Record<string, Awaited<ReturnType<typeof projectsApi.get>>> = {}

  async function openPicker() {
    pickerOpen = true
    pickerLoading = true
    pickerItems = []
    hasUploadedThisSession = false
    try {
      await refreshPickerItems()
    } catch {
      pickerItems = []
    } finally {
      pickerLoading = false
    }
  }

  // Opens the OS file picker. The actual upload happens in `onUploadFiles`
  // once the user selects something — the picker resolves synchronously.
  function startUpload() {
    uploadInputEl?.click()
  }

  async function onUploadFiles(ev: Event) {
    const target = ev.target as HTMLInputElement
    const files = target.files ? Array.from(target.files) : []
    if (files.length === 0) return
    uploadingDoc = true
    const uploadedIds: string[] = []
    try {
      // Upload sequentially — the backend extracts text + indexes per
      // file synchronously; running uploads in parallel just spikes
      // CPU on the embedding worker without finishing sooner.
      for (const f of files) {
        const doc = await documentsApi.upload(f, {
          domain: review?.domain,
          // Inherit the review's project so an upload made through the
          // review's picker stays visible under the strict ("rigoroso")
          // scope of that same project. Falls back to global (no
          // project_id) when the review isn't project-bound.
          projectId: review?.project_id ?? undefined,
        })
        uploadedIds.push(doc.id)
      }
      await refreshPickerItems()
      toastStore.success(t('TabularReviews.docUploaded', { count: uploadedIds.length }))
      // Wire the freshly-uploaded docs straight into the review so the
      // user doesn't have to re-select them in the picker. The picker
      // stays open for any follow-up selection from existing docs.
      await onPickDocuments(uploadedIds)
      hasUploadedThisSession = true
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    } finally {
      uploadingDoc = false
      // Reset the input so picking the same file again still triggers `change`.
      if (uploadInputEl) uploadInputEl.value = ''
    }
  }

  async function onPickDocuments(ids: string[]) {
    if (ids.length === 0) return
    const current = rows.map((r) => r.document_id).filter((d): d is string => !!d)
    const merged = Array.from(new Set([...current, ...ids]))
    try {
      review = await tabularApi.patch(id, { document_ids: merged })
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    }
  }

  // ── cell detail ──────────────────────────────────────────────────
  let detailRow = $state<TabularRow | null>(null)
  let detailKey = $state<string | null>(null)
  let regenerating = $state(false)

  const detailColumn = $derived(
    detailKey != null
      ? columns.find((c, i) => colKey(c, i) === detailKey)
      : undefined,
  )
  const detailCell = $derived(
    detailRow && detailKey ? cellOf(detailRow, detailKey) : undefined,
  )

  function openCell(row: TabularRow, key: string) {
    detailRow = row
    detailKey = key
  }

  async function regenerate() {
    if (!detailRow || !detailKey) return
    regenerating = true
    try {
      const res = await tabularApi.regenerateCell(id, detailRow.id, detailKey)
      const cell = cellOf(detailRow, detailKey)
      if (cell) {
        cell.status = res.status as TabularCell['status']
        cell.content = res.content
      }
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    } finally {
      regenerating = false
    }
  }
</script>

<div class="h-full flex flex-col">
  <!--
    Width policy: header + table fill the parent container so the grid
    can use whatever width the window gives us. The container shrinks
    automatically when the resizable / collapsible doc-viewer panel is
    open on the right (it lives outside this component's tree and
    consumes part of the viewport via the same flex parent), and
    expands edge-to-edge with reasonable padding when it isn't. The
    historical `max-w-6xl` was a hold-over from the chat layout and
    wasted horizontal space on wide monitors.
  -->
  <div class="w-full mx-auto px-8 pt-8 pb-3 shrink-0 space-y-4">
    <button
      type="button"
      onclick={onback}
      class="flex items-center gap-1.5 text-sm text-(--color-text-secondary) hover:text-(--color-text-primary)"
    >
      <ArrowLeft size={15} />{t('TabularReviews.title')}
    </button>

    {#if loading}
      <div class="flex items-center gap-2 text-sm text-(--color-text-secondary) py-8 justify-center">
        <Spinner size="sm" />
        {t('Common.loading')}
      </div>
    {:else if error && !review}
      <EmptyState title={t('Errors.somethingWrong')} description={error} />
    {:else if review}
      <header class="flex items-center justify-between gap-4">
        <h2 class="text-2xl font-semibold text-(--color-text-primary)">{review.title}</h2>
        <div class="flex items-center gap-2">
          <Badge tone="brand">{domainLabel(review.domain)}</Badge>
          <Button size="sm" variant="secondary" onclick={openPicker}>
            <Plus size={14} class="mr-1" />{t('TabularReviews.addDocuments')}
          </Button>
          <Button size="sm" variant="ghost" onclick={clearResults} disabled={running}>
            <Eraser size={14} class="mr-1" />{t('TabularReviews.clearResults')}
          </Button>
          <Button
            size="sm"
            variant="ghost"
            onclick={exportXlsx}
            loading={exporting}
            disabled={rows.length === 0}
          >
            <Download size={14} class="mr-1" />{t('TabularReviews.exportToExcel')}
          </Button>
          {#if running}
            <Button size="sm" variant="secondary" onclick={stop}>{t('Common.stop')}</Button>
          {:else}
            <Button size="sm" onclick={run} disabled={rows.length === 0 || columns.length === 0}>
              <Play size={14} class="mr-1" />{t('TabularReviews.run')}
            </Button>
          {/if}
        </div>
      </header>
    {/if}
  </div>

  {#if review && !loading}
    <div class="flex-1 min-h-0 overflow-auto px-8 pb-8">
      {#if rows.length === 0}
        <EmptyState title={t('TabularReviews.noDocuments')}>
          {#snippet action()}
            <Button size="sm" onclick={openPicker}>{t('TabularReviews.addDocuments')}</Button>
          {/snippet}
        </EmptyState>
      {:else}
        <div class="w-full mx-auto border border-(--color-surface-200) rounded-(--radius-md) overflow-auto">
          <table class="w-full border-collapse text-sm">
            <thead>
              <tr class="bg-(--color-surface-50)">
                <th class="sticky left-0 z-10 bg-(--color-surface-50) text-left font-medium
                           text-(--color-text-secondary) px-3 py-2 border-b border-r border-(--color-surface-200)
                           min-w-48">
                  {t('TabularReviews.documentColumn')}
                </th>
                {#each columns as col, ci (colKey(col, ci))}
                  <th class="text-left font-medium text-(--color-text-secondary) px-3 py-2
                             border-b border-r border-(--color-surface-200) min-w-44">
                    {colLabel(col, ci)}
                  </th>
                {/each}
              </tr>
            </thead>
            <tbody>
              {#each rows as row, ri (row.id)}
                <tr class="hover:bg-(--color-hover-bg)">
                  <td class="sticky left-0 z-10 bg-(--color-surface-0) px-3 py-2
                             border-b border-r border-(--color-surface-200) align-top">
                    <button
                      type="button"
                      class="flex items-center gap-1.5 text-left text-(--color-text-primary) hover:underline"
                      onclick={() =>
                        row.document_id &&
                        docViewer.openDocument(row.document_id, row.filename ?? row.document_id)}
                    >
                      <FileText size={13} class="shrink-0 text-(--color-text-secondary)" />
                      <!-- Imported (document-less) rows have no filename
                           — fall back to a 1-based row number. -->
                      <span class="truncate">{row.filename ?? `#${ri + 1}`}</span>
                    </button>
                  </td>
                  {#each columns as col, ci (colKey(col, ci))}
                    {@const cell = cellOf(row, colKey(col, ci))}
                    <td class="px-3 py-2 border-b border-r border-(--color-surface-200) align-top">
                      <button
                        type="button"
                        class="w-full text-left min-h-6"
                        onclick={() => openCell(row, colKey(col, ci))}
                      >
                        {#if !cell || cell.status === 'pending'}
                          <span class="text-(--color-text-disabled)">—</span>
                        {:else if cell.status === 'generating'}
                          <span class="inline-flex items-center gap-1 text-(--color-text-secondary)">
                            <Spinner size="sm" />
                          </span>
                        {:else if cell.status === 'rate_limited'}
                          <!-- v0.6.3 — transient 429 with auto-retry
                               scheduled. Hourglass icon + tooltip
                               with the countdown text from
                               cell.content (e.g. "Tentativo 3/10
                               fra 15s"). Distinct from the red
                               AlertCircle so the user can tell
                               apart "we're waiting for Mistral to
                               cool down" from "permanently failed". -->
                          <span
                            class="inline-flex items-center gap-1 text-(--color-warning-700)"
                            title={cell.content}
                          >
                            <Hourglass size={13} />
                          </span>
                        {:else if cell.status === 'error'}
                          <span class="inline-flex items-center gap-1 text-(--color-danger-500)">
                            <AlertCircle size={13} />
                          </span>
                        {:else}
                          <span class="text-(--color-text-primary) line-clamp-3">
                            {cell.content.split('\n')[0]}
                          </span>
                        {/if}
                      </button>
                    </td>
                  {/each}
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </div>
  {/if}
</div>

<PickerModal
  bind:open={pickerOpen}
  title={t('TabularReviews.addDocuments')}
  items={pickerItems}
  loading={pickerLoading}
  multi
  onpick={onPickDocuments}
  onUpload={startUpload}
  uploading={uploadingDoc}
  confirmAlwaysEnabled={hasUploadedThisSession}
/>
<input
  bind:this={uploadInputEl}
  type="file"
  multiple
  hidden
  onchange={onUploadFiles}
/>

<!-- cell detail -->
<Modal
  open={detailRow !== null && detailKey !== null}
  title={t('TabularReviews.cellDetail')}
  size="md"
  onclose={() => {
    detailRow = null
    detailKey = null
  }}
>
  {#if detailRow && detailColumn}
    <div class="space-y-3">
      <div class="text-xs text-(--color-text-secondary) space-y-0.5">
        <p>{t('Tabular.document')}: <span class="text-(--color-text-primary)">{detailRow.filename}</span></p>
        <p>
          {t('WorkflowColumns.format')}:
          <span class="text-(--color-text-primary)">
            {colLabel(detailColumn, columns.indexOf(detailColumn))}
          </span>
        </p>
      </div>
      <div class="rounded-(--radius-md) border border-(--color-surface-200) bg-(--color-surface-50) p-3
                  text-sm whitespace-pre-wrap min-h-24
                  {detailCell?.status === 'error'
                    ? 'text-(--color-danger-500)'
                    : 'text-(--color-text-primary)'}">
        {detailCell?.content || '—'}
      </div>
    </div>
  {/if}
  {#snippet footer()}
    <Button variant="ghost" onclick={() => { detailRow = null; detailKey = null }}>
      {t('Common.close')}
    </Button>
    <Button loading={regenerating} onclick={regenerate}>
      <RefreshCw size={14} class="mr-1" />{t('Assistant.regenerate')}
    </Button>
  {/snippet}
</Modal>
