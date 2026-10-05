<script lang="ts">
  import { onMount } from "svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import type { Language } from "$lib/i18n";
  import { loadCodexCliConversations, linkCodexCliConversation, type CodexCliConversationChoices } from "$lib/lume";

  let { sessionId, language = "en", dark = false, onClose, onLinked }: {
    sessionId: string;
    language?: Language;
    dark?: boolean;
    onClose: () => void;
    onLinked: () => void | Promise<void>;
  } = $props();
  let dialog: HTMLDialogElement;
  let searchInput: HTMLInputElement;
  let query = $state("");
  let choices = $state<CodexCliConversationChoices | null>(null);
  let selected = $state("");
  let loading = $state(true);
  let saving = $state(false);
  let error = $state("");
  let retry = $state(0);
  let generation = 0;
  let disposed = false;
  const tr = (en: string, pt: string) => language === "pt-BR" ? pt : en;

  $effect(() => {
    const search = query;
    retry;
    const target = sessionId;
    const request = ++generation;
    loading = true;
    error = "";
    const timer = setTimeout(async () => {
      try {
        const result = await loadCodexCliConversations(target, search);
        if (disposed || request !== generation) return;
        choices = result;
        if (selected && !result.candidates.some((candidate) => candidate.nativeSessionId === selected)
          && selected !== result.linkedNativeSessionId) selected = "";
        if (!selected && result.linkedNativeSessionId) selected = result.linkedNativeSessionId;
      } catch (reason) {
        if (!disposed && request === generation) { choices = null; error = String(reason).replace(/^Error:\s*/, ""); }
      } finally {
        if (!disposed && request === generation) loading = false;
      }
    }, search ? 220 : 0);
    return () => clearTimeout(timer);
  });

  async function link() {
    if (saving || loading || !choices || !selected) return;
    saving = true;
    error = "";
    try {
      await linkCodexCliConversation(sessionId, choices.processKey, selected);
      if (disposed) return;
      await onLinked();
      onClose();
    } catch (reason) {
      if (!disposed) error = String(reason).replace(/^Error:\s*/, "");
    } finally {
      if (!disposed) saving = false;
    }
  }

  onMount(() => {
    const previous = document.activeElement;
    dialog.showModal();
    searchInput.focus();
    return () => {
      disposed = true;
      generation++;
      dialog.close();
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  });
</script>

<dialog bind:this={dialog} class:dark class="cli-association" aria-labelledby="cli-association-title" aria-describedby="cli-association-help" oncancel={(event) => { event.preventDefault(); if (!saving) onClose(); }}>
  <form onsubmit={(event) => { event.preventDefault(); void link(); }}>
    <header>
      <h2 id="cli-association-title">{tr("Link conversation", "Vincular conversa")}</h2>
      <button class="icon-button" type="button" disabled={saving} aria-label={tr("Close", "Fechar")} onclick={onClose}><LumeIcon name="close" size={18} /></button>
    </header>
    <p id="cli-association-help">{tr("Select the conversation open in this CLI. This identifies its card for monitoring; it does not take control.", "Selecione a conversa aberta nesta CLI. O vínculo identifica o card para monitoramento; não assume o controle.")}</p>
    <label class="search">
      <LumeIcon name="search" size={16} />
      <input bind:this={searchInput} bind:value={query} maxlength="200" disabled={saving} type="search" autocomplete="off" aria-label={tr("Search conversations", "Pesquisar conversas")} placeholder={tr("Search by conversation name", "Pesquisar pelo nome da conversa")} />
    </label>
    <div class="choices" aria-busy={loading}>
      {#if loading}
        <p class="empty" role="status">{tr("Loading local conversations…", "Carregando conversas locais…")}</p>
      {:else if choices?.candidates.length}
        {#each choices.candidates as candidate (candidate.nativeSessionId)}
          <button class="choice" class:selected={selected === candidate.nativeSessionId} type="button" disabled={saving} aria-pressed={selected === candidate.nativeSessionId} onclick={() => { selected = candidate.nativeSessionId; }}>
            <span class="choice-copy"><strong>{candidate.name}</strong><small>{candidate.nativeSessionId.slice(0, 8)}…</small></span>
            {#if selected === candidate.nativeSessionId}<LumeIcon name="check" size={17} />{/if}
          </button>
        {/each}
        {#if choices.hasMore}<p class="hint">{tr("Search to find more conversations.", "Use a busca para encontrar mais conversas.")}</p>{/if}
      {:else if !error}
        <p class="empty">{tr("No available conversations. Try another name, or wait for the CLI to identify its session.", "Nenhuma conversa disponível. Tente outro nome ou aguarde a CLI identificar a sessão.")}</p>
      {/if}
    </div>
    {#if error}<p class="error" role="alert"><LumeIcon name="warning" size={17} /><span>{error}</span></p>{/if}
    <footer>
      {#if error && !saving}<button type="button" onclick={() => { retry++; }}>{tr("Retry", "Tentar novamente")}</button>{/if}
      <button type="button" disabled={saving} onclick={onClose}>{tr("Cancel", "Cancelar")}</button>
      <button class="primary" type="submit" disabled={loading || saving || !selected || !choices || selected === choices.linkedNativeSessionId}>{saving ? tr("Linking…", "Vinculando…") : tr("Link conversation", "Vincular conversa")}</button>
    </footer>
  </form>
</dialog>

<style>
  .cli-association { --association-bg: var(--lume-raised-light); --association-text: var(--lume-ink-light); --association-muted: var(--lume-ink-muted-light); --association-line: var(--lume-line-light); --association-subtle: var(--lume-subtle-light); --association-accent: var(--lume-accent-strong); --association-soft: var(--lume-accent-soft-light); --association-scroll: var(--lume-scroll-light); width: min(440px, calc(100vw - 24px)); max-height: calc(100dvh - 24px); padding: 0; border: 1px solid var(--association-line); border-radius: 14px; background: var(--association-bg); color: var(--association-text); font: inherit; overflow: hidden; }
  .cli-association.dark { --association-bg: var(--lume-raised-dark); --association-text: var(--lume-ink-dark); --association-muted: var(--lume-ink-muted-dark); --association-line: var(--lume-line-dark); --association-subtle: var(--lume-subtle-dark); --association-accent: var(--lume-accent); --association-soft: var(--lume-accent-soft-dark); --association-scroll: var(--lume-scroll-dark); }
  .cli-association::backdrop { background: rgb(0 0 0 / .4); }
  .cli-association[open] { animation: association-appear 150ms cubic-bezier(.16, 1, .3, 1) both; }
  form { display: flex; flex-direction: column; gap: 14px; max-height: calc(100dvh - 26px); padding: 18px; box-sizing: border-box; }
  header { display: flex; gap: 12px; align-items: center; justify-content: space-between; }
  h2 { margin: 0; font-size: 17px; font-weight: 680; letter-spacing: -.02em; }
  p { margin: 0; font-size: 12px; line-height: 1.55; color: var(--association-muted); }
  button { border: 1px solid var(--association-line); border-radius: 8px; background: transparent; color: inherit; font: inherit; font-size: 12px; padding: 9px 12px; cursor: pointer; transition: background 130ms ease; }
  button:hover:not(:disabled) { background: var(--association-subtle); }
  button:disabled { cursor: default; opacity: .55; }
  .icon-button { border: 0; padding: 5px; display: grid; place-items: center; }
  .search { display: flex; align-items: center; gap: 8px; border: 1px solid var(--association-line); border-radius: 8px; padding: 8px 10px; color: var(--association-muted); }
  input { min-width: 0; width: 100%; border: 0; background: transparent; color: var(--association-text); font: inherit; font-size: 12px; caret-color: var(--association-accent); }
  input::placeholder { color: var(--association-muted); opacity: 1; }
  input:focus-visible { outline: none; }
  .search:focus-within, button:focus-visible { outline: 2px solid var(--association-accent); outline-offset: 2px; }
  .choices { min-height: 70px; max-height: min(320px, 42dvh); overflow: auto; overscroll-behavior: contain; scrollbar-width: thin; scrollbar-color: var(--association-scroll) transparent; }
  .choices::-webkit-scrollbar { width: 5px; }
  .choices::-webkit-scrollbar-thumb { background: var(--association-scroll); border-radius: 5px; }
  .choice { display: flex; justify-content: space-between; align-items: center; width: 100%; gap: 10px; border: 0; padding: 9px 10px; text-align: left; }
  .choice + .choice { margin-top: 3px; }
  .choice.selected { background: var(--association-soft); color: var(--association-accent); }
  .choice-copy { min-width: 0; display: grid; gap: 3px; }
  .choice strong { overflow-wrap: anywhere; font-size: 12px; font-weight: 580; }
  small { font-size: 10px; color: var(--association-muted); }
  .empty { padding: 12px 4px; }
  .hint { padding: 8px 10px; }
  .error { display: flex; align-items: flex-start; gap: 8px; flex-shrink: 0; max-height: 80px; overflow: auto; color: var(--association-text); background: var(--association-subtle); padding: 9px; border-radius: 8px; overflow-wrap: anywhere; }
  footer { display: flex; flex-shrink: 0; justify-content: flex-end; gap: 8px; }
  .primary { background: var(--association-accent); color: var(--association-bg); border-color: transparent; font-weight: 640; }
  .primary:hover:not(:disabled) { background: var(--association-accent); filter: brightness(.95); }
  ::selection { background: var(--association-soft); color: var(--association-text); }
  @keyframes association-appear { from { opacity: .6; transform: translateY(6px); } to { opacity: 1; transform: translateY(0); } }
  @media (prefers-reduced-motion: reduce) { .cli-association[open] { animation: none; } button { transition: none; } }
</style>
