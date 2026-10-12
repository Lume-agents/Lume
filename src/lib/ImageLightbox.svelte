<script lang="ts">
  import { fade } from "svelte/transition";
  import { dialogFocus } from "$lib/dialogFocus";
  import type { Language } from "$lib/i18n";
  import { portal } from "$lib/portal";

  /** The whole photo over a darkened window. `src` shows at once; `load` swaps in the full-size file. */
  let { src, alt = "", load, language = "en", onclose } = $props<{
    src: string;
    alt?: string;
    load?: () => Promise<string>;
    language?: Language;
    onclose: () => void;
  }>();
  let full = $state("");
  const shown = $derived(full || src);
  const closeLabel = $derived(language === "pt-BR" ? "Fechar imagem" : "Close image");

  $effect(() => {
    let cancelled = false;
    void load?.().then((value: string) => { if (!cancelled) full = value; }).catch(() => undefined);
    return () => { cancelled = true; };
  });
</script>

<div
  class="image-lightbox"
  role="dialog"
  aria-modal="true"
  aria-label={alt || closeLabel}
  tabindex="-1"
  use:portal
  use:dialogFocus={{ onDismiss: onclose }}
  transition:fade={{ duration: 120 }}
  onclick={onclose}
  onkeydown={(event) => { if (event.key === "Enter" || event.key === " ") onclose(); }}
>
  {#if shown}
    <img src={shown} {alt} draggable="false" />
  {/if}
  <button type="button" class="close" aria-label={closeLabel} title={closeLabel} onclick={(event) => { event.stopPropagation(); onclose(); }}>
    <svg viewBox="0 0 20 20" aria-hidden="true"><path d="m5 5 10 10M15 5 5 15" /></svg>
  </button>
</div>

<style>
  .image-lightbox { position: fixed; z-index: 100000; inset: 0; display: grid; place-items: center; padding: 28px; background: rgba(4, 7, 6, 0.86); cursor: zoom-out; outline: none; backdrop-filter: blur(3px); }
  img { max-width: min(96vw, 1800px); max-height: 92vh; border-radius: 10px; object-fit: contain; box-shadow: 0 22px 70px rgba(0, 0, 0, 0.55); user-select: none; }
  .close { position: absolute; top: 14px; right: 14px; width: 36px; height: 36px; display: grid; place-items: center; border: 1px solid rgba(255, 255, 255, 0.18); border-radius: 50%; color: #f1f5f2; background: rgba(20, 28, 24, 0.7); cursor: pointer; }
  .close:hover { background: rgba(40, 54, 47, 0.9); }
  .close:focus-visible { outline: 2px solid #8fd0ae; outline-offset: 2px; }
  .close svg { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-width: 1.7; stroke-linecap: round; }
</style>
