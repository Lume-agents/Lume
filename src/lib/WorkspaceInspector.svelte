<script lang="ts">
  import BrandIcon from "$lib/BrandIcon.svelte";
  import FileTypeIcon from "$lib/FileTypeIcon.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import ThreadAvatar from "$lib/ThreadAvatar.svelte";
  import { displayFileChangePath } from "$lib/fileChanges";
  import type { AgentSession } from "$lib/domain";
  import type { Language } from "$lib/i18n";
  import { displayText } from "$lib/i18n";
  import { refreshAgentRateLimits } from "$lib/lume";
  import { buildReviewTurns } from "$lib/reviewDiffs";
  import { subagentsForSession } from "$lib/workspaceAgents";

  let {
    session,
    language = "en",
    onClose,
    onOpenReview,
    variant = "workspace",
    showCloseButton = true,
  } = $props<{
    session: (AgentSession & { activityTotal?: number }) | null;
    language?: Language;
    onClose?: () => void;
    onOpenReview?: (path?: string) => void;
    variant?: "workspace" | "orb";
    showCloseButton?: boolean;
  }>();

  let usageRefreshing = $state(false);
  let requestedUsageKey = "";

  const latestTurn = $derived(session ? buildReviewTurns(session.activities, session.results, session.workingDirectory)[0] : null);
  const changes = $derived(latestTurn?.files ?? []);
  const visibleChanges = $derived(changes.slice(0, 6));
  const checks = $derived(latestTurn?.checks ?? []);
  const subagents = $derived(session ? subagentsForSession(session) : []);
  const visibleSubagents = $derived([...subagents]
    .sort((left, right) => Number(["running", "waiting"].includes(right.status)) - Number(["running", "waiting"].includes(left.status)) || right.updatedAt - left.updatedAt)
    .slice(0, 5));
  const totalAdded = $derived(changes.reduce((total, file) => total + file.added, 0));
  const totalRemoved = $derived(changes.reduce((total, file) => total + file.removed, 0));
  const tokenSamples = $derived((session?.promptTokenUsage ?? []).slice(-14));
  const tokenGraph = $derived(tokenGraphGeometry(tokenSamples));
  const automaticAccess = $derived(session?.permissionProfile.mode !== "full_access" && ["auto_review", "approve_for_me"].includes(session?.permissionProfile.approvalsReviewer?.replaceAll("-", "_") ?? ""));
  const fullAccess = $derived(session?.permissionProfile.mode === "full_access");

  $effect(() => {
    const key = session?.agent === "codex" ? `${session.agent}:${session.nativeSessionId ?? session.id}` : "";
    if (!key || key === requestedUsageKey) return;
    requestedUsageKey = key;
    void refreshUsage();
  });

  function tr(english: string, portuguese: string) {
    return language === "pt-BR" ? portuguese : english;
  }

  function remainingRate(used: number) {
    return Math.max(0, Math.min(100, Math.round(100 - used)));
  }

  function rateWindowLabel(windowMinutes?: number, fallback = "") {
    if (windowMinutes) {
      if (windowMinutes >= 1_440) return `${Math.round(windowMinutes / 1_440)}d`;
      if (windowMinutes >= 60) return `${Math.round(windowMinutes / 60)}h`;
      return `${windowMinutes}m`;
    }
    return fallback.match(/\b\d+\s*[dhm]\b/i)?.[0]?.replaceAll(" ", "") ?? fallback;
  }

  function sessionSource(session: AgentSession) {
    if (session.source === "web") return session.sourceApp ?? "Web";
    return { cli: "CLI", vscode: "VS Code", desktop: "Lume" }[session.source];
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

  async function refreshUsage() {
    if (!session || session.agent !== "codex" || usageRefreshing) return;
    usageRefreshing = true;
    try {
      await refreshAgentRateLimits("codex");
    } catch { /* Keep the last known usage without adding noisy inspector copy. */ }
    finally {
      usageRefreshing = false;
    }
  }
</script>

<aside class="workspace-inspector" class:orb-inspector={variant === "orb"} aria-label={tr("Session inspector", "Inspector da sessão")}>
  <header>
    {#if session}
      <span class="agent-icon"><BrandIcon name={session.agent} size={21} /></span>
      <span class="header-copy">
        <strong>{session.sessionName?.trim() || session.project || session.agentLabel}</strong>
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
    <div class="inspector-scroll">
      <section class="session-overview" aria-label={tr("Session overview", "Resumo da sessão")}>
        <span><strong>{session.activityTotal ?? session.activities.length}</strong><small>{tr("events", "eventos")}</small></span>
        <span><strong>{session.results.length}</strong><small>{tr("results", "resultados")}</small></span>
        <span><strong>{subagents.length}</strong><small>{tr("subagents", "subagentes")}</small></span>
      </section>

      {#if session.agent === "codex"}
        <section class="usage-section" aria-label={tr("Codex usage", "Uso do Codex")}>
          <div class="usage-gauges" class:loading={usageRefreshing && !session.rateLimits?.length}>
            {#each session.rateLimits ?? [] as limit (limit.id)}
              {@const remaining = remainingRate(Number(limit.usedPercent))}
              <div class="usage-gauge" style:--usage-remaining={remaining}>
                <svg viewBox="0 0 60 39" aria-hidden="true">
                  <path class="gauge-track" pathLength="100" d="M7 33a23 23 0 0 1 46 0" />
                  <path class="gauge-progress" pathLength="100" d="M7 33a23 23 0 0 1 46 0" style:stroke-dasharray={`${remaining} 100`} />
                </svg>
                <strong>{remaining}</strong>
                <em>{rateWindowLabel(limit.windowMinutes, limit.label)}</em>
              </div>
            {/each}
            {#if !session.rateLimits?.length}<i></i><i></i>{/if}
          </div>
          <div class="token-chart">
            <span>{tr("Tokens / prompt", "Tokens / prompt")}</span>
            <svg class:empty={!tokenGraph.points} class="token-graph" viewBox="0 0 226 54" role="img" aria-label={tr("Tokens used per prompt over time", "Tokens usados por prompt ao longo do tempo")}>
              <path class="graph-grid" d="M4 14H222M4 28H222M4 42H222" />
              <polygon points={tokenGraph.area} />
              <polyline points={tokenGraph.points || "4,42 222,42"} />
              {#each tokenGraph.dots as dot}
                <circle cx={dot.x} cy={dot.y} r="2"><title>{dot.tokens.toLocaleString(language)} tokens</title></circle>
              {/each}
            </svg>
            <small>{tr("Time →", "Tempo →")}</small>
          </div>
        </section>
      {/if}

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

      <details class="inspector-section" open={checks.length > 0}>
        <summary><span>{tr("Validations", "Validações")}</span><em>{checks.length}</em></summary>
        <p class="section-description">{tr("Tests, builds, and checks reported by the agent in the latest turn.", "Testes, builds e verificações reportados pelo agente no último turno.")}</p>
        {#if checks.length}
          <ul class="check-list">{#each checks as check}<li title={check}><i><LumeIcon name="check" size={9} /></i><span>{check}</span></li>{/each}</ul>
        {:else}
          <p class="empty-section">{tr("No validation evidence was reported.", "Nenhuma evidência de validação foi reportada.")}</p>
        {/if}
      </details>

      <details class="inspector-section metadata-section">
        <summary>{tr("Session details", "Detalhes da sessão")}</summary>
        <dl>
          <div><dt>{tr("Source", "Origem")}</dt><dd>{sessionSource(session)}</dd></div>
          <div><dt>{tr("Control", "Controle")}</dt><dd>{session.controlOrigin === "lume" ? tr("Managed by Lume", "Gerenciado pelo Lume") : tr("Externally owned", "Controle externo")}</dd></div>
          <div><dt>{tr("Process ID", "ID do processo")}</dt><dd>{session.processId ?? tr("Not reported by source", "Não informado pela origem")}</dd></div>
          <div><dt>{tr("Directory", "Diretório")}</dt><dd title={session.workingDirectory}>{session.workingDirectory || "—"}</dd></div>
          <div><dt>{tr("Thread", "Thread")}</dt><dd title={session.nativeSessionId}>{session.nativeSessionId || "—"}</dd></div>
        </dl>
      </details>
    </div>
  {:else}
    <div class="inspector-empty"><BrandIcon name="lume" size={30} /><span>{tr("Select an agent to inspect its work.", "Selecione um agente para inspecionar o trabalho.")}</span></div>
  {/if}
</aside>

<style>
  .workspace-inspector { min-width: 0; height: 100%; display: grid; grid-template-rows: auto minmax(0, 1fr); overflow: hidden; border-left: 1px solid var(--workspace-line); color: var(--workspace-text); background: var(--workspace-sidebar); }
  .workspace-inspector.orb-inspector { --workspace-accent: #397b5c; --workspace-accent-soft: rgba(57, 123, 92, .11); --workspace-faint: #87968e; --workspace-line: rgba(92, 114, 103, .15); --workspace-muted: #697a71; --workspace-raised: rgba(255, 255, 255, .44); --workspace-scroll-thumb: #cad2ce; --workspace-sidebar: transparent; --workspace-strong: #34473d; --workspace-subtle: rgba(74, 108, 90, .07); --workspace-text: #526158; border-left: 0; }
  :global(.overlay-shell.dark) .workspace-inspector.orb-inspector { --workspace-accent: #83c29f; --workspace-accent-soft: rgba(100, 180, 143, .11); --workspace-faint: #8b9a92; --workspace-line: rgba(199, 218, 207, .11); --workspace-muted: #a0b0a7; --workspace-raised: rgba(206, 228, 216, .045); --workspace-scroll-thumb: rgba(199, 218, 207, .32); --workspace-strong: #e0e9e4; --workspace-subtle: rgba(206, 228, 216, .055); --workspace-text: #c5d2cb; }
  .workspace-inspector.orb-inspector > header { min-height: 50px; padding: 7px 8px; }
  .workspace-inspector.orb-inspector > header .header-copy strong { font-size: 11px; }
  .workspace-inspector.orb-inspector > header .header-copy small { font-size: 8px; }
  .workspace-inspector.orb-inspector .inspector-scroll { padding: 0 9px 14px; }
  .workspace-inspector.orb-inspector .inspector-section > summary { min-height: 40px; font-size: 9px; }
  .workspace-inspector > header { min-height: 64px; padding: 10px 12px 10px 15px; display: flex; align-items: center; gap: 9px; border-bottom: 1px solid var(--workspace-line); }
  .header-copy { min-width: 0; display: grid; gap: 2px; flex: 1; }.header-copy strong, .header-copy small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.header-copy strong { color: var(--workspace-strong); font-size: 12px; letter-spacing: -.015em; }.header-copy small { color: var(--workspace-muted); font-size: 9px; }
  .agent-icon { width: 31px; height: 31px; display: grid; place-items: center; flex: 0 0 auto; color: var(--workspace-accent); }
  .inspector-access-badge { min-height: 21px; padding: 0 6px; display: inline-flex; align-items: center; gap: 3px; flex: 0 0 auto; border-radius: 999px; font-size: 7px; font-weight: 780; line-height: 1; white-space: nowrap; }.inspector-access-badge svg { width: 9px; height: 9px; flex: 0 0 auto; fill: none; stroke: currentColor; stroke-width: 1.35; }.inspector-access-badge.auto-review { color: #315f86; background: #cbdff0; }.inspector-access-badge.auto-review svg { fill: currentColor; stroke: none; }.inspector-access-badge.full-access { color: #764c2e; background: #e8ceb1; }
  :global(.workspace.dark) .inspector-access-badge.auto-review { color: #b4d3ee; background: #29445d; }:global(.workspace.dark) .inspector-access-badge.full-access { color: #e4b88f; background: #543b29; }
  .workspace-inspector > header button { width: 29px; height: 29px; padding: 0; display: grid; place-items: center; flex: 0 0 auto; border: 0; border-radius: 8px; color: var(--workspace-muted); background: transparent; cursor: pointer; }.workspace-inspector > header button:hover { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .inspector-scroll { min-height: 0; padding: 0 15px 24px; overflow-y: auto; overscroll-behavior: contain; scrollbar-width: thin; scrollbar-color: var(--workspace-scroll-thumb) transparent; }
  .session-overview { min-height: 58px; display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); align-items: center; border-bottom: 1px solid var(--workspace-line); }.session-overview > span { min-width: 0; display: grid; gap: 2px; text-align: center; }.session-overview > span + span { border-left: 1px solid var(--workspace-line); }.session-overview strong { color: var(--workspace-strong); font-size: 13px; font-variant-numeric: tabular-nums; }.session-overview small { color: var(--workspace-faint); font-size: 8px; text-transform: uppercase; letter-spacing: .045em; }
  .usage-section { min-height: 72px; margin: 13px 0 3px; display: grid; grid-template-columns: minmax(126px, 47%) minmax(0, 1fr); align-items: center; gap: 9px; }
  .usage-gauges { min-width: 0; display: flex; align-items: center; justify-content: center; gap: 5px; }.usage-gauge { position: relative; width: 62px; height: 51px; flex: 0 1 62px; --usage-color: color-mix(in srgb, #43a873 calc(var(--usage-remaining) * 1%), #ca605c); }.usage-gauge svg { position: absolute; inset: 0; width: 100%; height: 100%; overflow: visible; fill: none; stroke-linecap: round; }.gauge-track { stroke: var(--workspace-line); stroke-width: 5.5; }.gauge-progress { stroke: var(--usage-color); stroke-width: 5.5; transition: stroke-dasharray 360ms cubic-bezier(.16, 1, .3, 1); }.usage-gauge strong { position: absolute; right: 0; bottom: 3px; left: 0; color: var(--workspace-strong); font-size: 15px; font-variant-numeric: tabular-nums; line-height: 1; text-align: center; }.usage-gauge em { position: absolute; top: 1px; right: 2px; color: var(--workspace-muted); font-size: 7px; font-style: normal; font-weight: 780; }.usage-gauges > i { width: 56px; height: 30px; border: 5px solid var(--workspace-line); border-bottom: 0; border-radius: 32px 32px 0 0; opacity: .5; }.usage-gauges.loading > i { animation: usage-pulse 1.2s ease-in-out infinite alternate; }
  .token-chart { min-width: 0; display: grid; grid-template-rows: auto 48px auto; gap: 3px; }.token-chart > span, .token-chart > small { color: var(--workspace-muted); font-size: 10px; font-weight: 720; line-height: 1.2; }.token-chart > small { color: var(--workspace-faint); text-align: right; }.token-graph { width: 100%; min-width: 0; height: 48px; overflow: visible; }.token-graph .graph-grid { fill: none; stroke: color-mix(in srgb, var(--workspace-line) 54%, transparent); stroke-width: .7; }.token-graph polygon { fill: color-mix(in srgb, var(--workspace-accent) 8%, transparent); }.token-graph polyline { fill: none; stroke: var(--workspace-accent); stroke-width: 1.4; stroke-linecap: round; stroke-linejoin: round; }.token-graph.empty polyline { stroke: var(--workspace-line); stroke-dasharray: 3 4; }.token-graph circle { fill: var(--workspace-raised); stroke: var(--workspace-accent); stroke-width: 1.3; }
  .inspector-section { border-bottom: 1px solid var(--workspace-line); }.inspector-section > summary { min-height: 46px; display: flex; align-items: center; gap: 7px; color: var(--workspace-strong); font-size: 10px; font-weight: 750; list-style: none; cursor: pointer; }.inspector-section > summary::-webkit-details-marker { display: none; }.inspector-section > summary::after { width: 6px; height: 6px; margin-left: auto; border-right: 1.5px solid currentColor; border-bottom: 1.5px solid currentColor; content: ""; opacity: .5; transform: rotate(45deg); transition: transform 160ms ease; }.inspector-section[open] > summary::after { transform: rotate(225deg); }.inspector-section > summary em { min-width: 19px; height: 19px; display: grid; place-items: center; border-radius: 6px; color: var(--workspace-muted); background: var(--workspace-subtle); font-size: 8px; font-style: normal; }.inspector-section[open] { padding-bottom: 13px; }
  .check-list { margin: 0; padding: 0; display: grid; gap: 7px; list-style: none; }.check-list li { min-width: 0; display: flex; align-items: flex-start; gap: 7px; color: var(--workspace-muted); font-size: 9px; line-height: 1.45; }
  .subagent-list { display: grid; gap: 3px; }.subagent-list > div { min-width: 0; min-height: 36px; padding: 4px 5px; display: flex; align-items: center; gap: 8px; border-radius: 8px; }.subagent-list > div:hover { background: var(--workspace-subtle); }.subagent-list > div > span { min-width: 0; display: grid; gap: 2px; }.subagent-list strong { overflow: hidden; color: var(--workspace-text); font-size: 9px; text-overflow: ellipsis; white-space: nowrap; }.subagent-list small { color: var(--workspace-faint); font-size: 8px; }.subagent-list small.status-running { color: #4e98ca; }.subagent-list small.status-failed { color: #c66762; }.subagent-list small.status-completed { color: #50aa79; }
  .files-toolbar { min-height: 40px; margin-bottom: 4px; padding-bottom: 8px; display: flex; align-items: center; gap: 8px; border-bottom: 1px solid color-mix(in srgb, var(--workspace-line) 65%, transparent); }.files-toolbar > span { min-width: 0; display: grid; gap: 2px; flex: 1; }.files-toolbar > span > strong { color: var(--workspace-text); font-size: 9px; }.files-toolbar > span small { display: flex; gap: 5px; font-size: 8px; }.files-toolbar b, .inspector-files b { color: #43a873; }.files-toolbar i, .inspector-files i { color: #c16660; font-style: normal; }.files-toolbar button { min-height: 28px; padding: 0 8px; display: inline-flex; align-items: center; gap: 5px; border: 1px solid color-mix(in srgb, var(--workspace-accent) 28%, var(--workspace-line)); border-radius: 7px; color: var(--workspace-accent); background: var(--workspace-accent-soft); font-size: 8px; font-weight: 740; cursor: pointer; }.files-toolbar button:hover { border-color: color-mix(in srgb, var(--workspace-accent) 52%, var(--workspace-line)); }
  .inspector-files { display: grid; gap: 1px; }.inspector-files > button, .inspector-files > .inspector-file-entry { width: 100%; min-width: 0; min-height: 31px; padding: 0 5px; display: flex; align-items: center; gap: 7px; border: 0; border-radius: 7px; color: var(--workspace-text); background: transparent; font-size: 9px; text-align: left; }.inspector-files > button { cursor: pointer; }.inspector-files > button:hover { background: var(--workspace-subtle); }.inspector-files > button > span, .inspector-file-entry > span { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.inspector-files > button > small, .inspector-file-entry > small { display: flex; gap: 4px; flex: 0 0 auto; font-size: 7px; }.review-more { margin: 5px 0 0 4px; padding: 3px 0; border: 0; color: var(--workspace-accent); background: transparent; font-size: 8px; font-weight: 720; cursor: pointer; }
  .section-description { margin: -1px 0 9px; color: var(--workspace-muted); font-size: 8px; line-height: 1.45; }.check-list li > i { width: 15px; height: 15px; margin-top: 0; display: grid; place-items: center; flex: 0 0 auto; border-radius: 50%; color: #43a873; background: color-mix(in srgb, #43a873 12%, transparent); }.check-list li span { display: -webkit-box; overflow: hidden; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; }
  .empty-section { margin: 0; padding: 0 0 5px; color: var(--workspace-faint); font-size: 9px; line-height: 1.45; }
  .metadata-section dl { margin: 0; display: grid; gap: 9px; }.metadata-section dl > div { min-width: 0; display: grid; grid-template-columns: 60px minmax(0, 1fr); gap: 8px; }.metadata-section dt { color: var(--workspace-faint); font-size: 8px; }.metadata-section dd { margin: 0; overflow: hidden; color: var(--workspace-text); font-size: 9px; text-overflow: ellipsis; text-transform: capitalize; white-space: nowrap; }
  .inspector-empty { margin: auto; padding: 24px; display: grid; justify-items: center; gap: 10px; color: var(--workspace-faint); font-size: 10px; text-align: center; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @keyframes usage-pulse { to { opacity: .9; } }
  @media (max-width: 300px) { .session-overview small { font-size: 7px; } }
  @media (prefers-reduced-motion: reduce) { .inspector-section > summary::after, .gauge-progress { transition: none; }.usage-gauges.loading > i { animation: none; } }
</style>
