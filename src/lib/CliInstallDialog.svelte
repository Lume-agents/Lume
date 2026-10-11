<script lang="ts">
  import { onMount, tick } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { dialogFocus } from "$lib/dialogFocus";
  import type { Language } from "$lib/i18n";
  import { cancelCliInstall, startCliInstall, type CliInstallFinished, type CliInstallPlan } from "$lib/lume";

  let { plan, language, onClose }: {
    plan: CliInstallPlan;
    language: Language;
    /** Called when the dialog closes; `installed` says whether the CLI can be found now. */
    onClose: (installed: boolean) => void;
  } = $props();

  const pt = $derived(language === "pt-BR");
  const tr = (english: string, portuguese: string) => (pt ? portuguese : english);

  let stage = $state<"confirm" | "running" | "done">("confirm");
  let chosen = $state<string | null>(null);
  const methodId = $derived(chosen ?? plan.methods.find((method) => method.available)?.id ?? "");
  const method = $derived(plan.methods.find((item) => item.id === methodId));
  const available = $derived(plan.methods.filter((item) => item.available));
  let runId = $state<string | null>(null);
  let lines = $state<string[]>([]);
  let finished = $state<CliInstallFinished | null>(null);
  let startError = $state("");
  let cancelling = $state(false);
  let copied = $state(false);
  let logElement = $state<HTMLElement | null>(null);

  onMount(() => {
    const unlisten: UnlistenFn[] = [];
    let disposed = false;
    void (async () => {
      // Only one install runs at a time, so everything heard while this one runs is its own.
      const output = await listen<{ id: string; line: string }>("lume://cli-install-output", (event) => {
        if (stage !== "running") return;
        lines = [...lines.slice(-399), event.payload.line];
        void tick().then(() => { if (logElement) logElement.scrollTop = logElement.scrollHeight; });
      });
      const end = await listen<CliInstallFinished>("lume://cli-install-finished", (event) => {
        if (stage !== "running") return;
        finished = event.payload;
        stage = "done";
      });
      if (disposed) { output(); end(); return; }
      unlisten.push(output, end);
    })();
    return () => { disposed = true; unlisten.forEach((off) => off()); };
  });

  async function install() {
    if (!method || stage === "running") return;
    stage = "running";
    lines = [];
    finished = null;
    startError = "";
    cancelling = false;
    try {
      runId = await startCliInstall(plan.kind, method.id);
    } catch (error) {
      startError = String(error).replace(/^Error:\s*/, "");
      stage = "done";
    }
  }

  async function cancel() {
    if (!runId || cancelling) return;
    cancelling = true;
    await cancelCliInstall(runId).catch(() => false);
  }

  async function copyLogin() {
    try {
      await navigator.clipboard.writeText(plan.loginCommand);
      copied = true;
    } catch {
      copied = false;
    }
  }

  function close() {
    if (stage === "running") return;
    onClose(Boolean(finished?.installed));
  }

  const succeeded = $derived(Boolean(finished?.success && finished.installed));
  const installedButNotFound = $derived(Boolean(finished?.success && !finished.installed));
</script>

<div class="cli-install-backdrop">
  <button class="backdrop-dismiss" type="button" aria-label={tr("Close", "Fechar")} disabled={stage === "running"} onclick={close}></button>
  <div class="cli-install-dialog" role="dialog" aria-modal="true" aria-labelledby="cli-install-title" tabindex="-1" use:dialogFocus={{ onDismiss: close }}>
    <h2 id="cli-install-title">
      {#if stage === "done" && succeeded}{tr(`${plan.label} is installed`, `${plan.label} instalado`)}
      {:else if stage === "running"}{tr(`Installing ${plan.label}…`, `Instalando ${plan.label}…`)}
      {:else}{tr(`Install ${plan.label}`, `Instalar ${plan.label}`)}{/if}
    </h2>

    {#if stage === "confirm"}
      {#if method}
        <p>{tr("Lume will run this command on your computer:", "O Lume vai executar este comando no seu computador:")}</p>
        <pre class="command"><code>{method.command}</code></pre>
        {#if available.length > 1}
          <div class="methods" role="radiogroup" aria-label={tr("Install method", "Método de instalação")}>
            {#each available as item (item.id)}
              <button class:active={item.id === methodId} type="button" role="radio" aria-checked={item.id === methodId} onclick={() => (chosen = item.id)}>{item.label}</button>
            {/each}
          </div>
        {/if}
        <small>{tr("You can watch the output, and cancel while it runs.", "Você acompanha a saída e pode cancelar enquanto roda.")}</small>
      {:else}
        <p>
          {#if plan.methods.length && plan.methods[0].missing}
            {tr(`Lume needs ${plan.methods[0].missing} on this computer to install ${plan.label}. Install it first, or install ${plan.label} by hand.`, `O Lume precisa do ${plan.methods[0].missing} neste computador para instalar o ${plan.label}. Instale-o primeiro ou instale o ${plan.label} manualmente.`)}
          {:else}
            {tr(`Lume has no installer for ${plan.label} on this system. Install it by hand.`, `O Lume não tem instalador do ${plan.label} neste sistema. Instale manualmente.`)}
          {/if}
        </p>
      {/if}
      <div class="actions">
        {#if plan.manualUrl}<button class="quiet" type="button" onclick={() => void openUrl(plan.manualUrl!).catch(() => undefined)}>{tr("Open install page", "Abrir página de instalação")}</button>{/if}
        <button class="quiet" type="button" onclick={close}>{tr("Cancel", "Cancelar")}</button>
        <button class="primary" type="button" disabled={!method} onclick={() => void install()}>{tr("Install", "Instalar")}</button>
      </div>
    {:else}
      <div bind:this={logElement} class="log" role="log" aria-live="polite">
        {#if startError}<p class="error">{startError}</p>{/if}
        {#each lines as line, index (index)}<span>{line}</span>{/each}
        {#if stage === "running" && !lines.length}<span class="waiting">{tr("Starting…", "Iniciando…")}</span>{/if}
      </div>

      {#if stage === "running"}
        <div class="actions">
          <span class="progress"><i aria-hidden="true"></i>{cancelling ? tr("Cancelling…", "Cancelando…") : tr("This can take a few minutes.", "Isso pode levar alguns minutos.")}</span>
          <button class="quiet" type="button" disabled={cancelling || !runId} onclick={() => void cancel()}>{tr("Cancel install", "Cancelar instalação")}</button>
        </div>
      {:else}
        {#if succeeded}
          <p class="ok">{tr("All set. Now sign in once, in a terminal:", "Tudo pronto. Agora entre na sua conta uma vez, em um terminal:")}</p>
          <div class="login"><code>{plan.loginCommand}</code><button type="button" onclick={() => void copyLogin()}>{copied ? tr("Copied", "Copiado") : tr("Copy", "Copiar")}</button></div>
          <small>{tr("Lume never sees your password; the sign-in happens in the CLI itself.", "O Lume nunca vê sua senha; o login acontece na própria CLI.")}</small>
        {:else if finished?.cancelled}
          <p class="error">{tr("The install was cancelled.", "A instalação foi cancelada.")}</p>
        {:else if finished?.timedOut}
          <p class="error">{tr("The install took too long and was stopped.", "A instalação demorou demais e foi interrompida.")}</p>
        {:else if installedButNotFound}
          <p class="error">{tr(`The installer finished, but Lume cannot find ${plan.label} yet. Restart Lume, or open a new terminal and run it.`, `O instalador terminou, mas o Lume ainda não encontra o ${plan.label}. Reinicie o Lume ou abra um novo terminal e execute.`)}</p>
        {:else if finished || startError}
          <p class="error">{tr(`The install did not finish${finished?.code != null ? ` (exit code ${finished.code})` : ""}. The output above shows why.`, `A instalação não terminou${finished?.code != null ? ` (código ${finished.code})` : ""}. A saída acima mostra o motivo.`)}</p>
        {/if}
        <div class="actions">
          {#if !succeeded && plan.manualUrl}<button class="quiet" type="button" onclick={() => void openUrl(plan.manualUrl!).catch(() => undefined)}>{tr("Open install page", "Abrir página de instalação")}</button>{/if}
          {#if !succeeded}<button class="quiet" type="button" onclick={() => { stage = "confirm"; }}>{tr("Try again", "Tentar de novo")}</button>{/if}
          <button class="primary" type="button" onclick={close}>{succeeded ? tr("Done", "Concluir") : tr("Close", "Fechar")}</button>
        </div>
      {/if}
    {/if}
  </div>
</div>

<style>
  :global(.overlay-shell:not(.dark)) .cli-install-backdrop { --workspace-pane: var(--lume-surface-light, #e8eee7); --workspace-strong: var(--lume-ink-strong-light, #243a30); --workspace-muted: var(--lume-ink-muted-light, #63746b); --workspace-line: var(--lume-line-light, #c4d2c7); --workspace-accent: var(--lume-accent-strong, #59a87e); }
  :global(.overlay-shell.dark) .cli-install-backdrop { --workspace-pane: var(--lume-surface-dark, #18221f); --workspace-strong: var(--lume-ink-strong-dark, #e9f2eb); --workspace-muted: var(--lume-ink-muted-dark, #a8b9ae); --workspace-line: var(--lume-line-dark, #344339); --workspace-accent: var(--lume-accent-strong, #8acaab); }
  .cli-install-backdrop { position: fixed; inset: 0; z-index: 100000; display: grid; place-items: center; padding: 18px; background: rgba(5, 12, 12, .56); backdrop-filter: blur(8px); }
  .backdrop-dismiss { position: absolute; inset: 0; width: 100%; height: 100%; border: 0; border-radius: 0; background: transparent; }
  .cli-install-dialog { position: relative; box-sizing: border-box; width: min(460px, 100%); max-height: calc(100vh - 36px); padding: 22px; display: grid; gap: 11px; overflow: auto; border: 1px solid var(--workspace-line, rgba(150, 180, 165, .24)); border-radius: 19px; color: var(--workspace-strong, #e9f2eb); background: var(--workspace-pane, #18221f); box-shadow: 0 22px 70px rgba(0, 0, 0, .3); }
  h2 { margin: 0; font-size: 17px; line-height: 1.25; }
  p { margin: 0; color: var(--workspace-muted, #a8b9ae); font-size: 13px; line-height: 1.5; overflow-wrap: anywhere; }
  p.ok { color: var(--workspace-strong, #e9f2eb); }
  p.error { color: #d6726c; }
  small { color: var(--workspace-muted, #a8b9ae); font-size: 11px; line-height: 1.4; }
  .command { margin: 0; padding: 10px 12px; overflow-x: auto; border: 1px solid var(--workspace-line, rgba(150, 180, 165, .24)); border-radius: 9px; background: color-mix(in srgb, var(--workspace-strong, #e9f2eb) 6%, transparent); }
  code { font: 12px/1.5 var(--lume-font-code, "SFMono-Regular", Consolas, "Liberation Mono", monospace); white-space: pre-wrap; overflow-wrap: anywhere; }
  .methods { display: flex; flex-wrap: wrap; gap: 6px; }
  .methods button { padding: 5px 11px; border: 1px solid var(--workspace-line, rgba(150, 180, 165, .24)); border-radius: 999px; font-size: 11px; }
  .methods button.active { border-color: var(--workspace-accent, #8acaab); color: var(--workspace-accent, #8acaab); }
  .log { box-sizing: border-box; height: 190px; padding: 9px 11px; display: flex; flex-direction: column; overflow: auto; border: 1px solid var(--workspace-line, rgba(150, 180, 165, .24)); border-radius: 9px; background: color-mix(in srgb, var(--workspace-strong, #e9f2eb) 5%, transparent); font: 11px/1.55 var(--lume-font-code, "SFMono-Regular", Consolas, "Liberation Mono", monospace); }
  .log span { overflow-wrap: anywhere; white-space: pre-wrap; }
  .log .waiting { color: var(--workspace-muted, #a8b9ae); }
  .login { display: flex; align-items: center; justify-content: space-between; gap: 9px; padding: 8px 8px 8px 12px; border: 1px solid var(--workspace-line, rgba(150, 180, 165, .24)); border-radius: 9px; background: color-mix(in srgb, var(--workspace-strong, #e9f2eb) 6%, transparent); }
  .login button { padding: 6px 10px; font-size: 11px; }
  .actions { margin-top: 4px; display: flex; flex-wrap: wrap; align-items: center; justify-content: flex-end; gap: 8px; }
  .progress { margin-right: auto; display: inline-flex; align-items: center; gap: 8px; color: var(--workspace-muted, #a8b9ae); font-size: 11px; }
  .progress i { width: 11px; height: 11px; border: 2px solid currentColor; border-right-color: transparent; border-radius: 50%; animation: install-spin .7s linear infinite; }
  @keyframes install-spin { to { transform: rotate(360deg); } }
  button { cursor: pointer; border: 0; border-radius: 8px; color: var(--workspace-strong, #e9f2eb); background: color-mix(in srgb, var(--workspace-strong, #e9f2eb) 10%, transparent); }
  button:disabled { opacity: .5; cursor: default; }
  .actions button { padding: 9px 14px; font-size: 12px; font-weight: 650; }
  .actions .quiet { background: transparent; border: 1px solid var(--workspace-line, rgba(150, 180, 165, .24)); }
  .actions .primary { color: var(--workspace-pane, #18221f); background: var(--workspace-accent, #8acaab); font-weight: 750; }
  button:focus-visible { outline: 2px solid var(--workspace-accent, #8acaab); outline-offset: 2px; }
  @media (prefers-reduced-motion: reduce) { .progress i { animation: none; } }
</style>
