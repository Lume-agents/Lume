import assert from "node:assert/strict";
import test from "node:test";
import { buildWorkflowTimeline, timelineTicks } from "../src/lib/workflowTimeline.ts";

const step = (id) => ({ id, sessionNativeId: `native-${id}`, role: "implementer", customRoleLabel: "", attempt: 0, instruction: "x", expectedInput: "x", producedOutput: "x", completionCondition: "x" });
const connection = (from, to, extra = {}) => ({ id: `c-${from}-${to}`, fromStepId: from, toStepId: to, advanceMode: "automatic", requiresApproval: false, ...extra });
const activity = (id, kind, createdAt, extra = {}) => ({ id, kind, title: id, status: "completed", createdAt, files: [], ...extra });
const steps = [step("plan"), step("build"), step("review")];
const order = ["plan", "build", "review"];

test("a finished run lays out each step's work and the handoffs between them", () => {
  const run = { id: "r", workflowId: "g", objective: "", status: "completed", handoffApproved: false, recovering: false, transitionCount: 2, createdAt: 0, updatedAt: 30_000,
    steps: [
      { stepId: "plan", status: "completed", attempt: 1, startedAt: 0, completedAt: 8_000 },
      { stepId: "build", status: "completed", attempt: 2, startedAt: 12_000, completedAt: 24_000 },
      { stepId: "review", status: "completed", attempt: 1, startedAt: 24_500, completedAt: 30_000 },
    ] };
  const activities = {
    plan: [activity("p1", "command", 1_000), activity("p2", "message", 5_000)],
    build: [activity("b1", "file", 13_000), activity("b2", "test", 18_000, { status: "failed" }), activity("b3", "message", 22_000)],
    review: [],
  };
  const model = buildWorkflowTimeline({ run, steps, order, connections: [connection("plan", "build", { requiresApproval: true }), connection("build", "review")], activitiesFor: (item) => activities[item.id], now: 99_999 });
  assert.equal(model.live, false);
  assert.deepEqual([model.start, model.end], [0, 30_000]);
  assert.deepEqual(model.rows.map((row) => row.stepId), order);
  const plan = model.spans.filter((span) => span.rowId === "row:plan");
  assert.deepEqual(plan.map((span) => [span.kind, span.start, span.end]), [["thinking", 0, 1_000], ["tool", 1_000, 5_000], ["thinking", 5_000, 8_000]]);
  // The build step waited four seconds for the approval of the plan's handoff.
  const wait = model.spans.find((span) => span.rowId === "row:build" && span.kind === "waiting");
  assert.deepEqual([wait.start, wait.end, wait.label], [8_000, 12_000, "approval"]);
  assert.ok(model.spans.some((span) => span.id === "row:build:b2" && span.failed));
  assert.deepEqual(model.handoffs.map((handoff) => [handoff.from, handoff.to, handoff.at, handoff.approval, handoff.done]), [
    ["row:plan", "row:build", 8_000, true, true],
    ["row:build", "row:review", 24_000, false, true],
  ]);
  assert.equal(model.retries, 1);
  assert.equal(model.workMs, 8_000 + 12_000 + 5_500);
});

test("a live run grows its open span to now and shows a handoff still waiting", () => {
  const run = { id: "r", workflowId: "g", objective: "", status: "waiting_for_approval", handoffApproved: false, recovering: false, transitionCount: 1, createdAt: 0, updatedAt: 9_000,
    steps: [{ stepId: "plan", status: "completed", attempt: 1, startedAt: 0, completedAt: 6_000 }, { stepId: "build", status: "pending", attempt: 0 }] };
  const model = buildWorkflowTimeline({ run, steps, order, connections: [connection("plan", "build", { requiresApproval: true })], activitiesFor: () => [], now: 15_000 });
  assert.equal(model.live, true);
  assert.equal(model.end, 15_000);
  const wait = model.spans.find((span) => span.kind === "waiting");
  assert.deepEqual([wait.rowId, wait.start, wait.end, wait.open], ["row:build", 6_000, 15_000, true]);
  assert.equal(model.handoffs[0].done, false);
});

test("no run, no timeline; ticks stay round", () => {
  assert.equal(buildWorkflowTimeline({ run: null, steps, order, connections: [], activitiesFor: () => [], now: 0 }), null);
  assert.deepEqual(timelineTicks(26_400), [0, 10_000, 20_000]);
  assert.deepEqual(timelineTicks(4 * 60_000).slice(0, 3), [0, 60_000, 120_000]);
});
