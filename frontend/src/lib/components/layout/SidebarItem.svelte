<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    label: string
    active?: boolean
    icon?: Snippet
    badge?: string | number
    onclick?: () => void
    class?: string
  }

  let { label, active = false, icon, badge, onclick, class: extraClass = '' }: Props =
    $props()
</script>

<button
  type="button"
  {onclick}
  aria-current={active ? 'page' : undefined}
  class="w-full flex items-center gap-2.5 px-3 h-9 rounded-(--radius-md)
         text-sm font-medium text-left
         transition-colors duration-(--transition-fast)
         focus:outline-none focus-visible:ring-2 focus-visible:ring-(--color-brand-500)
         {active
           ? 'bg-(--color-active-bg) text-(--color-brand-700)'
           : 'text-(--color-text-secondary) hover:bg-(--color-hover-bg) hover:text-(--color-text-primary)'}
         {extraClass}"
>
  {#if icon}
    <span class="shrink-0 inline-flex {active ? 'text-(--color-brand-600)' : ''}">
      {@render icon()}
    </span>
  {/if}
  <span class="flex-1 truncate">{label}</span>
  {#if badge !== undefined}
    <span
      class="shrink-0 inline-flex items-center justify-center min-w-5 h-5 px-1.5
             rounded-full text-[10px] font-semibold
             bg-(--color-surface-200) text-(--color-text-secondary)"
    >
      {badge}
    </span>
  {/if}
</button>
