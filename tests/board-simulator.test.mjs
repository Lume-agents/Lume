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
const { demoFrames, demoSession } = await import("../src/lib/boardSimulator.ts");
const { stepActivity } = await import("../src/lib/boardActivity.ts");

test("the script walks every step from running to completed, in order", () => {
  const frames = demoFrames(3);
  assert.ok(frames.every((frame) => frame.length === 3));
  assert.deepEqual(frames[0].map((cell) => cell.state), ["running", "pending", "pending"]);
  const lastBeforeEnd = frames[frames.length - 2];
  assert.ok(lastBeforeEnd.every((cell) => cell.state === "completed"));
  assert.equal(frames[frames.length - 1][2].state, "failed", "the run ends with a failure to preview that state");
  for (const frame of frames) {
    const running = frame.findIndex((cell) => cell.state === "running");
    if (running > 0) assert.ok(frame.slice(0, running).every((cell) => cell.state === "completed"), "earlier steps are done while a later one runs");
  }
});

test("steps ask for permission and the last one asks a question", () => {
  const frames = demoFrames(3).flat();
  assert.ok(frames.some((cell) => cell.attention === "permission"));
  assert.ok(frames.some((cell) => cell.attention === "question"));
  assert.equal(demoFrames(1).flat().some((cell) => cell.attention !== "none"), false, "a single step never waits on anyone");
});

test("a simulated running agent reads like a real one to the cards", () => {
  const base = { id: "a", status: "waiting_for_input", statusLabel: "x", activities: [], results: [], workSummary: {} };
  const running = demoSession(base, { state: "running", attention: "none", progress: 1 }, 1_000);
  const info = stepActivity(running, { stepId: "a", status: "running", attempt: 1, startedAt: 1_000 }, "en", 62_000);
  assert.equal(info.line, "Editing Navigation.svelte +1");
  assert.deepEqual(info.progress, { done: 1, total: 4 });
  assert.equal(info.elapsed, "1m 1s");
  const waiting = stepActivity(demoSession(base, { state: "running", attention: "permission", progress: 2 }, 1_000), undefined, "en", 0);
  assert.equal(waiting.attention.kind, "permission");
  const done = stepActivity(demoSession(base, { state: "completed", attention: "none", progress: 4 }, 1_000), undefined, "en", 0);
  assert.equal(done.line, "2 files changed");
  assert.equal(demoSession(undefined, { state: "running", attention: "none", progress: 0 }, 0), undefined);
});
