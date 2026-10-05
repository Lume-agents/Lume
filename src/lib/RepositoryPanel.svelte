<script lang="ts">
  import { getContext, untrack } from "svelte";
  import { createSystemBannerSource, SYSTEM_BANNER_CONTEXT, type SystemBannerReporter, type SystemBannerNotice } from "$lib/systemBannerContext";
  import FileTypeIcon from "$lib/FileTypeIcon.svelte";
  import GitHubActivity from "$lib/GitHubActivity.svelte";
  import LumeIcon, { type LumeIconName } from "$lib/LumeIcon.svelte";
  import BrandIcon from "$lib/BrandIcon.svelte";
  import { transientScrollbar } from "$lib/transientScrollbar";
  import type { AgentSession } from "$lib/domain";
  import { getGitHubAccount, getRepositoryDiff, getSessionGitHub, observeRepository, openGitHub, repositoryError, type GitHubAccountSnapshot, type GitHubRepoSnapshot, type RepositoryCommit, type RepositoryDiff, type RepositoryState } from "$lib/repositories";

  let { session, language = "en", compact = false } = $props<{ session: AgentSession; language?: "en" | "pt-BR"; compact?: boolean }>();
  let repository = $state<RepositoryState>({ loading: true, value: null, error: "" });
  let github = $state<GitHubRepoSnapshot | null>(null);
  let githubLoading = $state(false);
  let tab = $state("activity");
  let activityScope = $state<"repository" | "account">("repository");
  let account = $state<GitHubAccountSnapshot | null>(null);
  let accountLoading = $state(false);
  let accountError = $state("");
  let githubError = $state("");
  let accountGeneration = 0;
  let preview = $state<RepositoryDiff | null>(null);
  let previewPath = $state("");
  let previewError = $state("");
  let previewLoading = $state(false);
  let actionError = $state("");
  let copied = $state(false);
  let fileLimit = $state(100);
  let generation = 0;
  let previewGeneration = 0;
  let requestedGitHub = "";
  const bannerReporter = getContext<SystemBannerReporter | undefined>(SYSTEM_BANNER_CONTEXT);
  const reportNotices = createSystemBannerSource(bannerReporter);
  const tr = (en: string, pt: string) => language === "pt-BR" ? pt : en;
  const local = $derived(repository.value);
  const remote = $derived(github?.repository);
  const needsGitHub = $derived(["pulls", "issues", "actions"].includes(tab));
  const sections = $derived<Array<{ value: string; label: string; short: string; icon: LumeIconName }>>([
    { value: "activity", label: tr("Repository overview", "Resumo do repositório"), short: tr("Overview", "Resumo"), icon: "repository" },
    { value: "changes", label: `${tr("Changes", "Mudanças")} · ${local?.files.length ?? 0}`, short: tr("Changes", "Mudanças"), icon: "diff" },
    { value: "commits", label: tr("Recent commits", "Commits recentes"), short: "Commits", icon: "commit" },
    { value: "pulls", label: "Pull requests", short: "PRs", icon: "pull-request" },
    { value: "issues", label: "Issues", short: "Issues", icon: "issue" },
    { value: "actions", label: "GitHub Actions", short: "Actions", icon: "bolt" },
  ]);
  const accountRepositories = $derived((account?.topRepositories ?? []).map((item) => ({
    name: item.repository.nameWithOwner,
    count: item.contributions.totalCount,
    url: item.repository.url,
    avatarUrl: item.repository.usesCustomOpenGraphImage ? item.repository.openGraphImageUrl : undefined,
    fallbackAvatarUrl: item.repository.owner?.avatarUrl,
  })).sort((a, b) => b.count - a.count));

  $effect(() => {
    if (!bannerReporter) return;
    const notices: SystemBannerNotice[] = [];
    for (const [id, message] of [
      ["repository", repository.error ? repositoryError(repository.error, language === "pt-BR") : ""],
      ["diff", previewError],
      ["action", actionError],
      ["account", accountError],
      ["github", githubError],
      ["checks", github?.workflowsError ? repositoryError(github.workflowsError, language === "pt-BR") : ""],
    ]) {
      if (message) notices.push({ id: `${session.id}:${id}`, message, tone: "error" });
    }
    untrack(() => reportNotices(notices));
  });

  $effect(() => {
    const id = session.id;
    const directory = session.workingDirectory;
    const nativeSessionId = session.nativeSessionId;
    generation += 1;
    fileLimit = 100;
    previewGeneration += 1;
    github = null; requestedGitHub = ""; preview = null; previewPath = ""; previewError = ""; actionError = "";
    githubLoading = false;
    githubError = "";
    const observer = observeRepository({ id, nativeSessionId, workingDirectory: directory });
    return observer.subscribe((state) => { repository = state; });
  });
  $effect(() => {
    const id = session.id;
    if (!needsGitHub || !local?.github) return;
    const key = `${id}:${local.github}:${local.head}`;
    if (requestedGitHub === key) return;
    requestedGitHub = key;
    void loadGitHub(id);
  });
  $effect(() => {
    if (tab === "activity" && activityScope === "account" && !account && !accountLoading) void loadAccount();
  });
  $effect(() => () => { accountGeneration += 1; });

  async function loadAccount(refresh = false) {
    const current = ++accountGeneration;
    accountLoading = true;
    try { const value = await getGitHubAccount(refresh); if (current === accountGeneration) { account = value; accountError = ""; } }
    catch (error) { if (current === accountGeneration) { account = { state: String(error), fetchedAt: 0 }; accountError = String(error).replace(/^Error:\s*/, ""); } }
    finally { if (current === accountGeneration) accountLoading = false; }
  }
  function selectSection(value: string) {
    tab = value; previewGeneration += 1; previewPath = ""; preview = null;
  }

  async function loadGitHub(id: string, refresh = false) {
    const current = generation;
    githubLoading = true;
    try { const value = await getSessionGitHub(id, refresh); if (generation === current) { github = value; githubError = ""; } }
    catch (error) { if (generation === current) { github = { state: String(error), fetchedAt: 0 }; githubError = String(error).replace(/^Error:\s*/, ""); } }
    finally { if (generation === current) githubLoading = false; }
  }
  async function refresh() {
    await observeRepository(session).refresh(true);
    if (needsGitHub && local?.github) await loadGitHub(session.id, true);
    if (tab === "activity" && activityScope === "account") await loadAccount(true);
  }
  async function open(url: string) {
    actionError = "";
    try { await openGitHub(url); } catch (error) { actionError = tr("Could not open GitHub. Try again.", "Não foi possível abrir o GitHub. Tente novamente.") + ` ${String(error).replace(/^Error:\s*/, "")}`; }
  }
  async function showDiff(path: string) {
    const current = ++previewGeneration;
    const id = session.id;
    previewPath = path; preview = null; previewError = ""; previewLoading = true;
    try { const value = await getRepositoryDiff(id, path); if (current === previewGeneration) preview = value; }
    catch (error) { if (current === previewGeneration) previewError = repositoryError(String(error), language === "pt-BR"); }
    finally { if (current === previewGeneration) previewLoading = false; }
  }
  async function copyBranch() {
    try { await navigator.clipboard.writeText(local?.branch ?? ""); copied = true; setTimeout(() => (copied = false), 1800); }
    catch (error) { actionError = tr("Could not copy the branch name.", "Não foi possível copiar o nome da branch.") + ` ${String(error).replace(/^Error:\s*/, "")}`; }
  }
  function statusLabel(status: string | null | undefined) {
    return ({ success: tr("Passed", "Passou"), failure: tr("Failed", "Falhou"), cancelled: tr("Cancelled", "Cancelado"), in_progress: tr("Running", "Em execução"), queued: tr("Queued", "Na fila"), completed: tr("Completed", "Concluído"), timed_out: tr("Timed out", "Tempo esgotado"), skipped: tr("Skipped", "Ignorado"), action_required: tr("Action required", "Ação necessária") })[status ?? ""] ?? status?.replaceAll("_", " ") ?? tr("Pending", "Pendente");
  }
  function fileName(path: string) { return path.split("/").at(-1) ?? path; }
  function fileDirectory(path: string) { return path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : ""; }
  function fileLabel(file: NonNullable<typeof local>["files"][number]) {
    if (file.conflict) return tr("Conflict", "Conflito");
    if (file.untracked) return tr("New", "Novo");
    if (`${file.index}${file.worktree}`.includes("D")) return tr("Deleted", "Removido");
    if (`${file.index}${file.worktree}`.includes("R")) return tr("Renamed", "Renomeado");
    return file.worktree === "." ? tr("Staged", "Preparado") : tr("Modified", "Modificado");
  }
  function commitDate(value: string) {
    const date = new Date(`${value}T00:00:00Z`);
    return Number.isFinite(date.getTime()) ? new Intl.DateTimeFormat(language, { day: "numeric", month: "short", timeZone: "UTC" }).format(date) : value;
  }
  const diffLines = $derived.by(() => {
    let before = 0; let after = preview?.untracked ? 1 : 0;
    return (preview?.diff ?? "").split("\n").map((text) => {
      const hunk = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(text);
      if (hunk) { before = Number(hunk[1]); after = Number(hunk[2]); return { text, kind: "hunk", before: "", after: "" }; }
      if (text.startsWith("diff --git ") || text.startsWith("index ") || text.startsWith("--- ") || text.startsWith("+++ ") || text.startsWith("\\")) return { text, kind: "meta", before: "", after: "" };
      if (text.startsWith("+")) return { text, kind: "added", before: "", after: after ? String(after++) : "" };
      if (text.startsWith("-")) return { text, kind: "removed", before: before ? String(before++) : "", after: "" };
      return { text, kind: "context", before: before ? String(before++) : "", after: after ? String(after++) : "" };
    });
  });
</script>


{#snippet commitHistory(commits: RepositoryCommit[])}
  <ol class="commit-list">{#each commits as commit (commit.oid)}
    <li>
      <span class="commit-mark" aria-hidden="true"><LumeIcon name="commit" size={16} /></span>
      <div class="commit-copy"><strong>{commit.subject}</strong><div class="commit-meta"><span>{commit.author}</span><time datetime={commit.date} title={commit.date}>{commitDate(commit.date)}</time>{#if local?.github}<button class="commit-hash" type="button" title={tr("Open commit on GitHub", "Abrir commit no GitHub")} onclick={() => void open(`https://github.com/${local?.github}/commit/${commit.oid}`)}>{commit.oid.slice(0, 7)}</button>{:else}<code>{commit.oid.slice(0, 7)}</code>{/if}</div></div>
    </li>
  {/each}</ol>
{/snippet}

<section class="repository-panel" class:compact aria-label={tr("Chat repository", "Repositório do chat")}>
  {#if repository.loading && !local}
    <div class="repository-skeleton" role="status" aria-label={tr("Loading repository", "Carregando repositório")}><i></i><i></i><i></i></div>
  {:else if !local}
    <div class="repository-empty"><LumeIcon name="repository" size={28} /><strong>{tr("Repository context", "Contexto do repositório")}</strong><p>{repositoryError(repository.error, language === "pt-BR")}</p><button type="button" disabled={repository.loading} onclick={() => void refresh()}><LumeIcon name="refresh" size={14} />{tr("Refresh", "Atualizar")}</button></div>
  {:else}
    <header class="repository-heading">
      <span class="repository-mark"><LumeIcon name="repository" size={22} /></span>
      <span class="repository-title"><strong>{local.github?.split("/").at(-1) ?? local.name}</strong>{#if local.github}<small>{local.github.split("/").slice(0, -1).join("/")}</small>{/if}</span>
      {#if local.github}<button class="icon-button" type="button" title={tr("Open repository on GitHub", "Abrir repositório no GitHub")} aria-label={tr("Open repository on GitHub", "Abrir repositório no GitHub")} onclick={() => void open(`https://github.com/${local.github}`)}><LumeIcon name="external" size={14} /></button>{/if}
      <button class="icon-button" type="button" disabled={repository.loading || githubLoading} aria-label={tr("Refresh repository", "Atualizar repositório")} title={tr("Refresh repository", "Atualizar repositório")} onclick={() => void refresh()}><LumeIcon name="refresh" size={14} /></button>
    </header>
    <p class="repository-location" title={local.root}>{local.root}</p>
    <div class="branch-row">
      <button class="branch" type="button" title={copied ? tr("Copied", "Copiado") : tr("Copy branch name", "Copiar nome da branch")} onclick={() => void copyBranch()}><LumeIcon name={copied ? "check" : "branch"} size={15} /><span>{local.branch === "(detached)" ? local.head?.slice(0, 7) : local.branch}</span><span class="copy-branch"><LumeIcon name="copy" size={12} /></span></button>
      <span class="sync-count" title={local.upstream ? `${local.upstream} · ${tr("commits ahead / behind", "commits à frente / atrás")}` : tr("No upstream configured", "Sem upstream configurado")}><LumeIcon name="arrow-up" size={11} />{local.ahead}<LumeIcon name="arrow-down" size={11} />{local.behind}</span>
    </div>
    {#if !bannerReporter && repository.error}<p class="inline-error" role="alert">{repositoryError(repository.error, language === "pt-BR")}</p>{/if}
    <nav class="repository-navigation" aria-label={tr("Repository tools", "Ferramentas do repositório")}>
      {#each sections as section (section.value)}<button type="button" class:active={tab === section.value} aria-pressed={tab === section.value} aria-label={section.label} title={section.label} onclick={() => selectSection(section.value)}><LumeIcon name={section.icon} size={18} /><span>{section.short}</span></button>{/each}
    </nav>
    {#if tab === "activity"}
      <div class="activity-scope" role="group" aria-label={tr("Activity source", "Origem da atividade")}><button type="button" class:active={activityScope === "repository"} aria-pressed={activityScope === "repository"} onclick={() => (activityScope = "repository")}><LumeIcon name="branch" size={13} />{tr("This repository", "Este repositório")}</button><button type="button" class:active={activityScope === "account"} aria-pressed={activityScope === "account"} onclick={() => (activityScope = "account")}><BrandIcon name="github" size={13} />{tr("My account", "Minha conta")}</button></div>
      {#if activityScope === "repository"}
      <button class="working-summary" class:clean={!local.files.length} type="button" onclick={() => (tab = "changes")}>
        <LumeIcon name={local.files.length ? "diff" : "check"} size={19} />
        <span><strong>{local.files.length ? tr(`${local.files.length} changed ${local.files.length === 1 ? "file" : "files"}`, `${local.files.length} ${local.files.length === 1 ? "arquivo alterado" : "arquivos alterados"}`) : tr("Working tree clean", "Árvore de trabalho limpa")}</strong><small>{local.files.length ? tr(`${local.staged} staged · ${local.modified} modified · ${local.untracked} new`, `${local.staged} ${local.staged === 1 ? "preparado" : "preparados"} · ${local.modified} ${local.modified === 1 ? "modificado" : "modificados"} · ${local.untracked} ${local.untracked === 1 ? "novo" : "novos"}`) : tr("No pending local changes", "Sem mudanças locais pendentes")}{local.conflicts ? ` · ${local.conflicts} ${tr("conflicts", "conflitos")}` : ""}</small></span><LumeIcon name="arrow-right" size={14} />
      </button>
      <GitHubActivity days={local.days} {language} kind="commits" rangeDays={90} compact />
      {#if local.activityLimited || local.shallow}<p class="source-note">{local.shallow ? tr("Shallow clone: activity covers downloaded history.", "Clone raso: a atividade cobre o histórico baixado.") : tr("Activity is limited to 20,000 recent commits.", "A atividade está limitada aos 20.000 commits mais recentes.")}</p>{/if}
      <div class="section-heading"><h3>{tr("Recent commits", "Commits recentes")}</h3><button type="button" onclick={() => (tab = "commits")}>{tr("View history", "Ver histórico")}<LumeIcon name="arrow-right" size={12} /></button></div>
      {@render commitHistory(local.commits.slice(0, 3))}
      {#if !local.commits.length}<p class="empty-list">{tr("This branch has no commits yet.", "Esta branch ainda não tem commits.")}</p>{/if}
      {:else if accountLoading && !account}
        <div class="repository-skeleton" role="status" aria-label={tr("Loading account activity", "Carregando atividade da conta")}><i></i><i></i><i></i></div>
      {:else if account?.state === "connected"}
        <div class="account-identity"><BrandIcon name="github" size={16} /><span><strong>{account.name || account.login}</strong><small>@{account.login}</small></span><button class="icon-button" type="button" disabled={accountLoading} aria-label={tr("Refresh account activity", "Atualizar atividade da conta")} onclick={() => void loadAccount(true)}><LumeIcon name="refresh" size={14} /></button></div>
        <GitHubActivity days={account.days ?? []} total={account.totalContributions} repositories={accountRepositories} {language} compact />
      {:else}
        <div class="github-connection"><p>{repositoryError(account?.state ?? "command_failed", language === "pt-BR")}</p>{#if ["auth_required", "cli_missing"].includes(account?.state ?? "")}<code>gh auth login --hostname github.com --scopes read:user</code>{/if}<button class="text-action" type="button" disabled={accountLoading} onclick={() => void loadAccount(true)}><LumeIcon name="refresh" size={13} />{tr("Try again", "Tentar novamente")}</button></div>
      {/if}
    {:else if tab === "changes"}
      <div class="change-summary"><span><strong>{local.staged}</strong>{tr("staged", local.staged === 1 ? "preparado" : "preparados")}</span><span><strong>{local.modified}</strong>{tr("modified", local.modified === 1 ? "modificado" : "modificados")}</span><span><strong>{local.untracked}</strong>{tr("new", local.untracked === 1 ? "novo" : "novos")}</span>{#if local.conflicts}<span class="conflict"><strong>{local.conflicts}</strong>{tr("conflicts", "conflitos")}</span>{/if}</div>
      <p class="source-note">{tr("Shared working tree · not attributed to this chat", "Árvore de trabalho compartilhada · sem atribuição a este chat")}</p>
      {#if !local.files.length}<p class="empty-list"><LumeIcon name="check" size={18} />{tr("Working tree is clean.", "Árvore de trabalho limpa.")}</p>{/if}
      <div class="repository-list">{#each local.files.slice(0, fileLimit) as file (file.path)}
        <button type="button" class:selected={previewPath === file.path} onclick={() => void showDiff(file.path)} title={file.path} aria-pressed={previewPath === file.path}>
          <FileTypeIcon path={file.path} size={16} /><span class="file-copy"><strong>{fileName(file.path)}</strong>{#if fileDirectory(file.path)}<small>{fileDirectory(file.path)}</small>{/if}</span><span class="file-status" class:conflict={file.conflict} class:new-file={file.untracked}>{fileLabel(file)}</span>
        </button>
      {/each}</div>
      {#if local.files.length > fileLimit}<button class="text-action" type="button" onclick={() => (fileLimit += 100)}>{tr("Show more files", "Mostrar mais arquivos")} · {local.files.length - fileLimit}</button>{/if}
      {#if previewPath}
        <section class="diff-preview"><header><FileTypeIcon path={previewPath} size={14} /><strong title={previewPath}>{fileName(previewPath)}</strong><button class="icon-button" type="button" aria-label={tr("Close diff", "Fechar diff")} onclick={() => { previewGeneration += 1; previewPath = ""; preview = null; }}><LumeIcon name="close" size={13} /></button></header>
          {#if previewLoading}<p class="empty-list" role="status">{tr("Reading changes…", "Lendo mudanças…")}</p>
          {:else if previewError}{#if !bannerReporter}<p class="inline-error" role="alert">{previewError}</p>{/if}
          {:else if preview?.binary}<p class="empty-list">{tr("Binary file. Preview unavailable.", "Arquivo binário. Visualização indisponível.")}</p>
          {:else if preview && !preview.diff}<p class="empty-list">{tr("No text difference from HEAD. Check staged and working tree status.", "Sem diferença de texto em relação ao HEAD. Confira o status do index e da árvore de trabalho.")}</p>
          {:else if preview}<div class="diff-code" use:transientScrollbar tabindex="0" role="textbox" aria-readonly="true" aria-multiline="true" aria-label={tr("File changes", "Mudanças do arquivo")}>{#each diffLines as line}<div class="diff-line {line.kind}"><span class="line-number" aria-hidden="true">{line.before}</span><span class="line-number" aria-hidden="true">{line.after}</span><code>{line.text || " "}</code></div>{/each}</div>{/if}
        </section>
      {/if}
    {:else if tab === "commits"}
      <div class="section-heading history-heading"><h3>{tr("Branch history", "Histórico da branch")}</h3><span>{local.commits.length} {tr("recent", "recentes")}</span></div>
      {@render commitHistory(local.commits)}
      {#if !local.commits.length}<p class="empty-list">{tr("This branch has no commits yet.", "Esta branch ainda não tem commits.")}</p>{/if}
    {:else if !local.github}
      <p class="empty-list"><LumeIcon name="repository" size={21} />{tr("Connect a GitHub remote to use these tools.", "Conecte um remote do GitHub para usar estas ferramentas.")}</p>
    {:else if githubLoading && !github}
      <div class="repository-skeleton" role="status" aria-label={tr("Loading GitHub", "Carregando GitHub")}><i></i><i></i><i></i></div>
    {:else if github?.state !== "connected"}
      <div class="github-connection"><p>{repositoryError(github?.state ?? "command_failed", language === "pt-BR")}</p>{#if ["auth_required", "cli_missing"].includes(github?.state ?? "")}<code>gh auth login --hostname github.com</code><button class="text-action" type="button" onclick={() => void open("https://github.com/cli/cli#installation")}>{tr("GitHub CLI setup", "Configurar GitHub CLI")}</button>{/if}<button class="text-action" type="button" disabled={githubLoading} onclick={() => void loadGitHub(session.id, true)}><LumeIcon name="refresh" size={13} />{tr("Try again", "Tentar novamente")}</button></div>
    {:else if tab === "pulls"}
      <div class="section-heading list-heading"><h3>Pull requests <small>{remote?.pullRequests?.totalCount ?? 0}</small></h3><button class="create-item" type="button" onclick={() => void open(`https://github.com/${local.github}/compare/${encodeURIComponent(local.branch)}?expand=1`)}><LumeIcon name="plus" size={13} />{tr("New PR", "Novo PR")}</button></div>
      <div class="github-items">{#each remote?.pullRequests?.nodes ?? [] as pr (pr.number)}
        <button type="button" class:current-branch={pr.headRefName === local.branch} class:draft={pr.isDraft} onclick={() => void open(pr.url)}><span class="item-mark"><LumeIcon name="pull-request" size={18} /></span><span class="item-copy"><strong>{pr.title}</strong><span class="item-meta"><span>#{pr.number}</span><span>{pr.isDraft ? tr("Draft", "Rascunho") : pr.reviewDecision === "APPROVED" ? tr("Approved", "Aprovado") : pr.reviewDecision === "CHANGES_REQUESTED" ? tr("Changes requested", "Mudanças solicitadas") : tr("Awaiting review", "Aguardando revisão")}</span></span><small class="pr-branches"><LumeIcon name="branch" size={11} />{pr.headRefName}<LumeIcon name="arrow-right" size={10} />{pr.baseRefName}</small>{#if pr.headRefName === local.branch}<small class="current-label">{tr("Current branch", "Branch atual")}</small>{/if}</span><LumeIcon name="external" size={11} /></button>
      {/each}</div>
      {#if !remote?.pullRequests?.nodes.length}<p class="empty-list"><LumeIcon name="pull-request" size={21} />{tr("No open pull requests in this repository.", "Nenhum pull request aberto neste repositório.")}</p>{/if}
    {:else if tab === "issues"}
      <div class="section-heading list-heading"><h3>Issues <small>{remote?.issues?.totalCount ?? 0}</small></h3><button class="create-item" type="button" onclick={() => void open(`https://github.com/${local.github}/issues/new`)}><LumeIcon name="plus" size={13} />{tr("New issue", "Nova issue")}</button></div>
      <div class="github-items">{#each remote?.issues?.nodes ?? [] as issue (issue.number)}<button type="button" onclick={() => void open(issue.url)}><span class="item-mark"><LumeIcon name="issue" size={18} /></span><span class="item-copy"><strong>{issue.title}</strong><span class="item-meta"><span>#{issue.number}</span><span>{issue.author?.login ?? "GitHub"}</span></span></span><LumeIcon name="external" size={11} /></button>{/each}</div>
      {#if !remote?.issues?.nodes.length}<p class="empty-list"><LumeIcon name="issue" size={21} />{tr("No open issues in this repository.", "Nenhuma issue aberta neste repositório.")}</p>{/if}
    {:else if tab === "actions"}
      <div class="section-heading list-heading"><h3>GitHub Actions</h3></div>
      {#if remote?.object?.statusCheckRollup}<p class="checks-status"><LumeIcon name={remote.object.statusCheckRollup.state === "SUCCESS" ? "check" : "warning"} size={14} /><span>HEAD · {remote.object.statusCheckRollup.state.toLowerCase()}</span></p>{/if}
      <div class="github-items">{#each github.workflows ?? [] as run (run.id)}<button type="button" onclick={() => void open(run.url)}><span class="item-mark" class:passed={run.conclusion === "success"} class:failed={run.conclusion === "failure"}><LumeIcon name={run.conclusion === "success" ? "check" : run.conclusion === "failure" ? "warning" : "bolt"} size={18} /></span><span class="item-copy"><strong>{run.title || run.name}</strong><span class="item-meta"><span>{run.name}</span><span>{statusLabel(run.conclusion ?? run.status)}</span></span><small class="pr-branches"><LumeIcon name="branch" size={11} />{run.branch}</small></span><LumeIcon name="external" size={11} /></button>{/each}</div>
      {#if github.workflowsError}{#if !bannerReporter}<p class="inline-error" role="alert">{repositoryError(github.workflowsError, language === "pt-BR")}</p>{/if}{:else if !github.workflows?.length}<p class="empty-list"><LumeIcon name="bolt" size={21} />{tr("No workflow runs available.", "Nenhuma execução de workflow disponível.")}</p>{/if}
    {/if}
    {#if !bannerReporter && actionError}<p class="inline-error" role="alert">{actionError}</p>{/if}
  {/if}
</section>

<style>
  .repository-panel { min-width: 0; padding: 18px 0 4px; color: var(--workspace-text); container-type: inline-size; }
  button { font: inherit; cursor: pointer; }
  button:focus-visible, .diff-code:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 2px; }
  button:disabled { opacity: .5; cursor: default; }
  ::selection { background: var(--workspace-accent-soft); color: var(--workspace-strong); }
  .repository-heading { min-width: 0; display: flex; align-items: center; gap: 9px; }
  .repository-mark { flex: 0 0 auto; color: var(--workspace-muted); }
  .repository-title { min-width: 0; flex: 1; display: grid; gap: 3px; }
  .repository-title strong { color: var(--workspace-strong); font-size: 15px; font-weight: 600; letter-spacing: -.02em; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .repository-title small { color: var(--workspace-muted); font-size: 10px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .repository-location { margin: 10px 0 13px; color: var(--workspace-muted); font-size: 10px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .icon-button { width: 28px; height: 28px; flex: 0 0 auto; display: grid; place-items: center; padding: 0; border: 0; border-radius: 6px; color: var(--workspace-muted); background: transparent; }
  .icon-button:hover:not(:disabled) { color: var(--workspace-strong); background: var(--workspace-subtle); }
  .branch-row { min-width: 0; min-height: 37px; display: flex; align-items: center; gap: 8px; padding: 0 9px; border-radius: 7px; background: var(--workspace-subtle); }
  .branch { min-width: 0; flex: 1; display: flex; align-items: center; gap: 7px; padding: 9px 0; border: 0; color: var(--workspace-text); background: transparent; font-size: 11px; }
  .branch > span:first-of-type { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .branch > :global(svg) { color: var(--workspace-accent); }
  .branch:hover { color: var(--workspace-accent); }
  .copy-branch { margin-left: auto; opacity: 0; color: var(--workspace-muted); }
  .branch:hover .copy-branch, .branch:focus-visible .copy-branch { opacity: 1; }
  .sync-count { display: flex; align-items: center; gap: 3px; flex: 0 0 auto; color: var(--workspace-muted); font-size: 10px; font-variant-numeric: tabular-nums; }
  .sync-count > :global(svg:nth-of-type(2)) { margin-left: 4px; }
  .repository-navigation { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 4px; margin: 14px -3px 14px; padding-bottom: 10px; border-bottom: 1px solid var(--workspace-line); }
  .repository-navigation > button { min-width: 0; min-height: 34px; display: flex; align-items: center; justify-content: flex-start; gap: 5px; padding: 6px 4px; border: 0; border-radius: 7px; color: var(--workspace-muted); background: transparent; font-size: 10px; white-space: nowrap; }
  .repository-navigation > button :global(svg) { flex: 0 0 auto; }
  .repository-navigation > button:hover { color: var(--workspace-strong); background: var(--workspace-subtle); }
  .repository-navigation > button.active { color: var(--workspace-accent); background: var(--workspace-accent-soft); }
  .activity-scope { display: flex; align-items: center; gap: 16px; margin: 0 0 14px; }
  .activity-scope > button { min-height: 27px; padding: 4px 0; display: flex; align-items: center; gap: 5px; border: 0; color: var(--workspace-muted); background: transparent; font-size: 10px; }
  .activity-scope > button.active { color: var(--workspace-strong); font-weight: 550; }
  .activity-scope > button.active :global(svg) { color: var(--workspace-accent); }
  .activity-scope > button:hover { color: var(--workspace-accent); }
  .account-identity { min-width: 0; display: flex; align-items: center; gap: 8px; margin: 17px 0 13px; color: var(--workspace-muted); }
  .account-identity > span { min-width: 0; flex: 1; display: grid; gap: 3px; }
  .account-identity strong { color: var(--workspace-strong); font-size: 12px; font-weight: 550; }
  .account-identity small { font-size: 10px; }
  .create-item { min-height: 29px; padding: 0 7px; display: flex; align-items: center; gap: 4px; border: 0; border-radius: 6px; color: var(--workspace-accent); background: var(--workspace-accent-soft); font-size: 10px; }
  .working-summary { width: 100%; min-width: 0; margin: 0 0 15px; display: flex; align-items: center; gap: 10px; padding: 12px 9px; border: 0; border-block: 1px solid var(--workspace-line); color: var(--workspace-muted); background: transparent; text-align: left; }
  .working-summary > span { min-width: 0; flex: 1; display: grid; gap: 5px; }
  .working-summary strong { color: var(--workspace-strong); font-size: 12px; font-weight: 550; }
  .working-summary small { color: var(--workspace-muted); font-size: 10px; line-height: 1.5; }
  .working-summary.clean { color: var(--workspace-accent); }
  .working-summary:hover { background: var(--workspace-subtle); }
  .section-heading { min-width: 0; display: flex; align-items: center; justify-content: space-between; gap: 8px; margin: 20px 0 4px; }
  .section-heading h3 > small { margin-left: 5px; color: var(--workspace-muted); font-size: 10px; font-weight: 400; }
  .section-heading h3 { margin: 0; color: var(--workspace-strong); font-size: 11px; font-weight: 600; }
  .section-heading > button { display: flex; align-items: center; gap: 5px; padding: 4px 0; border: 0; color: var(--workspace-muted); background: transparent; font-size: 10px; }
  .section-heading > button:hover { color: var(--workspace-accent); }
  .section-heading > span { color: var(--workspace-muted); font-size: 10px; font-variant-numeric: tabular-nums; }
  .history-heading, .list-heading { margin-top: 18px; }
  .source-note { margin: 11px 0; color: var(--workspace-muted); font-size: 10px; line-height: 1.6; }
  .change-summary { display: flex; flex-wrap: wrap; gap: 7px 14px; padding: 1px 0; color: var(--workspace-muted); }
  .change-summary > span { display: flex; align-items: baseline; gap: 5px; font-size: 10px; }
  .change-summary strong { color: var(--workspace-strong); font-weight: 600; font-size: 13px; font-variant-numeric: tabular-nums; }
  .conflict, .failed { color: #c36c60; }
  .repository-list { display: grid; gap: 2px; margin-inline: -5px; }
  .repository-list > button { width: 100%; min-width: 0; min-height: 48px; display: flex; align-items: center; gap: 9px; padding: 8px 7px; border: 0; border-radius: 6px; color: var(--workspace-text); background: transparent; text-align: left; }
  .repository-list > button:hover { background: var(--workspace-subtle); }
  .repository-list > button.selected { background: var(--workspace-accent-soft); }
  .file-copy { min-width: 0; flex: 1; display: grid; gap: 4px; }
  .file-copy strong { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11px; font-weight: 500; }
  .file-copy small { color: var(--workspace-muted); font-size: 10px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .file-status { flex: 0 0 auto; color: var(--workspace-muted); font-size: 9px; }
  .file-status.new-file { color: var(--workspace-accent); }
  .file-status.conflict { color: #c36c60; }
  .empty-list { display: flex; align-items: center; gap: 9px; margin: 22px 0; color: var(--workspace-muted); font-size: 11px; line-height: 1.6; }
  .diff-preview { min-width: 0; margin-top: 17px; overflow: hidden; border: 1px solid var(--workspace-line); border-radius: 8px; }
  .diff-preview > header { min-width: 0; min-height: 34px; padding: 5px 8px; display: flex; align-items: center; gap: 7px; color: var(--workspace-muted); background: var(--workspace-subtle); }
  .diff-preview > header > strong { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--workspace-text); font-size: 10px; font-weight: 500; }
  .diff-preview > p { padding-inline: 9px; }
  .diff-code { max-height: 300px; padding-block: 9px; overflow: auto; scrollbar-width: thin; scrollbar-color: transparent transparent; font-size: 10px; line-height: 1.8; }
  .diff-code:global(.is-scrolling) { scrollbar-color: var(--workspace-scroll-thumb) transparent; }
  .diff-code::-webkit-scrollbar { height: 5px; width: 5px; }
  .diff-code::-webkit-scrollbar-thumb { background: transparent; border-radius: 5px; }
  .diff-code:global(.is-scrolling)::-webkit-scrollbar-thumb { background: var(--workspace-scroll-thumb); }
  .diff-line { min-width: max-content; display: grid; grid-template-columns: 26px 26px minmax(0, 1fr); white-space: pre; }
  .diff-line > code { padding: 0 10px 0 5px; }
  .line-number { padding-inline: 3px; color: var(--workspace-muted); font: 9px ui-monospace, monospace; line-height: inherit; text-align: right; user-select: none; }
  .diff-code .added { background: color-mix(in srgb, var(--workspace-accent) 10%, transparent); color: var(--workspace-accent); }
  .diff-code .removed { background: rgba(190, 81, 69, .1); color: #c36c60; }
  .diff-code .hunk { color: var(--workspace-muted); background: var(--workspace-subtle); }
  .diff-code .meta { color: var(--workspace-muted); }
  .diff-code .meta > code, .diff-code .hunk > code { grid-column: 1 / -1; padding-inline: 10px; }
  .diff-code .meta > .line-number, .diff-code .hunk > .line-number { display: none; }
  .commit-list { margin: 0; padding: 0; list-style: none; }
  .commit-list > li { position: relative; min-width: 0; display: flex; align-items: flex-start; gap: 9px; padding: 13px 0; }
  .commit-list > li + li { border-top: 1px solid var(--workspace-line); }
  .commit-mark { margin-top: 1px; flex: 0 0 auto; color: var(--workspace-muted); }
  .commit-copy { min-width: 0; display: grid; gap: 7px; flex: 1; }
  .commit-copy > strong { color: var(--workspace-text); font-size: 11px; font-weight: 500; line-height: 1.55; overflow-wrap: anywhere; }
  .commit-meta { min-width: 0; display: flex; flex-wrap: wrap; align-items: center; gap: 5px 10px; color: var(--workspace-muted); font-size: 10px; }
  .commit-meta > span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .commit-hash, .commit-meta code { padding: 1px 0; border: 0; background: transparent; color: var(--workspace-muted); font: 9px ui-monospace, monospace; }
  .commit-hash:hover { color: var(--workspace-accent); }
  .github-items { margin-top: 4px; }
  .github-items > button { width: 100%; min-width: 0; display: flex; align-items: flex-start; gap: 8px; padding: 13px 5px; border: 0; border-bottom: 1px solid var(--workspace-line); color: var(--workspace-muted); background: transparent; text-align: left; }
  .github-items > button:hover { background: var(--workspace-subtle); }
  .github-items > button.current-branch { background: var(--workspace-accent-soft); border-radius: 7px; border-bottom-color: transparent; }
  .item-mark { color: var(--workspace-accent); flex: 0 0 auto; margin-top: 1px; }
  .draft .item-mark { color: var(--workspace-muted); }
  .item-mark.failed { color: #c36c60; }
  .item-copy { min-width: 0; display: grid; gap: 7px; flex: 1; }
  .item-copy strong { color: var(--workspace-strong); font-size: 12px; line-height: 1.5; font-weight: 550; overflow-wrap: anywhere; }
  .item-meta { display: flex; flex-wrap: wrap; gap: 5px 9px; color: var(--workspace-muted); font-size: 10px; }
  .pr-branches { display: flex; align-items: center; flex-wrap: wrap; gap: 4px; color: var(--workspace-muted); font-size: 9px; overflow-wrap: anywhere; }
  .current-label { color: var(--workspace-accent); font-size: 9px; }
  .checks-status { display: flex; align-items: center; gap: 7px; padding: 10px 0; margin: 0; color: var(--workspace-muted); font-size: 10px; }
  .repository-empty { padding: 32px 7px; display: grid; justify-items: center; gap: 13px; text-align: center; color: var(--workspace-muted); }
  .repository-empty strong { color: var(--workspace-strong); font-size: 13px; }
  .repository-empty p, .github-connection p { margin: 0; max-width: 40ch; line-height: 1.65; font-size: 11px; }
  .repository-empty > button { display: flex; align-items: center; gap: 6px; padding: 8px 11px; border: 1px solid var(--workspace-line); border-radius: 6px; color: var(--workspace-text); background: transparent; font-size: 11px; }
  .github-connection { padding-block: 18px; display: grid; gap: 13px; color: var(--workspace-muted); }
  .github-connection code { font-size: 10px; overflow-wrap: anywhere; }
  .text-action { display: inline-flex; align-items: center; gap: 6px; padding: 5px 0; border: 0; color: var(--workspace-accent); background: transparent; font-size: 11px; text-align: left; }
  .repository-skeleton { padding-block: 18px; display: grid; gap: 10px; }
  .repository-skeleton > i { height: 18px; border-radius: 4px; background: var(--workspace-subtle); }
  .repository-skeleton > i:nth-child(2) { width: 72%; }
  .repository-skeleton > i:nth-child(3) { height: 96px; }
  .inline-error { color: #c36c60; font-size: 11px; line-height: 1.6; }
  .compact { padding-top: 10px; }
  .compact .repository-title { display: flex; align-items: baseline; gap: 8px; }
  .compact .repository-title strong { font-size: 14px; }
  .compact .repository-location { margin: 6px 0 8px; }
  .compact .branch-row { min-height: 32px; }
  .compact .branch { padding-block: 6px; }
  .compact .repository-navigation { margin-block: 9px 8px; padding-bottom: 8px; }
  .compact .activity-scope { margin-bottom: 8px; }
  @container (min-width: 340px) {
    .repository-navigation { grid-template-columns: repeat(6, minmax(0, 1fr)); gap: 2px; }
    .repository-navigation > button { min-height: 42px; flex-direction: column; justify-content: center; gap: 5px; padding: 6px 1px; }
  }
</style>
