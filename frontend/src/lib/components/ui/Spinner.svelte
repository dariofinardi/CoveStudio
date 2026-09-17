<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  interface Props {
    size?: 'sm' | 'md' | 'lg'
    label?: string
    class?: string
  }

  let { size = 'md', label = 'Loading', class: extraClass = '' }: Props = $props()

  const sizeClass = $derived(
    { sm: 'h-3.5 w-3.5 border-2', md: 'h-5 w-5 border-2', lg: 'h-8 w-8 border-[3px]' }[size]
  )
</script>

<span
  role="status"
  aria-label={label}
  class="inline-block rounded-full animate-spin
         border-current border-t-transparent
         {sizeClass} {extraClass}"
></span>
