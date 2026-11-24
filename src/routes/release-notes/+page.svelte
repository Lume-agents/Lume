<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { appUpdates, appUpdateAction, startAppUpdatesPeer } from "$lib/appUpdateRuntime";
  import { loadPreferences } from "$lib/lume";
  import type { Preferences } from "$lib/domain";
  import type { Language } from "$lib/i18n";
  import { appearanceAttributes } from "$lib/appearance";
  import { dialogFocus } from "$lib/dialogFocus";
  import OrbResponse from "$lib/OrbResponse.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";

  let language = $state<Language>("pt-BR");
  let darkModePref = $state<boolean | undefined>(undefined);
  let systemDark = $state(false);
  let accentCss = $state<string | null>(null);
  let linkError = $state(false);
  let seenNotes = $state(false);

  const dark = $derived(darkModePref ?? systemDark);
  const pt = $derived(language === "pt-BR");
  const notes = $derived($appUpdates.notes);

  function applyPreferences(prefs: Preferences) {
    language = prefs.language;
    darkModePref = prefs.darkMode;
    accentCss = appearanceAttributes(prefs).accentCss || null;
  }

  async function closeNotes() {
    await appUpdateAction("dismiss_notes").catch(() => undefined);
    await invoke("close_release_notes_window").catch(() => undefined);
  }

  function openSource(event: MouseEvent) {
    if (!("__TAURI_INTERNALS__" in window) || !notes) return;
    event.preventDefault();
    linkError = false;
    void openUrl(notes.url).catch(() => { linkError = true; });
  }

  $effect(() => {
    if (notes) {
      seenNotes = true;
    } else if (seenNotes) {
      void invoke("close_release_notes_window").catch(() => undefined);
    }
  });

  onMount(() => {
    const colorScheme = window.matchMedia("(prefers-color-scheme: dark)");
    systemDark = colorScheme.matches;
    const syncSystemTheme = (event: MediaQueryListEvent) => { systemDark = event.matches; };
    colorScheme.addEventListener("change", syncSystemTheme);

    let disposed = false;
    let stopPeer: (() => void) | undefined;
    let stopPrefs: (() => void) | undefined;

    void loadPreferences().then((prefs) => {
      if (!disposed) applyPreferences(prefs);
    }).catch(() => undefined);

    void startAppUpdatesPeer().then((stop) => {
      if (disposed) stop();
      else stopPeer = stop;
    }).catch(() => undefined);

    void listen<Preferences>("lume://preferences-changed", ({ payload }) => {
      if (!disposed) applyPreferences(payload);
    }).then((stop) => {
      if (disposed) stop();
      else stopPrefs = stop;
    }).catch(() => undefined);

    return () => {
      disposed = true;
      colorScheme.removeEventListener("change", syncSystemTheme);
      stopPeer?.();
      stopPrefs?.();
    };
  });
</script>

<svelte:head><title>Lume · {pt ? "Novidades" : "What's new"}</title></svelte:head>

<main class="release-notes-host" class:dark style={accentCss ? `--notes-accent: ${accentCss};` : undefined}>
  {#if notes}
    <div
      class="release-notes-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="release-notes-title"
      tabindex="-1"
      use:dialogFocus={{ onDismiss: () => void closeNotes() }}
    >
      <header data-tauri-drag-region>
        <h2 id="release-notes-title" data-tauri-drag-region>
          {pt ? "Novidades do Lume" : "What’s new in Lume"}
          <span data-tauri-drag-region>{notes.version}</span>
        </h2>
        <button
          class="close"
          type="button"
          aria-label={pt ? "Fechar notas da versão" : "Close release notes"}
          onclick={() => void closeNotes()}
        >
          <LumeIcon name="close" size={18} />
        </button>
      </header>
      <div class="release-notes-body">
        {#if notes.body}
          <OrbResponse text={notes.body} {language} />
        {:else if notes.loading}
          <p role="status">{pt ? "Carregando as notas desta versão…" : "Loading release notes…"}</p>
        {:else}
          <p role="status">
            {pt
              ? "Não foi possível carregar as notas desta versão. Tente novamente ou leia a release no GitHub."
              : "Could not load release notes. Try again or read this release on GitHub."}
          </p>
          <button type="button" onclick={() => void appUpdateAction("retry_notes")}>
            {pt ? "Tentar novamente" : "Try again"}
          </button>
        {/if}
      </div>
      <footer>
        <a href={notes.url} target="_blank" rel="noopener noreferrer" onclick={openSource}>
          {pt ? "Ver release no GitHub" : "View release on GitHub"}
        </a>
        <button class="primary" type="button" onclick={() => void closeNotes()}>
          {pt ? "Entendi" : "Got it"}
        </button>
      </footer>
      {#if linkError}
        <p class="link-error" role="status">{pt ? "Não foi possível abrir o link." : "Could not open the link."}</p>
      {/if}
    </div>
  {:else}
    <div class="release-notes-dialog loading-shell" role="status">
      <p>{pt ? "Carregando as notas desta versão…" : "Loading release notes…"}</p>
    </div>
  {/if}
</main>

<style>
  :global(html), :global(body) {
    width: 100%;
    height: 100%;
    margin: 0;
    overflow: hidden;
    background: transparent !important;
    font-family: "Inter", "SF Pro Text", "Segoe UI", system-ui, -apple-system, sans-serif;
  }

  .release-notes-host {
    --notes-surface: #f2f6f2;
    --notes-text: #20342b;
    --notes-muted: #5d7066;
    --notes-line: rgba(54, 88, 70, 0.18);
    --notes-accent: #397b5c;
    width: 100%;
    height: 100%;
    padding: 18px;
    box-sizing: border-box;
    display: flex;
    align-items: stretch;
    justify-content: center;
  }

  .release-notes-host.dark {
    --notes-surface: #18221f;
    --notes-text: #e9f2eb;
    --notes-muted: #a3b5aa;
    --notes-line: rgba(190, 214, 200, 0.14);
    --notes-accent: #4fae7e;
  }

  .release-notes-dialog {
    box-sizing: border-box;
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border: 1px solid var(--notes-line);
    border-radius: 16px;
    color: var(--notes-text);
    background: var(--notes-surface);
    box-shadow: 0 18px 42px rgba(4, 14, 10, 0.34), 0 4px 12px rgba(4, 14, 10, 0.18);
    outline: none;
  }

  .loading-shell {
    align-items: center;
    justify-content: center;
    color: var(--notes-muted);
    font-size: 13px;
  }

  header {
    display: flex;
    align-items: flex-start;
    gap: 14px;
    padding: 18px 18px 14px;
    border-bottom: 1px solid var(--notes-line);
    cursor: grab;
    user-select: none;
  }

  header:active { cursor: grabbing; }

  h2 {
    flex: 1;
    min-width: 0;
    margin: 0;
    font-size: 19px;
    font-weight: 700;
    line-height: 1.35;
    text-wrap: balance;
  }

  h2 span {
    display: block;
    margin-top: 4px;
    font-size: 13px;
    font-weight: 500;
    color: var(--notes-muted);
    font-variant-numeric: tabular-nums;
  }

  .release-notes-body {
    flex: 1;
    min-height: 0;
    padding: 18px;
    overflow-y: auto;
    overflow-wrap: anywhere;
    overscroll-behavior: contain;
    scrollbar-width: thin;
    scrollbar-color: var(--notes-line) transparent;
  }

  .release-notes-body :global(.orb-response) { font-size: 14px; }
  .release-notes-body :global(.orb-response h1),
  .release-notes-body :global(.orb-response h2),
  .release-notes-body :global(.orb-response h3) { font-size: 16px; }
  .release-notes-body :global(.orb-response a) { color: var(--notes-text); }
  .release-notes-body > p, .link-error { margin: 0 0 14px; font-size: 13px; line-height: 1.6; color: var(--notes-muted); }

  footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 16px;
    flex-wrap: wrap;
    padding: 14px 18px;
    border-top: 1px solid var(--notes-line);
  }

  a { color: var(--notes-text); font-size: 12px; text-underline-offset: 3px; }

  button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-height: 32px;
    padding: 8px 12px;
    border: 1px solid var(--notes-line);
    border-radius: 8px;
    background: transparent;
    color: var(--notes-text);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
    transition: background 160ms ease;
  }

  button:hover { background: color-mix(in srgb, var(--notes-accent) 12%, transparent); }

  button.primary {
    color: var(--notes-text);
    background: color-mix(in srgb, var(--notes-accent) 14%, var(--notes-surface));
    border-color: color-mix(in srgb, var(--notes-accent) 50%, var(--notes-line));
    font-weight: 650;
  }

  button.primary:hover {
    background: color-mix(in srgb, var(--notes-accent) 22%, var(--notes-surface));
  }

  button.close {
    flex: 0 0 auto;
    min-height: 28px;
    padding: 5px;
    border: 0;
  }

  button:focus-visible, a:focus-visible {
    outline: 2px solid var(--notes-accent);
    outline-offset: 3px;
  }

  .link-error { padding: 0 18px; }
  @media (prefers-reduced-motion: reduce) { button { transition: none; } }
</style>
