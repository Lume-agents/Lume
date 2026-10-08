<script lang="ts">
  import { onMount } from "svelte";
  import FileTypeIcon from "$lib/FileTypeIcon.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import LumeSelect from "$lib/LumeSelect.svelte";
  import ResponseAttachments from "$lib/ResponseAttachments.svelte";
  import SystemBannerStack, { type SystemBannerItem } from "$lib/SystemBannerStack.svelte";
  import type { HubSession } from "$lib/hubProtocol";
  import type { PromptDelivery, ReviewDecision, ReviewNote, WorkflowHistoryRecord, WorkflowRun } from "$lib/domain";
  import type { Language } from "$lib/i18n";
  import { cleanPromptTransport, extractResponseFiles } from "$lib/chatAttachments";
  import { displayFileChangePath } from "$lib/fileChanges";
  import { buildReviewTurns, parseReviewDiff, type ReviewDiffLine } from "$lib/reviewDiffs";
  import { approveWorkflowHandoff, deleteReviewNote, loadReviewDecisions, loadReviewNotes, loadWorkflowHistory, loadWorkflowRun, saveReviewNote, setReviewDecision, submitPrompt } from "$lib/lume";
  import { getRepositoryDiff, getRepositorySnapshot, repositoryError, type RepositoryFile } from "$lib/repositories";
  import { listen } from "@tauri-apps/api/event";
  import { renderSafeMarkdown } from "$lib/markdown.js";
  import { foldUnchanged, pairForSplit, presentLines } from "$lib/diffPresentation";
  import { commentKey, formatCommentsForPrompt, indexComments, parseStoredComments, type CommentSide, type ReviewComment } from "$lib/reviewComments";

  type DiffMode = "unified" | "split";
  type GitDiff = { diff: string; binary: boolean } | { error: string };
  const GIT_TURN = "git:worktree";

  let { session, language = "en", initialPath, wide = false, onToggleWide, onClose } = $props<{
    session: HubSession;
    language?: Language;
    initialPath?: string;
    wide?: boolean;
    onToggleWide?: () => void;
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
  let liveRun = $state<WorkflowRun | null>(null);
  let gitFiles = $state<RepositoryFile[]>([]);
  let gitState = $state<"idle" | "loading" | "ready" | "error">("idle");
  let gitMessage = $state("");
  let gitDiffs = $state<Record<string, GitDiff>>({});
  let reviewedPaths = $state<string[]>([]);
  let comments = $state<ReviewComment[]>([]);
  let composer = $state<{ path: string; side: CommentSide; line: number; excerpt: string; body: string; editId?: string } | null>(null);
  let expandedFolds = $state<string[]>([]);
  let answerOpen = $state(false);
  let diffScroller = $state<HTMLElement | null>(null);
  let notesLoadToken = 0;
  let decisionsLoadToken = 0;
  let correctionOpen = $state(false);
  let correctionDraft = $state("");
  let correctionSending = $state(false);
  let decisionSaving = $state(false);
  const turns = $derived(buildReviewTurns(session.activities, session.results, session.workingDirectory));
  const gitMode = $derived(selectedTurnId === GIT_TURN);
  const selectedTurn = $derived(gitMode ? null : turns.find((turn) => turn.id === selectedTurnId) ?? turns[0] ?? null);
  const turnKey = $derived(gitMode ? GIT_TURN : selectedTurn?.id ?? "");
  const loadedGitDiff = (path: string) => { const entry = gitDiffs[path]; return entry && "diff" in entry ? entry : null; };
  const files = $derived.by(() => {
    if (gitMode) {
      return gitFiles.map((file) => {
        const loaded = loadedGitDiff(file.path);
        const lines = loaded ? parseReviewDiff(loaded.diff) : [];
        return { path: file.path, added: lines.filter((line) => line.kind === "added").length, removed: lines.filter((line) => line.kind === "removed").length, diff: loaded?.diff, loaded: Boolean(loaded) };
      });
    }
    return (selectedTurn?.files ?? []).map((file) => {
      const git = loadedGitDiff(file.path);
      const fallbackLines = !file.diff && git ? parseReviewDiff(git.diff) : [];
      return {
        ...file,
        added: file.diff ? file.added : fallbackLines.filter((line) => line.kind === "added").length || file.added,
        removed: file.diff ? file.removed : fallbackLines.filter((line) => line.kind === "removed").length || file.removed,
        diff: file.diff ?? git?.diff,
        loaded: true,
      };
    });
  });
  const selectedFile = $derived(files.find((file) => file.path === selectedPath) ?? files[0] ?? null);
  const selectedFileHasCapturedDiff = $derived(Boolean(
    selectedTurn?.files.find((file) => file.path === selectedFile?.path)?.diff,
  ));
  const selectedFileUsesGitFallback = $derived(!gitMode && Boolean(selectedFile?.diff) && !selectedFileHasCapturedDiff);
  const diffLines = $derived(selectedFile?.diff ? parseReviewDiff(selectedFile.diff) : []);
  const cells = $derived(presentLines(diffLines.filter((line) => line.kind !== "meta"), selectedFile?.path ?? ""));
  const expandedSet = $derived(new Set(expandedFolds));
  const unifiedRows = $derived(foldUnchanged(cells, (cell) => cell.line.kind === "added" || cell.line.kind === "removed", (cell) => cell.line.kind === "context", expandedSet));
  const splitRows = $derived(foldUnchanged(
    pairForSplit(cells),
    (pair) => pair.left?.line.kind === "removed" || pair.right?.line.kind === "added",
    (pair) => pair.left?.line.kind === "context",
    expandedSet,
  ));
  const totalAdded = $derived(files.reduce((sum, file) => sum + file.added, 0));
  const totalRemoved = $derived(files.reduce((sum, file) => sum + file.removed, 0));
  const reviewedKey = $derived(`lume:review:reviewed:${session.id}:${turnKey}`);
  const commentsStoreKey = $derived(`lume:review:comments:${session.id}:${turnKey}`);
  const commentIndex = $derived(indexComments(comments));
  const pendingComments = $derived(comments.filter((comment) => !comment.sentAt));
  const reviewedCount = $derived(files.filter((file) => reviewedPaths.includes(file.path)).length);
  const selectedReviewed = $derived(Boolean(selectedFile && reviewedPaths.includes(selectedFile.path)));
  const answerSummary = $derived.by(() => {
    const text = selectedTurn?.result?.response.trim() ?? "";
    if (!text) return "";
    const paragraphs = text.split(/\n\s*\n/);
    return paragraphs[0].length > 420 ? paragraphs[0].slice(0, 419).trimEnd() + "…" : paragraphs[0];
  });
  const workflowLink = $derived.by(() => {
    const resultId = selectedTurn?.result?.id;
    if (!resultId) return null;
    for (const record of workflowHistory) {
      const step = record.run.steps.find((candidate) => candidate.resultId === resultId);
      if (!step) continue;
      const captured = record.steps.find((candidate) => candidate.stepId === step.stepId);
      return { objective: record.run.objective, role: captured?.roleLabel ?? tr("Workflow step", "Etapa do workflow"), workflowId: record.run.workflowId, runId: record.run.id, stepId: step.stepId, record };
    }
    return null;
  });
  // The handoff leaving this result's step is waiting for the review: approving can release it.
  const handoff = $derived.by(() => {
    const link = workflowLink;
    if (!link || !liveRun || liveRun.id !== link.runId || liveRun.status !== "waiting_for_approval") return null;
    const connection = link.record.group.connections.find((item) => item.id === liveRun?.pendingConnectionId && item.fromStepId === link.stepId);
    if (!connection) return null;
    return { workflowId: link.workflowId, target: link.record.steps.find((step) => step.stepId === connection.toStepId)?.roleLabel ?? "" };
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
    let stop: (() => void) | undefined;
    void listen<WorkflowRun>("lume://workflow-run-changed", ({ payload }) => {
      if (payload.workflowId !== workflowLink?.workflowId) return;
      liveRun = payload;
      void loadWorkflowHistory(30).then((history) => { if (mounted) workflowHistory = history; }).catch(() => {});
    }).then((unlisten) => { if (mounted) stop = unlisten; else unlisten(); }).catch(() => {});
    return () => { mounted = false; stop?.(); };
  });

  $effect(() => {
    const workflowId = workflowLink?.workflowId;
    if (!workflowId) { liveRun = null; return; }
    void loadWorkflowRun(workflowId).then((run) => { if (workflowLink?.workflowId === workflowId) liveRun = run; }).catch(() => {});
  });

  $effect(() => {
    if (!gitMode) return;
    gitState = "loading";
    gitDiffs = {};
    const sessionId = session.id;
    void getRepositorySnapshot(sessionId, true)
      .then((snapshot) => { if (gitMode && session.id === sessionId) { gitFiles = snapshot.files; gitState = "ready"; } })
      .catch((error) => { gitState = "error"; gitMessage = repositoryError(String(error).replace(/^Error:\s*/, ""), language === "pt-BR"); });
  });

  $effect(() => {
    // Load missing captured diffs from the repository; this is a working-tree
    // fallback, not a historical snapshot of the selected prompt.
    if (!selectedFile || gitDiffs[selectedFile.path]) return;
    if (gitMode || !selectedFileHasCapturedDiff) void loadGitDiff(selectedFile.path);
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
    const turnId = turnKey;
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

  $effect(() => {
    const key = reviewedKey;
    try { reviewedPaths = JSON.parse(localStorage.getItem(key) ?? "[]"); } catch { reviewedPaths = []; }
  });

  $effect(() => {
    void selectedPath;
    expandedFolds = [];
    diffScroller?.scrollTo({ top: 0 });
  });

  $effect(() => {
    const key = commentsStoreKey;
    composer = null;
    try { comments = parseStoredComments(localStorage.getItem(key)); } catch { comments = []; }
  });

  function storeComments(next: ReviewComment[]) {
    comments = next;
    try { localStorage.setItem(commentsStoreKey, JSON.stringify(next)); } catch { /* comments are a convenience until sent */ }
  }

  function anchorOf(line?: ReviewDiffLine): { side: CommentSide; number: number } | null {
    if (!line || line.kind === "meta" || line.kind === "hunk") return null;
    if (line.kind === "removed") return line.oldLine === undefined ? null : { side: "old", number: line.oldLine };
    return line.newLine === undefined ? null : { side: "new", number: line.newLine };
  }

  function openComposer(side: CommentSide, number: number, excerpt: string, existing?: ReviewComment) {
    if (!selectedFile) return;
    composer = { path: selectedFile.path, side, line: number, excerpt, body: existing?.body ?? "", editId: existing?.id };
  }

  function saveComposer() {
    const draft = composer;
    const body = draft?.body.trim();
    if (!draft || !body) return;
    if (draft.editId) storeComments(comments.map((comment) => comment.id === draft.editId ? { ...comment, body } : comment));
    else storeComments([...comments, { id: crypto.randomUUID(), path: draft.path, side: draft.side, line: draft.line, body, excerpt: draft.excerpt.slice(0, 200), createdAt: Date.now() }]);
    composer = null;
  }

  function deleteComment(id: string) {
    storeComments(comments.filter((comment) => comment.id !== id));
  }

  function commentCount(path: string) {
    return pendingComments.filter((comment) => comment.path === path).length;
  }

  function storeReviewed(paths: string[]) {
    reviewedPaths = paths;
    try { localStorage.setItem(reviewedKey, JSON.stringify(paths)); } catch { /* the marks are a convenience */ }
  }

  function toggleReviewed(path: string) {
    storeReviewed(reviewedPaths.includes(path) ? reviewedPaths.filter((item) => item !== path) : [...reviewedPaths, path]);
  }

  function moveFile(step: number) {
    if (!files.length) return;
    const index = files.findIndex((file) => file.path === selectedFile?.path);
    selectedPath = files[(index + step + files.length) % files.length].path;
  }

  function goToNextUnreviewed(from = files.findIndex((file) => file.path === selectedFile?.path)) {
    for (let step = 1; step <= files.length; step += 1) {
      const candidate = files[(from + step) % files.length];
      if (!reviewedPaths.includes(candidate.path)) { selectedPath = candidate.path; return; }
    }
  }

  function markReviewedAndAdvance() {
    if (!selectedFile) return;
    const path = selectedFile.path;
    const index = files.findIndex((file) => file.path === path);
    if (reviewedPaths.includes(path)) { toggleReviewed(path); return; }
    toggleReviewed(path);
    goToNextUnreviewed(index);
  }

  function expandFold(id: string) {
    expandedFolds = [...expandedFolds, id];
  }

  function handleKeys(event: KeyboardEvent) {
    const target = event.target as HTMLElement | null;
    if (event.metaKey || event.ctrlKey || event.altKey || target?.closest("input, textarea, select, [contenteditable]")) return;
    if (viewMode !== "changes") return;
    if (event.key === "j" || event.key === "]") { event.preventDefault(); moveFile(1); }
    else if (event.key === "k" || event.key === "[") { event.preventDefault(); moveFile(-1); }
    else if (event.key === "v") { event.preventDefault(); markReviewedAndAdvance(); }
    else if (event.key === "n") { event.preventDefault(); goToNextUnreviewed(); }
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

  function repositoryPath(path: string) {
    const root = session.workingDirectory?.replace(/[\\/]+$/, "") ?? "";
    return root && path.startsWith(root + "/") ? path.slice(root.length + 1) : path;
  }

  async function loadGitDiff(path: string) {
    gitDiffs = { ...gitDiffs, [path]: { error: "" } };
    try {
      const result = await getRepositoryDiff(session.id, repositoryPath(path));
      gitDiffs = { ...gitDiffs, [path]: { diff: result.diff, binary: result.binary } };
    } catch (error) {
      const code = String(error).replace(/^Error:\s*/, "");
      gitDiffs = { ...gitDiffs, [path]: { error: code === "file_no_longer_changed"
        ? tr("Git shows no pending changes for this file (it may already be committed).", "O git não mostra mudanças pendentes neste arquivo (ele pode já ter sido commitado).")
        : repositoryError(code, language === "pt-BR") } };
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
      if (handoff) {
        liveRun = await approveWorkflowHandoff(handoff.workflowId);
        reviewMessage = tr("Result approved and handoff released", "Resultado aprovado e handoff liberado");
      }
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
    const pending = pendingComments;
    if ((!turn?.result && !gitMode) || correctionSending || (!feedback && !pending.length)) return;
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
      const originalTask = cleanPromptTransport(turn?.prompt?.detail ?? "").split(/\r?\n/, 1)[0].slice(0, 240);
      const commentsText = formatCommentsForPrompt(pending);
      const request = [
        turn?.result ? `Review correction for result ${turn.result.id}${originalTask ? ` (task: ${originalTask})` : ""}.` : "Review correction for the pending changes in the git working tree.",
        "Please apply the following review feedback:",
        feedback,
        commentsText,
      ].filter(Boolean).join("\n\n");
      await submitPrompt(session.id, request, [], delivery);
      const decision = turn?.result
        ? await setReviewDecision(session.id, turn.result.id, "changes_requested", [feedback, commentsText].filter(Boolean).join("\n\n"))
        : null;
      const sentAt = Date.now();
      storeComments(comments.map((comment) => pending.some((item) => item.id === comment.id) ? { ...comment, sentAt } : comment));
      if (decision) reviewDecisions = [decision, ...reviewDecisions.filter((item) => item.resultId !== decision.resultId)];
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

{#snippet threads(side: CommentSide, number: number)}
  {#each commentIndex.get(commentKey(selectedFile?.path ?? "", side, number)) ?? [] as comment (comment.id)}
    <div class="comment-card" class:sent={Boolean(comment.sentAt)}>
      <LumeIcon name="note" size={13} />
      <p>{comment.body}</p>
      <span class="comment-actions">
        {#if comment.sentAt}<small>{tr("Sent", "Enviado")}</small>{:else}
          <button type="button" title={tr("Edit", "Editar")} aria-label={tr("Edit comment", "Editar comentário")} onclick={() => openComposer(comment.side, comment.line, comment.excerpt, comment)}><LumeIcon name="rename" size={12} /></button>
        {/if}
        <button type="button" title={tr("Delete", "Excluir")} aria-label={tr("Delete comment", "Excluir comentário")} onclick={() => deleteComment(comment.id)}><LumeIcon name="trash" size={12} /></button>
      </span>
    </div>
  {/each}
  {#if composer && composer.path === selectedFile?.path && composer.side === side && composer.line === number}
    <form class="comment-composer" onsubmit={(event) => { event.preventDefault(); saveComposer(); }}>
      <!-- svelte-ignore a11y_autofocus -->
      <textarea autofocus rows="3" aria-label={tr("Comment on this line", "Comentário nesta linha")} placeholder={tr("What should change here?", "O que deve mudar aqui?")} value={composer.body}
        oninput={(event) => { if (composer) composer.body = event.currentTarget.value; }}
        onkeydown={(event) => { if (event.key === "Escape") { event.stopPropagation(); composer = null; } else if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) { event.preventDefault(); saveComposer(); } }}></textarea>
      <span><small>{tr("Ctrl+Enter to save", "Ctrl+Enter para salvar")}</small><button type="button" class="comment-cancel" onclick={() => composer = null}>{tr("Cancel", "Cancelar")}</button><button type="submit" class="comment-save" disabled={!composer.body.trim()}>{tr("Comment", "Comentar")}</button></span>
    </form>
  {/if}
{/snippet}

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section class:correction-open={correctionOpen} class:changes-view={viewMode === "changes"} class="review-center" aria-label={tr("Review center", "Central de revisão")} tabindex="-1" onkeydown={handleKeys}>
  <SystemBannerStack items={systemBanners} contained {language} dismissLabel={tr("Dismiss", "Fechar")} />
  <header class="review-header">
    <span class="review-title"><i><LumeIcon name="diff" size={16} /></i><strong>{tr("Review", "Revisão")}</strong></span>
    <div class="turn-picker">
      <LumeSelect
        ariaLabel={tr("Choose a task to review", "Escolha uma tarefa para revisar")}
        value={gitMode ? GIT_TURN : selectedTurn?.id ?? ""}
        options={[...turns.map((turn) => ({ value: turn.id, label: turnLabel(turn) })), { value: GIT_TURN, label: tr("Git · pending changes", "Git · mudanças pendentes") }]}
        minWidth={220}
        onValueChange={(value) => void selectTurn(value)}
      />
    </div>
    {#if workflowLink}<span class="workflow-tag" title={workflowLink.objective}><LumeIcon name="bolt" size={11} />{workflowLink.role}</span>{/if}
    <div class="mode-switch" role="group" aria-label={tr("Diff layout", "Layout do diff")}>
      <button class:active={mode === "unified"} type="button" aria-pressed={mode === "unified"} title={tr("Unified", "Unificado")} onclick={() => mode = "unified"}><LumeIcon name="diff" size={13} /><span>{tr("Unified", "Unificado")}</span></button>
      <button class:active={mode === "split"} type="button" aria-pressed={mode === "split"} title={tr("Split", "Lado a lado")} onclick={() => mode = "split"}><LumeIcon name="columns" size={13} /><span>{tr("Split", "Lado a lado")}</span></button>
    </div>
    {#if onToggleWide}<button class:active={wide} class="icon-button" type="button" title={wide ? tr("Show the chat", "Mostrar o chat") : tr("Use the full window", "Usar a janela inteira")} aria-label={wide ? tr("Show the chat", "Mostrar o chat") : tr("Use the full window", "Usar a janela inteira")} aria-pressed={wide} onclick={onToggleWide}><LumeIcon name={wide ? "restore" : "maximize"} size={15} /></button>{/if}
    <button class:active={notesOpen} class="icon-button review-note-toggle" type="button" title={tr("Review note", "Nota de revisão")} aria-label={tr("Review note", "Nota de revisão")} aria-pressed={notesOpen} onclick={() => notesOpen = !notesOpen}><LumeIcon name="note" size={15} />{#if selectedReviewNote}<i aria-hidden="true"></i>{/if}</button>
    <button class="icon-button close-review" type="button" title={tr("Close review", "Fechar revisão")} aria-label={tr("Close review", "Fechar revisão")} onclick={() => void closeReviewCenter()}><LumeIcon name="close" size={16} /></button>
  </header>

  <div class="review-summary">
    <nav class="review-views" aria-label={tr("Review views", "Visualizações da revisão")}>
      <button class:active={viewMode === "changes"} type="button" aria-pressed={viewMode === "changes"} onclick={() => viewMode = "changes"}><span>{tr("Changes", "Alterações")}</span><small>{files.length}</small></button>
      <button class:active={viewMode === "context"} type="button" aria-pressed={viewMode === "context"} onclick={() => viewMode = "context"}><span>{tr("Context", "Contexto")}</span>{#if selectedTurn?.checks.length}<small>{selectedTurn.checks.length}</small>{/if}</button>
      <button class:active={viewMode === "deliverables"} type="button" aria-pressed={viewMode === "deliverables"} onclick={() => viewMode = "deliverables"}><span>{tr("Deliverables", "Entregáveis")}</span><small>{deliverableFiles.length}</small></button>
    </nav>
    {#if files.length}
      <span class="totals" title={tr("Lines added and removed", "Linhas adicionadas e removidas")}><b>+{totalAdded}</b><i>−{totalRemoved}</i></span>
      <span class="progress" title={tr("Files marked as reviewed", "Arquivos marcados como revisados")}><span class="bar" aria-hidden="true"><i style:width={(reviewedCount / files.length) * 100 + "%"}></i></span>{reviewedCount}/{files.length} {tr("reviewed", "revisados")}</span>
    {/if}
    {#if selectedTurn?.checks.length}
      <button class="checks-chip" type="button" title={selectedTurn.checks.join("\n")} onclick={() => viewMode = "context"}><LumeIcon name="check" size={12} />{selectedTurn.checks.length} {tr(selectedTurn.checks.length === 1 ? "check" : "checks", selectedTurn.checks.length === 1 ? "validação" : "validações")}</button>
    {/if}
    {#if pendingComments.length}
      <button class="comments-chip" type="button" title={tr("Open the request with these comments", "Abrir o pedido com estes comentários")} onclick={() => correctionOpen = true}><LumeIcon name="note" size={12} />{pendingComments.length} {tr(pendingComments.length === 1 ? "comment" : "comments", pendingComments.length === 1 ? "comentário" : "comentários")}</button>
    {/if}
    {#if handoff}<span class="handoff-chip" title={tr("This result's workflow handoff is waiting for your approval", "O handoff do workflow deste resultado aguarda sua aprovação")}><LumeIcon name="shield" size={12} />{tr("Handoff pending", "Handoff pendente")}{#if handoff.target} → {handoff.target}{/if}</span>{/if}
    <div class="review-decisions" aria-label={tr("Result decision", "Decisão sobre o resultado")}>
      {#if selectedReviewDecision}
        <span class="decision-state {selectedReviewDecision.decision}"><LumeIcon name={selectedReviewDecision.decision === "approved" ? "check" : "note"} size={12} />{selectedReviewDecision.decision === "approved" ? tr("Approved", "Aprovado") : tr("Changes requested", "Correções solicitadas")}</span>
      {/if}
      <button class="request-changes" type="button" disabled={(!selectedTurn?.result && !gitMode) || decisionSaving || correctionSending} onclick={() => correctionOpen = !correctionOpen}><LumeIcon name="refresh" size={13} /><span>{tr("Request changes", "Solicitar correções")}</span></button>
      <button class="approve-result" type="button" title={handoff ? tr(`Approves the result and releases the handoff${handoff.target ? " to " + handoff.target : ""}.`, `Aprova o resultado e libera o handoff${handoff.target ? " para " + handoff.target : ""}.`) : tr("Records a review decision; it does not advance a workflow handoff.", "Registra a revisão; não avança o handoff do workflow.")} disabled={!selectedTurn?.result || decisionSaving || correctionSending} onclick={() => void approveSelectedResult()}><LumeIcon name={handoff ? "arrow-right" : "check"} size={13} /><span>{decisionSaving ? tr("Saving…", "Salvando…") : handoff ? tr("Approve and advance", "Aprovar e avançar") : tr("Approve", "Aprovar")}</span></button>
    </div>
  </div>

  {#if correctionOpen}
    <form class="correction-request" onsubmit={(event) => { event.preventDefault(); void sendCorrectionRequest(); }}>
      <label for="review-correction">{tr("What should be corrected?", "O que precisa ser corrigido?")}</label>
      {#if pendingComments.length}
        <p class="correction-comments"><LumeIcon name="note" size={12} />{pendingComments.length} {tr(pendingComments.length === 1 ? "line comment goes with this request" : "line comments go with this request", pendingComments.length === 1 ? "comentário de linha vai junto" : "comentários de linha vão junto")}: {[...new Set(pendingComments.map((comment) => comment.path.split("/").pop()))].join(", ")}</p>
      {/if}
      <textarea id="review-correction" value={correctionDraft} disabled={correctionSending} placeholder={pendingComments.length ? tr("Optional: add general feedback…", "Opcional: adicione um comentário geral…") : tr("Describe the changes to make…", "Descreva as correções necessárias…")} oninput={(event) => correctionDraft = event.currentTarget.value}></textarea>
      <div class="correction-footer">
        <small>{!session.capabilities.canPrompt
          ? tr("Take control of this session to send a correction request.", "Assuma o controle desta sessão para enviar o pedido de correção.")
          : session.status === "running" && !session.capabilities.promptDeliveries.includes("queue")
            ? tr("This session cannot queue a correction while running.", "Esta sessão não pode enfileirar correções enquanto está executando.")
            : session.status === "permission_required"
              ? tr("Resolve the pending permission first.", "Resolva a permissão pendente primeiro.")
              : tr("The request is sent as a new prompt and recorded for this result.", "O pedido é enviado como novo prompt e registrado para este resultado.")}</small>
        <button type="button" class="cancel-correction" disabled={correctionSending} onclick={() => correctionOpen = false}>{tr("Cancel", "Cancelar")}</button>
        <button type="submit" class="send-correction" disabled={correctionSending || (!correctionDraft.trim() && !pendingComments.length) || !session.capabilities.canPrompt || session.status === "permission_required" || (session.status === "running" && !session.capabilities.promptDeliveries.includes("queue"))}><LumeIcon name="arrow-down" size={13} />{correctionSending ? tr("Sending…", "Enviando…") : tr("Send request", "Enviar pedido")}</button>
      </div>
    </form>
  {/if}

  <div class:notes-open={notesOpen} class:simple-view={viewMode !== "changes"} class="review-body">
    {#if viewMode === "changes"}
      <aside class="review-files" aria-label={tr("Changed files", "Arquivos alterados")}>
        <nav>
          {#each files as file (file.path)}
            {@const done = reviewedPaths.includes(file.path)}
            <div class="file-row" class:active={selectedFile?.path === file.path} class:done>
              <button class="file-check" type="button" role="checkbox" aria-checked={done} title={done ? tr("Reviewed — click to undo", "Revisado — clique para desfazer") : tr("Mark as reviewed", "Marcar como revisado")} aria-label={tr("Reviewed", "Revisado") + ": " + file.path} onclick={() => toggleReviewed(file.path)}>{#if done}<LumeIcon name="check" size={11} />{/if}</button>
              <button class="file-open" type="button" title={file.path} onclick={() => selectedPath = file.path}>
                <FileTypeIcon path={file.path} />
                <span>{displayFileChangePath(file.path)}</span>
                {#if commentCount(file.path)}<em class="file-comments" title={tr("Pending comments", "Comentários pendentes")}>{commentCount(file.path)}</em>{/if}
                {#if file.loaded}<b>+{file.added}</b><i>−{file.removed}</i>{/if}
              </button>
            </div>
          {:else}
            <p>{gitMode
              ? gitState === "loading" ? tr("Reading the repository…", "Lendo o repositório…") : gitState === "error" ? gitMessage : tr("The git working tree has no pending changes.", "A árvore de trabalho do git não tem mudanças pendentes.")
              : selectedTurn && !selectedTurn.activityAvailable
                ? tr("This result is older than the loaded activity history.", "Este resultado é mais antigo que o histórico de atividades carregado.")
                : tr("No file changes were captured for this task.", "Nenhuma alteração foi capturada nesta tarefa.")}</p>
            {#if !gitMode}<button class="empty-action" type="button" onclick={() => void selectTurn(GIT_TURN)}>{tr("See pending changes in git", "Ver mudanças pendentes no git")}</button>{/if}
          {/each}
        </nav>
        {#if files.length}<footer><kbd>j</kbd><kbd>k</kbd> {tr("files", "arquivos")} · <kbd>v</kbd> {tr("reviewed", "revisado")}</footer>{/if}
      </aside>

      <main class="diff-workspace" class:has-answer={Boolean(answerSummary) || gitMode}>
        {#if gitMode || selectedFileUsesGitFallback}
          <p class="git-note"><LumeIcon name="branch" size={12} /> {gitMode
            ? tr("Pending changes in the git working tree (compared with HEAD), not tied to one result.", "Mudanças pendentes na árvore de trabalho do git (comparadas com o HEAD), sem vínculo com um resultado.")
            : tr("This diff is from the current git working tree; it may include changes beyond this prompt.", "Este diff vem da árvore de trabalho atual do git e pode incluir mudanças além deste prompt.")}</p>
        {/if}
        {#if !gitMode && answerSummary}
          <div class="answer-strip" class:open={answerOpen}>
            <button type="button" aria-expanded={answerOpen} onclick={() => answerOpen = !answerOpen}><span class="turn-chevron"><LumeIcon name="chevron-down" size={12} /></span><strong>{tr("Agent's summary", "Resumo do agente")}</strong>{#if !answerOpen}<span>{answerSummary.replace(/\s+/g, " ")}</span>{/if}</button>
            {#if answerOpen}<div class="reading-copy">{@html renderSafeMarkdown(selectedTurn?.result?.response ?? "")}</div>{/if}
          </div>
        {/if}
        {#if selectedFile}
          <header class="diff-header">
            <span class="diff-path"><FileTypeIcon path={selectedFile.path} /><strong title={selectedFile.path}>{selectedFile.path}</strong></span>
            <span class="change-count"><b>+{selectedFile.added}</b><i>−{selectedFile.removed}</i></span>
            <span class="file-nav">
              <button type="button" title={tr("Previous file (k)", "Arquivo anterior (k)")} aria-label={tr("Previous file", "Arquivo anterior")} disabled={files.length < 2} onclick={() => moveFile(-1)}><span class="flip"><LumeIcon name="chevron-down" size={13} /></span></button>
              <button type="button" title={tr("Next file (j)", "Próximo arquivo (j)")} aria-label={tr("Next file", "Próximo arquivo")} disabled={files.length < 2} onclick={() => moveFile(1)}><LumeIcon name="chevron-down" size={13} /></button>
            </span>
            <button class="mark-reviewed" class:done={selectedReviewed} type="button" title="v" onclick={markReviewedAndAdvance}><LumeIcon name="check" size={12} />{selectedReviewed ? tr("Reviewed", "Revisado") : tr("Mark reviewed", "Marcar revisado")}</button>
          </header>
          {#if diffLines.length}
            <div class="diff-scroll" bind:this={diffScroller}>
              {#if mode === "unified"}
                <div class="unified-diff" aria-label={tr("Unified diff", "Diff unificado")}>
                  {#each unifiedRows as row (row.type === "fold" ? "f" + row.id : row.item)}
                    {#if row.type === "fold"}
                      <button class="fold-row" type="button" onclick={() => expandFold(row.id)}><LumeIcon name="chevron-down" size={12} />{row.count} {tr("unchanged lines", "linhas sem mudança")}</button>
                    {:else}
                      {@const line = row.item.line}
                      {@const anchor = anchorOf(line)}
                      <div class="diff-line line-{line.kind}"><span class="num">{#if anchor?.side === "old"}<button class="add-comment" type="button" title={tr("Comment on this line", "Comentar esta linha")} aria-label={tr("Comment on this line", "Comentar esta linha")} onclick={() => openComposer("old", anchor.number, line.content)}>+</button>{/if}{line.oldLine ?? ""}</span><span class="num">{#if anchor?.side === "new"}<button class="add-comment" type="button" title={tr("Comment on this line", "Comentar esta linha")} aria-label={tr("Comment on this line", "Comentar esta linha")} onclick={() => openComposer("new", anchor.number, line.content)}>+</button>{/if}{line.newLine ?? ""}</span><code>{line.kind === "added" ? "+" : line.kind === "removed" ? "−" : " "}{#each row.item.segments as part}<span class="tk-{part.kind}" class:chg={part.changed}>{part.text}</span>{/each}</code></div>
                      {#if anchor}{@render threads(anchor.side, anchor.number)}{/if}
                    {/if}
                  {/each}
                </div>
              {:else}
                <div class="split-diff" aria-label={tr("Side by side diff", "Diff lado a lado")}>
                  {#each splitRows as row (row.type === "fold" ? "f" + row.id : row.item)}
                    {#if row.type === "fold"}
                      <button class="fold-row" type="button" onclick={() => expandFold(row.id)}><LumeIcon name="chevron-down" size={12} />{row.count} {tr("unchanged lines", "linhas sem mudança")}</button>
                    {:else}
                      {#each [row.item.left, row.item.right] as cell, side}
                        {@const anchor = side === 0 ? (cell?.line.kind === "removed" ? anchorOf(cell.line) : null) : (cell && cell.line.kind !== "removed" ? anchorOf(cell.line) : null)}
                        <div class="split-cell line-{cell?.line.kind ?? "empty"}"><span class="num">{#if anchor && cell}<button class="add-comment" type="button" title={tr("Comment on this line", "Comentar esta linha")} aria-label={tr("Comment on this line", "Comentar esta linha")} onclick={() => openComposer(anchor.side, anchor.number, cell.line.content)}>+</button>{/if}{cell && cell.line.kind !== "meta" && cell.line.kind !== "hunk" ? (side === 0 ? cell.line.oldLine ?? "" : cell.line.newLine ?? "") : ""}</span><code>{cell ? (cell.line.kind === "removed" ? "−" : cell.line.kind === "added" ? "+" : " ") : " "}{#if cell}{#each cell.segments as part}<span class="tk-{part.kind}" class:chg={part.changed}>{part.text}</span>{/each}{/if}</code></div>
                      {/each}
                      {@const leftAnchor = row.item.left?.line.kind === "removed" ? anchorOf(row.item.left.line) : null}
                      {@const rightAnchor = row.item.right && row.item.right.line.kind !== "removed" ? anchorOf(row.item.right.line) : null}
                      {#if leftAnchor || rightAnchor}
                        <div class="split-threads">
                          {#if leftAnchor}{@render threads(leftAnchor.side, leftAnchor.number)}{/if}
                          {#if rightAnchor}{@render threads(rightAnchor.side, rightAnchor.number)}{/if}
                        </div>
                      {/if}
                    {/if}
                  {/each}
                </div>
              {/if}
            </div>
          {:else}
            {@const gitEntry = gitDiffs[selectedFile.path]}
            <div class="diff-empty"><LumeIcon name="file" size={25} />
              {#if gitEntry && "error" in gitEntry && gitEntry.error}
                <strong>{tr("Could not read the diff", "Não foi possível ler o diff")}</strong><p>{gitEntry.error}</p>
              {:else if gitEntry && "binary" in gitEntry && gitEntry.binary}
                <strong>{tr("Binary file", "Arquivo binário")}</strong><p>{tr("Git does not show a text diff for this file.", "O git não mostra um diff de texto para este arquivo.")}</p>
              {:else if gitEntry}
                <strong>{tr("Reading the diff…", "Lendo o diff…")}</strong>
              {:else}
                <strong>{tr("Summary available", "Resumo disponível")}</strong><p>{tr("Lume captured the file and line totals, but this source did not provide the full diff.", "O Lume capturou o arquivo e os totais, mas esta origem não forneceu o diff completo.")}</p>
                <button class="empty-action" type="button" onclick={() => void loadGitDiff(selectedFile.path)}>{tr("Load the diff from git", "Carregar o diff do git")}</button>
              {/if}
            </div>
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
  .review-center { position: relative; min-width: 0; min-height: 0; width: 100%; height: 100%; container: review / inline-size; display: grid; grid-template-rows: auto auto minmax(0, 1fr); overflow: hidden; outline: 0; color: var(--workspace-text); background: var(--workspace-raised); }
  .review-center.correction-open { grid-template-rows: auto auto auto minmax(0, 1fr); }
  .review-center:not(.changes-view) .mode-switch { display: none; }
  button { font-family: inherit; }
  .review-header { min-height: 50px; padding: 7px 10px 7px 14px; display: flex; align-items: center; gap: 9px; border-bottom: 1px solid var(--workspace-line); }
  .review-title { display: flex; align-items: center; gap: 7px; flex: 0 0 auto; color: var(--workspace-accent); }
  .review-title strong { color: var(--workspace-strong); font-size: 13px; }
  .turn-picker { min-width: 0; flex: 1 1 auto; max-width: 460px; }
  .turn-picker :global(.lume-select) { width: 100%; }
  .no-turns { color: var(--workspace-faint); font-size: 12px; }
  .workflow-tag { max-width: 150px; min-height: 22px; padding: 0 8px; display: inline-flex; align-items: center; gap: 5px; overflow: hidden; border-radius: 999px; color: var(--workspace-accent); background: var(--workspace-accent-soft); font-size: 11px; font-weight: 650; text-overflow: ellipsis; white-space: nowrap; }
  .mode-switch { margin-left: auto; padding: 3px; display: flex; border: 1px solid var(--workspace-line); border-radius: 9px; background: var(--workspace-subtle); }
  .mode-switch button { height: 27px; padding: 0 9px; display: flex; align-items: center; gap: 5px; border: 0; border-radius: 6px; color: var(--workspace-muted); background: transparent; font-size: 11px; font-weight: 650; cursor: pointer; }
  .mode-switch button.active { color: var(--workspace-accent); background: var(--workspace-raised); box-shadow: 0 1px 4px rgba(18, 34, 25, .1); }
  .icon-button { position: relative; width: 31px; height: 31px; padding: 0; display: grid; place-items: center; flex: 0 0 auto; border: 0; border-radius: 9px; color: var(--workspace-muted); background: transparent; cursor: pointer; }
  .icon-button:hover, .icon-button.active { color: var(--workspace-strong); background: var(--workspace-subtle); }
  .icon-button.active { color: var(--workspace-accent); }
  .review-note-toggle > i { position: absolute; top: 6px; right: 6px; width: 5px; height: 5px; border-radius: 50%; background: var(--workspace-accent); }
  .review-summary { min-width: 0; min-height: 44px; padding: 5px 10px 5px 12px; display: flex; flex-wrap: wrap; align-items: center; gap: 6px 12px; border-bottom: 1px solid var(--workspace-line); background: var(--workspace-pane); }
  .review-views { display: flex; gap: 2px; }
  .review-views button { min-height: 30px; padding: 0 10px; display: inline-flex; align-items: center; gap: 6px; border: 0; border-radius: 8px; color: var(--workspace-muted); background: transparent; font-size: 12px; font-weight: 650; white-space: nowrap; cursor: pointer; transition: color 140ms ease, background 140ms ease; }
  .review-views button:hover { color: var(--workspace-strong); background: var(--workspace-subtle); }
  .review-views button.active { color: var(--workspace-accent); background: var(--workspace-accent-soft); }
  .review-views button small { min-width: 16px; padding: 1px 5px; border-radius: 5px; color: var(--workspace-muted); background: var(--workspace-subtle); font-size: 10px; text-align: center; }
  .totals { display: inline-flex; gap: 6px; font: 650 12px var(--lume-font-code, ui-monospace, monospace); font-variant-numeric: tabular-nums; }
  .totals b, .change-count b { color: #438f67; font-weight: 700; }
  .totals i, .change-count i { color: #b96862; font-style: normal; font-weight: 700; }
  .progress { display: inline-flex; align-items: center; gap: 7px; color: var(--workspace-muted); font-size: 11px; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .progress .bar { position: relative; width: 54px; height: 5px; overflow: hidden; border-radius: 999px; background: var(--workspace-line); }
  .progress .bar i { position: absolute; inset: 0 auto 0 0; border-radius: inherit; background: var(--workspace-accent); transition: width 240ms ease; }
  .checks-chip { min-height: 24px; padding: 0 9px; display: inline-flex; align-items: center; gap: 5px; border: 1px solid color-mix(in srgb, #438f67 40%, transparent); border-radius: 999px; color: #438f67; background: color-mix(in srgb, #438f67 10%, transparent); font-size: 11px; font-weight: 650; cursor: pointer; }
  .review-decisions { margin-left: auto; min-width: 0; display: flex; align-items: center; justify-content: flex-end; gap: 5px; }
  .review-decisions button { min-height: 30px; padding: 0 11px; display: inline-flex; align-items: center; justify-content: center; gap: 6px; border: 1px solid transparent; border-radius: 8px; color: var(--workspace-muted); background: transparent; font-size: 12px; font-weight: 700; white-space: nowrap; cursor: pointer; transition: color 140ms ease, background 140ms ease, transform 140ms ease; }
  .review-decisions button:hover:not(:disabled) { color: var(--workspace-strong); background: var(--workspace-subtle); }
  .review-decisions button:active:not(:disabled) { transform: scale(.97); }
  .review-decisions .request-changes { border-color: var(--workspace-line); background: var(--workspace-raised); }
  .review-decisions .approve-result { color: var(--workspace-raised); background: var(--workspace-accent); }
  .review-decisions .approve-result:hover:not(:disabled) { color: var(--workspace-raised); background: color-mix(in srgb, var(--workspace-accent) 88%, black); }
  .review-decisions button:disabled { opacity: .42; cursor: default; }
  .decision-state { display: inline-flex; align-items: center; gap: 4px; padding: 0 6px; color: var(--workspace-muted); font-size: 11px; white-space: nowrap; }
  .decision-state.approved { color: #438f67; } .decision-state.changes_requested { color: #bd8434; }
  .correction-request { min-width: 0; padding: 10px 12px; display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 6px 10px; border-bottom: 1px solid var(--workspace-line); background: var(--workspace-subtle); }
  .correction-request label { color: var(--workspace-strong); font-size: 12px; font-weight: 700; }
  .correction-request textarea { box-sizing: border-box; min-width: 0; min-height: 64px; grid-column: 1 / -1; padding: 8px 10px; resize: vertical; border: 1px solid var(--workspace-line); border-radius: 8px; outline: 0; color: var(--workspace-text); background: var(--workspace-raised); font: inherit; font-size: 12px; line-height: 1.5; }
  .correction-request textarea:focus { border-color: color-mix(in srgb, var(--workspace-accent) 48%, var(--workspace-line)); box-shadow: 0 0 0 2px var(--workspace-accent-soft); }
  .correction-footer { min-width: 0; grid-column: 1 / -1; display: flex; align-items: center; justify-content: flex-end; gap: 6px; }
  .correction-footer small { min-width: 0; margin-right: auto; overflow: hidden; color: var(--workspace-faint); font-size: 11px; text-overflow: ellipsis; }
  .correction-footer button { min-height: 28px; padding: 0 10px; display: inline-flex; align-items: center; gap: 5px; border: 1px solid var(--workspace-line); border-radius: 7px; color: var(--workspace-muted); background: var(--workspace-raised); font-size: 11px; font-weight: 650; cursor: pointer; }
  .correction-footer .send-correction { color: var(--workspace-raised); border-color: transparent; background: var(--workspace-accent); }
  .correction-footer button:disabled { opacity: .42; cursor: default; }
  .review-body { position: relative; min-width: 0; min-height: 0; display: grid; grid-template-columns: clamp(170px, 26%, 250px) minmax(0, 1fr); }
  .review-body.notes-open { grid-template-columns: clamp(160px, 22%, 215px) minmax(0, 1fr) clamp(220px, 27%, 290px); }
  .review-body.simple-view { grid-template-columns: minmax(0, 1fr); }
  .review-body.simple-view.notes-open { grid-template-columns: minmax(0, 1fr) clamp(220px, 27%, 290px); }
  .review-files { min-width: 0; min-height: 0; padding: 8px 6px 0; display: grid; grid-template-rows: minmax(0, 1fr) auto; overflow: hidden; border-right: 1px solid var(--workspace-line); background: var(--workspace-sidebar); }
  .review-files nav { min-height: 0; display: grid; align-content: start; gap: 1px; overflow-y: auto; scrollbar-width: thin; scrollbar-color: var(--workspace-scroll-thumb) transparent; }
  .file-row { min-width: 0; display: flex; align-items: center; border-radius: 7px; }
  .file-row:hover { background: var(--workspace-subtle); }
  .file-row.active { background: var(--workspace-accent-soft); }
  .file-check { width: 22px; height: 22px; margin: 0 2px 0 3px; padding: 0; display: grid; place-items: center; flex: 0 0 auto; border: 1.5px solid var(--workspace-line); border-radius: 6px; color: var(--workspace-raised); background: transparent; cursor: pointer; }
  .file-check:hover { border-color: var(--workspace-accent); }
  .file-row.done .file-check { border-color: var(--workspace-accent); background: var(--workspace-accent); }
  .file-open { min-width: 0; min-height: 36px; padding: 0 7px 0 3px; display: flex; align-items: center; flex: 1; gap: 6px; border: 0; border-radius: 7px; color: var(--workspace-muted); background: transparent; font-size: 12px; text-align: left; cursor: pointer; }
  .file-row.active .file-open { color: var(--workspace-strong); }
  .file-row.done .file-open > span { opacity: .62; }
  .file-open > span { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .file-open b { color: #438f67; font-size: 10px; }
  .file-open i { color: #b96862; font-size: 10px; font-style: normal; }
  .review-files nav p { padding: 12px 7px; color: var(--workspace-faint); font-size: 12px; line-height: 1.45; }
  .review-files footer { padding: 8px 6px; color: var(--workspace-faint); font-size: 10px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  kbd { padding: 0 4px; border: 1px solid var(--workspace-line); border-radius: 4px; color: var(--workspace-muted); background: var(--workspace-raised); font: 600 10px var(--lume-font-code, ui-monospace, monospace); }
  .diff-workspace { min-width: 0; min-height: 0; display: grid; grid-template-rows: auto minmax(0, 1fr); overflow: hidden; background: var(--workspace-code); }
  .diff-workspace.has-answer { grid-template-rows: auto auto minmax(0, 1fr); }
  .answer-strip { min-width: 0; max-height: 40vh; overflow: auto; border-bottom: 1px solid var(--workspace-line); background: color-mix(in srgb, var(--workspace-accent) 5%, var(--workspace-pane)); }
  .answer-strip > button { width: 100%; min-width: 0; min-height: 32px; padding: 0 12px; display: flex; align-items: center; gap: 7px; border: 0; color: var(--workspace-muted); background: transparent; font-size: 12px; text-align: left; cursor: pointer; }
  .answer-strip > button strong { flex: 0 0 auto; color: var(--workspace-strong); font-size: 12px; }
  .answer-strip > button span:not(.turn-chevron) { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .turn-chevron { display: grid; flex: 0 0 auto; transform: rotate(-90deg); transition: transform 160ms ease; }
  .answer-strip.open .turn-chevron { transform: none; }
  .answer-strip .reading-copy { padding: 2px 14px 12px 31px; }
  .diff-header { min-height: 42px; padding: 0 10px 0 13px; display: flex; align-items: center; gap: 10px; border-bottom: 1px solid var(--workspace-line); background: var(--workspace-pane); }
  .diff-path { min-width: 0; display: flex; align-items: center; gap: 7px; flex: 1; }
  .diff-path strong { overflow: hidden; color: var(--workspace-strong); font: 650 12px/1.2 var(--lume-font-code, ui-monospace, monospace); text-overflow: ellipsis; white-space: nowrap; direction: rtl; text-align: left; }
  .change-count { display: flex; gap: 7px; font: 700 11px var(--lume-font-code, ui-monospace, monospace); }
  .file-nav { display: flex; gap: 2px; }
  .file-nav button { width: 26px; height: 26px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 7px; color: var(--workspace-muted); background: transparent; cursor: pointer; }
  .file-nav button:hover:not(:disabled) { color: var(--workspace-strong); background: var(--workspace-subtle); }
  .file-nav button:disabled { opacity: .35; cursor: default; }
  .flip { display: grid; transform: rotate(180deg); }
  .mark-reviewed { min-height: 28px; padding: 0 10px; display: inline-flex; align-items: center; gap: 5px; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-muted); background: var(--workspace-raised); font-size: 11px; font-weight: 700; white-space: nowrap; cursor: pointer; }
  .mark-reviewed:hover { color: var(--workspace-strong); border-color: color-mix(in srgb, var(--workspace-accent) 45%, var(--workspace-line)); }
  .mark-reviewed.done { color: var(--workspace-accent); border-color: color-mix(in srgb, var(--workspace-accent) 40%, transparent); background: var(--workspace-accent-soft); }
  .diff-scroll { min-width: 0; min-height: 0; overflow: auto; overscroll-behavior: contain; scrollbar-width: thin; scrollbar-color: var(--workspace-scroll-thumb) transparent; }
  .unified-diff { min-width: max-content; }
  .diff-line { min-width: max-content; display: grid; grid-template-columns: 46px 46px minmax(520px, 1fr); font: 12px/1.65 var(--lume-font-code, ui-monospace, SFMono-Regular, Consolas, monospace); }
  .diff-line > span { padding: 0 8px; color: var(--workspace-faint); border-right: 1px solid color-mix(in srgb, var(--workspace-line) 55%, transparent); text-align: right; user-select: none; }
  .diff-line code, .split-cell code { padding: 0 10px; color: var(--workspace-text); white-space: pre; }
  .diff-line code span, .split-cell code span { padding: 0; border: 0; text-align: inherit; user-select: auto; }
  .line-added { background: rgba(61, 143, 98, .14); } .line-removed { background: rgba(179, 83, 78, .14); } .line-hunk { background: rgba(67, 123, 161, .12); } .line-hunk code { color: #70a9cf; } .line-meta code { color: var(--workspace-faint); }
  .line-added .chg { border-radius: 3px; background: rgba(61, 143, 98, .38); }
  .line-removed .chg { border-radius: 3px; background: rgba(179, 83, 78, .38); }
  .tk-kw { color: color-mix(in srgb, #a371e0 72%, var(--workspace-text)); }
  .tk-str { color: color-mix(in srgb, #3f9d6b 72%, var(--workspace-text)); }
  .tk-num { color: color-mix(in srgb, #d9962b 75%, var(--workspace-text)); }
  .tk-fn { color: color-mix(in srgb, #4b8fd1 72%, var(--workspace-text)); }
  .tk-com { color: var(--workspace-faint); font-style: italic; }
  .split-diff { min-width: 640px; display: grid; grid-template-columns: repeat(2, minmax(320px, 1fr)); align-content: start; }
  .split-cell { min-width: 0; display: grid; grid-template-columns: 38px max-content; border-right: 1px solid var(--workspace-line); font: 12px/1.65 var(--lume-font-code, ui-monospace, SFMono-Regular, Consolas, monospace); }
  .split-cell > span { padding: 0 6px; color: var(--workspace-faint); border-right: 1px solid color-mix(in srgb, var(--workspace-line) 55%, transparent); text-align: right; user-select: none; }
  .split-cell.line-empty { background: color-mix(in srgb, var(--workspace-line) 18%, transparent); }
  .split-cell.line-empty code { opacity: .3; }
  .num { position: relative; }
  .add-comment { position: absolute; top: 50%; left: 3px; width: 17px; height: 17px; padding: 0; display: none; place-items: center; border: 0; border-radius: 5px; color: var(--workspace-raised); background: var(--workspace-accent); font: 700 13px/1 var(--lume-font-ui, sans-serif); transform: translateY(-50%); cursor: pointer; }
  .diff-line:hover .add-comment, .split-cell:hover .add-comment, .add-comment:focus-visible { display: grid; }
  .split-threads { grid-column: 1 / -1; min-width: 0; }
  .comment-card, .comment-composer { position: sticky; left: 0; box-sizing: border-box; width: min(560px, 62cqw); margin: 5px 0 6px 96px; border: 1px solid color-mix(in srgb, var(--workspace-accent) 38%, var(--workspace-line)); border-radius: 10px; background: var(--workspace-pane); font-family: var(--lume-font-ui, sans-serif); white-space: normal; }
  .split-threads .comment-card, .split-threads .comment-composer { margin-left: 52px; }
  .comment-card { padding: 8px 9px 8px 10px; display: flex; align-items: flex-start; gap: 8px; color: var(--workspace-text); }
  .comment-card > :global(.lume-icon) { margin-top: 2px; flex: 0 0 auto; color: var(--workspace-accent); }
  .comment-card p { min-width: 0; margin: 0; flex: 1; font-size: 12px; line-height: 1.5; white-space: pre-wrap; overflow-wrap: anywhere; }
  .comment-card.sent { opacity: .6; border-color: var(--workspace-line); }
  .comment-actions { display: flex; align-items: center; gap: 2px; flex: 0 0 auto; }
  .comment-actions small { padding-right: 4px; color: var(--workspace-muted); font-size: 10px; }
  .comment-actions button { width: 22px; height: 22px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 6px; color: var(--workspace-muted); background: transparent; cursor: pointer; }
  .comment-actions button:hover { color: var(--workspace-strong); background: var(--workspace-subtle); }
  .comment-composer { padding: 8px; display: grid; gap: 7px; }
  .comment-composer textarea { box-sizing: border-box; width: 100%; min-height: 62px; padding: 8px 10px; resize: vertical; border: 1px solid var(--workspace-line); border-radius: 8px; outline: 0; color: var(--workspace-text); background: var(--workspace-raised); font: inherit; font-size: 12px; line-height: 1.5; }
  .comment-composer textarea:focus { border-color: color-mix(in srgb, var(--workspace-accent) 55%, var(--workspace-line)); box-shadow: 0 0 0 2px var(--workspace-accent-soft); }
  .comment-composer > span { display: flex; align-items: center; justify-content: flex-end; gap: 6px; }
  .comment-composer small { margin-right: auto; color: var(--workspace-faint); font-size: 10px; }
  .comment-composer button { min-height: 26px; padding: 0 10px; border: 1px solid var(--workspace-line); border-radius: 7px; color: var(--workspace-muted); background: var(--workspace-raised); font-size: 11px; font-weight: 650; cursor: pointer; }
  .comment-composer .comment-save { color: var(--workspace-raised); border-color: transparent; background: var(--workspace-accent); }
  .comment-composer .comment-save:disabled { opacity: .42; cursor: default; }
  .handoff-chip { max-width: 220px; min-height: 24px; padding: 0 9px; display: inline-flex; align-items: center; gap: 5px; overflow: hidden; border: 1px solid color-mix(in srgb, #bd8434 45%, transparent); border-radius: 999px; color: #bd8434; background: color-mix(in srgb, #bd8434 10%, transparent); font-size: 11px; font-weight: 650; text-overflow: ellipsis; white-space: nowrap; }
  .git-note { margin: 0; padding: 7px 12px; display: flex; align-items: center; gap: 6px; color: var(--workspace-muted); background: var(--workspace-pane); border-bottom: 1px solid var(--workspace-line); font-size: 11px; }
  .empty-action { min-height: 28px; padding: 0 12px; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-accent); background: var(--workspace-raised); font-size: 12px; font-weight: 650; cursor: pointer; }
  .comments-chip { min-height: 24px; padding: 0 9px; display: inline-flex; align-items: center; gap: 5px; border: 1px solid color-mix(in srgb, var(--workspace-accent) 40%, transparent); border-radius: 999px; color: var(--workspace-accent); background: var(--workspace-accent-soft); font-size: 11px; font-weight: 650; cursor: pointer; }
  .file-comments { min-width: 16px; padding: 0 5px; border-radius: 999px; color: var(--workspace-raised); background: var(--workspace-accent); font-size: 10px; font-style: normal; font-weight: 700; line-height: 16px; text-align: center; }
  .correction-comments { grid-column: 1 / -1; margin: 0; display: flex; align-items: center; gap: 6px; color: var(--workspace-accent); font-size: 11px; font-weight: 650; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .fold-row { grid-column: 1 / -1; width: 100%; min-height: 26px; padding: 0 12px; display: flex; align-items: center; gap: 7px; border: 0; border-block: 1px solid color-mix(in srgb, var(--workspace-line) 60%, transparent); color: var(--workspace-muted); background: color-mix(in srgb, var(--workspace-accent) 6%, var(--workspace-pane)); font-size: 11px; text-align: left; cursor: pointer; position: sticky; left: 0; }
  .fold-row:hover { color: var(--workspace-accent); }
  .diff-empty { margin: auto; max-width: 340px; padding: 28px; display: grid; justify-items: center; gap: 8px; color: var(--workspace-faint); text-align: center; }
  .diff-empty :global(.lume-icon) { color: var(--workspace-accent); }
  .diff-empty strong { color: var(--workspace-strong); font-size: 13px; }
  .diff-empty p { margin: 0; font-size: 12px; line-height: 1.5; }
  .review-reading-pane, .review-deliverables { min-width: 0; min-height: 0; overflow: auto; overscroll-behavior: contain; scrollbar-width: thin; scrollbar-color: var(--workspace-scroll-thumb) transparent; background: var(--workspace-code); }
  .review-reading-pane { padding: clamp(14px, 3vw, 28px); display: flex; flex-direction: column; gap: 16px; }
  .reading-section { min-width: 0; padding-bottom: 14px; border-bottom: 1px solid var(--workspace-line); }
  .reading-section > header, .review-deliverables > header { min-height: 26px; margin-bottom: 8px; display: flex; align-items: center; gap: 7px; color: var(--workspace-accent); }
  .reading-section > header strong, .review-deliverables > header strong { color: var(--workspace-strong); font-size: 12px; }
  .reading-section > header time { margin-left: auto; color: var(--workspace-faint); font-size: 11px; }
  .reading-copy { color: var(--workspace-text); font-size: 12px; line-height: 1.62; overflow-wrap: anywhere; }
  .reading-copy :global(p:first-child) { margin-top: 0; }.reading-copy :global(p:last-child) { margin-bottom: 0; }
  .reading-copy :global(pre) { max-width: 100%; padding: 9px; overflow: auto; border-radius: 7px; color: var(--workspace-text); background: var(--workspace-subtle); font-size: 11px; }
  .reading-copy :global(code) { overflow-wrap: anywhere; }.reading-copy :global(table) { display: block; max-width: 100%; overflow-x: auto; border-collapse: collapse; }
  .context-attachments { margin-top: 10px; display: flex; flex-wrap: wrap; gap: 5px; }
  .context-attachments span { max-width: 100%; min-height: 24px; padding: 0 7px; display: inline-flex; align-items: center; gap: 5px; overflow: hidden; border: 1px solid var(--workspace-line); border-radius: 6px; color: var(--workspace-muted); background: var(--workspace-pane); font-size: 11px; text-overflow: ellipsis; white-space: nowrap; }
  .checks-section ul { margin: 0; padding-left: 17px; color: var(--workspace-text); font-size: 11px; line-height: 1.7; }
  .final-response { padding: 10px; border: 1px solid var(--workspace-line); border-radius: 9px; background: var(--workspace-pane); }
  .review-deliverables { padding: clamp(14px, 3vw, 25px); display: grid; grid-template-rows: auto minmax(0, 1fr); align-content: start; }
  .review-deliverables > header { margin-bottom: 10px; justify-content: space-between; }
  .review-deliverables > header > span { display: inline-flex; align-items: center; gap: 7px; }
  .review-deliverables > header small { padding: 2px 6px; border-radius: 5px; color: var(--workspace-muted); background: var(--workspace-subtle); font-size: 11px; }
  .review-deliverables :global(.response-attachments) { align-self: start; }
  .review-notes { min-width: 0; min-height: 0; padding: 12px; display: grid; grid-template-rows: auto minmax(0, 1fr) auto; gap: 10px; overflow: hidden; border-left: 1px solid var(--workspace-line); background: var(--workspace-pane); animation: reveal-notes 190ms cubic-bezier(.16, 1, .3, 1); }.review-notes > header { min-width: 0; min-height: 28px; display: flex; align-items: center; gap: 8px; }.review-notes > header > span { min-width: 0; display: flex; align-items: center; gap: 6px; flex: 1; color: var(--workspace-accent); }.review-notes header strong { color: var(--workspace-strong); font-size: 10px; }.review-notes header small { overflow: hidden; color: var(--workspace-faint); font-size: 11px; text-overflow: ellipsis; white-space: nowrap; }.review-notes textarea { box-sizing: border-box; min-width: 0; min-height: 0; width: 100%; padding: 10px; resize: none; border: 1px solid var(--workspace-line); border-radius: 10px; outline: 0; color: var(--workspace-text); background: var(--workspace-raised); font: 12px/1.55 inherit; }.review-notes textarea::placeholder { color: var(--workspace-faint); }.review-notes textarea:focus { border-color: color-mix(in srgb, var(--workspace-accent) 48%, var(--workspace-line)); box-shadow: 0 0 0 2px var(--workspace-accent-soft); }.review-notes textarea:disabled { opacity: .65; }.review-notes footer { min-width: 0; min-height: 29px; display: flex; align-items: center; gap: 6px; }.review-notes footer > span { min-width: 0; flex: 1; overflow: hidden; color: var(--workspace-faint); font-size: 11px; text-overflow: ellipsis; white-space: nowrap; }.review-notes footer > span.dirty { color: #bd8434; }.review-notes footer button { min-height: 28px; padding: 0 8px; display: flex; align-items: center; gap: 5px; border: 0; border-radius: 8px; color: var(--workspace-muted); background: var(--workspace-subtle); font-size: 11px; font-weight: 720; cursor: pointer; }.review-notes footer button:hover:not(:disabled) { color: var(--workspace-strong); }.review-notes footer button:disabled { opacity: .45; cursor: default; }.review-notes footer .delete-note { width: 28px; padding: 0; justify-content: center; color: #b96862; }.review-notes footer .save-note { color: var(--workspace-raised); background: var(--workspace-accent); }.note-empty { margin: auto; max-width: 190px; display: grid; justify-items: center; gap: 8px; color: var(--workspace-faint); text-align: center; }.note-empty p { margin: 0; font-size: 11px; line-height: 1.5; }@keyframes reveal-notes { from { opacity: .4; transform: translateX(12px); } to { opacity: 1; transform: translateX(0); } }

  .review-views button:focus-visible, .review-decisions button:focus-visible, .correction-footer button:focus-visible, .correction-request textarea:focus-visible, .file-check:focus-visible, .file-open:focus-visible, .mark-reviewed:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 2px; }
  @container review (max-width: 760px) { .review-decisions .decision-state, .progress .bar { display: none; }.review-reading-pane { padding: 14px; }.review-body.simple-view.notes-open { grid-template-columns: minmax(0, 1fr); }.review-body.notes-open { grid-template-columns: clamp(160px, 27%, 210px) minmax(0, 1fr); }.review-notes { position: absolute; z-index: 3; inset: 0 0 0 auto; width: min(290px, 82%); box-shadow: -12px 0 32px rgba(8, 20, 14, .16); }.mode-switch button span { display: none; } }
  @container review (max-width: 540px) { .review-summary { padding: 5px 6px; gap: 5px; }.review-views { width: 100%; }.review-views button { flex: 1; justify-content: center; }.review-decisions { width: 100%; margin-left: 0; }.review-decisions button { min-width: 0; flex: 1; padding: 0 6px; }.correction-request { padding: 8px; }.correction-footer { flex-wrap: wrap; }.correction-footer small { flex-basis: 100%; }.review-body, .review-body.notes-open { grid-template-columns: minmax(0, 1fr); grid-template-rows: auto minmax(0, 1fr); }.review-body.simple-view, .review-body.simple-view.notes-open { grid-template-rows: minmax(0, 1fr); }.review-files { max-height: 96px; padding: 6px 8px; grid-template-rows: minmax(0, 1fr); border-right: 0; border-bottom: 1px solid var(--workspace-line); }.review-files footer { display: none; }.review-files nav { display: flex; gap: 3px; overflow-x: auto; overflow-y: hidden; }.file-row { width: min(190px, 52cqw); flex: 0 0 auto; }.workflow-tag { display: none; } }
  @media (prefers-reduced-motion: reduce) { .review-notes { animation: none; }.review-views button, .review-decisions button, .turn-chevron, .progress .bar i { transition: none; } }
</style>
