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
  import SystemBannerDetails from "$lib/SystemBannerDetails.svelte";

  let {
    items,
    contained = false,
    offset,
    language = "en",
    dismissLabel = "Dismiss",
  } = $props<{
    items: SystemBannerItem[];
    contained?: boolean;
    offset?: string;
    language?: "en" | "pt-BR";
    dismissLabel?: string;
  }>();

  const visibleItems = $derived(items.slice(0, 4));
  let inspectedMessage = $state<string | null>(null);
</script>

{#if visibleItems.length}
  <section class:contained class="system-banner-stack" style:--system-banner-offset={offset} aria-label="System notifications">
    {#each visibleItems as item (item.id)}
      <SystemBanner
        message={item.message}
        tone={item.tone}
        duration={item.duration}
        {dismissLabel}
        detailsLabel={language === "pt-BR" ? "Ver detalhes" : "View details"}
        onInspect={() => (inspectedMessage = item.message)}
        onDismiss={item.onDismiss}
        inline
      />
    {/each}
  </section>
{/if}

{#if inspectedMessage !== null}
  <SystemBannerDetails message={inspectedMessage} {language} onClose={() => (inspectedMessage = null)} />
{/if}

<style>
  .system-banner-stack { position: fixed; z-index: var(--system-banner-layer, 80); top: max(10px, env(safe-area-inset-top)); left: 50%; width: min(420px, calc(100vw - 22px)); display: grid; gap: 6px; pointer-events: none; transform: translateX(-50%); }
  .system-banner-stack.contained { position: absolute; top: var(--system-banner-offset, 72px); width: min(390px, calc(100% - 22px)); }
  .system-banner-stack :global(.system-banner) { pointer-events: auto; }
</style>
