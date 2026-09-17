<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts" module>
  export interface Chip {
    value: string
    label: string
    disabled?: boolean
  }
</script>

<script lang="ts">
  interface Props {
    chips: Chip[]
    /** Selected value(s). Array when multi, string|null when single. */
    selected?: string[] | string | null
    multi?: boolean
    size?: 'sm' | 'md'
    onchange?: (selected: string[] | string | null) => void
    class?: string
  }

  let {
    chips,
    selected = $bindable(undefined),
    multi = false,
    size = 'md',
    onchange,
    class: extraClass = '',
  }: Props = $props()

  // Normalize once for hit-testing.
  const selectedSet = $derived.by(() => {
    if (multi) return new Set(Array.isArray(selected) ? selected : [])
    return new Set(typeof selected === 'string' ? [selected] : [])
  })

  const padClass = $derived(size === 'sm' ? 'px-2 py-0.5 text-xs' : 'px-2.5 py-1 text-sm')

  function toggle(chip: Chip) {
    if (chip.disabled) return
    if (multi) {
      const current = Array.isArray(selected) ? [...selected] : []
      const idx = current.indexOf(chip.value)
      if (idx >= 0) current.splice(idx, 1)
      else current.push(chip.value)
      selected = current
      onchange?.(current)
    } else {
      const next = selectedSet.has(chip.value) ? null : chip.value
      selected = next
      onchange?.(next)
    }
  }
</script>

<div class="flex flex-wrap gap-1.5 {extraClass}">
  {#each chips as chip (chip.value)}
    {@const on = selectedSet.has(chip.value)}
    <button
      type="button"
      aria-pressed={on}
      disabled={chip.disabled}
      onclick={() => toggle(chip)}
      class="rounded-full border font-medium
             transition-colors duration-(--transition-fast)
             focus:outline-none focus-visible:ring-2 focus-visible:ring-(--color-brand-500)
             disabled:opacity-40 disabled:cursor-not-allowed
             {padClass}
             {on
               ? 'bg-(--color-brand-500) border-(--color-brand-500) text-white'
               : 'bg-(--color-surface-0) border-(--color-surface-300) text-(--color-text-secondary) hover:border-(--color-brand-300) hover:text-(--color-text-primary)'}"
    >
      {chip.label}
    </button>
  {/each}
</div>
