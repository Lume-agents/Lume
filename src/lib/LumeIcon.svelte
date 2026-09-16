<script module lang="ts">
  export type LumeIconName =
    | "arrow-down"
    | "attachment"
    | "bolt"
    | "bolt-filled"
    | "check"
    | "chevron-down"
    | "close"
    | "download"
    | "columns"
    | "copy"
    | "diff"
    | "file"
    | "inspector"
    | "layout"
    | "mode-default"
    | "mode-plan"
    | "note"
    | "maximize"
    | "orb"
    | "restore"
    | "save"
    | "search"
    | "send"
    | "sources"
    | "fork"
    | "settings"
    | "split"
    | "stop"
    | "take-control"
    | "steer"
    | "plus"
    | "rename"
    | "trash";
</script>

<script lang="ts">
  let {
    name,
    size = 20,
    label,
    strokeWidth = 1.75,
  } = $props<{
    name: LumeIconName;
    size?: number;
    label?: string;
    strokeWidth?: number;
  }>();
</script>

<svg
  class="lume-icon"
  xmlns="http://www.w3.org/2000/svg"
  viewBox="0 0 24 24"
  width={size}
  height={size}
  fill="none"
  stroke="currentColor"
  stroke-width={strokeWidth}
  stroke-linecap="round"
  stroke-linejoin="round"
  role={label ? "img" : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : "true"}
  focusable="false"
>
  {#if label}<title>{label}</title>{/if}
  {#if name === "inspector"}
    <rect x="3" y="4" width="18" height="16" rx="3" />
    <path d="M14.5 4v16M17.5 8h.01M17.5 12h.01M17.5 16h.01" />
  {:else if name === "layout"}
    <rect x="3" y="4" width="18" height="16" rx="3" />
    <path d="M9 4v16M9 11h12" />
  {:else if name === "settings"}
    <path d="M4 7h5M15 7h5M4 17h9M19 17h1" />
    <rect x="9" y="4.5" width="6" height="5" rx="2" />
    <rect x="13" y="14.5" width="6" height="5" rx="2" />
  {:else if name === "orb"}
    <circle cx="12" cy="12" r="5" />
    <path d="M12 3v2M12 19v2M3 12h2M19 12h2" />
  {:else if name === "search"}
    <circle cx="10.5" cy="10.5" r="5.5" />
    <path d="m14.5 14.5 5 5" />
  {:else if name === "split"}
    <rect x="3" y="4" width="18" height="16" rx="3" />
    <path d="M12 4v16" />
  {:else if name === "close"}
    <path d="m7 7 10 10M17 7 7 17" />
  {:else if name === "maximize"}
    <path d="M10 4H4v6M14 20h6v-6M4.5 9.5 10 4M14 20l5.5-5.5" />
  {:else if name === "restore"}
    <path d="M10 4v6H4M14 20v-6h6M10 10 4.5 4.5M14 14l5.5 5.5" />
  {:else if name === "save"}
    <path d="M5 3.5h11l3 3v14H5zM8 3.5v6h8v-6M8 20.5v-7h8v7" />
  {:else if name === "arrow-down"}
    <path d="M12 4v14M6.5 12.5 12 18l5.5-5.5" />
  {:else if name === "download"}
    <path d="M12 3.5v10M8 10l4 4 4-4M4 16.5V20h16v-3.5" />
  {:else if name === "diff"}
    <path d="M7 4v16M17 4v16M4 8h6M14 16h6" />
  {:else if name === "columns"}
    <rect x="3" y="4" width="18" height="16" rx="3" />
    <path d="M12 4v16M7 8h2M7 12h2M15 8h2M15 12h2" />
  {:else if name === "copy"}
    <rect x="8" y="8" width="11" height="12" rx="2" />
    <path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h2" />
  {:else if name === "sources"}
    <path d="M5 5.5A2.5 2.5 0 0 1 7.5 3H19v15H7.5A2.5 2.5 0 0 0 5 20.5z" />
    <path d="M5 5.5v15M9 7h6M9 11h7" />
  {:else if name === "fork"}
    <circle cx="7" cy="5" r="2" />
    <circle cx="17" cy="6" r="2" />
    <circle cx="17" cy="18" r="2" />
    <path d="M7 7v4a3 3 0 0 0 3 3h5M9 9h4a4 4 0 0 0 4-4" />
  {:else if name === "file"}
    <path d="M6 3h8l4 4v14H6zM14 3v5h4M9 13h6M9 17h4" />
  {:else if name === "note"}
    <path d="M5 4h14v13H9l-4 3v-3H5zM8 8h8M8 12h6" />
  {:else if name === "attachment"}
    <path d="m8 12.5 6.5-6.5a3 3 0 0 1 4.2 4.2l-8.4 8.4a4.5 4.5 0 0 1-6.3-6.3l8-8" />
  {:else if name === "bolt" || name === "bolt-filled"}
    <path d="M13.8 2.5 5.5 13h6.1l-1.4 8.5L18.5 11h-6.1z" fill={name === "bolt-filled" ? "currentColor" : "none"} />
  {:else if name === "mode-default"}
    <path d="M5 7h14M5 12h10M5 17h7" />
  {:else if name === "mode-plan"}
    <path d="M5 4.5h14v15H5zM8 8h8M8 12h6M8 16h3" />
  {:else if name === "send"}
    <path d="m3.5 11 17-7-6.5 17-3.2-7-7.3-3Z" />
    <path d="m10.8 14 4.5-5" />
  {:else if name === "stop"}
    <rect x="6.5" y="6.5" width="11" height="11" rx="2.5" fill="currentColor" stroke="none" />
  {:else if name === "take-control"}
    <rect x="8" y="3.5" width="12.5" height="17" rx="3" />
    <path d="M3.5 12h10M10 8.5l3.5 3.5-3.5 3.5" />
  {:else if name === "steer"}
    <path d="M5 17.5c2.8-5.7 6.1-8.3 13.5-9M14.5 5.5l4 3-3 4" />
    <path d="M5 7v3.5" />
  {:else if name === "plus"}
    <path d="M12 5v14M5 12h14" />
  {:else if name === "rename"}
    <path d="m4 20 1.3-5.2L15.6 4.5a2.1 2.1 0 0 1 3 0l.9.9a2.1 2.1 0 0 1 0 3L9.2 18.7Z" />
    <path d="m14 6.1 3.9 3.9M5.3 14.8l3.9 3.9" />
  {:else if name === "trash"}
    <path d="M5 7h14M9 7V4h6v3M7 7l1 13h8l1-13M10 11v5M14 11v5" />
  {:else if name === "chevron-down"}
    <path d="m6.5 9 5.5 5.5L17.5 9" />
  {:else if name === "check"}
    <path d="m5 12.5 4.2 4.2L19 7" />
  {/if}
</svg>

<style>
  .lume-icon { display: block; flex: 0 0 auto; overflow: visible; }
</style>
