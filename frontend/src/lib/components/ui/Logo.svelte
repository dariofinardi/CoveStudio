<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Product mark "Alveo" — a 5×5 modular grid crossed by an empty channel
  (src/assets/cove-studio-logo.svg). `activity` animates and recolours it:
    · idle     — static, theme text colour
    · thinking — modules assemble along the current, brand colour
    · docx     — same motion, blue (generating a .docx)
    · upload   — same motion, green (extracting text from an upload)

  The motion reproduces the assembly loop of the original artwork: every
  module fades and scales in with a delay that follows the diagonal from
  upstream to downstream. Honours prefers-reduced-motion (no animation).
-->
<script lang="ts">
  import { PRODUCT_NAME } from '$lib/product'
  interface Props {
    size?: number
    activity?: 'idle' | 'thinking' | 'docx' | 'upload'
    class?: string
  }

  let { size = 40, activity = 'idle', class: extraClass = '' }: Props = $props()

  // Filled modules as [column, row, assembly delay in seconds], on a
  // 30-unit pitch with 24-unit modules (144×144 view box).
  const MODULES: [number, number, number][] = [
    [4, 0, 0], [3, 0, 0.15], [2, 0, 0.3], [1, 0, 0.45], [0, 0, 0.6],
    [1, 1, 0.6], [0, 1, 0.75],
    [4, 2, 0.3], [3, 2, 0.45], [0, 2, 0.9],
    [4, 3, 0.45], [3, 3, 0.6], [2, 3, 0.75],
    [4, 4, 0.6], [3, 4, 0.75], [2, 4, 0.9], [1, 4, 1.05], [0, 4, 1.2],
  ]
</script>

<svg
  class="app-logo app-logo-{activity} {extraClass}"
  width={size}
  height={size}
  viewBox="0 0 144 144"
  role="img"
  aria-label={PRODUCT_NAME}
>
  {#each MODULES as [col, row, delay] (`${col},${row}`)}
    <rect
      x={col * 30}
      y={row * 30}
      width="24"
      height="24"
      fill="currentColor"
      class="app-logo-module"
      style="animation-delay: {delay}s"
    />
  {/each}
</svg>
