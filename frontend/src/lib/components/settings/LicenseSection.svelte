<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Settings → License panel.

  Renders the product's identity (name + the running version) and
  the full AGPL-3.0 licence text bundled with the binary. The licence
  text is the canonical `LICENSE` file at the repo root, pulled in via
  Vite's `?raw` import so it ships as a string constant in the bundle
  — no runtime fetch, no risk of a missing-file 404 once installed.
-->
<script lang="ts">
  import { PRODUCT_NAME } from '$lib/product'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { APP_VERSION } from '$lib/stores/app-version.svelte'
  // Repo-root LICENSE (AGPL-3.0-only), inlined by Vite at build time.
  // Relative path crosses out of the frontend project, which the
  // dev-server allows via `server.fs.allow` in vite.config.ts.
  import licenseText from '../../../../../LICENSE?raw'

  // The full AGPL is ~620 lines; rendered in a scrollable
  // monospace block so it doesn't dominate the settings panel.
</script>

<section class="space-y-5">
  <div
    class="rounded-(--radius-md) border border-(--color-surface-200)
           bg-(--color-surface-0) px-5 py-4"
  >
    <h3 class="text-base font-semibold text-(--color-text-primary)">
      {PRODUCT_NAME}
      <span class="ml-2 text-sm font-normal text-(--color-text-secondary) tabular-nums">
        v{APP_VERSION}
      </span>
    </h3>
    <p class="mt-1 text-sm text-(--color-text-secondary)">
      {i18n.t('Settings.licenseIntro')}
    </p>
    <p class="mt-2 text-xs text-(--color-text-secondary)">
      <span class="font-medium text-(--color-text-primary)">
        {i18n.t('Settings.licenseSpdx')}:
      </span>
      AGPL-3.0-only
    </p>
  </div>

  <div
    class="rounded-(--radius-md) border border-(--color-surface-200)
           bg-(--color-surface-50)"
  >
    <header
      class="px-5 py-2.5 border-b border-(--color-surface-200)
             text-xs uppercase tracking-wide text-(--color-text-secondary)"
    >
      {i18n.t('Settings.licenseFullText')}
    </header>
    <pre
      class="px-5 py-4 max-h-[60vh] overflow-auto
             text-[11px] leading-snug font-mono whitespace-pre-wrap break-words
             text-(--color-text-primary)">{licenseText}</pre>
  </div>
</section>
