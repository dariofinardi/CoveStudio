// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { describe, it, expect } from 'vitest'
import { scopeForRef, toCitation, PAGE_BREAK_SENTINEL } from './citation'

describe('scopeForRef', () => {
  it('maps a c-prefixed marker to a document citation', () => {
    expect(scopeForRef('c1')).toBe('document')
    expect(scopeForRef('c42')).toBe('document')
  })

  it('maps a g-prefixed marker to the global pool', () => {
    expect(scopeForRef('g1')).toBe('global')
  })

  it('maps a p-prefixed marker to the project pool', () => {
    expect(scopeForRef('p3')).toBe('project')
  })

  it('falls back to document for a legacy bare numeric marker', () => {
    expect(scopeForRef('1')).toBe('document')
  })
})

describe('toCitation', () => {
  it('prefers the real document UUID over the chat-local label', () => {
    const c = toCitation({
      ref: 'c1',
      doc_id: 'doc-0',
      document_id: '11111111-2222-3333-4444-555555555555',
      filename: 'contract.pdf',
      page: 12,
      quote: 'the parties agree',
    })
    expect(c.docId).toBe('11111111-2222-3333-4444-555555555555')
    expect(c.scope).toBe('document')
    expect(c.source).toBe('contract.pdf')
    expect(c.page).toBe(12)
    expect(c.quote).toBe('the parties agree')
  })

  it('falls back to the chat-local label when no UUID is present', () => {
    const c = toCitation({ ref: 'g2', doc_id: 'doc-3', quote: 'x' })
    expect(c.docId).toBe('doc-3')
    expect(c.scope).toBe('global')
  })

  it('accepts a page range string for a quote spanning a page break', () => {
    const c = toCitation({ ref: 'c1', page: '41-42' })
    expect(c.page).toBe('41-42')
  })

  it('drops a non-number / non-string page into undefined', () => {
    const c = toCitation({ ref: 'c1', page: { nonsense: true } })
    expect(c.page).toBeUndefined()
  })

  it('trims the ref and tolerates fully empty payloads', () => {
    const c = toCitation({ ref: '  2  ' })
    expect(c.ref).toBe('2')
    expect(c.docId).toBe('')
    expect(c.source).toBe('')
    expect(c.quote).toBe('')
  })

  it('uses the source label when no filename is given', () => {
    const c = toCitation({ ref: 'p1', source: 'eurlex/32016R0679' })
    expect(c.source).toBe('eurlex/32016R0679')
  })

  it('keeps kb path from backend payload for KB citations', () => {
    const c = toCitation({
      ref: 'g1',
      path: 'corpora/eurlex/32016R0679.txt',
      filename: '32016R0679.txt',
      quote: 'x',
    })
    expect(c.kbPath).toBe('corpora/eurlex/32016R0679.txt')
    expect(c.scope).toBe('global')
  })
})

describe('PAGE_BREAK_SENTINEL', () => {
  it('is the literal token the backend embeds across a page break', () => {
    expect(PAGE_BREAK_SENTINEL).toBe('[[PAGE_BREAK]]')
  })
})
