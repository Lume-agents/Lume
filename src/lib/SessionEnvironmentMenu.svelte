<script lang="ts">
  import { getContext, onMount, tick } from "svelte";
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import { copyResolvedColorTokens } from "$lib/floatingTheme";
  import { SYSTEM_BANNER_CONTEXT, type SystemBannerReporter } from "$lib/systemBannerContext";
  import { sessionEnvironments, environmentsForSession, runningEnvironments, environmentElapsed,
    stopSessionEnvironment, refreshSessionEnvironments, type SessionEnvironment } from "$lib/sessionEnvironments";
  import type { Language } from "$lib/i18n";

  let { sessionId, language = "en", variant = "fab", compact = false } = $props<{
    sessionId: string; language?: Language; variant?: "fab" | "sidebar"; compact?: boolean;
  }>();
  const report = getContext<SystemBannerReporter | undefined>(SYSTEM_BANNER_CONTEXT);
  const environments = $derived(environmentsForSession($sessionEnvironments, sessionId));
  const running = $derived(runningEnvironments(environments));
  const visible = $derived(variant === "fab" ? environments.length > 0 : running.length > 0);
  const databaseOnly = $derived(running.length > 0 && running.every((item) => item.kind === "database"));
  let open = $state(false);
  let trigger = $state<HTMLButtonElement | null>(null);
  let menu = $state<HTMLElement | null>(null);
  let busy = $state<string | null>(null);
  let error = $state("");
  let reducedMotion = $state(false);
  let left = $state(0);
  let top = $state(0);
  let maxHeight = $state(360);
  const tr = (en: string, pt: string) => language === "pt-BR" ? pt : en;
  const label = $derived(running.length
    ? tr(`${running.length} running environment${running.length > 1 ? "s" : ""}`, `${running.length} ambiente${running.length > 1 ? "s" : ""} rodando`)
    : tr("Session environments", "Ambientes da sessão"));

  function positionMenu() {
    if (!trigger || !menu) return;
    copyResolvedColorTokens(trigger, menu, ["raised", "text", "strong", "muted", "line", "accent", "subtle", "accent-soft"].map((token) => ({ source: `--workspace-${token}` })));
    const rect = trigger.getBoundingClientRect();
    const width = Math.min(324, window.innerWidth - 24);
    const height = Math.min(menu.scrollHeight, window.innerHeight - 32, 390);
    left = Math.max(12, Math.min(variant === "sidebar" ? rect.right + 10 : rect.left, window.innerWidth - width - 12));
    top = Math.max(12, Math.min(variant === "sidebar" ? rect.top : rect.top - height - 10, window.innerHeight - height - 12));
    maxHeight = Math.min(390, window.innerHeight - top - 12);
  }

  function floatMenu(node: HTMLElement) {
    menu = node;
    const retain = (event: PointerEvent) => event.stopPropagation();
    node.addEventListener("pointerdown", retain);
    document.body.appendChild(node);
    positionMenu();
    const observer = new ResizeObserver(positionMenu);
    observer.observe(node);
    return { destroy() { observer.disconnect(); node.removeEventListener("pointerdown", retain); menu = null; node.remove(); } };
  }

  async function toggle(event: MouseEvent) {
    event.stopPropagation();
    if (!open) window.dispatchEvent(new CustomEvent("lume:environment-menu-open", { detail: trigger }));
    open = !open;
    if (open) {
      error = "";
      await refreshSessionEnvironments();
      await tick();
      positionMenu();
      menu?.focus();
    }
  }
  function close(returnFocus = false) { open = false; if (returnFocus) trigger?.focus(); }
  function showError(value: unknown) {
    error = String(value);
    report?.({ id: `environment-${sessionId}`, message: error, tone: "error" });
  }
  async function stop(environment: SessionEnvironment) {
    busy = environment.id;
    error = "";
    try { await stopSessionEnvironment(sessionId, environment.id); }
    catch (value) { showError(value); }
    finally { busy = null; }
  }
  async function browse(url: string) { try { await openUrl(url); } catch (value) { showError(value); } }
  function status(environment: SessionEnvironment) {
    switch (environment.status) {
      case "running": return tr("Port open", "Porta aberta");
      case "stopping": return tr("Stopping…", "Parando…");
      case "idle": return tr("No open port", "Sem porta aberta");
      case "stopped": return tr("Stopped", "Parado");
      default: return tr("Status unavailable", "Estado indisponível");
    }
  }
  $effect(() => { if (!visible) open = false; });
  $effect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      if (!menu?.contains(event.target as Node) && !trigger?.contains(event.target as Node)) close();
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.stopPropagation(); close(true); }
    };
    const scroll = (event: Event) => { if (!menu?.contains(event.target as Node)) positionMenu(); };
    const otherMenu = (event: Event) => { if ((event as CustomEvent).detail !== trigger) close(); };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", escape, true);
    document.addEventListener("scroll", scroll, true);
    window.addEventListener("resize", positionMenu);
    window.addEventListener("lume:environment-menu-open", otherMenu);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("keydown", escape, true);
      document.removeEventListener("scroll", scroll, true);
      window.removeEventListener("resize", positionMenu);
      window.removeEventListener("lume:environment-menu-open", otherMenu);
    };
  });
  onMount(() => { reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches; });
</script>

{#if visible}
  <button bind:this={trigger} class="environment-trigger" class:fab={variant === "fab"} class:compact class:active={running.length > 0} class:open
    type="button" aria-label={label} data-tooltip={label} aria-haspopup="dialog" aria-expanded={open}
    onclick={toggle} onpointerdown={(event) => event.stopPropagation()} draggable="false">
    <LumeIcon name={databaseOnly ? "database" : "server"} size={variant === "fab" ? 20 : 15} />
    {#if variant === "fab" && running.length > 1}<span class="environment-count">{running.length}</span>{/if}
  </button>
{/if}

{#if open}
  <div use:floatMenu class="environment-menu" role="dialog" aria-label={tr("Session environments", "Ambientes da sessão")} tabindex="-1"
    style:left={`${left}px`} style:top={`${top}px`} style:max-height={`${maxHeight}px`}
    transition:fly={{ y: variant === "fab" ? 6 : 0, x: variant === "sidebar" ? -4 : 0, duration: reducedMotion ? 0 : 180, easing: cubicOut }}>
    <header><strong>{tr("Environments", "Ambientes")}</strong><button type="button" aria-label={tr("Close environments", "Fechar ambientes")} onclick={() => close(true)}><LumeIcon name="close" size={15} /></button></header>
    <div class="environment-list">
      {#each environments as environment (environment.id)}
        <article class="environment-row" class:stopped={environment.status === "stopped"}>
          <span class="environment-mark"><LumeIcon name={environment.kind === "database" ? "database" : "server"} size={21} /></span>
          <div class="environment-details">
            <div class="environment-name"><strong>{environment.name}</strong><time>{environmentElapsed(environment)}</time></div>
            <div class="environment-state"><i class:live={environment.status === "running"}></i><span>{status(environment)}</span><span class="environment-pid">PID {environment.processId}</span></div>
            <div class="environment-ports">
              {#each environment.ports as port (`${port.address}:${port.port}`)}
                {#if port.url && environment.status === "running"}
                  <button type="button" class="port-link" data-tooltip={tr("Open in browser", "Abrir no navegador")} onclick={() => void browse(port.url!)}><span>:{port.port}</span><LumeIcon name="external" size={11} /></button>
                {:else}<span class="port-label">:{port.port}</span>{/if}
              {/each}
            </div>
          </div>
          {#if environment.status !== "stopped"}
            <button class="stop-environment" type="button" disabled={!environment.canStop || busy === environment.id}
              aria-label={tr(`Stop ${environment.name}`, `Parar ${environment.name}`)} data-tooltip={tr("Stop this process", "Parar este processo")}
              onclick={() => void stop(environment)}><LumeIcon name={busy === environment.id || environment.status === "stopping" ? "refresh" : "stop"} size={15} /></button>
          {/if}
        </article>
      {/each}
    </div>
    {#if error || $sessionEnvironments.error}<p class="environment-error" role="alert">{error || $sessionEnvironments.error}</p>{/if}
  </div>
{/if}

<style>
  .environment-trigger { position: relative; width: 24px; height: 24px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 7px; color: var(--workspace-muted); background: transparent; cursor: pointer; transition: color 140ms ease, background 140ms ease; }
  .environment-trigger.active { color: var(--workspace-accent); }
  .environment-trigger:hover, .environment-trigger.open { color: var(--workspace-accent); background: var(--workspace-accent-soft); }
  .environment-trigger.fab { width: 36px; height: 36px; border-radius: 12px; color: var(--workspace-text); background: var(--workspace-raised); box-shadow: 0 5px 16px rgba(17, 35, 27, .12); }
  .environment-trigger.fab.active { color: var(--workspace-accent); }
  .environment-trigger.fab:hover, .environment-trigger.fab.open { background: var(--workspace-accent-soft); }
  .environment-trigger.compact { width: 24px; height: 24px; border-radius: 7px; background: var(--workspace-sidebar); }
  .environment-trigger:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 3px; }
  .environment-count { position: absolute; top: -3px; right: -3px; min-width: 15px; height: 15px; padding: 0 3px; display: grid; place-items: center; border-radius: 8px; color: var(--workspace-raised); background: var(--workspace-accent); font-size: 9px; font-weight: 750; font-variant-numeric: tabular-nums; }
  .environment-menu { position: fixed; z-index: 300; width: min(324px, calc(100vw - 24px)); padding: 8px; overflow: auto; overscroll-behavior: contain; box-sizing: border-box; border-radius: 14px; color: var(--workspace-text); background: var(--workspace-raised); box-shadow: 0 14px 42px rgba(8, 18, 13, .22); font-family: inherit; scrollbar-width: thin; scrollbar-color: var(--workspace-line) transparent; outline: none; }
  .environment-menu header { display: flex; align-items: center; justify-content: space-between; padding: 4px 5px 9px; }
  .environment-menu header > strong { color: var(--workspace-strong); font-size: 12px; font-weight: 700; }
  .environment-menu button { border: 0; padding: 0; color: var(--workspace-muted); background: transparent; cursor: pointer; }
  .environment-menu button:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 2px; }
  .environment-menu header > button { width: 24px; height: 24px; display: grid; place-items: center; border-radius: 6px; }
  .environment-menu button:hover:not(:disabled) { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .environment-list { display: grid; gap: 3px; }
  .environment-row { min-width: 0; display: flex; align-items: flex-start; gap: 10px; padding: 10px 7px; }
  .environment-row + .environment-row { border-top: 1px solid var(--workspace-line); }
  .environment-mark { display: grid; place-items: center; width: 26px; height: 26px; flex: 0 0 auto; color: var(--workspace-accent); }
  .environment-row.stopped .environment-mark { color: var(--workspace-muted); }
  .environment-details { flex: 1; min-width: 0; display: grid; gap: 5px; }
  .environment-name { display: flex; align-items: baseline; gap: 9px; justify-content: space-between; }
  .environment-name strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--workspace-strong); font-size: 12px; font-weight: 670; }
  .environment-name time { color: var(--workspace-muted); font-size: 10px; white-space: nowrap; font-variant-numeric: tabular-nums; }
  .environment-state { display: flex; align-items: center; flex-wrap: wrap; gap: 5px; color: var(--workspace-muted); font-size: 10px; }
  .environment-state i { width: 5px; height: 5px; border-radius: 50%; background: var(--workspace-muted); }
  .environment-state i.live { background: var(--workspace-accent); }
  .environment-pid { margin-left: auto; font-size: 9px; font-variant-numeric: tabular-nums; }
  .environment-ports { display: flex; flex-wrap: wrap; gap: 5px; }
  .environment-menu .port-link, .port-label { display: inline-flex; align-items: center; gap: 5px; padding: 3px 6px; border-radius: 5px; color: var(--workspace-accent); background: var(--workspace-subtle); font-size: 10px; font-variant-numeric: tabular-nums; }
  .port-label { color: var(--workspace-muted); }
  .environment-menu .stop-environment { width: 26px; height: 26px; display: grid; place-items: center; flex: 0 0 auto; border-radius: 7px; }
  .environment-menu .stop-environment:disabled { opacity: .35; cursor: default; }
  .environment-error { margin: 5px; padding: 8px; border-radius: 7px; color: var(--workspace-text); background: var(--workspace-subtle); font-size: 11px; line-height: 1.5; overflow-wrap: anywhere; }
  @media (prefers-reduced-motion: reduce) { .environment-trigger { transition: none; } }
</style>
