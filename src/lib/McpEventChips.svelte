<script lang="ts">
  import type { McpChip, McpEventInfo } from "$lib/mcpEvents";
  import type { Language } from "$lib/i18n";

  // The product an MCP call went to (Jira, Confluence, a server's name), then what it did: a ticket's new status, the
  // fields it changed, how many results it found. `chips` stands in for a single call when the row is a whole run.
  let { info, chips, serverLabel, language = "en" } = $props<{ info?: McpEventInfo; chips?: McpChip[]; serverLabel?: string; language?: Language }>();
  const pt = $derived(language === "pt-BR");
  const label = $derived(info?.serverLabel ?? serverLabel ?? "");
  const list = $derived<McpChip[]>([...(chips ?? info?.chips ?? []), ...(info?.failed ? [{ label: pt ? "Falhou" : "Failed", tone: "bad" as const, title: info.error }] : [])]);
</script>

<span class="mcp-chips">
  {#if label}<span class="mcp-chip server" title={info ? `${info.server} · ${info.tool}` : label}><span>{label}</span></span>{/if}
  {#each list as chip, index (index)}
    <span class="mcp-chip {chip.tone}" title={chip.title ?? chip.label}><span>{chip.label}</span></span>
  {/each}
</span>

<style>
  .mcp-chips { min-width: 0; display: flex; flex-wrap: wrap; align-items: center; gap: 4px; }
  .mcp-chip { --tone: var(--workspace-muted, #61736a); box-sizing: border-box; max-width: 100%; min-height: 17px; padding: 1px 6px; display: inline-flex; align-items: center; gap: 4px; border: 1px solid color-mix(in srgb, var(--tone) 28%, var(--workspace-line, rgba(65, 94, 80, .14))); border-radius: 6px; color: color-mix(in srgb, var(--tone) 78%, var(--workspace-strong, #52665c)); background: color-mix(in srgb, var(--tone) 10%, transparent); font: 650 var(--activity-detail-size, var(--chat-tiny-font-size, 10px))/1.3 var(--lume-font-ui, Inter, sans-serif); }
  .mcp-chip > span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .server { --tone: #6b7bd6; }
  .info { --tone: #4d99cc; }
  .ok { --tone: #3f9b69; }
  .warn { --tone: #c78d35; }
  .bad { --tone: #c45f5b; }
  .muted { --tone: var(--workspace-muted, #61736a); }
</style>
