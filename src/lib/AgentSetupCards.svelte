<script lang="ts">
  import BrandIcon from "$lib/BrandIcon.svelte";
  import type { AgentKind, IntegrationDiagnostic, IntegrationStatus } from "$lib/domain";
  import type { Language } from "$lib/i18n";
  import type { CliInstallPlan } from "$lib/lume";

  type Kind = IntegrationStatus["kind"] | "github";
  type Step = "missing" | "installed" | "connected" | "ready";

  let {
    language,
    integrations,
    plans,
    diagnostics = {},
    configuring = null,
    diagnosing = null,
    compact = false,
    only,
    onInstall,
    onConnect,
    onDisconnect,
    onTest,
  }: {
    language: Language;
    integrations: IntegrationStatus[];
    plans: CliInstallPlan[];
    diagnostics?: Partial<Record<IntegrationStatus["kind"], IntegrationDiagnostic>>;
    configuring?: IntegrationStatus["kind"] | null;
    diagnosing?: IntegrationStatus["kind"] | null;
    /** Tighter rows for the first-run guide. */
    compact?: boolean;
    /** Shows just the agents, or just the tools (GitHub CLI). */
    only?: "agents" | "tools";
    onInstall: (plan: CliInstallPlan) => void;
    onConnect: (integration: IntegrationStatus) => void;
    onDisconnect: (integration: IntegrationStatus) => void;
    onTest: (integration: IntegrationStatus) => void;
  } = $props();

  const tr = (english: string, portuguese: string) => (language === "pt-BR" ? portuguese : english);

  type Entry = {
    kind: Kind;
    label: string;
    detail: string;
    integration?: IntegrationStatus;
    plan?: CliInstallPlan;
    step: Step;
  };

  function stepOf(integration: IntegrationStatus | undefined, plan: CliInstallPlan | undefined): Step {
    const installed = integration ? integration.installed : Boolean(plan?.installed);
    if (!installed) return "missing";
    if (!integration || !integration.canConfigure) return "ready";
    return integration.configured ? "connected" : "installed";
  }

  const entries = $derived.by(() => {
    const planFor = (kind: Kind) => plans.find((plan) => plan.kind === kind);
    const agents = integrations.map((integration): Entry => ({
      kind: integration.kind,
      label: integration.label,
      detail: integration.kind === "omp"
        ? tr(
            "Optional extension for live status; Lume observes approvals but cannot approve external omp tool calls. Passive monitoring works without it.",
            "Extensão opcional para status ao vivo; o Lume observa aprovações, mas não pode aprovar chamadas externas de ferramentas do omp. O monitoramento passivo funciona sem ela.",
          )
        : integration.detail,
      integration,
      plan: planFor(integration.kind),
      step: stepOf(integration, planFor(integration.kind)),
    }));
    const github = planFor("github");
    const tools: Entry[] = github
      ? [{
          kind: "github",
          label: github.label,
          detail: tr("Powers pull request and issue previews, and commits from Lume.", "Habilita as prévias de PRs e issues e os commits pelo Lume."),
          plan: github,
          step: stepOf(undefined, github),
        }]
      : [];
    return [
      { title: "", tools: false, items: agents.filter((entry) => entry.integration?.canLaunch) },
      { title: tr("Monitoring only", "Somente monitoramento"), tools: false, items: agents.filter((entry) => !entry.integration?.canLaunch) },
      { title: only === "tools" ? "" : tr("Tools", "Ferramentas"), tools: true, items: tools },
    ].filter((group) => group.items.length && (only === undefined || (only === "tools") === group.tools));
  });

  const stepLabel = (step: Step) => ({
    missing: tr("Not installed", "Não instalado"),
    installed: tr("Installed", "Instalado"),
    connected: tr("Connected", "Conectado"),
    ready: tr("Installed", "Instalado"),
  })[step];

  const brand = (kind: Kind): AgentKind | "github" => (kind === "claude" ? "claude_code" : kind);

  let copied = $state<Kind | null>(null);
  async function copyLogin(entry: Entry) {
    if (!entry.plan) return;
    try {
      await navigator.clipboard.writeText(entry.plan.loginCommand);
      copied = entry.kind;
    } catch {
      copied = null;
    }
  }

  const canInstall = (entry: Entry) => Boolean(entry.plan && (entry.plan.methods.length || entry.plan.manualUrl));
</script>

<div class:compact class="agent-setup">
  {#each entries as group (group.title)}
    {#if group.title}<small class="group-title">{group.title}</small>{/if}
    {#each group.items as entry (entry.kind)}
      {@const busy = entry.integration ? configuring === entry.integration.kind || diagnosing === entry.integration.kind : false}
      <article class="agent-card step-{entry.step}">
        <span class="agent-mark"><BrandIcon name={brand(entry.kind)} size={compact ? 18 : 22} /></span>
        <span class="agent-copy">
          <span class="agent-heading"><strong>{entry.label}</strong><b class="badge {entry.step}"><i aria-hidden="true"></i>{stepLabel(entry.step)}</b></span>
          <small>{entry.detail}</small>
        </span>
        <span class="agent-actions">
          {#if entry.step === "missing"}
            {#if canInstall(entry)}
              <button class="primary" type="button" onclick={() => onInstall(entry.plan!)}>{tr("Install", "Instalar")}</button>
            {:else}
              <span class="unavailable">{tr("Install it yourself to use it", "Instale por conta própria para usar")}</span>
            {/if}
          {:else if entry.step === "installed" && entry.integration}
            <button class="quiet" type="button" disabled={busy || diagnosing !== null} onclick={() => onTest(entry.integration!)}>{diagnosing === entry.integration.kind ? "…" : tr("Test", "Testar")}</button>
            <button class="primary" type="button" disabled={busy || configuring !== null} onclick={() => onConnect(entry.integration!)}>{configuring === entry.integration.kind ? "…" : tr("Connect", "Conectar")}</button>
          {:else if entry.step === "connected" && entry.integration}
            <button class="quiet" type="button" disabled={busy || configuring !== null} onclick={() => onDisconnect(entry.integration!)}>{tr("Disconnect", "Desconectar")}</button>
            <button class="primary" type="button" disabled={busy || diagnosing !== null} onclick={() => onTest(entry.integration!)}>{diagnosing === entry.integration.kind ? "…" : tr("Test", "Testar")}</button>
          {:else if entry.integration}
            <button class="primary" type="button" disabled={diagnosing !== null} onclick={() => onTest(entry.integration!)}>{diagnosing === entry.integration.kind ? "…" : tr("Test", "Testar")}</button>
          {:else if entry.plan}
            <button class="quiet" type="button" onclick={() => void copyLogin(entry)} title={entry.plan.loginCommand}>{copied === entry.kind ? tr("Copied", "Copiado") : tr("Copy sign-in command", "Copiar comando de login")}</button>
          {/if}
        </span>
        {#if entry.step === "installed" && !compact && entry.plan}
          <small class="hint">{entry.kind === "omp"
            ? tr("Connect to install Lume's optional observe-only extension. It cannot approve external omp tool calls; passive monitoring works without it.", "Conecte para instalar a extensão opcional do Lume, somente observável. Ela não pode aprovar chamadas externas de ferramentas do omp; o monitoramento passivo funciona sem ela.")
            : tr("Connect it so Lume can follow its sessions and answer permission requests.", "Conecte para o Lume acompanhar as sessões e responder pedidos de permissão.")}</small>
        {/if}
        {#if entry.integration && diagnostics[entry.integration.kind]}
          <div class="diagnostic-list">
            {#each diagnostics[entry.integration.kind]?.checks ?? [] as item (item.id)}
              <span class="diagnostic-{item.status}"><i></i><b>{item.label}</b><small>{item.detail}</small></span>
            {/each}
          </div>
        {/if}
      </article>
    {/each}
  {/each}
</div>

<style>
  .agent-setup { display: grid; gap: .9em; font-size: var(--agent-setup-font, 12px); }
  .group-title { margin: .5em 2px 0; opacity: .62; font-size: .8em; font-weight: 700; letter-spacing: .04em; text-transform: uppercase; }
  .agent-card { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: .35em 1em; padding: 1em 1.1em; border: 1px solid color-mix(in srgb, currentColor 13%, transparent); border-radius: 1.1em; background: color-mix(in srgb, currentColor 4%, transparent); }
  .agent-card.step-connected { border-color: color-mix(in srgb, var(--workspace-accent, #3f9b69) 38%, transparent); }
  .compact .agent-card { padding: .7em .9em; border-radius: .9em; }
  .agent-mark { width: 3em; height: 3em; display: grid; place-items: center; border-radius: .9em; background: color-mix(in srgb, currentColor 7%, transparent); }
  .compact .agent-mark { width: 2.5em; height: 2.5em; }
  .agent-copy { min-width: 0; display: grid; gap: 2px; }
  .agent-heading { min-width: 0; display: flex; flex-wrap: wrap; align-items: center; gap: 4px 8px; }
  .agent-heading strong { font-size: 1.08em; }
  .agent-copy small { opacity: .68; font-size: .9em; line-height: 1.4; overflow-wrap: anywhere; }
  .badge { display: inline-flex; align-items: center; gap: 5px; padding: .1em .65em .1em .55em; border-radius: 999px; background: color-mix(in srgb, currentColor 8%, transparent); font-size: .8em; font-weight: 700; }
  .badge i { width: .55em; height: .55em; border-radius: 50%; background: currentColor; opacity: .45; }
  .badge.connected { color: var(--workspace-accent, #3f9b69); background: color-mix(in srgb, var(--workspace-accent, #3f9b69) 14%, transparent); }
  .badge.connected i { background: var(--workspace-accent, #3f9b69); opacity: 1; }
  .badge.installed, .badge.ready { color: #4d8cb8; background: color-mix(in srgb, #4d8cb8 13%, transparent); }
  .badge.installed i, .badge.ready i { background: #4d8cb8; opacity: 1; }
  .agent-actions { display: flex; align-items: center; justify-content: flex-end; gap: 6px; }
  .agent-actions button { min-height: 2.5em; padding: 0 1.1em; border: 1px solid color-mix(in srgb, currentColor 18%, transparent); border-radius: .8em; color: inherit; background: transparent; font: inherit; font-size: .95em; font-weight: 650; cursor: pointer; }
  .agent-actions button:hover:not(:disabled) { background: color-mix(in srgb, currentColor 8%, transparent); }
  .agent-actions button.primary { border-color: transparent; color: #f5fbf7; background: var(--workspace-accent, #3f9b69); }
  .agent-actions button.primary:hover:not(:disabled) { filter: brightness(1.08); background: var(--workspace-accent, #3f9b69); }
  .agent-actions button:disabled { opacity: .5; cursor: default; }
  .unavailable { max-width: 13em; opacity: .6; font-size: .85em; line-height: 1.35; text-align: right; }
  .hint { grid-column: 2 / -1; opacity: .62; font-size: .85em; line-height: 1.4; }
  .diagnostic-list { grid-column: 1 / -1; display: grid; gap: .3em; margin-top: .4em; padding-top: .6em; border-top: 1px solid color-mix(in srgb, currentColor 10%, transparent); font-size: .9em; }
  .diagnostic-list span { display: flex; align-items: baseline; gap: 6px; flex-wrap: wrap; }
  .diagnostic-list i { width: 6px; height: 6px; flex: 0 0 auto; border-radius: 50%; background: #aab6b0; }
  .diagnostic-list .diagnostic-ok i { background: #3f9b69; }
  .diagnostic-list .diagnostic-warning i { background: #d0a142; }
  .diagnostic-list .diagnostic-error i, .diagnostic-list .diagnostic-failed i { background: #c0554f; }
  .diagnostic-list small { opacity: .68; }
  @media (max-width: 420px) { .agent-card { grid-template-columns: auto minmax(0, 1fr); } .agent-actions { grid-column: 1 / -1; justify-content: flex-start; } }
</style>
