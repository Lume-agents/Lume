<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import type { Preferences, WorkflowGroupDefinition, WorkflowRole, WorkflowStepDefinition, WorkflowConnectionDefinition, WorkflowContextPackage, WorkflowRun } from "$lib/domain";
  import type { HubSession } from "$lib/hubProtocol";
  import type { Language } from "$lib/i18n";
  import { localize } from "$lib/i18n";
  import { loadWorkflowRoleContract, loadWorkflowRun, startWorkflowRun, approveWorkflowHandoff, advanceWorkflowRun, pauseWorkflowRun, resumeWorkflowRun, retryWorkflowStep, skipWorkflowStep, cancelWorkflowRun, previewWorkflowContext } from "$lib/lume";
  import { BOARD_CARD_WIDTH, BOARD_CARD_HEIGHT, analyzeChain, checkConnection, connectionRefusalMessage, problemMessage, newGroup, newStep, preserveRoleOverrides, defaultRole, defaultContextSelection, addStep, updateStep, removeStep, addConnection, updateConnection, removeConnection, insertStep, edgeGeometry, cardRect, cardAt, placeCard, toBoardPoint, zoomAround, fitView, normalizeLayout, stepVisualState, connectionWaitsForApproval, type BoardPoint, type BoardLayout, type BoardView } from "$lib/workflowBoard";
  import { WorkflowBoardSaveQueue } from "$lib/workflowBoardPersistence";
  import ThreadAvatar from "$lib/ThreadAvatar.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import LumeSelect from "$lib/LumeSelect.svelte";
  import WorkflowRoleIcon from "$lib/WorkflowRoleIcon.svelte";
  import WorkspaceHeaderIcon from "$lib/WorkspaceHeaderIcon.svelte";
  import SystemBannerStack, { type SystemBannerItem } from "$lib/SystemBannerStack.svelte";

  let { sessions, preferences, language, active, keyboardEnabled = true, onSaveGroup, onOpenChat, onClose, onFinishSidebarDrag }: {
    sessions: HubSession[];
    preferences: Preferences;
    language: Language;
    active: boolean;
    keyboardEnabled?: boolean;
    onSaveGroup: (group: WorkflowGroupDefinition) => Promise<void>;
    onOpenChat: (session: HubSession) => void;
    onClose: () => void;
    onFinishSidebarDrag: () => void;
  } = $props();

  const storageKey = "lume:workflow-board:v1";
  const markerId = "board-arrow-" + crypto.randomUUID();
  const roles: WorkflowRole[] = ["planner", "implementer", "reviewer", "tester", "researcher", "custom"];
  let activeId = $state("");
  let layouts = $state<Record<string, BoardLayout>>({});
  let objectives = $state<Record<string, string>>({});
  let drafts = $state<Record<string, WorkflowGroupDefinition>>({});
  let runs = $state<Record<string, WorkflowRun | null>>({});
  let selection = $state<{ kind: "step" | "connection"; id: string } | null>(null);
  let pickerOpen = $state(false);
  let insertingConnectionId = $state<string | null>(null);
  let adding = $state(false);
  let saving = $state(false);
  let saveFailed = $state(false);
  let actionLoading = $state(false);
  let runLoading = $state(false);
  let previewLoading = $state(false);
  let preview = $state<WorkflowContextPackage | null>(null);
  let previewRevision = 0;
  let roleRevision = 0;
  let changingRole = $state(false);
  let banners = $state<SystemBannerItem[]>([]);
  let ready = $state(false);
  let reducedMotion = $state(false);
  let viewport = $state<HTMLDivElement | null>(null);
  let viewportWidth = $state(0);
  let viewportHeight = $state(0);
  let storageTimer: ReturnType<typeof setTimeout> | undefined;
  let disposed = false;
  let gesture = $state<{
    kind: "pan" | "card" | "connection";
    pointerId: number;
    start: BoardPoint;
    origin: BoardPoint;
    stepId?: string;
  } | null>(null);
  let connectionSource = $state<string | null>(null);
  let connectionPoint = $state<BoardPoint | null>(null);
  let connectionTarget = $state<string | null>(null);
  let dropActive = $state(false);
  let problemStepIds = $state<string[]>([]);

  const queue = new WorkflowBoardSaveQueue((next) => onSaveGroup(next));
  const groups = $derived([
    ...preferences.workflowGroups.map((item) => drafts[item.id] ?? item),
    ...Object.values(drafts).filter((item) => !preferences.workflowGroups.some((saved) => saved.id === item.id)),
  ]);
  const group = $derived(groups.find((item) => item.id === activeId) ?? null);
  const layout = $derived(layouts[activeId] ?? normalizeLayout(null));
  const view = $derived(layout.view);
  const run = $derived(runs[activeId] ?? null);
  const finished = $derived(Boolean(run && ["draft", "completed", "cancelled"].includes(run.status)));
  const locked = $derived(actionLoading || runLoading || Boolean(run && !finished));
  const objective = $derived(objectives[activeId] ?? run?.objective ?? "");
  const chain = $derived(group ? analyzeChain(group) : null);
  const selectedStep = $derived(selection?.kind === "step" ? group?.steps.find((step) => step.id === selection?.id) : undefined);
  const selectedConnection = $derived(selection?.kind === "connection" ? group?.connections.find((item) => item.id === selection?.id) : undefined);
  const blockedStep = $derived(group?.steps.find((step) => {
    const session = sessionFor(step);
    return !session || !session.capabilities.canPrompt;
  }));
  const edges = $derived(group?.connections.flatMap((connection) => {
    const from = layout.positions[connection.fromStepId];
    const to = layout.positions[connection.toStepId];
    return from && to ? [{ connection, geometry: edgeGeometry(cardRect(from), cardRect(to)) }] : [];
  }) ?? []);
  const pendingArrow = $derived(connectionSource && connectionPoint && layout.positions[connectionSource]
    ? edgeGeometry(cardRect(layout.positions[connectionSource]), {
      x: connectionPoint.x, y: connectionPoint.y, width: 0, height: 0,
    }) : null);
  const targetCheck = $derived(group && connectionSource && connectionTarget
    ? checkConnection(group, connectionSource, connectionTarget) : null);
  const duration = (time: number) => reducedMotion ? 0 : time;

  function tr(english: string, portuguese: string) { return localize(language, english, portuguese); }
  function name(session?: HubSession) { return session?.sessionName?.trim() || session?.project || session?.agentLabel || tr("Session unavailable", "Sessão indisponível"); }
  function sessionFor(step?: WorkflowStepDefinition) { return step ? sessions.find((session) => session.nativeSessionId === step.sessionNativeId || session.id === step.sessionNativeId) : undefined; }
  function groupName(item: WorkflowGroupDefinition, index = groups.indexOf(item)) { return layouts[item.id]?.name || (item.terminalGroupId.startsWith("board-") ? "Workflow " : tr("Orb workflow ", "Workflow do Orb ")) + (index + 1); }
  function roleName(role: WorkflowRole) {
    return { planner: tr("Planner", "Planejador"), implementer: tr("Implementer", "Implementador"), reviewer: tr("Reviewer", "Revisor"), tester: tr("Tester", "Testador"), researcher: tr("Researcher", "Pesquisador"), custom: tr("Custom", "Personalizado") }[role];
  }
  function runStatus(status: WorkflowRun["status"]) {
    return { draft: tr("Draft", "Rascunho"), ready: tr("Ready for next step", "Pronto para a próxima etapa"), running: tr("Running", "Executando"), waiting_for_approval: tr("Waiting for approval", "Aguardando aprovação"), paused: tr("Paused", "Pausado"), completed: tr("Completed", "Concluído"), failed: tr("Failed", "Falhou"), cancelled: tr("Cancelled", "Cancelado") }[status];
  }
  function stepStatus(step: WorkflowStepDefinition) {
    const state = stepVisualState(run, step.id);
    if (state === "idle") return sessionFor(step)?.statusLabel ?? tr("Unavailable", "Indisponível");
    return { pending: tr("Pending", "Pendente"), running: tr("Running", "Executando"), completed: tr("Completed", "Concluído"), failed: tr("Failed", "Falhou"), skipped: tr("Skipped", "Ignorado") }[state];
  }
  function notify(message: string, tone: SystemBannerItem["tone"] = "error") {
    const id = crypto.randomUUID();
    banners = [{ id, message, tone, onDismiss: () => banners = banners.filter((item) => item.id !== id) }, ...banners].slice(0, 4);
  }
  function errorMessage(error: unknown) { return String(error).replace(/^Error:\s*/, ""); }

  function persistLayout() {
    if (!ready) return;
    if (storageTimer) clearTimeout(storageTimer);
    storageTimer = setTimeout(writeLayout, 180);
  }
  function writeLayout() {
    if (!ready) return;
    if (storageTimer) clearTimeout(storageTimer);
    storageTimer = undefined;
    try { localStorage.setItem(storageKey, JSON.stringify({ activeGroupId: activeId, layouts, objectives })); }
    catch { notify(tr("The board layout could not be saved on this computer.", "Não foi possível salvar a posição do board neste computador.")); }
  }
  function patchLayout(patch: Partial<BoardLayout>, id = activeId) {
    layouts = { ...layouts, [id]: { ...(layouts[id] ?? normalizeLayout(null)), ...patch } };
    persistLayout();
  }
  function ensureLayout(item: WorkflowGroupDefinition) {
    const stored = layouts[item.id] ?? normalizeLayout(null);
    const positions = { ...stored.positions };
    let changed = !layouts[item.id];
    const orderedIds = [...(analyzeChain(item).order), ...item.steps.map((step) => step.id)];
    for (const id of new Set(orderedIds)) {
      if (positions[id]) continue;
      positions[id] = placeCard(Object.values(positions));
      changed = true;
    }
    if (changed) patchLayout({ ...stored, positions }, item.id);
  }
  function activate(id: string) {
    cancelGesture();
    activeId = id;
    selection = null;
    pickerOpen = false;
    insertingConnectionId = null;
    problemStepIds = [];
    invalidatePreview();
    persistLayout();
  }
  async function persistGroup(next: WorkflowGroupDefinition) {
    drafts = { ...drafts, [next.id]: next };
    saving = true;
    saveFailed = false;
    try { await queue.enqueue(next); }
    catch (error) { saveFailed = true; if (!disposed) notify(errorMessage(error)); }
    finally { if (!disposed) saving = false; }
  }
  async function retrySave() {
    saving = true;
    saveFailed = false;
    try { await queue.flush(); }
    catch (error) { saveFailed = true; notify(errorMessage(error)); }
    finally { saving = false; }
  }
  function edit(next: WorkflowGroupDefinition) {
    if (locked) return;
    problemStepIds = [];
    invalidatePreview();
    void persistGroup(next);
  }
  function createGroup() {
    const next = newGroup();
    drafts = { ...drafts, [next.id]: next };
    activate(next.id);
    patchLayout({ name: "Workflow " + groups.length });
    void persistGroup(next);
    pickerOpen = true;
  }
  function choose(selectionNext: typeof selection) {
    selection = selectionNext;
    pickerOpen = false;
    insertingConnectionId = null;
    invalidatePreview();
  }
  function openPicker(connectionId: string | null = null) {
    if (locked) return;
    if (!group) { createGroup(); return; }
    selection = null;
    insertingConnectionId = connectionId;
    pickerOpen = true;
  }
  async function addSession(session: HubSession, point?: BoardPoint) {
    if (!group || locked || adding) return;
    if (!session.nativeSessionId?.trim()) { notify(tr("Connect this conversation before adding it.", "Conecte esta conversa antes de adicioná-la."), "warning"); return; }
    if (group.steps.some((step) => step.sessionNativeId === session.nativeSessionId)) { notify(tr("This conversation is already in the workflow.", "Esta conversa já está no workflow."), "info"); return; }
    const id = group.id;
    const insertionId = insertingConnectionId;
    adding = true;
    try {
      const role = defaultRole(group.steps.length);
      const contract = await loadWorkflowRoleContract(role);
      const latest = groups.find((item) => item.id === id);
      if (!latest || disposed) return;
      const step = newStep(session.nativeSessionId, role, contract);
      const next = insertionId ? insertStep(latest, insertionId, step) : addStep(latest, step);
      if (next === latest) return;
      const stored = layouts[id] ?? normalizeLayout(null);
      const insertion = latest.connections.find((connection) => connection.id === insertionId);
      const midpoint = insertion && stored.positions[insertion.fromStepId] && stored.positions[insertion.toStepId]
        ? edgeGeometry(cardRect(stored.positions[insertion.fromStepId]), cardRect(stored.positions[insertion.toStepId])).mid : undefined;
      patchLayout({ positions: { ...stored.positions, [step.id]: placeCard(Object.values(stored.positions), point ?? midpoint) } }, id);
      await persistGroup(next);
      if (activeId === id) { pickerOpen = false; insertingConnectionId = null; choose({ kind: "step", id: step.id }); }
    } catch (error) { notify(errorMessage(error)); }
    finally { adding = false; }
  }
  function editStep(patch: Partial<WorkflowStepDefinition>) {
    if (group && selectedStep && !locked) edit(updateStep(group, selectedStep.id, patch));
  }
  async function changeRole(role: WorkflowRole) {
    if (!group || !selectedStep || locked) return;
    const id = selectedStep.id;
    const groupId = group.id;
    const revision = ++roleRevision;
    changingRole = true;
    try {
      const [previous, contract] = await Promise.all([loadWorkflowRoleContract(selectedStep.role), loadWorkflowRoleContract(role)]);
      if (revision !== roleRevision || disposed || activeId !== groupId || !group || locked) return;
      const current = group.steps.find((step) => step.id === id);
      if (current) edit(updateStep(group, id, { role, ...preserveRoleOverrides(current, previous, contract) }));
    } catch (error) { notify(errorMessage(error)); }
    finally { if (revision === roleRevision) changingRole = false; }
  }
  function editConnection(patch: Partial<WorkflowConnectionDefinition>) {
    if (group && selectedConnection && !locked) edit(updateConnection(group, selectedConnection.id, patch));
  }
  function changePolicy(policy: "minimal" | "standard" | "detailed" | "custom") {
    if (policy === "custom") return;
    const selected = policy === "minimal"
      ? { response: true, files: false, checks: false, plan: false, activity: false, diffs: false }
      : policy === "detailed"
        ? { response: true, files: true, checks: true, plan: true, activity: true, diffs: true }
        : defaultContextSelection();
    editConnection({ contextPolicy: policy, contextSelection: selected, includeResponse: selected.response, includeFiles: selected.files, includeTests: selected.checks });
  }
  function removeSelected() {
    if (!group || !selection || locked) return;
    if (selection.kind === "step") {
      const positions = { ...layout.positions };
      delete positions[selection.id];
      patchLayout({ positions });
      edit(removeStep(group, selection.id));
    } else edit(removeConnection(group, selection.id));
    choose(null);
  }
  function invalidatePreview() { previewRevision++; preview = null; previewLoading = false; }
  async function showPreview() {
    if (!group || !selectedConnection || !objective.trim()) return;
    const revision = ++previewRevision;
    previewLoading = true;
    try {
      const sourceResultId = run?.steps.find((step) => step.stepId === selectedConnection.fromStepId)?.resultId;
      const next = await previewWorkflowContext(group, selectedConnection.id, objective, sourceResultId);
      if (revision === previewRevision && !disposed) preview = next;
    } catch (error) { if (revision === previewRevision) notify(errorMessage(error)); }
    finally { if (revision === previewRevision) previewLoading = false; }
  }

  function boardPoint(clientX: number, clientY: number) {
    const bounds = viewport!.getBoundingClientRect();
    return toBoardPoint({ x: clientX, y: clientY }, { x: bounds.left, y: bounds.top }, view);
  }
  function capture(event: PointerEvent) {
    viewport?.focus({ preventScroll: true });
    viewport?.setPointerCapture(event.pointerId);
    event.preventDefault();
    event.stopPropagation();
  }
  function beginPan(event: PointerEvent) {
    if (event.button !== 0 && event.button !== 1) return;
    if (event.button === 0 && event.target instanceof Element && event.target.closest("button, input, textarea, select, [data-board-card], [data-board-edge]")) return;
    capture(event);
    gesture = { kind: "pan", pointerId: event.pointerId, start: { x: event.clientX, y: event.clientY }, origin: { x: view.x, y: view.y } };
    if (!connectionSource) choose(null);
  }
  function beginCard(event: PointerEvent, id: string) {
    if (event.button !== 0 || !layout.positions[id]) return;
    if (connectionSource && !gesture) { event.stopPropagation(); connectTo(id); return; }
    choose({ kind: "step", id });
    capture(event);
    gesture = { kind: "card", pointerId: event.pointerId, start: { x: event.clientX, y: event.clientY }, origin: { ...layout.positions[id] }, stepId: id };
  }
  function beginConnection(event: PointerEvent, id: string) {
    if (event.button !== 0 || locked) return;
    capture(event);
    connectionSource = id;
    connectionTarget = null;
    connectionPoint = boardPoint(event.clientX, event.clientY);
    gesture = { kind: "connection", pointerId: event.pointerId, start: connectionPoint, origin: connectionPoint, stepId: id };
    choose(null);
  }
  function connectTo(id: string) {
    if (!group || !connectionSource || locked) return;
    const result = checkConnection(group, connectionSource, id);
    if (result.ok) edit(addConnection(group, connectionSource, id));
    else notify(connectionRefusalMessage(result.reason, language === "pt-BR"), "warning");
    cancelGesture();
  }
  function movePointer(event: PointerEvent) {
    if (!viewport) return;
    if (connectionSource) {
      connectionPoint = boardPoint(event.clientX, event.clientY);
      connectionTarget = cardAt(connectionPoint, layout.positions, group?.steps.map((step) => step.id) ?? []);
    }
    if (!gesture || gesture.pointerId !== event.pointerId) return;
    const dx = event.clientX - gesture.start.x;
    const dy = event.clientY - gesture.start.y;
    if (gesture.kind === "pan") patchLayout({ view: { ...view, x: gesture.origin.x + dx, y: gesture.origin.y + dy } });
    else if (gesture.kind === "card" && gesture.stepId) patchLayout({ positions: { ...layout.positions, [gesture.stepId]: { x: gesture.origin.x + dx / view.zoom, y: gesture.origin.y + dy / view.zoom } } });
  }
  function endPointer(event: PointerEvent) {
    if (!gesture || gesture.pointerId !== event.pointerId) return;
    if (gesture.kind === "connection" && connectionTarget) connectTo(connectionTarget);
    cancelGesture();
  }
  function cancelGesture() {
    const previous = gesture;
    gesture = null;
    connectionSource = null;
    connectionPoint = null;
    connectionTarget = null;
    if (previous && viewport?.hasPointerCapture(previous.pointerId)) viewport.releasePointerCapture(previous.pointerId);
  }
  function keyboardConnect(id: string) {
    if (locked) return;
    if (connectionSource) { connectTo(id); return; }
    connectionSource = id;
    const position = layout.positions[id];
    connectionPoint = { x: position.x + BOARD_CARD_WIDTH + 60, y: position.y + BOARD_CARD_HEIGHT / 2 };
    viewport?.focus();
  }
  function zoom(factor: number) {
    patchLayout({ view: zoomAround(view, { x: viewportWidth / 2, y: viewportHeight / 2 }, view.zoom * factor) });
  }
  function fit() {
    // A fit click can land before ResizeObserver publishes the new window size.
    const width = viewport?.clientWidth ?? viewportWidth;
    const height = viewport?.clientHeight ?? viewportHeight;
    const panelSpace = selection || pickerOpen ? Math.min(310, width * 0.45) : 0;
    const positions = group?.steps.map((step) => layout.positions[step.id]).filter((point): point is BoardPoint => Boolean(point)) ?? [];
    patchLayout({ view: fitView(positions, width - panelSpace, height - 160) });
  }
  function wheel(event: WheelEvent) {
    if (!active || !viewport) return;
    event.preventDefault();
    if (event.ctrlKey || event.metaKey) {
      const bounds = viewport.getBoundingClientRect();
      patchLayout({ view: zoomAround(view, { x: event.clientX - bounds.left, y: event.clientY - bounds.top }, view.zoom * Math.exp(-event.deltaY * 0.002)) });
    } else {
      const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? viewportHeight : 1;
      patchLayout({ view: { ...view, x: view.x - event.deltaX * unit, y: view.y - event.deltaY * unit } });
    }
  }
  function canvasEvents(node: HTMLDivElement) {
    node.addEventListener("wheel", wheel, { passive: false });
    const observer = new ResizeObserver(() => { viewportWidth = node.clientWidth; viewportHeight = node.clientHeight; });
    observer.observe(node);
    return { destroy() { node.removeEventListener("wheel", wheel); observer.disconnect(); } };
  }
  function dragOver(event: DragEvent) {
    event.stopPropagation();
    if (!Array.from(event.dataTransfer?.types ?? []).includes("text/x-lume-session")) return;
    event.preventDefault();
    dropActive = true;
    if (event.dataTransfer) event.dataTransfer.dropEffect = locked ? "none" : "move";
  }
  function drop(event: DragEvent) {
    event.preventDefault();
    event.stopPropagation();
    dropActive = false;
    const session = sessions.find((item) => item.id === event.dataTransfer?.getData("text/x-lume-session"));
    const point = viewport ? boardPoint(event.clientX, event.clientY) : undefined;
    onFinishSidebarDrag();
    if (!session || locked) return;
    if (!group) createGroup();
    insertingConnectionId = null;
    void addSession(session, point);
  }
  function keydown(event: KeyboardEvent) {
    if (!active || !keyboardEnabled || event.defaultPrevented) return;
    const target = event.target;
    if (target instanceof Element && !target.closest(".workflow-board")) return;
    if (target instanceof Element && target.closest("input, textarea, select, [contenteditable='true']")) return;
    if (event.key === "Escape") {
      event.preventDefault();
      if (gesture || connectionSource) cancelGesture();
      else if (selection || pickerOpen) { choose(null); pickerOpen = false; }
      else onClose();
    } else if ((event.key === "Delete" || event.key === "Backspace") && selection) {
      event.preventDefault();
      removeSelected();
    }
  }

  function acceptRun(next: WorkflowRun | null) {
    if (!next || disposed) return;
    const current = runs[next.workflowId];
    const newer = !current || (next.id === current.id ? next.updatedAt > current.updatedAt
      : next.createdAt > current.createdAt || (next.createdAt === current.createdAt && next.updatedAt > current.updatedAt));
    if (!newer) return;
    runs = { ...runs, [next.workflowId]: next };
    if (!objectives[next.workflowId] || !["completed", "cancelled"].includes(next.status)) {
      objectives = { ...objectives, [next.workflowId]: next.objective };
    }
    if (next.workflowId === activeId) invalidatePreview();
  }
  async function refreshRun(id: string) {
    runLoading = true;
    try { acceptRun(await loadWorkflowRun(id)); }
    catch (error) { if (id === activeId && !disposed) notify(errorMessage(error)); }
    finally { if (id === activeId && !disposed) runLoading = false; }
  }
  async function perform(action: () => Promise<WorkflowRun>) {
    if (actionLoading) return;
    actionLoading = true;
    try { acceptRun(await action()); }
    catch (error) { notify(errorMessage(error)); }
    finally { actionLoading = false; }
  }
  async function execute() {
    if (!group || !objective.trim() || chain?.problems.length || blockedStep || runLoading) return;
    const definition = group;
    const runObjective = objective;
    await perform(async () => {
      await queue.flush();
      return startWorkflowRun(definition, runObjective);
    });
  }

  $effect(() => {
    const incoming = preferences.workflowGroups;
    saving;
    untrack(() => {
      const pending = Object.fromEntries(Object.entries(drafts).filter(([id]) => queue.hasPending(id) || !incoming.some((item) => item.id === id)));
      if (Object.keys(pending).length !== Object.keys(drafts).length) drafts = pending;
    });
  });
  $effect(() => {
    if (!ready) return;
    if (!group && groups.length) untrack(() => activate(groups[0].id));
    if (group) untrack(() => ensureLayout(group));
  });
  $effect(() => { if (ready && active && activeId) void refreshRun(activeId); });
  $effect(() => { if (!active) untrack(cancelGesture); });

  onMount(() => {
    try {
      const raw = JSON.parse(localStorage.getItem(storageKey) || "{}");
      layouts = Object.fromEntries(Object.entries(raw.layouts ?? {}).map(([id, value]) => [id, normalizeLayout(value)]));
      activeId = typeof raw.activeGroupId === "string" ? raw.activeGroupId : "";
      objectives = Object.fromEntries(Object.entries(raw.objectives ?? {}).filter(([, value]) => typeof value === "string")) as Record<string, string>;
    } catch { /* Invalid local layout falls back to the existing workflow definitions. */ }
    reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    ready = true;
    let stop: (() => void) | undefined;
    void listen<WorkflowRun>("lume://workflow-run-changed", ({ payload }) => acceptRun(payload))
      .then((unlisten) => { if (disposed) unlisten(); else stop = unlisten; })
      .catch((error) => { if (!disposed) notify(errorMessage(error)); });
    return () => { writeLayout(); disposed = true; stop?.(); if (storageTimer) clearTimeout(storageTimer); };
  });
</script>

<svelte:window onkeydown={keydown} />

<section class="workflow-board" hidden={!active} inert={!active} aria-label={tr("Workflow board", "Board de workflow")} data-testid="workflow-board" ondragover={dragOver} ondrop={drop}>
  <div
    class="board-viewport"
    class:panning={gesture?.kind === "pan"}
    class:connecting={connectionSource !== null}
    class:drop-active={dropActive && !locked}
    bind:this={viewport}
    use:canvasEvents
    role="region"
    tabindex="-1"
    aria-label={tr("Workflow canvas. Drag to pan; Control and scroll to zoom.", "Canvas de workflow. Arraste o fundo para navegar; Control e rolagem para ampliar.")}
    style:background-size={24 * view.zoom + "px " + 24 * view.zoom + "px"}
    style:background-position={view.x + "px " + view.y + "px"}
    onpointerdown={beginPan}
    onpointermove={movePointer}
    onpointerup={endPointer}
    onpointercancel={cancelGesture}
    onlostpointercapture={cancelGesture}
    ondragover={dragOver}
    ondragleave={(event) => { if (!(event.relatedTarget instanceof Node) || !viewport?.contains(event.relatedTarget)) dropActive = false; }}
    ondrop={drop}
  >
    <div class="board-world" style:transform={"translate(" + view.x + "px," + view.y + "px) scale(" + view.zoom + ")"}>
      <svg class="board-edges" xmlns="http://www.w3.org/2000/svg" aria-label={tr("Workflow connections", "Conexões do workflow")}>
        <defs>
          <marker id={markerId} markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto" markerUnits="userSpaceOnUse"><path d="M1 1 7 4 1 7" fill="none" stroke="var(--workspace-muted)" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" /></marker>
          <marker id={markerId + "-active"} markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto" markerUnits="userSpaceOnUse"><path d="M1 1 7 4 1 7" fill="none" stroke="var(--workspace-accent)" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" /></marker>
        </defs>
        {#each edges as { connection, geometry } (connection.id)}
          {@const chosen = selection?.kind === "connection" && selection.id === connection.id}
          {@const waiting = connectionWaitsForApproval(run, connection.id)}
          <g class:selected={chosen} class:waiting data-board-edge={connection.id}>
            <path class="edge-hit" d={geometry.path} role="button" tabindex="0" aria-label={tr("Edit connection", "Editar conexão") + ": " + name(sessionFor(group!.steps.find((step) => step.id === connection.fromStepId)!)) + " → " + name(sessionFor(group!.steps.find((step) => step.id === connection.toStepId)!))} onpointerdown={(event) => { event.stopPropagation(); choose({ kind: "connection", id: connection.id }); }} onkeydown={(event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); choose({ kind: "connection", id: connection.id }); } }} />
            <path class="edge-line" d={geometry.path} marker-end={"url(#" + markerId + (chosen ? "-active" : "") + ")"} />
          </g>
        {/each}
        {#if pendingArrow}<path class="edge-pending" d={pendingArrow.path} marker-end={"url(#" + markerId + "-active)"} />{/if}
      </svg>
      {#each edges as { connection, geometry } (connection.id)}
        <div class="edge-controls" style:left={geometry.mid.x + "px"} style:top={geometry.mid.y + "px"}>
          <button class="edge-policy" class:waiting={connectionWaitsForApproval(run, connection.id)} type="button" title={tr("Connection settings", "Configurações da conexão")} aria-label={tr("Connection settings", "Configurações da conexão")} onclick={() => choose({ kind: "connection", id: connection.id })}>
            {#if connection.requiresApproval}<LumeIcon name="shield" size={12} />{/if}
            {#if connection.advanceMode === "automatic"}<LumeIcon name="bolt" size={12} />{/if}
            {#if !connection.requiresApproval && connection.advanceMode === "manual"}<LumeIcon name="arrow-right" size={12} />{/if}
          </button>
          <button class="edge-insert" type="button" disabled={locked} title={tr("Insert an agent", "Inserir um agente")} aria-label={tr("Insert an agent in this connection", "Inserir um agente nesta conexão")} onclick={() => openPicker(connection.id)}><LumeIcon name="plus" size={12} /></button>
        </div>
      {/each}
      {#each group?.steps ?? [] as step (step.id)}
        {@const position = layout.positions[step.id] ?? { x: 96, y: 120 }}
        {@const session = sessionFor(step)}
        {@const state = stepVisualState(run, step.id)}
        {@const order = chain?.order.indexOf(step.id) ?? -1}
        <article class="board-card" class:selected={selection?.kind === "step" && selection.id === step.id} class:moving={gesture?.kind === "card" && gesture.stepId === step.id} class:problem={problemStepIds.includes(step.id)} class:target-valid={connectionTarget === step.id && targetCheck?.ok} class:target-invalid={connectionTarget === step.id && targetCheck && !targetCheck.ok} data-board-card={step.id} data-state={state === "idle" ? session?.status : state} style:left={position.x + "px"} style:top={position.y + "px"} style:width={BOARD_CARD_WIDTH + "px"} style:height={BOARD_CARD_HEIGHT + "px"}>
          <button class="card-body" type="button" onpointerdown={(event) => beginCard(event, step.id)} onclick={(event) => { if (event.detail === 0) connectionSource ? connectTo(step.id) : choose({ kind: "step", id: step.id }); }} aria-label={name(session) + " · " + roleName(step.role)}>
            <span class="card-identity">
              <ThreadAvatar seed={session?.nativeSessionId || step.sessionNativeId} label={name(session)} size={38} />
              <span class="card-copy"><strong>{name(session)}</strong><small>{session?.agentLabel ?? tr("Disconnected", "Desconectado")} · {session?.project || tr("No project", "Sem projeto")}</small></span>
              {#if order >= 0}<span class="step-number" title={order === 0 ? tr("First step", "Primeira etapa") : tr("Step", "Etapa")}>{order + 1}</span>{/if}
            </span>
            <span class="card-bottom"><span class="card-role"><span><WorkflowRoleIcon role={step.role} /></span>{step.role === "custom" ? step.customRoleLabel || roleName(step.role) : roleName(step.role)}</span><span class="card-status"><i></i>{stepStatus(step)}</span></span>
          </button>
          <button class="card-port port-in" type="button" disabled={locked} aria-label={tr("Connect to ", "Conectar a ") + name(session)} title={tr("Connection input", "Entrada da conexão")} onclick={() => connectionSource ? connectTo(step.id) : choose({ kind: "step", id: step.id })}><i></i></button>
          <button class="card-port port-out" type="button" disabled={locked} aria-label={tr("Connect from ", "Conectar de ") + name(session)} title={tr("Drag to connect", "Arraste para conectar")} onpointerdown={(event) => beginConnection(event, step.id)} onclick={(event) => { if (event.detail === 0) keyboardConnect(step.id); }}><i></i></button>
        </article>
      {/each}
    </div>
  </div>

  <header class="board-toolbar">
    <span class="board-title"><WorkspaceHeaderIcon name="workflow" size={19} /><strong>Workflow</strong></span>
    {#if groups.length}<LumeSelect value={activeId} options={groups.map((item, index) => ({ value: item.id, label: groupName(item, index) }))} ariaLabel={tr("Select workflow", "Selecionar workflow")} minWidth={132} variant="heading" onValueChange={activate} />{/if}
    <span class="toolbar-divider"></span>
    <button type="button" title={tr("New workflow", "Novo workflow")} aria-label={tr("New workflow", "Novo workflow")} onclick={createGroup}><LumeIcon name="plus" size={16} /></button>
    <button class="add-agent" type="button" disabled={locked || adding} onclick={() => openPicker()}><LumeIcon name="plus" size={14} />{tr("Agent", "Agente")}</button>
    <span class="save-state" aria-live="polite">{saving ? tr("Saving…", "Salvando…") : saveFailed ? tr("Not saved", "Não salvo") : ""}</span>
    {#if saveFailed}<button type="button" title={tr("Retry saving", "Tentar salvar novamente")} aria-label={tr("Retry saving", "Tentar salvar novamente")} onclick={() => void retrySave()}><LumeIcon name="refresh" size={15} /></button>{/if}
    <button class="close-board" type="button" title={tr("Back to chats", "Voltar aos chats")} aria-label={tr("Back to chats", "Voltar aos chats")} onclick={onClose}><LumeIcon name="close" size={17} /></button>
  </header>

  {#if !group?.steps.length && !pickerOpen}
    <div class="board-empty">
      <WorkspaceHeaderIcon name="workflow" size={42} />
      <strong>{tr("Give your agents a direction", "Dê uma direção aos seus agentes")}</strong>
      <p>{tr("Add agents and draw connections to build your workflow.", "Adicione agentes e desenhe conexões para montar seu workflow.")}</p>
      <button type="button" onclick={() => openPicker()}><LumeIcon name="plus" size={16} />{tr("Add first agent", "Adicionar primeiro agente")}</button>
      <small>{tr("You can also drag an agent here from the sidebar.", "Você também pode arrastar um agente da sidebar para cá.")}</small>
    </div>
  {/if}

  {#if pickerOpen || selectedStep || selectedConnection}
    <aside class="board-panel" aria-label={pickerOpen ? tr("Add agent", "Adicionar agente") : selectedStep ? tr("Step settings", "Configurações da etapa") : tr("Connection settings", "Configurações da conexão")} in:fly={{ x: 14, duration: duration(180), easing: cubicOut }} out:fade={{ duration: duration(90) }}>
      <header><strong>{pickerOpen ? insertingConnectionId ? tr("Insert an agent", "Inserir um agente") : tr("Add an agent", "Adicionar um agente") : selectedStep ? tr("Step", "Etapa") : tr("Connection", "Conexão")}</strong><button type="button" aria-label={tr("Close panel", "Fechar painel")} onclick={() => { choose(null); pickerOpen = false; }}><LumeIcon name="close" size={16} /></button></header>
      <div class="panel-scroll">
        {#if pickerOpen}
          <p class="panel-help">{insertingConnectionId ? tr("The selected agent will be inserted between these two steps.", "O agente escolhido entrará entre estas duas etapas.") : tr("Choose an open conversation or drag it from the sidebar.", "Escolha uma conversa aberta ou arraste-a da sidebar.")}</p>
          <div class="agent-options">
            {#each sessions as session (session.id)}
              {@const used = group?.steps.some((step) => step.sessionNativeId === session.nativeSessionId)}
              <button type="button" disabled={adding || used || !session.nativeSessionId?.trim()} onclick={() => void addSession(session)}>
                <ThreadAvatar seed={session.nativeSessionId || session.id} label={name(session)} size={32} />
                <span><strong>{name(session)}</strong><small>{used ? tr("Already added", "Já adicionado") : !session.nativeSessionId?.trim() ? tr("Connect first", "Conecte primeiro") : session.agentLabel + " · " + session.project}</small></span>
                <LumeIcon name={used ? "check" : "plus"} size={15} />
              </button>
            {/each}
            {#if !sessions.length}<p class="panel-help">{tr("Open an agent from the sidebar to get started.", "Abra um agente pela sidebar para começar.")}</p>{/if}
          </div>
        {:else if selectedStep}
          {@const session = sessionFor(selectedStep)}
          <div class="selected-identity"><ThreadAvatar seed={selectedStep.sessionNativeId} label={name(session)} size={36} /><span><strong>{name(session)}</strong><small>{session?.agentLabel} · {session?.project}</small></span></div>
          {#if !session || !session.capabilities.canPrompt}<p class="session-warning"><LumeIcon name="warning" size={15} />{session?.controlOrigin === "external" ? tr("Take control in the chat before running this workflow.", "Assuma o controle no chat antes de executar este workflow.") : tr("Connect this session before running the workflow.", "Conecte esta sessão antes de executar o workflow.")}</p>{/if}
          <div class="field" role="radiogroup" aria-label={tr("Step role", "Papel da etapa")}>
            <span>{tr("Role", "Papel")}</span>
            <div class="role-picker">
              {#each roles as role (role)}
                <button class:selected={selectedStep.role === role} class="role-option role-{role}" type="button" role="radio" aria-checked={selectedStep.role === role} disabled={locked || changingRole} onclick={() => { if (selectedStep.role !== role) void changeRole(role); }}><WorkflowRoleIcon {role} /><span>{roleName(role)}</span></button>
              {/each}
            </div>
          </div>
          {#if selectedStep.role === "custom"}<label class="field"><span>{tr("Role name", "Nome do papel")}</span><input value={selectedStep.customRoleLabel} disabled={locked} maxlength="80" oninput={(event) => editStep({ customRoleLabel: event.currentTarget.value })} /></label>{/if}
          <label class="field"><span>{tr("Instruction", "Instrução")}</span><textarea rows="3" disabled={locked || changingRole} value={selectedStep.instruction} oninput={(event) => editStep({ instruction: event.currentTarget.value })}></textarea></label>
          <label class="field"><span>{tr("Expected input", "Entrada esperada")}</span><textarea rows="2" disabled={locked || changingRole} value={selectedStep.expectedInput} oninput={(event) => editStep({ expectedInput: event.currentTarget.value })}></textarea></label>
          <label class="field"><span>{tr("Produced output", "Saída produzida")}</span><textarea rows="2" disabled={locked || changingRole} value={selectedStep.producedOutput} oninput={(event) => editStep({ producedOutput: event.currentTarget.value })}></textarea></label>
          <label class="field"><span>{tr("Completion condition", "Condição de término")}</span><textarea rows="2" disabled={locked || changingRole} value={selectedStep.completionCondition} oninput={(event) => editStep({ completionCondition: event.currentTarget.value })}></textarea></label>
          <div class="panel-actions"><button type="button" disabled={!session} onclick={() => { if (session) onOpenChat(session); }}><LumeIcon name="external" size={14} />{tr("Open chat", "Abrir chat")}</button><button class="danger" disabled={locked} type="button" onclick={removeSelected}><LumeIcon name="trash" size={14} />{tr("Remove", "Remover")}</button></div>
        {:else if selectedConnection}
          <p class="connection-route">{name(sessionFor(group!.steps.find((step) => step.id === selectedConnection.fromStepId)!))}<LumeIcon name="arrow-right" size={14} />{name(sessionFor(group!.steps.find((step) => step.id === selectedConnection.toStepId)!))}</p>
          <label class="field"><span>{tr("Shared context", "Contexto compartilhado")}</span><LumeSelect value={selectedConnection.contextPolicy} options={[{ value: "minimal", label: tr("Minimal", "Mínimo") }, { value: "standard", label: tr("Standard", "Padrão") }, { value: "detailed", label: tr("Detailed", "Detalhado") }, ...(selectedConnection.contextPolicy === "custom" ? [{ value: "custom", label: tr("Custom · Orb", "Personalizado · Orb") }] : [])]} ariaLabel={tr("Context policy", "Política de contexto")} minWidth={0} disabled={locked} onValueChange={(value) => changePolicy(value as WorkflowConnectionDefinition["contextPolicy"])} /></label>
          {#if selectedConnection.contextPolicy === "custom"}<p class="panel-help">{tr("Custom selections are edited in the Orb connection editor.", "As seleções personalizadas são editadas na ponte do Orb.")}</p>{/if}
          <label class="switch-field"><span>{tr("Require handoff approval", "Pedir aprovação do handoff")}</span><input type="checkbox" checked={selectedConnection.requiresApproval} disabled={locked} onchange={(event) => editConnection({ requiresApproval: event.currentTarget.checked })} /></label>
          <label class="switch-field"><span>{tr("Advance automatically", "Avançar automaticamente")}</span><input type="checkbox" checked={selectedConnection.advanceMode === "automatic"} disabled={locked} onchange={(event) => editConnection({ advanceMode: event.currentTarget.checked ? "automatic" : "manual" })} /></label>
          <label class="field"><span>{tr("Additional instruction", "Instrução adicional")}</span><textarea rows="3" disabled={locked} value={selectedConnection.additionalInstruction} oninput={(event) => editConnection({ additionalInstruction: event.currentTarget.value })}></textarea></label>
          <button class="preview-button" type="button" disabled={previewLoading || !objective.trim()} onclick={() => void showPreview()}><LumeIcon name="sources" size={15} />{previewLoading ? tr("Building preview…", "Gerando prévia…") : tr("View exact context", "Ver contexto exato")}</button>
          {#if !objective.trim()}<p class="panel-help">{tr("Add an objective below to preview the handoff.", "Preencha o objetivo abaixo para visualizar o handoff.")}</p>{/if}
          {#if preview}<div class="context-preview"><small>{preview.estimatedTokens.toLocaleString()} {tr("estimated tokens", "tokens estimados")}</small><pre>{preview.markdown}</pre></div>{/if}
          <div class="panel-actions"><button type="button" disabled={locked} onclick={() => openPicker(selectedConnection!.id)}><LumeIcon name="plus" size={14} />{tr("Insert agent", "Inserir agente")}</button><button class="danger" disabled={locked} type="button" onclick={removeSelected}><LumeIcon name="trash" size={14} />{tr("Remove", "Remover")}</button></div>
        {/if}
      </div>
    </aside>
  {/if}

  <div class="board-navigation" role="group" aria-label={tr("Canvas view", "Vista do canvas")}>
    <button type="button" aria-label={tr("Zoom out", "Diminuir zoom")} title={tr("Zoom out", "Diminuir zoom")} onclick={() => zoom(1 / 1.2)}><span aria-hidden="true">−</span></button>
    <button class="zoom-label" type="button" title={tr("Reset zoom", "Restaurar zoom")} aria-label={tr("Reset zoom", "Restaurar zoom")} onclick={() => patchLayout({ view: zoomAround(view, { x: viewportWidth / 2, y: viewportHeight / 2 }, 1) })}>{Math.round(view.zoom * 100)}%</button>
    <button type="button" aria-label={tr("Zoom in", "Aumentar zoom")} title={tr("Zoom in", "Aumentar zoom")} onclick={() => zoom(1.2)}><LumeIcon name="plus" size={14} /></button>
    <span></span><button type="button" aria-label={tr("Fit to view", "Ajustar à tela")} title={tr("Fit to view", "Ajustar à tela")} onclick={fit}><LumeIcon name="maximize" size={15} /></button>
  </div>
  {#if connectionSource}<p class="connection-hint">{targetCheck && !targetCheck.ok ? connectionRefusalMessage(targetCheck.reason, language === "pt-BR") : tr("Choose another card to connect · Esc to cancel", "Escolha outro card para conectar · Esc para cancelar")}</p>{/if}

  {#if group}
    <footer class="board-run" data-status={run?.status ?? "draft"}>
      <div class="run-heading"><span class="run-state"><i></i><strong>{runLoading ? tr("Loading run…", "Carregando execução…") : run ? runStatus(run.status) : tr("Ready to build", "Pronto para montar")}</strong></span><span class="run-progress">{run ? run.steps.filter((step) => ["completed", "skipped"].includes(step.status)).length : chain?.order.length ?? 0}/{group.steps.length} {tr("steps", "etapas")}</span><input class="workflow-name" aria-label={tr("Workflow name", "Nome do workflow")} value={groupName(group)} maxlength="80" onchange={(event) => patchLayout({ name: event.currentTarget.value.trim() || undefined })} /></div>
      {#if chain?.problems.length}
        <button class="chain-problem" type="button" onclick={() => { problemStepIds = chain!.problems[0].stepIds; fit(); }}><LumeIcon name="warning" size={13} />{problemMessage(chain.problems[0], language === "pt-BR")}</button>
      {:else if blockedStep && !locked}
        <button class="chain-problem" type="button" onclick={() => choose({ kind: "step", id: blockedStep!.id })}><LumeIcon name="warning" size={13} />{tr("Connect or take control of ", "Conecte ou assuma o controle de ") + name(sessionFor(blockedStep))}</button>
      {/if}
      {#if run?.error}<p class="run-error">{run.error}</p>{/if}
      <div class="run-controls">
        <textarea rows="1" maxlength="4000" aria-label={tr("Workflow objective", "Objetivo do workflow")} placeholder={tr("What should these agents accomplish?", "O que estes agentes devem realizar?")} disabled={locked} value={objective} oninput={(event) => { objectives = { ...objectives, [activeId]: event.currentTarget.value }; invalidatePreview(); persistLayout(); }}></textarea>
        <div class="run-actions">
          {#if !run || finished}
            <button class="primary" type="button" disabled={actionLoading || runLoading || changingRole || adding || !objective.trim() || Boolean(chain?.problems.length) || Boolean(blockedStep)} onclick={() => void execute()}><LumeIcon name="send" size={14} />{actionLoading ? tr("Starting…", "Iniciando…") : finished ? tr("Run again", "Executar novamente") : tr("Run", "Executar")}</button>
          {:else}
            {#if run.recovering}<small>{tr("Waiting for the original agent", "Aguardando o agente original")}</small>{/if}
            {#if run.status === "waiting_for_approval"}<button class="primary" type="button" disabled={actionLoading || run.recovering} onclick={() => void perform(() => approveWorkflowHandoff(activeId))}>{tr("Approve handoff", "Aprovar handoff")}</button>
            {:else if run.status === "ready"}<button class="primary" type="button" disabled={actionLoading || run.recovering} onclick={() => void perform(() => advanceWorkflowRun(activeId))}>{tr("Run next", "Executar próxima")}</button>
            {:else if run.status === "paused"}<button class="primary" type="button" disabled={actionLoading || run.recovering} onclick={() => void perform(() => resumeWorkflowRun(activeId))}>{tr("Resume", "Retomar")}</button>
            {:else if run.status === "failed"}<button class="primary" type="button" disabled={actionLoading || run.recovering} onclick={() => void perform(() => retryWorkflowStep(activeId))}>{tr("Retry", "Tentar novamente")}</button><button type="button" disabled={actionLoading || run.recovering} onclick={() => void perform(() => skipWorkflowStep(activeId))}>{tr("Skip", "Ignorar")}</button>{/if}
            {#if ["running", "ready", "waiting_for_approval"].includes(run.status)}<button type="button" disabled={actionLoading} onclick={() => void perform(() => pauseWorkflowRun(activeId))}>{tr("Pause", "Pausar")}</button>{/if}
            <button class="danger" type="button" disabled={actionLoading} onclick={() => void perform(() => cancelWorkflowRun(activeId))}><LumeIcon name="stop" size={13} />{tr("Stop", "Parar")}</button>
          {/if}
        </div>
      </div>
    </footer>
  {/if}
  <SystemBannerStack items={banners} {language} contained offset="65px" dismissLabel={tr("Dismiss", "Fechar")} />
</section>

<style>
  .workflow-board { position: absolute; z-index: 30; inset: 0; overflow: hidden; color: var(--workspace-text); background: var(--workspace-bg); isolation: isolate; --board-danger: #a83e3a; --board-warning: #91640f; --select-surface: var(--workspace-raised); --select-text: var(--workspace-strong); --select-muted: var(--workspace-muted); --select-line: var(--workspace-line); --select-accent: var(--workspace-accent); --select-hover: var(--workspace-subtle); --select-active: var(--workspace-accent-soft); }
  :global(.workspace.dark) .workflow-board { --board-danger: #ec9894; --board-warning: #e0b75b; }
  .workflow-board[hidden] { display: none; }
  .board-viewport { position: absolute; inset: 0; overflow: hidden; outline: none; touch-action: none; background-image: radial-gradient(circle, color-mix(in srgb, var(--workspace-muted) 28%, transparent) 1px, transparent 1px); cursor: grab; }
  .board-viewport.panning { cursor: grabbing; }
  .board-viewport.connecting { cursor: crosshair; }
  .board-viewport.drop-active { box-shadow: inset 0 0 0 2px var(--workspace-accent); }
  .board-world { position: absolute; inset: 0 auto auto 0; transform-origin: 0 0; width: 0; height: 0; }
  .board-edges { position: absolute; width: 1px; height: 1px; overflow: visible; pointer-events: none; }
  .edge-hit { fill: none; stroke: transparent; stroke-width: 18; pointer-events: stroke; cursor: pointer; }
  .edge-line { fill: none; stroke: var(--workspace-muted); stroke-width: 1.7; transition: stroke 160ms ease; }
  .selected .edge-line { stroke: var(--workspace-accent); stroke-width: 2.1; }
  .waiting .edge-line { stroke: var(--board-warning); stroke-dasharray: 6 5; }
  .edge-pending { fill: none; stroke: var(--workspace-accent); stroke-width: 1.8; stroke-dasharray: 5 5; }
  .edge-controls { position: absolute; display: flex; gap: 3px; transform: translate(-50%, -50%); }
  button { font: inherit; cursor: pointer; }
  button:disabled { opacity: .45; cursor: default; }
  button:focus-visible, input:focus-visible, textarea:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 3px; }
  .edge-controls button { width: 24px; height: 24px; padding: 0; display: grid; place-items: center; border: 1px solid var(--workspace-line); border-radius: 50%; color: var(--workspace-muted); background: var(--workspace-raised); }
  .edge-controls button:hover { color: var(--workspace-accent); border-color: var(--workspace-accent); }
  .edge-controls .waiting { color: var(--board-warning); border-color: var(--board-warning); }
  .board-card { position: absolute; box-sizing: border-box; border: 1px solid var(--workspace-line); border-radius: 14px; background: var(--workspace-raised); transition: border-color 140ms ease; }
  .board-card.selected, .board-card.target-valid { border-color: var(--workspace-accent); }
  .board-card.problem, .board-card.target-invalid { border-color: var(--board-danger); }
  .board-card.moving { z-index: 3; cursor: grabbing; }
  .card-body { width: 100%; height: 100%; padding: 17px 16px 14px; display: flex; flex-direction: column; justify-content: space-between; gap: 12px; border: 0; border-radius: inherit; color: inherit; background: transparent; text-align: left; user-select: none; cursor: grab; touch-action: none; }
  .moving .card-body { cursor: grabbing; }
  .card-identity { min-width: 0; display: flex; align-items: center; gap: 10px; }
  .card-copy { flex: 1; min-width: 0; display: grid; gap: 5px; }
  .card-copy strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--workspace-strong); font-size: 12px; font-weight: 650; }
  .card-copy small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--workspace-muted); font-size: 10px; }
  .step-number { align-self: flex-start; font-size: 10px; font-variant-numeric: tabular-nums; color: var(--workspace-muted); }
  .card-bottom { min-width: 0; display: flex; align-items: center; justify-content: space-between; gap: 8px; font-size: 10px; }
  .card-role { min-width: 0; display: flex; align-items: center; gap: 5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .card-role > span { width: 18px; height: 18px; display: grid; place-items: center; color: var(--workspace-accent); }
  .card-status { min-width: 0; max-width: 118px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--workspace-muted); }
  .card-status i, .run-state i { width: 6px; height: 6px; margin-right: 5px; display: inline-block; border-radius: 50%; background: var(--workspace-muted); }
  [data-state="running"] .card-status i, [data-status="running"] .run-state i { background: #56aada; }
  [data-state="completed"] .card-status i, [data-status="completed"] .run-state i { background: var(--workspace-accent); }
  [data-state="failed"] .card-status i, [data-status="failed"] .run-state i { background: var(--board-danger); }
  [data-state="permission_required"] .card-status i, [data-status="waiting_for_approval"] .run-state i, [data-status="paused"] .run-state i { background: var(--board-warning); }
  .card-port { position: absolute; top: calc(50% - 13px); width: 26px; height: 26px; padding: 0; display: grid; place-items: center; border: 0; color: var(--workspace-muted); background: transparent; touch-action: none; cursor: crosshair; }
  .port-in { left: -13px; }.port-out { right: -13px; }
  .card-port i { width: 8px; height: 8px; border: 1.5px solid currentColor; border-radius: 50%; background: var(--workspace-raised); transition: transform 140ms ease, color 140ms ease; }
  .card-port:hover i, .card-port:focus-visible i { color: var(--workspace-accent); transform: scale(1.25); }
  .board-toolbar { position: absolute; z-index: 5; top: 16px; left: 18px; right: 18px; display: flex; align-items: center; gap: 7px; min-width: 0; padding: 5px 8px; border: 1px solid var(--workspace-line); border-radius: 12px; background: var(--workspace-raised); }
  .board-title { display: flex; align-items: center; gap: 7px; padding: 0 5px; color: var(--workspace-strong); }
  .board-title strong { font-size: 12px; font-weight: 650; }
  .toolbar-divider { height: 19px; width: 1px; background: var(--workspace-line); }
  .board-toolbar button, .board-navigation button, .board-panel > header button { width: 29px; height: 29px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 7px; color: var(--workspace-muted); background: transparent; }
  .board-toolbar button:hover, .board-navigation button:hover, .board-panel > header button:hover { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .board-toolbar .add-agent { width: auto; display: flex; gap: 5px; padding: 0 8px; font-size: 11px; }
  .board-toolbar .close-board { margin-left: auto; }
  .save-state { margin-left: auto; color: var(--workspace-muted); font-size: 10px; }
  .save-state:empty { display: none; }
  .board-panel { position: absolute; z-index: 6; top: 70px; right: 18px; bottom: 157px; width: min(292px, calc(100% - 36px)); display: grid; grid-template-rows: auto minmax(0, 1fr); border: 1px solid var(--workspace-line); border-radius: 14px; background: var(--workspace-raised); overflow: hidden; }
  .board-panel > header { display: flex; align-items: center; justify-content: space-between; min-height: 42px; padding: 4px 12px 4px 16px; border-bottom: 1px solid var(--workspace-line); }
  .board-panel > header strong { font-size: 12px; font-weight: 650; color: var(--workspace-strong); }
  .panel-scroll { min-height: 0; padding: 16px; display: flex; flex-direction: column; gap: 15px; overflow-y: auto; overscroll-behavior: contain; scrollbar-width: thin; scrollbar-color: var(--workspace-line) transparent; }
  .panel-help { margin: 0; font-size: 11px; line-height: 1.55; color: var(--workspace-muted); }
  .agent-options { display: grid; gap: 4px; }
  .agent-options button { min-width: 0; padding: 9px 6px; display: flex; align-items: center; gap: 9px; border: 0; border-radius: 9px; color: var(--workspace-text); background: transparent; text-align: left; }
  .agent-options button:hover:not(:disabled) { background: var(--workspace-subtle); }
  .agent-options button > span, .selected-identity > span { min-width: 0; flex: 1; display: grid; gap: 4px; }
  .agent-options strong, .selected-identity strong { font-size: 11px; font-weight: 650; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .agent-options small, .selected-identity small { font-size: 10px; color: var(--workspace-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .selected-identity { display: flex; gap: 9px; align-items: center; }
  .field { min-width: 0; display: grid; gap: 7px; font-size: 11px; }
  .field > span { color: var(--workspace-muted); }
  .field :global(.lume-select) { width: 100%; }
  .role-picker { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 5px; }
  .role-option { min-width: 0; min-height: 52px; padding: 7px 4px 6px; display: grid; justify-items: center; align-content: center; gap: 4px; border: 1px solid var(--workspace-line); border-radius: 9px; color: var(--workspace-muted); background: var(--workspace-subtle); font: inherit; font-size: 10px; font-weight: 650; cursor: pointer; transition: color 120ms ease, border-color 120ms ease, background 120ms ease; }
  .role-option > span { max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .role-option :global(.workflow-role-icon) { width: 18px; height: 18px; }
  .role-option:hover:not(:disabled) { color: var(--workspace-strong); border-color: color-mix(in srgb, var(--workspace-accent) 40%, var(--workspace-line)); }
  .role-option.selected { color: var(--workspace-accent); border-color: var(--workspace-accent); background: var(--workspace-accent-soft); }
  .role-option:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 2px; }
  .role-option:disabled { opacity: .5; cursor: default; }
  .field input, .field textarea, .run-controls textarea { box-sizing: border-box; width: 100%; min-width: 0; padding: 8px 10px; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-strong); background: var(--workspace-subtle); font: inherit; line-height: 1.5; resize: vertical; caret-color: var(--workspace-accent); }
  .field textarea { min-height: 56px; max-height: 220px; }
  .switch-field { display: flex; gap: 10px; align-items: center; justify-content: space-between; font-size: 11px; line-height: 1.5; }
  .switch-field input { accent-color: var(--workspace-accent); width: 15px; height: 15px; }
  .panel-actions { display: flex; gap: 6px; margin-top: 5px; }
  .panel-actions button, .preview-button, .run-actions button, .board-empty button { min-height: 31px; padding: 0 10px; display: flex; align-items: center; justify-content: center; gap: 6px; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-text); background: var(--workspace-subtle); font-size: 11px; }
  .panel-actions button:hover:not(:disabled), .preview-button:hover:not(:disabled), .run-actions button:hover:not(:disabled) { border-color: var(--workspace-accent); }
  .panel-actions .danger, .run-actions .danger { color: var(--board-danger); background: transparent; }
  .connection-route { margin: 0; display: flex; flex-wrap: wrap; align-items: center; gap: 6px; font-size: 11px; line-height: 1.5; overflow-wrap: anywhere; }
  .session-warning { margin: 0; display: flex; align-items: flex-start; gap: 7px; color: var(--board-warning); font-size: 11px; line-height: 1.5; }
  .context-preview { min-width: 0; display: grid; gap: 8px; }
  .context-preview small { font-size: 10px; color: var(--workspace-muted); font-variant-numeric: tabular-nums; }
  .context-preview pre { max-height: 250px; margin: 0; overflow-y: auto; white-space: pre-wrap; overflow-wrap: anywhere; font: 10px/1.55 ui-monospace, monospace; color: var(--workspace-text); }
  .board-navigation { position: absolute; z-index: 5; left: 18px; bottom: 153px; display: flex; align-items: center; gap: 2px; padding: 4px; border: 1px solid var(--workspace-line); border-radius: 10px; background: var(--workspace-raised); }
  .board-navigation > span { width: 1px; height: 15px; margin: 0 4px; background: var(--workspace-line); }
  .board-navigation .zoom-label { width: 44px; font-size: 10px; font-variant-numeric: tabular-nums; }
  .connection-hint { position: absolute; z-index: 4; bottom: 155px; left: 50%; width: max-content; max-width: calc(100% - 50px); padding: 9px 12px; margin: 0; border-radius: 8px; background: var(--workspace-raised); color: var(--workspace-strong); font-size: 11px; transform: translateX(-50%); pointer-events: none; }
  .board-empty { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; flex-direction: column; gap: 14px; padding: 70px 28px 100px; box-sizing: border-box; pointer-events: none; text-align: center; }
  .board-empty > :global(svg) { color: var(--workspace-accent); }
  .board-empty strong { margin-top: 4px; color: var(--workspace-strong); font-size: 19px; font-weight: 600; letter-spacing: -.02em; }
  .board-empty p { margin: 0; max-width: 300px; font-size: 12px; line-height: 1.6; color: var(--workspace-muted); }
  .board-empty button { margin: 3px 0; pointer-events: auto; color: var(--workspace-strong); }
  .board-empty small { max-width: 280px; font-size: 10px; color: var(--workspace-muted); line-height: 1.5; }
  .board-run { position: absolute; z-index: 5; left: 18px; right: 18px; bottom: 16px; padding: 12px 14px; display: grid; gap: 10px; border: 1px solid var(--workspace-line); border-radius: 12px; background: var(--workspace-raised); }
  .run-heading { min-width: 0; display: flex; align-items: center; gap: 9px; }
  .run-state { display: flex; align-items: center; color: var(--workspace-strong); font-size: 11px; white-space: nowrap; }
  .run-state strong { font-weight: 600; }
  .run-progress { color: var(--workspace-muted); font-size: 10px; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .workflow-name { min-width: 0; width: 130px; margin-left: auto; border: 0; padding: 3px 5px; border-radius: 5px; color: var(--workspace-muted); background: transparent; text-align: right; font-family: inherit; font-size: 10px; }
  .workflow-name:hover { background: var(--workspace-subtle); }
  .run-controls { display: flex; align-items: flex-end; gap: 10px; }
  .run-controls textarea { min-height: 35px; max-height: 100px; flex: 1; font-size: 11px; resize: none; }
  .run-controls textarea::placeholder { color: var(--workspace-muted); }
  .run-actions { display: flex; align-items: center; justify-content: flex-end; gap: 5px; flex-wrap: wrap; }
  .run-actions .primary { color: var(--workspace-strong); border-color: color-mix(in srgb, var(--workspace-accent) 48%, var(--workspace-line)); background: var(--workspace-accent-soft); }
  .run-actions small { flex-basis: 100%; font-size: 10px; color: var(--workspace-muted); text-align: right; }
  .chain-problem { min-width: 0; padding: 0; display: flex; align-items: flex-start; gap: 6px; border: 0; color: var(--board-warning); background: transparent; text-align: left; font-size: 10px; line-height: 1.4; }
  .run-error { margin: 0; max-height: 44px; overflow: auto; color: var(--board-danger); font-size: 10px; line-height: 1.5; }
  @media (max-width: 820px) {
    .board-toolbar { gap: 3px; left: 12px; right: 12px; }
    .board-title strong, .save-state { display: none; }
    .board-toolbar :global(.lume-select) { min-width: 95px !important; }
    .board-run { left: 12px; right: 12px; }
    .board-panel { right: 12px; width: min(278px, calc(100% - 24px)); }
    .run-controls { flex-wrap: wrap; }
    .run-controls textarea { flex-basis: 100%; }
    .run-actions { margin-left: auto; }
    .workflow-name { width: 95px; }
    .board-navigation { bottom: 190px; }
  }
  @media (prefers-reduced-motion: reduce) { .board-card, .card-port i, .edge-line { transition: none; } }
</style>
