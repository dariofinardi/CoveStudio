<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    title?: string
    subtitle?: string
    padded?: boolean
    header?: Snippet
    footer?: Snippet
    children: Snippet
    class?: string
  }

  let {
    title,
    subtitle,
    padded = true,
    header,
    footer,
    children,
    class: extraClass = '',
  }: Props = $props()
</script>

<section
  class="bg-(--color-surface-0) border border-(--color-surface-200)
         rounded-(--radius-lg) shadow-(--shadow-sm) overflow-hidden
         {extraClass}"
>
  {#if header || title}
    <header
      class="px-4 py-3 border-b border-(--color-surface-100)
             flex items-center justify-between gap-3"
    >
      {#if header}
        {@render header()}
      {:else}
        <div class="flex flex-col">
          <h3 class="text-sm font-semibold text-(--color-text-primary)">{title}</h3>
          {#if subtitle}
            <p class="text-xs text-(--color-text-secondary)">{subtitle}</p>
          {/if}
        </div>
      {/if}
    </header>
  {/if}

  <div class={padded ? 'p-4' : ''}>
    {@render children()}
  </div>

  {#if footer}
    <footer class="px-4 py-3 border-t border-(--color-surface-100) bg-(--color-surface-50)">
      {@render footer()}
    </footer>
  {/if}
</section>
