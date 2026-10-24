<script lang="ts">
  import type { ActivityCategory } from "$lib/activityPresentation";

  let { category, size = 15 }: { category: ActivityCategory | "analysis"; size?: number } = $props();
</script>

<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width={size} height={size} fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">
  {#if category === "skill"}
    <path d="M12 5a3 3 0 1 0-6 .13 4 4 0 0 0-2.53 5.77 4 4 0 0 0 .56 6.59A4 4 0 1 0 12 18Z" />
    <path d="M12 5a3 3 0 1 1 6 .13 4 4 0 0 1 2.53 5.77 4 4 0 0 1-.56 6.59A4 4 0 1 1 12 18Z" />
    <path d="M12 5v13M15 13a4.5 4.5 0 0 1-3-4 4.5 4.5 0 0 1-3 4M17.6 6.5a3 3 0 0 0 .4-1.37M6 5.13a3 3 0 0 0 .4 1.37" />
  {:else if category === "instruction"}
    <path d="M7 3h7l4 4v14H7a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1ZM14 3v4h4M9.5 11h5M9.5 14h5M9.5 17h3" />
  {:else if category === "command"}
    <path d="m4 7 5 5-5 5M11 17h9" />
  {:else if category === "test"}
    <rect x="4" y="3" width="16" height="18" rx="2" />
    <path d="m8 12 2.5 2.5 5.5-5.5" />
  {:else if category === "edit"}
    <path d="M6 3h8l4 4v14H6zM14 3v5h4M9 16l5.7-5.7 2 2L11 18H9z" />
  {:else if category === "read" || category === "analysis"}
    <path d="M8 16.5a4.5 4.5 0 0 1-3.6-7.2A4.5 4.5 0 0 1 11.8 5a4.5 4.5 0 0 1 7.6 4.3A4.5 4.5 0 0 1 16 16.5Z" />
    <circle cx="5.5" cy="19" r="1.1" />
    <circle cx="2.8" cy="21.5" r=".6" fill="currentColor" stroke="none" />
  {:else if category === "search"}
    <circle cx="10.5" cy="10.5" r="5.5" />
    <path d="m15 15 4.5 4.5" />
  {:else if category === "git"}
    <path d="M6 3v12" />
    <circle cx="18" cy="6" r="2.6" />
    <circle cx="6" cy="18" r="2.6" />
    <path d="M18 8.6A9 9 0 0 1 8.7 17.6" />
  {:else if category === "plan"}
    <path d="M5 3.5h14v17H5zM8 8h8M8 12h8M8 16h5" />
  {:else}
    <path d="M14.2 3.8a5.5 5.5 0 0 0-6.5 7.6l-4.1 4.1a2.8 2.8 0 0 0 4 4l4.1-4.1a5.5 5.5 0 0 0 7.6-6.5l-3.6 3.6-4.2-4.2Z" />
  {/if}
</svg>

<style>
  svg { display: block; flex: 0 0 auto; }
</style>
