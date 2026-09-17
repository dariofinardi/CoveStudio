<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Plain-textarea Markdown editor with a formatting toolbar. The toolbar
  buttons insert Markdown syntax at the cursor (heading prefixes, bold/
  italic wrappers, list prefixes) — no contenteditable, so the value is
  always clean Markdown that round-trips to the backend `prompt_md`.
-->
<script lang="ts">
  import { Heading1, Heading2, Heading3, Bold, Italic, List, ListOrdered } from 'lucide-svelte'

  interface Props {
    value?: string
    placeholder?: string
    minHeight?: string
    class?: string
    /** Fired on any value change (typing or toolbar insertion). */
    oninput?: () => void
  }

  let {
    value = $bindable(''),
    placeholder = '',
    minHeight = '260px',
    class: extraClass = '',
    oninput,
  }: Props = $props()

  let ta: HTMLTextAreaElement | undefined = $state()

  function restore(start: number, end: number) {
    queueMicrotask(() => {
      if (!ta) return
      ta.focus()
      ta.selectionStart = start
      ta.selectionEnd = end
    })
  }

  /** Prefix the line(s) overlapping the selection with `prefix`. */
  function linePrefix(prefix: string) {
    if (!ta) return
    const s = ta.selectionStart
    const e = ta.selectionEnd
    const lineStart = value.lastIndexOf('\n', s - 1) + 1
    value = value.slice(0, lineStart) + prefix + value.slice(lineStart)
    restore(s + prefix.length, e + prefix.length)
  }

  /** Wrap the selection with `marker` on both sides. */
  function wrap(marker: string) {
    if (!ta) return
    const s = ta.selectionStart
    const e = ta.selectionEnd
    const sel = value.slice(s, e)
    value = value.slice(0, s) + marker + sel + marker + value.slice(e)
    restore(s + marker.length, e + marker.length)
  }

  const tools = [
    { icon: Heading1, label: 'H1', run: () => linePrefix('# ') },
    { icon: Heading2, label: 'H2', run: () => linePrefix('## ') },
    { icon: Heading3, label: 'H3', run: () => linePrefix('### ') },
    { icon: Bold, label: 'Bold', run: () => wrap('**') },
    { icon: Italic, label: 'Italic', run: () => wrap('*') },
    { icon: List, label: 'Bulleted list', run: () => linePrefix('- ') },
    { icon: ListOrdered, label: 'Numbered list', run: () => linePrefix('1. ') },
  ]
</script>

<div
  class="border border-(--color-surface-200) rounded-(--radius-md) overflow-hidden
         bg-(--color-surface-0) {extraClass}"
>
  <div class="flex items-center gap-0.5 px-2 py-1.5 border-b border-(--color-surface-100) bg-(--color-surface-50)">
    {#each tools as tool, i (tool.label)}
      {#if i === 3 || i === 5}
        <span class="w-px h-4 bg-(--color-surface-200) mx-1"></span>
      {/if}
      <button
        type="button"
        title={tool.label}
        aria-label={tool.label}
        onclick={() => {
          tool.run()
          oninput?.()
        }}
        class="inline-flex h-7 w-7 items-center justify-center rounded-(--radius-sm)
               text-(--color-text-secondary) hover:bg-(--color-hover-bg) hover:text-(--color-text-primary)
               focus:outline-none focus-visible:ring-2 focus-visible:ring-(--color-brand-500)"
      >
        <tool.icon size={15} />
      </button>
    {/each}
  </div>

  <textarea
    bind:this={ta}
    bind:value
    oninput={() => oninput?.()}
    {placeholder}
    style:min-height={minHeight}
    class="w-full block resize-y px-3 py-2.5 text-sm leading-relaxed
           bg-transparent text-(--color-text-primary)
           placeholder:text-(--color-text-disabled)
           focus:outline-none font-mono"
  ></textarea>
</div>
