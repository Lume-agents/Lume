<script lang="ts">
  import ActivityTypeIcon from "$lib/ActivityTypeIcon.svelte";
  import PluginImage from "$lib/PluginImage.svelte";
  import { mcpBrand } from "$lib/mcpBrands";
  import { mcpPluginIcon, type McpPluginIcon } from "$lib/mcpPluginIcons";

  // An MCP server's own icon when there is one: a known brand, then its installed plugin's, then a plug.
  let { server, label = "", size = 15 }: { server: string; label?: string; size?: number } = $props();
  const brand = $derived(mcpBrand(server, label));
  let plugin = $state<McpPluginIcon | null>(null);
  $effect(() => {
    const name = server;
    plugin = null;
    if (brand) return;
    let live = true;
    void mcpPluginIcon(name).then((icon) => { if (live) plugin = icon; });
    return () => { live = false; };
  });
</script>

{#if brand}
  <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width={size} height={size} aria-hidden="true" focusable="false" style:display="block" style:flex="0 0 auto"><path d={brand.path} fill={brand.color ?? "currentColor"} /></svg>
{:else if plugin}
  <span class="plugin" style:width="{size}px" style:height="{size}px"><PluginImage url={plugin.url} darkUrl={plugin.darkUrl}><ActivityTypeIcon category="mcp" {size} /></PluginImage></span>
{:else}
  <ActivityTypeIcon category="mcp" {size} />
{/if}

<style>
  .plugin { display: block; flex: 0 0 auto; overflow: hidden; border-radius: 4px; }
  .plugin :global(img) { display: block; width: 100%; height: 100%; object-fit: contain; }
</style>
