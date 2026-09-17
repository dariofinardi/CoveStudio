<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Settings → LLM models. Catalogue-driven (GET /models) editor over the
  user's LlmSettings. Four configurable providers (Anthropic, Google,
  OpenAI, Local); model-role assignments (main / title / tabular);
  active-provider selection. Saves the whole form as a patch — the
  backend PUT has COALESCE / empty-string-clears semantics so unchanged
  fields are preserved and blanked fields are cleared.
-->
<script lang="ts" module>
  import type { LlmProvider, LlmSettings } from '$lib/types/user'

  // View-model: every text field is a non-null string so it binds
  // cleanly to Input/Select (whose value types exclude null).
  interface LlmForm {
    main_model: string
    title_model: string
    tabular_model: string
    claude_api_key: string
    gemini_api_key: string
    gemini_region: string
    gemini_model: string
    openai_api_key: string
    openai_model: string
    mistral_api_key: string
    mistral_model: string
    local_base_url: string
    local_api_key: string
    local_model: string
    active_provider: LlmProvider | null
    /** v0.5.6 "Modalità sicura locale" — when true the form swaps the
     *  free-form URL / api key / model fields for a curated picker. */
    local_secure_mode: boolean
    /** v0.6.0 Mistral request-knob toggles (migration 0033). */
    mistral_safe_prompt: boolean
    mistral_parallel_tools: boolean
  }

  const s = (v: string | null | undefined): string => v ?? ''

  function toForm(x: LlmSettings): LlmForm {
    return {
      main_model: s(x.main_model),
      title_model: s(x.title_model),
      tabular_model: s(x.tabular_model),
      claude_api_key: s(x.claude_api_key),
      gemini_api_key: s(x.gemini_api_key),
      gemini_region: s(x.gemini_region),
      gemini_model: s(x.gemini_model),
      openai_api_key: s(x.openai_api_key),
      openai_model: s(x.openai_model),
      mistral_api_key: s(x.mistral_api_key),
      mistral_model: s(x.mistral_model),
      local_base_url: s(x.local_base_url),
      local_api_key: s(x.local_api_key),
      local_model: s(x.local_model),
      active_provider: x.active_provider ?? null,
      local_secure_mode: x.local_secure_mode ?? false,
      mistral_safe_prompt: x.mistral_safe_prompt ?? false,
      mistral_parallel_tools: x.mistral_parallel_tools ?? false,
    }
  }
</script>

<script lang="ts">
  import { readLocalPreference, writeLocalPreference } from '$lib/product'
  import Card from '$lib/components/ui/Card.svelte'
  import Input from '$lib/components/ui/Input.svelte'
  import Select from '$lib/components/ui/Select.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import Badge from '$lib/components/ui/Badge.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import ChipGroup from '$lib/components/ui/ChipGroup.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import Toggle from '$lib/components/ui/Toggle.svelte'
  import ConfirmDialog from '$lib/components/ui/ConfirmDialog.svelte'
  import ContextWindowHint from './ContextWindowHint.svelte'
  import { modelsStore } from '$lib/stores/models.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { api } from '$lib/api/client'
  import { userApi } from '$lib/api/user'
  import type { CuratedModelEntry } from '$lib/types/user'
  import { secureInstall } from '$lib/stores/secureInstall.svelte'
  import { Download, CheckCircle2, AlertCircle, Trash2, X } from 'lucide-svelte'

  let form = $state<LlmForm>(toForm({}))
  let initialized = $state(false)
  let activeProvidersUi = $state<string[]>([])
  let localRuntimeModels = $state<string[]>([])
  let localModelsLoaded = $state(false)
  let localModelsLoading = $state(false)

  let localFetchSeq = 0
  const ACTIVE_PROVIDERS_PREFERENCE = 'settings.activeProviders.v1'

  function isProviderId(v: string): v is LlmProvider {
    return ['anthropic', 'google', 'openai', 'mistral', 'local'].includes(v)
  }

  function readPersistedActiveProviders(): string[] {
    if (typeof window === 'undefined') return []
    try {
      const raw = readLocalPreference(ACTIVE_PROVIDERS_PREFERENCE)
      if (!raw) return []
      const parsed = JSON.parse(raw)
      if (!Array.isArray(parsed)) return []
      return parsed.filter((x): x is string => typeof x === 'string' && isProviderId(x))
    } catch {
      return []
    }
  }

  function writePersistedActiveProviders(v: string[]) {
    if (typeof window === 'undefined') return
    writeLocalPreference(ACTIVE_PROVIDERS_PREFERENCE, JSON.stringify(v))
  }

  function setActiveProvidersUi(v: string[]) {
    const clean = Array.from(new Set(v.filter((x): x is string => typeof x === 'string' && isProviderId(x))))
    activeProvidersUi = clean
    writePersistedActiveProviders(clean)
  }

  function normalizeBaseUrl(url: string): string {
    return url.trim().replace(/\/+$/, '')
  }

  function configuredProvidersFromForm(v: LlmForm): string[] {
    const out: string[] = []
    if (keySet(v.claude_api_key)) out.push('anthropic')
    if (keySet(v.gemini_api_key)) out.push('google')
    if (keySet(v.openai_api_key)) out.push('openai')
    if (keySet(v.mistral_api_key)) out.push('mistral')
    if (keySet(v.local_base_url)) out.push('local')
    return out
  }

  async function refreshLocalRuntimeModels() {
    const base = normalizeBaseUrl(form.local_base_url)
    const seq = ++localFetchSeq

    if (!base) {
      localRuntimeModels = []
      localModelsLoaded = false
      localModelsLoading = false
      return
    }

    localModelsLoading = true

    try {
      // Route the model-list probe through Cove Studio's own backend
      // instead of `fetch` against the Ollama / llama-server URL
      // directly. The WebView origin is `http://tauri.localhost`, and
      // external OpenAI-compatible runtimes rarely advertise that
      // origin in their `Access-Control-Allow-Origin` header, so the
      // browser used to block the probe with the CORS message users
      // reported. The backend proxy at `/models/local/probe` does the
      // server-to-server fetch (no Origin involved) and returns the
      // same payload shape Ollama / lm-studio / vLLM produce.
      const payload = await api<{ data?: Array<{ id?: string }> }>(
        '/models/local/probe',
        {
          query: {
            base,
            api_key: form.local_api_key.trim() || undefined,
          },
        },
      )
      const ids = (payload.data ?? [])
        .map((x) => (x.id ?? '').trim())
        .filter((x) => x.length > 0)

      if (seq !== localFetchSeq) return
      localRuntimeModels = Array.from(new Set(ids))
      localModelsLoaded = true
      localModelsLoading = false

      const first = ids[0] ?? ''
      if (first) {
        const currentMain = form.main_model.trim()
        const hasValidLocalMain =
          currentMain.startsWith('local:') && ids.includes(currentMain.slice('local:'.length))
        if (!currentMain || !hasValidLocalMain) {
          form.main_model = `local:${first}`
        }
        if (!form.local_model.trim()) {
          form.local_model = first
        }
      }
    } catch {
      if (seq !== localFetchSeq) return
      localRuntimeModels = []
      localModelsLoaded = false
      localModelsLoading = false
    }
  }

  $effect(() => {
    void modelsStore.load()
  })

  $effect(() => {
    if (!initialized && !modelsStore.loading && modelsStore.catalogue) {
      form = toForm(modelsStore.settings)
      const configured = configuredProvidersFromForm(form)
      const persisted = readPersistedActiveProviders().filter((p) => configured.includes(p))
      setActiveProvidersUi(
        persisted.length > 0
          ? persisted
          : form.active_provider
            ? [form.active_provider]
            : configured
      )
      initialized = true
      if (keySet(form.local_base_url)) {
        void refreshLocalRuntimeModels()
      }
    }
  })

  const dirty = $derived.by(() => {
    if (!initialized) return false
    const formDirty = JSON.stringify(form) !== JSON.stringify(toForm(modelsStore.settings))
    const currentActive = modelsStore.settings.active_provider ?? null
    const nextActive = pickPersistedActiveProvider()
    return formDirty || currentActive !== nextActive
  })

  function modelOptions(providerId: string) {
    const p = modelsStore.providerById(providerId)
    return [
      { value: '', label: i18n.t('Settings.selectModel') },
      ...(p?.models ?? []).map((m) => ({
        value: m.id,
        label: m.preview ? `${m.display_name} (preview)` : m.display_name,
      })),
    ]
  }
  const regionOptions = $derived(
    (modelsStore.providerById('google')?.regions ?? []).map((r) => ({
      value: r.id,
      label: r.display_name,
    }))
  )
  const keySet = (v: string): boolean => v.trim().length > 0

  // Providers the user has actually configured.
  // Drives which models the role dropdowns may offer.
  const configuredProviders = $derived.by(() => {
    const s = new Set<string>()
    if (keySet(form.claude_api_key)) s.add('anthropic')
    if (keySet(form.gemini_api_key)) s.add('google')
    if (keySet(form.openai_api_key)) s.add('openai')
    if (keySet(form.mistral_api_key)) s.add('mistral')
    if (keySet(form.local_base_url)) s.add('local')
    return s
  })

  const activeRoleProviders = $derived.by(() => {
    const cfg = configuredProviders
    return activeProvidersUi.filter((p) => cfg.has(p))
  })

  const localRoleIds = $derived.by(() => {
    if (localModelsLoaded) return new Set(localRuntimeModels)
    return new Set<string>()
  })

  // Role-model options — only models from configured providers. Ids
  // carry the dispatch prefix the backend expects: openai:/mistral: for
  // those providers, bare id for Claude/Gemini.
  const roleOptions = $derived.by(() => {
    const out = [{ value: '', label: i18n.t('Settings.notSet') }]

    const nonLocal = modelsStore.allModels
      .filter((m) => m.providerId !== 'local')
      .filter((m) => activeRoleProviders.includes(m.providerId))
      .map((m) => ({
        value: m.providerId === 'openai' ? `openai:${m.id}` : m.providerId === 'mistral' ? `mistral:${m.id}` : m.id,
        label: `${m.display_name} · ${m.provider}`,
      }))

    const local = activeRoleProviders.includes('local')
      ? Array.from(localRoleIds).map((id) => ({
          value: `local:${id}`,
          label: `${id} · ${i18n.t('Settings.providerLocal')}`,
        }))
      : []

    return [...out, ...nonLocal, ...local]
  })

  // Order = data-sovereignty / data-residency. Local first (data
  // never leaves the device), then EU-hosted Mistral, then the three
  // US providers. The order is intentional — it's the same logic that
  // drives the card layout below: the user reads top-to-bottom and
  // the most "private" choice is the first thing in view.
  const providerChips = $derived([
    { value: 'local', label: i18n.t('Settings.providerLocal'), disabled: !configuredProviders.has('local') },
    { value: 'mistral', label: 'Mistral', disabled: !configuredProviders.has('mistral') },
    { value: 'google', label: 'Google', disabled: !configuredProviders.has('google') },
    { value: 'openai', label: 'OpenAI', disabled: !configuredProviders.has('openai') },
    { value: 'anthropic', label: 'Anthropic', disabled: !configuredProviders.has('anthropic') },
  ])

  function normalizeRoleModelValue(value: string): string {
    const v = value.trim()
    if (!v) return ''
    if (v.startsWith('openai:') || v.startsWith('mistral:') || v.startsWith('local:')) return v
    if (localRoleIds.has(v)) return `local:${v}`
    return v
  }

  function pickPersistedActiveProvider(): LlmProvider | null {
    for (const p of ['anthropic', 'google', 'openai', 'mistral', 'local'] as const) {
      if (activeRoleProviders.includes(p)) return p
    }
    return null
  }

  // ── v0.6.0 Modalità sicura locale ─────────────────────────────────
  let secureOllamaRunning = $state<boolean | null>(null)
  let secureModels = $state<CuratedModelEntry[]>([])
  let secureLoading = $state(false)
  // Per-model install progress + AbortController registry live in a
  // module-singleton store (secureInstall.svelte.ts) so they survive
  // the user navigating away from Settings → Modelli LLM and back.
  // Bug fixed 2026-06-07: with the state on the component, an unmount
  // wiped the AbortController map, the next mount re-enabled the
  // Install button mid-pull, the user clicked it again, and a second
  // parallel pull fired.
  const installProgress = $derived(secureInstall.progress)

  async function refreshSecureCatalogue() {
    secureLoading = true
    try {
      const hb = await userApi.localSecureHeartbeat()
      secureOllamaRunning = hb.ollama_running
      const m = await userApi.localSecureModels()
      secureModels = m.models
      if (m.models.some((x) => x.legacy_installed.length > 0)) {
        void migrateLegacyModels()
      }
    } catch (e) {
      secureOllamaRunning = false
      toastStore.danger(i18n.t('Settings.localSecureLoadError'), {
        detail: (e as Error).message,
      })
    } finally {
      secureLoading = false
    }
  }

  /** Persist the toggle the instant the user flips it. The section's
   *  bulk "Salva modifiche" button still works for the other fields,
   *  but the secure-mode flag is special: the rest of the section's
   *  UI (curated picker vs. free-form inputs) keys off it, and
   *  losing the choice on navigation away was a real bug. */
  async function onSecureModeToggle(checked: boolean) {
    try {
      await modelsStore.save({ local_secure_mode: checked })
      if (checked) await refreshSecureCatalogue()
    } catch (e) {
      toastStore.danger(i18n.t('Settings.llmSettingsError'), {
        detail: (e as Error).message,
      })
      // Revert the visible form state so the toggle reflects what's
      // actually persisted server-side.
      form.local_secure_mode = !checked
    }
  }

  /** Thin wrapper around the secureInstall store — funnels the
   *  per-install outcomes through the section's localised toasts. The
   *  install / cancel bookkeeping itself lives in the store so it
   *  survives component unmount. */
  async function installSecure(modelId: string) {
    await secureInstall.install(modelId, {
      onCompleted: async () => {
        await refreshSecureCatalogue()
        toastStore.success(i18n.t('Settings.localSecureInstalledToast'))
      },
      onCancelled: async () => {
        await refreshSecureCatalogue()
        toastStore.info(i18n.t('Settings.localSecureCancelled'))
      },
      onError: (msg) => {
        toastStore.danger(i18n.t('Settings.localSecureInstallError'), {
          detail: msg,
        })
      },
    })
  }

  // ── Models installed under names from earlier releases ────────────
  // Renaming is a copy in Ollama (no download, nothing deleted) and runs
  // on its own. Removing the previous names, or reinstalling a model that
  // could not be copied, happens only after the user confirms.
  let legacyMigrationStarted = false
  let legacyNamesToRemove = $state<string[]>([])
  let modelsToReinstall = $state<string[]>([])
  let legacyDialogOpen = $state(false)

  const legacyDialogMessage = $derived(
    modelsToReinstall.length > 0
      ? i18n.t('Settings.localSecureCleanupReinstallMessage', {
          models: modelsToReinstall.map(displayNameOf).join(', '),
          names: legacyNamesToRemove.join(', '),
        })
      : i18n.t('Settings.localSecureCleanupMessage', { names: legacyNamesToRemove.join(', ') }),
  )

  function displayNameOf(modelId: string): string {
    return secureModels.find((m) => m.id === modelId)?.display_name ?? modelId
  }

  function sameModelName(value: string, name: string): boolean {
    const bare = name.replace(/:latest$/, '')
    return value === bare || value === `${bare}:latest`
  }

  /** Mirror the backend settings update in the open form, so a later
   *  "Salva" doesn't write the previous names back. */
  function renameInForm(from: string, to: string) {
    if (sameModelName(form.local_model, from)) form.local_model = to
    for (const key of ['main_model', 'title_model', 'tabular_model'] as const) {
      if (form[key].startsWith('local:') && sameModelName(form[key].slice(6), from)) {
        form[key] = `local:${to}`
      }
    }
  }

  async function migrateLegacyModels() {
    if (legacyMigrationStarted) return
    legacyMigrationStarted = true
    try {
      const report = await userApi.localSecureMigrate()
      for (const r of report.migrated) renameInForm(r.from, r.to)
      if (report.migrated.length > 0) {
        await modelsStore.load()
        toastStore.success(i18n.t('Settings.localSecureMigratedToast', { n: report.migrated.length }))
      }
      legacyNamesToRemove = [...report.leftovers, ...report.failed.map((f) => f.from)]
      modelsToReinstall = [...new Set(report.failed.map((f) => f.to))]
      legacyDialogOpen = legacyNamesToRemove.length > 0
      const m = await userApi.localSecureModels()
      secureModels = m.models
    } catch (e) {
      toastStore.danger(i18n.t('Settings.localSecureMigrateError'), {
        detail: (e as Error).message,
      })
    }
  }

  async function removeLegacyModels() {
    try {
      await userApi.localSecureRemoveLegacy(legacyNamesToRemove)
      toastStore.success(i18n.t('Settings.localSecureCleanupDoneToast'))
    } catch (e) {
      toastStore.danger(i18n.t('Settings.localSecureCleanupError'), {
        detail: (e as Error).message,
      })
      return
    }
    const reinstall = modelsToReinstall
    legacyNamesToRemove = []
    modelsToReinstall = []
    legacyDialogOpen = false
    await refreshSecureCatalogue()
    for (const id of reinstall) void installSecure(id)
  }

  function keepLegacyModels() {
    legacyDialogOpen = false
  }

  function cancelInstall(modelId: string) {
    secureInstall.cancel(modelId)
  }

  async function uninstallSecure(modelId: string) {
    try {
      await userApi.localSecureUninstall(modelId)
      await refreshSecureCatalogue()
      toastStore.success(i18n.t('Settings.localSecureUninstalledToast'))
    } catch (e) {
      toastStore.danger(i18n.t('Settings.localSecureUninstallError'), {
        detail: (e as Error).message,
      })
    }
  }

  // Fetch the catalogue when the user toggles secure mode ON (and the
  // first time the section mounts in secure mode already).
  $effect(() => {
    if (form.local_secure_mode && secureModels.length === 0 && !secureLoading) {
      void refreshSecureCatalogue()
    }
  })

  function pct(b?: number, t?: number): string {
    if (!t || t === 0) return ''
    const p = Math.round(((b ?? 0) / t) * 100)
    return `${p}%`
  }

  // ── Mistral role-model profiles ───────────────────────────────────
  // Three opinionated presets the user can apply with one click,
  // plus an implicit "custom" fall-through when the role assignments
  // don't match any preset. The presets fill main / title /
  // tabular role models with Mistral ids; clicking again with the
  // same preset is a no-op (idempotent). All buttons are disabled
  // when no API key is configured — otherwise we'd save role
  // assignments that the chat dispatcher would fail to fulfil.
  type MistralProfile = 'fast' | 'balanced' | 'premium' | 'custom'

  const MISTRAL_PRESETS: Record<Exclude<MistralProfile, 'custom'>, {
    main: string
    title: string
    tabular: string
  }> = {
    // Latency-first path. Small 4 ($0.1/$0.3 per Mtok, vision +
    // function calling + 128K) is ~3-5× faster than Large 3 on
    // legal queries and ~5× cheaper on input — fine for triage,
    // quick lookups, and tabular runs where each cell is a
    // focused extraction (not a deep reasoning task).
    fast: {
      main: 'mistral:mistral-small-latest',
      title: 'mistral:ministral-3b-latest',
      tabular: 'mistral:mistral-small-latest',
    },
    // Default sweet spot. Large 3 ($0.5/$1.5 per Mtok, vision +
    // function calling + 128K) covers the bulk of legal chat;
    // Ministral 3B ($0.1/$0.1) crunches chat titles for cents per
    // thousand. Both share 128K context.
    balanced: {
      main: 'mistral:mistral-large-latest',
      title: 'mistral:ministral-3b-latest',
      tabular: 'mistral:mistral-large-latest',
    },
    // Frontier path. Medium 3.5 ($1.5/$7.5) is Mistral's current
    // top-of-line multimodal; Small 4 ($0.1/$0.3) is a better title
    // model than Ministral when the user is okay paying ~3× for it
    // (they're already paying Medium's premium).
    premium: {
      main: 'mistral:mistral-medium-latest',
      title: 'mistral:mistral-small-latest',
      tabular: 'mistral:mistral-medium-latest',
    },
  }

  const activeMistralProfile = $derived.by<MistralProfile>(() => {
    for (const [profile, models] of Object.entries(MISTRAL_PRESETS)) {
      if (
        form.main_model === models.main &&
        form.title_model === models.title &&
        form.tabular_model === models.tabular
      ) {
        return profile as MistralProfile
      }
    }
    return 'custom'
  })

  async function applyMistralProfile(profile: 'fast' | 'balanced' | 'premium') {
    const preset = MISTRAL_PRESETS[profile]
    // Snapshot the previous role assignments so we can revert the
    // form on a save failure (network drop, 5xx). Without this the
    // user would see the highlight jump to the new profile but the
    // backend state stays old — confusing.
    const before = {
      main_model: form.main_model,
      title_model: form.title_model,
      tabular_model: form.tabular_model,
    }
    form.main_model = preset.main
    form.title_model = preset.title
    form.tabular_model = preset.tabular
    try {
      // Persist immediately. The earlier behaviour (only sync to
      // the form, save on the section's "Salva modifiche" button)
      // confused users: clicking a profile felt like a no-op until
      // they realised they had to click Save. Now profile clicks
      // are committed in one shot, and the `activeMistralProfile`
      // highlight on the next re-entry comes straight from the
      // persisted `modelsStore.settings.*` fields.
      await modelsStore.save({
        main_model: preset.main,
        title_model: preset.title,
        tabular_model: preset.tabular,
      })
      const toastKey =
        profile === 'fast'
          ? 'Settings.mistralProfileFastApplied'
          : profile === 'premium'
            ? 'Settings.mistralProfilePremiumApplied'
            : 'Settings.mistralProfileBalancedApplied'
      toastStore.info(i18n.t(toastKey))
    } catch (e) {
      // Revert the visible form so the highlight reflects what's
      // actually persisted server-side.
      form.main_model = before.main_model
      form.title_model = before.title_model
      form.tabular_model = before.tabular_model
      toastStore.danger(i18n.t('Settings.llmSettingsError'), {
        detail: (e as Error).message,
      })
    }
  }

  async function save() {
    try {
      const main = normalizeRoleModelValue(form.main_model)
      const title = normalizeRoleModelValue(form.title_model)
      const tabular = normalizeRoleModelValue(form.tabular_model)
      const mainLocal = main.startsWith('local:') ? main.slice('local:'.length) : ''
      await modelsStore.save({
        ...form,
        main_model: main,
        title_model: title,
        tabular_model: tabular,
        local_model: mainLocal || form.local_model,
        active_provider: pickPersistedActiveProvider(),
      })
      writePersistedActiveProviders(activeProvidersUi)
      toastStore.success(i18n.t('Settings.llmSettingsSaved'))
    } catch (e) {
      toastStore.danger(i18n.t('Settings.llmSettingsError'), { detail: (e as Error).message })
    }
  }
</script>

{#if modelsStore.loading && !initialized}
  <div class="flex items-center gap-2 text-sm text-(--color-text-secondary) py-12 justify-center">
    <Spinner size="sm" />
    {i18n.t('Settings.loadingCatalogue')}
  </div>
{:else if modelsStore.error && !initialized}
  <EmptyState title={i18n.t('Settings.loadModelsError')} description={modelsStore.error}>
    {#snippet action()}
      <Button size="sm" variant="secondary" onclick={() => modelsStore.load()}>
        {i18n.t('Common.retry')}
      </Button>
    {/snippet}
  </EmptyState>
{:else}
  <div class="space-y-4">
    <Card title={i18n.t('Settings.activeProvider')} subtitle={i18n.t('Settings.activeProviderHint')}>
      <ChipGroup
        chips={providerChips}
        multi
        selected={activeProvidersUi}
        onchange={(v) => setActiveProvidersUi((Array.isArray(v) ? v : []) as string[])}
      />
    </Card>

    <!-- Provider cards ordered by data-residency (local → EU → US).
         Matches the chip order above and signals to legal-target users
         that the choices nearest the top keep their data closest to
         them. Local first (data never leaves device), then EU-hosted
         Mistral, then the US providers Google/OpenAI/Anthropic. -->
    <Card title={i18n.t('Settings.localProvider')}>
      <div class="space-y-3">
        <!-- v0.5.6 — Modalità sicura locale toggle. ON → swap the free
             URL/api-key fields for the curated picker below. -->
        <div class="flex items-start gap-3 pb-2 border-b border-(--color-surface-200)">
          <!-- Auto-saves on flip. Persisting only via the section's
               "Salva modifiche" button confused users (2026-06-07):
               toggling, leaving Settings, coming back showed the
               server-side OFF again because the click never reached
               the save batch. -->
          <Toggle
            bind:checked={form.local_secure_mode}
            onchange={(v) => void onSecureModeToggle(v)}
            label={i18n.t('Settings.localSecureMode')}
            description={i18n.t('Settings.localSecureModeHint')}
          />
        </div>

        {#if form.local_secure_mode}
          <!-- Curated picker. Server: locked to localhost:11434 -->
          <div class="text-xs text-(--color-text-secondary)">
            {i18n.t('Settings.localSecureServerLabel')}: <code>http://localhost:11434</code>
            <span class="ml-1">🔒</span>
          </div>

          {#if secureLoading && secureModels.length === 0}
            <div class="flex items-center gap-2 text-sm text-(--color-text-secondary) py-3">
              <Spinner size="sm" />
              {i18n.t('Common.loading')}
            </div>
          {:else if secureOllamaRunning === false}
            <div class="flex items-start gap-2 text-sm text-(--color-warning-700) bg-(--color-warning-50) border border-(--color-warning-200) rounded-(--radius-md) p-3">
              <AlertCircle size={16} class="shrink-0 mt-0.5" />
              <div>
                <p class="font-medium">{i18n.t('Settings.localSecureOllamaMissing')}</p>
                <p class="text-xs mt-1">{i18n.t('Settings.localSecureOllamaMissingHint')}</p>
                <a
                  href="https://ollama.com/download"
                  target="_blank"
                  rel="noopener"
                  class="text-xs underline text-(--color-brand-600) hover:text-(--color-brand-700)"
                >ollama.com/download</a>
                <Button size="sm" variant="ghost" class="ml-2" onclick={() => void refreshSecureCatalogue()}>
                  {i18n.t('Common.retry')}
                </Button>
              </div>
            </div>
          {:else}
            <ul class="flex flex-col gap-2">
              {#each secureModels as m (m.id)}
                {@const prog = installProgress[m.id]}
                <li class="border border-(--color-surface-200) rounded-(--radius-md) p-3">
                  <div class="flex items-center gap-3">
                    <div class="flex-1 min-w-0">
                      <div class="flex items-center gap-2">
                        <span class="text-sm font-medium text-(--color-text-primary) truncate">
                          {m.display_name}
                        </span>
                        {#if m.ready}
                          <Badge tone="success" size="xs">
                            <CheckCircle2 size={10} class="mr-1" />
                            {i18n.t('Settings.localSecureInstalled')}
                          </Badge>
                        {/if}
                      </div>
                      <p class="text-xs text-(--color-text-secondary) truncate">
                        {m.base_model}
                      </p>
                      <p class="text-xs text-(--color-text-disabled)">
                        ~{m.approx_size_gb.toFixed(1)} GB · {i18n.t('Settings.localSecureMinRam', { n: m.min_ram_gb })}
                      </p>
                    </div>
                    {#if m.ready}
                      <Button size="sm" variant="ghost" onclick={() => void uninstallSecure(m.id)}>
                        <Trash2 size={14} class="mr-1" />
                        {i18n.t('Settings.remove')}
                      </Button>
                    {:else if prog && prog.phase !== 'error'}
                      <div class="flex items-center gap-2">
                        <Button size="sm" disabled>
                          <Spinner size="sm" class="mr-2" />
                          {#if prog.phase === 'pulling'}
                            {i18n.t('Settings.localSecurePulling')} {pct(prog.bytes, prog.total)}
                          {:else if prog.phase === 'creating'}
                            {i18n.t('Settings.localSecureCreating')}
                          {:else}
                            {i18n.t('Settings.localSecureStarting')}
                          {/if}
                        </Button>
                        <!-- Cancel button — only meaningful during
                             pulling / starting (creating is too quick
                             for a user click to ever land mid-phase).
                             Disabled during creating because there's
                             no clean abort point on the Ollama side
                             of /api/create. -->
                        <Button
                          size="sm"
                          variant="ghost"
                          disabled={prog.phase === 'creating'}
                          onclick={() => cancelInstall(m.id)}
                          title={i18n.t('Settings.localSecureCancelHint')}
                        >
                          <X size={14} class="mr-1" />
                          {i18n.t('Common.cancel')}
                        </Button>
                      </div>
                    {:else}
                      <Button size="sm" onclick={() => void installSecure(m.id)}>
                        <Download size={14} class="mr-1" />
                        {i18n.t('Settings.localSecureInstall')}
                      </Button>
                    {/if}
                  </div>
                  {#if prog && prog.phase === 'pulling' && prog.total}
                    <div class="mt-2 h-1.5 w-full bg-(--color-surface-200) rounded-full overflow-hidden">
                      <div
                        class="h-full bg-(--color-brand-500) transition-all"
                        style="width: {pct(prog.bytes, prog.total)}"
                      ></div>
                    </div>
                  {/if}
                  {#if prog && prog.phase === 'error'}
                    <p class="mt-2 text-xs text-(--color-danger-600)">{prog.error}</p>
                  {/if}
                </li>
              {/each}
            </ul>
          {/if}
        {:else}
          <!-- Free-form path (legacy / power user) -->
          <div class="grid grid-cols-[1fr_auto] gap-2 items-end">
            <Input
              label={i18n.t('Settings.baseUrl')}
              bind:value={form.local_base_url}
              placeholder="http://127.0.0.1:11434/v1"
              autocomplete="off"
            />
            <Button
              size="sm"
              variant="secondary"
              disabled={!keySet(form.local_base_url)}
              loading={localModelsLoading}
              onclick={() => void refreshLocalRuntimeModels()}
            >
              Refresh
            </Button>
          </div>
          <Input
            label={i18n.t('Settings.apiKeyOptional')}
            type="password"
            bind:value={form.local_api_key}
            autocomplete="off"
          />
          {#if form.local_model.trim()}
            <ContextWindowHint
              model={`local:${form.local_model.trim()}`}
              baseUrl={form.local_base_url}
              secure={false}
            />
          {/if}
        {/if}
      </div>
    </Card>

    <Card>
      {#snippet header()}
        <div class="flex items-center gap-2">
          <h3 class="text-sm font-semibold text-(--color-text-primary)">Mistral AI</h3>
          {#if keySet(form.mistral_api_key)}<Badge tone="success" size="xs">{i18n.t('Settings.keySet')}</Badge>{/if}
        </div>
      {/snippet}
      <div class="space-y-3">
        <Input
          label="API key"
          type="password"
          bind:value={form.mistral_api_key}
          autocomplete="off"
        />
        <Select label={i18n.t('Settings.model')} options={modelOptions('mistral')} bind:value={form.mistral_model} />

        <!-- Profile preset picker. Sets main_model / title_model /
             tabular_model to a curated combination of Mistral models
             in one click. Active state is computed from the current
             role assignments so the user immediately sees which
             profile they're on (or "Personalizzato" when their
             choices don't match any preset). Disabled when no API
             key is set — clicking would assign Mistral roles that
             the chat dispatcher can't fulfil. -->
        <div class="pt-2 border-t border-(--color-surface-200)">
          <p class="text-xs font-medium text-(--color-text-secondary) mb-2">
            {i18n.t('Settings.mistralProfileTitle')}
          </p>
          <div class="grid grid-cols-3 gap-2">
            <button
              type="button"
              disabled={!keySet(form.mistral_api_key)}
              onclick={() => void applyMistralProfile('fast')}
              class="text-left px-3 py-2 rounded-(--radius-md) border transition-colors duration-(--transition-fast)
                     {activeMistralProfile === 'fast'
                       ? 'border-(--color-brand-500) bg-(--color-brand-50)'
                       : 'border-(--color-surface-300) hover:border-(--color-surface-400)'}
                     disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <div class="flex items-center gap-2">
                <span class="text-sm font-medium text-(--color-text-primary)">
                  {i18n.t('Settings.mistralProfileFast')}
                </span>
                {#if activeMistralProfile === 'fast'}
                  <Badge tone="brand" size="xs">{i18n.t('Settings.mistralProfileActive')}</Badge>
                {/if}
              </div>
              <p class="text-xs text-(--color-text-secondary) mt-1">
                {i18n.t('Settings.mistralProfileFastHint')}
              </p>
              <p class="text-[11px] text-(--color-text-disabled) mt-1 font-mono">
                Small 4 · Ministral 3B · Small 4
              </p>
            </button>

            <button
              type="button"
              disabled={!keySet(form.mistral_api_key)}
              onclick={() => void applyMistralProfile('balanced')}
              class="text-left px-3 py-2 rounded-(--radius-md) border transition-colors duration-(--transition-fast)
                     {activeMistralProfile === 'balanced'
                       ? 'border-(--color-brand-500) bg-(--color-brand-50)'
                       : 'border-(--color-surface-300) hover:border-(--color-surface-400)'}
                     disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <div class="flex items-center gap-2">
                <span class="text-sm font-medium text-(--color-text-primary)">
                  {i18n.t('Settings.mistralProfileBalanced')}
                </span>
                {#if activeMistralProfile === 'balanced'}
                  <Badge tone="brand" size="xs">{i18n.t('Settings.mistralProfileActive')}</Badge>
                {/if}
              </div>
              <p class="text-xs text-(--color-text-secondary) mt-1">
                {i18n.t('Settings.mistralProfileBalancedHint')}
              </p>
              <p class="text-[11px] text-(--color-text-disabled) mt-1 font-mono">
                Large 3 · Ministral 3B · Large 3
              </p>
            </button>

            <button
              type="button"
              disabled={!keySet(form.mistral_api_key)}
              onclick={() => void applyMistralProfile('premium')}
              class="text-left px-3 py-2 rounded-(--radius-md) border transition-colors duration-(--transition-fast)
                     {activeMistralProfile === 'premium'
                       ? 'border-(--color-brand-500) bg-(--color-brand-50)'
                       : 'border-(--color-surface-300) hover:border-(--color-surface-400)'}
                     disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <div class="flex items-center gap-2">
                <span class="text-sm font-medium text-(--color-text-primary)">
                  {i18n.t('Settings.mistralProfilePremium')}
                </span>
                {#if activeMistralProfile === 'premium'}
                  <Badge tone="brand" size="xs">{i18n.t('Settings.mistralProfileActive')}</Badge>
                {/if}
              </div>
              <p class="text-xs text-(--color-text-secondary) mt-1">
                {i18n.t('Settings.mistralProfilePremiumHint')}
              </p>
              <p class="text-[11px] text-(--color-text-disabled) mt-1 font-mono">
                Medium 3.5 · Small 4 · Medium 3.5
              </p>
            </button>
          </div>
          {#if activeMistralProfile === 'custom' && keySet(form.mistral_api_key)}
            <p class="text-xs text-(--color-text-disabled) mt-2 italic">
              {i18n.t('Settings.mistralProfileCustomNote')}
            </p>
          {/if}
        </div>

        <!-- v0.6.0 — Mistral-specific request knobs.
             Both default to OFF (migration 0033 DEFAULT 0). The card
             surfaces them so users hitting safe_prompt false-positives
             on legal content can flip on the upstream filter, or
             power users who want concurrent tool calls can re-enable
             Mistral's `parallel_tool_calls: true` default. The
             corresponding backend pass-through lives in
             src/llm/mistral.rs::build_body. -->
        <div class="pt-2 border-t border-(--color-surface-200) space-y-3">
          <Toggle
            bind:checked={form.mistral_safe_prompt}
            disabled={!keySet(form.mistral_api_key)}
            label={i18n.t('Settings.mistralSafePrompt')}
            description={i18n.t('Settings.mistralSafePromptHint')}
          />
          <Toggle
            bind:checked={form.mistral_parallel_tools}
            disabled={!keySet(form.mistral_api_key)}
            label={i18n.t('Settings.mistralParallelTools')}
            description={i18n.t('Settings.mistralParallelToolsHint')}
          />
        </div>

        <!-- ZDR / EU hosting info. NOT a checkbox — Mistral's Zero
             Data Retention requires a manual support ticket (confirmed
             against the official help center 2026-06). The note
             explains the default 30-day rolling abuse-monitoring
             retention and offers the help-center link for users who
             want stricter handling. -->
        <div class="pt-2 border-t border-(--color-surface-200) text-xs text-(--color-text-secondary) space-y-1">
          <p>{i18n.t('Settings.mistralEuHostingNote')}</p>
          <a
            href="https://help.mistral.ai/en/articles/347612-can-i-activate-zero-data-retention-zdr"
            target="_blank"
            rel="noopener"
            class="inline-flex items-center gap-1 text-(--color-brand-600) hover:text-(--color-brand-700) underline"
          >
            {i18n.t('Settings.mistralZdrLink')}
          </a>
        </div>
      </div>
    </Card>

    <Card>
      {#snippet header()}
        <div class="flex items-center gap-2">
          <h3 class="text-sm font-semibold text-(--color-text-primary)">Google Gemini</h3>
          {#if keySet(form.gemini_api_key)}<Badge tone="success" size="xs">{i18n.t('Settings.keySet')}</Badge>{/if}
        </div>
      {/snippet}
      <div class="space-y-3">
        <Input
          label="API key"
          type="password"
          bind:value={form.gemini_api_key}
          placeholder="AIza…"
          autocomplete="off"
        />
        <div class="grid grid-cols-2 gap-3">
          <Select label={i18n.t('Settings.model')} options={modelOptions('google')} bind:value={form.gemini_model} />
          <Select label={i18n.t('Settings.region')} options={regionOptions} bind:value={form.gemini_region} />
        </div>
      </div>
    </Card>

    <Card>
      {#snippet header()}
        <div class="flex items-center gap-2">
          <h3 class="text-sm font-semibold text-(--color-text-primary)">OpenAI</h3>
          {#if keySet(form.openai_api_key)}<Badge tone="success" size="xs">{i18n.t('Settings.keySet')}</Badge>{/if}
        </div>
      {/snippet}
      <div class="space-y-3">
        <Input
          label="API key"
          type="password"
          bind:value={form.openai_api_key}
          placeholder="sk-…"
          autocomplete="off"
        />
        <Select label={i18n.t('Settings.model')} options={modelOptions('openai')} bind:value={form.openai_model} />
      </div>
    </Card>

    <Card>
      {#snippet header()}
        <div class="flex items-center gap-2">
          <h3 class="text-sm font-semibold text-(--color-text-primary)">Anthropic (Claude)</h3>
          {#if keySet(form.claude_api_key)}<Badge tone="success" size="xs">{i18n.t('Settings.keySet')}</Badge>{/if}
        </div>
      {/snippet}
      <Input
        label={i18n.t('Settings.apiKey')}
        type="password"
        bind:value={form.claude_api_key}
        placeholder="sk-ant-…"
        autocomplete="off"
      />
    </Card>

    <Card title={i18n.t('Settings.modelRoles')} subtitle={i18n.t('Settings.modelRolesHint')}>
      <div class="grid grid-cols-3 gap-3">
        <div>
          <Select label={i18n.t('Settings.roleMain')} options={roleOptions} bind:value={form.main_model} />
          <ContextWindowHint model={form.main_model} baseUrl={form.local_base_url} secure={form.local_secure_mode} />
        </div>
        <div>
          <Select label={i18n.t('Settings.roleTitles')} options={roleOptions} bind:value={form.title_model} />
          <ContextWindowHint model={form.title_model} baseUrl={form.local_base_url} secure={form.local_secure_mode} />
        </div>
        <div>
          <Select label={i18n.t('Settings.roleTabular')} options={roleOptions} bind:value={form.tabular_model} />
          <ContextWindowHint model={form.tabular_model} baseUrl={form.local_base_url} secure={form.local_secure_mode} />
        </div>
      </div>
    </Card>

    <div class="flex justify-end">
      <Button disabled={!dirty} loading={modelsStore.saving} onclick={save}>
        {i18n.t('Settings.saveChanges')}
      </Button>
    </div>
  </div>
{/if}

<ConfirmDialog
  bind:open={legacyDialogOpen}
  title={i18n.t('Settings.localSecureCleanupTitle')}
  message={legacyDialogMessage}
  confirmLabel={i18n.t('Settings.localSecureCleanupConfirm')}
  cancelLabel={i18n.t('Settings.localSecureCleanupKeep')}
  danger
  onconfirm={removeLegacyModels}
  oncancel={keepLegacyModels}
/>
