// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { chatApi, streamChat } from '$lib/api/chat'
import { modelsStore } from '$lib/stores/models.svelte'
import { docViewer } from '$lib/stores/doc-viewer.svelte'
import { toCitation } from '$lib/types/citation'
import type {
  Chat,
  ChatMessage,
  ChatStep,
  FileRef,
  OutgoingMessage,
  TemplateRef,
  WorkflowRef,
} from '$lib/types/chat'

/** Replace the most recent generic `tool` step for `toolName` with a
 *  typed step (doc_read / doc_find / workflow_applied), so the UI shows
 *  "Read contract.pdf" instead of "Running read_document…". Appends
 *  when no matching tool step is found. */
function upgradeToolStep(
  steps: ChatStep[],
  toolName: string,
  typed: ChatStep,
): void {
  for (let i = steps.length - 1; i >= 0; i--) {
    const s = steps[i]
    if (s.kind === 'tool' && s.name === toolName) {
      steps[i] = typed
      return
    }
  }
  steps.push(typed)
}

/** Attachments selected in the composer for the next user message. */
export interface SendAttachments {
  files?: FileRef[]
  workflow?: WorkflowRef
  template?: TemplateRef
  /** When set on a brand-new chat, the chat is created under this project. */
  projectId?: string
}

function toOutgoing(m: ChatMessage): OutgoingMessage {
  return {
    role: m.role,
    content: m.content,
    ...(m.workflow ? { workflow: m.workflow } : {}),
    ...(m.template ? { template: m.template } : {}),
    ...(m.files && m.files.length
      ? {
          files: m.files.map((f) => ({
            document_id: f.document_id,
            ...(f.piiProtected ? { pii_protected: true } : {}),
          })),
        }
      : {}),
  }
}

/**
 * Payload-size guardrail for `/chat` streaming requests.
 *
 * Quality-first strategy: let the backend summarizer handle normal
 * context management (it is model-aware and triggers near 80% of the
 * real context window). Frontend compaction is only an emergency brake
 * for extremely large payloads (pathological long sessions), to protect
 * transport/latency without degrading answer quality in regular chats.
 */
const EMERGENCY_MAX_CONTEXT_MESSAGES = 80
const EMERGENCY_MAX_CONTEXT_CHARS = 240_000
const EMERGENCY_PINNED_STRUCTURED_USER_MESSAGES = 6

interface EmergencyBudget {
  maxMessages: number
  maxChars: number
}

function normalizeModelId(id: string): string {
  return id.toLowerCase().replace(/^openai:/, '').replace(/^local:/, '')
}

function contextWindowForModel(modelId: string): number | undefined {
  const normalized = normalizeModelId(modelId)
  const hit = modelsStore.allModels.find((m) => normalizeModelId(m.id) === normalized)
  return hit?.context_window
}

function emergencyBudgetForModel(modelId?: string | null): EmergencyBudget {
  const id = (modelId ?? '').trim()
  const normalized = normalizeModelId(id)
  const cw = id ? contextWindowForModel(id) : undefined

  if (cw != null) {
    if (cw >= 1_000_000) return { maxMessages: 220, maxChars: 1_500_000 }
    if (cw >= 200_000) return { maxMessages: 160, maxChars: 900_000 }
    if (cw >= 128_000) return { maxMessages: 140, maxChars: 700_000 }
    if (cw >= 32_000) return { maxMessages: 100, maxChars: 360_000 }
  }

  // Local/OpenAI-compatible endpoints often serve Ollama/vLLM with larger
  // context windows than the conservative generic fallback.
  if (
    normalized.startsWith('ollama') ||
    normalized.includes('vllm') ||
    normalized.startsWith('qwen') ||
    normalized.startsWith('llama') ||
    id.toLowerCase().startsWith('local:')
  ) {
    return { maxMessages: 140, maxChars: 700_000 }
  }

  return {
    maxMessages: EMERGENCY_MAX_CONTEXT_MESSAGES,
    maxChars: EMERGENCY_MAX_CONTEXT_CHARS,
  }
}

function estimateOutgoingChars(all: OutgoingMessage[]): number {
  return all.reduce((n, m) => {
    const filesCost = (m.files?.length ?? 0) * 40
    const workflowCost = m.workflow?.title.length ?? 0
    const templateCost = m.template?.title.length ?? 0
    return n + m.content.length + filesCost + workflowCost + templateCost + 24
  }, 0)
}

function compactOutgoingContext(all: OutgoingMessage[], modelId?: string | null): OutgoingMessage[] {
  const budget = emergencyBudgetForModel(modelId)
  const estimatedChars = estimateOutgoingChars(all)
  const emergency = all.length > budget.maxMessages || estimatedChars > budget.maxChars
  if (!emergency) return all

  const tailStart = Math.max(0, all.length - budget.maxMessages)
  const keep = new Set<number>()

  for (let i = tailStart; i < all.length; i++) keep.add(i)

  // Keep first user turn as a stable task anchor.
  const firstUserIdx = all.findIndex((m) => m.role === 'user')
  if (firstUserIdx >= 0) keep.add(firstUserIdx)

  // Keep a few older structured user turns (attachments/workflow/template)
  // because they often encode constraints the model should not forget.
  let pinned = 0
  for (let i = tailStart - 1; i >= 0 && pinned < EMERGENCY_PINNED_STRUCTURED_USER_MESSAGES; i--) {
    const m = all[i]
    const structured =
      m.role === 'user' && (!!m.workflow || !!m.template || (m.files?.length ?? 0) > 0)
    if (structured) {
      keep.add(i)
      pinned++
    }
  }

  return all.filter((_, idx) => keep.has(idx))
}

function createChatStore() {
  let chats = $state<Chat[]>([])
  let activeId = $state<string | null>(null)
  let messages = $state<ChatMessage[]>([])
  let streaming = $state<boolean>(false)
  /** Model id of the reply currently streaming, for the interrupt warning. */
  let streamingModel = $state<string | null>(null)
  let loadingChats = $state<boolean>(false)
  let loadingMessages = $state<boolean>(false)
  let error = $state<string | null>(null)
  let abortCtrl: AbortController | null = null
  // Monotonic — bumped by `newChat({ clearProject: true })`. The
  // composer (ChatInput.svelte) watches it to drop its project chip
  // when the user explicitly says "voglio una chat indipendente"
  // after a confirm modal in the sidebar.
  let clearProjectTick = $state(0)

  async function refreshChats() {
    loadingChats = true
    try {
      const res = await chatApi.list()
      chats = res.chats
    } catch (e) {
      error = (e as Error).message
    } finally {
      loadingChats = false
    }
  }

  return {
    get chats() {
      return chats
    },
    get activeId() {
      return activeId
    },
    /** project_id of the active chat — null for a global chat. Drives
     *  the composer's project chip + domain-scoped pickers. */
    get activeProjectId(): string | null {
      return chats.find((c) => c.id === activeId)?.project_id ?? null
    },
    get messages() {
      return messages
    },
    get streaming() {
      return streaming
    },
    get streamingModel() {
      return streamingModel
    },
    get loadingChats() {
      return loadingChats
    },
    get loadingMessages() {
      return loadingMessages
    },
    get error() {
      return error
    },

    refreshChats,

    /** Start a fresh, unsaved chat.
     *
     *  Pass `{ clearProject: true }` to also tell the composer to drop
     *  any project chip it was carrying. Default (no opts) preserves
     *  the chip — that's the right behaviour when the user picks
     *  "Sì, mantieni il progetto" from the new-chat confirm modal in
     *  Shell.svelte, because ChatInput's existing $effect only loads
     *  a project, never clears one (an absent `activeProjectId` is a
     *  no-op there, not a reset signal).
     *
     *  The clear is communicated to the composer via the
     *  `clearProjectTick` getter — a monotonic counter the composer's
     *  $effect watches, bumping it triggers exactly one chip reset.
     *  Counter pattern (not a boolean) because consecutive "clear"
     *  calls without a chip in between still need to fire the effect. */
    newChat(opts: { clearProject?: boolean } = {}) {
      if (opts.clearProject) clearProjectTick++
      activeId = null
      messages = []
      error = null
      docViewer.closeAll()
    },

    /** Monotonic tick bumped on every `newChat({ clearProject: true })`
     *  call. ChatInput.svelte tracks this to know when to drop its
     *  project chip on next-chat creation. See [[newChat]] for why. */
    get clearProjectTick() {
      return clearProjectTick
    },

    async selectChat(id: string) {
      activeId = id
      messages = []
      error = null
      loadingMessages = true
      try {
        const res = await chatApi.messages(id)
        messages = res.messages.map((m) => {
          const isAssistant = m.role === 'assistant'
          const annots = Array.isArray(m.annotations) ? m.annotations : []
          // Re-hydrate persisted assistant events (today: doc_created)
          // into the same `steps` shape the live SSE pipeline builds in
          // `onDocCreated`, so the download card for a generated docx /
          // xlsx is back where it was when the chat is reopened.
          const events = Array.isArray(m.events) ? m.events : []
          const steps = isAssistant
            ? events
                .map((e) => e as Record<string, unknown>)
                .filter((e) => e.type === 'doc_created')
                .map((e) => ({
                  kind: 'doc' as const,
                  filename: String(e.filename ?? ''),
                  documentId: String(e.document_id ?? ''),
                  downloadUrl: String(e.download_url ?? ''),
                }))
            : []
          return {
            role: isAssistant ? ('assistant' as const) : ('user' as const),
            content: m.content,
            ...(isAssistant && annots.length
              ? { citations: annots.map((a) => toCitation(a as Record<string, unknown>)) }
              : {}),
            ...(steps.length ? { steps } : {}),
          }
        })
      } catch (e) {
        error = (e as Error).message
      } finally {
        loadingMessages = false
      }
    },

    async remove(id: string) {
      await chatApi.remove(id)
      chats = chats.filter((c) => c.id !== id)
      if (activeId === id) {
        activeId = null
        messages = []
      }
    },

    async rename(id: string, title: string) {
      const next = title.trim()
      if (!next) return
      await chatApi.rename(id, next)
      chats = chats.map((c) => (c.id === id ? { ...c, title: next } : c))
    },

    /** Send a user message and stream the assistant reply. */
    async send(text: string, attach: SendAttachments = {}) {
      if (streaming || !text.trim()) return
      error = null
      const isFirstTurn = messages.length === 0

      // A project attachment on a new chat needs a real chat row first.
      if (!activeId && attach.projectId) {
        try {
          const created = await chatApi.createRecord(attach.projectId)
          activeId = created.id
        } catch (e) {
          error = (e as Error).message
          return
        }
      }

      const userMsg: ChatMessage = {
        role: 'user',
        content: text.trim(),
        ...(attach.workflow ? { workflow: attach.workflow } : {}),
        ...(attach.template ? { template: attach.template } : {}),
        ...(attach.files && attach.files.length ? { files: attach.files } : {}),
      }
      messages = [...messages, userMsg]
      const model = modelsStore.settings.main_model
      const outgoingAll = messages.map(toOutgoing)
      const budget = emergencyBudgetForModel(model)
      const outgoing = compactOutgoingContext(outgoingAll, model)
      if (outgoing.length !== outgoingAll.length) {
        console.info('[chat] context compacted before stream', {
          before: outgoingAll.length,
          after: outgoing.length,
          maxMessages: budget.maxMessages,
          maxChars: budget.maxChars,
          model: model ?? '(backend default)',
          strategy: 'quality-first-emergency-only',
        })
      }
      messages = [
        ...messages,
        { role: 'assistant', content: '', streaming: true, steps: [], citations: [] },
      ]
      streaming = true

      const assistant = () => {
        const last = messages[messages.length - 1]
        return last && last.role === 'assistant' ? last : null
      }

      streamingModel = model ?? null
      abortCtrl = streamChat(
        {
          messages: outgoing,
          ...(activeId ? { chat_id: activeId } : {}),
          ...(model ? { model } : {}),
        },
        {
          onChatId: (id) => {
            if (!activeId) activeId = id
          },
          onDelta: (delta) => {
            const m = assistant()
            if (m) m.content += delta
          },
          onContentReplace: (text) => {
            // Backend rewrote free-form `[doc-id: …]` references into
            // `[cN]` markers and is sending us the canonical body so
            // pills render on this turn (without needing a reload).
            const m = assistant()
            if (m) m.content = text
          },
          onReasoningDelta: (delta) => {
            const m = assistant()
            if (m) m.reasoning = (m.reasoning ?? '') + delta
          },
          onToolCallStart: (name) => {
            const m = assistant()
            if (!m) return
            m.steps ??= []
            // A new tool starting means any earlier tool has finished.
            for (const s of m.steps) if (s.kind === 'tool') s.done = true
            m.steps.push({ kind: 'tool', name, elapsedSecs: 0, done: false })
          },
          onToolCallProgress: (name, secs) => {
            const m = assistant()
            if (!m?.steps) return
            for (let i = m.steps.length - 1; i >= 0; i--) {
              const s = m.steps[i]
              if (s.kind === 'tool' && s.name === name && !s.done) {
                s.elapsedSecs = secs
                break
              }
            }
          },
          onToolCallDone: (name) => {
            // The tool finished — resolve its spinner to a check now,
            // instead of waiting for the next tool to start (which on a
            // docx generation is the whole body-writing phase).
            const m = assistant()
            if (!m?.steps) return
            for (let i = m.steps.length - 1; i >= 0; i--) {
              const s = m.steps[i]
              if (s.kind === 'tool' && s.name === name && !s.done) {
                s.done = true
                break
              }
            }
          },
          onDocCreated: (doc) => {
            const m = assistant()
            if (!m) return
            m.steps ??= []
            m.steps.push({
              kind: 'doc',
              filename: doc.filename,
              documentId: doc.documentId,
              downloadUrl: doc.downloadUrl,
            })
          },
          onDocRead: (filename) => {
            const m = assistant()
            if (!m) return
            m.steps ??= []
            upgradeToolStep(m.steps, 'read_document', { kind: 'doc_read', filename })
          },
          onDocFind: (query, filename, occurrences) => {
            const m = assistant()
            if (!m) return
            m.steps ??= []
            upgradeToolStep(m.steps, 'find_in_document', {
              kind: 'doc_find',
              query,
              filename,
              occurrences,
            })
          },
          onWorkflowApplied: (title) => {
            const m = assistant()
            if (!m) return
            m.steps ??= []
            upgradeToolStep(m.steps, 'read_workflow', {
              kind: 'workflow_applied',
              title,
            })
          },
          onDocExtractStart: (filename) => {
            const m = assistant()
            if (!m) return
            m.steps ??= []
            // Only push if there isn't already an in-flight extract
            // step for this filename — guards against duplicate
            // start events from the same doc.
            const open = m.steps.find(
              (s) => s.kind === 'doc_extract' && s.filename === filename && !s.done,
            )
            if (open) return
            m.steps.push({
              kind: 'doc_extract',
              filename,
              chars: 0,
              done: false,
            })
          },
          onDocExtractDone: (filename, chars) => {
            const m = assistant()
            if (!m) return
            m.steps ??= []
            for (let i = m.steps.length - 1; i >= 0; i--) {
              const s = m.steps[i]
              if (s.kind === 'doc_extract' && s.filename === filename && !s.done) {
                s.chars = chars
                s.done = true
                return
              }
            }
            // No start event was seen — synthesise a done step.
            m.steps.push({
              kind: 'doc_extract',
              filename,
              chars,
              done: true,
            })
          },
          onPiiRedactStart: (filename, total) => {
            const m = assistant()
            if (!m) return
            m.steps ??= []
            m.steps.push({
              kind: 'pii_redact',
              filename,
              current: 0,
              total,
              done: false,
            })
          },
          onPiiRedactProgress: (filename, current, total) => {
            const m = assistant()
            if (!m) return
            m.steps ??= []
            // Find the most-recent unfinished pii_redact step for
            // this file and bump its counter. A second send during
            // the same turn (rare but possible) would just push a
            // new step rather than confusing the existing one.
            for (let i = m.steps.length - 1; i >= 0; i--) {
              const s = m.steps[i]
              if (s.kind === 'pii_redact' && s.filename === filename && !s.done) {
                s.current = current
                s.total = total
                return
              }
            }
            // No start event was seen — synthesise one so the UI
            // doesn't drop the progress on the floor.
            m.steps.push({
              kind: 'pii_redact',
              filename,
              current,
              total,
              done: false,
            })
          },
          onPiiRedactDone: (filename) => {
            const m = assistant()
            if (!m) return
            m.steps ??= []
            for (let i = m.steps.length - 1; i >= 0; i--) {
              const s = m.steps[i]
              if (s.kind === 'pii_redact' && s.filename === filename && !s.done) {
                s.done = true
                if (s.total > 0) s.current = s.total
                return
              }
            }
          },
          onCitations: (data) => {
            const ev = data as { citations?: unknown[] }
            const list = Array.isArray(ev.citations) ? ev.citations : []
            const m = assistant()
            if (m) m.citations = list.map((c) => toCitation(c as Record<string, unknown>))
          },
          onError: (msg) => {
            // Surface to the chat UI banner AND the DevTools console.
            // The browser-side console.warn is the v0.6.2 diagnostic
            // hook: when the backend retry-with-backoff + semaphore
            // gate can't fully mask a Mistral 429 (genuinely
            // exhausted quota, or an upstream outage), this is
            // where the user sees the structured error in the
            // Developer Tools (F12 → Console) for triage.
            console.warn(
              '[chat] LLM error:',
              msg,
              { activeModel: streamingModel, chatId: activeId },
            )
            error = msg
          },
          onDone: () => {
            streaming = false
            streamingModel = null
            abortCtrl = null
            const m = assistant()
            if (m) {
              m.streaming = false
              for (const s of m.steps ?? []) if (s.kind === 'tool') s.done = true
            }
            // After the first exchange of a fresh chat, ask the backend
            // to title it from the opening message, then refresh.
            if (isFirstTurn && activeId) {
              void chatApi
                .generateTitle(activeId)
                .catch(() => undefined)
                .then(() => refreshChats())
            } else {
              void refreshChats()
            }
          },
        },
      )
    },

    /** Stop an in-flight generation. */
    abort() {
      abortCtrl?.abort()
      abortCtrl = null
      streaming = false
      streamingModel = null
      const last = messages[messages.length - 1]
      if (last && last.role === 'assistant') last.streaming = false
    },
  }
}

export const chatStore = createChatStore()
