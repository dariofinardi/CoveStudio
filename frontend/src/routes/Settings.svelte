<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Settings screen. Vertical sub-nav + active section. Profile, Security
  and Danger zone are implemented; Models / MCP / Data sources are
  listed as upcoming sections (their API layers land in later passes).
-->
<script lang="ts">
  import ProfileSection from '$lib/components/settings/ProfileSection.svelte'
  import SecuritySection from '$lib/components/settings/SecuritySection.svelte'
  import ModelsSection from '$lib/components/settings/ModelsSection.svelte'
  import McpSection from '$lib/components/settings/McpSection.svelte'
  import DataSourcesSection from '$lib/components/settings/DataSourcesSection.svelte'
  import DomainsSection from '$lib/components/settings/DomainsSection.svelte'
  import RetrievalSection from '$lib/components/settings/RetrievalSection.svelte'
  import DangerZoneSection from '$lib/components/settings/DangerZoneSection.svelte'
  import LicenseSection from '$lib/components/settings/LicenseSection.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'

  type SectionId =
    | 'profile'
    | 'security'
    | 'models'
    | 'mcp'
    | 'data'
    | 'domains'
    | 'retrieval'
    | 'license'
    | 'danger'

  /**
   * Visual grouping for the sub-nav. Order = render order; a hairline
   * separator is drawn between any two consecutive entries whose `group`
   * field differs. Groups (top to bottom):
   *   - account   — who you are (profile, security)
   *   - content   — what the app shows (domains, data sources)
   *   - ai        — how the assistant reasons (LLM models, MCP servers)
   *   - retrieval — chat-time RAG knobs (HyDE, future: adaptive K, BM25, MMR)
   *   - about     — meta (product identity + licence)
   *   - danger    — destructive, deliberately isolated
   */
  type SectionGroup = 'account' | 'content' | 'ai' | 'retrieval' | 'about' | 'danger'

  interface SectionEntry {
    id: SectionId
    labelKey: string
    group: SectionGroup
    ready: boolean
  }

  const sections: SectionEntry[] = [
    { id: 'profile',  labelKey: 'Account.profile',     group: 'account', ready: true },
    { id: 'security', labelKey: 'Account.security',    group: 'account', ready: true },
    { id: 'domains',  labelKey: 'Settings.domains',    group: 'content', ready: true },
    { id: 'data',     labelKey: 'Settings.dataSources', group: 'content', ready: true },
    { id: 'models',   labelKey: 'Settings.llmModels',  group: 'ai',      ready: true },
    { id: 'mcp',      labelKey: 'Settings.mcpServers', group: 'ai',        ready: true },
    { id: 'retrieval',labelKey: 'Settings.retrieval',  group: 'retrieval', ready: true },
    { id: 'license',  labelKey: 'Settings.license',    group: 'about',     ready: true },
    { id: 'danger',   labelKey: 'Settings.dangerZone', group: 'danger',    ready: true },
  ]

  let active = $state<SectionId>('profile')
</script>

<div class="max-w-4xl mx-auto p-8">
  <h2 class="text-2xl font-semibold text-(--color-text-primary) mb-5">{i18n.t('Common.settings')}</h2>

  <div class="flex gap-8 items-start">
    <nav class="w-44 shrink-0 flex flex-col gap-0.5">
      {#each sections as s, i (s.id)}
        {#if i > 0 && s.group !== sections[i - 1].group}
          <div
            class="h-px my-1.5 mx-3 bg-(--color-surface-200)"
            aria-hidden="true"
          ></div>
        {/if}
        <button
          type="button"
          disabled={!s.ready}
          onclick={() => (active = s.id)}
          class="text-left px-3 h-9 rounded-(--radius-md) text-sm
                 transition-colors duration-(--transition-fast)
                 focus:outline-none focus-visible:ring-2 focus-visible:ring-(--color-brand-500)
                 disabled:opacity-40 disabled:cursor-not-allowed
                 {active === s.id
                   ? 'bg-(--color-active-bg) text-(--color-brand-700) font-medium'
                   : 'text-(--color-text-secondary) hover:bg-(--color-hover-bg) hover:text-(--color-text-primary)'}"
        >
          {i18n.t(s.labelKey)}
          {#if !s.ready}
            <span class="text-[10px] uppercase tracking-wide ml-1">{i18n.t('Ui.soon')}</span>
          {/if}
        </button>
      {/each}
    </nav>

    <div class="flex-1 min-w-0">
      {#if active === 'profile'}
        <ProfileSection />
      {:else if active === 'security'}
        <SecuritySection />
      {:else if active === 'models'}
        <ModelsSection />
      {:else if active === 'mcp'}
        <McpSection />
      {:else if active === 'data'}
        <DataSourcesSection />
      {:else if active === 'domains'}
        <DomainsSection />
      {:else if active === 'retrieval'}
        <RetrievalSection />
      {:else if active === 'license'}
        <LicenseSection />
      {:else if active === 'danger'}
        <DangerZoneSection />
      {:else}
        <EmptyState
          title={i18n.t('Ui.comingSoonShort')}
          description={i18n.t('Ui.comingSoonBody')}
        />
      {/if}
    </div>
  </div>
</div>
