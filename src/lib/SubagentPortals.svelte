<script lang="ts">
  import { onMount, tick } from "svelte";
  import type { SessionActivity } from "$lib/domain";
  import type { HubSession } from "$lib/hubProtocol";
  import type { Language } from "$lib/i18n";
  import { noteSubagentInteraction, type WorkspaceSubagent } from "$lib/workspaceAgents";
  import { activityCategory, activityDisplayTitle, isGenericAnalysisPlaceholder } from "$lib/activityPresentation";
  import { loadSubagentTimeline, submitPrompt } from "$lib/lume";
  import { renderSafeMarkdown } from "$lib/markdown.js";
  import { collectReviewFiles, parseReviewDiff } from "$lib/reviewDiffs";
  import ActivityTypeIcon from "$lib/ActivityTypeIcon.svelte";
  import FileTypeIcon from "$lib/FileTypeIcon.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import ThreadAvatar from "$lib/ThreadAvatar.svelte";

  let { session, children, language }: { session: HubSession; children: WorkspaceSubagent[]; language: Language } = $props();
  let root: HTMLElement;
  let rail: HTMLDivElement;
  let timelineElement = $state<HTMLDivElement | null>(null);
  let portalOrigin = $state(32);
  let showLeftOverflow = $state(false);
  let showRightOverflow = $state(false);
  let railCollapsed = $state(false);
  let selectedId = $state<string | null>(null);
  let expanded = $state(false);
  let timelines = $state<Record<string, SessionActivity[]>>({});
  let errors = $state<Record<string, string>>({});
  let loading = $state<Record<string, boolean>>({});
  let messageOpen = $state(false);
  let messageDraft = $state("");
  let messageSending = $state(false);
  let messageNotice = $state("");
  let mounted = false;
  let currentSessionId = "";
  const lastFetched = new Map<string, number>();
  const inFlight = new Set<string>();
  const compactChildren = $derived(children.slice(-3));
  const compactOverflow = $derived(Math.max(0, children.length - 3));
  const selectedChild = $derived(children.find((child) => child.id === selectedId));
  const selectedActivities = $derived(selectedId ? timelines[selectedId] ?? [] : []);
  const selectedFinal = $derived(selectedActivities.findLast((activity) => activity.kind === "message" && activity.title === "Resposta final")
    ?? (selectedChild?.status === "completed" ? selectedActivities.findLast((activity) => activity.kind === "message") : undefined));
  const selectedEvents = $derived(selectedActivities.filter((activity) => activity.id !== selectedFinal?.id));
  const selectedChangedFiles = $derived.by(() => {
    const fileActivities = selectedActivities.filter((activity) => activity.kind === "file" && activity.status === "completed");
    const changes = collectReviewFiles(fileActivities, session.workingDirectory);
    const seen = new Set(changes.map((change) => change.path));
    for (const activity of fileActivities) {
      for (const path of activity.files) {
        if (seen.has(path)) continue;
        changes.push({ path, added: 0, removed: 0 });
        seen.add(path);
      }
    }
    return changes.sort((left, right) => left.path.localeCompare(right.path));
  });
  const canMessageChild = $derived(Boolean(
    selectedChild
    && selectedChild.status === "running"
    && selectedChild.path.startsWith("/")
    && session.agent === "codex"
    && session.status === "running"
    && session.capabilities.canPrompt
    && session.capabilities.promptDeliveries.includes("steer"),
  ));

  $effect(() => {
    if (!expanded || !selectedId) return;
    // Track the selected timeline's updates, including a final response growing in place.
    void selectedActivities;
    const childId = selectedId;
    void tick().then(() => {
      if (expanded && selectedId === childId && timelineElement) {
        timelineElement.scrollTop = timelineElement.scrollHeight;
      }
    });
  });

  function tr(english: string, portuguese: string) {
    return language === "pt-BR" ? portuguese : english;
  }

  function statusText(status: WorkspaceSubagent["status"]) {
    if (status === "running") return tr("Working", "Executando");
    if (status === "completed") return tr("Finished", "Concluído");
    if (status === "failed") return tr("Failed", "Falhou");
    if (status === "waiting") return tr("Waiting", "Aguardando");
    return tr("Interrupted", "Interrompido");
  }

  function timeText(value: number) {
    return new Date(value).toLocaleTimeString(language === "pt-BR" ? "pt-BR" : "en-US", { hour: "2-digit", minute: "2-digit" });
  }

  function activityText(activity: SessionActivity) {
    if (activity.kind === "message" || activity.kind === "analysis" || activity.kind === "plan") {
      return activity.detail?.trim() || activity.title;
    }
    return activity.files[0] || "";
  }

  function updatePortalOrigin() {
    if (!selectedId || !root || !rail) return;
    const selector = railCollapsed ? ".compact-agent" : ".portal-button";
    const button = [...root.querySelectorAll<HTMLButtonElement>(selector)]
      .find((item) => item.dataset.childId === selectedId);
    if (!button) return;
    const buttonRect = button.getBoundingClientRect();
    const rootRect = root.getBoundingClientRect();
    portalOrigin = Math.max(24, Math.min(rootRect.width - 24, buttonRect.left + buttonRect.width / 2 - rootRect.left - 8));
  }

  async function setRailCollapsed(collapsed: boolean) {
    railCollapsed = collapsed;
    await tick();
    root.querySelector<HTMLButtonElement>(collapsed ? ".compact-expand" : ".portal-rail-toolbar button")?.focus({ preventScroll: true });
  }

  function collapseRailOnBackgroundDoubleClick(event: MouseEvent) {
    if (event.target instanceof Element && event.target.closest(".portal-button")) return;
    void setRailCollapsed(true);
  }

  function updateRailOverflow() {
    if (!rail) return;
    const maxScroll = Math.max(0, rail.scrollWidth - rail.clientWidth);
    showLeftOverflow = rail.scrollLeft > 2;
    showRightOverflow = maxScroll - rail.scrollLeft > 2;
  }

  function onRailScroll() {
    updatePortalOrigin();
    updateRailOverflow();
  }

  function scrollRail(direction: -1 | 1) {
    if (!rail) return;
    rail.scrollBy({
      left: direction * Math.max(144, Math.floor(rail.clientWidth * 0.72)),
      behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth",
    });
  }

  function closePortal(restoreFocus = false) {
    expanded = false;
    messageOpen = false;
    if (restoreFocus) {
      queueMicrotask(() => {
        [...root.querySelectorAll<HTMLButtonElement>(railCollapsed ? ".compact-agent" : ".portal-button")]
          .find((item) => item.dataset.childId === selectedId)?.focus();
      });
    }
  }

  async function refreshChild(child: WorkspaceSubagent) {
    if (session.agent !== "codex" || !child.id.startsWith("codex:")) return;
    const requestSessionId = session.id;
    const requestKey = `${requestSessionId}:${child.id}`;
    if (inFlight.has(requestKey)) return;
    inFlight.add(requestKey);
    loading = { ...loading, [child.id]: true };
    try {
      const activities = await loadSubagentTimeline(requestSessionId, child.id);
      if (session.id !== requestSessionId) return;
      timelines = { ...timelines, [child.id]: activities.filter((activity) => !isGenericAnalysisPlaceholder(activity)) };
      errors = { ...errors, [child.id]: "" };
      lastFetched.set(child.id, Date.now());
    } catch (error) {
      if (session.id !== requestSessionId) return;
      errors = { ...errors, [child.id]: String(error) };
      lastFetched.set(child.id, Date.now());
    } finally {
      inFlight.delete(requestKey);
      if (session.id === requestSessionId) loading = { ...loading, [child.id]: false };
      queueMicrotask(refreshVisible);
    }
  }

  function refreshVisible() {
    if (!mounted || document.hidden || session.agent !== "codex") return;
    const now = Date.now();
    for (const child of children) {
      if (inFlight.size >= 2) break;
      if (!child.id.startsWith("codex:")) continue;
      if (inFlight.has(`${session.id}:${child.id}`)) continue;
      const fetched = lastFetched.get(child.id) ?? 0;
      if (fetched && child.status !== "running" && child.id !== selectedId) continue;
      if (now - fetched < 8_000) continue;
      void refreshChild(child);
    }
  }

  function togglePortal(childId: string) {
    if (selectedId === childId && expanded) {
      closePortal();
      return;
    }
    noteSubagentInteraction(childId);
    if (selectedId !== childId) {
      messageOpen = false;
      messageDraft = "";
      messageNotice = "";
    }
    selectedId = childId;
    updatePortalOrigin();
    expanded = true;
    const child = children.find((item) => item.id === childId);
    if (child && Date.now() - (lastFetched.get(childId) ?? 0) > 4_000) void refreshChild(child);
  }

  async function sendToSubagent() {
    const child = selectedChild;
    const message = messageDraft.trim();
    if (!child || !canMessageChild || !message || messageSending) return;
    messageSending = true;
    messageNotice = "";
    try {
      const routedPrompt = [
        "The Lume user wants to message an existing subagent. Use your collaboration send_message tool to relay the exact message below to the target path. Do not perform the subagent's task yourself. If the subagent is unavailable, tell the user that delivery failed.",
        JSON.stringify({ target: child.path, message }),
      ].join("\n\n");
      await submitPrompt(session.id, routedPrompt, [], "steer");
      noteSubagentInteraction(child.id);
      messageDraft = "";
      messageNotice = tr("Request sent to the main agent for delivery.", "Pedido enviado ao agente principal para encaminhamento.");
    } catch (error) {
      messageNotice = String(error).replace(/^Error:\s*/, "");
    } finally {
      messageSending = false;
    }
  }

  $effect(() => {
    const fingerprint = `${session.id}|${children.map((child) => `${child.id}:${child.status}`).join("|")}`;
    if (!mounted || !fingerprint) return;
    if (currentSessionId !== session.id) {
      currentSessionId = session.id;
      selectedId = null;
      expanded = false;
      railCollapsed = false;
      messageOpen = false;
      messageDraft = "";
      messageNotice = "";
      timelines = {};
      errors = {};
      lastFetched.clear();
    }
    if (selectedId && !children.some((child) => child.id === selectedId)) expanded = false;
    refreshVisible();
    void tick().then(updateRailOverflow);
  });

  onMount(() => {
    mounted = true;
    currentSessionId = session.id;
    refreshVisible();
    const resizeObserver = new ResizeObserver(updateRailOverflow);
    resizeObserver.observe(rail);
    const mutationObserver = new MutationObserver(() => void tick().then(updateRailOverflow));
    mutationObserver.observe(rail, { childList: true });
    void tick().then(updateRailOverflow);
    const timer = window.setInterval(refreshVisible, 8_000);
    const closeOutside = (event: PointerEvent) => {
      if (expanded && root && !root.contains(event.target as Node)) closePortal();
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && expanded) closePortal(true);
    };
    document.addEventListener("pointerdown", closeOutside);
    window.addEventListener("keydown", closeOnEscape);
    document.addEventListener("visibilitychange", refreshVisible);
    return () => {
      mounted = false;
      window.clearInterval(timer);
      resizeObserver.disconnect();
      mutationObserver.disconnect();
      document.removeEventListener("pointerdown", closeOutside);
      window.removeEventListener("keydown", closeOnEscape);
      document.removeEventListener("visibilitychange", refreshVisible);
    };
  });
</script>

<section class:compact={railCollapsed} class="subagent-portals" bind:this={root} aria-label={tr("Subagent activity", "Atividade dos subagentes")}
  style={`--portal-origin:${portalOrigin}px`}
  style:background={railCollapsed ? "transparent" : undefined}
  style:border-bottom-color={railCollapsed ? "transparent" : undefined}>
  <div class:visible={railCollapsed} class="portal-compact" role="group" aria-label={tr("Subagents", "Subagentes")} aria-hidden={!railCollapsed} inert={!railCollapsed}>
    <div class="portal-compact-inner">
      <div class="compact-avatar-stack">
        {#each compactChildren as child, index (child.id)}
          <button
            type="button"
            class="compact-agent status-{child.status}"
            data-child-id={child.id}
            style={`--stack-index:${index}`}
            aria-label={tr(`Show ${child.label} timeline · ${statusText(child.status)}`, `Mostrar timeline de ${child.label} · ${statusText(child.status)}`)}
            aria-expanded={selectedId === child.id && expanded}
            aria-controls={`subagent-timeline-${session.id}`}
            title={`${child.label} · ${statusText(child.status)}`}
            onclick={() => togglePortal(child.id)}
          >
            <span class="compact-agent-avatar"><ThreadAvatar seed={`${session.id}:subagent:${child.id}`} label={child.label} size={23} /></span>
            <i aria-hidden="true"></i>
          </button>
        {/each}
        {#if compactOverflow > 0}
          <span class="compact-more" role="img" aria-label={tr(`${compactOverflow} more subagents`, `${compactOverflow} subagentes a mais`)}>+{compactOverflow}</span>
        {/if}
      </div>
      <button type="button" class="compact-expand" aria-expanded={false} aria-controls={`subagent-rail-${session.id}`} aria-label={tr("Show subagent rail", "Abrir trilho de subagentes")} title={tr("Show subagent rail", "Abrir trilho de subagentes")} onclick={() => void setRailCollapsed(false)}>
        <svg viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="m4 6 4 4 4-4" /></svg>
      </button>
    </div>
  </div>

  <div class:collapsed={railCollapsed} class="portal-rail-content" aria-hidden={railCollapsed} inert={railCollapsed}>
    <div class="portal-rail-content-inner">
      <div class="portal-rail-toolbar">
        <span>{tr("Subagents", "Subagentes")} <b>{children.length}</b></span>
        <button type="button" aria-expanded={true} aria-controls={`subagent-rail-${session.id}`} aria-label={tr("Minimize subagent rail", "Recolher trilho de subagentes")} title={tr("Minimize subagent rail", "Recolher trilho de subagentes")} onclick={() => void setRailCollapsed(true)}>
          <svg viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="m4 10 4-4 4 4" /></svg>
        </button>
      </div>

      <div id={`subagent-rail-${session.id}`} class="portal-rail-shell" role="group" aria-label={tr("Subagent rail", "Trilho de subagentes")} ondblclick={collapseRailOnBackgroundDoubleClick}>
        <div class="portal-rail" bind:this={rail} onscroll={onRailScroll}>
          {#each children as child (child.id)}
            {@const recent = (timelines[child.id] ?? []).slice(-3)}
            <button
              type="button"
              class:chosen={selectedId === child.id && expanded}
              class="portal-button status-{child.status}"
              data-child-id={child.id}
              aria-label={tr(`Show ${child.label} timeline · ${statusText(child.status)}`, `Mostrar timeline de ${child.label} · ${statusText(child.status)}`)}
              aria-expanded={selectedId === child.id && expanded}
              aria-controls={`subagent-timeline-${session.id}`}
              title={`${child.label} · ${statusText(child.status)}`}
              onclick={() => togglePortal(child.id)}
            >
              <span class="portal-surface" aria-hidden="true">
                <span class="portal-feed">
                  {#if recent.length}
                    {#each recent as activity (activity.id)}
                      <span class="portal-feed-row"><i class:live={activity.status === "running"}></i><b>{activityDisplayTitle(activity, language)}</b></span>
                    {/each}
                  {:else}
                    <span class="portal-feed-row"><i class:live={child.status === "running"}></i><b>{statusText(child.status)}</b></span>
                    <span class="portal-feed-row faint"><i></i><b>{child.label}</b></span>
                  {/if}
                </span>
              </span>
              <span class="portal-avatar" aria-hidden="true"><ThreadAvatar seed={`${session.id}:subagent:${child.id}`} label={child.label} size={20} /></span>
              <span class="portal-name">{child.label}</span>
            </button>
          {/each}
        </div>
        {#if showLeftOverflow}
          <button type="button" class="portal-overflow-control left" aria-label={tr("Show previous subagents", "Mostrar subagentes anteriores")} title={tr("Show previous subagents", "Mostrar subagentes anteriores")} onclick={() => scrollRail(-1)}>
            <span aria-hidden="true"><svg viewBox="0 0 16 16" fill="none"><path d="m10 3-5 5 5 5" /></svg></span>
          </button>
        {/if}
        {#if showRightOverflow}
          <button type="button" class="portal-overflow-control right" aria-label={tr("Show more subagents", "Mostrar mais subagentes")} title={tr("Show more subagents", "Mostrar mais subagentes")} onclick={() => scrollRail(1)}>
            <span aria-hidden="true"><svg viewBox="0 0 16 16" fill="none"><path d="m6 3 5 5-5 5" /></svg></span>
          </button>
        {/if}
      </div>
    </div>
  </div>

  <div id={`subagent-timeline-${session.id}`} class:open={expanded} class:has-final={Boolean(selectedFinal)} class="portal-panel" aria-hidden={!expanded} inert={!expanded}>
    {#if selectedChild}
      <header class="portal-panel-header">
        <span class="panel-avatar"><ThreadAvatar seed={`${session.id}:subagent:${selectedChild.id}`} label={selectedChild.label} size={28} /></span>
        <span class="panel-heading"><strong>{selectedChild.label}</strong><small>{statusText(selectedChild.status)}</small></span>
        {#if session.agent === "codex"}
          <button type="button" class:active={messageOpen} disabled={!canMessageChild} aria-label={tr(`Message ${selectedChild.label}`, `Enviar mensagem para ${selectedChild.label}`)} aria-expanded={messageOpen} title={canMessageChild ? tr("Message via the main agent", "Enviar pelo agente principal") : tr("Messaging requires an active Codex subagent controlled by Lume", "O envio exige um subagente Codex ativo controlado pelo Lume")} onclick={() => messageOpen = !messageOpen}><LumeIcon name="send" size={14} /></button>
        {/if}
        <button type="button" aria-label={tr("Close timeline", "Fechar timeline")} onclick={() => closePortal(true)}><LumeIcon name="close" size={14} /></button>
      </header>
      <div class="portal-timeline" bind:this={timelineElement} role="list" aria-label={tr(`Timeline for ${selectedChild.label}`, `Timeline de ${selectedChild.label}`)}>
        {#if selectedActivities.length}
          {#each selectedEvents as activity (activity.id)}
            <div class:live={activity.status === "running"} class="timeline-entry" role="listitem">
              <span class="entry-rail"><i></i></span>
              <span class="entry-icon"><ActivityTypeIcon category={activity.kind === "analysis" ? "analysis" : activityCategory(activity)} size={14} /></span>
              <div class="entry-copy">
                <strong>{activityDisplayTitle(activity, language)}</strong>
                {#if activity.kind === "message" && activity.detail}
                  <div class="entry-message">{@html renderSafeMarkdown(activity.detail)}</div>
                {:else if activityText(activity)}
                  <small>{activityText(activity)}</small>
                {/if}
              </div>
              <time datetime={new Date(activity.createdAt).toISOString()}>{timeText(activity.createdAt)}</time>
            </div>
          {/each}
          {#if selectedChangedFiles.length}
            <section class="subagent-files" aria-label={tr("Files changed by this subagent", "Arquivos alterados por este subagente")}>
              <strong>{tr(`${selectedChangedFiles.length} file${selectedChangedFiles.length === 1 ? "" : "s"} changed`, `${selectedChangedFiles.length} arquivo${selectedChangedFiles.length === 1 ? "" : "s"} alterado${selectedChangedFiles.length === 1 ? "" : "s"}`)}</strong>
              <div class="subagent-file-list">
                {#each selectedChangedFiles as file (file.path)}
                  <details class="subagent-file">
                    <summary title={file.path}><FileTypeIcon path={file.path} /><span>{file.path.split(/[\\/]/).at(-1) || file.path}</span>{#if file.added || file.removed}<small>+{file.added} −{file.removed}</small>{/if}<LumeIcon name="chevron-down" size={12} /></summary>
                    {#if file.diff}
                      <div class="subagent-diff" aria-label={tr(`Diff for ${file.path}`, `Diff de ${file.path}`)}>
                        {#each parseReviewDiff(file.diff) as line, index (index)}
                          <div class="diff-line kind-{line.kind}"><code>{line.kind === "added" ? "+" : line.kind === "removed" ? "−" : " "}{line.content}</code></div>
                        {/each}
                      </div>
                    {:else}
                      <p class="diff-unavailable">{tr("This subagent did not provide a recorded diff for this file.", "Este subagente não forneceu um diff registrado deste arquivo.")}</p>
                    {/if}
                  </details>
                {/each}
              </div>
            </section>
          {/if}
          {#if selectedFinal?.detail}
            <section class="subagent-final" role="listitem" aria-label={tr("Final response", "Resposta final")}>
              <header><LumeIcon name="check" size={15} /><strong>{tr("Final response", "Resposta final")}</strong><time datetime={new Date(selectedFinal.createdAt).toISOString()}>{timeText(selectedFinal.createdAt)}</time></header>
              <div class="final-content">{@html renderSafeMarkdown(selectedFinal.detail)}</div>
            </section>
          {/if}
        {:else if loading[selectedChild.id]}
          <p class="timeline-empty">{tr("Loading activity…", "Carregando atividade…")}</p>
        {:else if errors[selectedChild.id]}
          <p class="timeline-empty">{tr("This timeline is not available yet.", "Esta timeline ainda não está disponível.")}</p>
        {:else if session.agent !== "codex"}
          <div class="timeline-entry" role="listitem">
            <span class="entry-rail"><i></i></span>
            <span class="entry-icon"><ActivityTypeIcon category="tool" size={14} /></span>
            <span class="entry-copy"><strong>{tr("Subagent started", "Subagente iniciado")}</strong></span>
            <time datetime={new Date(selectedChild.startedAt).toISOString()}>{timeText(selectedChild.startedAt)}</time>
          </div>
          {#if selectedChild.status !== "running" && selectedChild.updatedAt > selectedChild.startedAt}
            <div class="timeline-entry" role="listitem">
              <span class="entry-rail"><i></i></span>
              <span class="entry-icon"><ActivityTypeIcon category="tool" size={14} /></span>
              <span class="entry-copy"><strong>{statusText(selectedChild.status)}</strong></span>
              <time datetime={new Date(selectedChild.updatedAt).toISOString()}>{timeText(selectedChild.updatedAt)}</time>
            </div>
          {/if}
          <p class="timeline-empty">{tr("Detailed activity is not available for this provider yet.", "A atividade detalhada ainda não está disponível para este provedor.")}</p>
        {:else}
          <p class="timeline-empty">{tr("No activity recorded yet.", "Nenhuma atividade registrada ainda.")}</p>
        {/if}
      </div>
      {#if messageOpen}
        <form class="subagent-composer" onsubmit={(event) => { event.preventDefault(); void sendToSubagent(); }}>
          <label for={`subagent-message-${session.id}`}>{tr(`Message ${selectedChild.label}`, `Mensagem para ${selectedChild.label}`)} <small>{tr("via main agent", "via agente principal")}</small></label>
          <div class="composer-row">
            <textarea id={`subagent-message-${session.id}`} bind:value={messageDraft} disabled={!canMessageChild || messageSending} maxlength="4000" rows="2" placeholder={tr("Write a message for this subagent…", "Escreva uma mensagem para este subagente…")}></textarea>
            <button type="submit" disabled={!canMessageChild || !messageDraft.trim() || messageSending} aria-label={tr("Ask the main agent to relay", "Pedir ao agente principal para encaminhar")}><LumeIcon name="send" size={16} /></button>
          </div>
          {#if messageNotice}<p role="status">{messageNotice}</p>{/if}
        </form>
      {/if}
    {/if}
  </div>
</section>

<style>
  .subagent-portals { position: relative; min-width: 0; flex: 0 0 auto; border-bottom: 1px solid var(--workspace-line); background: color-mix(in srgb, var(--workspace-pane) 94%, transparent); }
  .subagent-portals.compact { border: 0; background: none; }
  .portal-rail-content { display: grid; grid-template-rows: minmax(0, 1fr); overflow: hidden; opacity: 1; transform: translateY(0); transition: grid-template-rows 240ms cubic-bezier(.16, 1, .3, 1), opacity 170ms ease, transform 240ms cubic-bezier(.16, 1, .3, 1); }
  .portal-rail-content.collapsed { grid-template-rows: minmax(0, 0fr); opacity: 0; pointer-events: none; transform: translateY(-8px); }
  .portal-rail-content-inner { min-height: 0; overflow: hidden; }
  .portal-rail-toolbar { position: relative; min-height: 24px; padding: 2px 10px 0; display: flex; align-items: center; justify-content: space-between; color: var(--workspace-faint); font-size: 8px; font-weight: 700; letter-spacing: .08em; text-transform: uppercase; }
  .portal-rail-toolbar b { margin-left: 4px; color: var(--workspace-muted); font-size: 8px; font-variant-numeric: tabular-nums; }
  .portal-rail-toolbar button { position: absolute; left: 50%; width: 24px; height: 20px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 5px; color: var(--workspace-accent); background: transparent; cursor: pointer; transform: translateX(-50%); }
  .portal-rail-toolbar button:hover, .portal-rail-toolbar button:focus-visible { background: var(--workspace-subtle); }
  .portal-rail-toolbar button:focus-visible, .compact-expand:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 1px; }
  .portal-rail-toolbar svg, .compact-expand svg { width: 14px; height: 14px; stroke: currentColor; stroke-width: 1.7; stroke-linecap: round; stroke-linejoin: round; }
  .portal-compact { position: relative; z-index: 1; width: max-content; margin: 0 auto; padding: 0; display: grid; grid-template-rows: minmax(0, 0fr); align-items: center; overflow: hidden; opacity: 0; pointer-events: none; transform: translateY(8px); transition: grid-template-rows 240ms cubic-bezier(.16, 1, .3, 1), opacity 170ms ease, transform 240ms cubic-bezier(.16, 1, .3, 1); }
  .portal-compact.visible { grid-template-rows: minmax(0, 1fr); opacity: 1; pointer-events: auto; transform: translateY(0); }
  .portal-compact-inner { min-height: 0; display: flex; flex-direction: column; align-items: center; gap: 1px; overflow: hidden; padding: 6px 0 7px; }
  .compact-avatar-stack { min-height: 26px; display: flex; align-items: center; gap: 0; }
  .compact-agent { position: relative; z-index: calc(1 + var(--stack-index)); width: 26px; height: 26px; margin-left: -7px; padding: 0; display: grid; place-items: center; border: 0; color: var(--workspace-text); background: transparent; cursor: pointer; }
  .compact-agent:first-child { margin-left: 0; }
  .compact-agent-avatar { width: 24px; height: 24px; display: grid; place-items: center; overflow: hidden; border: 1px solid color-mix(in srgb, var(--portal-color) 62%, var(--workspace-line)); border-radius: 50%; background: var(--workspace-raised); transition: border-color 150ms ease, transform 150ms ease; }
  .compact-agent-avatar :global(svg), .compact-agent-avatar :global(img) { width: 100%; height: 100%; }
  .compact-agent > i { position: absolute; right: 0; bottom: 3px; width: 6px; height: 6px; border: 1px solid var(--workspace-raised); border-radius: 50%; background: var(--portal-color); }
  .compact-agent.status-running { --portal-color: #4d99cc; }
  .compact-agent.status-waiting { --portal-color: #d2a257; }
  .compact-agent.status-completed { --portal-color: #50aa79; }
  .compact-agent.status-failed { --portal-color: #ce736c; }
  .compact-agent.status-interrupted { --portal-color: var(--workspace-muted); }
  .compact-agent:hover .compact-agent-avatar, .compact-agent:focus-visible .compact-agent-avatar { border-color: var(--portal-color); transform: scale(1.08); }
  .compact-agent:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 1px; border-radius: 50%; }
  .compact-more { margin-left: 3px; color: var(--workspace-muted); font-size: 9px; font-weight: 700; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .compact-expand { width: 26px; height: 18px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 5px; color: var(--workspace-accent); background: transparent; cursor: pointer; transition: color 150ms ease, transform 150ms ease; }
  .compact-expand:hover { color: var(--workspace-strong); transform: translateY(1px); }
  .portal-rail-shell { position: relative; min-width: 0; }
  .portal-rail { min-width: 0; min-height: 81px; padding: 9px 12px 7px; display: flex; align-items: start; gap: 10px; overflow-x: auto; overflow-y: hidden; scrollbar-width: thin; scrollbar-color: var(--workspace-scroll-thumb) transparent; }
  .portal-overflow-control { position: absolute; z-index: 2; top: 0; bottom: 0; width: 40px; padding: 0; display: flex; align-items: center; border: 0; color: var(--workspace-accent); background: transparent; cursor: pointer; }
  .portal-overflow-control::before { position: absolute; inset: 0; z-index: 0; pointer-events: none; content: ""; backdrop-filter: blur(3px); -webkit-backdrop-filter: blur(3px); }
  .portal-overflow-control.left { left: 0; justify-content: flex-start; padding-left: 9px; }
  .portal-overflow-control.right { right: 0; justify-content: flex-end; padding-right: 9px; }
  .portal-overflow-control.left::before { background: linear-gradient(90deg, color-mix(in srgb, #07110f 68%, var(--workspace-pane)), color-mix(in srgb, #07110f 38%, transparent) 58%, transparent); -webkit-mask-image: linear-gradient(90deg, transparent 0%, #000 20%, #000 54%, transparent 100%); mask-image: linear-gradient(90deg, transparent 0%, #000 20%, #000 54%, transparent 100%); }
  .portal-overflow-control.right::before { background: linear-gradient(270deg, color-mix(in srgb, #07110f 68%, var(--workspace-pane)), color-mix(in srgb, #07110f 38%, transparent) 58%, transparent); -webkit-mask-image: linear-gradient(270deg, transparent 0%, #000 20%, #000 54%, transparent 100%); mask-image: linear-gradient(270deg, transparent 0%, #000 20%, #000 54%, transparent 100%); }
  .portal-overflow-control > span { position: relative; z-index: 1; width: 22px; height: 22px; display: grid; place-items: center; color: var(--workspace-accent); transition: transform 150ms ease; }
  .portal-overflow-control svg { width: 14px; height: 14px; stroke: currentColor; stroke-width: 1.7; stroke-linecap: round; stroke-linejoin: round; }
  .portal-overflow-control.left:hover > span { transform: translateX(-2px); }
  .portal-overflow-control.right:hover > span { transform: translateX(2px); }
  .portal-overflow-control:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: -3px; }
  .portal-button { position: relative; width: 60px; min-width: 60px; padding: 0; display: grid; justify-items: center; gap: 5px; border: 0; color: var(--workspace-muted); background: transparent; cursor: pointer; text-align: center; }
  .portal-button.status-running { --portal-color: #4d99cc; --portal-speed: 2.9s; }
  .portal-button.status-waiting { --portal-color: #d2a257; --portal-speed: 9s; }
  .portal-button.status-completed { --portal-color: #50aa79; --portal-speed: 16s; }
  .portal-button.status-failed { --portal-color: #ce736c; --portal-speed: 13s; }
  .portal-button.status-interrupted { --portal-color: var(--workspace-muted); --portal-speed: 16s; }
  .portal-surface { position: relative; width: 52px; height: 52px; box-sizing: border-box; display: grid; place-items: center; overflow: visible; border: 1px solid color-mix(in srgb, var(--portal-color) 64%, var(--workspace-line)); border-radius: 49% 51% 52% 48% / 50% 48% 52% 50%; background: radial-gradient(circle at 68% 24%, color-mix(in srgb, var(--portal-color) 13%, transparent), transparent 65%), var(--workspace-raised); box-shadow: inset 0 0 0 3px color-mix(in srgb, var(--portal-color) 7%, transparent), 0 2px 9px color-mix(in srgb, var(--portal-color) 12%, transparent); animation: portal-breathe var(--portal-speed) ease-in-out infinite; transition: border-color 160ms ease, border-radius 260ms cubic-bezier(.16, 1, .3, 1), transform 220ms cubic-bezier(.16, 1, .3, 1), box-shadow 220ms ease; }
  .portal-button.status-running .portal-surface { border-color: color-mix(in srgb, var(--portal-color) 32%, var(--workspace-line)); border-radius: 50%; animation: none; }
  .portal-button.status-running .portal-surface::before,
  .portal-button.status-running .portal-surface::after { position: absolute; z-index: 0; inset: -3px; pointer-events: none; border: 1.5px solid color-mix(in srgb, var(--portal-color) 82%, transparent); border-radius: 45% 55% 51% 49% / 53% 44% 56% 47%; box-shadow: 0 0 9px color-mix(in srgb, var(--portal-color) 18%, transparent); content: ""; animation: portal-wave-spin 3s linear infinite, portal-wave-morph 1.7s ease-in-out infinite alternate; }
  .portal-button.status-running .portal-surface::after { inset: -1px; border-color: color-mix(in srgb, var(--portal-color) 42%, transparent); opacity: .72; animation-duration: 4.2s, 2.1s; animation-direction: reverse, alternate-reverse; }
  .portal-button.status-completed .portal-surface { border-color: color-mix(in srgb, #50aa79 76%, var(--workspace-line)); border-radius: 50%; box-shadow: inset 0 0 0 3px color-mix(in srgb, #50aa79 9%, transparent), 0 2px 10px color-mix(in srgb, #50aa79 16%, transparent); animation: none; }
  .portal-button.status-interrupted .portal-surface { animation-play-state: paused; }
  .portal-button:hover .portal-surface, .portal-button.chosen .portal-surface { border-color: var(--portal-color); box-shadow: inset 0 0 0 3px color-mix(in srgb, var(--portal-color) 10%, transparent), 0 3px 14px color-mix(in srgb, var(--portal-color) 20%, transparent); transform: scale(1.06); }
  .portal-button:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 3px; border-radius: 8px; }
  .portal-feed { position: relative; z-index: 1; width: 38px; display: grid; gap: 4px; transform: rotate(-5deg); }
  .portal-feed-row { min-width: 0; display: flex; align-items: center; gap: 3px; color: var(--workspace-muted); }
  .portal-feed-row i { width: 4px; height: 4px; flex: 0 0 auto; border-radius: 50%; background: var(--portal-color); }
  .portal-feed-row i.live { background: #4d99cc; }
  .portal-feed-row b { min-width: 0; overflow: hidden; font-size: 5px; font-weight: 640; text-overflow: clip; white-space: nowrap; }
  .portal-feed-row.faint { opacity: .48; }
  .portal-avatar { position: absolute; top: -5px; left: -3px; width: 22px; height: 22px; display: grid; place-items: center; border: 1px solid var(--workspace-pane); border-radius: 7px; background: var(--workspace-pane); box-shadow: 0 2px 5px rgba(0, 0, 0, .14); }
  .portal-name { max-width: 60px; overflow: hidden; color: var(--workspace-muted); font-size: 8px; font-weight: 690; text-overflow: ellipsis; white-space: nowrap; }
  .portal-button.chosen .portal-name { color: var(--portal-color); }
  .portal-panel { position: absolute; z-index: 1; top: calc(100% - 4px); left: 8px; width: min(520px, calc(100% - 16px)); max-height: min(600px, 70vh); box-sizing: border-box; display: flex; flex-direction: column; overflow: hidden; border: 1px solid color-mix(in srgb, var(--workspace-accent) 25%, var(--workspace-line)); border-radius: 19px; color: var(--workspace-text); background: var(--workspace-raised); box-shadow: 0 16px 48px rgba(3, 19, 14, .23); opacity: 0; pointer-events: none; clip-path: circle(24px at var(--portal-origin) 0); transform: translateY(-8px) scale(.96); transform-origin: var(--portal-origin) 0; transition: clip-path 220ms cubic-bezier(.2, .8, .2, 1), transform 220ms cubic-bezier(.2, .8, .2, 1), opacity 160ms ease; }
  .portal-panel.has-final { width: min(560px, calc(100% - 16px)); max-height: min(650px, 76vh); }
  .portal-panel.open { opacity: 1; pointer-events: auto; z-index: 6; clip-path: circle(1000px at var(--portal-origin) 0); transform: translateY(0) scale(1); transition-duration: 320ms; }
  .portal-panel-header { min-height: 52px; padding: 10px 12px; display: flex; align-items: center; gap: 9px; flex: 0 0 auto; border-bottom: 1px solid var(--workspace-line); }
  .panel-avatar { flex: 0 0 auto; }
  .panel-heading { min-width: 0; display: grid; gap: 2px; flex: 1; }
  .panel-heading strong { overflow: hidden; color: var(--workspace-strong); font-size: 11px; text-overflow: ellipsis; white-space: nowrap; }
  .panel-heading small { color: var(--workspace-muted); font-size: 9px; }
  .portal-panel-header button { width: 26px; height: 26px; display: grid; place-items: center; border: 0; border-radius: 7px; color: var(--workspace-muted); background: transparent; cursor: pointer; }
  .portal-panel-header button:hover { color: var(--workspace-strong); background: var(--workspace-subtle); }
  .portal-panel-header button.active { color: var(--workspace-accent); background: var(--workspace-accent-soft); }
  .portal-panel-header button:disabled { cursor: not-allowed; opacity: .4; }
  .portal-timeline { min-height: 0; padding: 8px 11px 11px 14px; overflow-y: auto; scrollbar-width: thin; scrollbar-color: var(--workspace-scroll-thumb) transparent; }
  .timeline-entry { position: relative; min-width: 0; min-height: 39px; padding: 3px 0 8px 18px; display: flex; align-items: flex-start; gap: 8px; }
  .entry-rail { position: absolute; top: 0; bottom: 0; left: 3px; width: 1px; background: var(--workspace-line); }
  .timeline-entry:last-child .entry-rail { bottom: 24px; }
  .entry-rail i { position: absolute; top: 10px; left: -3px; width: 7px; height: 7px; box-sizing: border-box; border: 1px solid var(--workspace-accent); border-radius: 50%; background: var(--workspace-raised); }
  .timeline-entry.live .entry-rail i { background: #4d99cc; }
  .entry-icon { padding-top: 3px; color: var(--workspace-accent); }
  .entry-copy { min-width: 0; display: grid; gap: 3px; flex: 1; }
  .entry-copy strong { color: var(--workspace-strong); font-size: 10px; font-weight: 680; line-height: 1.35; }
  .entry-copy small { display: -webkit-box; overflow: hidden; color: var(--workspace-muted); font-size: 9px; line-height: 1.45; overflow-wrap: anywhere; white-space: pre-wrap; line-clamp: 5; -webkit-box-orient: vertical; -webkit-line-clamp: 5; }
  .entry-message { min-width: 0; color: var(--workspace-text); font-size: var(--workspace-chat-font-size, 12px); line-height: 1.6; overflow-wrap: anywhere; }
  .entry-message :global(p) { margin: 0 0 8px; }
  .entry-message :global(p:last-child) { margin-bottom: 0; }
  .entry-message :global(pre) { max-width: 100%; overflow-x: auto; padding: 8px; border-radius: 6px; background: var(--workspace-subtle); }
  .subagent-files { margin: 7px 0 12px 18px; padding-top: 11px; border-top: 1px solid var(--workspace-line); }
  .subagent-files > strong { color: var(--workspace-strong); font-size: 10px; font-weight: 700; }
  .subagent-file-list { margin-top: 7px; display: grid; }
  .subagent-file + .subagent-file { border-top: 1px solid var(--workspace-line); }
  .subagent-file summary { min-width: 0; min-height: 30px; padding: 5px 6px; display: flex; align-items: center; gap: 6px; border-radius: 6px; color: var(--workspace-text); cursor: pointer; list-style: none; font-size: 9px; }
  .subagent-file summary::-webkit-details-marker { display: none; }
  .subagent-file summary:hover { background: var(--workspace-subtle); }
  .subagent-file summary > span { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .subagent-file summary small { color: var(--workspace-muted); font-size: 8px; white-space: nowrap; }
  .subagent-file summary :global(.lume-icon) { transition: transform 160ms ease; }
  .subagent-file[open] summary :global(.lume-icon) { transform: rotate(180deg); }
  .subagent-diff { max-height: 230px; margin: 1px 0 8px 6px; overflow: auto; border-radius: 6px; background: var(--workspace-subtle); font: 9px/1.5 "SFMono-Regular", Consolas, monospace; }
  .diff-line { min-width: max-content; padding: 0 7px; white-space: pre; }
  .diff-line.kind-added { color: #3e9870; background: color-mix(in srgb, #4aa87a 10%, transparent); }
  .diff-line.kind-removed { color: #b36a68; background: color-mix(in srgb, #c07170 9%, transparent); }
  .diff-line.kind-hunk { color: var(--workspace-accent); }
  .diff-line.kind-meta { color: var(--workspace-muted); }
  .diff-unavailable { margin: 2px 7px 9px; color: var(--workspace-muted); font-size: 9px; line-height: 1.4; }
  .subagent-final { margin: 8px 0 3px 18px; padding-top: 13px; border-top: 1px solid var(--workspace-line); }
  .subagent-final header { display: flex; align-items: center; gap: 7px; color: var(--workspace-accent); }
  .subagent-final header strong { min-width: 0; flex: 1; color: var(--workspace-strong); font-size: 11px; }
  .subagent-final time { color: var(--workspace-faint); font-size: 8px; }
  .final-content { margin-top: 9px; color: var(--workspace-text); font-size: var(--workspace-chat-font-size, 12px); line-height: 1.6; overflow-wrap: anywhere; }
  .final-content :global(p) { margin: 0 0 10px; }
  .final-content :global(p:last-child) { margin-bottom: 0; }
  .final-content :global(ul), .final-content :global(ol) { margin: 5px 0 10px; padding-left: 19px; }
  .final-content :global(li) { margin: 3px 0; }
  .final-content :global(pre) { max-width: 100%; padding: 8px; overflow-x: auto; border-radius: 7px; background: var(--workspace-subtle); font-size: 10px; }
  .final-content :global(code) { overflow-wrap: anywhere; }
  .subagent-composer { padding: 9px 12px 11px; display: grid; gap: 6px; flex: 0 0 auto; border-top: 1px solid var(--workspace-line); }
  .subagent-composer label { color: var(--workspace-strong); font-size: 10px; font-weight: 700; }
  .subagent-composer label small { color: var(--workspace-muted); font-size: 9px; font-weight: 500; }
  .composer-row { min-width: 0; display: flex; align-items: flex-end; gap: 7px; }
  .composer-row textarea { min-width: 0; min-height: 47px; max-height: 130px; padding: 7px 9px; flex: 1; resize: vertical; border: 1px solid var(--workspace-line); border-radius: 8px; outline: none; color: var(--workspace-text); background: var(--workspace-pane); font: inherit; font-size: 11px; line-height: 1.45; }
  .composer-row textarea:focus-visible { border-color: var(--workspace-accent); }
  .composer-row button { width: 31px; height: 31px; display: grid; place-items: center; flex: 0 0 auto; border: 0; border-radius: 7px; color: var(--workspace-raised); background: var(--workspace-accent); cursor: pointer; }
  .composer-row button:disabled { cursor: not-allowed; opacity: .45; }
  .subagent-composer p { margin: 0; color: var(--workspace-muted); font-size: 9px; line-height: 1.4; }
  .timeline-entry time { padding-top: 2px; color: var(--workspace-faint); font-size: 8px; white-space: nowrap; }
  .timeline-empty { margin: 12px 4px 14px; color: var(--workspace-muted); font-size: 10px; line-height: 1.5; }
  @keyframes portal-breathe { 0%, 100% { border-radius: 49% 51% 52% 48% / 50% 48% 52% 50%; } 50% { border-radius: 52% 48% 48% 52% / 47% 53% 49% 51%; } }
  @keyframes portal-wave-spin { to { transform: rotate(360deg); } }
  @keyframes portal-wave-morph {
    0% { border-radius: 43% 57% 49% 51% / 55% 45% 55% 45%; }
    50% { border-radius: 56% 44% 58% 42% / 45% 57% 43% 55%; }
    100% { border-radius: 48% 52% 42% 58% / 58% 43% 57% 42%; }
  }
  @media (prefers-reduced-motion: reduce) { .portal-button .portal-surface, .portal-button.status-running .portal-surface::before, .portal-button.status-running .portal-surface::after { animation: none; }.portal-panel, .portal-button .portal-surface { transition: opacity 100ms ease; }.portal-rail-content, .portal-compact, .portal-compact-inner, .portal-overflow-control > span, .compact-agent-avatar, .compact-expand { transition: none; } }
</style>
