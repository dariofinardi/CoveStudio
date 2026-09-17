<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  interface Props {
    /** 0..1 fraction, or null/undefined for indeterminate. */
    value?: number | null
    label?: string
    showPercent?: boolean
    size?: 'sm' | 'md'
    tone?: 'brand' | 'success' | 'warning' | 'danger'
    class?: string
  }

  let {
    value = null,
    label,
    showPercent = false,
    size = 'md',
    tone = 'brand',
    class: extraClass = '',
  }: Props = $props()

  const indeterminate = $derived(value === null || value === undefined)
  const pct = $derived(
    indeterminate ? 0 : Math.round(Math.min(1, Math.max(0, value as number)) * 100)
  )

  const trackHeight = $derived(size === 'sm' ? 'h-1' : 'h-2')

  const fillColor = $derived(
    {
      brand: 'bg-(--color-brand-500)',
      success: 'bg-(--color-success-500)',
      warning: 'bg-(--color-warning-500)',
      danger: 'bg-(--color-danger-500)',
    }[tone]
  )
</script>

<div class="flex flex-col gap-1 {extraClass}">
  {#if label || showPercent}
    <div class="flex justify-between text-xs text-(--color-text-secondary)">
      {#if label}<span>{label}</span>{/if}
      {#if showPercent && !indeterminate}<span class="font-mono">{pct}%</span>{/if}
    </div>
  {/if}

  <div
    role="progressbar"
    aria-valuemin={0}
    aria-valuemax={100}
    aria-valuenow={indeterminate ? undefined : pct}
    aria-label={label}
    class="w-full overflow-hidden rounded-full bg-(--color-surface-200) {trackHeight}"
  >
    {#if indeterminate}
      <div class="h-full w-1/3 rounded-full {fillColor} progress-indeterminate"></div>
    {:else}
      <div
        class="h-full rounded-full {fillColor} transition-[width] duration-(--transition-medium)"
        style:width="{pct}%"
      ></div>
    {/if}
  </div>
</div>

<style>
  .progress-indeterminate {
    animation: progress-slide 1.2s ease-in-out infinite;
  }
  @keyframes progress-slide {
    0% { transform: translateX(-100%); }
    100% { transform: translateX(400%); }
  }
</style>
