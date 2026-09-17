// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

/**
 * Document-viewer store. Drives the resizable side panel that holds one
 * browser-style tab per opened document. A citation pill, a generated-
 * document card or a "read document" step all funnel through here; the
 * panel reuses an existing tab for the same document and only refreshes
 * its highlight/page hint.
 */

import type { Citation } from '$lib/types/citation'

/** How a tab is shown — plain, citation-focused, or tracked-changes mode. */
export type ViewerMode = 'citation' | 'plain' | 'tracked'

/** Visual policy for DOCX tracked changes while in `mode: tracked`. */
export type TrackedPolicy = 'show' | 'accept' | 'reject'

/** Per-chat accept/reject state — see migration 0029. */
export type DocDecision = 'accepted' | 'rejected'

export interface ViewerTab {
  /** Unique per tab (a document may be reopened from several places). */
  id: string
  /** Backing identifier (`document` UUID or synthetic `kb:<path>`). */
  docId: string
  /** Fetch source for this tab. */
  source: 'document' | 'kb'
  /** Synced KB source path (only for `source: "kb"`). */
  kbPath?: string
  /** Filename / label shown on the tab. */
  title: string
  mode: ViewerMode
  /** Passage to highlight inside the rendered document, if any. */
  quote?: string
  /** Tracked-changes visual policy (DOCX only). */
  trackedPolicy: TrackedPolicy
  /** Page hint (number) or `"41-42"` range string. */
  page?: number | string
  /** Source label for the citation header card. */
  citationSource?: string
  /** Current accept/reject state for this document in this chat. */
  decision: DocDecision
  /** Archived reject reason (kept across flips so a re-reject doesn't
   *  ask the user to retype). */
  decisionReason?: string | null
  /** LLM-generated summary captured at reject-time. */
  decisionSummary?: string | null
}

interface OpenOptions {
  docId: string
  source?: 'document' | 'kb'
  kbPath?: string
  title: string
  mode?: ViewerMode
  quote?: string
  page?: number | string
  citationSource?: string
}

const MIN_WIDTH = 360
const MAX_WIDTH = 1100
const DEFAULT_WIDTH = 600

function createDocViewer() {
  let tabs = $state<ViewerTab[]>([])
  let activeId = $state<string | null>(null)
  let open = $state(false)
  let collapsed = $state(false)
  let width = $state(DEFAULT_WIDTH)
  /** Bumped whenever an existing tab is re-targeted, so the view re-runs
   *  its highlight pass even though the tab object identity is stable. */
  let revision = $state(0)

  function open_(opts: OpenOptions) {
    const existing = tabs.find((t) => t.docId === opts.docId)
    if (existing) {
      existing.source = opts.source ?? existing.source
      existing.kbPath = opts.kbPath
      existing.mode = opts.mode ?? existing.mode
      existing.quote = opts.quote
      if (opts.mode !== 'tracked') existing.trackedPolicy = 'show'
      existing.page = opts.page
      existing.citationSource = opts.citationSource
      activeId = existing.id
      revision++
    } else {
      const tab: ViewerTab = {
        id:
          typeof crypto !== 'undefined' && crypto.randomUUID
            ? crypto.randomUUID()
            : `tab-${Date.now()}-${tabs.length}`,
        docId: opts.docId,
        source: opts.source ?? 'document',
        kbPath: opts.kbPath,
        title: opts.title,
        mode: opts.mode ?? 'plain',
        quote: opts.quote,
        trackedPolicy: 'show',
        page: opts.page,
        citationSource: opts.citationSource,
        // Default to "accepted" — matches the documents.decision
        // server-side default. The DocViewerPanel hydrates the real
        // state from `GET /document/:id` on open and then mutates it
        // through `setDecision` after the server confirms.
        decision: 'accepted',
        decisionReason: null,
        decisionSummary: null,
      }
      tabs = [...tabs, tab]
      activeId = tab.id
    }
    open = true
    collapsed = false
  }

  return {
    get tabs() {
      return tabs
    },
    get activeId() {
      return activeId
    },
    get activeTab() {
      return tabs.find((t) => t.id === activeId) ?? null
    },
    get open() {
      return open
    },
    get collapsed() {
      return collapsed
    },
    get width() {
      return width
    },

    /** Collapse the panel to a thin strip, or restore the prior width. */
    toggleCollapse() {
      collapsed = !collapsed
    },
    get revision() {
      return revision
    },

    /** Open (or re-target) a plain document view. */
    openDocument(docId: string, title: string) {
      open_({ docId, title, mode: 'plain', source: 'document' })
    },

    /** Open a citation: highlights the quoted passage on the cited page. */
    openCitation(c: Citation) {
      const isKb = !!c.kbPath
      const docId = isKb ? `kb:${c.kbPath}` : c.docId
      open_({
        docId,
        source: isKb ? 'kb' : 'document',
        ...(isKb ? { kbPath: c.kbPath } : {}),
        title: c.source || c.docId,
        mode: 'citation',
        quote: c.quote,
        page: c.page,
        citationSource: c.source,
      })
    },

    select(id: string) {
      if (tabs.some((t) => t.id === id)) {
        activeId = id
      }
    },

    setMode(mode: ViewerMode) {
      const tab = tabs.find((t) => t.id === activeId)
      if (!tab) return
      tab.mode = mode
      if (mode !== 'tracked') tab.trackedPolicy = 'show'
      revision++
    },

    setTrackedPolicy(policy: TrackedPolicy) {
      const tab = tabs.find((t) => t.id === activeId)
      if (!tab) return
      tab.mode = 'tracked'
      tab.trackedPolicy = policy
      revision++
    },

    /** Reflect the accept/reject decision the backend just confirmed
     *  into the active tab. Pure state mutation — the network call to
     *  `POST /document/:id/decision` lives on the component side so
     *  the modal can wait on the promise and show the generated
     *  summary before closing. `reason` / `summary` are kept across
     *  flips so a future re-reject can pre-fill them. */
    setDecision(
      docId: string,
      decision: DocDecision,
      reason: string | null,
      summary: string | null,
    ) {
      const tab = tabs.find((t) => t.docId === docId)
      if (!tab) return
      tab.decision = decision
      // Only overwrite the archived reason / summary when the backend
      // hands us new ones — on a re-accept the backend returns the
      // previously-stored values, so we just mirror them verbatim.
      tab.decisionReason = reason
      tab.decisionSummary = summary
      revision++
    },

    closeTab(id: string) {
      const idx = tabs.findIndex((t) => t.id === id)
      if (idx < 0) return
      tabs = tabs.filter((t) => t.id !== id)
      if (activeId === id) {
        activeId = tabs[Math.min(idx, tabs.length - 1)]?.id ?? null
      }
      if (tabs.length === 0) open = false
    },

    closeAll() {
      tabs = []
      activeId = null
      open = false
    },

    setWidth(px: number) {
      width = Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, Math.round(px)))
    },
  }
}

export const docViewer = createDocViewer()
