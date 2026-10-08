import type {
  WorkflowConnectionDefinition,
  WorkflowContextSelection,
  WorkflowGroupDefinition,
  WorkflowRole,
  WorkflowRoleContract,
  WorkflowRun,
  WorkflowStepDefinition,
  WorkflowStepRunStatus,
} from "./domain";

/**
 * The workflow board is another way to see and connect the same
 * `WorkflowGroupDefinition` the orb builds: it never adds a concept the runtime
 * cannot execute. Position is layout only; the order is the arrows.
 */

export const BOARD_CARD_WIDTH = 252;
export const BOARD_CARD_HEIGHT = 168;
export const BOARD_MIN_ZOOM = 0.35;
export const BOARD_MAX_ZOOM = 1.6;

export type BoardPoint = { x: number; y: number };
export type BoardRect = BoardPoint & { width: number; height: number };
export type BoardView = { x: number; y: number; zoom: number };
export type BoardLayout = {
  name?: string;
  positions: Record<string, BoardPoint>;
  view: BoardView;
};

// ───────────────────────── defaults the orb also uses ─────────────────────────

export function defaultRole(index: number): WorkflowRole {
  return (["planner", "implementer", "reviewer", "tester", "researcher"] as WorkflowRole[])[index] ?? "custom";
}

export function defaultContextSelection(): WorkflowContextSelection {
  return { response: true, files: true, checks: true, plan: false, activity: false, diffs: false };
}

export function newStep(sessionKey: string, role: WorkflowRole, contract: WorkflowRoleContract): WorkflowStepDefinition {
  return { id: `step-${sessionKey}`, sessionNativeId: sessionKey, role, customRoleLabel: "", ...contract, attempt: 0 };
}

/** Refresh defaults when changing roles, without silently discarding tailored fields. */
export function preserveRoleOverrides(step: WorkflowStepDefinition, previous: WorkflowRoleContract, next: WorkflowRoleContract): WorkflowRoleContract {
  const field = (key: keyof WorkflowRoleContract) => step[key] === previous[key] ? next[key] : step[key];
  return {
    instruction: field("instruction"), expectedInput: field("expectedInput"),
    producedOutput: field("producedOutput"), completionCondition: field("completionCondition"),
  };
}

export function newConnection(fromStepId: string, toStepId: string): WorkflowConnectionDefinition {
  return {
    id: `connection-${crypto.randomUUID()}`,
    fromStepId,
    toStepId,
    includeResponse: true,
    includeFiles: true,
    includeTests: true,
    contextPolicy: "standard",
    contextSelection: defaultContextSelection(),
    additionalInstruction: "",
    requiresApproval: true,
    advanceMode: "manual",
  };
}

export function newGroup(): WorkflowGroupDefinition {
  const id = crypto.randomUUID();
  // The backend only needs a stable, non-empty identifier here; a board group
  // has no docked terminals behind it.
  return { id: `workflow-board-${id}`, terminalGroupId: `board-${id}`, steps: [], connections: [] };
}

// ───────────────────────── the chain the runtime can run ─────────────────────────

export type BoardProblemCode =
  | "empty"
  | "custom-label"
  | "contract"
  | "no-start"
  | "multiple-starts"
  | "branch-out"
  | "branch-in"
  | "cycle"
  | "disconnected";

export type BoardProblem = { code: BoardProblemCode; stepIds: string[] };

export type ChainAnalysis = {
  /** Step ids along the chain, from the single first step. */
  order: string[];
  start: string | null;
  /** Steps that are not part of the chain; the runtime refuses to start with any. */
  loose: string[];
  problems: BoardProblem[];
};

/** Mirrors `validate_manual_workflow` in the runtime so the board can say why a group cannot run yet. */
export function analyzeChain(group: WorkflowGroupDefinition): ChainAnalysis {
  const problems: BoardProblem[] = [];
  const stepIds = group.steps.map((step) => step.id);
  if (!group.steps.length) {
    return { order: [], start: null, loose: [], problems: [{ code: "empty", stepIds: [] }] };
  }
  const unlabeled = group.steps.filter((step) => step.role === "custom" && !step.customRoleLabel.trim()).map((step) => step.id);
  if (unlabeled.length) problems.push({ code: "custom-label", stepIds: unlabeled });
  const incomplete = group.steps
    .filter((step) => ![step.instruction, step.expectedInput, step.producedOutput, step.completionCondition].every((value) => value.trim()))
    .map((step) => step.id);
  if (incomplete.length) problems.push({ code: "contract", stepIds: incomplete });

  const outgoing = new Map<string, WorkflowConnectionDefinition[]>();
  const incoming = new Map<string, WorkflowConnectionDefinition[]>();
  for (const connection of group.connections) {
    outgoing.set(connection.fromStepId, [...(outgoing.get(connection.fromStepId) ?? []), connection]);
    incoming.set(connection.toStepId, [...(incoming.get(connection.toStepId) ?? []), connection]);
  }
  const branchingOut = stepIds.filter((id) => (outgoing.get(id)?.length ?? 0) > 1);
  const branchingIn = stepIds.filter((id) => (incoming.get(id)?.length ?? 0) > 1);
  if (branchingOut.length) problems.push({ code: "branch-out", stepIds: branchingOut });
  if (branchingIn.length) problems.push({ code: "branch-in", stepIds: branchingIn });

  const starts = stepIds.filter((id) => !incoming.has(id));
  if (starts.length === 0) problems.push({ code: "no-start", stepIds: [] });
  if (starts.length > 1) problems.push({ code: "multiple-starts", stepIds: starts });
  const start = starts.length === 1 ? starts[0] : null;

  const order: string[] = [];
  const seen = new Set<string>();
  let current = start;
  while (current) {
    if (seen.has(current)) {
      problems.push({ code: "cycle", stepIds: [...seen] });
      break;
    }
    seen.add(current);
    order.push(current);
    current = outgoing.get(current)?.[0]?.toStepId ?? null;
  }
  const loose = start ? stepIds.filter((id) => !seen.has(id)) : [];
  if (start && loose.length && !problems.some((problem) => problem.code === "cycle")) {
    problems.push({ code: "disconnected", stepIds: loose });
  }
  return { order, start, loose, problems };
}

export type ConnectionRefusal = "same-step" | "exists" | "has-outgoing" | "has-incoming" | "cycle";
export type ConnectionCheck = { ok: true } | { ok: false; reason: ConnectionRefusal };

/** Whether drawing `fromStepId → toStepId` keeps the group a chain the runtime can run. */
export function checkConnection(group: WorkflowGroupDefinition, fromStepId: string, toStepId: string): ConnectionCheck {
  if (fromStepId === toStepId) return { ok: false, reason: "same-step" };
  if (group.connections.some((item) => item.fromStepId === fromStepId && item.toStepId === toStepId)) {
    return { ok: false, reason: "exists" };
  }
  if (group.connections.some((item) => item.fromStepId === fromStepId)) return { ok: false, reason: "has-outgoing" };
  if (group.connections.some((item) => item.toStepId === toStepId)) return { ok: false, reason: "has-incoming" };
  // The target's own chain reaching the source would close a loop.
  let cursor: string | undefined = toStepId;
  const seen = new Set<string>();
  while (cursor && !seen.has(cursor)) {
    if (cursor === fromStepId) return { ok: false, reason: "cycle" };
    seen.add(cursor);
    cursor = group.connections.find((item) => item.fromStepId === cursor)?.toStepId;
  }
  return { ok: true };
}

export function connectionRefusalMessage(reason: ConnectionRefusal, portuguese: boolean): string {
  const messages: Record<ConnectionRefusal, [string, string]> = {
    "same-step": ["A step cannot connect to itself.", "Uma etapa não pode se conectar a ela mesma."],
    exists: ["These steps are already connected.", "Essas etapas já estão conectadas."],
    "has-outgoing": ["This step already passes its result on. Remove its outgoing arrow first.", "Esta etapa já passa o resultado adiante. Remova a seta de saída antes."],
    "has-incoming": ["The next step already receives from another one. Remove that arrow first.", "A próxima etapa já recebe de outra. Remova essa seta antes."],
    cycle: ["That arrow would close a loop. The workflow runs once, in a line.", "Essa seta fecharia um laço. O workflow roda uma vez, em linha."],
  };
  return messages[reason][portuguese ? 1 : 0];
}

export function problemMessage(problem: BoardProblem, portuguese: boolean): string {
  const messages: Record<BoardProblemCode, [string, string]> = {
    empty: ["Add at least one step.", "Adicione ao menos uma etapa."],
    "custom-label": ["Name the custom role.", "Dê um nome ao papel personalizado."],
    contract: ["Complete the role contract.", "Complete o contrato do papel."],
    "no-start": ["There is no first step.", "Não há uma primeira etapa."],
    "multiple-starts": ["Only one step can start the workflow. Connect the others.", "Só uma etapa pode iniciar o workflow. Conecte as outras."],
    "branch-out": ["A step can pass its result to only one other.", "Uma etapa só pode passar o resultado para uma outra."],
    "branch-in": ["A step can receive from only one other.", "Uma etapa só pode receber de uma outra."],
    cycle: ["The arrows form a loop.", "As setas formam um laço."],
    disconnected: ["Connect every step into one route.", "Conecte todas as etapas em uma única rota."],
  };
  return messages[problem.code][portuguese ? 1 : 0];
}

// ───────────────────────── edits (each returns a new group) ─────────────────────────

export function addStep(group: WorkflowGroupDefinition, step: WorkflowStepDefinition): WorkflowGroupDefinition {
  if (group.steps.some((item) => item.id === step.id || item.sessionNativeId === step.sessionNativeId)) return group;
  return { ...group, steps: [...group.steps, step] };
}

export function updateStep(group: WorkflowGroupDefinition, stepId: string, patch: Partial<WorkflowStepDefinition>): WorkflowGroupDefinition {
  return { ...group, steps: group.steps.map((step) => (step.id === stepId ? { ...step, ...patch } : step)) };
}

/**
 * Removes a step. In the middle of a chain the neighbours are reconnected, so
 * taking out B from A → B → C leaves A → C with what A was passing on.
 */
export function removeStep(group: WorkflowGroupDefinition, stepId: string): WorkflowGroupDefinition {
  const before = group.connections.find((item) => item.toStepId === stepId);
  const after = group.connections.find((item) => item.fromStepId === stepId);
  let connections = group.connections.filter((item) => item.fromStepId !== stepId && item.toStepId !== stepId);
  if (before && after && before.fromStepId !== after.toStepId) {
    connections = [...connections, { ...before, toStepId: after.toStepId }];
  }
  return { ...group, steps: group.steps.filter((step) => step.id !== stepId), connections };
}

export function addConnection(group: WorkflowGroupDefinition, fromStepId: string, toStepId: string): WorkflowGroupDefinition {
  if (!checkConnection(group, fromStepId, toStepId).ok) return group;
  return { ...group, connections: [...group.connections, newConnection(fromStepId, toStepId)] };
}

export function updateConnection(group: WorkflowGroupDefinition, connectionId: string, patch: Partial<WorkflowConnectionDefinition>): WorkflowGroupDefinition {
  return { ...group, connections: group.connections.map((item) => (item.id === connectionId ? { ...item, ...patch } : item)) };
}

export function removeConnection(group: WorkflowGroupDefinition, connectionId: string): WorkflowGroupDefinition {
  return { ...group, connections: group.connections.filter((item) => item.id !== connectionId) };
}

/** Puts `step` in the middle of an arrow: A → B becomes A → step → B, both keeping what the arrow carried. */
export function insertStep(group: WorkflowGroupDefinition, connectionId: string, step: WorkflowStepDefinition): WorkflowGroupDefinition {
  const connection = group.connections.find((item) => item.id === connectionId);
  if (!connection || group.steps.some((item) => item.id === step.id || item.sessionNativeId === step.sessionNativeId)) return group;
  const first = { ...connection, toStepId: step.id };
  const second = { ...connection, id: `connection-${crypto.randomUUID()}`, fromStepId: step.id, toStepId: connection.toStepId };
  return {
    ...group,
    steps: [...group.steps, step],
    connections: group.connections.flatMap((item) => (item.id === connectionId ? [first, second] : [item])),
  };
}

// ───────────────────────── geometry ─────────────────────────

export function cardRect(position: BoardPoint): BoardRect {
  return { ...position, width: BOARD_CARD_WIDTH, height: BOARD_CARD_HEIGHT };
}

/** Length of the arrowhead drawn over the end of a connection. */
export const BOARD_ARROW_LENGTH = 13;

/** `path` runs from side to side; `pipe` stops where the arrowhead begins so the pipe never pokes out of its tip. */
export type EdgeGeometry = { path: string; pipe: string; start: BoardPoint; end: BoardPoint; mid: BoardPoint };

/** A smooth arrow between two cards, leaving and entering on the sides that face each other. */
export function edgeGeometry(from: BoardRect, to: BoardRect): EdgeGeometry {
  const fromCenter = { x: from.x + from.width / 2, y: from.y + from.height / 2 };
  const toCenter = { x: to.x + to.width / 2, y: to.y + to.height / 2 };
  const dx = toCenter.x - fromCenter.x;
  const dy = toCenter.y - fromCenter.y;
  const horizontal = Math.abs(dx) * (from.height / from.width) >= Math.abs(dy) * 0.55 || Math.abs(dx) > from.width * 0.9;
  let start: BoardPoint;
  let end: BoardPoint;
  let startDirection: BoardPoint;
  let endDirection: BoardPoint;
  if (horizontal) {
    const rightward = dx >= 0;
    start = { x: rightward ? from.x + from.width : from.x, y: fromCenter.y };
    end = { x: rightward ? to.x : to.x + to.width, y: toCenter.y };
    startDirection = { x: rightward ? 1 : -1, y: 0 };
    endDirection = { x: rightward ? -1 : 1, y: 0 };
  } else {
    const downward = dy >= 0;
    start = { x: fromCenter.x, y: downward ? from.y + from.height : from.y };
    end = { x: toCenter.x, y: downward ? to.y : to.y + to.height };
    startDirection = { x: 0, y: downward ? 1 : -1 };
    endDirection = { x: 0, y: downward ? -1 : 1 };
  }
  const reach = Math.max(48, Math.min(180, Math.hypot(end.x - start.x, end.y - start.y) * 0.42));
  const c1 = { x: start.x + startDirection.x * reach, y: start.y + startDirection.y * reach };
  const c2 = { x: end.x + endDirection.x * reach, y: end.y + endDirection.y * reach };
  const at = (t: number, a: number, b: number, c: number, d: number) =>
    (1 - t) ** 3 * a + 3 * (1 - t) ** 2 * t * b + 3 * (1 - t) * t ** 2 * c + t ** 3 * d;
  const pipeEnd = { x: end.x + endDirection.x * BOARD_ARROW_LENGTH, y: end.y + endDirection.y * BOARD_ARROW_LENGTH };
  return {
    path: `M ${start.x} ${start.y} C ${c1.x} ${c1.y}, ${c2.x} ${c2.y}, ${end.x} ${end.y}`,
    pipe: `M ${start.x} ${start.y} C ${c1.x} ${c1.y}, ${c2.x} ${c2.y}, ${pipeEnd.x} ${pipeEnd.y}`,
    start,
    end,
    mid: { x: at(0.5, start.x, c1.x, c2.x, end.x), y: at(0.5, start.y, c1.y, c2.y, end.y) },
  };
}

/** The card under a point (the topmost one when they overlap), or null. */
export function cardAt(point: BoardPoint, positions: Record<string, BoardPoint>, order: string[]): string | null {
  for (const id of [...order].reverse()) {
    const position = positions[id];
    if (!position) continue;
    const rect = cardRect(position);
    if (point.x >= rect.x && point.x <= rect.x + rect.width && point.y >= rect.y && point.y <= rect.y + rect.height) return id;
  }
  return null;
}

/** Where a card with no saved position goes: where it was dropped, or next to the others. */
export function placeCard(existing: BoardPoint[], dropped?: BoardPoint): BoardPoint {
  if (dropped) return { x: Math.round(dropped.x - BOARD_CARD_WIDTH / 2), y: Math.round(dropped.y - BOARD_CARD_HEIGHT / 2) };
  if (!existing.length) return { x: 96, y: 120 };
  const last = existing.reduce((best, point) => (point.x > best.x ? point : best), existing[0]);
  return { x: last.x + BOARD_CARD_WIDTH + 84, y: last.y };
}

/** Screen → board coordinates for a pan/zoom view. */
export function toBoardPoint(screen: BoardPoint, origin: BoardPoint, view: BoardView): BoardPoint {
  return { x: (screen.x - origin.x - view.x) / view.zoom, y: (screen.y - origin.y - view.y) / view.zoom };
}

export function clampZoom(zoom: number): number {
  return Math.min(BOARD_MAX_ZOOM, Math.max(BOARD_MIN_ZOOM, zoom));
}

/** Zooms around a point on the screen so what is under the cursor stays under it. */
export function zoomAround(view: BoardView, anchor: BoardPoint, nextZoom: number): BoardView {
  const zoom = clampZoom(nextZoom);
  const ratio = zoom / view.zoom;
  return { zoom, x: anchor.x - (anchor.x - view.x) * ratio, y: anchor.y - (anchor.y - view.y) * ratio };
}

/** A view that shows every card with some margin. */
export function fitView(positions: BoardPoint[], width: number, height: number): BoardView {
  if (!positions.length || width <= 0 || height <= 0) return { x: 0, y: 0, zoom: 1 };
  const minX = Math.min(...positions.map((point) => point.x));
  const minY = Math.min(...positions.map((point) => point.y));
  const maxX = Math.max(...positions.map((point) => point.x)) + BOARD_CARD_WIDTH;
  const maxY = Math.max(...positions.map((point) => point.y)) + BOARD_CARD_HEIGHT;
  const margin = 72;
  const zoom = clampZoom(Math.min((width - margin * 2) / (maxX - minX), (height - margin * 2) / (maxY - minY), 1));
  return { zoom, x: (width - (maxX - minX) * zoom) / 2 - minX * zoom, y: (height - (maxY - minY) * zoom) / 2 - minY * zoom };
}

// ───────────────────────── saved layout ─────────────────────────

const finite = (value: unknown): value is number => typeof value === "number" && Number.isFinite(value);

/** Reads a saved layout defensively: anything malformed falls back to an empty one. */
export function normalizeLayout(raw: unknown): BoardLayout {
  const value = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  const positions: Record<string, BoardPoint> = {};
  for (const [id, point] of Object.entries((value.positions && typeof value.positions === "object" ? value.positions : {}) as Record<string, unknown>)) {
    const candidate = point as Partial<BoardPoint> | null;
    if (candidate && finite(candidate.x) && finite(candidate.y)) positions[id] = { x: candidate.x, y: candidate.y };
  }
  const view = (value.view && typeof value.view === "object" ? value.view : {}) as Partial<BoardView>;
  return {
    name: typeof value.name === "string" && value.name.trim() ? value.name.trim().slice(0, 80) : undefined,
    positions,
    view: { x: finite(view.x) ? view.x : 0, y: finite(view.y) ? view.y : 0, zoom: finite(view.zoom) ? clampZoom(view.zoom) : 1 },
  };
}

// ───────────────────────── run state ─────────────────────────

export type StepVisualState = WorkflowStepRunStatus | "idle";

/** What a card shows for its step in the latest run of the workflow. */
export function stepVisualState(run: WorkflowRun | null | undefined, stepId: string): StepVisualState {
  return run?.steps.find((step) => step.stepId === stepId)?.status ?? "idle";
}

/** The arrow whose handoff is waiting for the person's approval. */
export function connectionWaitsForApproval(run: WorkflowRun | null | undefined, connectionId: string): boolean {
  return Boolean(run && run.status === "waiting_for_approval" && run.pendingConnectionId === connectionId);
}
