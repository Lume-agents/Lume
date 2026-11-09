import assert from "node:assert/strict";
import { sessionShells, shellElapsed } from "../src/lib/sessionShells.ts";

const now = 1_800_000_000_000;
const shell = (id, status, ago, title = id, detail = `run ${id}`) => ({ id, kind: "shell", title, detail, status, createdAt: now - ago, files: [], attachments: [] });
const activities = [
  shell("dev", "running", 60_000, "Start the dev server"),
  shell("old", "completed", 30 * 60_000),
  shell("tests", "completed", 2 * 60_000),
  shell("watch", "running", 10_000),
  { id: "cmd", kind: "command", title: "ls", status: "completed", createdAt: now, files: [], attachments: [] },
];

// Running shells first, newest first; a shell that ended long ago is dropped; other activities are not shells.
assert.deepEqual(sessionShells({ status: "running", activities }, now).map((item) => [item.id, item.status]), [
  ["watch", "running"], ["dev", "running"], ["tests", "completed"],
]);
assert.equal(sessionShells({ status: "running", activities }, now)[1].title, "Start the dev server");
// A session that ended has nothing running anymore.
assert.deepEqual(sessionShells({ status: "completed", activities }, now).map((item) => item.id), ["tests"]);
assert.equal(shellElapsed({ startedAt: now - 3_725_000 }, now), "1h 2m");
console.log("session shells test passed");
