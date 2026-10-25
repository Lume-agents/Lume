<script lang="ts">
  import { onMount } from "svelte";
  import { fade } from "svelte/transition";
  import type { MobileClient } from "./client.svelte";

  let { client }: { client: MobileClient } = $props();
  let visible = $state(typeof matchMedia === "function" && matchMedia("(orientation: portrait)").matches);

  // A quick reminder on entering the board, gone after a few seconds or as soon as the phone lies down.
  onMount(() => {
    if (!visible) return;
    const timer = setTimeout(() => (visible = false), 4200);
    const query = matchMedia("(orientation: portrait)");
    const turned = () => { if (!query.matches) visible = false; };
    query.addEventListener("change", turned);
    return () => { clearTimeout(timer); query.removeEventListener("change", turned); };
  });
</script>

{#if visible}
  <div class="rotate-hint" role="status" transition:fade={{ duration: 220 }}>
    <svg viewBox="0 0 24 24" aria-hidden="true"><rect class="body" x="7" y="3" width="10" height="18" rx="2.4" /><path class="speaker" d="M11 5.6h2" /></svg>
    <span>{client.tr("Turn your phone sideways for a better view", "Deite o celular para ver melhor")}</span>
  </div>
{/if}

<style>
  .rotate-hint { position: absolute; z-index: 12; left: max(10px, env(safe-area-inset-left, 0px)); top: 50%; transform: translateY(-50%); width: 76px; padding: 12px 8px; display: grid; justify-items: center; gap: 8px; border: 1px solid var(--workspace-line); border-radius: 16px; color: var(--workspace-strong, var(--workspace-text)); background: var(--hud-surface, var(--workspace-pane)); box-shadow: 0 10px 28px rgba(0, 0, 0, .28); text-align: center; pointer-events: none; }
  svg { width: 34px; height: 34px; fill: none; stroke: var(--workspace-accent); stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; transform-origin: center; animation: turn 1.9s cubic-bezier(.5, 0, .2, 1) infinite; }
  span { font-size: 10px; line-height: 1.25; font-weight: 600; color: var(--workspace-muted); }
  @keyframes turn { 0%, 12% { transform: rotate(0deg); } 48%, 78% { transform: rotate(-90deg); } 100% { transform: rotate(-90deg); opacity: .15; } }
  @media (prefers-reduced-motion: reduce) { svg { animation: none; transform: rotate(-90deg); } }
</style>
