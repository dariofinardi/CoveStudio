<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Change-PIN form. The default path requires the current PIN. When
  biometrics are enrolled, a "forgot PIN" path lets the user set a new
  PIN by verifying with Windows Hello / Touch ID instead.
-->
<script lang="ts">
  import Input from '$lib/components/ui/Input.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import BiometricPrompt from '$lib/components/auth/BiometricPrompt.svelte'
  import { authApi } from '$lib/api/auth'
  import { isValidPinFormat, PIN_MIN_LENGTH, PIN_MAX_LENGTH } from '$lib/types/auth'
  import { ApiError } from '$lib/types/error'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'

  let mode = $state<'pin' | 'biometric'>('pin')
  let currentPin = $state('')
  let newPin = $state('')
  let confirmPin = $state('')
  let submitting = $state(false)
  let formError = $state<string | null>(null)

  // Whether the biometric reset path can be offered.
  let biometricReady = $state(false)
  $effect(() => {
    authApi
      .biometricAvailable()
      .then((b) => (biometricReady = b.available && b.enabled))
      .catch(() => (biometricReady = false))
  })

  const newPinError = $derived.by(() => {
    if (newPin.length === 0) return undefined
    if (!isValidPinFormat(newPin))
      return i18n.t('Auth.pinFormat', { min: PIN_MIN_LENGTH, max: PIN_MAX_LENGTH })
    return undefined
  })
  const confirmError = $derived.by(() => {
    if (confirmPin.length === 0) return undefined
    if (confirmPin !== newPin) return i18n.t('Auth.pinMismatch')
    return undefined
  })
  const canSubmit = $derived(
    isValidPinFormat(newPin) &&
      newPin === confirmPin &&
      !submitting &&
      (mode === 'biometric' || currentPin.length > 0)
  )

  function reset() {
    currentPin = ''
    newPin = ''
    confirmPin = ''
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault()
    if (!canSubmit) return
    submitting = true
    formError = null
    try {
      if (mode === 'biometric') {
        // Triggers the OS biometric prompt; resolves once verified.
        await authApi.changePinBiometric(newPin)
      } else {
        await authApi.changePin(currentPin, newPin)
      }
      toastStore.success(i18n.t('Settings.pinChanged'))
      reset()
      mode = 'pin'
    } catch (err) {
      formError = err instanceof ApiError ? err.detail : (err as Error).message
    } finally {
      submitting = false
    }
  }
</script>

<form class="space-y-4 max-w-sm" onsubmit={submit}>
  {#if mode === 'pin'}
    <Input
      label={i18n.t('Account.currentPin')}
      type="password"
      bind:value={currentPin}
      inputmode="numeric"
      maxlength={PIN_MAX_LENGTH}
      autocomplete="current-password"
    />
  {:else}
    <p class="text-xs text-(--color-text-secondary)">{i18n.t('Settings.pinResetHint')}</p>
  {/if}

  <Input
    label={i18n.t('Account.newPin')}
    type="password"
    bind:value={newPin}
    inputmode="numeric"
    maxlength={PIN_MAX_LENGTH}
    error={newPinError}
    autocomplete="new-password"
  />
  <Input
    label={i18n.t('Account.confirmNewPin')}
    type="password"
    bind:value={confirmPin}
    inputmode="numeric"
    maxlength={PIN_MAX_LENGTH}
    error={confirmError}
    autocomplete="new-password"
  />

  {#if formError}
    <p class="text-sm text-(--color-danger-500)">{formError}</p>
  {/if}

  <div class="flex items-center gap-3">
    <Button type="submit" loading={submitting} disabled={!canSubmit}>
      {mode === 'biometric'
        ? i18n.t('Settings.resetWithBiometric')
        : i18n.t('Account.changePin')}
    </Button>
    {#if biometricReady}
      <button
        type="button"
        class="text-xs text-(--color-brand-600) hover:underline"
        onclick={() => {
          mode = mode === 'pin' ? 'biometric' : 'pin'
          formError = null
        }}
      >
        {mode === 'pin' ? i18n.t('Settings.forgotPin') : i18n.t('Settings.usePinInstead')}
      </button>
    {/if}
  </div>
</form>

<BiometricPrompt
  open={submitting && mode === 'biometric'}
  reason={i18n.t('Settings.pinResetReason')}
/>
