<script lang="ts">
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import type { MobileClient } from "./client.svelte";

  let { client }: { client: MobileClient } = $props();
</script>

{#if client.banner}
  {@const banner = client.banner}
  <div class="banner {banner.kind}" role={banner.kind === "error" ? "alert" : "status"} aria-live={banner.kind === "error" ? "assertive" : "polite"} transition:fly={{ y: -14, duration: 200, easing: cubicOut }}>
    <span>{banner.message}</span>
    <button type="button" aria-label={client.tr("Dismiss", "Fechar")} onclick={() => client.hideBanner()}><LumeIcon name="close" size={14} /></button>
  </div>
{/if}

<style>
  .banner { position: absolute; z-index: 40; top: calc(var(--safe-top) + 10px); left: 12px; right: 12px; padding: 11px 10px 11px 14px; display: flex; align-items: center; gap: 10px; border: 1px solid var(--workspace-line); border-radius: 14px; color: var(--workspace-strong); background: var(--workspace-raised); box-shadow: 0 14px 34px rgba(0, 0, 0, .28); font-size: 13.5px; line-height: 1.4; }
  .banner span { flex: 1; }
  .banner.error { border-color: color-mix(in srgb, #d0645c 45%, var(--workspace-line)); }
  .banner.error span { color: #d76e66; }
  .banner.success span { color: var(--workspace-accent); }
  button { width: 30px; height: 30px; padding: 0; display: grid; place-items: center; flex: 0 0 auto; border: 0; border-radius: 9px; color: var(--workspace-muted); background: transparent; }
</style>
