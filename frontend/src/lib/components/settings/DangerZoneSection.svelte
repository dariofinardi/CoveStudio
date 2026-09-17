<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import Card from '$lib/components/ui/Card.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import ConfirmDialog from '$lib/components/ui/ConfirmDialog.svelte'
  import { userApi } from '$lib/api/user'
  import { authStore } from '$lib/stores/auth.svelte'
  import { userStore } from '$lib/stores/user.svelte'
  import { router } from '$lib/stores/router.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'

  let confirmOpen = $state(false)

  async function deleteAccount() {
    try {
      await userApi.deleteAccount()
      authStore.invalidate()
      userStore.reset()
      toastStore.info(i18n.t('Settings.accountDeleted'))
      router.go('setup')
    } catch (e) {
      toastStore.danger(i18n.t('Settings.deleteAccountError'), { detail: (e as Error).message })
    }
  }
</script>

<Card title={i18n.t('Settings.dangerZone')}>
  <div class="flex items-center justify-between gap-4">
    <div class="space-y-0.5">
      <p class="text-sm font-medium text-(--color-text-primary)">{i18n.t('Settings.deleteAccount')}</p>
      <p class="text-xs text-(--color-text-secondary)">
        {i18n.t('Settings.deleteAccountHint')}
      </p>
    </div>
    <Button variant="danger" onclick={() => (confirmOpen = true)}>
      {i18n.t('Settings.deleteAccount')}
    </Button>
  </div>
</Card>

<ConfirmDialog
  bind:open={confirmOpen}
  title={i18n.t('Settings.deleteAccountConfirmTitle')}
  message={i18n.t('Settings.deleteAccountConfirmBody')}
  confirmLabel={i18n.t('Settings.deleteEverything')}
  danger
  onconfirm={deleteAccount}
/>
