<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Model-status banner. Visible only while the chat is waiting for a
  reply AND one of the heavy local models (embeddings or GLiNER2 PII)
  is busy: downloading, loading the session, or running a pass.
  Invisible in the steady state.
-->
<script lang="ts">
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import {
    syncApi,
    eurlexApi,
    type ModelStatus,
    type EmbedProgress,
    type NerStatus,
  } from '$lib/api/data-sources'
  import { i18n } from '$lib/stores/i18n.svelte'

  let { active }: { active: boolean } = $props()

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)

  let model = $state<ModelStatus | null>(null)
  let progress = $state<EmbedProgress | null>(null)
  let ner = $state<NerStatus | null>(null)
  let timer: ReturnType<typeof setInterval> | undefined

  async function poll() {
    try {
      const [m, p, n] = await Promise.all([
        syncApi.modelStatus(),
        eurlexApi.embedProgress(),
        syncApi.nerStatus(),
      ])
      model = m
      progress = p
      ner = n
    } catch {
      /* transient — keep last snapshot */
    }
  }

  $effect(() => {
    clearInterval(timer)
    if (active) {
      void poll()
      timer = setInterval(poll, 600)
    } else {
      model = null
      progress = null
      ner = null
    }
    return () => clearInterval(timer)
  })

  // Priority: GLiNER2 first when the user explicitly asked for PII
  // redaction this turn — it gates the LLM call, so showing its
  // lifecycle matters more than the (often parallel) embedding work.
  // Otherwise fall back to the embedding-pipeline status.
  const message = $derived.by(() => {
    if (ner?.state === 'downloading') {
      const mb = Math.round((ner.downloaded / 1_048_576) * 10) / 10
      if (ner.total && ner.total > 0) {
        const totalMb = Math.round((ner.total / 1_048_576) * 10) / 10
        return `${t('NerStatus.downloading', { file: ner.file })} (${mb}/${totalMb} MB)`
      }
      return `${t('NerStatus.downloading', { file: ner.file })} (${mb} MB)`
    }
    if (ner?.state === 'loading') return t('NerStatus.loadingModel')
    if (ner?.state === 'failed') return t('NerStatus.failed', { error: ner.error })
    if (model?.state === 'downloading') {
      const mb = Math.round((model.downloaded / 1_048_576) * 10) / 10
      const totalMb = Math.round((model.total / 1_048_576) * 10) / 10
      return `${t('EmbeddingStatus.downloadingTitle')} (${mb}/${totalMb} MB)`
    }
    if (model?.state === 'loading') return t('EmbeddingStatus.loadingModelTitle')
    if (progress && progress.total > 0) {
      return `${t('EmbeddingStatus.embeddingTitle')} ${progress.current}/${progress.total}`
    }
    return null
  })
</script>

{#if active && message}
  <div class="flex items-center gap-2 px-3 py-1.5 mb-2 rounded-(--radius-md)
              bg-(--color-surface-100) text-xs text-(--color-text-secondary)">
    <Spinner size="sm" />
    <span>{message}</span>
  </div>
{/if}
