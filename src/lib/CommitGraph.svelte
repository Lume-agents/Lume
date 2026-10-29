<script lang="ts">
  import { onMount } from "svelte";
  import FileTypeIcon from "$lib/FileTypeIcon.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import { layoutGraph, graphWidth } from "$lib/commitGraph";
  import { getSessionCommit, getSessionCommitDiff, getSessionHistory, repositoryError, type CommitDetail, type HistoryCommit, type RepositoryDiff } from "$lib/repositories";
  import { transientScrollbar } from "$lib/transientScrollbar";

  // The repository's history as a graph, like the one in VS Code: lines for branches and merges, the branches and tags
  // at their commits, and the files and diff of any commit one click away.
  let { sessionId, language = "en", head = null }: { sessionId: string; language?: "en" | "pt-BR"; head?: string | null } = $props();
  const tr = (english: string, portuguese: string) => (language === "pt-BR" ? portuguese : english);

  const ROW = 40;
  const LANE = 14;
  const colors = ["#4daa77", "#4d99cc", "#d6a441", "#b57edc", "#d0625c", "#3fb8af", "#e0869a", "#8f9e5a"];

  let commits = $state<HistoryCommit[]>([]);
  let loading = $state(true);
  let more = $state(true);
  let error = $state("");
  let selected = $state<string | null>(null);
  let detail = $state<CommitDetail | null>(null);
  let detailError = $state("");
  let filePath = $state("");
  let diff = $state<RepositoryDiff | null>(null);
  let diffError = $state("");
  let generation = 0;

  const rows = $derived(layoutGraph(commits));
  const lanes = $derived(graphWidth(rows));
  const graphPixels = $derived(Math.max(lanes, 1) * LANE + 8);
  const x = (lane: number) => 8 + lane * LANE + LANE / 2 - 4;
  const color = (lane: number) => colors[lane % colors.length];

  async function load(skip: number) {
    const current = generation;
    loading = true;
    error = "";
    try {
      const page = await getSessionHistory(sessionId, skip);
      if (current !== generation) return;
      commits = skip === 0 ? page : [...commits, ...page];
      more = page.length >= 200;
    } catch (reason) {
      if (current === generation) error = repositoryError(String(reason).replace(/^Error:\s*/, ""), language === "pt-BR");
    } finally {
      if (current === generation) loading = false;
    }
  }

  async function select(oid: string) {
    if (selected === oid) { selected = null; detail = null; filePath = ""; diff = null; return; }
    selected = oid;
    detail = null;
    detailError = "";
    filePath = "";
    diff = null;
    try {
      const value = await getSessionCommit(sessionId, oid);
      if (selected === oid) detail = value;
    } catch (reason) {
      if (selected === oid) detailError = repositoryError(String(reason).replace(/^Error:\s*/, ""), language === "pt-BR");
    }
  }

  async function openFile(path: string) {
    if (!selected) return;
    const oid = selected;
    filePath = path;
    diff = null;
    diffError = "";
    try {
      const value = await getSessionCommitDiff(sessionId, oid, path);
      if (filePath === path && selected === oid) diff = value;
    } catch (reason) {
      if (filePath === path) diffError = repositoryError(String(reason).replace(/^Error:\s*/, ""), language === "pt-BR");
    }
  }

  const diffLines = $derived.by(() => {
    let before = 0; let after = 0;
    return (diff?.diff ?? "").split("\n").map((text) => {
      const hunk = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(text);
      if (hunk) { before = Number(hunk[1]); after = Number(hunk[2]); return { text, kind: "hunk", before: "", after: "" }; }
      if (text.startsWith("diff --git ") || text.startsWith("index ") || text.startsWith("--- ") || text.startsWith("+++ ") || text.startsWith("\\")) return { text, kind: "meta", before: "", after: "" };
      if (text.startsWith("+")) return { text, kind: "added", before: "", after: after ? String(after++) : "" };
      if (text.startsWith("-")) return { text, kind: "removed", before: before ? String(before++) : "", after: "" };
      return { text, kind: "context", before: before ? String(before++) : "", after: after ? String(after++) : "" };
    });
  });

  function when(seconds: number) {
    const date = new Date(seconds * 1000);
    const days = (Date.now() - date.getTime()) / 86_400_000;
    return days < 1
      ? new Intl.DateTimeFormat(language, { hour: "2-digit", minute: "2-digit" }).format(date)
      : new Intl.DateTimeFormat(language, { day: "numeric", month: "short", ...(days > 300 ? { year: "2-digit" } : {}) }).format(date);
  }

  const fileName = (path: string) => path.split("/").at(-1) ?? path;

  // The history is read again when the repository's head moves (a commit made from the chat, a pull).
  $effect(() => {
    void head;
    generation += 1;
    void load(0);
  });
  onMount(() => () => { generation += 1; });
</script>

<div class="history">
  {#if loading && !commits.length}
    <div class="skeleton" role="status" aria-label={tr("Loading history", "Carregando histórico")}><i></i><i></i><i></i></div>
  {:else if error}
    <p class="inline-error" role="alert">{error}</p>
  {:else if !commits.length}
    <p class="empty">{tr("This repository has no commits yet.", "Este repositório ainda não tem commits.")}</p>
  {:else}
    <ol class="rows" aria-label={tr("Commit history", "Histórico de commits")}>
      {#each commits as commit, index (commit.oid)}
        {@const row = rows[index]}
        <li class:selected={selected === commit.oid}>
          <button type="button" class="commit" aria-expanded={selected === commit.oid} onclick={() => void select(commit.oid)}>
            <svg class="graph" width={graphPixels} height={ROW} viewBox={`0 0 ${graphPixels} ${ROW}`} aria-hidden="true">
              {#each row.through as lane (lane)}<line x1={x(lane)} y1="0" x2={x(lane)} y2={ROW} stroke={color(lane)} />{/each}
              {#each row.from as lane (lane)}
                {#if lane === row.lane}<line x1={x(lane)} y1="0" x2={x(lane)} y2={ROW / 2} stroke={color(lane)} />
                {:else}<path d={`M ${x(lane)} 0 C ${x(lane)} ${ROW / 3}, ${x(row.lane)} ${ROW / 6}, ${x(row.lane)} ${ROW / 2}`} stroke={color(lane)} fill="none" />{/if}
              {/each}
              {#each row.to as lane (lane)}
                {#if lane === row.lane}<line x1={x(lane)} y1={ROW / 2} x2={x(lane)} y2={ROW} stroke={color(lane)} />
                {:else}<path d={`M ${x(row.lane)} ${ROW / 2} C ${x(row.lane)} ${(ROW * 2) / 3}, ${x(lane)} ${(ROW * 5) / 6}, ${x(lane)} ${ROW}`} stroke={color(lane)} fill="none" />{/if}
              {/each}
              <circle cx={x(row.lane)} cy={ROW / 2} r={commit.parents.length > 1 ? 3.2 : 4} fill={commit.parents.length > 1 ? "var(--workspace-bg, #1c1f1e)" : color(row.lane)} stroke={color(row.lane)} stroke-width="1.8" />
            </svg>
            <span class="copy">
              <strong>{commit.subject}</strong>
              <span class="meta-row">
                {#each commit.refs.slice(0, 3) as name (name)}<span class="ref" class:current={name.startsWith("HEAD")}>{name.replace(/^HEAD -> /, "")}</span>{/each}
                {#if commit.refs.length > 3}<span class="ref">+{commit.refs.length - 3}</span>{/if}
                <small>{commit.oid.slice(0, 7)} · {commit.author} · {when(commit.at)}</small>
              </span>
            </span>
          </button>
          {#if selected === commit.oid}
            <div class="detail">
              {#if detailError}<p class="inline-error" role="alert">{detailError}</p>
              {:else if !detail}<p class="empty" role="status">{tr("Reading commit…", "Lendo commit…")}</p>
              {:else}
                {#if detail.message.includes("\n")}<pre class="message">{detail.message.split("\n").slice(1).join("\n").trim()}</pre>{/if}
                <ul class="files">
                  {#each detail.files as file (file.path)}
                    <li><button type="button" class:active={filePath === file.path} onclick={() => void openFile(file.path)} title={file.path}>
                      <FileTypeIcon path={file.path} size={14} /><span class="path">{fileName(file.path)}</span>
                      <span class="stat">{#if file.added !== null}<b class="plus">+{file.added}</b><b class="minus">−{file.removed ?? 0}</b>{:else}<b>{tr("binary", "binário")}</b>{/if}</span>
                    </button></li>
                  {/each}
                  {#if !detail.files.length}<li class="empty">{tr("No file changes (a merge or an empty commit).", "Sem mudanças em arquivos (um merge ou um commit vazio).")}</li>{/if}
                </ul>
                {#if filePath}
                  <section class="diff" aria-label={tr("File changes", "Mudanças do arquivo")}>
                    {#if diffError}<p class="inline-error" role="alert">{diffError}</p>
                    {:else if !diff}<p class="empty" role="status">{tr("Reading changes…", "Lendo mudanças…")}</p>
                    {:else if diff.binary}<p class="empty">{tr("Binary file. Preview unavailable.", "Arquivo binário. Visualização indisponível.")}</p>
                    {:else}<div class="diff-code" use:transientScrollbar tabindex="0" role="textbox" aria-readonly="true" aria-multiline="true">{#each diffLines as line}<div class="diff-line {line.kind}"><span class="line-number" aria-hidden="true">{line.before}</span><span class="line-number" aria-hidden="true">{line.after}</span><code>{line.text || " "}</code></div>{/each}</div>{/if}
                  </section>
                {/if}
              {/if}
            </div>
          {/if}
        </li>
      {/each}
    </ol>
    {#if more}<button class="more" type="button" disabled={loading} onclick={() => void load(commits.length)}>{loading ? tr("Loading…", "Carregando…") : tr("Load more", "Carregar mais")}</button>{/if}
  {/if}
</div>

<style>
  .history { min-width: 0; display: grid; gap: 6px; }
  .rows { margin: 0; padding: 0; list-style: none; }
  .rows > li { border-radius: 8px; }
  .rows > li.selected { background: color-mix(in srgb, var(--workspace-accent) 7%, transparent); }
  .commit { width: 100%; min-width: 0; padding: 0 6px 0 0; display: flex; align-items: center; gap: 4px; border: 0; border-radius: 8px; color: inherit; background: transparent; font: inherit; text-align: left; cursor: pointer; }
  .commit:hover { background: color-mix(in srgb, var(--workspace-muted) 9%, transparent); }
  .graph { flex: 0 0 auto; overflow: visible; }
  .graph line, .graph path { stroke-width: 1.8; stroke-linecap: round; }
  .copy { min-width: 0; flex: 1; display: grid; gap: 0; }
  .meta-row { min-width: 0; display: flex; align-items: center; gap: 5px; }
  .copy strong { min-width: 0; overflow: hidden; color: var(--workspace-strong); font-size: 11px; font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
  .copy small { min-width: 0; flex: 1 1 auto; overflow: hidden; color: var(--workspace-muted); font-size: 9.5px; text-overflow: ellipsis; white-space: nowrap; }
  .ref { max-width: 45%; flex: 0 1 auto; overflow: hidden; padding: 0 6px; border: 1px solid color-mix(in srgb, var(--workspace-accent) 35%, transparent); border-radius: 999px; color: var(--workspace-accent); background: color-mix(in srgb, var(--workspace-accent) 9%, transparent); font-size: 9px; font-weight: 650; line-height: 1.6; text-overflow: ellipsis; white-space: nowrap; }
  .ref.current { color: #fff; background: var(--workspace-accent); }
  .detail { padding: 2px 8px 8px 22px; display: grid; gap: 6px; }
  .message { margin: 0; color: var(--workspace-text); font: inherit; font-size: 10.5px; line-height: 1.45; white-space: pre-wrap; overflow-wrap: anywhere; }
  .files { margin: 0; padding: 0; list-style: none; display: grid; gap: 1px; }
  .files button { width: 100%; min-width: 0; padding: 4px 6px; display: flex; align-items: center; gap: 7px; border: 0; border-radius: 6px; color: var(--workspace-text); background: transparent; font: inherit; font-size: 10.5px; text-align: left; cursor: pointer; }
  .files button:hover, .files button.active { background: color-mix(in srgb, var(--workspace-accent) 10%, transparent); }
  .path { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .stat { display: flex; gap: 6px; font-size: 9.5px; font-variant-numeric: tabular-nums; }
  .plus { color: var(--workspace-accent); font-weight: 650; }
  .minus { color: #c36c60; font-weight: 650; }
  .diff { border: 1px solid var(--workspace-line); border-radius: 8px; overflow: hidden; background: var(--workspace-subtle); }
  .diff-code { max-height: 300px; padding-block: 8px; overflow: auto; scrollbar-width: thin; scrollbar-color: transparent transparent; font-size: 10px; line-height: 1.8; }
  .diff-code:global(.is-scrolling) { scrollbar-color: var(--workspace-scroll-thumb) transparent; }
  .diff-line { min-width: max-content; display: grid; grid-template-columns: 26px 26px minmax(0, 1fr); white-space: pre; }
  .diff-line > code { padding: 0 10px 0 5px; }
  .line-number { color: var(--workspace-faint, var(--workspace-muted)); text-align: right; user-select: none; }
  .diff-code .added { background: color-mix(in srgb, var(--workspace-accent) 10%, transparent); color: var(--workspace-accent); }
  .diff-code .removed { background: rgba(190, 81, 69, .1); color: #c36c60; }
  .diff-code .hunk { color: var(--workspace-muted); background: color-mix(in srgb, var(--workspace-muted) 10%, transparent); }
  .diff-code .meta { color: var(--workspace-muted); }
  .empty, .inline-error { margin: 0; padding: 6px 4px; color: var(--workspace-muted); font-size: 10.5px; }
  .inline-error { color: #c0554f; }
  .more { height: 30px; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-text); background: transparent; font: inherit; font-size: 10.5px; cursor: pointer; }
  .more:disabled { opacity: .5; cursor: default; }
  .skeleton { display: grid; gap: 8px; padding: 6px 0; }
  .skeleton i { height: 22px; border-radius: 6px; background: color-mix(in srgb, var(--workspace-muted) 14%, transparent); }
  button:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 2px; }
</style>
