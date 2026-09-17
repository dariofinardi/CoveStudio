<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Create-workflow modal. Handles both workflow types in one surface:
  - Assistant: a Markdown prompt editor (the workflow *is* the prompt).
  - Tabular: a column editor — each column has a name, an output format
    and an extraction prompt (mirrors the legacy "Add column" dialog).
  Metadata (name / type / domain / practice) is shared by both.
-->
<script lang="ts">
  import Modal from '$lib/components/ui/Modal.svelte'
  import Input from '$lib/components/ui/Input.svelte'
  import Textarea from '$lib/components/ui/Textarea.svelte'
  import Select from '$lib/components/ui/Select.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import IconButton from '$lib/components/ui/IconButton.svelte'
  import ChipGroup from '$lib/components/ui/ChipGroup.svelte'
  import DomainPicker from '$lib/components/ui/DomainPicker.svelte'
  import MarkdownEditor from '$lib/components/ui/MarkdownEditor.svelte'
  import { workflowsApi } from '$lib/api/workflows'
  import { userStore } from '$lib/stores/user.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { ApiError } from '$lib/types/error'
  import { DOMAINS, domainLabel, type Domain } from '$lib/types/domain'
  import {
    COLUMN_FORMATS,
    type ColumnFormat,
    type WorkflowColumn,
    type WorkflowType,
  } from '$lib/types/workflow'
  import { Trash2 } from 'lucide-svelte'

  interface Props {
    open?: boolean
    onsuccess: (createdId?: string) => void
  }

  let { open = $bindable(false), onsuccess }: Props = $props()

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)

  const PRACTICES = [
    'General Transactions', 'Corporate', 'Finance', 'Litigation', 'Real Estate',
    'Tax', 'Employment', 'IP', 'Competition', 'Tech Transactions',
    'Project Finance', 'EC/VC', 'Private Equity', 'Private Credit', 'ECM',
    'DCM', 'Lev Fin', 'Arbitration', 'Others',
  ]

  interface ColumnDraft {
    name: string
    format: ColumnFormat
    prompt: string
  }

  let title = $state('')
  let type = $state<WorkflowType>('assistant')
  let domain = $state<Domain>('legal')
  let alsoApplicableTo = $state<string[]>([])
  let practice = $state<string | null>(null)
  let promptMd = $state('')
  let columns = $state<ColumnDraft[]>([])
  let submitting = $state(false)
  let formError = $state<string | null>(null)

  const domainOptions = $derived(DOMAINS.map((d) => ({ value: d, label: domainLabel(d) })))

  // Drop the primary domain if the user picked it as an extra after the
  // fact — it can never be both primary and additional.
  $effect(() => {
    if (alsoApplicableTo.includes(domain)) {
      alsoApplicableTo = alsoApplicableTo.filter((d) => d !== domain)
    }
  })
  const practiceChips = $derived(
    PRACTICES.map((p) => ({ value: p, label: t(`Workflows.practiceLabels.${p}`) }))
  )
  const formatOptions = $derived(
    COLUMN_FORMATS.map((f) => ({ value: f, label: t(`ColumnFormats.${f}`) }))
  )

  const canSubmit = $derived(
    title.trim().length > 0 &&
      !submitting &&
      (type === 'assistant' ||
        (columns.length > 0 && columns.every((c) => c.name.trim().length > 0)))
  )

  $effect(() => {
    // Reset the form each time the modal opens.
    if (open) {
      title = ''
      type = 'assistant'
      domain = userStore.defaultDomain
      alsoApplicableTo = []
      practice = null
      promptMd = ''
      columns = []
      formError = null
    }
  })

  function addColumn() {
    columns = [...columns, { name: '', format: 'free_text', prompt: '' }]
  }
  function removeColumn(i: number) {
    columns = columns.filter((_, idx) => idx !== i)
  }

  function slugKey(name: string, i: number): string {
    const s = name
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '_')
      .replace(/^_+|_+$/g, '')
    return s || `col_${i + 1}`
  }

  async function create() {
    if (!canSubmit) return
    submitting = true
    formError = null
    try {
      const payload: Record<string, unknown> = {
        title: title.trim(),
        type,
        domain,
        also_applicable_to: alsoApplicableTo,
        practice: practice ?? undefined,
      }
      if (type === 'assistant') {
        payload.prompt_md = promptMd
      } else {
        payload.columns_config = columns.map(
          (c, i): WorkflowColumn => ({
            key: slugKey(c.name, i),
            name: c.name.trim(),
            prompt: c.prompt.trim(),
            format: c.format,
          })
        )
      }
      const created = await workflowsApi.create(payload)
      open = false
      onsuccess(created?.id)
    } catch (e) {
      formError = e instanceof ApiError ? e.detail : (e as Error).message
    } finally {
      submitting = false
    }
  }
</script>

<Modal bind:open title={t('Workflows.newWorkflow')} size="xl">
  <div class="space-y-5">
    <Input
      bind:value={title}
      placeholder={t('Workflows.workflowNamePlaceholder')}
      class="text-base"
    />

    <!-- Type -->
    <div class="space-y-1.5">
      <span class="text-xs font-medium text-(--color-text-secondary)">{t('Workflows.typeLabel')}</span>
      <div class="inline-flex gap-1 p-0.5 bg-(--color-surface-100) rounded-(--radius-md)">
        {#each (['assistant', 'tabular'] as WorkflowType[]) as ty (ty)}
          <button
            type="button"
            onclick={() => (type = ty)}
            class="px-3 h-8 rounded-(--radius-sm) text-sm font-medium
                   transition-colors duration-(--transition-fast)
                   {type === ty
                     ? 'bg-(--color-surface-0) text-(--color-brand-600) shadow-(--shadow-xs)'
                     : 'text-(--color-text-secondary) hover:text-(--color-text-primary)'}"
          >
            {ty === 'assistant' ? t('Workflows.typeAssistant') : t('Workflows.typeTabular')}
          </button>
        {/each}
      </div>
    </div>

    <!-- Domain -->
    <Select label={t('Domains.label')} options={domainOptions} bind:value={domain} class="w-60" />

    <!-- Additional domains (cross-domain registration) -->
    <div class="space-y-1.5">
      <span class="text-xs font-medium text-(--color-text-secondary)">{t('Workflows.alsoApplicableTo')}</span>
      <DomainPicker
        bind:selected={alsoApplicableTo}
        available={domainOptions}
        exclude={domain}
        placeholder={t('Workflows.alsoApplicableToPlaceholder')}
      />
      <p class="text-xs text-(--color-text-disabled)">{t('Workflows.alsoApplicableToHint')}</p>
    </div>

    <!-- Practice -->
    <div class="space-y-1.5">
      <span class="text-xs font-medium text-(--color-text-secondary)">{t('Workflows.practiceArea')}</span>
      <ChipGroup chips={practiceChips} bind:selected={practice} size="sm" />
    </div>

    <div class="border-t border-(--color-surface-100)"></div>

    <!-- The Modal body is the single scroll container; the prompt
         editor scrolls internally only once it overflows. -->
    <div>
    {#if type === 'assistant'}
      <div class="space-y-1.5">
        <span class="text-xs font-medium text-(--color-text-secondary)">{t('Workflows.prompt')}</span>
        <MarkdownEditor
          bind:value={promptMd}
          placeholder={t('Workflows.promptPlaceholder')}
          minHeight="240px"
        />
      </div>
    {:else}
      <div class="space-y-2">
        <div class="flex items-center justify-between">
          <span class="text-xs font-medium text-(--color-text-secondary)">{t('Workflows.columns')}</span>
          <Button size="sm" variant="secondary" onclick={addColumn}>{t('Workflows.addColumn')}</Button>
        </div>

        {#if columns.length === 0}
          <p class="text-sm text-(--color-text-secondary) py-4 text-center">
            {t('Workflows.columnsEmptyHint')}
          </p>
        {/if}

        {#each columns as col, i (i)}
          <div class="border border-(--color-surface-200) rounded-(--radius-md) p-3 space-y-2.5">
            <div class="flex items-center gap-2">
              <Input
                bind:value={col.name}
                placeholder={t('Workflows.columnName')}
                class="flex-1"
              />
              <Select
                options={formatOptions}
                bind:value={col.format}
                size="md"
                class="w-44"
              />
              <IconButton label={t('Common.delete')} variant="danger" size="md" onclick={() => removeColumn(i)}>
                <Trash2 size={15} />
              </IconButton>
            </div>
            <Textarea
              bind:value={col.prompt}
              placeholder={t('Workflows.columnPromptPlaceholder')}
              minRows={2}
            />
          </div>
        {/each}
      </div>
    {/if}
    </div>

    {#if formError}
      <p class="text-sm text-(--color-danger-500)">{formError}</p>
    {/if}
  </div>

  {#snippet footer()}
    <Button variant="ghost" onclick={() => (open = false)}>{t('Common.cancel')}</Button>
    <Button loading={submitting} disabled={!canSubmit} onclick={create}>
      {t('Workflows.createWorkflow')}
    </Button>
  {/snippet}
</Modal>
