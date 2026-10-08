<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import type { Preferences, WorkflowContextPolicy, WorkflowContextSelection, WorkflowGroupDefinition, WorkflowRole, WorkflowStepDefinition, WorkflowConnectionDefinition, WorkflowContextPackage, WorkflowRun } from "$lib/domain";
  import type { HubSession } from "$lib/hubProtocol";
  import type { Language } from "$lib/i18n";
  import { localize } from "$lib/i18n";
  import { loadWorkflowRoleContract, loadWorkflowRun, startWorkflowRun, approveWorkflowHandoff, advanceWorkflowRun, pauseWorkflowRun, resumeWorkflowRun, retryWorkflowStep, skipWorkflowStep, cancelWorkflowRun, previewWorkflowContext } from "$lib/lume";
  import { BOARD_CARD_WIDTH, BOARD_CARD_HEIGHT, analyzeChain, checkConnection, connectionRefusalMessage, problemMessage, newGroup, newStep, preserveRoleOverrides, defaultRole, defaultContextSelection, addStep, updateStep, removeStep, addConnection, updateConnection, removeConnection, insertStep, edgeGeometry, cardRect, cardAt, placeCard, toBoardPoint, zoomAround, fitView, normalizeLayout, stepVisualState, connectionWaitsForApproval, type BoardPoint, type BoardLayout, type BoardView } from "$lib/workflowBoard";
  import { WorkflowBoardSaveQueue } from "$lib/workflowBoardPersistence";
  import { activityFeed, lastResponse, stepActivity } from "$lib/boardActivity";
  import { formatAgentDuration } from "$lib/activityPresentation";
  import { demoFrames, demoSession, demoTodoCount, type DemoState } from "$lib/boardSimulator";
  import ThreadAvatar from "$lib/ThreadAvatar.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import LumeSelect from "$lib/LumeSelect.svelte";
  import WorkflowRoleIcon from "$lib/WorkflowRoleIcon.svelte";
  import BrandIcon from "$lib/BrandIcon.svelte";
  import WorkspaceHeaderIcon from "$lib/WorkspaceHeaderIcon.svelte";
  import SystemBannerStack, { type SystemBannerItem } from "$lib/SystemBannerStack.svelte";

  let { sessions, preferences, language, active, keyboardEnabled = true, onSaveGroup, onDeleteGroup, onOpenChat, onClose, onFinishSidebarDrag, draggingSessionId = null, onSidebarDragOverBoard }: {
    sessions: HubSession[];
    preferences: Preferences;
    language: Language;
    active: boolean;
    keyboardEnabled?: boolean;
    onSaveGroup: (group: WorkflowGroupDefinition) => Promise<void>;
    onDeleteGroup: (id: string) => Promise<void>;
    onOpenChat: (session: HubSession) => void;
    onClose: () => void;
    onFinishSidebarDrag: () => void;
    /** The sidebar agent being dragged, so the board can show it as a card under the cursor. */
    draggingSessionId?: string | null;
    onSidebarDragOverBoard?: (over: boolean) => void;
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
  let cardOutside = $state(false);
  let confirmingDelete = $state(false);
  let deleting = $state(false);
  let confirmTimer: ReturnType<typeof setTimeout> | undefined;
  let dragPoint = $state<BoardPoint | null>(null);
  let instructionOpen = $state(false);
  let previewControlsOpen = $state(false);
  let previewOpen = $state(false);
  let edgeMenu = $state<{ id: string; mode: "menu" | "settings" | "insert" } | null>(null);
  let problemStepIds = $state<string[]>([]);

  const queue = new WorkflowBoardSaveQueue((next) => onSaveGroup(next));
  const groups = $derived([
    ...preferences.workflowGroups.map((item) => drafts[item.id] ?? item),
    ...Object.values(drafts).filter((item) => !preferences.workflowGroups.some((saved) => saved.id === item.id)),
  ]);
  const group = $derived(groups.find((item) => item.id === activeId) ?? null);
  const layout = $derived(layouts[activeId] ?? normalizeLayout(null));
  const view = $derived(layout.view);
  // A simulation previews every card and pipe state without touching a real run or any agent.
  const simulatorAvailable = Boolean(import.meta.env?.DEV);
  let demo = $state<{ frame: number; playing: boolean; manual: DemoState[] | null; started: Record<string, number> } | null>(null);
  const demoScript = $derived(group ? demoFrames(group.steps.length) : []);
  const demoCells = $derived(demo && group ? demo.manual ?? demoScript[Math.min(demo.frame, demoScript.length - 1)] ?? null : null);
  const demoRun = $derived.by((): WorkflowRun | null => {
    if (!demo || !group || !demoCells) return null;
    const stamp = Date.now();
    const steps = group.steps.map((step, index) => {
      const cell = demoCells[index];
      const startedAt = demo!.started[step.id] ?? stamp;
      return { stepId: step.id, status: cell.state === "idle" ? "pending" : cell.state, attempt: 1, startedAt: ["running", "completed", "failed"].includes(cell.state) ? startedAt : undefined, completedAt: cell.state === "completed" || cell.state === "failed" ? startedAt + 4000 + index * 900 : undefined } as WorkflowRun["steps"][number];
    });
    const status = steps.some((item) => item.status === "failed") ? "failed" : steps.some((item) => item.status === "running") ? "running" : steps.every((item) => item.status === "completed") ? "completed" : "ready";
    return { id: "demo", workflowId: group.id, objective: "", status, currentStepId: steps.find((item) => item.status === "running")?.stepId, handoffApproved: false, recovering: false, transitionCount: 0, steps, createdAt: stamp, updatedAt: stamp } as WorkflowRun;
  });
  const run = $derived(demoRun ?? runs[activeId] ?? null);
  const finished = $derived(Boolean(run && ["draft", "completed", "cancelled"].includes(run.status)));
  const locked = $derived(!demo && (actionLoading || runLoading || Boolean(run && !finished)));
  function applyFrame(index: number, manual: DemoState[] | null = null) {
    if (!demo || !group) return;
    const next = manual ?? demoScript[index] ?? [];
    const started = { ...demo.started };
    group.steps.forEach((step, position) => {
      const was = demoCells?.[position]?.state;
      if (next[position]?.state === "running" && was !== "running") started[step.id] = Date.now();
      if (next[position]?.state === "pending") delete started[step.id];
    });
    demo = { ...demo, frame: index, manual, started };
  }
  function toggleDemo() {
    if (demo) { demo = null; return; }
    if (!group?.steps.length) { notify(tr("Add agents to simulate their states.", "Adicione agentes para simular os estados."), "info"); return; }
    choose(null);
    demo = { frame: 0, playing: true, manual: null, started: { [group.steps[0].id]: Date.now() } };
  }
  function editDemo(index: number, patch: Partial<DemoState>) {
    if (!demo || !demoCells) return;
    const manual = demoCells.map((cell, position) => (position === index ? { ...cell, ...patch } : cell));
    applyFrame(demo.frame, manual);
    demo = { ...demo!, playing: false };
  }
  $effect(() => {
    if (!demo?.playing || !group) return;
    const timer = setInterval(() => applyFrame(demo!.frame + 1 >= demoScript.length ? 0 : demo!.frame + 1), reducedMotion ? 2600 : 2100);
    return () => clearInterval(timer);
  });
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
  // The elapsed time of a running step ticks once a second, only while something is running.
  let panelTab = $state<"settings" | "activity">("settings");
  // The board fades over the chats instead of cutting: it stays displayed while it fades out.
  let shown = $state(untrack(() => active));
  let entered = $state(false);
  $effect(() => {
    if (active) {
      shown = true;
      const frame = requestAnimationFrame(() => { entered = true; });
      return () => cancelAnimationFrame(frame);
    }
    entered = false;
    const reduced = typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
    const timer = setTimeout(() => { shown = false; }, reduced ? 0 : 240);
    return () => clearTimeout(timer);
  });
  let now = $state(Date.now());
  $effect(() => {
    if (!active || !run || !run.steps.some((step) => step.status === "running")) return;
    now = Date.now();
    const timer = setInterval(() => { now = Date.now(); }, 1000);
    return () => clearInterval(timer);
  });
  const dragSession = $derived(draggingSessionId ? sessions.find((item) => item.id === draggingSessionId) : undefined);
  const dragIssue = $derived.by(() => {
    if (!dragSession) return null;
    if (locked) return "locked" as const;
    if (!dragSession.nativeSessionId?.trim()) return "unconnected" as const;
    if (group?.steps.some((step) => step.sessionNativeId === dragSession.nativeSessionId)) return "present" as const;
    return null;
  });
  const dragRole = $derived<WorkflowRole>(defaultRole(group?.steps.length ?? 0));
  let popoverHeight = $state(0);
  /** The popover is placed from its measured height so it fits the window instead of scrolling. */
  const edgeAnchor = $derived.by(() => {
    const entry = edgeMenu ? edges.find((item) => item.connection.id === edgeMenu!.id) : undefined;
    if (!entry) return null;
    const margin = 12;
    const x = view.x + entry.geometry.mid.x * view.zoom;
    const y = view.y + entry.geometry.mid.y * view.zoom;
    const width = Math.min(edgeMenu!.mode === "menu" ? 262 : edgeMenu!.mode === "settings" ? 360 : 312, Math.max(200, viewportWidth - margin * 2));
    const room = Math.max(160, viewportHeight - margin * 2);
    const height = Math.min(popoverHeight || 360, room);
    const belowTop = y + 22;
    const belowSpace = viewportHeight - margin - belowTop;
    const aboveSpace = y - 22 - margin;
    const placeBelow = belowSpace >= height || belowSpace >= aboveSpace;
    const top = placeBelow ? Math.min(belowTop, viewportHeight - margin - height) : Math.max(margin, y - 22 - height);
    const left = Math.max(margin, Math.min(viewportWidth - width - margin, x - width / 2));
    // When the window is too short to sit beside the button, the popover slides over it and drops its pointer.
    const attached = placeBelow ? top >= y + 14 : top + height <= y - 14;
    return { left, caret: Math.max(18, Math.min(width - 18, x - left)), width, placeBelow, attached, top, maxHeight: room, compact: viewportHeight < 660 };
  });
  let helpHeight = $state(0);
  /** The help opens beside the connection window (right, else left) and slides to stay inside the board. */
  const helpLayout = $derived.by(() => {
    const anchor = edgeAnchor;
    if (!anchor) return null;
    const margin = 12;
    const rightRoom = viewportWidth - margin - (anchor.left + anchor.width + 10);
    const leftRoom = anchor.left - 10 - margin;
    const side = rightRoom >= 250 ? "right" : leftRoom >= 250 ? "left" : "inside";
    const width = Math.min(310, side === "right" ? rightRoom : leftRoom);
    const height = helpHeight || 430;
    const shift = Math.max(margin - anchor.top, Math.min(0, viewportHeight - margin - (anchor.top + height)));
    return { side, width, top: side === "inside" ? 0 : shift };
  });
  const helpItems: { icon: string; title: [string, string]; text: [string, string] }[] = [
    { icon: "M4 5h12M4 10h12M4 15h12", title: ["Context policy", "Política de contexto"], text: ["Minimal, standard, detailed, or your custom selection.", "Mínimo, padrão, detalhado ou sua seleção personalizada."] },
    { icon: "M4 4h12v9H9l-4 3v-3H4z", title: ["Response", "Resposta"], text: ["Sends the agent's final answer.", "Envia a resposta final do agente."] },
    { icon: "M3.5 5.5h5l1.5 2h6.5v8h-13z", title: ["Files", "Arquivos"], text: ["Includes paths and change totals.", "Inclui caminhos e totais alterados."] },
    { icon: "m4 10 3.5 3.5L16 5", title: ["Checks", "Validações"], text: ["Shares tests and validation results.", "Compartilha testes e resultados de validação."] },
    { icon: "M5 5h10M5 10h10M5 15h7", title: ["Plan and activity", "Plano e atividade"], text: ["Adds the relevant plan and recent actions.", "Adiciona o plano relevante e as ações recentes."] },
    { icon: "M7 4 3 10l4 6M13 4l4 6-4 6", title: ["Diffs", "Diffs"], text: ["Adds sanitized excerpts from changed files.", "Adiciona trechos sanitizados dos arquivos alterados."] },
    { icon: "M4 7h10m-3-3 3 3-3 3M16 13H6m3-3-3 3 3 3", title: ["Manual / Auto", "Manual / Auto"], text: ["Manual waits for you. Auto continues when ready, pausing whenever approval is required.", "Manual espera por você. Auto continua quando estiver pronto e pausa sempre que uma aprovação for exigida."] },
    { icon: "M10 3.5 15 5v4c0 3.2-1.9 5.6-5 7-3.1-1.4-5-3.8-5-7V5z", title: ["Approval", "Aprovação"], text: ["Requires confirmation before continuing.", "Exige confirmação antes de continuar."] },
    { icon: "m5 14 1-4 7-7 3 3-7 7zM12 4l3 3", title: ["Instruction", "Instrução"], text: ["Adds guidance for the next agent.", "Adiciona orientação para o próximo agente."] },
  ];
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
    edgeMenu = null;
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
    edgeMenu = null;
    previewOpen = false;
    invalidatePreview();
  }
  function openPicker() {
    if (locked) return;
    if (!group) { createGroup(); return; }
    selection = null;
    insertingConnectionId = null;
    edgeMenu = null;
    pickerOpen = true;
  }
  /** The menu, its settings and the intermediate-agent picker all open under the button in the middle of the pipe. */
  function openEdgeMenu(id: string, mode: "menu" | "settings" | "insert" = "menu") {
    if (mode === "insert" && locked) return;
    if (!(selection?.kind === "connection" && selection.id === id)) choose({ kind: "connection", id });
    pickerOpen = false;
    insertingConnectionId = mode === "insert" ? id : null;
    previewOpen = false;
    edgeMenu = { id, mode };
  }
  function toggleEdgeMenu(id: string) {
    if (edgeMenu?.id === id) choose(null);
    else openEdgeMenu(id);
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
      if (activeId === id) { pickerOpen = false; insertingConnectionId = null; edgeMenu = null; choose({ kind: "step", id: step.id }); }
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
  function cargoLabels(selection: WorkflowContextSelection): string[] {
    const names: Array<[keyof WorkflowContextSelection, string, string]> = [["response", "Response", "Resposta"], ["files", "Files", "Arquivos"], ["checks", "Checks", "Testes"], ["plan", "Plan", "Plano"], ["activity", "Activity", "Atividade"], ["diffs", "Diffs", "Mudanças"]];
    return names.filter(([key]) => selection[key]).map(([, en, pt]) => tr(en, pt));
  }
  function effectiveContextSelection(item: WorkflowConnectionDefinition): WorkflowContextSelection {
    if (item.contextPolicy === "minimal") return { response: true, files: false, checks: false, plan: false, activity: false, diffs: false };
    if (item.contextPolicy === "detailed") return { response: true, files: true, checks: true, plan: true, activity: true, diffs: true };
    if (item.contextPolicy === "custom") return item.contextSelection;
    return defaultContextSelection();
  }
  function changePolicy(policy: WorkflowContextPolicy) {
    const current = selectedConnection;
    if (!current) return;
    const selected = policy === "custom" ? effectiveContextSelection(current) : effectiveContextSelection({ ...current, contextPolicy: policy });
    editConnection({ contextPolicy: policy, contextSelection: selected, includeResponse: selected.response, includeFiles: selected.files, includeTests: selected.checks });
  }
  function removeStepCard(id: string) {
    if (!group || locked) return;
    const positions = { ...layout.positions };
    delete positions[id];
    patchLayout({ positions });
    edit(removeStep(group, id));
    choose(null);
  }
  function toggleContextOption(key: keyof WorkflowContextSelection) {
    const current = selectedConnection;
    if (!current) return;
    const before = effectiveContextSelection(current);
    const next = { ...before, [key]: !before[key] };
    if (key === "diffs" && next.diffs) next.files = true;
    if (key === "files" && !next.files) next.diffs = false;
    editConnection({ contextPolicy: "custom", contextSelection: next, includeResponse: next.response, includeFiles: next.files, includeTests: next.checks });
  }
  /** Swapping the ends is only offered when the chain stays valid. */
  function reverseConnection() {
    if (!group || !selectedConnection || locked) return;
    const candidate = updateConnection(group, selectedConnection.id, { fromStepId: selectedConnection.toStepId, toStepId: selectedConnection.fromStepId });
    if (analyzeChain(candidate).problems.length) { notify(tr("Reversing this connection would break the workflow order.", "Inverter esta conexão quebraria a ordem do workflow."), "warning"); return; }
    edit(candidate);
  }
  async function togglePreview() {
    if (previewOpen) { previewOpen = false; return; }
    if (!objective.trim()) return;
    previewOpen = true;
    await showPreview();
  }
  function removeSelected() {
    if (!group || !selection || locked) return;
    if (selection.kind === "step") { removeStepCard(selection.id); return; }
    edit(removeConnection(group, selection.id));
    choose(null);
  }
  function requestDeleteGroup() {
    if (!group || locked || deleting) return;
    if (!confirmingDelete) {
      confirmingDelete = true;
      clearTimeout(confirmTimer);
      confirmTimer = setTimeout(() => { confirmingDelete = false; }, 4000);
      return;
    }
    void deleteGroup();
  }
  async function deleteGroup() {
    if (!group) return;
    const id = group.id;
    clearTimeout(confirmTimer);
    confirmingDelete = false;
    deleting = true;
    try {
      queue.discard(id);
      await queue.settle();
      await onDeleteGroup(id);
      const without = <T,>(record: Record<string, T>) => Object.fromEntries(Object.entries(record).filter(([key]) => key !== id));
      drafts = without(drafts);
      layouts = without(layouts);
      objectives = without(objectives);
      runs = without(runs);
      saveFailed = false;
      activeId = "";
      cancelGesture();
      choose(null);
      persistLayout();
    } catch (error) { notify(errorMessage(error)); }
    finally { if (!disposed) deleting = false; }
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
    if (gesture.kind === "card") {
      const bounds = viewport.getBoundingClientRect();
      cardOutside = event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom;
    }
    if (gesture.kind === "pan") patchLayout({ view: { ...view, x: gesture.origin.x + dx, y: gesture.origin.y + dy } });
    else if (gesture.kind === "card" && gesture.stepId) patchLayout({ positions: { ...layout.positions, [gesture.stepId]: { x: gesture.origin.x + dx / view.zoom, y: gesture.origin.y + dy / view.zoom } } });
  }
  function endPointer(event: PointerEvent) {
    const current = gesture;
    if (!current || current.pointerId !== event.pointerId) return;
    // Connecting can end the gesture itself, so the checks below use the snapshot taken above.
    if (current.kind === "connection" && connectionTarget) connectTo(connectionTarget);
    if (current.kind === "card" && current.stepId && cardOutside) {
      // Dropping a card outside the canvas takes it out of the workflow; a running workflow only gets its card back.
      if (locked) patchLayout({ positions: { ...layout.positions, [current.stepId]: current.origin } });
      else { const id = current.stepId; cancelGesture(); removeStepCard(id); return; }
    }
    cancelGesture();
  }
  function cancelGesture() {
    const previous = gesture;
    gesture = null;
    cardOutside = false;
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
    const panelSpace = selectedStep || pickerOpen ? Math.min(310, width * 0.45) : 0;
    const positions = group?.steps.map((step) => layout.positions[step.id]).filter((point): point is BoardPoint => Boolean(point)) ?? [];
    patchLayout({ view: fitView(positions, width - panelSpace, height - 160) });
  }
  /** The wheel zooms around the cursor; the canvas is moved by dragging its background. */
  function wheel(event: WheelEvent) {
    if (!active || !viewport) return;
    event.preventDefault();
    const unit = event.deltaMode === 1 ? 33 : event.deltaMode === 2 ? viewportHeight : 1;
    const delta = Math.max(-240, Math.min(240, (event.deltaY || event.deltaX) * unit));
    const bounds = viewport.getBoundingClientRect();
    patchLayout({ view: zoomAround(view, { x: event.clientX - bounds.left, y: event.clientY - bounds.top }, view.zoom * Math.exp(-delta * 0.0022)) });
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
    if (event.dataTransfer) event.dataTransfer.dropEffect = locked || dragIssue ? "none" : "move";
    if (viewport) dragPoint = boardPoint(event.clientX, event.clientY);
    onSidebarDragOverBoard?.(true);
  }
  function endDragOver() {
    dropActive = false;
    dragPoint = null;
    onSidebarDragOverBoard?.(false);
  }
  function drop(event: DragEvent) {
    event.preventDefault();
    event.stopPropagation();
    const session = sessions.find((item) => item.id === event.dataTransfer?.getData("text/x-lume-session"));
    const point = viewport ? boardPoint(event.clientX, event.clientY) : undefined;
    endDragOver();
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

  $effect(() => { if (!draggingSessionId && dragPoint) endDragOver(); });

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

{#snippet agentChoices()}
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
{/snippet}

{#snippet connectionSettings(connection: WorkflowConnectionDefinition)}
  {@const sourceStep = group?.steps.find((step) => step.id === connection.fromStepId)}
  {@const targetStep = group?.steps.find((step) => step.id === connection.toStepId)}
  {@const sourceSession = sessionFor(sourceStep)}
  {@const targetSession = sessionFor(targetStep)}
  {@const shares = effectiveContextSelection(connection)}
  <div class="bridge" class:run-locked={locked}>
    <header class="route-heading">
      <button type="button" aria-label={tr("Back", "Voltar")} title={tr("Back", "Voltar")} onclick={() => openEdgeMenu(connection.id)}><svg viewBox="0 0 20 20"><path d="m12 5-5 5 5 5" /></svg></button>
      <span class="agent">{#if sourceSession}<BrandIcon name={sourceSession.agent} size={16} />{/if}<strong>{name(sourceSession)}</strong></span>
      <button class="reverse" type="button" title={tr("Reverse direction", "Inverter direção")} aria-label={tr("Reverse direction", "Inverter direção")} onclick={reverseConnection}><svg viewBox="0 0 20 20"><path d="M4 7h11m-3-3 3 3-3 3M16 13H5m3 3-3-3 3-3" /></svg></button>
      <span class="agent target"><strong>{name(targetSession)}</strong>{#if targetSession}<BrandIcon name={targetSession.agent} size={16} />{/if}</span>
      <button type="button" aria-label={tr("Close", "Fechar")} title={tr("Close", "Fechar")} onclick={() => choose(null)}><svg viewBox="0 0 20 20"><path d="m6 6 8 8M14 6l-8 8" /></svg></button>
    </header>

    {#if previewOpen}
      <section class="bridge-preview" aria-live="polite">
        <header>
          <strong>{tr("Exact context", "Contexto exato")}</strong>
          {#if preview}<span>~{preview.estimatedTokens.toLocaleString()} tokens</span>{/if}
          <button type="button" aria-label={tr("Close preview", "Fechar prévia")} onclick={() => (previewOpen = false)}><svg viewBox="0 0 20 20"><path d="m6 6 8 8M14 6l-8 8" /></svg></button>
        </header>
        {#if previewLoading}
          <span class="preview-note"><i></i>{tr("Building safe context…", "Montando contexto seguro…")}</span>
        {:else if preview}
          {#if preview.redactions.length}<div class="redaction-summary">{#each preview.redactions as redaction}<span>{redaction.count} · {redaction.summary}</span>{/each}</div>{/if}
          <pre>{preview.markdown}</pre>
        {:else}
          <span class="preview-note">{objective.trim() ? tr("Context changed. Build the preview again.", "O contexto mudou. Gere a prévia novamente.") : tr("Add an objective to generate the preview.", "Adicione um objetivo para gerar a prévia.")}</span>
        {/if}
      </section>
    {:else}
      <section class="context-policy" aria-label={tr("Context policy", "Política de contexto")}>
        <strong>{tr("Context", "Contexto")}</strong>
        <div>
          {#each [["minimal", "Minimal", "Mínimo"], ["standard", "Standard", "Padrão"], ["detailed", "Detailed", "Detalhado"], ["custom", "Custom", "Personalizado"]] as [policy, en, pt]}
            <button class:active={connection.contextPolicy === policy} type="button" disabled={locked} onclick={() => changePolicy(policy as WorkflowContextPolicy)}>{tr(en, pt)}</button>
          {/each}
        </div>
      </section>

      <section class="share-options" aria-label={tr("Shared context", "Contexto compartilhado")}>
        <button class:active={shares.response} type="button" disabled={locked} aria-pressed={shares.response} title={tr("Final response", "Resposta final")} onclick={() => toggleContextOption("response")}><svg viewBox="0 0 20 20"><path d="M4 4h12v9H9l-4 3v-3H4Z" /></svg><span>{tr("Response", "Resposta")}</span><i>✓</i></button>
        <button class:active={shares.files} type="button" disabled={locked} aria-pressed={shares.files} title={tr("Changed files", "Arquivos alterados")} onclick={() => toggleContextOption("files")}><svg viewBox="0 0 20 20"><path d="M3.5 5.5h5l1.5 2h6.5v8h-13Z" /></svg><span>{tr("Files", "Arquivos")}</span><i>✓</i></button>
        <button class:active={shares.checks} type="button" disabled={locked} aria-pressed={shares.checks} title={tr("Tests and checks", "Testes e validações")} onclick={() => toggleContextOption("checks")}><svg viewBox="0 0 20 20"><path d="m4 10 3.5 3.5L16 5" /></svg><span>{tr("Checks", "Validações")}</span><i>✓</i></button>
        <button class:active={shares.plan} type="button" disabled={locked} aria-pressed={shares.plan} title={tr("Relevant plan", "Plano relevante")} onclick={() => toggleContextOption("plan")}><svg viewBox="0 0 20 20"><path d="M5 5h10M5 10h10M5 15h7" /></svg><span>{tr("Plan", "Plano")}</span><i>✓</i></button>
        <button class:active={shares.activity} type="button" disabled={locked} aria-pressed={shares.activity} title={tr("Relevant activity", "Atividade relevante")} onclick={() => toggleContextOption("activity")}><svg viewBox="0 0 20 20"><path d="M4 13h3l2-7 3 9 2-5h2" /></svg><span>{tr("Activity", "Atividade")}</span><i>✓</i></button>
        <button class:active={shares.diffs} type="button" disabled={locked} aria-pressed={shares.diffs} title={tr("Sanitized diffs", "Diffs sanitizados")} onclick={() => toggleContextOption("diffs")}><svg viewBox="0 0 20 20"><path d="M7 4 3 10l4 6M13 4l4 6-4 6M11 3 9 17" /></svg><span>Diffs</span><i>✓</i></button>
      </section>

      <div class="behavior-row">
        <span class="transition-toggle" class:manual-active={connection.advanceMode === "manual"}>
          <button class:active={connection.advanceMode === "manual"} type="button" disabled={locked} aria-label={tr("Manual", "Manual")} onclick={() => editConnection({ advanceMode: "manual" })}><svg viewBox="0 0 20 20" aria-hidden="true"><path d="M5 4v12M9 6v8M13 7.5v5M17 9v2" /></svg><span>{tr("Manual", "Manual")}</span></button>
          <button class:active={connection.advanceMode === "automatic"} type="button" disabled={locked} aria-label={tr("Automatic", "Automático")} title={tr("Continue automatically when the step is ready", "Continuar automaticamente quando a etapa estiver pronta")} onclick={() => editConnection({ advanceMode: "automatic" })}><svg viewBox="0 0 20 20" aria-hidden="true"><path d="m11.5 2.5-6 8H10l-1.5 7 6-9H10Z" /></svg><span>Auto</span></button>
        </span>
        <button class:active={connection.requiresApproval} class="approval" type="button" role="switch" aria-checked={connection.requiresApproval} disabled={locked} onclick={() => editConnection({ requiresApproval: !connection.requiresApproval })}><svg viewBox="0 0 20 20"><path d="M10 3 16 5v4c0 4-2.5 6.5-6 8-3.5-1.5-6-4-6-8V5Z" /></svg><span>{tr("Approval", "Aprovação")}</span><i></i></button>
      </div>

      <button class:open={instructionOpen} class="instruction-toggle" type="button" onclick={() => { instructionOpen = !instructionOpen; if (instructionOpen) previewControlsOpen = false; }}>
        <svg viewBox="0 0 20 20"><path d="M4 15.5h3l8-8-3-3-8 8Zm6.5-9.5 3 3" /></svg>
        <span>{connection.additionalInstruction.trim() ? tr("Edit instruction", "Editar instrução") : tr("Add instruction", "Adicionar instrução")}</span>
        <svg viewBox="0 0 20 20"><path d="m6 8 4 4 4-4" /></svg>
      </button>
      {#if instructionOpen}
        <textarea rows="3" maxlength="4000" disabled={locked} placeholder={tr("What should the next agent do?", "O que o próximo agente deve fazer?")} value={connection.additionalInstruction} oninput={(event) => editConnection({ additionalInstruction: event.currentTarget.value })}></textarea>
      {/if}

      <button class:open={previewControlsOpen} class="instruction-toggle" type="button" onclick={() => { previewControlsOpen = !previewControlsOpen; if (previewControlsOpen) instructionOpen = false; }}>
        <svg viewBox="0 0 20 20"><path d="M2.5 10s2.8-5 7.5-5 7.5 5 7.5 5-2.8 5-7.5 5-7.5-5-7.5-5Z" /><circle cx="10" cy="10" r="2" /></svg>
        <span>{tr("Preview", "Prévia")}</span>
        <svg viewBox="0 0 20 20"><path d="m6 8 4 4 4-4" /></svg>
      </button>
      {#if previewControlsOpen}
        <div class="preview-actions">
          <textarea rows="2" maxlength="4000" aria-label={tr("Workflow objective", "Objetivo do workflow")} placeholder={tr("Objective for this run", "Objetivo desta execução")} disabled={locked} value={objective} oninput={(event) => { objectives = { ...objectives, [activeId]: event.currentTarget.value }; invalidatePreview(); persistLayout(); }}></textarea>
          <button class="preview-run" disabled={previewLoading || !objective.trim()} type="button" aria-label={tr("Preview context", "Visualizar contexto")} title={tr("Preview context", "Visualizar contexto")} onclick={() => void togglePreview()}><svg viewBox="0 0 20 20"><path d="M2.5 10s2.8-5 7.5-5 7.5 5 7.5 5-2.8 5-7.5 5-7.5-5-7.5-5Z" /><circle cx="10" cy="10" r="2" /></svg></button>
        </div>
      {/if}

      <footer>
        <button class="remove" type="button" disabled={locked} onclick={removeSelected}>{tr("Remove", "Remover")}</button>
        <span class="connection-help">
          <button class="help-trigger" type="button" aria-label={tr("Explain connection options", "Explicar opções da conexão")}>?</button>
          <span class="help-tooltip" class:inside={helpLayout?.side === "inside"} class:left={helpLayout?.side === "left"} role="tooltip" bind:clientHeight={helpHeight} style:width={helpLayout && helpLayout.side !== "inside" ? helpLayout.width + "px" : null} style:top={helpLayout && helpLayout.side !== "inside" ? helpLayout.top + "px" : null}>
            <strong>{tr("How this connection works", "Como esta conexão funciona")}</strong>
            {#each helpItems as item}
              <span class="help-item"><svg viewBox="0 0 20 20" aria-hidden="true"><path d={item.icon} /></svg><span><b>{tr(item.title[0], item.title[1])}</b>{tr(item.text[0], item.text[1])}</span></span>
            {/each}
          </span>
        </span>
        <span class="autosave">{saving ? tr("Saving…", "Salvando…") : saveFailed ? tr("Not saved", "Não salvo") : tr("Saved", "Salvo")}</span>
      </footer>
    {/if}
  </div>
{/snippet}


<section class="workflow-board" class:entered hidden={!shown} inert={!active} aria-label={tr("Workflow board", "Board de workflow")} data-testid="workflow-board" ondragover={dragOver} ondragleave={(event) => { if (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget)) endDragOver(); }} ondrop={drop}>
  <div
    class="board-viewport"
    class:panning={gesture?.kind === "pan"}
    class:connecting={connectionSource !== null}
    class:drop-active={dropActive && !locked}
    bind:this={viewport}
    use:canvasEvents
    role="region"
    tabindex="-1"
    aria-label={tr("Workflow canvas. Drag the background to pan; scroll to zoom.", "Canvas de workflow. Arraste o fundo para navegar; role para ampliar ou reduzir.")}
    style:background-size={24 * view.zoom + "px " + 24 * view.zoom + "px"}
    style:background-position={view.x + "px " + view.y + "px"}
    onpointerdown={beginPan}
    onpointermove={movePointer}
    onpointerup={endPointer}
    onpointercancel={cancelGesture}
    onlostpointercapture={cancelGesture}
    ondragover={dragOver}
    ondrop={drop}
  >
    <div class="board-world" style:transform={"translate(" + view.x + "px," + view.y + "px) scale(" + view.zoom + ")"}>
      <svg class="board-edges" xmlns="http://www.w3.org/2000/svg" aria-label={tr("Workflow connections", "Conexões do workflow")}>
        <defs>
          <marker id={markerId + "-active"} markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto" markerUnits="userSpaceOnUse"><path d="M1 1 7 4 1 7" fill="none" stroke="var(--workspace-accent)" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" /></marker>
          {#each edges as { connection, geometry } (connection.id)}
            {@const fromRole = group?.steps.find((step) => step.id === connection.fromStepId)?.role ?? "custom"}
            {@const toRole = group?.steps.find((step) => step.id === connection.toStepId)?.role ?? "custom"}
            <linearGradient id={markerId + "-pipe-" + connection.id} gradientUnits="userSpaceOnUse" x1={geometry.start.x} y1={geometry.start.y} x2={geometry.end.x} y2={geometry.end.y}>
              <stop offset="0" style:stop-color={"var(--role-" + fromRole + ")"} />
              <stop offset="1" style:stop-color={"var(--role-" + toRole + ")"} />
            </linearGradient>
            <linearGradient id={markerId + "-core-" + connection.id} gradientUnits="userSpaceOnUse" x1={geometry.start.x} y1={geometry.start.y} x2={geometry.end.x} y2={geometry.end.y}>
              <stop offset="0" style:stop-color={"color-mix(in srgb, var(--role-" + fromRole + ") 72%, #fff)"} />
              <stop offset="1" style:stop-color={"color-mix(in srgb, var(--role-" + toRole + ") 72%, #fff)"} />
            </linearGradient>
            <linearGradient id={markerId + "-head-" + connection.id} x1="0" y1="0" x2="0" y2="1">
              <stop offset="0" style:stop-color={"color-mix(in srgb, var(--role-" + toRole + ") 60%, #fff)"} />
              <stop offset="1" style:stop-color={"color-mix(in srgb, var(--role-" + toRole + ") 72%, #000)"} />
            </linearGradient>
            <marker id={markerId + "-arrow-" + connection.id} markerWidth="16" markerHeight="16" refX="0" refY="8" orient="auto" markerUnits="userSpaceOnUse"><path d="M0 1.2 13 8 0 14.8 3.4 8Z" fill={"url(#" + markerId + "-head-" + connection.id + ")"} stroke={"color-mix(in srgb, var(--role-" + toRole + ") 58%, #000)"} stroke-width=".9" stroke-linejoin="round" /></marker>
          {/each}
        </defs>
        {#each edges as { connection, geometry } (connection.id)}
          {@const chosen = selection?.kind === "connection" && selection.id === connection.id}
          {@const waiting = connectionWaitsForApproval(run, connection.id)}
          {@const live = stepVisualState(run, connection.toStepId) === "running"}
          {@const feeding = !live && stepVisualState(run, connection.fromStepId) === "running"}
          <g class="board-pipe" class:selected={chosen} class:waiting class:live class:feeding class:quiet={Boolean(run) && !finished && !live && stepVisualState(run, connection.fromStepId) !== "running"} data-board-edge={connection.id}>
            <path class="edge-hit" d={geometry.path} role="button" tabindex="0" aria-label={tr("Edit connection", "Editar conexão") + ": " + name(sessionFor(group!.steps.find((step) => step.id === connection.fromStepId)!)) + " → " + name(sessionFor(group!.steps.find((step) => step.id === connection.toStepId)!))} onpointerdown={(event) => { event.stopPropagation(); choose({ kind: "connection", id: connection.id }); }} onkeydown={(event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); choose({ kind: "connection", id: connection.id }); } }} />
            <path class="pipe-shadow" d={geometry.pipe} />
            <path class="pipe-wall" d={geometry.pipe} style:stroke={"url(#" + markerId + "-pipe-" + connection.id + ")"} />
            <path class="pipe-shade" d={geometry.pipe} />
            <path class="pipe-core" d={geometry.pipe} style:stroke={"url(#" + markerId + "-core-" + connection.id + ")"} />
            <path class="pipe-glint" d={geometry.pipe} />
            <path class="pipe-energy-glow" d={geometry.pipe} />
            <path class="pipe-energy" d={geometry.pipe} />
            <path class="pipe-head" d={geometry.pipe} marker-end={"url(#" + markerId + "-arrow-" + connection.id + ")"} />
          </g>
        {/each}
        {#if pendingArrow}<path class="edge-pending" d={pendingArrow.path} marker-end={"url(#" + markerId + "-active)"} />{/if}
      </svg>
      {#each edges as { connection, geometry } (connection.id)}
        {@const waiting = connectionWaitsForApproval(run, connection.id)}
        {@const traveling = stepVisualState(run, connection.toStepId) === "running"}
        {@const cargo = cargoLabels(effectiveContextSelection(connection))}
        <div class="edge-controls" class:cargo-shown={waiting || traveling || edgeMenu?.id === connection.id || (selection?.kind === "connection" && selection.id === connection.id)} style:left={geometry.mid.x + "px"} style:top={geometry.mid.y + "px"}>
          <button class="edge-hub" class:waiting class:open={edgeMenu?.id === connection.id} type="button" aria-haspopup="menu" aria-expanded={edgeMenu?.id === connection.id} title={tr("Connection options", "Opções da conexão")} aria-label={tr("Connection options", "Opções da conexão")} onpointerdown={(event) => event.stopPropagation()} onclick={() => toggleEdgeMenu(connection.id)}>
            {#if connection.requiresApproval}<LumeIcon name="shield" size={13} />
            {:else if connection.advanceMode === "automatic"}<LumeIcon name="bolt" size={13} />
            {:else}<LumeIcon name="arrow-right" size={13} />{/if}
          </button>
          {#if cargo.length}<span class="edge-cargo" class:waiting class:traveling aria-hidden="true">{#each cargo as item (item)}<b>{item}</b>{/each}</span>{/if}
        </div>
      {/each}
      {#each group?.steps ?? [] as step (step.id)}
        {@const position = layout.positions[step.id] ?? { x: 96, y: 120 }}
        {@const session = sessionFor(step)}
        {@const state = stepVisualState(run, step.id)}
        {@const order = chain?.order.indexOf(step.id) ?? -1}
        {@const shown = demoCells ? demoSession(session, demoCells[group?.steps.indexOf(step) ?? 0], now) : session}
        {@const live = stepActivity(shown, run?.steps.find((item) => item.stepId === step.id), language, now)}
        <article class="board-card" class:working={state === "running" || (state === "idle" && shown?.status === "running")} class:selected={selection?.kind === "step" && selection.id === step.id} class:moving={gesture?.kind === "card" && gesture.stepId === step.id} class:leaving={cardOutside && gesture?.kind === "card" && gesture.stepId === step.id} class:problem={problemStepIds.includes(step.id)} class:target-valid={connectionTarget === step.id && targetCheck?.ok} class:target-invalid={connectionTarget === step.id && targetCheck && !targetCheck.ok} data-board-card={step.id} data-attention={live.attention?.kind} data-state={state === "idle" ? session?.status : state} style:left={position.x + "px"} style:top={position.y + "px"} style:width={BOARD_CARD_WIDTH + "px"} style:height={BOARD_CARD_HEIGHT + "px"} style:--card-role={"var(--role-" + step.role + ")"}>
          <button class="card-body" type="button" onpointerdown={(event) => beginCard(event, step.id)} onclick={(event) => { if (event.detail === 0) connectionSource ? connectTo(step.id) : choose({ kind: "step", id: step.id }); }} aria-label={name(session) + " · " + roleName(step.role)}>
            <span class="card-role-fab" title={step.role === "custom" ? step.customRoleLabel || roleName(step.role) : roleName(step.role)} aria-hidden="true">
              <i class="role-symbol"><WorkflowRoleIcon role={step.role} /></i>
              {#if order >= 0}<span class="step-number">{order + 1}</span>{/if}
              <span class="role-tooltip"><strong>{step.role === "custom" ? step.customRoleLabel || roleName(step.role) : roleName(step.role)}</strong>{#if order >= 0}<small>{tr("Step ", "Etapa ") + (order + 1) + tr(" of ", " de ") + (chain?.order.length ?? 0)}</small>{/if}</span>
            </span>
            <span class="card-avatar"><ThreadAvatar seed={session?.nativeSessionId || step.sessionNativeId} label={name(session)} size={44} />{#if session}<span class="agent-badge" title={session.agentLabel}><BrandIcon name={session.agent} size={12} /></span>{/if}</span>
            <span class="card-copy"><strong>{name(session)}</strong><small>{session?.agentLabel ?? tr("Disconnected", "Desconectado")} · {session?.project || tr("No project", "Sem projeto")}</small></span>
            <span class="card-status"><i></i>{stepStatus(step) + (live.elapsed ? " · " + live.elapsed : "") + (live.progress ? " · " + live.progress.done + "/" + live.progress.total : "")}</span>
            {#if live.line}<span class="card-activity" class:live={state === "running" || shown?.status === "running"}><i></i><span>{live.line}</span></span>{/if}
          </button>
          {#if live.progress}<span class="card-progress" aria-hidden="true"><i style:width={(live.progress.done / live.progress.total) * 100 + "%"}></i></span>{/if}
          {#if live.attention && session}
            <button class="card-attention" type="button" title={live.attention.text} aria-label={tr("Needs you: ", "Precisa de você: ") + live.attention.text} onpointerdown={(event) => event.stopPropagation()} onclick={() => onOpenChat(session)}><LumeIcon name="warning" size={11} />{live.attention.kind === "permission" ? tr("Approve", "Aprovar") : tr("Answer", "Responder")}</button>
          {/if}
          <button class="card-port port-in" type="button" disabled={locked} aria-label={tr("Connect to ", "Conectar a ") + name(session)} title={tr("Connection input", "Entrada da conexão")} onclick={() => connectionSource ? connectTo(step.id) : choose({ kind: "step", id: step.id })}><i></i></button>
          <button class="card-port port-out" type="button" disabled={locked} aria-label={tr("Connect from ", "Conectar de ") + name(session)} title={tr("Drag to connect", "Arraste para conectar")} onpointerdown={(event) => beginConnection(event, step.id)} onclick={(event) => { if (event.detail === 0) keyboardConnect(step.id); }}><i></i></button>
        </article>
      {/each}
      {#if dragSession && dragPoint}
        <div class="board-card ghost" class:invalid={dragIssue !== null} style:left={Math.round(dragPoint.x - BOARD_CARD_WIDTH / 2) + "px"} style:top={Math.round(dragPoint.y - BOARD_CARD_HEIGHT / 2) + "px"} style:width={BOARD_CARD_WIDTH + "px"} style:height={BOARD_CARD_HEIGHT + "px"} style:--card-role={"var(--role-" + dragRole + ")"} aria-hidden="true">
          <div class="card-body">
            <span class="card-role-fab"><i class="role-symbol"><WorkflowRoleIcon role={dragRole} /></i></span>
            <span class="card-avatar"><ThreadAvatar seed={dragSession.nativeSessionId || dragSession.id} label={name(dragSession)} size={44} /><span class="agent-badge"><BrandIcon name={dragSession.agent} size={12} /></span></span>
            <span class="card-copy"><strong>{name(dragSession)}</strong><small>{dragSession.agentLabel} · {dragSession.project || tr("No project", "Sem projeto")}</small></span>
            <span class="card-status"><i></i>{dragIssue === "present" ? tr("Already in this workflow", "Já está neste workflow") : dragIssue === "unconnected" ? tr("Connect first", "Conecte primeiro") : dragIssue === "locked" ? tr("Workflow is running", "Workflow em execução") : tr("Release to place", "Solte para posicionar")}</span>
          </div>
        </div>
      {/if}
    </div>
  </div>

  <header class="board-toolbar">
    <span class="board-title"><WorkspaceHeaderIcon name="workflow" size={19} /><strong>Workflow</strong></span>
    {#if groups.length}<LumeSelect value={activeId} options={groups.map((item, index) => ({ value: item.id, label: groupName(item, index) }))} ariaLabel={tr("Select workflow", "Selecionar workflow")} minWidth={132} variant="heading" onValueChange={activate} />{/if}
    <span class="toolbar-divider"></span>
    <button type="button" title={tr("New workflow", "Novo workflow")} aria-label={tr("New workflow", "Novo workflow")} onclick={createGroup}><LumeIcon name="plus" size={16} /></button>
    <button class:confirm={confirmingDelete} class="delete-workflow" type="button" disabled={!group || locked || deleting} title={confirmingDelete ? tr("Click again to delete this workflow", "Clique de novo para excluir este workflow") : tr("Delete workflow", "Excluir workflow")} aria-label={confirmingDelete ? tr("Confirm deleting this workflow", "Confirmar exclusão deste workflow") : tr("Delete workflow", "Excluir workflow")} onclick={requestDeleteGroup}><LumeIcon name="trash" size={15} />{#if confirmingDelete}<span>{tr("Delete?", "Excluir?")}</span>{/if}</button>
    {#if simulatorAvailable}<button class="sim-toggle" class:active={Boolean(demo)} type="button" title={tr("Preview card and pipe states", "Visualizar estados dos cards e canos")} aria-pressed={Boolean(demo)} onclick={toggleDemo}><LumeIcon name="bolt" size={14} />{tr("Simulate", "Simular")}</button>{/if}
    <button class="add-agent" type="button" disabled={locked || adding} onclick={() => openPicker()}><LumeIcon name="plus" size={14} />{tr("Agent", "Agente")}</button>
    <span class="save-state" aria-live="polite">{saving ? tr("Saving…", "Salvando…") : saveFailed ? tr("Not saved", "Não salvo") : ""}</span>
    {#if saveFailed}<button type="button" title={tr("Retry saving", "Tentar salvar novamente")} aria-label={tr("Retry saving", "Tentar salvar novamente")} onclick={() => void retrySave()}><LumeIcon name="refresh" size={15} /></button>{/if}
    <button class="close-board" type="button" title={tr("Back to chats", "Voltar aos chats")} aria-label={tr("Back to chats", "Voltar aos chats")} onclick={onClose}><LumeIcon name="close" size={17} /></button>
  </header>

  {#if !group?.steps.length && !pickerOpen}
    <div class="board-empty">
      <div class="empty-card">
        <WorkspaceHeaderIcon name="workflow" size={42} />
        <strong>{tr("Give your agents a direction", "Dê uma direção aos seus agentes")}</strong>
        <p>{tr("Add agents and draw connections to build your workflow.", "Adicione agentes e desenhe conexões para montar seu workflow.")}</p>
        <button type="button" onclick={() => openPicker()}><LumeIcon name="plus" size={16} />{tr("Add first agent", "Adicionar primeiro agente")}</button>
        <small>{tr("You can also drag an agent here from the sidebar.", "Você também pode arrastar um agente da sidebar para cá.")}</small>
      </div>
    </div>
  {/if}

  {#if pickerOpen || selectedStep}
    <aside class="board-panel" aria-label={pickerOpen ? tr("Add agent", "Adicionar agente") : tr("Step settings", "Configurações da etapa")} in:fly={{ x: 14, duration: duration(180), easing: cubicOut }} out:fade={{ duration: duration(90) }}>
      <header><strong>{pickerOpen ? tr("Add an agent", "Adicionar um agente") : tr("Step", "Etapa")}</strong><button type="button" aria-label={tr("Close panel", "Fechar painel")} onclick={() => { choose(null); pickerOpen = false; }}><LumeIcon name="close" size={16} /></button></header>
      <div class="panel-scroll">
        {#if pickerOpen}
          <p class="panel-help">{tr("Choose an open conversation or drag it from the sidebar.", "Escolha uma conversa aberta ou arraste-a da sidebar.")}</p>
          {@render agentChoices()}
        {:else if selectedStep}
          {@const session = sessionFor(selectedStep)}
          {@const shown = demoCells ? demoSession(session, demoCells[group?.steps.indexOf(selectedStep) ?? 0], now) : session}
          {@const feed = activityFeed(shown, language)}
          {@const answer = lastResponse(shown)}
          {@const stepRun = run?.steps.find((item) => item.stepId === selectedStep.id)}
          {@const info = stepActivity(shown, stepRun, language, now)}
          <div class="selected-identity"><ThreadAvatar seed={selectedStep.sessionNativeId} label={name(session)} size={36} /><span><strong>{name(session)}</strong><small>{session?.agentLabel} · {session?.project}</small></span></div>
          {#if !session || !session.capabilities.canPrompt}<p class="session-warning"><LumeIcon name="warning" size={15} />{session?.controlOrigin === "external" ? tr("Take control in the chat before running this workflow.", "Assuma o controle no chat antes de executar este workflow.") : tr("Connect this session before running the workflow.", "Conecte esta sessão antes de executar o workflow.")}</p>{/if}
          <div class="panel-tabs" role="tablist" aria-label={tr("Step", "Etapa")}>
            <button type="button" role="tab" aria-selected={panelTab === "settings"} class:active={panelTab === "settings"} onclick={() => (panelTab = "settings")}>{tr("Settings", "Ajustes")}</button>
            <button type="button" role="tab" aria-selected={panelTab === "activity"} class:active={panelTab === "activity"} onclick={() => (panelTab = "activity")}>{tr("Activity", "Atividade")}{#if feed.length || info.attention}<i></i>{/if}</button>
          </div>
          {#if panelTab === "activity"}
            <div class="live-summary" data-state={stepVisualState(run, selectedStep.id)}>
              <span class="live-state"><i></i>{stepStatus(selectedStep) + (info.elapsed ? " · " + info.elapsed : "")}</span>
              {#if info.attention}<button type="button" class="live-attention" onclick={() => { if (session) onOpenChat(session); }}><LumeIcon name="warning" size={12} />{info.attention.kind === "permission" ? tr("Approve in the chat", "Aprovar no chat") : tr("Answer in the chat", "Responder no chat")}</button><p class="live-ask">{info.attention.text}</p>
              {:else if info.line}<p class="live-now">{info.line}</p>{/if}
            </div>
            {#if info.progress}
              <div class="live-plan">
                <span class="live-plan-head"><strong>{tr("Plan", "Plano")}</strong><small>{info.progress.done}/{info.progress.total}</small></span>
                <span class="card-progress live-bar" aria-hidden="true" style:--card-role={"var(--role-" + selectedStep.role + ")"}><i style:width={(info.progress.done / info.progress.total) * 100 + "%"}></i></span>
                <ul>{#each (shown?.workSummary.todo?.items ?? shown?.workSummary.plan?.items ?? []) as item (item.label)}<li data-status={item.status}><i></i><span>{item.label}</span></li>{/each}</ul>
              </div>
            {/if}
            {#if answer}<div class="live-answer"><strong>{tr("Latest answer", "Última resposta")}</strong><p>{answer}</p></div>{/if}
            <div class="live-feed">
              <strong>{tr("Recent work", "Trabalho recente")}</strong>
              {#if feed.length}
                <ol>{#each feed as item (item.id)}<li data-status={item.status} data-category={item.category}><i></i><span><b>{item.title}</b>{#if item.detail}<small>{item.detail}</small>{/if}</span><time>{formatAgentDuration(Math.max(0, now - item.at))}</time></li>{/each}</ol>
              {:else}<p class="panel-help">{tr("No activity yet. It appears here as the agent works.", "Nenhuma atividade ainda. Ela aparece aqui conforme o agente trabalha.")}</p>{/if}
            </div>
            <div class="panel-actions"><button type="button" disabled={!session} onclick={() => { if (session) onOpenChat(session); }}><LumeIcon name="external" size={14} />{tr("Open chat", "Abrir chat")}</button></div>
          {:else}
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
          {/if}
        {/if}
      </div>
    </aside>
  {/if}

  {#if edgeMenu && edgeAnchor}
    {@const connection = group?.connections.find((item) => item.id === edgeMenu!.id)}
    {#if connection}
      <div class="edge-popover" tabindex="-1" class:above={!edgeAnchor.placeBelow} class:detached={!edgeAnchor.attached} class:compact={edgeAnchor.compact} bind:clientHeight={popoverHeight} role="dialog" aria-label={edgeMenu.mode === "settings" ? tr("Connection settings", "Configurações da conexão") : edgeMenu.mode === "insert" ? tr("Add an intermediate agent", "Adicionar agente intermediário") : tr("Connection options", "Opções da conexão")} style:left={edgeAnchor.left + "px"} style:width={edgeAnchor.width + "px"} style:top={edgeAnchor.top + "px"} style:max-height={edgeAnchor.maxHeight + "px"} style:--caret={edgeAnchor.caret + "px"} in:fly={{ y: edgeAnchor.placeBelow ? -6 : 6, duration: duration(150), easing: cubicOut }} out:fade={{ duration: duration(80) }} onpointerdown={(event) => event.stopPropagation()}>
        {#if edgeMenu.mode === "menu"}
          <div class="edge-menu" role="menu">
            <button type="button" role="menuitem" data-edge-action="settings" onclick={() => openEdgeMenu(connection.id, "settings")}>
              <span class="edge-menu-icon"><LumeIcon name="settings" size={15} /></span>
              <span><strong>{tr("Connection settings", "Configurações da conexão")}</strong><small>{tr("Context, approval and instructions", "Contexto, aprovação e instruções")}</small></span>
            </button>
            <button type="button" role="menuitem" data-edge-action="insert" disabled={locked} onclick={() => openEdgeMenu(connection.id, "insert")}>
              <span class="edge-menu-icon"><LumeIcon name="plus" size={15} /></span>
              <span><strong>{tr("Add an intermediate agent", "Adicionar agente intermediário")}</strong><small>{tr("Placed between these two steps", "Entra entre estas duas etapas")}</small></span>
            </button>
          </div>
        {:else if edgeMenu.mode === "settings"}
          {@render connectionSettings(connection)}
        {:else}
          <header>
            <button type="button" aria-label={tr("Back", "Voltar")} onclick={() => openEdgeMenu(connection.id)}><LumeIcon name="arrow-right" size={14} /></button>
            <strong>{tr("Add an intermediate agent", "Adicionar agente intermediário")}</strong>
            <button type="button" aria-label={tr("Close", "Fechar")} onclick={() => choose(null)}><LumeIcon name="close" size={15} /></button>
          </header>
          <div class="panel-scroll">
            <p class="panel-help">{tr("The selected agent will be inserted between these two steps.", "O agente escolhido entrará entre estas duas etapas.")}</p>
            {@render agentChoices()}
          </div>
        {/if}
      </div>
    {/if}
  {/if}

  <div class="board-navigation" role="group" aria-label={tr("Canvas view", "Vista do canvas")}>
    <button type="button" aria-label={tr("Zoom out", "Diminuir zoom")} title={tr("Zoom out", "Diminuir zoom")} onclick={() => zoom(1 / 1.2)}><span aria-hidden="true">−</span></button>
    <button class="zoom-label" type="button" title={tr("Reset zoom", "Restaurar zoom")} aria-label={tr("Reset zoom", "Restaurar zoom")} onclick={() => patchLayout({ view: zoomAround(view, { x: viewportWidth / 2, y: viewportHeight / 2 }, 1) })}>{Math.round(view.zoom * 100)}%</button>
    <button type="button" aria-label={tr("Zoom in", "Aumentar zoom")} title={tr("Zoom in", "Aumentar zoom")} onclick={() => zoom(1.2)}><LumeIcon name="plus" size={14} /></button>
    <span></span><button type="button" aria-label={tr("Fit to view", "Ajustar à tela")} title={tr("Fit to view", "Ajustar à tela")} onclick={fit}><LumeIcon name="maximize" size={15} /></button>
  </div>
  {#if connectionSource}<p class="connection-hint">{targetCheck && !targetCheck.ok ? connectionRefusalMessage(targetCheck.reason, language === "pt-BR") : tr("Choose another card to connect · Esc to cancel", "Escolha outro card para conectar · Esc para cancelar")}</p>{/if}

  {#if demo && group && demoCells}
    <aside class="board-sim" aria-label={tr("State simulation", "Simulação de estados")}>
      <header>
        <strong>{tr("Simulation", "Simulação")}</strong>
        <small>{demo.manual ? tr("Manual", "Manual") : demo.frame + 1 + "/" + demoScript.length}</small>
        <span class="sim-actions">
          <button type="button" title={tr("Back to the start", "Voltar ao início")} aria-label={tr("Back to the start", "Voltar ao início")} onclick={() => { demo = { ...demo!, started: {} }; applyFrame(0); }}><LumeIcon name="refresh" size={13} /></button>
          <button type="button" class:on={demo.playing} onclick={() => { if (demo!.manual) applyFrame(0); demo = { ...demo!, playing: !demo!.playing }; }}>{demo.playing ? tr("Pause", "Pausar") : tr("Play", "Executar")}</button>
          <button type="button" title={tr("Next state", "Próximo estado")} aria-label={tr("Next state", "Próximo estado")} onclick={() => { demo = { ...demo!, playing: false }; applyFrame(demo!.manual ? 0 : demo!.frame + 1 >= demoScript.length ? 0 : demo!.frame + 1); }}><LumeIcon name="arrow-right" size={13} /></button>
          <button type="button" title={tr("Close the simulation", "Fechar a simulação")} aria-label={tr("Close the simulation", "Fechar a simulação")} onclick={() => (demo = null)}><LumeIcon name="close" size={13} /></button>
        </span>
      </header>
      <div class="sim-steps">
        {#each group.steps as step, index (step.id)}
          {@const cell = demoCells[index]}
          <div class="sim-step">
            <strong>{name(sessionFor(step))}</strong>
            <select aria-label={tr("State", "Estado")} value={cell.state} onchange={(event) => editDemo(index, { state: event.currentTarget.value as DemoState["state"], progress: event.currentTarget.value === "completed" ? demoTodoCount : cell.progress })}>
              <option value="pending">{tr("Pending", "Pendente")}</option>
              <option value="running">{tr("Running", "Executando")}</option>
              <option value="completed">{tr("Completed", "Concluído")}</option>
              <option value="failed">{tr("Failed", "Falhou")}</option>
            </select>
            <select aria-label={tr("Needs attention", "Precisa de atenção")} value={cell.attention} onchange={(event) => editDemo(index, { attention: event.currentTarget.value as DemoState["attention"] })}>
              <option value="none">{tr("No request", "Sem pedido")}</option>
              <option value="permission">{tr("Permission", "Permissão")}</option>
              <option value="question">{tr("Question", "Pergunta")}</option>
            </select>
            <input type="range" min="0" max={demoTodoCount} step="1" value={cell.progress} aria-label={tr("Plan progress", "Progresso do plano")} oninput={(event) => editDemo(index, { progress: Number(event.currentTarget.value) })} />
          </div>
        {/each}
      </div>
    </aside>
  {:else if group}
    <footer class="board-run" data-status={run?.status ?? "draft"}>
      <span class="run-track" aria-hidden="true"><i style:--progress={(run ? run.steps.filter((step) => ["completed", "skipped"].includes(step.status)).length / Math.max(1, group.steps.length) * 100 : 0) + "%"}></i></span>
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
  .workflow-board { position: absolute; z-index: 30; inset: 0; overflow: hidden; opacity: 0; transform: scale(.985); transition: opacity 220ms ease, transform 280ms cubic-bezier(.16, 1, .3, 1); color: var(--workspace-text); background: var(--workspace-bg); isolation: isolate; --board-danger: #a83e3a; --board-warning: #91640f; --select-surface: var(--workspace-raised); --select-text: var(--workspace-strong); --select-muted: var(--workspace-muted); --select-line: var(--workspace-line); --select-accent: var(--workspace-accent); --select-hover: var(--workspace-subtle); --select-active: var(--workspace-accent-soft);
    --role-planner: #4f86bd; --role-implementer: #3d9a6c; --role-reviewer: #c28a3c; --role-tester: #8460bd; --role-researcher: #3c97a6; --role-custom: #7b8a83;
    --hud-surface: var(--workspace-raised); --hud-shadow: 0 10px 28px rgba(20, 38, 30, .12), 0 1px 3px rgba(20, 38, 30, .1); }
  :global(.workspace.dark) .workflow-board { --board-danger: #ec9894; --board-warning: #e0b75b; --role-planner: #6aa0d6; --role-implementer: #56bd8a; --role-reviewer: #dba653; --role-tester: #a583d8; --role-researcher: #58b6c5; --role-custom: #93a39b; --hud-shadow: 0 12px 30px rgba(0, 0, 0, .38), 0 1px 3px rgba(0, 0, 0, .4); }
  .workflow-board[hidden] { display: none; }
  .board-viewport { position: absolute; inset: 0; overflow: hidden; outline: none; touch-action: none; background-image: radial-gradient(circle, color-mix(in srgb, var(--workspace-muted) 28%, transparent) 1px, transparent 1px); cursor: grab; }
  .board-viewport.panning { cursor: grabbing; }
  .board-viewport.connecting { cursor: crosshair; }
  .board-viewport.drop-active { box-shadow: inset 0 0 0 2px var(--workspace-accent), inset 0 0 60px color-mix(in srgb, var(--workspace-accent) 14%, transparent); }
  .board-world { position: absolute; inset: 0 auto auto 0; transform-origin: 0 0; width: 0; height: 0; }
  .board-edges { position: absolute; width: 1px; height: 1px; overflow: visible; pointer-events: none; }
  .edge-hit { fill: none; stroke: transparent; stroke-width: 22; pointer-events: stroke; cursor: pointer; }
  .board-pipe :where(path:not(.edge-hit)) { fill: none; pointer-events: none; stroke-linecap: butt; stroke-linejoin: round; }
  .pipe-shadow { stroke: rgba(10, 24, 18, .2); stroke-width: 11; transform: translate(1.5px, 3.5px); }
  .pipe-wall { stroke-width: 10; transition: stroke-width 160ms ease; }
  .pipe-shade { stroke: rgba(8, 18, 14, .34); stroke-width: 10; }
  .pipe-core { stroke-width: 6.4; }
  .pipe-glint { stroke: rgba(255, 255, 255, .6); stroke-width: 1.7; stroke-linecap: round; transform: translate(-1.2px, -1.5px); }
  .pipe-energy { stroke: #fff; stroke-width: 3; stroke-linecap: round; stroke-dasharray: 5 21; stroke-dashoffset: 0; opacity: .9; animation: pipe-flow 2.6s linear infinite; }
  .pipe-energy-glow { stroke: rgba(255, 255, 255, .32); stroke-width: 8; stroke-linecap: round; stroke-dasharray: 5 21; animation: pipe-flow 2.6s linear infinite; }
  .live .pipe-energy, .live .pipe-energy-glow { stroke-dasharray: 7 19; animation-duration: .85s; }
  .live .pipe-energy { opacity: 1; stroke-width: 3.4; }
  .live .pipe-energy-glow { stroke: rgba(255, 255, 255, .45); stroke-width: 12; }
  .live .pipe-core { animation: pipe-breathe 1.6s ease-in-out infinite; }
  .quiet .pipe-energy, .quiet .pipe-energy-glow { opacity: .22; animation-duration: 6s; }
  @keyframes pipe-breathe { 50% { stroke-width: 7.6; } }
  .selected .pipe-wall, .selected .pipe-shade { stroke-width: 12; }
  .selected .pipe-core { stroke-width: 8; }
  .selected .pipe-glint { stroke-width: 2; }
  .waiting .pipe-wall, .waiting .pipe-core { stroke-dasharray: 11 6; }
  .waiting .pipe-energy, .waiting .pipe-energy-glow { animation-play-state: paused; opacity: .35; }
  .board-pipe:hover .pipe-wall { stroke-width: 11; }
  @keyframes pipe-flow { to { stroke-dashoffset: -26; } }
  .edge-pending { fill: none; stroke: var(--workspace-accent); stroke-width: 2.2; stroke-linecap: round; stroke-dasharray: 2 7; }
  .edge-controls { position: absolute; z-index: 2; display: flex; transform: translate(-50%, -50%); }
  button { font: inherit; cursor: pointer; }
  button:disabled { opacity: .45; cursor: default; }
  button:focus-visible, input:focus-visible, textarea:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 3px; }
  .edge-hub { width: 28px; height: 28px; padding: 0; display: grid; place-items: center; border: 1px solid var(--workspace-line); border-radius: 50%; color: var(--workspace-muted); background: radial-gradient(circle at 34% 24%, rgba(255, 255, 255, .5), transparent 55%), var(--workspace-raised); box-shadow: 0 3px 9px rgba(20, 38, 30, .22), 0 0 0 3px color-mix(in srgb, var(--workspace-raised) 70%, transparent); transition: color 120ms ease, border-color 120ms ease, transform 160ms cubic-bezier(.2, .8, .2, 1), box-shadow 160ms ease; }
  .edge-hub:hover, .edge-hub.open { color: var(--workspace-accent); border-color: var(--workspace-accent); transform: scale(1.12); }
  .edge-hub.open { box-shadow: 0 4px 12px rgba(20, 38, 30, .26), 0 0 0 4px color-mix(in srgb, var(--workspace-accent) 20%, transparent); }
  .edge-cargo { position: absolute; top: calc(100% + 7px); left: 50%; display: flex; gap: 3px; padding: 3px; border: 1px solid var(--workspace-line); border-radius: 999px; background: color-mix(in srgb, var(--workspace-raised) 92%, transparent); box-shadow: 0 3px 9px rgba(20, 38, 30, .18); opacity: 0; transform: translate(-50%, -3px); transition: opacity 140ms ease, transform 140ms ease; pointer-events: none; white-space: nowrap; }
  .edge-cargo b { padding: 1px 7px; border-radius: 999px; color: var(--workspace-muted); background: var(--workspace-subtle); font-size: 9.5px; font-weight: 650; }
  .edge-cargo.traveling b { color: var(--workspace-accent); }
  .edge-cargo.waiting b { color: var(--board-warning); }
  .edge-controls:hover .edge-cargo, .edge-controls.cargo-shown .edge-cargo { opacity: 1; transform: translate(-50%, 0); }
  .feeding .pipe-energy-glow { opacity: .35; }
  .feeding .pipe-core { animation: pipe-breathe 3.2s ease-in-out infinite; }
  .edge-hub.waiting { color: var(--board-warning); border-color: var(--board-warning); }
  .edge-popover { position: absolute; z-index: 7; box-sizing: border-box; display: flex; flex-direction: column; min-height: 0; border: 1px solid var(--workspace-line); border-radius: 16px; background: var(--workspace-raised); box-shadow: var(--hud-shadow), 0 18px 40px rgba(20, 38, 30, .16); outline: none; }
  .edge-popover::before { content: ""; position: absolute; top: -6px; left: calc(var(--caret) - 6px); width: 10px; height: 10px; border: 1px solid var(--workspace-line); border-right: 0; border-bottom: 0; background: var(--workspace-raised); transform: rotate(45deg); }
  .edge-popover.detached::before { display: none; }
  .edge-popover.above::before { top: auto; bottom: -6px; transform: rotate(225deg); }
  .edge-popover > header { flex: 0 0 auto; border-radius: 15px 15px 0 0; background: var(--workspace-raised); display: flex; align-items: center; gap: 4px; min-height: 40px; padding: 4px 8px 4px 6px; border-bottom: 1px solid var(--workspace-line); }
  .edge-popover > header strong { flex: 1; min-width: 0; padding: 0 4px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--workspace-strong); font-size: 12px; font-weight: 650; }
  .edge-popover > header button { width: 28px; height: 28px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 7px; color: var(--workspace-muted); background: transparent; }
  .edge-popover > header button:hover { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .edge-popover > header button:first-child :global(svg) { transform: scaleX(-1); }
  .edge-popover .panel-scroll { flex: 1 1 auto; border-radius: 0 0 15px 15px; }
  .edge-menu { border-radius: 15px; }
  .edge-menu { padding: 6px; display: grid; gap: 2px; }
  .edge-menu button { width: 100%; padding: 9px 10px; display: flex; align-items: center; gap: 11px; border: 0; border-radius: 11px; color: var(--workspace-text); background: transparent; text-align: left; }
  .edge-menu button:hover:not(:disabled), .edge-menu button:focus-visible { background: var(--workspace-subtle); }
  .edge-menu button > span:last-child { min-width: 0; display: grid; gap: 3px; }
  .edge-menu strong { color: var(--workspace-strong); font-size: 12px; font-weight: 650; }
  .edge-menu small { color: var(--workspace-muted); font-size: 10px; line-height: 1.35; }
  .edge-menu-icon { width: 30px; height: 30px; flex: 0 0 auto; display: grid; place-items: center; border-radius: 9px; color: var(--workspace-accent); background: var(--workspace-accent-soft); }
  .board-card { --card-role: var(--role-custom); position: absolute; box-sizing: border-box; border: 1px solid color-mix(in srgb, var(--card-role) 30%, var(--workspace-line)); border-radius: 18px; background: linear-gradient(180deg, color-mix(in srgb, var(--card-role) 7%, var(--workspace-raised)), var(--workspace-raised) 62%); box-shadow: 0 10px 24px rgba(20, 38, 30, .1), 0 1px 3px rgba(20, 38, 30, .1), inset 0 1px 0 rgba(255, 255, 255, .5); transition: border-color 140ms ease, box-shadow 180ms ease, transform 180ms cubic-bezier(.2, .8, .2, 1); }
  :global(.workspace.dark) .board-card { box-shadow: 0 12px 26px rgba(0, 0, 0, .34), 0 1px 3px rgba(0, 0, 0, .4), inset 0 1px 0 rgba(255, 255, 255, .06); }
  .board-card:hover { box-shadow: 0 14px 30px rgba(20, 38, 30, .15), 0 0 0 3px color-mix(in srgb, var(--card-role) 12%, transparent); }
  .board-card.selected, .board-card.target-valid { border-color: var(--card-role); box-shadow: 0 14px 30px rgba(20, 38, 30, .16), 0 0 0 3px color-mix(in srgb, var(--card-role) 22%, transparent); }
  .board-card.target-valid { border-color: var(--workspace-accent); }
  .board-card.problem, .board-card.target-invalid { border-color: var(--board-danger); box-shadow: 0 0 0 3px color-mix(in srgb, var(--board-danger) 18%, transparent); }
  .board-card.working:not(.moving):not(.leaving) { animation: card-sway 3.8s ease-in-out infinite; }
  .board-card.working:not(.moving):not(.leaving) .card-role-fab { box-shadow: inset 0 1px 0 rgba(255, 255, 255, .95), inset 0 -3px 5px color-mix(in srgb, var(--card-role) 22%, transparent), 0 5px 10px rgba(24, 58, 41, .2), 0 0 0 5px color-mix(in srgb, var(--card-role) 20%, transparent); }
  @keyframes card-sway { 0%, 100% { translate: 0 0; rotate: -.55deg; } 50% { translate: 0 -3px; rotate: .55deg; } }
  .board-card.moving { z-index: 3; cursor: grabbing; transform: translateY(-3px) scale(1.015); box-shadow: 0 22px 40px rgba(20, 38, 30, .22), 0 0 0 3px color-mix(in srgb, var(--card-role) 20%, transparent); }
  .card-body { position: relative; width: 100%; height: 100%; padding: 26px 14px 12px; box-sizing: border-box; display: flex; flex-direction: column; align-items: center; justify-content: flex-start; gap: 6px; border: 0; border-radius: inherit; color: inherit; background: transparent; text-align: center; user-select: none; cursor: grab; touch-action: none; }
  .moving .card-body { cursor: grabbing; }
  .card-role-fab { position: absolute; z-index: 2; top: -18px; left: 50%; width: 36px; height: 36px; display: grid; place-items: center; border: 1px solid color-mix(in srgb, var(--card-role) 42%, transparent); border-radius: 50%; color: var(--card-role); background: radial-gradient(circle at 34% 24%, rgba(255, 255, 255, .96) 0 13%, rgba(255, 255, 255, .34) 31%, transparent 52%), linear-gradient(145deg, #fbfdfc 12%, color-mix(in srgb, var(--card-role) 14%, #e7eeea) 88%); box-shadow: inset 0 1px 0 rgba(255, 255, 255, .95), inset 0 -3px 5px color-mix(in srgb, var(--card-role) 22%, transparent), 0 5px 10px rgba(24, 58, 41, .2), 0 1px 2px rgba(24, 58, 41, .16), 0 0 0 3px color-mix(in srgb, var(--card-role) 10%, transparent); transform: translateX(-50%); transition: box-shadow 170ms ease, transform 170ms cubic-bezier(.2, .8, .2, 1); }
  :global(.workspace.dark) .card-role-fab { background: radial-gradient(circle at 34% 24%, rgba(255, 255, 255, .22) 0 13%, transparent 46%), linear-gradient(145deg, color-mix(in srgb, var(--card-role) 20%, #2c3631), #1e2623 88%); box-shadow: inset 0 1px 0 rgba(255, 255, 255, .16), inset 0 -3px 5px rgba(0, 0, 0, .3), 0 5px 12px rgba(0, 0, 0, .45), 0 0 0 3px color-mix(in srgb, var(--card-role) 16%, transparent); }
  .board-card:hover .card-role-fab, .board-card.selected .card-role-fab { transform: translateX(-50%) translateY(-2px); }
  .role-symbol { width: 30px; height: 30px; display: grid; place-items: center; border-radius: 50%; transform: scale(.9); font-style: normal; }
  .step-number { position: absolute; top: -3px; right: -5px; min-width: 14px; height: 14px; padding: 0 3px; box-sizing: border-box; display: grid; place-items: center; border: 1px solid rgba(255, 255, 255, .86); border-radius: 999px; color: #fff; background: var(--card-role); box-shadow: 0 2px 5px rgba(20, 40, 30, .3); font: 800 8px/1 var(--lume-font-ui, Inter, sans-serif); font-variant-numeric: tabular-nums; }
  :global(.workspace.dark) .step-number { border-color: rgba(24, 35, 29, .9); }
  .role-tooltip { position: absolute; bottom: calc(100% + 7px); left: 50%; min-width: max-content; padding: 5px 9px; display: grid; gap: 1px; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-strong); background: var(--workspace-raised); box-shadow: var(--hud-shadow); opacity: 0; pointer-events: none; transform: translate(-50%, 4px) scale(.96); transition: opacity 130ms ease, transform 150ms cubic-bezier(.2, .8, .2, 1); }
  .role-tooltip strong { font-size: 10px; font-weight: 700; }
  .role-tooltip small { color: var(--workspace-muted); font-size: 9px; }
  .board-card:hover .role-tooltip { opacity: 1; transform: translate(-50%, 0) scale(1); }
  .board-card.moving .role-tooltip { opacity: 0; }
  .card-body > * { flex-shrink: 0; }
  .card-avatar { display: grid; }
  .card-avatar { position: relative; }
  .agent-badge { position: absolute; right: -7px; bottom: -5px; width: 20px; height: 20px; display: grid; place-items: center; border: 1px solid var(--workspace-line); border-radius: 50%; background: var(--workspace-raised); box-shadow: 0 2px 5px rgba(20, 38, 30, .22); }
  .board-card.ghost { z-index: 4; pointer-events: none; border-style: dashed; border-color: var(--card-role); background: color-mix(in srgb, var(--card-role) 9%, var(--workspace-raised)); opacity: .92; transform: scale(1.03); box-shadow: 0 20px 40px rgba(20, 38, 30, .24), 0 0 0 4px color-mix(in srgb, var(--card-role) 18%, transparent); animation: ghost-in 150ms cubic-bezier(.2, .8, .2, 1) both; transition: none; }
  .board-card.ghost.invalid { border-color: var(--board-danger); opacity: .6; }
  .board-card.ghost .card-body { cursor: copy; }
  @keyframes ghost-in { from { opacity: 0; transform: scale(.9); } }
  .card-copy { width: 100%; min-width: 0; display: grid; gap: 4px; }
  .card-copy strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--workspace-strong); font-size: 13px; font-weight: 650; letter-spacing: -.01em; }
  .card-copy small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--workspace-muted); font-size: 10px; }
  .card-status { max-width: 100%; margin-top: auto; flex-shrink: 1; padding: 3px 9px; box-sizing: border-box; display: inline-flex; align-items: center; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; border-radius: 999px; color: var(--workspace-muted); background: color-mix(in srgb, var(--workspace-muted) 9%, transparent); font-size: 9.5px; }
  .card-status i, .run-state i { width: 6px; height: 6px; margin-right: 5px; flex: 0 0 auto; display: inline-block; border-radius: 50%; background: var(--workspace-muted); }
  [data-state="running"] .card-status i, [data-status="running"] .run-state i { background: #56aada; box-shadow: 0 0 0 3px rgba(86, 170, 218, .22); animation: status-pulse 1.4s ease-in-out infinite; }
  [data-state="completed"] .card-status i, [data-status="completed"] .run-state i { background: var(--workspace-accent); }
  [data-state="failed"] .card-status i, [data-status="failed"] .run-state i { background: var(--board-danger); }
  [data-state="permission_required"] .card-status i, [data-status="waiting_for_approval"] .run-state i, [data-status="paused"] .run-state i { background: var(--board-warning); }
  @keyframes status-pulse { 50% { box-shadow: 0 0 0 5px rgba(86, 170, 218, 0); } }
  .card-activity { max-width: 100%; margin-top: -2px; display: inline-flex; align-items: center; gap: 6px; color: var(--workspace-muted); font-size: 10px; line-height: 1.3; }
  .card-activity > span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .card-activity i { width: 5px; height: 5px; flex: 0 0 auto; border-radius: 50%; background: color-mix(in srgb, var(--workspace-muted) 55%, transparent); }
  .card-activity.live { color: var(--workspace-text); }
  .card-activity.live i { background: #56aada; animation: status-pulse 1.4s ease-in-out infinite; }
  .card-progress { position: absolute; left: 14px; right: 14px; bottom: 6px; height: 3px; overflow: hidden; border-radius: 3px; background: color-mix(in srgb, var(--card-role) 14%, transparent); pointer-events: none; }
  .card-progress i { display: block; height: 100%; border-radius: inherit; background: var(--card-role); transition: width 320ms cubic-bezier(.2, .8, .2, 1); }
  .card-attention { position: absolute; z-index: 3; top: 8px; right: 8px; min-height: 22px; padding: 0 9px 0 7px; display: inline-flex; align-items: center; gap: 5px; border: 1px solid color-mix(in srgb, var(--board-warning) 55%, transparent); border-radius: 999px; color: var(--board-warning); background: color-mix(in srgb, var(--board-warning) 14%, var(--workspace-raised)); font-size: 10px; font-weight: 700; cursor: pointer; animation: attention-pulse 1.8s ease-in-out infinite; }
  .card-attention:hover { background: color-mix(in srgb, var(--board-warning) 24%, var(--workspace-raised)); }
  .board-card[data-attention] { border-color: color-mix(in srgb, var(--board-warning) 60%, var(--workspace-line)); }
  @keyframes attention-pulse { 50% { box-shadow: 0 0 0 4px color-mix(in srgb, var(--board-warning) 18%, transparent); } }
  .card-port { position: absolute; top: calc(50% - 13px); width: 26px; height: 26px; padding: 0; display: grid; place-items: center; border: 0; color: var(--card-role); background: transparent; touch-action: none; cursor: crosshair; }
  .port-in { left: -13px; }.port-out { right: -13px; }
  .card-port i { width: 9px; height: 9px; border: 2px solid currentColor; border-radius: 50%; background: var(--workspace-raised); box-shadow: 0 1px 3px rgba(20, 38, 30, .25); transition: transform 140ms ease, color 140ms ease; }
  .card-port:hover i, .card-port:focus-visible i { color: var(--workspace-accent); transform: scale(1.35); }
  .board-toolbar { position: absolute; z-index: 5; top: 16px; left: 18px; max-width: calc(100% - 270px); display: flex; align-items: center; gap: 7px; min-width: 0; padding: 6px 9px; border: 1px solid var(--workspace-line); border-radius: 16px; background: var(--hud-surface); box-shadow: var(--hud-shadow); }
  .board-title { display: flex; align-items: center; gap: 7px; padding: 0 5px; color: var(--workspace-strong); }
  .board-title strong { font-size: 12px; font-weight: 650; }
  .toolbar-divider { height: 19px; width: 1px; background: var(--workspace-line); }
  .board-toolbar button, .board-navigation button, .board-panel > header button { width: 29px; height: 29px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 7px; color: var(--workspace-muted); background: transparent; }
  .board-toolbar button:hover, .board-navigation button:hover, .board-panel > header button:hover { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .board-toolbar .add-agent { width: auto; display: flex; gap: 5px; padding: 0 8px; font-size: 11px; }
  .board-toolbar .close-board { margin-left: 6px; }
  .board-toolbar .delete-workflow:hover:not(:disabled) { color: var(--board-danger); }
  .board-toolbar .delete-workflow:disabled { opacity: .4; }
  .board-toolbar .delete-workflow.confirm { width: auto; padding: 0 9px; display: flex; gap: 5px; color: var(--board-danger); background: color-mix(in srgb, var(--board-danger) 12%, transparent); font-size: 10px; font-weight: 700; }
  .board-card.leaving { opacity: .55; outline: 2px dashed var(--board-danger); outline-offset: 3px; }
  .save-state { margin-left: 4px; color: var(--workspace-muted); font-size: 10px; }
  .save-state:empty { display: none; }
  .board-panel { position: absolute; z-index: 6; top: 70px; right: 18px; bottom: 157px; width: min(292px, calc(100% - 36px)); display: grid; grid-template-rows: auto minmax(0, 1fr); border: 1px solid var(--workspace-line); border-radius: 18px; background: var(--hud-surface); box-shadow: var(--hud-shadow); overflow: hidden; }
  .board-panel > header { background: color-mix(in srgb, var(--workspace-subtle) 55%, transparent); display: flex; align-items: center; justify-content: space-between; min-height: 42px; padding: 4px 12px 4px 16px; border-bottom: 1px solid var(--workspace-line); }
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
  .panel-actions { display: flex; gap: 6px; margin-top: 5px; }
  .panel-actions button, .run-actions button, .empty-card button { min-height: 31px; padding: 0 10px; display: flex; align-items: center; justify-content: center; gap: 6px; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-text); background: var(--workspace-subtle); font-size: 11px; }
  .panel-actions button:hover:not(:disabled), .run-actions button:hover:not(:disabled) { border-color: var(--workspace-accent); }
  .panel-actions .danger, .run-actions .danger { color: var(--board-danger); background: transparent; }
  .session-warning { margin: 0; display: flex; align-items: flex-start; gap: 7px; color: var(--board-warning); font-size: 11px; line-height: 1.5; }
  .bridge { padding: 10px; display: flex; flex-direction: column; gap: 8px; min-height: 0; overflow-y: auto; overscroll-behavior: contain; scrollbar-width: thin; scrollbar-color: var(--workspace-line) transparent; border-radius: 16px; }
  .bridge.run-locked .context-policy button, .bridge.run-locked .share-options button, .bridge.run-locked .behavior-row button, .bridge.run-locked textarea, .bridge.run-locked .remove, .bridge.run-locked .reverse { opacity: .45; pointer-events: none; }
  .route-heading { min-height: 32px; display: grid; grid-template-columns: 26px minmax(0, 1fr) 26px minmax(0, 1fr) 26px; align-items: center; gap: 4px; }
  .route-heading > button { width: 26px; height: 26px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 8px; color: var(--workspace-muted); background: transparent; }
  .route-heading > button:hover { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .route-heading svg, .instruction-toggle svg, .preview-actions svg, .bridge-preview svg { width: 15px; height: 15px; fill: none; stroke: currentColor; stroke-width: 1.5; stroke-linecap: round; stroke-linejoin: round; }
  .route-heading .reverse:hover svg { transform: rotate(180deg); transition: transform 200ms ease; }
  .agent { min-width: 0; display: flex; align-items: center; gap: 6px; }
  .agent strong { overflow: hidden; color: var(--workspace-strong); font-size: 11px; font-weight: 650; text-overflow: ellipsis; white-space: nowrap; }
  .agent.target { justify-content: flex-end; text-align: right; }
  .context-policy { display: grid; gap: 6px; }
  .context-policy > strong { color: var(--workspace-muted); font-size: 10.5px; font-weight: 650; }
  .context-policy > div { height: 34px; padding: 3px; display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 3px; border: 1px solid var(--workspace-line); border-radius: 10px; background: var(--workspace-subtle); }
  .context-policy button { min-width: 0; padding: 0 4px; overflow: hidden; border: 0; border-radius: 7px; color: var(--workspace-muted); background: transparent; font-size: 9.5px; font-weight: 700; text-overflow: ellipsis; white-space: nowrap; transition: color 120ms ease, background 140ms ease, box-shadow 140ms ease; }
  .context-policy button:hover:not(.active) { color: var(--workspace-strong); }
  .context-policy button.active { color: var(--workspace-accent); background: var(--workspace-raised); box-shadow: 0 2px 6px rgba(20, 38, 30, .12); }
  .share-options { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 5px; }
  .share-options button { position: relative; min-width: 0; height: 50px; padding: 5px 2px; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 4px; border: 1px solid var(--workspace-line); border-radius: 10px; color: var(--workspace-muted); background: var(--workspace-subtle); transition: color 120ms ease, border-color 120ms ease, background 140ms ease; }
  .share-options button > svg { width: 17px; height: 17px; fill: none; stroke: currentColor; stroke-width: 1.5; stroke-linecap: round; stroke-linejoin: round; }
  .share-options button:hover:not(.active) { color: var(--workspace-strong); }
  .share-options button.active { color: var(--workspace-accent); border-color: color-mix(in srgb, var(--workspace-accent) 40%, var(--workspace-line)); background: var(--workspace-accent-soft); }
  .share-options span { width: 100%; overflow: hidden; font-size: 9px; font-weight: 700; text-align: center; text-overflow: ellipsis; white-space: nowrap; }
  .share-options i { position: absolute; top: 4px; right: 4px; width: 11px; height: 11px; display: grid; place-items: center; border: 1px solid var(--workspace-line); border-radius: 50%; color: transparent; font: 800 7px var(--lume-font-ui, Inter, sans-serif); font-style: normal; }
  .share-options button.active i { color: #fff; border-color: var(--workspace-accent); background: var(--workspace-accent); }
  .behavior-row { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; }
  .transition-toggle { height: 38px; padding: 3px; display: flex; gap: 2px; overflow: hidden; border: 1px solid var(--workspace-line); border-radius: 10px; background: var(--workspace-subtle); }
  .transition-toggle button { width: 30px; min-width: 30px; padding: 0; display: flex; align-items: center; justify-content: center; gap: 3px; overflow: hidden; border: 0; border-radius: 7px; color: var(--workspace-muted); background: transparent; font-size: 9.5px; font-weight: 700; transition: width 180ms cubic-bezier(.2, .8, .2, 1), color 140ms ease, background 160ms ease, box-shadow 160ms ease; }
  .transition-toggle button svg { width: 14px; height: 14px; flex: 0 0 auto; fill: none; stroke: currentColor; stroke-width: 1.5; stroke-linecap: round; stroke-linejoin: round; }
  .transition-toggle button span { max-width: 0; overflow: hidden; opacity: 0; transform: translateX(-3px); white-space: nowrap; transition: max-width 180ms cubic-bezier(.2, .8, .2, 1), opacity 120ms ease, transform 180ms ease; }
  .transition-toggle button.active { width: calc(100% - 32px); color: var(--workspace-accent); background: var(--workspace-raised); box-shadow: 0 2px 6px rgba(20, 38, 30, .12); }
  .transition-toggle button.active span { max-width: 52px; opacity: 1; transform: translateX(0); }
  .approval { height: 38px; padding: 0 9px; display: flex; align-items: center; gap: 6px; border: 1px solid var(--workspace-line); border-radius: 10px; color: var(--workspace-muted); background: var(--workspace-subtle); }
  .approval svg { width: 15px; height: 15px; flex: 0 0 auto; fill: none; stroke: currentColor; stroke-width: 1.5; stroke-linecap: round; stroke-linejoin: round; }
  .approval span { min-width: 0; flex: 1; overflow: hidden; font-size: 10px; font-weight: 700; text-align: left; text-overflow: ellipsis; white-space: nowrap; }
  .approval > i { width: 23px; height: 13px; padding: 2px; box-sizing: border-box; flex: 0 0 auto; border-radius: 7px; background: color-mix(in srgb, var(--workspace-muted) 40%, transparent); transition: background 140ms ease; }
  .approval > i::before { width: 9px; height: 9px; display: block; border-radius: 50%; content: ""; background: #fff; transition: transform 140ms ease; }
  .approval.active { color: var(--workspace-accent); border-color: color-mix(in srgb, var(--workspace-accent) 40%, var(--workspace-line)); background: var(--workspace-accent-soft); }
  .approval.active > i { background: var(--workspace-accent); }
  .approval.active > i::before { transform: translateX(10px); }
  .instruction-toggle { height: 32px; padding: 0 7px; display: flex; align-items: center; gap: 7px; border: 0; border-radius: 8px; color: var(--workspace-muted); background: transparent; font-size: 10.5px; font-weight: 700; }
  .instruction-toggle:hover { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .instruction-toggle span { min-width: 0; flex: 1; text-align: left; }
  .instruction-toggle svg:last-child { transition: transform 140ms ease; }
  .instruction-toggle.open svg:last-child { transform: rotate(180deg); }
  .edge-popover.compact .bridge { padding: 8px; gap: 5px; }
  .edge-popover.compact .route-heading { min-height: 28px; }
  .edge-popover.compact .context-policy { gap: 4px; }
  .edge-popover.compact .context-policy > div { height: 30px; }
  .edge-popover.compact .share-options { gap: 4px; }
  .edge-popover.compact .share-options button { height: 40px; gap: 2px; }
  .edge-popover.compact .share-options button > svg { width: 15px; height: 15px; }
  .edge-popover.compact .transition-toggle, .edge-popover.compact .approval { height: 32px; }
  .edge-popover.compact .instruction-toggle { height: 28px; }
  .edge-popover.compact .bridge textarea { min-height: 46px; }
  .bridge textarea { box-sizing: border-box; width: 100%; min-width: 0; min-height: 56px; padding: 8px 9px; resize: none; border: 1px solid var(--workspace-line); border-radius: 9px; outline: none; color: var(--workspace-strong); background: var(--workspace-subtle); font: 10.5px/1.45 inherit; caret-color: var(--workspace-accent); }
  .bridge textarea:focus { border-color: color-mix(in srgb, var(--workspace-accent) 55%, var(--workspace-line)); box-shadow: 0 0 0 2px var(--workspace-accent-soft); }
  .preview-actions { display: grid; grid-template-columns: minmax(0, 1fr) 36px; gap: 6px; align-items: start; }
  .preview-actions textarea { min-height: 38px; }
  .preview-run { width: 36px; height: 36px; padding: 0; display: grid; place-items: center; border: 1px solid color-mix(in srgb, var(--workspace-accent) 40%, var(--workspace-line)); border-radius: 9px; color: var(--workspace-accent); background: var(--workspace-accent-soft); }
  .preview-run svg { width: 16px; height: 16px; }
  .bridge-preview { min-height: 0; max-height: min(420px, calc(100vh - 150px)); display: flex; flex-direction: column; gap: 8px; }
  .bridge-preview > header { min-height: 28px; display: grid; grid-template-columns: minmax(0, 1fr) auto 26px; align-items: center; gap: 6px; }
  .bridge-preview > header strong { color: var(--workspace-accent); font-size: 11px; }
  .bridge-preview > header span { color: var(--workspace-muted); font-size: 9.5px; font-variant-numeric: tabular-nums; }
  .bridge-preview > header button { width: 26px; height: 26px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 8px; color: var(--workspace-muted); background: transparent; }
  .bridge-preview > header button:hover { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .bridge-preview pre { min-height: 0; max-height: 100%; flex: 1; margin: 0; padding: 10px; overflow: auto; border-radius: 9px; color: var(--workspace-text); background: var(--workspace-subtle); font: 10px/1.55 var(--lume-font-code, ui-monospace, "SFMono-Regular", Consolas, monospace); overflow-wrap: anywhere; white-space: pre-wrap; }
  .redaction-summary { display: flex; flex-wrap: wrap; gap: 4px; }
  .redaction-summary span { padding: 3px 6px; border-radius: 6px; color: var(--board-warning); background: color-mix(in srgb, var(--board-warning) 12%, transparent); font-size: 9px; }
  .preview-note { min-height: 80px; display: flex; align-items: center; justify-content: center; gap: 8px; color: var(--workspace-muted); font-size: 10.5px; text-align: center; }
  .preview-note i { width: 14px; height: 14px; border: 1.5px solid var(--workspace-line); border-top-color: var(--workspace-accent); border-radius: 50%; animation: bridge-spin .7s linear infinite; }
  .bridge footer { display: grid; grid-template-columns: 1fr auto 1fr; align-items: center; gap: 4px; }
  .bridge footer .remove { justify-self: start; min-width: 56px; height: 28px; padding: 0 9px; border: 1px solid transparent; border-radius: 8px; color: var(--board-danger); background: transparent; font-size: 10px; font-weight: 700; }
  .bridge footer .remove:hover:not(:disabled) { border-color: color-mix(in srgb, var(--board-danger) 35%, transparent); }
  .autosave { justify-self: end; color: var(--workspace-muted); font-size: 9.5px; }
  .connection-help { display: grid; place-items: center; }
  .help-trigger { position: relative; z-index: 7; width: 24px; height: 24px; padding: 0; border: 1px solid var(--workspace-line); border-radius: 50%; color: var(--workspace-muted); background: var(--workspace-subtle); font-size: 10px; font-weight: 700; }
  .connection-help:hover .help-trigger, .connection-help:focus-within .help-trigger { color: var(--workspace-accent); border-color: color-mix(in srgb, var(--workspace-accent) 40%, var(--workspace-line)); background: var(--workspace-accent-soft); }
  .help-tooltip { position: absolute; z-index: 6; left: calc(100% + 10px); box-sizing: border-box; padding: 13px 15px; display: flex; flex-direction: column; gap: 7px; border: 1px solid var(--workspace-line); border-radius: 16px; color: var(--workspace-muted); background: var(--workspace-raised); box-shadow: var(--hud-shadow), 0 18px 40px rgba(20, 38, 30, .16); opacity: 0; pointer-events: none; transform: translateX(-6px) scale(.98); transform-origin: left top; transition: opacity 130ms ease, transform 150ms cubic-bezier(.2, .8, .2, 1); }
  .help-tooltip.left { right: calc(100% + 10px); left: auto; transform: translateX(6px) scale(.98); transform-origin: right top; }
  .help-tooltip.inside { inset: 0; width: auto; overflow-y: auto; justify-content: center; border: 0; border-radius: 16px; box-shadow: none; transform: scale(.98); transform-origin: center; }
  .connection-help:hover .help-tooltip, .connection-help:focus-within .help-tooltip { opacity: 1; transform: none; }
  .help-tooltip > strong { color: var(--workspace-strong); font-size: 12.5px; }
  .help-item { display: grid; grid-template-columns: 22px minmax(0, 1fr); align-items: start; gap: 9px; font-size: 11px; line-height: 1.35; }
  .help-tooltip.inside { gap: 4px; padding: 8px 12px; }
  .help-tooltip.inside .help-item { gap: 7px; font-size: 10px; line-height: 1.28; }
  .help-tooltip.inside .help-item > svg { width: 14px; height: 14px; }
  .help-item > svg { width: 17px; height: 17px; margin-top: 1px; justify-self: center; fill: none; stroke: var(--workspace-accent); stroke-width: 1.5; stroke-linecap: round; stroke-linejoin: round; }
  .help-item > span { display: grid; gap: 1px; }
  .help-tooltip b { color: var(--workspace-strong); font-size: 11px; }
  .help-tooltip.inside b { font-size: 10px; }
  @keyframes bridge-spin { to { transform: rotate(360deg); } }
  .workflow-board.entered { opacity: 1; transform: none; }
  @media (prefers-reduced-motion: reduce) { .workflow-board { transition: none; transform: none; } }
  .panel-tabs { display: grid; grid-template-columns: 1fr 1fr; gap: 3px; padding: 3px; border: 1px solid var(--workspace-line); border-radius: 10px; background: var(--workspace-subtle); }
  .panel-tabs button { position: relative; height: 28px; border: 0; border-radius: 7px; color: var(--workspace-muted); background: transparent; font-size: 11px; font-weight: 650; }
  .panel-tabs button.active { color: var(--workspace-accent); background: var(--workspace-raised); box-shadow: 0 2px 6px rgba(20, 38, 30, .12); }
  .panel-tabs button i { position: absolute; top: 7px; right: 14px; width: 5px; height: 5px; border-radius: 50%; background: var(--workspace-accent); }
  .live-summary { display: grid; gap: 6px; padding: 10px 11px; border: 1px solid var(--workspace-line); border-radius: 11px; background: var(--workspace-subtle); }
  .live-state { display: inline-flex; align-items: center; gap: 7px; color: var(--workspace-strong); font-size: 11px; font-weight: 650; }
  .live-state i { width: 7px; height: 7px; border-radius: 50%; background: var(--workspace-muted); }
  .live-summary[data-state="running"] .live-state i { background: #56aada; animation: status-pulse 1.4s ease-in-out infinite; }
  .live-summary[data-state="completed"] .live-state i { background: var(--workspace-accent); }
  .live-summary[data-state="failed"] .live-state i { background: var(--board-danger); }
  .live-now, .live-ask { margin: 0; color: var(--workspace-text); font-size: 11px; line-height: 1.5; overflow-wrap: anywhere; }
  .live-ask { color: var(--board-warning); }
  .live-attention { justify-self: start; min-height: 24px; padding: 0 10px; display: inline-flex; align-items: center; gap: 5px; border: 1px solid color-mix(in srgb, var(--board-warning) 55%, transparent); border-radius: 999px; color: var(--board-warning); background: color-mix(in srgb, var(--board-warning) 12%, transparent); font-size: 10.5px; font-weight: 700; }
  .live-plan { display: grid; gap: 7px; }
  .live-plan-head, .live-feed > strong, .live-answer > strong { display: flex; justify-content: space-between; color: var(--workspace-strong); font-size: 11px; font-weight: 650; }
  .live-plan-head small { color: var(--workspace-muted); font-variant-numeric: tabular-nums; }
  .card-progress.live-bar { position: relative; left: auto; right: auto; bottom: auto; }
  .live-plan ul, .live-feed ol { margin: 0; padding: 0; display: grid; gap: 5px; list-style: none; }
  .live-plan li { display: flex; align-items: center; gap: 8px; color: var(--workspace-muted); font-size: 10.5px; }
  .live-plan li i { width: 9px; height: 9px; flex: 0 0 auto; border: 1.5px solid currentColor; border-radius: 50%; }
  .live-plan li[data-status="completed"] { color: var(--workspace-text); text-decoration: line-through; text-decoration-color: color-mix(in srgb, currentColor 40%, transparent); }
  .live-plan li[data-status="completed"] i { border-color: var(--workspace-accent); background: var(--workspace-accent); }
  .live-plan li[data-status="in_progress"] { color: var(--workspace-strong); }
  .live-plan li[data-status="in_progress"] i { border-color: #56aada; animation: status-pulse 1.4s ease-in-out infinite; }
  .live-answer { display: grid; gap: 6px; }
  .live-answer p { margin: 0; max-height: 120px; padding: 9px 10px; overflow: auto; border-radius: 9px; color: var(--workspace-text); background: var(--workspace-subtle); font-size: 10.5px; line-height: 1.55; white-space: pre-wrap; overflow-wrap: anywhere; }
  .live-feed { display: grid; gap: 8px; }
  .live-feed li { display: grid; grid-template-columns: 8px minmax(0, 1fr) auto; align-items: start; gap: 8px; font-size: 10.5px; }
  .live-feed li i { width: 6px; height: 6px; margin-top: 4px; border-radius: 50%; background: var(--workspace-muted); }
  .live-feed li[data-status="running"] i { background: #56aada; animation: status-pulse 1.4s ease-in-out infinite; }
  .live-feed li[data-status="failed"] i { background: var(--board-danger); }
  .live-feed li span { min-width: 0; display: grid; gap: 1px; }
  .live-feed li b { color: var(--workspace-strong); font-weight: 650; }
  .live-feed li small { overflow: hidden; color: var(--workspace-muted); font-size: 10px; text-overflow: ellipsis; white-space: nowrap; }
  .live-feed time { color: var(--workspace-muted); font-size: 9.5px; font-variant-numeric: tabular-nums; }
  .sim-toggle { width: auto !important; padding: 0 9px !important; display: flex !important; gap: 5px; font-size: 11px; }
  .sim-toggle.active { color: var(--workspace-accent) !important; background: var(--workspace-accent-soft) !important; }
  .board-sim { position: absolute; z-index: 5; left: 50%; bottom: 16px; width: min(780px, calc(100% - 36px)); box-sizing: border-box; transform: translateX(-50%); padding: 10px 14px 12px; display: grid; gap: 8px; border: 1px solid color-mix(in srgb, var(--workspace-accent) 40%, var(--workspace-line)); border-radius: 18px; background: var(--hud-surface); box-shadow: var(--hud-shadow); }
  .board-sim > header { display: flex; align-items: center; gap: 10px; }
  .board-sim > header strong { color: var(--workspace-strong); font-size: 11px; }
  .board-sim > header small { color: var(--workspace-muted); font-size: 10px; font-variant-numeric: tabular-nums; }
  .sim-actions { margin-left: auto; display: flex; gap: 4px; }
  .sim-actions button { min-width: 28px; height: 26px; padding: 0 9px; display: inline-flex; align-items: center; justify-content: center; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-text); background: var(--workspace-subtle); font-size: 10px; font-weight: 650; cursor: pointer; }
  .sim-actions button:hover, .sim-actions button.on { color: var(--workspace-accent); border-color: var(--workspace-accent); }
  .sim-steps { display: grid; gap: 6px; }
  .sim-step { display: grid; grid-template-columns: minmax(0, 1.4fr) 112px 112px minmax(80px, 1fr); align-items: center; gap: 8px; }
  .sim-step strong { overflow: hidden; color: var(--workspace-strong); font-size: 10.5px; text-overflow: ellipsis; white-space: nowrap; }
  .sim-step select { height: 26px; min-width: 0; padding: 0 6px; border: 1px solid var(--workspace-line); border-radius: 7px; color: var(--workspace-text); background: var(--workspace-subtle); font: inherit; font-size: 10.5px; }
  .sim-step input[type="range"] { width: 100%; accent-color: var(--workspace-accent); }
  .board-navigation { position: absolute; z-index: 5; top: 16px; right: 18px; display: flex; align-items: center; gap: 2px; padding: 5px 4px; border: 1px solid var(--workspace-line); border-radius: 13px; background: var(--hud-surface); box-shadow: var(--hud-shadow); }
  .board-navigation button { transition: color 120ms ease, background 120ms ease, transform 120ms ease; }
  .board-navigation button:active { transform: scale(.92); }
  .board-navigation > span { width: 1px; height: 15px; margin: 0 4px; background: var(--workspace-line); }
  .board-navigation .zoom-label { width: 44px; font-size: 10px; font-variant-numeric: tabular-nums; }
  .connection-hint { position: absolute; z-index: 4; bottom: 155px; left: 50%; width: max-content; max-width: calc(100% - 50px); padding: 9px 14px; margin: 0; border: 1px solid var(--workspace-line); border-radius: 999px; background: var(--hud-surface); box-shadow: var(--hud-shadow); color: var(--workspace-strong); font-size: 11px; transform: translateX(-50%); pointer-events: none; }
  .board-empty { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; padding: 70px 28px 100px; box-sizing: border-box; pointer-events: none; }
  .empty-card { max-width: 340px; padding: 28px 30px 24px; box-sizing: border-box; display: flex; align-items: center; flex-direction: column; gap: 14px; border: 1px solid var(--workspace-line); border-radius: 22px; background: var(--workspace-raised); box-shadow: var(--hud-shadow); text-align: center; }
  .empty-card > :global(svg) { box-sizing: content-box; padding: 16px; border: 1px dashed color-mix(in srgb, var(--workspace-accent) 50%, var(--workspace-line)); border-radius: 20px; color: var(--workspace-accent); background: color-mix(in srgb, var(--workspace-accent) 8%, var(--workspace-raised)); }
  .empty-card strong { margin-top: 2px; color: var(--workspace-strong); font-size: 19px; font-weight: 600; letter-spacing: -.02em; }
  .empty-card p { margin: 0; max-width: 300px; font-size: 12px; line-height: 1.6; color: var(--workspace-muted); }
  .empty-card button { margin: 3px 0; pointer-events: auto; color: var(--workspace-strong); }
  .empty-card small { max-width: 280px; font-size: 10px; color: var(--workspace-muted); line-height: 1.5; }
  .board-run { position: absolute; z-index: 5; left: 50%; bottom: 16px; width: min(780px, calc(100% - 36px)); transform: translateX(-50%); padding: 12px 14px 13px; overflow: hidden; display: grid; gap: 10px; border: 1px solid var(--workspace-line); border-radius: 18px; background: var(--hud-surface); box-shadow: var(--hud-shadow); }
  .run-track { position: absolute; inset: 0 0 auto; height: 3px; background: color-mix(in srgb, var(--workspace-muted) 14%, transparent); }
  .run-track i { display: block; height: 100%; width: var(--progress, 0%); border-radius: 0 3px 3px 0; background: linear-gradient(90deg, var(--role-planner), var(--role-implementer)); transition: width 320ms cubic-bezier(.2, .8, .2, 1); }
  [data-status="failed"] .run-track i { background: var(--board-danger); }
  [data-status="waiting_for_approval"] .run-track i, [data-status="paused"] .run-track i { background: var(--board-warning); }
  .run-heading { min-width: 0; display: flex; align-items: center; gap: 9px; }
  .run-state { padding: 4px 10px 4px 9px; display: flex; align-items: center; border-radius: 999px; color: var(--workspace-strong); background: color-mix(in srgb, var(--workspace-muted) 10%, transparent); font-size: 11px; white-space: nowrap; }
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
    .board-toolbar { gap: 3px; left: 12px; max-width: calc(100% - 200px); }
    .board-title strong, .save-state { display: none; }
    .board-toolbar :global(.lume-select) { min-width: 95px !important; }
    .board-run { width: calc(100% - 24px); }
    .board-panel { right: 12px; width: min(278px, calc(100% - 24px)); }
    .run-controls { flex-wrap: wrap; }
    .run-controls textarea { flex-basis: 100%; }
    .run-actions { margin-left: auto; }
    .workflow-name { width: 95px; }
    .board-navigation { right: 12px; }
  }
  @media (prefers-reduced-motion: reduce) {
    .card-attention, .card-activity.live i, .board-card.working, .live .pipe-core { animation: none; }
    .board-card, .card-port i, .card-role-fab, .role-tooltip, .pipe-wall, .run-track i, .board-navigation button { transition: none; }
    .pipe-energy, .pipe-energy-glow, .card-status i { animation: none !important; }
    .pipe-energy, .pipe-energy-glow { stroke-dasharray: 2 24; opacity: .55; }
    .board-card.moving { transform: none; }
    .board-card.ghost { animation: none; }
    .edge-hub { transition: none; }
  }
</style>
