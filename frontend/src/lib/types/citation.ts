// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

/**
 * Citation model. The assistant emits inline markers `[g1]`, `[c2]`,
 * `[p3]` in its prose and a trailing machine-readable block; the
 * backend parses that block into a `citations` SSE event whose entries
 * map onto this type.
 */

/** Origin pool of a cited passage — drives the pill colour. */
export type CitationScope = 'document' | 'global' | 'project'

/** One resolved citation, keyed by its marker reference. */
export interface Citation {
  /** Marker text without brackets: `g1`, `c2`, `p3`. */
  ref: string
  /** Pool the passage came from. */
  scope: CitationScope
  /** Document / knowledge-base chunk identifier. */
  docId: string
  /** Human-readable source label (filename or KB path). */
  source: string
  /** Synced KB source path (for global/project citations). */
  kbPath?: string
  /** Page number, or a `"41-42"` range for a quote crossing a page break. */
  page?: number | string
  /** The exact quoted passage (used to highlight inside the viewer). */
  quote: string
}

/**
 * Marker prefix → scope: `g` global library, `c` chat/attached
 * document, `p` project. A bare (legacy, prefix-less) ref also falls
 * through to `document`.
 */
export function scopeForRef(ref: string): CitationScope {
  if (ref.startsWith('g')) return 'global'
  if (ref.startsWith('p')) return 'project'
  return 'document'
}

/**
 * Normalise a raw `citations` SSE payload entry into a `Citation`.
 *
 * The backend entry carries BOTH `doc_id` (the chat-local label like
 * `doc-0`) and `document_id` (the real document UUID). The viewer needs
 * the UUID — `doc_id` would 404. KB chunks instead carry `path`.
 */
export function toCitation(raw: Record<string, unknown>): Citation {
  const ref = String(raw.ref ?? '').trim()
  const pageRaw = raw.page
  const realId = raw.document_id ?? raw.documentId
  const labelRaw = raw.doc_id ?? raw.docId
  // A `doc-N` handle is chat-local: the backend resolves it to a UUID
  // when it can, and a handle that survives here is one it could not
  // resolve (a label the model invented, a document deleted meanwhile).
  // Passing it on as an id makes the viewer request `/document/doc-1`,
  // get a 404 and blame a "removed source" — so it is dropped.
  const label =
    typeof labelRaw === 'string' && /^doc-\d+$/i.test(labelRaw.trim()) ? undefined : labelRaw
  const kbPathRaw = raw.path
  const kbPath = typeof kbPathRaw === 'string' && kbPathRaw.trim() ? kbPathRaw : undefined
  return {
    ref,
    scope: scopeForRef(ref),
    docId: String(realId ?? label ?? ''),
    source: String(raw.filename ?? raw.source ?? ''),
    ...(kbPath ? { kbPath } : {}),
    page:
      typeof pageRaw === 'number' || typeof pageRaw === 'string'
        ? (pageRaw as number | string)
        : undefined,
    quote: String(raw.quote ?? ''),
  }
}

/** Sentinel the backend embeds for a quote that spans a page break. */
export const PAGE_BREAK_SENTINEL = '[[PAGE_BREAK]]'
