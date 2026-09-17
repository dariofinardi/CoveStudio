<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts" module>
  export interface SelectOption<V extends string | number = string | number> {
    value: V
    label: string
    disabled?: boolean
  }
</script>

<script lang="ts" generics="T extends string | number">
  import type { HTMLSelectAttributes } from 'svelte/elements'
  import { ChevronDown } from 'lucide-svelte'

  interface Props extends Omit<HTMLSelectAttributes, 'class' | 'value' | 'size'> {
    value?: T
    options: SelectOption<T>[]
    label?: string
    hint?: string
    error?: string
    size?: 'sm' | 'md' | 'lg'
    placeholder?: string
    class?: string
  }

  let {
    value = $bindable(),
    options,
    label,
    hint,
    error,
    size = 'md',
    placeholder,
    id,
    class: extraClass = '',
    ...rest
  }: Props = $props()

  const autoId = `select-${Math.random().toString(36).slice(2, 9)}`
  const selectId = $derived(id ?? autoId)

  const sizeClass = $derived(
    { sm: 'h-8 text-xs', md: 'h-9 text-sm', lg: 'h-11 text-base' }[size]
  )
</script>

<div class="flex flex-col gap-1 {extraClass}">
  {#if label}
    <label for={selectId} class="text-xs font-medium text-(--color-text-secondary)">
      {label}
    </label>
  {/if}

  <!-- relative wrapper so the chevron sits over the select -->
  <div class="relative">
    <select
      id={selectId}
      bind:value
      class="w-full bg-(--color-surface-0) text-(--color-text-primary)
             border rounded-(--radius-md) px-3 pr-9
             transition-colors duration-(--transition-fast)
             focus:outline-none focus:ring-2 focus:ring-(--color-brand-500) focus:border-(--color-brand-500)
             disabled:opacity-50 disabled:cursor-not-allowed
             appearance-none cursor-pointer
             {sizeClass}
             {error ? 'border-(--color-danger-500)' : 'border-(--color-surface-200)'}"
      {...rest}
    >
      {#if placeholder}
        <option value="" disabled selected={value === undefined || value === ''}>
          {placeholder}
        </option>
      {/if}
      {#each options as opt (opt.value)}
        <option value={opt.value} disabled={opt.disabled}>{opt.label}</option>
      {/each}
    </select>
    <!-- Dropdown affordance — always visible, click passes through. -->
    <span
      class="pointer-events-none absolute right-2.5 top-1/2 -translate-y-1/2
             text-(--color-text-secondary)"
    >
      <ChevronDown size={16} />
    </span>
  </div>

  {#if error}
    <p class="text-xs text-(--color-danger-500)">{error}</p>
  {:else if hint}
    <p class="text-xs text-(--color-text-secondary)">{hint}</p>
  {/if}
</div>
