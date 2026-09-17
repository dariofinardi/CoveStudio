<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import type { HTMLInputAttributes } from 'svelte/elements'

  interface Props extends Omit<HTMLInputAttributes, 'class' | 'value' | 'size'> {
    value?: string
    label?: string
    hint?: string
    error?: string
    size?: 'sm' | 'md' | 'lg'
    iconBefore?: Snippet
    iconAfter?: Snippet
    class?: string
  }

  let {
    value = $bindable(''),
    label,
    hint,
    error,
    size = 'md',
    iconBefore,
    iconAfter,
    id,
    type = 'text',
    class: extraClass = '',
    ...rest
  }: Props = $props()

  const autoId = `input-${Math.random().toString(36).slice(2, 9)}`
  const inputId = $derived(id ?? autoId)

  const sizeClass = $derived(
    { sm: 'h-8 text-xs', md: 'h-9 text-sm', lg: 'h-11 text-base' }[size]
  )
</script>

<div class="flex flex-col gap-1 {extraClass}">
  {#if label}
    <label for={inputId} class="text-xs font-medium text-(--color-text-secondary)">
      {label}
    </label>
  {/if}

  <div class="relative flex items-center">
    {#if iconBefore}
      <span class="absolute left-2.5 inline-flex text-(--color-text-secondary) pointer-events-none">
        {@render iconBefore()}
      </span>
    {/if}

    <input
      id={inputId}
      {type}
      bind:value
      class="w-full bg-(--color-surface-0) text-(--color-text-primary)
             border rounded-(--radius-md)
             transition-colors duration-(--transition-fast)
             focus:outline-none focus:ring-2 focus:ring-(--color-brand-500) focus:border-(--color-brand-500)
             disabled:opacity-50 disabled:cursor-not-allowed
             placeholder:text-(--color-text-disabled)
             {sizeClass}
             {iconBefore ? 'pl-9' : 'pl-3'}
             {iconAfter ? 'pr-9' : 'pr-3'}
             {error ? 'border-(--color-danger-500)' : 'border-(--color-surface-200)'}"
      {...rest}
    />

    {#if iconAfter}
      <span class="absolute right-2.5 inline-flex text-(--color-text-secondary) pointer-events-none">
        {@render iconAfter()}
      </span>
    {/if}
  </div>

  {#if error}
    <p class="text-xs text-(--color-danger-500)">{error}</p>
  {:else if hint}
    <p class="text-xs text-(--color-text-secondary)">{hint}</p>
  {/if}
</div>
