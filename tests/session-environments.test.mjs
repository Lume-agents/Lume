import assert from "node:assert/strict";
import { environmentElapsed, environmentsForSession, runningEnvironments } from "../src/lib/sessionEnvironments.ts";

const current = { id: "one", sessionId: "chat-a", startedAt: 1000, stoppedAt: null, status: "running" };
const stopped = { ...current, id: "two", stoppedAt: 1_180_000, status: "stopped" };
const other = { ...current, id: "three", sessionId: "chat-b", status: "unknown" };
const snapshot = { environments: [current, stopped, other], error: null };
assert.deepEqual(environmentsForSession(snapshot, "chat-a"), [current, stopped]);
assert.deepEqual(runningEnvironments(snapshot.environments), [current]);
assert.equal(environmentElapsed(current, 1_121_000), "2m");
assert.equal(environmentElapsed(stopped, 8_000_000), "3m", "stopped time must freeze");
assert.equal(environmentElapsed(current, 4_661_000), "1h 1m");
assert.equal(environmentElapsed(current, 999_000), "0s", "clock drift must not show negative durations");
console.log("Session environments: grouping, active counts and elapsed time passed.");
