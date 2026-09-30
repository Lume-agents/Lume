<script lang="ts">
  import { onMount } from "svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import SystemBanner from "$lib/SystemBanner.svelte";

  let { message, language = "en", onClose }: {
    message: string;
    language?: "en" | "pt-BR";
    onClose: () => void;
  } = $props();
  let dialog: HTMLDialogElement;
  let copied = $state(false);
  let copyError = $state("");
  const tr = (en: string, pt: string) => language === "pt-BR" ? pt : en;

  onMount(() => { dialog.showModal(); });

  async function copy() {
    copyError = "";
    try {
      await navigator.clipboard.writeText(message);
      copied = true;
    } catch {
      copyError = tr("Could not copy. Select the text below to copy it manually.", "Não foi possível copiar. Selecione o texto abaixo para copiar manualmente.");
    }
  }
</script>

<dialog bind:this={dialog} class="banner-details" aria-label={tr("Message details", "Detalhes da mensagem")} onclose={onClose}>
  <header>
    <h2>{tr("Message details", "Detalhes da mensagem")}</h2>
    <button type="button" class="close" aria-label={tr("Close", "Fechar")} onclick={() => dialog.close()}><LumeIcon name="close" size={16} /></button>
  </header>
  {#if copyError}
    <SystemBanner message={copyError} tone="error" inline onDismiss={() => (copyError = "")} dismissLabel={tr("Dismiss", "Fechar")} />
  {/if}
  <textarea readonly rows="12" value={message} aria-label={tr("Full message", "Mensagem completa")}></textarea>
  <button type="button" class="copy" onclick={() => void copy()}><LumeIcon name={copied ? "check" : "copy"} size={14} />{copied ? tr("Copied", "Copiado") : tr("Copy message", "Copiar mensagem")}</button>
</dialog>

<style>
  .banner-details {
    --detail-surface: var(--workspace-raised, var(--lume-raised-light, #fbfcf8));
    --detail-text: var(--workspace-text, var(--lume-ink-light, #26332d));
    --detail-line: var(--workspace-line, var(--lume-line-light, rgba(90, 110, 99, .25)));
    --detail-accent: var(--workspace-accent, var(--lume-accent-strong, #3f9e72));
    width: min(560px, calc(100vw - 28px)); max-height: calc(100dvh - 28px);
    margin: auto; padding: 14px; overflow: auto; border: 1px solid var(--detail-line);
    border-radius: 14px; color: var(--detail-text); background: var(--detail-surface);
    box-shadow: 0 18px 50px rgba(7, 22, 14, .22);
    font-family: inherit;
  }
  :global(.dark) .banner-details { --detail-surface: var(--workspace-raised, var(--lume-raised-dark, #17221d)); --detail-text: var(--workspace-text, var(--lume-ink-dark, #dce7e1)); --detail-line: var(--workspace-line, var(--lume-line-dark, rgba(210, 230, 220, .12))); }
  .banner-details::backdrop { background: rgba(9, 16, 12, .44); }
  header { display: flex; align-items: center; gap: 10px; margin-bottom: 12px; }
  h2 { min-width: 0; flex: 1; margin: 0; font-size: 13px; font-weight: 650; }
  button { display: flex; align-items: center; justify-content: center; gap: 6px; border: 0; border-radius: 7px; color: inherit; background: transparent; cursor: pointer; font: inherit; }
  .close { width: 28px; height: 28px; flex: 0 0 auto; padding: 0; }
  .copy { min-height: 30px; margin: 12px 0 0 auto; padding: 0 9px; color: var(--detail-accent); background: color-mix(in srgb, var(--detail-accent) 12%, transparent); font-size: 11px; font-weight: 650; }
  button:hover { background: color-mix(in srgb, var(--detail-accent) 15%, transparent); }
  button:focus-visible, textarea:focus-visible { outline: 2px solid var(--detail-accent); outline-offset: 2px; }
  textarea { display: block; width: 100%; max-height: calc(100dvh - 150px); box-sizing: border-box; margin: 0; padding: 10px; resize: vertical; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; border: 0; border-radius: 8px; background: color-mix(in srgb, var(--detail-text) 5%, transparent); color: inherit; font: 11px/1.65 ui-monospace, "SFMono-Regular", Consolas, monospace; user-select: text; scrollbar-width: thin; scrollbar-color: var(--detail-line) transparent; }
  textarea::-webkit-scrollbar { width: 6px; height: 6px; }
  textarea::-webkit-scrollbar-thumb { border-radius: 3px; background: var(--detail-line); }
</style>
