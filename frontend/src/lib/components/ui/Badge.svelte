<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts" module>
  export type BadgeTone =
    | 'neutral'
    | 'brand'
    | 'success'
    | 'warning'
    | 'danger'
    | 'info'
    | 'assistant'  // blu — workflow type Assistant (brand audit)
    | 'tabular'    // viola — workflow type Tabular (brand audit)
    | 'level'      // arancio — DOCX template levels L1/L2/L3
</script>

<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    tone?: BadgeTone
    size?: 'xs' | 'sm'
    children: Snippet
    class?: string
  }

  let {
    tone = 'neutral',
    size = 'sm',
    children,
    class: extraClass = '',
  }: Props = $props()

  const sizeClass = $derived(
    size === 'xs' ? 'px-1.5 py-0 text-[10px] h-4' : 'px-2 py-0.5 text-xs h-5'
  )

  const toneClass = $derived(
    {
      neutral:
        'bg-(--color-surface-100) text-(--color-text-secondary)',
      brand:
        'bg-(--color-brand-50) text-(--color-brand-700)',
      // Light-tone backgrounds were the `-50` shade, which sits at
      // almost the same luminance as `--color-surface-100` (the grey
      // we use for corpus cards). The badge silhouette disappeared and
      // the coloured text became hard to pick out. Bumped to `-100` bg
      // + `-800` fg so the badge has a visible pill shape and the
      // text meets WCAG AA contrast on a grey-100 surface.
      success:
        'bg-emerald-100 text-emerald-800 dark:bg-emerald-900/50 dark:text-emerald-200',
      warning:
        'bg-amber-100 text-amber-800 dark:bg-amber-900/50 dark:text-amber-200',
      danger:
        'bg-red-100 text-red-800 dark:bg-red-900/50 dark:text-red-200',
      info:
        'bg-blue-100 text-blue-800 dark:bg-blue-900/50 dark:text-blue-200',
      assistant:
        'bg-[var(--badge-assistant-bg)] text-[var(--badge-assistant-fg)]',
      tabular:
        'bg-[var(--badge-tabular-bg)] text-[var(--badge-tabular-fg)]',
      level:
        'bg-[var(--badge-level-bg)] text-[var(--badge-level-fg)]',
    }[tone]
  )
</script>

<span
  class="inline-flex items-center font-medium rounded-(--radius-sm)
         whitespace-nowrap
         {sizeClass} {toneClass} {extraClass}"
>
  {@render children()}
</span>
