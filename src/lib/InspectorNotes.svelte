<script lang="ts">
  import { onMount } from "svelte";
  import type { SessionNote } from "$lib/domain";
  import type { Language } from "$lib/i18n";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import { loadSessionNotes, saveSessionNote } from "$lib/lume";
  import { NOTES_CHANGED_EVENT, noteExcerpt, noteTitleFrom } from "$lib/notes";
  import type { PromptJumpTarget } from "$lib/tokenUsage";

  /** The Inspector's notes for one chat: its notes and plans, and a quick new note. */
  let { sessionId, language, onJump, onCount }: {
    sessionId: string;
    language: Language;
    /** Opens a note's prompt in the chat. */
    onJump?: (target: PromptJumpTarget) => void;
    onCount?: (count: number) => void;
  } = $props();

  let notes = $state<SessionNote[]>([]);
  let loading = $state(true);
  let writing = $state(false);
  let draft = $state("");
  let saving = $state(false);
  let error = $state("");
  let request = 0;

  function tr(english: string, portuguese: string) {
    return language === "pt-BR" ? portuguese : english;
  }

  async function load(id: string) {
    const current = ++request;
    try {
      const found = await loadSessionNotes(id);
      if (current !== request) return;
      notes = found;
      onCount?.(found.length);
    } catch (reason) {
      if (current === request) error = String(reason).replace(/^Error:\s*/, "");
    } finally {
      if (current === request) loading = false;
    }
  }

  $effect(() => {
    const id = sessionId;
    loading = true;
    error = "";
    void load(id);
  });

  onMount(() => {
    const changed = (event: Event) => { if ((event as CustomEvent<{ sessionId?: string }>).detail?.sessionId === sessionId) void load(sessionId); };
    window.addEventListener(NOTES_CHANGED_EVENT, changed);
    return () => window.removeEventListener(NOTES_CHANGED_EVENT, changed);
  });

  async function save() {
    const body = draft.trim();
    if (!body || saving) return;
    saving = true;
    error = "";
    try {
      const saved = await saveSessionNote(sessionId, { title: noteTitleFrom(body, tr("Note", "Nota")), body, kind: "note", pinned: false });
      notes = [saved, ...notes];
      onCount?.(notes.length);
      draft = "";
      writing = false;
    } catch (reason) {
      error = String(reason).replace(/^Error:\s*/, "");
    } finally {
      saving = false;
    }
  }
</script>

<div class="inspector-notes">
  {#if loading && !notes.length}
    <div class="notes-loading" aria-hidden="true"><i></i><i></i></div>
  {:else}
    <ul>
      {#each notes.slice(0, 8) as note (note.id)}
        <li>
          {#if note.promptActivityId && onJump}
            <button type="button" title={tr("Open its prompt in the chat", "Abrir o prompt no chat")} onclick={() => onJump?.({ activityId: note.promptActivityId })}>
              <strong>{note.title}</strong><small>{noteExcerpt(note.body, 80)}</small>
            </button>
          {:else}
            <div><strong>{note.title}</strong><small>{noteExcerpt(note.body, 80)}</small></div>
          {/if}
          {#if note.kind === "plan"}<b>{tr("Plan", "Plano")}</b>{:else if note.pinned}<i class="pin" title={tr("Pinned", "Fixada")}></i>{/if}
        </li>
      {:else}
        <li class="empty">{tr("No notes for this chat yet.", "Nenhuma nota neste chat ainda.")}</li>
      {/each}
    </ul>
  {/if}
  {#if error}<p class="notes-error" role="alert">{error}</p>{/if}
  {#if writing}
    <textarea bind:value={draft} rows="4" placeholder={tr("Write a note in Markdown…", "Escreva uma nota em Markdown…")} aria-label={tr("New note", "Nova nota")}></textarea>
    <div class="note-actions">
      <button type="button" class="ghost" onclick={() => { writing = false; draft = ""; }}>{tr("Cancel", "Cancelar")}</button>
      <button type="button" class="primary" disabled={!draft.trim() || saving} onclick={() => void save()}>{saving ? tr("Saving…", "Salvando…") : tr("Save note", "Salvar nota")}</button>
    </div>
  {:else}
    <button type="button" class="new-note" onclick={() => (writing = true)}><LumeIcon name="plus" size={12} /><span>{tr("New note", "Nova nota")}</span></button>
  {/if}
</div>

<style>
  .inspector-notes { display: grid; gap: 6px; }
  ul { margin: 0; padding: 0; display: grid; gap: 1px; list-style: none; }
  li { min-width: 0; display: flex; align-items: center; gap: 6px; }
  li > button, li > div { min-width: 0; flex: 1; padding: 5px 6px; display: grid; gap: 2px; border: 0; border-radius: 7px; color: var(--workspace-text); background: transparent; font: inherit; text-align: left; }
  li > button { cursor: pointer; }
  li > button:hover { background: var(--workspace-subtle); }
  li strong { overflow: hidden; color: var(--workspace-strong); font-size: 11px; font-weight: 650; text-overflow: ellipsis; white-space: nowrap; }
  li small { overflow: hidden; color: var(--workspace-faint); font-size: 10px; text-overflow: ellipsis; white-space: nowrap; }
  li b { flex: 0 0 auto; padding: 1px 5px; border-radius: 5px; color: #315f86; background: #cbdff0; font-size: 9px; font-weight: 720; }
  :global(.dark) li b { color: #b4d3ee; background: #29445d; }
  .pin { width: 6px; height: 6px; margin-right: 4px; flex: 0 0 auto; border-radius: 50%; background: var(--workspace-accent); }
  li.empty { padding: 2px 0 4px; color: var(--workspace-faint); font-size: 10px; }
  textarea { width: 100%; box-sizing: border-box; padding: 7px 9px; resize: vertical; border: 1px solid var(--workspace-line); border-radius: 8px; outline: 0; color: var(--workspace-text); background: var(--workspace-raised); font: 11px/1.5 var(--lume-font-ui, Inter, sans-serif); }
  textarea:focus { border-color: color-mix(in srgb, var(--workspace-accent) 45%, var(--workspace-line)); }
  .note-actions { display: flex; justify-content: flex-end; gap: 6px; }
  .new-note, .ghost, .primary { min-height: 26px; padding: 0 8px; display: inline-flex; align-items: center; gap: 5px; border: 1px solid var(--workspace-line); border-radius: 7px; color: var(--workspace-muted); background: transparent; font-size: 10px; font-weight: 700; cursor: pointer; }
  .new-note { justify-self: start; color: var(--workspace-accent); border-color: color-mix(in srgb, var(--workspace-accent) 28%, var(--workspace-line)); background: var(--workspace-accent-soft); }
  .primary { color: var(--workspace-pane); background: var(--workspace-accent); border-color: transparent; }
  .primary:disabled { opacity: .45; cursor: default; }
  .notes-error { margin: 0; color: #c0554f; font-size: 10px; }
  .notes-loading { display: grid; gap: 8px; padding: 4px 0; }
  .notes-loading i { height: 11px; border-radius: 6px; background: var(--workspace-line); opacity: .5; }
  .notes-loading i:nth-child(2) { width: 70%; }
</style>
