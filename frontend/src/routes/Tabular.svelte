<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Tabular reviews screen. List + create + delete over /tabular-review.
  A review inherits its column definitions and domain from a chosen
  tabular workflow. The per-document cell grid is a later phase — the
  backend endpoint here carries only review metadata, not cells.
-->
<script lang="ts">
  import Badge from '$lib/components/ui/Badge.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import IconButton from '$lib/components/ui/IconButton.svelte'
  import Select from '$lib/components/ui/Select.svelte'
  import Input from '$lib/components/ui/Input.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import Modal from '$lib/components/ui/Modal.svelte'
  import ConfirmDialog from '$lib/components/ui/ConfirmDialog.svelte'
  import TabularDetail from '$lib/components/tabular/TabularDetail.svelte'
  import { tabularStore } from '$lib/stores/tabular.svelte'
  import { tabularApi } from '$lib/api/tabular'
  import { workflowsApi } from '$lib/api/workflows'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { userStore } from '$lib/stores/user.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { DOMAINS, domainLabel, DEFAULT_DOMAIN, type Domain } from '$lib/types/domain'
  import type { Workflow } from '$lib/types/workflow'
  import { ApiError } from '$lib/types/error'
  import { Trash2, Pencil } from 'lucide-svelte'

  let domainFilter = $state<string>('')
  let tabularWorkflows = $state<Workflow[]>([])
  let detailId = $state<string | null>(null)

  $effect(() => {
    void tabularStore.refresh()
    // If another screen (e.g. ProjectDetail) asked us to open a
    // specific review on mount, pick it up here.
    const pending = tabularStore.consumePendingDetailId()
    if (pending) detailId = pending
  })
  $effect(() => {
    workflowsApi
      .list({ type: 'tabular' })
      .then((r) => (tabularWorkflows = r.workflows))
      .catch(() => {
        /* the create modal just shows an empty workflow list */
      })
  })

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)

  const domainOptions = $derived([
    { value: '', label: t('Domains.filterPlaceholder') },
    ...DOMAINS.map((d) => ({ value: d, label: domainLabel(d) })),
  ])

  const rows = $derived(
    domainFilter
      ? tabularStore.items.filter((r) => r.domain === domainFilter)
      : tabularStore.items
  )

  // ── create modal ────────────────────────────────────────────────────
  let modalOpen = $state(false)
  let fTitle = $state('')
  let fWorkflowId = $state('')
  let fDomain = $state<Domain>(DEFAULT_DOMAIN)
  let creating = $state(false)
  let formError = $state<string | null>(null)

  const domainFormOptions = $derived(DOMAINS.map((d) => ({ value: d, label: domainLabel(d) })))
  // Domain is chosen first; it scopes the tabular-workflow list.
  const workflowOptions = $derived([
    { value: '', label: t('TabularReviews.selectWorkflowOption') },
    ...tabularWorkflows
      .filter((w) => w.domain === fDomain)
      .map((w) => ({ value: w.id, label: w.title })),
  ])
  const selectedWorkflow = $derived(tabularWorkflows.find((w) => w.id === fWorkflowId))

  // Changing the domain re-scopes the workflow list — drop a selection
  // that no longer belongs to the chosen domain.
  $effect(() => {
    if (selectedWorkflow && selectedWorkflow.domain !== fDomain) fWorkflowId = ''
  })

  function openCreate() {
    fTitle = ''
    fWorkflowId = ''
    fDomain = userStore.defaultDomain
    formError = null
    modalOpen = true
  }

  async function create() {
    if (!fWorkflowId) {
      formError = t('TabularReviews.pickWorkflowError')
      return
    }
    creating = true
    formError = null
    try {
      await tabularStore.create({
        title: fTitle.trim() || undefined,
        workflow_id: fWorkflowId,
        columns_config: selectedWorkflow?.columns_config,
        domain: fDomain,
      })
      toastStore.success(t('TabularReviews.createdToast'))
      modalOpen = false
    } catch (e) {
      formError = e instanceof ApiError ? e.detail : (e as Error).message
    } finally {
      creating = false
    }
  }

  // ── rename ──────────────────────────────────────────────────────────
  // Mirror of the Projects-list pencil affordance — opens a small
  // inline modal with the current title pre-filled and PATCHes
  // /tabular-review/:id with the new title on confirm.
  let renameTarget = $state<{ id: string; title: string } | null>(null)
  let renameTitle = $state('')
  let renaming = $state(false)

  function openRename(r: { id: string; title: string }) {
    renameTarget = { id: r.id, title: r.title }
    renameTitle = r.title
  }

  async function runRename() {
    if (!renameTarget) return
    const next = renameTitle.trim()
    if (!next || next === renameTarget.title) {
      renameTarget = null
      return
    }
    renaming = true
    try {
      await tabularApi.patch(renameTarget.id, { title: next })
      await tabularStore.refresh()
      toastStore.success(t('TabularReviews.renamedToast'))
      renameTarget = null
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    } finally {
      renaming = false
    }
  }

  // ── delete ──────────────────────────────────────────────────────────
  let deleteTarget = $state<{ id: string; title: string } | null>(null)

  async function confirmDelete() {
    if (!deleteTarget) return
    try {
      await tabularStore.remove(deleteTarget.id)
      toastStore.info(t('TabularReviews.deletedToast'))
    } catch (e) {
      toastStore.danger(t('TabularReviews.deleteError'), { detail: (e as Error).message })
    } finally {
      deleteTarget = null
    }
  }

  // ── import from spreadsheet (one review per worksheet) ──────────────
  let fileInput = $state<HTMLInputElement>()
  let importing = $state(false)

  async function onFilePicked(e: Event) {
    const input = e.currentTarget as HTMLInputElement
    const file = input.files?.[0]
    input.value = '' // let the user re-pick the same file later
    if (!file) return
    importing = true
    try {
      const r = await tabularApi.importXlsx(file)
      toastStore.success(t('TabularReviews.importedToast', { n: r.reviews.length }))
      await tabularStore.refresh()
    } catch (err) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (err as Error).message })
    } finally {
      importing = false
    }
  }

  function fmtDate(iso: string): string {
    const d = new Date(iso)
    return Number.isNaN(d.getTime()) ? iso : d.toLocaleDateString()
  }
</script>

{#if detailId}
  <TabularDetail id={detailId} onback={() => (detailId = null)} />
{:else}
<div class="max-w-4xl mx-auto p-8 space-y-5">
  <header class="flex items-end justify-between gap-4">
    <div class="space-y-1">
      <h2 class="text-2xl font-semibold text-(--color-text-primary)">{t('TabularReviews.title')}</h2>
      <p class="text-sm text-(--color-text-secondary)">
        {t('TabularReviews.subtitle')}
      </p>
    </div>
    <div class="flex items-center gap-2">
      <input
        type="file"
        accept=".xlsx,.xls,.xlsb,.ods"
        class="hidden"
        bind:this={fileInput}
        onchange={onFilePicked}
      />
      <Button variant="secondary" loading={importing} onclick={() => fileInput?.click()}>
        {t('TabularReviews.importExcel')}
      </Button>
      <Button onclick={openCreate}>{t('TabularReviews.newReview')}</Button>
    </div>
  </header>

  <div class="flex justify-end">
    <Select options={domainOptions} bind:value={domainFilter} size="sm" class="w-44" />
  </div>

  {#if tabularStore.loading}
    <div class="flex items-center gap-2 text-sm text-(--color-text-secondary) py-12 justify-center">
      <Spinner size="sm" />
      {t('Common.loading')}
    </div>
  {:else if tabularStore.error}
    <EmptyState title={t('Errors.somethingWrong')} description={tabularStore.error}>
      {#snippet action()}
        <Button size="sm" variant="secondary" onclick={() => tabularStore.refresh()}>{t('Common.retry')}</Button>
      {/snippet}
    </EmptyState>
  {:else if rows.length === 0}
    <EmptyState
      title={t('TabularReviews.noReviews')}
      description={t('TabularReviews.emptyHint')}
    >
      {#snippet action()}
        <Button size="sm" onclick={openCreate}>{t('TabularReviews.newReview')}</Button>
      {/snippet}
    </EmptyState>
  {:else}
    <ul class="flex flex-col gap-2">
      {#each rows as r (r.id)}
        <li class="flex items-center gap-3 px-4 py-3 bg-(--color-surface-0) border border-(--color-surface-200) rounded-(--radius-md) hover:border-(--color-surface-300)">
          <button
            type="button"
            class="flex-1 min-w-0 text-left"
            onclick={() => (detailId = r.id)}
          >
            <span class="text-sm font-medium text-(--color-text-primary) truncate">{r.title}</span>
            <p class="text-xs text-(--color-text-secondary)">
              {t('Ui.columnCountFull', { n: r.columns_config.length })}
              · {t('Ui.createdOn', { date: fmtDate(r.created_at) })}
            </p>
          </button>
          <Badge tone="brand">{domainLabel(r.domain)}</Badge>
          <IconButton
            label={t('TabularReviews.renameReview')}
            size="sm"
            onclick={() => openRename({ id: r.id, title: r.title })}
          >
            <Pencil size={14} />
          </IconButton>
          <IconButton
            label={t('Ui.deleteReview')}
            size="sm"
            variant="danger"
            onclick={() => (deleteTarget = { id: r.id, title: r.title })}
          >
            <Trash2 size={14} />
          </IconButton>
        </li>
      {/each}
    </ul>
  {/if}
</div>
{/if}

<Modal bind:open={modalOpen} title={t('TabularReviews.newReview')} size="md">
  <div class="space-y-3">
    <Input label={t('Common.name')} bind:value={fTitle} placeholder={t('Common.untitled')} />
    <Select label={t('Domains.label')} options={domainFormOptions} bind:value={fDomain} />
    <Select label={t('TabularReviews.workflowTemplate')} options={workflowOptions} bind:value={fWorkflowId} />
    {#if selectedWorkflow}
      <p class="text-xs text-(--color-text-secondary)">
        {t('TabularReviews.inheritsColumns', { n: selectedWorkflow.columns_config.length })}
      </p>
    {:else}
      <p class="text-xs text-(--color-text-secondary)">
        {t('TabularReviews.scopedToDomain', { domain: domainLabel(fDomain) })}
      </p>
    {/if}
    {#if formError}
      <p class="text-sm text-(--color-danger-500)">{formError}</p>
    {/if}
  </div>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (modalOpen = false)}>{t('Common.cancel')}</Button>
    <Button loading={creating} onclick={create}>{t('TabularReviews.createReview')}</Button>
  {/snippet}
</Modal>

<!-- rename modal — small inline input, mirror of Projects pencil flow -->
<Modal
  open={renameTarget !== null}
  title={t('TabularReviews.renameReview')}
  size="sm"
  onclose={() => (renameTarget = null)}
>
  <Input
    label={t('Common.name')}
    bind:value={renameTitle}
    placeholder={t('Common.untitled')}
    onkeydown={(e: KeyboardEvent) => {
      if (e.key === 'Enter') void runRename()
      if (e.key === 'Escape') (renameTarget = null)
    }}
  />
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (renameTarget = null)}>{t('Common.cancel')}</Button>
    <Button
      loading={renaming}
      disabled={!renameTitle.trim() || renameTitle.trim() === renameTarget?.title}
      onclick={runRename}
    >
      {t('Common.save')}
    </Button>
  {/snippet}
</Modal>

<ConfirmDialog
  open={deleteTarget !== null}
  title={t('TabularReviews.deleteConfirmTitle')}
  message={t('TabularReviews.deleteConfirmBody', { title: deleteTarget?.title ?? '' })}
  confirmLabel={t('Common.delete')}
  danger
  onconfirm={confirmDelete}
  oncancel={() => (deleteTarget = null)}
/>
