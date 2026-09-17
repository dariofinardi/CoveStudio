<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Context window of a model, shown under the model pickers in Settings.
  Asks `GET /models/context-window`: for local Ollama models the value
  comes from the server (loaded model, Modelfile num_ctx, model maximum),
  for the others from the model catalogue or a built-in estimate.
-->
<script lang="ts">
  import { api } from '$lib/api/client'
  import { i18n } from '$lib/stores/i18n.svelte'

  interface Report {
    tokens: number
    source: 'server' | 'server_error' | 'catalogue' | 'default'
    server_reachable: boolean
    loaded: boolean
    modelfile_num_ctx: number | null
    model_max: number | null
  }

  interface Props {
    /** Model id as stored in the settings (`local:…`, `openai:…`, …). */
    model: string
    /** Local base URL from the form, when it may not be saved yet. */
    baseUrl?: string
    /** Secure local mode from the form. */
    secure?: boolean
  }

  let { model, baseUrl = '', secure = false }: Props = $props()

  let report = $state<Report | null>(null)
  let loading = $state(false)
  let requestSeq = 0

  const sourceKey: Record<Report['source'], string> = {
    server: 'Settings.contextWindowSourceServer',
    server_error: 'Settings.contextWindowSourceServerError',
    catalogue: 'Settings.contextWindowSourceCatalogue',
    default: 'Settings.contextWindowSourceDefault',
  }

  function fmt(n: number): string {
    return new Intl.NumberFormat(i18n.locale).format(n)
  }

  $effect(() => {
    const id = model.trim()
    const base = baseUrl.trim()
    const isSecure = secure
    report = null
    if (!id) return
    const seq = ++requestSeq
    // Debounced: the local base URL and model names change while typing.
    const timer = setTimeout(async () => {
      loading = true
      try {
        const params = new URLSearchParams({ model: id })
        if (id.startsWith('local:')) {
          if (base) params.set('base_url', base)
          params.set('secure', String(isSecure))
        }
        const res = await api<{ report: Report }>(`/models/context-window?${params.toString()}`)
        if (seq === requestSeq) report = res.report
      } catch {
        if (seq === requestSeq) report = null
      } finally {
        if (seq === requestSeq) loading = false
      }
    }, 400)
    return () => clearTimeout(timer)
  })

  const details = $derived.by(() => {
    if (!report) return [] as string[]
    const out: string[] = []
    if (report.model_max) out.push(i18n.t('Settings.contextWindowModelMax', { max: fmt(report.model_max) }))
    if (report.modelfile_num_ctx) {
      out.push(i18n.t('Settings.contextWindowModelfile', { n: fmt(report.modelfile_num_ctx) }))
    }
    if (report.server_reachable && !report.loaded && report.source !== 'server') {
      out.push(i18n.t('Settings.contextWindowNotLoaded'))
    }
    return out
  })
</script>

{#if loading && !report}
  <p class="mt-1 text-xs text-(--color-text-disabled)">{i18n.t('Settings.contextWindowLoading')}</p>
{:else if report}
  <p class="mt-1 text-xs text-(--color-text-secondary)">
    {i18n.t('Settings.contextWindowLabel', { tokens: fmt(report.tokens) })}
    <span class="text-(--color-text-disabled)">· {i18n.t(sourceKey[report.source])}</span>
  </p>
  {#if details.length > 0}
    <p class="text-xs text-(--color-text-disabled)">{details.join(' · ')}</p>
  {/if}
{/if}
