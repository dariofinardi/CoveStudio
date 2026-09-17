<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Settings → Data sources. Sub-nav over the document corpora the user
  can index into the RAG knowledge base: local folder sync plus every
  corpus plugin registered in config/corpora-plugins/ (EUR-Lex,
  Italian Legal, CNIL, …).
-->
<script lang="ts">
  import SyncSection from './SyncSection.svelte'
  import EurlexSection from './EurlexSection.svelte'
  import CorpusSection from './CorpusSection.svelte'
  import Input from '$lib/components/ui/Input.svelte'
  import Select from '$lib/components/ui/Select.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import { corporaApi, type CorpusItem } from '$lib/api/data-sources'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { ChevronLeft, ChevronRight } from 'lucide-svelte'

  let corpora = $state<CorpusItem[]>([])
  let loading = $state(true)
  /** 'sync' or a corpus id. */
  let active = $state<string>('sync')
  let titleFilter = $state('')
  let jurisdictionFilter = $state('')
  let typeFilter = $state('')

  /** Localized jurisdiction name for the filter dropdown. Falls back
   *  to the upper-cased code if the locale lacks an entry. */
  function jurLabel(code: string): string {
    const key = `Corpora.jurisdiction.${code}`
    const label = i18n.t(key)
    return label === key ? code.toUpperCase() : label
  }

  // A corpus gets a tab only when it has a runnable adapter AND its
  // manifest hasn't retired it (`available: false`). EUR-Lex renders
  // its dedicated EurlexSection (via the active === 'eurlex' branch
  // below); CNIL, Italian-Legal and declarative plugins render
  // CorpusSection.
  const allCorpora = $derived.by(() => {
    return corpora
      .filter((c) => c.runnable && c.available)
      .sort((a, b) => a.display_name.localeCompare(b.display_name))
  })

  /** Jurisdiction dropdown options, derived from the visible corpora. */
  const jurisdictionOptions = $derived.by(() => {
    const set = new Set<string>()
    for (const c of allCorpora) {
      if (c.discovery?.jurisdiction) set.add(c.discovery.jurisdiction)
    }
    return [
      { value: '', label: i18n.t('Corpora.filters.allJurisdictions') },
      ...[...set].sort().map((j) => ({ value: j, label: jurLabel(j) })),
    ]
  })

  const typeOptions = $derived([
    { value: '', label: i18n.t('Corpora.filters.allTypes') },
    { value: 'legislation', label: i18n.t('Corpora.docType.legislation') },
    { value: 'case_law', label: i18n.t('Corpora.docType.caseLaw') },
  ])

  const visibleCorpora = $derived.by(() => {
    const q = titleFilter.trim().toLowerCase()
    return allCorpora.filter((c) => {
      if (q && !c.display_name.toLowerCase().includes(q)) return false
      if (jurisdictionFilter && c.discovery?.jurisdiction !== jurisdictionFilter)
        return false
      if (typeFilter && !(c.discovery?.doc_types ?? []).includes(typeFilter))
        return false
      return true
    })
  })

  $effect(() => {
    corporaApi
      .list()
      .then((r) => (corpora = r.corpora))
      .catch(() => (corpora = []))
      .finally(() => (loading = false))
  })

  const activeCorpus = $derived(allCorpora.find((c) => c.id === active))

  $effect(() => {
    void visibleCorpora.length
    if (active !== 'sync' && !visibleCorpora.some((c) => c.id === active)) {
      active = visibleCorpora[0]?.id ?? 'sync'
    }
  })

  // ── horizontally-scrollable tab strip ────────────────────────────
  let strip = $state<HTMLDivElement>()
  let overflowing = $state(false)

  function measure() {
    if (strip) overflowing = strip.scrollWidth > strip.clientWidth + 4
  }
  $effect(() => {
    void visibleCorpora.length
    queueMicrotask(measure)
    window.addEventListener('resize', measure)
    return () => window.removeEventListener('resize', measure)
  })

  function scrollStrip(dir: -1 | 1) {
    strip?.scrollBy({ left: dir * 200, behavior: 'smooth' })
  }
</script>

<div class="space-y-4">
  <div class="flex flex-wrap items-end gap-2">
    <Input
      bind:value={titleFilter}
      placeholder={i18n.t('Corpora.filters.titlePlaceholder')}
      class="min-w-48 flex-1 max-w-xs"
    />
    <Select
      bind:value={jurisdictionFilter}
      options={jurisdictionOptions}
      label={i18n.t('Corpora.filters.jurisdiction')}
      size="md"
      class="w-44"
    />
    <Select
      bind:value={typeFilter}
      options={typeOptions}
      label={i18n.t('Corpora.filters.type')}
      size="md"
      class="w-44"
    />
  </div>

  <div class="flex items-center border-b border-(--color-surface-200)">
    {#if overflowing}
      <button
        type="button"
        class="shrink-0 px-1 h-9 text-(--color-text-secondary) hover:text-(--color-text-primary)"
        aria-label={i18n.t('Common.previous')}
        onclick={() => scrollStrip(-1)}
      >
        <ChevronLeft size={16} />
      </button>
    {/if}

    <div class="flex flex-1 min-w-0 items-stretch">
      <button
        type="button"
        onclick={() => (active = 'sync')}
        class="sticky left-0 z-10 px-3 h-9 text-sm border-b-2 -mb-px border-r border-(--color-surface-200) whitespace-nowrap bg-(--color-surface-0)
               {active === 'sync'
                 ? 'border-(--color-brand-500) text-(--color-text-primary) font-medium'
                 : 'border-transparent text-(--color-text-secondary) hover:text-(--color-text-primary)'}"
      >
        {i18n.t('Account.localDocsLink')}
      </button>
      <div bind:this={strip} class="flex gap-1 overflow-x-auto no-scrollbar flex-1 min-w-0 pl-1">
        {#each visibleCorpora as c (c.id)}
          <button
            type="button"
            onclick={() => (active = c.id)}
            class="px-3 h-9 text-sm border-b-2 -mb-px whitespace-nowrap
                 {active === c.id
                   ? 'border-(--color-brand-500) text-(--color-text-primary) font-medium'
                   : 'border-transparent text-(--color-text-secondary) hover:text-(--color-text-primary)'}"
          >
            {c.display_name}
          </button>
        {/each}
        {#if loading}
          <span class="flex items-center px-2"><Spinner size="sm" /></span>
        {/if}
      </div>
    </div>

    {#if overflowing}
      <button
        type="button"
        class="shrink-0 px-1 h-9 text-(--color-text-secondary) hover:text-(--color-text-primary)"
        aria-label={i18n.t('Common.next')}
        onclick={() => scrollStrip(1)}
      >
        <ChevronRight size={16} />
      </button>
    {/if}
  </div>

  {#if active === 'sync'}
    <SyncSection />
  {:else if active === 'eurlex'}
    <EurlexSection />
  {:else if activeCorpus}
    {#key activeCorpus.id}
      <CorpusSection corpus={activeCorpus} />
    {/key}
  {/if}
</div>
