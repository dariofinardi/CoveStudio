<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Language picker modal for translation actions. Shared by the workflow
  editor and the DOCX template editor: the caller decides what gets
  translated, this dialog only collects the target language.
-->
<script lang="ts">
  import Modal from './Modal.svelte'
  import Button from './Button.svelte'
  import Select from './Select.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { LOCALES, LOCALE_LABELS, type Locale } from '$lib/types/user'

  interface Props {
    open?: boolean
    /** Awaited; the modal shows a spinner until it resolves, then closes. */
    onconfirm: (locale: Locale) => void | Promise<void>
    oncancel?: () => void
    /** Optional live progress shown while translating. */
    done?: number
    total?: number
  }

  let { open = $bindable(false), onconfirm, oncancel, done = 0, total = 0 }: Props = $props()

  // Default the picker to the current UI language — the common case.
  /* svelte-ignore state_referenced_locally */
  let target = $state<Locale>(i18n.locale)
  let busy = $state(false)

  const options = LOCALES.map((l) => ({ value: l, label: LOCALE_LABELS[l] }))

  function cancel() {
    if (busy) return
    open = false
    oncancel?.()
  }

  async function confirm() {
    busy = true
    try {
      await onconfirm(target)
      open = false
    } finally {
      busy = false
    }
  }
</script>

<Modal
  bind:open
  title={i18n.t('Translate.title')}
  size="sm"
  closeOnBackdrop={!busy}
  closeOnEsc={!busy}
  onclose={oncancel}
>
  <div class="space-y-3">
    <p class="text-sm text-(--color-text-secondary)">{i18n.t('Translate.description')}</p>
    <Select
      label={i18n.t('Translate.language')}
      options={options}
      bind:value={target}
      disabled={busy}
    />
    {#if busy && total > 0}
      <p class="text-xs text-(--color-text-secondary)">
        {i18n.t('Translate.progress', { done, total })}
      </p>
    {/if}
  </div>
  {#snippet footer()}
    <Button variant="ghost" onclick={cancel} disabled={busy}>{i18n.t('Common.cancel')}</Button>
    <Button variant="primary" loading={busy} onclick={confirm}>
      {i18n.t('Workflows.translate')}
    </Button>
  {/snippet}
</Modal>
