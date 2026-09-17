<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Workflows screen — first real feature route. Lists DB workflows merged
  with shipped presets (GET /workflow), with tab filtering (all / built-in
  / custom / hidden), a domain filter, and hide/unhide on built-ins.
  Create + edit modals land in a later phase.
-->
<script lang="ts">
  import { PRODUCT_NAME } from '$lib/product'
  import Tabs from '$lib/components/ui/Tabs.svelte'
  import Badge from '$lib/components/ui/Badge.svelte'
  import Select from '$lib/components/ui/Select.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import IconButton from '$lib/components/ui/IconButton.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import WorkflowModal from '$lib/components/workflow/WorkflowModal.svelte'
  import WorkflowEditor from '$lib/components/workflow/WorkflowEditor.svelte'
  import Logo from '$lib/components/ui/Logo.svelte'
  import { workflowStore } from '$lib/stores/workflows.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { DOMAINS, domainLabel } from '$lib/types/domain'
  import type { Workflow } from '$lib/types/workflow'
  import { Eye, EyeOff, Pencil } from 'lucide-svelte'

  type TabId = 'all' | 'builtin' | 'custom' | 'hidden'
  let activeTab = $state<TabId>('all')
  let domainFilter = $state<string>('')
  let modalOpen = $state(false)
  let editId = $state<string | null>(null)

  $effect(() => {
    void workflowStore.refresh()
  })

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)

  // Tab partitioning is purely client-side over the fetched set.
  const builtin = $derived(workflowStore.items.filter((w) => w.is_system))
  const custom = $derived(workflowStore.items.filter((w) => !w.is_system))
  const hiddenItems = $derived(
    workflowStore.items.filter((w) => workflowStore.isHidden(w.id))
  )

  const tabs = $derived([
    { id: 'all', label: t('Common.all'), count: workflowStore.visible.length },
    { id: 'builtin', label: t('Workflows.tabBuiltin'), count: builtin.filter((w) => !workflowStore.isHidden(w.id)).length },
    { id: 'custom', label: t('Workflows.tabCustom'), count: custom.length },
    { id: 'hidden', label: t('Workflows.tabHidden'), count: hiddenItems.length },
  ])

  const domainOptions = $derived([
    { value: '', label: t('Domains.filterPlaceholder') },
    ...DOMAINS.map((d) => ({ value: d, label: domainLabel(d) })),
  ])

  const rows = $derived.by<Workflow[]>(() => {
    let list: Workflow[]
    switch (activeTab) {
      case 'builtin':
        list = builtin.filter((w) => !workflowStore.isHidden(w.id))
        break
      case 'custom':
        list = custom
        break
      case 'hidden':
        list = hiddenItems
        break
      default:
        list = workflowStore.visible
    }
    if (domainFilter) list = list.filter((w) => w.domain === domainFilter)
    return list
  })

  async function toggleHidden(w: Workflow) {
    try {
      if (workflowStore.isHidden(w.id)) {
        await workflowStore.unhide(w.id)
        toastStore.info(t('Workflows.restoredToast', { title: w.title }))
      } else {
        await workflowStore.hide(w.id)
        toastStore.info(t('Workflows.hiddenToast', { title: w.title }))
      }
    } catch (e) {
      toastStore.danger(t('Workflows.updateError'), { detail: (e as Error).message })
    }
  }
</script>

{#if editId}
  <WorkflowEditor
    id={editId}
    onback={() => { editId = null; void workflowStore.refresh() }}
    ondeleted={() => { editId = null; void workflowStore.refresh() }}
    onopen={(wid) => { editId = wid; void workflowStore.refresh() }}
  />
{:else}
<div class="max-w-4xl mx-auto p-8 space-y-5">
  <header class="flex items-end justify-between gap-4">
    <div class="space-y-1">
      <h2 class="text-2xl font-semibold text-(--color-text-primary)">{t('Workflows.title')}</h2>
      <p class="text-sm text-(--color-text-secondary)">
        {t('Workflows.allHint')}
      </p>
    </div>
    <Button variant="primary" onclick={() => (modalOpen = true)}>{t('Workflows.newWorkflow')}</Button>
  </header>

  <div class="flex items-end justify-between gap-4">
    <Tabs tabs={tabs} bind:active={activeTab} />
    <Select
      options={domainOptions}
      bind:value={domainFilter}
      size="sm"
      class="w-44"
    />
  </div>

  {#if workflowStore.loading}
    <div class="flex items-center gap-2 text-sm text-(--color-text-secondary) py-12 justify-center">
      <Spinner size="sm" />
      {t('Common.loading')}
    </div>
  {:else if workflowStore.error}
    <EmptyState
      title={t('Errors.somethingWrong')}
      description={workflowStore.error}
    >
      {#snippet action()}
        <Button size="sm" variant="secondary" onclick={() => workflowStore.refresh()}>
          {t('Common.retry')}
        </Button>
      {/snippet}
    </EmptyState>
  {:else if rows.length === 0}
    <EmptyState title={t('Workflows.noWorkflows')} />
  {:else}
    <div class="border border-(--color-surface-200) rounded-(--radius-md) overflow-x-auto">
      <table class="w-full text-sm">
        <thead>
          <tr class="bg-(--color-surface-50) text-xs text-(--color-text-secondary)">
            <th class="text-left font-medium px-4 py-2.5">{t('Common.name')}</th>
            <th class="text-left font-medium px-3 py-2.5 w-32">{t('Workflows.type')}</th>
            <th class="text-left font-medium px-3 py-2.5">{t('Workflows.practice')}</th>
            <th class="text-left font-medium px-3 py-2.5">{t('Domains.label')}</th>
            <th class="text-left font-medium px-3 py-2.5 w-32">{t('Workflows.source')}</th>
            <th class="px-3 py-2.5 w-10"></th>
          </tr>
        </thead>
        <tbody>
          {#each rows as w (w.id)}
            <tr class="border-t border-(--color-surface-200) hover:bg-(--color-hover-bg)">
              <td class="px-4 py-2.5">
                <button
                  type="button"
                  class="text-left text-(--color-text-primary) font-medium hover:underline"
                  onclick={() => (editId = w.id)}
                >
                  {w.title}
                </button>
                {#if w.type === 'tabular'}
                  <span class="text-xs text-(--color-text-disabled) ml-2">
                    {t('Ui.columnCount', { n: w.columns_config.length })}
                  </span>
                {/if}
              </td>
              <td class="px-3 py-2.5">
                <Badge tone={w.type === 'assistant' ? 'assistant' : 'tabular'} size="xs">
                  {w.type === 'assistant' ? t('Workflows.typeAssistant') : t('Workflows.typeTabular')}
                </Badge>
              </td>
              <td class="px-3 py-2.5 text-(--color-text-secondary)">{w.practice ?? '—'}</td>
              <td class="px-3 py-2.5">
                <Badge tone="brand" size="xs">{domainLabel(w.domain)}</Badge>
              </td>
              <td class="px-3 py-2.5 text-(--color-text-secondary)">
                {#if w.is_system}
                  <span class="inline-flex items-center gap-1.5">
                    <Logo size={13} activity="idle" />{PRODUCT_NAME}
                  </span>
                {:else}
                  {t('Workflows.originSelf')}
                {/if}
              </td>
              <td class="px-3 py-2.5">
                <div class="flex items-center justify-end gap-1">
                  <IconButton label={t('Common.edit')} size="sm" onclick={() => (editId = w.id)}>
                    <Pencil size={15} />
                  </IconButton>
                  {#if w.is_system}
                    <IconButton
                      label={workflowStore.isHidden(w.id) ? t('Workflows.unhide') : t('Ui.hide')}
                      size="sm"
                      onclick={() => toggleHidden(w)}
                    >
                      {#if workflowStore.isHidden(w.id)}
                        <EyeOff size={15} />
                      {:else}
                        <Eye size={15} />
                      {/if}
                    </IconButton>
                  {/if}
                </div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>
{/if}

<WorkflowModal
  bind:open={modalOpen}
  onsuccess={(createdId) => {
    void workflowStore.refresh()
    if (createdId) editId = createdId
  }}
/>
