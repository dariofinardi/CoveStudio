// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

/**
 * Format-aware icon colour for a filename's extension. Kept centralised
 * so the chat-steps doc card, the chat-files shortcut and any future
 * surface that lists user-visible files all paint icons the same way:
 * Excel → green, Word → blue, PDF → red, PowerPoint → orange, Markdown
 * → primary text, anything else → brand accent.
 *
 * Returns a Tailwind colour class (the CSS variable form already used
 * across the app) so it can be dropped straight into `class={…}` on a
 * lucide icon.
 */
export function fileIconColor(filename: string): string {
  const ext = /\.([a-z0-9]+)$/i.exec(filename.trim())?.[1]?.toLowerCase() ?? ''
  switch (ext) {
    case 'xlsx':
    case 'xls':
    case 'xlsb':
    case 'ods':
    case 'csv':
      return 'text-(--color-success-500)'
    case 'docx':
    case 'doc':
    case 'rtf':
      return 'text-(--color-info-500)'
    case 'md':
    case 'markdown':
      return 'text-(--color-text-primary)'
    case 'pdf':
      return 'text-(--color-danger-500)'
    case 'pptx':
    case 'ppt':
      return 'text-(--color-warning-500)'
    default:
      return 'text-(--color-brand-600)'
  }
}
