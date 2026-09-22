<script module lang="ts">
  export type WorkspaceChatIconName =
    | "prompts"
    | "text-size"
    | "maximize"
    | "restore"
    | "close"
    | "fast"
    | "mode-default"
    | "mode-plan";
</script>

<script lang="ts">
  let {
    name,
    size = 18,
    active = false,
  } = $props<{
    name: WorkspaceChatIconName;
    size?: number;
    active?: boolean;
  }>();
</script>

<svg
  xmlns="http://www.w3.org/2000/svg"
  viewBox="0 0 24 24"
  width={size}
  height={size}
  fill="none"
  stroke="currentColor"
  stroke-width="1.9"
  stroke-linecap="round"
  stroke-linejoin="round"
  class:active
  class="workspace-chat-icon icon-{name}"
  aria-hidden="true"
  focusable="false"
>
  {#if name === "prompts"}
    <path class="prompt-spine" d="M6 5v14" />
    <circle class="prompt-node node-a" cx="6" cy="7" r="1.5" />
    <circle class="prompt-node node-b" cx="6" cy="12" r="1.5" />
    <circle class="prompt-node node-c" cx="6" cy="17" r="1.5" />
    <path class="prompt-line line-a" d="M10 7h9" />
    <path class="prompt-line line-b" d="M10 12h7" />
    <path class="prompt-line line-c" d="M10 17h5" />
  {:else if name === "text-size"}
    <path class="letter-large" d="m3.5 18 4.2-12h2.6l4.2 12M5.4 13h7.2" />
    <path class="letter-small" d="m14.5 18 2.2-7h1.7l2.1 7M15.6 15h3.9" />
  {:else if name === "maximize"}
    <path class="corner corner-nw" d="M9 4H4v5" />
    <path class="corner corner-ne" d="M15 4h5v5" />
    <path class="corner corner-sw" d="M9 20H4v-5" />
    <path class="corner corner-se" d="M15 20h5v-5" />
  {:else if name === "restore"}
    <path class="corner corner-nw" d="M4 9h5V4" />
    <path class="corner corner-ne" d="M20 9h-5V4" />
    <path class="corner corner-sw" d="M4 15h5v5" />
    <path class="corner corner-se" d="M20 15h-5v5" />
  {:else if name === "close"}
    <path class="close-stroke close-a" d="m6.5 6.5 11 11" />
    <path class="close-stroke close-b" d="m17.5 6.5-11 11" />
  {:else if name === "fast"}
    <path class="fast-trail trail-a" d="M3.5 8H8" />
    <path class="fast-trail trail-b" d="M2.5 12H7" />
    <path class="fast-trail trail-c" d="M4 16h3" />
    <path class="fast-bolt" d="m14 3-6 10h5l-1 8 7-11h-5z" fill={active ? "currentColor" : "none"} />
  {:else if name === "mode-plan"}
    <path class="plan-spine" d="M6 6v12" />
    <circle class="plan-node node-a" cx="6" cy="6" r="1.7" fill={active ? "currentColor" : "none"} />
    <circle class="plan-node node-b" cx="6" cy="12" r="1.7" fill={active ? "currentColor" : "none"} />
    <circle class="plan-node node-c" cx="6" cy="18" r="1.7" fill={active ? "currentColor" : "none"} />
    <path class="plan-line line-a" d="M10 6h9" />
    <path class="plan-line line-b" d="M10 12h7" />
    <path class="plan-line line-c" d="M10 18h9" />
  {:else}
    <path class="default-line line-a" d="M5 7h14" />
    <path class="default-line line-b" d="M5 12h10" />
    <path class="default-line line-c" d="M5 17h7" />
    <circle class="default-pulse" cx="18.5" cy="17" r="1.5" fill={active ? "currentColor" : "none"} />
  {/if}
</svg>

<style>
  .workspace-chat-icon { display: block; flex: 0 0 auto; overflow: visible; }
  .prompt-node, .prompt-line, .letter-large, .letter-small, .corner, .close-stroke, .fast-trail, .fast-bolt, .plan-node, .plan-line, .default-line, .default-pulse { transform-box: fill-box; transform-origin: center; transition: transform 220ms cubic-bezier(.16, 1, .3, 1), opacity 150ms ease, fill 150ms ease; }
  .active { color: var(--workspace-accent, currentColor); }

  :global(button:is(:hover, :focus-visible)) .icon-prompts .node-a { transform: translateX(1px) scale(1.08); }
  :global(button:is(:hover, :focus-visible)) .icon-prompts .node-b { transform: translateX(1px) scale(1.08); transition-delay: 30ms; }
  :global(button:is(:hover, :focus-visible)) .icon-prompts .node-c { transform: translateX(1px) scale(1.08); transition-delay: 60ms; }
  :global(button:is(:hover, :focus-visible)) .icon-prompts .line-a { transform: translateX(1px); }
  :global(button:is(:hover, :focus-visible)) .icon-prompts .line-b { transform: translateX(2px); }
  :global(button:is(:hover, :focus-visible)) .icon-prompts .line-c { transform: translateX(3px); }

  :global(button:is(:hover, :focus-visible)) .icon-text-size .letter-large { transform: translateY(-1px); }
  :global(button:is(:hover, :focus-visible)) .icon-text-size .letter-small { transform: translate(1px, -2px) scale(1.08); }

  :global(button:is(:hover, :focus-visible)) .icon-maximize .corner-nw { transform: translate(-1px, -1px); }
  :global(button:is(:hover, :focus-visible)) .icon-maximize .corner-ne { transform: translate(1px, -1px); }
  :global(button:is(:hover, :focus-visible)) .icon-maximize .corner-sw { transform: translate(-1px, 1px); }
  :global(button:is(:hover, :focus-visible)) .icon-maximize .corner-se { transform: translate(1px, 1px); }
  :global(button:is(:hover, :focus-visible)) .icon-restore .corner-nw { transform: translate(1px, 1px); }
  :global(button:is(:hover, :focus-visible)) .icon-restore .corner-ne { transform: translate(-1px, 1px); }
  :global(button:is(:hover, :focus-visible)) .icon-restore .corner-sw { transform: translate(1px, -1px); }
  :global(button:is(:hover, :focus-visible)) .icon-restore .corner-se { transform: translate(-1px, -1px); }

  :global(button:is(:hover, :focus-visible)) .icon-close .close-a { transform: rotate(7deg) scale(.92); }
  :global(button:is(:hover, :focus-visible)) .icon-close .close-b { transform: rotate(-7deg) scale(.92); }

  :global(button:is(:hover, :focus-visible)) .icon-fast .fast-bolt { transform: translate(1px, -1px) skewX(-4deg); }
  :global(button:is(:hover, :focus-visible)) .icon-fast .trail-a { transform: translateX(-2px); opacity: .74; }
  :global(button:is(:hover, :focus-visible)) .icon-fast .trail-b { transform: translateX(-3px); opacity: .5; }
  :global(button:is(:hover, :focus-visible)) .icon-fast .trail-c { transform: translateX(-1px); opacity: .3; }

  :global(button:is(:hover, :focus-visible)) .icon-mode-plan .node-a { transform: scale(1.14); }
  :global(button:is(:hover, :focus-visible)) .icon-mode-plan .node-b { transform: scale(1.14); transition-delay: 35ms; }
  :global(button:is(:hover, :focus-visible)) .icon-mode-plan .node-c { transform: scale(1.14); transition-delay: 70ms; }
  :global(button:is(:hover, :focus-visible)) .icon-mode-plan .line-a { transform: translateX(1px); }
  :global(button:is(:hover, :focus-visible)) .icon-mode-plan .line-b { transform: translateX(2px); }
  :global(button:is(:hover, :focus-visible)) .icon-mode-plan .line-c { transform: translateX(1px); }
  :global(button:is(:hover, :focus-visible)) .icon-mode-default .line-a { transform: translateX(1px); }
  :global(button:is(:hover, :focus-visible)) .icon-mode-default .line-b { transform: translateX(2px); }
  :global(button:is(:hover, :focus-visible)) .icon-mode-default .line-c { transform: translateX(3px); }
  :global(button:is(:hover, :focus-visible)) .icon-mode-default .default-pulse { transform: scale(1.24); }

  @media (prefers-reduced-motion: reduce) {
    .prompt-node, .prompt-line, .letter-large, .letter-small, .corner, .close-stroke, .fast-trail, .fast-bolt, .plan-node, .plan-line, .default-line, .default-pulse { transition: none; }
  }
</style>
