<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Read-only viewer for a rejected document's archived reason + summary.

  RejectReasonModal handles the *write* path (user enters a motive →
  backend generates the summary → both persist). Once the modal closes
  the summary is no longer visible anywhere. This component reopens it
  on demand: the toolbar surfaces a "Vedi riassunto" button next to the
  rejected-state chip, which mounts this modal in pure preview mode —
  no LLM call, no mutation, just the values already in the store.

  Visible only when the active tab is in `decision: 'rejected'`. If the
  store has no archived summary (legacy rows, summarizer failure) we
  still render the reason and a small note explaining the gap.
-->
<script lang="ts">
  import Modal from '$lib/components/ui/Modal.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'

  interface Props {
    open: boolean
    filename: string
    reason: string | null | undefined
    summary: string | null | undefined
    onclose?: () => void
  }

  let { open = $bindable(false), filename, reason, summary, onclose }: Props = $props()

  function confirmClose() {
    open = false
    onclose?.()
  }
</script>

<Modal bind:open size="lg" title={i18n.t('DocViewer.summaryModal.title', { file: filename })}>
  <p class="text-sm text-(--color-text-secondary) mb-3">
    {i18n.t('DocViewer.summaryModal.intro')}
  </p>
  <div class="space-y-3">
    <div>
      <p class="text-xs font-medium text-(--color-text-secondary) mb-1">
        {i18n.t('DocViewer.reject.yourReason')}
      </p>
      <p
        class="text-sm text-(--color-text-primary) whitespace-pre-wrap
               p-3 rounded-(--radius-md) bg-(--color-surface-50)"
      >
        {reason ?? i18n.t('DocViewer.summaryModal.noReason')}
      </p>
    </div>
    <div>
      <p class="text-xs font-medium text-(--color-text-secondary) mb-1">
        {i18n.t('DocViewer.reject.summaryLabel')}
      </p>
      {#if summary}
        <p
          class="text-sm text-(--color-text-primary) whitespace-pre-wrap
                 p-3 rounded-(--radius-md) bg-(--color-surface-50)
                 border-l-2 border-(--color-brand-500)"
        >
          {summary}
        </p>
      {:else}
        <p class="text-xs text-(--color-text-secondary) italic
                  p-3 rounded-(--radius-md) bg-(--color-surface-50)">
          {i18n.t('DocViewer.summaryModal.noSummary')}
        </p>
      {/if}
    </div>
    <p class="text-[11px] text-(--color-text-secondary)">
      {i18n.t('DocViewer.summaryModal.explain')}
    </p>
  </div>

  {#snippet footer()}
    <Button onclick={confirmClose}>
      {i18n.t('DocViewer.reject.done')}
    </Button>
  {/snippet}
</Modal>
