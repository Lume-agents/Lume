<script lang="ts">
  import { onMount } from "svelte";

  let { text, animate = false, live = false, startFromBeginning = false, render }: {
    text: string;
    animate?: boolean;
    live?: boolean;
    startFromBeginning?: boolean;
    render: (value: string) => string;
  } = $props();

  let visible = $state("");
  let streaming = $state(false);
  let started = $state(false);
  let frame = 0;
  let lastFrame = 0;
  const maxAnimatedLength = 48_000;

  function advance(now: number) {
    if (!streaming) return;
    if (lastFrame && now - lastFrame < 72) {
      frame = requestAnimationFrame(advance);
      return;
    }
    const elapsed = Math.min(100, now - (lastFrame || now - 34));
    lastFrame = now;
    const speed = Math.max(260, text.length / 2);
    const next = Math.min(text.length, visible.length + Math.max(1, Math.ceil(speed * elapsed / 1000)));
    visible = text.slice(0, next);
    if (next < text.length) frame = requestAnimationFrame(advance);
    else { streaming = false; lastFrame = 0; }
  }

  $effect(() => {
    const current = text;
    const enabled = animate && current.length <= maxAnimatedLength;
    if (!started || !enabled) {
      visible = current;
      streaming = false;
      return;
    }
    if (!current.startsWith(visible)) {
      visible = current;
      streaming = false;
      return;
    }
    if (visible.length < current.length && !streaming) {
      streaming = true;
      frame = requestAnimationFrame(advance);
    }
  });

  onMount(() => {
    if (animate && text.length <= maxAnimatedLength && !window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      started = true;
      visible = startFromBeginning ? "" : text;
      if (visible.length < text.length) {
        streaming = true;
        frame = requestAnimationFrame(advance);
      }
    }
    return () => cancelAnimationFrame(frame);
  });
</script>

{#if animate && text.length <= maxAnimatedLength && (streaming || (started && live))}
  <div class="streaming-markdown" aria-hidden="true">{@html render(visible)}</div>
  <span class="sr-only">{text}</span>
{:else}
  {@html render(text)}
{/if}

<style>
  .streaming-markdown { min-width: 0; overflow-wrap: anywhere; word-break: break-word; }
  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; border: 0; }
</style>
