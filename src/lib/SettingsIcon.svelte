<script lang="ts" module>
  export type SettingsIconName =
    | "agents" | "companions" | "detectors" | "interface" | "tour" | "appearance" | "preferences"
    | "shortcuts" | "profiles" | "remote" | "mobile" | "reset";
</script>

<script lang="ts">
  // The icon in front of each Settings section title, the same in the Orb and the Workspace.
  let { name, size = 14 }: { name: SettingsIconName; size?: number } = $props();
</script>

<svg class="settings-icon" width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
  {#if name === "agents"}
    <rect x="4" y="8" width="16" height="12" rx="3" /><path d="M12 4v4M9 13v1M15 13v1M2 14h2M20 14h2" />
  {:else if name === "companions"}
    <circle cx="9" cy="8" r="3.5" /><path d="M2.5 20a6.5 6.5 0 0 1 13 0" /><path d="M16 4.6a3.5 3.5 0 0 1 0 6.8M18 14.5a6.5 6.5 0 0 1 3.5 5.5" />
  {:else if name === "detectors"}
    <circle cx="12" cy="12" r="9" /><circle cx="12" cy="12" r="5" /><path d="M12 12 18.5 5.5" />
  {:else if name === "interface"}
    <rect x="3" y="4" width="18" height="16" rx="2.5" /><path d="M3 9h18M9 9v11" />
  {:else if name === "tour"}
    <circle cx="12" cy="12" r="9" /><path d="m15.5 8.5-2 5-5 2 2-5z" />
  {:else if name === "appearance"}
    <path d="M12 3a9 9 0 1 0 0 18c1.1 0 1.6-.8 1.6-1.6 0-.5-.2-.9-.5-1.2-.3-.3-.5-.7-.5-1.2 0-.9.7-1.6 1.6-1.6H16a5 5 0 0 0 5-5c0-4-4-7.4-9-7.4z" /><circle cx="7.5" cy="11" r="1" /><circle cx="10" cy="7" r="1" /><circle cx="15" cy="7.5" r="1" />
  {:else if name === "preferences"}
    <path d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0" /><circle cx="16" cy="6" r="2" /><circle cx="10" cy="12" r="2" /><circle cx="18" cy="18" r="2" />
  {:else if name === "shortcuts"}
    <rect x="2.5" y="6" width="19" height="12" rx="2.5" /><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10" />
  {:else if name === "profiles"}
    <path d="M3 7.5A2.5 2.5 0 0 1 5.5 5H9l2 2h7.5A2.5 2.5 0 0 1 21 9.5v7a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 16.5z" /><path d="M9 14.5h6" />
  {:else if name === "remote"}
    <rect x="3" y="4" width="18" height="12" rx="2" /><path d="M8 20h8M12 16v4" />
  {:else if name === "mobile"}
    <rect x="6.5" y="2.5" width="11" height="19" rx="2.5" /><path d="M11 18.5h2" />
  {:else}
    <path d="M3 12a9 9 0 1 0 3-6.7L3 8" /><path d="M3 3v5h5" />
  {/if}
</svg>

<style>
  .settings-icon { flex: 0 0 auto; margin-right: 7px; vertical-align: -2px; fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; opacity: .85; }
</style>
