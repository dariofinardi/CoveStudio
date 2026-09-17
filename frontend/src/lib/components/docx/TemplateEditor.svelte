<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Full-page DOCX template editor. Covers the whole DocxTemplate model —
  identity, layout, typography, styles, and the authoring contract.
  System templates open read-only and can be duplicated into an
  editable user template; user templates are saved as writable JSON
  files under config/docx-templates/user/ via POST /docx-templates/save.
-->
<script lang="ts">
  import Input from '$lib/components/ui/Input.svelte'
  import Textarea from '$lib/components/ui/Textarea.svelte'
  import Select from '$lib/components/ui/Select.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import IconButton from '$lib/components/ui/IconButton.svelte'
  import Badge from '$lib/components/ui/Badge.svelte'
  import Toggle from '$lib/components/ui/Toggle.svelte'
  import Checkbox from '$lib/components/ui/Checkbox.svelte'
  import ConfirmDialog from '$lib/components/ui/ConfirmDialog.svelte'
  import TranslateModal from '$lib/components/ui/TranslateModal.svelte'
  import { templatesApi } from '$lib/api/templates'
  import { translateAll, type TranslateJob } from '$lib/utils/translate'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { DOMAINS, domainLabel, type Domain } from '$lib/types/domain'
  import type { Locale } from '$lib/types/user'
  import {
    blankUserTemplate,
    defaultFootnotes,
    defaultUsoBollo,
    isValidSlug,
    templateDisplayName,
    userSlug,
    TEMPLATE_NAME_LOCALES,
    PLACEHOLDER_SYNTAXES,
    type DocxTemplate,
    type SectionSkeletonEntry,
    type FewShotExample,
  } from '$lib/types/template'
  import { ArrowLeft, Trash2, Copy, Plus, X, Languages } from 'lucide-svelte'

  let {
    initial,
    onback,
    onsaved,
  }: {
    /** Template to edit, or null to create a new one. */
    initial: DocxTemplate | null
    onback: () => void
    onsaved: () => void
  } = $props()

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)

  // ── Working copy ───────────────────────────────────────────────────
  // The editor takes a one-time snapshot of `initial`: it owns a mutable
  // draft and is remounted by the parent whenever the target changes, so
  // the prop is deliberately read only at construction time.
  /* svelte-ignore state_referenced_locally */
  const seed: DocxTemplate | null = initial

  let tpl = $state<DocxTemplate>(
    seed ? (structuredClone($state.snapshot(seed)) as DocxTemplate) : blankUserTemplate(),
  )
  // Ensure every editable display-name slot exists so binding stays string.
  for (const loc of TEMPLATE_NAME_LOCALES) tpl.display_name[loc] ??= ''

  let readOnly = $state(seed?.is_system ?? false)
  let isNew = $state(seed === null)
  let slug = $state(seed ? userSlug(seed.id) : '')
  let saving = $state(false)
  let deleteOpen = $state(false)
  let translateOpen = $state(false)
  let translateDone = $state(0)
  let translateTotal = $state(0)

  // Optional-block toggles.
  let usoBolloOn = $state(!!seed?.uso_bollo)
  let footnotesOn = $state(!!seed?.footnotes)

  // Map fields edited as key/value pair lists.
  type Pair = { k: string; v: string }
  function toPairs(rec: Record<string, unknown> | undefined): Pair[] {
    return Object.entries(rec ?? {}).map(([k, v]) => ({ k, v: String(v) }))
  }
  let styleMapPairs = $state<Pair[]>(toPairs(tpl.style_map))
  let fieldPromptPairs = $state<Pair[]>(toPairs(tpl.field_prompts))
  let charLimitPairs = $state<Pair[]>(toPairs(tpl.character_limits))

  // List fields edited inline. Optional string fields are normalised to
  // '' so `bind:value` never sees `undefined` (Svelte rejects binding an
  // undefined value into an input that has a fallback). `buildPayload`
  // converts the empties back to `undefined` before saving.
  let directives = $state<string[]>([...(tpl.directives_supported ?? [])])
  let requiredMeta = $state<string[]>([...(tpl.required_metadata ?? [])])
  let skeleton = $state<SectionSkeletonEntry[]>(
    (tpl.section_skeleton ?? []).map((s) => ({
      id: s.id ?? '',
      title: s.title ?? '',
      render: s.render ?? '',
      guidance: s.guidance ?? '',
      repeating: !!s.repeating,
    })),
  )
  let fewShot = $state<FewShotExample[]>(
    (tpl.few_shot_examples ?? []).map((e) => ({ label: e.label ?? '', path: e.path ?? '' })),
  )

  // New-tag drafts.
  let directiveDraft = $state('')
  let metaDraft = $state('')

  const numberClass =
    'h-9 w-full rounded-(--radius-md) border border-(--color-surface-300) bg-(--color-surface-0) ' +
    'px-2.5 text-sm text-(--color-text-primary) focus:outline-none focus:border-(--color-brand-500) ' +
    'disabled:opacity-60'

  const domainOptions = DOMAINS.map((d) => ({ value: d, label: domainLabel(d) }))
  const automationOptions = (['L1', 'L2', 'L3', 'L4'] as const).map((l) => ({
    value: l,
    label: t(`DocxTemplates.automation${l}`),
  }))
  const placeholderOptions = PLACEHOLDER_SYNTAXES.map((s) => ({
    value: s,
    label: t(`DocxTemplates.edSyntax_${s}`),
  }))
  const orientationOptions = [
    { value: 'portrait', label: t('DocxTemplates.edOrientationPortrait') },
    { value: 'landscape', label: t('DocxTemplates.edOrientationLandscape') },
  ]
  const paperFormatOptions = [
    { value: 'standard', label: t('DocxTemplates.edFormatStandard') },
    { value: 'uso_bollo', label: t('DocxTemplates.edFormatUsoBollo') },
  ]
  const alignmentOptions = [
    { value: 'justify', label: t('DocxTemplates.edAlignJustify') },
    { value: 'left', label: t('DocxTemplates.edAlignLeft') },
  ]
  const numberingOptions = [
    { value: 'manual', label: t('DocxTemplates.edNumberingManual') },
    { value: 'auto', label: t('DocxTemplates.edNumberingAuto') },
  ]

  const headingTitle = $derived(
    isNew
      ? t('DocxTemplates.edNewTemplate')
      : templateDisplayName(tpl, i18n.locale) || t('DocxTemplates.detailTitle'),
  )

  // ── uso_bollo / footnotes optional blocks ─────────────────────────
  $effect(() => {
    if (tpl.paper.format === 'uso_bollo') usoBolloOn = true
  })
  function ensureUsoBollo() {
    if (usoBolloOn && !tpl.uso_bollo) tpl.uso_bollo = defaultUsoBollo()
  }
  function ensureFootnotes() {
    if (footnotesOn && !tpl.footnotes) tpl.footnotes = defaultFootnotes()
  }
  $effect(ensureUsoBollo)
  $effect(ensureFootnotes)

  // ── tag / list helpers ────────────────────────────────────────────
  function addTag(list: string[], draft: string): string[] {
    const v = draft.trim()
    return v && !list.includes(v) ? [...list, v] : list
  }

  function addSkeleton() {
    skeleton = [...skeleton, { id: '', title: '', render: '', guidance: '', repeating: false }]
  }
  function addFewShot() {
    fewShot = [...fewShot, { label: '', path: '' }]
  }
  function addStyleMap() {
    styleMapPairs = [...styleMapPairs, { k: '', v: '' }]
  }
  function addFieldPrompt() {
    fieldPromptPairs = [...fieldPromptPairs, { k: '', v: '' }]
  }
  function addCharLimit() {
    charLimitPairs = [...charLimitPairs, { k: '', v: '' }]
  }

  // ── duplicate / save / delete ─────────────────────────────────────
  function duplicate() {
    readOnly = false
    isNew = true
    slug = ''
    tpl.is_system = false
    tpl.is_owner = true
    for (const loc of TEMPLATE_NAME_LOCALES) {
      if (tpl.display_name[loc]) {
        tpl.display_name[loc] = t('Workflows.copySuffix', { title: tpl.display_name[loc] })
      }
    }
    toastStore.info(t('DocxTemplates.edDuplicateHint'))
  }

  /**
   * Translate the free-text authoring fields into the chosen language.
   * Runs through the concurrency pool so a 25-field template finishes in
   * seconds instead of grinding one request at a time.
   */
  async function translateTo(locale: Locale) {
    if (readOnly) return
    const jobs: TranslateJob[] = []
    for (const p of fieldPromptPairs) jobs.push({ text: p.v, apply: (v) => (p.v = v) })
    for (const s of skeleton) {
      jobs.push({ text: s.title ?? '', apply: (v) => (s.title = v) })
      jobs.push({ text: s.guidance ?? '', apply: (v) => (s.guidance = v) })
    }
    jobs.push({ text: tpl.prompt_md_extra ?? '', apply: (v) => (tpl.prompt_md_extra = v) })
    jobs.push({ text: tpl.header_block ?? '', apply: (v) => (tpl.header_block = v) })
    jobs.push({ text: tpl.footer_block ?? '', apply: (v) => (tpl.footer_block = v) })

    const err = await translateAll(jobs, locale, (d, total) => {
      translateDone = d
      translateTotal = total
    })
    if (err) toastStore.danger(t('Translate.error'), { detail: err.message })
    else toastStore.success(t('Translate.done'))
  }

  function buildPayload(): DocxTemplate | null {
    if (!isValidSlug(slug)) {
      toastStore.danger(t('DocxTemplates.edValidationSlug'))
      return null
    }
    const names: Record<string, string> = {}
    for (const [k, v] of Object.entries(tpl.display_name)) {
      if (v.trim()) names[k] = v.trim()
    }
    if (Object.keys(names).length === 0) {
      toastStore.danger(t('DocxTemplates.edValidationName'))
      return null
    }
    if (!tpl.category.trim()) {
      toastStore.danger(t('DocxTemplates.edValidationCategory'))
      return null
    }

    const styleMap: Record<string, string> = {}
    for (const p of styleMapPairs) if (p.k.trim()) styleMap[p.k.trim()] = p.v
    const fieldPrompts: Record<string, string> = {}
    for (const p of fieldPromptPairs) if (p.k.trim()) fieldPrompts[p.k.trim()] = p.v
    const charLimits: Record<string, number> = {}
    for (const p of charLimitPairs) {
      const n = Number(p.v)
      if (p.k.trim() && Number.isFinite(n)) charLimits[p.k.trim()] = Math.round(n)
    }

    const out: DocxTemplate = {
      ...$state.snapshot(tpl),
      id: `user/${slug}`,
      display_name: names,
      is_system: false,
      is_owner: true,
      directives_supported: [...directives],
      required_metadata: [...requiredMeta],
      section_skeleton: skeleton
        .filter((s) => s.id.trim())
        .map((s) => ({
          id: s.id.trim(),
          title: s.title?.trim() || undefined,
          render: s.render?.trim() || undefined,
          guidance: s.guidance?.trim() || undefined,
          repeating: !!s.repeating,
        })),
      few_shot_examples: fewShot.filter((e) => e.label.trim() && e.path.trim()),
      style_map: styleMap,
      field_prompts: fieldPrompts,
    }
    out.character_limits = Object.keys(charLimits).length ? charLimits : undefined
    out.uso_bollo = usoBolloOn ? tpl.uso_bollo : undefined
    out.footnotes = footnotesOn ? tpl.footnotes : undefined
    if (!out.source_reference?.trim()) out.source_reference = undefined
    if (!out.header_block?.trim()) out.header_block = undefined
    if (!out.footer_block?.trim()) out.footer_block = undefined
    if (!out.prompt_md_extra?.trim()) out.prompt_md_extra = undefined
    return out
  }

  async function save() {
    const payload = buildPayload()
    if (!payload) return
    saving = true
    try {
      await templatesApi.save(payload)
      toastStore.success(t('DocxTemplates.edSavedToast'))
      onsaved()
    } catch (e) {
      toastStore.danger(t('DocxTemplates.edSaveError'), { detail: (e as Error).message })
    } finally {
      saving = false
    }
  }

  async function confirmDelete() {
    deleteOpen = false
    try {
      await templatesApi.remove(tpl.id)
      toastStore.info(t('DocxTemplates.edDeletedToast'))
      onsaved()
    } catch (e) {
      toastStore.danger(t('DocxTemplates.edSaveError'), { detail: (e as Error).message })
    }
  }
</script>

<div class="max-w-3xl mx-auto p-8 space-y-6">
  <button
    type="button"
    onclick={onback}
    class="flex items-center gap-1.5 text-sm text-(--color-text-secondary) hover:text-(--color-text-primary)"
  >
    <ArrowLeft size={15} />{t('DocxTemplates.title')}
  </button>

  <header class="flex items-start justify-between gap-4">
    <div class="space-y-1 min-w-0">
      <h2 class="text-2xl font-semibold text-(--color-text-primary) truncate">{headingTitle}</h2>
      <p class="text-sm text-(--color-text-secondary)">
        {readOnly ? t('DocxTemplates.edSystemReadOnly') : t('DocxTemplates.edSubtitle')}
      </p>
    </div>
    <div class="flex items-center gap-2 shrink-0">
      {#if readOnly}
        <Badge tone="neutral" size="xs">{t('Workflows.readOnly')}</Badge>
      {/if}
      <Button size="sm" variant="secondary" onclick={duplicate}>
        <Copy size={14} class="mr-1" />{t('Workflows.duplicate')}
      </Button>
      {#if !readOnly}
        <Button
          size="sm"
          variant="ghost"
          title={t('Translate.title')}
          onclick={() => (translateOpen = true)}
        >
          <Languages size={14} class="mr-1" />{t('Workflows.translate')}
        </Button>
        {#if !isNew}
          <IconButton
            label={t('Common.delete')}
            size="sm"
            variant="danger"
            onclick={() => (deleteOpen = true)}
          >
            <Trash2 size={15} />
          </IconButton>
        {/if}
        <Button size="sm" variant="primary" loading={saving} onclick={save}>
          {t('Common.save')}
        </Button>
      {/if}
    </div>
  </header>

  {#snippet sectionHead(label: string)}
    <h3 class="text-xs font-semibold uppercase tracking-wide text-(--color-text-secondary)
               border-b border-(--color-surface-200) pb-1.5">
      {label}
    </h3>
  {/snippet}

  <!-- ── Identity ──────────────────────────────────────────────────── -->
  <section class="space-y-3">
    {@render sectionHead(t('DocxTemplates.edSectionIdentity'))}

    <div class="grid grid-cols-2 gap-3">
      <Input
        label={t('DocxTemplates.edIdentifier')}
        value={slug}
        oninput={(e) => (slug = (e.currentTarget as HTMLInputElement).value)}
        placeholder="contratto-mio"
        hint={t('DocxTemplates.edIdentifierHint')}
        disabled={readOnly || !isNew}
        error={slug && !isValidSlug(slug) ? t('DocxTemplates.edValidationSlug') : undefined}
      />
      <Input label={t('DocxTemplates.edCategory')} bind:value={tpl.category} disabled={readOnly} />
    </div>

    <div class="space-y-1.5">
      <span class="text-xs font-medium text-(--color-text-secondary)">
        {t('DocxTemplates.edDisplayNames')}
      </span>
      <div class="grid grid-cols-2 gap-2">
        {#each TEMPLATE_NAME_LOCALES as loc (loc)}
          <Input
            size="sm"
            label={loc.toUpperCase()}
            bind:value={tpl.display_name[loc]}
            disabled={readOnly}
          />
        {/each}
      </div>
    </div>

    <div class="grid grid-cols-3 gap-3">
      <Select
        label={t('DocxTemplates.edDomain')}
        options={domainOptions}
        bind:value={tpl.domain}
        disabled={readOnly}
      />
      <Select
        label={t('DocxTemplates.automationLevel')}
        options={automationOptions}
        bind:value={tpl.automation_level}
        disabled={readOnly}
      />
      <Input label={t('DocxTemplates.edLocale')} bind:value={tpl.locale} disabled={readOnly} />
    </div>

    <div class="space-y-1.5">
      <span class="text-xs font-medium text-(--color-text-secondary)">
        {t('DocxTemplates.edAlsoApplicable')}
      </span>
      <div class="flex flex-wrap gap-x-4 gap-y-1.5">
        {#each DOMAINS.filter((d) => d !== tpl.domain) as d (d)}
          <Checkbox
            label={domainLabel(d)}
            checked={tpl.also_applicable_to.includes(d)}
            disabled={readOnly}
            onchange={(e) => {
              const on = (e.currentTarget as HTMLInputElement).checked
              tpl.also_applicable_to = on
                ? [...tpl.also_applicable_to, d as Domain]
                : tpl.also_applicable_to.filter((x) => x !== d)
            }}
          />
        {/each}
      </div>
    </div>

    <div class="grid grid-cols-2 gap-3">
      <Select
        label={t('DocxTemplates.edPlaceholderSyntax')}
        options={placeholderOptions}
        bind:value={tpl.placeholder_syntax}
        disabled={readOnly}
      />
      <Input
        label={t('DocxTemplates.sourceReference')}
        value={tpl.source_reference ?? ''}
        oninput={(e) => (tpl.source_reference = (e.currentTarget as HTMLInputElement).value)}
        disabled={readOnly}
      />
    </div>
  </section>

  <!-- ── Layout & margins ──────────────────────────────────────────── -->
  <section class="space-y-3">
    {@render sectionHead(t('DocxTemplates.edSectionLayout'))}

    <div class="grid grid-cols-3 gap-3">
      <Input label={t('DocxTemplates.edPaperSize')} bind:value={tpl.paper.size} disabled={readOnly} />
      <Select
        label={t('DocxTemplates.edOrientation')}
        options={orientationOptions}
        bind:value={tpl.paper.orientation}
        disabled={readOnly}
      />
      <Select
        label={t('DocxTemplates.edPaperFormat')}
        options={paperFormatOptions}
        bind:value={tpl.paper.format}
        disabled={readOnly}
      />
    </div>

    <div class="space-y-1">
      <span class="text-xs font-medium text-(--color-text-secondary)">
        {t('DocxTemplates.edMargins')}
      </span>
      <div class="grid grid-cols-4 gap-2">
        {#each [['top', 'edMarginTop'], ['right', 'edMarginRight'], ['bottom', 'edMarginBottom'], ['left', 'edMarginLeft']] as [key, label] (key)}
          <label class="block space-y-1">
            <span class="text-xs text-(--color-text-secondary)">{t(`DocxTemplates.${label}`)}</span>
            <input
              type="number"
              step="0.1"
              min="0"
              class={numberClass}
              disabled={readOnly}
              bind:value={
                tpl.margins_cm[key as 'top' | 'right' | 'bottom' | 'left']
              }
            />
          </label>
        {/each}
      </div>
    </div>

    {#if !readOnly || usoBolloOn}
      <Toggle
        label={t('DocxTemplates.edUsoBollo')}
        description={t('DocxTemplates.edUsoBolloHint')}
        checked={usoBolloOn}
        disabled={readOnly || tpl.paper.format === 'uso_bollo'}
        onchange={(v) => {
          usoBolloOn = v
          if (v) ensureUsoBollo()
        }}
      />
    {/if}
    {#if usoBolloOn && tpl.uso_bollo}
      <div class="grid grid-cols-3 gap-3 pl-1 border-l-2 border-(--color-surface-200)">
        <label class="block space-y-1">
          <span class="text-xs text-(--color-text-secondary)">{t('DocxTemplates.edLineSpacingPt')}</span>
          <input type="number" step="0.01" class={numberClass} disabled={readOnly}
            bind:value={tpl.uso_bollo.line_spacing_pt_exact} />
        </label>
        <label class="block space-y-1">
          <span class="text-xs text-(--color-text-secondary)">{t('DocxTemplates.edLinesPerFacciata')}</span>
          <input type="number" class={numberClass} disabled={readOnly}
            bind:value={tpl.uso_bollo.lines_per_facciata} />
        </label>
        <label class="block space-y-1">
          <span class="text-xs text-(--color-text-secondary)">{t('DocxTemplates.edFacciatePerFoglio')}</span>
          <input type="number" class={numberClass} disabled={readOnly}
            bind:value={tpl.uso_bollo.facciate_per_foglio} />
        </label>
        <div class="col-span-3 flex flex-wrap gap-x-4 gap-y-1.5">
          <Checkbox label={t('DocxTemplates.edMirrorMargins')} bind:checked={tpl.uso_bollo.mirror_margins} disabled={readOnly} />
          <Checkbox label={t('DocxTemplates.edDuplex')} bind:checked={tpl.uso_bollo.duplex} disabled={readOnly} />
          <Checkbox label={t('DocxTemplates.edForbidEmptyLines')} bind:checked={tpl.uso_bollo.forbid_empty_lines} disabled={readOnly} />
          <Checkbox label={t('DocxTemplates.edMarginalSignature')} bind:checked={tpl.uso_bollo.marginal_signature_required} disabled={readOnly} />
          <Checkbox label={t('DocxTemplates.edSignatureExcludeLast')} bind:checked={tpl.uso_bollo.signature_exclude_last_page} disabled={readOnly} />
        </div>
      </div>
    {/if}
  </section>

  <!-- ── Typography ────────────────────────────────────────────────── -->
  <section class="space-y-3">
    {@render sectionHead(t('DocxTemplates.edSectionTypography'))}

    <div class="grid grid-cols-3 gap-3">
      <Input label={t('DocxTemplates.edBodyFont')} bind:value={tpl.typography.body_font} disabled={readOnly} />
      <label class="block space-y-1">
        <span class="text-xs text-(--color-text-secondary)">{t('DocxTemplates.edBodySize')}</span>
        <input type="number" step="0.5" class={numberClass} disabled={readOnly}
          bind:value={tpl.typography.body_size_pt} />
      </label>
      <label class="block space-y-1">
        <span class="text-xs text-(--color-text-secondary)">{t('DocxTemplates.edLineSpacing')}</span>
        <input type="number" step="0.05" class={numberClass} disabled={readOnly}
          bind:value={tpl.typography.line_spacing} />
      </label>
      <label class="block space-y-1">
        <span class="text-xs text-(--color-text-secondary)">{t('DocxTemplates.edParagraphAfter')}</span>
        <input type="number" step="1" class={numberClass} disabled={readOnly}
          bind:value={tpl.typography.paragraph_after_pt} />
      </label>
      <Select
        label={t('DocxTemplates.edAlignment')}
        options={alignmentOptions}
        bind:value={tpl.typography.alignment}
        disabled={readOnly}
      />
      <label class="block space-y-1">
        <span class="text-xs text-(--color-text-secondary)">{t('DocxTemplates.edFirstLineIndent')}</span>
        <input type="number" step="0.1" class={numberClass} disabled={readOnly}
          bind:value={tpl.typography.first_line_indent_cm} />
      </label>
    </div>

    {#if !readOnly || footnotesOn}
      <Toggle
        label={t('DocxTemplates.edFootnotes')}
        checked={footnotesOn}
        disabled={readOnly}
        onchange={(v) => {
          footnotesOn = v
          if (v) ensureFootnotes()
        }}
      />
    {/if}
    {#if footnotesOn && tpl.footnotes}
      <div class="grid grid-cols-3 gap-3 pl-1 border-l-2 border-(--color-surface-200)">
        <Input label={t('DocxTemplates.edFootnoteFont')} bind:value={tpl.footnotes.font} disabled={readOnly} />
        <label class="block space-y-1">
          <span class="text-xs text-(--color-text-secondary)">{t('DocxTemplates.edFootnoteSize')}</span>
          <input type="number" step="0.5" class={numberClass} disabled={readOnly}
            bind:value={tpl.footnotes.size_pt} />
        </label>
        <label class="block space-y-1">
          <span class="text-xs text-(--color-text-secondary)">{t('DocxTemplates.edLineSpacing')}</span>
          <input type="number" step="0.05" class={numberClass} disabled={readOnly}
            bind:value={tpl.footnotes.line_spacing} />
        </label>
      </div>
    {/if}
  </section>

  <!-- ── Styles & structure ────────────────────────────────────────── -->
  <section class="space-y-3">
    {@render sectionHead(t('DocxTemplates.edSectionStyles'))}

    <div class="space-y-1.5">
      <span class="text-xs font-medium text-(--color-text-secondary)">
        {t('DocxTemplates.edStyleMapBaseline')}
      </span>
      <div class="grid grid-cols-2 gap-2">
        {#each Object.keys(tpl.style_map_baseline) as key (key)}
          <Input size="sm" label={key} bind:value={tpl.style_map_baseline[key]} disabled={readOnly} />
        {/each}
      </div>
    </div>

    {@render kvList(
      t('DocxTemplates.edStyleMap'),
      styleMapPairs,
      addStyleMap,
      (i) => (styleMapPairs = styleMapPairs.filter((_, idx) => idx !== i)),
    )}

    <div class="grid grid-cols-2 gap-3">
      <Select
        label={t('DocxTemplates.edSectionNumbering')}
        options={numberingOptions}
        bind:value={tpl.section_numbering}
        disabled={readOnly}
      />
    </div>

    {@render tagList(
      t('DocxTemplates.edDirectives'),
      directives,
      () => directiveDraft,
      (v) => (directiveDraft = v),
      () => {
        directives = addTag(directives, directiveDraft)
        directiveDraft = ''
      },
      (i) => (directives = directives.filter((_, idx) => idx !== i)),
    )}

    <Textarea
      label={t('DocxTemplates.edHeaderBlock')}
      value={tpl.header_block ?? ''}
      oninput={(e) => (tpl.header_block = (e.currentTarget as HTMLTextAreaElement).value)}
      minRows={2}
      disabled={readOnly}
    />
    <Textarea
      label={t('DocxTemplates.edFooterBlock')}
      value={tpl.footer_block ?? ''}
      oninput={(e) => (tpl.footer_block = (e.currentTarget as HTMLTextAreaElement).value)}
      minRows={2}
      disabled={readOnly}
    />
  </section>

  <!-- ── Authoring contract ────────────────────────────────────────── -->
  <section class="space-y-3">
    {@render sectionHead(t('DocxTemplates.edSectionAuthoring'))}

    <!-- section skeleton -->
    <div class="space-y-2">
      <div class="flex items-center justify-between">
        <span class="text-xs font-medium text-(--color-text-secondary)">
          {t('DocxTemplates.sectionSkeleton')}
        </span>
        {#if !readOnly}
          <Button size="sm" variant="secondary" onclick={addSkeleton}>
            <Plus size={13} class="mr-1" />{t('DocxTemplates.edAddSection')}
          </Button>
        {/if}
      </div>
      {#each skeleton as entry, i (i)}
        <div class="border border-(--color-surface-200) rounded-(--radius-md) p-2.5 space-y-2">
          <div class="flex items-center gap-2">
            <Input size="sm" placeholder={t('DocxTemplates.edSectionId')} bind:value={entry.id}
              class="flex-1" disabled={readOnly} />
            <Input size="sm" placeholder={t('DocxTemplates.edSectionTitle')} bind:value={entry.title}
              class="flex-1" disabled={readOnly} />
            {#if !readOnly}
              <IconButton label={t('Common.delete')} size="sm" variant="danger"
                onclick={() => (skeleton = skeleton.filter((_, idx) => idx !== i))}>
                <X size={14} />
              </IconButton>
            {/if}
          </div>
          <div class="flex items-center gap-2">
            <Input size="sm" placeholder={t('DocxTemplates.edSectionRender')} bind:value={entry.render}
              class="flex-1" disabled={readOnly} />
            <Input size="sm" placeholder={t('DocxTemplates.edSectionGuidance')} bind:value={entry.guidance}
              class="flex-1" disabled={readOnly} />
            <Checkbox label={t('DocxTemplates.edRepeating')} bind:checked={entry.repeating}
              disabled={readOnly} />
          </div>
        </div>
      {/each}
    </div>

    {@render tagList(
      t('DocxTemplates.edRequiredMetadata'),
      requiredMeta,
      () => metaDraft,
      (v) => (metaDraft = v),
      () => {
        requiredMeta = addTag(requiredMeta, metaDraft)
        metaDraft = ''
      },
      (i) => (requiredMeta = requiredMeta.filter((_, idx) => idx !== i)),
    )}

    {@render kvList(
      t('DocxTemplates.edFieldPrompts'),
      fieldPromptPairs,
      addFieldPrompt,
      (i) => (fieldPromptPairs = fieldPromptPairs.filter((_, idx) => idx !== i)),
    )}

    {@render kvList(
      t('DocxTemplates.edCharacterLimits'),
      charLimitPairs,
      addCharLimit,
      (i) => (charLimitPairs = charLimitPairs.filter((_, idx) => idx !== i)),
    )}

    <!-- few-shot examples -->
    <div class="space-y-2">
      <div class="flex items-center justify-between">
        <span class="text-xs font-medium text-(--color-text-secondary)">
          {t('DocxTemplates.edFewShot')}
        </span>
        {#if !readOnly}
          <Button size="sm" variant="secondary" onclick={addFewShot}>
            <Plus size={13} class="mr-1" />{t('DocxTemplates.edAddRow')}
          </Button>
        {/if}
      </div>
      {#each fewShot as ex, i (i)}
        <div class="flex items-center gap-2">
          <Input size="sm" placeholder={t('DocxTemplates.edExampleLabel')} bind:value={ex.label}
            class="flex-1" disabled={readOnly} />
          <Input size="sm" placeholder={t('DocxTemplates.edExamplePath')} bind:value={ex.path}
            class="flex-1" disabled={readOnly} />
          {#if !readOnly}
            <IconButton label={t('Common.delete')} size="sm" variant="danger"
              onclick={() => (fewShot = fewShot.filter((_, idx) => idx !== i))}>
              <X size={14} />
            </IconButton>
          {/if}
        </div>
      {/each}
    </div>

    <Textarea
      label={t('DocxTemplates.edPromptExtra')}
      value={tpl.prompt_md_extra ?? ''}
      oninput={(e) => (tpl.prompt_md_extra = (e.currentTarget as HTMLTextAreaElement).value)}
      minRows={3}
      disabled={readOnly}
    />
  </section>

  {#if !readOnly}
    <div class="flex justify-end border-t border-(--color-surface-200) pt-4">
      <Button variant="primary" loading={saving} onclick={save}>{t('Common.save')}</Button>
    </div>
  {/if}
</div>

<!-- ── reusable snippets ─────────────────────────────────────────────── -->
{#snippet kvList(label: string, pairs: { k: string; v: string }[], add: () => void, remove: (i: number) => void)}
  <div class="space-y-2">
    <div class="flex items-center justify-between">
      <span class="text-xs font-medium text-(--color-text-secondary)">{label}</span>
      {#if !readOnly}
        <Button size="sm" variant="secondary" onclick={add}>
          <Plus size={13} class="mr-1" />{t('DocxTemplates.edAddRow')}
        </Button>
      {/if}
    </div>
    {#each pairs as pair, i (i)}
      <div class="flex items-center gap-2">
        <Input size="sm" placeholder={t('DocxTemplates.edKey')} bind:value={pair.k}
          class="w-1/3" disabled={readOnly} />
        <Input size="sm" placeholder={t('DocxTemplates.edValue')} bind:value={pair.v}
          class="flex-1" disabled={readOnly} />
        {#if !readOnly}
          <IconButton label={t('Common.delete')} size="sm" variant="danger" onclick={() => remove(i)}>
            <X size={14} />
          </IconButton>
        {/if}
      </div>
    {/each}
  </div>
{/snippet}

{#snippet tagList(
  label: string,
  items: string[],
  getDraft: () => string,
  setDraft: (v: string) => void,
  commit: () => void,
  remove: (i: number) => void,
)}
  <div class="space-y-1.5">
    <span class="text-xs font-medium text-(--color-text-secondary)">{label}</span>
    <div class="flex flex-wrap items-center gap-1.5">
      {#each items as item, i (i)}
        <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-(--radius-sm)
                     bg-(--color-surface-100) text-xs text-(--color-text-primary)">
          {item}
          {#if !readOnly}
            <button type="button" onclick={() => remove(i)} aria-label={t('Common.delete')}
              class="text-(--color-text-secondary) hover:text-(--color-danger-500)">
              <X size={11} />
            </button>
          {/if}
        </span>
      {/each}
      {#if !readOnly}
        <input
          value={getDraft()}
          oninput={(e) => setDraft((e.currentTarget as HTMLInputElement).value)}
          onkeydown={(e) => {
            if (e.key === 'Enter' || e.key === ',') {
              e.preventDefault()
              commit()
            }
          }}
          placeholder={t('DocxTemplates.edAddTag')}
          class="h-7 w-40 rounded-(--radius-sm) border border-(--color-surface-300)
                 bg-(--color-surface-0) px-2 text-xs text-(--color-text-primary)
                 focus:outline-none focus:border-(--color-brand-500)"
        />
      {/if}
    </div>
  </div>
{/snippet}

<ConfirmDialog
  bind:open={deleteOpen}
  title={t('DocxTemplates.edDeleteConfirmTitle')}
  message={t('DocxTemplates.edDeleteConfirmBody', { title: templateDisplayName(tpl, i18n.locale) })}
  confirmLabel={t('Common.delete')}
  danger
  onconfirm={confirmDelete}
/>

<TranslateModal
  bind:open={translateOpen}
  onconfirm={translateTo}
  done={translateDone}
  total={translateTotal}
/>
