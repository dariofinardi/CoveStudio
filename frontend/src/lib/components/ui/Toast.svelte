<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import type { Toast } from '$lib/stores/toast.svelte'

  interface Props {
    toast: Toast
    ondismiss: (id: number) => void
  }

  let { toast, ondismiss }: Props = $props()

  const toneClass = $derived(
    {
      info: 'border-l-(--color-info-500)',
      success: 'border-l-(--color-success-500)',
      warning: 'border-l-(--color-warning-500)',
      danger: 'border-l-(--color-danger-500)',
    }[toast.tone]
  )

  const iconColor = $derived(
    {
      info: 'text-(--color-info-500)',
      success: 'text-(--color-success-500)',
      warning: 'text-(--color-warning-500)',
      danger: 'text-(--color-danger-500)',
    }[toast.tone]
  )
</script>

<div
  role={toast.tone === 'danger' ? 'alert' : 'status'}
  class="flex items-start gap-3 w-80 p-3
         bg-(--color-surface-0) border border-(--color-surface-200)
         border-l-4 {toneClass}
         rounded-(--radius-md) shadow-(--shadow-lg)"
>
  <span class="mt-0.5 shrink-0 {iconColor}" aria-hidden="true">
    {#if toast.tone === 'success'}
      <svg width="16" height="16" viewBox="0 0 20 20" fill="currentColor">
        <path fill-rule="evenodd" d="M16.7 5.3a1 1 0 010 1.4l-7.5 7.5a1 1 0 01-1.4 0L3.3 9.7a1 1 0 011.4-1.4l3.8 3.8 6.8-6.8a1 1 0 011.4 0z" clip-rule="evenodd"/>
      </svg>
    {:else if toast.tone === 'danger'}
      <svg width="16" height="16" viewBox="0 0 20 20" fill="currentColor">
        <path fill-rule="evenodd" d="M10 2a8 8 0 100 16 8 8 0 000-16zm-1 4a1 1 0 112 0v4a1 1 0 11-2 0V6zm1 9a1.25 1.25 0 100-2.5A1.25 1.25 0 0010 15z" clip-rule="evenodd"/>
      </svg>
    {:else}
      <svg width="16" height="16" viewBox="0 0 20 20" fill="currentColor">
        <path fill-rule="evenodd" d="M10 2a8 8 0 100 16 8 8 0 000-16zm1 5a1 1 0 10-2 0 1 1 0 002 0zm-1 3a1 1 0 00-1 1v4a1 1 0 102 0v-4a1 1 0 00-1-1z" clip-rule="evenodd"/>
      </svg>
    {/if}
  </span>

  <div class="flex-1 min-w-0">
    <p class="text-sm font-medium text-(--color-text-primary)">{toast.title}</p>
    {#if toast.detail}
      <p class="text-xs text-(--color-text-secondary) mt-0.5 break-words">{toast.detail}</p>
    {/if}
  </div>

  <button
    type="button"
    aria-label="Dismiss"
    onclick={() => ondismiss(toast.id)}
    class="shrink-0 inline-flex h-5 w-5 items-center justify-center rounded-(--radius-sm)
           text-(--color-text-secondary) hover:bg-(--color-hover-bg)
           focus:outline-none focus-visible:ring-2 focus-visible:ring-(--color-brand-500)"
  >
    <svg width="12" height="12" viewBox="0 0 20 20" fill="currentColor">
      <path d="M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z"/>
    </svg>
  </button>
</div>
