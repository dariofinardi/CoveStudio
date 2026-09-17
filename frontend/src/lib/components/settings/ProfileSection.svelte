<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import Card from '$lib/components/ui/Card.svelte'
  import Input from '$lib/components/ui/Input.svelte'
  import Select from '$lib/components/ui/Select.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import { userStore } from '$lib/stores/user.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { DOMAINS, domainLabel } from '$lib/types/domain'
  import { LOCALES, LOCALE_LABELS, type Locale } from '$lib/types/user'
  import type { Domain } from '$lib/types/domain'

  let displayName = $state(userStore.profile?.display_name ?? '')
  let savingName = $state(false)

  const nameDirty = $derived(
    displayName.trim() !== (userStore.profile?.display_name ?? '')
  )

  const localeOptions = LOCALES.map((l) => ({ value: l, label: LOCALE_LABELS[l] }))
  const domainOptions = DOMAINS.map((d) => ({ value: d, label: domainLabel(d) }))

  async function saveName() {
    savingName = true
    try {
      await userStore.setDisplayName(displayName.trim() || null)
      toastStore.success(i18n.t('Settings.displayNameSaved'))
    } catch (e) {
      toastStore.danger(i18n.t('Settings.displayNameError'), { detail: (e as Error).message })
    } finally {
      savingName = false
    }
  }

  async function changeLocale(next: Locale) {
    try {
      await userStore.setLocale(next)
      toastStore.success(i18n.t('Account.savedLanguage'))
    } catch (e) {
      toastStore.danger(i18n.t('Settings.languageError'), { detail: (e as Error).message })
    }
  }

  async function changeDomain(next: Domain) {
    try {
      await userStore.setDefaultDomain(next)
      toastStore.success(i18n.t('Settings.domainSaved'))
    } catch (e) {
      toastStore.danger(i18n.t('Settings.domainError'), { detail: (e as Error).message })
    }
  }
</script>

<div class="space-y-4">
  <Card title={i18n.t('Account.profile')}>
    <div class="space-y-4">
      <div class="grid grid-cols-[140px_1fr] items-center gap-3">
        <span class="text-sm text-(--color-text-secondary)">{i18n.t('Account.username')}</span>
        <span class="text-sm font-mono text-(--color-text-primary)">
          {userStore.profile?.username ?? '—'}
        </span>
      </div>

      <div class="flex items-end gap-2">
        <Input
          label={i18n.t('Account.displayName')}
          bind:value={displayName}
          placeholder={i18n.t('Account.displayNamePlaceholder')}
          class="flex-1"
        />
        <Button size="md" disabled={!nameDirty} loading={savingName} onclick={saveName}>
          {i18n.t('Common.save')}
        </Button>
      </div>

      {#if userStore.profile?.created_at}
        <div class="grid grid-cols-[140px_1fr] items-center gap-3">
          <span class="text-sm text-(--color-text-secondary)">{i18n.t('Settings.createdLabel')}</span>
          <span class="text-sm text-(--color-text-primary)">
            {new Date(userStore.profile.created_at).toLocaleDateString()}
          </span>
        </div>
      {/if}
    </div>
  </Card>

  <Card title={i18n.t('Settings.preferences')}>
    <div class="grid grid-cols-2 gap-4">
      <Select
        label={i18n.t('Account.language')}
        options={localeOptions}
        value={userStore.locale}
        onchange={(e) => changeLocale((e.currentTarget as HTMLSelectElement).value as Locale)}
      />
      <Select
        label={i18n.t('Settings.defaultDomain')}
        options={domainOptions}
        value={userStore.defaultDomain}
        onchange={(e) => changeDomain((e.currentTarget as HTMLSelectElement).value as Domain)}
      />
    </div>
    <p class="text-xs text-(--color-text-secondary) mt-3">
      {i18n.t('Settings.defaultDomainHint')}
    </p>
  </Card>
</div>
