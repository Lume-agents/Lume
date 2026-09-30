<script lang="ts">
  import { getContext, onMount, untrack } from "svelte";
  import { createSystemBannerSource, SYSTEM_BANNER_CONTEXT, type SystemBannerReporter, type SystemBannerNotice } from "$lib/systemBannerContext";
  import BrandIcon from "$lib/BrandIcon.svelte";
  import type { DiscoveredLumeNode, RemoteLumeNode, RemoteLumeNodeHealth, RemoteLumeNodeInventory } from "$lib/domain";
  import { localize, type Language } from "$lib/i18n";
  import {
    discoverLumeNodes,
    forgetRemoteLumeNode,
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
    void perform(async () => { nodes = await loadRemoteLumeNodes(); });
    return () => clearTimeout(scrollTimer);
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
      autocomplete="off" placeholder="lume://pair-node?…" spellcheck="false" type="password"
      disabled={!desktop || busy} bind:value={pairingUri}
      onkeydown={(event) => { if (event.key === "Enter") void pair(); }}
    />
    <button disabled={!desktop || busy || !pairingUri.trim()} type="button" onclick={() => void pair()}>{tr("Pair", "Parear")}</button>
  </div>
  {#if !desktop}
    <p class="empty">{tr("Open Lume Desktop to connect a computer.", "Abra o Lume Desktop para conectar um computador.")}</p>
  {/if}
  {#if discovered.length}
    <div class="discovered" aria-label={tr("Discovered computers", "Computadores encontrados")}>
      {#each discovered as node (`${node.nodeId}:${node.address}:${node.port}`)}
        <span><strong>{node.nodeId}</strong><small>{node.address}:{node.port}</small></span>
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
    {:else}
      <p class="empty">{tr("No paired computers yet.", "Nenhum computador pareado ainda.")}</p>
    {/each}
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
  .empty, .message, .inventory-error { margin: 0; font-size: 9px; line-height: 1.5; color: var(--muted); overflow-wrap: anywhere; }
  .inventory-error { margin-top: 7px; }
  .error { color: var(--lume-error, #bb7168); }
</style>
