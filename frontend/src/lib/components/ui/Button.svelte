<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts" module>
  export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger' | 'pill'
  export type ButtonSize = 'sm' | 'md' | 'lg'
</script>

<script lang="ts">
  import type { Snippet } from 'svelte'
  import type { HTMLButtonAttributes } from 'svelte/elements'
  import Spinner from './Spinner.svelte'

  interface Props extends Omit<HTMLButtonAttributes, 'class' | 'children'> {
    variant?: ButtonVariant
    size?: ButtonSize
    loading?: boolean
    full?: boolean
    iconBefore?: Snippet
    iconAfter?: Snippet
    children: Snippet
    class?: string
  }

  let {
    variant = 'primary',
    size = 'md',
    loading = false,
    full = false,
    iconBefore,
    iconAfter,
    disabled,
    type = 'button',
    children,
    class: extraClass = '',
    ...rest
  }: Props = $props()

  const sizeClass = $derived(
    {
      sm: 'h-8 px-3 text-xs gap-1.5',
      md: 'h-9 px-4 text-sm gap-2',
      lg: 'h-11 px-5 text-base gap-2.5',
    }[size]
  )

  const variantClass = $derived(
    {
      primary:
        'bg-(--color-brand-500) text-white hover:bg-(--color-brand-600) active:bg-(--color-brand-700) focus-visible:ring-(--color-brand-500)',
      secondary:
        'bg-(--color-surface-100) text-(--color-text-primary) hover:bg-(--color-surface-200) focus-visible:ring-(--color-brand-500) border border-(--color-surface-200)',
      ghost:
        'bg-transparent text-(--color-text-primary) hover:bg-(--color-hover-bg) focus-visible:ring-(--color-brand-500)',
      danger:
        'bg-(--color-danger-500) text-white hover:opacity-90 focus-visible:ring-(--color-danger-500)',
      pill: 'bg-[var(--cta-pill-bg)] text-[var(--cta-pill-fg)] rounded-[var(--cta-pill-radius)] hover:opacity-90 focus-visible:ring-(--color-brand-500)',
    }[variant]
  )

  const radiusClass = $derived(variant === 'pill' ? '' : 'rounded-(--radius-md)')
</script>

<button
  {type}
  disabled={disabled || loading}
  class="inline-flex items-center justify-center whitespace-nowrap font-medium select-none cursor-pointer
         transition-colors duration-(--transition-fast)
         focus:outline-none focus-visible:ring-2 focus-visible:ring-offset-1
         disabled:opacity-50 disabled:cursor-not-allowed
         {sizeClass} {variantClass} {radiusClass}
         {full ? 'w-full' : ''}
         {extraClass}"
  {...rest}
>
  {#if loading}
    <Spinner size={size === 'lg' ? 'md' : 'sm'} />
  {:else if iconBefore}
    {@render iconBefore()}
  {/if}
  {@render children()}
  {#if iconAfter && !loading}
    {@render iconAfter()}
  {/if}
</button>
