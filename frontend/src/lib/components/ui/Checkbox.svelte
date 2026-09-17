<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import type { HTMLInputAttributes } from 'svelte/elements'

  interface Props extends Omit<HTMLInputAttributes, 'class' | 'type' | 'checked' | 'children'> {
    checked?: boolean
    label?: string
    description?: string
    children?: Snippet
    class?: string
  }

  let {
    checked = $bindable(false),
    label,
    description,
    children,
    disabled,
    id,
    class: extraClass = '',
    ...rest
  }: Props = $props()

  const autoId = `cb-${Math.random().toString(36).slice(2, 9)}`
  const cbId = $derived(id ?? autoId)
</script>

<label
  for={cbId}
  class="inline-flex items-start gap-2.5 cursor-pointer
         {disabled ? 'opacity-50 cursor-not-allowed' : ''}
         {extraClass}"
>
  <input
    id={cbId}
    type="checkbox"
    bind:checked
    {disabled}
    class="mt-0.5 h-4 w-4 rounded-(--radius-sm) border-(--color-surface-300)
           text-(--color-brand-500)
           focus:ring-2 focus:ring-(--color-brand-500) focus:ring-offset-1
           accent-(--color-brand-500) cursor-pointer disabled:cursor-not-allowed"
    {...rest}
  />
  <span class="flex flex-col">
    {#if label}
      <span class="text-sm text-(--color-text-primary)">{label}</span>
    {:else if children}
      {@render children()}
    {/if}
    {#if description}
      <span class="text-xs text-(--color-text-secondary)">{description}</span>
    {/if}
  </span>
</label>
