<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    /** Brand / logo area at the top. */
    brand?: Snippet
    /** Navigation items (typically SidebarItem instances). */
    children: Snippet
    /** Optional pinned footer (user avatar, settings shortcut). */
    footer?: Snippet
    class?: string
  }

  let { brand, children, footer, class: extraClass = '' }: Props = $props()
</script>

<aside
  class="flex flex-col shrink-0 h-full
         bg-(--color-surface-50) border-r border-(--color-surface-200)
         {extraClass}"
  style:width="var(--sidebar-width)"
>
  {#if brand}
    <div class="px-3 flex items-center" style:height="var(--topbar-height)">
      {@render brand()}
    </div>
  {/if}

  <!-- Body: fixed nav + (consumer-managed) scrollable region. The
       consumer controls its own padding and which part scrolls. -->
  <div class="flex-1 min-h-0 flex flex-col">
    {@render children()}
  </div>

  {#if footer}
    <div class="px-2 py-2 border-t border-(--color-surface-200) shrink-0">
      {@render footer()}
    </div>
  {/if}
</aside>
