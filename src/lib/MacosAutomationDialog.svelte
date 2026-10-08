<script lang="ts">
  import { dialogFocus } from "$lib/dialogFocus";
  import type { Language } from "$lib/i18n";
  import { openAutomationSettings } from "$lib/lume";

  let { language, onClose }: {
    language: Language;
    onClose: () => void;
  } = $props();
  let openError = $state("");
  const pt = $derived(language === "pt-BR");

  async function openSettings() {
    openError = "";
    try {
      await openAutomationSettings();
    } catch (error) {
      openError = String(error).replace(/^Error:\s*/, "");
    }
  }
</script>

<div class="automation-backdrop">
  <button class="backdrop-dismiss" type="button" aria-label={pt ? "Fechar aviso" : "Dismiss notice"} onclick={onClose}></button>
  <div class="automation-dialog" role="dialog" aria-modal="true" aria-labelledby="automation-title" tabindex="-1" use:dialogFocus={{ onDismiss: onClose }}>
    <span class="automation-mark" aria-hidden="true">!</span>
    <h2 id="automation-title">{pt ? "Permita que o Lume abra o Terminal" : "Allow Lume to open Terminal"}</h2>
    <p>{pt
      ? "O macOS bloqueou o Lume de controlar o Terminal. Em Ajustes do Sistema → Privacidade e Segurança → Automação, ative o Terminal abaixo do Lume e tente de novo."
      : "macOS blocked Lume from controlling Terminal. In System Settings → Privacy & Security → Automation, turn on Terminal under Lume, then try again."}</p>
    {#if openError}<small role="alert">{openError}</small>{/if}
    <div class="actions">
      <button class="close" type="button" onclick={onClose}>{pt ? "Agora não" : "Not now"}</button>
      <button class="primary" type="button" onclick={() => void openSettings()}>{pt ? "Abrir Ajustes de Automação" : "Open Automation Settings"}</button>
    </div>
  </div>
</div>

<style>
  :global(.overlay-shell:not(.dark)) .automation-backdrop { --workspace-pane: var(--lume-surface-light, #e8eee7); --workspace-strong: var(--lume-ink-strong-light, #243a30); --workspace-muted: var(--lume-ink-muted-light, #63746b); --workspace-line: var(--lume-line-light, #c4d2c7); --workspace-accent: var(--lume-accent-strong, #59a87e); }
  :global(.overlay-shell.dark) .automation-backdrop { --workspace-pane: var(--lume-surface-dark, #18221f); --workspace-strong: var(--lume-ink-strong-dark, #e9f2eb); --workspace-muted: var(--lume-ink-muted-dark, #a8b9ae); --workspace-line: var(--lume-line-dark, #344339); --workspace-accent: var(--lume-accent-strong, #8acaab); }
  .automation-backdrop { position: fixed; inset: 0; z-index: 100000; display: grid; place-items: center; padding: 18px; background: rgba(5, 12, 12, .56); backdrop-filter: blur(8px); }
  .backdrop-dismiss { position: absolute; inset: 0; width: 100%; height: 100%; padding: 0; border: 0; border-radius: 0; background: transparent; }
  .automation-dialog { position: relative; box-sizing: border-box; width: min(370px, 100%); padding: 22px; border: 1px solid var(--workspace-line, rgba(150, 180, 165, .24)); border-radius: 19px; color: var(--workspace-strong, #e9f2eb); background: var(--workspace-pane, #18221f); box-shadow: 0 22px 70px rgba(0, 0, 0, .3); }
  .automation-mark { display: grid; place-items: center; width: 30px; height: 30px; border-radius: 10px; color: #d9a847; background: color-mix(in srgb, #d9a847 14%, transparent); font-weight: 800; }
  h2 { margin: 13px 0 8px; font-size: 17px; line-height: 1.25; }
  p { margin: 0; color: var(--workspace-muted, #a8b9ae); font-size: 13px; line-height: 1.5; overflow-wrap: anywhere; }
  small { display: block; margin-top: 9px; color: var(--workspace-muted, #a8b9ae); font-size: 11px; line-height: 1.4; }
  .actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 20px; }
  button { cursor: pointer; border: 0; border-radius: 7px; padding: 9px 16px; color: var(--workspace-strong, #e9f2eb); background: color-mix(in srgb, var(--workspace-strong, #e9f2eb) 10%, transparent); }
  .primary { color: var(--workspace-pane, #18221f); background: var(--workspace-accent, #8acaab); font-weight: 700; }
  button:focus-visible { outline: 2px solid var(--workspace-accent, #8acaab); outline-offset: 2px; }
</style>
