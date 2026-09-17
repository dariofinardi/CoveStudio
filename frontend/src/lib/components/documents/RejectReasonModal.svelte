<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Two-step rejection requester for a model-generated document.

  Step 1 — the user explains *why* the version is being rejected
  (free-text, min 10 chars). Hitting "Genera riassunto" calls
  `POST /document/:id/decision` which fires a one-shot LLM call
  server-side to summarise the rejected document; the response
  carries the summary string.

  Step 2 — the modal renders the summary the backend just produced
  next to the user's reason and asks for a final confirmation
  ("Conferma rifiuto"). On confirm the modal closes and the active
  tab's decision state is updated; subsequent chat turns will see
  the summary + reason in place of the document body.

  Annulla at any step rolls back. The first call already persisted
  the rejection state server-side (the backend is the source of
  truth), so closing the modal between step 1 and 2 leaves the
  document rejected — which matches the "Reject means reject"
  expectation: the user pressed Reject, they meant it; the summary
  preview is a transparency feature, not a confirmation gate.
-->
<script lang="ts">
  import Modal from '$lib/components/ui/Modal.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import { untrack } from 'svelte'
  import { documentsApi } from '$lib/api/documents'
  import { docViewer } from '$lib/stores/doc-viewer.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'

  interface Props {
    open: boolean
    docId: string
    filename: string
    /** Optional pre-fill (archived reason from a previous reject cycle). */
    initialReason?: string | null
    onclose?: () => void
  }

  let { open = $bindable(false), docId, filename, initialReason, onclose }: Props = $props()

  let reason = $state('')
  let summary = $state<string | null>(null)
  let busy = $state(false)

  // Re-fill reason from the archived value every time the modal opens,
  // so a re-reject after an earlier accept doesn't lose the prior text.
  //
  // `initialReason` is read inside `untrack`: we want this effect to
  // fire on the open→true transition, not whenever the parent updates
  // the prop. Without untrack the effect would re-run during
  // `generateAndPersist` — `docViewer.setDecision` archives the new
  // reason, the parent re-passes it as `initialReason`, and the
  // effect would clobber `summary` back to null mid-await, pinning
  // the modal in step 1 even after the backend returned the summary.
  $effect(() => {
    if (open) {
      reason = untrack(() => initialReason ?? '')
      summary = null
      busy = false
    }
  })

  const reasonValid = $derived(reason.trim().length >= 10)

  async function generateAndPersist() {
    if (!reasonValid || busy) return
    busy = true
    try {
      const res = await documentsApi.setDecision(docId, {
        decision: 'rejected',
        reason: reason.trim(),
      })
      summary = res.summary ?? ''
      docViewer.setDecision(docId, 'rejected', res.reason, res.summary)
    } catch (err) {
      toastStore.danger(
        i18n.t('DocViewer.reject.error', {
          err: (err as Error).message ?? String(err),
        }),
      )
    } finally {
      busy = false
    }
  }

  function confirmClose() {
    open = false
    onclose?.()
  }
</script>

<Modal bind:open size="lg" title={i18n.t('DocViewer.reject.title', { file: filename })}>
  {#if summary === null}
    <p class="text-sm text-(--color-text-secondary) mb-3">
      {i18n.t('DocViewer.reject.intro')}
    </p>
    <label class="block text-xs font-medium text-(--color-text-secondary) mb-1.5" for="reject-reason">
      {i18n.t('DocViewer.reject.reasonLabel')}
    </label>
    <textarea
      id="reject-reason"
      bind:value={reason}
      rows="6"
      placeholder={i18n.t('DocViewer.reject.placeholder')}
      class="w-full px-3 py-2 text-sm rounded-(--radius-md)
             border border-(--color-surface-200) bg-(--color-surface-0)
             text-(--color-text-primary)
             focus:outline-none focus-visible:ring-2 focus-visible:ring-(--color-brand-500)"
    ></textarea>
    <p class="mt-1 text-[11px] text-(--color-text-secondary)">
      {i18n.t('DocViewer.reject.minHint', { n: 10 })}
    </p>
  {:else}
    <div class="space-y-3">
      <div>
        <p class="text-xs font-medium text-(--color-text-secondary) mb-1">
          {i18n.t('DocViewer.reject.yourReason')}
        </p>
        <p class="text-sm text-(--color-text-primary) whitespace-pre-wrap
                  p-3 rounded-(--radius-md) bg-(--color-surface-50)">
          {reason}
        </p>
      </div>
      <div>
        <p class="text-xs font-medium text-(--color-text-secondary) mb-1">
          {i18n.t('DocViewer.reject.summaryLabel')}
        </p>
        <p class="text-sm text-(--color-text-primary) whitespace-pre-wrap
                  p-3 rounded-(--radius-md) bg-(--color-surface-50)
                  border-l-2 border-(--color-brand-500)">
          {summary}
        </p>
      </div>
      <p class="text-[11px] text-(--color-text-secondary)">
        {i18n.t('DocViewer.reject.summaryExplain')}
      </p>
    </div>
  {/if}

  {#snippet footer()}
    {#if summary === null}
      <Button variant="secondary" onclick={confirmClose} disabled={busy}>
        {i18n.t('Common.cancel')}
      </Button>
      <Button onclick={generateAndPersist} disabled={!reasonValid || busy}>
        {#if busy}
          <span class="inline-flex items-center gap-2"><Spinner size="sm" />{i18n.t('DocViewer.reject.generating')}</span>
        {:else}
          {i18n.t('DocViewer.reject.confirm')}
        {/if}
      </Button>
    {:else}
      <Button onclick={confirmClose}>
        {i18n.t('DocViewer.reject.done')}
      </Button>
    {/if}
  {/snippet}
</Modal>
