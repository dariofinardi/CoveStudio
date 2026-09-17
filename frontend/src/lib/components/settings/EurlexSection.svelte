<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Settings → EUR-Lex corpus. Search EU legislation, index documents
  into the RAG knowledge base, and manage already-indexed documents.
-->
<script lang="ts">
  import Card from '$lib/components/ui/Card.svelte'
  import Input from '$lib/components/ui/Input.svelte'
  import Select from '$lib/components/ui/Select.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import IconButton from '$lib/components/ui/IconButton.svelte'
  import Toggle from '$lib/components/ui/Toggle.svelte'
  import Checkbox from '$lib/components/ui/Checkbox.svelte'
  import Badge from '$lib/components/ui/Badge.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import {
    eurlexApi,
    type EurlexConfig,
    type CorpusHit,
    type EurlexDocument,
  } from '$lib/api/data-sources'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { openExternal } from '$lib/tauri/commands'
  import { Trash2, RefreshCw, ExternalLink } from 'lucide-svelte'

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)

  /** The 24 official EU languages EUR-Lex publishes in. */
  const EU_LANGS = [
    'bg', 'cs', 'da', 'de', 'el', 'en', 'es', 'et', 'fi', 'fr', 'ga', 'hr',
    'hu', 'it', 'lt', 'lv', 'mt', 'nl', 'pl', 'pt', 'ro', 'sk', 'sl', 'sv',
  ]

  let config = $state<EurlexConfig>({ enabled: false, language: 'en', fallback_en: true })
  let loaded = $state(false)

  let query = $state('')
  let hits = $state<CorpusHit[]>([])
  let searchNote = $state<string | null>(null)
  let searching = $state(false)
  let syncingId = $state<string | null>(null)

  let docs = $state<EurlexDocument[]>([])
  let docsLoading = $state(true)

  const langOptions = $derived(EU_LANGS.map((l) => ({ value: l, label: l.toUpperCase() })))

  $effect(() => {
    eurlexApi
      .getConfig()
      .then((c) => (config = c))
      .catch(() => undefined)
      .finally(() => (loaded = true))
    void loadDocs()
  })

  async function loadDocs() {
    docsLoading = true
    try {
      const r = await eurlexApi.listDocuments()
      docs = r.documents
    } catch {
      docs = []
    } finally {
      docsLoading = false
    }
  }

  // Debounced config auto-save.
  let saveTimer: ReturnType<typeof setTimeout> | undefined
  function saveConfig() {
    if (!loaded) return
    clearTimeout(saveTimer)
    saveTimer = setTimeout(() => {
      void eurlexApi.putConfig($state.snapshot(config)).catch((e) => {
        toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
      })
    }, 400)
  }

  async function search() {
    if (!query.trim()) return
    searching = true
    searchNote = null
    hits = []
    try {
      const r = await eurlexApi.search(query.trim(), config.language)
      hits = r.hits
      searchNote = r.note
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    } finally {
      searching = false
    }
  }

  async function indexHit(hit: CorpusHit) {
    syncingId = hit.identifier
    try {
      await eurlexApi.fetchCelex(hit.identifier, config.language, hit.date ?? undefined)
      toastStore.success(t('Eurlex.alreadyIndexed'))
      await loadDocs()
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    } finally {
      syncingId = null
    }
  }

  async function resync(doc: EurlexDocument) {
    syncingId = doc.id
    try {
      await eurlexApi.resyncDocument(doc.id)
      await loadDocs()
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    } finally {
      syncingId = null
    }
  }

  async function removeDoc(doc: EurlexDocument) {
    try {
      await eurlexApi.deleteDocument(doc.id)
      docs = docs.filter((d) => d.id !== doc.id)
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    }
  }

  function indexedDocDate(doc: EurlexDocument): string | null {
    if (doc.corpus_date) return doc.corpus_date
    return hits.find((h) => h.identifier === doc.corpus_identifier)?.date ?? null
  }
</script>

<div class="space-y-4">
  <Card title={t('Eurlex.title')} subtitle={t('Eurlex.subtitle')}>
    <div class="space-y-3">
      <Toggle
        checked={config.enabled}
        label={t('Eurlex.enabled')}
        description={t('Eurlex.enabledHint')}
        onchange={(v) => {
          config.enabled = v
          saveConfig()
        }}
      />
      <div class="grid grid-cols-2 gap-3">
        <Select
          label={t('Eurlex.language')}
          options={langOptions}
          bind:value={config.language}
          onchange={saveConfig}
        />
      </div>
      <Checkbox
        label={t('Eurlex.fallbackEnglish')}
        checked={config.fallback_en}
        onchange={(e) => {
          config.fallback_en = (e.currentTarget as HTMLInputElement).checked
          saveConfig()
        }}
      />
    </div>
  </Card>

  <Card title={t('Eurlex.smartSearch')}>
    <div class="space-y-3">
      <div class="flex items-end gap-2">
        <Input
          bind:value={query}
          placeholder={t('Eurlex.smartSearchPlaceholder')}
          class="flex-1"
          onkeydown={(e: KeyboardEvent) => {
            if (e.key === 'Enter') search()
          }}
        />
        <Button loading={searching} disabled={!query.trim()} onclick={search}>
          {t('Eurlex.searchButton')}
        </Button>
      </div>
      <p class="text-xs text-(--color-text-secondary)">{t('Eurlex.smartSearchHint')}</p>

      {#if searchNote}
        <p class="text-xs text-(--color-text-secondary)">{searchNote}</p>
      {/if}
      {#if hits.length}
        <ul class="flex flex-col gap-2">
          {#each hits as hit (hit.identifier)}
            <li class="flex items-center gap-3 px-3 py-2 border border-(--color-surface-200) rounded-(--radius-md)">
              <div class="flex-1 min-w-0">
                <p class="text-sm text-(--color-text-primary) truncate">{hit.title}</p>
                <p class="text-xs text-(--color-text-secondary) font-mono">
                  {hit.identifier}{#if hit.date} · {hit.date}{/if}
                </p>
              </div>
              {#if hit.url}
                <IconButton label={t('Corpora.openHit')} size="sm" onclick={() => openExternal(hit.url)}>
                  <ExternalLink size={14} />
                </IconButton>
              {/if}
              <Button
                size="sm"
                variant="secondary"
                loading={syncingId === hit.identifier}
                onclick={() => indexHit(hit)}
              >
                {t('Eurlex.indexAction')}
              </Button>
            </li>
          {/each}
        </ul>
      {:else if !searching && query}
        <p class="text-sm text-(--color-text-secondary)">{t('Eurlex.noResults')}</p>
      {/if}
    </div>
  </Card>

  <Card title={t('Eurlex.indexedTitle')}>
    {#if docsLoading}
      <div class="flex justify-center py-6"><Spinner size="sm" /></div>
    {:else if docs.length === 0}
      <EmptyState title={t('Eurlex.indexedEmpty')} />
    {:else}
      <ul class="flex flex-col gap-2">
        {#each docs as doc (doc.id)}
          <li class="flex items-center gap-3 px-3 py-2 border border-(--color-surface-200) rounded-(--radius-md)">
            <div class="flex-1 min-w-0">
              <p class="text-sm text-(--color-text-primary) truncate">{doc.filename}</p>
              <p class="text-xs text-(--color-text-secondary) font-mono">
                {doc.corpus_identifier ?? ''}
                {#if indexedDocDate(doc)} · {indexedDocDate(doc)}{/if}
                {#if doc.corpus_language} · {doc.corpus_language.toUpperCase()}{/if}
                · {doc.chunks_indexed} {t('Sync.chunks')}
              </p>
            </div>
            <Badge tone={doc.status === 'ready' ? 'success' : 'neutral'} size="xs">
              {doc.status}
            </Badge>
            {#if doc.source_url}
              <IconButton label={t('Corpora.sourceOriginalLink')} size="sm"
                onclick={() => openExternal(doc.source_url as string)}>
                <ExternalLink size={14} />
              </IconButton>
            {/if}
            <IconButton
              label={t('Corpora.reindexHit')}
              size="sm"
              onclick={() => resync(doc)}
            >
              {#if syncingId === doc.id}<Spinner size="sm" />{:else}<RefreshCw size={14} />{/if}
            </IconButton>
            <IconButton label={t('Eurlex.remove')} size="sm" variant="danger" onclick={() => removeDoc(doc)}>
              <Trash2 size={14} />
            </IconButton>
          </li>
        {/each}
      </ul>
    {/if}
  </Card>
</div>
