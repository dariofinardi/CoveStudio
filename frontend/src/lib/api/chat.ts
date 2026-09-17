// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { api } from './client'
import { apiBase } from '$lib/stores/api-base.svelte'
import { authStore } from '$lib/stores/auth.svelte'
import type { Chat, ChatMessage, OutgoingMessage } from '$lib/types/chat'

/** Non-streaming wrappers for `src/routes/chat.rs`. */
export const chatApi = {
  list: () => api<{ chats: Chat[] }>('/chat'),

  /** Create an empty chat record (no messages). */
  createRecord: (project_id?: string, title?: string) =>
    api<{ id: string }>('/chat', { method: 'POST', body: { project_id, title } }),

  messages: (id: string) =>
    api<{
      messages: {
        id: string
        role: string
        content: string
        created_at: string
        /** Persisted citation annotations (assistant turns). */
        annotations?: unknown[]
        /** Persisted non-text events (doc_created etc.) replayed on
         *  reload so generated-document cards survive a chat refresh. */
        events?: unknown[]
      }[]
    }>(`/chat/${encodeURIComponent(id)}/messages`),

  /** Every document a chat has interacted with, across five
   *  categories the chat archive is expected to retain:
   *  - origin='uploaded'    — composer attachment
   *  - origin='generated'   — tool output (e.g. generate_docx)
   *  - origin='project'     — inherited from the chat's project
   *  - origin='referenced'  — KB / corpora doc cited by the assistant
   *  Rejected docs surface in their original origin with
   *  decision='rejected'; the row is preserved across re-accept so the
   *  audit trail is intact. Server-side precedence: chat-linked
   *  (uploaded/generated) > project > referenced. */
  documents: (id: string) =>
    api<{
      documents: {
        id: string
        filename: string
        file_type: string
        decision: 'accepted' | 'rejected'
        decision_reason: string | null
        decision_summary: string | null
        origin: 'uploaded' | 'generated' | 'project' | 'referenced'
      }[]
    }>(`/chat/${encodeURIComponent(id)}/documents`),

  remove: (id: string) =>
    api<{ ok: boolean }>(`/chat/${encodeURIComponent(id)}`, { method: 'DELETE' }),

  rename: (id: string, title: string) =>
    api<unknown>(`/chat/${encodeURIComponent(id)}`, { method: 'PATCH', body: { title } }),

  /** Ask the backend to generate a title from the chat's first message. */
  generateTitle: (id: string) =>
    api<{ title: string | null }>(`/chat/${encodeURIComponent(id)}/generate-title`, {
      method: 'POST',
    }),
}

export interface DocCreatedEvent {
  filename: string
  documentId: string
  downloadUrl: string
}

export interface ChatStreamCallbacks {
  onChatId?: (chatId: string) => void
  onDelta: (text: string) => void
  /** Replace the entire assistant body — sent by the backend after it
   *  rewrites free-form `[doc-id: …]` references into canonical `[cN]`
   *  markers so pills render live on this turn (not only after reload). */
  onContentReplace?: (text: string) => void
  onToolCallStart?: (name: string) => void
  onToolCallProgress?: (name: string, elapsedSecs: number) => void
  onToolCallDone?: (name: string) => void
  onDocCreated?: (doc: DocCreatedEvent) => void
  onCitations?: (data: unknown) => void
  /** Model reasoning / "thinking" chunks — shown in a collapsible block. */
  onReasoningDelta?: (text: string) => void
  onReasoningDone?: () => void
  /** Typed builtin-tool steps — nicer than the generic tool_call_*. */
  onDocRead?: (filename: string) => void
  onDocFind?: (query: string, filename: string, occurrences: number) => void
  onWorkflowApplied?: (title: string) => void
  /** Text extraction has started on an attachment — pdfium /
   *  docx / xlsx / rtf, or a cached-text read. Lets the chat UI
   *  show a step BEFORE the PII pass so a multi-second
   *  pdfium pass on a long doc doesn't read as a frozen send. */
  onDocExtractStart?: (filename: string) => void
  /** Text extraction finished. `chars` is the size of the
   *  extracted text — surfaced in the step label as a sanity check. */
  onDocExtractDone?: (filename: string, chars: number) => void
  /** GLiNER2 PII redaction over an attachment started: total = number
   *  of 2000-char chunks the engine will process. */
  onPiiRedactStart?: (filename: string, total: number) => void
  /** Per-chunk progress while redaction is in flight. */
  onPiiRedactProgress?: (filename: string, current: number, total: number) => void
  /** Redaction finished (success or graceful fallback). */
  onPiiRedactDone?: (filename: string) => void
  onError: (message: string) => void
  onDone: () => void
}

function dispatchSseChunk(chunk: string, cb: ChatStreamCallbacks): void {
  for (const line of chunk.split('\n')) {
    const l = line.replace(/^\s+/, '')
    if (!l.startsWith('data:')) continue
    const data = l.slice(5).trim()
    if (!data || data === '[DONE]') continue
    let ev: Record<string, unknown>
    try {
      ev = JSON.parse(data)
    } catch {
      continue
    }
    switch (ev.type) {
      case 'chat_id':
        cb.onChatId?.(String(ev.chatId ?? ''))
        break
      case 'content_delta':
      case 'content':
        cb.onDelta(String(ev.text ?? ''))
        break
      case 'content_replace':
        cb.onContentReplace?.(String(ev.text ?? ''))
        break
      case 'tool_call_start':
        cb.onToolCallStart?.(String(ev.name ?? ''))
        break
      case 'tool_call_progress':
        cb.onToolCallProgress?.(String(ev.name ?? ''), Number(ev.elapsed_secs ?? 0))
        break
      case 'tool_call_done':
        cb.onToolCallDone?.(String(ev.name ?? ''))
        break
      case 'doc_created':
        cb.onDocCreated?.({
          filename: String(ev.filename ?? ''),
          documentId: String(ev.document_id ?? ''),
          downloadUrl: String(ev.download_url ?? ''),
        })
        break
      case 'citations': {
        const list = Array.isArray(ev.citations) ? ev.citations : []
        console.info(
          `[chat-sse] citations event: ${list.length} entries`,
          list.map((c) => (c as Record<string, unknown>).ref),
        )
        cb.onCitations?.(ev)
        break
      }
      case 'reasoning_delta':
        cb.onReasoningDelta?.(String(ev.text ?? ''))
        break
      case 'reasoning_done':
        cb.onReasoningDone?.()
        break
      case 'doc_read':
        cb.onDocRead?.(String(ev.filename ?? ev.doc_id ?? ''))
        break
      case 'doc_find':
        cb.onDocFind?.(
          String(ev.query ?? ''),
          String(ev.filename ?? ev.doc_id ?? ''),
          Number(ev.match_count ?? 0),
        )
        break
      case 'workflow_applied':
        cb.onWorkflowApplied?.(String(ev.title ?? ''))
        break
      case 'doc_extract_start':
        cb.onDocExtractStart?.(String(ev.filename ?? ''))
        break
      case 'doc_extract_done':
        cb.onDocExtractDone?.(String(ev.filename ?? ''), Number(ev.chars ?? 0))
        break
      case 'pii_redact_start':
        cb.onPiiRedactStart?.(String(ev.filename ?? ''), Number(ev.total ?? 0))
        break
      case 'pii_redact_progress':
        cb.onPiiRedactProgress?.(
          String(ev.filename ?? ''),
          Number(ev.current ?? 0),
          Number(ev.total ?? 0),
        )
        break
      case 'pii_redact_done':
        cb.onPiiRedactDone?.(String(ev.filename ?? ''))
        break
      case 'error':
        cb.onError(String(ev.message ?? 'stream error'))
        break
      default:
        console.debug('[chat-sse]', ev.type)
    }
  }
}

/**
 * Stream an assistant reply. POSTs the conversation to `/chat`
 * (the rich SSE path: handles document/workflow/template attachments)
 * and parses the `data: {type}` event stream. Uses fetch + ReadableStream
 * because EventSource cannot send the Authorization header.
 * Returns an AbortController so the caller can stop generation.
 */
export function streamChat(
  payload: { messages: OutgoingMessage[]; chat_id?: string; model?: string },
  cb: ChatStreamCallbacks,
): AbortController {
  const ctrl = new AbortController()
  let sawCitations = false
  const cbW: ChatStreamCallbacks = {
    ...cb,
    onCitations: (d) => {
      sawCitations = true
      cb.onCitations?.(d)
    },
  }

  void (async () => {
    console.info('[streamChat] request', {
      messages: payload.messages.length,
      model: payload.model ?? '(backend default)',
      attachedFiles: payload.messages.reduce((n, m) => n + (m.files?.length ?? 0), 0),
      piiProtectedFiles: payload.messages.reduce(
        (n, m) => n + (m.files?.filter((f) => f.pii_protected).length ?? 0),
        0,
      ),
    })
    let res: Response
    try {
      res = await fetch(new URL('/chat', apiBase.url || 'http://127.0.0.1:3001'), {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Accept: 'text/event-stream',
          Authorization: `Bearer ${authStore.token ?? ''}`,
        },
        body: JSON.stringify(payload),
        signal: ctrl.signal,
      })
    } catch (e) {
      if ((e as Error).name !== 'AbortError') cb.onError((e as Error).message)
      cb.onDone()
      return
    }

    if (!res.ok || !res.body) {
      cb.onError(`stream failed (${res.status})`)
      cb.onDone()
      return
    }

    const reader = res.body.getReader()
    const decoder = new TextDecoder()
    let buf = ''
    try {
      for (;;) {
        const { value, done } = await reader.read()
        if (done) break
        buf += decoder.decode(value, { stream: true })
        let idx: number
        while ((idx = buf.indexOf('\n\n')) >= 0) {
          dispatchSseChunk(buf.slice(0, idx), cbW)
          buf = buf.slice(idx + 2)
        }
      }
      if (buf.trim()) dispatchSseChunk(buf, cbW)
    } catch (e) {
      if ((e as Error).name !== 'AbortError') cb.onError((e as Error).message)
    }
    if (!sawCitations) {
      console.warn(
        '[streamChat] stream ended with NO citations event — the model did not emit a <CITATIONS> block (or the backend did not parse one).',
      )
    }
    cb.onDone()
  })()

  return ctrl
}

export type { ChatMessage }
