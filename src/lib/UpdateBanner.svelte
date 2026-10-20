<script lang="ts">
  import LumeLogo from "$lib/LumeLogo.svelte";
  import { appUpdateAction } from "$lib/appUpdateRuntime";
  import type { AppUpdateState } from "$lib/appUpdates";
  import type { Language } from "$lib/i18n";

  let { state: update, language, dark = false, notice = false }: {
    state: AppUpdateState; language: Language; dark?: boolean; notice?: boolean;
  } = $props();
  const pt = $derived(language === "pt-BR");
  const busy = $derived(["checking", "downloading"].includes(update.status) || (update.status === "ready" && !update.error));
  const title = $derived(update.status === "available"
    ? (pt ? `Lume ${update.availableVersion} disponível` : `Lume ${update.availableVersion} is available`)
    : update.status === "downloading" ? (pt ? "Instalando atualização" : "Installing update")
    : update.status === "ready" ? (pt ? "Atualização instalada" : "Update installed")
    : update.status === "error" ? (pt ? "Não foi possível atualizar" : "Could not update Lume")
    : `Lume ${update.currentVersion}`);
  const description = $derived(update.status === "available" ? ""
    : update.status === "checking" ? (pt ? "Procurando atualizações…" : "Checking for updates…")
    : update.status === "downloading" ? (pt ? "Baixando e preparando a atualização…" : "Downloading and preparing the update…")
    : update.status === "ready" ? (update.error ? (pt ? "Reinicie o Lume para concluir." : "Restart Lume to finish.") : (pt ? "Reiniciando o Lume…" : "Restarting Lume…"))
    : update.status === "error" ? (pt ? "Tente novamente. Seus agentes continuam disponíveis." : "Try again. Your agents are still available.")
    : update.status === "up_to_date" ? (pt ? "Você está usando a versão mais recente." : "You are using the latest version.")
    : (pt ? "As atualizações são verificadas ao abrir o Lume." : "Updates are checked when Lume starts."));
  const action = $derived(update.status === "ready" ? "restart"
    : update.status === "available" || update.errorOperation === "install" ? "install" : "check");
  let actionError = $state("");
  async function activate() {
    actionError = "";
    try { await appUpdateAction(action); }
    catch (error) { actionError = String(error).replace(/^Error:\s*/, ""); }
  }
</script>

<section class="update-banner" class:dark class:notice data-update-card aria-label={pt ? "Atualizações do Lume" : "Lume updates"}>
  <div class="update-banner-main">
    <LumeLogo size={26} />
    <div class="update-banner-copy" aria-live="polite"><strong>{title}</strong>{#if description}<p>{description}</p>{/if}</div>
    <div class="update-banner-actions">
      {#if notice && update.status === "available"}<button class="dismiss" type="button" onclick={() => void appUpdateAction("dismiss_notice")}>{pt ? "Agora não" : "Not now"}</button>{/if}
      <button class="action" class:primary={action !== "check"} type="button" disabled={busy} onclick={() => void activate()}>
        {busy ? (update.status === "checking" ? (pt ? "Verificando…" : "Checking…") : update.status === "ready" ? (pt ? "Reiniciando…" : "Restarting…") : (pt ? "Instalando…" : "Installing…"))
          : action === "install" ? (pt ? "Instalar" : "Install") : action === "restart" ? (pt ? "Reiniciar" : "Restart") : (pt ? "Verificar" : "Check")}
      </button>
    </div>
  </div>
  {#if update.status === "downloading" || update.status === "ready"}
    <div class="update-download" class:indeterminate={update.progress === null} role="progressbar" aria-label={pt ? "Progresso da atualização" : "Update progress"} aria-valuemin="0" aria-valuemax="100" aria-valuenow={update.progress ?? undefined} aria-valuetext={update.progress === null ? (pt ? "Baixando" : "Downloading") : `${update.progress}%`}><span style:transform={`scaleX(${update.progress === null ? 1 : update.progress / 100})`}></span></div>
  {/if}
  {#if update.error || actionError}<details class="update-error"><summary>{pt ? "Detalhes do erro" : "Error details"}</summary><pre>{actionError || update.error}</pre></details>{/if}
</section>

<style>
  .update-banner { --update-surface: var(--workspace-pane, var(--lume-surface-light, #e8eee7)); --update-text: var(--workspace-text, var(--lume-ink-light, #243a30)); --update-muted: var(--workspace-muted, var(--lume-ink-muted-light, #63746b)); --update-line: var(--workspace-line, var(--lume-line-light, #c4d2c7)); --update-accent: var(--workspace-accent, var(--lume-accent-strong, #397b5c)); box-sizing: border-box; min-width: 0; padding: 10px 10px 10px 14px; border: 1px solid color-mix(in srgb, var(--update-line) 70%, transparent); border-radius: 16px; background: var(--update-surface); color: var(--update-text); }
  .update-banner.notice { box-shadow: 0 14px 40px rgba(0, 0, 0, .16), 0 2px 8px rgba(0, 0, 0, .08); }
  .update-banner.dark { --update-surface: var(--workspace-pane, var(--lume-surface-dark, #18221f)); --update-text: var(--workspace-text, var(--lume-ink-dark, #e9f2eb)); --update-muted: var(--workspace-muted, var(--lume-ink-muted-dark, #a8b9ae)); --update-line: var(--workspace-line, var(--lume-line-dark, #344339)); }
  .update-banner-main { display: flex; align-items: center; gap: 11px; }
  .update-banner-copy { flex: 1; min-width: 0; }
  strong { display: block; font-size: 13px; font-weight: 650; line-height: 1.3; letter-spacing: -.01em; overflow-wrap: anywhere; }
  p { margin: 2px 0 0; color: var(--update-muted); font-size: 11.5px; line-height: 1.45; }
  button { display: inline-flex; align-items: center; justify-content: center; min-height: 30px; padding: 6px 14px; border: 0; border-radius: 999px; color: var(--update-text); background: transparent; font: inherit; font-size: 12px; font-weight: 650; white-space: nowrap; cursor: pointer; transition: background 160ms ease, color 160ms ease, opacity 160ms ease; }
  button:hover:not(:disabled) { background: color-mix(in srgb, var(--update-accent) 12%, transparent); }
  button.dismiss { color: var(--update-muted); }
  button.primary { color: #fff; background: var(--update-accent); }
  button.primary:hover:not(:disabled) { background: color-mix(in srgb, var(--update-accent) 86%, #000); }
  button:disabled { opacity: .6; cursor: default; }
  button:focus-visible, summary:focus-visible { outline: 2px solid var(--update-accent); outline-offset: 2px; }
  .update-banner-actions { display: flex; flex: 0 0 auto; align-items: center; gap: 2px; }
  .update-download { height: 3px; margin: 10px 4px 2px 0; overflow: hidden; border-radius: 3px; background: color-mix(in srgb, var(--update-accent) 14%, transparent); }
  .update-download > span { display: block; width: 100%; height: 100%; transform-origin: left; border-radius: inherit; background: var(--update-accent); transition: transform 180ms ease; }
  .update-download.indeterminate > span { width: 24%; animation: update-sweep 1.4s ease-in-out infinite alternate; }
  .update-error { margin-top: 8px; color: var(--update-muted); font-size: 12px; }
  .update-error summary { cursor: pointer; }
  .update-error pre { max-height: 120px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; font-size: 11px; }
  @keyframes update-sweep { from { transform: translateX(-100%); } to { transform: translateX(420%); } }
  @media (prefers-reduced-motion: reduce) { button, .update-download > span { transition: none; animation: none; } }
</style>
