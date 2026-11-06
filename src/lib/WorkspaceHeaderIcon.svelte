<script lang="ts">
  let { name, size = 20 }: {
    name: "project" | "layout" | "extensions" | "workflow" | "inspector" | "settings" | "orb";
    size?: number;
  } = $props();
  const uid = $props.id();
  const puzzleCut = `puzzle-cut-${uid}`;
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
  class:extensions={name === "extensions"}
  class:workflow={name === "workflow"}
  class:inspector={name === "inspector"}
  class:settings={name === "settings"}
  class:orb={name === "orb"}
  aria-hidden="true"
  focusable="false"
>
  <!-- Filled shapes in two tones: the icon's colour and a lighter one (--header-icon-dim). -->
  {#if name === "project"}
    <g fill="currentColor" stroke="none">
      <path class="dim" d="M3.5 15V7a2 2 0 0 1 2-2H9l2 2h7.5a2 2 0 0 1 2 2v6z" />
      <path class="folder-paper dim" d="M7.5 7.8h9v5.4h-9z" />
      <path class="folder-front" d="M3.5 10.6a1.3 1.3 0 0 1 1.3-1.3h14.4a1.3 1.3 0 0 1 1.3 1.3v6.9a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z" />
    </g>
  {:else if name === "layout"}
    <rect x="3.5" y="4.5" width="17" height="15" rx="2.2" />
    <path class="layout-vertical" d="M9.5 6.5v11" />
    <path class="layout-horizontal" d="M11.5 11.5h7" />
  {:else if name === "extensions"}
    <!-- Four corner pieces: the top left one has two tabs and comes loose on hover; the bottom left one has only sockets. -->
    <mask id={puzzleCut} maskUnits="userSpaceOnUse" x="-4" y="-4" width="32" height="32">
      <rect x="-4" y="-4" width="32" height="32" fill="#fff" />
      <path d="M12 2L12 4L12 6.65A1.6 1.6 0 1 1 12 7.95L12 12L12 16.05A1.6 1.6 0 1 0 12 17.35L12 20L12 22M2 12L4 12L6.65 12A1.6 1.6 0 1 0 7.95 12L12 12L16.05 12A1.6 1.6 0 1 0 17.35 12L20 12L22 12" stroke="#000" stroke-width=".9" />
    </mask>
    <g fill="currentColor" stroke="none">
      <g mask="url(#{puzzleCut})">
        <path class="dim" d="M12 4L18 4A2 2 0 0 1 20 6L20 12L17.35 12A1.6 1.6 0 1 1 16.05 12L12 12L12 7.95A1.6 1.6 0 1 0 12 6.65L12 4Z" />
        <path class="dim" d="M4 12L6.65 12A1.6 1.6 0 1 0 7.95 12L12 12L12 16.05A1.6 1.6 0 1 0 12 17.35L12 20L6 20A2 2 0 0 1 4 18Z" />
        <path d="M12 12L16.05 12A1.6 1.6 0 1 0 17.35 12L20 12L20 18A2 2 0 0 1 18 20L12 20L12 17.35A1.6 1.6 0 1 1 12 16.05L12 12Z" />
      </g>
      <g class="puzzle-corner"><path mask="url(#{puzzleCut})" d="M6 4L12 4L12 6.65A1.6 1.6 0 1 1 12 7.95L12 12L7.95 12A1.6 1.6 0 1 1 6.65 12L4 12L4 6A2 2 0 0 1 6 4Z" /></g>
    </g>
  {:else if name === "workflow"}
    <path class="dim-stroke" stroke-width="2.6" d="M9 7h3a3 3 0 0 1 3 3v4M9 17h5" />
    <!-- A pulse of energy that runs once along each cable, towards the block on the right. -->
    <path class="board-pulse" stroke-width="2.6" pathLength="1" d="M9 7h3a3 3 0 0 1 3 3v4" />
    <path class="board-pulse" stroke-width="2.6" pathLength="1" d="M9 17h5" />
    <g fill="currentColor" stroke="none">
      <rect class="board-node-one" x="3" y="4" width="6" height="6" rx="1.5" />
      <rect class="board-node-two dim" x="3" y="14" width="6" height="6" rx="1.5" />
      <rect class="board-node-three" x="15" y="14" width="6" height="6" rx="1.5" />
    </g>
  {:else if name === "inspector"}
    <g fill="currentColor" stroke="none" transform="translate(12 12.2) scale(1.12) translate(-12 -12.2)">
      <g class="detective-face">
        <path class="dim" d="M6.6 12h10.8v2a5.4 5.4 0 0 1-10.8 0z" />
        <circle cx="9.4" cy="13.7" r="1.75" />
        <circle cx="14.6" cy="13.7" r="1.75" />
        <rect x="10.8" y="13.2" width="2.4" height="1" rx=".5" />
      </g>
      <g class="detective-hat">
        <path d="M8.4 5H15.6L15.98 6.9H8.02Z" />
        <path class="dim" d="M7.92 7.4H16.08L16.32 8.6H7.68Z" />
        <path d="M7.58 9.1H16.42L16.6 10H7.4Z" />
        <rect x="4.5" y="9.5" width="15" height="1.8" rx=".9" />
      </g>
    </g>
  {:else if name === "settings"}
    <g class="gear" fill="currentColor" stroke="none">
      <g transform="translate(12 12) scale(.92) translate(-12 -12)">
        <path fill-rule="evenodd" d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2zM12 8.1a3.9 3.9 0 1 0 0 7.8a3.9 3.9 0 1 0 0-7.8z" />
        <circle class="dim" cx="12" cy="12" r="2.3" />
      </g>
    </g>
  {:else}
    <path class="lume-head" fill="currentColor" fill-rule="evenodd" stroke="none" d="M4.5 6H6V4.5h12V6h1.5v1.5H21V15h-9v1.5h6v3h-1.5V21H9v-1.5H6V18H4.5z M14.25 8.25h3v3h-3z" />
  {/if}
</svg>

<style>
  .header-icon { display: block; flex: 0 0 auto; overflow: visible; }
  .dim { fill-opacity: var(--header-icon-dim, .58); }
  .dim-stroke { stroke-opacity: var(--header-icon-dim, .58); }
  .header-icon.layout { overflow: hidden; }
  .board-pulse { stroke-dasharray: .32 1.4; stroke-dashoffset: .4; }
  .board-node-one, .board-node-two, .board-node-three { transform-box: fill-box; transition: transform 180ms cubic-bezier(.16, 1, .3, 1); }
  :global(button:is(:hover, :focus-visible)) .header-icon.workflow .board-node-one { transform: translateY(-1px); }
  :global(button:is(:hover, :focus-visible)) .header-icon.workflow .board-node-three { transform: translateX(1px); }
  :global(button:is(:hover, :focus-visible)) .header-icon.workflow .board-pulse { animation: board-pulse 620ms cubic-bezier(.45, 0, .3, 1) 1; }
  @keyframes board-pulse { from { stroke-dashoffset: .4; } to { stroke-dashoffset: -1.1; } }
  .folder-front, .folder-paper, .layout-vertical, .layout-horizontal, .detective-face, .detective-hat, .lume-head { transform-box: fill-box; transition: transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .folder-paper { opacity: 0; transition: opacity 120ms ease, transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .folder-front { transform-origin: 4px 9.5px; }
  .detective-face { transform-origin: 12px 15px; }
  .detective-hat { transform-origin: 12px 10px; }
  .lume-head { transform-origin: 12px 12px; }
  :global(button:is(:hover, :focus-visible)) .header-icon.project .folder-front { transform: translate(1px, 2px) rotate(-6deg); }
  :global(button:is(:hover, :focus-visible)) .header-icon.project .folder-paper { opacity: 1; transform: translateY(-2px); }
  :global(button:is(:hover, :focus-visible)) .header-icon.layout .layout-vertical { transform: translateX(2px); }
  :global(button:is(:hover, :focus-visible)) .header-icon.layout .layout-horizontal { transform: translateY(-2px); }
  /* The top left piece comes loose from the other three and leaves its sockets behind. */
  .puzzle-corner { transform-box: view-box; transform-origin: 8px 8px; transition: transform 240ms cubic-bezier(.34, 1.56, .64, 1); }
  :global(button:is(:hover, :focus-visible)) .header-icon.extensions .puzzle-corner { transform: translate(-2.8px, -2.8px) rotate(-8deg); }
  :global(button:is(:hover, :focus-visible)) .header-icon.inspector .detective-face { transform: translateX(.5px) rotate(3deg); }
  :global(button:is(:hover, :focus-visible)) .header-icon.inspector .detective-hat { transform: translateY(-2px) rotate(-4deg); }
  /* A third of a turn looks the same as where the gear started, so it never ends crooked. */
  .gear { transform-box: view-box; transform-origin: 12px 12px; transition: transform 560ms cubic-bezier(.16, 1, .3, 1); }
  :global(button:is(:hover, :focus-visible)) .header-icon.settings .gear { transform: rotate(120deg); }
  :global(button:is(:hover, :focus-visible)) .header-icon.orb .lume-head { transform: rotate(-7deg) translateY(-1px); }
  @media (prefers-reduced-motion: reduce) {
    .puzzle-corner, .gear, .folder-front, .folder-paper, .layout-vertical, .layout-horizontal, .detective-face, .detective-hat, .lume-head, .board-node-one, .board-node-two, .board-node-three { transition: none !important; transform: none !important; }
    .board-pulse { animation: none !important; }
  }
</style>
