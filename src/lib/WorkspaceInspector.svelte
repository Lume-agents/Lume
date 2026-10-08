<script lang="ts">
  import BrandIcon from "$lib/BrandIcon.svelte";
  import FileTypeIcon from "$lib/FileTypeIcon.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import LumeSelect, { type LumeSelectOption } from "$lib/LumeSelect.svelte";
  import RepositoryPanel from "$lib/RepositoryPanel.svelte";
  import ThreadAvatar from "$lib/ThreadAvatar.svelte";
  import { transientScrollbar } from "$lib/transientScrollbar";
  import { displayFileChangePath } from "$lib/fileChanges";
  import { cleanPromptTransport } from "$lib/chatAttachments";
  import type { AgentSession, SessionActivity } from "$lib/domain";
  import type { Language } from "$lib/i18n";
  import { displayText } from "$lib/i18n";
  import { loadWorkspacePromptIndexPage, refreshAgentRateLimits, type WorkspacePromptIndexEntry } from "$lib/lume";
  import { buildLatestReviewTurn } from "$lib/reviewDiffs";
  import { subagentsForSession } from "$lib/workspaceAgents";
  import { expectedUsageLimitCount, stacksUsageGauges, usageGaugeGroups } from "$lib/usageGauges";

  let {
    session,
    language = "en",
    onClose,
    onOpenReview,
    variant = "workspace",
    showCloseButton = true,
    section = $bindable("session"),
    sessionOptions = [],
    onSelectSession,
    loading = false,
  } = $props<{
    session: (AgentSession & { activityTotal?: number }) | null;
    language?: Language;
    onClose?: () => void;
    onOpenReview?: (path?: string) => void;
    variant?: "workspace" | "orb";
    showCloseButton?: boolean;
    section?: "session" | "repository";
    sessionOptions?: LumeSelectOption[];
    onSelectSession?: (sessionId: string) => void;
    loading?: boolean;
  }>();

  let usageRefreshing = $state(false);
  let usageSettledKey = $state("");
  let usagePendingKey = $state("");
  let requestedUsageKey = "";
  let indexedPrompts = $state<WorkspacePromptIndexEntry[]>([]);
  let promptIndexLoading = $state(false);
  let promptIndexFailed = $state(false);
  let promptReload = $state(0);
  const promptSessionId = $derived(session?.id ?? "");
  const promptSourceKey = $derived(section === "session" && session?.nativeSessionId
    ? `${session.id}:${session.nativeSessionId}:${session.activities.filter((activity: SessionActivity) => activity.kind === "prompt").at(-1)?.id ?? ""}`
    : "");
  const recentPrompts = $derived.by(() => {
    const byId = new Map(indexedPrompts.map((prompt) => [prompt.id, prompt]));
    for (const activity of session?.activities ?? []) {
      if (activity.kind === "prompt") byId.set(activity.id, {
        id: activity.id, createdAt: activity.createdAt, detail: activity.detail ?? "",
      });
    }
    return [...byId.values()]
      .sort((left, right) => right.createdAt - left.createdAt || right.id.localeCompare(left.id))
      .slice(0, 5);
  });

  const latestTurn = $derived(session ? buildLatestReviewTurn(session.activities, session.results, session.workingDirectory) : null);
  const changes = $derived(latestTurn?.files ?? []);
  const visibleChanges = $derived(changes.slice(0, 6));
  const subagents = $derived(session ? subagentsForSession(session) : []);
  const visibleSubagents = $derived([...subagents]
    .sort((left, right) => Number(["running", "waiting"].includes(right.status)) - Number(["running", "waiting"].includes(left.status)) || right.updatedAt - left.updatedAt)
    .slice(0, 5));
  const totalAdded = $derived(changes.reduce((total, file) => total + file.added, 0));
  const totalRemoved = $derived(changes.reduce((total, file) => total + file.removed, 0));
  const tokenSamples = $derived((session?.promptTokenUsage ?? []).slice(-14));
  const tokenGraph = $derived(tokenGraphGeometry(tokenSamples));
  // The first read of a session's limits is the only wait worth showing; later refreshes keep the last values.
  const usageLoading = $derived(Boolean(session && hasUsage(session.agent) && usagePendingKey !== usageSettledKey && !session.rateLimits?.length));
  const usageGroups = $derived(usageGaugeGroups(session?.rateLimits ?? [], language));
  const usagePlaceholders = $derived(session ? expectedUsageLimitCount(session.agent) : 0);
  const usageStacked = $derived(session ? stacksUsageGauges(session.agent, session.rateLimits?.length ?? 0) : false);
  const automaticAccess = $derived(session?.permissionProfile.mode !== "full_access" && ["auto_review", "approve_for_me"].includes(session?.permissionProfile.approvalsReviewer?.replaceAll("-", "_") ?? ""));
  const fullAccess = $derived(session?.permissionProfile.mode === "full_access");

  $effect(() => {
    // Claude does not push its limits, so they are refreshed after every turn.
    const key = session && hasUsage(session.agent)
      ? `${session.agent}:${session.nativeSessionId ?? session.id}${session.agent === "claude_code" ? `:${session.status}` : ""}`
      : "";
    if (!key || key === requestedUsageKey) return;
    requestedUsageKey = key;
    usagePendingKey = key;
    void refreshUsage(key);
  });

  $effect(() => {
    const key = promptSourceKey;
    const sessionId = promptSessionId;
    void promptReload;
    indexedPrompts = [];
    promptIndexFailed = false;
    promptIndexLoading = Boolean(key);
    if (!key) return;
    let active = true;
    void loadWorkspacePromptIndexPage(sessionId)
      .then((page) => { if (active) indexedPrompts = page.prompts.slice(0, 5); })
      .catch(() => { if (active) promptIndexFailed = true; })
      .finally(() => { if (active) promptIndexLoading = false; });
    return () => { active = false; };
  });

  function tr(english: string, portuguese: string) {
    return language === "pt-BR" ? portuguese : english;
  }

  function promptTime(value: number) {
    return new Intl.DateTimeFormat(language, { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" }).format(new Date(value));
  }

  function tokenGraphGeometry(samples: NonNullable<AgentSession["promptTokenUsage"]>) {
    if (!samples.length) return { points: "", area: "", dots: [] as Array<{ x: number; y: number; tokens: number }> };
    const width = 226;
    const height = 54;
    const insetX = 4;
    const insetTop = 12;
    const insetBottom = 10;
    const firstTime = samples[0]?.createdAt ?? 0;
    const lastTime = samples.at(-1)?.createdAt ?? firstTime;
    const timeSpan = Math.max(1, lastTime - firstTime);
    const maximum = Math.max(1, ...samples.map((sample) => sample.totalTokens));
    const dots = samples.map((sample, index) => ({
      x: samples.length === 1 ? width / 2 : insetX + ((sample.createdAt - firstTime) / timeSpan || index / (samples.length - 1)) * (width - insetX * 2),
      y: height - insetBottom - (sample.totalTokens / maximum) * (height - insetTop - insetBottom),
      tokens: sample.totalTokens,
    }));
    const points = dots.length === 1
      ? `${insetX},${dots[0].y.toFixed(1)} ${width - insetX},${dots[0].y.toFixed(1)}`
      : dots.map((dot) => `${dot.x.toFixed(1)},${dot.y.toFixed(1)}`).join(" ");
    const area = `${insetX},${height - insetBottom} ${points} ${width - insetX},${height - insetBottom}`;
    return { points, area, dots };
  }

  function subagentStatus(status: (typeof subagents)[number]["status"]) {
    if (status === "running") return tr("Working", "Executando");
    if (status === "waiting") return tr("Waiting", "Aguardando");
    if (status === "failed") return tr("Failed", "Falhou");
    if (status === "interrupted") return tr("Interrupted", "Interrompido");
    return tr("Finished", "Concluído");
  }

  function hasUsage(agent: AgentSession["agent"]) {
    return expectedUsageLimitCount(agent) > 0;
  }

  async function refreshUsage(key: string) {
    if (!session || !hasUsage(session.agent) || usageRefreshing) {
      if (!session || !hasUsage(session.agent)) usageSettledKey = key;
      return;
    }
    usageRefreshing = true;
    try {
      await refreshAgentRateLimits(session.agent);
    } catch { /* Keep the last known usage without adding noisy inspector copy. */ }
    finally {
      usageRefreshing = false;
      usageSettledKey = usagePendingKey;
    }
  }
</script>

<aside class="workspace-inspector" class:has-session={Boolean(session)} class:orb-inspector={variant === "orb"} aria-label={tr("Session inspector", "Inspector da sessão")}>
  <header>
    {#if session}
      <span class="agent-icon"><BrandIcon name={session.agent} size={21} /></span>
      <span class="header-copy">
        {#if variant === "orb" && onSelectSession && sessionOptions.length}
          <LumeSelect
            value={session.id}
            options={sessionOptions}
            ariaLabel={tr("Session to inspect", "Sessão para inspecionar")}
            minWidth={0}
            menuMinWidth={260}
            variant="heading"
            onValueChange={onSelectSession}
          />
        {:else}
          <strong>{session.sessionName?.trim() || session.project || session.agentLabel}</strong>
        {/if}
        <small>{tr("Inspector", "Inspector")} · {displayText(language, session.statusLabel)}</small>
      </span>
      {#if automaticAccess}
        <span class="inspector-access-badge auto-review" title={tr("Approve for me", "Aprovar por mim")}>
          <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M6.8.8 2.9 6.3h2.5L4.9 11l4.2-5.7H6.5Z" /></svg>
          <span>{tr("Approve for me", "Aprovar por mim")}</span>
        </span>
      {:else if fullAccess}
        <span class="inspector-access-badge full-access" title={tr("Full access", "Acesso total")}>
          <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3 5V3.7a3 3 0 0 1 5.6-1.5M2.2 5.2h7.6v5.5H2.2Z" /></svg>
          <span>{tr("Full access", "Acesso total")}</span>
        </span>
      {/if}
    {:else}
      <span class="header-copy"><strong>{tr("Inspector", "Inspector")}</strong><small>{tr("No session selected", "Nenhuma sessão selecionada")}</small></span>
    {/if}
    {#if showCloseButton}
      <button type="button" aria-label={tr("Close inspector", "Fechar inspector")} title={tr("Close inspector", "Fechar inspector")} onclick={() => onClose?.()}>
        <LumeIcon name="close" size={16} />
      </button>
    {/if}
  </header>

  {#if session}
    <nav class="inspector-tabs" aria-label={tr("Inspector sections", "Seções do Inspector")}>
      <button type="button" class:active={section === "session"} aria-pressed={section === "session"} onclick={() => (section = "session")}><LumeIcon name="inspector" size={14} />{tr("Chat overview", "Visão do chat")}</button>
      <button type="button" class:active={section === "repository"} aria-pressed={section === "repository"} onclick={() => (section = "repository")}><LumeIcon name="repository" size={14} />{tr("Repository", "Repositório")}</button>
    </nav>
    {#if section === "repository"}
      <div class="inspector-scroll" use:transientScrollbar><RepositoryPanel {session} {language} compact /></div>
    {:else}
    <div class="inspector-scroll" use:transientScrollbar>
      {#if hasUsage(session.agent)}
        <section class="usage-section" class:stacked={usageStacked} aria-label={tr(`${session.agentLabel} usage`, `Uso do ${session.agentLabel}`)}>
          <div class="usage-gauges" class:loading={usageRefreshing && !usageGroups.length} aria-busy={usageLoading}>
            {#each usageGroups as group (group.label)}
              <div class="usage-group" role={group.label ? "group" : undefined} aria-label={group.label || undefined}>
                {#if group.label}<span class="usage-group-label">{group.label}</span>{/if}
                <div class="usage-group-gauges">
                  {#each group.gauges as gauge (gauge.id)}
                    <div class="usage-gauge" style:--usage-remaining={gauge.remaining} role="img" aria-label={gauge.title} title={gauge.title}>
                      <svg viewBox="0 0 60 39" aria-hidden="true">
                        <path class="gauge-track" pathLength="100" d="M7 33a23 23 0 0 1 46 0" />
                        <path class="gauge-progress" pathLength="100" d="M7 33a23 23 0 0 1 46 0" style:stroke-dasharray={`${gauge.remaining} 100`} />
                      </svg>
                      <strong aria-hidden="true">{gauge.remaining}</strong>
                      <em aria-hidden="true">{gauge.window}</em>
                    </div>
                  {/each}
                </div>
              </div>
            {:else}
              {#each { length: usagePlaceholders }, index (index)}<i></i>{/each}
            {/each}
          </div>
          <div class="token-chart" aria-busy={usageLoading && !tokenSamples.length}>
            <span>{tr("Tokens / prompt", "Tokens / prompt")}</span>
            <svg class:empty={!tokenGraph.points} class:loading={usageLoading && !tokenSamples.length} class="token-graph" viewBox="0 0 226 54" preserveAspectRatio="none" role="img" aria-label={tr("Tokens used per prompt over time", "Tokens usados por prompt ao longo do tempo")}>
              <path class="graph-grid" d="M4 14H222M4 28H222M4 42H222" />
              <polygon points={tokenGraph.area} />
              <polyline points={tokenGraph.points || "4,42 222,42"} />
              {#each tokenGraph.dots as dot}
                <circle cx={dot.x} cy={dot.y} r="2"><title>{dot.tokens.toLocaleString(language)} tokens</title></circle>
              {/each}
            </svg>
            <small>{usageLoading && !tokenSamples.length ? tr("Loading…", "Carregando…") : tr("Time →", "Tempo →")}</small>
          </div>
        </section>
      {/if}

      <details class="inspector-section events-section" open>
        <summary><LumeIcon name="send" size={14} /><span>{tr("Events", "Eventos")}</span><em>{recentPrompts.length}</em></summary>
        <p class="section-description">{tr("Latest prompts", "Últimos prompts")}</p>
        {#if recentPrompts.length}
          <ol class="prompt-events">
            {#each recentPrompts as prompt (prompt.id)}
              {@const text = cleanPromptTransport(prompt.detail) || tr("Prompt sent", "Prompt enviado")}
              <li><span class="event-mark"><LumeIcon name="send" size={12} /></span><div><time datetime={new Date(prompt.createdAt).toISOString()}>{promptTime(prompt.createdAt)}</time><p title={text}>{text}</p></div></li>
            {/each}
          </ol>
        {:else if promptIndexLoading}
          <p class="empty-section" role="status">{tr("Loading recent prompts…", "Carregando prompts recentes…")}</p>
        {:else if promptIndexFailed}
          <p class="empty-section" role="status">{tr("Could not load recent prompts.", "Não foi possível carregar os prompts recentes.")}</p>
        {:else}
          <p class="empty-section">{tr("No prompts recorded yet.", "Nenhum prompt registrado ainda.")}</p>
        {/if}
        {#if promptIndexFailed}<button class="event-retry" type="button" onclick={() => (promptReload += 1)}><LumeIcon name="refresh" size={12} />{tr("Retry history", "Recarregar histórico")}</button>{/if}
      </details>

      {#if subagents.length}
        <details class="inspector-section" open>
          <summary><span>{tr("Subagents", "Subagentes")}</span><em>{subagents.length}</em></summary>
          <div class="subagent-list">
            {#each visibleSubagents as agent (agent.id)}
              <div>
                <ThreadAvatar seed={`${session.id}:inspector:${agent.id}`} label={agent.label} size={24} />
                <span><strong>{agent.label}</strong><small class="status-{agent.status}">{subagentStatus(agent.status)}</small></span>
              </div>
            {/each}
          </div>
        </details>
      {/if}

      <details class="inspector-section" open>
        <summary><span>{tr("Changed files", "Arquivos alterados")}</span><em>{changes.length}</em></summary>
        {#if changes.length}
          <div class="files-toolbar">
            <span><strong>{changes.length} {changes.length === 1 ? tr("file", "arquivo") : tr("files", "arquivos")}</strong><small><b>+{totalAdded}</b><i>−{totalRemoved}</i></small></span>
            {#if onOpenReview}<button type="button" onclick={() => onOpenReview?.()}><LumeIcon name="diff" size={13} /><span>{tr("Review", "Revisar")}</span></button>{/if}
          </div>
          <div class="inspector-files">
            {#each visibleChanges as file (file.path)}
              {#if onOpenReview}
                <button type="button" title={file.path} onclick={() => onOpenReview?.(file.path)}>
                  <FileTypeIcon path={file.path} size={15} />
                  <span>{displayFileChangePath(file.path)}</span>
                  <small><b>+{file.added}</b><i>−{file.removed}</i></small>
                </button>
              {:else}
                <div class="inspector-file-entry" title={file.path}>
                  <FileTypeIcon path={file.path} size={15} />
                  <span>{displayFileChangePath(file.path)}</span>
                  <small><b>+{file.added}</b><i>−{file.removed}</i></small>
                </div>
              {/if}
            {/each}
          </div>
          {#if changes.length > visibleChanges.length && onOpenReview}<button class="review-more" type="button" onclick={() => onOpenReview?.()}>{tr(`View all ${changes.length} files`, `Ver todos os ${changes.length} arquivos`)}</button>{/if}
        {:else}
          <p class="empty-section">{tr("No changed files in the latest turn.", "Nenhum arquivo alterado no último turno.")}</p>
        {/if}
      </details>

    </div>
    {/if}
  {:else}
    {#if loading}
      <div class="inspector-scroll inspector-loading" role="status" aria-label={tr("Loading inspector", "Carregando o inspector")}>
        <section class="usage-section">
          <div class="usage-gauges loading"><i></i><i></i></div>
          <div class="token-chart"><span>{tr("Tokens / prompt", "Tokens / prompt")}</span><svg class="token-graph empty loading" viewBox="0 0 226 54" preserveAspectRatio="none" aria-hidden="true"><path class="graph-grid" d="M4 14H222M4 28H222M4 42H222" /><polyline points="4,42 222,42" /></svg><small>{tr("Loading…", "Carregando…")}</small></div>
        </section>
        <div class="loading-rows" aria-hidden="true"><i></i><i></i><i></i></div>
      </div>
    {:else}
    <div class="inspector-empty"><BrandIcon name="lume" size={30} /><span>{tr("Select an agent to inspect its work.", "Selecione um agente para inspecionar o trabalho.")}</span></div>
    {/if}
  {/if}
</aside>

<style>
  .workspace-inspector { min-width: 0; height: 100%; display: grid; grid-template-rows: auto minmax(0, 1fr); overflow: hidden; border-left: 1px solid var(--workspace-line); color: var(--workspace-text); background: var(--workspace-sidebar); }
  .workspace-inspector.has-session { grid-template-rows: auto auto minmax(0, 1fr); }
  .inspector-tabs { display: flex; gap: 3px; margin: 12px 12px 0; padding: 3px; border-radius: 8px; background: var(--workspace-subtle); }
  .inspector-tabs > button { min-width: 0; flex: 1; min-height: 31px; display: flex; align-items: center; justify-content: center; gap: 6px; padding: 0 5px; border: 0; border-radius: 6px; color: var(--workspace-muted); background: transparent; font: inherit; font-size: 11px; cursor: pointer; }
  .inspector-tabs > button.active { color: var(--workspace-strong); background: var(--workspace-pane); }
  .inspector-tabs > button.active :global(svg) { color: var(--workspace-accent); }
  .inspector-tabs > button:hover:not(.active) { color: var(--workspace-strong); }
  .inspector-tabs > button:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: -2px; }
  .workspace-inspector.orb-inspector { --workspace-pane: var(--lume-surface-light); --workspace-accent: #397b5c; --workspace-accent-soft: rgba(57, 123, 92, .11); --workspace-faint: #87968e; --workspace-line: rgba(92, 114, 103, .15); --workspace-muted: var(--lume-ink-muted-light); --workspace-raised: rgba(255, 255, 255, .44); --workspace-scroll-thumb: #cad2ce; --workspace-sidebar: transparent; --workspace-strong: #34473d; --workspace-subtle: rgba(74, 108, 90, .07); --workspace-text: #526158; border-left: 0; }
  :global(.overlay-shell.dark) .workspace-inspector.orb-inspector { --workspace-pane: var(--lume-surface-dark); --workspace-accent: #83c29f; --workspace-accent-soft: rgba(100, 180, 143, .11); --workspace-faint: #8b9a92; --workspace-line: rgba(199, 218, 207, .11); --workspace-muted: #a0b0a7; --workspace-raised: rgba(206, 228, 216, .045); --workspace-scroll-thumb: rgba(199, 218, 207, .32); --workspace-strong: #e0e9e4; --workspace-subtle: rgba(206, 228, 216, .055); --workspace-text: #c5d2cb; }
  .workspace-inspector > header { min-height: 50px; padding: 7px 8px; display: flex; align-items: center; gap: 9px; border-bottom: 1px solid var(--workspace-line); }
  .header-copy { min-width: 0; display: grid; gap: 2px; flex: 1; }.header-copy strong, .header-copy small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.header-copy strong { color: var(--workspace-strong); font-size: 11px; letter-spacing: -.015em; }.header-copy small { color: var(--workspace-muted); font-size: 8px; }
  .header-copy :global(.lume-select.heading) { margin: -2px -4px; }
  .agent-icon { width: 31px; height: 31px; display: grid; place-items: center; flex: 0 0 auto; color: var(--workspace-accent); }
  .inspector-access-badge { min-height: 21px; padding: 0 6px; display: inline-flex; align-items: center; gap: 3px; flex: 0 0 auto; border-radius: 999px; font-size: 7px; font-weight: 780; line-height: 1; white-space: nowrap; }.inspector-access-badge svg { width: 9px; height: 9px; flex: 0 0 auto; fill: none; stroke: currentColor; stroke-width: 1.35; }.inspector-access-badge.auto-review { color: #315f86; background: #cbdff0; }.inspector-access-badge.auto-review svg { fill: currentColor; stroke: none; }.inspector-access-badge.full-access { color: #764c2e; background: #e8ceb1; }
  :global(.workspace.dark) .inspector-access-badge.auto-review { color: #b4d3ee; background: #29445d; }:global(.workspace.dark) .inspector-access-badge.full-access { color: #e4b88f; background: #543b29; }
  .workspace-inspector > header button { width: 29px; height: 29px; padding: 0; display: grid; place-items: center; flex: 0 0 auto; border: 0; border-radius: 8px; color: var(--workspace-muted); background: transparent; cursor: pointer; }.workspace-inspector > header button:hover { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .inspector-scroll { min-height: 0; padding: 0 12px 16px; overflow-y: auto; overscroll-behavior: contain; scrollbar-width: thin; scrollbar-color: transparent transparent; }
  .inspector-scroll:global(.is-scrolling) { scrollbar-color: var(--workspace-scroll-thumb) transparent; }
  .inspector-scroll::-webkit-scrollbar { width: 5px; }
  .inspector-scroll::-webkit-scrollbar-thumb { border-radius: 5px; background: transparent; }
  .inspector-scroll:global(.is-scrolling)::-webkit-scrollbar-thumb { background: var(--workspace-scroll-thumb); }
  .usage-section { min-height: 72px; margin: 13px 0 3px; display: grid; grid-template-columns: max-content minmax(0, 1fr); align-items: center; gap: 12px; }.usage-section.stacked { grid-template-columns: minmax(0, 1fr); gap: 10px; }
  .token-graph :is(path, polyline, circle) { vector-effect: non-scaling-stroke; }
  .usage-gauges { min-width: 0; display: flex; align-items: center; justify-content: center; gap: 5px; }
  .usage-section.stacked .usage-gauges { justify-content: space-evenly; gap: 10px; }
  .usage-group { min-width: 0; display: grid; grid-template-columns: minmax(0, 1fr); justify-items: center; gap: 2px; }
  .usage-group-label { max-width: 100%; overflow: hidden; color: var(--workspace-muted); font-size: 8px; font-weight: 720; line-height: 1.2; text-overflow: ellipsis; white-space: nowrap; }
  .usage-group-gauges { width: 100%; min-width: 0; display: flex; align-items: center; justify-content: center; gap: 5px; }
  .usage-gauge { position: relative; min-width: 0; width: 62px; height: 51px; flex: 0 1 62px; --usage-color: color-mix(in srgb, #43a873 calc(var(--usage-remaining) * 1%), #ca605c); }
  .usage-gauge svg { position: absolute; inset: 0; width: 100%; height: 100%; overflow: visible; fill: none; stroke-linecap: round; }
  .gauge-track { stroke: var(--workspace-line); stroke-width: 5.5; }
  .gauge-progress { stroke: var(--usage-color); stroke-width: 5.5; transition: stroke-dasharray 360ms cubic-bezier(.16, 1, .3, 1); }
  .usage-gauge strong { position: absolute; right: 0; bottom: 3px; left: 0; color: var(--workspace-strong); font-size: 15px; font-variant-numeric: tabular-nums; line-height: 1; text-align: center; }
  .usage-gauge em { position: absolute; top: 1px; right: 2px; max-width: 45%; overflow: hidden; color: var(--workspace-strong); font-size: 10px; font-style: normal; font-weight: 780; line-height: 1; text-overflow: ellipsis; white-space: nowrap; }
  .usage-gauges > i { width: 56px; min-width: 0; flex: 0 1 56px; height: 30px; border: 5px solid var(--workspace-line); border-bottom: 0; border-radius: 32px 32px 0 0; opacity: .5; }
  .usage-gauges.loading > i { animation: usage-pulse 1.2s ease-in-out infinite alternate; }
  .token-chart { min-width: 0; display: grid; grid-template-rows: auto 48px auto; gap: 3px; }.token-chart > span, .token-chart > small { color: var(--workspace-muted); font-size: 10px; font-weight: 720; line-height: 1.2; }.token-chart > small { color: var(--workspace-faint); text-align: right; }.token-graph { width: 100%; min-width: 0; height: 48px; overflow: visible; }.token-graph .graph-grid { fill: none; stroke: color-mix(in srgb, var(--workspace-line) 54%, transparent); stroke-width: .7; }.token-graph polygon { fill: color-mix(in srgb, var(--workspace-accent) 8%, transparent); }.token-graph polyline { fill: none; stroke: var(--workspace-accent); stroke-width: 1.4; stroke-linecap: round; stroke-linejoin: round; }.token-graph.empty polyline { stroke: var(--workspace-line); stroke-dasharray: 3 4; }.token-graph circle { fill: var(--workspace-raised); stroke: var(--workspace-accent); stroke-width: 1.3; }
  .inspector-section { border-bottom: 1px solid var(--workspace-line); }.inspector-section > summary { min-height: 40px; display: flex; align-items: center; gap: 7px; color: var(--workspace-strong); font-size: 11px; font-weight: 600; list-style: none; cursor: pointer; }.inspector-section > summary::-webkit-details-marker { display: none; }.inspector-section > summary::after { width: 6px; height: 6px; margin-left: auto; border-right: 1.5px solid currentColor; border-bottom: 1.5px solid currentColor; content: ""; opacity: .5; transform: rotate(45deg); transition: transform 160ms ease; }.inspector-section[open] > summary::after { transform: rotate(225deg); }.inspector-section > summary em { min-width: 19px; height: 19px; display: grid; place-items: center; border-radius: 6px; color: var(--workspace-muted); background: var(--workspace-subtle); font-size: 9px; font-style: normal; }.inspector-section[open] { padding-bottom: 13px; }
  .prompt-events { display: grid; gap: 0; margin: 0; padding: 0; list-style: none; }
  .prompt-events > li { min-width: 0; display: flex; align-items: flex-start; gap: 8px; padding: 9px 0; }
  .prompt-events > li + li { border-top: 1px solid var(--workspace-line); }
  .event-mark { width: 22px; height: 22px; display: grid; place-items: center; flex: 0 0 auto; border-radius: 6px; color: var(--workspace-accent); background: var(--workspace-accent-soft); }
  .prompt-events > li > div { min-width: 0; flex: 1; }
  .prompt-events time { color: var(--workspace-muted); font-size: 9px; font-variant-numeric: tabular-nums; }
  .prompt-events p { margin: 4px 0 0; color: var(--workspace-text); font-size: 11px; line-height: 1.5; overflow-wrap: anywhere; display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; overflow: hidden; }
  .event-retry { display: inline-flex; align-items: center; gap: 5px; padding: 6px 0; border: 0; color: var(--workspace-accent); background: transparent; font: inherit; font-size: 10px; cursor: pointer; }
  .event-retry:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 2px; }
  .subagent-list { display: grid; gap: 3px; }.subagent-list > div { min-width: 0; min-height: 36px; padding: 4px 5px; display: flex; align-items: center; gap: 8px; border-radius: 8px; }.subagent-list > div:hover { background: var(--workspace-subtle); }.subagent-list > div > span { min-width: 0; display: grid; gap: 2px; }.subagent-list strong { overflow: hidden; color: var(--workspace-text); font-size: 9px; text-overflow: ellipsis; white-space: nowrap; }.subagent-list small { color: var(--workspace-faint); font-size: 8px; }.subagent-list small.status-running { color: #4e98ca; }.subagent-list small.status-failed { color: #c66762; }.subagent-list small.status-completed { color: #50aa79; }
  .files-toolbar { min-height: 40px; margin-bottom: 4px; padding-bottom: 8px; display: flex; align-items: center; gap: 8px; border-bottom: 1px solid color-mix(in srgb, var(--workspace-line) 65%, transparent); }.files-toolbar > span { min-width: 0; display: grid; gap: 2px; flex: 1; }.files-toolbar > span > strong { color: var(--workspace-text); font-size: 9px; }.files-toolbar > span small { display: flex; gap: 5px; font-size: 8px; }.files-toolbar b, .inspector-files b { color: #43a873; }.files-toolbar i, .inspector-files i { color: #c16660; font-style: normal; }.files-toolbar button { min-height: 28px; padding: 0 8px; display: inline-flex; align-items: center; gap: 5px; border: 1px solid color-mix(in srgb, var(--workspace-accent) 28%, var(--workspace-line)); border-radius: 7px; color: var(--workspace-accent); background: var(--workspace-accent-soft); font-size: 8px; font-weight: 740; cursor: pointer; }.files-toolbar button:hover { border-color: color-mix(in srgb, var(--workspace-accent) 52%, var(--workspace-line)); }
  .inspector-files { display: grid; gap: 1px; }.inspector-files > button, .inspector-files > .inspector-file-entry { width: 100%; min-width: 0; min-height: 31px; padding: 0 5px; display: flex; align-items: center; gap: 7px; border: 0; border-radius: 7px; color: var(--workspace-text); background: transparent; font-size: 9px; text-align: left; }.inspector-files > button { cursor: pointer; }.inspector-files > button:hover { background: var(--workspace-subtle); }.inspector-files > button > span, .inspector-file-entry > span { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.inspector-files > button > small, .inspector-file-entry > small { display: flex; gap: 4px; flex: 0 0 auto; font-size: 7px; }.review-more { margin: 5px 0 0 4px; padding: 3px 0; border: 0; color: var(--workspace-accent); background: transparent; font-size: 8px; font-weight: 720; cursor: pointer; }
  .section-description { margin: -1px 0 3px; color: var(--workspace-muted); font-size: 10px; line-height: 1.45; }
  .empty-section { margin: 0; padding: 0 0 5px; color: var(--workspace-faint); font-size: 9px; line-height: 1.45; }
  .inspector-empty { margin: auto; padding: 24px; display: grid; justify-items: center; gap: 10px; color: var(--workspace-faint); font-size: 10px; text-align: center; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .token-graph.loading { animation: usage-pulse 1.2s ease-in-out infinite alternate; }
  .inspector-loading { padding-top: 1px; }
  .loading-rows { margin-top: 18px; display: grid; gap: 10px; }
  .loading-rows > i { height: 12px; border-radius: 6px; background: var(--workspace-line); opacity: .5; animation: usage-pulse 1.2s ease-in-out infinite alternate; }
  .loading-rows > i:nth-child(2) { width: 78%; }
  .loading-rows > i:nth-child(3) { width: 54%; }
  @keyframes usage-pulse { to { opacity: .9; } }
  @media (prefers-reduced-motion: reduce) { .inspector-section > summary::after, .gauge-progress { transition: none; }.usage-gauges.loading > i, .token-graph.loading, .loading-rows > i { animation: none; } }
</style>
