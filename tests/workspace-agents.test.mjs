import assert from "node:assert/strict";
import { subagentsForSession } from "../src/lib/workspaceAgents.ts";

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

const codex = subagentsForSession({
  status: "running",
  activities: [
    activity("codex:thread:item-1", "Subagente · /root/reviewer", "running", 1),
    activity("codex:thread:item-2", "Subagente · /root/reviewer", "completed", 2),
  ],
});
assert.deepEqual(codex.map(({ label, status }) => [label, status]), [["reviewer", "completed"]]);

console.log("workspace agent tree tests passed");
