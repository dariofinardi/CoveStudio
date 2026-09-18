<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  A labelled range input. Used where a setting is a point on a scale
  rather than a switch, and where the two ends mean something the user
  can read (`minLabel` / `maxLabel`) — the raw number rarely does.

  The value is committed on `change` (pointer released, or arrow keys
  settled), not on every `input` tick, so a caller that persists to the
  server does not fire a request per pixel dragged. `value` still
  updates live so the on-screen figure follows the thumb.
-->
<script lang="ts">
  interface Props {
    label: string
    value: number
    min?: number
    max?: number
    step?: number
    /** Caption under the left end of the track. */
    minLabel?: string
    /** Caption under the right end of the track. */
    maxLabel?: string
    /** Plain-language explanation shown under the control. */
    description?: string
    /** How to render the current value; defaults to the raw number. */
    format?: (v: number) => string
    disabled?: boolean
    /** Fired when the user settles on a value. */
    onchange?: (v: number) => void
  }

  let {
    label,
    value = $bindable(),
    min = 0,
    max = 1,
    step = 0.05,
    minLabel = '',
    maxLabel = '',
    description = '',
    format,
    disabled = false,
    onchange,
  }: Props = $props()

  const display = $derived(format ? format(value) : String(value))
  const inputId = `slider-${Math.random().toString(36).slice(2, 9)}`
</script>

<div class="space-y-1.5">
  <div class="flex items-baseline justify-between gap-2">
    <label for={inputId} class="text-sm font-medium text-(--color-text-primary)">{label}</label>
    <span class="text-sm tabular-nums text-(--color-text-secondary)">{display}</span>
  </div>
  <input
    id={inputId}
    type="range"
    {min}
    {max}
    {step}
    {disabled}
    bind:value
    onchange={() => {
      // A browser does not emit `change` on a disabled input, but the
      // guard keeps a programmatic event (a test, a script) from
      // committing a value the user could not have set.
      if (!disabled) onchange?.(value)
    }}
    class="w-full accent-(--color-accent) disabled:opacity-50"
  />
  {#if minLabel || maxLabel}
    <div class="flex justify-between text-xs text-(--color-text-disabled)">
      <span>{minLabel}</span>
      <span>{maxLabel}</span>
    </div>
  {/if}
  {#if description}
    <p class="text-xs text-(--color-text-secondary)">{description}</p>
  {/if}
</div>
