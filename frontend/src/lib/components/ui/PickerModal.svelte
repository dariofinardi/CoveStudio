<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!-- Generic searchable single/multi-select picker (documents, workflows…). -->
<script lang="ts" module>
  import type { BadgeTone } from './Badge.svelte'

  export interface PickerItem {
    id: string
    label: string
    sublabel?: string
    /** Optional filter key (e.g. primary domain) matched against the filter select. */
    tag?: string
    /** Extra filter keys — the item matches the filter if `tag` OR any of
     *  these equals the selected value (e.g. a template's also-applicable
     *  domains). */
    tags?: string[]
    /** Optional right-aligned pill — used by the workflow picker to surface
     *  Tabellare vs Assistente at a glance without lengthening the sublabel. */
    badge?: { text: string; tone: BadgeTone }
  }
</script>

<script lang="ts">
  import Modal from './Modal.svelte'
  import Input from './Input.svelte'
  import Select from './Select.svelte'
  import Button from './Button.svelte'
  import Spinner from './Spinner.svelte'
  import EmptyState from './EmptyState.svelte'
  import Badge from './Badge.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { Search, Check, Upload } from 'lucide-svelte'

  interface Props {
    open?: boolean
    title: string
    items: PickerItem[]
    multi?: boolean
    loading?: boolean
    /** Pre-selected ids. */
    initial?: string[]
    /** When set, renders a filter select that matches `PickerItem.tag`. */
    filterOptions?: { value: string; label: string }[]
    /** Current filter value (bindable). Empty string = no filter. */
    filterValue?: string
    onpick: (ids: string[]) => void
    /** When set, renders an "Upload" affordance bottom-left. The host is
     *  expected to open a file picker, run the upload, refresh `items`,
     *  and optionally pre-select the newly uploaded ids — this component
     *  doesn't own that flow because the storage endpoint differs per
     *  caller (chat attachments vs. tabular-review docs vs. project
     *  uploads). The button is disabled while `uploading` is true. */
    onUpload?: () => void | Promise<void>
    /** Whether an upload is currently in flight — shows a spinner on the
     *  Upload button and disables it. */
    uploading?: boolean
    /** When true, the Confirm button stays enabled even with no items
     *  selected. The Upload flow commits its changes server-side as
     *  soon as the upload finishes, so once the user has uploaded
     *  anything during this picker session the modal should still be
     *  closeable via Confirm even if they don't tick anything else.
     *  Confirm still calls `onpick(selected)` if anything is selected. */
    confirmAlwaysEnabled?: boolean
  }

  let {
    open = $bindable(false),
    title,
    items,
    multi = false,
    loading = false,
    initial = [],
    filterOptions,
    filterValue = $bindable(''),
    onpick,
    onUpload,
    uploading = false,
    confirmAlwaysEnabled = false,
  }: Props = $props()

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)

  let query = $state('')
  let selected = $state<Set<string>>(new Set())

  $effect(() => {
    if (open) {
      query = ''
      selected = new Set(initial)
    }
  })

  const filtered = $derived(
    items.filter((i) => {
      if (!i.label.toLowerCase().includes(query.trim().toLowerCase())) return false
      if (filterValue && i.tag !== filterValue && !(i.tags ?? []).includes(filterValue))
        return false
      return true
    })
  )

  // Are all the *visible* (search + filter applied) items selected?
  // The select-all toggle scopes to the visible set so the user can
  // select-all-medical-PDFs without affecting other domains they
  // haven't surfaced.
  const allVisibleSelected = $derived(
    filtered.length > 0 && filtered.every((i) => selected.has(i.id))
  )

  function toggle(id: string) {
    if (multi) {
      const next = new Set(selected)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      selected = next
    } else {
      selected = new Set([id])
    }
  }

  function toggleAllVisible() {
    const next = new Set(selected)
    if (allVisibleSelected) {
      for (const i of filtered) next.delete(i.id)
    } else {
      for (const i of filtered) next.add(i.id)
    }
    selected = next
  }

  function confirm() {
    // Empty selection can legitimately reach here when the host enabled
    // `confirmAlwaysEnabled` after a successful upload — skip the
    // callback in that case since there's nothing new to pick.
    if (selected.size > 0) onpick([...selected])
    open = false
  }
</script>

<Modal bind:open {title} size="md">
  <div class="space-y-3">
    <div class="flex items-center gap-2">
      <Input bind:value={query} placeholder={t('Common.search')} size="sm" class="flex-1">
        {#snippet iconBefore()}
          <Search size={14} />
        {/snippet}
      </Input>
      {#if filterOptions}
        <Select options={filterOptions} bind:value={filterValue} size="sm" class="w-40" />
      {/if}
    </div>

    {#if multi && !loading && filtered.length > 1}
      <!-- Select-all bar — appears only in multi mode and only when
           there's more than one visible row (no point on a list of one).
           Selection state is scoped to the *visible* set so the user
           can select-all on a search-filtered subset without disturbing
           selections outside it. -->
      <div class="flex items-center justify-between px-1 text-xs text-(--color-text-secondary)">
        <span>
          {t('PickerModal.selectedCount', { n: selected.size })}
        </span>
        <button
          type="button"
          onclick={toggleAllVisible}
          class="font-medium text-(--color-brand-600) hover:text-(--color-brand-700) focus:outline-none"
        >
          {allVisibleSelected ? t('PickerModal.deselectAll') : t('PickerModal.selectAll')}
        </button>
      </div>
    {/if}

    <div class="h-72 overflow-y-auto -mx-1 px-1">
      {#if loading}
        <div class="flex items-center justify-center gap-2 text-sm text-(--color-text-secondary) py-12">
          <Spinner size="sm" />
          {t('Common.loading')}
        </div>
      {:else if filtered.length === 0}
        <EmptyState title={t('Common.none')} />
      {:else}
        <ul class="flex flex-col gap-1">
          {#each filtered as item (item.id)}
            {@const on = selected.has(item.id)}
            <li>
              <button
                type="button"
                onclick={() => toggle(item.id)}
                class="w-full flex items-center gap-2.5 px-2.5 py-2 rounded-(--radius-md) text-left
                       transition-colors duration-(--transition-fast)
                       focus:outline-none focus-visible:ring-2 focus-visible:ring-(--color-brand-500)
                       {on ? 'bg-(--color-brand-50)' : 'hover:bg-(--color-hover-bg)'}"
              >
                <span
                  class="shrink-0 inline-flex h-4 w-4 items-center justify-center rounded-(--radius-sm) border
                         {on
                           ? 'bg-(--color-brand-500) border-(--color-brand-500) text-white'
                           : 'border-(--color-surface-300)'}"
                >
                  {#if on}<Check size={11} />{/if}
                </span>
                <span class="flex-1 min-w-0">
                  <span class="block text-sm text-(--color-text-primary) truncate">{item.label}</span>
                  {#if item.sublabel}
                    <span class="block text-xs text-(--color-text-secondary) truncate">{item.sublabel}</span>
                  {/if}
                </span>
                {#if item.badge}
                  <span class="shrink-0 ml-2">
                    <Badge tone={item.badge.tone} size="xs">{item.badge.text}</Badge>
                  </span>
                {/if}
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </div>

  {#snippet footer()}
    {#if onUpload}
      <Button
        variant="secondary"
        size="sm"
        loading={uploading}
        onclick={() => void onUpload?.()}
        class="mr-auto"
      >
        <Upload size={14} class="mr-1" />{t('Common.upload')}
      </Button>
    {/if}
    <Button variant="ghost" onclick={() => (open = false)}>{t('Common.cancel')}</Button>
    <Button
      disabled={selected.size === 0 && !confirmAlwaysEnabled}
      onclick={confirm}
    >{t('Common.confirm')}</Button>
  {/snippet}
</Modal>
