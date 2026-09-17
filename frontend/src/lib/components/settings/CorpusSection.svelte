<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Generic corpus panel. Drives any registered corpus plugin — Italian
  Legal (dedicated /italian-legal/* routes) or a declarative plugin
  like CNIL (/corpora/{id}/* routes) — from its capability flags:
  bulk import with progress, search → index, and the indexed-document
  list. EUR-Lex keeps its own richer panel.
-->
<script lang="ts">
  import Card from '$lib/components/ui/Card.svelte'
  import Input from '$lib/components/ui/Input.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import IconButton from '$lib/components/ui/IconButton.svelte'
  import Badge from '$lib/components/ui/Badge.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import Progress from '$lib/components/ui/Progress.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import Toggle from '$lib/components/ui/Toggle.svelte'
  import {
    italianLegalApi,
    genericCorpusApi,
    type CorpusItem,
    type CorpusDocument,
    type ImportStatus,
    type ItalianLegalConfig,
  } from '$lib/api/data-sources'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { openExternal } from '$lib/tauri/commands'
  import { Trash2, ExternalLink, Eye } from 'lucide-svelte'
  import Modal from '$lib/components/ui/Modal.svelte'

  let { corpus }: { corpus: CorpusItem } = $props()

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)
  const suzieLawGithub = 'https://github.com/firelex/suzielaw'

  const sourceDisclaimers: Record<string, string> = {
    'at-ris': 'Disclaimer: contenuti da RIS/BKA Austria; verificare sempre la versione vigente ufficiale.',
    'de-openlegaldata': 'Disclaimer: contenuti da OpenLegalData; metadati e testi possono riflettere normalizzazioni del provider.',
    'de-gesetze': 'Disclaimer: contenuti da Gesetze im Internet; per uso professionale confermare su pubblicazione ufficiale.',
    'es-boe': 'Disclaimer: contenuti da BOE; fanno fede gli atti pubblicati nel Bolletín Oficial del Estado.',
    'ie-irishstatutebook': 'Disclaimer: contenuti da Irish Statute Book; usare citazioni complete (year/act|si/number).',
    'it-normattiva': 'Disclaimer: contenuti da Normattiva; verificare consolidato e testo vigente prima dell\'uso.',
    'jp-egov': 'Disclaimer: contenuti da e-Gov Japan; confermare la versione ufficiale in lingua originale.',
    'nl-wetten': 'Disclaimer: contenuti da Wetten.nl; riferimenti e versioni possono cambiare per consolidamenti.',
    'uk-findcaselaw': 'Disclaimer: contenuti da Find Case Law UK; validare citazioni e neutral citation nel registro ufficiale.',
    'uk-legislation': 'Disclaimer: contenuti da legislation.gov.uk; stato di revisione e in-force da verificare per ogni sezione.',
    'us-cfr': 'Disclaimer: contenuti da eCFR; l\'eCFR è aggiornato ma non sostituisce la pubblicazione legale ufficiale.',
    'us-courtlistener': 'Disclaimer: contenuti da CourtListener; verificare sempre il testo ufficiale della corte.',
    'au-federalregister': 'Disclaimer: contenuti da Federal Register of Legislation AU; verificare eventuali versioni consolidate.',
  }

  /** Italian Legal has bespoke routes; everything else is a plugin. */
  const isItalian = $derived(corpus.id === 'italian-legal')
  const corpusDisclaimer = $derived(
    sourceDisclaimers[corpus.id] ??
      `Disclaimer: contenuti da ${corpus.display_name}; verificare sempre la versione ufficiale della fonte.`
  )
  const indexedEmptyTitle = $derived(
    `Nessun documento ${corpus.display_name} ancora indicizzato. Cerca un atto qui sopra per aggiungerlo.`
  )

  // ── Discovery badges + dynamic search hint ───────────────────────
  const disc = $derived(corpus.discovery)

  function authLabel(a: string): string {
    return (
      {
        public: t('Corpora.badge.authPublic'),
        'api-key': t('Corpora.badge.authApiKey'),
        oauth2: t('Corpora.badge.authOauth2'),
        'optional-token': t('Corpora.badge.authOptionalToken'),
      } as Record<string, string>
    )[a] ?? a
  }
  function searchModeLabel(s: string): string {
    return (
      {
        'free-text': t('Corpora.badge.searchFreeText'),
        'citation-only': t('Corpora.badge.searchCitationOnly'),
        'date-window': t('Corpora.badge.searchDateWindow'),
        sparql: t('Corpora.badge.searchSparql'),
      } as Record<string, string>
    )[s] ?? s
  }
  function docTypeLabel(dt: string): string {
    return dt === 'case_law'
      ? t('Corpora.docType.caseLaw')
      : t('Corpora.docType.legislation')
  }

  /** Search-mode-specific guidance shown under the search box. */
  const searchHint = $derived.by(() => {
    const s = corpus.discovery?.search_mode
    if (s === 'citation-only') return t('Corpora.hint.citationOnly')
    if (s === 'date-window') return t('Corpora.hint.dateWindow')
    return null
  })

  interface Hit {
    id: string
    title: string
    sub: string
    date?: string
  }

  let query = $state('')
  let hits = $state<Hit[]>([])
  let searching = $state(false)

  // Per-hit indexing queue: each clicked hit gets its own job and the
  // worker drains them one at a time, so multiple "Index" clicks each
  // show their own state (queued → running → done/error) instead of
  // only the last click showing a progress bar.
  type IndexState = 'queued' | 'running' | 'done' | 'error'
  interface IndexJob {
    state: IndexState
    controller?: AbortController
    error?: string
  }
  let indexJobs = $state<Record<string, IndexJob>>({})
  let indexQueue: string[] = []
  let queueWorking = false

  // Lightweight text-preview modal driven from the search-hit list.
  // Browser-native Ctrl+F works because the body lives inside a real
  // <pre> in the DOM — no JS search bar needed.
  let preview = $state<{
    open: boolean
    loading: boolean
    title: string
    text: string
    sourceUrl: string
    error: string
  }>({
    open: false,
    loading: false,
    title: '',
    text: '',
    sourceUrl: '',
    error: '',
  })

  async function openPreview(identifier: string, fallbackTitle: string) {
    preview.open = true
    preview.loading = true
    preview.title = fallbackTitle
    preview.text = ''
    preview.sourceUrl = ''
    preview.error = ''
    try {
      const r = await genericCorpusApi(corpus.id).preview(identifier)
      preview.title = r.title || fallbackTitle
      preview.text = r.text
      preview.sourceUrl = r.source_url ?? ''
    } catch (e) {
      preview.error = (e as Error).message
    } finally {
      preview.loading = false
    }
  }

  function closePreview() {
    preview.open = false
  }

  let docs = $state<CorpusDocument[]>([])
  let docsLoading = $state(true)

  let importStatus = $state<ImportStatus | null>(null)
  let importing = $state(false)
  let pollTimer: ReturnType<typeof setInterval> | undefined

  // Job-state strings that mean "an import is in flight". italian-legal
  // reports `downloading`/`importing`; the generic DILA importer
  // reports phase names; `running` is our optimistic post-start value.
  const IMPORT_RUNNING = new Set([
    'running', 'downloading', 'importing',
    'discovering', 'extracting', 'inserting',
  ])
  const isImporting = $derived(
    !!importStatus && IMPORT_RUNNING.has(importStatus.job_state)
  )

  /** Human-readable byte size for the indexed-document list. */
  function formatBytes(n: number): string {
    if (!n || n < 0) return '0 B'
    if (n < 1024) return `${n} B`
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
    return `${(n / 1024 / 1024).toFixed(1)} MB`
  }

  async function loadDocs() {
    if (!corpus.runnable) {
      docs = []
      docsLoading = false
      return
    }
    docsLoading = true
    try {
      const r = isItalian
        ? await italianLegalApi.documents()
        : await genericCorpusApi(corpus.id).documents()
      docs = r.documents
    } catch {
      docs = []
    } finally {
      docsLoading = false
    }
  }

  async function loadImportStatus() {
    if (!corpus.runnable) {
      importStatus = null
      return
    }
    try {
      importStatus = isItalian
        ? await italianLegalApi.importStatus()
        : await genericCorpusApi(corpus.id).importStatus()
    } catch {
      importStatus = null
    }
  }

  // ── Enable/disable toggle (per-user, persisted in corpus_settings) ─
  let corpusEnabled = $state(true)
  let italianConfig = $state<ItalianLegalConfig | null>(null)

  async function loadConfig() {
    if (!corpus.runnable) return
    try {
      if (isItalian) {
        italianConfig = await italianLegalApi.getConfig()
        corpusEnabled = italianConfig.enabled
      } else {
        corpusEnabled = (await genericCorpusApi(corpus.id).getConfig()).enabled
      }
    } catch {
      /* keep the optimistic default */
    }
  }

  async function toggleEnabled(next: boolean) {
    const prev = !next
    corpusEnabled = next
    try {
      if (isItalian) {
        const base = italianConfig ?? { enabled: prev, sources: [] }
        italianConfig = await italianLegalApi.putConfig({ ...base, enabled: next })
      } else {
        await genericCorpusApi(corpus.id).setConfig({ enabled: next })
      }
    } catch (e) {
      corpusEnabled = prev // rollback on failure
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    }
  }

  $effect(() => {
    void corpus.id
    void loadDocs()
    void loadConfig()
    if (corpus.capabilities.bulk_import) {
      void loadImportStatus().then(() => {
        // An import may already be running when the panel opens —
        // pick the polling back up so the bar isn't frozen.
        if (isImporting) startPolling()
      })
    }
    return () => clearInterval(pollTimer)
  })

  function startPolling() {
    clearInterval(pollTimer)
    pollTimer = setInterval(async () => {
      await loadImportStatus()
      if (!isImporting) {
        clearInterval(pollTimer)
        importing = false
        void loadDocs()
      }
    }, 2000)
  }

  async function runImport() {
    if (!corpus.runnable) return
    importing = true
    try {
      if (isItalian) await italianLegalApi.startImport()
      else await genericCorpusApi(corpus.id).startImport()
      importStatus = { job_state: 'running' }
      startPolling()
    } catch (e) {
      importing = false
      const msg = (e as Error).message ?? ''
      // "Import already in progress" is an expected state, not a
      // failure — surface it calmly and resume tracking the job.
      if (/in corso|in progress|already/i.test(msg)) {
        toastStore.info(msg)
        await loadImportStatus()
        if (isImporting) startPolling()
      } else {
        toastStore.danger(t('Errors.somethingWrong'), { detail: msg })
      }
    }
  }

  async function search() {
    if (!corpus.runnable) return
    if (!query.trim()) return
    searching = true
    hits = []
    try {
      if (isItalian) {
        const r = await italianLegalApi.search(query.trim())
        hits = r.hits.map((h) => ({
          id: h.hf_id,
          title: h.title ?? h.hf_id,
          date: h.date ?? undefined,
          sub: [h.authority, h.number, h.year].filter(Boolean).join(' · '),
        }))
      } else {
        const r = await genericCorpusApi(corpus.id).search(query.trim())
        hits = r.hits.map((h) => ({
          id: h.identifier,
          title: h.title,
          date: h.date ?? undefined,
          sub: [h.identifier, h.date].filter(Boolean).join(' · '),
        }))
      }
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    } finally {
      searching = false
    }
  }

  function findIndexedDoc(hit: Hit): CorpusDocument | undefined {
    return docs.find((d) => d.corpus_identifier === hit.id)
  }

  function indexBtnLabel(hit: Hit): string {
    const st = indexJobs[hit.id]?.state
    if (st === 'queued') return t('Corpora.queued')
    if (st === 'running') return t('Common.cancel')
    if (st === 'error') return t('Corpora.retry')
    return findIndexedDoc(hit) || st === 'done'
      ? t('Corpora.reindexHit')
      : t('Corpora.indexHit')
  }

  function indexBtnVariant(hit: Hit): 'secondary' | 'danger' {
    const st = indexJobs[hit.id]?.state
    return st === 'queued' || st === 'running' ? 'danger' : 'secondary'
  }

  function indexedDocDate(doc: CorpusDocument): string | null {
    if (doc.corpus_date) return doc.corpus_date
    const fromHit = hits.find((h) => h.id === doc.corpus_identifier)?.date
    if (fromHit) return fromHit
    return null
  }

  /** Enqueue a hit for indexing — or cancel it if still queued/running. */
  function indexHit(hit: Hit) {
    if (!corpus.runnable) return
    const job = indexJobs[hit.id]
    if (job && (job.state === 'queued' || job.state === 'running')) {
      job.controller?.abort()
      indexQueue = indexQueue.filter((id) => id !== hit.id)
      delete indexJobs[hit.id]
      indexJobs = { ...indexJobs }
      return
    }
    indexJobs[hit.id] = { state: 'queued' }
    indexQueue.push(hit.id)
    void runIndexQueue()
  }

  /** Sequential worker — drains the queue one hit at a time so every
   *  clicked hit shows its own queued → running → done/error state. */
  async function runIndexQueue() {
    if (queueWorking) return
    queueWorking = true
    try {
      while (indexQueue.length > 0) {
        const id = indexQueue.shift()!
        const hit = hits.find((h) => h.id === id)
        const job = indexJobs[id]
        if (!hit || !job) continue // cancelled before it started
        const controller = new AbortController()
        job.controller = controller
        job.state = 'running'
        try {
          const indexedDoc = findIndexedDoc(hit)
          if (indexedDoc && corpus.capabilities.documents_resync) {
            if (isItalian)
              await italianLegalApi.resyncDocument(indexedDoc.id, { signal: controller.signal })
            else
              await genericCorpusApi(corpus.id).resyncDocument(indexedDoc.id, { signal: controller.signal })
          } else {
            if (isItalian)
              await italianLegalApi.fetchRow(hit.id, { signal: controller.signal })
            else
              await genericCorpusApi(corpus.id).fetch(hit.id, { signal: controller.signal, date: hit.date })
          }
          if (indexJobs[id]) indexJobs[id].state = 'done'
          await loadDocs()
        } catch (e) {
          if ((e as Error).name === 'AbortError') {
            delete indexJobs[id]
            indexJobs = { ...indexJobs }
          } else if (indexJobs[id]) {
            indexJobs[id].state = 'error'
            indexJobs[id].error = (e as Error).message
          }
        }
      }
    } finally {
      queueWorking = false
    }
  }

  async function removeDoc(doc: CorpusDocument) {
    if (!corpus.runnable) return
    try {
      if (isItalian) await italianLegalApi.deleteDocument(doc.id)
      else await genericCorpusApi(corpus.id).deleteDocument(doc.id)
      docs = docs.filter((d) => d.id !== doc.id)
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    }
  }
</script>

<div class="space-y-4">
  <Card title={corpus.display_name} subtitle={corpus.description}>
    <div class="space-y-1.5">
      <Toggle
        checked={corpusEnabled}
        label={t('Corpora.sourceEnabled')}
        size="sm"
        onchange={toggleEnabled}
      />
      {#if corpus.homepage}
        <button
          type="button"
          class="flex items-center gap-1.5 text-xs text-(--color-brand-600) hover:underline"
          onclick={() => openExternal(corpus.homepage)}
        >
          <ExternalLink size={12} />{corpus.homepage}
        </button>
      {/if}
      {#if disc}
        <div class="flex flex-wrap gap-1.5 pt-0.5">
          {#each disc.doc_types as dt (dt)}
            <Badge tone="brand" size="xs">{docTypeLabel(dt)}</Badge>
          {/each}
          {#if disc.auth}
            <Badge tone={disc.auth === 'public' ? 'success' : 'warning'} size="xs">
              {authLabel(disc.auth)}
            </Badge>
          {/if}
          {#if disc.search_mode}
            <Badge tone="info" size="xs">{searchModeLabel(disc.search_mode)}</Badge>
          {/if}
          {#if disc.fetch_format}
            <Badge tone="neutral" size="xs">{disc.fetch_format.toUpperCase()}</Badge>
          {/if}
        </div>
      {/if}
      <p class="text-xs text-(--color-text-secondary)">{corpusDisclaimer}</p>
      <button
        type="button"
        class="flex items-center gap-1.5 text-xs text-(--color-brand-600) hover:underline"
        onclick={() => openExternal(suzieLawGithub)}
      >
        <ExternalLink size={12} />Versione originale della fonte: thanks to SuzieLaw ({suzieLawGithub})
      </button>
    </div>
  </Card>

  {#if !corpus.runnable}
    <Card title="Fonte pianificata">
      <p class="text-sm text-(--color-text-secondary)">
        Questa fonte è visibile in catalogo ma non è ancora attiva nel runtime locale.
      </p>

      {#if corpus.sources?.length}
        <div class="mt-3 space-y-2">
          {#each corpus.sources as source (source.id)}
            <div class="rounded-(--radius-md) border border-(--color-surface-200) px-3 py-2">
              <div class="flex items-center justify-between gap-2">
                <p class="text-sm text-(--color-text-primary)">{source.display_name}</p>
                <Badge tone={source.available ? 'success' : 'neutral'} size="xs">
                  {source.available ? 'available' : (source.status_label ?? 'planned')}
                </Badge>
              </div>
              {#if source.subtitle}
                <p class="text-xs text-(--color-text-secondary)">{source.subtitle}</p>
              {/if}
              {#if source.description}
                <p class="mt-1 text-xs text-(--color-text-secondary)">{source.description}</p>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </Card>
  {/if}

  {#if corpus.runnable && corpus.capabilities.bulk_import}
    <Card title={t('Corpora.bulk.snapshotImport')}>
      <div class="space-y-2">
        {#if isImporting}
          <Progress value={importStatus?.percent != null ? importStatus.percent / 100 : null} />
          <p class="text-xs text-(--color-text-secondary)">
            {t('Corpora.bulk.importing')}
            {#if importStatus?.current_shard != null && importStatus?.total_shards}
              · {importStatus?.current_shard}/{importStatus?.total_shards}
            {/if}
            {#if importStatus?.rows_imported != null}· {importStatus?.rows_imported}{/if}
          </p>
        {:else}
          <div class="flex items-center justify-between gap-3">
            <p class="text-xs text-(--color-text-secondary)">
              {#if importStatus?.row_count}
                {importStatus.row_count} · {importStatus.last_import_at ?? ''}
              {:else}
                {t('Corpora.bulk.snapshotFresh')}
              {/if}
            </p>
            <Button size="sm" loading={importing} onclick={runImport}>
              {importStatus?.row_count
                ? t('Corpora.bulk.updateAction')
                : t('Corpora.bulk.importAction')}
            </Button>
          </div>
          {#if importStatus?.job_error}
            <p class="text-xs text-(--color-danger-500)">{importStatus.job_error}</p>
          {/if}
        {/if}
      </div>
    </Card>
  {/if}

  {#if corpus.runnable && corpus.capabilities.search}
    <Card title={t('Corpora.searchButton')}>
      <div class="space-y-3">
        <div class="flex items-end gap-2">
          <Input
            bind:value={query}
            placeholder={t('Corpora.exampleHint', { example: corpus.identifier_example })}
            class="flex-1"
            onkeydown={(e: KeyboardEvent) => {
              if (e.key === 'Enter') search()
            }}
          />
          <Button loading={searching} disabled={!query.trim()} onclick={search}>
            {t('Corpora.searchButton')}
          </Button>
        </div>
        {#if searchHint}
          <p class="text-xs text-(--color-text-secondary)">{searchHint}</p>
        {/if}
        {#if hits.length}
          <ul class="flex flex-col gap-2">
            {#each hits as hit (hit.id)}
              <li class="flex items-center gap-3 px-3 py-2 border border-(--color-surface-200) rounded-(--radius-md)">
                <div class="flex-1 min-w-0">
                  <p class="text-sm text-(--color-text-primary) truncate" title={hit.title}>{hit.title}</p>
                  <p class="text-xs text-(--color-text-secondary) font-mono truncate" title={hit.sub}>{hit.sub}</p>
                  {#if indexJobs[hit.id]?.state === 'running'}
                    <div class="mt-2">
                      <Progress value={null} size="sm" />
                    </div>
                  {:else if indexJobs[hit.id]?.state === 'queued'}
                    <p class="mt-1 text-xs text-(--color-text-secondary)">{t('Corpora.queued')}</p>
                  {:else if indexJobs[hit.id]?.state === 'error'}
                    <p class="mt-1 text-xs text-(--color-danger-500)">{indexJobs[hit.id].error}</p>
                  {/if}
                </div>
                <IconButton
                  size="sm"
                  variant="ghost"
                  label={t('Corpora.viewText')}
                  onclick={() => openPreview(hit.id, hit.title)}
                >
                  <Eye size={14} />
                </IconButton>
                <Button
                  size="sm"
                  variant={indexBtnVariant(hit)}
                  onclick={() => indexHit(hit)}
                >
                  {indexBtnLabel(hit)}
                </Button>
              </li>
            {/each}
          </ul>
        {:else if !searching && query}
          <p class="text-sm text-(--color-text-secondary)">
            {t('Corpora.noResultsFor', { query })}
          </p>
        {/if}
      </div>
    </Card>
  {/if}

  {#if corpus.runnable}
    <Card title={t('Corpora.indexedHeader', { count: docs.length })}>
      {#if docsLoading}
        <div class="flex justify-center py-6"><Spinner size="sm" /></div>
      {:else if docs.length === 0}
        <EmptyState title={indexedEmptyTitle} />
      {:else}
        <ul class="flex flex-col gap-2">
          {#each docs as doc (doc.id)}
            <li class="flex items-center gap-3 px-3 py-2 border border-(--color-surface-200) rounded-(--radius-md)">
              <div class="flex-1 min-w-0">
                <p class="text-sm text-(--color-text-primary) truncate">{doc.filename}</p>
                <p class="text-xs text-(--color-text-secondary) font-mono">
                  {#if doc.corpus_identifier}{doc.corpus_identifier} · {/if}
                  {#if indexedDocDate(doc)}{indexedDocDate(doc)} · {/if}
                  {formatBytes(doc.size_bytes)}
                </p>
              </div>
              <Badge tone={doc.status === 'ready' ? 'success' : 'neutral'} size="xs">{doc.status}</Badge>
              {#if doc.corpus_identifier}
                <IconButton
                  size="sm"
                  variant="ghost"
                  label={t('Corpora.viewText')}
                  onclick={() => openPreview(doc.corpus_identifier!, doc.filename)}
                >
                  <Eye size={14} />
                </IconButton>
              {/if}
              <IconButton label={t('Corpora.removeDoc')} size="sm" variant="danger"
                onclick={() => removeDoc(doc)}>
                <Trash2 size={14} />
              </IconButton>
            </li>
          {/each}
        </ul>
      {/if}
    </Card>
  {/if}
</div>

<!--
  Plain-text preview of a corpus source. Layout is deliberately
  minimal: a header carrying the title + an external-link button to the
  upstream page, and a vertically-scrolling <pre> with the full body.
  No custom search bar — the user is told to use the browser-native
  Ctrl+F, which works because the text lives in a real DOM element.
-->
<Modal
  open={preview.open}
  title={preview.title}
  size="xl"
  onclose={closePreview}
>
  {#if preview.loading}
    <div class="flex items-center justify-center gap-2 py-12 text-sm text-(--color-text-secondary)">
      <Spinner size="sm" />
      {t('Corpora.previewLoading')}
    </div>
  {:else if preview.error}
    <p class="py-12 text-center text-sm text-(--color-danger-500)">
      {preview.error}
    </p>
  {:else}
    <!--
      Modal's body wrapper is already `overflow-y-auto`, so we just
      drop the prose in a `whitespace-pre-wrap` <pre> and let the
      browser's native Ctrl+F handle search. No custom search bar.
    -->
    <div class="flex items-center justify-between gap-2 mb-3 text-xs text-(--color-text-secondary)">
      <span>{t('Corpora.previewCtrlFHint')}</span>
      {#if preview.sourceUrl}
        <button
          type="button"
          class="inline-flex items-center gap-1 hover:text-(--color-text-primary) hover:underline"
          onclick={() => openExternal(preview.sourceUrl)}
        >
          <ExternalLink size={12} />
          {t('Corpora.openOnSource')}
        </button>
      {/if}
    </div>
    <pre
      class="whitespace-pre-wrap break-words font-mono text-xs leading-relaxed
             text-(--color-text-primary)"
    >{preview.text}</pre>
  {/if}
</Modal>
