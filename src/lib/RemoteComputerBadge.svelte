<script lang="ts">
  // Beside the name of a chat that runs on another computer: a computer icon and that computer's name.
  let { computer, size = "small" }: { computer: string; size?: "small" | "medium" } = $props();
</script>

<span class="remote-computer-badge {size}" title={computer}>
  <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="4" width="18" height="12" rx="2" /><path d="M8 20h8M12 16v4" /></svg>
  <span>{computer}</span>
</span>

<style>
  .remote-computer-badge { min-width: 0; max-width: 46%; flex: 0 1 auto; display: inline-flex; align-items: center; gap: 3px; padding: 1px 6px 1px 4px; overflow: hidden; border: 1px solid color-mix(in srgb, var(--workspace-accent, #4daa77) 35%, transparent); border-radius: 999px; color: var(--workspace-accent, #4daa77); background: color-mix(in srgb, var(--workspace-accent, #4daa77) 10%, transparent); font-size: 8px; font-weight: 700; line-height: 1.5; white-space: nowrap; }
  .remote-computer-badge span { overflow: hidden; text-overflow: ellipsis; }
  .remote-computer-badge svg { width: 10px; height: 10px; flex: 0 0 auto; fill: none; stroke: currentColor; stroke-width: 2; stroke-linecap: round; stroke-linejoin: round; }
  .remote-computer-badge.medium { padding: 2px 8px 2px 6px; font-size: 10px; }
  .remote-computer-badge.medium svg { width: 12px; height: 12px; }
</style>
