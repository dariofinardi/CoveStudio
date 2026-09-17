<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import DocViewerPanel from '$lib/components/documents/DocViewerPanel.svelte'

  interface Props {
    sidebar: Snippet
    topbar?: Snippet
    statusbar?: Snippet
    children: Snippet
  }

  let { sidebar, topbar, statusbar, children }: Props = $props()
</script>

<div class="flex h-screen w-screen overflow-hidden bg-(--color-surface-0)">
  {@render sidebar()}

  <div class="flex flex-col flex-1 min-w-0">
    {#if topbar}
      {@render topbar()}
    {/if}

    <main class="flex-1 overflow-y-auto">
      {@render children()}
    </main>

    {#if statusbar}
      {@render statusbar()}
    {/if}
  </div>

  <!-- Global document-viewer panel (renders only when a doc is open). -->
  <DocViewerPanel />
</div>
