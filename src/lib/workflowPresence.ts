import type {
  WorkflowGroupDefinition,
  WorkflowRole,
  WorkflowRoleContract,
  WorkflowRun,
  WorkflowRunStatus,
  WorkflowStepDefinition,
} from "$lib/domain";
import {
  addConnection,
  addStep,
  analyzeChain,
  connectionWaitsForApproval,
  defaultRole,
  newGroup,
  newStep,
  normalizeLayout,
  stepVisualState,
  type StepVisualState,
} from "$lib/workflowBoard";

/**
 * Where a chat stands in the workflows that use it, so the sidebar and the chat
 * itself can show the step without opening the board. Pure: the Workspace feeds
 * it the saved groups and the latest run of each.
 */

export const BOARD_LAYOUT_STORAGE_KEY = "lume:workflow-board:v1";

export type WorkflowPresence = {
  workflowId: string;
  name: string;
  objective: string;
  stepId: string;
  role: WorkflowRole;
  roleLabel: string;
  /** 1-based position along the chain. */
  index: number;
  total: number;
  previousRoleLabel: string | null;
  nextRoleLabel: string | null;
  state: StepVisualState;
  runStatus: WorkflowRunStatus | null;
  /** The run has started and is not over yet (a failed step still waits for retry or skip). */
  runActive: boolean;
  /** The handoff arriving at this step is waiting for the person's approval. */
  incomingPending: boolean;
  /** The handoff leaving this step is waiting for the person's approval. */
  outgoingPending: boolean;
  /** The handoff arriving at or leaving this step is approved or automatic and the next step can run. */
  incomingReady: boolean;
  outgoingReady: boolean;
  /** The run's current step is this one. */
  isCurrent: boolean;
};

export type PresenceTone = "running" | "approval" | "ready" | "paused" | "failed" | "done" | "idle";

/** Whether `next` should replace `current` as the latest run of a workflow. */
export function newerRun(current: WorkflowRun | null | undefined, next: WorkflowRun): boolean {
  if (!current) return true;
  if (next.id === current.id) return next.updatedAt > current.updatedAt;
  return next.createdAt > current.createdAt || (next.createdAt === current.createdAt && next.updatedAt > current.updatedAt);
}

export function runIsActive(run: WorkflowRun | null | undefined): boolean {
  return Boolean(run && !["draft", "completed", "cancelled"].includes(run.status));
}

const roleNames: Record<WorkflowRole, [string, string]> = {
  planner: ["Planner", "Planejador"],
  implementer: ["Implementer", "Implementador"],
  reviewer: ["Reviewer", "Revisor"],
  tester: ["Tester", "Testador"],
  researcher: ["Researcher", "Pesquisador"],
  custom: ["Custom", "Personalizado"],
};

export function stepRoleLabel(step: WorkflowStepDefinition, portuguese = false): string {
  if (step.role === "custom" && step.customRoleLabel.trim()) return step.customRoleLabel.trim();
  return roleNames[step.role][portuguese ? 1 : 0];
}

/** The same name the board shows for a group: the one given on the board, else its position. */
export function workflowGroupName(group: WorkflowGroupDefinition, index: number, layoutNames: Record<string, string>, portuguese = false): string {
  const named = layoutNames[group.id]?.trim();
  if (named) return named;
  const prefix = group.terminalGroupId.startsWith("board-") ? "Workflow " : portuguese ? "Workflow do Orb " : "Orb workflow ";
  return prefix + (index + 1);
}

/** Names given to groups on the board, which keeps them with its layout on this computer. */
export function boardLayoutNames(): Record<string, string> {
  try {
    const raw = JSON.parse(localStorage.getItem(BOARD_LAYOUT_STORAGE_KEY) || "{}");
    const layouts = raw && typeof raw === "object" && raw.layouts && typeof raw.layouts === "object" ? raw.layouts : {};
    const names: Record<string, string> = {};
    for (const [id, value] of Object.entries(layouts)) {
      const name = normalizeLayout(value).name;
      if (name) names[id] = name;
    }
    return names;
  } catch {
    return {};
  }
}

export function boardLayoutName(id: string): string | undefined {
  return boardLayoutNames()[id];
}

/** Every workflow step each chat plays, keyed by the chat's native session id, active runs first. */
export function sessionWorkflowPresence(
  groups: WorkflowGroupDefinition[],
  runs: Record<string, WorkflowRun | null | undefined>,
  layoutNames: Record<string, string> = {},
  portuguese = false,
): Map<string, WorkflowPresence[]> {
  const presence = new Map<string, WorkflowPresence[]>();
  groups.forEach((group, groupIndex) => {
    const run = runs[group.id] ?? null;
    const chain = analyzeChain(group);
    // Steps outside the chain still belong to the workflow; they follow it in the order they were added.
    const order = [...chain.order, ...group.steps.map((step) => step.id).filter((id) => !chain.order.includes(id))];
    const pending = run?.pendingConnectionId ? group.connections.find((item) => item.id === run.pendingConnectionId) : undefined;
    const waiting = Boolean(pending && connectionWaitsForApproval(run, pending.id));
    const ready = Boolean(pending && run?.status === "ready");
    const name = workflowGroupName(group, groupIndex, layoutNames, portuguese);
    const active = runIsActive(run);
    const label = (stepId?: string) => {
      const step = stepId ? group.steps.find((item) => item.id === stepId) : undefined;
      return step ? stepRoleLabel(step, portuguese) : null;
    };
    order.forEach((stepId, position) => {
      const step = group.steps.find((item) => item.id === stepId);
      const key = step?.sessionNativeId?.trim();
      if (!step || !key) return;
      const entry: WorkflowPresence = {
        workflowId: group.id,
        name,
        objective: run?.objective ?? "",
        stepId,
        role: step.role,
        roleLabel: stepRoleLabel(step, portuguese),
        index: position + 1,
        total: order.length,
        previousRoleLabel: label(group.connections.find((item) => item.toStepId === stepId)?.fromStepId),
        nextRoleLabel: label(group.connections.find((item) => item.fromStepId === stepId)?.toStepId),
        state: stepVisualState(run, stepId),
        runStatus: run?.status ?? null,
        runActive: active,
        incomingPending: waiting && pending?.toStepId === stepId,
        outgoingPending: waiting && pending?.fromStepId === stepId,
        incomingReady: ready && pending?.toStepId === stepId,
        outgoingReady: ready && pending?.fromStepId === stepId,
        isCurrent: Boolean(run && active && run.currentStepId === stepId),
      };
      presence.set(key, [...(presence.get(key) ?? []), entry]);
    });
  });
  for (const [key, list] of presence) {
    presence.set(key, [...list].sort((a, b) => Number(b.runActive) - Number(a.runActive) || Number(b.isCurrent) - Number(a.isCurrent)));
  }
  return presence;
}

/** The single state a chip shows for a step. */
export function presenceTone(presence: WorkflowPresence): PresenceTone {
  if (!presence.runStatus) return "idle";
  if (presence.incomingPending || presence.outgoingPending) return "approval";
  if (presence.state === "running") return presence.runStatus === "paused" ? "paused" : "running";
  if (presence.state === "failed") return "failed";
  if (presence.isCurrent && presence.runStatus === "paused") return "paused";
  if (presence.incomingReady || presence.outgoingReady) return "ready";
  if (presence.state === "completed" || presence.state === "skipped") return "done";
  return "idle";
}

export type ChainableSession = { nativeSessionId?: string | null };
export type ChainResult = { group: WorkflowGroupDefinition; added: string[] };

/**
 * Chains chats into a workflow in the order given: each one becomes a step with the next default role,
 * connected from the end of the existing chain. Chats without a native id or already present are skipped.
 */
export function chainGroupFromSessions(
  sessions: ChainableSession[],
  contracts: Partial<Record<WorkflowRole, WorkflowRoleContract>>,
  base: WorkflowGroupDefinition = newGroup(),
): ChainResult {
  let group = base;
  const added: string[] = [];
  const order = analyzeChain(base).order;
  let tail = order[order.length - 1] ?? base.steps[base.steps.length - 1]?.id ?? null;
  for (const session of sessions) {
    const key = session.nativeSessionId?.trim();
    if (!key || group.steps.some((step) => step.sessionNativeId === key)) continue;
    const role = defaultRole(group.steps.length);
    const contract = contracts[role];
    if (!contract) continue;
    // Past the five built-in roles a step is custom, which needs a name to run.
    const step = { ...newStep(key, role, contract), customRoleLabel: role === "custom" ? `Agent ${group.steps.length + 1}` : "" };
    const withStep = addStep(group, step);
    if (withStep === group) continue;
    group = tail ? addConnection(withStep, tail, step.id) : withStep;
    tail = step.id;
    added.push(key);
  }
  return { group, added };
}
