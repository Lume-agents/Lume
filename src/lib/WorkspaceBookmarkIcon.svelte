<script lang="ts">
  let { name, size = 21 } = $props<{
    name: "goal" | "plan" | "todo";
    size?: number;
  }>();
</script>

<svg
  xmlns="http://www.w3.org/2000/svg"
  class="bookmark-icon kind-{name}"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.9"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  focusable="false"
>
  {#if name === "goal"}
    <circle class="goal-ring goal-ring-outer" cx="11" cy="13" r="7.5" />
    <circle class="goal-ring goal-ring-inner" cx="11" cy="13" r="3.5" />
    <circle class="goal-core" cx="11" cy="13" r="1.2" />
    <path class="goal-arrow" d="m13 11 6.5-6.5m0 0H16m3.5 0V8" />
  {:else if name === "plan"}
    <rect class="plan-node plan-node-left" x="3" y="4" width="7" height="5" rx="1.5" />
    <rect class="plan-node plan-node-right" x="14" y="4" width="7" height="5" rx="1.5" />
    <rect class="plan-node plan-node-end" x="8.5" y="15" width="7" height="5" rx="1.5" />
    <path class="plan-link" d="M6.5 9v3h11V9M12 12v3" />
  {:else}
    <rect class="todo-box todo-box-done" x="3.5" y="4" width="4.5" height="4.5" rx="1.2" />
    <path class="todo-mark" d="m4.5 6.2 1 1 1.8-2" />
    <path class="todo-line todo-line-first" d="M11 6.25h9" />
    <rect class="todo-box" x="3.5" y="10" width="4.5" height="4.5" rx="1.2" />
    <path class="todo-line todo-line-middle" d="M11 12.25h7" />
    <rect class="todo-box todo-box-last" x="3.5" y="16" width="4.5" height="4.5" rx="1.2" />
    <path class="todo-line todo-line-last" d="M11 18.25h9" />
  {/if}
</svg>

<style>
  .bookmark-icon { display: block; flex: 0 0 auto; overflow: visible; }
  .goal-ring, .goal-core, .goal-arrow, .plan-node, .plan-link, .todo-box, .todo-mark, .todo-line { transform-box: fill-box; transform-origin: center; transition: transform 260ms cubic-bezier(.22, 1, .36, 1), opacity 180ms ease, stroke-dashoffset 380ms cubic-bezier(.22, 1, .36, 1); }
  .goal-core { fill: currentColor; stroke: none; }
  .goal-arrow { transform-origin: bottom left; }
  .plan-link { stroke-dasharray: 28; stroke-dashoffset: 0; }
  .todo-mark { stroke-dasharray: 5; stroke-dashoffset: 0; }
  :global(button:is(:hover, :focus-visible, .active)) .goal-arrow { transform: translate(1px, -1px); }
  :global(button:is(:hover, :focus-visible, .active)) .goal-ring-inner { transform: scale(1.08); }
  :global(button:is(:hover, :focus-visible, .active)) .goal-core { animation: goal-pulse 560ms cubic-bezier(.22, 1, .36, 1); }
  :global(button:is(:hover, :focus-visible, .active)) .plan-link { animation: plan-draw 520ms cubic-bezier(.22, 1, .36, 1); }
  :global(button:is(:hover, :focus-visible, .active)) .plan-node-left { transform: translateY(-.6px); }
  :global(button:is(:hover, :focus-visible, .active)) .plan-node-right { transform: translateY(-.6px); transition-delay: 35ms; }
  :global(button:is(:hover, :focus-visible, .active)) .plan-node-end { transform: translateY(.6px); transition-delay: 70ms; }
  :global(button:is(:hover, :focus-visible, .active)) .todo-mark { animation: todo-draw 420ms cubic-bezier(.22, 1, .36, 1); }
  :global(button:is(:hover, :focus-visible, .active)) .todo-line-first { transform: translateX(.7px); }
  :global(button:is(:hover, :focus-visible, .active)) .todo-line-middle { transform: translateX(.7px); transition-delay: 35ms; }
  :global(button:is(:hover, :focus-visible, .active)) .todo-line-last { transform: translateX(.7px); transition-delay: 70ms; }
  @keyframes goal-pulse { 50% { transform: scale(1.7); opacity: .38; } }
  @keyframes plan-draw { from { stroke-dashoffset: 28; } to { stroke-dashoffset: 0; } }
  @keyframes todo-draw { from { stroke-dashoffset: 5; opacity: .2; } to { stroke-dashoffset: 0; opacity: 1; } }
  @media (prefers-reduced-motion: reduce) { .bookmark-icon * { animation: none !important; transition: none !important; } }
</style>
