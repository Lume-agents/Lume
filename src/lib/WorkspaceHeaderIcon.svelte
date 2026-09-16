<script lang="ts">
  let { name, size = 20 }: {
    name: "project" | "layout" | "inspector" | "settings" | "orb";
    size?: number;
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
  class="header-icon"
  class:project={name === "project"}
  class:layout={name === "layout"}
  class:inspector={name === "inspector"}
  class:settings={name === "settings"}
  class:orb={name === "orb"}
  aria-hidden="true"
  focusable="false"
>
  {#if name === "project"}
    <path d="M3.5 10V7a2 2 0 0 1 2-2H9l2 2h7.5a2 2 0 0 1 2 2v1" />
    <path class="folder-paper" d="M8.5 11h7v5.5h-7z" />
    <path class="folder-front" d="M4 9.5h16l-1.8 9H5.8z" />
  {:else if name === "layout"}
    <rect x="3.5" y="4.5" width="17" height="15" rx="2.2" />
    <path class="layout-vertical" d="M9.5 6.5v11" />
    <path class="layout-horizontal" d="M11.5 11.5h7" />
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
  {:else if name === "settings"}
    <path d="M3.5 8h17M3.5 16h17" />
    <circle class="bead bead-one" cx="8.5" cy="8" r="2.35" />
    <circle class="bead bead-two" cx="15.5" cy="16" r="2.35" />
  {:else}
    <path class="lume-head" fill="currentColor" fill-rule="evenodd" stroke="none" d="M4.5 6H6V4.5h12V6h1.5v1.5H21V15h-9v1.5h6v3h-1.5V21H9v-1.5H6V18H4.5z M14.25 8.25h3v3h-3z" />
  {/if}
</svg>

<style>
  .header-icon { display: block; flex: 0 0 auto; overflow: visible; }
  .header-icon.layout { overflow: hidden; }
  .folder-front, .folder-paper, .layout-vertical, .layout-horizontal, .detective-face, .detective-hat, .bead, .lume-head { transition: transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .folder-paper { opacity: 0; transition: opacity 120ms ease, transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .folder-front { transform-origin: 4px 9.5px; }
  .bead { fill: var(--workspace-sidebar, #17231d); transition: transform 160ms cubic-bezier(.16, 1, .3, 1), fill 120ms ease; }
  .detective-face { transform-origin: 12px 15px; }
  .detective-hat { transform-origin: 12px 10px; }
  .lume-head { transform-origin: 12px 12px; }
  :global(button:is(:hover, :focus-visible)) .header-icon.project .folder-front { transform: translate(1px, 2px) rotate(-6deg); }
  :global(button:is(:hover, :focus-visible)) .header-icon.project .folder-paper { opacity: 1; transform: translateY(-2px); }
  :global(button:is(:hover, :focus-visible)) .header-icon.layout .layout-vertical { transform: translateX(2px); }
  :global(button:is(:hover, :focus-visible)) .header-icon.layout .layout-horizontal { transform: translateY(-2px); }
  :global(button:is(:hover, :focus-visible)) .header-icon.inspector .detective-face { transform: translateX(.5px) rotate(3deg); }
  :global(button:is(:hover, :focus-visible)) .header-icon.inspector .detective-hat { transform: translateY(-2px) rotate(-4deg); }
  :global(button:is(:hover, :focus-visible)) .header-icon.settings .bead-one { transform: translateX(4px); }
  :global(button:is(:hover, :focus-visible)) .header-icon.settings .bead-two { transform: translateX(-4px); }
  :global(button:is(:hover, :focus-visible, .active)) .header-icon.settings .bead { fill: var(--workspace-subtle, #26392e); }
  :global(button:is(:hover, :focus-visible)) .header-icon.orb .lume-head { transform: rotate(-7deg) translateY(-1px); }
  @media (prefers-reduced-motion: reduce) {
    .folder-front, .folder-paper, .layout-vertical, .layout-horizontal, .detective-face, .detective-hat, .bead, .lume-head { transition: none !important; transform: none !important; }
  }
</style>
