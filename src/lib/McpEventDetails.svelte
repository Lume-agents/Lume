<script lang="ts">
  import type { McpEventInfo } from "$lib/mcpEvents";
  import type { Language } from "$lib/i18n";

  // What an MCP call was about as a short table, with its raw arguments and answer one fold away.
  let { info, language = "en" } = $props<{ info: McpEventInfo; language?: Language }>();
  const pt = $derived(language === "pt-BR");
  const rows = $derived<Array<[string, string]>>([...info.rows, ...(info.error ? [[pt ? "Erro" : "Error", info.error] as [string, string]] : [])]);
  const raw = (value: unknown) => {
    const text = typeof value === "string" ? value : JSON.stringify(value, null, 2);
    return text && text.length > 6_000 ? `${text.slice(0, 6_000)}…` : text;
  };
</script>

<div class="mcp-details">
  {#if rows.length}
    <dl>{#each rows as [label, value] (label)}<dt>{label}</dt><dd title={value}>{value}</dd>{/each}</dl>
  {/if}
  <details class="mcp-raw">
    <summary>{pt ? "Chamada completa" : "Full call"}</summary>
    <small>{info.server} · {info.tool}</small>
    <pre>{raw(info.input)}</pre>
    {#if info.resultText}<pre>{raw(info.resultText)}</pre>{/if}
  </details>
</div>

<style>
  .mcp-details { min-width: 0; margin: 0 8px 8px 20px; display: grid; gap: 7px; }
  dl { margin: 0; min-width: 0; display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: 3px 12px; }
  dt { color: var(--workspace-faint, #89958f); font: 650 var(--activity-detail-size, var(--chat-tiny-font-size, 8px))/1.5 var(--lume-font-ui, Inter, sans-serif); }
  dd { min-width: 0; margin: 0; overflow: hidden; color: var(--workspace-strong, #52665c); font: 650 var(--activity-detail-size, var(--chat-tiny-font-size, 8px))/1.5 var(--lume-font-ui, Inter, sans-serif); text-overflow: ellipsis; white-space: nowrap; }
  .mcp-raw summary { color: var(--workspace-muted, #61736a); font: 650 var(--activity-detail-size, var(--chat-tiny-font-size, 8px))/1.5 var(--lume-font-ui, Inter, sans-serif); cursor: pointer; }
  .mcp-raw small { display: block; margin: 4px 0; color: var(--workspace-faint, #89958f); font: 600 var(--activity-detail-size, var(--chat-tiny-font-size, 8px))/1.4 var(--lume-font-code, "SFMono-Regular", Consolas, monospace); }
  pre { min-width: 0; max-width: 100%; max-height: 180px; margin: 4px 0 0; padding: 7px 8px; overflow: auto; border: 1px solid var(--workspace-line, rgba(65, 94, 80, .12)); border-radius: 7px; color: var(--workspace-text, #4f6258); background: rgba(70, 100, 85, .035); font: var(--activity-detail-size, var(--chat-tiny-font-size, 8px))/1.5 var(--lume-font-code, "SFMono-Regular", Consolas, monospace); white-space: pre-wrap; overflow-wrap: anywhere; }
  :global(.terminal-window.dark) pre, :global(.workspace.dark) pre { color: #adbbb4; background: rgba(4, 12, 8, .18); }
</style>
