<script lang="ts">
  // One drawing for every state of an event, so a state change morphs instead of swapping icons: the running arc
  // keeps turning while it closes into a full ring, and only then stops, as the check draws itself inside it.
  let { status }: { status?: string } = $props();
  const CLOSING_MS = 460;
  const kind = $derived(status === "completed" || status === "failed" || status === "running" || status === "waiting" || status === "interrupted" ? status : "completed");
  let spinning = $state(false);
  $effect(() => {
    if (kind === "running") { spinning = true; return; }
    if (!spinning) return;
    const timer = setTimeout(() => (spinning = false), CLOSING_MS);
    return () => clearTimeout(timer);
  });
</script>

<svg class="mark {kind}" class:spinning={spinning || kind === "running"} viewBox="0 0 16 16" aria-hidden="true" focusable="false">
  <circle class="ring" cx="8" cy="8" r="6.9" pathLength="100" />
  <path class="check" d="m4.9 8.2 2.2 2.2 4-4.3" pathLength="100" />
  <path class="cross" d="m5.7 5.7 4.6 4.6m0-4.6-4.6 4.6" pathLength="100" />
  <circle class="dot" cx="8" cy="8" r="1.9" />
</svg>

<style>
  .mark { display: block; width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-linecap: round; stroke-linejoin: round; }
  .ring { stroke-width: 1.25; stroke-dasharray: 100 0; opacity: .34; transform-origin: 8px 8px; transition: stroke-dasharray 420ms cubic-bezier(.4, 0, .2, 1), opacity 420ms ease; }
  .running .ring { stroke-dasharray: 26 74; opacity: 1; }
  .spinning .ring { animation: ring-turn 850ms linear infinite; }
  .check, .cross { stroke-width: 1.7; stroke-dasharray: 100; stroke-dashoffset: 100; transition: stroke-dashoffset 280ms cubic-bezier(.4, 0, .2, 1); }
  .completed .check, .failed .cross { stroke-dashoffset: 0; transition-delay: 170ms; }
  .dot { fill: currentColor; stroke: none; opacity: 0; transform-origin: 8px 8px; transform: scale(.4); transition: opacity 200ms ease, transform 240ms cubic-bezier(.16, 1, .3, 1); }
  .waiting .dot, .interrupted .dot { opacity: 1; transform: scale(1); }
  .waiting .ring { opacity: .7; }
  @keyframes ring-turn { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) {
    .ring, .check, .cross, .dot { transition: none; }
    .spinning .ring { animation: none; }
  }
</style>
