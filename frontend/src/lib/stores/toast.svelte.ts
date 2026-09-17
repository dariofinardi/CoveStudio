// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

export type ToastTone = 'info' | 'success' | 'warning' | 'danger'

export interface Toast {
  id: number
  tone: ToastTone
  title: string
  detail?: string
  /** ms before auto-dismiss; danger toasts default to sticky (0). */
  duration: number
}

interface PushOptions {
  detail?: string
  duration?: number
}

const MAX_TOASTS = 5

function createToastStore() {
  let queue = $state<Toast[]>([])
  let nextId = 1
  const timers = new Map<number, ReturnType<typeof setTimeout>>()

  function dismiss(id: number) {
    queue = queue.filter((t) => t.id !== id)
    const timer = timers.get(id)
    if (timer) {
      clearTimeout(timer)
      timers.delete(id)
    }
  }

  function push(tone: ToastTone, title: string, opts: PushOptions = {}): number {
    const id = nextId++
    const duration = opts.duration ?? (tone === 'danger' ? 0 : 6000)
    const toast: Toast = { id, tone, title, detail: opts.detail, duration }

    queue = [...queue, toast].slice(-MAX_TOASTS)

    if (duration > 0) {
      timers.set(
        id,
        setTimeout(() => dismiss(id), duration),
      )
    }
    return id
  }

  return {
    get queue() {
      return queue
    },
    push,
    dismiss,
    info: (title: string, opts?: PushOptions) => push('info', title, opts),
    success: (title: string, opts?: PushOptions) => push('success', title, opts),
    warning: (title: string, opts?: PushOptions) => push('warning', title, opts),
    danger: (title: string, opts?: PushOptions) => push('danger', title, opts),
    clear() {
      for (const timer of timers.values()) clearTimeout(timer)
      timers.clear()
      queue = []
    },
  }
}

export const toastStore = createToastStore()
