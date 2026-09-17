<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import Card from '$lib/components/ui/Card.svelte'
  import SetupForm from '$lib/components/auth/SetupForm.svelte'
  import { router } from '$lib/stores/router.svelte'
  import { userStore } from '$lib/stores/user.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import type { SessionUser } from '$lib/types/auth'

  async function onSetupDone(user: SessionUser) {
    toastStore.success(i18n.t('Auth.welcome', { name: user.display_name ?? user.username }))
    // Setup returns a live token — hydrate preferences then enter the app.
    try {
      await userStore.hydrate()
    } catch {
      // non-fatal: defaults are fine, Settings can fix later
    }
    router.go('assistant')
  }
</script>

<div class="min-h-full flex items-center justify-center p-8 bg-(--color-surface-50)">
  <div class="w-full max-w-sm space-y-6">
    <header class="text-center space-y-1">
      <h1 class="text-2xl font-semibold text-(--color-text-primary)">{i18n.t('Auth.setupTitle')}</h1>
      <p class="text-sm text-(--color-text-secondary)">
        {i18n.t('Auth.setupSubtitle')}
      </p>
    </header>

    <Card>
      <SetupForm onsuccess={onSetupDone} />
    </Card>

    <p class="text-xs text-(--color-text-secondary) text-center">
      {i18n.t('Auth.setupPinNote')}
    </p>
  </div>
</div>
