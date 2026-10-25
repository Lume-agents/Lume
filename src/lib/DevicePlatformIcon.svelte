<script lang="ts">
  import { devicePlatform } from "$lib/connectionWatch";

  let { name, size = 22 }: { name: string; size?: number } = $props();
  const platform = $derived(devicePlatform(name));
</script>

<span class="platform-icon" class:ios={platform === "ios"} role="img" aria-label={platform === "ios" ? "iOS" : "Android"} title={platform === "ios" ? "iOS" : "Android"} style:width="{size}px" style:height="{size}px">
  {#if platform === "ios"}
    <svg viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="M16.4 12.6c0-2.3 1.9-3.4 2-3.5-1.1-1.6-2.8-1.8-3.4-1.8-1.4-.15-2.8.85-3.5.85-.73 0-1.85-.83-3.05-.8-1.57.02-3.02.91-3.83 2.3-1.63 2.83-.42 7.03 1.17 9.32.78 1.12 1.7 2.38 2.9 2.34 1.17-.05 1.61-.75 3.02-.75 1.4 0 1.8.75 3.03.73 1.25-.02 2.05-1.14 2.81-2.27.89-1.3 1.25-2.55 1.27-2.62-.03-.01-2.44-.94-2.46-3.77ZM14.1 5.8c.64-.78 1.08-1.86.96-2.94-.93.04-2.05.62-2.71 1.4-.6.69-1.12 1.8-.98 2.85 1.03.08 2.09-.53 2.73-1.31Z" /></svg>
  {:else}
    <svg viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="M17.5 15.3a.9.9 0 1 1 0-1.8.9.9 0 0 1 0 1.8Zm-11 0a.9.9 0 1 1 0-1.8.9.9 0 0 1 0 1.8ZM17.8 9.9l1.6-2.8a.33.33 0 1 0-.57-.33l-1.62 2.8A9.9 9.9 0 0 0 12 8.5c-1.5 0-2.9.33-4.2.9L6.18 6.6a.33.33 0 1 0-.57.33l1.6 2.8C4.5 11.2 2.7 13.8 2.5 16.8h19c-.2-3-2-5.6-4.7-6.9Z" /></svg>
  {/if}
</span>

<style>
  .platform-icon { display: inline-grid; place-items: center; flex: 0 0 auto; border-radius: 50%; color: #3ddc84; background: color-mix(in srgb, #3ddc84 14%, transparent); }
  .platform-icon.ios { color: var(--workspace-text, currentColor); background: color-mix(in srgb, currentColor 10%, transparent); }
  .platform-icon svg { width: 62%; height: 62%; }
</style>
