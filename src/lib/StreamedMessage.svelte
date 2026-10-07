<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { STREAM_STEP_MS, stepsForBacklog, wordsForStep } from "$lib/streamPacing";

  let { text, animate = false, live = false, startFromBeginning = false, onstreaming, render }: {
    text: string;
    animate?: boolean;
    live?: boolean;
    startFromBeginning?: boolean;
    /** Tells the chat whether the message is still being written, so it can wait for it. */
    onstreaming?: (active: boolean) => void;
    render: (value: string) => string;
  } = $props();

  let visible = $state("");
  let streaming = $state(false);
  let fading = $state(false);
  let started = $state(false);
  let streamElement = $state<HTMLDivElement | null>(null);
  let frame = 0;
  let lastFrame = 0;
  let finishTimer = 0;
  let lastWrappedText = "";
  let revealedWords = 0;
  // The reveal is planned to finish in a number of steps that grows with the text.
  let plannedLength = 0;
  let stepsLeft = 0;
  const maxAnimatedLength = 6_000;

  function fadeNewestWords(root: HTMLElement, count: number) {
    if (!count || window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    const nodes: Text[] = [];
    while (walker.nextNode()) nodes.push(walker.currentNode as Text);
    for (const node of nodes.reverse()) {
      const words = [...node.data.matchAll(/\S+/gu)];
      for (const word of words.reverse()) {
        if (count <= 0) return;
        const range = document.createRange();
        range.setStart(node, word.index);
        range.setEnd(node, word.index + word[0].length);
        const span = document.createElement("span");
        span.className = "stream-word";
        range.surroundContents(span);
        count -= 1;
      }
    }
  }

  function advance(now: number) {
    if (!streaming) return;
    if (lastFrame && now - lastFrame < STREAM_STEP_MS) {
      frame = requestAnimationFrame(advance);
      return;
    }
    lastFrame = now;
    const backlog = text.length - visible.length;
    // Plan again whenever the text has grown past what the plan covered.
    if (text.length !== plannedLength) {
      plannedLength = text.length;
      stepsLeft = stepsForBacklog(backlog);
    }
    const wordsLeft = text.slice(visible.length).match(/\S+/gu)?.length ?? 1;
    const batch = wordsForStep(wordsLeft, stepsLeft);
    stepsLeft = Math.max(1, stepsLeft - 1);
    let next = visible.length;
    revealedWords = 0;
    for (let index = 0; index < batch && next < text.length; index += 1) {
      const segment = text.slice(next).match(/^\s*\S+\s*/u);
      next += segment?.[0].length ?? text.length - next;
      revealedWords += 1;
    }
    visible = text.slice(0, next);
    if (next < text.length) {
      frame = requestAnimationFrame(advance);
    } else {
      streaming = false;
      fading = true;
      lastFrame = 0;
      plannedLength = 0;
      finishTimer = window.setTimeout(() => (fading = false), 240);
    }
  }

  $effect(() => {
    const current = text;
    const enabled = animate && current.length <= maxAnimatedLength && document.visibilityState === "visible"
      && !window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (!started && enabled) return;
    if (!enabled) {
      cancelAnimationFrame(frame);
      window.clearTimeout(finishTimer);
      visible = current;
      streaming = false;
      fading = false;
      return;
    }
    if (!current.startsWith(visible)) {
      cancelAnimationFrame(frame);
      window.clearTimeout(finishTimer);
      visible = current;
      streaming = false;
      fading = false;
      return;
    }
    if (visible.length < current.length && !streaming) {
      window.clearTimeout(finishTimer);
      fading = false;
      streaming = true;
      frame = requestAnimationFrame(advance);
    }
  });

  // The chat waits for a message to finish writing before showing what follows it.
  $effect(() => {
    onstreaming?.(streaming);
  });
  onDestroy(() => onstreaming?.(false));

  $effect(() => {
    const current = visible;
    if (!current || !(streaming || fading || live)) return;
    void tick().then(() => {
      if (current === visible && current !== lastWrappedText && streamElement) {
        fadeNewestWords(streamElement, revealedWords);
        lastWrappedText = current;
      }
    });
  });

  onMount(() => {
    const settleWhenHidden = () => {
      if (document.visibilityState !== "hidden") return;
      cancelAnimationFrame(frame);
      visible = text;
      streaming = false;
      fading = false;
      lastFrame = 0;
    };
    document.addEventListener("visibilitychange", settleWhenHidden);
    if (animate && text.length <= maxAnimatedLength && document.visibilityState === "visible" && !window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      started = true;
      visible = startFromBeginning ? "" : text;
      if (visible.length < text.length) {
        streaming = true;
        frame = requestAnimationFrame(advance);
      }
    }
    return () => {
      document.removeEventListener("visibilitychange", settleWhenHidden);
      cancelAnimationFrame(frame);
      window.clearTimeout(finishTimer);
    };
  });
</script>

{#if animate && text.length <= maxAnimatedLength && (started || startFromBeginning || live)}
  <div bind:this={streamElement} class="streaming-markdown" aria-hidden={streaming || fading || live}>{@html render(!started && !startFromBeginning ? text : visible)}</div>
  {#if streaming || fading || live}<span class="sr-only">{text}</span>{/if}
{:else}
  {@html render(text)}
{/if}

<style>
  .streaming-markdown { min-width: 0; overflow-wrap: anywhere; word-break: break-word; }
  .streaming-markdown :global(.stream-word) { animation: stream-word-fade 220ms cubic-bezier(.16, 1, .3, 1) both; }
  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; border: 0; }
  @keyframes stream-word-fade { from { opacity: .08; } to { opacity: 1; } }
  @media (prefers-reduced-motion: reduce) { .streaming-markdown :global(.stream-word) { animation: none; } }
</style>
