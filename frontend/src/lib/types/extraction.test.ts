import { describe, it, expect, beforeAll } from 'vitest'
import { splitReason, isUnreadable, extractionMessage } from './extraction'
import { i18n } from '$lib/stores/i18n.svelte'
// Suffixed: a bare `it` would shadow vitest's own `it`.
import itLocale from '../../../locales/it.json'
import enLocale from '../../../locales/en.json'
import frLocale from '../../../locales/fr.json'
import deLocale from '../../../locales/de.json'
import esLocale from '../../../locales/es.json'
import ptLocale from '../../../locales/pt.json'

/** Every code the backend can store, from src/ingest/outcome.rs. */
const CODES = [
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
]

const CATALOGUES: Record<string, Record<string, unknown>> = {
  it: itLocale,
  en: enLocale,
  fr: frLocale,
  de: deLocale,
  es: esLocale,
  pt: ptLocale,
}

describe('splitReason', () => {
  it('reads a bare code', () => {
    expect(splitReason('scanned_pdf')).toEqual({ code: 'scanned_pdf' })
  })

  it('separates the technical detail that read_failed carries', () => {
    // The detail exists so the user can paste it into a report; it must
    // survive translation rather than being swallowed.
    expect(splitReason('read_failed: pdfium load error 0x8007')).toEqual({
      code: 'read_failed',
      detail: 'pdfium load error 0x8007',
    })
  })

  it('treats a missing reason as no code', () => {
    expect(splitReason(null).code).toBe('')
    expect(splitReason(undefined).code).toBe('')
  })
})

describe('isUnreadable', () => {
  it('is true exactly for the two failing verdicts', () => {
    expect(isUnreadable('no_text')).toBe(true)
    expect(isUnreadable('failed')).toBe(true)
    expect(isUnreadable('ready')).toBe(false)
    expect(isUnreadable(undefined)).toBe(false)
  })
})

describe('the six catalogues', () => {
  it('explain every code the backend can produce', () => {
    for (const [locale, cat] of Object.entries(CATALOGUES)) {
      const reasons = (cat.Ingest as Record<string, Record<string, string>>)?.reason
      expect(reasons, `${locale} has no Ingest.reason block`).toBeTruthy()
      for (const code of [...CODES, 'unknown']) {
        expect(reasons[code], `${locale} is missing ${code}`).toBeTruthy()
      }
    }
  })

  it('never leave a sentence untranslated from Italian', () => {
    // A copy-pasted Italian string in another catalogue is the usual
    // way a "six languages" claim quietly becomes five.
    const itReasons = (itLocale as any).Ingest.reason as Record<string, string>
    for (const [locale, cat] of Object.entries(CATALOGUES)) {
      if (locale === 'it') continue
      const reasons = (cat as any).Ingest.reason as Record<string, string>
      for (const code of CODES) {
        expect(reasons[code], `${locale}.${code} is still the Italian text`).not.toBe(
          itReasons[code],
        )
      }
    }
  })
})

describe('extractionMessage', () => {
  beforeAll(() => {
    i18n.setLocale?.('it')
  })

  it('explains a known code in words, not in code', () => {
    const msg = extractionMessage('scanned_pdf')
    expect(msg).not.toBe('scanned_pdf')
    expect(msg.length).toBeGreaterThan(10)
  })

  it('keeps the technical detail alongside the explanation', () => {
    const msg = extractionMessage('read_failed: pdfium load error 0x8007')
    expect(msg).toContain('0x8007')
  })

  it('stays honest about a code this build does not know', () => {
    // A newer backend may ship a code this frontend has never seen. The
    // interface must still say something true about the file rather
    // than printing the raw identifier or nothing at all.
    const msg = extractionMessage('qualcosa_di_nuovo')
    expect(msg).not.toContain('qualcosa_di_nuovo')
    expect(msg.length).toBeGreaterThan(5)
  })
})
