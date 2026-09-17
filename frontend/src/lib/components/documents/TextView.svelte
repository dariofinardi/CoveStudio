<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Plain-text renderer for Markdown, TXT, CSV and RTF. Markdown is
  rendered formatted; the others as monospaced text. RTF is decoded
  with a lightweight control-word stripper (no plugin). The cited
  passage is highlighted and text stays selectable.
-->
<script lang="ts">
  import { renderMarkdown } from '$lib/utils/markdown'
  import { highlightCitation } from '$lib/utils/highlight'
  import { PAGE_BREAK_SENTINEL } from '$lib/types/citation'
  import { i18n } from '$lib/stores/i18n.svelte'

  interface Props {
    text: string
    /** `md` renders formatted; everything else renders monospaced. */
    kind: 'md' | 'rtf' | 'plain'
    quote?: string
    revision?: number
  }

  let { text, kind, quote, revision = 0 }: Props = $props()

  /** Strip RTF control words / groups down to readable text. */
  function rtfToText(rtf: string): string {
    return rtf
      .replace(/\\par[d]?\b/g, '\n')
      .replace(/\\tab\b/g, '\t')
      .replace(/\\line\b/g, '\n')
      .replace(/\\'[0-9a-fA-F]{2}/g, ' ')
      .replace(/\\[a-zA-Z]+-?\d* ?/g, '')
      .replace(/[{}]/g, '')
      .replace(/\r\n?/g, '\n')
      .replace(/\n{3,}/g, '\n\n')
      .trim()
  }

  /** Normalise line endings: collapse CRLF / lone CR to LF, strip BOM.
   *  The browser preserves CR characters in <pre> text nodes; visually
   *  they can render as a double-break and they pollute any text-based
   *  search that doesn't strip them. `highlightCitation` already
   *  filters non-alphanumerics, so this is mostly a cosmetic fix — but
   *  it also keeps the displayed text consistent across Windows-saved
   *  TXT (CRLF), legacy Mac (CR), and Unix (LF) sources. */
  function normaliseNewlines(s: string): string {
    return s.replace(/^﻿/, '').replace(/\r\n?/g, '\n')
  }

  const body = $derived(
    kind === 'rtf' ? rtfToText(text) : normaliseNewlines(text),
  )
  const html = $derived(kind === 'md' ? renderMarkdown(body) : '')

  let host = $state<HTMLElement>()

  $effect(() => {
    void body
    void html
    void revision
    void quote
    const el = host
    if (!el || !quote) return
    queueMicrotask(() => {
      const mark = highlightCitation(el, quote!, PAGE_BREAK_SENTINEL)
      if (mark) {
        mark.classList.add('doc-hl-flash')
        mark.scrollIntoView({ block: 'center', behavior: 'smooth' })
      } else {
        // No fragment of the quote was located in the rendered text —
        // most often because the model invented the quote (citing a
        // section of its own report instead of the source). Make the
        // failure visible: log a console warning and reset the scroll
        // so the viewer doesn't sit silently in the middle of the doc.
        console.warn(
          '[TextView] citation quote not found in rendered text — opening at the top.',
          { quoteHead: quote!.slice(0, 80) },
        )
        el.scrollTop = 0
      }
    })
  })
</script>

<div class="h-full min-h-0 overflow-auto bg-(--color-surface-0) p-5">
  {#if kind === 'md'}
    <div bind:this={host} class="md-body text-sm text-(--color-text-primary) max-w-2xl mx-auto">
      {@html html}
    </div>
  {:else}
    <pre
      bind:this={host}
      class="text-xs text-(--color-text-primary) whitespace-pre-wrap break-words font-mono leading-relaxed">{body}</pre>
  {/if}
  {#if !body.trim()}
    <p class="text-sm text-(--color-text-secondary) text-center py-12">
      {i18n.t('Documents.viewer.errorLoading')}
    </p>
  {/if}
</div>
