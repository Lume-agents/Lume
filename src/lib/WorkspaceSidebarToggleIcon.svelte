<script lang="ts">
  let { collapsed, size = 20 }: { collapsed: boolean; size?: number } = $props();
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
  class:collapsed
  class="sidebar-toggle-icon"
  aria-hidden="true"
  focusable="false"
>
  <rect x="2.5" y="4" width="19" height="16" rx="2.5" />
  <path class="sidebar-divider" d="M9.5 4v16" />
  <path class="sidebar-rail-lines" d="M5.4 8h1.3m-1.3 4h1.3m-1.3 4h1.3" />
  <path class="sidebar-arrow" d="m16.5 9-3 3 3 3" />
</svg>

<style>
  .sidebar-toggle-icon { display: block; flex: 0 0 auto; }
  .sidebar-divider, .sidebar-rail-lines, .sidebar-arrow { transition: transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .sidebar-arrow { transform-box: fill-box; transform-origin: center; }
  .sidebar-toggle-icon.collapsed .sidebar-arrow { transform: rotate(180deg); }
  :global(button:is(:hover, :focus-visible)) .sidebar-toggle-icon:not(.collapsed) .sidebar-divider,
  :global(button:is(:hover, :focus-visible)) .sidebar-toggle-icon:not(.collapsed) .sidebar-rail-lines { transform: translateX(-2px); }
  :global(button:is(:hover, :focus-visible)) .sidebar-toggle-icon:not(.collapsed) .sidebar-arrow { transform: translateX(-1.5px); }
  :global(button:is(:hover, :focus-visible)) .sidebar-toggle-icon.collapsed .sidebar-divider,
  :global(button:is(:hover, :focus-visible)) .sidebar-toggle-icon.collapsed .sidebar-rail-lines { transform: translateX(2px); }
  :global(button:is(:hover, :focus-visible)) .sidebar-toggle-icon.collapsed .sidebar-arrow { transform: rotate(180deg) translateX(-1.5px); }
  @media (prefers-reduced-motion: reduce) {
    .sidebar-divider, .sidebar-rail-lines, .sidebar-arrow { transition: none; }
  }
</style>
