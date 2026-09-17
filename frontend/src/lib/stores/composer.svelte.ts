// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

/**
 * One-shot prefill for the chat composer. Lets another screen (e.g. the
 * DOCX-templates page) hand an attachment to a freshly opened chat: the
 * composer reads it once on mount and clears it.
 */

import type { TemplateRef } from '$lib/types/chat'

function createComposerPrefill() {
  let pendingTemplate = $state<TemplateRef | null>(null)

  return {
    get hasPending() {
      return pendingTemplate !== null
    },
    /** Queue a template to attach to the next composer mount. */
    queueTemplate(t: TemplateRef) {
      pendingTemplate = t
    },
    /** Consume the queued template (returns it once, then clears). */
    takeTemplate(): TemplateRef | null {
      const t = pendingTemplate
      pendingTemplate = null
      return t
    },
  }
}

export const composerPrefill = createComposerPrefill()
