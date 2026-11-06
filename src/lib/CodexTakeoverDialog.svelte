<script lang="ts">
  import { onMount } from "svelte";
  import type { AgentSession } from "$lib/domain";
  import { dialogFocus } from "$lib/dialogFocus";
  import { localize, type Language } from "$lib/i18n";
  import { takeControlSession } from "$lib/lume";
  import { sessionCapabilities } from "$lib/sessionCapabilities";

  let { session, language, onClose, onTaken }: { session: AgentSession; language: Language; onClose: () => void; onTaken: () => void } = $props();
  let busy = $state(false);
  let error = $state("");
  let primaryButton = $state<HTMLButtonElement | null>(null);
  let cancelButton = $state<HTMLButtonElement | null>(null);
  const tr = (english: string, portuguese: string) => localize(language, english, portuguese);
  const canTakeControl = $derived(sessionCapabilities(session).canTakeControl);
  onMount(() => (canTakeControl ? primaryButton : cancelButton)?.focus());

  function close() { if (!busy) onClose(); }

  async function takeControl() {
    if (busy || !canTakeControl) return;
    busy = true;
    error = "";
    try {
      await takeControlSession(session.id);
      onTaken();
    } catch (reason) {
      error = String(reason).replace(/^Error:\s*/, "");
    } finally {
      busy = false;
    }
  }
</script>

<div class="backdrop" role="presentation" onclick={(event) => { if (event.target === event.currentTarget) close(); }} onpointerdown={(event) => event.stopPropagation()}>
  <div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="codex-takeover-title" aria-describedby="codex-takeover-description" tabindex="-1" use:dialogFocus={{ onDismiss: close }}>
    <strong id="codex-takeover-title">{tr("Take control of this CLI?", "Assumir o controle desta CLI?")}</strong>
    <p id="codex-takeover-description">{!canTakeControl
      ? tr("This CLI is not ready for automatic takeover. Identify the conversation and try again.", "Esta CLI ainda não pode ser assumida automaticamente. Identifique a conversa e tente novamente.")
      : session.status === "running" || session.status === "permission_required"
        ? tr("Lume must stop the current task, close the agent process and resume the conversation before changing it.", "O Lume precisa interromper a tarefa atual, fechar o processo do agente e retomar a conversa antes de alterá-la.")
        : tr("Lume must close the agent process and resume the conversation before changing it.", "O Lume precisa fechar o processo do agente e retomar a conversa antes de alterá-la.")}</p>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <footer>
      <button bind:this={cancelButton} type="button" disabled={busy} onclick={close}>{tr("Cancel", "Cancelar")}</button>
      <button bind:this={primaryButton} class="primary" type="button" disabled={busy || !canTakeControl} onclick={() => void takeControl()}>{busy ? tr("Taking control…", "Assumindo controle…") : tr("Take control", "Assumir controle")}</button>
    </footer>
  </div>
</div>

<style>
  .backdrop { position: fixed; z-index: 1500; inset: 0; display: grid; place-items: center; padding: 14px; background: rgba(5, 12, 9, .42); }
  .dialog { box-sizing: border-box; width: min(340px, 100%); padding: 16px; display: grid; gap: 8px; border: 1px solid var(--workspace-line, var(--lume-line-light, rgba(92, 111, 103, .2))); border-radius: 14px; color: var(--workspace-text, var(--lume-ink-light, #35423d)); background: var(--workspace-raised, var(--lume-raised-light, #f3f6f4)); box-shadow: 0 12px 36px #0003; font-family: var(--lume-font-ui, system-ui, sans-serif); }
  .dialog strong { color: var(--workspace-strong, inherit); font-size: 13px; }
  .dialog p { margin: 0; font-size: 11px; line-height: 1.5; }
  .dialog .error { color: #d86655; overflow-wrap: anywhere; }
  footer { display: flex; justify-content: flex-end; flex-wrap: wrap; gap: 7px; margin-top: 4px; }
  button { min-height: 30px; padding: 0 10px; border: 1px solid currentColor; border-radius: 8px; color: inherit; background: transparent; font: inherit; font-size: 10px; cursor: pointer; }
  button.primary { border-color: transparent; color: #fff; background: var(--lume-accent-strong, #397b5c); }
  button:focus-visible { outline: 2px solid var(--lume-accent-strong, #397b5c); outline-offset: 2px; }
  button:disabled { opacity: .5; cursor: default; }
  :global(.dark) .dialog { color: var(--workspace-text, var(--lume-ink-dark, #dce6e0)); background: var(--workspace-raised, var(--lume-raised-dark, #25312c)); }
</style>
