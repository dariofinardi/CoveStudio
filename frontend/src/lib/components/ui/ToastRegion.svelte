<!-- Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only. -->
<script lang="ts">
  import { toastStore } from '$lib/stores/toast.svelte'
  import Toast from './Toast.svelte'
</script>

<div
  class="fixed bottom-4 right-4 z-[60] flex flex-col gap-2 items-end
         pointer-events-none"
  aria-live="polite"
>
  {#each toastStore.queue as toast (toast.id)}
    <div class="pointer-events-auto">
      <Toast {toast} ondismiss={toastStore.dismiss} />
    </div>
  {/each}
</div>
