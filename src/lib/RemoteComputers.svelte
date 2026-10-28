<script lang="ts">
  import { getContext, onMount, untrack } from "svelte";
  import QRCode from "qrcode";
  import QrCode from "$lib/QrCode.svelte";
  import { createSystemBannerSource, SYSTEM_BANNER_CONTEXT, type SystemBannerReporter, type SystemBannerNotice } from "$lib/systemBannerContext";
  import type { DiscoveredLumeNode, PairingRequestHandle, PendingPairRequest, RelayLumeNode, RemoteLumeNode } from "$lib/domain";
  import { localize, type Language } from "$lib/i18n";
  import {
    checkLumeNodePairing,
    createThisNodePairing,
    decideNodePairRequest,
    discoverLumeNodes,
    forgetRelayLumeNode,
    forgetRemoteLumeNode,
    loadNodePairRequests,
    loadRelayLumeNodeOverview,
    loadRelayLumeNodes,
    loadRemoteLumeNodeHealth,
    loadRemoteLumeNodes,
    loadThisNode,
    pairLumeNode,
    pairLumeRelayNode,
    requestLumeNodePairing,
    revokeThisNodeDevice,
    saveNodeShareConversations,
    setNodeAllowApprovals,
    setNodeAllowPrompts,
    setThisNodeOn,
    type ThisNode,
  } from "$lib/lume";

  // Settings → Remote computers: this computer's access for other devices, and the computers this one is paired with.
  // Their sessions show in the session lists, not here.
  let { language = "en", dark = false }: { language?: Language; dark?: boolean } = $props();
  const desktop = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  const REFRESH_MS = 15_000;

  let me = $state<ThisNode | null>(null);
  let pairing = $state<{ link: string; expiresAt: number; qr: string } | null>(null);
  let now = $state(Date.now());
  let incoming = $state<PendingPairRequest[]>([]);
  let relayNodes = $state<RelayLumeNode[]>([]);
  let lanNodes = $state<RemoteLumeNode[]>([]);
  // What each paired computer answered last: its name, whether it is online and how many sessions it has.
  let computers = $state<Record<string, { name: string; online: boolean; sessions?: number }>>({});
  let discovered = $state<DiscoveredLumeNode[]>([]);
  let requesting = $state<{ node: DiscoveredLumeNode; handle: PairingRequestHandle } | null>(null);
  let requestTimer: ReturnType<typeof setInterval> | undefined;
  let link = $state("");
  let busy = $state(false);
  let message = $state("");
  let failed = $state(false);
  const bannerReporter = getContext<SystemBannerReporter | undefined>(SYSTEM_BANNER_CONTEXT);
  const reportNotices = createSystemBannerSource(bannerReporter);

  const tr = (english: string, portuguese: string) => localize(language, english, portuguese);
  const errorMessage = (error: unknown) => String(error).replace(/^Error:\s*/, "");
  const on = $derived(Boolean(me && me.lifecycle !== "disabled"));
  const presence = $derived(!me || me.lifecycle === "disabled" ? "off" : me.lifecycle === "running" && me.relayConnected ? "online" : "starting");
  const remaining = $derived(pairing ? Math.max(0, Math.round((pairing.expiresAt - now) / 1000)) : 0);
  const paired = $derived([
    ...relayNodes.map((node) => node.nodeId),
    ...lanNodes.map((node) => node.nodeId).filter((id) => !relayNodes.some((node) => node.nodeId === id)),
  ]);

  $effect(() => {
    if (!bannerReporter) return;
    const notices: SystemBannerNotice[] = [];
    const current = message;
    if (current) notices.push({ id: "remote-computers-message", message: current, tone: failed ? "error" : "info", onDismiss: () => { if (message === current) message = ""; } });
    untrack(() => reportNotices(notices));
  });

  async function perform(action: () => Promise<void>) {
    if (!desktop || busy) return;
    busy = true;
    message = "";
    failed = false;
    try { await action(); }
    catch (error) { failed = true; message = errorMessage(error); }
    finally { busy = false; }
  }

  function relativeTime(at?: number | null) {
    if (!at) return tr("never", "nunca");
    const minutes = Math.round((Date.now() - at) / 60_000);
    if (minutes < 1) return tr("now", "agora");
    if (minutes < 60) return tr(`${minutes} min ago`, `há ${minutes} min`);
    const hours = Math.round(minutes / 60);
    return hours < 24 ? tr(`${hours} h ago`, `há ${hours} h`) : new Intl.DateTimeFormat(language, { day: "numeric", month: "short" }).format(at);
  }

  async function refreshComputers() {
    if (!desktop || document.hidden) return;
    relayNodes = await loadRelayLumeNodes().catch(() => relayNodes);
    lanNodes = await loadRemoteLumeNodes().catch(() => lanNodes);
    await Promise.all(paired.map(async (nodeId) => {
      try {
        if (relayNodes.some((node) => node.nodeId === nodeId)) {
          const overview = await loadRelayLumeNodeOverview(nodeId);
          computers[nodeId] = { name: overview.health.displayName || nodeId, online: overview.health.lifecycle === "running", sessions: overview.sessions?.sessions.length };
        } else {
          const health = await loadRemoteLumeNodeHealth(nodeId);
          computers[nodeId] = { name: health.displayName || nodeId, online: health.lifecycle === "running" };
        }
      } catch {
        computers[nodeId] = { name: computers[nodeId]?.name ?? nodeId, online: false };
      }
    }));
  }

  async function refreshMe() {
    if (!desktop || document.hidden) return;
    me = await loadThisNode().catch(() => me);
    incoming = await loadNodePairRequests().catch(() => []);
  }

  const turn = (value: boolean) => perform(async () => {
    me = await setThisNodeOn(value);
    if (!value) pairing = null;
    incoming = value ? await loadNodePairRequests().catch(() => []) : [];
  });

  const newLink = () => perform(async () => {
    const created = await createThisNodePairing();
    pairing = { ...created, qr: await QRCode.toDataURL(created.link, { width: 176, margin: 2, errorCorrectionLevel: "M" }) };
  });

  const copyLink = () => { if (pairing) void navigator.clipboard?.writeText(pairing.link); message = tr("Link copied.", "Link copiado."); failed = false; };

  const revoke = (deviceId: string) => perform(async () => { me = await revokeThisNodeDevice(deviceId); });

  const setSwitch = (key: "shareConversations" | "allowPrompts" | "allowApprovals", value: boolean) => perform(async () => {
    const saved = key === "shareConversations" ? await saveNodeShareConversations(value) : key === "allowPrompts" ? await setNodeAllowPrompts(value) : await setNodeAllowApprovals(value);
    if (me) me = { ...me, [key]: saved };
  });

  const answer = (request: PendingPairRequest, approve: boolean) => perform(async () => {
    await decideNodePairRequest(request.requestId, approve);
    incoming = await loadNodePairRequests();
    await refreshMe();
  });

  const connect = () => {
    const uri = link.trim();
    if (!uri) return;
    return perform(async () => {
      if (uri.startsWith("lume://pair-relay")) await pairLumeRelayNode(uri);
      else await pairLumeNode(uri);
      link = "";
      message = tr("Computer connected.", "Computador conectado.");
      await refreshComputers();
    });
  };

  const scan = () => perform(async () => {
    discovered = await discoverLumeNodes();
    if (!discovered.length) message = tr("No computer found on this network.", "Nenhum computador encontrado nesta rede.");
  });

  function stopRequest() {
    clearInterval(requestTimer);
    requestTimer = undefined;
    requesting = null;
  }

  const askToPair = (node: DiscoveredLumeNode) => perform(async () => {
    stopRequest();
    requesting = { node, handle: await requestLumeNodePairing(node) };
    requestTimer = setInterval(() => void pollRequest(), 2000);
  });

  async function pollRequest() {
    const current = requesting;
    if (!current) return;
    try {
      const check = await checkLumeNodePairing(current.node, current.handle.requestId);
      if (requesting !== current || check.status === "pending") return;
      stopRequest();
      failed = check.status !== "approved";
      message = check.status === "approved" ? tr("Computer connected.", "Computador conectado.")
        : check.status === "denied" ? tr("The other computer refused.", "O outro computador recusou.")
        : tr("The request expired.", "O pedido expirou.");
      discovered = [];
      await refreshComputers();
    } catch (error) {
      stopRequest();
      failed = true;
      message = errorMessage(error);
    }
  }

  const forget = (nodeId: string) => perform(async () => {
    if (relayNodes.some((node) => node.nodeId === nodeId)) await forgetRelayLumeNode(nodeId);
    if (lanNodes.some((node) => node.nodeId === nodeId)) await forgetRemoteLumeNode(nodeId);
    delete computers[nodeId];
    await refreshComputers();
  });

  onMount(() => {
    void refreshMe();
    void refreshComputers();
    const timer = setInterval(() => { void refreshMe(); void refreshComputers(); }, REFRESH_MS);
    const clock = setInterval(() => { now = Date.now(); if (pairing && pairing.expiresAt <= now) pairing = null; }, 1000);
    const onVisible = () => { if (!document.hidden) { void refreshMe(); void refreshComputers(); } };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      clearInterval(timer);
      clearInterval(clock);
      stopRequest();
      document.removeEventListener("visibilitychange", onVisible);
    };
  });
</script>

<div class="remote-computers" class:dark aria-busy={busy}>
  <section class="block">
    <label class="row switch-row">
      <span class="title"><strong>{tr("This computer", "Este computador")}</strong><small class={`state ${presence}`}><i></i>{presence === "online" ? tr("Online", "Online") : presence === "starting" ? tr("Connecting…", "Conectando…") : tr("Off", "Desligado")}</small></span>
      <input type="checkbox" checked={on} disabled={!desktop || busy} onchange={(event) => void turn(event.currentTarget.checked)} aria-label={tr("Access from other computers", "Acesso de outros computadores")} />
      <i class="track" aria-hidden="true"></i>
    </label>
    {#if on}
      {#if pairing}
        <div class="pairing">
          <span class="qr-box"><QrCode src={pairing.qr} alt={tr("Pairing QR code", "QR Code de pareamento")} /></span>
          <div>
            <code>{pairing.link}</code>
            <span class="actions"><button type="button" onclick={copyLink}>{tr("Copy", "Copiar")}</button><small>{Math.floor(remaining / 60)}:{String(remaining % 60).padStart(2, "0")}</small></span>
          </div>
        </div>
      {:else}
        <button class="wide" type="button" disabled={!desktop || busy} onclick={() => void newLink()}>{tr("Pairing link", "Link de pareamento")}</button>
      {/if}
      {#each incoming as request (request.requestId)}
        <div class="row request">
          <span class="title"><strong>{request.displayName}</strong><small>{tr("Code", "Código")} {request.code}</small></span>
          <button type="button" disabled={busy} onclick={() => void answer(request, false)}>{tr("Refuse", "Recusar")}</button>
          <button class="primary" type="button" disabled={busy} onclick={() => void answer(request, true)}>{tr("Allow", "Permitir")}</button>
        </div>
      {/each}
      {#each [["shareConversations", tr("Read conversations", "Ler conversas")], ["allowPrompts", tr("Send prompts", "Enviar prompts")], ["allowApprovals", tr("Answer permissions", "Responder permissões")]] as [key, label] (key)}
        <label class="row switch-row compact">
          <span>{label}</span>
          <input type="checkbox" checked={Boolean(me?.[key as "shareConversations"])} disabled={!desktop || busy} onchange={(event) => void setSwitch(key as "shareConversations", event.currentTarget.checked)} />
          <i class="track" aria-hidden="true"></i>
        </label>
      {/each}
      {#each me?.devices ?? [] as device (device.deviceId)}
        <div class="row device">
          <span class="title"><strong>{device.name}</strong>{#if device.lastSeenAt && now - device.lastSeenAt <= 90_000}<small class="state online"><i></i>{tr("Connected", "Conectado")}</small>{:else}<small>{relativeTime(device.lastSeenAt)}</small>{/if}</span>
          <button type="button" disabled={busy} onclick={() => void revoke(device.deviceId)}>{tr("Revoke", "Revogar")}</button>
        </div>
      {/each}
    {/if}
  </section>

  <section class="block">
    <div class="row"><strong class="heading">{tr("Other computers", "Outros computadores")}</strong><button type="button" disabled={!desktop || busy} onclick={() => void scan()}>{tr("Scan", "Buscar")}</button></div>
    <div class="row connect">
      <input aria-label={tr("Pairing link", "Link de pareamento")} autocomplete="off" spellcheck="false" placeholder={tr("Paste a pairing link", "Cole um link de pareamento")} disabled={!desktop || busy} bind:value={link} onkeydown={(event) => { if (event.key === "Enter") void connect(); }} />
      <button class="primary" type="button" disabled={!desktop || busy || !link.trim()} onclick={() => void connect()}>{tr("Connect", "Conectar")}</button>
    </div>
    {#if requesting}
      <div class="row request">
        <span class="title"><strong>{tr("Approve on the other computer", "Aprove no outro computador")}</strong><small>{tr("Code", "Código")} <b class="code">{requesting.handle.code}</b></small></span>
        <button type="button" onclick={stopRequest}>{tr("Cancel", "Cancelar")}</button>
      </div>
    {/if}
    {#each discovered.filter((node) => !paired.includes(node.nodeId)) as node (node.nodeId)}
      <div class="row">
        <span class="title"><strong>{node.nodeId}</strong><small>{node.address}</small></span>
        <button type="button" disabled={busy || Boolean(requesting)} onclick={() => void askToPair(node)}>{tr("Pair", "Parear")}</button>
      </div>
    {/each}
    {#each paired as nodeId (nodeId)}
      {@const computer = computers[nodeId]}
      <article class="row computer" data-node={nodeId}>
        <span class="title">
          <strong>{computer?.name ?? nodeId}</strong>
          <small class={`state ${computer?.online ? "online" : "off"}`}><i></i>{computer?.online ? tr("Online", "Online") : tr("Offline", "Offline")}{#if computer?.online && computer.sessions !== undefined}{" · "}{computer.sessions === 1 ? tr("1 session", "1 sessão") : tr(`${computer.sessions} sessions`, `${computer.sessions} sessões`)}{/if}</small>
        </span>
        <button type="button" disabled={busy} onclick={() => void forget(nodeId)}>{tr("Forget", "Esquecer")}</button>
      </article>
    {/each}
  </section>
</div>

<style>
  .remote-computers { --ink: var(--lume-ink-light, #35423d); --muted: var(--lume-ink-muted-light, #89938f); --line: var(--lume-line-light, rgba(92, 111, 103, 0.14)); --surface: var(--lume-raised-light, rgba(255, 255, 255, 0.48)); display: grid; gap: 10px; color: var(--ink); font-size: 10px; }
  .dark { --ink: var(--lume-ink-dark, #dce6e0); --muted: var(--lume-ink-muted-dark, #8e9b94); --line: var(--lume-line-dark, rgba(190, 209, 200, 0.14)); --surface: var(--lume-raised-dark, #25312c); }
  .block { padding: 10px 11px; display: grid; gap: 7px; border: 1px solid var(--line); border-radius: 12px; background: color-mix(in srgb, var(--surface) 35%, transparent); }
  .row { min-height: 28px; display: flex; align-items: center; gap: 8px; min-width: 0; }
  .title { flex: 1; min-width: 0; display: grid; gap: 1px; }
  .title strong, .heading { overflow: hidden; font-size: 10.5px; font-weight: 700; text-overflow: ellipsis; white-space: nowrap; }
  .heading { flex: 1; }
  .title small { overflow: hidden; color: var(--muted); font-size: 9px; text-overflow: ellipsis; white-space: nowrap; }
  .state { display: inline-flex; align-items: center; gap: 5px; }
  .state i { width: 6px; height: 6px; border-radius: 50%; background: #8a9891; }
  .state.online i { background: #4daa77; }
  .state.starting i { background: #d6a441; }
  button { min-height: 26px; padding: 0 9px; flex-shrink: 0; border: 1px solid var(--line); border-radius: 7px; color: var(--ink); background: transparent; font: inherit; font-size: 9.5px; cursor: pointer; }
  button:disabled { opacity: .5; cursor: default; }
  button.primary { border-color: transparent; color: #fff; background: var(--lume-accent-strong, #397b5c); }
  button.wide { width: 100%; }
  input:not([type="checkbox"]) { flex: 1; min-width: 0; height: 28px; padding: 0 9px; border: 1px solid var(--line); border-radius: 8px; color: inherit; background: transparent; font: inherit; }
  .switch-row { position: relative; cursor: pointer; }
  .switch-row > span:first-child { flex: 1; }
  .switch-row.compact { min-height: 24px; }
  .switch-row input { position: absolute; opacity: 0; pointer-events: none; }
  .track { position: relative; width: 28px; height: 16px; flex: 0 0 auto; border-radius: 999px; background: color-mix(in srgb, var(--muted) 35%, transparent); transition: background 140ms ease; }
  .track::after { position: absolute; top: 2px; left: 2px; width: 12px; height: 12px; border-radius: 50%; background: #fff; content: ""; transition: transform 140ms ease; }
  .switch-row input:checked + .track { background: var(--lume-accent-strong, #397b5c); }
  .switch-row input:checked + .track::after { transform: translateX(12px); }
  .switch-row input:focus-visible + .track { outline: 2px solid var(--lume-accent-strong, #397b5c); outline-offset: 2px; }
  .pairing { display: flex; gap: 10px; align-items: center; }
  .qr-box { width: 96px; height: 96px; flex: 0 0 auto; border-radius: 8px; background: #fff; }
  .pairing > div { min-width: 0; flex: 1; display: grid; gap: 6px; }
  .pairing code { display: block; max-height: 48px; overflow: hidden; color: var(--muted); font-size: 8.5px; word-break: break-all; }
  .actions { display: flex; align-items: center; gap: 8px; }
  .actions small { color: var(--muted); }
  .code { letter-spacing: .12em; }
  @media (prefers-reduced-motion: reduce) { .track, .track::after { transition: none; } }
</style>
