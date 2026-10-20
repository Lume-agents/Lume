<script lang="ts">
  import { agentLoginCommand, type ConnectableAgent } from "$lib/agentConnection";
  import { dialogFocus } from "$lib/dialogFocus";
  import type { Language } from "$lib/i18n";

  let { agent, message, language, onClose, onSetup }: {
    agent: ConnectableAgent;
    message: string;
    language: Language;
    onClose: () => void;
    onSetup?: () => void;
  } = $props();
  let copied = $state(false);
  const command = $derived(agentLoginCommand(agent));
  const label = $derived(agent === "claude" ? "Claude Code" : agent === "opencode" ? "OpenCode" : agent === "omp" ? "Oh My Pi" : agent);
  const pt = $derived(language === "pt-BR");

  async function copyCommand() {
    if (!command) return;
    try {
      await navigator.clipboard.writeText(command);
      copied = true;
    } catch {
      copied = false;
    }
  }
</script>

<div class="agent-connection-backdrop">
  <button class="backdrop-dismiss" type="button" aria-label={pt ? "Fechar aviso" : "Dismiss notice"} onclick={onClose}></button>
  <div class="agent-connection-dialog" role="dialog" aria-modal="true" aria-labelledby="agent-connection-title" tabindex="-1" use:dialogFocus={{ onDismiss: onClose }}>
    <span class="connection-mark" aria-hidden="true">!</span>
    <h2 id="agent-connection-title">{pt ? `Conecte o ${label} para usar pelo Lume` : `Connect ${label} to use it in Lume`}</h2>
    <p>{message}</p>
    {#if command}
      <div class="connection-command"><code>{command}</code><button type="button" onclick={() => void copyCommand()}>{copied ? (pt ? "Copiado" : "Copied") : (pt ? "Copiar" : "Copy")}</button></div>
      <small>{pt ? "Faça login na CLI e tente novamente. O Lume não acessa nem armazena sua senha." : "Sign in through the CLI and try again. Lume does not access or store your password."}</small>
    {:else}
      <small>{pt ? "Conecte sua conta no aplicativo original e tente novamente pelo Lume." : "Connect your account in the original app, then try again in Lume."}</small>
    {/if}
    {#if agent === "omp" && onSetup}
      <button class="close" type="button" onclick={onSetup}>{pt ? "Configurar omp" : "Set up omp"}</button>
    {/if}
    <button class="close" type="button" onclick={onClose}>{pt ? "Entendi" : "Got it"}</button>
  </div>
</div>

<style>
  :global(.overlay-shell:not(.dark)) .agent-connection-backdrop { --workspace-pane: var(--lume-surface-light, #e8eee7); --workspace-strong: var(--lume-ink-strong-light, #243a30); --workspace-muted: var(--lume-ink-muted-light, #63746b); --workspace-line: var(--lume-line-light, #c4d2c7); --workspace-accent: var(--lume-accent-strong, #59a87e); }
  :global(.overlay-shell.dark) .agent-connection-backdrop { --workspace-pane: var(--lume-surface-dark, #18221f); --workspace-strong: var(--lume-ink-strong-dark, #e9f2eb); --workspace-muted: var(--lume-ink-muted-dark, #a8b9ae); --workspace-line: var(--lume-line-dark, #344339); --workspace-accent: var(--lume-accent-strong, #8acaab); }
  .agent-connection-backdrop { position: fixed; inset: 0; z-index: 100000; display: grid; place-items: center; padding: 18px; background: rgba(5, 12, 12, .56); backdrop-filter: blur(8px); }
  .backdrop-dismiss { position: absolute; inset: 0; width: 100%; height: 100%; border: 0; border-radius: 0; background: transparent; }
  .agent-connection-dialog { position: relative; box-sizing: border-box; width: min(370px, 100%); padding: 22px; border: 1px solid var(--workspace-line, rgba(150, 180, 165, .24)); border-radius: 19px; color: var(--workspace-strong, #e9f2eb); background: var(--workspace-pane, #18221f); box-shadow: 0 22px 70px rgba(0, 0, 0, .3); }
  .connection-mark { display: grid; place-items: center; width: 30px; height: 30px; border-radius: 10px; color: #d9a847; background: color-mix(in srgb, #d9a847 14%, transparent); font-weight: 800; }
  h2 { margin: 13px 0 8px; font-size: 17px; line-height: 1.25; }
  p { margin: 0; color: var(--workspace-muted, #a8b9ae); font-size: 13px; line-height: 1.5; overflow-wrap: anywhere; }
  small { display: block; margin-top: 9px; color: var(--workspace-muted, #a8b9ae); font-size: 11px; line-height: 1.4; }
  .connection-command { display: flex; align-items: center; justify-content: space-between; gap: 9px; margin-top: 15px; padding: 8px 8px 8px 11px; border: 1px solid var(--workspace-line, rgba(150, 180, 165, .24)); border-radius: 9px; background: color-mix(in srgb, var(--workspace-strong, #e9f2eb) 5%, transparent); }
  code { font-size: 12px; overflow-wrap: anywhere; }
  button { cursor: pointer; border: 0; border-radius: 7px; color: var(--workspace-strong, #e9f2eb); background: color-mix(in srgb, var(--workspace-strong, #e9f2eb) 10%, transparent); }
  .connection-command button { padding: 6px 9px; font-size: 11px; }
  .close { display: block; margin: 20px 0 0 auto; padding: 9px 16px; color: var(--workspace-pane, #18221f); background: var(--workspace-accent, #8acaab); font-weight: 700; }
  button:focus-visible { outline: 2px solid var(--workspace-accent, #8acaab); outline-offset: 2px; }
</style>
