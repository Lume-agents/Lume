<script lang="ts">
  import { onMount } from "svelte";
  import FileTypeIcon from "$lib/FileTypeIcon.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import LumeSelect from "$lib/LumeSelect.svelte";
  import SystemBannerStack, { type SystemBannerItem } from "$lib/SystemBannerStack.svelte";
  import type { HubSession } from "$lib/hubProtocol";
  import type { ReviewNote, WorkflowHistoryRecord } from "$lib/domain";
  import type { Language } from "$lib/i18n";
  import { displayFileChangePath } from "$lib/fileChanges";
  import { buildReviewTurns, parseReviewDiff, type ReviewDiffLine } from "$lib/reviewDiffs";
  import { deleteReviewNote, loadReviewNotes, loadWorkflowHistory, saveReviewNote } from "$lib/lume";

  type DiffMode = "unified" | "split";
  type SplitRow = { left?: ReviewDiffLine; right?: ReviewDiffLine };

  let { session, language = "en", initialPath, onClose } = $props<{
    session: HubSession;
    language?: Language;
    initialPath?: string;
    onClose: () => void;
  }>();

  let mode = $state<DiffMode>("unified");
  let selectedPath = $state("");
  let selectedTurnId = $state("");
  let previousTurnId = $state("");
  let previousInitialPath = $state("");
  let workflowHistory = $state<WorkflowHistoryRecord[]>([]);
  let reviewNotes = $state<ReviewNote[]>([]);
  let notesOpen = $state(false);
  let noteDraft = $state("");
  let noteDraftResultId = $state("");
  let noteDirty = $state(false);
  let noteSaving = $state(false);
  let reviewMessage = $state("");
  let reviewMessageIsError = $state(false);
  let notesLoadToken = 0;
  const turns = $derived(buildReviewTurns(session.activities, session.results, session.workingDirectory));
  const selectedTurn = $derived(turns.find((turn) => turn.id === selectedTurnId) ?? turns[0] ?? null);
  const files = $derived(selectedTurn?.files ?? []);
  const selectedFile = $derived(files.find((file) => file.path === selectedPath) ?? files[0] ?? null);
  const diffLines = $derived(selectedFile?.diff ? parseReviewDiff(selectedFile.diff) : []);
  const splitRows = $derived(buildSplitRows(diffLines));
  const workflowLink = $derived.by(() => {
    const resultId = selectedTurn?.result?.id;
    if (!resultId) return null;
    for (const record of workflowHistory) {
      const step = record.run.steps.find((candidate) => candidate.resultId === resultId);
      if (!step) continue;
      const captured = record.steps.find((candidate) => candidate.stepId === step.stepId);
      return { objective: record.run.objective, role: captured?.roleLabel ?? tr("Workflow step", "Etapa do workflow") };
    }
    return null;
  });
  const selectedResultId = $derived(selectedTurn?.result?.id ?? "");
  const selectedReviewNote = $derived(reviewNotes.find((note) => note.resultId === selectedResultId) ?? null);
  const systemBanners = $derived.by<SystemBannerItem[]>(() => reviewMessage ? [{
    id: "review-message",
    message: reviewMessage,
    tone: reviewMessageIsError ? "error" : "success",
    onDismiss: () => { reviewMessage = ""; },
  }] : []);

  onMount(() => {
    let mounted = true;
    void loadWorkflowHistory(30).then((history) => {
      if (mounted) workflowHistory = history;
    }).catch(() => {});
    return () => { mounted = false; };
  });

  $effect(() => {
    const sessionId = session.id;
    const token = ++notesLoadToken;
    reviewNotes = [];
    noteDraft = "";
    noteDraftResultId = "";
    noteDirty = false;
    void loadReviewNotes(sessionId)
      .then((notes) => {
        if (token === notesLoadToken && session.id === sessionId) reviewNotes = notes;
      })
      .catch((error) => {
        if (token !== notesLoadToken || session.id !== sessionId) return;
        reviewMessageIsError = true;
        reviewMessage = String(error).replace(/^Error:\s*/, "");
      });
  });

  $effect(() => {
    const resultId = selectedResultId;
    const persistedBody = selectedReviewNote?.body ?? "";
    if (noteDraftResultId !== resultId || (!noteDirty && noteDraft !== persistedBody)) {
      noteDraftResultId = resultId;
      noteDraft = persistedBody;
      noteDirty = false;
    }
  });

  $effect(() => {
    const turnId = selectedTurn?.id ?? "";
    const requestedPath = initialPath ?? "";
    if (turnId !== previousTurnId) {
      previousTurnId = turnId;
      previousInitialPath = requestedPath;
      selectedPath = initialPath && files.some((file) => file.path === initialPath)
        ? initialPath
        : files[0]?.path ?? "";
      return;
    }
    if (requestedPath && requestedPath !== previousInitialPath && files.some((file) => file.path === requestedPath)) {
      previousInitialPath = requestedPath;
      selectedPath = requestedPath;
      return;
    }
    previousInitialPath = requestedPath;
    if (!files.length) {
      selectedPath = "";
      return;
    }
    if (!files.some((file) => file.path === selectedPath)) selectedPath = files[0].path;
  });

  function tr(english: string, portuguese: string) {
    return language === "pt-BR" ? portuguese : english;
  }

  function turnLabel(turn: (typeof turns)[number]) {
    const time = new Intl.DateTimeFormat(language, { hour: "2-digit", minute: "2-digit" }).format(new Date(turn.createdAt));
    const prompt = turn.prompt?.detail?.trim().split(/\r?\n/, 1)[0];
    const subject = prompt || (turn.result ? tr("Archived result", "Resultado arquivado") : tr("Current turn", "Turno atual"));
    return `${time} · ${subject.slice(0, 78)}`;
  }

  function buildSplitRows(lines: ReviewDiffLine[]): SplitRow[] {
    const rows: SplitRow[] = [];
    let removed: ReviewDiffLine[] = [];
    let added: ReviewDiffLine[] = [];
    const flush = () => {
      const size = Math.max(removed.length, added.length);
      for (let index = 0; index < size; index += 1) rows.push({ left: removed[index], right: added[index] });
      removed = [];
      added = [];
    };
    for (const line of lines) {
      if (line.kind === "removed") {
        removed.push(line);
      } else if (line.kind === "added") {
        added.push(line);
      } else {
        flush();
        rows.push({ left: line, right: line });
      }
    }
    flush();
    return rows;
  }

  function lineLabel(line?: ReviewDiffLine, side: "left" | "right" = "left") {
    if (!line || line.kind === "meta" || line.kind === "hunk") return "";
    return side === "left" ? line.oldLine ?? "" : line.newLine ?? "";
  }

  async function persistReviewNote(): Promise<boolean> {
    if (!selectedResultId || noteSaving) return !noteSaving;
    const body = noteDraft.trim();
    if (!body) {
      if (selectedReviewNote) return removeReviewNote();
      noteDirty = false;
      return true;
    }
    noteSaving = true;
    try {
      const saved = await saveReviewNote(session.id, selectedResultId, body);
      reviewNotes = [saved, ...reviewNotes.filter((note) => note.resultId !== saved.resultId)];
      noteDraft = saved.body;
      noteDirty = false;
      reviewMessageIsError = false;
      reviewMessage = tr("Review note saved", "Nota de revisão salva");
      return true;
    } catch (error) {
      reviewMessageIsError = true;
      reviewMessage = String(error).replace(/^Error:\s*/, "");
      return false;
    } finally {
      noteSaving = false;
    }
  }

  async function removeReviewNote(): Promise<boolean> {
    if (!selectedResultId || noteSaving) return false;
    noteSaving = true;
    try {
      await deleteReviewNote(session.id, selectedResultId);
      reviewNotes = reviewNotes.filter((note) => note.resultId !== selectedResultId);
      noteDraft = "";
      noteDirty = false;
      reviewMessageIsError = false;
      reviewMessage = tr("Review note removed", "Nota de revisão removida");
      return true;
    } catch (error) {
      reviewMessageIsError = true;
      reviewMessage = String(error).replace(/^Error:\s*/, "");
      return false;
    } finally {
      noteSaving = false;
    }
  }

  async function selectTurn(turnId: string) {
    if (noteDirty && !(await persistReviewNote())) return;
    selectedTurnId = turnId;
  }

  async function closeReviewCenter() {
    if (noteDirty && !(await persistReviewNote())) return;
    onClose();
  }
</script>

<section class="review-center" aria-label={tr("Review center", "Central de revisão")}>
  <SystemBannerStack items={systemBanners} contained dismissLabel={tr("Dismiss", "Fechar")} />
  <header class="review-header">
    <span class="review-title">
      <i><LumeIcon name="diff" size={16} /></i>
      <span><strong>{tr("Review", "Revisão")}</strong><small>{files.length} {tr(files.length === 1 ? "changed file" : "changed files", files.length === 1 ? "arquivo alterado" : "arquivos alterados")}</small></span>
    </span>
    <div class="mode-switch" role="group" aria-label={tr("Diff layout", "Layout do diff")}>
      <button class:active={mode === "unified"} type="button" aria-pressed={mode === "unified"} onclick={() => mode = "unified"}><LumeIcon name="diff" size={13} /><span>{tr("Unified", "Unificado")}</span></button>
      <button class:active={mode === "split"} type="button" aria-pressed={mode === "split"} onclick={() => mode = "split"}><LumeIcon name="columns" size={13} /><span>{tr("Split", "Lado a lado")}</span></button>
    </div>
    <button class:active={notesOpen} class="review-note-toggle" type="button" title={tr("Review note", "Nota de revisão")} aria-label={tr("Review note", "Nota de revisão")} aria-pressed={notesOpen} onclick={() => notesOpen = !notesOpen}><LumeIcon name="note" size={15} />{#if selectedReviewNote}<i aria-hidden="true"></i>{/if}</button>
    <button class="close-review" type="button" title={tr("Close review", "Fechar revisão")} aria-label={tr("Close review", "Fechar revisão")} onclick={() => void closeReviewCenter()}><LumeIcon name="close" size={16} /></button>
  </header>

  <div class="turn-context">
    <div class="turn-picker">
      {#if turns.length}
        <LumeSelect
          ariaLabel={tr("Choose a task to review", "Escolha uma tarefa para revisar")}
          value={selectedTurn?.id ?? ""}
          options={turns.map((turn) => ({ value: turn.id, label: turnLabel(turn) }))}
          minWidth={260}
          onValueChange={(value) => void selectTurn(value)}
        />
      {:else}
        <span class="no-turns">{tr("No tasks available", "Nenhuma tarefa disponível")}</span>
      {/if}
    </div>
    {#if selectedTurn}
      <small title={workflowLink?.objective}>{workflowLink
        ? `${tr("Workflow", "Workflow")} · ${workflowLink.role}`
        : selectedTurn.result ? tr("Completed", "Concluída") : tr("No final result yet", "Sem resultado final")}</small>
    {/if}
  </div>

  <div class:notes-open={notesOpen} class="review-body">
    <aside class="review-files" aria-label={tr("Changed files", "Arquivos alterados")}>
      <header><strong>{tr("Changed files", "Arquivos alterados")}</strong><span>{files.length}</span></header>
      <nav>
        {#each files as file (file.path)}
          <button class:active={selectedFile?.path === file.path} type="button" title={file.path} onclick={() => selectedPath = file.path}>
            <FileTypeIcon path={file.path} />
            <span>{displayFileChangePath(file.path)}</span>
            <b>+{file.added}</b><i>-{file.removed}</i>
          </button>
        {:else}
          <p>{selectedTurn && !selectedTurn.activityAvailable
            ? tr("This result is older than the loaded activity history.", "Este resultado é mais antigo que o histórico de atividades carregado.")
            : tr("No file changes were captured for this task.", "Nenhuma alteração foi capturada nesta tarefa.")}</p>
        {/each}
      </nav>
    </aside>

    <main class="diff-workspace">
      {#if selectedFile}
        <header class="diff-header">
          <span><FileTypeIcon path={selectedFile.path} /><strong>{selectedFile.path}</strong></span>
          <span class="change-count"><b>+{selectedFile.added}</b><i>-{selectedFile.removed}</i></span>
        </header>
        {#if diffLines.length}
          {#if mode === "unified"}
            <div class="unified-diff" aria-label={tr("Unified diff", "Diff unificado")}>
              {#each diffLines as line}
                <div class="diff-line line-{line.kind}"><span>{line.oldLine ?? ""}</span><span>{line.newLine ?? ""}</span><code>{line.kind === "added" ? "+" : line.kind === "removed" ? "−" : " "}{line.content || " "}</code></div>
              {/each}
            </div>
          {:else}
            <div class="split-diff" aria-label={tr("Side by side diff", "Diff lado a lado")}>
              {#each splitRows as row}
                <div class="split-cell line-{row.left?.kind ?? "empty"}"><span>{lineLabel(row.left, "left")}</span><code>{row.left?.kind === "removed" ? "−" : " "}{row.left?.content ?? " "}</code></div>
                <div class="split-cell line-{row.right?.kind ?? "empty"}"><span>{lineLabel(row.right, "right")}</span><code>{row.right?.kind === "added" ? "+" : " "}{row.right?.content ?? " "}</code></div>
              {/each}
            </div>
          {/if}
        {:else}
          <div class="diff-empty"><LumeIcon name="file" size={25} /><strong>{tr("Summary available", "Resumo disponível")}</strong><p>{tr("Lume captured the file and line totals, but this source did not provide the full diff.", "O Lume capturou o arquivo e os totais, mas esta origem não forneceu o diff completo.")}</p></div>
        {/if}
      {:else}
        <div class="diff-empty"><LumeIcon name="diff" size={25} /><strong>{tr("Nothing to review yet", "Nada para revisar ainda")}</strong><p>{selectedTurn && !selectedTurn.activityAvailable
          ? tr("The final response is preserved, but its activity details are no longer in the loaded history.", "A resposta final está preservada, mas as atividades dela não estão mais no histórico carregado.")
          : tr("Changed files from the selected task will appear here.", "Arquivos alterados na tarefa selecionada aparecerão aqui.")}</p></div>
      {/if}
    </main>

    {#if notesOpen}
      <aside class="review-notes" aria-label={tr("Review note", "Nota de revisão")}>
        <header>
          <span><LumeIcon name="note" size={15} /><strong>{tr("Review note", "Nota de revisão")}</strong></span>
          {#if selectedReviewNote}<small>{new Intl.DateTimeFormat(language, { dateStyle: "short", timeStyle: "short" }).format(new Date(selectedReviewNote.updatedAt))}</small>{/if}
        </header>
        {#if selectedResultId}
          <textarea
            value={noteDraft}
            disabled={noteSaving}
            placeholder={tr("Record findings, questions, or follow-up work for this result…", "Registre achados, dúvidas ou trabalho posterior para este resultado…")}
            oninput={(event) => { noteDraft = event.currentTarget.value; noteDirty = true; }}
          ></textarea>
          <footer>
            <span class:dirty={noteDirty}>{noteDirty ? tr("Unsaved changes", "Alterações não salvas") : selectedReviewNote ? tr("Saved locally", "Salva localmente") : ""}</span>
            {#if selectedReviewNote}
              <button class="delete-note" type="button" disabled={noteSaving} title={tr("Delete note", "Excluir nota")} aria-label={tr("Delete note", "Excluir nota")} onclick={() => void removeReviewNote()}><LumeIcon name="trash" size={13} /></button>
            {/if}
            <button class="save-note" type="button" disabled={noteSaving || !noteDirty || !noteDraft.trim()} onclick={() => void persistReviewNote()}><LumeIcon name="save" size={13} />{noteSaving ? tr("Saving…", "Salvando…") : tr("Save", "Salvar")}</button>
          </footer>
        {:else}
          <div class="note-empty"><LumeIcon name="note" size={23} /><p>{tr("A review note can be added after this task has a final result.", "Uma nota de revisão pode ser adicionada após esta tarefa ter um resultado final.")}</p></div>
        {/if}
      </aside>
    {/if}
  </div>

</section>

<style>
  .review-center { position: relative; min-width: 0; min-height: 0; width: 100%; height: 100%; container: review / inline-size; display: grid; grid-template-rows: auto auto minmax(0, 1fr); overflow: hidden; color: var(--workspace-text); background: var(--workspace-raised); }
  .review-header { min-height: 56px; padding: 8px 10px 8px 14px; display: flex; align-items: center; gap: 10px; border-bottom: 1px solid var(--workspace-line); }.review-title { min-width: 0; display: flex; align-items: center; gap: 8px; flex: 1; }.review-title > i { width: 24px; height: 24px; display: grid; place-items: center; flex: 0 0 auto; color: var(--workspace-accent); }.review-title > span { min-width: 0; display: grid; gap: 1px; }.review-title strong { color: var(--workspace-strong); font-size: 11px; }.review-title small { overflow: hidden; color: var(--workspace-muted); font-size: 8px; text-overflow: ellipsis; white-space: nowrap; }
  .mode-switch { padding: 3px; display: flex; border: 1px solid var(--workspace-line); border-radius: 9px; background: var(--workspace-subtle); }.mode-switch button { height: 27px; padding: 0 8px; display: flex; align-items: center; gap: 5px; border: 0; border-radius: 6px; color: var(--workspace-muted); background: transparent; font-size: 8px; font-weight: 720; cursor: pointer; }.mode-switch button.active { color: var(--workspace-accent); background: var(--workspace-raised); box-shadow: 0 1px 4px rgba(18, 34, 25, .08); }.close-review, .review-note-toggle { position: relative; width: 31px; height: 31px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 9px; color: var(--workspace-muted); background: transparent; cursor: pointer; }.close-review:hover, .review-note-toggle:hover, .review-note-toggle.active { color: var(--workspace-strong); background: var(--workspace-subtle); }.review-note-toggle.active { color: var(--workspace-accent); }.review-note-toggle > i { position: absolute; top: 6px; right: 6px; width: 5px; height: 5px; border-radius: 50%; background: var(--workspace-accent); }
  .turn-context { min-width: 0; min-height: 43px; padding: 6px 12px; display: flex; align-items: center; gap: 9px; border-bottom: 1px solid var(--workspace-line); background: var(--workspace-pane); }.turn-picker { width: min(430px, 78%); min-width: 0; }.turn-picker :global(.lume-select) { width: 100%; }.turn-context small { margin-left: auto; overflow: hidden; color: var(--workspace-muted); font-size: 8px; text-overflow: ellipsis; white-space: nowrap; }
  .no-turns { color: var(--workspace-faint); font-size: 9px; }
  .review-body { position: relative; min-width: 0; min-height: 0; display: grid; grid-template-columns: clamp(155px, 29%, 210px) minmax(0, 1fr); }.review-body.notes-open { grid-template-columns: clamp(155px, 24%, 195px) minmax(0, 1fr) clamp(220px, 27%, 290px); }.review-files { min-width: 0; min-height: 0; padding: 10px 7px; display: grid; grid-template-rows: auto minmax(0, 1fr); overflow: hidden; border-right: 1px solid var(--workspace-line); background: var(--workspace-sidebar); }.review-files > header { min-height: 28px; padding: 0 7px; display: flex; align-items: center; color: var(--workspace-muted); }.review-files header strong { flex: 1; color: var(--workspace-strong); font-size: 9px; }.review-files header span { font-size: 8px; font-variant-numeric: tabular-nums; }.review-files nav { min-height: 0; overflow-y: auto; scrollbar-width: thin; scrollbar-color: var(--workspace-scroll-thumb) transparent; }.review-files nav button { width: 100%; min-width: 0; min-height: 36px; padding: 0 7px; display: flex; align-items: center; gap: 6px; border: 0; border-radius: 7px; color: var(--workspace-muted); background: transparent; font-size: 8px; text-align: left; cursor: pointer; }.review-files nav button:hover { color: var(--workspace-text); background: var(--workspace-subtle); }.review-files nav button.active { color: var(--workspace-strong); background: var(--workspace-accent-soft); }.review-files nav button > span { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.review-files nav button b, .change-count b { color: #438f67; font-size: 7px; }.review-files nav button i, .change-count i { color: #b96862; font-size: 7px; font-style: normal; }.review-files nav p { padding: 12px 7px; color: var(--workspace-faint); font-size: 8px; line-height: 1.45; }
  .diff-workspace { min-width: 0; min-height: 0; display: grid; grid-template-rows: auto minmax(0, 1fr); overflow: hidden; background: var(--workspace-code); }.diff-header { min-height: 43px; padding: 0 13px; display: flex; align-items: center; gap: 10px; border-bottom: 1px solid var(--workspace-line); background: var(--workspace-pane); }.diff-header > span:first-child { min-width: 0; display: flex; align-items: center; gap: 7px; flex: 1; }.diff-header strong { overflow: hidden; color: var(--workspace-strong); font: 720 9px/1.2 ui-monospace, monospace; text-overflow: ellipsis; white-space: nowrap; }.change-count { display: flex; gap: 7px; }
  .unified-diff, .split-diff { min-width: 0; min-height: 0; overflow: auto; overscroll-behavior: contain; scrollbar-width: thin; scrollbar-color: var(--workspace-scroll-thumb) transparent; }.diff-line { min-width: max-content; display: grid; grid-template-columns: 42px 42px minmax(520px, 1fr); font: 8px/1.65 ui-monospace, SFMono-Regular, Consolas, monospace; }.diff-line > span { padding: 0 8px; color: var(--workspace-faint); border-right: 1px solid color-mix(in srgb, var(--workspace-line) 55%, transparent); text-align: right; user-select: none; }.diff-line code, .split-cell code { padding: 0 10px; color: var(--workspace-text); white-space: pre; }.line-added { background: rgba(61, 143, 98, .14); }.line-removed { background: rgba(179, 83, 78, .14); }.line-hunk { background: rgba(67, 123, 161, .12); }.line-hunk code { color: #70a9cf; }.line-meta code { color: var(--workspace-faint); }
  .split-diff { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); align-content: start; overflow-x: hidden; }.split-cell { min-width: 0; display: grid; grid-template-columns: 34px minmax(0, 1fr); border-right: 1px solid var(--workspace-line); font: 8px/1.65 ui-monospace, SFMono-Regular, Consolas, monospace; }.split-cell > span { padding: 0 6px; color: var(--workspace-faint); border-right: 1px solid color-mix(in srgb, var(--workspace-line) 55%, transparent); text-align: right; user-select: none; }.split-cell code { min-width: 0; white-space: pre-wrap; overflow-wrap: anywhere; word-break: break-word; }.split-cell.line-empty { background: color-mix(in srgb, var(--workspace-line) 18%, transparent); }.split-cell.line-empty code { opacity: .3; }
  .diff-empty { margin: auto; max-width: 320px; padding: 28px; display: grid; justify-items: center; gap: 8px; color: var(--workspace-faint); text-align: center; }.diff-empty :global(.lume-icon) { color: var(--workspace-accent); }.diff-empty strong { color: var(--workspace-strong); font-size: 11px; }.diff-empty p { margin: 0; font-size: 8px; line-height: 1.5; }
  .review-notes { min-width: 0; min-height: 0; padding: 12px; display: grid; grid-template-rows: auto minmax(0, 1fr) auto; gap: 10px; overflow: hidden; border-left: 1px solid var(--workspace-line); background: var(--workspace-pane); animation: reveal-notes 190ms cubic-bezier(.16, 1, .3, 1); }.review-notes > header { min-width: 0; min-height: 28px; display: flex; align-items: center; gap: 8px; }.review-notes > header > span { min-width: 0; display: flex; align-items: center; gap: 6px; flex: 1; color: var(--workspace-accent); }.review-notes header strong { color: var(--workspace-strong); font-size: 10px; }.review-notes header small { overflow: hidden; color: var(--workspace-faint); font-size: 7px; text-overflow: ellipsis; white-space: nowrap; }.review-notes textarea { box-sizing: border-box; min-width: 0; min-height: 0; width: 100%; padding: 10px; resize: none; border: 1px solid var(--workspace-line); border-radius: 10px; outline: 0; color: var(--workspace-text); background: var(--workspace-raised); font: 9px/1.55 inherit; }.review-notes textarea::placeholder { color: var(--workspace-faint); }.review-notes textarea:focus { border-color: color-mix(in srgb, var(--workspace-accent) 48%, var(--workspace-line)); box-shadow: 0 0 0 2px var(--workspace-accent-soft); }.review-notes textarea:disabled { opacity: .65; }.review-notes footer { min-width: 0; min-height: 29px; display: flex; align-items: center; gap: 6px; }.review-notes footer > span { min-width: 0; flex: 1; overflow: hidden; color: var(--workspace-faint); font-size: 7px; text-overflow: ellipsis; white-space: nowrap; }.review-notes footer > span.dirty { color: #bd8434; }.review-notes footer button { min-height: 28px; padding: 0 8px; display: flex; align-items: center; gap: 5px; border: 0; border-radius: 8px; color: var(--workspace-muted); background: var(--workspace-subtle); font-size: 8px; font-weight: 720; cursor: pointer; }.review-notes footer button:hover:not(:disabled) { color: var(--workspace-strong); }.review-notes footer button:disabled { opacity: .45; cursor: default; }.review-notes footer .delete-note { width: 28px; padding: 0; justify-content: center; color: #b96862; }.review-notes footer .save-note { color: var(--workspace-raised); background: var(--workspace-accent); }.note-empty { margin: auto; max-width: 190px; display: grid; justify-items: center; gap: 8px; color: var(--workspace-faint); text-align: center; }.note-empty p { margin: 0; font-size: 8px; line-height: 1.5; }@keyframes reveal-notes { from { opacity: .4; transform: translateX(12px); } to { opacity: 1; transform: translateX(0); } }
  .review-files nav button { min-height: 38px; font-size: 10px; }.review-files header strong { font-size: 10px; }
  .diff-line, .split-cell { font-size: 10px; line-height: 1.55; }
  @container review (max-width: 760px) { .review-body.notes-open { grid-template-columns: clamp(145px, 27%, 185px) minmax(0, 1fr); }.review-notes { position: absolute; z-index: 3; inset: 0 0 0 auto; width: min(290px, 82%); box-shadow: -12px 0 32px rgba(8, 20, 14, .16); } }
  @container review (max-width: 540px) { .review-body, .review-body.notes-open { grid-template-columns: minmax(0, 1fr); grid-template-rows: auto minmax(0, 1fr); }.review-files { max-height: 100px; padding: 6px 8px; grid-template-columns: auto minmax(0, 1fr); grid-template-rows: minmax(0, 1fr); align-items: center; border-right: 0; border-bottom: 1px solid var(--workspace-line); }.review-files > header { padding-right: 9px; }.review-files > header span { display: none; }.review-files nav { display: flex; gap: 3px; overflow-x: auto; overflow-y: hidden; }.review-files nav button { width: min(170px, 48cqw); flex: 0 0 auto; }.turn-picker { width: 100%; }.turn-context small { display: none; }.mode-switch button span { display: none; } }
  @media (prefers-reduced-motion: reduce) { .review-notes { animation: none; } }
</style>
