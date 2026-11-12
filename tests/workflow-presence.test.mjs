import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import test from "node:test";

registerHooks({
  resolve(specifier, context, nextResolve) {
    if (specifier.startsWith("$lib/")) {
      const path = specifier.slice(5);
      return nextResolve(new URL(`../src/lib/${path}${/\.[jt]s$/.test(path) ? "" : ".ts"}`, import.meta.url).href, context);
    }
    return nextResolve(specifier, context);
  },
});
const { chainGroupFromSessions, newerRun, presenceTone, sessionWorkflowPresence, boardLayoutNames } = await import("../src/lib/workflowPresence.ts");
const { analyzeChain } = await import("../src/lib/workflowBoard.ts");

const contract = (role) => ({ instruction: "Do " + role, expectedInput: "Input", producedOutput: "Output", completionCondition: "Done" });
const contracts = Object.fromEntries(["planner", "implementer", "reviewer", "tester", "researcher", "custom"].map((role) => [role, contract(role)]));
const run = (group, overrides = {}) => ({
  id: "run-1", workflowId: group.id, objective: "Ship it", status: "running", handoffApproved: false, recovering: false, transitionCount: 0,
  steps: group.steps.map((step) => ({ stepId: step.id, status: "pending", attempt: 0 })), createdAt: 10, updatedAt: 10, ...overrides,
});

test("chained chats form a group the runtime can run, skipping unconnected and repeated chats", () => {
  const { group, added } = chainGroupFromSessions([
    { nativeSessionId: "a" }, { nativeSessionId: "" }, { nativeSessionId: "b" }, { nativeSessionId: "a" }, { nativeSessionId: "c" },
  ], contracts);
  assert.deepEqual(added, ["a", "b", "c"]);
  assert.deepEqual(analyzeChain(group).problems, []);
  assert.deepEqual(group.steps.map((step) => step.role), ["planner", "implementer", "reviewer"]);
  assert.deepEqual(analyzeChain(group).order, ["step-a", "step-b", "step-c"]);
});

test("adding a chat to a group appends it after the end of the chain", () => {
  const first = chainGroupFromSessions([{ nativeSessionId: "a" }, { nativeSessionId: "b" }], contracts).group;
  const { group, added } = chainGroupFromSessions([{ nativeSessionId: "c" }, { nativeSessionId: "a" }], contracts, first);
  assert.deepEqual(added, ["c"]);
  assert.equal(group.id, first.id);
  assert.deepEqual(analyzeChain(group).order, ["step-a", "step-b", "step-c"]);
  assert.deepEqual(analyzeChain(group).problems, []);
});

test("past the built-in roles a chained chat gets a named custom role", () => {
  const { group } = chainGroupFromSessions(["a", "b", "c", "d", "e", "f"].map((id) => ({ nativeSessionId: id })), contracts);
  assert.equal(group.steps[5].role, "custom");
  assert.deepEqual(analyzeChain(group).problems, []);
});

test("each chat knows its step, position and run state", () => {
  const { group } = chainGroupFromSessions(["a", "b", "c"].map((id) => ({ nativeSessionId: id })), contracts);
  const idle = sessionWorkflowPresence([group], {}, { [group.id]: "Release" });
  assert.equal(idle.get("b")[0].name, "Release");
  assert.equal(idle.get("b")[0].index, 2);
  assert.equal(idle.get("b")[0].total, 3);
  assert.equal(idle.get("b")[0].previousRoleLabel, "Planner");
  assert.equal(idle.get("b")[0].nextRoleLabel, "Reviewer");
  assert.equal(presenceTone(idle.get("b")[0]), "idle");
  assert.equal(idle.get("a")[0].runActive, false);

  const running = run(group, { currentStepId: "step-a", steps: group.steps.map((step, index) => ({ stepId: step.id, status: index ? "pending" : "running", attempt: 1 })) });
  const live = sessionWorkflowPresence([group], { [group.id]: running }, {}, true);
  assert.equal(live.get("a")[0].isCurrent, true);
  assert.equal(live.get("a")[0].roleLabel, "Planejador");
  assert.equal(live.get("a")[0].name, "Workflow 1");
  assert.equal(presenceTone(live.get("a")[0]), "running");
  assert.equal(presenceTone(live.get("b")[0]), "idle");
});

test("a handoff waiting for approval marks the step that sends and the one that receives", () => {
  const { group } = chainGroupFromSessions(["a", "b"].map((id) => ({ nativeSessionId: id })), contracts);
  const waiting = run(group, { status: "waiting_for_approval", currentStepId: "step-a", pendingConnectionId: group.connections[0].id,
    steps: [{ stepId: "step-a", status: "completed", attempt: 1 }, { stepId: "step-b", status: "pending", attempt: 0 }] });
  const presence = sessionWorkflowPresence([group], { [group.id]: waiting });
  assert.equal(presence.get("a")[0].outgoingPending, true);
  assert.equal(presence.get("a")[0].incomingPending, false);
  assert.equal(presence.get("b")[0].incomingPending, true);
  assert.equal(presence.get("b")[0].outgoingPending, false);
  assert.equal(presenceTone(presence.get("a")[0]), "approval");

  const ready = sessionWorkflowPresence([group], { [group.id]: { ...waiting, status: "ready" } });
  assert.equal(ready.get("a")[0].outgoingPending, false);
  assert.equal(ready.get("a")[0].outgoingReady, true);
  assert.equal(ready.get("b")[0].incomingReady, true);
  assert.equal(presenceTone(ready.get("b")[0]), "ready");

  const done = sessionWorkflowPresence([group], { [group.id]: { ...waiting, status: "completed", pendingConnectionId: undefined, steps: waiting.steps.map((step) => ({ ...step, status: "completed" })) } });
  assert.equal(done.get("b")[0].runActive, false);
  assert.equal(presenceTone(done.get("b")[0]), "done");
});

test("a chat in several workflows lists the one with an active run first", () => {
  const idleGroup = chainGroupFromSessions(["a", "b"].map((id) => ({ nativeSessionId: id })), contracts).group;
  const busyGroup = chainGroupFromSessions(["c", "a"].map((id) => ({ nativeSessionId: id })), contracts).group;
  const presence = sessionWorkflowPresence([idleGroup, busyGroup], { [busyGroup.id]: run(busyGroup, { currentStepId: "step-c" }) });
  assert.deepEqual(presence.get("a").map((item) => item.workflowId), [busyGroup.id, idleGroup.id]);
});

test("the latest run wins regardless of the order events arrive in", () => {
  const base = { id: "run-1", createdAt: 10, updatedAt: 20 };
  assert.equal(newerRun(null, base), true);
  assert.equal(newerRun(base, { ...base, updatedAt: 21 }), true);
  assert.equal(newerRun(base, { ...base, updatedAt: 19 }), false);
  assert.equal(newerRun(base, { id: "run-2", createdAt: 11, updatedAt: 11 }), true);
  assert.equal(newerRun(base, { id: "run-0", createdAt: 9, updatedAt: 99 }), false);
});

test("board names are read defensively from local storage", () => {
  const store = new Map();
  globalThis.localStorage = { getItem: (key) => store.get(key) ?? null, setItem: (key, value) => store.set(key, value) };
  assert.deepEqual(boardLayoutNames(), {});
  store.set("lume:workflow-board:v1", "{not json");
  assert.deepEqual(boardLayoutNames(), {});
  store.set("lume:workflow-board:v1", JSON.stringify({ layouts: { one: { name: " Release " }, two: { positions: {} } } }));
  assert.deepEqual(boardLayoutNames(), { one: "Release" });
  delete globalThis.localStorage;
});
