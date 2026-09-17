<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Spreadsheet renderer. Parses XLSX/XLS/ODS/CSV via SheetJS (pure JS)
  and renders each sheet as a selectable HTML table; the cited passage
  is highlighted in the active sheet.
-->
<script lang="ts">
  import { read, utils } from 'xlsx'
  import DOMPurify from 'dompurify'
  import { highlightCitation } from '$lib/utils/highlight'
  import { PAGE_BREAK_SENTINEL } from '$lib/types/citation'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'

  interface Props {
    bytes: Uint8Array
    quote?: string
    revision?: number
  }

  let { bytes, quote, revision = 0 }: Props = $props()

  let loading = $state(true)
  let err = $state<string | null>(null)
  let sheetNames = $state<string[]>([])
  let activeSheet = $state(0)
  let host: HTMLDivElement
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  let workbook: any = null

  function renderSheet() {
    if (!workbook || !host) return
    const name = sheetNames[activeSheet]
    const ws = workbook.Sheets[name]
    const raw = utils.sheet_to_html(ws, { id: 'sheet' })
    host.innerHTML = DOMPurify.sanitize(raw)
    if (quote) {
      const mark = highlightCitation(host, quote, PAGE_BREAK_SENTINEL)
      if (mark) {
        mark.classList.add('doc-hl-flash')
        mark.scrollIntoView({ block: 'center', behavior: 'smooth' })
      }
    }
  }

  // load() ONLY parses + stamps the local state. It must NOT call
  // renderSheet directly: renderSheet reads `sheetNames`, `activeSheet`,
  // and `quote`, which would become tracked deps of the outer $effect
  // that calls load(), and load() then writes back to `sheetNames` /
  // `activeSheet` in the same run → Svelte 5 detects the
  // read-then-write loop and bails with effect_update_depth_exceeded.
  // Instead we let the dedicated render $effect below pick up the new
  // workbook on the `loading: true → false` transition.
  function load() {
    loading = true
    err = null
    try {
      workbook = read(bytes, { type: 'array' })
      sheetNames = workbook.SheetNames ?? []
      activeSheet = 0
    } catch (e) {
      err = (e as Error).message
    } finally {
      loading = false
    }
  }

  $effect(() => {
    void bytes
    load()
  })

  $effect(() => {
    // Render-only effect: fires whenever the user switches tab, a tab
    // is re-targeted (revision++), the highlight quote changes, OR
    // load() finishes (loading → false). Reads the state, never
    // writes it — no risk of feedback with the load() effect above.
    void activeSheet
    void revision
    void quote
    if (!loading && !err) renderSheet()
  })
</script>

<div class="flex flex-col h-full min-h-0">
  {#if sheetNames.length > 1}
    <div class="flex gap-1 px-2 py-1.5 shrink-0 border-b border-(--color-surface-200) overflow-x-auto">
      {#each sheetNames as name, i (name)}
        <button
          type="button"
          onclick={() => (activeSheet = i)}
          class="px-2.5 h-7 rounded-(--radius-md) text-xs whitespace-nowrap
                 {activeSheet === i
                   ? 'bg-(--color-active-bg) text-(--color-brand-700) font-medium'
                   : 'text-(--color-text-secondary) hover:bg-(--color-hover-bg)'}"
        >
          {name}
        </button>
      {/each}
    </div>
  {/if}

  <div class="flex-1 min-h-0 overflow-auto bg-(--color-surface-50) p-3">
    {#if loading}
      <div class="flex items-center justify-center gap-2 py-12 text-sm text-(--color-text-secondary)">
        <Spinner size="sm" />
        {i18n.t('Documents.viewer.loadingDocument')}
      </div>
    {:else if err}
      <p class="text-sm text-(--color-danger-500) py-12 text-center">
        {i18n.t('Documents.viewer.errorLoading')} — {err}
      </p>
    {/if}
    <div bind:this={host} class="sheet-body"></div>
  </div>
</div>
