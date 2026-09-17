<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Project documents tab — a folder tree with drag-and-drop.

  Folders nest via parent_id; documents sit in a folder (or at the root
  when project_folder_id is null). The tree is flattened to a single
  list with a per-row depth so drag-and-drop has flat, non-overlapping
  drop targets. Dragging a document onto a folder moves it; dragging a
  folder onto a folder re-parents it (the backend rejects cycles).
-->
<script lang="ts">
  import Button from '$lib/components/ui/Button.svelte'
  import IconButton from '$lib/components/ui/IconButton.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import ConfirmDialog from '$lib/components/ui/ConfirmDialog.svelte'
  import { projectsApi, type ProjectFolder } from '$lib/api/projects'
  import { documentsApi } from '$lib/api/documents'
  import { docViewer } from '$lib/stores/doc-viewer.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import type { DocumentMeta } from '$lib/types/document'
  import {
    Folder, FolderOpen, FolderPlus, FileText, ChevronRight, ChevronDown,
    Upload, Pencil, Trash2, Check, X, AlertCircle,
  } from 'lucide-svelte'

  let { projectId }: { projectId: string } = $props()
  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)

  let folders = $state<ProjectFolder[]>([])
  let docs = $state<DocumentMeta[]>([])
  let loading = $state(true)
  let uploading = $state(false)
  let expandedIds = $state<string[]>([])
  let fileInput = $state<HTMLInputElement>()

  let renamingFolder = $state<string | null>(null)
  let renamingDoc = $state<string | null>(null)
  let renameValue = $state('')
  let deleteFolderTarget = $state<ProjectFolder | null>(null)
  let deleteDocTarget = $state<DocumentMeta | null>(null)

  /** What is currently being dragged, and which row is hovered. */
  let dragItem = $state<{ kind: 'folder' | 'doc'; id: string } | null>(null)
  let dragOver = $state<string | null>(null) // folder id, or 'root'

  async function load() {
    loading = true
    try {
      const [f, d] = await Promise.all([
        projectsApi.listFolders(projectId),
        documentsApi.list({ project_id: projectId }),
      ])
      folders = f.folders
      docs = d.documents
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    } finally {
      loading = false
    }
  }
  $effect(() => {
    void projectId
    void load()
  })

  // ── tree flattening ─────────────────────────────────────────────────
  type Row =
    | { type: 'folder'; folder: ProjectFolder; depth: number }
    | { type: 'doc'; doc: DocumentMeta; depth: number }

  const isExpanded = (id: string) => expandedIds.includes(id)
  const childFolders = (parentId: string | null) =>
    folders
      .filter((f) => f.parent_id === parentId)
      .sort((a, b) => a.name.localeCompare(b.name))
  const folderDocs = (folderId: string | null) =>
    docs
      .filter((d) => (d.project_folder_id ?? null) === folderId)
      .sort((a, b) => a.filename.localeCompare(b.filename))

  const rows = $derived.by(() => {
    const out: Row[] = []
    const walk = (parentId: string | null, depth: number) => {
      for (const f of childFolders(parentId)) {
        out.push({ type: 'folder', folder: f, depth })
        if (isExpanded(f.id)) {
          walk(f.id, depth + 1)
          for (const d of folderDocs(f.id)) {
            out.push({ type: 'doc', doc: d, depth: depth + 1 })
          }
        }
      }
    }
    walk(null, 0)
    for (const d of folderDocs(null)) out.push({ type: 'doc', doc: d, depth: 0 })
    return out
  })

  function toggle(id: string) {
    expandedIds = isExpanded(id)
      ? expandedIds.filter((x) => x !== id)
      : [...expandedIds, id]
  }

  // ── uploads ─────────────────────────────────────────────────────────
  async function onFilesChosen(e: Event) {
    const input = e.currentTarget as HTMLInputElement
    const chosen = Array.from(input.files ?? [])
    input.value = ''
    if (chosen.length === 0) return
    uploading = true
    try {
      for (const f of chosen) await documentsApi.upload(f, { projectId })
      await load()
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    } finally {
      uploading = false
    }
  }

  // ── folder CRUD ─────────────────────────────────────────────────────
  async function newFolder(parentId: string | null) {
    try {
      const r = await projectsApi.createFolder(projectId, t('Projects.newFolder'), parentId)
      if (parentId && !isExpanded(parentId)) expandedIds = [...expandedIds, parentId]
      await load()
      renamingFolder = r.id
      renameValue = t('Projects.newFolder')
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    }
  }

  async function commitFolderRename() {
    const id = renamingFolder
    const name = renameValue.trim()
    renamingFolder = null
    if (!id || !name) return
    try {
      await projectsApi.updateFolder(projectId, id, { name })
      const f = folders.find((x) => x.id === id)
      if (f) f.name = name
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    }
  }

  async function confirmDeleteFolder() {
    const target = deleteFolderTarget
    deleteFolderTarget = null
    if (!target) return
    try {
      await projectsApi.deleteFolder(projectId, target.id)
      await load()
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    }
  }

  // ── document rename / delete ────────────────────────────────────────
  async function commitDocRename() {
    const id = renamingDoc
    const name = renameValue.trim()
    renamingDoc = null
    if (!id || !name) return
    try {
      await projectsApi.renameDocument(projectId, id, name)
      const d = docs.find((x) => x.id === id)
      if (d) d.filename = name
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    }
  }

  async function confirmDeleteDoc() {
    const target = deleteDocTarget
    deleteDocTarget = null
    if (!target) return
    try {
      await documentsApi.remove(target.id)
      docs = docs.filter((d) => d.id !== target.id)
    } catch (e) {
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    }
  }

  // ── drag and drop ───────────────────────────────────────────────────
  /** Drop the dragged item into `targetFolderId` (null = project root). */
  async function drop(targetFolderId: string | null) {
    const item = dragItem
    dragItem = null
    dragOver = null
    if (!item) return
    try {
      if (item.kind === 'doc') {
        await projectsApi.moveDocument(projectId, item.id, targetFolderId)
      } else {
        if (item.id === targetFolderId) return // onto itself — no-op
        await projectsApi.updateFolder(projectId, item.id, {
          parent_id: targetFolderId,
        })
      }
      await load()
    } catch (e) {
      // Backend rejects cycles / bad moves with a 400 — surface it.
      toastStore.danger(t('Errors.somethingWrong'), { detail: (e as Error).message })
    }
  }
</script>

<div class="space-y-3">
  <div class="flex justify-end gap-2">
    <input type="file" multiple class="hidden" bind:this={fileInput} onchange={onFilesChosen} />
    <Button size="sm" variant="secondary" onclick={() => newFolder(null)}>
      <FolderPlus size={14} class="mr-1" />{t('Projects.newFolder')}
    </Button>
    <Button size="sm" loading={uploading} onclick={() => fileInput?.click()}>
      <Upload size={14} class="mr-1" />{t('Projects.uploadFiles')}
    </Button>
  </div>

  {#if loading}
    <div class="flex justify-center py-8"><Spinner size="sm" /></div>
  {:else if rows.length === 0}
    <EmptyState title={t('Documents.noDocuments')} description={t('Projects.emptyHint')} />
  {:else}
    <!-- The container is the project-root drop target. Folder/doc rows
         stopPropagation on their own drop so this fires only for the
         empty space around them. -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <ul
      class="flex flex-col gap-0.5 rounded-(--radius-md) border p-1
             {dragOver === 'root'
               ? 'border-(--color-brand-500) bg-(--color-brand-50)'
               : 'border-(--color-surface-200)'}"
      ondragover={(e) => {
        e.preventDefault()
        dragOver = 'root'
      }}
      ondragleave={() => (dragOver === 'root' ? (dragOver = null) : null)}
      ondrop={(e) => {
        e.preventDefault()
        void drop(null)
      }}
    >
      {#each rows as row (row.type + (row.type === 'folder' ? row.folder.id : row.doc.id))}
        {#if row.type === 'folder'}
          {@const f = row.folder}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <li
            class="group flex items-center gap-1.5 px-2 py-1.5 rounded-(--radius-sm)
                   {dragOver === f.id
                     ? 'bg-(--color-brand-100) ring-1 ring-(--color-brand-500)'
                     : 'hover:bg-(--color-hover-bg)'}"
            style="margin-left: {row.depth * 1.1}rem"
            draggable={renamingFolder !== f.id}
            ondragstart={(e) => {
              e.stopPropagation()
              dragItem = { kind: 'folder', id: f.id }
            }}
            ondragover={(e) => {
              e.preventDefault()
              e.stopPropagation()
              dragOver = f.id
            }}
            ondragleave={() => (dragOver === f.id ? (dragOver = null) : null)}
            ondrop={(e) => {
              e.preventDefault()
              e.stopPropagation()
              void drop(f.id)
            }}
          >
            <button
              type="button"
              class="shrink-0 text-(--color-text-secondary)"
              aria-label={t('Common.expand')}
              onclick={() => toggle(f.id)}
            >
              {#if isExpanded(f.id)}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}
            </button>
            {#if isExpanded(f.id)}
              <FolderOpen size={15} class="shrink-0 text-(--color-brand-600)" />
            {:else}
              <Folder size={15} class="shrink-0 text-(--color-brand-600)" />
            {/if}

            {#if renamingFolder === f.id}
              <input
                bind:value={renameValue}
                class="flex-1 min-w-0 text-sm bg-transparent border-b border-(--color-brand-500) focus:outline-none"
                onkeydown={(e) => {
                  if (e.key === 'Enter') commitFolderRename()
                  if (e.key === 'Escape') (renamingFolder = null)
                }}
              />
              <IconButton label={t('Common.save')} size="sm" onclick={commitFolderRename}>
                <Check size={13} />
              </IconButton>
              <IconButton label={t('Common.cancel')} size="sm" onclick={() => (renamingFolder = null)}>
                <X size={13} />
              </IconButton>
            {:else}
              <button
                type="button"
                class="flex-1 min-w-0 text-left text-sm font-medium text-(--color-text-primary) truncate"
                onclick={() => toggle(f.id)}
              >
                {f.name}
              </button>
              <div class="flex items-center gap-0.5 opacity-0 group-hover:opacity-100">
                <IconButton label={t('Projects.newSubfolder')} size="sm" onclick={() => newFolder(f.id)}>
                  <FolderPlus size={13} />
                </IconButton>
                <IconButton
                  label={t('Projects.renameFolder')}
                  size="sm"
                  onclick={() => {
                    renamingFolder = f.id
                    renameValue = f.name
                  }}
                >
                  <Pencil size={13} />
                </IconButton>
                <IconButton
                  label={t('Common.delete')}
                  size="sm"
                  variant="danger"
                  onclick={() => (deleteFolderTarget = f)}
                >
                  <Trash2 size={13} />
                </IconButton>
              </div>
            {/if}
          </li>
        {:else}
          {@const d = row.doc}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <li
            class="group flex items-center gap-1.5 px-2 py-1.5 rounded-(--radius-sm) hover:bg-(--color-hover-bg)"
            style="margin-left: {row.depth * 1.1 + 1.25}rem"
            draggable={renamingDoc !== d.id}
            ondragstart={(e) => {
              e.stopPropagation()
              dragItem = { kind: 'doc', id: d.id }
            }}
          >
            {#if d.status === 'pending' || d.status === 'processing'}
              <Spinner size="sm" />
            {:else if d.status === 'error'}
              <AlertCircle size={14} class="shrink-0 text-(--color-danger-500)" />
            {:else}
              <FileText size={14} class="shrink-0 text-(--color-text-secondary)" />
            {/if}

            {#if renamingDoc === d.id}
              <input
                bind:value={renameValue}
                class="flex-1 min-w-0 text-sm bg-transparent border-b border-(--color-brand-500) focus:outline-none"
                onkeydown={(e) => {
                  if (e.key === 'Enter') commitDocRename()
                  if (e.key === 'Escape') (renamingDoc = null)
                }}
              />
              <IconButton label={t('Common.save')} size="sm" onclick={commitDocRename}>
                <Check size={13} />
              </IconButton>
              <IconButton label={t('Common.cancel')} size="sm" onclick={() => (renamingDoc = null)}>
                <X size={13} />
              </IconButton>
            {:else}
              <button
                type="button"
                class="flex-1 min-w-0 text-left text-sm text-(--color-text-primary) truncate hover:underline"
                onclick={() => docViewer.openDocument(d.id, d.filename)}
              >
                {d.filename}
              </button>
              <div class="flex items-center gap-0.5 opacity-0 group-hover:opacity-100">
                <IconButton
                  label={t('Projects.renameFolder')}
                  size="sm"
                  onclick={() => {
                    renamingDoc = d.id
                    renameValue = d.filename
                  }}
                >
                  <Pencil size={13} />
                </IconButton>
                <IconButton
                  label={t('Common.delete')}
                  size="sm"
                  variant="danger"
                  onclick={() => (deleteDocTarget = d)}
                >
                  <Trash2 size={13} />
                </IconButton>
              </div>
            {/if}
          </li>
        {/if}
      {/each}
    </ul>
  {/if}
</div>

<ConfirmDialog
  open={deleteFolderTarget !== null}
  title={t('Projects.deleteFolderTitle')}
  message={t('Projects.deleteFolderBody', { name: deleteFolderTarget?.name ?? '' })}
  confirmLabel={t('Common.delete')}
  danger
  onconfirm={confirmDeleteFolder}
  oncancel={() => (deleteFolderTarget = null)}
/>
<ConfirmDialog
  open={deleteDocTarget !== null}
  title={t('Common.delete')}
  message={t('Projects.deleteFolderBody', { name: deleteDocTarget?.filename ?? '' })}
  confirmLabel={t('Common.delete')}
  danger
  onconfirm={confirmDeleteDoc}
  oncancel={() => (deleteDocTarget = null)}
/>
