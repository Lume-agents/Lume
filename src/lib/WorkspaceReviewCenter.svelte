<script lang="ts">
  import { onMount } from "svelte";
  import FileTypeIcon from "$lib/FileTypeIcon.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import LumeSelect from "$lib/LumeSelect.svelte";
  import ResponseAttachments from "$lib/ResponseAttachments.svelte";
  import SystemBannerStack, { type SystemBannerItem } from "$lib/SystemBannerStack.svelte";
  import type { HubSession } from "$lib/hubProtocol";
  import type { PromptDelivery, ReviewDecision, ReviewNote, WorkflowHistoryRecord } from "$lib/domain";
  import type { Language } from "$lib/i18n";
  import { cleanPromptTransport, extractResponseFiles } from "$lib/chatAttachments";
  import { displayFileChangePath } from "$lib/fileChanges";
  import { buildReviewTurns, parseReviewDiff, type ReviewDiffLine } from "$lib/reviewDiffs";
  import { deleteReviewNote, loadReviewDecisions, loadReviewNotes, loadWorkflowHistory, saveReviewNote, setReviewDecision, submitPrompt } from "$lib/lume";
  import { renderSafeMarkdown } from "$lib/markdown.js";

  type DiffMode = "unified" | "split";
  type SplitRow = { left?: ReviewDiffLine; right?: ReviewDiffLine };

  let { session, language = "en", initialPath, onClose } = $props<{
    session: HubSession;
    language?: Language;
    initialPath?: string;
    onClose: () => void;
  }>();

  let mode = $state<DiffMode>("unified");
  let viewMode = $state<"changes" | "context" | "deliverables">("changes");
  let selectedPath = $state("");
  let selectedTurnId = $state("");
  let previousTurnId = $state("");
  let previousInitialPath = $state("");
  let workflowHistory = $state<WorkflowHistoryRecord[]>([]);
  let reviewNotes = $state<ReviewNote[]>([]);
  let reviewDecisions = $state<ReviewDecision[]>([]);
  let notesOpen = $state(false);
  let noteDraft = $state("");
  let noteDraftResultId = $state("");
  let noteDirty = $state(false);
  let noteSaving = $state(false);
  let reviewMessage = $state("");
  let reviewMessageIsError = $state(false);
  let notesLoadToken = 0;
  let decisionsLoadToken = 0;
  let correctionOpen = $state(false);
  let correctionDraft = $state("");
  let correctionSending = $state(false);
  let decisionSaving = $state(false);
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
  const selectedReviewDecision = $derived(reviewDecisions.find((decision) => decision.resultId === selectedResultId) ?? null);
  const deliverableFiles = $derived(selectedTurn?.result
    ? extractResponseFiles(selectedTurn.result.response, selectedTurn.responseAttachments, session.workingDirectory)
    : []);
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
    const token = ++decisionsLoadToken;
    reviewDecisions = [];
    void loadReviewDecisions(sessionId)
      .then((decisions) => {
        if (token === decisionsLoadToken && session.id === sessionId) reviewDecisions = decisions;
      })
      .catch((error) => {
        if (token !== decisionsLoadToken || session.id !== sessionId) return;
        reviewMessageIsError = true;
        reviewMessage = String(error).replace(/^Error:\s*/, "");
      });
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
    correctionOpen = false;
    correctionDraft = "";
  }

  async function closeReviewCenter() {
    if (noteDirty && !(await persistReviewNote())) return;
    onClose();
  }

  async function approveSelectedResult() {
    if (!selectedTurn?.result || decisionSaving) return;
    decisionSaving = true;
    try {
      const decision = await setReviewDecision(session.id, selectedTurn.result.id, "approved");
      reviewDecisions = [decision, ...reviewDecisions.filter((item) => item.resultId !== decision.resultId)];
      reviewMessageIsError = false;
      reviewMessage = tr("Result approved", "Resultado aprovado");
      correctionOpen = false;
    } catch (error) {
      reviewMessageIsError = true;
      reviewMessage = String(error).replace(/^Error:\s*/, "");
    } finally {
      decisionSaving = false;
    }
  }

  async function sendCorrectionRequest() {
    const turn = selectedTurn;
    const feedback = correctionDraft.trim();
    if (!turn?.result || correctionSending || !feedback) return;
    if (!session.capabilities.canPrompt) {
      reviewMessageIsError = true;
      reviewMessage = tr("Take control of this session before requesting changes.", "Assuma o controle desta sessão antes de solicitar correções.");
      return;
    }
    const delivery: PromptDelivery | null = session.status === "running"
      ? session.capabilities.promptDeliveries.includes("queue") ? "queue" : null
      : session.status === "permission_required" ? null : "new_turn";
    if (!delivery) {
      reviewMessageIsError = true;
      reviewMessage = session.status === "permission_required"
        ? tr("Resolve the pending permission before requesting changes.", "Resolva a permissão pendente antes de solicitar correções.")
        : tr("This running session cannot queue a correction request.", "Esta sessão em execução não pode enfileirar um pedido de correção.");
      return;
    }
    correctionSending = true;
    try {
      const originalTask = cleanPromptTransport(turn.prompt?.detail ?? "").split(/\r?\n/, 1)[0].slice(0, 240);
      const request = [
        `Review correction for result ${turn.result.id}${originalTask ? ` (task: ${originalTask})` : ""}.`,
        "Please apply the following review feedback:",
        feedback,
      ].join("\n\n");
      await submitPrompt(session.id, request, [], delivery);
      const decision = await setReviewDecision(
        session.id,
        turn.result.id,
        "changes_requested",
        feedback,
      );
      reviewDecisions = [decision, ...reviewDecisions.filter((item) => item.resultId !== decision.resultId)];
      reviewMessageIsError = false;
      reviewMessage = delivery === "queue"
        ? tr("Correction request queued after the active prompt.", "Pedido de correção enfileirado após o prompt atual.")
        : tr("Correction request sent.", "Pedido de correção enviado.");
      correctionDraft = "";
      correctionOpen = false;
    } catch (error) {
      reviewMessageIsError = true;
      reviewMessage = String(error).replace(/^Error:\s*/, "");
    } finally {
      correctionSending = false;
    }
  }
</script>

<section class:correction-open={correctionOpen} class:changes-view={viewMode === "changes"} class="review-center" aria-label={tr("Review center", "Central de revisão")}>
  <SystemBannerStack items={systemBanners} contained {language} dismissLabel={tr("Dismiss", "Fechar")} />
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

  <div class="review-toolbar">
    <nav class="review-views" aria-label={tr("Review views", "Visualizações da revisão")}>
      <button class:active={viewMode === "changes"} type="button" aria-pressed={viewMode === "changes"} onclick={() => viewMode = "changes"}><LumeIcon name="diff" size={13} /><span>{tr("Changes", "Alterações")}</span><small>{files.length}</small></button>
      <button class:active={viewMode === "context"} type="button" aria-pressed={viewMode === "context"} onclick={() => viewMode = "context"}><LumeIcon name="sources" size={13} /><span>{tr("Context", "Contexto")}</span></button>
      <button class:active={viewMode === "deliverables"} type="button" aria-pressed={viewMode === "deliverables"} onclick={() => viewMode = "deliverables"}><LumeIcon name="download" size={13} /><span>{tr("Deliverables", "Entregáveis")}</span><small>{deliverableFiles.length}</small></button>
    </nav>
    <div class="review-decisions" aria-label={tr("Result decision", "Decisão sobre o resultado")}>
      {#if selectedReviewDecision}
        <span class="decision-state {selectedReviewDecision.decision}"><LumeIcon name={selectedReviewDecision.decision === "approved" ? "check" : "note"} size={12} />{selectedReviewDecision.decision === "approved" ? tr("Approved", "Aprovado") : tr("Changes requested", "Correções solicitadas")}</span>
      {/if}
      <button class="request-changes" type="button" disabled={!selectedTurn?.result || decisionSaving || correctionSending} onclick={() => correctionOpen = !correctionOpen}><LumeIcon name="refresh" size={13} /><span>{tr("Request changes", "Solicitar correções")}</span></button>
      <button class="approve-result" type="button" title={tr("Records a review decision; it does not advance a workflow handoff.", "Registra a revisão; não avança o handoff do workflow.")} disabled={!selectedTurn?.result || decisionSaving || correctionSending} onclick={() => void approveSelectedResult()}><LumeIcon name="check" size={13} /><span>{decisionSaving ? tr("Saving…", "Salvando…") : tr("Approve result", "Aprovar resultado")}</span></button>
    </div>
  </div>

  {#if correctionOpen}
    <form class="correction-request" onsubmit={(event) => { event.preventDefault(); void sendCorrectionRequest(); }}>
      <label for="review-correction">{tr("What should be corrected?", "O que precisa ser corrigido?")}</label>
      <textarea id="review-correction" value={correctionDraft} disabled={correctionSending} placeholder={tr("Describe the changes to make…", "Descreva as correções necessárias…")} oninput={(event) => correctionDraft = event.currentTarget.value}></textarea>
      <div class="correction-footer">
        <small>{!session.capabilities.canPrompt
          ? tr("Take control of this session to send a correction request.", "Assuma o controle desta sessão para enviar o pedido de correção.")
          : session.status === "running" && !session.capabilities.promptDeliveries.includes("queue")
            ? tr("This session cannot queue a correction while running.", "Esta sessão não pode enfileirar correções enquanto está executando.")
            : session.status === "permission_required"
              ? tr("Resolve the pending permission first.", "Resolva a permissão pendente primeiro.")
              : tr("The request is sent as a new prompt and recorded for this result.", "O pedido é enviado como novo prompt e registrado para este resultado.")}</small>
        <button type="button" class="cancel-correction" disabled={correctionSending} onclick={() => correctionOpen = false}>{tr("Cancel", "Cancelar")}</button>
        <button type="submit" class="send-correction" disabled={correctionSending || !correctionDraft.trim() || !session.capabilities.canPrompt || session.status === "permission_required" || (session.status === "running" && !session.capabilities.promptDeliveries.includes("queue"))}><LumeIcon name="arrow-down" size={13} />{correctionSending ? tr("Sending…", "Enviando…") : tr("Send request", "Enviar pedido")}</button>
      </div>
    </form>
  {/if}

  <div class:notes-open={notesOpen} class:simple-view={viewMode !== "changes"} class="review-body">
    {#if viewMode === "changes"}
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
    {:else if viewMode === "context"}
      <main class="review-reading-pane" aria-label={tr("Prompt and result context", "Contexto do prompt e resultado")}>
        {#if selectedTurn?.prompt}
          <section class="reading-section">
            <header><LumeIcon name="sources" size={14} /><strong>{tr("Prompt sent", "Prompt enviado")}</strong><time>{new Intl.DateTimeFormat(language, { dateStyle: "short", timeStyle: "short" }).format(new Date(selectedTurn.prompt.createdAt))}</time></header>
            <div class="reading-copy">{@html renderSafeMarkdown(cleanPromptTransport(selectedTurn.prompt.detail ?? ""))}</div>
            {#if selectedTurn.prompt.attachments?.length}
              <div class="context-attachments" aria-label={tr("Prompt attachments", "Anexos do prompt")}>
                {#each selectedTurn.prompt.attachments ?? [] as attachment (attachment.id)}<span><FileTypeIcon path={attachment.name} />{attachment.name}</span>{/each}
              </div>
            {/if}
          </section>
        {:else}
          <div class="diff-empty"><LumeIcon name="sources" size={24} /><strong>{tr("Prompt context unavailable", "Contexto do prompt indisponível")}</strong><p>{tr("This archived result no longer has its original prompt in the loaded activity history.", "Este resultado arquivado não tem mais o prompt original no histórico de atividades carregado.")}</p></div>
        {/if}
        {#if selectedTurn?.checks.length}
          <section class="reading-section checks-section"><header><LumeIcon name="check" size={14} /><strong>{tr("Checks", "Validações")}</strong></header><ul>{#each selectedTurn.checks as check (check)}<li>{check}</li>{/each}</ul></section>
        {/if}
        {#if selectedTurn?.result}
          <section class="reading-section final-response"><header><LumeIcon name="check" size={14} /><strong>{tr("Final response", "Resposta final")}</strong><time>{new Intl.DateTimeFormat(language, { dateStyle: "short", timeStyle: "short" }).format(new Date(selectedTurn.result.createdAt))}</time></header><div class="reading-copy">{@html renderSafeMarkdown(selectedTurn.result.response)}</div></section>
        {/if}
      </main>
    {:else}
      <main class="review-deliverables" aria-label={tr("Explicit deliverables", "Entregáveis explícitos")}>
        {#if selectedTurn?.result && deliverableFiles.length}
          <header><span><LumeIcon name="download" size={15} /><strong>{tr("Offered for download", "Oferecidos para baixar")}</strong></span><small>{deliverableFiles.length}</small></header>
          <ResponseAttachments text={selectedTurn.result.response} attachments={selectedTurn.responseAttachments} workingDirectory={session.workingDirectory} {language} />
        {:else}
          <div class="diff-empty"><LumeIcon name="download" size={25} /><strong>{tr("No explicit deliverables", "Nenhum entregável explícito")}</strong><p>{tr("Files changed during the task stay under Changes. Only files explicitly offered in the final response appear here.", "Arquivos alterados durante a tarefa ficam em Alterações. Aqui aparecem apenas arquivos oferecidos explicitamente na resposta final.")}</p></div>
        {/if}
      </main>
    {/if}

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
  .review-center { position: relative; min-width: 0; min-height: 0; width: 100%; height: 100%; container: review / inline-size; display: grid; grid-template-rows: auto auto auto minmax(0, 1fr); overflow: hidden; color: var(--workspace-text); background: var(--workspace-raised); }
  .review-center.correction-open { grid-template-rows: auto auto auto auto minmax(0, 1fr); }
  .review-center:not(.changes-view) .mode-switch { display: none; }
  .review-header { min-height: 56px; padding: 8px 10px 8px 14px; display: flex; align-items: center; gap: 10px; border-bottom: 1px solid var(--workspace-line); }.review-title { min-width: 0; display: flex; align-items: center; gap: 8px; flex: 1; }.review-title > i { width: 24px; height: 24px; display: grid; place-items: center; flex: 0 0 auto; color: var(--workspace-accent); }.review-title > span { min-width: 0; display: grid; gap: 1px; }.review-title strong { color: var(--workspace-strong); font-size: 11px; }.review-title small { overflow: hidden; color: var(--workspace-muted); font-size: 8px; text-overflow: ellipsis; white-space: nowrap; }
  .mode-switch { padding: 3px; display: flex; border: 1px solid var(--workspace-line); border-radius: 9px; background: var(--workspace-subtle); }.mode-switch button { height: 27px; padding: 0 8px; display: flex; align-items: center; gap: 5px; border: 0; border-radius: 6px; color: var(--workspace-muted); background: transparent; font-size: 8px; font-weight: 720; cursor: pointer; }.mode-switch button.active { color: var(--workspace-accent); background: var(--workspace-raised); box-shadow: 0 1px 4px rgba(18, 34, 25, .08); }.close-review, .review-note-toggle { position: relative; width: 31px; height: 31px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 9px; color: var(--workspace-muted); background: transparent; cursor: pointer; }.close-review:hover, .review-note-toggle:hover, .review-note-toggle.active { color: var(--workspace-strong); background: var(--workspace-subtle); }.review-note-toggle.active { color: var(--workspace-accent); }.review-note-toggle > i { position: absolute; top: 6px; right: 6px; width: 5px; height: 5px; border-radius: 50%; background: var(--workspace-accent); }
  .turn-context { min-width: 0; min-height: 43px; padding: 6px 12px; display: flex; align-items: center; gap: 9px; border-bottom: 1px solid var(--workspace-line); background: var(--workspace-pane); }.turn-picker { width: min(430px, 78%); min-width: 0; }.turn-picker :global(.lume-select) { width: 100%; }.turn-context small { margin-left: auto; overflow: hidden; color: var(--workspace-muted); font-size: 8px; text-overflow: ellipsis; white-space: nowrap; }
  .no-turns { color: var(--workspace-faint); font-size: 9px; }
  .review-body { position: relative; min-width: 0; min-height: 0; display: grid; grid-template-columns: clamp(155px, 29%, 210px) minmax(0, 1fr); }.review-body.notes-open { grid-template-columns: clamp(155px, 24%, 195px) minmax(0, 1fr) clamp(220px, 27%, 290px); }.review-files { min-width: 0; min-height: 0; padding: 10px 7px; display: grid; grid-template-rows: auto minmax(0, 1fr); overflow: hidden; border-right: 1px solid var(--workspace-line); background: var(--workspace-sidebar); }.review-files > header { min-height: 28px; padding: 0 7px; display: flex; align-items: center; color: var(--workspace-muted); }.review-files header strong { flex: 1; color: var(--workspace-strong); font-size: 9px; }.review-files header span { font-size: 8px; font-variant-numeric: tabular-nums; }.review-files nav { min-height: 0; overflow-y: auto; scrollbar-width: thin; scrollbar-color: var(--workspace-scroll-thumb) transparent; }.review-files nav button { width: 100%; min-width: 0; min-height: 36px; padding: 0 7px; display: flex; align-items: center; gap: 6px; border: 0; border-radius: 7px; color: var(--workspace-muted); background: transparent; font-size: 8px; text-align: left; cursor: pointer; }.review-files nav button:hover { color: var(--workspace-text); background: var(--workspace-subtle); }.review-files nav button.active { color: var(--workspace-strong); background: var(--workspace-accent-soft); }.review-files nav button > span { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.review-files nav button b, .change-count b { color: #438f67; font-size: 7px; }.review-files nav button i, .change-count i { color: #b96862; font-size: 7px; font-style: normal; }.review-files nav p { padding: 12px 7px; color: var(--workspace-faint); font-size: 8px; line-height: 1.45; }
  .diff-workspace { min-width: 0; min-height: 0; display: grid; grid-template-rows: auto minmax(0, 1fr); overflow: hidden; background: var(--workspace-code); }.diff-header { min-height: 43px; padding: 0 13px; display: flex; align-items: center; gap: 10px; border-bottom: 1px solid var(--workspace-line); background: var(--workspace-pane); }.diff-header > span:first-child { min-width: 0; display: flex; align-items: center; gap: 7px; flex: 1; }.diff-header strong { overflow: hidden; color: var(--workspace-strong); font: 720 9px/1.2 ui-monospace, monospace; text-overflow: ellipsis; white-space: nowrap; }.change-count { display: flex; gap: 7px; }
  .unified-diff, .split-diff { min-width: 0; min-height: 0; overflow: auto; overscroll-behavior: contain; scrollbar-width: thin; scrollbar-color: var(--workspace-scroll-thumb) transparent; }.diff-line { min-width: max-content; display: grid; grid-template-columns: 42px 42px minmax(520px, 1fr); font: 8px/1.65 ui-monospace, SFMono-Regular, Consolas, monospace; }.diff-line > span { padding: 0 8px; color: var(--workspace-faint); border-right: 1px solid color-mix(in srgb, var(--workspace-line) 55%, transparent); text-align: right; user-select: none; }.diff-line code, .split-cell code { padding: 0 10px; color: var(--workspace-text); white-space: pre; }.line-added { background: rgba(61, 143, 98, .14); }.line-removed { background: rgba(179, 83, 78, .14); }.line-hunk { background: rgba(67, 123, 161, .12); }.line-hunk code { color: #70a9cf; }.line-meta code { color: var(--workspace-faint); }
  .split-diff { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); align-content: start; overflow-x: hidden; }.split-cell { min-width: 0; display: grid; grid-template-columns: 34px minmax(0, 1fr); border-right: 1px solid var(--workspace-line); font: 8px/1.65 ui-monospace, SFMono-Regular, Consolas, monospace; }.split-cell > span { padding: 0 6px; color: var(--workspace-faint); border-right: 1px solid color-mix(in srgb, var(--workspace-line) 55%, transparent); text-align: right; user-select: none; }.split-cell code { min-width: 0; white-space: pre-wrap; overflow-wrap: anywhere; word-break: break-word; }.split-cell.line-empty { background: color-mix(in srgb, var(--workspace-line) 18%, transparent); }.split-cell.line-empty code { opacity: .3; }
  .diff-empty { margin: auto; max-width: 320px; padding: 28px; display: grid; justify-items: center; gap: 8px; color: var(--workspace-faint); text-align: center; }.diff-empty :global(.lume-icon) { color: var(--workspace-accent); }.diff-empty strong { color: var(--workspace-strong); font-size: 11px; }.diff-empty p { margin: 0; font-size: 8px; line-height: 1.5; }
  .review-toolbar { min-width: 0; min-height: 42px; padding: 5px 9px; display: flex; align-items: center; justify-content: space-between; gap: 8px; border-bottom: 1px solid var(--workspace-line); background: var(--workspace-pane); }
  .review-views, .review-decisions { min-width: 0; display: flex; align-items: center; gap: 4px; }
  .review-views button, .review-decisions button { min-height: 30px; padding: 0 9px; display: inline-flex; align-items: center; justify-content: center; gap: 5px; border: 1px solid transparent; border-radius: 8px; color: var(--workspace-muted); background: transparent; font-size: 8px; font-weight: 720; white-space: nowrap; cursor: pointer; transition: color 140ms ease, background 140ms ease, border-color 140ms ease, transform 140ms ease; }
  .review-views button:hover, .review-decisions button:hover:not(:disabled) { color: var(--workspace-strong); background: var(--workspace-subtle); }
  .review-views button:active, .review-decisions button:active:not(:disabled) { transform: scale(.97); }
  .review-views button.active { color: var(--workspace-accent); border-color: color-mix(in srgb, var(--workspace-accent) 18%, transparent); background: var(--workspace-accent-soft); }
  .review-views button small { min-width: 15px; padding: 1px 4px; border-radius: 5px; color: var(--workspace-muted); background: var(--workspace-subtle); font-size: 7px; text-align: center; }
  .review-decisions { margin-left: auto; justify-content: flex-end; }
  .review-decisions .decision-state { display: inline-flex; align-items: center; gap: 4px; padding: 0 6px; color: var(--workspace-muted); font-size: 7px; white-space: nowrap; }
  .decision-state.approved { color: #438f67; }.decision-state.changes_requested { color: #bd8434; }
  .review-decisions .request-changes { border-color: var(--workspace-line); background: var(--workspace-raised); }
  .review-decisions .approve-result { color: var(--workspace-raised); background: var(--workspace-accent); }
  .review-decisions .approve-result:hover:not(:disabled) { color: var(--workspace-raised); background: color-mix(in srgb, var(--workspace-accent) 88%, black); }
  .review-decisions button:disabled { opacity: .42; cursor: default; }
  .correction-request { min-width: 0; padding: 10px 12px; display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 6px 10px; border-bottom: 1px solid var(--workspace-line); background: var(--workspace-subtle); }
  .correction-request label { color: var(--workspace-strong); font-size: 9px; font-weight: 760; }
  .correction-request textarea { box-sizing: border-box; min-width: 0; min-height: 58px; grid-column: 1 / -1; padding: 8px 10px; resize: vertical; border: 1px solid var(--workspace-line); border-radius: 8px; outline: 0; color: var(--workspace-text); background: var(--workspace-raised); font: inherit; font-size: 9px; line-height: 1.45; }
  .correction-request textarea:focus { border-color: color-mix(in srgb, var(--workspace-accent) 48%, var(--workspace-line)); box-shadow: 0 0 0 2px var(--workspace-accent-soft); }
  .correction-footer { min-width: 0; grid-column: 1 / -1; display: flex; align-items: center; justify-content: flex-end; gap: 6px; }
  .correction-footer small { min-width: 0; margin-right: auto; overflow: hidden; color: var(--workspace-faint); font-size: 7px; text-overflow: ellipsis; }
  .correction-footer button { min-height: 27px; padding: 0 8px; display: inline-flex; align-items: center; gap: 5px; border: 1px solid var(--workspace-line); border-radius: 7px; color: var(--workspace-muted); background: var(--workspace-raised); font-size: 8px; font-weight: 700; cursor: pointer; }
  .correction-footer .send-correction { color: var(--workspace-raised); border-color: transparent; background: var(--workspace-accent); }
  .correction-footer button:disabled { opacity: .42; cursor: default; }
  .review-body.simple-view { grid-template-columns: minmax(0, 1fr); }
  .review-body.simple-view.notes-open { grid-template-columns: minmax(0, 1fr) clamp(220px, 27%, 290px); }
  .review-reading-pane, .review-deliverables { min-width: 0; min-height: 0; overflow: auto; overscroll-behavior: contain; scrollbar-width: thin; scrollbar-color: var(--workspace-scroll-thumb) transparent; background: var(--workspace-code); }
  .review-reading-pane { padding: clamp(14px, 3vw, 28px); display: flex; flex-direction: column; gap: 16px; }
  .reading-section { min-width: 0; padding-bottom: 14px; border-bottom: 1px solid var(--workspace-line); }
  .reading-section > header, .review-deliverables > header { min-height: 26px; margin-bottom: 8px; display: flex; align-items: center; gap: 7px; color: var(--workspace-accent); }
  .reading-section > header strong, .review-deliverables > header strong { color: var(--workspace-strong); font-size: 9px; }
  .reading-section > header time { margin-left: auto; color: var(--workspace-faint); font-size: 7px; }
  .reading-copy { color: var(--workspace-text); font-size: 9px; line-height: 1.62; overflow-wrap: anywhere; }
  .reading-copy :global(p:first-child) { margin-top: 0; }.reading-copy :global(p:last-child) { margin-bottom: 0; }
  .reading-copy :global(pre) { max-width: 100%; padding: 9px; overflow: auto; border-radius: 7px; color: var(--workspace-text); background: var(--workspace-subtle); font-size: 8px; }
  .reading-copy :global(code) { overflow-wrap: anywhere; }.reading-copy :global(table) { display: block; max-width: 100%; overflow-x: auto; border-collapse: collapse; }
  .context-attachments { margin-top: 10px; display: flex; flex-wrap: wrap; gap: 5px; }
  .context-attachments span { max-width: 100%; min-height: 24px; padding: 0 7px; display: inline-flex; align-items: center; gap: 5px; overflow: hidden; border: 1px solid var(--workspace-line); border-radius: 6px; color: var(--workspace-muted); background: var(--workspace-pane); font-size: 7px; text-overflow: ellipsis; white-space: nowrap; }
  .checks-section ul { margin: 0; padding-left: 17px; color: var(--workspace-text); font-size: 8px; line-height: 1.7; }
  .final-response { padding: 10px; border: 1px solid var(--workspace-line); border-radius: 9px; background: var(--workspace-pane); }
  .review-deliverables { padding: clamp(14px, 3vw, 25px); display: grid; grid-template-rows: auto minmax(0, 1fr); align-content: start; }
  .review-deliverables > header { margin-bottom: 10px; justify-content: space-between; }
  .review-deliverables > header > span { display: inline-flex; align-items: center; gap: 7px; }
  .review-deliverables > header small { padding: 2px 6px; border-radius: 5px; color: var(--workspace-muted); background: var(--workspace-subtle); font-size: 7px; }
  .review-deliverables :global(.response-attachments) { align-self: start; }
  .review-notes { min-width: 0; min-height: 0; padding: 12px; display: grid; grid-template-rows: auto minmax(0, 1fr) auto; gap: 10px; overflow: hidden; border-left: 1px solid var(--workspace-line); background: var(--workspace-pane); animation: reveal-notes 190ms cubic-bezier(.16, 1, .3, 1); }.review-notes > header { min-width: 0; min-height: 28px; display: flex; align-items: center; gap: 8px; }.review-notes > header > span { min-width: 0; display: flex; align-items: center; gap: 6px; flex: 1; color: var(--workspace-accent); }.review-notes header strong { color: var(--workspace-strong); font-size: 10px; }.review-notes header small { overflow: hidden; color: var(--workspace-faint); font-size: 7px; text-overflow: ellipsis; white-space: nowrap; }.review-notes textarea { box-sizing: border-box; min-width: 0; min-height: 0; width: 100%; padding: 10px; resize: none; border: 1px solid var(--workspace-line); border-radius: 10px; outline: 0; color: var(--workspace-text); background: var(--workspace-raised); font: 9px/1.55 inherit; }.review-notes textarea::placeholder { color: var(--workspace-faint); }.review-notes textarea:focus { border-color: color-mix(in srgb, var(--workspace-accent) 48%, var(--workspace-line)); box-shadow: 0 0 0 2px var(--workspace-accent-soft); }.review-notes textarea:disabled { opacity: .65; }.review-notes footer { min-width: 0; min-height: 29px; display: flex; align-items: center; gap: 6px; }.review-notes footer > span { min-width: 0; flex: 1; overflow: hidden; color: var(--workspace-faint); font-size: 7px; text-overflow: ellipsis; white-space: nowrap; }.review-notes footer > span.dirty { color: #bd8434; }.review-notes footer button { min-height: 28px; padding: 0 8px; display: flex; align-items: center; gap: 5px; border: 0; border-radius: 8px; color: var(--workspace-muted); background: var(--workspace-subtle); font-size: 8px; font-weight: 720; cursor: pointer; }.review-notes footer button:hover:not(:disabled) { color: var(--workspace-strong); }.review-notes footer button:disabled { opacity: .45; cursor: default; }.review-notes footer .delete-note { width: 28px; padding: 0; justify-content: center; color: #b96862; }.review-notes footer .save-note { color: var(--workspace-raised); background: var(--workspace-accent); }.note-empty { margin: auto; max-width: 190px; display: grid; justify-items: center; gap: 8px; color: var(--workspace-faint); text-align: center; }.note-empty p { margin: 0; font-size: 8px; line-height: 1.5; }@keyframes reveal-notes { from { opacity: .4; transform: translateX(12px); } to { opacity: 1; transform: translateX(0); } }
  .review-files nav button { min-height: 38px; font-size: 10px; }.review-files header strong { font-size: 10px; }
  .diff-line, .split-cell { font-size: 10px; line-height: 1.55; }
  .review-views button:focus-visible, .review-decisions button:focus-visible, .correction-footer button:focus-visible, .correction-request textarea:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 2px; }
  @container review (max-width: 760px) { .review-toolbar { flex-wrap: wrap; }.review-decisions .decision-state { display: none; }.review-reading-pane { padding: 14px; }.review-body.simple-view.notes-open { grid-template-columns: minmax(0, 1fr); } }
  @container review (max-width: 540px) { .review-toolbar { padding: 5px 6px; gap: 5px; }.review-views { width: 100%; }.review-views button { flex: 1; padding: 0 4px; }.review-views button span { display: none; }.review-decisions { width: 100%; margin-left: 0; }.review-decisions button { min-width: 0; flex: 1; padding: 0 5px; font-size: 7px; }.correction-request { padding: 8px; }.correction-footer { flex-wrap: wrap; }.correction-footer small { flex-basis: 100%; }.review-body.simple-view { grid-template-rows: minmax(0, 1fr); }.review-body.simple-view.notes-open { grid-template-columns: minmax(0, 1fr); }.review-body.simple-view .review-notes { position: absolute; z-index: 3; inset: 0 0 0 auto; width: min(290px, 82%); box-shadow: -12px 0 32px rgba(8, 20, 14, .16); } }
  @container review (max-width: 760px) { .review-body.notes-open { grid-template-columns: clamp(145px, 27%, 185px) minmax(0, 1fr); }.review-notes { position: absolute; z-index: 3; inset: 0 0 0 auto; width: min(290px, 82%); box-shadow: -12px 0 32px rgba(8, 20, 14, .16); } }
  @container review (max-width: 540px) { .review-body, .review-body.notes-open { grid-template-columns: minmax(0, 1fr); grid-template-rows: auto minmax(0, 1fr); }.review-files { max-height: 100px; padding: 6px 8px; grid-template-columns: auto minmax(0, 1fr); grid-template-rows: minmax(0, 1fr); align-items: center; border-right: 0; border-bottom: 1px solid var(--workspace-line); }.review-files > header { padding-right: 9px; }.review-files > header span { display: none; }.review-files nav { display: flex; gap: 3px; overflow-x: auto; overflow-y: hidden; }.review-files nav button { width: min(170px, 48cqw); flex: 0 0 auto; }.turn-picker { width: 100%; }.turn-context small { display: none; }.mode-switch button span { display: none; } }
  @media (prefers-reduced-motion: reduce) { .review-notes { animation: none; }.review-views button, .review-decisions button { transition: none; } }
</style>
