<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    title: string
    description?: string
    icon?: Snippet
    action?: Snippet
    class?: string
  }

  let { title, description, icon, action, class: extraClass = '' }: Props = $props()
</script>

<div
  class="flex flex-col items-center justify-center text-center
         px-6 py-12 gap-3 {extraClass}"
>
  {#if icon}
    <div class="text-(--color-text-disabled) mb-1">
      {@render icon()}
    </div>
  {/if}
  <h3 class="text-sm font-medium text-(--color-text-primary)">{title}</h3>
  {#if description}
    <p class="text-xs text-(--color-text-secondary) max-w-sm">{description}</p>
  {/if}
  {#if action}
    <div class="mt-2">
      {@render action()}
    </div>
  {/if}
</div>
