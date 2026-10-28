<script lang="ts">
  import { getContext, onMount, untrack } from "svelte";
  import { createSystemBannerSource, SYSTEM_BANNER_CONTEXT, type SystemBannerReporter, type SystemBannerNotice } from "$lib/systemBannerContext";
  import BrandIcon from "$lib/BrandIcon.svelte";
  import type { DiscoveredLumeNode, PairingRequestHandle, PendingPairRequest, RelayLumeNode, RelayLumeNodeOverview, RemoteAgentSession, RemoteConversation, RemoteLumeNode, RemoteLumeNodeHealth, RemoteLumeNodeInventory } from "$lib/domain";
  import { localize, type Language } from "$lib/i18n";
  import {
    checkLumeNodePairing,
    decideNodePairRequest,
    discoverLumeNodes,
    forgetRelayLumeNode,
    forgetRemoteLumeNode,
    loadNodePairRequests,
    loadNodeShareConversations,
    loadRelayLumeNodeOverview,
    loadRelayLumeSession,
    loadNodeAllowPrompts,
    sendRelayLumePrompt,
    setNodeAllowPrompts,
    loadRelayLumeNodes,
    pairLumeRelayNode,
    requestLumeNodePairing,
    saveNodeShareConversations,
    loadRemoteLumeNodeHealth,
    loadRemoteLumeNodeInventory,
    loadRemoteLumeNodes,
    pairLumeNode,
  } from "$lib/lume";

  let { language = "en", dark = false }: { language?: Language; dark?: boolean } = $props();
  const desktop = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  let nodes = $state<RemoteLumeNode[]>([]);
  let discovered = $state<DiscoveredLumeNode[]>([]);
  let health = $state<Record<string, RemoteLumeNodeHealth>>({});
  let inventory = $state<Record<string, RemoteLumeNodeInventory>>({});
  let inventoryErrors = $state<Record<string, string>>({});
  let checkedAt = $state<Record<string, number>>({});
  let relayNodes = $state<RelayLumeNode[]>([]);
  let overviews = $state<Record<string, RelayLumeNodeOverview>>({});
  let relayErrors = $state<Record<string, string>>({});
  let relayCheckedAt = $state<Record<string, number>>({});
  const relayInFlight = new Set<string>();
  const RELAY_REFRESH_MS = 6000;
  let openChat = $state<{ nodeId: string; sessionId: string; title: string } | null>(null);
  let conversation = $state<RemoteConversation | null>(null);
  let conversationError = $state("");
  let shareConversations = $state(false);
  let allowPrompts = $state(false);
  let promptText = $state("");
  let promptNote = $state("");
  let promptFailed = $state(false);
  let sendingPrompt = $state(false);
  // The same id is kept until a prompt is delivered, so sending again after a lost answer never runs it twice.
  let promptRequestId: string | null = null;
  let chatScroller = $state<HTMLElement | undefined>();
  let requesting = $state<{ node: DiscoveredLumeNode; handle: PairingRequestHandle } | null>(null);
  let requestTimer: ReturnType<typeof setInterval> | undefined;
  let incoming = $state<PendingPairRequest[]>([]);
  let pairingUri = $state("");
  let busy = $state(false);
  let message = $state("");
  let failed = $state(false);
  let scrolling = $state(false);
  let scrollTimer: ReturnType<typeof setTimeout> | undefined;
  const bannerReporter = getContext<SystemBannerReporter | undefined>(SYSTEM_BANNER_CONTEXT);
  const reportNotices = createSystemBannerSource(bannerReporter);

  $effect(() => {
    if (!bannerReporter) return;
    const notices: SystemBannerNotice[] = [];
    const currentMessage = message;
    if (currentMessage) notices.push({
      id: "remote-computers-message",
      message: currentMessage,
      tone: failed ? "error" : "info",
      onDismiss: () => { if (message === currentMessage) message = ""; },
    });
    for (const [nodeId, error] of Object.entries(inventoryErrors)) notices.push({
      id: `remote-inventory:${nodeId}`,
      message: tr("Inventory unavailable: ", "Inventário indisponível: ") + error,
      tone: "error",
    });
    for (const [nodeId, error] of Object.entries(relayErrors)) notices.push({
      id: `relay-computer:${nodeId}`,
      message: tr("Computer unreachable: ", "Computador inacessível: ") + error,
      tone: "error",
    });
    untrack(() => reportNotices(notices));
  });

  function tr(english: string, portuguese: string) {
    return localize(language, english, portuguese);
  }

  function errorMessage(error: unknown) {
    return String(error).replace(/^Error:\s*/, "");
  }

  function snapshotTime(timestamp: number) {
    if (!Number.isFinite(timestamp) || Math.abs(timestamp) > 8.64e15) return "—";
    return new Intl.DateTimeFormat(language, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }).format(timestamp);
  }

  function showScrollbar() {
    scrolling = true;
    clearTimeout(scrollTimer);
    scrollTimer = setTimeout(() => { scrolling = false; }, 700);
  }

  async function perform(action: () => Promise<void>) {
    if (!desktop || busy) return;
    busy = true;
    message = "";
    failed = false;
    try {
      await action();
    } catch (error) {
      failed = true;
      message = errorMessage(error);
    } finally {
      busy = false;
    }
  }

  async function inspect(nodeId: string) {
    const [healthResult, inventoryResult] = await Promise.allSettled([
      loadRemoteLumeNodeHealth(nodeId),
      loadRemoteLumeNodeInventory(nodeId),
    ]);
    checkedAt[nodeId] = Date.now();
    if (healthResult.status === "rejected") {
      // A failed check must not leave the previous snapshot marked reachable.
      delete health[nodeId];
      delete inventory[nodeId];
      delete inventoryErrors[nodeId];
      throw healthResult.reason;
    }
    health[nodeId] = healthResult.value;
    if (inventoryResult.status === "fulfilled") {
      inventory[nodeId] = inventoryResult.value;
      delete inventoryErrors[nodeId];
    } else {
      delete inventory[nodeId];
      inventoryErrors[nodeId] = errorMessage(inventoryResult.reason);
    }
    nodes = await loadRemoteLumeNodes();
  }

  // One connection through the Relay per computer; a computer is never asked again while the previous answer is pending.
  async function refreshRelay(nodeId: string) {
    if (relayInFlight.has(nodeId)) return;
    relayInFlight.add(nodeId);
    // While a conversation is open only it is refreshed: one connection per computer at a time.
    const chat = openChat?.nodeId === nodeId ? openChat : null;
    try {
      if (chat) {
        const next = await loadRelayLumeSession(nodeId, chat.sessionId);
        if (openChat === chat) { conversation = next; conversationError = ""; }
      } else {
        overviews[nodeId] = await loadRelayLumeNodeOverview(nodeId);
      }
      delete relayErrors[nodeId];
    } catch (error) {
      if (chat) {
        if (openChat === chat) conversationError = friendlyConversationError(errorMessage(error));
      } else {
        delete overviews[nodeId];
        relayErrors[nodeId] = errorMessage(error);
      }
    } finally {
      relayCheckedAt[nodeId] = Date.now();
      relayInFlight.delete(nodeId);
    }
  }

  function friendlyConversationError(error: string) {
    if (error.includes("not allowed")) return tr("The owner of that computer has not allowed reading its conversations. They can turn it on in Settings → Remote computers.", "O dono daquele computador não liberou a leitura das conversas. Ele pode ativar em Configurações → Computadores remotos.");
    if (error.includes("pair it again")) return tr("This computer was paired before conversations could be shared. Forget it and pair it again.", "Este computador foi pareado antes de as conversas poderem ser compartilhadas. Esqueça e pareie de novo.");
    return error;
  }

  function friendlyPromptError(error: string) {
    if (error.includes("not allowed")) return tr("The owner of that computer has not allowed prompts from other devices. They can turn it on in Settings → Remote computers.", "O dono daquele computador não permitiu prompts de outros dispositivos. Ele pode ligar em Ajustes → Computadores remotos.");
    if (error.includes("pair it again")) return tr("This computer was paired before prompts could be sent. Forget it and pair it again.", "Este computador foi pareado antes de os prompts poderem ser enviados. Esqueça-o e pareie de novo.");
    if (error.includes("not open")) return tr("Lume is not open on that computer.", "O Lume não está aberto naquele computador.");
    if (error.includes("clocks")) return tr("The two computers' clocks differ by more than a minute. Fix the time on one of them.", "Os relógios dos dois computadores diferem em mais de um minuto. Acerte a hora de um deles.");
    return error;
  }

  async function sendPrompt() {
    const chat = openChat;
    const text = promptText.trim();
    if (!chat || !text || sendingPrompt) return;
    sendingPrompt = true;
    promptNote = "";
    promptFailed = false;
    promptRequestId ??= crypto.randomUUID();
    try {
      await sendRelayLumePrompt(chat.nodeId, chat.sessionId, promptRequestId, text);
      promptText = "";
      promptRequestId = null;
      promptNote = tr("Sent.", "Enviado.");
      void refreshRelay(chat.nodeId);
    } catch (error) {
      promptFailed = true;
      promptNote = friendlyPromptError(errorMessage(error));
    } finally {
      sendingPrompt = false;
    }
  }

  function setAllowPrompts(enabled: boolean) {
    return perform(async () => {
      allowPrompts = await setNodeAllowPrompts(enabled);
      message = allowPrompts
        ? tr("Paired devices can now send prompts to this computer's agents.", "Dispositivos pareados agora podem enviar prompts aos agentes deste computador.")
        : tr("Prompts from other devices are blocked again.", "Os prompts de outros dispositivos voltaram a ser bloqueados.");
    });
  }

  function openConversation(nodeId: string, session: RemoteAgentSession) {
    openChat = { nodeId, sessionId: session.id, title: sessionTitle(session) };
    conversation = null;
    conversationError = "";
    void refreshRelay(nodeId);
  }

  function closeConversation() {
    const nodeId = openChat?.nodeId;
    openChat = null;
    conversation = null;
    conversationError = "";
    if (nodeId) void refreshRelay(nodeId);
  }

  function setShareConversations(enabled: boolean) {
    return perform(async () => {
      shareConversations = await saveNodeShareConversations(enabled);
      message = shareConversations
        ? tr("Paired devices can now read this computer's conversations.", "Dispositivos pareados agora podem ler as conversas deste computador.")
        : tr("Conversations are private again.", "As conversas voltaram a ser privadas.");
    });
  }

  $effect(() => {
    // Keep the newest message in view when a conversation opens or grows.
    const count = conversation?.messages.length;
    if (count && chatScroller) untrack(() => { chatScroller!.scrollTop = chatScroller!.scrollHeight; });
  });

  function refreshAllRelay() {
    if (!desktop || document.hidden) return;
    for (const node of relayNodes) void refreshRelay(node.nodeId);
  }

  function sessionDot(status: string) {
    return status === "running" ? "running" : status === "permission_required" || status === "waiting_for_input" ? "attention" : status === "failed" ? "failed" : "idle";
  }

  function sessionState(session: { status: string; statusLabel: string; needsPermission: boolean; needsAnswer: boolean }) {
    if (session.needsPermission) return tr("Needs permission", "Pede permissão");
    if (session.needsAnswer) return tr("Needs an answer", "Pede uma resposta");
    switch (session.status) {
      case "running": return tr("Working", "Trabalhando");
      case "waiting_for_input": return tr("Waiting for you", "Esperando você");
      case "completed": return tr("Finished", "Finalizado");
      case "failed": return tr("Failed", "Falhou");
      default: return session.statusLabel;
    }
  }

  function sessionTitle(session: { name: string; project: string }) {
    return session.name || session.project || "—";
  }

  function agoLabel(timestamp: number) {
    const seconds = Math.max(0, Math.round((Date.now() - timestamp) / 1000));
    if (seconds < 60) return tr("now", "agora");
    if (seconds < 3600) return `${Math.floor(seconds / 60)} min`;
    if (seconds < 86400) return `${Math.floor(seconds / 3600)} h`;
    return `${Math.floor(seconds / 86400)} d`;
  }

  function forgetRelay(nodeId: string) {
    return perform(async () => {
      await forgetRelayLumeNode(nodeId);
      relayNodes = await loadRelayLumeNodes();
      delete overviews[nodeId];
      delete relayErrors[nodeId];
      delete relayCheckedAt[nodeId];
      message = tr("Computer forgotten on this Lume client. Revoke the device on that computer to remove its access.", "Computador esquecido neste Lume. Revogue o dispositivo naquele computador para retirar o acesso dele.");
    });
  }

  function spacedCode(code: string) {
    return code.length === 6 ? `${code.slice(0, 3)} ${code.slice(3)}` : code;
  }

  function stopRequest() {
    clearInterval(requestTimer);
    requestTimer = undefined;
    requesting = null;
  }

  // Pairing without a link: the other computer approves after comparing the code both screens show.
  function askToPair(node: DiscoveredLumeNode) {
    return perform(async () => {
      stopRequest();
      const handle = await requestLumeNodePairing(node);
      requesting = { node, handle };
      requestTimer = setInterval(() => void pollRequest(), 2000);
    });
  }

  async function pollRequest() {
    const current = requesting;
    if (!current) return;
    try {
      const check = await checkLumeNodePairing(current.node, current.handle.requestId);
      if (requesting !== current || check.status === "pending") return;
      stopRequest();
      if (check.status === "approved") {
        nodes = await loadRemoteLumeNodes();
        relayNodes = await loadRelayLumeNodes();
        failed = false;
        message = check.relay
          ? tr("Computer paired. It is also reachable away from this network.", "Computador pareado. Ele também fica acessível fora desta rede.")
          : tr("Computer paired on this network.", "Computador pareado nesta rede.");
        refreshAllRelay();
        if (check.node) void inspect(check.node.nodeId).catch(() => undefined);
      } else {
        failed = true;
        message = check.status === "denied"
          ? tr("The other computer refused the pairing.", "O outro computador recusou o pareamento.")
          : tr("The pairing request expired. Try again.", "O pedido de pareamento expirou. Tente de novo.");
      }
    } catch (error) {
      stopRequest();
      failed = true;
      message = errorMessage(error);
    }
  }

  async function refreshIncoming() {
    if (!desktop || document.hidden) return;
    try { incoming = await loadNodePairRequests(); } catch { incoming = []; }
  }

  function answer(request: PendingPairRequest, approve: boolean) {
    return perform(async () => {
      await decideNodePairRequest(request.requestId, approve);
      incoming = await loadNodePairRequests();
      message = approve
        ? tr(`${request.displayName} can now see this computer's agents.`, `${request.displayName} já pode ver os agentes deste computador.`)
        : tr("Request refused.", "Pedido recusado.");
    });
  }

  function scan() {
    return perform(async () => {
      discovered = await discoverLumeNodes();
      message = discovered.length
        ? tr(
            `${discovered.length} computer${discovered.length === 1 ? "" : "s"} found.`,
            `${discovered.length} computador${discovered.length === 1 ? " encontrado" : "es encontrados"}.`,
          )
        : tr("No Lume Node found on this network.", "Nenhum Lume Node encontrado nesta rede.");
    });
  }

  function pair() {
    const uri = pairingUri.trim();
    if (!uri) return;
    if (uri.startsWith("lume://pair-relay")) {
      return perform(async () => {
        const node = await pairLumeRelayNode(uri);
        pairingUri = "";
        relayNodes = await loadRelayLumeNodes();
        message = tr("Computer paired through the Relay.", "Computador pareado pelo Relay.");
        void refreshRelay(node.nodeId);
      });
    }
    return perform(async () => {
      const node = await pairLumeNode(uri);
      pairingUri = "";
      nodes = await loadRemoteLumeNodes();
      // Pairing is already durable even if the first inventory check fails.
      try {
        await inspect(node.nodeId);
        message = tr("Computer paired.", "Computador pareado.");
      } catch (error) {
        failed = true;
        message = tr("Paired. Check the connection again: ", "Pareado. Verifique a conexão novamente: ") + errorMessage(error);
      }
    });
  }

  function check(nodeId: string) {
    return perform(async () => {
      await inspect(nodeId);
    });
  }

  function forget(nodeId: string) {
    return perform(async () => {
      await forgetRemoteLumeNode(nodeId);
      nodes = await loadRemoteLumeNodes();
      delete health[nodeId];
      delete inventory[nodeId];
      delete inventoryErrors[nodeId];
      delete checkedAt[nodeId];
      message = tr("Computer forgotten on this Lume client.", "Computador esquecido neste Lume.");
    });
  }

  onMount(() => {
    void perform(async () => {
      nodes = await loadRemoteLumeNodes();
      relayNodes = await loadRelayLumeNodes();
      refreshAllRelay();
      await refreshIncoming();
      shareConversations = await loadNodeShareConversations().catch(() => false);
      allowPrompts = await loadNodeAllowPrompts().catch(() => false);
    });
    const timer = setInterval(() => { refreshAllRelay(); void refreshIncoming(); }, RELAY_REFRESH_MS / 2);
    const onVisible = () => { if (!document.hidden) { refreshAllRelay(); void refreshIncoming(); } };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      clearTimeout(scrollTimer);
      clearInterval(timer);
      stopRequest();
      document.removeEventListener("visibilitychange", onVisible);
    };
  });
</script>

<div class="remote-computers" class:dark aria-busy={busy}>
  <div class="heading">
    <span><strong>Lume Node</strong><small>{tr("Pair a trusted computer to see its agents and available models here.", "Pareie um computador confiável para ver seus agentes e modelos disponíveis aqui.")}</small></span>
    <button disabled={!desktop || busy} type="button" onclick={() => void scan()}>{busy ? "…" : tr("Scan", "Buscar")}</button>
  </div>
  <div class="pair-control">
    <input
      aria-label={tr("One-time Lume Node pairing link", "Link de pareamento de uso único do Lume Node")}
      autocomplete="off" placeholder="lume://pair-node?… / lume://pair-relay?…" spellcheck="false" type="password"
      disabled={!desktop || busy} bind:value={pairingUri}
      onkeydown={(event) => { if (event.key === "Enter") void pair(); }}
    />
    <button disabled={!desktop || busy || !pairingUri.trim()} type="button" onclick={() => void pair()}>{tr("Pair", "Parear")}</button>
  </div>
  <label class="share-switch">
    <input type="checkbox" checked={shareConversations} disabled={!desktop || busy} onchange={(event) => void setShareConversations(event.currentTarget.checked)} />
    <i class="track" aria-hidden="true"></i>
    <span><strong>{tr("Let paired devices read this computer's conversations", "Permitir que dispositivos pareados leiam as conversas deste computador")}</strong><small>{tr("Off by default. Only what was said, never commands or files. Takes effect at once.", "Desligado por padrão. Só o que foi dito, nunca comandos nem arquivos. Vale na hora.")}</small></span>
  </label>
  <label class="share-switch">
    <input type="checkbox" checked={allowPrompts} disabled={!desktop || busy} onchange={(event) => void setAllowPrompts(event.currentTarget.checked)} />
    <i class="track" aria-hidden="true"></i>
    <span><strong>{tr("Let paired devices send prompts to this computer's agents", "Permitir que dispositivos pareados enviem prompts aos agentes deste computador")}</strong><small>{tr("Off by default. Only sessions Lume controls, and only while Lume is open. Takes effect at once.", "Desligado por padrão. Só sessões que o Lume controla, e só com o Lume aberto. Vale na hora.")}</small></span>
  </label>
  {#if requesting}
    <div class="request-panel" role="status">
      <small>{tr(`Approve on ${requesting.node.nodeId} after checking that it shows this code:`, `Aprove em ${requesting.node.nodeId} depois de conferir que ele mostra este código:`)}</small>
      <strong class="code">{spacedCode(requesting.handle.code)}</strong>
      <span class="request-actions"><small>{tr("Waiting for approval…", "Aguardando aprovação…")}</small><button type="button" onclick={stopRequest}>{tr("Cancel", "Cancelar")}</button></span>
    </div>
  {/if}
  {#if incoming.length}
    <div class="request-panel attention" role="alert">
      {#each incoming as request (request.requestId)}
        <small>{tr(`${request.displayName} wants to see this computer's agents. Approve only if the other screen shows this code:`, `${request.displayName} quer ver os agentes deste computador. Aprove só se a outra tela mostrar este código:`)}</small>
        <strong class="code">{spacedCode(request.code)}</strong>
        <span class="request-actions">
          <button disabled={busy} type="button" onclick={() => void answer(request, true)}>{tr("Approve", "Aprovar")}</button>
          <button disabled={busy} type="button" onclick={() => void answer(request, false)}>{tr("Refuse", "Recusar")}</button>
        </span>
      {/each}
    </div>
  {/if}
  {#if !desktop}
    <p class="empty">{tr("Open Lume Desktop to connect a computer.", "Abra o Lume Desktop para conectar um computador.")}</p>
  {/if}
  {#if discovered.length}
    <div class="discovered" aria-label={tr("Discovered computers", "Computadores encontrados")}>
      {#each discovered as node (`${node.nodeId}:${node.address}:${node.port}`)}
        {@const alreadyPaired = nodes.some((item) => item.nodeId === node.nodeId) || relayNodes.some((item) => item.nodeId === node.nodeId)}
        <span>
          <strong>{node.nodeId}</strong><small>{node.address}:{node.port}</small>
          {#if alreadyPaired}<small>{tr("Paired", "Pareado")}</small>
          {:else}<button disabled={busy || !!requesting} type="button" onclick={() => void askToPair(node)}>{tr("Pair", "Parear")}</button>{/if}
        </span>
      {/each}
    </div>
  {/if}
  <div class="computers">
    {#each nodes as node (node.nodeId)}
      {@const nodeHealth = health[node.nodeId]}
      {@const snapshot = inventory[node.nodeId]}
      <article>
        <div class="computer-row">
          <span class="computer-name">
            <strong>{nodeHealth?.displayName ?? node.nodeId}</strong>
            <small>{nodeHealth ? `${nodeHealth.machine.operatingSystem} · ${nodeHealth.machine.architecture}` : `${node.address}:${node.port}`}</small>
            {#if checkedAt[node.nodeId]}
              <small class:error={!nodeHealth}>{tr("Last check", "Última consulta")} {snapshotTime(checkedAt[node.nodeId])} · {nodeHealth ? tr("Responded", "Respondeu") : tr("Unavailable", "Indisponível")}</small>
            {/if}
          </span>
          <div class="actions">
            <button disabled={busy} type="button" onclick={() => void check(node.nodeId)}>{tr("Check", "Verificar")}</button>
            <button disabled={busy} type="button" title={tr("Remove the record on this client. Revoke its key on the remote computer to remove authorization.", "Remove o registro neste cliente. Revogue a chave no computador remoto para retirar a autorização.")} onclick={() => void forget(node.nodeId)}>{tr("Forget", "Esquecer")}</button>
          </div>
        </div>
        {#if snapshot}
          <details class="inventory" open>
            <summary>{snapshot.agents.length}{snapshot.agentsTruncated ? "+" : ""} {tr("agents", "agentes")} · {snapshot.runtimes.reduce((total, runtime) => total + runtime.models.length, 0)} {tr("models", "modelos")}</summary>
            <small class="snapshot-label">{tr("Read-only snapshot", "Consulta somente leitura")} · {snapshotTime(snapshot.observedAt)}</small>
            <div class="inventory-list" class:scrolling onscroll={showScrollbar}>
              {#each snapshot.agents as agent (agent.id)}
                <div class="inventory-row"><BrandIcon name={agent.agent} size={13} /><strong>{agent.agentLabel}</strong><small>{tr("Observed", "Observado")}</small></div>
              {/each}
              {#if !snapshot.agents.length}<p class="empty">{tr("No running agent detected.", "Nenhum agente em execução detectado.")}</p>{/if}
              {#each snapshot.runtimes as runtime (runtime.id)}
                <div class="runtime-label"><strong>{runtime.id === "ollama" ? "Ollama" : runtime.id}</strong><small>{runtime.availability === "available" ? tr("On this computer", "Neste computador") : tr("Runtime unavailable", "Runtime indisponível")}</small></div>
                {#each runtime.models as model (model.name)}
                  <div class="inventory-row model-row"><span><strong>{model.name}</strong><small>{[model.parameterSize, model.quantization].filter(Boolean).join(" · ")}</small></span><small>{model.loaded === true ? tr("Loaded", "Carregado") : model.loaded === false ? tr("Installed", "Instalado") : tr("Load unknown", "Carga desconhecida")}</small></div>
                {/each}
                {#if runtime.availability === "available" && !runtime.models.length}<p class="empty">{tr("No installed model reported.", "Nenhum modelo instalado informado.")}</p>{/if}
                {#if runtime.modelsTruncated}<p class="empty">{tr("Partial model list, up to 128 entries.", "Lista parcial de modelos, com até 128 entradas.")}</p>{/if}
              {/each}
            </div>
          </details>
        {/if}
        {#if !bannerReporter && inventoryErrors[node.nodeId]}<p class="error inventory-error">{tr("Inventory unavailable: ", "Inventário indisponível: ")}{inventoryErrors[node.nodeId]}</p>{/if}
      </article>
    {/each}
    {#each relayNodes as node (node.nodeId)}
      {@const overview = overviews[node.nodeId]}
      {@const snapshot = overview?.sessions}
      <article>
        <div class="computer-row">
          <span class="computer-name">
            <strong>{overview?.health.displayName ?? node.nodeId}</strong>
            <small>{overview ? `${overview.health.machine.operatingSystem} · ${overview.health.machine.architecture} · Relay` : `Relay · ${new URL(node.relayUrl).host}`}</small>
            {#if relayCheckedAt[node.nodeId]}
              <small class:error={!overview}>{overview ? tr("Online · updates every few seconds", "Online · atualiza a cada poucos segundos") : tr("Unavailable", "Indisponível")}</small>
            {/if}
          </span>
          <div class="actions">
            <button disabled={busy} type="button" title={tr("Remove the record on this client. Revoke its key on the remote computer to remove authorization.", "Remove o registro neste cliente. Revogue a chave no computador remoto para retirar a autorização.")} onclick={() => void forgetRelay(node.nodeId)}>{tr("Forget", "Esquecer")}</button>
          </div>
        </div>
        {#if openChat?.nodeId === node.nodeId}
          <div class="conversation">
            <div class="conversation-head">
              <button type="button" onclick={closeConversation}>← {tr("Back", "Voltar")}</button>
              <strong>{openChat.title}</strong>
            </div>
            <div class="conversation-body" class:scrolling bind:this={chatScroller} onscroll={showScrollbar}>
              {#if conversation?.truncated}<p class="empty">{tr("Older messages are not shown.", "Mensagens mais antigas não aparecem.")}</p>{/if}
              {#each conversation?.messages ?? [] as item (item.id)}
                <div class={`bubble ${item.role}`}><small>{item.role === "user" ? tr("You asked", "Pedido") : conversation?.session.agentLabel}</small><p>{item.text}</p></div>
              {/each}
              {#if conversation && !conversation.messages.length}<p class="empty">{tr("Nothing was said in this session yet.", "Nada foi dito nesta sessão ainda.")}</p>{/if}
              {#if !conversation && !conversationError}<p class="empty">{tr("Loading…", "Carregando…")}</p>{/if}
              {#if conversationError}<p class="error inventory-error">{conversationError}</p>{/if}
            </div>
            <form class="prompt-form" onsubmit={(event) => { event.preventDefault(); void sendPrompt(); }}>
              <textarea rows="2" maxlength="32000" disabled={sendingPrompt} bind:value={promptText} placeholder={tr("Send a prompt to this agent", "Enviar um prompt a este agente")} onkeydown={(event) => { if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) { event.preventDefault(); void sendPrompt(); } }}></textarea>
              <button type="submit" disabled={!desktop || sendingPrompt || !promptText.trim()}>{sendingPrompt ? tr("Sending…", "Enviando…") : tr("Send", "Enviar")}</button>
            </form>
            {#if promptNote}<p class={`prompt-note ${promptFailed ? "error" : ""}`} role="status">{promptNote}</p>{/if}
            <small class="snapshot-label">{tr("Updates every few seconds", "Atualiza a cada poucos segundos")}</small>
          </div>
        {:else if snapshot}
          <details class="inventory" open>
            <summary>{snapshot.sessions.length} {tr("agent sessions", "sessões de agentes")}</summary>
            <small class="snapshot-label">{snapshot.live === true || !snapshot.appFound ? tr("Live view", "Visão ao vivo") : tr("Last saved state, not live (Lume closed there, or an older version)", "Último estado salvo, não ao vivo (Lume fechado lá, ou versão antiga)")} · {snapshotTime(snapshot.generatedAt)}</small>
            <div class="inventory-list" class:scrolling onscroll={showScrollbar}>
              {#each snapshot.sessions as session (session.id)}
                <button type="button" class="inventory-row session-row session-button" onclick={() => openConversation(node.nodeId, session)} title={tr("Read this conversation", "Ler esta conversa")}>
                  <span class={`session-dot ${sessionDot(session.status)}`} aria-hidden="true"></span>
                  <BrandIcon name={session.agent} size={13} />
                  <span class="session-title"><strong>{sessionTitle(session)}</strong><small>{session.agentLabel}{session.name && session.project ? ` · ${session.project}` : ""}</small></span>
                  <small>{sessionState(session)} · {agoLabel(session.updatedAt)}</small>
                </button>
              {/each}
              {#if !snapshot.appFound}<p class="empty">{tr("The Lume app has not been opened on that computer yet.", "O app do Lume ainda não foi aberto naquele computador.")}</p>
              {:else if !snapshot.sessions.length}<p class="empty">{tr("No agent sessions there.", "Nenhuma sessão de agente por lá.")}</p>{/if}
            </div>
          </details>
        {:else if overview?.sessionsError}
          <p class="error inventory-error">{tr("Sessions unavailable: ", "Sessões indisponíveis: ")}{overview.sessionsError}</p>
        {/if}
        {#if !bannerReporter && relayErrors[node.nodeId]}<p class="error inventory-error">{relayErrors[node.nodeId]}</p>{/if}
      </article>
    {/each}
    {#if !nodes.length && !relayNodes.length}
      <p class="empty">{tr("No paired computers yet.", "Nenhum computador pareado ainda.")}</p>
    {/if}
  </div>
  {#if !bannerReporter && message}<p class="message" class:error={failed} role="status">{message}</p>{/if}
</div>

<style>
  .remote-computers { --ink: var(--lume-ink-light, #35423d); --muted: var(--lume-ink-muted-light, #89938f); --line: var(--lume-line-light, rgba(92, 111, 103, 0.14)); --surface: var(--lume-raised-light, rgba(255, 255, 255, 0.48)); padding: 11px; display: grid; gap: 9px; border: 1px solid var(--line); border-radius: 13px; color: var(--ink); background: color-mix(in srgb, var(--surface) 35%, transparent); font-size: 10px; }
  .dark { --ink: var(--lume-ink-dark, #dce6e0); --muted: var(--lume-ink-muted-dark, #8e9b94); --line: var(--lume-line-dark, rgba(190, 209, 200, 0.14)); --surface: var(--lume-raised-dark, #25312c); }
  strong { font-size: inherit; font-weight: 650; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  small { color: var(--muted); font-size: 9px; line-height: 1.5; }
  .heading, .computer-row, .actions, .pair-control, .inventory-row, .runtime-label { display: flex; align-items: center; gap: 7px; min-width: 0; }
  .heading > span, .computer-name, .model-row > span { display: grid; gap: 2px; min-width: 0; flex: 1; }
  .heading small { white-space: normal; }
  button { min-height: 27px; padding: 0 7px; flex-shrink: 0; border: 1px solid var(--line); border-radius: 7px; color: var(--ink); background: transparent; font: inherit; font-size: 9px; cursor: pointer; }
  button:hover { background: var(--surface); }
  button:disabled, input:disabled { opacity: 0.5; cursor: default; }
  button:focus-visible, input:focus-visible, summary:focus-visible { outline: 2px solid var(--lume-accent-strong, #4d9d76); outline-offset: 2px; }
  input { width: 0; min-width: 0; height: 29px; padding: 0 8px; flex: 1; border: 1px solid var(--line); border-radius: 8px; color: var(--ink); background: var(--surface); font: inherit; }
  .discovered { display: grid; gap: 5px; padding-top: 7px; border-top: 1px solid var(--line); }
  .discovered > span { display: flex; align-items: center; gap: 5px; min-width: 0; }
  .discovered strong { min-width: 0; flex: 1; }
  .computers article { padding: 8px 0; border-top: 1px solid var(--line); }
  .computer-name small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .actions { flex-wrap: wrap; justify-content: flex-end; }
  .inventory { margin-top: 9px; }
  summary { cursor: pointer; font-weight: 650; font-size: 10px; }
  .snapshot-label { display: block; margin: 4px 0; }
  .inventory-list { max-height: 200px; overflow: auto; scrollbar-width: thin; scrollbar-color: transparent transparent; }
  .inventory-list.scrolling { scrollbar-color: var(--line) transparent; }
  .inventory-list::-webkit-scrollbar { width: 4px; height: 4px; }
  .inventory-list::-webkit-scrollbar-track { background: transparent; }
  .inventory-list::-webkit-scrollbar-thumb { border-radius: 4px; background: transparent; }
  .inventory-list.scrolling::-webkit-scrollbar-thumb { background: var(--line); }
  .inventory-row { min-height: 28px; border-bottom: 1px solid var(--line); }
  .inventory-row > strong { min-width: 0; flex: 1; }
  .inventory-row > small, .runtime-label > small { flex-shrink: 0; }
  .runtime-label { padding-top: 10px; padding-bottom: 5px; justify-content: space-between; }
  .request-panel { display: grid; gap: 6px; padding: 9px; border: 1px solid var(--line); border-radius: 9px; background: var(--surface); }
  .request-panel.attention { border-color: #d3a24b; }
  .request-panel .code { font-size: 20px; letter-spacing: 3px; font-variant-numeric: tabular-nums; }
  .request-actions { display: flex; align-items: center; justify-content: space-between; gap: 7px; }
  .session-row { gap: 6px; }
  .session-button { width: 100%; padding: 0; border: 0; border-bottom: 1px solid var(--line); border-radius: 0; text-align: left; }
  .session-button:hover { background: var(--surface); }
  .share-switch { position: relative; display: flex; align-items: flex-start; gap: 8px; cursor: pointer; }
  .share-switch input { position: absolute; width: 1px; height: 1px; opacity: 0; pointer-events: none; }
  .track { position: relative; width: 26px; height: 15px; flex-shrink: 0; margin-top: 1px; border-radius: 8px; background: var(--line); transition: background 160ms ease; }
  .track::after { position: absolute; top: 2px; left: 2px; width: 11px; height: 11px; border-radius: 50%; content: ""; background: var(--ink); transition: transform 160ms ease; }
  .share-switch input:checked + .track { background: var(--lume-accent-strong, #4d9d76); }
  .share-switch input:checked + .track::after { transform: translateX(11px); }
  .share-switch input:focus-visible + .track { outline: 2px solid var(--lume-accent-strong, #4d9d76); outline-offset: 2px; }
  .share-switch input:disabled + .track { opacity: 0.5; }
  .share-switch > span { display: grid; gap: 2px; min-width: 0; }
  .share-switch strong { white-space: normal; }
  .prompt-form { display: flex; align-items: flex-end; gap: 6px; }
  .prompt-form textarea { flex: 1; min-width: 0; resize: vertical; max-height: 120px; padding: 7px 9px; border: 1px solid var(--line); border-radius: 9px; color: inherit; background: transparent; font: inherit; }
  .prompt-form button { flex: 0 0 auto; }
  .prompt-note { margin: 0; font-size: 11px; color: var(--muted); }
  .prompt-note.error { color: #c0645e; }
  .conversation { margin-top: 9px; display: grid; gap: 6px; }
  .conversation-head { display: flex; align-items: center; gap: 7px; min-width: 0; }
  .conversation-body { display: grid; align-content: start; gap: 6px; max-height: 280px; overflow: auto; scrollbar-width: thin; scrollbar-color: transparent transparent; }
  .conversation-body.scrolling { scrollbar-color: var(--line) transparent; }
  .bubble { display: grid; gap: 2px; padding: 7px 9px; border: 1px solid var(--line); border-radius: 9px; }
  .bubble.user { background: var(--surface); }
  .bubble p { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; font-size: 10px; line-height: 1.5; }
  .session-title { display: grid; min-width: 0; flex: 1; }
  .session-title small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .session-dot { width: 7px; height: 7px; flex-shrink: 0; border-radius: 50%; background: var(--muted); }
  .session-dot.running { background: var(--lume-accent-strong, #4d9d76); }
  .session-dot.attention { background: #d3a24b; }
  .session-dot.failed { background: var(--lume-error, #bb7168); }
  .empty, .message, .inventory-error { margin: 0; font-size: 9px; line-height: 1.5; color: var(--muted); overflow-wrap: anywhere; }
  .inventory-error { margin-top: 7px; }
  .error { color: var(--lume-error, #bb7168); }
</style>
