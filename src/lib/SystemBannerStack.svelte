<script module lang="ts">
  export type SystemBannerTone = "info" | "success" | "warning" | "error";
  export type SystemBannerItem = {
    id: string;
    message: string;
    tone?: SystemBannerTone;
    duration?: number;
    onDismiss: () => void;
  };
</script>

<script lang="ts">
  import SystemBanner from "$lib/SystemBanner.svelte";

  let {
    items,
    contained = false,
    dismissLabel = "Dismiss",
  } = $props<{
    items: SystemBannerItem[];
    contained?: boolean;
    dismissLabel?: string;
  }>();

  const visibleItems = $derived(items.slice(0, 4));
</script>

{#if visibleItems.length}
  <section class:contained class="system-banner-stack" aria-label="System notifications">
    {#each visibleItems as item (item.id)}
      <SystemBanner
        message={item.message}
        tone={item.tone}
        duration={item.duration}
        {dismissLabel}
        onDismiss={item.onDismiss}
        inline
      />
    {/each}
  </section>
{/if}

<style>
  .system-banner-stack { position: fixed; z-index: 80; top: max(10px, env(safe-area-inset-top)); left: 50%; width: min(420px, calc(100vw - 22px)); display: grid; gap: 6px; pointer-events: none; transform: translateX(-50%); }
  .system-banner-stack.contained { position: absolute; top: 72px; width: min(390px, calc(100% - 22px)); }
  .system-banner-stack :global(.system-banner) { pointer-events: auto; }
</style>
