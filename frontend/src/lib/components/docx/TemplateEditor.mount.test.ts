// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { describe, it, expect } from 'vitest'
import { mount, unmount } from 'svelte'
import TemplateEditor from './TemplateEditor.svelte'
import { blankUserTemplate } from '$lib/types/template'

describe('TemplateEditor mounts', () => {
  it('renders for a new template', () => {
    const target = document.createElement('div')
    const c = mount(TemplateEditor, {
      target,
      props: { initial: null, onback: () => {}, onsaved: () => {} },
    })
    expect(target.querySelector('h2')).toBeTruthy()
    unmount(c)
  })

  it('renders for an existing user template', () => {
    const target = document.createElement('div')
    const tpl = { ...blankUserTemplate(), id: 'user/test', display_name: { it: 'Test' } }
    const c = mount(TemplateEditor, {
      target,
      props: { initial: tpl, onback: () => {}, onsaved: () => {} },
    })
    expect(target.querySelector('h2')).toBeTruthy()
    unmount(c)
  })

  it('renders a template whose section skeleton omits optional fields', () => {
    // A real template often has skeleton entries with `title`/`render`/
    // `guidance` absent — they must not reach `bind:value` as undefined.
    const target = document.createElement('div')
    const tpl = {
      ...blankUserTemplate(),
      id: 'it/sample',
      is_system: true,
      display_name: { it: 'Esempio' },
      section_skeleton: [{ id: 'in_fatto' }, { id: 'sep', render: '* * *' }],
      few_shot_examples: [{ label: 'x', path: 'x.md' }],
    }
    const c = mount(TemplateEditor, {
      target,
      props: { initial: tpl, onback: () => {}, onsaved: () => {} },
    })
    expect(target.querySelector('h2')).toBeTruthy()
    unmount(c)
  })
})
