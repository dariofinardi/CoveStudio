<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    title?: string
    /** Left-aligned content (breadcrumbs, back button). */
    leading?: Snippet
    /** Right-aligned content (actions, model selector). */
    actions?: Snippet
    class?: string
  }

  let { title, leading, actions, class: extraClass = '' }: Props = $props()
</script>

<header
  class="shrink-0 flex items-center gap-3 px-4
         border-b border-(--color-surface-200) bg-(--color-surface-0)
         {extraClass}"
  style:height="var(--topbar-height)"
>
  {#if leading}
    {@render leading()}
  {/if}
  {#if title}
    <h1 class="text-sm font-semibold text-(--color-text-primary)">{title}</h1>
  {/if}
  <div class="flex-1"></div>
  {#if actions}
    <div class="flex items-center gap-2">
      {@render actions()}
    </div>
  {/if}
</header>
