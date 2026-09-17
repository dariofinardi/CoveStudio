// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { describe, it, expect } from 'vitest'
import { stripCitationsBlock, renderMessageHtml } from './citations'
import type { Citation } from '$lib/types/citation'

function cite(ref: string): Citation {
  return {
    ref,
    scope: ref.startsWith('g') ? 'global' : ref.startsWith('p') ? 'project' : 'document',
    docId: `doc-${ref}`,
    source: `source-${ref}`,
    page: 1,
    quote: `quote ${ref}`,
  }
}

describe('stripCitationsBlock', () => {
  it('removes a complete trailing CITATIONS block', () => {
    const text = 'The answer is 42.\n<CITATIONS>\n[1] something\n</CITATIONS>'
    expect(stripCitationsBlock(text)).toBe('The answer is 42.')
  })

  it('removes a partial block that arrived mid-stream', () => {
    expect(stripCitationsBlock('Hello world.\n<CITATIONS>\n[1] partial')).toBe('Hello world.')
  })

  it('removes a block even with just the opening tag', () => {
    expect(stripCitationsBlock('Body text<CITATIONS>')).toBe('Body text')
  })

  it('is case-insensitive on the opening tag', () => {
    expect(stripCitationsBlock('Body<citations>junk')).toBe('Body')
  })

  it('leaves text without a block untouched', () => {
    expect(stripCitationsBlock('Plain answer.')).toBe('Plain answer.')
  })

  it('tolerates null / undefined input', () => {
    expect(stripCitationsBlock(undefined as unknown as string)).toBe('')
  })
})

describe('renderMessageHtml', () => {
  it('returns sanitised HTML untouched when there are no citations', () => {
    const html = renderMessageHtml('Just **bold** text.')
    expect(html).toContain('<strong>bold</strong>')
  })

  it('replaces a resolvable marker with a citation pill', () => {
    const html = renderMessageHtml('See clause [c1] here.', [cite('c1')])
    expect(html).toContain('cite-pill')
    expect(html).toContain('data-cite-ref="c1"')
  })

  it('colours the pill by scope', () => {
    expect(renderMessageHtml('Ref [g1].', [cite('g1')])).toContain('cite-global')
    expect(renderMessageHtml('Ref [p1].', [cite('p1')])).toContain('cite-project')
    expect(renderMessageHtml('Ref [c1].', [cite('c1')])).toContain('cite-document')
  })

  it('leaves a bare bracketed number as plain text — only prefixed markers cite', () => {
    const html = renderMessageHtml('See clause [1] and year [2024].', [cite('c1')])
    expect(html).not.toContain('cite-pill')
    expect(html).toContain('[1]')
    expect(html).toContain('[2024]')
  })

  it('leaves an unresolvable marker as plain text', () => {
    const html = renderMessageHtml('The clause [c9] was fine.', [cite('c1')])
    expect(html).not.toContain('cite-pill')
    expect(html).toContain('[c9]')
  })

  it('strips the trailing CITATIONS block before rendering', () => {
    const html = renderMessageHtml('Answer [c1].\n<CITATIONS>\n[1] x\n</CITATIONS>', [cite('c1')])
    expect(html).not.toContain('CITATIONS')
  })

  it('renders each marker of a comma group as its own pill', () => {
    const html = renderMessageHtml('Both [c1, c2] apply.', [cite('c1'), cite('c2')])
    expect(html).toContain('data-cite-ref="c1"')
    expect(html).toContain('data-cite-ref="c2"')
  })

  it('falls back to priorCitations when current message lacks a ref', () => {
    // Real-world case from the 10-PDF medical-legal chat: turn 2 (docx
    // generation) emits prose mentioning [c1..c30] but its <CITATIONS>
    // JSON only carries c63..c77 because the model reused turn 1's
    // labels without re-emitting their annotations. The renderer must
    // still pill-ify [c1] by looking up turn 1's citations.
    const turn2Cits = [cite('c63'), cite('c64')]
    const turn1Cits = [cite('c1'), cite('c2'), cite('c3')]
    const html = renderMessageHtml(
      'See [c1, c2, c3] and also [c63].',
      turn2Cits,
      turn1Cits,
    )
    expect(html).toContain('data-cite-ref="c1"')
    expect(html).toContain('data-cite-ref="c2"')
    expect(html).toContain('data-cite-ref="c3"')
    expect(html).toContain('data-cite-ref="c63"')
  })

  it('current-message citation wins over priorCitations on duplicate ref', () => {
    // If the model re-emits an annotation in the current turn, that one
    // is more specific to the turn's intent and must take precedence.
    // The test asserts the pill is rendered (presence) — semantic
    // identity (which underlying doc it points to) is enforced at click
    // time in ChatMessage.svelte's openCitation, also current-first.
    const turn2Cits = [cite('c1')]
    const turn1Cits = [cite('c1')]
    const html = renderMessageHtml('Look at [c1].', turn2Cits, turn1Cits)
    // One pill rendered (Set-based dedup in renderMessageHtml prevents
    // double-counting), so a single data-cite-ref="c1" element is what
    // the user sees.
    expect(html.match(/data-cite-ref="c1"/g)?.length).toBe(1)
  })

  it('leaves a ref alone when neither current nor prior citations match', () => {
    const html = renderMessageHtml(
      'Unknown [c99] reference.',
      [cite('c1')],
      [cite('c2')],
    )
    expect(html).not.toContain('data-cite-ref="c99"')
    expect(html).toContain('[c99]')
  })
})
