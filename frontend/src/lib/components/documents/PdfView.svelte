<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  PDF renderer. Renders every page to a canvas with a transparent,
  selectable text layer (so cited passages highlight and text copies).
  Opens fit-to-width; zoom / fit buttons + ctrl-wheel; Ctrl+G focuses
  the go-to-page field.
-->
<script lang="ts">
  import { getDocument, TextLayer } from '$lib/utils/pdf'
  import { highlightCitation } from '$lib/utils/highlight'
  import { PAGE_BREAK_SENTINEL } from '$lib/types/citation'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { ZoomIn, ZoomOut, MoveHorizontal, MoveVertical, Maximize2 } from 'lucide-svelte'

  interface Props {
    bytes: Uint8Array
    quote?: string
    page?: number | string
    /** Bumped by the store when an open tab is re-targeted. */
    revision?: number
  }

  let { bytes, quote, page, revision = 0 }: Props = $props()

  type FitMode = 'width' | 'height' | 'page' | null

  let scale = $state(1)
  let fitMode = $state<FitMode>('width')
  let numPages = $state(0)
  let currentPage = $state(1)
  let gotoValue = $state('')
  let loading = $state(true)
  /** True while `renderAll` is still rasterising pages into `pagesEl`.
   *  On long PDFs the page-by-page rasterisation can outlast `loading`
   *  (which only covers the initial document fetch) by seconds. The
   *  highlight effect gates on this so it doesn't scan an empty / partial
   *  DOM and silently no-op when the user clicks a citation early. */
  let renderInProgress = $state(false)
  let err = $state<string | null>(null)
  let scrollEl: HTMLDivElement
  let pagesEl: HTMLDivElement
  let gotoEl = $state<HTMLInputElement>()
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  let pdfDoc: any = null
  let renderToken = 0
  let ready = false
  /** Page natural size (CSS px at scale 1). */
  let naturalW = 0
  let naturalH = 0

  function pageHint(): number | null {
    if (page == null) return null
    const n = typeof page === 'number' ? page : parseInt(String(page), 10)
    return Number.isFinite(n) ? n : null
  }

  /** Compute the scale for the current fit mode against the viewport. */
  function applyFit() {
    if (!fitMode || !scrollEl || naturalW === 0) return
    const availW = scrollEl.clientWidth - 32
    const availH = scrollEl.clientHeight - 32
    const sw = availW / naturalW
    const sh = availH / naturalH
    const s = fitMode === 'width' ? sw : fitMode === 'height' ? sh : Math.min(sw, sh)
    scale = Math.max(0.2, Math.min(5, Math.round(s * 100) / 100))
  }

  async function renderAll() {
    const token = ++renderToken
    if (!pagesEl || !pdfDoc) return
    renderInProgress = true
    try {
      pagesEl.innerHTML = ''
      for (let p = 1; p <= numPages; p++) {
        if (token !== renderToken) return
        const pageObj = await pdfDoc.getPage(p)
        const viewport = pageObj.getViewport({ scale })

        const wrap = document.createElement('div')
        wrap.className = 'pdf-page relative shadow-(--shadow-card) bg-white'
        wrap.style.width = `${viewport.width}px`
        wrap.style.height = `${viewport.height}px`
        wrap.style.margin = '0 auto 12px'
        wrap.dataset.page = String(p)

        const canvas = document.createElement('canvas')
        canvas.width = viewport.width
        canvas.height = viewport.height
        canvas.style.width = `${viewport.width}px`
        canvas.style.height = `${viewport.height}px`
        wrap.appendChild(canvas)

        const textDiv = document.createElement('div')
        textDiv.className = 'pdf-text-layer'
        textDiv.style.setProperty('--scale-factor', String(scale))
        textDiv.style.width = `${viewport.width}px`
        textDiv.style.height = `${viewport.height}px`
        wrap.appendChild(textDiv)

        pagesEl.appendChild(wrap)

        const ctx = canvas.getContext('2d')
        if (ctx) await pageObj.render({ canvasContext: ctx, viewport }).promise

        const layer = new TextLayer({
          textContentSource: pageObj.streamTextContent(),
          container: textDiv,
          viewport,
        })
        await layer.render()
      }
    } finally {
      // Only the LATEST render's finally flips the flag back — an
      // aborted older pass (token mismatch) must leave the signal in
      // the new pass's hands. The reactive effect at the bottom will
      // re-fire on the transition `true → false` and run the
      // highlight against the now-fully-rendered DOM.
      if (token === renderToken) renderInProgress = false
    }
  }

  function applyHighlight() {
    if (!pagesEl) return
    const hint = pageHint()
    if (quote) {
      const order: HTMLElement[] = []
      const all = Array.from(pagesEl.querySelectorAll<HTMLElement>('.pdf-text-layer'))
      if (hint && all[hint - 1]) order.push(all[hint - 1])
      for (const l of all) if (!order.includes(l)) order.push(l)

      for (const layer of order) {
        const mark = highlightCitation(layer, quote, PAGE_BREAK_SENTINEL)
        if (mark) {
          mark.classList.add('doc-hl-flash')
          mark.scrollIntoView({ block: 'center', behavior: 'smooth' })
          return
        }
      }
    }
    // Fallback: scroll to the hinted page even when the quote-search
    // missed (or no quote was supplied). Without this, citations that
    // carry only `page` (KB chunks where the verbatim text was lost in
    // re-flow) silently did nothing.
    if (hint) {
      const pageEl = pagesEl.querySelector<HTMLElement>(`.pdf-page[data-page="${hint}"]`)
      pageEl?.scrollIntoView({ block: 'start', behavior: 'smooth' })
    }
  }

  async function load() {
    loading = true
    err = null
    ready = false
    try {
      // getDocument takes ownership of the buffer — hand it a copy.
      pdfDoc = await getDocument({ data: bytes.slice() }).promise
      numPages = pdfDoc.numPages
      const first = await pdfDoc.getPage(1)
      const vp1 = first.getViewport({ scale: 1 })
      naturalW = vp1.width
      naturalH = vp1.height
      applyFit()
      await renderAll()
    } catch (e) {
      err = (e as Error).message
    } finally {
      loading = false
      ready = true
    }
  }

  function setFit(mode: Exclude<FitMode, null>) {
    fitMode = mode
    applyFit()
  }

  function zoom(delta: number) {
    fitMode = null
    scale = Math.min(5, Math.max(0.2, Math.round((scale + delta) * 100) / 100))
  }

  function onWheel(e: WheelEvent) {
    if (!(e.ctrlKey || e.metaKey)) return
    e.preventDefault()
    zoom(e.deltaY < 0 ? 0.15 : -0.15)
  }

  function onScroll() {
    if (!pagesEl || !scrollEl) return
    const mid = scrollEl.scrollTop + scrollEl.clientHeight / 2
    const pages = Array.from(pagesEl.querySelectorAll<HTMLElement>('.pdf-page'))
    for (const pg of pages) {
      if (pg.offsetTop <= mid && pg.offsetTop + pg.offsetHeight >= mid) {
        currentPage = Number(pg.dataset.page)
        break
      }
    }
  }

  function gotoPage() {
    const n = parseInt(gotoValue, 10)
    if (!Number.isFinite(n) || n < 1 || n > numPages) return
    pagesEl
      ?.querySelector<HTMLElement>(`.pdf-page[data-page="${n}"]`)
      ?.scrollIntoView({ block: 'start', behavior: 'smooth' })
  }

  // Initial load + reload when the document bytes change.
  $effect(() => {
    void bytes
    void load()
  })

  // Re-render whenever the scale changes (after the first load).
  $effect(() => {
    void scale
    if (ready) void renderAll()
  })

  // Re-fit on viewport resize while a fit mode is active.
  $effect(() => {
    function onResize() {
      if (fitMode) applyFit()
    }
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  })

  // Ctrl/⌘+G focuses the go-to-page field.
  $effect(() => {
    function onKey(e: KeyboardEvent) {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'g') {
        e.preventDefault()
        gotoEl?.focus()
        gotoEl?.select()
      }
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  })

  // Re-run the highlight pass whenever the tab is re-targeted to a new
  // quote/page AND the renderer has finished painting the pages it has
  // to scan. Gating on `renderInProgress` (not just `loading`) is what
  // closes the long-PDF race: `loading` only covers the initial fetch,
  // while `renderInProgress` outlives it by however many seconds the
  // page-by-page rasterisation takes. The effect re-fires on the
  // `true → false` transition, so an early citation click is honoured
  // the moment the document is actually paintable.
  $effect(() => {
    void revision
    void quote
    void renderInProgress
    if (!loading && !renderInProgress) applyHighlight()
  })

  const fitBtn =
    'p-1 rounded hover:bg-(--color-hover-bg) text-(--color-text-secondary)'
  const fitBtnOn = 'p-1 rounded bg-(--color-active-bg) text-(--color-brand-700)'
</script>

<div class="flex flex-col h-full min-h-0">
  <div
    bind:this={scrollEl}
    onscroll={onScroll}
    onwheel={onWheel}
    class="flex-1 min-h-0 overflow-auto bg-(--color-surface-100) p-4"
  >
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
    <div bind:this={pagesEl}></div>
  </div>

  {#if !loading && !err}
    <div class="flex items-center justify-between gap-2 px-3 h-9 shrink-0 border-t border-(--color-surface-200) text-xs text-(--color-text-secondary)">
      <div class="flex items-center gap-1">
        <span>{i18n.t('Documents.viewer.page')}</span>
        <input
          bind:this={gotoEl}
          bind:value={gotoValue}
          onkeydown={(e) => e.key === 'Enter' && gotoPage()}
          inputmode="numeric"
          placeholder={String(currentPage)}
          class="w-10 h-6 text-center rounded border border-(--color-surface-200)
                 bg-(--color-surface-0) focus:outline-none focus:ring-1 focus:ring-(--color-brand-500)"
        />
        <span>{i18n.t('Documents.viewer.of')} {numPages}</span>
      </div>
      <div class="flex items-center gap-0.5">
        <button type="button" class={fitMode === 'width' ? fitBtnOn : fitBtn}
          aria-label={i18n.t('Documents.viewer.fitWidth')} title={i18n.t('Documents.viewer.fitWidth')}
          onclick={() => setFit('width')}>
          <MoveHorizontal size={14} />
        </button>
        <button type="button" class={fitMode === 'height' ? fitBtnOn : fitBtn}
          aria-label={i18n.t('Documents.viewer.fitHeight')} title={i18n.t('Documents.viewer.fitHeight')}
          onclick={() => setFit('height')}>
          <MoveVertical size={14} />
        </button>
        <button type="button" class={fitMode === 'page' ? fitBtnOn : fitBtn}
          aria-label={i18n.t('Documents.viewer.fitPage')} title={i18n.t('Documents.viewer.fitPage')}
          onclick={() => setFit('page')}>
          <Maximize2 size={14} />
        </button>
        <span class="w-px h-4 bg-(--color-surface-200) mx-1"></span>
        <button type="button" class={fitBtn}
          aria-label={i18n.t('Documents.viewer.zoomOut')} onclick={() => zoom(-0.15)}>
          <ZoomOut size={14} />
        </button>
        <span class="tabular-nums w-10 text-center">{Math.round(scale * 100)}%</span>
        <button type="button" class={fitBtn}
          aria-label={i18n.t('Documents.viewer.zoomIn')} onclick={() => zoom(0.15)}>
          <ZoomIn size={14} />
        </button>
      </div>
    </div>
  {/if}
</div>
