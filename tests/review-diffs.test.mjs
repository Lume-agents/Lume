import assert from "node:assert/strict";
import { buildReviewTurns, collectReviewFiles, parseReviewDiff } from "../src/lib/reviewDiffs.ts";

const detail = `diff --git a/src/example.ts b/src/example.ts
index 111..222 100644
--- a/src/example.ts
+++ b/src/example.ts
@@ -1,2 +1,2 @@
-const oldValue = 1;
+const newValue = 2;
 keep();`;

const files = collectReviewFiles([{
  id: "diff-1",
  kind: "file",
  title: "Files changed",
  detail,
  status: "completed",
  createdAt: 20,
  files: ["src/example.ts"],
}], "/work/project", 10);

assert.deepEqual(files.map(({ path, added, removed }) => ({ path, added, removed })), [
  { path: "src/example.ts", added: 1, removed: 1 },
]);
assert.equal(files[0].diff, detail);

const subagentPatch = `diff --git a/src/new.ts b/src/new.ts\n+export const ready = true;`;
const subagentFiles = collectReviewFiles([{
  id: "subagent-patch", kind: "file", title: "File changed", detail: subagentPatch,
  status: "completed", createdAt: 21, files: ["src/new.ts"],
}], "/work/project");
assert.deepEqual(subagentFiles.map(({ path, added, removed }) => ({ path, added, removed })), [
  { path: "src/new.ts", added: 1, removed: 0 },
]);
assert.equal(subagentFiles[0].diff, subagentPatch);

const lines = parseReviewDiff(detail);
assert.ok(lines.some((line) => line.kind === "removed" && line.oldLine === 1));
assert.ok(lines.some((line) => line.kind === "added" && line.newLine === 1));
assert.ok(lines.some((line) => line.kind === "context" && line.oldLine === 2 && line.newLine === 2));

const activity = (id, kind, createdAt, detail = "", files = []) => ({
  id, kind, createdAt, detail, files, title: id, status: "completed",
});
const result = (id, createdAt, response, tests = []) => ({
  id, createdAt, response, tests, files: [],
});
const turns = buildReviewTurns([
  activity("prompt-one", "prompt", 100, "First request"),
  activity("file-one", "file", 110, detail, ["src/example.ts"]),
  activity("check-one", "test", 115, "First check passed"),
  activity("prompt-two", "prompt", 200, "Second request"),
  activity("file-two", "file", 210, detail.replaceAll("example.ts", "second.ts"), ["src/second.ts"]),
], [
  result("result-one", 130, "First response", ["First check passed"]),
  result("result-two", 230, "Second response"),
], "/work/project");

assert.deepEqual(turns.map((turn) => [turn.prompt?.id, turn.result?.id]), [
  ["prompt-two", "result-two"],
  ["prompt-one", "result-one"],
]);
assert.deepEqual(turns[0].files.map((file) => file.path), ["src/second.ts"]);
assert.deepEqual(turns[1].files.map((file) => file.path), ["src/example.ts"]);
assert.deepEqual(turns[0].checks, []);
assert.deepEqual(turns[1].checks, ["First check passed"]);

const partialHistory = buildReviewTurns([
  activity("prompt-current", "prompt", 400, "Still working"),
], [result("archived", 300, "Earlier result")]);
assert.equal(partialHistory[0].result, undefined);
assert.equal(partialHistory[1].activityAvailable, false);
assert.deepEqual(partialHistory[1].files, []);

const revisedResult = buildReviewTurns([
  activity("prompt-revised", "prompt", 500, "One task"),
], [
  result("draft-result", 510, "Draft"),
  result("final-result", 520, "Final"),
]);
assert.deepEqual(revisedResult.map((turn) => turn.result?.id), ["final-result"]);

console.log("review diff test suite passed");
