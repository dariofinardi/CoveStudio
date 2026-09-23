/**
 * What the backend says about a document it onboarded.
 *
 * `documents.status` plus `extraction_reason` travel together: the
 * status says whether the text is usable, the reason says why not. The
 * reason is a **canonical English code** (`scanned_pdf`,
 * `encrypted_pdf`, …) produced by `src/ingest/outcome.rs`, not a
 * sentence — so it can be shown in any of the six languages, and so the
 * same value works in the composer, in the data-sources panel and in
 * the prompt the model receives.
 *
 * For `read_failed` the backend appends the technical detail after a
 * colon (`read_failed: pdfium load error …`). We translate the code and
 * show the tail verbatim: a sentence the user can paste into a report
 * is worth more than a tidy message that hides which file broke.
 */
import { i18n } from '$lib/stores/i18n.svelte'

export type ExtractionStatus = 'ready' | 'no_text' | 'failed'

/** Splits `code: detail` into its two halves. */
export function splitReason(reason: string | null | undefined): {
  code: string
  detail?: string
} {
  if (!reason) return { code: '' }
  const i = reason.indexOf(':')
  if (i === -1) return { code: reason.trim() }
  return {
    code: reason.slice(0, i).trim(),
    detail: reason.slice(i + 1).trim() || undefined,
  }
}

/** Codes this build knows how to explain. */
const KNOWN = new Set([
  'scanned_pdf',
  'image_only',
  'empty_file',
  'office_no_text',
  'image_needs_vision_model',
  'encrypted_pdf',
  'unsupported_format',
  'corrupt_file',
  'read_failed',
  'text_layer_unreliable',
])

/**
 * One sentence explaining why a document carries no usable text, in the
 * user's language. An unknown code falls back to a true sentence rather
 * than to an empty string: a new code shipped by a newer backend must
 * never make the interface go silent about a file.
 */
export function extractionMessage(reason: string | null | undefined): string {
  const { code, detail } = splitReason(reason)
  const key = KNOWN.has(code) ? code : 'unknown'
  const base = i18n.t(`Ingest.reason.${key}`)
  return detail ? `${base} (${detail})` : base
}

/** True when the document's text cannot be used to answer. */
export function isUnreadable(status: string | null | undefined): boolean {
  return status === 'no_text' || status === 'failed'
}
