<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Domain toggles. Lets the user hide professional verticals they don't
  use from the rest of the app. At least one domain must stay enabled
  — the last toggle is locked. Persisted server-side via
  PUT /user/enabled-domains (NULL = "every domain enabled" sentinel).
-->
<script lang="ts">
  import Card from '$lib/components/ui/Card.svelte'
  import Toggle from '$lib/components/ui/Toggle.svelte'
  import { userStore } from '$lib/stores/user.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { DOMAINS, domainLabel, type Domain } from '$lib/types/domain'

  // Working copy initialised from the store; resynced when the store
  // load completes (post-hydrate). The actual save fires per-toggle so
  // there is no "dirty + Save" flow to manage.
  let enabled = $state<Set<Domain>>(
    new Set(userStore.effectiveEnabledDomains),
  )

  $effect(() => {
    enabled = new Set(userStore.effectiveEnabledDomains)
  })

  async function toggleDomain(d: Domain, next: boolean) {
    const draft = new Set(enabled)
    if (next) draft.add(d)
    else draft.delete(d)
    if (draft.size === 0) {
      toastStore.warning(i18n.t('Settings.atLeastOneDomain'))
      enabled = new Set(enabled)
      return
    }
    const list = (DOMAINS as readonly Domain[]).filter((id) => draft.has(id))
    try {
      await userStore.setEnabledDomains(list)
      enabled = draft
      toastStore.success(i18n.t('Settings.enabledDomainsSaved'))
    } catch (e) {
      toastStore.danger(i18n.t('Settings.enabledDomainsError'), {
        detail: (e as Error).message,
      })
      enabled = new Set(enabled)
    }
  }
</script>

<div class="space-y-4">
  <Card title={i18n.t('Settings.domains')}>
    <p class="text-sm text-(--color-text-secondary) mb-4">
      {i18n.t('Settings.domainsHint')}
    </p>
    <div class="grid grid-cols-1 sm:grid-cols-2 gap-y-3 gap-x-6">
      {#each DOMAINS as d (d)}
        <Toggle
          checked={enabled.has(d)}
          label={domainLabel(d)}
          onchange={(v) => toggleDomain(d, v)}
        />
      {/each}
    </div>
  </Card>
</div>
