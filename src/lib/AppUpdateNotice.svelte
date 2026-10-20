<script lang="ts">
  import { onMount } from "svelte";
  import { fade } from "svelte/transition";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { appUpdates, appUpdateAction } from "$lib/appUpdateRuntime";
  import { dialogFocus } from "$lib/dialogFocus";
  import UpdateBanner from "$lib/UpdateBanner.svelte";
  import OrbResponse from "$lib/OrbResponse.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import type { Language } from "$lib/i18n";

  let { language, dark = false, ready = true, surfaceOpen = true, noticeVisible = true, onReveal = async () => {} }: {
    language: Language; dark?: boolean; ready?: boolean; surfaceOpen?: boolean; noticeVisible?: boolean;
    onReveal?: () => Promise<void>;
  } = $props();
  const pt = $derived(language === "pt-BR");
  const notes = $derived($appUpdates.notes);
  const showNotice = $derived($appUpdates.notify && ($appUpdates.status !== "available" || $appUpdates.dismissedVersion !== $appUpdates.availableVersion));
  let visible = $state(false);
  let visibilityRevision = $state(0);
  let presented = "";
  let disposed = false;
  let linkError = $state(false);

  $effect(() => {
    const enabled = ready;
    const key = notes ? `notes:${notes.version}` : showNotice ? `update:${$appUpdates.availableVersion}:${$appUpdates.errorOperation ?? ""}` : "";
    visibilityRevision;
    if (!enabled || !key) { visible = false; return; }
    let cancelled = false;
    void (async () => {
      const native = "__TAURI_INTERNALS__" in window;
      const shown = native ? await getCurrentWindow().isVisible() : document.visibilityState !== "hidden";
      if (cancelled || disposed) return;
      if (!shown) { visible = false; return; }
      if (presented !== key) { presented = key; await onReveal(); }
      if (!cancelled && !disposed) visible = true;
    })().catch(() => { /* A hidden/unavailable window must not disrupt the active mode. */ });
    return () => { cancelled = true; };
  });

  onMount(() => {
    const refresh = () => { visibilityRevision += 1; };
    document.addEventListener("visibilitychange", refresh);
    window.addEventListener("focus", refresh);
    let unlisten: (() => void) | undefined;
    if ("__TAURI_INTERNALS__" in window) void getCurrentWindow().onFocusChanged(refresh).then(stop => {
      if (disposed) stop(); else unlisten = stop;
    }).catch(() => undefined);
    return () => { disposed = true; unlisten?.(); document.removeEventListener("visibilitychange", refresh); window.removeEventListener("focus", refresh); };
  });

  function closeNotes() { void appUpdateAction("dismiss_notes"); }
  function openSource(event: MouseEvent) {
    if (!("__TAURI_INTERNALS__" in window) || !notes) return;
    event.preventDefault(); linkError = false;
    void openUrl(notes.url).catch(() => { linkError = true; });
  }
</script>

{#if visible && ready && surfaceOpen}
  {#if notes}
    <div class="release-notes-backdrop" class:dark transition:fade={{ duration: 160 }}>
      <div class="release-notes-dialog" role="dialog" aria-modal="true" aria-labelledby="release-notes-title" tabindex="-1" use:dialogFocus={{ onDismiss: closeNotes }}>
        <header><h2 id="release-notes-title">{pt ? "Novidades do Lume" : "What’s new in Lume"} <span>{notes.version}</span></h2><button class="close" type="button" aria-label={pt ? "Fechar notas da versão" : "Close release notes"} onclick={closeNotes}><LumeIcon name="close" size={18} /></button></header>
        <div class="release-notes-body">
          {#if notes.body}<OrbResponse text={notes.body} {language} />
          {:else if notes.loading}<p role="status">{pt ? "Carregando as notas desta versão…" : "Loading release notes…"}</p>
          {:else}<p role="status">{pt ? "Não foi possível carregar as notas desta versão. Tente novamente ou leia a release no GitHub." : "Could not load release notes. Try again or read this release on GitHub."}</p><button type="button" onclick={() => void appUpdateAction("retry_notes")}>{pt ? "Tentar novamente" : "Try again"}</button>{/if}
        </div>
        <footer><a href={notes.url} target="_blank" rel="noopener noreferrer" onclick={openSource}>{pt ? "Ver release no GitHub" : "View release on GitHub"}</a><button class="primary" type="button" onclick={closeNotes}>{pt ? "Entendi" : "Got it"}</button></footer>
        {#if linkError}<p class="link-error" role="status">{pt ? "Não foi possível abrir o link." : "Could not open the link."}</p>{/if}
      </div>
    </div>
  {:else if showNotice && noticeVisible}
    <aside class="app-update-notice" aria-label={pt ? "Atualização disponível" : "Lume update"} transition:fade={{ duration: 160 }}><UpdateBanner state={$appUpdates} {language} {dark} notice /></aside>
  {/if}
{/if}

<style>
  .app-update-notice { position: fixed; z-index: 100050; inset-inline-end: 16px; bottom: 16px; width: min(400px, calc(100% - 32px)); }
  .release-notes-backdrop { --notes-surface: var(--workspace-pane, var(--lume-surface-light, #e8eee7)); --notes-text: var(--workspace-text, var(--lume-ink-light, #243a30)); --notes-muted: var(--workspace-muted, var(--lume-ink-muted-light, #63746b)); --notes-line: var(--workspace-line, var(--lume-line-light, #c4d2c7)); --notes-accent: var(--workspace-accent, var(--lume-accent-strong, #397b5c)); position: fixed; z-index: 100060; inset: 0; display: grid; place-items: center; padding: 14px; background: rgba(5, 12, 12, .5); }
  .release-notes-backdrop.dark { --notes-surface: var(--workspace-pane, var(--lume-surface-dark, #18221f)); --notes-text: var(--workspace-text, var(--lume-ink-dark, #e9f2eb)); --notes-muted: var(--workspace-muted, var(--lume-ink-muted-dark, #a8b9ae)); --notes-line: var(--workspace-line, var(--lume-line-dark, #344339)); }
  .release-notes-dialog { box-sizing: border-box; display: flex; flex-direction: column; width: min(600px, 100%); max-height: calc(100dvh - 28px); overflow: hidden; border: 1px solid var(--notes-line); border-radius: 14px; color: var(--notes-text); background: var(--notes-surface); outline: none; }
  header { display: flex; align-items: flex-start; gap: 14px; padding: 18px 18px 14px; border-bottom: 1px solid var(--notes-line); }
  h2 { flex: 1; min-width: 0; margin: 0; font-size: 19px; font-weight: 700; line-height: 1.35; text-wrap: balance; }
  h2 span { display: block; margin-top: 4px; font-size: 13px; font-weight: 500; color: var(--notes-muted); font-variant-numeric: tabular-nums; }
  .release-notes-body { min-height: 0; padding: 18px; overflow-y: auto; overflow-wrap: anywhere; overscroll-behavior: contain; scrollbar-width: thin; scrollbar-color: var(--notes-line) transparent; }
  .release-notes-body :global(.orb-response) { font-size: 14px; }
  .release-notes-body :global(.orb-response h1), .release-notes-body :global(.orb-response h2), .release-notes-body :global(.orb-response h3) { font-size: 16px; }
  .release-notes-body :global(.orb-response a) { color: var(--notes-text); }
  .release-notes-body > p, .link-error { margin: 0 0 14px; font-size: 13px; line-height: 1.6; color: var(--notes-muted); }
  footer { display: flex; justify-content: space-between; align-items: center; gap: 16px; flex-wrap: wrap; padding: 14px 18px; border-top: 1px solid var(--notes-line); }
  a { color: var(--notes-text); font-size: 12px; text-underline-offset: 3px; }
  button { display: inline-flex; align-items: center; justify-content: center; min-height: 32px; padding: 8px 12px; border: 1px solid var(--notes-line); border-radius: 8px; background: transparent; color: var(--notes-text); font: inherit; font-size: 12px; cursor: pointer; transition: background 160ms ease; }
  button:hover { background: color-mix(in srgb, var(--notes-accent) 12%, transparent); }
  button.primary { color: var(--notes-text); background: color-mix(in srgb, var(--notes-accent) 14%, var(--notes-surface)); border-color: color-mix(in srgb, var(--notes-accent) 50%, var(--notes-line)); font-weight: 650; }
  button.primary:hover { background: color-mix(in srgb, var(--notes-accent) 22%, var(--notes-surface)); }
  button.close { flex: 0 0 auto; min-height: 28px; padding: 5px; border: 0; }
  button:focus-visible, a:focus-visible { outline: 2px solid var(--notes-accent); outline-offset: 3px; }
  .link-error { padding: 0 18px; }
  @media (prefers-reduced-motion: reduce) { button { transition: none; } }
</style>
