// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { describe, it, expect } from 'vitest'
import { highlightQuote, highlightCitation, findAll, clearHighlights } from './highlight'

function host(html: string): HTMLElement {
  const el = document.createElement('div')
  el.innerHTML = html
  return el
}

describe('highlightQuote', () => {
  it('wraps a matched passage in a mark.doc-hl element', () => {
    const el = host('<p>The quick brown fox jumps.</p>')
    const mark = highlightQuote(el, 'quick brown')
    expect(mark).not.toBeNull()
    expect(mark?.tagName).toBe('MARK')
    expect(mark?.classList.contains('doc-hl')).toBe(true)
    expect(el.querySelectorAll('mark.doc-hl').length).toBe(1)
  })

  it('matches despite differing whitespace and punctuation', () => {
    const el = host('<p>The   quick,\n brown   fox.</p>')
    expect(highlightQuote(el, 'quick brown')).not.toBeNull()
  })

  it('matches across multiple text nodes / spans', () => {
    const el = host('<span>quick </span><span>brown </span><span>fox</span>')
    expect(highlightQuote(el, 'quick brown fox')).not.toBeNull()
  })

  it('returns null for a too-short quote', () => {
    const el = host('<p>The quick brown fox.</p>')
    expect(highlightQuote(el, 'fox')).toBeNull()
  })

  it('returns null when the passage is absent', () => {
    const el = host('<p>The quick brown fox.</p>')
    expect(highlightQuote(el, 'lazy sleeping dog')).toBeNull()
  })

  it('falls back to a shrinking prefix when the tail is paraphrased', () => {
    const el = host('<p>The contract was signed in two thousand and twenty by both parties.</p>')
    const mark = highlightQuote(el, 'The contract was signed in two thousand and twenty by entirely different wording')
    expect(mark).not.toBeNull()
  })
})

describe('clearHighlights', () => {
  it('removes previously added marks and restores the text', () => {
    const el = host('<p>The quick brown fox.</p>')
    highlightQuote(el, 'quick brown')
    expect(el.querySelectorAll('mark.doc-hl').length).toBe(1)
    clearHighlights(el)
    expect(el.querySelectorAll('mark.doc-hl').length).toBe(0)
    expect(el.textContent).toBe('The quick brown fox.')
  })
})

describe('highlightCitation', () => {
  it('highlights both halves of a quote that spans a page break', () => {
    const el = host('<p>Article one is binding. Article two is optional.</p>')
    const first = highlightCitation(el, 'Article one is binding[[PAGE_BREAK]]Article two is optional', '[[PAGE_BREAK]]')
    expect(first).not.toBeNull()
    expect(el.querySelectorAll('mark.doc-hl').length).toBe(2)
  })

  it('highlights each segment split by an ellipsis', () => {
    const el = host('<p>The beginning of the clause and the end of the clause.</p>')
    highlightCitation(el, 'The beginning of the clause … the end of the clause', '[[PAGE_BREAK]]')
    expect(el.querySelectorAll('mark.doc-hl').length).toBe(2)
  })

  it('clears stale marks before applying new ones', () => {
    const el = host('<p>The quick brown fox jumps over.</p>')
    highlightQuote(el, 'quick brown')
    highlightCitation(el, 'jumps over', '[[PAGE_BREAK]]')
    expect(el.querySelectorAll('mark.doc-hl').length).toBe(1)
  })
})

describe('findAll', () => {
  it('highlights every occurrence and returns them in document order', () => {
    const el = host('<p>alpha beta alpha gamma alpha</p>')
    const marks = findAll(el, 'alpha')
    expect(marks.length).toBe(3)
    expect(el.querySelectorAll('mark.doc-hl').length).toBe(3)
  })

  it('returns an empty array for a too-short term', () => {
    const el = host('<p>a a a</p>')
    expect(findAll(el, 'a')).toEqual([])
  })

  it('returns an empty array when the term is absent', () => {
    const el = host('<p>nothing to see here</p>')
    expect(findAll(el, 'absent')).toEqual([])
  })
})
