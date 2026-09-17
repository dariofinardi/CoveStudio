<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  DOCX Templates screen. Lists the template registry (GET /docx-templates),
  merging shipped system templates with writable user templates. Filter by
  domain, locale and free-text search. A row opens the detail modal; the
  edit action opens the full-page editor (system templates read-only,
  duplicable into editable user templates).
-->
<script lang="ts">
  import Badge from '$lib/components/ui/Badge.svelte'
  import Select from '$lib/components/ui/Select.svelte'
  import Input from '$lib/components/ui/Input.svelte'
  import Tabs from '$lib/components/ui/Tabs.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import IconButton from '$lib/components/ui/IconButton.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import TemplateDetailModal from '$lib/components/docx/TemplateDetailModal.svelte'
  import TemplateEditor from '$lib/components/docx/TemplateEditor.svelte'
  import { templateStore } from '$lib/stores/templates.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { DOMAINS, domainLabel } from '$lib/types/domain'
  import { templateDisplayName, type DocxTemplate } from '$lib/types/template'
  import { Search, Pencil, Eye, EyeOff } from 'lucide-svelte'

  type TabId = 'all' | 'system' | 'custom' | 'hidden'
  let activeTab = $state<TabId>('all')
  let domainFilter = $state<string>('')
  let localeFilter = $state<string>('')
  let search = $state<string>('')
  let detailId = $state<string | null>(null)

  // Full-page editor: open with a template to edit, or null to create.
  let editorOpen = $state(false)
  let editorInitial = $state<DocxTemplate | null>(null)

  function openEditor(initial: DocxTemplate | null) {
    editorInitial = initial
    editorOpen = true
  }
  function closeEditor(refresh: boolean) {
    editorOpen = false
    editorInitial = null
    if (refresh) void templateStore.refresh()
  }

  $effect(() => {
    void templateStore.refresh()
  })

  const domainOptions = $derived([
    { value: '', label: i18n.t('Domains.filterPlaceholder') },
    ...DOMAINS.map((d) => ({ value: d, label: domainLabel(d) })),
  ])
  const localeOptions = $derived([
    { value: '', label: i18n.t('Ui.allLocales') },
    ...templateStore.locales.map((l) => ({ value: l, label: l })),
  ])

  function matchesDomain(tpl: DocxTemplate): boolean {
    if (!domainFilter) return true
    return tpl.domain === domainFilter || tpl.also_applicable_to.includes(domainFilter as never)
  }

  // Tab partitioning over the fetched set (client-side).
  const systemItems = $derived(templateStore.items.filter((tpl) => tpl.is_system))
  const customItems = $derived(templateStore.items.filter((tpl) => !tpl.is_system))
  const hiddenItems = $derived(
    templateStore.items.filter((tpl) => templateStore.isHidden(tpl.id)),
  )

  const tabs = $derived([
    { id: 'all', label: i18n.t('Common.all'), count: templateStore.visible.length },
    {
      id: 'system',
      label: i18n.t('Workflows.tabBuiltin'),
      count: systemItems.filter((tpl) => !templateStore.isHidden(tpl.id)).length,
    },
    { id: 'custom', label: i18n.t('Workflows.tabCustom'), count: customItems.length },
    { id: 'hidden', label: i18n.t('Workflows.tabHidden'), count: hiddenItems.length },
  ])

  const rows = $derived.by<DocxTemplate[]>(() => {
    let list: DocxTemplate[]
    switch (activeTab) {
      case 'system':
        list = systemItems.filter((tpl) => !templateStore.isHidden(tpl.id))
        break
      case 'custom':
        list = customItems
        break
      case 'hidden':
        list = hiddenItems
        break
      default:
        list = templateStore.visible
    }
    const q = search.trim().toLowerCase()
    return list.filter((tpl) => {
      if (!matchesDomain(tpl)) return false
      if (localeFilter && tpl.locale !== localeFilter) return false
      if (q) {
        const hay = `${templateDisplayName(tpl, 'en')} ${tpl.id} ${tpl.category}`.toLowerCase()
        if (!hay.includes(q)) return false
      }
      return true
    })
  })

  async function toggleHidden(tpl: DocxTemplate) {
    try {
      if (templateStore.isHidden(tpl.id)) await templateStore.unhide(tpl.id)
      else await templateStore.hide(tpl.id)
    } catch (e) {
      toastStore.danger(i18n.t('Errors.somethingWrong'), { detail: (e as Error).message })
    }
  }
</script>

{#if editorOpen}
  <TemplateEditor
    initial={editorInitial}
    onback={() => closeEditor(false)}
    onsaved={() => closeEditor(true)}
  />
{:else}
<div class="max-w-4xl mx-auto p-8 space-y-5">
  <header class="flex items-end justify-between gap-4">
    <div class="space-y-1">
      <h2 class="text-2xl font-semibold text-(--color-text-primary)">{i18n.t('DocxTemplates.title')}</h2>
      <p class="text-sm text-(--color-text-secondary)">
        {i18n.t('DocxTemplates.subtitle')}
      </p>
    </div>
    <Button variant="primary" onclick={() => openEditor(null)}>
      {i18n.t('DocxTemplates.edNewTemplate')}
    </Button>
  </header>

  <Tabs tabs={tabs} bind:active={activeTab} />

  <div class="flex items-end gap-3 flex-wrap">
    <Input
      bind:value={search}
      placeholder={i18n.t('DocxTemplates.searchPlaceholder')}
      size="sm"
      class="w-60"
    >
      {#snippet iconBefore()}
        <Search size={14} />
      {/snippet}
    </Input>
    <Select options={domainOptions} bind:value={domainFilter} size="sm" class="w-44" />
    <Select options={localeOptions} bind:value={localeFilter} size="sm" class="w-36" />
    <div class="flex-1"></div>
    <span class="text-xs text-(--color-text-secondary) pb-2">
      {rows.length} of {templateStore.items.length}
    </span>
  </div>

  {#if templateStore.loading}
    <div class="flex items-center gap-2 text-sm text-(--color-text-secondary) py-12 justify-center">
      <Spinner size="sm" />
      {i18n.t('Common.loading')}
    </div>
  {:else if templateStore.error}
    <EmptyState title={i18n.t('Errors.somethingWrong')} description={templateStore.error}>
      {#snippet action()}
        <Button size="sm" variant="secondary" onclick={() => templateStore.refresh()}>
          {i18n.t('Common.retry')}
        </Button>
      {/snippet}
    </EmptyState>
  {:else if rows.length === 0}
    <EmptyState
      title={i18n.t('DocxTemplates.noTemplates')}
      description={i18n.t('Ui.clearFiltersHint')}
    />
  {:else}
    <ul class="flex flex-col gap-2">
      {#each rows as t (t.id)}
        <li
          class="flex items-stretch gap-1 bg-(--color-surface-0) border border-(--color-surface-200)
                 rounded-(--radius-md) hover:border-(--color-surface-300)"
        >
          <button
            type="button"
            onclick={() => (detailId = t.id)}
            class="flex-1 min-w-0 text-left px-4 py-3 space-y-1.5"
          >
          <div class="flex items-center gap-2">
            <Badge tone="level" size="xs">{t.automation_level}</Badge>
            <span class="text-sm font-medium text-(--color-text-primary) flex-1 min-w-0 truncate">
              {templateDisplayName(t, 'en')}
            </span>
            {#if !t.is_system}
              <Badge tone="brand" size="xs">{i18n.t('Workflows.originSelf')}</Badge>
            {/if}
            <Badge tone="brand">{domainLabel(t.domain)}</Badge>
            <span class="text-xs text-(--color-text-secondary) font-mono">{t.locale}</span>
          </div>
          <div class="flex items-center gap-2 flex-wrap">
            <span class="text-xs text-(--color-text-secondary) font-mono">{t.id}</span>
            {#if t.category}
              <span class="text-xs text-(--color-text-secondary)">· {t.category}</span>
            {/if}
            {#each t.also_applicable_to as extra (extra)}
              <Badge tone="neutral" size="xs">
                {i18n.t('Ui.alsoDomain', { domain: domainLabel(extra) })}
              </Badge>
            {/each}
            {#if t.required_metadata.length > 0}
              <span class="text-xs text-(--color-text-disabled)">
                · {i18n.t('Ui.requiredFieldCount', { n: t.required_metadata.length })}
              </span>
            {/if}
          </div>
          </button>
          <div class="flex items-center gap-1 pr-2">
            <IconButton label={i18n.t('Common.edit')} size="sm" onclick={() => openEditor(t)}>
              <Pencil size={15} />
            </IconButton>
            <IconButton
              label={templateStore.isHidden(t.id) ? i18n.t('Workflows.unhide') : i18n.t('Ui.hide')}
              size="sm"
              onclick={() => toggleHidden(t)}
            >
              {#if templateStore.isHidden(t.id)}
                <EyeOff size={15} />
              {:else}
                <Eye size={15} />
              {/if}
            </IconButton>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</div>
{/if}

<TemplateDetailModal templateId={detailId} onclose={() => (detailId = null)} />
