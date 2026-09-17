<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!-- Segmented light / system / dark control bound to the theme store. -->
<script lang="ts">
  import { themeStore, type ThemeMode } from '$lib/stores/theme.svelte'
  import { Sun, Monitor, Moon } from 'lucide-svelte'

  const options: { mode: ThemeMode; label: string; icon: typeof Sun }[] = [
    { mode: 'light', label: 'Light theme', icon: Sun },
    { mode: 'system', label: 'System theme', icon: Monitor },
    { mode: 'dark', label: 'Dark theme', icon: Moon },
  ]
</script>

<div
  role="radiogroup"
  aria-label="Theme"
  class="inline-flex items-center gap-0.5 p-0.5
         bg-(--color-surface-100) rounded-(--radius-md)"
>
  {#each options as opt (opt.mode)}
    {@const active = themeStore.mode === opt.mode}
    <button
      type="button"
      role="radio"
      aria-checked={active}
      aria-label={opt.label}
      title={opt.label}
      onclick={() => themeStore.set(opt.mode)}
      class="inline-flex items-center justify-center h-6 w-6 rounded-(--radius-sm)
             transition-colors duration-(--transition-fast)
             focus:outline-none focus-visible:ring-2 focus-visible:ring-(--color-brand-500)
             {active
               ? 'bg-(--color-surface-0) text-(--color-brand-600) shadow-(--shadow-xs)'
               : 'text-(--color-text-secondary) hover:text-(--color-text-primary)'}"
    >
      <opt.icon size={14} />
    </button>
  {/each}
</div>
