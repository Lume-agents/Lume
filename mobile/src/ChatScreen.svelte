<script lang="ts">
  import { fade, fly, slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import type { SessionActivity } from "$lib/domain";
  import type { HubSession } from "$lib/hubProtocol";
  import { buildConversationEntries, buildConversationFeed, fileChangesForFinalResponses } from "$lib/sessionConversation";
  import { displayFileChangePath, normalizeFileChangePath, summarizeFileChanges } from "$lib/fileChanges";
  import { activityDisplayTitle, activityThinkingLabel, activityThinkingState, formatAgentDuration, isGenericAnalysisPlaceholder } from "$lib/activityPresentation";
  import { interruptNoticeText } from "$lib/interruptNotice";
  import { renderWorkspaceMarkdownWithFileBadges } from "$lib/workspaceFileReferences.js";
  import { renderFileTypeIconHtml } from "$lib/fileTypeIcons";
  import { BoundedRenderCache } from "$lib/boundedRenderCache";
  import { displayText } from "$lib/i18n";
  import { usageGaugeGroups } from "$lib/usageGauges";
  import ActivityTraceGroup from "$lib/ActivityTraceGroup.svelte";
  import CollapsibleUserMessage from "$lib/CollapsibleUserMessage.svelte";
  import StreamedMessage from "$lib/StreamedMessage.svelte";
  import ThinkingOrb from "$lib/ThinkingOrb.svelte";
  import ThreadAvatar from "$lib/ThreadAvatar.svelte";
  import BrandIcon from "$lib/BrandIcon.svelte";
  import FileTypeIcon from "$lib/FileTypeIcon.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import ImageLightbox from "$lib/ImageLightbox.svelte";
  import WorkspaceWorkBookmarks from "$lib/WorkspaceWorkBookmarks.svelte";
  import { sessionName, type MobileClient } from "./client.svelte";
  import Composer from "./Composer.svelte";

  let { client, session, onBack }: { client: MobileClient; session: HubSession | null; onBack: () => void } = $props();

  const markdownCache = new BoundedRenderCache(64, 5 * 1024 * 1024);
  let feedElement = $state<HTMLElement | null>(null);
  let followingTail = $state(true);
  let expandedFiles = $state<string[]>([]);
  let lightbox = $state<{ src: string; alt: string } | null>(null);
  let clock = $state(Date.now());
  let subagent = $state<{ activity: SessionActivity; events: SessionActivity[] | null; error: string } | null>(null);
  const openedAt = Date.now();

  function reportedFiles(activity: SessionActivity): string[] {
    const files = [...activity.files];
    const title = activity.title.trim();
    const looksLikePath = activity.kind === "file"
      && !/^(?:arquivos alterados|alterações da tarefa|\d+\s+arquivos alterados)$/i.test(title)
      && (title.includes("/") || title.includes("\\") || /\.[a-z0-9]{1,8}$/i.test(title));
    if (looksLikePath && !files.includes(title)) files.push(title);
    return files;
  }

  // The same steps as the Workspace pane (WorkspaceSessionPane.svelte), so the two chats read alike.
  const chatActivities = $derived((session?.activities ?? []).filter((activity: SessionActivity) =>
    activity.kind !== "warning"
    && !/^functions\s*[·:]\s*(?:create_goal|get_goal|update_goal)$/i.test(activity.title.trim())
    && !isGenericAnalysisPlaceholder(activity)));
  const entries = $derived(session
    ? buildConversationEntries(session, chatActivities, (activity) => summarizeFileChanges(activity.detail ?? "", reportedFiles(activity), session.workingDirectory))
    : []);
  const feed = $derived(buildConversationFeed(entries, { includeAnalysisInTrace: true, workingDirectory: session?.workingDirectory }));
  const filesByResponse = $derived(fileChangesForFinalResponses(feed, session?.workingDirectory));
  const running = $derived(session?.status === "running");
  const activeTraceId = $derived.by(() => {
    if (!session || !["running", "permission_required"].includes(session.status)) return null;
    const traceIndex = feed.findLastIndex((item) => item.kind === "trace");
    const promptIndex = feed.findLastIndex((item) => item.kind === "entry" && item.entry.activity.kind === "prompt");
    return traceIndex >= promptIndex && traceIndex >= 0 ? feed[traceIndex].id : null;
  });
  const thinking = $derived.by(() => {
    const activity = chatActivities.findLast((item: SessionActivity) => item.kind !== "subagent");
    return activity ? activityThinkingState(activity) : "breathing";
  });
  const promptStartedAt = $derived.by(() => {
    const latestFinal = entries.reduce((latest, entry) => entry.isFinalResponse ? Math.max(latest, entry.activity.createdAt) : latest, 0);
    return chatActivities.findLast((activity: SessionActivity) => activity.kind === "prompt" && activity.createdAt > latestFinal)?.createdAt ?? null;
  });
  const newestMessageId = $derived(feed.findLast((item) => item.kind === "entry" && item.entry.activity.kind === "message")?.id ?? null);
  const usage = $derived(usageGaugeGroups(session?.rateLimits ?? [], client.language).flatMap((group) => group.gauges));
  const tightest = $derived(usage.length ? usage.reduce((low, gauge) => gauge.remaining < low.remaining ? gauge : low) : null);
  const canStop = $derived(Boolean(session && client.scopes.includes("terminate") && (session.capabilities?.canInterrupt || session.capabilities?.canTerminate) && ["running", "permission_required"].includes(session.status)));

  // Subagents Codex and Oh My Pi start keep their own thread; each opens its timeline in a sheet.
  const subagents = $derived(chatActivities.filter((activity: SessionActivity) => activity.kind === "subagent"));

  async function openSubagent(activity: SessionActivity) {
    if (!session) return;
    subagent = { activity, events: null, error: "" };
    try {
      const events = await client.subagentTimeline(session.id, activity.id);
      if (subagent?.activity.id === activity.id) subagent = { activity, events, error: "" };
    } catch (error) {
      if (subagent?.activity.id === activity.id) subagent = { activity, events: [], error: (error as Error).message };
    }
  }

  function render(key: string, value: string) {
    return markdownCache.render(key, value, (source) => renderWorkspaceMarkdownWithFileBadges(source, renderFileTypeIconHtml, session?.workingDirectory));
  }

  function time(value: number) {
    return new Intl.DateTimeFormat(client.language, { hour: "2-digit", minute: "2-digit" }).format(new Date(value));
  }

  function trackScroll() {
    if (!feedElement) return;
    // The feed is laid out bottom-up: 0 is the newest message.
    followingTail = Math.abs(feedElement.scrollTop) < 80;
  }

  function scrollToLatest() {
    feedElement?.scrollTo({ top: 0, behavior: "smooth" });
  }

  function toggleFiles(id: string) {
    expandedFiles = expandedFiles.includes(id) ? expandedFiles.filter((value) => value !== id) : [...expandedFiles, id];
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      client.showBanner(client.tr("Response copied.", "Resposta copiada."), "success");
    } catch {
      client.showBanner(client.tr("Could not copy the response.", "Não foi possível copiar a resposta."), "error");
    }
  }

  $effect(() => {
    if (!session) return;
    void client.loadResponseImages(session);
  });
  $effect(() => {
    if (session) void client.refreshRateLimits(session);
  });
  $effect(() => {
    if (!running) return;
    clock = Date.now();
    const timer = setInterval(() => (clock = Date.now()), 1_000);
    return () => clearInterval(timer);
  });
</script>

<section class="chat">
  <header class="chat-header">
    <button class="back" type="button" aria-label={client.tr("Back to agents", "Voltar aos agentes")} onclick={onBack}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m14.5 6-6 6 6 6" /></svg></button>
    {#if session}
      <ThreadAvatar seed={session.nativeSessionId || session.sessionName || session.id} label={sessionName(session)} size={36} />
      <span class="identity">
        <strong>{sessionName(session)}</strong>
        <small><BrandIcon name={session.agent} size={11} />{session.agentLabel} · <span class="state state-{session.status}">{session.pendingQuestion ? client.tr("Question waiting", "Pergunta esperando") : displayText(client.language, session.statusLabel)}</span></small>
      </span>
      {#if tightest}
        <span class="usage" class:low={tightest.remaining <= 20} class:mid={tightest.remaining > 20 && tightest.remaining <= 50} style:--remaining={`${tightest.remaining * 3.6}deg`} title={tightest.title} role="img" aria-label={tightest.title}><b>{tightest.remaining}</b></span>
      {/if}
      {#if canStop}
        <button class="stop" type="button" aria-label={session.capabilities?.canInterrupt ? client.tr("Interrupt", "Interromper") : client.tr("Stop agent", "Parar agente")} onclick={() => void client.stop(session)}><LumeIcon name="stop" size={16} /></button>
      {/if}
    {:else}
      <span class="identity"><strong>{client.tr("Agent closed", "Agente fechado")}</strong><small>{client.tr("This session is no longer open", "Esta sessão não está mais aberta")}</small></span>
    {/if}
  </header>

  {#if session}
    <div class="bookmarks"><WorkspaceWorkBookmarks {session} language={client.language} hasSubagents={subagents.length > 0} /></div>
    {#if subagents.length}
      <div class="subagents" aria-label={client.tr("Subagents", "Subagentes")}>
        {#each subagents as item (item.id)}
          <button class="subagent status-{item.status}" type="button" onclick={() => void openSubagent(item)}>
            <i></i>{activityDisplayTitle(item, client.language)}
          </button>
        {/each}
      </div>
    {/if}
    <div class="feed-shell">
      <div class="feed" bind:this={feedElement} onscroll={trackScroll} role="region" aria-label={client.tr("Conversation", "Conversa")}>
        <div class="feed-content">
          {#each feed as item (item.id)}
            {#if item.kind === "trace"}
              <div class="trace" in:slide={{ duration: 170, easing: cubicOut }}>
                <ActivityTraceGroup activities={item.entries.map((entry) => entry.activity)} active={item.id === activeTraceId} plain language={client.language} workingDirectory={session.workingDirectory} />
              </div>
            {:else}
              {@const entry = item.entry}
              {@const activity = entry.activity}
              {@const changed = filesByResponse.get(entry.id) ?? []}
              {#if activity.kind === "prompt" && (activity.detail || activity.attachments?.length)}
                <div class="row user">
                  <section class="message user-message">
                    {#if activity.detail}<CollapsibleUserMessage text={activity.detail} language={client.language} />{/if}
                    {#if activity.attachments?.length}
                      <div class="attachments">
                        {#each activity.attachments as attachment, index (index)}
                          {#if attachment.previewDataUrl}
                            <button class="thumb" type="button" onclick={() => (lightbox = { src: attachment.previewDataUrl!, alt: attachment.name })}><img src={attachment.previewDataUrl} alt={attachment.name} /></button>
                          {:else}
                            <span class="file-chip"><FileTypeIcon path={attachment.name} />{attachment.name}</span>
                          {/if}
                        {/each}
                      </div>
                    {/if}
                  </section>
                  <time>{time(activity.createdAt)}</time>
                </div>
              {:else if activity.kind === "message" && activity.detail}
                <div class="row agent">
                  <section class="message agent-message">
                    <div class="markdown-content">
                      <StreamedMessage text={activity.detail} animate={item.id === newestMessageId && (activity.createdAt > openedAt || activity.status === "running")} live={activity.status === "running"} startFromBeginning={activity.createdAt > openedAt} render={(value) => render(entry.id, value)} />
                    </div>
                    {#if activity.attachments?.some((attachment) => attachment.id)}
                      <div class="attachments">
                        {#each activity.attachments.filter((attachment) => attachment.id) as attachment (attachment.id)}
                          {@const image = client.responseImages[attachment.id!]}
                          {#if String(attachment.mimeType || "").startsWith("image/") && image}
                            <button class="thumb wide" type="button" onclick={() => (lightbox = { src: image, alt: attachment.name })}><img src={image} alt={attachment.name} /></button>
                          {:else}
                            <button class="file-chip" type="button" onclick={() => void client.downloadResponseFile(session.id, attachment.id!)}><FileTypeIcon path={attachment.name} />{attachment.name}<LumeIcon name="download" size={13} /></button>
                          {/if}
                        {/each}
                      </div>
                    {/if}
                    {#if entry.isFinalResponse}
                      <footer class="response-footer">
                        <span>{entry.durationMs !== undefined ? `${client.tr("Worked for", "Trabalhou por")} ${formatAgentDuration(entry.durationMs)}` : time(activity.createdAt)}</span>
                        <button type="button" aria-label={client.tr("Copy response", "Copiar resposta")} onclick={() => void copy(activity.detail ?? "")}><LumeIcon name="copy" size={15} /></button>
                      </footer>
                    {/if}
                  </section>
                  {#if entry.isFinalResponse && changed.length}
                    {@const added = changed.reduce((total, file) => total + file.added, 0)}
                    {@const removed = changed.reduce((total, file) => total + file.removed, 0)}
                    <div class:open={expandedFiles.includes(entry.id)} class="changed-files">
                      <button class="changed-toggle" type="button" aria-expanded={expandedFiles.includes(entry.id)} onclick={() => toggleFiles(entry.id)}>
                        <LumeIcon name="file" size={15} />
                        <span>{client.tr(`${changed.length} file${changed.length === 1 ? "" : "s"} changed`, `${changed.length} arquivo${changed.length === 1 ? "" : "s"} alterado${changed.length === 1 ? "" : "s"}`)}</span>
                        {#if added || removed}<b class="added">+{added}</b><b class="removed">−{removed}</b>{/if}
                        <LumeIcon name="chevron-down" size={14} />
                      </button>
                      {#if expandedFiles.includes(entry.id)}
                        <div class="changed-list" transition:slide={{ duration: 160, easing: cubicOut }}>
                          {#each changed as file (file.path)}
                            <span class="changed-file" title={file.path}>
                              <FileTypeIcon path={file.path} />
                              <span>{displayFileChangePath(normalizeFileChangePath(file.path, session.workingDirectory))}</span>
                              {#if file.added || file.removed}<b class="added">+{file.added}</b><b class="removed">−{file.removed}</b>{/if}
                            </span>
                          {/each}
                        </div>
                      {/if}
                    </div>
                  {/if}
                </div>
              {:else if activity.kind === "interrupt"}
                <p class="interrupt" role="status"><LumeIcon name="stop" size={12} />{interruptNoticeText(activity.detail, (en: string, pt: string) => client.tr(en, pt))}</p>
              {:else if activity.kind === "analysis" && activity.detail}
                <section class="analysis">
                  <header>{displayText(client.language, activity.title)}</header>
                  <div class="markdown-content">{@html render(entry.id, activity.detail)}</div>
                </section>
              {/if}
            {/if}
          {:else}
            <div class="empty">
              <BrandIcon name={session.agent} size={30} />
              <strong>{client.tr("No conversation yet", "Nenhuma conversa ainda")}</strong>
              <span>{client.tr("Messages and activity will appear here in real time.", "Mensagens e atividades aparecem aqui em tempo real.")}</span>
            </div>
          {/each}
          {#if running}
            <div class="typing">
              <ThinkingOrb state={thinking} size={36} speed={0.92} label={client.tr(`${sessionName(session)} is working`, `${sessionName(session)} está trabalhando`)} />
              <span class="typing-label">{activityThinkingLabel(thinking, client.language)}</span>
              {#if promptStartedAt}<span class="elapsed">{formatAgentDuration(Math.max(0, clock - promptStartedAt))}</span>{/if}
            </div>
          {/if}
        </div>
      </div>
      {#if !followingTail}
        <button class="latest" type="button" aria-label={client.tr("Scroll to latest", "Ir para o final")} onclick={scrollToLatest} transition:fly={{ y: 10, duration: 180, easing: cubicOut }}><LumeIcon name="arrow-down" size={18} /></button>
      {/if}
    </div>
    <Composer {client} {session} />
  {:else}
    <div class="closed" in:fade>
      <strong>{client.tr("Session closed", "Sessão encerrada")}</strong>
      <p>{client.tr("Return to the agent list to choose another session.", "Volte à lista de agentes para escolher outra sessão.")}</p>
    </div>
  {/if}
</section>

{#if subagent && session}
  <div class="sheet-backdrop" transition:fade={{ duration: 150 }} onclick={(event) => { if (event.target === event.currentTarget) subagent = null; }} role="presentation">
    <div class="sheet" role="dialog" aria-modal="true" aria-label={activityDisplayTitle(subagent.activity, client.language)} transition:fly={{ y: 40, duration: 220, easing: cubicOut }}>
      <header>
        <strong>{activityDisplayTitle(subagent.activity, client.language)}</strong>
        <button type="button" aria-label={client.tr("Close", "Fechar")} onclick={() => (subagent = null)}><LumeIcon name="close" size={16} /></button>
      </header>
      <div class="sheet-body">
        {#if subagent.events === null}
          <p class="sheet-note">{client.tr("Loading what it did…", "Carregando o que ele fez…")}</p>
        {:else if subagent.error}
          <p class="sheet-note">{subagent.error}</p>
        {:else if !subagent.events.length}
          <p class="sheet-note">{client.tr("Nothing recorded yet.", "Nada registrado ainda.")}</p>
        {:else}
          <ActivityTraceGroup activities={subagent.events} active={subagent.activity.status === "running"} plain language={client.language} workingDirectory={session.workingDirectory} />
          {#each subagent.events.filter((event) => event.kind === "message" && event.detail).slice(-1) as reply (reply.id)}
            <div class="markdown-content subagent-reply">{@html render(`subagent:${reply.id}`, reply.detail ?? "")}</div>
          {/each}
        {/if}
      </div>
    </div>
  </div>
{/if}

{#if lightbox}
  <ImageLightbox src={lightbox.src} alt={lightbox.alt} language={client.language} onclose={() => (lightbox = null)} />
{/if}

<style>
  .chat { height: 100%; display: flex; flex-direction: column; }
  .chat > :global(*) { flex: 0 0 auto; min-width: 0; }
  .chat-header { padding: calc(var(--safe-top) + 8px) 12px 10px 6px; display: flex; align-items: center; gap: 10px; border-bottom: 1px solid var(--workspace-line); background: color-mix(in srgb, var(--workspace-sidebar) 90%, transparent); backdrop-filter: blur(14px); }
  .back { width: 40px; height: 40px; display: grid; place-items: center; flex: 0 0 auto; border: 0; border-radius: 12px; color: var(--workspace-strong); background: transparent; }
  .back svg { width: 24px; height: 24px; fill: none; stroke: currentColor; stroke-width: 2.2; stroke-linecap: round; stroke-linejoin: round; }
  .identity { min-width: 0; flex: 1; display: grid; gap: 2px; }
  .identity strong { overflow: hidden; color: var(--workspace-strong); font-size: 16px; font-weight: 740; text-overflow: ellipsis; white-space: nowrap; }
  .identity small { min-width: 0; display: flex; align-items: center; gap: 5px; overflow: hidden; color: var(--workspace-muted); font-size: 12.5px; white-space: nowrap; }
  .state-running { color: #6ea6d6; }
  .state-permission_required, .state-waiting_for_input { color: #d39a4c; }
  .state-completed { color: #57b380; }
  .state-failed { color: #d06e66; }
  .usage { --tone: #57b380; width: 30px; height: 30px; display: grid; place-items: center; flex: 0 0 auto; border-radius: 50%; background: conic-gradient(var(--tone) var(--remaining), color-mix(in srgb, var(--workspace-muted) 22%, transparent) 0); }
  .usage::before { position: absolute; content: ""; }
  .usage b { width: 24px; height: 24px; display: grid; place-items: center; border-radius: 50%; color: var(--workspace-strong); background: var(--workspace-sidebar); font-size: 9.5px; font-weight: 800; }
  .usage.mid { --tone: #d6a24d; }
  .usage.low { --tone: #d0645c; }
  .stop { width: 38px; height: 38px; display: grid; place-items: center; flex: 0 0 auto; border: 1px solid color-mix(in srgb, #d0645c 40%, var(--workspace-line)); border-radius: 12px; color: #d76e66; background: color-mix(in srgb, #d0645c 8%, transparent); }
  .bookmarks:empty { display: none; }
  .subagents { padding: 8px 12px 0; display: flex; gap: 6px; overflow-x: auto; scrollbar-width: none; }
  .subagent { height: 32px; padding: 0 11px; display: inline-flex; align-items: center; gap: 6px; flex: 0 0 auto; border: 1px solid var(--workspace-line); border-radius: 999px; color: var(--workspace-strong); background: var(--workspace-raised); font-size: 12.5px; font-weight: 650; }
  .subagent i { width: 7px; height: 7px; border-radius: 50%; background: var(--workspace-faint); }
  .subagent.status-running i { background: #6ea6d6; box-shadow: 0 0 0 3px rgba(110, 166, 214, .2); }
  .subagent.status-completed i { background: #57b380; }
  .subagent.status-failed i { background: #d06e66; }
  .sheet-backdrop { position: fixed; z-index: 30; inset: 0; display: grid; align-items: end; background: rgba(4, 9, 6, .55); }
  .sheet { max-height: 82vh; display: grid; grid-template-rows: auto minmax(0, 1fr); border: 1px solid var(--workspace-line); border-bottom: 0; border-radius: 22px 22px 0 0; background: var(--workspace-bg); }
  .sheet header { padding: 14px 12px 10px 18px; display: flex; align-items: center; gap: 10px; border-bottom: 1px solid var(--workspace-line); }
  .sheet header strong { flex: 1; color: var(--workspace-strong); font-size: 16px; }
  .sheet header button { width: 36px; height: 36px; display: grid; place-items: center; border: 0; border-radius: 11px; color: var(--workspace-muted); background: transparent; }
  .sheet-body { min-height: 0; padding: 12px 14px calc(var(--safe-bottom) + 18px); display: grid; align-content: start; gap: 12px; overflow-y: auto; }
  .sheet-note { margin: 0; color: var(--workspace-muted); font-size: 14px; }
  .subagent-reply { padding-top: 6px; border-top: 1px solid var(--workspace-line); }
  .bookmarks { padding: 0 12px; }
  .feed-shell { position: relative; min-height: 0; flex: 1 1 auto !important; }
  .feed { height: 100%; padding: 14px 14px 18px; display: flex; flex-direction: column-reverse; overflow-x: hidden; overflow-y: auto; overscroll-behavior: contain; }
  .feed-content { width: 100%; min-width: 0; display: flex; flex-direction: column; gap: 14px; }
  .row { min-width: 0; display: grid; grid-template-columns: minmax(0, 1fr); gap: 6px; }
  .row.user { justify-items: end; }
  .row.user time { color: var(--workspace-faint); font-size: 11px; }
  .message { min-width: 0; max-width: 100%; }
  .user-message { max-width: 88%; padding: 10px 13px; border: 1px solid var(--workspace-user-line); border-radius: 18px 18px 6px 18px; color: var(--workspace-strong); background: var(--workspace-user); font-size: var(--chat-font-size); line-height: 1.5; }
  .agent-message { color: var(--workspace-text); }
  .attachments { margin-top: 8px; display: flex; flex-wrap: wrap; gap: 6px; }
  .thumb { width: 92px; height: 68px; padding: 0; overflow: hidden; border: 1px solid var(--workspace-line); border-radius: 10px; background: var(--workspace-subtle); }
  .thumb.wide { width: min(100%, 260px); height: auto; max-height: 220px; }
  .thumb img { width: 100%; height: 100%; display: block; object-fit: cover; }
  .file-chip { min-height: 32px; padding: 4px 10px; display: inline-flex; align-items: center; gap: 6px; border: 1px solid var(--workspace-line); border-radius: 10px; color: var(--workspace-strong); background: var(--workspace-raised); font-size: 12.5px; }
  .response-footer { margin-top: 8px; display: flex; align-items: center; gap: 8px; color: var(--workspace-faint); font-size: 12px; }
  .response-footer span { flex: 1; }
  .response-footer button { width: 34px; height: 34px; display: grid; place-items: center; border: 0; border-radius: 10px; color: var(--workspace-muted); background: transparent; }
  .changed-files { border: 1px solid var(--workspace-line); border-radius: 14px; background: var(--workspace-raised); }
  .changed-toggle { width: 100%; min-height: 44px; padding: 0 12px; display: flex; align-items: center; gap: 8px; border: 0; color: var(--workspace-strong); background: transparent; font-size: 13.5px; font-weight: 650; }
  .changed-toggle span { flex: 1; text-align: left; }
  .changed-files.open .changed-toggle > :global(svg:last-child) { transform: rotate(180deg); }
  .changed-list { padding: 0 8px 8px; display: grid; gap: 2px; }
  .changed-file { min-height: 34px; padding: 0 6px; display: flex; align-items: center; gap: 8px; border-radius: 9px; color: var(--workspace-text); font: 12.5px var(--lume-font-code, monospace); }
  .changed-file span { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .added, .removed { font: 700 12px var(--lume-font-code, monospace); font-variant-numeric: tabular-nums; }
  .added { color: #4fa877; }
  .removed { color: #cf6b66; }
  .interrupt { margin: 0; display: flex; align-items: center; gap: 6px; color: var(--workspace-faint); font-size: 12.5px; }
  .analysis { padding: 10px 12px; border-left: 2px solid color-mix(in srgb, var(--workspace-accent) 50%, transparent); color: var(--workspace-muted); }
  .analysis header { margin-bottom: 4px; color: var(--workspace-faint); font-size: 12px; font-weight: 700; }
  .trace { min-width: 0; max-width: 100%; }
  .trace :global(.activity-cluster) { max-width: 100%; }
  .typing { display: flex; align-items: center; gap: 10px; color: var(--workspace-muted); font-size: 13px; }
  .typing-label { flex: 1; }
  .elapsed { color: var(--workspace-faint); font-variant-numeric: tabular-nums; }
  .latest { position: absolute; right: 16px; bottom: 14px; width: 42px; height: 42px; display: grid; place-items: center; border: 1px solid var(--workspace-line); border-radius: 50%; color: var(--workspace-strong); background: var(--workspace-raised); box-shadow: 0 8px 22px rgba(0, 0, 0, .25); }
  .empty, .closed { margin: auto; padding: 40px 20px; display: grid; justify-items: center; gap: 8px; color: var(--workspace-muted); text-align: center; font-size: 14px; }
  .empty strong, .closed strong { color: var(--workspace-strong); font-size: 16px; }
  .closed p { margin: 0; }
</style>
