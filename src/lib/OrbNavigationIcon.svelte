<script lang="ts">
  let { name, size = 22, activation = 0 }: {
    name: "sessions" | "terminals" | "inspector" | "settings";
    size?: number;
    activation?: number;
  } = $props();
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
  class="orb-navigation-icon"
  aria-hidden="true"
  focusable="false"
>
  {#key activation}
    <g class:activated={activation > 0}>
      {#if name === "sessions"}
        <g class="monitor">
          <rect x="3" y="4" width="18" height="12" rx="2.5" />
          <path class="monitor-prompt" d="m6.5 7.5 2.5 2-2.5 2M12 11.5h4.5" />
        </g>
        <path d="M12 16v4M8.5 20h7" />
      {:else if name === "terminals"}
        <path class="connections" d="M8.5 6h7M7.4 8.1l3.2 7.6M16.6 8.1l-3.2 7.6" />
        <circle class="node node-one" cx="6" cy="6" r="2.5" />
        <circle class="node node-two" cx="18" cy="6" r="2.5" />
        <circle class="node node-three" cx="12" cy="18" r="2.5" />
      {:else if name === "inspector"}
        <g class="detective-face">
          <path d="M6.5 11.5v2.5a5.5 5.5 0 0 0 11 0v-2.5" />
          <circle cx="9.5" cy="13.5" r="1.7" />
          <circle cx="14.5" cy="13.5" r="1.7" />
          <path d="M11.2 13.5h1.6" />
        </g>
        <g class="detective-hat">
          <path d="M4.5 10.5h15M7.4 10l1-5h7.2l1 5M8.1 7.5h7.8" />
        </g>
      {:else}
        <g class="cog">
          <path d="M10.47 5.37 10.42 3.04h3.16l-.05 2.33A6.8 6.8 0 0 1 15.6 6.23l1.62-1.68 2.23 2.23-1.68 1.62A6.8 6.8 0 0 1 18.63 10.47l2.33-.05v3.16l-2.33-.05A6.8 6.8 0 0 1 17.77 15.6l1.68 1.62-2.23 2.23-1.62-1.68A6.8 6.8 0 0 1 13.53 18.63l.05 2.33h-3.16l.05-2.33A6.8 6.8 0 0 1 8.4 17.77l-1.62 1.68-2.23-2.23 1.68-1.62A6.8 6.8 0 0 1 5.37 13.53l-2.33.05v-3.16l2.33.05A6.8 6.8 0 0 1 6.23 8.4L4.55 6.78l2.23-2.23L8.4 6.23A6.8 6.8 0 0 1 10.47 5.37Z" />
          <circle cx="12" cy="12" r="3" />
        </g>
      {/if}
    </g>
  {/key}
</svg>

<style>
  .orb-navigation-icon { display: block; flex: 0 0 auto; }
  .monitor, .node, .detective-face, .detective-hat { transform-box: fill-box; transform-origin: center; }
  .cog { transform-box: view-box; transform-origin: 12px 12px; }
  .activated .monitor { animation: monitor-confirm 340ms cubic-bezier(.16, 1, .3, 1); }
  .activated .monitor-prompt { animation: prompt-reveal 340ms ease-out; }
  .activated .connections { animation: connections-confirm 350ms ease-out; }
  .activated .node { animation: node-confirm 300ms cubic-bezier(.16, 1, .3, 1) both; }
  .activated .node-two { animation-delay: 45ms; }
  .activated .node-three { animation-delay: 90ms; }
  .activated .detective-face { animation: detective-look 360ms cubic-bezier(.16, 1, .3, 1); }
  .activated .detective-hat { animation: detective-greet 360ms cubic-bezier(.16, 1, .3, 1); }
  .activated .cog { animation: cog-turn 390ms cubic-bezier(.16, 1, .3, 1); }
  @keyframes monitor-confirm { 40% { transform: translateY(-1px); } }
  @keyframes prompt-reveal { 0% { opacity: .25; } 100% { opacity: 1; } }
  @keyframes connections-confirm { 0% { opacity: .25; } 100% { opacity: 1; } }
  @keyframes node-confirm { 0% { transform: scale(.78); } 100% { transform: scale(1); } }
  @keyframes detective-look { 40% { transform: translateX(.7px) rotate(3deg); } }
  @keyframes detective-greet { 40% { transform: translateY(-1.5px) rotate(-4deg); } }
  @keyframes cog-turn { from { transform: rotate(-45deg); } to { transform: rotate(0); } }
  @media (prefers-reduced-motion: reduce) {
    .activated :is(.monitor, .monitor-prompt, .connections, .node, .detective-face, .detective-hat, .cog) { animation: none; }
  }
</style>
