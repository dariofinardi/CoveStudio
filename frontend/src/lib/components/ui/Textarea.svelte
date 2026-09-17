<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import type { HTMLTextareaAttributes } from 'svelte/elements'

  interface Props extends Omit<HTMLTextareaAttributes, 'class' | 'value'> {
    value?: string
    label?: string
    hint?: string
    error?: string
    autosize?: boolean
    minRows?: number
    maxRows?: number
    class?: string
  }

  let {
    value = $bindable(''),
    label,
    hint,
    error,
    autosize = false,
    minRows = 3,
    maxRows = 12,
    id,
    rows,
    class: extraClass = '',
    ...rest
  }: Props = $props()

  const autoId = `textarea-${Math.random().toString(36).slice(2, 9)}`
  const taId = $derived(id ?? autoId)

  let textarea: HTMLTextAreaElement | undefined = $state()

  $effect(() => {
    if (!autosize || !textarea) return
    void value
    textarea.style.height = 'auto'
    const lineHeight = parseFloat(getComputedStyle(textarea).lineHeight)
    const minH = lineHeight * minRows
    const maxH = lineHeight * maxRows
    const next = Math.min(maxH, Math.max(minH, textarea.scrollHeight))
    textarea.style.height = `${next}px`
    textarea.style.overflowY = textarea.scrollHeight > maxH ? 'auto' : 'hidden'
  })
</script>

<div class="flex flex-col gap-1 {extraClass}">
  {#if label}
    <label for={taId} class="text-xs font-medium text-(--color-text-secondary)">
      {label}
    </label>
  {/if}

  <textarea
    bind:this={textarea}
    id={taId}
    bind:value
    rows={rows ?? minRows}
    class="w-full bg-(--color-surface-0) text-(--color-text-primary)
           border rounded-(--radius-md) px-3 py-2 text-sm leading-relaxed
           transition-colors duration-(--transition-fast)
           focus:outline-none focus:ring-2 focus:ring-(--color-brand-500) focus:border-(--color-brand-500)
           disabled:opacity-50 disabled:cursor-not-allowed
           placeholder:text-(--color-text-disabled)
           {autosize ? 'resize-none' : 'resize-y'}
           {error ? 'border-(--color-danger-500)' : 'border-(--color-surface-200)'}"
    {...rest}
  ></textarea>

  {#if error}
    <p class="text-xs text-(--color-danger-500)">{error}</p>
  {:else if hint}
    <p class="text-xs text-(--color-text-secondary)">{hint}</p>
  {/if}
</div>
