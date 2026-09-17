<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Tag-style picker for *additional* domains (the `also_applicable_to`
  set). Selected domains render as removable chips; a free-text field
  filters the remaining available domains and offers them in a dropdown.
  Click a suggestion (or press Enter on the top match) to add it;
  Backspace on an empty field removes the last chip.

  Values are canonical domain ids; labels are localised by the caller
  via the `available` option list.
-->
<script lang="ts">
  import { X } from 'lucide-svelte'

  interface Option {
    value: string
    label: string
  }

  interface Props {
    /** Canonical ids of the additional domains, bindable. */
    selected?: string[]
    /** All selectable domains (already-localised labels). */
    available: Option[]
    /** Primary domain id — excluded from suggestions and chips. */
    exclude?: string
    placeholder?: string
    disabled?: boolean
    onchange?: (selected: string[]) => void
  }

  let {
    selected = $bindable([]),
    available,
    exclude = '',
    placeholder = '',
    disabled = false,
    onchange,
  }: Props = $props()

  let query = $state('')
  let openList = $state(false)
  let activeIdx = $state(0)

  const labelOf = (v: string) => available.find((o) => o.value === v)?.label ?? v

  // Suggestions: not the primary, not already chosen, matches the query.
  const suggestions = $derived.by(() => {
    const q = query.trim().toLowerCase()
    return available.filter(
      (o) =>
        o.value !== exclude &&
        !selected.includes(o.value) &&
        (q === '' || o.label.toLowerCase().includes(q) || o.value.toLowerCase().includes(q)),
    )
  })

  function commit(next: string[]) {
    selected = next
    onchange?.(next)
  }

  function add(value: string) {
    if (disabled || value === exclude || selected.includes(value)) return
    commit([...selected, value])
    query = ''
    activeIdx = 0
    openList = false
  }

  function remove(value: string) {
    if (disabled) return
    commit(selected.filter((v) => v !== value))
  }

  function onkeydown(e: KeyboardEvent) {
    if (disabled) return
    if (e.key === 'Backspace' && query === '' && selected.length > 0) {
      remove(selected[selected.length - 1])
      return
    }
    if (suggestions.length === 0) return
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      activeIdx = (activeIdx + 1) % suggestions.length
      openList = true
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      activeIdx = (activeIdx - 1 + suggestions.length) % suggestions.length
      openList = true
    } else if (e.key === 'Enter') {
      e.preventDefault()
      add(suggestions[Math.min(activeIdx, suggestions.length - 1)].value)
    } else if (e.key === 'Escape') {
      openList = false
    }
  }
</script>

<div class="relative">
  <div
    class="flex flex-wrap items-center gap-1.5 min-h-9 px-2 py-1.5
           bg-(--color-surface-0) border border-(--color-surface-300)
           rounded-(--radius-md) focus-within:border-(--color-brand-400)
           {disabled ? 'opacity-50' : ''}"
  >
    {#each selected as value (value)}
      <span
        class="inline-flex items-center gap-1 pl-2 pr-1 py-0.5 rounded-full text-xs font-medium
               bg-(--color-brand-50) text-(--color-brand-700) border border-(--color-brand-200)"
      >
        {labelOf(value)}
        {#if !disabled}
          <button
            type="button"
            aria-label="remove"
            onclick={() => remove(value)}
            class="rounded-full p-0.5 hover:bg-(--color-brand-100) text-(--color-brand-600)"
          >
            <X size={11} />
          </button>
        {/if}
      </span>
    {/each}

    {#if !disabled}
      <input
        bind:value={query}
        {placeholder}
        onfocus={() => (openList = true)}
        onblur={() => setTimeout(() => (openList = false), 120)}
        oninput={() => {
          openList = true
          activeIdx = 0
        }}
        {onkeydown}
        class="flex-1 min-w-24 bg-transparent text-sm text-(--color-text-primary)
               placeholder:text-(--color-text-disabled) focus:outline-none"
      />
    {/if}
  </div>

  {#if openList && !disabled && suggestions.length > 0}
    <ul
      class="absolute z-20 mt-1 w-full max-h-56 overflow-auto py-1
             bg-(--color-surface-0) border border-(--color-surface-200)
             rounded-(--radius-md) shadow-(--shadow-md)"
    >
      {#each suggestions as opt, i (opt.value)}
        <li>
          <button
            type="button"
            onmousedown={(e) => {
              e.preventDefault()
              add(opt.value)
            }}
            class="flex w-full items-center px-3 py-1.5 text-left text-sm
                   {i === activeIdx
                     ? 'bg-(--color-brand-50) text-(--color-brand-700)'
                     : 'text-(--color-text-primary) hover:bg-(--color-surface-100)'}"
          >
            {opt.label}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>
