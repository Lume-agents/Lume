<script lang="ts">
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import LumeIcon, { type LumeIconName } from "$lib/LumeIcon.svelte";
  import { observeRepository, getSessionGitTools, gitFetch, gitPull, gitRebase, gitStash, gitStashApply, gitStashDrop, gitSwitch, repositoryError, type GitTools, type RepositorySnapshot } from "$lib/repositories";
  import type { AgentSession } from "$lib/domain";

  // Branch switching, fetch, pull (only forward), rebase and stash for the session's own folder, in two floating menus.
  // Nothing here merges or pushes; whatever could lose work asks first, a rebase that conflicts is cancelled, and what
  // moves files waits while an agent works in the folder.
  let { session, repo, language = "en" }: { session: AgentSession; repo: RepositorySnapshot; language?: "en" | "pt-BR" } = $props();
  const tr = (english: string, portuguese: string) => (language === "pt-BR" ? portuguese : english);

  type View = "main" | "rebase" | "stashes";
  let tools = $state<GitTools>({ stashes: [], branches: [], busy: false });
  let working = $state("");
  let notice = $state<{ kind: "ok" | "error"; text: string } | null>(null);
  let menu = $state<"" | "branches" | "actions">("");
  let view = $state<View>("main");
  let confirm = $state<{ kind: "switch" | "rebase" | "drop"; target: string; index?: number } | null>(null);
  let root = $state<HTMLElement | null>(null);
  let generation = 0;

  const dirty = $derived(repo.staged + repo.modified + repo.conflicts > 0);
  const changes = $derived(repo.staged + repo.modified + repo.untracked);
  const agentWorking = $derived(tools.busy || session.status === "running" || session.status === "permission_required");
  const diverged = $derived(repo.ahead > 0 && repo.behind > 0);
  const detached = $derived(repo.branch === "(detached)");
  const canPull = $derived(!agentWorking && !!repo.upstream && repo.behind > 0 && !diverged && repo.conflicts === 0);
  const canRebase = $derived(!agentWorking && !dirty && !detached);
  const otherBranches = $derived(tools.branches.filter((branch) => !branch.current));
  const pop = { y: -6, duration: 150, easing: cubicOut };

  async function load() {
    const current = ++generation;
    try {
      const value = await getSessionGitTools(session.id);
      if (current === generation) tools = value;
    } catch { /* The bar keeps what it had; the next change of the repository reads it again. */ }
  }

  function toggle(name: "branches" | "actions") {
    menu = menu === name ? "" : name;
    view = "main";
    confirm = null;
  }
  function close() { menu = ""; view = "main"; confirm = null; }

  async function run(label: string, action: () => Promise<unknown>, done: string) {
    if (working) return;
    working = label;
    notice = null;
    close();
    try {
      await action();
      notice = { kind: "ok", text: done };
    } catch (reason) {
      notice = { kind: "error", text: repositoryError(String(reason).replace(/^Error:\s*/, ""), language === "pt-BR") };
    } finally {
      working = "";
      await observeRepository(session).refresh(true);
      await load();
    }
  }

  const fetchNow = (prune: boolean) => run("fetch", () => gitFetch(session.id, prune), prune ? tr("Fetched and pruned the deleted remote branches.", "Fetch feito, e as branches apagadas no remote foram removidas.") : tr("Up to date with the remote.", "Atualizado com o remote."));
  const pullNow = () => run("pull", () => gitPull(session.id), tr("Branch brought up to date.", "Branch atualizada."));
  const rebaseNow = (onto: string) => run("rebase", () => gitRebase(session.id, onto), tr(`Rebased onto ${onto === "@{upstream}" ? repo.upstream : onto}.`, `Rebase feito sobre ${onto === "@{upstream}" ? repo.upstream : onto}.`));
  const stashNow = () => run("stash", () => gitStash(session.id, `Lume: ${repo.branch}`), tr("Changes put in a stash.", "Mudanças guardadas em um stash."));
  const apply = (oid: string, index: number) => run("apply", () => gitStashApply(session.id, oid, index), tr("Stash applied. It is still in the list.", "Stash aplicado. Ele continua na lista."));
  const drop = (oid: string, index: number) => run("drop", () => gitStashDrop(session.id, oid, index), tr("Stash dropped.", "Stash descartado."));

  function pick(name: string) {
    if (name === repo.branch) { close(); return; }
    if (dirty) { confirm = { kind: "switch", target: name }; return; }
    void switchTo(name);
  }
  function switchTo(name: string, stashFirst = false) {
    return run("switch", async () => {
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
  const pausedTitle = $derived(agentWorking ? tr("Paused while an agent is working here", "Pausado enquanto um agente trabalha aqui") : undefined);

  // The lists are read again when the repository moves (a commit, a pull) and when an agent starts or stops.
  const reloadKey = $derived(`${session.id}|${repo.head}|${repo.branch}|${session.status}`);
  $effect(() => {
    void reloadKey;
    void load();
  });
</script>

<svelte:window
  onpointerdown={(event) => { if (menu && root && !root.contains(event.target as Node)) close(); }}
  onkeydown={(event) => { if (menu && event.key === "Escape") { event.stopPropagation(); close(); } }}
/>

{#snippet item(icon: LumeIconName, label: string, onclick: () => void, options: { disabled?: boolean; hint?: string; next?: boolean; danger?: boolean; title?: string } = {})}
  <button type="button" role="menuitem" class="item" class:danger={options.danger} disabled={options.disabled || !!working} title={options.title} {onclick}>
    <LumeIcon name={icon} size={14} /><span>{label}</span>{#if options.hint}<small>{options.hint}</small>{/if}{#if options.next}<span class="next"><LumeIcon name="arrow-right" size={11} /></span>{/if}
  </button>
{/snippet}

{#snippet confirmBox(text: string, action: string, onyes: () => void, danger = false)}
  <div class="confirm" role="alertdialog" aria-label={action} transition:fly={pop}>
    <p>{text}</p>
    <div><button type="button" class="solid" class:danger onclick={onyes}>{action}</button><button type="button" class="plain" onclick={() => (confirm = null)}>{tr("Cancel", "Cancelar")}</button></div>
  </div>
{/snippet}

<section class="git" bind:this={root} aria-label={tr("Git actions", "Ações do Git")}>
  <div class="bar">
    <button type="button" class="select" aria-haspopup="listbox" aria-expanded={menu === "branches"} onclick={() => toggle("branches")}>
      <LumeIcon name="branch" size={13} /><span>{detached ? repo.head?.slice(0, 7) : repo.branch}</span><span class="caret" class:open={menu === "branches"}><LumeIcon name="chevron-down" size={11} /></span>
    </button>
    <span class="counts" title={repo.upstream ?? tr("No upstream", "Sem branch original")}><LumeIcon name="arrow-up" size={11} />{repo.ahead}<LumeIcon name="arrow-down" size={11} />{repo.behind}</span>
    <button type="button" class="icon" title={tr("Pull", "Receber")} aria-label={tr("Pull", "Receber")} disabled={!canPull || !!working} onclick={() => void pullNow()}><LumeIcon name="arrow-down" size={14} />{#if repo.behind > 0 && !diverged}<b>{repo.behind}</b>{/if}</button>
    <button type="button" class="icon" class:spinning={!!working} aria-haspopup="menu" aria-expanded={menu === "actions"} title={tr("Git actions", "Ações do Git")} aria-label={tr("Git actions", "Ações do Git")} onclick={() => toggle("actions")}><LumeIcon name={working ? "refresh" : "menu"} size={15} /></button>

    {#if menu === "branches"}
      <div class="menu branches" role="listbox" aria-label={tr("Local branches", "Branches locais")} transition:fly={pop}>
        <p class="menu-title">{tr("Switch branch", "Trocar de branch")}</p>
        {#each tools.branches as branch (branch.name)}
          <button type="button" role="option" class="item" aria-selected={branch.current} class:current={branch.current} disabled={!!working || (agentWorking && !branch.current)} title={pausedTitle} onclick={() => pick(branch.name)}>
            <span class="check">{#if branch.current}<LumeIcon name="check" size={12} />{/if}</span>
            <span class="name">{branch.name}</span><small>{branch.upstream ?? tr("local only", "só local")} · {when(branch.at)}</small>
          </button>
          {#if confirm?.kind === "switch" && confirm.target === branch.name}
            {@render confirmBox(tr(`You have ${changes} uncommitted ${changes === 1 ? "change" : "changes"}. Put them in a stash and switch to ${branch.name}?`, `Você tem ${changes} ${changes === 1 ? "mudança não commitada" : "mudanças não commitadas"}. Guardar em um stash e trocar para ${branch.name}?`), tr("Stash and switch", "Guardar e trocar"), () => void switchTo(branch.name, true))}
          {/if}
        {/each}
      </div>
    {/if}

    {#if menu === "actions"}
      <div class="menu actions" role="menu" aria-label={tr("Git actions", "Ações do Git")} transition:fly={pop}>
        {#if view === "main"}
          {@render item("refresh", tr("Fetch", "Fetch"), () => void fetchNow(false))}
          {@render item("trash", tr("Fetch and prune", "Fetch e prune"), () => void fetchNow(true), { title: tr("Also removes the remote branches deleted on the remote", "Também remove as branches apagadas no remote") })}
          {@render item("arrow-down", tr("Pull (fast-forward)", "Receber (fast-forward)"), () => void pullNow(), { disabled: !canPull, hint: repo.behind ? `↓${repo.behind}` : "", title: pausedTitle })}
          <hr />
          {@render item("rebase", tr("Rebase onto upstream", "Rebase sobre a original"), () => (confirm = { kind: "rebase", target: "@{upstream}" }), { disabled: !canRebase || !repo.upstream, hint: repo.upstream ?? "", title: pausedTitle ?? (dirty ? tr("Commit or stash your changes first", "Faça commit ou guarde suas mudanças antes") : undefined) })}
          {@render item("rebase", tr("Rebase onto…", "Rebase sobre…"), () => (view = "rebase"), { disabled: !canRebase || !otherBranches.length, next: true })}
          {#if confirm?.kind === "rebase"}
            {@render confirmBox(tr(`Rebase ${repo.branch} onto ${repo.upstream}? Its commits get new hashes; if they were pushed, the branch will need a force push. A conflict cancels the rebase.`, `Fazer rebase de ${repo.branch} sobre ${repo.upstream}? Os commits ganham hashes novos; se já foram enviados, a branch vai precisar de force push. Um conflito cancela o rebase.`), tr("Rebase", "Fazer rebase"), () => void rebaseNow("@{upstream}"))}
          {/if}
          <hr />
          {@render item("stash", changes ? tr(`Stash ${changes} ${changes === 1 ? "change" : "changes"}`, `Guardar ${changes} ${changes === 1 ? "mudança" : "mudanças"}`) : tr("Nothing to stash", "Nada para guardar"), () => void stashNow(), { disabled: agentWorking || changes === 0, title: pausedTitle })}
          {@render item("stash", tr("Stashes", "Stashes"), () => (view = "stashes"), { hint: String(tools.stashes.length), next: true })}
        {:else}
          <button type="button" class="back" onclick={() => { view = "main"; confirm = null; }}><LumeIcon name="arrow-right" size={11} /><span>{view === "rebase" ? tr("Rebase onto", "Rebase sobre") : tr("Stashes", "Stashes")}</span></button>
          {#if view === "rebase"}
            {#each otherBranches as branch (branch.name)}
              {@render item("branch", branch.name, () => (confirm = { kind: "rebase", target: branch.name }), { hint: when(branch.at) })}
              {#if confirm?.kind === "rebase" && confirm.target === branch.name}
                {@render confirmBox(tr(`Rebase ${repo.branch} onto ${branch.name}? Its commits get new hashes, and a conflict cancels the rebase.`, `Fazer rebase de ${repo.branch} sobre ${branch.name}? Os commits ganham hashes novos, e um conflito cancela o rebase.`), tr("Rebase", "Fazer rebase"), () => void rebaseNow(branch.name))}
              {/if}
            {/each}
          {:else}
            {#each tools.stashes as stash (stash.oid)}
              <div class="stash">
                <span class="name" title={stash.message}>{stash.message.replace(/^(WIP on|On) [^:]*: /, "")}</span><small>{when(stash.at)}</small>
                <button type="button" class="mini" disabled={!!working || agentWorking} title={pausedTitle} onclick={() => void apply(stash.oid, stash.index)}><LumeIcon name="download" size={11} />{tr("Apply", "Aplicar")}</button>
                <button type="button" class="mini danger" disabled={!!working} title={tr("Drop", "Descartar")} aria-label={tr("Drop", "Descartar")} onclick={() => (confirm = confirm?.target === stash.oid ? null : { kind: "drop", target: stash.oid, index: stash.index })}><LumeIcon name="trash" size={11} /></button>
              </div>
              {#if confirm?.kind === "drop" && confirm.target === stash.oid}
                {@render confirmBox(tr("Drop this stash? Its changes cannot be recovered afterwards.", "Descartar este stash? Depois disso as mudanças não podem ser recuperadas."), tr("Drop stash", "Descartar stash"), () => void drop(stash.oid, stash.index), true)}
              {/if}
            {/each}
            {#if !tools.stashes.length}<p class="empty">{tr("No stashes.", "Nenhum stash.")}</p>{/if}
          {/if}
        {/if}
      </div>
    {/if}
  </div>

  {#if agentWorking}<p class="note" role="status" transition:fly={pop}>{tr("An agent is working in this folder. Pull, rebase, stash and branch changes wait until it finishes.", "Um agente está trabalhando nesta pasta. Receber, rebase, stash e trocar de branch esperam ele terminar.")}</p>{/if}
  {#if diverged}<p class="note warn" role="status" transition:fly={pop}>{tr(`Diverged: ${repo.ahead} ahead, ${repo.behind} behind. Pull only moves a branch forward; use Rebase onto upstream to put your commits on top.`, `Divergiu: ${repo.ahead} à frente, ${repo.behind} atrás. Receber só avança a branch; use Rebase sobre a original para colocar seus commits por cima.`)}</p>{/if}
  {#if notice}<p class="note" class:warn={notice.kind === "error"} role={notice.kind === "error" ? "alert" : "status"} transition:fly={pop}>{notice.text}<button type="button" class="dismiss" aria-label={tr("Dismiss", "Fechar")} onclick={() => (notice = null)}><LumeIcon name="close" size={11} /></button></p>{/if}
</section>

<style>
  .git { min-width: 0; margin-bottom: 8px; display: grid; gap: 6px; }
  .bar { position: relative; min-width: 0; display: flex; align-items: center; gap: 5px; }
  button { font: inherit; cursor: pointer; }
  button:disabled { opacity: .45; cursor: default; }
  button:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 2px; }
  .select { min-width: 0; flex: 1 1 auto; height: 28px; padding: 0 8px; display: flex; align-items: center; gap: 6px; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-strong); background: color-mix(in srgb, var(--workspace-muted) 6%, transparent); font-size: 11px; font-weight: 600; transition: border-color .15s ease, background-color .15s ease; }
  .select:hover, .select[aria-expanded="true"] { border-color: color-mix(in srgb, var(--workspace-accent) 45%, transparent); }
  .select > span:not(.caret) { min-width: 0; flex: 1; overflow: hidden; text-align: left; text-overflow: ellipsis; white-space: nowrap; }
  .caret { display: inline-flex; color: var(--workspace-muted); transition: transform .2s ease; }
  .caret.open { transform: rotate(180deg); }
  .counts { display: inline-flex; align-items: center; gap: 1px; color: var(--workspace-muted); font-size: 10.5px; font-variant-numeric: tabular-nums; }
  .counts :global(svg:last-of-type) { margin-left: 4px; }
  .icon { height: 28px; min-width: 28px; padding: 0 7px; display: inline-flex; align-items: center; justify-content: center; gap: 3px; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-text); background: transparent; transition: background-color .15s ease, border-color .15s ease; }
  .icon:hover:not(:disabled), .icon[aria-expanded="true"] { background: color-mix(in srgb, var(--workspace-muted) 10%, transparent); }
  .icon b { color: var(--workspace-accent); font-size: 10px; }
  .spinning :global(svg) { animation: spin 1s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .menu { position: absolute; top: calc(100% + 5px); z-index: 40; max-height: 340px; padding: 4px; overflow: auto; display: grid; gap: 1px; border: 1px solid var(--workspace-line); border-radius: 10px; background: var(--workspace-raised, #262a28); background-clip: padding-box; box-shadow: 0 16px 38px rgba(12, 20, 16, .28); scrollbar-width: thin; }
  .menu.branches { left: 0; right: 0; }
  .menu.actions { right: 0; width: min(270px, 100%); }
  .menu-title { margin: 2px 6px 4px; color: var(--workspace-muted); font-size: 9.5px; font-weight: 650; letter-spacing: .04em; text-transform: uppercase; }
  .item { width: 100%; min-width: 0; min-height: 30px; padding: 0 8px; display: flex; align-items: center; gap: 8px; border: 0; border-radius: 7px; color: var(--workspace-text); background: transparent; font-size: 11px; text-align: left; transition: background-color .12s ease; }
  .item:hover:not(:disabled) { background: color-mix(in srgb, var(--workspace-accent) 11%, transparent); }
  .item > span:not(.next):not(.check) { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .item :global(svg) { flex: 0 0 auto; color: var(--workspace-muted); }
  .item.danger, .item.danger :global(svg) { color: #d0625c; }
  .item.current .name { color: var(--workspace-accent); font-weight: 650; }
  .next { display: inline-flex; }
  .check { width: 12px; flex: 0 0 12px; color: var(--workspace-accent); }
  .check :global(svg) { color: var(--workspace-accent) !important; }
  .name { min-width: 0; flex: 1; overflow: hidden; color: var(--workspace-strong); text-overflow: ellipsis; white-space: nowrap; }
  small { min-width: 0; max-width: 50%; flex: 0 1 auto; overflow: hidden; color: var(--workspace-muted); font-size: 9.5px; text-overflow: ellipsis; white-space: nowrap; }
  hr { width: calc(100% - 12px); height: 1px; margin: 3px 6px; border: 0; background: var(--workspace-line); }
  .back { height: 28px; padding: 0 8px; display: flex; align-items: center; gap: 7px; border: 0; border-radius: 7px; color: var(--workspace-muted); background: transparent; font-size: 10.5px; font-weight: 650; }
  .back :global(svg) { transform: rotate(180deg); }
  .back:hover { color: var(--workspace-text); }
  .stash { min-width: 0; min-height: 30px; padding: 0 4px 0 8px; display: flex; align-items: center; gap: 6px; border-radius: 7px; font-size: 11px; }
  .stash:hover { background: color-mix(in srgb, var(--workspace-muted) 7%, transparent); }
  .mini { height: 22px; padding: 0 6px; display: inline-flex; align-items: center; gap: 4px; border: 1px solid var(--workspace-line); border-radius: 6px; color: var(--workspace-text); background: transparent; font-size: 10px; }
  .mini.danger { color: #d0625c; }
  .confirm { margin: 2px 2px 4px; padding: 8px; display: grid; gap: 7px; border: 1px solid var(--workspace-line); border-radius: 8px; background: color-mix(in srgb, var(--workspace-muted) 6%, transparent); }
  .confirm p { margin: 0; color: var(--workspace-text); font-size: 10.5px; line-height: 1.4; }
  .confirm div { display: flex; gap: 6px; }
  .solid, .plain { height: 24px; padding: 0 9px; border: 1px solid transparent; border-radius: 6px; font-size: 10.5px; }
  .solid { color: #fff; background: var(--workspace-accent); }
  .solid.danger { background: #b9483e; }
  .plain { border-color: var(--workspace-line); color: var(--workspace-text); background: transparent; }
  .note { position: relative; margin: 0; padding: 6px 26px 6px 8px; border-radius: 7px; color: var(--workspace-text); background: color-mix(in srgb, var(--workspace-muted) 9%, transparent); font-size: 10.5px; line-height: 1.4; }
  .note.warn { color: #d6a441; background: color-mix(in srgb, #d6a441 11%, transparent); }
  .dismiss { position: absolute; top: 5px; right: 5px; width: 18px; height: 18px; display: grid; place-items: center; border: 0; border-radius: 5px; color: inherit; background: transparent; opacity: .7; }
  .dismiss:hover { opacity: 1; }
  .empty { margin: 0; padding: 6px 8px; color: var(--workspace-muted); font-size: 10.5px; }
</style>
