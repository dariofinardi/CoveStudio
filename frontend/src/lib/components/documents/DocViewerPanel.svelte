<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Document-viewer side panel: a resizable column on the right with one
  browser-style tab per opened document. Picks a renderer per file type
  (PDF / DOCX / spreadsheet / text) and shows a per-tab header card for
  citations. All rendering is in-browser JS — no native plugin.
-->
<script lang="ts">
  import { docViewer, type ViewerTab } from '$lib/stores/doc-viewer.svelte'
  import { documentsApi } from '$lib/api/documents'
  import PdfView from './PdfView.svelte'
  import DocxView from './DocxView.svelte'
  import SheetView from './SheetView.svelte'
  import TextView from './TextView.svelte'
  import RejectReasonModal from './RejectReasonModal.svelte'
  import ViewSummaryModal from './ViewSummaryModal.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { openExternalPath } from '$lib/tauri/commands'
  import {
    X,
    Download,
    Quote,
    PanelRightClose,
    PanelRightOpen,
    Check,
    ExternalLink,
    FileText,
  } from 'lucide-svelte'

  let rejectModalOpen = $state(false)
  let viewSummaryOpen = $state(false)
  /** True when the most recent `loadActive` failed with a 404 — the
   *  source file backing this KB citation has been removed from disk
   *  but its embeddings outlived it. The renderer surfaces a
   *  "Pulisci sorgenti rimosse" action that POSTs to
   *  /sync/cleanup-orphans and toasts the row count. */
  let orphanSuspected = $state(false)
  let cleaningOrphans = $state(false)

  async function runCleanup() {
    if (cleaningOrphans) return
    cleaningOrphans = true
    try {
      const { api } = await import('$lib/api/client')
      const res = await api<{
        scanned: number
        orphans: number
        deleted_docs: number
        deleted_chunks: number
      }>('/sync/cleanup-orphans', { method: 'POST' })
      if (res.orphans === 0) {
        toastStore.success(i18n.t('DocViewer.cleanup.noOrphans'))
      } else {
        toastStore.success(
          i18n.t('DocViewer.cleanup.done', {
            orphans: res.orphans,
            chunks: res.deleted_chunks,
          }),
        )
        orphanSuspected = false
      }
    } catch (err) {
      toastStore.danger(
        i18n.t('DocViewer.cleanup.failed', {
          err: (err as Error).message ?? String(err),
        }),
      )
    } finally {
      cleaningOrphans = false
    }
  }

  async function hydrateDecision(tab: ViewerTab) {
    if (tab.source !== 'document') return
    try {
      const meta = await documentsApi.get(tab.docId)
      docViewer.setDecision(
        tab.docId,
        meta.decision ?? 'accepted',
        meta.decision_reason ?? null,
        meta.decision_summary ?? null,
      )
    } catch {
      // Non-fatal: keep the default 'accepted' the store seeded with.
    }
  }

  async function acceptActive(tab: ViewerTab) {
    if (tab.source !== 'document') return
    try {
      const res = await documentsApi.setDecision(tab.docId, { decision: 'accepted' })
      docViewer.setDecision(tab.docId, 'accepted', res.reason, res.summary)
    } catch (err) {
      toastStore.danger(
        i18n.t('DocViewer.accept.error', {
          err: (err as Error).message ?? String(err),
        }),
      )
    }
  }

  function openRejectModal() {
    rejectModalOpen = true
  }

  async function openExternally(tab: ViewerTab) {
    if (tab.source !== 'document') return
    try {
      const meta = await documentsApi.filePath(tab.docId)
      await openExternalPath(meta.path)
    } catch (err) {
      toastStore.danger(
        i18n.t('DocViewer.openExternal.error', {
          err: (err as Error).message ?? String(err),
        }),
      )
    }
  }

  type RendererKind = 'pdf' | 'docx' | 'sheet' | 'md' | 'rtf' | 'text' | 'unsupported'

  interface Loaded {
    kind: RendererKind
    blob: Blob
    bytes: Uint8Array
    text: string
  }

  let loading = $state(false)
  let loadError = $state<string | null>(null)
  let loaded = $state<Loaded | null>(null)
  let loadedDocId: string | null = null

  const cache = new Map<string, Loaded>()

  const activeTab = $derived(docViewer.activeTab)

  function extOf(name: string): string {
    const m = /\.([a-z0-9]+)$/i.exec(name.trim())
    return m ? m[1].toLowerCase() : ''
  }

  function rendererFor(blob: Blob, filename: string): RendererKind {
    const t = blob.type.toLowerCase()
    const ext = extOf(filename)
    if (t.includes('pdf') || ext === 'pdf') return 'pdf'
    if (t.includes('wordprocessing') || t.includes('msword') || ext === 'docx' || ext === 'doc')
      return 'docx'
    if (
      t.includes('spreadsheet') ||
      t.includes('excel') ||
      ['xlsx', 'xls', 'xlsb', 'ods', 'csv'].includes(ext)
    )
      return 'sheet'
    if (ext === 'md' || ext === 'markdown') return 'md'
    if (ext === 'rtf' || t.includes('rtf')) return 'rtf'
    if (t.startsWith('text/') || ['txt', 'log'].includes(ext)) return 'text'
    // A PDF rendition with a stale extension still sniffs as PDF above;
    // anything left is genuinely unknown.
    return 'unsupported'
  }

  function isDocxTab(tab: ViewerTab): boolean {
    return ['docx', 'doc'].includes(extOf(tab.title))
  }

  async function loadActive(tab: ViewerTab) {
    if (loadedDocId === tab.docId && loaded) return
    const cached = cache.get(tab.docId)
    if (cached) {
      loaded = cached
      loadedDocId = tab.docId
      return
    }
    loading = true
    loadError = null
    loaded = null
    orphanSuspected = false
    try {
      const blob =
        tab.source === 'kb'
          ? tab.kbPath
            ? await documentsApi.kbBytes(tab.kbPath)
            : (() => {
                throw new Error('KB citation missing path')
              })()
          : await documentsApi.displayBytes(tab.docId)
      // Hydrate the per-chat accept/reject state alongside the bytes.
      // Fire-and-forget so the renderer doesn't wait for it.
      void hydrateDecision(tab)
      const buf = new Uint8Array(await blob.arrayBuffer())
      const kind = rendererFor(blob, tab.title)
      const text =
        kind === 'md' || kind === 'rtf' || kind === 'text'
          ? new TextDecoder('utf-8').decode(buf)
          : ''
      const result: Loaded = { kind, blob, bytes: buf, text }
      cache.set(tab.docId, result)
      loaded = result
      loadedDocId = tab.docId
    } catch (e) {
      const msg = (e as Error).message ?? String(e)
      loadError = msg
      // 404 on /sync/kb-doc or /document/:id/display means the row's
      // file is missing on disk. Hint a cleanup action; the user
      // accepts and we purge orphan embeddings via the new endpoint.
      if (/\b404\b/.test(msg) || msg.toLowerCase().includes('not found')) {
        orphanSuspected = true
      }
    } finally {
      loading = false
    }
  }

  $effect(() => {
    const tab = activeTab
    if (tab) void loadActive(tab)
  })

  async function download(tab: ViewerTab) {
    try {
      const blob =
        tab.source === 'kb'
          ? tab.kbPath
            ? await documentsApi.kbBytes(tab.kbPath)
            : (() => {
                throw new Error('KB citation missing path')
              })()
          : await documentsApi.downloadBytes(tab.docId)
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = tab.title || 'document'
      a.click()
      URL.revokeObjectURL(url)
    } catch {
      // surfaced elsewhere; download failure is non-fatal
    }
  }

  // ── Resize handle ────────────────────────────────────────────────
  // The grip uses pointer capture: once the drag starts, every move/up
  // event for that pointer is routed to the grip element itself — even
  // when the cursor travels over the chat pane or a document renderer.
  // Window-level listeners alone would lose the stream the moment the
  // cursor crossed another element, freezing the drag mid-resize.
  let resizing = $state(false)
  function startResize(e: PointerEvent) {
    e.preventDefault()
    const grip = e.currentTarget as HTMLElement
    grip.setPointerCapture(e.pointerId)
    resizing = true
    // Suppress text selection / cursor flicker for the whole drag.
    document.body.style.userSelect = 'none'
    document.body.style.cursor = 'col-resize'

    const onMove = (ev: PointerEvent) => {
      docViewer.setWidth(window.innerWidth - ev.clientX)
    }
    const onUp = (ev: PointerEvent) => {
      resizing = false
      document.body.style.userSelect = ''
      document.body.style.cursor = ''
      try {
        grip.releasePointerCapture(ev.pointerId)
      } catch {
        // pointer already released — nothing to do
      }
      grip.removeEventListener('pointermove', onMove)
      grip.removeEventListener('pointerup', onUp)
      grip.removeEventListener('pointercancel', onUp)
    }
    grip.addEventListener('pointermove', onMove)
    grip.addEventListener('pointerup', onUp)
    grip.addEventListener('pointercancel', onUp)
  }
</script>

{#if docViewer.open}
  <aside
    class="relative shrink-0 h-full flex flex-col border-l border-(--color-surface-200) bg-(--color-surface-0)"
    style:width={docViewer.collapsed ? '38px' : `${docViewer.width}px`}
  >
    {#if docViewer.collapsed}
      <!-- collapsed: a thin strip; click to restore the previous width -->
      <button
        type="button"
        onclick={() => docViewer.toggleCollapse()}
        aria-label={i18n.t('DocViewer.expand')}
        title={i18n.t('DocViewer.expand')}
        class="h-full w-full flex items-start justify-center pt-2.5
               text-(--color-text-secondary) hover:text-(--color-text-primary)
               hover:bg-(--color-hover-bg)"
      >
        <PanelRightOpen size={16} />
      </button>
    {:else}
    <!-- resize grip -->
    <div
      role="separator"
      aria-label="Resize document viewer"
      aria-orientation="vertical"
      tabindex="-1"
      onpointerdown={startResize}
      class="absolute left-0 top-0 h-full w-1.5 -ml-1 cursor-col-resize z-10 touch-none
             hover:bg-(--color-brand-400) {resizing ? 'bg-(--color-brand-400)' : ''}"
    ></div>

    <!-- tab bar -->
    <div class="flex items-stretch shrink-0 border-b border-(--color-surface-200) bg-(--color-surface-50)">
      <button
        type="button"
        onclick={() => docViewer.toggleCollapse()}
        aria-label={i18n.t('DocViewer.collapse')}
        title={i18n.t('DocViewer.collapse')}
        class="shrink-0 w-9 flex items-center justify-center border-r border-(--color-surface-200)
               text-(--color-text-secondary) hover:text-(--color-text-primary) hover:bg-(--color-hover-bg)"
      >
        <PanelRightClose size={15} />
      </button>
      <div class="flex-1 min-w-0 flex items-stretch overflow-x-auto">
        {#each docViewer.tabs as tab (tab.id)}
          <div
            class="group flex items-center gap-1.5 pl-3 pr-1.5 h-9 max-w-44 border-r border-(--color-surface-200)
                   {tab.id === docViewer.activeId
                     ? 'bg-(--color-surface-0)'
                     : 'bg-(--color-surface-50) hover:bg-(--color-hover-bg)'}"
          >
            <button
              type="button"
              class="flex-1 min-w-0 truncate text-left text-xs
                     {tab.id === docViewer.activeId
                       ? 'text-(--color-text-primary) font-medium'
                       : 'text-(--color-text-secondary)'}"
              onclick={() => docViewer.select(tab.id)}
            >
              {tab.title}
            </button>
            <button
              type="button"
              class="p-0.5 rounded opacity-0 group-hover:opacity-100 hover:bg-(--color-hover-bg)"
              aria-label={i18n.t('DocViewer.closeTab')}
              onclick={() => docViewer.closeTab(tab.id)}
            >
              <X size={12} />
            </button>
          </div>
        {/each}
      </div>
      <button
        type="button"
        class="px-2 shrink-0 text-xs text-(--color-text-secondary) hover:text-(--color-text-primary)"
        onclick={() => docViewer.closeAll()}
      >
        {i18n.t('DocViewer.closeAll')}
      </button>
    </div>

    {#if activeTab}
      <!-- per-tab header card -->
      <div class="shrink-0 px-3 py-2 border-b border-(--color-surface-200)">
        {#if activeTab.mode === 'citation' && activeTab.quote}
          <div class="rounded-(--radius-md) bg-(--color-surface-50) border border-(--color-surface-200) p-2.5">
            <div class="flex items-center justify-between gap-2 mb-1">
              <span class="flex items-center gap-1 text-xs font-semibold text-(--color-brand-600)">
                <Quote size={12} />{i18n.t('DocViewer.citation')}
                {#if activeTab.page != null}
                  · {i18n.t('DocViewer.page')} {activeTab.page}
                {/if}
              </span>
              <button
                type="button"
                class="flex items-center gap-1 text-xs text-(--color-text-secondary) hover:text-(--color-text-primary)"
                onclick={() => download(activeTab)}
              >
                <Download size={12} />{i18n.t('Documents.viewer.download')}
              </button>
            </div>
            {#if isDocxTab(activeTab)}
              {@render decisionToolbar(activeTab)}
            {/if}
            <p class="text-xs text-(--color-text-secondary) line-clamp-3 whitespace-pre-wrap">
              {activeTab.quote.replaceAll('[[PAGE_BREAK]]', ' … ')}
            </p>
          </div>
        {:else}
          <div class="flex flex-col gap-1.5">
            <div class="flex items-center justify-between gap-2">
              <span class="text-xs text-(--color-text-secondary) truncate">{activeTab.title}</span>
              <button
                type="button"
                class="flex items-center gap-1 text-xs text-(--color-text-secondary) hover:text-(--color-text-primary)"
                onclick={() => download(activeTab)}
              >
                <Download size={12} />{i18n.t('Documents.viewer.download')}
              </button>
            </div>
            {#if isDocxTab(activeTab)}
              {@render decisionToolbar(activeTab)}
            {/if}
          </div>
        {/if}
      </div>

      <!-- renderer -->
      <div class="flex-1 min-h-0">
        {#if loading}
          <div class="flex items-center justify-center gap-2 h-full text-sm text-(--color-text-secondary)">
            <Spinner size="sm" />
            {i18n.t('Documents.viewer.loadingDocument')}
          </div>
        {:else if loadError}
          <div class="p-8 text-center max-w-md mx-auto">
            <p class="text-sm text-(--color-danger-500) mb-3">
              {i18n.t('Documents.viewer.errorLoading')} — {loadError}
            </p>
            {#if orphanSuspected}
              <div
                class="mt-4 p-4 rounded-(--radius-md) border border-(--color-warning-300)
                       bg-(--color-warning-50) text-left"
              >
                <p class="text-sm font-medium text-(--color-text-primary) mb-1.5">
                  {i18n.t('DocViewer.cleanup.title')}
                </p>
                <p class="text-xs text-(--color-text-secondary) mb-3">
                  {i18n.t('DocViewer.cleanup.body')}
                </p>
                <button
                  type="button"
                  onclick={runCleanup}
                  disabled={cleaningOrphans}
                  class="h-7 px-3 text-[11px] font-medium inline-flex items-center gap-1.5
                         rounded-(--radius-md) bg-(--color-warning-500) text-white
                         hover:bg-(--color-warning-600) disabled:opacity-60
                         disabled:cursor-not-allowed"
                >
                  {#if cleaningOrphans}
                    <Spinner size="sm" />{i18n.t('DocViewer.cleanup.running')}
                  {:else}
                    {i18n.t('DocViewer.cleanup.action')}
                  {/if}
                </button>
              </div>
            {/if}
          </div>
        {:else if loaded}
          {#if loaded.kind === 'pdf'}
            <PdfView
              bytes={loaded.bytes}
              quote={activeTab.quote}
              page={activeTab.page}
              revision={docViewer.revision}
            />
          {:else if loaded.kind === 'docx'}
            <DocxView
              blob={loaded.blob}
              quote={activeTab.quote}
              trackedPolicy={activeTab.trackedPolicy}
              revision={docViewer.revision}
            />
          {:else if loaded.kind === 'sheet'}
            <SheetView bytes={loaded.bytes} quote={activeTab.quote} revision={docViewer.revision} />
          {:else if loaded.kind === 'md'}
            <TextView text={loaded.text} kind="md" quote={activeTab.quote} revision={docViewer.revision} />
          {:else if loaded.kind === 'rtf'}
            <TextView text={loaded.text} kind="rtf" quote={activeTab.quote} revision={docViewer.revision} />
          {:else if loaded.kind === 'text'}
            <TextView text={loaded.text} kind="plain" quote={activeTab.quote} revision={docViewer.revision} />
          {:else}
            <p class="text-sm text-(--color-text-secondary) p-8 text-center">
              {i18n.t('DocViewer.unsupported')}
            </p>
          {/if}
        {/if}
      </div>
    {/if}
    {/if}
  </aside>
{/if}

{#if activeTab && activeTab.source === 'document'}
  <RejectReasonModal
    bind:open={rejectModalOpen}
    docId={activeTab.docId}
    filename={activeTab.title}
    initialReason={activeTab.decisionReason}
  />
  <ViewSummaryModal
    bind:open={viewSummaryOpen}
    filename={activeTab.title}
    reason={activeTab.decisionReason}
    summary={activeTab.decisionSummary}
  />
{/if}

{#snippet decisionToolbar(tab: ViewerTab)}
  <!--
    Per-chat decision controls for a model-generated docx.
    - Accept (green when active): keeps the document in the chat
      context as if the user had uploaded it manually.
    - Reject (red when active): opens RejectReasonModal; on confirm
      the docx is swapped server-side with a summary + the user's
      reason on subsequent chat turns. The decision is reversible
      at any time by clicking the opposite button.
    - Apri in Word (icon + label, neutral): hands off to the OS
      default app via Tauri's open_external_path — for real
      inline track-changes editing the WebView can't do.
    Tracked-change preview policies (show/accept/reject as a
    pure render hint) were removed when the buttons stopped being
    decorative; see HISTORY v0.3.5.
  -->
  <div class="flex flex-wrap items-center gap-2">
    <div
      role="radiogroup"
      aria-label={i18n.t('DocViewer.decision.groupLabel')}
      class="inline-flex rounded-(--radius-md) overflow-hidden
             border border-(--color-surface-200)"
    >
      <button
        type="button"
        role="radio"
        aria-checked={tab.decision === 'accepted'}
        onclick={() => acceptActive(tab)}
        title={i18n.t('DocViewer.decision.acceptTooltip')}
        class="h-7 px-3 text-[11px] font-medium inline-flex items-center gap-1
               border-r border-(--color-surface-200)
               {tab.decision === 'accepted'
                 ? 'bg-(--color-success-500) text-white'
                 : 'text-(--color-text-secondary) hover:bg-(--color-surface-50)'}"
      >
        <Check size={12} />{i18n.t('DocViewer.decision.accept')}
      </button>
      <button
        type="button"
        role="radio"
        aria-checked={tab.decision === 'rejected'}
        onclick={openRejectModal}
        title={i18n.t('DocViewer.decision.rejectTooltip')}
        class="h-7 px-3 text-[11px] font-medium inline-flex items-center gap-1
               {tab.decision === 'rejected'
                 ? 'bg-(--color-danger-500) text-white'
                 : 'text-(--color-text-secondary) hover:bg-(--color-surface-50)'}"
      >
        <X size={12} />{i18n.t('DocViewer.decision.reject')}
      </button>
    </div>
    <button
      type="button"
      onclick={() => openExternally(tab)}
      title={i18n.t('DocViewer.openExternal.tooltip')}
      class="h-7 px-3 text-[11px] font-medium inline-flex items-center gap-1
             rounded-(--radius-md) border border-(--color-surface-200)
             text-(--color-text-secondary) hover:bg-(--color-surface-50)
             hover:text-(--color-text-primary)"
    >
      <ExternalLink size={12} />{i18n.t('DocViewer.openExternal.label')}
    </button>
    {#if tab.decision === 'rejected'}
      <button
        type="button"
        onclick={() => (viewSummaryOpen = true)}
        title={i18n.t('DocViewer.decision.viewSummaryTooltip')}
        class="h-7 px-3 text-[11px] font-medium inline-flex items-center gap-1
               rounded-(--radius-md) border border-(--color-surface-200)
               text-(--color-text-secondary) hover:bg-(--color-surface-50)
               hover:text-(--color-text-primary)"
      >
        <FileText size={12} />{i18n.t('DocViewer.decision.viewSummary')}
      </button>
      {#if tab.decisionReason}
        <span
          class="text-[11px] text-(--color-danger-500) truncate max-w-xs"
          title={tab.decisionReason}
        >
          {i18n.t('DocViewer.decision.rejectedBadge')}: {tab.decisionReason}
        </span>
      {/if}
    {/if}
  </div>
{/snippet}
