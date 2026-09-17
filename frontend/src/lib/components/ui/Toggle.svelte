<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    checked?: boolean
    label?: string
    description?: string
    disabled?: boolean
    size?: 'sm' | 'md'
    children?: Snippet
    onchange?: (value: boolean) => void
    class?: string
  }

  let {
    checked = $bindable(false),
    label,
    description,
    disabled = false,
    size = 'md',
    children,
    onchange,
    class: extraClass = '',
  }: Props = $props()

  const trackSize = $derived(size === 'sm' ? 'h-4 w-7' : 'h-5 w-9')
  const thumbSize = $derived(size === 'sm' ? 'h-3 w-3' : 'h-4 w-4')
  const thumbOffset = $derived(
    checked ? (size === 'sm' ? 'translate-x-3' : 'translate-x-4') : 'translate-x-0.5'
  )

  function toggle() {
    if (disabled) return
    checked = !checked
    onchange?.(checked)
  }

  function handleKey(e: KeyboardEvent) {
    if (e.key === ' ' || e.key === 'Enter') {
      e.preventDefault()
      toggle()
    }
  }
</script>

<label
  class="inline-flex items-start gap-3 cursor-pointer
         {disabled ? 'opacity-50 cursor-not-allowed' : ''}
         {extraClass}"
>
  <button
    type="button"
    role="switch"
    aria-checked={checked}
    aria-label={label}
    {disabled}
    onclick={toggle}
    onkeydown={handleKey}
    class="relative inline-flex shrink-0 items-center rounded-full
           transition-colors duration-(--transition-fast)
           focus:outline-none focus-visible:ring-2 focus-visible:ring-(--color-brand-500) focus-visible:ring-offset-1
           {trackSize}
           {checked ? 'bg-(--color-brand-500)' : 'bg-(--color-surface-300)'}
           disabled:cursor-not-allowed"
  >
    <span
      class="inline-block rounded-full bg-white shadow-sm
             transform transition-transform duration-(--transition-fast)
             {thumbSize} {thumbOffset}"
    ></span>
  </button>

  {#if label || description || children}
    <span class="flex flex-col -mt-0.5">
      {#if label}<span class="text-sm text-(--color-text-primary)">{label}</span>{/if}
      {#if children}{@render children()}{/if}
      {#if description}
        <span class="text-xs text-(--color-text-secondary)">{description}</span>
      {/if}
    </span>
  {/if}
</label>
