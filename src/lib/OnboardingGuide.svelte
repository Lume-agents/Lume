<script lang="ts">
  import AgentSetupCards from "$lib/AgentSetupCards.svelte";
  import BrandIcon from "$lib/BrandIcon.svelte";
  import type { IntegrationDiagnostic, IntegrationStatus } from "$lib/domain";
  import type { Language } from "$lib/i18n";
  import type { CliInstallPlan } from "$lib/lume";

  let {
    language,
    integrations,
    plans,
    diagnostics = {},
    configuring = null,
    diagnosing = null,
    onInstall,
    onConnect,
    onDisconnect,
    onTest,
    onFinish,
  }: {
    language: Language;
    integrations: IntegrationStatus[];
    plans: CliInstallPlan[];
    diagnostics?: Partial<Record<IntegrationStatus["kind"], IntegrationDiagnostic>>;
    configuring?: IntegrationStatus["kind"] | null;
    diagnosing?: IntegrationStatus["kind"] | null;
    onInstall: (plan: CliInstallPlan) => void;
    onConnect: (integration: IntegrationStatus) => void;
    onDisconnect: (integration: IntegrationStatus) => void;
    onTest: (integration: IntegrationStatus) => void;
    /** The guide ends here, finished or skipped. */
    onFinish: () => void | Promise<void>;
  } = $props();

  const tr = (english: string, portuguese: string) => (language === "pt-BR" ? portuguese : english);
  const steps = ["welcome", "agents", "github", "ready"] as const;
  let index = $state(0);
  let finishing = $state(false);
  const step = $derived(steps[index]);
  const connected = $derived(integrations.filter((integration) => integration.configured));
  const installedCount = $derived(integrations.filter((integration) => integration.installed).length);

  async function finish() {
    if (finishing) return;
    finishing = true;
    try { await onFinish(); }
    finally { finishing = false; }
  }
</script>

<div class="onboarding" role="dialog" aria-modal="true" aria-labelledby="onboarding-title">
  <header>
    <span class="mark"><BrandIcon name="lume" size={25} /></span>
    <span class="heading">
      <strong id="onboarding-title">
        {#if step === "welcome"}{tr("Welcome to Lume", "Bem-vindo ao Lume")}
        {:else if step === "agents"}{tr("Set up your agents", "Configure seus agentes")}
        {:else if step === "github"}{tr("GitHub CLI", "GitHub CLI")}
        {:else}{tr("You're all set", "Tudo pronto")}{/if}
      </strong>
      <small>{tr(`Step ${index + 1} of ${steps.length}`, `Passo ${index + 1} de ${steps.length}`)}</small>
    </span>
    <span class="dots" aria-hidden="true">{#each steps as _, position}<i class:done={position <= index}></i>{/each}</span>
  </header>

  <div class="body">
    {#if step === "welcome"}
      <p class="lead">{tr("Lume keeps an eye on every AI coding agent on your computer, so you always know who is working, who is waiting and who needs you.", "O Lume acompanha todos os agentes de IA do seu computador, para você saber quem está trabalhando, quem está esperando e quem precisa de você.")}</p>
      <ul class="points">
        <li><b>{tr("See every session", "Veja todas as sessões")}</b><span>{tr("Claude Code, Codex, Gemini, OpenCode and more, in one place.", "Claude Code, Codex, Gemini, OpenCode e mais, em um só lugar.")}</span></li>
        <li><b>{tr("Answer without switching windows", "Responda sem trocar de janela")}</b><span>{tr("Approve permissions, reply to questions and send prompts from Lume.", "Aprove permissões, responda perguntas e envie prompts pelo Lume.")}</span></li>
        <li><b>{tr("Everything stays on your computer", "Tudo fica no seu computador")}</b><span>{tr("No account, and nothing is sent to the cloud.", "Sem conta, e nada é enviado para a nuvem.")}</span></li>
      </ul>
    {:else if step === "agents"}
      <p class="lead">{tr("Install the CLIs you want to use, then connect them so Lume can follow their sessions. You can do this later in Settings.", "Instale as CLIs que quer usar e conecte-as para o Lume acompanhar as sessões. Você pode fazer isso depois nas configurações.")}</p>
      <AgentSetupCards {language} {integrations} {plans} {diagnostics} {configuring} {diagnosing} only="agents" compact {onInstall} {onConnect} {onDisconnect} {onTest} />
    {:else if step === "github"}
      <p class="lead">{tr("Optional. With the GitHub CLI, Lume shows previews of pull requests and issues in the chat and can commit your changes.", "Opcional. Com o GitHub CLI, o Lume mostra prévias de pull requests e issues no chat e consegue commitar suas mudanças.")}</p>
      <AgentSetupCards {language} {integrations} {plans} {diagnostics} only="tools" compact {onInstall} {onConnect} {onDisconnect} {onTest} />
    {:else}
      <p class="lead">
        {#if connected.length}
          {tr(`${connected.length} agent${connected.length === 1 ? "" : "s"} connected.`, `${connected.length} agente${connected.length === 1 ? "" : "s"} conectado${connected.length === 1 ? "" : "s"}.`)}
        {:else if installedCount}
          {tr("You have agents installed but none connected yet. Connect them in Settings → Agents.", "Você tem agentes instalados, mas nenhum conectado ainda. Conecte em Configurações → Agentes.")}
        {:else}
          {tr("No agent yet. Install one in Settings → Agents whenever you like.", "Nenhum agente ainda. Instale um em Configurações → Agentes quando quiser.")}
        {/if}
      </p>
      <ul class="points tips">
        <li><b>{tr("Open the panel", "Abra o painel")}</b><span>{tr("Click the capsule at the top of the screen. You can drag it anywhere.", "Clique na cápsula no topo da tela. Você pode arrastá-la para qualquer lugar.")}</span></li>
        <li><b>{tr("Chat with your agents", "Converse com seus agentes")}</b><span>{tr("Open the Workspace to run several agents side by side. Type @ to reference files and / for commands.", "Abra o Workspace para rodar vários agentes lado a lado. Digite @ para citar arquivos e / para comandos.")}</span></li>
        <li><b>{tr("Come back anytime", "Volte quando quiser")}</b><span>{tr("Settings → Agents lets you install, connect or test any agent, and reopen this guide.", "Configurações → Agentes permite instalar, conectar ou testar qualquer agente e reabrir este guia.")}</span></li>
      </ul>
    {/if}
  </div>

  <footer>
    {#if step === "ready"}
      <button class="quiet" type="button" onclick={() => { index = index - 1; }}>{tr("Back", "Voltar")}</button>
      <button class="primary" type="button" disabled={finishing} onclick={() => void finish()}>{tr("Start using Lume", "Começar a usar o Lume")}</button>
    {:else}
      {#if index > 0}
        <button class="quiet" type="button" onclick={() => { index = index - 1; }}>{tr("Back", "Voltar")}</button>
      {:else}
        <button class="quiet" type="button" disabled={finishing} onclick={() => void finish()}>{tr("Skip guide", "Pular guia")}</button>
      {/if}
      <button class="primary" type="button" onclick={() => { index = index + 1; }}>{index === 0 ? tr("Get started", "Começar") : step === "github" ? tr("Continue", "Continuar") : tr("Next", "Próximo")}</button>
    {/if}
  </footer>
</div>

<style>
  .onboarding { --workspace-accent: var(--lume-accent-strong, #3f9b69); --agent-setup-font: 10px; position: absolute; z-index: 45; inset: 0; padding: 16px 16px 12px; display: flex; flex-direction: column; gap: 12px; overflow: hidden; border-radius: inherit; color: #27342e; background: #f7faf7; }
  header { display: flex; align-items: center; gap: 10px; }
  .mark { width: 36px; height: 36px; display: grid; place-items: center; flex: 0 0 auto; color: var(--lume-accent-strong); }
  .heading { min-width: 0; flex: 1; display: grid; gap: 2px; }
  .heading strong { font-size: 15px; font-weight: 780; letter-spacing: -.025em; }
  .heading small { color: #6f7e76; font-size: 9px; }
  .dots { display: flex; gap: 5px; }
  .dots i { width: 6px; height: 6px; border-radius: 50%; background: rgba(60, 90, 75, .2); transition: background 200ms ease, transform 200ms ease; }
  .dots i.done { background: var(--lume-accent-strong); }
  .body { min-height: 0; flex: 1; display: flex; flex-direction: column; gap: 11px; overflow-y: auto; overscroll-behavior: contain; padding-right: 2px; }
  .lead { margin: 0; color: #4a5b52; font-size: 10.5px; line-height: 1.55; }
  .points { margin: 0; padding: 0; display: grid; gap: 8px; list-style: none; }
  .points li { padding: 10px 12px; display: grid; gap: 3px; border: 1px solid rgba(74, 104, 89, .14); border-radius: 12px; background: rgba(255, 255, 252, .7); }
  .points b { font-size: 11px; font-weight: 750; }
  .points span { color: #62736a; font-size: 9.5px; line-height: 1.5; }
  footer { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  footer button { min-height: 34px; padding: 0 16px; border: 1px solid rgba(74, 104, 89, .2); border-radius: 10px; color: inherit; background: transparent; font: inherit; font-size: 11px; font-weight: 700; cursor: pointer; }
  footer button.primary { margin-left: auto; border-color: transparent; color: #f5fbf7; background: var(--lume-accent-strong, #3f9b69); }
  footer button:disabled { opacity: .55; cursor: default; }
  footer button:focus-visible { outline: 2px solid color-mix(in srgb, var(--lume-accent-strong) 55%, transparent); outline-offset: 2px; }
  :global(.dark) .onboarding { color: #dce8e1; background: #111b16; }
  :global(.dark) .heading small, :global(.dark) .lead, :global(.dark) .points span { color: #98aaa0; }
  :global(.dark) .points li { border-color: rgba(201, 224, 211, .1); background: rgba(27, 41, 34, .8); }
  :global(.dark) .dots i { background: rgba(190, 215, 200, .22); }
  :global(.dark) .dots i.done { background: var(--lume-accent); }
  :global(.dark) footer button { border-color: rgba(201, 224, 211, .16); }
  :global(.dark) footer button.primary { color: #0e1612; background: var(--lume-accent); }
</style>
