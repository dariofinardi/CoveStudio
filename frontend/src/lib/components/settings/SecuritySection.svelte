<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import Card from '$lib/components/ui/Card.svelte'
  import Toggle from '$lib/components/ui/Toggle.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import ChangePinForm from './ChangePinForm.svelte'
  import BiometricPrompt from '$lib/components/auth/BiometricPrompt.svelte'
  import { authApi } from '$lib/api/auth'
  import { authStore } from '$lib/stores/auth.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { ApiError } from '$lib/types/error'

  let probing = $state(true)
  let available = $state(false)
  let enabled = $state(false)
  let busy = $state(false)

  $effect(() => {
    authApi
      .biometricAvailable()
      .then((b) => {
        available = b.available
        enabled = b.enabled
      })
      .catch(() => {
        available = false
      })
      .finally(() => {
        probing = false
      })
  })

  async function onToggle(next: boolean) {
    busy = true
    try {
      if (next) {
        await authApi.biometricEnable()
        enabled = true
        authStore.setBiometricEnrolled(true)
        toastStore.success(i18n.t('Settings.biometricEnabled'))
      } else {
        await authApi.biometricDisable()
        enabled = false
        authStore.setBiometricEnrolled(false)
        toastStore.info(i18n.t('Settings.biometricDisabled'))
      }
    } catch (err) {
      // revert the optimistic toggle
      enabled = !next
      toastStore.danger(i18n.t('Settings.biometricChangeError'), {
        detail: err instanceof ApiError ? err.detail : (err as Error).message,
      })
    } finally {
      busy = false
    }
  }
</script>

<div class="space-y-4">
  <Card title={i18n.t('Settings.pin')}>
    <ChangePinForm />
  </Card>

  <Card title={i18n.t('Settings.biometricUnlock')}>
    {#if probing}
      <div class="flex items-center gap-2 text-sm text-(--color-text-secondary)">
        <Spinner size="sm" />
        {i18n.t('Settings.checkingDevice')}
      </div>
    {:else if !available}
      <p class="text-sm text-(--color-text-secondary)">
        {i18n.t('Settings.noBiometricHw')}
      </p>
    {:else}
      <Toggle
        checked={enabled}
        disabled={busy}
        label={i18n.t('Settings.unlockWithBiometric')}
        description={i18n.t('Settings.unlockWithBiometricHint')}
        onchange={onToggle}
      />
    {/if}
  </Card>
</div>

<BiometricPrompt open={busy} reason={i18n.t('Settings.biometricVerifyReason')} />
