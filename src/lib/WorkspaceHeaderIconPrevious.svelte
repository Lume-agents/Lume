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
    <path class="folder-back" d="M3 10V6.5C3 5.7 3.7 5 4.5 5H9l2 2h8.5c.8 0 1.5.7 1.5 1.5V10" />
    <path class="folder-paper" d="M9 11h7v6H9z" />
    <path class="folder-front" d="M3 9.5h18L19 19H5z" />
  {:else if name === "layout"}
    <rect x="3" y="3" width="18" height="18" rx="2" />
    <path class="layout-vertical" d="M9 5.5v13" />
    <path class="layout-horizontal" d="M10 11h9" />
  {:else if name === "inspector"}
    <g class="detective-face">
      <path d="M7 11v3a5 5 0 0 0 10 0v-3" />
      <circle cx="9.3" cy="13" r="1.7" />
      <circle cx="14.7" cy="13" r="1.7" />
      <path d="M11 13h2M11 17c.7.4 1.3.4 2 0" />
    </g>
    <g class="detective-hat">
      <path d="M4 10.5h16M7 9.5l1-5h8l1 5M8 7h8" />
    </g>
  {:else if name === "settings"}
    <path d="M3 8h18M3 16h18" />
    <circle class="bead bead-one" cx="8" cy="8" r="2.5" />
    <circle class="bead bead-two" cx="16" cy="16" r="2.5" />
  {:else}
    <g class="lume-head">
      <g transform="scale(.046875)">
        <path fill="currentColor" fill-rule="evenodd" stroke="none" d="M96 128h32V96h256v32h32v32h32v160H256v32h128v64h-32v32H192v-32h-64v-32H96z M304 176h64v64h-64z" />
      </g>
    </g>
  {/if}
</svg>

<style>
  .header-icon { display: block; flex: 0 0 auto; overflow: visible; }
  .header-icon.layout { overflow: hidden; }
  .folder-front, .folder-paper, .layout-vertical, .layout-horizontal, .detective-face, .detective-hat, .bead, .lume-head { transition: transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .folder-paper { opacity: 0; transition: opacity 120ms ease, transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .folder-front { transform-origin: 4px 9px; }
  .detective-face { transform-origin: 12px 15px; }
  .detective-hat { transform-origin: 12px 10px; }
  .lume-head { transform-origin: 12px 12px; }
  :global(button:is(:hover, :focus-visible)) .header-icon.project .folder-front { transform: translate(1px, 2px) rotate(-5deg); }
  :global(button:is(:hover, :focus-visible)) .header-icon.project .folder-paper { opacity: 1; transform: translateY(-2px); }
  :global(button:is(:hover, :focus-visible)) .header-icon.layout .layout-vertical { transform: translateX(2px); }
  :global(button:is(:hover, :focus-visible)) .header-icon.layout .layout-horizontal { transform: translateY(3px); }
  :global(button:is(:hover, :focus-visible)) .header-icon.inspector .detective-face { transform: translateX(1px) rotate(4deg); }
  :global(button:is(:hover, :focus-visible)) .header-icon.inspector .detective-hat { transform: translateY(-2px) rotate(-5deg); }
  :global(button:is(:hover, :focus-visible)) .header-icon.settings .bead-one { transform: translateX(5px); }
  :global(button:is(:hover, :focus-visible)) .header-icon.settings .bead-two { transform: translateX(-5px); }
  :global(button:is(:hover, :focus-visible)) .header-icon.orb .lume-head { transform: rotate(-7deg) translateY(-1px); }
  @media (prefers-reduced-motion: reduce) {
    .folder-front, .folder-paper, .layout-vertical, .layout-horizontal, .detective-face, .detective-hat, .bead, .lume-head { transition: none !important; transform: none !important; }
  }
</style>
