<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Full-page workflow editor. Assistant workflows edit a Markdown prompt;
  tabular workflows edit a column table. Both auto-save (debounced).
  System presets open read-only.
-->
<script lang="ts">
  import Input from '$lib/components/ui/Input.svelte'
  import Textarea from '$lib/components/ui/Textarea.svelte'
  import Select from '$lib/components/ui/Select.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import IconButton from '$lib/components/ui/IconButton.svelte'
  import Badge from '$lib/components/ui/Badge.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import ConfirmDialog from '$lib/components/ui/ConfirmDialog.svelte'
  import TranslateModal from '$lib/components/ui/TranslateModal.svelte'
  import MarkdownEditor from '$lib/components/ui/MarkdownEditor.svelte'
  import DomainPicker from '$lib/components/ui/DomainPicker.svelte'
  import { renderMarkdown } from '$lib/utils/markdown'
  import { translateAll, type TranslateJob } from '$lib/utils/translate'
  import { workflowsApi } from '$lib/api/workflows'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { DOMAINS, domainLabel } from '$lib/types/domain'
  import type { Locale } from '$lib/types/user'
  import {
    COLUMN_FORMATS,
    columnTitle,
    normalizeColumnFormat,
    type ColumnFormat,
    type Workflow,
    type WorkflowColumn,
  } from '$lib/types/workflow'
  import { ArrowLeft, Trash2, Check, Table2, Copy, Languages } from 'lucide-svelte'

  let {
    id,
    onback,
    ondeleted,
    onopen,
  }: {
    id: string
    onback: () => void
    ondeleted: () => void
    /** Navigate the editor to another workflow (used after duplicate). */
    onopen: (workflowId: string) => void
  } = $props()

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)

  interface ColumnDraft {
    name: string
    format: ColumnFormat
    prompt: string
  }

  let wf = $state<Workflow | null>(null)
  let loading = $state(true)
  let loadError = $state<string | null>(null)

  let title = $state('')
  let promptMd = $state('')
  let columns = $state<ColumnDraft[]>([])
  let alsoApplicableTo = $state<string[]>([])
  let saveState = $state<'idle' | 'saving' | 'saved'>('idle')
  let deleteOpen = $state(false)
  let promptMode = $state<'edit' | 'preview'>('edit')
  let duplicating = $state(false)
  let translateOpen = $state(false)
  let translateDone = $state(0)
  let translateTotal = $state(0)

  const readOnly = $derived(wf?.is_system ?? true)

  const formatOptions = $derived(
    COLUMN_FORMATS.map((f) => ({ value: f, label: t(`ColumnFormats.${f}`) })),
  )
  const domainOptions = $derived(DOMAINS.map((d) => ({ value: d, label: domainLabel(d) })))

  $effect(() => {
    loading = true
    loadError = null
    workflowsApi
      .get(id)
      .then((w) => {
        wf = w
        title = w.title
        promptMd = w.prompt_md ?? ''
        alsoApplicableTo = [...(w.also_applicable_to ?? [])]
        columns = (w.columns_config ?? []).map((c) => ({
          name: ((c.name ?? c.label ?? c.key ?? '') as string).trim(),
          format: normalizeColumnFormat(c.format),
          prompt: (c.prompt ?? '') as string,
        }))
      })
      .catch((e) => (loadError = (e as Error).message))
      .finally(() => (loading = false))
  })

  function slugKey(name: string, i: number): string {
    const s = name
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '_')
      .replace(/^_+|_+$/g, '')
    return s || `col_${i + 1}`
  }

  let saveTimer: ReturnType<typeof setTimeout> | undefined

  function scheduleSave() {
    if (readOnly) return
    saveState = 'saving'
    clearTimeout(saveTimer)
    saveTimer = setTimeout(doSave, 800)
  }

  function buildColumns(): WorkflowColumn[] {
    return columns.map((c, i): WorkflowColumn => ({
      key: slugKey(c.name, i),
      name: c.name.trim(),
      prompt: c.prompt.trim(),
      format: c.format,
    }))
  }

  async function doSave() {
    if (!wf || readOnly) return
    const payload: Partial<Workflow> = {
      title: title.trim() || wf.title,
      also_applicable_to: alsoApplicableTo as Workflow['also_applicable_to'],
    }
    if (wf.type === 'assistant') {
      payload.prompt_md = promptMd
    } else {
      payload.columns_config = buildColumns()
    }
    try {
      await workflowsApi.update(id, payload)
      saveState = 'saved'
    } catch (e) {
      saveState = 'idle'
      toastStore.danger(t('Workflows.updateError'), { detail: (e as Error).message })
    }
  }

  /** Duplicate the workflow into an editable custom copy. */
  async function duplicate() {
    if (!wf) return
    duplicating = true
    try {
      const created = await workflowsApi.create({
        title: t('Workflows.copySuffix', { title: wf.title }),
        type: wf.type,
        domain: wf.domain,
        also_applicable_to: alsoApplicableTo as Workflow['also_applicable_to'],
        practice: wf.practice ?? undefined,
        prompt_md: wf.type === 'assistant' ? promptMd : undefined,
        columns_config: wf.type === 'tabular' ? buildColumns() : undefined,
      })
      toastStore.success(t('Workflows.duplicatedToast'))
      onopen(created.id)
    } catch (e) {
      toastStore.danger(t('Workflows.updateError'), { detail: (e as Error).message })
    } finally {
      duplicating = false
    }
  }

  /** Translate the prompt(s) into the language chosen in the modal. */
  async function translateTo(locale: Locale) {
    if (readOnly) return
    const jobs: TranslateJob[] = []
    if (wf?.type === 'assistant') {
      jobs.push({ text: promptMd, apply: (v) => (promptMd = v) })
    } else {
      for (const col of columns) jobs.push({ text: col.prompt, apply: (v) => (col.prompt = v) })
    }
    const err = await translateAll(jobs, locale, (d, total) => {
      translateDone = d
      translateTotal = total
    })
    scheduleSave()
    if (err) toastStore.danger(t('Translate.error'), { detail: err.message })
    else toastStore.success(t('Translate.done'))
  }

  function addColumn() {
    columns = [...columns, { name: '', format: 'free_text', prompt: '' }]
    scheduleSave()
  }
  function removeColumn(i: number) {
    columns = columns.filter((_, idx) => idx !== i)
    scheduleSave()
  }

  async function confirmDelete() {
    deleteOpen = false
    try {
      await workflowsApi.remove(id)
      toastStore.info(t('Workflows.deletedToast'))
      ondeleted()
    } catch (e) {
      toastStore.danger(t('Workflows.updateError'), { detail: (e as Error).message })
    }
  }
</script>

<div class="max-w-4xl mx-auto p-8 space-y-5">
  <button
    type="button"
    onclick={onback}
    class="flex items-center gap-1.5 text-sm text-(--color-text-secondary) hover:text-(--color-text-primary)"
  >
    <ArrowLeft size={15} />{t('Workflows.title')}
  </button>

  {#if loading}
    <div class="flex items-center gap-2 text-sm text-(--color-text-secondary) py-12 justify-center">
      <Spinner size="sm" />
      {t('Common.loading')}
    </div>
  {:else if loadError || !wf}
    <EmptyState title={t('Errors.somethingWrong')} description={loadError ?? ''} />
  {:else}
    <header class="space-y-3">
      <div class="flex items-center justify-between gap-4">
        <input
          bind:value={title}
          disabled={readOnly}
          oninput={scheduleSave}
          class="flex-1 min-w-0 bg-transparent text-2xl font-semibold text-(--color-text-primary)
                 focus:outline-none disabled:opacity-100"
        />
        <div class="flex items-center gap-2 shrink-0">
          {#if !readOnly}
            <span class="text-xs text-(--color-text-secondary)">
              {#if saveState === 'saving'}{t('Common.saving')}
              {:else if saveState === 'saved'}<span class="inline-flex items-center gap-1">
                  <Check size={12} class="text-(--color-success-500)" />{t('Workflows.savedStatus')}
                </span>{/if}
            </span>
          {/if}
          <Button size="sm" variant="secondary" loading={duplicating} onclick={duplicate}>
            <Copy size={14} class="mr-1" />{t('Workflows.duplicate')}
          </Button>
          {#if readOnly}
            <Badge tone="neutral" size="xs">{t('Workflows.readOnly')}</Badge>
          {:else}
            <IconButton label={t('Workflows.deleteWorkflow')} size="sm" variant="danger"
              onclick={() => (deleteOpen = true)}>
              <Trash2 size={15} />
            </IconButton>
          {/if}
        </div>
      </div>
      <div class="flex items-center gap-2">
        <Badge tone={wf.type === 'assistant' ? 'assistant' : 'tabular'}>
          {wf.type === 'assistant' ? t('Workflows.typeAssistant') : t('Workflows.typeTabular')}
        </Badge>
        <Badge tone="brand">{domainLabel(wf.domain)}</Badge>
        {#if readOnly}
          {#each alsoApplicableTo as d (d)}
            <Badge tone="neutral">{domainLabel(d)}</Badge>
          {/each}
        {/if}
        {#if wf.practice}
          <span class="text-xs text-(--color-text-secondary)">{wf.practice}</span>
        {/if}
      </div>

      {#if !readOnly}
        <div class="space-y-1.5">
          <span class="text-xs font-medium text-(--color-text-secondary)">{t('Workflows.alsoApplicableTo')}</span>
          <DomainPicker
            bind:selected={alsoApplicableTo}
            available={domainOptions}
            exclude={wf.domain}
            placeholder={t('Workflows.alsoApplicableToPlaceholder')}
            onchange={scheduleSave}
          />
          <p class="text-xs text-(--color-text-disabled)">{t('Workflows.alsoApplicableToHint')}</p>
        </div>
      {/if}
    </header>

    {#if wf.type === 'assistant'}
      <div class="space-y-1.5">
        <div class="flex items-center justify-between gap-2">
          <span class="text-xs font-medium text-(--color-text-secondary)">{t('Workflows.prompt')}</span>
          {#if !readOnly}
            <div class="flex items-center gap-2">
              <Button
                size="sm"
                variant="ghost"
                title={t('Workflows.translateHint')}
                onclick={() => (translateOpen = true)}
              >
                <Languages size={14} class="mr-1" />{t('Workflows.translate')}
              </Button>
              <div class="inline-flex gap-0.5 p-0.5 bg-(--color-surface-100) rounded-(--radius-md)">
                {#each [['edit', 'Workflows.editPrompt'], ['preview', 'Workflows.preview']] as [mode, key] (mode)}
                  <button
                    type="button"
                    onclick={() => (promptMode = mode as 'edit' | 'preview')}
                    class="px-2.5 h-6 rounded-(--radius-sm) text-xs font-medium
                           {promptMode === mode
                             ? 'bg-(--color-surface-0) text-(--color-brand-600) shadow-(--shadow-xs)'
                             : 'text-(--color-text-secondary) hover:text-(--color-text-primary)'}"
                  >
                    {t(key)}
                  </button>
                {/each}
              </div>
            </div>
          {/if}
        </div>
        {#if readOnly || promptMode === 'preview'}
          <div class="md-body text-sm border border-(--color-surface-200) rounded-(--radius-md)
                      p-4 min-h-40 text-(--color-text-primary)">
            {#if promptMd.trim()}
              {@html renderMarkdown(promptMd)}
            {:else}
              <span class="text-(--color-text-disabled)">{t('Assistant.noPromptDefined')}</span>
            {/if}
          </div>
        {:else}
          <MarkdownEditor
            bind:value={promptMd}
            placeholder={t('Workflows.promptPlaceholder')}
            minHeight="320px"
            oninput={scheduleSave}
          />
        {/if}
      </div>
    {:else}
      <div class="space-y-2">
        <div class="flex items-center justify-between gap-2">
          <span class="text-xs font-medium text-(--color-text-secondary)">{t('Workflows.columns')}</span>
          {#if !readOnly}
            <div class="flex items-center gap-2">
              <Button
                size="sm"
                variant="ghost"
                title={t('Workflows.translateHint')}
                onclick={() => (translateOpen = true)}
              >
                <Languages size={14} class="mr-1" />{t('Workflows.translate')}
              </Button>
              <Button size="sm" variant="secondary" onclick={addColumn}>{t('Workflows.addColumn')}</Button>
            </div>
          {/if}
        </div>
        {#if columns.length === 0}
          <p class="text-sm text-(--color-text-secondary) py-4 text-center">
            {t('Workflows.columnsEmptyHint')}
          </p>
        {/if}
        {#if readOnly}
          <!-- Read-only: clean column list (name + format). -->
          <ul class="flex flex-col gap-1.5">
            {#each columns as col, i (i)}
              <li class="flex items-center gap-2.5 px-3 py-2.5 border border-(--color-surface-200)
                         rounded-(--radius-md)">
                <Table2 size={15} class="shrink-0 text-(--color-text-secondary)" />
                <span class="flex-1 min-w-0 text-sm text-(--color-text-primary) truncate">
                  {columnTitle(col, i)}
                </span>
                <Badge tone="neutral" size="xs">{t(`ColumnFormats.${col.format}`)}</Badge>
              </li>
            {/each}
          </ul>
        {:else}
          {#each columns as col, i (i)}
            <div class="border border-(--color-surface-200) rounded-(--radius-md) p-3 space-y-2.5">
              <div class="flex items-center gap-2">
                <Input
                  bind:value={col.name}
                  placeholder={t('Workflows.columnName')}
                  class="flex-1"
                  oninput={scheduleSave}
                />
                <Select
                  options={formatOptions}
                  bind:value={col.format}
                  size="md"
                  class="w-44"
                  onchange={scheduleSave}
                />
                <IconButton label={t('Common.delete')} variant="danger" size="md"
                  onclick={() => removeColumn(i)}>
                  <Trash2 size={15} />
                </IconButton>
              </div>
              <Textarea
                bind:value={col.prompt}
                placeholder={t('Workflows.columnPromptPlaceholder')}
                minRows={2}
                oninput={scheduleSave}
              />
            </div>
          {/each}
        {/if}
      </div>
    {/if}
  {/if}
</div>

<ConfirmDialog
  bind:open={deleteOpen}
  title={t('Workflows.deleteConfirmTitle')}
  message={t('Workflows.deleteConfirmBody', { title: wf?.title ?? '' })}
  confirmLabel={t('Common.delete')}
  danger
  onconfirm={confirmDelete}
/>

<TranslateModal
  bind:open={translateOpen}
  onconfirm={translateTo}
  done={translateDone}
  total={translateTotal}
/>
