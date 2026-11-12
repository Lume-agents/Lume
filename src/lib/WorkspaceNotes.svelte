<script lang="ts">
  import { onMount, tick } from "svelte";
  import { fade } from "svelte/transition";
  import type { SessionNote } from "$lib/domain";
  import type { Language } from "$lib/i18n";
  import BrandIcon from "$lib/BrandIcon.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import LumeSelect, { type LumeSelectOption } from "$lib/LumeSelect.svelte";
  import { renderSafeMarkdown } from "$lib/markdown.js";
  import { deleteSessionNote, listNotes, saveSessionNote } from "$lib/lume";
  import { filterNotes, noteExcerpt, noteTags, parseNoteTags, projectLabel } from "$lib/notes";

  /** The Notes tab: every note and saved plan, by project, agent and tag, with a markdown editor. */
  let { language = "en", project, canOpenLinked, onOpenLinked, onClose } = $props<{
    language?: Language;
    /** The focused chat's project, chosen first and given to new notes. */
    project?: string;
    /** Whether the chat a note came from is still in the workspace. */
    canOpenLinked?: (note: SessionNote) => boolean;
    /** Opens the chat a note came from, at its prompt when it has one. */
    onOpenLinked?: (note: SessionNote) => void;
    onClose: () => void;
  }>();

  let notes = $state<SessionNote[]>([]);
  let allProjects = $state<string[]>([]);
  let loading = $state(true);
  let error = $state("");
  let query = $state("");
  let kind = $state<"" | "note" | "plan">("");
  // svelte-ignore state_referenced_locally
  let projectFilter = $state(project ?? "");
  let agentFilter = $state("");
  let tagFilter = $state("");
  let selectedId = $state<string | null>(null);
  let draft = $state<{ id?: string; title: string; body: string; tags: string; pinned: boolean; kind: SessionNote["kind"] } | null>(null);
  let previewing = $state(false);
  let saving = $state(false);
  let titleInput = $state<HTMLInputElement | null>(null);
  let request = 0;

  const shown = $derived(filterNotes(notes, { agent: agentFilter, tag: tagFilter }));
  const selected = $derived(notes.find((note) => note.id === selectedId) ?? null);
  const tags = $derived(noteTags(notes));
  const agents = $derived([...new Set(notes.map((note) => note.agent).filter((agent): agent is NonNullable<typeof agent> => Boolean(agent)))]);
  const projectOptions = $derived<LumeSelectOption[]>([
    { value: "", label: tr("All projects", "Todos os projetos") },
    ...allProjects.map((value) => ({ value, label: projectLabel(value) })),
  ]);
  const agentOptions = $derived<LumeSelectOption[]>([
    { value: "", label: tr("All agents", "Todos os agentes") },
    ...agents.map((value) => ({ value, label: agentLabel(value) })),
  ]);
  const dirty = $derived(Boolean(draft && (!selected
    ? draft.body.trim() || draft.title.trim()
    : draft.title !== selected.title || draft.body !== selected.body || draft.tags !== (selected.tags ?? []).join(", ") || draft.pinned !== selected.pinned)));

  function tr(english: string, portuguese: string) {
    return language === "pt-BR" ? portuguese : english;
  }
  function agentLabel(agent: string) {
    return ({ claude_code: "Claude Code", codex: "Codex", opencode: "OpenCode", gemini: "Gemini", omp: "Oh My Pi" } as Record<string, string>)[agent] ?? agent;
  }
  const date = (value: number) => new Date(value).toLocaleString(language, { day: "2-digit", month: "short", hour: "2-digit", minute: "2-digit" });

  async function load() {
    const current = ++request;
    loading = true;
    error = "";
    try {
      const [found, every] = await Promise.all([
        listNotes({ project: projectFilter, query: query.trim(), kind: kind || undefined }),
        allProjects.length ? Promise.resolve(null) : listNotes(),
      ]);
      if (current !== request) return;
      notes = found;
      if (every) allProjects = [...new Set([...(project ? [project] : []), ...every.map((note) => note.project).filter((value): value is string => Boolean(value))])];
      if (selectedId && !found.some((note) => note.id === selectedId) && !draft?.id) selectedId = null;
    } catch (reason) {
      if (current === request) error = String(reason).replace(/^Error:\s*/, "");
    } finally {
      if (current === request) loading = false;
    }
  }

  // The backend narrows by project, kind and text; the list reloads as they change.
  let searchTimer = 0;
  $effect(() => {
    void projectFilter; void kind; void query;
    window.clearTimeout(searchTimer);
    searchTimer = window.setTimeout(() => void load(), query ? 180 : 0);
    return () => window.clearTimeout(searchTimer);
  });

  function open(note: SessionNote) {
    if (dirty && !window.confirm(tr("Discard the unsaved changes?", "Descartar as alterações não salvas?"))) return;
    selectedId = note.id;
    previewing = true;
    draft = { id: note.id, title: note.title, body: note.body, tags: (note.tags ?? []).join(", "), pinned: note.pinned, kind: note.kind };
  }

  async function create() {
    if (dirty && !window.confirm(tr("Discard the unsaved changes?", "Descartar as alterações não salvas?"))) return;
    selectedId = null;
    previewing = false;
    draft = { title: "", body: "", tags: tagFilter, pinned: false, kind: "note" };
    await tick();
    titleInput?.focus();
  }

  async function save() {
    if (!draft || !draft.body.trim() || saving) return;
    saving = true;
    error = "";
    try {
      const saved = await saveSessionNote(null, { id: draft.id, title: draft.title, body: draft.body, kind: draft.kind, pinned: draft.pinned }, {
        tags: parseNoteTags(draft.tags),
        ...(draft.id ? {} : { project: projectFilter || project || "" }),
      });
      selectedId = saved.id;
      draft = { id: saved.id, title: saved.title, body: saved.body, tags: (saved.tags ?? []).join(", "), pinned: saved.pinned, kind: saved.kind };
      notes = [saved, ...notes.filter((note) => note.id !== saved.id)];
      if (saved.project && !allProjects.includes(saved.project)) allProjects = [...allProjects, saved.project];
      previewing = true;
    } catch (reason) {
      error = String(reason).replace(/^Error:\s*/, "");
    } finally {
      saving = false;
    }
  }

  async function togglePin(note: SessionNote) {
    try {
      const saved = await saveSessionNote(null, { ...note, pinned: !note.pinned });
      notes = notes.map((item) => (item.id === saved.id ? saved : item)).sort((left, right) => Number(right.pinned) - Number(left.pinned) || right.updatedAt - left.updatedAt);
      if (draft?.id === saved.id) draft = { ...draft, pinned: saved.pinned };
    } catch (reason) {
      error = String(reason).replace(/^Error:\s*/, "");
    }
  }

  async function remove(note: SessionNote) {
    if (!window.confirm(tr(`Delete "${note.title}"? This cannot be undone.`, `Excluir "${note.title}"? Isso não pode ser desfeito.`))) return;
    try {
      await deleteSessionNote(note.id);
      notes = notes.filter((item) => item.id !== note.id);
      if (selectedId === note.id) { selectedId = null; draft = null; }
    } catch (reason) {
      error = String(reason).replace(/^Error:\s*/, "");
    }
  }

  function keydown(event: KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s" && draft) { event.preventDefault(); void save(); }
  }

  onMount(() => {
    window.addEventListener("keydown", keydown);
    return () => window.removeEventListener("keydown", keydown);
  });
</script>

<section class="notes" aria-label={tr("Notes", "Notas")} in:fade={{ duration: 160 }} out:fade={{ duration: 110 }}>
  <header class="notes-bar">
    <span class="notes-title"><span class="title-mark"><LumeIcon name="note" size={16} /></span><strong>{tr("Notes", "Notas")}</strong></span>
    <span class="notes-tabs" role="tablist" aria-label={tr("Kind", "Tipo")}>
      <button class:on={kind === ""} type="button" role="tab" aria-selected={kind === ""} onclick={() => (kind = "")}>{tr("All", "Todas")}</button>
      <button class:on={kind === "note"} type="button" role="tab" aria-selected={kind === "note"} onclick={() => (kind = "note")}>{tr("Notes", "Notas")}</button>
      <button class:on={kind === "plan"} type="button" role="tab" aria-selected={kind === "plan"} onclick={() => (kind = "plan")}>{tr("Plans", "Planos")}</button>
    </span>
    <label class="notes-search">
      <LumeIcon name="search" size={14} />
      <input bind:value={query} type="search" placeholder={tr("Search notes", "Buscar notas")} aria-label={tr("Search notes", "Buscar notas")} />
    </label>
    <button class="new-note" type="button" onclick={() => void create()}><LumeIcon name="plus" size={14} /><span>{tr("New note", "Nova nota")}</span></button>
    <button class="bar-icon" type="button" title={tr("Close", "Fechar")} aria-label={tr("Close notes", "Fechar notas")} onclick={onClose}><LumeIcon name="close" size={15} /></button>
  </header>

  <aside class="notes-list">
    <div class="filters">
      <LumeSelect value={projectFilter} options={projectOptions} ariaLabel={tr("Project", "Projeto")} minWidth={0} onValueChange={(value) => (projectFilter = value)} />
      {#if agents.length > 1}
        <LumeSelect value={agentFilter} options={agentOptions} ariaLabel={tr("Agent", "Agente")} minWidth={0} onValueChange={(value) => (agentFilter = value)} />
      {/if}
      {#if tags.length}
        <div class="tag-filter" role="group" aria-label={tr("Tags", "Tags")}>
          {#each tags.slice(0, 14) as tag (tag)}
            <button type="button" class:on={tagFilter === tag} aria-pressed={tagFilter === tag} onclick={() => (tagFilter = tagFilter === tag ? "" : tag)}>#{tag}</button>
          {/each}
        </div>
      {/if}
    </div>
    <ul aria-busy={loading}>
      {#each shown as note (note.id)}
        <li>
          <button class="note-row" class:on={note.id === selectedId} type="button" onclick={() => open(note)}>
            <span class="row-top">
              {#if note.agent}<BrandIcon name={note.agent as never} size={13} />{/if}
              <strong>{note.title}</strong>
              {#if note.pinned}<i class="pin" title={tr("Pinned", "Fixada")}></i>{/if}
            </span>
            <small>{noteExcerpt(note.body, 90)}</small>
            <span class="row-meta">
              {#if note.kind === "plan"}<b class="kind-plan">{tr("Plan", "Plano")}</b>{/if}
              {#if note.project}<em>{projectLabel(note.project)}</em>{/if}
              {#each (note.tags ?? []).slice(0, 3) as tag (tag)}<em class="tag">#{tag}</em>{/each}
              <time>{date(note.updatedAt)}</time>
            </span>
          </button>
        </li>
      {:else}
        <li class="empty">{loading ? tr("Loading…", "Carregando…") : query || tagFilter || agentFilter || kind ? tr("No notes match.", "Nenhuma nota corresponde.") : tr("No notes yet. Save one from a chat or create it here.", "Nenhuma nota ainda. Salve uma a partir de um chat ou crie aqui.")}</li>
      {/each}
    </ul>
  </aside>

  <div class="notes-editor">
    {#if error}<p class="notes-error" role="alert">{error}</p>{/if}
    {#if draft}
      <div class="editor-head">
        <input bind:this={titleInput} bind:value={draft.title} class="title-input" maxlength="120" placeholder={tr("Title", "Título")} aria-label={tr("Title", "Título")} />
        <span class="mode-toggle" role="group" aria-label={tr("View", "Visualização")}>
          <button type="button" class:on={!previewing} aria-pressed={!previewing} onclick={() => (previewing = false)}>{tr("Edit", "Editar")}</button>
          <button type="button" class:on={previewing} aria-pressed={previewing} onclick={() => (previewing = true)}>{tr("Preview", "Visualizar")}</button>
        </span>
      </div>
      <input bind:value={draft.tags} class="tags-input" placeholder={tr("Tags, separated by commas", "Tags, separadas por vírgulas")} aria-label={tr("Tags", "Tags")} />
      {#if selected && (selected.promptExcerpt || selected.nativeSessionId)}
        <div class="linked">
          <LumeIcon name="attachment" size={13} />
          <span>{selected.promptExcerpt ? tr(`From the prompt “${selected.promptExcerpt}”`, `Do prompt “${selected.promptExcerpt}”`) : tr("Saved from a chat", "Salva a partir de um chat")}</span>
          {#if onOpenLinked && canOpenLinked?.(selected)}
            <button type="button" onclick={() => onOpenLinked?.(selected)}>{selected.promptActivityId ? tr("Open linked prompt", "Abrir prompt vinculado") : tr("Open linked chat", "Abrir chat vinculado")}</button>
          {/if}
        </div>
      {/if}
      {#if previewing}
        <div class="markdown preview" aria-label={tr("Preview", "Visualização")}>
          {#if draft.body.trim()}{@html renderSafeMarkdown(draft.body)}{:else}<p class="placeholder">{tr("Nothing written yet.", "Nada escrito ainda.")}</p>{/if}
        </div>
      {:else}
        <textarea bind:value={draft.body} class="body-input" spellcheck="true" placeholder={tr("Write in Markdown…", "Escreva em Markdown…")} aria-label={tr("Note", "Nota")}></textarea>
      {/if}
      <footer class="editor-actions">
        {#if selected}
          <button type="button" class="ghost" onclick={() => void togglePin(selected)}>{selected.pinned ? tr("Unpin", "Desafixar") : tr("Pin", "Fixar")}</button>
          <button type="button" class="ghost danger" onclick={() => void remove(selected)}><LumeIcon name="trash" size={13} /><span>{tr("Delete", "Excluir")}</span></button>
          <small>{tr("Updated", "Atualizada")} {date(selected.updatedAt)}</small>
        {:else}
          <small>{projectFilter || project ? tr(`New note in ${projectLabel(projectFilter || project)}`, `Nova nota em ${projectLabel(projectFilter || project)}`) : tr("New note", "Nova nota")}</small>
        {/if}
        <button type="button" class="primary" disabled={!draft.body.trim() || saving || !dirty} onclick={() => void save()}>{saving ? tr("Saving…", "Salvando…") : tr("Save", "Salvar")}</button>
      </footer>
    {:else}
      <div class="editor-empty"><LumeIcon name="note" size={26} /><span>{tr("Choose a note or create a new one.", "Escolha uma nota ou crie uma nova.")}</span></div>
    {/if}
  </div>
</section>

<style>
  .notes { position: absolute; z-index: 31; inset: 0; display: grid; grid-template-columns: minmax(250px, 320px) minmax(0, 1fr); grid-template-rows: auto minmax(0, 1fr); overflow: hidden; color: var(--workspace-text); background: var(--workspace-bg); isolation: isolate; }
  .notes-bar { grid-column: 1 / -1; min-width: 0; height: 56px; padding: 0 14px 0 18px; display: flex; align-items: center; gap: 12px; border-bottom: 1px solid var(--workspace-line); }
  .notes-title { display: flex; align-items: center; gap: 9px; color: var(--workspace-strong); }
  .notes-title strong { font-size: 14px; font-weight: 720; letter-spacing: -.01em; }
  .title-mark { width: 28px; height: 28px; display: grid; place-items: center; border-radius: 8px; color: var(--workspace-accent); background: var(--workspace-accent-soft); }
  .notes-tabs, .mode-toggle { display: inline-flex; gap: 2px; padding: 2px; border-radius: 9px; background: var(--workspace-subtle); }
  .notes-tabs button, .mode-toggle button { height: 26px; padding: 0 11px; border: 0; border-radius: 7px; color: var(--workspace-muted); background: transparent; font: 650 11px/1 var(--lume-font-ui, Inter, sans-serif); cursor: pointer; }
  .notes-tabs button.on, .mode-toggle button.on { color: var(--workspace-strong); background: var(--workspace-raised); box-shadow: 0 1px 3px rgba(20, 38, 30, .12); }
  .notes-search { min-width: 0; max-width: 340px; height: 32px; margin-left: auto; padding: 0 10px; display: flex; flex: 1; align-items: center; gap: 7px; border: 1px solid var(--workspace-line); border-radius: 9px; color: var(--workspace-faint); background: var(--workspace-raised); }
  .notes-search:focus-within { border-color: color-mix(in srgb, var(--workspace-accent) 50%, var(--workspace-line)); }
  .notes-search input { min-width: 0; flex: 1; border: 0; outline: 0; color: var(--workspace-strong); background: transparent; font: 12px var(--lume-font-ui, Inter, sans-serif); }
  .new-note, .primary { height: 30px; padding: 0 11px; display: inline-flex; align-items: center; gap: 6px; border: 1px solid color-mix(in srgb, var(--workspace-accent) 30%, var(--workspace-line)); border-radius: 8px; color: var(--workspace-accent); background: var(--workspace-accent-soft); font: 700 11px/1 var(--lume-font-ui, Inter, sans-serif); cursor: pointer; }
  .primary { margin-left: auto; color: var(--workspace-pane); background: var(--workspace-accent); border-color: transparent; }
  .primary:disabled { opacity: .45; cursor: default; }
  .bar-icon { width: 30px; height: 30px; padding: 0; display: grid; place-items: center; flex: 0 0 auto; border: 0; border-radius: 8px; color: var(--workspace-muted); background: transparent; cursor: pointer; }
  .bar-icon:hover { color: var(--workspace-accent); background: var(--workspace-subtle); }
  :is(.notes button, .notes input, .notes textarea):focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 1px; }

  .notes-list { min-height: 0; display: grid; grid-template-rows: auto minmax(0, 1fr); border-right: 1px solid var(--workspace-line); }
  .filters { padding: 12px 12px 8px; display: grid; gap: 7px; border-bottom: 1px solid var(--workspace-line); }
  .tag-filter { display: flex; flex-wrap: wrap; gap: 4px; }
  .tag-filter button { height: 22px; padding: 0 7px; border: 1px solid var(--workspace-line); border-radius: 999px; color: var(--workspace-muted); background: transparent; font: 650 10px/1 var(--lume-font-ui, Inter, sans-serif); cursor: pointer; }
  .tag-filter button.on { color: var(--workspace-accent); border-color: color-mix(in srgb, var(--workspace-accent) 45%, var(--workspace-line)); background: var(--workspace-accent-soft); }
  .notes-list ul { min-height: 0; margin: 0; padding: 8px; display: flex; flex-direction: column; gap: 2px; overflow-y: auto; list-style: none; }
  .note-row { width: 100%; padding: 8px 9px; display: grid; gap: 4px; border: 0; border-radius: 9px; color: var(--workspace-text); background: transparent; text-align: left; cursor: pointer; }
  .note-row:hover { background: var(--workspace-subtle); }
  .note-row.on { background: var(--workspace-accent-soft); }
  .row-top { min-width: 0; display: flex; align-items: center; gap: 6px; }
  .row-top strong { min-width: 0; flex: 1; overflow: hidden; color: var(--workspace-strong); font: 680 12px/1.25 var(--lume-font-ui, Inter, sans-serif); text-overflow: ellipsis; white-space: nowrap; }
  .pin { width: 6px; height: 6px; flex: 0 0 auto; border-radius: 50%; background: var(--workspace-accent); }
  .note-row small { overflow: hidden; color: var(--workspace-muted); font-size: 10.5px; line-height: 1.35; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; }
  .row-meta { min-width: 0; display: flex; flex-wrap: wrap; align-items: center; gap: 5px; color: var(--workspace-faint); font-size: 10px; }
  .row-meta em { font-style: normal; }
  .row-meta .tag { color: var(--workspace-accent); }
  .row-meta b.kind-plan { padding: 1px 5px; border-radius: 5px; color: #315f86; background: #cbdff0; font-weight: 720; }
  :global(.workspace.dark) .row-meta b.kind-plan, :global(.dark) .row-meta b.kind-plan { color: #b4d3ee; background: #29445d; }
  .row-meta time { margin-left: auto; }
  .notes-list li.empty { padding: 18px 10px; color: var(--workspace-faint); font-size: 11px; line-height: 1.45; text-align: center; }

  .notes-editor { min-width: 0; min-height: 0; padding: 18px 22px 16px; display: flex; flex-direction: column; gap: 9px; }
  .editor-head { display: flex; align-items: center; gap: 10px; }
  .notes .title-input:focus-visible { outline: none; }
  .title-input { min-width: 0; flex: 1; padding: 4px 0; border: 0; outline: 0; color: var(--workspace-strong); background: transparent; font: 720 18px/1.25 var(--lume-font-ui, Inter, sans-serif); letter-spacing: -.015em; }
  .tags-input { height: 28px; padding: 0 9px; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-text); background: var(--workspace-raised); font: 11px var(--lume-font-ui, Inter, sans-serif); outline: 0; }
  .linked { display: flex; align-items: center; gap: 7px; padding: 6px 9px; border-radius: 8px; color: var(--workspace-muted); background: var(--workspace-subtle); font-size: 11px; }
  .linked span { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .linked button { flex: 0 0 auto; padding: 0; border: 0; color: var(--workspace-accent); background: transparent; font: 700 11px/1 var(--lume-font-ui, Inter, sans-serif); cursor: pointer; }
  .body-input, .preview { min-height: 0; flex: 1; padding: 12px 14px; border: 1px solid var(--workspace-line); border-radius: 10px; background: var(--workspace-pane); overflow-y: auto; }
  .body-input { resize: none; outline: 0; color: var(--workspace-text); font: 12.5px/1.6 var(--lume-font-mono, ui-monospace, monospace); }
  .body-input:focus { border-color: color-mix(in srgb, var(--workspace-accent) 45%, var(--workspace-line)); }
  .markdown { color: var(--workspace-text); font: 500 12.5px/1.6 var(--lume-font-ui, Inter, sans-serif); overflow-wrap: anywhere; }
  .markdown :global(:is(h1, h2, h3, h4)) { margin: 14px 0 6px; color: var(--workspace-strong); font-size: 13.5px; line-height: 1.3; }
  .markdown :global(:is(h1, h2, h3, h4):first-child) { margin-top: 0; }
  .markdown :global(p), .markdown :global(ul), .markdown :global(ol) { margin: 0 0 8px; }
  .markdown :global(ul), .markdown :global(ol) { padding-left: 18px; }
  .markdown :global(code) { padding: 1px 4px; border-radius: 4px; background: var(--workspace-subtle); font-size: 11.5px; }
  .markdown :global(pre) { padding: 9px 11px; border-radius: 8px; background: var(--workspace-subtle); overflow-x: auto; }
  .markdown :global(pre code) { padding: 0; background: transparent; }
  .markdown :global(a) { color: var(--workspace-accent); }
  .placeholder { color: var(--workspace-faint); }
  .editor-actions { display: flex; align-items: center; gap: 8px; }
  .editor-actions small { color: var(--workspace-faint); font-size: 10.5px; }
  .ghost { height: 28px; padding: 0 9px; display: inline-flex; align-items: center; gap: 5px; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-muted); background: transparent; font: 650 11px/1 var(--lume-font-ui, Inter, sans-serif); cursor: pointer; }
  .ghost:hover { color: var(--workspace-strong); background: var(--workspace-subtle); }
  .ghost.danger:hover { color: #c0554f; }
  .notes-error { margin: 0; padding: 7px 10px; border-radius: 8px; color: #a8463f; background: color-mix(in srgb, #c0554f 12%, transparent); font-size: 11px; }
  .editor-empty { margin: auto; display: grid; justify-items: center; gap: 10px; color: var(--workspace-faint); font-size: 11.5px; }
</style>
