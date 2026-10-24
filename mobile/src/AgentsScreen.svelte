<script lang="ts">
  import { flip } from "svelte/animate";
  import LumeMascot from "$lib/LumeMascot.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import BrandIcon from "$lib/BrandIcon.svelte";
  import ThreadAvatar from "$lib/ThreadAvatar.svelte";
  import { displayText } from "$lib/i18n";
  import { stripInternalAgentMetadata } from "$lib/markdown.js";
  import type { HubSession } from "$lib/hubProtocol";
  import { sessionName, type MobileClient } from "./client.svelte";

  let { client, onOpenChat }: { client: MobileClient; onOpenChat: (id: string) => void } = $props();

  type Filter = "all" | "active" | "attention" | "done";
  let filter = $state<Filter>("all");
  let refreshing = $state(false);

  const needsAttention = (session: HubSession) => Boolean(session.pendingPermission || session.pendingQuestion) || ["permission_required", "waiting_for_input", "failed"].includes(session.status);
  const counts = $derived({
    all: client.sessions.length,
    active: client.sessions.filter((session) => session.status === "running").length,
    attention: client.sessions.filter((session) => session.status === "permission_required" || Boolean(session.pendingQuestion)).length,
    done: client.sessions.filter((session) => ["completed", "failed", "waiting_for_input"].includes(session.status)).length,
  });
  const visible = $derived(client.sessions.filter((session) => filter === "all"
    || (filter === "active" && session.status === "running")
    || (filter === "attention" && (session.status === "permission_required" || session.pendingQuestion))
    || (filter === "done" && ["completed", "failed", "waiting_for_input"].includes(session.status))));
  // The mascot shows the most pressing state across all agents, like the Orb's capsule.
  const mascot = $derived.by(() => {
    const statuses = new Set(client.sessions.map((session) => session.status));
    for (const status of ["permission_required", "running", "failed", "completed", "waiting_for_input"] as const) if (statuses.has(status)) return status;
    return "idle" as const;
  });
  const active = $derived(client.sessions.filter((session) => session.status === "running").length);

  function directory(session: HubSession) {
    if (session.source === "web") return session.agentLabel;
    const path = session.workingDirectory?.trim().replace(/[\\/]+$/, "");
    return path?.split(/[\\/]/).pop() || session.project?.trim() || session.agentLabel;
  }

  function source(session: HubSession) {
    if (session.source === "web") return ({ chrome: "Chrome", edge: "Edge", brave: "Brave" } as Record<string, string>)[session.sourceApp ?? ""] ?? "Web";
    return ({ cli: "CLI", vscode: "VS Code", desktop: "Lume" } as Record<string, string>)[session.source] ?? session.source;
  }

  function sourceIcon(session: HubSession) {
    if (session.source === "vscode") return "vscode" as const;
    if (session.source === "web") return (session.sourceApp ?? "browsers") as never;
    return "terminal" as const;
  }

  function permissionLabel(action: string) {
    return ({ allow_once: client.tr("Allow once", "Permitir uma vez"), allow_session: client.tr("Allow for session", "Permitir na sessão"), deny: client.tr("Deny", "Negar") } as Record<string, string>)[action] ?? action;
  }

  async function refresh() {
    refreshing = true;
    await client.refresh();
    refreshing = false;
  }
</script>

<section class="screen">
  <header class="top">
    <span class="mascot"><LumeMascot status={mascot} size={38} awake={client.sessions.length > 0} /></span>
    <span class="brand">
      <strong>Lume</strong>
      <small>{active === 1 ? client.tr("1 active agent", "1 agente ativo") : client.tr(`${active} active agents`, `${active} agentes ativos`)}</small>
    </span>
    <span class="connection {client.connection.state}"><i></i>{client.connection.label}</span>
    <button class:spinning={refreshing} class="icon" type="button" aria-label={client.tr("Refresh", "Atualizar")} onclick={() => void refresh()}><LumeIcon name="refresh" size={18} /></button>
  </header>

  <div class="filters" role="group" aria-label={client.tr("Filter agents", "Filtrar agentes")}>
    {#each [["all", client.tr("All", "Todos")], ["active", client.tr("Active", "Ativos")], ["attention", client.tr("Attention", "Atenção")], ["done", client.tr("Done", "Concluídos")]] as [value, label] (value)}
      <button class:on={filter === value} type="button" onclick={() => (filter = value as Filter)}>{label}{#if counts[value as Filter]}<small>{counts[value as Filter]}</small>{/if}</button>
    {/each}
  </div>

  <div class="list">
    {#each visible as session (session.id)}
      {@const reply = stripInternalAgentMetadata(session.lastResponse ?? "")}
      <article class:attention={needsAttention(session)} class="row" animate:flip={{ duration: 220 }}>
        <button class="summary" type="button" onclick={() => onOpenChat(session.id)}>
          <ThreadAvatar seed={session.nativeSessionId || session.sessionName || session.id} label={sessionName(session)} size={42} />
          <span class="copy">
            <span class="title-row">
              <strong>{sessionName(session)}</strong>
              {#if session.controlOrigin === "external"}<span class="source"><BrandIcon name={sourceIcon(session)} size={10} />{source(session)}</span>{/if}
            </span>
            <span class="project"><BrandIcon name={session.agent} size={12} /><span>{directory(session)}</span></span>
            <span class="status status-{session.status}">
              {#if session.status === "running"}<span class="dots" aria-hidden="true"><i></i><i></i><i></i></span>{:else}<i></i>{/if}
              {session.pendingQuestion ? client.tr("Question waiting", "Pergunta esperando") : displayText(client.language, session.statusLabel)}
            </span>
          </span>
          <LumeIcon name="arrow-right" size={16} />
        </button>
        {#if session.pendingPermission}
          <div class="permission">
            <strong>{displayText(client.language, session.pendingPermission.summary)}</strong>
            <code>{session.pendingPermission.resource}</code>
            {#if client.scopes.includes("approve") && session.capabilities?.canApprove}
              <div class="actions">
                {#each session.permissionProfile?.availableActions ?? ["allow_once", "deny"] as action (action)}
                  <button class:deny={action === "deny"} type="button" onclick={() => void client.resolvePermission(session, action)}>{permissionLabel(action)}</button>
                {/each}
              </div>
            {/if}
          </div>
        {:else if reply && session.status !== "running"}
          <p class="reply">{reply}</p>
        {/if}
      </article>
    {:else}
      <div class="empty">
        <LumeMascot status="idle" size={56} />
        {#if client.sessions.length}
          <strong>{client.tr("Nothing in this view", "Nada nesta visualização")}</strong>
          <p>{client.tr("Try another filter.", "Tente outro filtro.")}</p>
        {:else}
          <strong>{client.tr("No agents are open", "Nenhum agente aberto")}</strong>
          <p>{client.tr("Start an agent on your computer. Lume will bring it here automatically.", "Abra um agente no computador. O Lume traz ele para cá automaticamente.")}</p>
        {/if}
      </div>
    {/each}
  </div>
</section>

<style>
  .screen { min-height: 0; display: grid; grid-template-columns: minmax(0, 1fr); grid-template-rows: auto auto minmax(0, 1fr); }
  .top { padding: calc(var(--safe-top) + 12px) 16px 10px; display: flex; align-items: center; gap: 10px; }
  .brand { min-width: 0; flex: 1; display: grid; }
  .brand strong { color: var(--workspace-strong); font-size: 20px; font-weight: 760; letter-spacing: -.02em; }
  .brand small { color: var(--workspace-muted); font-size: 12.5px; }
  .connection { height: 28px; padding: 0 10px; display: inline-flex; align-items: center; gap: 6px; border: 1px solid var(--workspace-line); border-radius: 999px; color: var(--workspace-muted); font-size: 12px; font-weight: 650; }
  .connection i { width: 7px; height: 7px; border-radius: 50%; background: var(--workspace-faint); }
  .connection.online i { background: #4fb37f; box-shadow: 0 0 0 3px rgba(79, 179, 127, .18); }
  .connection.offline i { background: #d28a4b; }
  .icon { width: 40px; height: 40px; display: grid; place-items: center; border: 0; border-radius: 12px; color: var(--workspace-muted); background: var(--workspace-subtle); }
  .icon.spinning :global(svg) { animation: spin 700ms linear infinite; }
  .filters { padding: 0 16px 10px; display: flex; gap: 6px; overflow-x: auto; scrollbar-width: none; }
  .filters button { height: 34px; padding: 0 13px; display: inline-flex; align-items: center; gap: 6px; flex: 0 0 auto; border: 1px solid var(--workspace-line); border-radius: 999px; color: var(--workspace-muted); background: transparent; font-size: 13px; font-weight: 650; }
  .filters button.on { border-color: color-mix(in srgb, var(--workspace-accent) 45%, var(--workspace-line)); color: var(--workspace-accent); background: var(--workspace-accent-soft); }
  .filters small { color: var(--workspace-faint); font-size: 11.5px; }
  .list { min-height: 0; padding: 2px 12px 18px; overflow-y: auto; overscroll-behavior: contain; }
  .row { margin-bottom: 6px; border: 1px solid transparent; border-radius: 18px; background: var(--workspace-raised); box-shadow: 0 1px 0 color-mix(in srgb, var(--workspace-line) 60%, transparent); }
  .row.attention { border-color: color-mix(in srgb, #d0903f 32%, transparent); background: linear-gradient(90deg, color-mix(in srgb, #d0903f 10%, var(--workspace-raised)), var(--workspace-raised) 78%); }
  .summary { width: 100%; padding: 12px 12px 12px 12px; display: flex; align-items: center; gap: 12px; border: 0; color: inherit; background: transparent; text-align: left; }
  .summary > :global(svg:last-child) { flex: 0 0 auto; color: var(--workspace-faint); }
  .copy { min-width: 0; flex: 1; display: grid; gap: 3px; }
  .title-row { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 7px; }
  .title-row strong { min-width: 0; color: var(--workspace-strong); font-size: 15.5px; font-weight: 720; }
  .source { display: inline-flex; align-items: center; gap: 4px; padding: 2px 7px; border-radius: 999px; color: var(--workspace-muted); background: var(--workspace-subtle); font-size: 10px; font-weight: 750; letter-spacing: .04em; text-transform: uppercase; }
  .project { min-width: 0; display: flex; align-items: center; gap: 5px; color: var(--workspace-muted); font-size: 13px; }
  .project span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .status { display: flex; align-items: center; gap: 6px; color: var(--workspace-faint); font-size: 12.5px; }
  .status > i { width: 6px; height: 6px; border-radius: 50%; background: currentColor; }
  .status-running { color: #6ea6d6; }
  .status-permission_required, .status-waiting_for_input { color: #d39a4c; }
  .status-completed { color: #57b380; }
  .status-failed { color: #d06e66; }
  .dots { display: inline-flex; gap: 2px; }
  .dots i { width: 4px; height: 4px; border-radius: 50%; background: currentColor; animation: bounce 900ms ease-in-out infinite; }
  .dots i:nth-child(2) { animation-delay: 120ms; }
  .dots i:nth-child(3) { animation-delay: 240ms; }
  .permission { margin: 0 12px 12px; padding: 12px; display: grid; gap: 8px; border: 1px solid color-mix(in srgb, #d0903f 35%, var(--workspace-line)); border-radius: 14px; background: color-mix(in srgb, #d0903f 7%, var(--workspace-bg)); }
  .permission strong { color: var(--workspace-strong); font-size: 14px; }
  .permission code { padding: 7px 9px; overflow-x: auto; border-radius: 9px; color: var(--workspace-strong); background: var(--workspace-code, rgba(0, 0, 0, .2)); font: 12.5px/1.45 var(--lume-font-code, monospace); white-space: pre-wrap; overflow-wrap: anywhere; }
  .actions { display: flex; flex-wrap: wrap; gap: 6px; }
  .actions button { height: 38px; padding: 0 14px; flex: 1; border: 0; border-radius: 11px; color: #fff; background: var(--workspace-accent); font-size: 13.5px; font-weight: 700; }
  .actions button.deny { color: var(--workspace-strong); background: var(--workspace-subtle); }
  .reply { margin: -4px 14px 12px 66px; display: -webkit-box; overflow: hidden; color: var(--workspace-muted); font-size: 13px; line-height: 1.45; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; }
  .empty { margin-top: 40px; display: grid; justify-items: center; gap: 8px; color: var(--workspace-muted); text-align: center; }
  .empty strong { color: var(--workspace-strong); font-size: 16px; }
  .empty p { margin: 0; max-width: 280px; font-size: 14px; line-height: 1.5; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @keyframes bounce { 0%, 100% { transform: translateY(0); opacity: .55; } 50% { transform: translateY(-2px); opacity: 1; } }
  @media (prefers-reduced-motion: reduce) { .dots i, .icon.spinning :global(svg) { animation: none; } }
</style>
