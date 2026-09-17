<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import type { HTMLButtonAttributes } from 'svelte/elements'
  import type { ButtonSize, ButtonVariant } from './Button.svelte'

  interface Props extends Omit<HTMLButtonAttributes, 'class' | 'children'> {
    variant?: ButtonVariant
    size?: ButtonSize
    label: string
    children: Snippet
    class?: string
  }

  let {
    variant = 'ghost',
    size = 'md',
    label,
    disabled,
    type = 'button',
    children,
    class: extraClass = '',
    ...rest
  }: Props = $props()

  const sizeClass = $derived(
    { sm: 'h-7 w-7', md: 'h-9 w-9', lg: 'h-11 w-11' }[size]
  )

  const variantClass = $derived(
    {
      primary:
        'bg-(--color-brand-500) text-white hover:bg-(--color-brand-600)',
      secondary:
        'bg-(--color-surface-100) text-(--color-text-primary) hover:bg-(--color-surface-200) border border-(--color-surface-200)',
      ghost:
        'bg-transparent text-(--color-text-primary) hover:bg-(--color-hover-bg)',
      danger:
        'bg-(--color-danger-500) text-white hover:opacity-90',
      pill: 'bg-[var(--cta-pill-bg)] text-[var(--cta-pill-fg)] rounded-full hover:opacity-90',
    }[variant]
  )
</script>

<button
  {type}
  {disabled}
  aria-label={label}
  title={label}
  class="inline-flex items-center justify-center cursor-pointer
         rounded-(--radius-md) transition-colors duration-(--transition-fast)
         focus:outline-none focus-visible:ring-2 focus-visible:ring-(--color-brand-500) focus-visible:ring-offset-1
         disabled:opacity-50 disabled:cursor-not-allowed
         {sizeClass} {variantClass} {extraClass}"
  {...rest}
>
  {@render children()}
</button>
