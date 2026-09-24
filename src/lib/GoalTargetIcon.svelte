<script lang="ts">
  let { size = 40 } = $props<{ size?: number }>();
</script>

<svg
  xmlns="http://www.w3.org/2000/svg"
  class="goal-target-icon"
  width={size}
  height={size}
  viewBox="0 0 48 48"
  fill="none"
  aria-hidden="true"
  focusable="false"
>
  <circle class="target-wash" cx="24" cy="25" r="18" />
  <circle class="target-ring target-ring-outer" cx="24" cy="25" r="16" />
  <circle class="target-ring target-ring-middle" cx="24" cy="25" r="10.5" />
  <circle class="target-ring target-ring-inner" cx="24" cy="25" r="5" />
  <circle class="target-center" cx="24" cy="25" r="1.8" />

  <path class="target-arrow-shaft" d="M38.5 9.5 24.8 23.2" />
  <path class="target-arrow-head" d="m24.8 23.2 1.6-6.1 4.5 4.5z" />
  <path class="target-arrow-feather" d="m35.7 9.1 4.1-.1-.1 4.1" />
</svg>

<style>
  .goal-target-icon { display: block; flex: 0 0 auto; overflow: visible; }
  .target-wash { fill: currentColor; opacity: .08; }
  .target-ring { stroke: currentColor; stroke-linecap: round; }
  .target-ring-outer { stroke-width: 1.8; opacity: .48; }
  .target-ring-middle { stroke-width: 2.1; opacity: .72; }
  .target-ring-inner { fill: color-mix(in srgb, currentColor 12%, transparent); stroke-width: 2.2; }
  .target-center { fill: currentColor; }
  .target-arrow-shaft { stroke: currentColor; stroke-width: 2.2; stroke-linecap: round; }
  .target-arrow-head { fill: currentColor; stroke: currentColor; stroke-width: 1; stroke-linejoin: round; }
  .target-arrow-feather { stroke: currentColor; stroke-width: 2; stroke-linecap: round; stroke-linejoin: round; }
  :global(.goal-detail:hover) .target-ring-inner { transform: scale(1.12); }
  .target-ring-inner { transform-box: fill-box; transform-origin: center; transition: transform 220ms cubic-bezier(.22, 1, .36, 1); }
  @media (prefers-reduced-motion: reduce) { .target-ring-inner { transition: none; } :global(.goal-detail:hover) .target-ring-inner { transform: none; } }
</style>
