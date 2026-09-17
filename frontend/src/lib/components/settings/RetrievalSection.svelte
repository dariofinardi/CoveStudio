<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Settings → Recupero documenti.

  Behavioral knobs for the chat-time RAG pipeline. v0.5.0 ships one
  toggle (HyDE); future RAG features (adaptive top-K, BM25+RRF
  fusion weight, MMR λ) will live in the same panel, each with their
  own on/off switch — see the project-wide rule in
  `feedback_behavioral_toggles.md`.

  All knobs default OFF when they add compute or LLM calls. Changing
  one is immediate; no chat restart needed.
-->
<script lang="ts">
  import Toggle from '$lib/components/ui/Toggle.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { userApi } from '$lib/api/user'

  let loading = $state(true)
  let saving = $state(false)
  let hydeEnabled = $state(false)

  $effect(() => {
    void (async () => {
      try {
        const res = await userApi.getHydeEnabled()
        hydeEnabled = res.hyde_enabled
      } catch (err) {
        toastStore.danger(
          i18n.t('Settings.retrievalLoadError', {
            err: (err as Error).message ?? String(err),
          }),
        )
      } finally {
        loading = false
      }
    })()
  })

  async function toggleHyde(next: boolean) {
    if (saving) return
    saving = true
    const prev = hydeEnabled
    hydeEnabled = next
    try {
      await userApi.updateHydeEnabled(next)
      toastStore.success(
        next
          ? i18n.t('Settings.retrievalHydeEnabledToast')
          : i18n.t('Settings.retrievalHydeDisabledToast'),
      )
    } catch (err) {
      hydeEnabled = prev
      toastStore.danger(
        i18n.t('Settings.retrievalSaveError', {
          err: (err as Error).message ?? String(err),
        }),
      )
    } finally {
      saving = false
    }
  }
</script>

<section class="space-y-5">
  <header>
    <h3 class="text-base font-semibold text-(--color-text-primary)">
      {i18n.t('Settings.retrievalTitle')}
    </h3>
    <p class="mt-1 text-sm text-(--color-text-secondary)">
      {i18n.t('Settings.retrievalIntro')}
    </p>
  </header>

  {#if loading}
    <div class="flex items-center gap-2 text-sm text-(--color-text-secondary)">
      <Spinner size="sm" />
      <span>{i18n.t('Common.loading')}</span>
    </div>
  {:else}
    <div
      class="rounded-(--radius-md) border border-(--color-surface-200)
             bg-(--color-surface-0) p-4"
    >
      <Toggle
        checked={hydeEnabled}
        label={i18n.t('Settings.retrievalHydeLabel')}
        description={i18n.t('Settings.retrievalHydeDescription')}
        disabled={saving}
        onchange={toggleHyde}
      />
      <p
        class="mt-3 text-[11px] text-(--color-text-secondary)
               flex items-start gap-1.5"
      >
        <span class="text-(--color-warning-500) shrink-0">●</span>
        <span>{i18n.t('Settings.retrievalHydeCost')}</span>
      </p>
    </div>
  {/if}
</section>
