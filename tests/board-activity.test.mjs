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
const { currentActivityLine, stepActivity, stepElapsed, stepProgress } = await import("../src/lib/boardActivity.ts");

const activity = (kind, overrides = {}) => ({ id: kind + Math.random(), kind, title: kind, status: "completed", createdAt: 1, files: [], ...overrides });
const session = (overrides = {}) => ({ status: "running", activities: [], results: [], workSummary: {}, ...overrides });

test("a running agent shows its live tool, never the conversation", () => {
  const live = session({ activities: [
    activity("prompt", { detail: "Do it" }),
    activity("file", { files: ["src/lib/Navigation.svelte", "src/routes/+layout.svelte"], status: "running" }),
    activity("message", { detail: "Working on it" }),
  ] });
  assert.equal(currentActivityLine(live, "en"), "Editing Navigation.svelte +1");
  assert.equal(currentActivityLine(live, "pt-BR"), "Editando Navigation.svelte +1");
});

test("commands are shown as a short shell line", () => {
  const live = session({ activities: [activity("command", { detail: "npm run check -- --watch --verbose --very-long-flag", status: "running" })] });
  const line = currentActivityLine(live, "en");
  assert.match(line, /^\$ npm run check/);
  assert.ok(line.length <= 36);
});

test("a running agent with no tool yet is thinking", () => {
  assert.equal(currentActivityLine(session(), "en"), "Thinking");
});

test("an idle agent reports the files its last turn changed", () => {
  const done = session({ status: "waiting_for_input", results: [{ id: "r", response: "x", createdAt: 1, files: ["a.ts", "b.ts", "a.ts"], tests: [] }] });
  assert.equal(currentActivityLine(done, "en"), "2 files changed");
  assert.equal(currentActivityLine(session({ status: "waiting_for_input" }), "en"), null);
});

test("plan progress prefers the to-do list over the plan", () => {
  const items = (statuses) => statuses.map((status, index) => ({ label: "i" + index, status }));
  assert.deepEqual(stepProgress(session({ workSummary: { todo: { items: items(["completed", "in_progress", "pending"]), updatedAt: 1 }, plan: { items: items(["completed"]), updatedAt: 1 } } })), { done: 1, total: 3 });
  assert.deepEqual(stepProgress(session({ workSummary: { plan: { items: items(["completed", "completed"]), updatedAt: 1 } } })), { done: 2, total: 2 });
  assert.equal(stepProgress(session()), null);
});

test("elapsed time counts up while running and freezes when finished", () => {
  assert.equal(stepElapsed({ stepId: "s", status: "running", attempt: 1, startedAt: 1_000 }, 66_000), "1m 5s");
  assert.equal(stepElapsed({ stepId: "s", status: "completed", attempt: 1, startedAt: 1_000, completedAt: 31_000 }, 99_000), "30s");
  assert.equal(stepElapsed({ stepId: "s", status: "pending", attempt: 0 }, 5_000), null);
});

test("a pending permission or question outranks the activity line", () => {
  const permission = stepActivity(session({ pendingPermission: { id: "p", kind: "command", summary: "Run the tests", resource: "", risk: "low", requestedAt: "" }, activities: [activity("command", { status: "running" })] }), undefined, "en", 0);
  assert.deepEqual(permission.attention, { kind: "permission", text: "Run the tests" });
  assert.equal(permission.line, null);
  const question = stepActivity(session({ pendingQuestion: { id: "q" } }), undefined, "pt-BR", 0);
  assert.equal(question.attention.kind, "question");
});

const { activityFeed, lastResponse } = await import("../src/lib/boardActivity.ts");

test("the feed lists recent work newest first, without prompts", () => {
  const feed = activityFeed(session({ activities: [
    activity("prompt", { createdAt: 1, detail: "Do it" }),
    activity("command", { createdAt: 2, detail: "npm run check" }),
    activity("file", { createdAt: 3, files: ["src/a.ts", "src/b.ts", "src/c.ts"] }),
    activity("message", { createdAt: 4, detail: "All\n  done." }),
  ] }), "en");
  assert.deepEqual(feed.map((item) => item.category), ["message", "edit", "test"]);
  assert.equal(feed[0].detail, "All done.");
  assert.equal(feed[1].detail, "a.ts, b.ts +1");
  assert.equal(feed[2].detail, "npm run check");
  assert.equal(activityFeed(session({ activities: Array.from({ length: 30 }, (_, index) => activity("command", { createdAt: index })) }), "en", 5).length, 5);
  assert.deepEqual(activityFeed(undefined, "en"), []);
});

test("the last response is the newest result, trimmed", () => {
  assert.equal(lastResponse(session({ results: [{ response: "old" }, { response: "new" }] })), "new");
  assert.equal(lastResponse(session({ results: [{ response: "x".repeat(900) }] }), 100).length, 100);
  assert.equal(lastResponse(session()), null);
});
