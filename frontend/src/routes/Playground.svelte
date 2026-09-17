<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Internal dev-only reference page exercising every UI primitive.
  Reached via `?playground` in the URL. Not part of the shipped routes.
-->
<script lang="ts">
  import { PRODUCT_NAME } from '$lib/product'
  import Button from '$lib/components/ui/Button.svelte'
  import IconButton from '$lib/components/ui/IconButton.svelte'
  import Input from '$lib/components/ui/Input.svelte'
  import Textarea from '$lib/components/ui/Textarea.svelte'
  import Checkbox from '$lib/components/ui/Checkbox.svelte'
  import Toggle from '$lib/components/ui/Toggle.svelte'
  import Select from '$lib/components/ui/Select.svelte'
  import Badge from '$lib/components/ui/Badge.svelte'
  import Spinner from '$lib/components/ui/Spinner.svelte'
  import Progress from '$lib/components/ui/Progress.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import Card from '$lib/components/ui/Card.svelte'
  import Modal from '$lib/components/ui/Modal.svelte'
  import Tabs from '$lib/components/ui/Tabs.svelte'
  import ChipGroup from '$lib/components/ui/ChipGroup.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'

  let inputValue = $state('')
  let textareaValue = $state('')
  let checkboxValue = $state(true)
  let toggleValue = $state(false)
  let selectValue = $state<string>('legal')
  let modalOpen = $state(false)
  let activeTab = $state('all')
  let chipSelection = $state<string[]>(['medical'])
  let progress = $state(0.42)

  const domainOptions = [
    { value: 'legal', label: 'Legal' },
    { value: 'medical', label: 'Medical' },
    { value: 'finance', label: 'Finance' },
    { value: 'insurance', label: 'Insurance' },
  ]

  const tabs = [
    { id: 'all', label: 'All', count: 56 },
    { id: 'preset', label: 'Built-in', count: 48 },
    { id: 'custom', label: 'Custom', count: 8 },
    { id: 'hidden', label: 'Hidden', count: 0, disabled: true },
  ]

  const practiceChips = [
    { value: 'legal', label: 'Legal' },
    { value: 'medical', label: 'Medical' },
    { value: 'finance', label: 'Finance' },
    { value: 'insurance', label: 'Insurance' },
    { value: 'compliance', label: 'Compliance' },
  ]
</script>

<div class="min-h-full bg-(--color-surface-50) p-8">
  <div class="max-w-4xl mx-auto space-y-8">
    <header class="space-y-1">
      <h1 class="text-2xl font-semibold text-(--color-text-primary)">
        {PRODUCT_NAME} — UI Playground
      </h1>
      <p class="text-sm text-(--color-text-secondary)">
        Fase 1 design system primitives. Append <code class="font-mono">?playground</code>
        to the URL to reach this page.
      </p>
    </header>

    <!-- Buttons -->
    <Card title="Button">
      <div class="space-y-3">
        <div class="flex flex-wrap items-center gap-2">
          <Button variant="primary">Primary</Button>
          <Button variant="secondary">Secondary</Button>
          <Button variant="ghost">Ghost</Button>
          <Button variant="danger">Danger</Button>
          <Button variant="pill">Pill CTA</Button>
        </div>
        <div class="flex flex-wrap items-center gap-2">
          <Button size="sm">Small</Button>
          <Button size="md">Medium</Button>
          <Button size="lg">Large</Button>
          <Button loading>Loading</Button>
          <Button disabled>Disabled</Button>
        </div>
        <div class="flex flex-wrap items-center gap-2">
          <IconButton label="Star" variant="secondary">
            <svg width="16" height="16" viewBox="0 0 20 20" fill="currentColor">
              <path d="M10 1.5l2.6 5.3 5.9.9-4.2 4.1 1 5.8L10 15l-5.3 2.6 1-5.8L1.5 7.7l5.9-.9L10 1.5z"/>
            </svg>
          </IconButton>
          <IconButton label="Delete" variant="danger">
            <svg width="16" height="16" viewBox="0 0 20 20" fill="currentColor">
              <path d="M8 2h4a1 1 0 011 1v1h3a1 1 0 110 2h-1v10a2 2 0 01-2 2H6a2 2 0 01-2-2V6H3a1 1 0 010-2h3V3a1 1 0 011-1z"/>
            </svg>
          </IconButton>
        </div>
      </div>
    </Card>

    <!-- Form controls -->
    <Card title="Form controls">
      <div class="grid grid-cols-2 gap-4">
        <Input label="Text input" placeholder="Type here…" bind:value={inputValue} />
        <Input label="With error" value="bad value" error="This field is required" />
        <Select label="Domain" options={domainOptions} bind:value={selectValue} />
        <Input label="PIN" type="password" placeholder="••••••" maxlength={6} />
        <Textarea label="Autosize textarea" autosize bind:value={textareaValue} placeholder="Grows as you type…" class="col-span-2" />
      </div>
      <div class="flex flex-wrap items-center gap-6 mt-4">
        <Checkbox label="Cache this document" bind:checked={checkboxValue} />
        <Toggle label="Keep me signed in" bind:checked={toggleValue} />
      </div>
    </Card>

    <!-- Badges -->
    <Card title="Badge">
      <div class="flex flex-wrap items-center gap-2">
        <Badge tone="neutral">neutral</Badge>
        <Badge tone="brand">brand</Badge>
        <Badge tone="success">ready</Badge>
        <Badge tone="warning">syncing</Badge>
        <Badge tone="danger">failed</Badge>
        <Badge tone="info">info</Badge>
        <Badge tone="assistant">Assistant</Badge>
        <Badge tone="tabular">Tabular</Badge>
        <Badge tone="level">L1</Badge>
        <Badge tone="level">L2</Badge>
        <Badge tone="level">L3</Badge>
      </div>
    </Card>

    <!-- Feedback -->
    <Card title="Spinner & Progress">
      <div class="space-y-4">
        <div class="flex items-center gap-4 text-(--color-brand-500)">
          <Spinner size="sm" />
          <Spinner size="md" />
          <Spinner size="lg" />
        </div>
        <Progress value={progress} label="Embedding documents" showPercent />
        <Progress value={null} label="Indeterminate" />
        <div class="flex gap-2">
          <Button size="sm" variant="secondary" onclick={() => (progress = Math.max(0, progress - 0.1))}>−10%</Button>
          <Button size="sm" variant="secondary" onclick={() => (progress = Math.min(1, progress + 0.1))}>+10%</Button>
        </div>
      </div>
    </Card>

    <!-- Tabs -->
    <Card title="Tabs">
      <Tabs {tabs} bind:active={activeTab} />
      <p class="text-sm text-(--color-text-secondary) mt-3">
        Active tab: <code class="font-mono">{activeTab}</code>
      </p>
    </Card>

    <!-- ChipGroup -->
    <Card title="ChipGroup (multi-select)">
      <ChipGroup chips={practiceChips} multi bind:selected={chipSelection} />
      <p class="text-sm text-(--color-text-secondary) mt-3">
        Selected: <code class="font-mono">{JSON.stringify(chipSelection)}</code>
      </p>
    </Card>

    <!-- Toasts -->
    <Card title="Toast">
      <div class="flex flex-wrap gap-2">
        <Button size="sm" variant="secondary" onclick={() => toastStore.info('Heads up', { detail: 'An informational message.' })}>Info</Button>
        <Button size="sm" variant="secondary" onclick={() => toastStore.success('Saved', { detail: 'Workflow created successfully.' })}>Success</Button>
        <Button size="sm" variant="secondary" onclick={() => toastStore.warning('Large file', { detail: 'Upload may take a while.' })}>Warning</Button>
        <Button size="sm" variant="secondary" onclick={() => toastStore.danger('Backend offline', { detail: 'Could not reach 127.0.0.1:3001.' })}>Danger (sticky)</Button>
      </div>
    </Card>

    <!-- Modal -->
    <Card title="Modal">
      <Button onclick={() => (modalOpen = true)}>Open modal</Button>
    </Card>

    <!-- EmptyState -->
    <Card title="EmptyState" padded={false}>
      <EmptyState
        title="No workflows yet"
        description="Create your first workflow or pick one from the built-in presets."
      >
        {#snippet action()}
          <Button size="sm">Create workflow</Button>
        {/snippet}
      </EmptyState>
    </Card>
  </div>
</div>

<Modal bind:open={modalOpen} title="Example modal" size="md">
  <p class="text-sm text-(--color-text-secondary)">
    This modal demonstrates backdrop click, Escape-to-close, body scroll lock,
    and a footer action row.
  </p>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (modalOpen = false)}>Cancel</Button>
    <Button onclick={() => (modalOpen = false)}>Confirm</Button>
  {/snippet}
</Modal>
