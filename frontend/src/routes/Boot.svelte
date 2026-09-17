<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Presentational boot screen. The boot sequence itself (port discovery,
  /healthz probe, /auth/status) lives in App.svelte; this just renders
  the connecting spinner or a connection-failed panel.
-->
<script lang="ts">
  import Card from '$lib/components/ui/Card.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'

  interface Props {
    error?: string | null
    onretry: () => void
  }

  let { error = null, onretry }: Props = $props()
</script>

<div class="min-h-full flex items-center justify-center p-8 bg-(--color-surface-50)">
  <div class="w-full max-w-sm">
    {#if error}
      <Card title={i18n.t('Boot.cannotReach')}>
        <div class="space-y-3">
          <p class="text-sm text-(--color-danger-500) font-mono whitespace-pre-wrap">
            {error}
          </p>
          <p class="text-xs text-(--color-text-secondary)">
            {i18n.t('Boot.cannotReachHint')}
            <code class="font-mono">cargo tauri dev --config src-tauri/tauri.svelte.conf.json</code>.
          </p>
          <Button size="sm" variant="secondary" onclick={onretry}>{i18n.t('Common.retry')}</Button>
        </div>
      </Card>
    {:else}
      <div class="flex flex-col items-center gap-4 text-(--color-text-secondary)">
        <div class="text-(--color-brand-500)">
          <Spinner size="lg" />
        </div>
        <p class="text-sm">{i18n.t('Boot.connecting')}</p>
      </div>
    {/if}
  </div>
</div>
