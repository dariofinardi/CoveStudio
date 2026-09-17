// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

/**
 * PDF.js bootstrap. Configures the web worker once (Vite resolves the
 * `?url` import to a hashed asset) and re-exports the entry points the
 * viewer needs. Rendering uses only this JS library — no native plugin.
 */

import { GlobalWorkerOptions, getDocument, TextLayer } from 'pdfjs-dist'
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url'

GlobalWorkerOptions.workerSrc = workerUrl

export { getDocument, TextLayer }
