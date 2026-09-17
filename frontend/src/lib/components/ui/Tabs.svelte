<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts" module>
  export interface TabItem {
    id: string
    label: string
    count?: number
    disabled?: boolean
  }
</script>

<script lang="ts">
  interface Props {
    tabs: TabItem[]
    active?: string
    size?: 'sm' | 'md'
    onchange?: (id: string) => void
    class?: string
  }

  let {
    tabs,
    active = $bindable(tabs[0]?.id ?? ''),
    size = 'md',
    onchange,
    class: extraClass = '',
  }: Props = $props()

  const padClass = $derived(size === 'sm' ? 'px-2.5 py-1.5 text-xs' : 'px-3 py-2 text-sm')

  function select(tab: TabItem) {
    if (tab.disabled) return
    active = tab.id
    onchange?.(tab.id)
  }
</script>

<div
  role="tablist"
  class="flex items-center gap-1 border-b border-(--color-surface-200) {extraClass}"
>
  {#each tabs as tab (tab.id)}
    {@const selected = tab.id === active}
    <button
      type="button"
      role="tab"
      aria-selected={selected}
      disabled={tab.disabled}
      onclick={() => select(tab)}
      class="relative font-medium -mb-px border-b-2
             transition-colors duration-(--transition-fast)
             focus:outline-none focus-visible:ring-2 focus-visible:ring-(--color-brand-500) rounded-t-(--radius-sm)
             disabled:opacity-40 disabled:cursor-not-allowed
             {padClass}
             {selected
               ? 'border-(--color-brand-500) text-(--color-brand-600)'
               : 'border-transparent text-(--color-text-secondary) hover:text-(--color-text-primary)'}"
    >
      {tab.label}
      {#if tab.count !== undefined}
        <span
          class="ml-1.5 inline-flex items-center justify-center min-w-4 h-4 px-1
                 rounded-full text-[10px] font-semibold
                 {selected
                   ? 'bg-(--color-brand-100) text-(--color-brand-700)'
                   : 'bg-(--color-surface-200) text-(--color-text-secondary)'}"
        >
          {tab.count}
        </span>
      {/if}
    </button>
  {/each}
</div>
