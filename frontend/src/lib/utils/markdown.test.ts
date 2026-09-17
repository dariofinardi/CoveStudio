// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { describe, it, expect } from 'vitest'
import { renderMarkdown } from './markdown'

describe('renderMarkdown', () => {
  it('renders basic Markdown formatting', () => {
    expect(renderMarkdown('**bold**')).toContain('<strong>bold</strong>')
    expect(renderMarkdown('# Heading')).toContain('<h1')
    expect(renderMarkdown('- one\n- two')).toContain('<li>')
  })

  it('keeps single line breaks (breaks: true)', () => {
    expect(renderMarkdown('line one\nline two')).toContain('<br>')
  })

  it('strips a script tag injected through the Markdown', () => {
    const html = renderMarkdown('hello <script>alert(1)</script> world')
    expect(html).not.toContain('<script>')
  })

  it('strips an inline event handler attribute', () => {
    const html = renderMarkdown('<img src="x" onerror="alert(1)">')
    expect(html).not.toContain('onerror')
  })

  it('tolerates null / undefined input', () => {
    expect(renderMarkdown(null as unknown as string)).toBe('')
  })
})
