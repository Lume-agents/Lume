import assert from "node:assert/strict";
import { buildLatestReviewTurn, buildReviewTurns, collectReviewFiles, fileDiffChunk, parseReviewDiff } from "../src/lib/reviewDiffs.ts";

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


// Claude's edit tools are recorded as a diff on an absolute path; it is shown relative to the project.
const claudeDiff = "diff --git a//work/project/src/a.ts b//work/project/src/a.ts\n--- a//work/project/src/a.ts\n+++ b//work/project/src/a.ts\n@@ -4,2 +4,2 @@\n keep\n-old\n+new\n";
const claudeFiles = collectReviewFiles([{ id: "claude-edit", kind: "file", title: "/work/project/src/a.ts", detail: claudeDiff, status: "completed", createdAt: 30, files: ["/work/project/src/a.ts"] }], "/work/project");
assert.deepEqual(claudeFiles.map(({ path, added, removed }) => ({ path, added, removed })), [{ path: "src/a.ts", added: 1, removed: 1 }]);
assert.ok(parseReviewDiff(claudeFiles[0].diff).some((line) => line.kind === "added" && line.newLine === 5), "line numbers follow the hunk header");

const lines = parseReviewDiff(detail);
assert.ok(lines.some((line) => line.kind === "removed" && line.oldLine === 1));
assert.ok(lines.some((line) => line.kind === "added" && line.newLine === 1));
assert.ok(lines.some((line) => line.kind === "context" && line.oldLine === 2 && line.newLine === 2));

const activity = (id, kind, createdAt, detail = "", files = []) => ({
  id, kind, createdAt, detail, files, title: kind === "file" ? "Files changed" : id, status: "completed",
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
assert.deepEqual(buildLatestReviewTurn([
  activity("prompt-one", "prompt", 100, "First request"),
  activity("file-one", "file", 110, detail, ["src/example.ts"]),
  activity("check-one", "test", 115, "First check passed"),
  activity("prompt-two", "prompt", 200, "Second request"),
  activity("file-two", "file", 210, detail.replaceAll("example.ts", "second.ts"), ["src/second.ts"]),
], [
  result("result-one", 130, "First response", ["First check passed"]),
  result("result-two", 230, "Second response"),
], "/work/project"), turns[0]);

const partialHistory = buildReviewTurns([
  activity("prompt-current", "prompt", 400, "Still working"),
], [result("archived", 300, "Earlier result")]);
assert.equal(partialHistory[0].result, undefined);
assert.equal(partialHistory[1].activityAvailable, false);
assert.deepEqual(partialHistory[1].files, []);
assert.deepEqual(buildLatestReviewTurn([
  activity("prompt-current", "prompt", 400, "Still working"),
], [result("archived", 300, "Earlier result")]), partialHistory[0]);
const equalTimeResults = [result("first", 300, "First"), result("second", 300, "Second")];
assert.deepEqual(buildLatestReviewTurn([], equalTimeResults), buildReviewTurns([], equalTimeResults)[0]);

const revisedResult = buildReviewTurns([
  activity("prompt-revised", "prompt", 500, "One task"),
], [
  result("draft-result", 510, "Draft"),
  result("final-result", 520, "Final"),
]);
assert.deepEqual(revisedResult.map((turn) => turn.result?.id), ["final-result"]);
assert.deepEqual(buildLatestReviewTurn([
  activity("prompt-revised", "prompt", 500, "One task"),
], [
  result("draft-result", 510, "Draft"),
  result("final-result", 520, "Final"),
]), revisedResult[0]);

// Windows paths ignore case: the same file reported absolute with another drive/folder casing is one file.
const windowsFiles = collectReviewFiles([{
  id: "win-1",
  kind: "file",
  title: "Files changed",
  detail: "",
  status: "completed",
  createdAt: 30,
  files: [
    "src-tauri/src/workspace_windows.rs",
    "C:\\Users\\Dev\\Lume\\src-tauri\\src\\workspace_windows.rs",
    "\\\\?\\c:\\users\\dev\\lume\\src-tauri\\src\\workspace_windows.rs",
  ],
}], "c:\\Users\\Dev\\Lume");
assert.deepEqual(windowsFiles.map((file) => file.path), ["src-tauri/src/workspace_windows.rs"]);

console.log("review diff test suite passed");

// Claude Code edits a file with several tool calls; each one is its own change and none replaces another.
const claudeEdit = (id, createdAt, hunk) => ({
  id: `claude:s1:tool:${id}`, kind: "file", title: "/work/project/src/b.ts", status: "completed", createdAt, files: ["/work/project/src/b.ts"],
  detail: `diff --git a//work/project/src/b.ts b//work/project/src/b.ts\n--- a//work/project/src/b.ts\n+++ b//work/project/src/b.ts\n${hunk}`,
});
const firstEdit = claudeEdit("toolu_1", 40, "@@ -1,2 +1,3 @@\n keep\n-old one\n+new one\n+extra one\n");
const secondEdit = claudeEdit("toolu_2", 41, "@@ -10,2 +11,2 @@\n keep\n-old two\n+new two\n");
const editedTwice = collectReviewFiles([firstEdit, secondEdit], "/work/project");
assert.deepEqual(editedTwice.map(({ path, added, removed }) => ({ path, added, removed })), [{ path: "src/b.ts", added: 3, removed: 2 }]);
assert.ok(editedTwice[0].diff.includes("new one") && editedTwice[0].diff.includes("new two"), "the diff keeps every edit");
// The same edit reported again (a retry of the hook) is not counted twice.
assert.deepEqual(collectReviewFiles([firstEdit, { ...firstEdit, id: "claude:s1:tool:toolu_1b" }], "/work/project").map(({ added, removed }) => ({ added, removed })), [{ added: 2, removed: 1 }]);

// Lines whose content starts with "--" or "++" are changes, not file headers.
const contentLines = "diff --git a/db/query.sql b/db/query.sql\n--- a/db/query.sql\n+++ b/db/query.sql\n@@ -1,3 +1,3 @@\n--- a comment\n+-- another comment\n ++i;\n-----\n+---\n";
const sqlFiles = collectReviewFiles([{ id: "sql", kind: "file", title: "db/query.sql", detail: contentLines, status: "completed", createdAt: 50, files: ["db/query.sql"] }], "/work/project");
assert.deepEqual(sqlFiles.map(({ added, removed }) => ({ added, removed })), [{ added: 2, removed: 2 }]);

// The second edit's hunk keeps its own line numbers inside the combined diff.
const combined = parseReviewDiff(editedTwice[0].diff);
assert.ok(combined.some((line) => line.kind === "added" && line.content === "new two" && line.newLine === 12), "line numbers of a later edit");
console.log("multi-edit review tests passed");

const shown = parseReviewDiff(contentLines);
assert.deepEqual(shown.filter((line) => line.kind === "removed").map((line) => line.content), ["-- a comment", "----"]);
assert.deepEqual(shown.filter((line) => line.kind === "added").map((line) => line.content), ["-- another comment", "---"]);
console.log("content lines tests passed");

// A file's diff is found whether the event names it relative or absolute.
{
  const detail = [
    "diff --git a/src/a.ts b/src/a.ts\n--- a/src/a.ts\n+++ b/src/a.ts\n@@ -1 +1 @@\n-a\n+A",
    "diff --git a/src/b.ts b/src/b.ts\n--- a/src/b.ts\n+++ /dev/null\n@@ -1 +0,0 @@\n-b",
  ].join("\n");
  assert.match(fileDiffChunk(detail, "/work/app/src/a.ts"), /\+A/);
  assert.match(fileDiffChunk(detail, "src/b.ts"), /-b/);
  assert.equal(fileDiffChunk(detail, "/work/app/src/c.ts"), null);
}
