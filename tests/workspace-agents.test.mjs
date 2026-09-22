import assert from "node:assert/strict";
import { noteSubagentInteraction, parentWaitingForSubagents, subagentsForSession } from "../src/lib/workspaceAgents.ts";

const activity = (id, title, status, createdAt) => ({ id, kind: "subagent", title, status, createdAt, files: [] });
const claude = subagentsForSession({
  status: "running",
  activities: [
    activity("claude:thread:subagent:worker-1", "Researcher", "running", 1),
    activity("claude:thread:subagent:worker-1", "Researcher", "completed", 2),
    { ...activity("tool:unrelated", "command", "running", 3), kind: "tool" },
  ],
});
assert.equal(claude.length, 1);
assert.equal(claude[0].status, "completed");
assert.equal(claude[0].startedAt, 1);

const codex = subagentsForSession({
  status: "running",
  activities: [
    activity("codex:thread:item-1", "Subagente · /root/reviewer", "running", 1),
    activity("codex:thread:item-1", "Subagente · /root/reviewer", "completed", 2),
    activity("codex:thread:item-2", "Subagente · /root/reviewer", "running", 3),
  ],
});
assert.deepEqual(codex.map(({ label, status }) => [label, status]), [["reviewer", "completed"], ["reviewer", "running"]]);
assert.notEqual(codex[0].id, codex[1].id);

const prompt = (id, createdAt) => ({ id, kind: "prompt", title: "Prompt enviado", status: "completed", createdAt, files: [] });
const justCompleted = subagentsForSession({
  status: "completed",
  results: [{ id: "result-just-completed", response: "Done", createdAt: 5, files: [], tests: [] }],
  activities: [prompt("launch-prompt", 0), activity("child-just-completed", "Subagente · /root/reviewer", "completed", 1)],
});
assert.deepEqual(justCompleted.map(({ id }) => id), ["child-just-completed"]);
const recent = subagentsForSession({
  status: "running",
  results: [{ id: "result-1", response: "Done", createdAt: 5, files: [], tests: [] }],
  activities: [
    activity("child-completed", "Subagente · /root/reviewer", "completed", 1),
    activity("child-running", "Subagente · /root/tester", "running", 1),
    prompt("prompt-1", 6),
    prompt("prompt-2", 7),
  ],
});
assert.deepEqual(recent.map(({ id }) => id), ["child-completed", "child-running"]);
const afterFivePrompts = subagentsForSession({
  status: "running",
  results: [{ id: "result-1", response: "Done", createdAt: 5, files: [], tests: [] }],
  activities: [...recent.map((child) => activity(child.id, `Subagente · /root/${child.label}`, child.status, 1)),
    ...[6, 7, 8, 9, 10].map((createdAt) => prompt(`prompt-${createdAt}`, createdAt))],
});
assert.deepEqual(afterFivePrompts.map(({ id }) => id), ["child-running"]);

noteSubagentInteraction("child-completed", 10);
const afterInteraction = subagentsForSession({
  status: "completed",
  results: [{ id: "result-1", response: "Done", createdAt: 5, files: [], tests: [] }],
  activities: [activity("child-completed", "Subagente · /root/reviewer", "completed", 1),
    ...[6, 7, 8, 9, 10, 11, 12, 13, 14].map((createdAt) => prompt(`prompt-${createdAt}`, createdAt))],
});
assert.deepEqual(afterInteraction.map(({ id }) => id), ["child-completed"]);
assert.deepEqual(subagentsForSession({
  status: "completed",
  results: [{ id: "result-1", response: "Done", createdAt: 5, files: [], tests: [] }],
  activities: [...afterInteraction.map((child) => activity(child.id, `Subagente · /root/${child.label}`, child.status, 1)),
    ...[11, 12, 13, 14, 15].map((createdAt) => prompt(`prompt-${createdAt}`, createdAt))],
}).map(({ id }) => id), []);

const unfinishedParent = subagentsForSession({
  status: "running",
  activities: [activity("child-unfinished", "Subagente · /root/reviewer", "completed", 1),
    ...[2, 3, 4, 5, 6, 7].map((createdAt) => prompt(`prompt-${createdAt}`, createdAt))],
});
assert.deepEqual(unfinishedParent.map(({ id }) => id), ["child-unfinished"]);

const waitingParent = {
  status: "running",
  activities: [
    { id: "parent-tool", kind: "tool", title: "Subagente · spawn", status: "running", createdAt: 1, files: [] },
    activity("child-1", "Subagente · /root/reviewer", "running", 2),
    { id: "wait", kind: "tool", title: "functions · wait_agent", status: "running", createdAt: 3, files: [] },
  ],
};
assert.equal(parentWaitingForSubagents(waitingParent), true);
assert.equal(parentWaitingForSubagents({ ...waitingParent, activities: [
  ...waitingParent.activities,
  { id: "parent-work", kind: "command", title: "cargo test", status: "running", createdAt: 4, files: [] },
] }), false);
assert.equal(parentWaitingForSubagents({ ...waitingParent, status: "completed" }), false);

console.log("workspace agent tree tests passed");
