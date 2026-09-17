<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Full-screen overlay shown while a biometric verification is in flight.
  The actual Windows Hello / Touch ID dialog is driven by the Tauri shell
  (backend → BiometricRequest channel); the frontend only waits for the
  POST /auth/unlock-biometric response and shows this while it pends.
-->
<script lang="ts">
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { Fingerprint } from 'lucide-svelte'

  interface Props {
    open: boolean
    reason?: string
  }

  let { open, reason }: Props = $props()
  const displayReason = $derived(reason ?? i18n.t('Settings.verifyingIdentity'))
</script>

{#if open}
  <div
    class="fixed inset-0 z-[70] flex items-center justify-center
           bg-black/50 backdrop-blur-sm"
    role="alertdialog"
    aria-label={i18n.t('Settings.biometricVerifyAria')}
    aria-busy="true"
  >
    <div
      class="flex flex-col items-center gap-4 px-8 py-7
             bg-(--color-surface-0) rounded-(--radius-lg) shadow-(--shadow-modal)"
    >
      <Fingerprint size={40} class="text-(--color-brand-500)" aria-hidden="true" />
      <!--
        Hand-drawn 7-path SVG used to live here. It looked off-centre and
        hairline-thin against the brand background; lucide's Fingerprint
        is the icon Windows Hello / Apple Touch ID users instinctively
        recognise. Same `currentColor` model, so the wrapping
        `text-(--color-brand-500)` still tints it.
      -->
      <p class="text-sm font-medium text-(--color-text-primary) text-center max-w-xs">
        {displayReason}
      </p>
      <div class="text-(--color-brand-500)">
        <Spinner size="sm" />
      </div>
      <p class="text-xs text-(--color-text-secondary) text-center">
        {i18n.t('Settings.followSystemPrompt')}
      </p>
    </div>
  </div>
{/if}
