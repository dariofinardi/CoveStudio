<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    open?: boolean
    title?: string
    size?: 'sm' | 'md' | 'lg' | 'xl'
    closeOnBackdrop?: boolean
    closeOnEsc?: boolean
    header?: Snippet
    footer?: Snippet
    children: Snippet
    onclose?: () => void
  }

  let {
    open = $bindable(false),
    title,
    size = 'md',
    closeOnBackdrop = true,
    closeOnEsc = true,
    header,
    footer,
    children,
    onclose,
  }: Props = $props()

  const widthClass = $derived(
    { sm: 'max-w-sm', md: 'max-w-lg', lg: 'max-w-2xl', xl: 'max-w-4xl' }[size]
  )

  function close() {
    open = false
    onclose?.()
  }

  function onBackdrop(e: MouseEvent) {
    if (closeOnBackdrop && e.target === e.currentTarget) close()
  }

  $effect(() => {
    if (!open || !closeOnEsc) return
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape') close()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  $effect(() => {
    if (!open) return
    const prev = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    return () => {
      document.body.style.overflow = prev
    }
  })
</script>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-50 flex items-center justify-center p-4
           bg-black/40 backdrop-blur-[1px]"
    onclick={onBackdrop}
  >
    <div
      role="dialog"
      aria-modal="true"
      aria-label={title}
      class="w-full {widthClass} max-h-[90vh] flex flex-col
             bg-(--color-surface-0) rounded-(--radius-lg)
             shadow-(--shadow-modal) overflow-hidden"
    >
      {#if header || title}
        <header
          class="px-5 py-3.5 border-b border-(--color-surface-100)
                 flex items-center justify-between gap-3 shrink-0"
        >
          {#if header}
            {@render header()}
          {:else}
            <h2 class="text-base font-semibold text-(--color-text-primary)">{title}</h2>
          {/if}
          <button
            type="button"
            aria-label="Close"
            onclick={close}
            class="inline-flex h-7 w-7 items-center justify-center rounded-(--radius-md)
                   text-(--color-text-secondary) hover:bg-(--color-hover-bg)
                   focus:outline-none focus-visible:ring-2 focus-visible:ring-(--color-brand-500)"
          >
            <svg width="16" height="16" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true">
              <path
                d="M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z"
              />
            </svg>
          </button>
        </header>
      {/if}

      <div class="px-5 py-4 overflow-y-auto flex-1 min-h-0">
        {@render children()}
      </div>

      {#if footer}
        <footer
          class="px-5 py-3.5 border-t border-(--color-surface-100)
                 bg-(--color-surface-50) flex items-center justify-end gap-2 shrink-0"
        >
          {@render footer()}
        </footer>
      {/if}
    </div>
  </div>
{/if}
