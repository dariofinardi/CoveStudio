<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Projects screen. List + create + edit + delete over /project.
  Export / import of encrypted project files.
-->
<script lang="ts">
  import { PROJECT_FILE_ACCEPT, isProjectFileName, projectFileName } from '$lib/product'
  import Badge from '$lib/components/ui/Badge.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import IconButton from '$lib/components/ui/IconButton.svelte'
  import Input from '$lib/components/ui/Input.svelte'
  import Select from '$lib/components/ui/Select.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import ConfirmDialog from '$lib/components/ui/ConfirmDialog.svelte'
  import Modal from '$lib/components/ui/Modal.svelte'
  import ProjectModal from '$lib/components/projects/ProjectModal.svelte'
  import ProjectDetail from '$lib/components/projects/ProjectDetail.svelte'
  import { projectStore } from '$lib/stores/projects.svelte'
  import { projectsApi } from '$lib/api/projects'
  import { router } from '$lib/stores/router.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { DOMAINS, domainLabel } from '$lib/types/domain'
  import type { Project } from '$lib/types/project'
  import Checkbox from '$lib/components/ui/Checkbox.svelte'
  import { Search, Pencil, Trash2, Upload, Download } from 'lucide-svelte'

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)

  let search = $state('')
  let domainFilter = $state('')
  let modalOpen = $state(false)
  let editTarget = $state<Project | null>(null)
  let deleteTarget = $state<Project | null>(null)
  let detailId = $state<string | null>(null)

  // ── project file import (drag & drop OR explicit Import button) ──────
  let dragActive = $state(false)
  let importFile = $state<File | null>(null)
  let importEmail = $state('')
  let importing = $state(false)
  let importInputEl: HTMLInputElement | undefined = $state()

  // ── per-row export (mirror of the modal in ProjectDetail.svelte) ──
  let exportTarget = $state<Project | null>(null)
  let exportEmail = $state('')
  let exportChats = $state(false)
  let exporting = $state(false)

  function openExport(p: Project) {
    exportTarget = p
    exportEmail = ''
    exportChats = false
  }

  async function runExport() {
    if (!exportTarget || !exportEmail.trim()) return
    exporting = true
    try {
      const blob = await projectsApi.exportProject(
        exportTarget.id,
        exportEmail.trim(),
        exportChats,
      )
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = projectFileName(exportTarget.name)
      a.click()
      URL.revokeObjectURL(url)
      toastStore.success(t('ProjectExport.downloadStarted'))
      exportTarget = null
    } catch (e) {
      toastStore.danger(t('ProjectExport.errorExport'), { detail: (e as Error).message })
    } finally {
      exporting = false
    }
  }

  function onPickImport(ev: Event) {
    const target = ev.target as HTMLInputElement
    const file = target.files?.[0] ?? null
    if (file && isProjectFileName(file.name)) {
      importFile = file
      importEmail = ''
    } else if (file) {
      toastStore.danger(t('ProjectImport.errorImport'))
    }
    // Reset so re-picking the same file fires `change` again.
    if (importInputEl) importInputEl.value = ''
  }

  function onDragOver(e: DragEvent) {
    if (e.dataTransfer?.types.includes('Files')) {
      e.preventDefault()
      dragActive = true
    }
  }
  function onDragLeave(e: DragEvent) {
    if (e.currentTarget === e.target) dragActive = false
  }
  function onDrop(e: DragEvent) {
    e.preventDefault()
    dragActive = false
    const file = e.dataTransfer?.files?.[0]
    if (file && isProjectFileName(file.name)) {
      importFile = file
      importEmail = ''
    } else if (file) {
      toastStore.danger(t('ProjectImport.errorImport'))
    }
  }

  async function runImport() {
    if (!importFile || !importEmail.trim()) return
    importing = true
    try {
      await projectsApi.importProject(importFile, importEmail.trim())
      toastStore.success(t('ProjectImport.imported'))
      importFile = null
      void projectStore.refresh()
    } catch (e) {
      toastStore.danger(t('ProjectImport.errorImport'), { detail: (e as Error).message })
    } finally {
      importing = false
    }
  }

  $effect(() => {
    // Track the router's nav-tick so this effect re-fires not just
    // on mount but also on RE-navigation to /projects (e.g. clicking
    // a different project in the sidebar accordion while already on
    // this screen). Without the tick Svelte short-circuits the
    // `current = 'projects'` re-assignment and the consumePending
    // below never runs the second time.
    void router.navTick
    void projectStore.refresh()
    // Restore a drill-down (e.g. project detail) requested by the
    // previous screen's back action — see router.NavContext.
    const ctx = router.consumePending()
    if (ctx.projectId) detailId = ctx.projectId
  })

  const domainOptions = $derived([
    { value: '', label: t('Domains.filterPlaceholder') },
    ...DOMAINS.map((d) => ({ value: d, label: domainLabel(d) })),
  ])

  const rows = $derived.by<Project[]>(() => {
    const q = search.trim().toLowerCase()
    return projectStore.items.filter((p) => {
      if (domainFilter && p.domain !== domainFilter) return false
      if (q) {
        const hay = `${p.name} ${p.description ?? ''}`.toLowerCase()
        if (!hay.includes(q)) return false
      }
      return true
    })
  })

  function openCreate() {
    editTarget = null
    modalOpen = true
  }
  function openEdit(p: Project) {
    editTarget = p
    modalOpen = true
  }

  async function confirmDelete() {
    if (!deleteTarget) return
    try {
      await projectStore.remove(deleteTarget.id)
      toastStore.info(t('Projects.deletedToast'))
    } catch (e) {
      toastStore.danger(t('Projects.deleteError'), { detail: (e as Error).message })
    } finally {
      deleteTarget = null
    }
  }

  function fmtDate(iso: string): string {
    const d = new Date(iso)
    return Number.isNaN(d.getTime()) ? iso : d.toLocaleDateString()
  }
</script>

{#if detailId}
  <ProjectDetail id={detailId} onback={() => { detailId = null; void projectStore.refresh() }} />
{:else}
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="relative max-w-4xl mx-auto p-8 space-y-5"
  ondragover={onDragOver}
  ondragleave={onDragLeave}
  ondrop={onDrop}
>
  {#if dragActive}
    <div class="absolute inset-4 z-10 flex items-center justify-center rounded-(--radius-lg)
                border-2 border-dashed border-(--color-brand-500) bg-(--color-brand-50)/80
                text-sm font-medium text-(--color-brand-700) pointer-events-none">
      {t('ProjectImport.dropHint')}
    </div>
  {/if}
  <header class="flex items-end justify-between gap-4">
    <div class="space-y-1">
      <h2 class="text-2xl font-semibold text-(--color-text-primary)">{t('Projects.title')}</h2>
      <p class="text-sm text-(--color-text-secondary)">{t('Projects.emptyHint')}</p>
    </div>
    <div class="flex items-center gap-2">
      <Button variant="secondary" onclick={() => importInputEl?.click()}>
        <Upload size={14} class="mr-1.5" />{t('ProjectImport.title')}
      </Button>
      <Button onclick={openCreate}>{t('Projects.newProject')}</Button>
    </div>
  </header>

  <input
    bind:this={importInputEl}
    type="file"
    accept={PROJECT_FILE_ACCEPT}
    hidden
    onchange={onPickImport}
  />

  <div class="flex items-end gap-3">
    <Input bind:value={search} placeholder={t('Projects.searchPlaceholder')} size="sm" class="w-60">
      {#snippet iconBefore()}
        <Search size={14} />
      {/snippet}
    </Input>
    <div class="flex-1"></div>
    <Select options={domainOptions} bind:value={domainFilter} size="sm" class="w-44" />
  </div>

  {#if projectStore.loading}
    <div class="flex items-center gap-2 text-sm text-(--color-text-secondary) py-12 justify-center">
      <Spinner size="sm" />
      {t('Common.loading')}
    </div>
  {:else if projectStore.error}
    <EmptyState title={t('Projects.loadFailed')} description={projectStore.error}>
      {#snippet action()}
        <Button size="sm" variant="secondary" onclick={() => projectStore.refresh()}>
          {t('Common.retry')}
        </Button>
      {/snippet}
    </EmptyState>
  {:else if rows.length === 0}
    <EmptyState title={t('Projects.noProjects')} description={t('Projects.emptyHint')}>
      {#snippet action()}
        <Button size="sm" onclick={openCreate}>{t('Projects.newProject')}</Button>
      {/snippet}
    </EmptyState>
  {:else}
    <ul class="flex flex-col gap-2">
      {#each rows as p (p.id)}
        <li class="flex items-center gap-3 px-4 py-3 bg-(--color-surface-0) border border-(--color-surface-200) rounded-(--radius-md) hover:border-(--color-surface-300)">
          <button type="button" class="flex-1 min-w-0 text-left" onclick={() => (detailId = p.id)}>
            <span class="text-sm font-medium text-(--color-text-primary) truncate">{p.name}</span>
            {#if p.description}
              <p class="text-xs text-(--color-text-secondary) truncate">{p.description}</p>
            {:else}
              <p class="text-xs text-(--color-text-disabled)">
                {t('Ui.createdOn', { date: fmtDate(p.created_at) })}
              </p>
            {/if}
          </button>
          <Badge tone="brand">{domainLabel(p.domain)}</Badge>
          <IconButton label={t('Projects.renameProject')} size="sm" onclick={() => openEdit(p)}>
            <Pencil size={14} />
          </IconButton>
          <IconButton
            label={t('ProjectExport.title')}
            size="sm"
            onclick={() => openExport(p)}
          >
            <Download size={14} />
          </IconButton>
          <IconButton
            label={t('Projects.deleteProject')}
            size="sm"
            variant="danger"
            onclick={() => (deleteTarget = p)}
          >
            <Trash2 size={14} />
          </IconButton>
        </li>
      {/each}
    </ul>
  {/if}
</div>
{/if}

<ProjectModal bind:open={modalOpen} project={editTarget} onsuccess={() => projectStore.refresh()} />

<ConfirmDialog
  open={deleteTarget !== null}
  title={t('Projects.deleteConfirmTitle')}
  message={t('Projects.deleteConfirmBody', { name: deleteTarget?.name ?? '' })}
  confirmLabel={t('Common.delete')}
  danger
  onconfirm={confirmDelete}
  oncancel={() => (deleteTarget = null)}
/>

<!-- project file import (after drag & drop) -->
<Modal
  open={importFile !== null}
  title={t('ProjectImport.title')}
  size="md"
  onclose={() => (importFile = null)}
>
  <div class="space-y-3">
    <p class="text-sm text-(--color-text-secondary)">{t('ProjectImport.subtitle')}</p>
    {#if importFile}
      <p class="text-sm font-medium text-(--color-text-primary)">{importFile.name}</p>
    {/if}
    <Input
      label={t('ProjectImport.yourEmail')}
      bind:value={importEmail}
      placeholder={t('ProjectExport.recipientEmailPlaceholder')}
      type="email"
    />
    <p class="text-xs text-(--color-text-secondary)">{t('ProjectImport.yourEmailHint')}</p>
  </div>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (importFile = null)}>{t('Common.cancel')}</Button>
    <Button loading={importing} disabled={!importEmail.trim()} onclick={runImport}>
      {t('ProjectImport.importNow')}
    </Button>
  {/snippet}
</Modal>

<!-- per-row project file export (mirror of the modal in ProjectDetail.svelte) -->
<Modal
  open={exportTarget !== null}
  title={t('ProjectExport.title')}
  size="md"
  onclose={() => (exportTarget = null)}
>
  <div class="space-y-3">
    <p class="text-sm text-(--color-text-secondary)">{t('ProjectExport.subtitle')}</p>
    {#if exportTarget}
      <p class="text-sm font-medium text-(--color-text-primary)">{exportTarget.name}</p>
    {/if}
    <Input
      label={t('ProjectExport.recipientEmail')}
      bind:value={exportEmail}
      placeholder={t('ProjectExport.recipientEmailPlaceholder')}
      type="email"
    />
    <Checkbox label={t('ProjectExport.includeChats')} bind:checked={exportChats} />
    <p class="text-xs text-(--color-text-secondary)">{t('ProjectExport.includeChatsHint')}</p>
  </div>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (exportTarget = null)}>{t('Common.cancel')}</Button>
    <Button loading={exporting} disabled={!exportEmail.trim()} onclick={runExport}>
      {t('ProjectExport.exportNow')}
    </Button>
  {/snippet}
</Modal>
