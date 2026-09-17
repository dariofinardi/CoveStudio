<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<!--
  Assistant screen — the active conversation. The chat history lives in
  the app sidebar; this screen is just the message stream + composer.
-->
<script lang="ts">
  import ChatMessage from '$lib/components/chat/ChatMessage.svelte'
  import ChatInput from '$lib/components/chat/ChatInput.svelte'
  import EmbeddingBanner from '$lib/components/chat/EmbeddingBanner.svelte'
  import Logo from '$lib/components/ui/Logo.svelte'
  import { chatStore, type SendAttachments } from '$lib/stores/chat.svelte'
  import { userStore } from '$lib/stores/user.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { router } from '$lib/stores/router.svelte'
  import { ArrowDown, ArrowLeft } from 'lucide-svelte'

  const t = (k: string, p?: Record<string, string | number>) => i18n.t(k, p)

  let scroller = $state<HTMLDivElement>()
  /** True while the view is pinned to the bottom (sticky auto-scroll). */
  let atBottom = $state(true)

  function checkAtBottom() {
    if (!scroller) return
    atBottom = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 120
  }

  function scrollToBottom() {
    if (scroller) scroller.scrollTop = scroller.scrollHeight
    atBottom = true
  }

  // Auto-scroll as messages grow / stream — but only while the user is
  // pinned to the bottom, so scrolling up to read isn't yanked back.
  $effect(() => {
    void chatStore.messages.length
    void chatStore.messages.at(-1)?.content
    void chatStore.messages.at(-1)?.steps?.length
    if (atBottom) queueMicrotask(scrollToBottom)
  })

  const greetingName = $derived(
    userStore.profile?.display_name ?? userStore.profile?.username ?? ''
  )

  function onsend(text: string, attach: SendAttachments) {
    // A new turn always re-pins to the bottom.
    atBottom = true
    void chatStore.send(text, attach)
  }
</script>

<div class="flex flex-col h-full">
  {#if router.back}
    <!-- Drill-down breadcrumb. The previous screen pushed a back entry
         (e.g. ProjectDetail.openChat) so the user has a single-click
         path back to the originating context — not the global sidebar
         flat-list which would lose where they came from. The label is
         supplied by the caller (e.g. "Torna a 'Studio 2026'"); falls
         back to the generic Common.back when omitted. -->
    <div class="border-b border-(--color-surface-200) px-6 py-2">
      <button
        type="button"
        onclick={() => router.popBack()}
        class="inline-flex items-center gap-1.5 text-sm text-(--color-text-secondary) hover:text-(--color-text-primary)"
      >
        <ArrowLeft size={14} />
        {router.back.label ?? t('Common.back')}
      </button>
    </div>
  {/if}
  <div class="relative flex-1 min-h-0">
    <div
      bind:this={scroller}
      onscroll={checkAtBottom}
      class="h-full overflow-y-auto"
    >
      {#if chatStore.messages.length === 0}
        <div class="h-full flex flex-col items-center justify-center text-center px-6 gap-2">
          <div class="flex items-center gap-3">
            <Logo size={42} activity={chatStore.streaming ? 'thinking' : 'idle'} />
            <h2 class="text-2xl font-semibold text-(--color-text-primary)">
              {t('Assistant.greeting', { name: greetingName })}
            </h2>
          </div>
          <p class="text-sm text-(--color-text-secondary) max-w-md">
            {t('Assistant.emptyHint')}
          </p>
        </div>
      {:else}
        <div class="max-w-3xl mx-auto px-6 py-6 flex flex-col gap-4">
          {#each chatStore.messages as msg, i (i)}
            <ChatMessage
              message={msg}
              priorCitations={chatStore.messages
                .slice(0, i)
                .filter((m) => m.role === 'assistant')
                .flatMap((m) => m.citations ?? [])}
            />
          {/each}
        </div>
      {/if}
    </div>

    <!-- Jump-to-latest: shown when scrolled up, especially while streaming. -->
    {#if !atBottom && chatStore.messages.length > 0}
      <button
        type="button"
        onclick={scrollToBottom}
        class="absolute bottom-3 left-1/2 -translate-x-1/2 z-10 inline-flex items-center gap-1.5
               px-3 h-8 rounded-full text-xs font-medium
               bg-(--color-surface-0) border border-(--color-surface-200)
               shadow-(--shadow-modal) text-(--color-text-secondary)
               hover:text-(--color-text-primary)"
      >
        {#if chatStore.streaming}
          <span class="streaming-caret"></span>
        {/if}
        <ArrowDown size={13} />
        {t('Assistant.jumpToLatest')}
      </button>
    {/if}
  </div>

  <div class="max-w-3xl w-full mx-auto px-6 pb-5 pt-1">
    {#if chatStore.error}
      <p class="text-xs text-(--color-danger-500) mb-2">{chatStore.error}</p>
    {/if}
    <EmbeddingBanner active={chatStore.streaming} />
    <ChatInput streaming={chatStore.streaming} {onsend} onstop={() => chatStore.abort()} />
    <p class="text-xs text-center text-(--color-text-secondary) mt-2">
      {t('Chat.disclaimer')}
    </p>
  </div>
</div>
