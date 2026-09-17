<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!-- Modal confirmation dialog for destructive or irreversible actions. -->
<script lang="ts">
  import Modal from './Modal.svelte'
  import Button from './Button.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'

  interface Props {
    open?: boolean
    title: string
    message: string
    confirmLabel?: string
    cancelLabel?: string
    danger?: boolean
    /** Awaited; the dialog shows a spinner until it resolves. */
    onconfirm: () => void | Promise<void>
    oncancel?: () => void
  }

  let {
    open = $bindable(false),
    title,
    message,
    confirmLabel,
    cancelLabel,
    danger = false,
    onconfirm,
    oncancel,
  }: Props = $props()

  const confirmText = $derived(confirmLabel ?? i18n.t('Common.confirm'))
  const cancelText = $derived(cancelLabel ?? i18n.t('Common.cancel'))

  let busy = $state(false)

  // Enter confirms; Esc is handled by the Modal (closeOnEsc → oncancel).
  $effect(() => {
    if (!open) return
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Enter' && !busy) {
        e.preventDefault()
        void confirm()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  function cancel() {
    if (busy) return
    open = false
    oncancel?.()
  }

  async function confirm() {
    busy = true
    try {
      await onconfirm()
      open = false
    } finally {
      busy = false
    }
  }
</script>

<Modal bind:open {title} size="sm" closeOnBackdrop={!busy} closeOnEsc={!busy} onclose={oncancel}>
  <p class="text-sm text-(--color-text-secondary)">{message}</p>
  {#snippet footer()}
    <Button variant="ghost" onclick={cancel} disabled={busy}>{cancelText}</Button>
    <Button variant={danger ? 'danger' : 'primary'} loading={busy} onclick={confirm}>
      {confirmText}
    </Button>
  {/snippet}
</Modal>
