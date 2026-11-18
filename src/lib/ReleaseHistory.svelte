<script lang="ts">
  import { fade } from "svelte/transition";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { loadReleaseHistory, type PublishedRelease } from "$lib/appUpdateRuntime";
  import { dialogFocus } from "$lib/dialogFocus";
  import OrbResponse from "$lib/OrbResponse.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import type { Language } from "$lib/i18n";

  /** Every published version's notes, newest first; the installed one starts open. */
  let { language, dark = false, currentVersion, onClose }: { language: Language; dark?: boolean; currentVersion: string; onClose: () => void } = $props();
  const pt = $derived(language === "pt-BR");
  let releases = $state<PublishedRelease[]>([]);
  let loading = $state(true);
  let failed = $state(false);

  async function load() {
    loading = true;
    failed = false;
    try { releases = await loadReleaseHistory(); }
    catch { failed = true; }
    finally { loading = false; }
  }
  void load();

  const date = (value: string | null) => value ? new Date(value).toLocaleDateString(language, { day: "2-digit", month: "short", year: "numeric" }) : "";
  function open(event: MouseEvent, url: string) {
    if (!("__TAURI_INTERNALS__" in window)) return;
    event.preventDefault();
    void openUrl(url).catch(() => undefined);
  }
</script>

<div class="release-history-backdrop" class:dark transition:fade={{ duration: 160 }}>
  <div class="release-history" role="dialog" aria-modal="true" aria-labelledby="release-history-title" tabindex="-1" use:dialogFocus={{ onDismiss: onClose }}>
    <header>
      <h2 id="release-history-title">{pt ? "Notas das versões" : "Release notes"}</h2>
      <button class="close" type="button" aria-label={pt ? "Fechar" : "Close"} onclick={onClose}><LumeIcon name="close" size={18} /></button>
    </header>
    <div class="release-history-body">
      {#if loading}
        <p role="status">{pt ? "Carregando as versões…" : "Loading releases…"}</p>
      {:else if failed}
        <p role="status">{pt ? "Não foi possível carregar as versões do GitHub." : "Could not load the releases from GitHub."}</p>
        <button type="button" onclick={() => void load()}>{pt ? "Tentar novamente" : "Try again"}</button>
      {:else if !releases.length}
        <p role="status">{pt ? "Nenhuma versão publicada." : "No published release."}</p>
      {:else}
        {#each releases as release (release.version)}
          <details class="release" open={release.version === currentVersion}>
            <summary>
              <strong>{release.version}</strong>
              {#if release.version === currentVersion}<em>{pt ? "instalada" : "installed"}</em>{/if}
              <span>{date(release.publishedAt)}</span>
            </summary>
            {#if release.body.trim()}<OrbResponse text={release.body} {language} />{:else}<p>{pt ? "Sem notas para esta versão." : "No notes for this version."}</p>{/if}
            <a href={release.url} target="_blank" rel="noopener noreferrer" onclick={(event) => open(event, release.url)}>{pt ? "Ver no GitHub" : "View on GitHub"}</a>
          </details>
        {/each}
      {/if}
    </div>
  </div>
</div>

<style>
  .release-history-backdrop { --notes-surface: var(--workspace-pane, var(--lume-surface-light, #e8eee7)); --notes-text: var(--workspace-text, var(--lume-ink-light, #243a30)); --notes-muted: var(--workspace-muted, var(--lume-ink-muted-light, #63746b)); --notes-line: var(--workspace-line, var(--lume-line-light, #c4d2c7)); --notes-accent: var(--workspace-accent, var(--lume-accent-strong, #397b5c)); position: fixed; z-index: 100060; inset: 0; display: grid; place-items: center; padding: 14px; background: rgba(5, 12, 12, .5); }
  .release-history-backdrop.dark { --notes-surface: var(--workspace-pane, var(--lume-surface-dark, #18221f)); --notes-text: var(--workspace-text, var(--lume-ink-dark, #e9f2eb)); --notes-muted: var(--workspace-muted, var(--lume-ink-muted-dark, #a8b9ae)); --notes-line: var(--workspace-line, var(--lume-line-dark, #344339)); }
  .release-history { box-sizing: border-box; display: flex; flex-direction: column; width: min(640px, 100%); max-height: calc(100dvh - 28px); overflow: hidden; border: 1px solid var(--notes-line); border-radius: 14px; color: var(--notes-text); background: var(--notes-surface); outline: none; }
  header { display: flex; align-items: center; gap: 14px; padding: 16px 18px 12px; border-bottom: 1px solid var(--notes-line); }
  h2 { flex: 1; min-width: 0; margin: 0; font-size: 18px; font-weight: 700; }
  .release-history-body { min-height: 0; padding: 8px 18px 18px; overflow-y: auto; overflow-wrap: anywhere; overscroll-behavior: contain; scrollbar-width: thin; scrollbar-color: var(--notes-line) transparent; }
  .release-history-body > p { margin: 10px 0 14px; color: var(--notes-muted); font-size: 13px; }
  .release { border-bottom: 1px solid color-mix(in srgb, var(--notes-line) 70%, transparent); }
  .release summary { display: flex; align-items: baseline; gap: 8px; padding: 11px 0; cursor: pointer; list-style: none; }
  .release summary::-webkit-details-marker { display: none; }
  .release summary strong { font-size: 14px; font-variant-numeric: tabular-nums; }
  .release summary em { padding: 1px 7px; border-radius: 999px; color: var(--notes-accent); background: color-mix(in srgb, var(--notes-accent) 14%, transparent); font-size: 10px; font-style: normal; font-weight: 700; }
  .release summary span { margin-left: auto; color: var(--notes-muted); font-size: 11px; }
  .release[open] { padding-bottom: 12px; }
  .release :global(.orb-response) { font-size: 13px; }
  .release :global(.orb-response h1) { display: none; }
  .release :global(.orb-response h2), .release :global(.orb-response h3) { font-size: 14px; }
  .release :global(.orb-response a) { color: var(--notes-text); }
  .release > p { margin: 0 0 8px; color: var(--notes-muted); font-size: 12px; }
  a { color: var(--notes-text); font-size: 12px; text-underline-offset: 3px; }
  button { display: inline-flex; align-items: center; justify-content: center; min-height: 32px; padding: 8px 12px; border: 1px solid var(--notes-line); border-radius: 8px; background: transparent; color: var(--notes-text); font: inherit; font-size: 12px; cursor: pointer; }
  button:hover { background: color-mix(in srgb, var(--notes-accent) 12%, transparent); }
  button.close { flex: 0 0 auto; min-height: 28px; padding: 5px; border: 0; }
  button:focus-visible, a:focus-visible, summary:focus-visible { outline: 2px solid var(--notes-accent); outline-offset: 3px; }
</style>
