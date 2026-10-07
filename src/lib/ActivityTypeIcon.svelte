<script lang="ts">
  import type { ActivityCategory } from "$lib/activityPresentation";

  let { category, size = 15 }: { category: ActivityCategory | "analysis"; size?: number } = $props();
</script>

<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width={size} height={size} fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">
  {#if category === "command"}
    <path d="m4 7 5 5-5 5M11 17h9" />
  {:else if category === "test"}
    <rect x="4" y="3" width="16" height="18" rx="2" />
    <path d="m8 12 2.5 2.5 5.5-5.5" />
  {:else if category === "edit"}
    <path d="M6 3h8l4 4v14H6zM14 3v5h4M9 16l5.7-5.7 2 2L11 18H9z" />
  {:else if category === "read"}
    <path d="M12 5C9.5 3.5 7 3.5 4 4v14c3-.5 5.5-.5 8 1 2.5-1.5 5-1.5 8-1V4c-3-.5-5.5-.5-8 1ZM12 5v14" />
  {:else if category === "search"}
    <circle cx="10.5" cy="10.5" r="5.5" />
    <path d="m15 15 4.5 4.5" />
  {:else if category === "analysis"}
    <path d="M7 7.5a6.5 6.5 0 0 1 10 0M6 13a6 6 0 0 0 12 0M9 19h6" />
    <circle cx="12" cy="10" r="2" />
  {:else if category === "git"}
    <path d="M6 3v12" />
    <circle cx="18" cy="6" r="2.6" />
    <circle cx="6" cy="18" r="2.6" />
    <path d="M18 8.6A9 9 0 0 1 8.7 17.6" />
  {:else if category === "plan"}
    <path d="M5 3.5h14v17H5zM8 8h8M8 12h8M8 16h5" />
  {:else}
    <path d="m14.5 4.5 5 5-4 4-5-5a4.5 4.5 0 0 1-6 6l6 6 3-3-6-6a4.5 4.5 0 0 1 7-7Z" />
  {/if}
</svg>

<style>
  svg { display: block; flex: 0 0 auto; }
</style>
