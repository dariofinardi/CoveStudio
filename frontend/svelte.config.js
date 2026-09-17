// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

import { vitePreprocess } from '@sveltejs/vite-plugin-svelte'

/** @type {import('@sveltejs/vite-plugin-svelte').SvelteConfig} */
export default {
  preprocess: vitePreprocess(),
  compilerOptions: {
    runes: true,
  },
  // Third-party .svelte components (e.g. lucide-svelte) may still use
  // legacy syntax like $$props. Forcing runes mode on them via the
  // global compilerOptions breaks their compile — so for anything under
  // node_modules, fall back to Svelte's per-file auto-detection.
  vitePlugin: {
    dynamicCompileOptions({ filename }) {
      if (filename.includes('node_modules')) {
        return { runes: undefined }
      }
    },
  },
}
