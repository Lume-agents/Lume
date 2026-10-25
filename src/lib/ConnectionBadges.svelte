<script lang="ts">
  import { connections } from "$lib/connectionWatch";
  import type { Language } from "$lib/i18n";

  let { language }: { language: Language } = $props();
  const pt = $derived(language === "pt-BR");
</script>

{#if $connections.phone || $connections.computer}
  <span class="connection-badges">
    {#if $connections.phone}
      <span class="badge" role="img" title={pt ? "Celular conectado" : "Phone connected"} aria-label={pt ? "Celular conectado" : "Phone connected"}>
        <svg viewBox="0 0 20 20" aria-hidden="true"><rect x="6" y="3" width="8" height="14" rx="2" /><path d="M9 14.5h2" /></svg>
      </span>
    {/if}
    {#if $connections.computer}
      <span class="badge" role="img" title={pt ? "Outro computador conectado" : "Another computer connected"} aria-label={pt ? "Outro computador conectado" : "Another computer connected"}>
        <svg viewBox="0 0 20 20" aria-hidden="true"><rect x="3.5" y="4.5" width="13" height="8.5" rx="1.5" /><path d="M7.5 16h5M10 13v3" /></svg>
      </span>
    {/if}
  </span>
{/if}

<style>
  .connection-badges { display: inline-flex; align-items: center; gap: 4px; flex: 0 0 auto; }
  .badge { display: grid; place-items: center; width: 18px; height: 18px; border-radius: 50%; color: #fff; background: #2f9e6e; box-shadow: 0 0 0 2px color-mix(in srgb, #2f9e6e 24%, transparent); }
  .badge svg { width: 11px; height: 11px; fill: none; stroke: currentColor; stroke-width: 1.7; stroke-linecap: round; stroke-linejoin: round; }
</style>
