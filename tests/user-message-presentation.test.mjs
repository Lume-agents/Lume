import assert from "node:assert/strict";
import { parseWorkflowPrompt, userMessagePresentation } from "../src/lib/userMessagePresentation.ts";

assert.deepEqual(userMessagePresentation("Short prompt"), {
  kind: "full",
  preview: "Short prompt",
  characterCount: 12,
});

const longMessage = "A long readable message ".repeat(50);
const longPresentation = userMessagePresentation(longMessage);
assert.equal(longPresentation.kind, "clamped");
assert.ok(longPresentation.preview.length < longMessage.length);
assert.ok(longPresentation.preview.endsWith("…"));

const multilinePaste = Array.from({ length: 80 }, (_, index) => `const line${index} = ${index};`).join("\n");
assert.equal(userMessagePresentation(multilinePaste.repeat(2)).kind, "pasted");
assert.equal(userMessagePresentation("x".repeat(6_000)).kind, "pasted");

console.log("user message presentation test suite passed");

assert.equal(parseWorkflowPrompt("Please fix the build"), null);
assert.deepEqual(
  parseWorkflowPrompt("# Lume workflow\n\n- Workflow: `w1`\n- Role: **Planner**\n\n## Objective\n\nShip the board\n\n## Instructions\n\nPlan it"),
  { kind: "start", role: "Planner", objective: "Ship the board" },
);
assert.deepEqual(
  parseWorkflowPrompt("# Lume workflow context\n\n- Workflow: `w1`\n- Transition: **Planner** → **Implementer**\n- Policy: `Default`\n\n## Objective\n\nShip the board\n\n## Source result\n\nDone"),
  { kind: "handoff", from: "Planner", to: "Implementer", objective: "Ship the board" },
);
assert.deepEqual(
  parseWorkflowPrompt("# Lume workflow\n\nThe previous **Tester** step was skipped by the user. Continue without its result.\n\n## Objective\n\nShip it\n\n## Instructions\n\nGo"),
  { kind: "skipped", role: "Tester", objective: "Ship it" },
);
console.log("Workflow prompt presentation tests passed");
