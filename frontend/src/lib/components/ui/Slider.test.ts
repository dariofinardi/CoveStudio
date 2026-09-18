// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { describe, it, expect, vi } from 'vitest'
import type { ComponentProps } from 'svelte'
import { mount, unmount, flushSync } from 'svelte'
import Slider from './Slider.svelte'

type SliderProps = ComponentProps<typeof Slider>

function render(props: SliderProps) {
  const target = document.createElement('div')
  document.body.appendChild(target)
  const component = mount(Slider, { target, props })
  const input = target.querySelector('input[type="range"]') as HTMLInputElement
  return { target, component, input }
}

describe('Slider', () => {
  it('renders the label, the formatted value and both end captions', () => {
    const { target, component, input } = render({
      label: 'Prudenza del mascheramento',
      value: 0.5,
      min: 0.05,
      max: 0.95,
      step: 0.05,
      minLabel: 'Maschera di più',
      maxLabel: 'Solo ciò che è certo',
      format: (v: number) => `${Math.round(v * 100)}%`,
    })

    expect(target.textContent).toContain('Prudenza del mascheramento')
    // The raw number means nothing to the reader; the percentage does.
    expect(target.textContent).toContain('50%')
    expect(target.textContent).toContain('Maschera di più')
    expect(target.textContent).toContain('Solo ciò che è certo')
    expect(input.min).toBe('0.05')
    expect(input.max).toBe('0.95')
    expect(input.step).toBe('0.05')
    unmount(component)
  })

  it('commits on change, not on every input tick', () => {
    // This is the whole point of the component: dragging fires `input`
    // continuously, and a caller that persists to the server must not
    // send a request per pixel.
    const onchange = vi.fn()
    const { component, input } = render({ label: 'Soglia', value: 0.5, onchange })

    input.value = '0.2'
    input.dispatchEvent(new Event('input', { bubbles: true }))
    flushSync()
    expect(onchange).not.toHaveBeenCalled()

    input.dispatchEvent(new Event('change', { bubbles: true }))
    flushSync()
    expect(onchange).toHaveBeenCalledTimes(1)
    expect(onchange).toHaveBeenCalledWith(0.2)
    unmount(component)
  })

  it('labels the input so the control is reachable by its name', () => {
    const { target, component, input } = render({ label: 'Soglia', value: 0.5 })
    const label = target.querySelector('label') as HTMLLabelElement
    expect(label.getAttribute('for')).toBe(input.id)
    expect(input.id).not.toBe('')
    unmount(component)
  })

  it('does not fire while disabled', () => {
    const onchange = vi.fn()
    const { component, input } = render({
      label: 'Soglia',
      value: 0.5,
      disabled: true,
      onchange,
    })
    expect(input.disabled).toBe(true)
    // A browser emits nothing from a disabled range; a programmatic
    // event must not commit a value either — while a save is in
    // flight the control is disabled, and a stray commit would race
    // the request that is already running.
    input.dispatchEvent(new Event('change', { bubbles: true }))
    flushSync()
    expect(onchange).not.toHaveBeenCalled()
    unmount(component)
  })

  it('shows the raw number when no formatter is given', () => {
    const { target, component } = render({ label: 'Soglia', value: 0.35 })
    expect(target.textContent).toContain('0.35')
    unmount(component)
  })
})
