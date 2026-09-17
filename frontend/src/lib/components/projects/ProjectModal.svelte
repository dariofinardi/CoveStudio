<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!-- Create / edit a project. `project` non-null switches to edit mode. -->
<script lang="ts">
  import Modal from '$lib/components/ui/Modal.svelte'
  import Input from '$lib/components/ui/Input.svelte'
  import Textarea from '$lib/components/ui/Textarea.svelte'
  import Select from '$lib/components/ui/Select.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import { projectStore } from '$lib/stores/projects.svelte'
  import { userStore } from '$lib/stores/user.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { ApiError } from '$lib/types/error'
  import { DOMAINS, domainLabel, type Domain } from '$lib/types/domain'
  import type { Project } from '$lib/types/project'

  interface Props {
    open?: boolean
    project?: Project | null
    onsuccess: () => void
  }

  let { open = $bindable(false), project = null, onsuccess }: Props = $props()

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)
  const editing = $derived(project !== null)

  let name = $state('')
  let description = $state('')
  let domain = $state<Domain>('legal')
  let submitting = $state(false)
  let formError = $state<string | null>(null)

  const domainOptions = $derived(DOMAINS.map((d) => ({ value: d, label: domainLabel(d) })))
  const canSubmit = $derived(name.trim().length > 0 && !submitting)

  $effect(() => {
    if (open) {
      name = project?.name ?? ''
      description = project?.description ?? ''
      domain = project?.domain ?? userStore.defaultDomain
      formError = null
    }
  })

  async function submit() {
    if (!canSubmit) return
    submitting = true
    formError = null
    try {
      if (project) {
        await projectStore.update(project.id, {
          name: name.trim(),
          description: description.trim(),
          domain,
        })
      } else {
        await projectStore.create({
          name: name.trim(),
          description: description.trim() || undefined,
          domain,
        })
      }
      open = false
      onsuccess()
    } catch (e) {
      formError = e instanceof ApiError ? e.detail : (e as Error).message
    } finally {
      submitting = false
    }
  }
</script>

<Modal bind:open title={editing ? t('Projects.editProject') : t('Projects.newProject')} size="md">
  <div class="space-y-3">
    <Input
      label={t('Projects.projectName')}
      bind:value={name}
      placeholder={t('Projects.projectNamePlaceholder')}
    />
    <Textarea
      label={t('Common.description')}
      bind:value={description}
      placeholder={t('Projects.descriptionPlaceholder')}
      minRows={2}
    />
    <Select label={t('Domains.label')} options={domainOptions} bind:value={domain} class="w-60" />
    {#if formError}
      <p class="text-sm text-(--color-danger-500)">{formError}</p>
    {/if}
  </div>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (open = false)}>{t('Common.cancel')}</Button>
    <Button loading={submitting} disabled={!canSubmit} onclick={submit}>
      {editing ? t('Common.save') : t('Projects.createProject')}
    </Button>
  {/snippet}
</Modal>
