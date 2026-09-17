<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  DOCX-template detail. Shows the authoring contract (auto-generated
  prompt + section skeleton + required metadata), lets the user apply
  the template to a fresh assistant chat, and offers a manual
  render-to-.docx form.
-->
<script lang="ts">
  import Modal from '$lib/components/ui/Modal.svelte'
  import Badge from '$lib/components/ui/Badge.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import Input from '$lib/components/ui/Input.svelte'
  import Textarea from '$lib/components/ui/Textarea.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import { templatesApi } from '$lib/api/templates'
  import { composerPrefill } from '$lib/stores/composer.svelte'
  import { chatStore } from '$lib/stores/chat.svelte'
  import { router } from '$lib/stores/router.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { domainLabel } from '$lib/types/domain'
  import type { DocxTemplate, TemplateDescription } from '$lib/types/template'

  let { templateId, onclose }: { templateId: string | null; onclose: () => void } = $props()

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)

  let desc = $state<TemplateDescription | null>(null)
  let loading = $state(false)
  let error = $state<string | null>(null)

  // render form
  let metadata = $state<Record<string, string>>({})
  let bodyMd = $state('')
  let rendering = $state(false)

  $effect(() => {
    const id = templateId
    if (!id) {
      desc = null
      return
    }
    loading = true
    error = null
    desc = null
    metadata = {}
    bodyMd = ''
    templatesApi
      .describe(id, i18n.locale)
      .then((d) => {
        desc = d
        const init: Record<string, string> = {}
        for (const f of d.sidecar.required_metadata) init[f] = ''
        metadata = init
      })
      .catch((e) => (error = (e as Error).message))
      .finally(() => (loading = false))
  })

  function automationLabel(level: string): string {
    return t(`DocxTemplates.automation${level}`)
  }

  function applyToChat() {
    if (!desc) return
    composerPrefill.queueTemplate({
      id: desc.template_id,
      title: desc.display_name,
    })
    chatStore.newChat()
    router.go('assistant')
    onclose()
  }

  async function render() {
    if (!desc) return
    rendering = true
    try {
      const { blob, unresolved } = await templatesApi.render({
        template_id: desc.template_id,
        body_md: bodyMd,
        metadata,
        filename: `${desc.display_name}.docx`,
      })
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = `${desc.display_name}.docx`
      a.click()
      URL.revokeObjectURL(url)
      if (unresolved.length) {
        toastStore.warning(t('DocxTemplates.unresolvedWarning', { list: unresolved.join(', ') }))
      } else {
        toastStore.success(t('DocxTemplates.rendered'))
      }
    } catch (e) {
      toastStore.danger(t('DocxTemplates.renderError'), { detail: (e as Error).message })
    } finally {
      rendering = false
    }
  }

  const sidecar = $derived<DocxTemplate | null>(desc?.sidecar ?? null)
</script>

<Modal
  open={templateId !== null}
  title={desc?.display_name ?? t('DocxTemplates.detailTitle')}
  size="lg"
  onclose={onclose}
>
  {#if loading}
    <div class="flex items-center gap-2 text-sm text-(--color-text-secondary) py-10 justify-center">
      <Spinner size="sm" />
      {t('Common.loading')}
    </div>
  {:else if error}
    <p class="text-sm text-(--color-danger-500) py-8 text-center">{error}</p>
  {:else if desc && sidecar}
    <div class="space-y-5">
      <!-- meta -->
      <div class="flex flex-wrap items-center gap-2">
        <Badge tone="level" size="xs">{sidecar.automation_level}</Badge>
        <Badge tone="brand">{domainLabel(sidecar.domain)}</Badge>
        {#each sidecar.also_applicable_to as extra (extra)}
          <Badge tone="neutral" size="xs">{t('Ui.alsoDomain', { domain: domainLabel(extra) })}</Badge>
        {/each}
        <span class="text-xs text-(--color-text-secondary) font-mono">{sidecar.id}</span>
      </div>
      <p class="text-xs text-(--color-text-secondary)">{automationLabel(sidecar.automation_level)}</p>

      <!-- section skeleton -->
      {#if sidecar.section_skeleton.length}
        <section class="space-y-1.5">
          <h4 class="text-xs font-semibold text-(--color-text-secondary)">
            {t('DocxTemplates.sectionSkeleton')}
          </h4>
          <ul class="text-sm text-(--color-text-primary) space-y-0.5">
            {#each sidecar.section_skeleton as s (s.id)}
              <li class="flex items-baseline gap-2">
                <span class="text-(--color-text-disabled) text-xs">•</span>
                <span>{s.title ?? s.id}</span>
                {#if s.repeating}
                  <Badge tone="neutral" size="xs">{t('DocxTemplates.repeatingBlock')}</Badge>
                {/if}
              </li>
            {/each}
          </ul>
        </section>
      {/if}

      <!-- apply to chat -->
      <div class="flex">
        <Button onclick={applyToChat}>{t('DocxTemplates.applyToChat')}</Button>
      </div>

      <!-- manual render -->
      <details class="border-t border-(--color-surface-200) pt-3">
        <summary class="text-sm font-medium text-(--color-text-primary) cursor-pointer">
          {t('DocxTemplates.generate')}
        </summary>
        <div class="space-y-3 pt-3">
          {#if sidecar.required_metadata.length}
            <div class="space-y-2">
              <span class="text-xs font-semibold text-(--color-text-secondary)">
                {t('DocxTemplates.metadataFields')}
              </span>
              {#each sidecar.required_metadata as field (field)}
                <Input label={field} bind:value={metadata[field]} />
              {/each}
            </div>
          {/if}
          <Textarea
            label={t('DocxTemplates.bodyMd')}
            bind:value={bodyMd}
            placeholder={t('DocxTemplates.bodyPlaceholder')}
            minRows={5}
          />
          <div class="flex justify-end">
            <Button loading={rendering} onclick={render}>{t('DocxTemplates.renderNow')}</Button>
          </div>
        </div>
      </details>
    </div>
  {/if}
</Modal>
