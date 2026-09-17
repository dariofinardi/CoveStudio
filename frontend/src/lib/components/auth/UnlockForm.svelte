<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import Input from '$lib/components/ui/Input.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import BiometricPrompt from './BiometricPrompt.svelte'
  import { authStore } from '$lib/stores/auth.svelte'
  import { authApi } from '$lib/api/auth'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { isValidPinFormat, PIN_MAX_LENGTH } from '$lib/types/auth'
  import { ApiError } from '$lib/types/error'
  import type { SessionUser } from '$lib/types/auth'

  interface Props {
    onsuccess: (user: SessionUser) => void
  }

  let { onsuccess }: Props = $props()

  let pin = $state('')
  let submitting = $state(false)
  let formError = $state<string | null>(null)
  let formEl = $state<HTMLFormElement>()

  // Focus the PIN field as soon as the unlock screen appears.
  $effect(() => {
    formEl?.querySelector('input')?.focus()
  })

  let biometricAvailable = $state(false)
  let biometricBusy = $state(false)

  // Lockout countdown derived from a 429 Retry-After header.
  let lockoutUntil = $state<number | null>(null)
  let now = $state(Date.now())

  const lockoutSecondsLeft = $derived(
    lockoutUntil ? Math.max(0, Math.ceil((lockoutUntil - now) / 1000)) : 0
  )
  const lockedOut = $derived(lockoutSecondsLeft > 0)

  const canSubmit = $derived(isValidPinFormat(pin) && !submitting && !lockedOut)

  $effect(() => {
    // Tick the clock only while a lockout is active.
    if (!lockoutUntil) return
    const id = setInterval(() => {
      now = Date.now()
    }, 250)
    return () => clearInterval(id)
  })

  $effect(() => {
    // Probe biometric availability once on mount.
    authApi
      .biometricAvailable()
      .then((b) => {
        biometricAvailable = b.available && b.enabled
      })
      .catch(() => {
        biometricAvailable = false
      })
  })

  function handleError(err: unknown) {
    if (err instanceof ApiError) {
      formError = err.detail
      if (err.isRateLimited && err.retryAfter) {
        lockoutUntil = Date.now() + err.retryAfter * 1000
        now = Date.now()
      }
    } else {
      formError = (err as Error).message
    }
  }

  async function submitPin(e: SubmitEvent) {
    e.preventDefault()
    if (!canSubmit) return
    submitting = true
    formError = null
    try {
      const user = await authStore.unlock(pin)
      onsuccess(user)
    } catch (err) {
      handleError(err)
      pin = ''
    } finally {
      submitting = false
    }
  }

  async function unlockBiometric() {
    biometricBusy = true
    formError = null
    try {
      const user = await authStore.unlockBiometric()
      onsuccess(user)
    } catch (err) {
      handleError(err)
    } finally {
      biometricBusy = false
    }
  }
</script>

<form class="space-y-4" bind:this={formEl} onsubmit={submitPin}>
  <Input
    label={i18n.t('Auth.pin')}
    type="password"
    bind:value={pin}
    autofocus
    placeholder={i18n.t('Auth.pinEnter')}
    inputmode="numeric"
    maxlength={PIN_MAX_LENGTH}
    autocomplete="current-password"
    disabled={lockedOut}
  />

  {#if formError}
    <p class="text-sm text-(--color-danger-500)">{formError}</p>
  {/if}

  {#if lockedOut}
    <p class="text-sm text-(--color-warning-500)">
      {i18n.t('Auth.lockout', { secs: lockoutSecondsLeft })}
    </p>
  {/if}

  <Button type="submit" full loading={submitting} disabled={!canSubmit}>
    {i18n.t('Auth.unlock')}
  </Button>

  {#if biometricAvailable}
    <div class="flex items-center gap-3">
      <span class="flex-1 h-px bg-(--color-surface-200)"></span>
      <span class="text-xs text-(--color-text-secondary)">{i18n.t('Common.or')}</span>
      <span class="flex-1 h-px bg-(--color-surface-200)"></span>
    </div>
    <Button
      variant="secondary"
      full
      loading={biometricBusy}
      disabled={biometricBusy || lockedOut}
      onclick={unlockBiometric}
    >
      {i18n.t('Auth.useBiometric')}
    </Button>
  {/if}
</form>

<BiometricPrompt open={biometricBusy} reason={i18n.t('Auth.biometricReason')} />
