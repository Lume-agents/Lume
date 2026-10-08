<script lang="ts">
  import { gitOperationLabel, gitResultLabel, gitSequence, type GitEventInfo } from "$lib/gitEvents";
  import type { Language } from "$lib/i18n";

  let { info, language = "en" } = $props<{ info: GitEventInfo; language?: Language }>();

  const pt = $derived(language === "pt-BR");
  const sequence = $derived(gitSequence(info, language));
  const headline = $derived(gitOperationLabel(info.operation, language));
  const result = $derived(gitResultLabel(info, language));
  // The local folder only adds something when it is not the repository's own name.
  const place = $derived([info.repo, info.directory && !info.repo?.endsWith(`/${info.directory}`) && info.directory !== info.repo ? info.directory : ""].filter(Boolean));
  // `all` and `new-branch` only describe how the command was typed; the rest change its outcome.
  const options = $derived(info.flags.filter((flag: string) => !["all", "new-branch", "staged"].includes(flag)));
  const rows = $derived.by(() => {
    const items: [string, string][] = [];
    items.push([pt ? "Operação" : "Operation", info.tool === "gh" ? `gh · ${headline}` : headline]);
    if (place.length) items.push([pt ? "Repositório" : "Repository", place.join(" · ")]);
    if (info.fromBranch && info.branch) items.push(["Branch", `${info.fromBranch} → ${info.branch}`]);
    else if (info.branch || info.fromBranch) items.push(["Branch", (info.branch ?? info.fromBranch) as string]);
    if (info.remote) items.push([pt ? "Remoto" : "Remote", info.remote]);
    if (info.hash) items.push([info.hash.startsWith("#") ? (pt ? "Referência" : "Reference") : "Commit", info.hash]);
    if (info.subject) items.push([pt ? "Mensagem" : "Message", info.subject]);
    if (info.sync) items.push([pt ? "Sincronia" : "Sync", [info.sync.ahead ? `↑${info.sync.ahead} ${pt ? "à frente" : "ahead"}` : "", info.sync.behind ? `↓${info.sync.behind} ${pt ? "atrás" : "behind"}` : ""].filter(Boolean).join("  ")]);
    if (info.clean) items.push([pt ? "Árvore" : "Tree", pt ? "limpa" : "clean"]);
    if (info.stats && (info.stats.files || info.stats.added || info.stats.removed)) {
      const files = info.stats.files ? `${info.stats.files} ${pt ? (info.stats.files === 1 ? "arquivo" : "arquivos") : (info.stats.files === 1 ? "file" : "files")}` : "";
      items.push([pt ? "Mudanças" : "Changes", [files, info.stats.added ? `+${info.stats.added}` : "", info.stats.removed ? `−${info.stats.removed}` : ""].filter(Boolean).join("  ")]);
    }
    if (options.length) items.push([pt ? "Opções" : "Options", options.join(", ")]);
    if (result) items.push([pt ? "Resultado" : "Result", info.result === "failed" && info.note ? `${result} · ${info.note}` : result]);
    return items;
  });
</script>

<div class="git-details">
  {#if sequence.length}
    <ol class="git-sequence" aria-label={pt ? "Sequência de operações" : "Operation sequence"}>
      {#each sequence as step, index (index)}
        <li class:headline={step === headline}>{step}</li>
      {/each}
    </ol>
  {/if}
  <dl>
    {#each rows as [label, value] (label)}
      <dt>{label}</dt>
      <dd title={value}>{value}</dd>
    {/each}
  </dl>
  <pre class="git-command"><span aria-hidden="true">$ </span>{info.command}</pre>
  {#if info.output}<pre class="git-output">{info.output}</pre>{/if}
</div>

<style>
  .git-details { --git-tone: #cf673d; min-width: 0; margin: 0 8px 8px 20px; display: grid; gap: 7px; }
  .git-sequence { margin: 0; padding: 0; display: flex; flex-wrap: wrap; align-items: center; gap: 3px; list-style: none; }
  .git-sequence li { display: flex; align-items: center; gap: 3px; color: var(--workspace-muted, #61736a); font: 650 var(--activity-detail-size, var(--chat-tiny-font-size, 8px))/1.3 var(--lume-font-ui, Inter, sans-serif); }
  .git-sequence li + li::before { color: var(--workspace-faint, #89958f); content: "›"; }
  .git-sequence li.headline { color: var(--git-tone); }
  dl { margin: 0; min-width: 0; display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: 3px 12px; }
  dt { color: var(--workspace-faint, #89958f); font: 650 var(--activity-detail-size, var(--chat-tiny-font-size, 8px))/1.5 var(--lume-font-ui, Inter, sans-serif); }
  dd { min-width: 0; margin: 0; overflow: hidden; color: var(--workspace-strong, #52665c); font: 650 var(--activity-detail-size, var(--chat-tiny-font-size, 8px))/1.5 var(--lume-font-code, "SFMono-Regular", Consolas, "Liberation Mono", monospace); text-overflow: ellipsis; white-space: nowrap; }
  pre { min-width: 0; max-width: 100%; max-height: 180px; margin: 0; padding: 7px 8px; overflow: auto; border: 1px solid var(--workspace-line, rgba(65, 94, 80, .12)); border-radius: 7px; color: var(--workspace-text, #4f6258); background: var(--workspace-pane, #eaf0ed); font: var(--activity-detail-size, var(--chat-tiny-font-size, 8px))/1.5 var(--lume-font-code, "SFMono-Regular", Consolas, "Liberation Mono", monospace); white-space: pre-wrap; overflow-wrap: anywhere; }
  .git-command { border-left: 2px solid color-mix(in srgb, var(--git-tone) 55%, transparent); color: var(--workspace-strong, #52665c); }
  .git-command span { color: var(--git-tone); }
  :global(.terminal-window.dark) .git-details, :global(.workspace.dark) .git-details { --git-tone: #e48a62; }
  :global(.terminal-window.dark) pre { color: #adbbb4; background: rgba(4, 12, 8, .18); }
  :global(.terminal-window.dark) dd, :global(.terminal-window.dark) .git-command { color: #c3d2ca; }
</style>
