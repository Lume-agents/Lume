<script lang="ts">
  import { fade, scale } from "svelte/transition";

  // A pairing QR code that opens larger on a click, so a phone can read it from farther away.
  let { src, alt, label = "" }: { src: string; alt: string; label?: string } = $props();
  let open = $state(false);

  function keydown(event: KeyboardEvent) {
    if (open && event.key === "Escape") { event.preventDefault(); open = false; }
  }
</script>

<svelte:window onkeydown={keydown} />

<button class="qr" type="button" title={alt} aria-label={alt} onclick={() => (open = true)}><img {src} {alt} /></button>
{#if open}
  <div class="qr-zoom" role="dialog" aria-modal="true" aria-label={alt} tabindex="-1" transition:fade={{ duration: 120 }} onclick={() => (open = false)} onkeydown={keydown}>
    <figure transition:scale={{ duration: 160, start: 0.9 }}>
      <img {src} {alt} />
      {#if label}<figcaption>{label}</figcaption>{/if}
    </figure>
  </div>
{/if}

<style>
  .qr { padding: 0; display: block; border: 0; border-radius: 8px; background: transparent; cursor: zoom-in; line-height: 0; }
  .qr img { width: 100%; height: 100%; border-radius: inherit; }
  .qr:focus-visible { outline: 2px solid var(--lume-accent-strong, #397b5c); outline-offset: 2px; }
  .qr-zoom { position: fixed; z-index: 1000; inset: 0; display: grid; place-items: center; background: rgba(4, 9, 6, .72); backdrop-filter: blur(3px); cursor: zoom-out; }
  figure { margin: 0; display: grid; justify-items: center; gap: 10px; }
  figure img { width: min(86vw, 86vh, 420px); height: min(86vw, 86vh, 420px); padding: 14px; box-sizing: border-box; border-radius: 16px; background: #fff; image-rendering: pixelated; }
  figcaption { color: #fff; font: 600 15px/1.3 var(--lume-font-ui, system-ui, sans-serif); letter-spacing: .12em; }
</style>
