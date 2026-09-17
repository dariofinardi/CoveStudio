<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  DOCX renderer. Renders Word documents to HTML in-browser via
  docx-preview (pure JS, no plugin). Tracked changes are shown with
  coloured insert/delete styling; the cited passage is highlighted.

  Two view modes:
    - `fit`    (default): preserves the document's native page size
                          (A4 width + height + margins) and applies
                          a CSS `zoom` factor so the page scales to
                          the available panel width. A ResizeObserver
                          re-fits whenever the user drags the panel
                          divider. Visual fidelity wins.
    - `reflow`          : drops the fixed page width/height
                          (`ignoreWidth/Height: true`) so prose
                          reflows naturally to the container.
                          Useful for narrow side panels where the
                          scaled page becomes uncomfortably small.

  The toggle floats in the top-right corner of the renderer.
-->
<script lang="ts">
  import { renderAsync } from 'docx-preview'
  import { highlightCitation } from '$lib/utils/highlight'
  import { PAGE_BREAK_SENTINEL } from '$lib/types/citation'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { Maximize2, AlignLeft } from 'lucide-svelte'

  interface Props {
    blob: Blob
    quote?: string
    trackedPolicy?: 'show' | 'accept' | 'reject'
    revision?: number
  }

  let { blob, quote, trackedPolicy = 'show', revision = 0 }: Props = $props()

  type ViewMode = 'fit' | 'reflow'
  let mode = $state<ViewMode>('fit')

  let loading = $state(true)
  let err = $state<string | null>(null)
  let container: HTMLDivElement
  let host: HTMLDivElement
  let resizeObserver: ResizeObserver | null = null

  /** Compute the zoom factor so the docx page exactly fills the
   *  available container width. Clamped to [0.4, 1.5] so extreme
   *  panel widths don't render the page absurdly tiny or absurdly
   *  large. */
  function updateZoom() {
    if (!host || !container || mode !== 'fit') return
    // docx-preview emits a `.docx-wrapper > section.docx` (or
    // `.docx-wrapper > .docx`) whose width carries the native page
    // dimensions in pixels. Read it before applying zoom so we
    // measure the intrinsic size, not the scaled one.
    const page = host.querySelector(
      '.docx-wrapper > section.docx, .docx-wrapper > .docx',
    ) as HTMLElement | null
    if (!page) return
    // Reset zoom briefly so offsetWidth reports the native size on
    // re-fits. `zoom` is read live by offset metrics in WebView2.
    host.style.zoom = ''
    const native = page.offsetWidth || 793 // A4 ~ 793 px @ 96dpi as fallback
    // Account for the container's own padding (16px each side).
    const avail = Math.max(120, container.clientWidth - 32)
    const z = Math.min(1.5, Math.max(0.4, avail / native))
    host.style.zoom = String(z)
  }

  function applyHighlight() {
    if (!quote || !host) return
    const mark = highlightCitation(host, quote, PAGE_BREAK_SENTINEL)
    if (mark) {
      mark.classList.add('doc-hl-flash')
      mark.scrollIntoView({ block: 'center', behavior: 'smooth' })
    }
  }

  async function render() {
    loading = true
    err = null
    try {
      host.innerHTML = ''
      host.style.zoom = ''
      const reflow = mode === 'reflow'
      await renderAsync(blob, host, undefined, {
        className: 'docx',
        inWrapper: true,
        // fit mode preserves A4 page geometry; reflow drops it.
        ignoreWidth: reflow,
        ignoreHeight: reflow,
        breakPages: !reflow,
        renderChanges: trackedPolicy !== 'reject',
        experimental: true,
        useBase64URL: true,
      })
      applyHighlight()
      if (!reflow) updateZoom()
    } catch (e) {
      err = (e as Error).message
    } finally {
      loading = false
    }
  }

  $effect(() => {
    void blob
    void trackedPolicy
    void mode
    void render()
  })

  $effect(() => {
    void revision
    void quote
    if (!loading && !err) applyHighlight()
  })

  // Re-fit on panel resize, but only in fit mode. The observer is
  // attached / detached as the mode flips so the reflow mode pays
  // no per-resize cost.
  $effect(() => {
    if (mode !== 'fit' || !container) {
      resizeObserver?.disconnect()
      resizeObserver = null
      return
    }
    resizeObserver = new ResizeObserver(() => updateZoom())
    resizeObserver.observe(container)
    return () => {
      resizeObserver?.disconnect()
      resizeObserver = null
    }
  })

  function toggleMode() {
    mode = mode === 'fit' ? 'reflow' : 'fit'
  }
</script>

<div bind:this={container} class="docx-container h-full min-h-0 overflow-auto bg-(--color-surface-100) p-4 relative">
  {#if !loading && !err}
    <button
      type="button"
      onclick={toggleMode}
      title={mode === 'fit'
        ? i18n.t('DocxView.switchToReflow')
        : i18n.t('DocxView.switchToFit')}
      class="absolute top-2 right-3 z-10 h-7 px-2.5 text-[11px] font-medium inline-flex items-center gap-1.5
             rounded-(--radius-md) border border-(--color-surface-200)
             bg-(--color-surface-0) text-(--color-text-secondary)
             hover:bg-(--color-surface-50) hover:text-(--color-text-primary)
             shadow-sm"
    >
      {#if mode === 'fit'}
        <AlignLeft size={12} />{i18n.t('DocxView.modeReflow')}
      {:else}
        <Maximize2 size={12} />{i18n.t('DocxView.modeFit')}
      {/if}
    </button>
  {/if}

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
  <div bind:this={host} class={`docx-body docx-mode-${mode} tracked-${trackedPolicy}`}></div>
</div>

<style>
  /* ── Reflow mode (auto-fit width, page geometry dropped) ───── */
  :global(.docx-body.docx-mode-reflow .docx-wrapper) {
    width: 100% !important;
    max-width: 100% !important;
    padding: 0 !important;
    background: transparent !important;
  }
  :global(.docx-body.docx-mode-reflow .docx-wrapper > .docx),
  :global(.docx-body.docx-mode-reflow .docx-wrapper > section.docx) {
    width: 100% !important;
    max-width: 100% !important;
    min-width: 0 !important;
    padding: 1.5rem 1.75rem !important;
    box-sizing: border-box !important;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.06) !important;
  }
  :global(.docx-body.docx-mode-reflow table) {
    max-width: 100% !important;
    width: auto;
    table-layout: auto;
  }
  :global(.docx-body.docx-mode-reflow p),
  :global(.docx-body.docx-mode-reflow li),
  :global(.docx-body.docx-mode-reflow td) {
    overflow-wrap: anywhere;
    word-break: normal;
  }

  /* ── Fit mode (native A4 page + CSS zoom) ───────────────────── */
  /* The page keeps its intrinsic width/height; `zoom` on .docx-body
     scales the visual + layout. We pin the wrapper centred so the
     page sits comfortably when the panel is wider than the scaled
     page after clamping (e.g. very wide screens hit the 1.5x cap). */
  :global(.docx-body.docx-mode-fit) {
    display: flex;
    justify-content: center;
  }

  /* Tracked-changes rules apply in both modes. */
  :global(.docx-body.tracked-accept del),
  :global(.docx-body.tracked-accept .docx-delete),
  :global(.docx-body.tracked-accept .docx-deletion) {
    display: none !important;
  }

  :global(.docx-body.tracked-reject ins),
  :global(.docx-body.tracked-reject .docx-insert),
  :global(.docx-body.tracked-reject .docx-insertion) {
    display: none !important;
  }

  :global(.docx-body.tracked-reject del),
  :global(.docx-body.tracked-reject .docx-delete),
  :global(.docx-body.tracked-reject .docx-deletion) {
    text-decoration: none !important;
    color: inherit !important;
    background: transparent !important;
  }
</style>
