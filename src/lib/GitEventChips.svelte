<script lang="ts">
  import LumeIcon from "$lib/LumeIcon.svelte";
  import { gitResultLabel, type GitEventInfo } from "$lib/gitEvents";
  import type { Language } from "$lib/i18n";

  let { info, language = "en" } = $props<{ info: GitEventInfo; language?: Language }>();

  const pt = $derived(language === "pt-BR");
  const place = $derived(info.repo ?? info.directory);
  const result = $derived(gitResultLabel(info, language));
  const reference = $derived(info.hash ? (info.hash.startsWith("#") ? info.hash : info.hash.slice(0, 7)) : "");
  const warnings = $derived(info.flags.filter((flag: string) => ["force", "hard", "amend", "squash", "abort"].includes(flag)));
  const flagLabel = (flag: string) => ({
    force: pt ? "forçado" : "force",
    hard: "--hard",
    amend: "amend",
    squash: "squash",
    abort: pt ? "abortar" : "abort",
  })[flag] ?? flag;
  // merge and pull requests move changes from one branch to another
  const showsFlow = $derived(Boolean(info.fromBranch && info.branch));
</script>

<span class="git-chips">
  {#if place}
    <span class="git-chip place" title={place}><LumeIcon name="repository" size={10} /><span>{place}</span></span>
  {/if}
  {#if showsFlow}
    <span class="git-chip branch" title={`${info.fromBranch} → ${info.branch}`}><LumeIcon name="branch" size={10} /><span>{info.fromBranch}</span><i aria-hidden="true">→</i><span>{info.branch}</span></span>
  {:else if info.branch || info.fromBranch}
    <span class="git-chip branch" title={info.branch ?? info.fromBranch}><LumeIcon name="branch" size={10} /><span>{info.branch ?? info.fromBranch}</span></span>
  {/if}
  {#if info.remote && !info.branch}
    <span class="git-chip" title={info.remote}><LumeIcon name="external" size={10} /><span>{info.remote}</span></span>
  {/if}
  {#if reference}
    <span class="git-chip hash" title={info.hash}><LumeIcon name={reference.startsWith("#") ? "pull-request" : "commit"} size={10} /><span>{reference}</span></span>
  {/if}
  {#if info.subject && !["commit", "pull-request", "issue", "release"].includes(info.operation)}
    <span class="git-chip subject" title={info.subject}><span>{info.subject}</span></span>
  {/if}
  {#if info.sync}
    <span class="git-chip sync" title={pt ? "Em relação ao remoto" : "Compared with the remote"}>
      {#if info.sync.ahead}<b class="added">↑{info.sync.ahead}</b>{/if}
      {#if info.sync.behind}<b class="removed">↓{info.sync.behind}</b>{/if}
    </span>
  {/if}
  {#if info.clean}
    <span class="git-chip clean">{pt ? "limpo" : "clean"}</span>
  {/if}
  {#if info.stats && (info.stats.files || info.stats.added || info.stats.removed)}
    <span class="git-chip stats" title={pt ? "Mudanças" : "Changes"}>
      {#if info.stats.files}<span>{info.stats.files} {pt ? (info.stats.files === 1 ? "arquivo" : "arquivos") : (info.stats.files === 1 ? "file" : "files")}</span>{/if}
      {#if info.stats.added}<b class="added">+{info.stats.added}</b>{/if}
      {#if info.stats.removed}<b class="removed">−{info.stats.removed}</b>{/if}
    </span>
  {/if}
  {#each warnings as flag (flag)}
    <span class="git-chip caution">{flagLabel(flag)}</span>
  {/each}
  {#if result}
    <span class="git-chip result {info.result}" title={info.note && info.result === "failed" ? info.note : undefined}>{result}</span>
  {/if}
</span>

<style>
  .git-chips { --git-tone: #cf673d; min-width: 0; display: flex; flex-wrap: wrap; align-items: center; gap: 4px; }
  .git-chip { box-sizing: border-box; max-width: 100%; min-height: 17px; padding: 1px 6px 1px 5px; display: inline-flex; align-items: center; gap: 4px; border: 1px solid color-mix(in srgb, var(--git-tone) 22%, var(--workspace-line, rgba(65, 94, 80, .18))); border-radius: 999px; color: var(--workspace-strong, #52665c); background: color-mix(in srgb, var(--git-tone) 7%, transparent); font: 650 var(--activity-detail-size, var(--chat-tiny-font-size, 8px))/1.25 "SFMono-Regular", Consolas, "Liberation Mono", monospace; white-space: nowrap; }
  .git-chip > span { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .git-chip :global(.lume-icon) { flex: 0 0 auto; color: var(--git-tone); }
  .git-chip i { color: var(--workspace-faint, #89958f); font-style: normal; }
  .git-chip.place { color: var(--workspace-muted, #61736a); }
  .git-chip.branch { color: color-mix(in srgb, var(--git-tone) 70%, var(--workspace-strong, #52665c)); background: color-mix(in srgb, var(--git-tone) 11%, transparent); }
  .git-chip.hash { color: var(--workspace-muted, #61736a); }
  .git-chip.subject { max-width: 26ch; padding-left: 6px; color: var(--workspace-muted, #61736a); font-family: Inter, sans-serif; }
  .git-chip.sync { padding-left: 6px; gap: 5px; }
  .git-chip.sync b, .git-chip.stats b { font-weight: 700; font-variant-numeric: tabular-nums; }
  .git-chip.sync .added { color: #3f9b69; }
  .git-chip.sync .removed { color: #c0605b; }
  .git-chip.clean { padding-left: 6px; color: #3f9b69; border-color: color-mix(in srgb, #3f9b69 32%, transparent); background: color-mix(in srgb, #3f9b69 9%, transparent); font-family: Inter, sans-serif; }
  .git-chip.stats { gap: 5px; padding-left: 6px; color: var(--workspace-muted, #61736a); font-family: Inter, sans-serif; }
  .git-chip.stats b { font: 700 inherit; font-family: "SFMono-Regular", Consolas, monospace; font-variant-numeric: tabular-nums; }
  .git-chip.stats .added { color: #3f9b69; }
  .git-chip.stats .removed { color: #c0605b; }
  .git-chip.caution { padding-left: 6px; border-color: color-mix(in srgb, #c78d35 38%, transparent); color: #a8741f; background: color-mix(in srgb, #c78d35 12%, transparent); font-family: Inter, sans-serif; }
  .git-chip.result { padding-left: 6px; font-family: Inter, sans-serif; }
  .git-chip.result.failed, .git-chip.result.conflict { border-color: color-mix(in srgb, #c45f5b 40%, transparent); color: #b45450; background: color-mix(in srgb, #c45f5b 11%, transparent); }
  .git-chip.result.noop { color: var(--workspace-muted, #61736a); background: transparent; }
  :global(.terminal-window.dark) .git-chips, :global(.workspace.dark) .git-chips { --git-tone: #e48a62; }
  :global(.terminal-window.dark) .git-chip, :global(.workspace.dark) .git-chip { color: #c3d2ca; }
  :global(.terminal-window.dark) .git-chip.caution, :global(.workspace.dark) .git-chip.caution { color: #e3b667; }
  :global(.terminal-window.dark) .git-chip.result.failed, :global(.terminal-window.dark) .git-chip.result.conflict, :global(.workspace.dark) .git-chip.result.failed, :global(.workspace.dark) .git-chip.result.conflict { color: #e48b87; }
</style>
