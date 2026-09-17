<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Authenticated app shell: sidebar (nav + collapsible chat list +
  pinned Settings) + topbar + the active feature route.
-->
<script lang="ts">
  import { PRODUCT_NAME } from '$lib/product'
  import { tick } from 'svelte'
  import AppShell from '$lib/components/layout/AppShell.svelte'
  import Sidebar from '$lib/components/layout/Sidebar.svelte'
  import SidebarItem from '$lib/components/layout/SidebarItem.svelte'
  import TopBar from '$lib/components/layout/TopBar.svelte'
  import Button from '$lib/components/ui/Button.svelte'
  import IconButton from '$lib/components/ui/IconButton.svelte'
  import Logo from '$lib/components/ui/Logo.svelte'
  import EmptyState from '$lib/components/ui/EmptyState.svelte'
  import ConfirmDialog from '$lib/components/ui/ConfirmDialog.svelte'
  import Modal from '$lib/components/ui/Modal.svelte'
  import ThemeToggle from '$lib/components/ui/ThemeToggle.svelte'
  import Workflows from './Workflows.svelte'
  import Templates from './Templates.svelte'
  import Tabular from './Tabular.svelte'
  import Projects from './Projects.svelte'
  import Assistant from './Assistant.svelte'
  import Settings from './Settings.svelte'
  import { router, type FeatureRoute } from '$lib/stores/router.svelte'
  import { authStore } from '$lib/stores/auth.svelte'
  import { userStore } from '$lib/stores/user.svelte'
  import Select from '$lib/components/ui/Select.svelte'
  import type { Domain } from '$lib/types/domain'
  import { domainLabel } from '$lib/types/domain'
  import { chatStore } from '$lib/stores/chat.svelte'
  import { projectStore } from '$lib/stores/projects.svelte'
  import { toastStore } from '$lib/stores/toast.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { APP_VERSION } from '$lib/stores/app-version.svelte'
  import {
    MessageSquare,
    FolderKanban,
    Table2,
    Workflow,
    FileText,
    Settings as SettingsIcon,
    Plus,
    ChevronDown,
    ChevronRight,
    Pencil,
    Trash2,
  } from 'lucide-svelte'

  interface NavEntry {
    route: FeatureRoute
    labelKey: string
    icon: typeof MessageSquare
  }

  // Assistant is rendered separately (it carries the new-chat "+");
  // these are the remaining feature routes.
  const nav: NavEntry[] = [
    { route: 'projects', labelKey: 'Sidebar.projects', icon: FolderKanban },
    { route: 'tabular', labelKey: 'Sidebar.tabularReviews', icon: Table2 },
    { route: 'workflows', labelKey: 'Sidebar.workflows', icon: Workflow },
    { route: 'templates', labelKey: 'Sidebar.docxTemplates', icon: FileText },
  ]

  const titleByRoute: Record<string, string> = {
    assistant: 'Sidebar.assistant',
    projects: 'Sidebar.projects',
    tabular: 'Sidebar.tabularReviews',
    workflows: 'Sidebar.workflows',
    templates: 'Sidebar.docxTemplates',
    settings: 'Common.settings',
  }
  const activeLabel = $derived(
    titleByRoute[router.current] ? i18n.t(titleByRoute[router.current]) : PRODUCT_NAME
  )

  const greetingName = $derived(
    userStore.profile?.display_name ??
      authStore.user?.display_name ??
      authStore.user?.username ??
      ''
  )

  let chatsCollapsed = $state(false)
  let projectsCollapsed = $state(false)

  // Sidebar-level active-domain picker. Persists immediately to
  // `user_settings.default_domain` on change — the global signal
  // that drives every domain-scoped UI (workflow / template
  // pickers, default for new projects + tabular reviews, the chat
  // composer's domain inheritance). Restricted to the enabled-
  // domains preference so users don't see selectors they've
  // hidden in Settings.
  const sidebarDomainOptions = $derived(
    userStore.effectiveEnabledDomains.map((d) => ({
      value: d,
      label: domainLabel(d),
    })),
  )
  async function onSidebarDomainChange(next: Domain) {
    if (!next || next === userStore.defaultDomain) return
    try {
      await userStore.setDefaultDomain(next)
    } catch (e) {
      toastStore.danger(i18n.t('Common.error'), {
        detail: (e as Error).message,
      })
    }
  }

  // "Recently active" surrogate: the top 5 by `updated_at` from the
  // global project list. The store loads the full list once on
  // sidebar mount (see $effect below) and reuses it elsewhere, so no
  // extra round-trip per render.
  const recentProjects = $derived(
    [...projectStore.items]
      .sort((a, b) => (a.updated_at < b.updated_at ? 1 : -1))
      .slice(0, 5)
  )

  $effect(() => {
    // Refresh once on shell mount so the accordion has content even
    // if the user hasn't visited the Projects screen yet. Cheap call
    // (single SELECT); the store dedupes if a refresh is already in
    // flight.
    void projectStore.refresh()
  })
  let deleteChatTarget = $state<{ id: string; title: string | null } | null>(null)
  let renamingChatId = $state<string | null>(null)
  let renameValue = $state('')

  async function confirmDeleteChat() {
    if (!deleteChatTarget) return
    const id = deleteChatTarget.id
    deleteChatTarget = null
    await chatStore.remove(id)
  }

  function startRenameChat(id: string, title: string | null) {
    renamingChatId = id
    renameValue = (title ?? '').trim()
  }

  function focusAtEnd(node: HTMLInputElement) {
    const placeCaret = async () => {
      await tick()
      node.focus()
      const end = node.value.length
      node.setSelectionRange(end, end)
    }
    void placeCaret()
    return {
      update() {
        void placeCaret()
      },
    }
  }

  function cancelRenameChat() {
    renamingChatId = null
    renameValue = ''
  }

  async function commitRenameChat(id: string) {
    const next = renameValue.trim()
    if (!next) {
      cancelRenameChat()
      return
    }
    try {
      await chatStore.rename(id, next)
    } catch (e) {
      toastStore.danger(i18n.t('Common.error'), { detail: (e as Error).message })
    } finally {
      cancelRenameChat()
    }
  }

  $effect(() => {
    void chatStore.refreshChats()
  })

  // Creating or opening a chat while a reply is still streaming would cut
  // that reply off — guard both actions behind a confirmation modal.
  let pendingNav = $state<(() => void) | null>(null)

  function guardedNav(action: () => void) {
    if (chatStore.streaming) pendingNav = action
    else action()
  }
  function confirmInterrupt() {
    const action = pendingNav
    pendingNav = null
    if (action) {
      chatStore.abort()
      action()
    }
  }

  // When the user clicks "+" while a project-scoped chat is active,
  // we don't silently inherit the project for the next chat (the
  // 2026-06-07 report: "ho creato una nuova chat e il progetto è
  // rimasto assegnato senza chiedermelo"). Ask first.
  let newChatModalOpen = $state(false)
  function newChat() {
    if (chatStore.activeProjectId) {
      newChatModalOpen = true
      return
    }
    guardedNav(() => {
      chatStore.newChat()
      router.go('assistant')
    })
  }

  function newChatKeepProject() {
    newChatModalOpen = false
    guardedNav(() => {
      // Default newChat() preserves the composer's project chip —
      // see chatStore.newChat docstring for why.
      chatStore.newChat()
      router.go('assistant')
    })
  }

  function newChatIndependent() {
    newChatModalOpen = false
    guardedNav(() => {
      chatStore.newChat({ clearProject: true })
      router.go('assistant')
    })
  }
  function openChat(id: string) {
    guardedNav(() => {
      void chatStore.selectChat(id)
      router.go('assistant')
    })
  }

  async function logout() {
    await authStore.logout()
    userStore.reset()
    toastStore.info(i18n.t('Common.logout'))
    router.go('unlock')
  }
</script>

<AppShell>
  {#snippet sidebar()}
    <Sidebar>
      {#snippet brand()}
        <div class="flex items-center gap-2 w-full">
          <Logo size={18} activity="idle" class="shrink-0" />
          <span class="text-sm font-semibold whitespace-nowrap shrink-0 text-(--color-brand-600)">{PRODUCT_NAME}</span>
          <span
            class="text-[11px] font-normal whitespace-nowrap shrink-0 text-(--color-text-secondary) tabular-nums"
            title={i18n.t('App.versionTooltip', { version: APP_VERSION })}
          >
            v{APP_VERSION}
          </span>
          <!-- Global active-domain picker. Initialised from the
               user's `default_domain` (set at sign-up), saved
               server-side on every change. Restricted to the
               enabled-domains preference so hidden domains stay
               hidden. -->
          <Select
            options={sidebarDomainOptions}
            value={userStore.defaultDomain}
            onchange={(e: Event) =>
              void onSidebarDomainChange(
                (e.target as HTMLSelectElement).value as Domain,
              )}
            size="sm"
            class="ml-auto min-w-0 max-w-[10rem]"
            title={i18n.t('Sidebar.activeDomainTooltip')}
          />
        </div>
      {/snippet}

      <!-- nav -->
      <div class="px-2 pt-2 flex flex-col gap-0.5 shrink-0">
        <!-- Assistant row carries the new-chat button -->
        <div class="flex items-center gap-1">
          <div class="flex-1 min-w-0">
            <SidebarItem
              label={i18n.t('Sidebar.assistant')}
              active={router.current === 'assistant'}
              onclick={() => router.go('assistant')}
            >
              {#snippet icon()}<MessageSquare size={16} />{/snippet}
            </SidebarItem>
          </div>
          <IconButton label={i18n.t('Assistant.newChat')} size="md" onclick={newChat}>
            <Plus size={16} />
          </IconButton>
        </div>

        {#each nav as entry (entry.route)}
          <SidebarItem
            label={i18n.t(entry.labelKey)}
            active={router.current === entry.route}
            onclick={() => router.go(entry.route)}
          >
            {#snippet icon()}<entry.icon size={16} />{/snippet}
          </SidebarItem>
        {/each}
      </div>

      <!-- collapsible recent-projects list — top 5 by updated_at -->
      <div class="shrink-0 flex flex-col mt-2 border-t border-(--color-surface-200)">
        <button
          type="button"
          onclick={() => (projectsCollapsed = !projectsCollapsed)}
          class="flex items-center gap-1 px-3 h-8 shrink-0 text-xs font-medium uppercase
                 tracking-wide text-(--color-text-secondary)
                 hover:text-(--color-text-primary) focus:outline-none"
        >
          {#if projectsCollapsed}<ChevronRight size={13} />{:else}<ChevronDown size={13} />{/if}
          {i18n.t('Sidebar.recentProjects')}
        </button>

        {#if !projectsCollapsed}
          <div class="px-2 pb-2 flex flex-col gap-0.5">
            {#each recentProjects as p (p.id)}
              <button
                type="button"
                onclick={() => router.go('projects', { projectId: p.id })}
                class="flex items-center gap-2 px-2.5 h-8 rounded-(--radius-md) text-left text-sm
                       focus:outline-none text-(--color-text-secondary)
                       hover:bg-(--color-hover-bg) hover:text-(--color-text-primary)"
              >
                <FolderKanban size={13} class="shrink-0" />
                <span class="truncate">{p.name}</span>
              </button>
            {/each}
            {#if recentProjects.length === 0}
              <p class="text-xs text-(--color-text-disabled) px-2.5 py-2">
                {i18n.t('Sidebar.noProjects')}
              </p>
            {/if}
          </div>
        {/if}
      </div>

      <!-- collapsible chat list -->
      <div class="flex-1 min-h-0 flex flex-col mt-2 border-t border-(--color-surface-200)">
        <button
          type="button"
          onclick={() => (chatsCollapsed = !chatsCollapsed)}
          class="flex items-center gap-1 px-3 h-8 shrink-0 text-xs font-medium uppercase
                 tracking-wide text-(--color-text-secondary)
                 hover:text-(--color-text-primary) focus:outline-none"
        >
          {#if chatsCollapsed}<ChevronRight size={13} />{:else}<ChevronDown size={13} />{/if}
          {i18n.t('Sidebar.recentChats')}
        </button>

        {#if !chatsCollapsed}
          <div class="flex-1 min-h-0 overflow-y-auto px-2 pb-2 flex flex-col gap-0.5">
            {#each chatStore.chats as c (c.id)}
              <div
                class="group flex items-center gap-1 rounded-(--radius-md)
                       {chatStore.activeId === c.id && router.current === 'assistant'
                         ? 'bg-(--color-active-bg)'
                         : 'hover:bg-(--color-hover-bg)'}"
              >
                {#if renamingChatId === c.id}
                  <div class="flex-1 min-w-0 flex items-center gap-2 px-2.5 h-8 text-sm text-(--color-text-secondary)">
                    <MessageSquare size={13} class="shrink-0" />
                    <input
                      bind:value={renameValue}
                      use:focusAtEnd
                      class="flex-1 min-w-0 bg-transparent border-b border-(--color-brand-500) focus:outline-none"
                      onkeydown={(e) => {
                        if (e.key === 'Enter') void commitRenameChat(c.id)
                        if (e.key === 'Escape') cancelRenameChat()
                      }}
                      onblur={() => void commitRenameChat(c.id)}
                    />
                  </div>
                {:else}
                  <button
                    type="button"
                    onclick={() => openChat(c.id)}
                    class="flex-1 min-w-0 flex items-center gap-2 px-2.5 h-8 text-left text-sm
                           focus:outline-none
                           {chatStore.activeId === c.id && router.current === 'assistant'
                             ? 'text-(--color-brand-700)'
                             : 'text-(--color-text-secondary)'}"
                  >
                    <MessageSquare size={13} class="shrink-0" />
                    <span class="truncate">{c.title || i18n.t('Assistant.untitledChat')}</span>
                  </button>
                {/if}
                <IconButton
                  label={i18n.t('Common.rename')}
                  size="sm"
                  class="opacity-0 group-hover:opacity-100"
                  onclick={() => startRenameChat(c.id, c.title)}
                >
                  <Pencil size={13} />
                </IconButton>
                <IconButton
                  label={i18n.t('Common.delete')}
                  size="sm"
                  class="opacity-0 group-hover:opacity-100"
                  onclick={() => (deleteChatTarget = { id: c.id, title: c.title })}
                >
                  <Trash2 size={13} />
                </IconButton>
              </div>
            {/each}
            {#if chatStore.chats.length === 0}
              <p class="text-xs text-(--color-text-disabled) px-2.5 py-2">
                {i18n.t('Sidebar.noChats')}
              </p>
            {/if}
          </div>
        {/if}
      </div>

      {#snippet footer()}
        <SidebarItem
          label={i18n.t('Common.settings')}
          active={router.current === 'settings'}
          onclick={() => router.go('settings')}
        >
          {#snippet icon()}<SettingsIcon size={16} />{/snippet}
        </SidebarItem>
      {/snippet}
    </Sidebar>
  {/snippet}

  {#snippet topbar()}
    <TopBar title={activeLabel}>
      {#snippet actions()}
        <ThemeToggle />
        {#if greetingName}
          <span class="inline-flex items-center gap-1.5 text-xs text-(--color-text-secondary)">
            {#if chatStore.streaming}
              <!-- Orange bullet: an assistant reply is still streaming,
                   even while the user is on another route. -->
              <span
                class="h-2 w-2 shrink-0 rounded-full bg-(--color-brand-500) animate-pulse"
                title={i18n.t('Assistant.responding')}
                aria-label={i18n.t('Assistant.responding')}
              ></span>
            {/if}
            {greetingName}
          </span>
        {/if}
        <Button size="sm" variant="ghost" onclick={logout}>{i18n.t('Common.logout')}</Button>
      {/snippet}
    </TopBar>
  {/snippet}

  {#if router.current === 'assistant'}
    <Assistant />
  {:else if router.current === 'workflows'}
    <Workflows />
  {:else if router.current === 'templates'}
    <Templates />
  {:else if router.current === 'tabular'}
    <Tabular />
  {:else if router.current === 'projects'}
    <Projects />
  {:else if router.current === 'settings'}
    <Settings />
  {:else}
    <div class="p-8">
      <EmptyState
        title={i18n.t('Ui.comingSoonTitle', { screen: activeLabel })}
        description={i18n.t('Ui.comingSoonBody')}
      />
    </div>
  {/if}
</AppShell>

<ConfirmDialog
  open={deleteChatTarget !== null}
  title={i18n.t('Sidebar.deleteChatTitle')}
  message={i18n.t('Sidebar.deleteChatBody', {
    title: deleteChatTarget?.title || i18n.t('Assistant.untitledChat'),
  })}
  confirmLabel={i18n.t('Common.delete')}
  danger
  onconfirm={confirmDeleteChat}
  oncancel={() => (deleteChatTarget = null)}
/>

<ConfirmDialog
  open={pendingNav !== null}
  title={i18n.t('Assistant.interruptTitle')}
  message={i18n.t('Assistant.interruptBody', {
    model: chatStore.streamingModel ?? i18n.t('Assistant.genericModel'),
  })}
  confirmLabel={i18n.t('Assistant.interruptConfirm')}
  danger
  onconfirm={confirmInterrupt}
  oncancel={() => (pendingNav = null)}
/>

<!-- New-chat-in-project confirm. Two action buttons in the footer;
     the third "cancel" action is implicit via the Modal's own close
     X (top-right) + Esc + backdrop click — adding it as a third
     footer button was overflowing the sm container and pushed
     "Annulla" off-screen. md sizing also gives the two action
     labels room to breathe. -->
<Modal
  open={newChatModalOpen}
  title={i18n.t('Sidebar.newChatInProjectTitle')}
  size="md"
  onclose={() => (newChatModalOpen = false)}
>
  <p class="text-sm text-(--color-text-secondary)">
    {i18n.t('Sidebar.newChatInProjectBody')}
  </p>
  {#snippet footer()}
    <Button variant="secondary" onclick={newChatIndependent}>
      {i18n.t('Sidebar.newChatIndependent')}
    </Button>
    <Button onclick={newChatKeepProject}>
      {i18n.t('Sidebar.newChatKeepProject')}
    </Button>
  {/snippet}
</Modal>
