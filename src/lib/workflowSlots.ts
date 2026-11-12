import type { WorkflowGroupDefinition, WorkflowRole, WorkflowRoleContract, WorkflowStepDefinition } from "$lib/domain";
import { analyzeChain, newConnection, preserveRoleOverrides } from "$lib/workflowBoard";

/**
 * The basic board: three fixed slots instead of the canvas. The first is the
 * orchestrator (the `planner` role), the second the implementer, the third an
 * optional reviewer or tester. The definition stays the same linear chain the
 * runtime runs, built from the filled slots in order. Pure.
 */

export const BASIC_SLOT_COUNT = 3;
export type ThirdRole = "reviewer" | "tester";
/** Step ids by slot; `null` is an empty slot. */
export type SlotIds = (string | null)[];
export type BasicMisfit = "too-many" | "roles" | "route";
export type BasicFit = { ok: true; slots: SlotIds; third: ThirdRole } | { ok: false; reason: BasicMisfit };

export function slotRole(index: number, third: ThirdRole): WorkflowRole {
  return index === 0 ? "planner" : index === 1 ? "implementer" : third;
}

function slotOf(role: WorkflowRole): number {
  return role === "planner" ? 0 : role === "implementer" ? 1 : role === "reviewer" || role === "tester" ? 2 : -1;
}

/** Whether a group can be shown in the slots, and where each step goes. */
export function basicSlotsFor(group: WorkflowGroupDefinition, preferredThird: ThirdRole = "reviewer"): BasicFit {
  if (group.steps.length > BASIC_SLOT_COUNT) return { ok: false, reason: "too-many" };
  const slots: SlotIds = [null, null, null];
  if (!group.steps.length) return { ok: true, slots, third: preferredThird };
  const chain = analyzeChain(group);
  const structural = chain.problems.some((problem) => !["custom-label", "contract"].includes(problem.code));
  if (structural || chain.order.length !== group.steps.length) return { ok: false, reason: "route" };
  let last = -1;
  let third = preferredThird;
  for (const id of chain.order) {
    const role = group.steps.find((step) => step.id === id)!.role;
    const index = slotOf(role);
    // Each role has one slot, and the route has to visit the slots left to right.
    if (index <= last) return { ok: false, reason: "roles" };
    if (index === 2) third = role as ThirdRole;
    slots[index] = id;
    last = index;
  }
  return { ok: true, slots, third };
}

/** The first slot that keeps the workflow from running: the orchestrator and implementer are required. */
export function missingSlot(slots: SlotIds): 0 | 1 | null {
  if (!slots.some(Boolean)) return null;
  return !slots[0] ? 0 : !slots[1] ? 1 : null;
}

/**
 * Puts a step in a slot. A step coming from another slot swaps places with the
 * one there; a step from outside replaces it.
 */
export function placeInSlot(slots: SlotIds, stepId: string, target: number): SlotIds {
  const next = [...slots];
  const from = next.indexOf(stepId);
  if (from === target) return next;
  if (from >= 0) next[from] = next[target];
  next[target] = stepId;
  return next;
}

export function clearSlot(slots: SlotIds, index: number): SlotIds {
  return slots.map((id, position) => (position === index ? null : id));
}

/**
 * Rebuilds the group from the slots: the filled ones in order, each with its
 * slot's role, connected one after another. Steps left out of the slots leave
 * the workflow. Each arrow keeps the settings of the one its source step already
 * had, so moving cards around does not reset what passes between them.
 * `contracts` refreshes the role fields that still follow the previous role's
 * defaults (`preserveRoleOverrides`).
 */
export function arrangeSlots(
  group: WorkflowGroupDefinition,
  slots: SlotIds,
  third: ThirdRole,
  contracts: Partial<Record<WorkflowRole, WorkflowRoleContract>> = {},
  extra: WorkflowStepDefinition[] = [],
): WorkflowGroupDefinition {
  const pool = [...group.steps, ...extra.filter((step) => !group.steps.some((item) => item.id === step.id))];
  const steps: WorkflowStepDefinition[] = [];
  slots.forEach((id, index) => {
    const step = id ? pool.find((item) => item.id === id) : undefined;
    if (!step || steps.some((item) => item.id === step.id)) return;
    const role = slotRole(index, third);
    if (step.role === role) { steps.push(step); return; }
    const previous = contracts[step.role];
    const next = contracts[role];
    steps.push({ ...step, role, customRoleLabel: "", ...(previous && next ? preserveRoleOverrides(step, previous, next) : {}) });
  });
  const connections = steps.slice(1).map((step, index) => {
    const from = steps[index].id;
    const kept = group.connections.find((item) => item.fromStepId === from && item.toStepId === step.id)
      ?? group.connections.find((item) => item.fromStepId === from);
    return kept ? { ...kept, toStepId: step.id } : newConnection(from, step.id);
  });
  return { ...group, steps, connections };
}
