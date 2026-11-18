import assert from "node:assert/strict";
import test from "node:test";

const { withoutDismissed, dismissEnvironments, dismissedEnvironments } = await import("../src/lib/sessionEnvironments.ts");
const { get } = await import("svelte/store");

const item = (id, status) => ({ id, status });

test("a stopped environment can be hidden, a running one cannot, and a hidden one returns when it runs again", () => {
  const list = [item("web-1", "stopped"), item("db-1", "idle"), item("api-1", "running"), item("old-1", "unknown")];
  assert.deepEqual(withoutDismissed(list, []).map((entry) => entry.id), ["web-1", "db-1", "api-1", "old-1"]);
  assert.deepEqual(withoutDismissed(list, ["web-1", "api-1", "old-1"]).map((entry) => entry.id), ["db-1", "api-1"], "running stays");
  assert.deepEqual(withoutDismissed([item("web-1", "running")], ["web-1"]).map((entry) => entry.id), ["web-1"], "it comes back running");
});

test("dismissals are remembered without duplicates", () => {
  dismissEnvironments(["a", "b"]);
  dismissEnvironments(["b", "c"]);
  assert.deepEqual(get(dismissedEnvironments), ["a", "b", "c"]);
});
