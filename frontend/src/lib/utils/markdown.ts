// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { marked } from 'marked'
import DOMPurify from 'dompurify'

/**
 * Render Markdown to sanitised HTML. Used for assistant chat messages.
 * marked produces the HTML; DOMPurify strips anything unsafe before it
 * reaches the DOM.
 */
export function renderMarkdown(md: string): string {
  const html = marked.parse(md ?? '', { async: false, breaks: true }) as string
  return DOMPurify.sanitize(html)
}
