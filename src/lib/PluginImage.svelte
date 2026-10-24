<script lang="ts">
  import type { Snippet } from "svelte";
  import { safePluginIconUrl } from "$lib/extensionCatalog";
  let { url, darkUrl, children }: { url?: string | null; darkUrl?: string | null; children: Snippet } = $props();
  let failedUrl = $state<string | null>(null);
  let failedDarkUrl = $state<string | null>(null);
  const source = $derived(safePluginIconUrl(url));
  const darkSource = $derived(safePluginIconUrl(darkUrl));
  const light = $derived(source && failedUrl !== source ? source : null);
  const dark = $derived(darkSource && failedDarkUrl !== darkSource ? darkSource : null);

  function imageFailed(event: Event) {
    const failed = (event.currentTarget as HTMLImageElement).getAttribute("src");
    if (failed === source) failedUrl = failed;
    if (failed === darkSource) failedDarkUrl = failed;
  }
</script>

<span class="plugin-image">
  {#if light || dark}
    <img class:has-dark={Boolean(light && dark && light !== dark)} src={light ?? dark ?? ""} alt="" loading="lazy" decoding="async" crossorigin="anonymous" referrerpolicy="no-referrer" onerror={imageFailed} />
    {#if light && dark && light !== dark}
      <img class="dark-image" src={dark} alt="" loading="lazy" decoding="async" crossorigin="anonymous" referrerpolicy="no-referrer" onerror={imageFailed} />
    {/if}
  {:else}
    {@render children()}
  {/if}
</span>

<style>
  .plugin-image { width: 100%; height: 100%; display: grid; place-items: center; border-radius: inherit; overflow: hidden; }
  img { width: 100%; height: 100%; object-fit: contain; }
  .dark-image { display: none; }
  :global(.workspace.dark) .has-dark { display: none; }
  :global(.workspace.dark) .dark-image { display: block; }
</style>
