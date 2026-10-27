<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
  import ThreadAvatar from "$lib/ThreadAvatar.svelte";
  import LumeMascot from "$lib/LumeMascot.svelte";
  import { appearanceAttributes } from "$lib/appearance";
  import type { AgentSession, Preferences } from "$lib/domain";
  import { loadPreferences, loadSessions } from "$lib/lume";

  // The panel behind the tray icon (see tray_panel.rs): what the agents are doing, without the rest of Lume.
  let sessions = $state<AgentSession[]>([]);
  let preferences = $state<Preferences | null>(null);
  let systemDark = $state(typeof matchMedia === "function" ? matchMedia("(prefers-color-scheme: dark)").matches : true);

  const pt = $derived(preferences?.language === "pt-BR");
  const tr = (english: string, portuguese: string) => (pt ? portuguese : english);
  const dark = $derived(preferences?.darkMode ?? systemDark);
  const look = $derived(preferences ? appearanceAttributes(preferences) : null);

  // "Need you" is a permission or a question; an agent that merely finished waits calmly as "ready".
  const needing = $derived(sessions.filter((session) => session.status === "permission_required" || Boolean(session.pendingQuestion)));
  const ready = $derived(sessions.filter((session) => session.status === "waiting_for_input" && !session.pendingQuestion));
  const running = $derived(sessions.filter((session) => session.status === "running"));
  const recent = $derived(sessions
    .filter((session) => session.status === "completed" || session.status === "failed")
    .sort((left, right) => right.updatedAt - left.updatedAt)
    .slice(0, 4));
  const summary = $derived([
    needing.length ? tr(`${needing.length} need you`, `${needing.length} precisam de você`) : "",
    running.length ? tr(`${running.length} running`, `${running.length} rodando`) : "",
    ready.length ? tr(`${ready.length} ready`, `${ready.length} prontos`) : "",
  ].filter(Boolean).join(" · ") || tr("Nothing running", "Nada rodando"));

  const nameOf = (session: AgentSession) => session.sessionName?.trim() || session.project || session.agentLabel;
  const askOf = (session: AgentSession) => session.pendingPermission?.summary
    ?? session.pendingQuestion?.questions[0]?.question
    ?? "";

  async function refresh() {
    sessions = await loadSessions().catch(() => sessions);
  }
  async function reload() {
    preferences = await loadPreferences().catch(() => preferences);
    await refresh();
  }
  const hide = () => void invoke("hide_tray_panel").catch(() => undefined);
  const go = (target: "orb" | "workspace") => void invoke("switch_from_tray", { target }).catch(() => undefined);

  function keydown(event: KeyboardEvent) {
    if (event.key === "Escape") hide();
  }

  onMount(() => {
    void reload();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const stops = [
      listen("lume://sessions-changed", () => { clearTimeout(timer); timer = setTimeout(() => void refresh(), 350); }),
      listen("lume://tray-panel-opened", () => void reload()),
    ];
    const scheme = matchMedia("(prefers-color-scheme: dark)");
    const followScheme = (event: MediaQueryListEvent) => (systemDark = event.matches);
    scheme.addEventListener("change", followScheme);
    return () => {
      clearTimeout(timer);
      scheme.removeEventListener("change", followScheme);
      for (const stop of stops) void stop.then((unlisten) => unlisten());
    };
  });
</script>

<svelte:head><title>Lume</title></svelte:head>
<svelte:window onkeydown={keydown} />

{#snippet row(session: AgentSession, attention: boolean)}
  <button class="row" class:attention type="button" onclick={() => go("workspace")} title={tr("Open in the Workspace", "Abrir no Workspace")}>
    <ThreadAvatar seed={session.nativeSessionId || session.sessionName || session.id} label={nameOf(session)} size={30} />
    <span class="copy">
      <strong>{nameOf(session)}</strong>
      <small>{attention && askOf(session) ? askOf(session) : `${session.agentLabel} · ${session.project}`}</small>
    </span>
    <em class="status status-{session.status}"><i></i>{session.statusLabel}</em>
  </button>
{/snippet}

<main class="tray" class:dark data-appearance={look?.theme} style={look?.baseCss} style:--tray-accent={look?.accent ?? "var(--lume-accent-strong, #3d9a73)"}>
  <header>
    <LumeMascot status={needing.length ? "permission_required" : running.length ? "running" : "idle"} awake size={28} />
    <span><strong>Lume</strong><small>{summary}</small></span>
    <button class="close" type="button" aria-label={tr("Close", "Fechar")} onclick={hide}><svg viewBox="0 0 20 20" aria-hidden="true"><path d="m5.5 5.5 9 9m0-9-9 9" /></svg></button>
  </header>

  <div class="list">
    {#if needing.length}
      <p class="label">{tr("Need you", "Precisam de você")}</p>
      {#each needing as session (session.id)}{@render row(session, true)}{/each}
    {/if}
    {#if running.length}
      <p class="label">{tr("Running", "Rodando")}</p>
      {#each running as session (session.id)}{@render row(session, false)}{/each}
    {/if}
    {#if ready.length}
      <p class="label">{tr("Ready to review", "Prontos para revisar")}</p>
      {#each ready as session (session.id)}{@render row(session, false)}{/each}
    {/if}
    {#if recent.length}
      <p class="label">{tr("Recent", "Recentes")}</p>
      {#each recent as session (session.id)}{@render row(session, false)}{/each}
    {/if}
    {#if !needing.length && !running.length && !ready.length && !recent.length}
      <div class="empty"><LumeMascot status="idle" awake size={44} /><p>{tr("No agents yet. Start one in your terminal and it shows up here.", "Nenhum agente ainda. Abra um no terminal e ele aparece aqui.")}</p></div>
    {/if}
  </div>

  <footer>
    <button class="primary" type="button" onclick={() => go("workspace")}>{tr("Workspace", "Workspace")}</button>
    <button type="button" onclick={() => go("orb")}>{tr("Switch to the Orb", "Trocar para o Orb")}</button>
  </footer>
</main>

<style>
  :global(html), :global(body) { margin: 0; height: 100%; overflow: hidden; background: transparent; }
  .tray {
    --t-text: var(--lume-ink-strong-light); --t-muted: var(--lume-ink-muted-light); --t-line: var(--lume-line-light);
    --t-surface: var(--lume-surface-light); --t-raised: var(--lume-raised-light);
    box-sizing: border-box; height: 100vh; display: grid; grid-template-rows: auto minmax(0, 1fr) auto; overflow: hidden;
    border: 1px solid var(--t-line); border-radius: 16px; color: var(--t-text); background: var(--t-surface);
    font: 13px/1.4 var(--lume-font-ui, system-ui, sans-serif);
  }
  .tray.dark {
    --t-text: var(--lume-ink-strong-dark); --t-muted: var(--lume-ink-muted-dark); --t-line: var(--lume-line-dark);
    --t-surface: var(--lume-surface-dark); --t-raised: var(--lume-raised-dark);
  }
  header { padding: 12px 12px 8px 14px; display: flex; align-items: center; gap: 10px; }
  header span { flex: 1; min-width: 0; display: grid; }
  header strong { font-size: 14px; font-weight: 700; }
  header small { color: var(--t-muted); font-size: 12px; }
  .close { width: 28px; height: 28px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 8px; color: var(--t-muted); background: transparent; cursor: pointer; }
  .close:hover { color: var(--t-text); background: color-mix(in srgb, var(--t-muted) 12%, transparent); }
  .close svg { width: 14px; height: 14px; fill: none; stroke: currentColor; stroke-width: 1.7; stroke-linecap: round; }
  .list { min-height: 0; padding: 0 8px 6px; overflow-y: auto; scrollbar-width: thin; }
  .label { margin: 10px 8px 4px; color: var(--t-muted); font-size: 10.5px; font-weight: 700; letter-spacing: .06em; text-transform: uppercase; }
  .row { width: 100%; padding: 8px; display: flex; align-items: center; gap: 10px; border: 0; border-radius: 12px; color: inherit; background: transparent; font: inherit; text-align: left; cursor: pointer; transition: background 120ms ease; }
  .row:hover { background: color-mix(in srgb, var(--t-muted) 10%, transparent); }
  .row.attention { background: color-mix(in srgb, #d6a441 12%, transparent); }
  .row.attention:hover { background: color-mix(in srgb, #d6a441 20%, transparent); }
  .copy { flex: 1; min-width: 0; display: grid; }
  .copy strong { overflow: hidden; font-size: 13px; font-weight: 650; text-overflow: ellipsis; white-space: nowrap; }
  .copy small { overflow: hidden; color: var(--t-muted); font-size: 11.5px; text-overflow: ellipsis; white-space: nowrap; }
  .status { flex: 0 0 auto; max-width: 42%; display: inline-flex; align-items: center; gap: 5px; overflow: hidden; color: var(--t-muted); font-size: 11px; font-style: normal; font-weight: 650; text-overflow: ellipsis; white-space: nowrap; }
  .status i { width: 6px; height: 6px; flex: 0 0 auto; border-radius: 50%; background: #8a9891; }
  .status-running i { background: #4d99cc; }
  .status-permission_required i { background: #d6a441; }
  .status-waiting_for_input i { background: #4d99cc; opacity: .6; }
  .status-completed i { background: #4daa77; }
  .status-failed i { background: #d0625c; }
  .empty { margin: 24px 14px; display: grid; justify-items: center; gap: 10px; color: var(--t-muted); text-align: center; }
  .empty p { margin: 0; font-size: 12.5px; }
  footer { padding: 8px 10px 10px; display: flex; gap: 8px; border-top: 1px solid var(--t-line); }
  footer button { flex: 1; height: 34px; padding: 0 10px; border: 1px solid var(--t-line); border-radius: 10px; color: var(--t-text); background: var(--t-raised); font: inherit; font-weight: 650; cursor: pointer; }
  footer button:hover { border-color: color-mix(in srgb, var(--tray-accent) 55%, var(--t-line)); }
  footer .primary { border-color: transparent; color: #fff; background: var(--tray-accent); }
  footer .primary:hover { filter: brightness(1.06); }
  button:focus-visible { outline: 2px solid var(--tray-accent); outline-offset: 2px; }
</style>
