// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

/**
 * Robust passage highlighting over rendered DOM. Used by every document
 * renderer (PDF text layer, DOCX, plain text) to mark a cited quote.
 *
 * The model's quote rarely matches the rendered text byte-for-byte:
 * whitespace, line breaks and punctuation differ, and the text layer
 * splits words across spans. The matcher therefore normalises both
 * sides to letters+digits only (lower-cased) and matches on that — the
 * one approach that survives all those differences.
 */

const MARK_CLASS = 'doc-hl'
const ALNUM = /[a-zA-Z0-9]/
const COMBINING = /[̀-ͯ]/g

/** Remove any highlight marks previously added inside `container`. */
export function clearHighlights(container: HTMLElement): void {
  for (const mark of Array.from(container.querySelectorAll(`mark.${MARK_CLASS}`))) {
    const parent = mark.parentNode
    if (!parent) continue
    while (mark.firstChild) parent.insertBefore(mark.firstChild, mark)
    parent.removeChild(mark)
    parent.normalize()
  }
}

/**
 * Strip every non-ASCII-alphanumeric character, lower-case the rest.
 * The string is first NFD-normalised so accented letters decompose to
 * base + combining mark; the combining mark is then dropped. This
 * makes Italian quotes ("perché", "città", "più") match a document
 * regardless of whether either side is NFC- or NFD-encoded, and
 * regardless of whether the source preserves accents at all. CR / LF /
 * tabs / `\r\n` / non-breaking spaces are stripped uniformly because
 * none survive the alphanumeric filter — the haystack is the same
 * sequence whether the document came in with Unix LF, Windows CRLF,
 * or with the model's quote wrapped across literal newlines.
 */
function onlyLetters(s: string): string {
  const decomposed = s.normalize('NFD').replace(COMBINING, '')
  let out = ''
  for (const c of decomposed) if (ALNUM.test(c)) out += c.toLowerCase()
  return out
}

interface NodeInfo {
  node: Text
  /** stripped-char index → original-char index within `node.data`. */
  s2o: number[]
  /** offset of this node's stripped text within the global haystack. */
  start: number
  len: number
}

/** Collect text nodes + the concatenated letters-only haystack. */
function collect(container: HTMLElement): { full: string; nodes: NodeInfo[] } {
  const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT)
  const nodes: NodeInfo[] = []
  let full = ''
  let n: Node | null
  while ((n = walker.nextNode())) {
    const node = n as Text
    const orig = node.data
    const s2o: number[] = []
    let stripped = ''
    for (let i = 0; i < orig.length; i++) {
      if (ALNUM.test(orig[i])) {
        s2o.push(i)
        stripped += orig[i].toLowerCase()
      }
    }
    nodes.push({ node, s2o, start: full.length, len: stripped.length })
    full += stripped
  }
  return { full, nodes }
}

/**
 * Wrap the haystack range [from, to) — described by `nodes` — in <mark>
 * elements (one per overlapped text node). Interior punctuation between
 * matched letters is included naturally. Returns the first mark.
 */
function wrapRange(nodes: NodeInfo[], from: number, to: number): HTMLElement | null {
  let first: HTMLElement | null = null
  for (const info of nodes) {
    const nodeEnd = info.start + info.len
    if (nodeEnd <= from || info.start >= to || info.len === 0) continue
    const localA = Math.max(0, from - info.start)
    const localB = Math.min(info.len, to - info.start)
    if (localB <= localA) continue

    const origA = info.s2o[localA]
    const origB = info.s2o[localB - 1] + 1

    let target = info.node
    if (origA > 0) target = target.splitText(origA)
    if (origB - origA < target.data.length) target.splitText(origB - origA)

    const mark = document.createElement('mark')
    mark.className = MARK_CLASS
    target.parentNode?.insertBefore(mark, target)
    mark.appendChild(target)
    if (!first) first = mark
  }
  return first
}

/**
 * Highlight `quote` inside `container`. Matches on the letters-only
 * normalisation; when the full quote can't be located it retries with
 * shrinking prefixes (the model often paraphrases the tail).
 * Returns the first mark element, or null.
 */
export function highlightQuote(container: HTMLElement, quote: string): HTMLElement | null {
  const needle = onlyLetters(quote)
  if (needle.length < 4) return null
  const { full, nodes } = collect(container)

  // Try the full quote, then progressively shorter prefixes (model
  // often paraphrases the tail). Add `min(needle.length, 24)` as a
  // last-resort cut so single-line references like "Settimane 1-4"
  // still find a foothold somewhere in the body. 4 is the floor.
  const lens = [needle.length, 160, 120, 80, 50, 32, 24, 16, 12, 8]
  for (const len of lens) {
    if (len > needle.length || len < 4) continue
    const slice = needle.slice(0, len)
    const pos = full.indexOf(slice)
    if (pos >= 0) return wrapRange(nodes, pos, pos + slice.length)
  }
  return null
}

/**
 * Highlight a citation quote. The quote may span a page break (sentinel)
 * and may use `…`/`...` to mark elided text — each segment is matched
 * and highlighted independently. Returns the first mark for scrolling.
 */
export function highlightCitation(
  container: HTMLElement,
  quote: string,
  pageBreakSentinel: string,
): HTMLElement | null {
  clearHighlights(container)
  const segments = quote
    .split(pageBreakSentinel)
    .flatMap((s) => s.split(/\.{3,}|…/))
    .map((s) => s.trim())
    .filter((s) => onlyLetters(s).length >= 4)
  let first: HTMLElement | null = null
  for (const seg of segments) {
    const m = highlightQuote(container, seg)
    if (m && !first) first = m
  }
  return first
}

/**
 * Free-text search: highlight every occurrence of `term` in `container`
 * and return the marks in document order (for find next/previous).
 */
export function findAll(container: HTMLElement, term: string): HTMLElement[] {
  clearHighlights(container)
  const needle = onlyLetters(term)
  if (needle.length < 2) return []
  const marks: HTMLElement[] = []
  // Re-collect after each wrap: splitting shifts later node offsets.
  for (let guard = 0; guard < 2000; guard++) {
    const { full, nodes } = collect(container)
    let searchFrom = 0
    // Skip past ranges already wrapped (their marks are still in the DOM).
    const already = marks.length
    let pos = -1
    let seen = 0
    for (;;) {
      const idx = full.indexOf(needle, searchFrom)
      if (idx < 0) break
      if (seen === already) {
        pos = idx
        break
      }
      seen++
      searchFrom = idx + needle.length
    }
    if (pos < 0) break
    const m = wrapRange(nodes, pos, pos + needle.length)
    if (!m) break
    marks.push(m)
  }
  return marks
}
