<script lang="ts">
  import LumeIcon from "$lib/LumeIcon.svelte";
  import { observeRepository, getSessionGitTools, gitFetch, gitPull, gitStash, gitStashApply, gitStashDrop, gitSwitch, repositoryError, type GitTools, type RepositorySnapshot } from "$lib/repositories";
  import type { AgentSession } from "$lib/domain";

  // Fetch, pull (only forward), stash and branch switching for the session's own folder. Nothing here merges, rebases
  // or pushes; whatever could lose work asks first, and what moves files waits while an agent works in the folder.
  let { session, repo, language = "en" }: { session: AgentSession; repo: RepositorySnapshot; language?: "en" | "pt-BR" } = $props();
  const tr = (english: string, portuguese: string) => (language === "pt-BR" ? portuguese : english);

  let tools = $state<GitTools>({ stashes: [], branches: [], busy: false });
  let working = $state("");
  let notice = $state<{ kind: "ok" | "error"; text: string } | null>(null);
  let panel = $state<"" | "branches" | "stashes">("");
  let switchTarget = $state("");
  let dropTarget = $state("");
  let generation = 0;

  const dirty = $derived(repo.staged + repo.modified + repo.conflicts > 0);
  const changes = $derived(repo.staged + repo.modified + repo.untracked);
  const agentWorking = $derived(tools.busy || session.status === "running" || session.status === "permission_required");
  const diverged = $derived(repo.ahead > 0 && repo.behind > 0);
  const canPull = $derived(!working && !agentWorking && !!repo.upstream && repo.behind > 0 && !diverged && repo.conflicts === 0);

  async function load() {
    const current = ++generation;
    try {
      const value = await getSessionGitTools(session.id);
      if (current === generation) tools = value;
    } catch { /* The bar keeps what it had; the next change of the repository reads it again. */ }
  }

  async function run(label: string, action: () => Promise<unknown>, done: string) {
    if (working) return;
    working = label;
    notice = null;
    try {
      await action();
      notice = { kind: "ok", text: done };
    } catch (reason) {
      notice = { kind: "error", text: repositoryError(String(reason).replace(/^Error:\s*/, ""), language === "pt-BR") };
    } finally {
      working = "";
      switchTarget = "";
      dropTarget = "";
      await observeRepository(session).refresh(true);
      await load();
    }
  }

  const fetchNow = () => run("fetch", () => gitFetch(session.id), tr("Up to date with the remote.", "Atualizado com o remote."));
  const pullNow = () => run("pull", () => gitPull(session.id), tr("Branch brought up to date.", "Branch atualizada."));
  const stashNow = (message = "") => run("stash", () => gitStash(session.id, message || `Lume: ${repo.branch}`), tr("Changes put in a stash.", "Mudanças guardadas em um stash."));
  const apply = (oid: string, index: number) => run("apply", () => gitStashApply(session.id, oid, index), tr("Stash applied. It is still in the list.", "Stash aplicado. Ele continua na lista."));
  const drop = (oid: string, index: number) => run("drop", () => gitStashDrop(session.id, oid, index), tr("Stash dropped.", "Stash descartado."));

  function pick(name: string) {
    if (name === repo.branch) { panel = ""; return; }
    if (dirty) { switchTarget = name; return; }
    void switchTo(name);
  }
  async function switchTo(name: string, stashFirst = false) {
    panel = "";
    await run("switch", async () => {
      if (stashFirst) await gitStash(session.id, tr(`Lume: before switching to ${name}`, `Lume: antes de trocar para ${name}`));
      await gitSwitch(session.id, name);
    }, stashFirst ? tr(`Switched to ${name}. Your changes are in the stash.`, `Trocou para ${name}. Suas mudanças estão no stash.`) : tr(`Switched to ${name}.`, `Trocou para ${name}.`));
  }

  function when(seconds: number) {
    const date = new Date(seconds * 1000);
    const days = (Date.now() - date.getTime()) / 86_400_000;
    return days < 1
      ? new Intl.DateTimeFormat(language, { hour: "2-digit", minute: "2-digit" }).format(date)
      : new Intl.DateTimeFormat(language, { day: "numeric", month: "short", ...(days > 300 ? { year: "2-digit" } : {}) }).format(date);
  }

  // The lists are read again when the repository moves (a commit, a pull) and when an agent starts or stops.
  $effect(() => {
    void repo.head; void repo.branch; void session.status;
    void load();
  });
</script>

<section class="git" aria-label={tr("Git actions", "Ações do Git")}>
  <div class="row">
    <button type="button" class="branch" aria-expanded={panel === "branches"} onclick={() => { panel = panel === "branches" ? "" : "branches"; switchTarget = ""; }}>
      <LumeIcon name="branch" size={13} /><span>{repo.branch === "(detached)" ? repo.head?.slice(0, 7) : repo.branch}</span><LumeIcon name="chevron-down" size={11} />
    </button>
    <span class="counts" title={repo.upstream ?? tr("No upstream", "Sem branch original")}>↑{repo.ahead} ↓{repo.behind}</span>
    <button type="button" onclick={() => void fetchNow()} disabled={!!working}>{working === "fetch" ? tr("Fetching…", "Buscando…") : tr("Fetch", "Fetch")}</button>
    <button type="button" class="primary" onclick={() => void pullNow()} disabled={!canPull} title={agentWorking ? tr("Paused while an agent is working here", "Pausado enquanto um agente trabalha aqui") : ""}>{working === "pull" ? tr("Pulling…", "Atualizando…") : repo.behind > 0 ? tr(`Pull ${repo.behind}`, `Receber ${repo.behind}`) : tr("Pull", "Receber")}</button>
    <button type="button" class="stash-toggle" aria-expanded={panel === "stashes"} onclick={() => { panel = panel === "stashes" ? "" : "stashes"; dropTarget = ""; }}>{tr("Stash", "Stash")}{#if tools.stashes.length}<b>{tools.stashes.length}</b>{/if}</button>
  </div>

  {#if agentWorking}<p class="note" role="status">{tr("An agent is working in this folder. Pull, stash and branch changes wait until it finishes.", "Um agente está trabalhando nesta pasta. Receber, stash e trocar de branch esperam ele terminar.")}</p>{/if}
  {#if diverged}<p class="note warn" role="status">{tr(`Diverged: ${repo.ahead} ahead, ${repo.behind} behind. Lume only moves a branch forward, so merge or rebase it in a terminal.`, `Divergiu: ${repo.ahead} à frente, ${repo.behind} atrás. O Lume só avança a branch, então faça o merge ou rebase em um terminal.`)}</p>{/if}
  {#if notice}<p class="note" class:warn={notice.kind === "error"} role={notice.kind === "error" ? "alert" : "status"}>{notice.text}</p>{/if}

  {#if panel === "branches"}
    <div class="list" role="listbox" aria-label={tr("Local branches", "Branches locais")}>
      {#each tools.branches as branch (branch.name)}
        <button type="button" role="option" aria-selected={branch.current} class:current={branch.current} disabled={!!working || agentWorking && !branch.current} onclick={() => pick(branch.name)}>
          <span class="check">{#if branch.current}<LumeIcon name="check" size={12} />{/if}</span>
          <span class="name">{branch.name}</span><small>{branch.upstream ?? tr("local only", "só local")} · {when(branch.at)}</small>
        </button>
        {#if switchTarget === branch.name}
          <div class="confirm" role="alertdialog" aria-label={tr("Switch with changes", "Trocar com mudanças")}>
            <p>{tr(`You have ${changes} uncommitted ${changes === 1 ? "change" : "changes"}. Put them in a stash and switch to ${branch.name}?`, `Você tem ${changes} ${changes === 1 ? "mudança não commitada" : "mudanças não commitadas"}. Guardar em um stash e trocar para ${branch.name}?`)}</p>
            <div><button type="button" class="primary" onclick={() => void switchTo(branch.name, true)}>{tr("Stash and switch", "Guardar e trocar")}</button><button type="button" onclick={() => (switchTarget = "")}>{tr("Cancel", "Cancelar")}</button></div>
          </div>
        {/if}
      {/each}
    </div>
  {/if}

  {#if panel === "stashes"}
    <div class="list" aria-label={tr("Stashes", "Stashes")}>
      <button type="button" class="stash-now" disabled={!!working || agentWorking || changes === 0} onclick={() => void stashNow()}>
        <LumeIcon name="plus" size={12} />{changes ? tr(`Stash ${changes} ${changes === 1 ? "change" : "changes"}`, `Guardar ${changes} ${changes === 1 ? "mudança" : "mudanças"}`) : tr("Nothing to stash", "Nada para guardar")}
      </button>
      {#each tools.stashes as stash (stash.oid)}
        <div class="stash">
          <span class="name" title={stash.message}>{stash.message.replace(/^(WIP on|On) [^:]*: /, "")}</span><small>{when(stash.at)}</small>
          <button type="button" disabled={!!working || agentWorking} onclick={() => void apply(stash.oid, stash.index)}>{tr("Apply", "Aplicar")}</button>
          <button type="button" class="danger" disabled={!!working} onclick={() => (dropTarget = dropTarget === stash.oid ? "" : stash.oid)}>{tr("Drop", "Descartar")}</button>
        </div>
        {#if dropTarget === stash.oid}
          <div class="confirm" role="alertdialog" aria-label={tr("Drop stash", "Descartar stash")}>
            <p>{tr("Drop this stash? Its changes cannot be recovered afterwards.", "Descartar este stash? Depois disso as mudanças não podem ser recuperadas.")}</p>
            <div><button type="button" class="danger solid" onclick={() => void drop(stash.oid, stash.index)}>{tr("Drop stash", "Descartar stash")}</button><button type="button" onclick={() => (dropTarget = "")}>{tr("Cancel", "Cancelar")}</button></div>
          </div>
        {/if}
      {/each}
      {#if !tools.stashes.length}<p class="empty">{tr("No stashes.", "Nenhum stash.")}</p>{/if}
    </div>
  {/if}
</section>

<style>
  .git { min-width: 0; margin-bottom: 8px; display: grid; gap: 6px; }
  .row { min-width: 0; display: flex; align-items: center; gap: 5px; flex-wrap: wrap; }
  button { height: 26px; padding: 0 9px; display: inline-flex; align-items: center; gap: 5px; border: 1px solid var(--workspace-line); border-radius: 7px; color: var(--workspace-text); background: transparent; font: inherit; font-size: 10.5px; cursor: pointer; }
  button:hover:not(:disabled) { background: color-mix(in srgb, var(--workspace-muted) 10%, transparent); }
  button:disabled { opacity: .45; cursor: default; }
  button:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 2px; }
  .branch { min-width: 0; max-width: 100%; flex: 1 1 60px; color: var(--workspace-strong); font-weight: 600; }
  .branch span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .counts { color: var(--workspace-muted); font-size: 10.5px; font-variant-numeric: tabular-nums; }
  .primary { color: #fff; border-color: transparent; background: var(--workspace-accent); }
  .primary:hover:not(:disabled) { background: color-mix(in srgb, var(--workspace-accent) 88%, #fff); }
  .primary:disabled { color: var(--workspace-muted); background: color-mix(in srgb, var(--workspace-muted) 14%, transparent); }
  .stash-toggle b { min-width: 16px; padding: 0 4px; border-radius: 999px; color: var(--workspace-accent); background: color-mix(in srgb, var(--workspace-accent) 14%, transparent); font-size: 9.5px; line-height: 16px; text-align: center; }
  .note { margin: 0; padding: 6px 8px; border-radius: 7px; color: var(--workspace-text); background: color-mix(in srgb, var(--workspace-muted) 9%, transparent); font-size: 10.5px; line-height: 1.4; }
  .note.warn { color: #d6a441; background: color-mix(in srgb, #d6a441 11%, transparent); }
  .list { min-width: 0; padding: 3px; display: grid; gap: 1px; border: 1px solid var(--workspace-line); border-radius: 9px; background: var(--workspace-subtle); }
  .list > button, .stash { min-width: 0; overflow: hidden; min-height: 28px; padding: 0 6px; display: flex; align-items: center; gap: 7px; border: 0; border-radius: 6px; font-size: 10.5px; text-align: left; }
  .list > button { height: auto; width: 100%; }
  .list > button.current .name { color: var(--workspace-accent); font-weight: 650; }
  .check { width: 12px; flex: 0 0 12px; color: var(--workspace-accent); }
  .name { min-width: 0; flex: 1; overflow: hidden; color: var(--workspace-strong); text-overflow: ellipsis; white-space: nowrap; }
  small { min-width: 0; max-width: 50%; flex: 0 1 auto; overflow: hidden; color: var(--workspace-muted); font-size: 9.5px; text-overflow: ellipsis; white-space: nowrap; }
  .stash { display: flex; }
  .stash button { height: 22px; padding: 0 7px; font-size: 10px; }
  .danger { color: #c36c60; }
  .danger.solid { color: #fff; border-color: transparent; background: #b9483e; }
  .stash-now { color: var(--workspace-accent); }
  .confirm { margin: 2px 4px 4px; padding: 8px; display: grid; gap: 7px; border: 1px solid var(--workspace-line); border-radius: 8px; background: var(--workspace-bg, transparent); }
  .confirm p { margin: 0; color: var(--workspace-text); font-size: 10.5px; line-height: 1.4; }
  .confirm div { display: flex; gap: 6px; }
  .empty { margin: 0; padding: 6px; color: var(--workspace-muted); font-size: 10.5px; }
</style>
