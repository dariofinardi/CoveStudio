<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Ordered "steps" block for an assistant turn: running/finished tool
  calls and generated-document cards. Rendered above the answer text.
-->
<script lang="ts">
  import type { ChatStep } from '$lib/types/chat'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { docViewer } from '$lib/stores/doc-viewer.svelte'
  import { documentsApi } from '$lib/api/documents'
  import { Check, FileText, Download, Eye, Search, Workflow } from 'lucide-svelte'
  import { fileIconColor as iconColor } from '$lib/utils/file-icon'

  let { steps }: { steps: ChatStep[] } = $props()

  /** A slow tool (≥10 s) is probably awaiting a manual approval. */
  const SLOW_THRESHOLD = 10

  function openDoc(documentId: string, filename: string) {
    docViewer.openDocument(documentId, filename)
  }

  async function download(documentId: string, filename: string) {
    try {
      const blob = await documentsApi.downloadBytes(documentId)
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = filename || 'document'
      a.click()
      URL.revokeObjectURL(url)
    } catch {
      // non-fatal
    }
  }
</script>

<div class="flex flex-col gap-1.5 mb-2">
  {#each steps as step, i (i)}
    {#if step.kind === 'tool'}
      <div class="flex flex-col gap-1">
        <div class="flex items-center gap-2 text-xs text-(--color-text-secondary)">
          {#if step.done}
            <Check size={13} class="text-(--color-success-500) shrink-0" />
            <span>{step.name}</span>
          {:else}
            <Spinner size="sm" />
            <span>
              {i18n.t('Assistant.running')} {step.name}…{#if step.elapsedSecs > 0}
                ({step.elapsedSecs}s){/if}
            </span>
          {/if}
        </div>
        {#if !step.done && step.elapsedSecs >= SLOW_THRESHOLD}
          <p class="text-[11px] text-(--color-text-secondary) pl-5 max-w-md leading-snug">
            {i18n.t('Assistant.slowToolHint')}
          </p>
        {/if}
      </div>
    {:else if step.kind === 'doc_read'}
      <div class="flex items-center gap-2 text-xs text-(--color-text-secondary)">
        <Eye size={13} class="text-(--color-success-500) shrink-0" />
        <span>{i18n.t('Assistant.stepDocRead', { file: step.filename })}</span>
      </div>
    {:else if step.kind === 'doc_find'}
      <div class="flex items-center gap-2 text-xs text-(--color-text-secondary)">
        <Search size={13} class="text-(--color-success-500) shrink-0" />
        <span>
          {i18n.t('Assistant.stepDocFind', {
            query: step.query,
            count: step.occurrences,
            file: step.filename,
          })}
        </span>
      </div>
    {:else if step.kind === 'workflow_applied'}
      <div class="flex items-center gap-2 text-xs text-(--color-text-secondary)">
        <Workflow size={13} class="text-(--color-success-500) shrink-0" />
        <span>{i18n.t('Assistant.stepWorkflowApplied', { title: step.title })}</span>
      </div>
    {:else if step.kind === 'doc_extract'}
      <div class="flex items-center gap-2 text-xs text-(--color-text-secondary)">
        {#if step.done}
          <Check size={13} class="text-(--color-success-500) shrink-0" />
          <span>{i18n.t('Assistant.stepDocExtractDone', { file: step.filename, chars: step.chars.toLocaleString() })}</span>
        {:else}
          <Spinner size="sm" />
          <span>{i18n.t('Assistant.stepDocExtractStarting', { file: step.filename })}</span>
        {/if}
      </div>
    {:else if step.kind === 'pii_redact'}
      <div class="flex items-center gap-2 text-xs text-(--color-text-secondary)">
        {#if step.done}
          <Check size={13} class="text-(--color-success-500) shrink-0" />
          <span>{i18n.t('Assistant.stepPiiRedactDone', { file: step.filename })}</span>
        {:else}
          <Spinner size="sm" />
          <span>
            {#if step.total > 0}
              {i18n.t('Assistant.stepPiiRedactProgress', {
                file: step.filename,
                current: step.current,
                total: step.total,
              })}
            {:else}
              {i18n.t('Assistant.stepPiiRedactStarting', { file: step.filename })}
            {/if}
          </span>
        {/if}
      </div>
    {:else}
      <div
        class="flex items-center gap-2 px-2.5 py-1.5 rounded-(--radius-md)
               border border-(--color-surface-200) bg-(--color-surface-50) w-fit max-w-sm"
      >
        <FileText size={14} class="{iconColor(step.filename)} shrink-0" />
        <button
          type="button"
          class="flex-1 min-w-0 truncate text-xs text-(--color-text-primary) text-left hover:underline"
          onclick={() => openDoc(step.documentId, step.filename)}
        >
          {step.filename}
        </button>
        <button
          type="button"
          class="shrink-0 text-(--color-text-secondary) hover:text-(--color-text-primary)"
          aria-label={i18n.t('Assistant.downloadDocument')}
          onclick={() => download(step.documentId, step.filename)}
        >
          <Download size={13} />
        </button>
      </div>
    {/if}
  {/each}
</div>
