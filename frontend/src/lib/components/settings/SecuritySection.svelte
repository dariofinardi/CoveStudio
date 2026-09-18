<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import Card from '$lib/components/ui/Card.svelte'
  import Toggle from '$lib/components/ui/Toggle.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import Slider from '$lib/components/ui/Slider.svelte'
  import ChangePinForm from './ChangePinForm.svelte'
  import BiometricPrompt from '$lib/components/auth/BiometricPrompt.svelte'
  import { authApi } from '$lib/api/auth'
  import { userApi } from '$lib/api/user'
  import { authStore } from '$lib/stores/auth.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { ApiError } from '$lib/types/error'

  let probing = $state(true)
  let available = $state(false)
  let enabled = $state(false)
  let busy = $state(false)

  // PII detection threshold. Loaded once; saved when the user settles
  // on a value (the Slider commits on `change`, not on every tick).
  let piiThreshold = $state(0.5)
  let piiLoaded = $state(false)
  let piiSaving = $state(false)

  $effect(() => {
    userApi
      .getPiiThreshold()
      .then((r) => {
        piiThreshold = r.pii_threshold
      })
      .catch(() => {
        // Keep the default on failure: the control stays usable and the
        // server clamps whatever it is sent.
      })
      .finally(() => {
        piiLoaded = true
      })
  })

  async function savePiiThreshold(v: number) {
    piiSaving = true
    try {
      const r = await userApi.updatePiiThreshold(v)
      piiThreshold = r.pii_threshold
      toastStore.success(i18n.t('Settings.piiThresholdSaved'))
    } catch {
      toastStore.danger(i18n.t('Settings.piiThresholdSaveError'))
    } finally {
      piiSaving = false
    }
  }

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

  <Card title={i18n.t('Settings.piiTitle')}>
    {#if piiLoaded}
      <Slider
        label={i18n.t('Settings.piiThresholdLabel')}
        bind:value={piiThreshold}
        min={0.05}
        max={0.95}
        step={0.05}
        minLabel={i18n.t('Settings.piiThresholdMinLabel')}
        maxLabel={i18n.t('Settings.piiThresholdMaxLabel')}
        description={i18n.t('Settings.piiThresholdHint')}
        format={(v) => `${Math.round(v * 100)}%`}
        disabled={piiSaving}
        onchange={savePiiThreshold}
      />
    {:else}
      <div class="flex items-center gap-2 text-sm text-(--color-text-secondary)">
        <Spinner size="sm" />
      </div>
    {/if}
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
