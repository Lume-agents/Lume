<script lang="ts">
  let { platform, unknownLabel = "Computer" }: { platform?: "windows" | "linux" | "macos" | null; unknownLabel?: string } = $props();
  const label = $derived(platform === "windows" ? "Windows" : platform === "linux" ? "Linux" : platform === "macos" ? "macOS" : unknownLabel);
</script>

<span class="platform-icon" class:windows={platform === "windows"} role="img" aria-label={label} title={label}>
  {#if platform === "windows"}
    <svg viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="M3 3h8v8H3zm10 0h8v8h-8zM3 13h8v8H3zm10 0h8v8h-8z" /></svg>
  {:else if platform === "macos"}
    <svg viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="M16.4 12.6c0-2.3 1.9-3.4 2-3.5-1.1-1.6-2.8-1.8-3.4-1.8-1.4-.15-2.8.85-3.5.85-.73 0-1.85-.83-3.05-.8-1.57.02-3.02.91-3.83 2.3-1.63 2.83-.42 7.03 1.17 9.32.78 1.12 1.7 2.38 2.9 2.34 1.17-.05 1.61-.75 3.02-.75 1.4 0 1.8.75 3.03.73 1.25-.02 2.05-1.14 2.81-2.27.89-1.3 1.25-2.55 1.27-2.62-.03-.01-2.44-.94-2.46-3.77ZM14.1 5.8c.64-.78 1.08-1.86.96-2.94-.93.04-2.05.62-2.71 1.4-.6.69-1.12 1.8-.98 2.85 1.03.08 2.09-.53 2.73-1.31Z" /></svg>
  {:else if platform === "linux"}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 9V7a4 4 0 0 1 8 0v2c0 2.2 1.3 3.7 2.2 5.6.8 1.7.8 3.3-.1 4.4H5.9c-.9-1.1-.9-2.7-.1-4.4C6.7 12.7 8 11.2 8 9Z" /><path d="M8.5 19 6 20.5M15.5 19l2.5 1.5M10.5 8h.01M13.5 8h.01M10.5 10.5 12 11.4l1.5-.9" /><path d="M8.2 17c.4-2.7 1.9-4.3 3.8-4.3s3.4 1.6 3.8 4.3" /></svg>
  {:else}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="13" rx="2" /><path d="M12 17v3m-4 0h8" /></svg>
  {/if}
</span>

<style>
  .platform-icon { width: 34px; height: 34px; display: inline-grid; place-items: center; flex: 0 0 auto; border-radius: 50%; color: var(--ink); background: color-mix(in srgb, currentColor 10%, transparent); }
  .platform-icon.windows { color: var(--desktop-windows, #2782c1); }
  .platform-icon svg { width: 20px; height: 20px; }
</style>
