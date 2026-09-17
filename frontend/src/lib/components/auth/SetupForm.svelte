<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import Input from '$lib/components/ui/Input.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import { authStore } from '$lib/stores/auth.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { isValidPinFormat, PIN_MIN_LENGTH, PIN_MAX_LENGTH } from '$lib/types/auth'
  import { ApiError } from '$lib/types/error'
  import type { SessionUser } from '$lib/types/auth'

  interface Props {
    onsuccess: (user: SessionUser) => void
  }

  let { onsuccess }: Props = $props()

  let username = $state('')
  let displayName = $state('')
  let pin = $state('')
  let pinConfirm = $state('')
  let submitting = $state(false)
  let formError = $state<string | null>(null)

  const pinError = $derived.by(() => {
    if (pin.length === 0) return undefined
    if (!isValidPinFormat(pin))
      return i18n.t('Auth.pinFormat', { min: PIN_MIN_LENGTH, max: PIN_MAX_LENGTH })
    return undefined
  })

  const pinConfirmError = $derived.by(() => {
    if (pinConfirm.length === 0) return undefined
    if (pinConfirm !== pin) return i18n.t('Auth.pinMismatch')
    return undefined
  })

  const canSubmit = $derived(
    username.trim().length > 0 &&
      isValidPinFormat(pin) &&
      pin === pinConfirm &&
      !submitting
  )

  async function submit(e: SubmitEvent) {
    e.preventDefault()
    if (!canSubmit) return
    submitting = true
    formError = null
    try {
      const user = await authStore.setup({
        username: username.trim(),
        pin,
        display_name: displayName.trim() || undefined,
      })
      onsuccess(user)
    } catch (err) {
      formError =
        err instanceof ApiError ? err.detail : (err as Error).message
    } finally {
      submitting = false
    }
  }
</script>

<form class="space-y-4" onsubmit={submit}>
  <Input
    label={i18n.t('Auth.username')}
    bind:value={username}
    placeholder={i18n.t('Auth.usernamePlaceholder')}
    autocomplete="username"
    required
  />

  <Input
    label={i18n.t('Auth.displayNameOptional')}
    bind:value={displayName}
    placeholder={i18n.t('Auth.displayNamePlaceholder')}
    autocomplete="name"
  />

  <Input
    label={i18n.t('Auth.pin')}
    type="password"
    bind:value={pin}
    autofocus
    placeholder={i18n.t('Auth.pinPlaceholder', { min: PIN_MIN_LENGTH, max: PIN_MAX_LENGTH })}
    inputmode="numeric"
    maxlength={PIN_MAX_LENGTH}
    error={pinError}
    autocomplete="new-password"
  />

  <Input
    label={i18n.t('Auth.confirmPin')}
    type="password"
    bind:value={pinConfirm}
    placeholder={i18n.t('Auth.confirmPinPlaceholder')}
    inputmode="numeric"
    maxlength={PIN_MAX_LENGTH}
    error={pinConfirmError}
    autocomplete="new-password"
  />

  {#if formError}
    <p class="text-sm text-(--color-danger-500)">{formError}</p>
  {/if}

  <Button type="submit" full loading={submitting} disabled={!canSubmit}>
    {i18n.t('Auth.createProfile')}
  </Button>
</form>
