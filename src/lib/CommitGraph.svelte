<script lang="ts">
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import AuthorAvatar from "$lib/AuthorAvatar.svelte";
  import FileStatusMark from "$lib/FileStatusMark.svelte";
  import FileTypeIcon from "$lib/FileTypeIcon.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import { layoutGraph, graphWidth } from "$lib/commitGraph";
  import { commitStatus } from "$lib/fileStatus";
  import { getAuthorAvatars, getSessionCommit, getSessionCommitDiff, getSessionHistory, repositoryError, type CommitDetail, type HistoryCommit, type RepositoryDiff } from "$lib/repositories";
  import { transientScrollbar } from "$lib/transientScrollbar";

  // The repository's history as a graph, like the one in VS Code: lines for branches and merges, the branches and tags
  // at their commits, each author's picture, and everything about a commit one click away. `readonly` shows only the
  // latest commits, and a click hands the commit to `onPick`.
  type Props = {
    sessionId: string;
    language?: "en" | "pt-BR";
    head?: string | null;
    branch?: string;
    readonly?: boolean;
    limit?: number;
    focus?: string | null;
    onPick?: (oid: string) => void;
    onOpenFile?: (commit: { oid: string; subject: string }, path: string) => void;
  };
  let { sessionId, language = "en", head = null, branch = "", readonly = false, limit = 0, focus = null, onPick, onOpenFile }: Props = $props();
  const tr = (english: string, portuguese: string) => (language === "pt-BR" ? portuguese : english);

  const ROW = 40;
  const LANE = 14;
  const colors = ["#4daa77", "#4d99cc", "#d6a441", "#b57edc", "#d0625c", "#3fb8af", "#e0869a", "#8f9e5a"];

  let commits = $state<HistoryCommit[]>([]);
  let avatars = $state<Record<string, string>>({});
  let loading = $state(true);
  let more = $state(true);
  let error = $state("");
  let selected = $state<string | null>(null);
  let detail = $state<CommitDetail | null>(null);
  let detailError = $state("");
  let filePath = $state("");
  let diff = $state<RepositoryDiff | null>(null);
  let diffError = $state("");
  let copied = $state(false);
  let appliedFocus = "";
  let generation = 0;
  let list = $state<HTMLOListElement | null>(null);

  const shown = $derived(limit ? commits.slice(0, limit) : commits);
  const rows = $derived(layoutGraph(shown));
  const lanes = $derived(graphWidth(rows));
  const graphPixels = $derived(Math.max(lanes, 1) * LANE + 8);
  const x = (lane: number) => 8 + lane * LANE + LANE / 2 - 4;
  const color = (lane: number) => colors[lane % colors.length];
  // The lanes that go on below a commit, drawn beside its open details so the graph does not break there.
  const below = (index: number) => { const next = rows[index + 1]; return next ? [...new Set([...next.through, ...next.from])] : []; };
  const totals = $derived(detail ? detail.files.reduce((sum, file) => ({ added: sum.added + (file.added ?? 0), removed: sum.removed + (file.removed ?? 0) }), { added: 0, removed: 0 }) : null);

  const clean = (reason: unknown) => repositoryError(String(reason).replace(/^Error:\s*/, ""), language === "pt-BR");

  async function load(skip: number) {
    const current = generation;
    loading = true;
    error = "";
    try {
      const page = await getSessionHistory(sessionId, skip);
      if (current !== generation) return;
      commits = skip === 0 ? page : [...commits, ...page];
      more = page.length >= 200;
      void loadAvatars(page, current);
    } catch (reason) {
      if (current === generation) error = clean(reason);
    } finally {
      if (current === generation) loading = false;
    }
  }

  async function loadAvatars(page: HistoryCommit[], current: number) {
    const authors = new Map<string, string>();
    for (const commit of page) if (commit.email && !avatars[commit.email] && !authors.has(commit.email)) authors.set(commit.email, commit.oid);
    if (!authors.size) return;
    try {
      const found = await getAuthorAvatars(sessionId, [...authors].slice(0, 60));
      if (current === generation) avatars = { ...avatars, ...found };
    } catch { /* Initials stay. */ }
  }

  async function select(oid: string) {
    if (readonly) { onPick?.(oid); return; }
    if (selected === oid) { selected = null; detail = null; filePath = ""; diff = null; return; }
    selected = oid;
    detail = null;
    detailError = "";
    filePath = "";
    diff = null;
    copied = false;
    try {
      const value = await getSessionCommit(sessionId, oid);
      if (selected === oid) detail = value;
    } catch (reason) {
      if (selected === oid) detailError = clean(reason);
    }
  }

  async function openFile(commit: HistoryCommit, path: string) {
    filePath = path;
    if (onOpenFile) { onOpenFile({ oid: commit.oid, subject: commit.subject }, path); return; }
    diff = null;
    diffError = "";
    try {
      const value = await getSessionCommitDiff(sessionId, commit.oid, path);
      if (filePath === path && selected === commit.oid) diff = value;
    } catch (reason) {
      if (filePath === path) diffError = clean(reason);
    }
  }

  async function copyHash(oid: string) {
    try { await navigator.clipboard.writeText(oid); copied = true; setTimeout(() => (copied = false), 1400); } catch { /* Clipboard unavailable. */ }
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
  const fullDate = (seconds: number) => new Intl.DateTimeFormat(language, { dateStyle: "medium", timeStyle: "short" }).format(new Date(seconds * 1000));
  const fileName = (path: string) => path.split("/").at(-1) ?? path;
  const fileFolder = (path: string) => path.split("/").slice(0, -1).join("/");
  const body = (message: string) => message.split("\n").slice(1).join("\n").trim();
  const isHead = (oid: string) => !!head && oid.startsWith(head);
  const refName = (name: string) => name.replace(/^tag: /, "");
  const loaded = (oid: string) => commits.some((commit) => commit.oid === oid);

  // The history is read again when the repository's head moves (a commit made from the chat, a pull).
  $effect(() => {
    void head;
    generation += 1;
    void load(0);
  });
  // A commit picked elsewhere (the overview) opens here once it is loaded.
  $effect(() => {
    if (!focus || appliedFocus === focus || !commits.some((commit) => commit.oid === focus)) return;
    appliedFocus = focus;
    if (selected !== focus) void select(focus);
    requestAnimationFrame(() => list?.querySelector(`[data-oid="${focus}"]`)?.scrollIntoView({ block: "nearest", behavior: "smooth" }));
  });
</script>

<div class="history" class:readonly>
  {#if loading && !commits.length}
    <div class="skeleton" role="status" aria-label={tr("Loading history", "Carregando histórico")}><i></i><i></i><i></i></div>
  {:else if error}
    <p class="inline-error" role="alert">{error}</p>
  {:else if !commits.length}
    <p class="empty">{tr("This repository has no commits yet.", "Este repositório ainda não tem commits.")}</p>
  {:else}
    <ol class="rows" bind:this={list} aria-label={tr("Commit history", "Histórico de commits")}>
      {#each shown as commit, index (commit.oid)}
        {@const row = rows[index]}
        {@const open = selected === commit.oid && !readonly}
        <li class:selected={open} data-oid={commit.oid}>
          <div class="graph-col" aria-hidden="true">
            <svg class="graph" width={graphPixels} height={ROW} viewBox={`0 0 ${graphPixels} ${ROW}`}>
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
            {#if open}
              <!-- The lanes that go on below the commit run beside its open details. -->
              <div class="gutter" style:width="{graphPixels}px">{#each below(index) as lane (lane)}<i style:left="{x(lane) - 0.9}px" style:background={color(lane)}></i>{/each}</div>
            {/if}
          </div>
          <div class="body">
            <button type="button" class="commit" class:open aria-expanded={readonly ? undefined : open} title={readonly ? tr("Open in History", "Abrir no Histórico") : undefined} onclick={() => void select(commit.oid)}>
              <AuthorAvatar name={commit.author} email={commit.email} url={avatars[commit.email]} size={20} />
              <span class="copy">
                <strong>{commit.subject}</strong>
                <span class="meta-row">
                  {#each (open ? commit.refs : commit.refs.slice(0, 3)) as name (name)}<span class="ref" class:current={name === branch && isHead(commit.oid)} class:tag={name.startsWith("tag: ")}>{refName(name)}</span>{/each}
                  {#if !open && commit.refs.length > 3}<span class="ref">+{commit.refs.length - 3}</span>{/if}
                  <small>{#if open}{commit.author} · {fullDate(commit.at)}{:else}{commit.oid.slice(0, 7)} · {when(commit.at)}{/if}</small>
                </span>
              </span>
              {#if !readonly}<span class="chevron" class:open aria-hidden="true"><LumeIcon name="chevron-down" size={12} /></span>{/if}
            </button>
            {#if open}
              <div class="detail" transition:slide={{ duration: 200, easing: cubicOut }}>
                {#if detailError}<p class="inline-error" role="alert">{detailError}</p>
                {:else if !detail}<p class="empty" role="status">{tr("Reading commit…", "Lendo commit…")}</p>
                {:else}
                  {#if detail.committer !== detail.author || detail.committedAt !== detail.at}
                    <p class="committed">{tr("Committed by", "Commit feito por")} <b>{detail.committer}</b> · {fullDate(detail.committedAt)}</p>
                  {/if}
                  <dl class="facts">
                    <dt>{tr("Commit", "Commit")}</dt>
                    <dd class="hash"><code>{detail.oid}</code><button type="button" class="copy-hash" title={tr("Copy hash", "Copiar hash")} aria-label={tr("Copy hash", "Copiar hash")} onclick={() => void copyHash(detail!.oid)}><LumeIcon name={copied ? "check" : "copy"} size={12} /></button></dd>
                    {#if detail.parents.length}
                      <dt>{detail.parents.length > 1 ? tr("Parents", "Pais") : tr("Parent", "Pai")}</dt>
                      <dd>{#each detail.parents as parent (parent)}<button type="button" class="parent" title={parent} disabled={!loaded(parent)} onclick={() => { void select(parent); requestAnimationFrame(() => list?.querySelector(`[data-oid="${parent}"]`)?.scrollIntoView({ block: "nearest", behavior: "smooth" })); }}>{parent.slice(0, 7)}</button>{/each}</dd>
                    {/if}
                    {#if totals}
                      <dt>{tr("Changes", "Mudanças")}</dt>
                      <dd>{detail.files.length} {detail.files.length === 1 ? tr("file", "arquivo") : tr("files", "arquivos")} · <b class="plus">+{totals.added}</b> <b class="minus">−{totals.removed}</b></dd>
                    {/if}
                  </dl>
                  {#if body(detail.message)}<pre class="message">{body(detail.message)}</pre>{/if}
                  <ul class="files">
                    {#each detail.files as file (file.path)}
                      <li><button type="button" class:active={filePath === file.path} onclick={() => void openFile(commit, file.path)} title={file.path}>
                        <FileStatusMark status={commitStatus(file.status)} {language} />
                        <FileTypeIcon path={file.path} size={16} />
                        <span class="path"><span>{fileName(file.path)}</span>{#if fileFolder(file.path)}<small>{fileFolder(file.path)}</small>{/if}</span>
                        <span class="stat">{#if file.added !== null}<b class="plus">+{file.added}</b><b class="minus">−{file.removed ?? 0}</b>{:else}<b>{tr("binary", "binário")}</b>{/if}</span>
                      </button></li>
                    {/each}
                    {#if !detail.files.length}<li class="empty">{tr("No file changes (a merge or an empty commit).", "Sem mudanças em arquivos (um merge ou um commit vazio).")}</li>{/if}
                  </ul>
                  {#if filePath && !onOpenFile}
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
          </div>
        </li>
      {/each}
    </ol>
    {#if more && !readonly}<button class="more" type="button" disabled={loading} onclick={() => void load(commits.length)}>{loading ? tr("Loading…", "Carregando…") : tr("Load more", "Carregar mais")}</button>{/if}
  {/if}
</div>

<style>
  .history { min-width: 0; display: grid; gap: 6px; }
  .rows { margin: 0; padding: 0; list-style: none; }
  .rows > li { display: flex; align-items: stretch; border-radius: 8px; transition: background-color .18s ease; }
  .rows > li.selected { background: color-mix(in srgb, var(--workspace-accent) 7%, transparent); }
  .graph-col { flex: 0 0 auto; display: flex; flex-direction: column; }
  .graph { flex: 0 0 auto; overflow: visible; }
  .graph line, .graph path { stroke-width: 1.8; stroke-linecap: round; }
  .gutter { position: relative; flex: 1 1 auto; }
  .gutter i { position: absolute; top: 0; bottom: 0; width: 1.8px; border-radius: 1px; }
  .body { min-width: 0; flex: 1; }
  .commit { width: 100%; min-width: 0; min-height: 40px; padding: 0 6px 0 2px; display: flex; align-items: center; gap: 7px; border: 0; border-radius: 8px; color: inherit; background: transparent; font: inherit; text-align: left; cursor: pointer; transition: background-color .15s ease; }
  .commit:hover { background: color-mix(in srgb, var(--workspace-muted) 9%, transparent); }
  .commit.open { padding-block: 8px 4px; align-items: flex-start; }
  .commit.open .copy strong { white-space: normal; overflow-wrap: anywhere; line-height: 1.4; }
  .commit.open .meta-row { flex-wrap: wrap; margin-top: 3px; }
  .commit.open :global(.avatar) { margin-top: 1px; }
  .copy { min-width: 0; flex: 1; display: grid; gap: 0; }
  .meta-row { min-width: 0; display: flex; align-items: center; gap: 5px; }
  .copy strong { min-width: 0; overflow: hidden; color: var(--workspace-strong); font-size: 11px; font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
  .copy small { min-width: 0; flex: 1 1 auto; overflow: hidden; color: var(--workspace-muted); font-size: 9.5px; text-overflow: ellipsis; white-space: nowrap; }
  .chevron { flex: 0 0 auto; display: inline-flex; color: var(--workspace-muted); opacity: .6; transition: transform .2s ease, opacity .15s ease; }
  .commit:hover .chevron { opacity: 1; }
  .chevron.open { transform: rotate(180deg); opacity: 1; }
  .ref { max-width: 45%; flex: 0 1 auto; overflow: hidden; padding: 0 6px; border: 1px solid color-mix(in srgb, var(--workspace-accent) 35%, transparent); border-radius: 999px; color: var(--workspace-accent); background: color-mix(in srgb, var(--workspace-accent) 9%, transparent); font-size: 9px; font-weight: 650; line-height: 1.6; text-overflow: ellipsis; white-space: nowrap; }
  .ref.tag { color: #d6a441; border-color: rgba(214, 164, 65, .4); background: rgba(214, 164, 65, .1); }
  .ref.current { color: #fff; background: var(--workspace-accent); }
  .detail { min-width: 0; padding: 2px 8px 12px 2px; display: grid; gap: 9px; }
  .committed { margin: 0; overflow: hidden; color: var(--workspace-muted); font-size: 10px; text-overflow: ellipsis; white-space: nowrap; }
  .committed b { color: var(--workspace-text); font-weight: 600; }
  .facts { margin: 0; display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 4px 10px; align-items: center; font-size: 10px; }
  .facts dt { color: var(--workspace-muted); }
  .facts dd { min-width: 0; margin: 0; display: flex; align-items: center; gap: 5px; flex-wrap: wrap; color: var(--workspace-text); }
  .facts dd.hash { flex-wrap: nowrap; }
  .facts code { min-width: 0; flex: 1 1 auto; overflow: hidden; color: var(--workspace-text); font-size: 9.5px; text-overflow: ellipsis; white-space: nowrap; }
  .copy-hash, .parent { height: 20px; padding: 0 6px; display: inline-flex; align-items: center; border: 1px solid var(--workspace-line); border-radius: 6px; color: var(--workspace-text); background: transparent; font: inherit; font-size: 9.5px; cursor: pointer; }
  .parent { font-family: ui-monospace, monospace; }
  .copy-hash:hover, .parent:hover { border-color: color-mix(in srgb, var(--workspace-accent) 45%, transparent); color: var(--workspace-accent); }
  .message { margin: 0; padding: 8px 10px; border-radius: 8px; color: var(--workspace-text); background: color-mix(in srgb, var(--workspace-muted) 7%, transparent); font: inherit; font-size: 10.5px; line-height: 1.5; white-space: pre-wrap; overflow-wrap: anywhere; }
  .files { margin: 0; padding: 0; list-style: none; display: grid; gap: 2px; }
  .files button { width: 100%; min-width: 0; min-height: 38px; padding: 5px 8px; display: flex; align-items: center; gap: 8px; border: 1px solid transparent; border-radius: 7px; color: var(--workspace-text); background: transparent; font: inherit; font-size: 11.5px; text-align: left; cursor: pointer; transition: background-color .15s ease, border-color .15s ease; }
  .files button:hover { background: color-mix(in srgb, var(--workspace-muted) 9%, transparent); }
  .files button.active { border-color: color-mix(in srgb, var(--workspace-accent) 55%, transparent); background: color-mix(in srgb, var(--workspace-accent) 10%, transparent); }
  .path { min-width: 0; flex: 1; display: grid; gap: 1px; }
  .path > span, .path small { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .path small { color: var(--workspace-muted); font-size: 9.5px; }
  .stat { display: flex; gap: 6px; font-size: 10.5px; font-variant-numeric: tabular-nums; }
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
