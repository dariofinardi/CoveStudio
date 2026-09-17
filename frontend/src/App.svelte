<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Root: runs the boot sequence (port discovery → /healthz → /auth/status)
  then dispatches on the router store. The full feature router (plan §13)
  arrives with the feature routes; this covers boot/setup/unlock/home.
  `?playground` in the URL short-circuits to the dev component gallery.
-->
<script lang="ts">
  import Boot from './routes/Boot.svelte'
  import Setup from './routes/Setup.svelte'
  import Unlock from './routes/Unlock.svelte'
  import Shell from './routes/Shell.svelte'
  import Playground from './routes/Playground.svelte'
  import ToastRegion from '$lib/components/ui/ToastRegion.svelte'
  import { router, isFeatureRoute } from '$lib/stores/router.svelte'
  import { apiBase } from '$lib/stores/api-base.svelte'
  import { authStore } from '$lib/stores/auth.svelte'
  import { healthApi } from '$lib/api/health'
  import { authApi } from '$lib/api/auth'

  const showPlayground = new URLSearchParams(window.location.search).has('playground')

  let bootError = $state<string | null>(null)
  let knownUsername = $state<string | undefined>(undefined)

  async function boot() {
    bootError = null
    router.go('boot')
    try {
      await apiBase.hydrate()
      // Confirm the backend is actually serving before deciding a route.
      await healthApi.get()

      const status = await authApi.status()
      if (status.setup_required) {
        router.go('setup')
      } else {
        knownUsername = status.user.display_name ?? status.user.username
        authStore.setBiometricEnrolled(status.biometric_enrolled)
        // Token is in-memory only (plan Q10) — always require an unlock
        // on a fresh process start.
        router.go('unlock')
      }
    } catch (err) {
      bootError = (err as Error).message
      router.go('boot')
    }
  }

  $effect(() => {
    if (!showPlayground) void boot()
  })
</script>

{#if showPlayground}
  <Playground />
{:else if router.current === 'boot'}
  <Boot error={bootError} onretry={boot} />
{:else if router.current === 'setup'}
  <Setup />
{:else if router.current === 'unlock'}
  <Unlock username={knownUsername} />
{:else if isFeatureRoute(router.current)}
  <Shell />
{/if}

<ToastRegion />
