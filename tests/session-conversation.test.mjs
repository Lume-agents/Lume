import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import test from "node:test";

// Match SvelteKit's runtime alias without bundling or changing app imports.
registerHooks({
  resolve(specifier, context, nextResolve) {
    if (specifier.startsWith("$lib/")) {
      const path = specifier.slice(5);
      const extension = /\.[jt]s$/.test(path) ? "" : ".ts";
      return nextResolve(new URL(`../src/lib/${path}${extension}`, import.meta.url).href, context);
    }
    return nextResolve(specifier, context);
  },
});

const { buildConversationEntries, buildConversationFeed, fileChangesForFinalResponses } =
  await import("../src/lib/sessionConversation.ts");

const activity = (id, kind, createdAt, detail = id, overrides = {}) => ({
  id, kind, createdAt, detail, title: kind, status: "completed", files: [],
  attachments: [], appendDetail: false, ...overrides,
});
const session = (overrides = {}) => ({
  id: "chat", agent: "codex", status: "running", updatedAt: 1_000,
  results: [], activities: [], workingDirectory: "/work", ...overrides,
});
const entriesFor = (activities, overrides = {}, changes = () => []) =>
  buildConversationEntries(session(overrides), activities, changes);

test("messages interrupt tool groups in the provider's timeline", () => {
  const activities = [
    activity("prompt", "prompt", 10),
    activity("first-update", "message", 20),
    activity("read", "command", 30),
    activity("search", "tool", 40),
    activity("second-update", "message", 50),
    activity("check", "command", 60),
    activity("final", "message", 70),
  ];
  const feed = buildConversationFeed(entriesFor(activities));
  assert.deepEqual(feed.map((item) => item.kind === "trace"
    ? item.entries.map((entry) => entry.activity.id)
    : item.entry.activity.id), [
    "prompt", "first-update", ["read", "search"], "second-update", ["check"], "final",
  ]);
});

test("out-of-order snapshots are sorted before deduplication", () => {
  const entries = entriesFor([
    activity("command", "command", 30),
    activity("message", "message", 20),
    activity("prompt", "prompt", 10),
  ]);
  assert.deepEqual(entries.map((entry) => entry.activity.id), ["prompt", "message", "command"]);
});

test("equal timestamps retain provider order, not alphabetical item IDs", () => {
  const entries = entriesFor([
    activity("z-call", "command", 10),
    activity("a-message", "message", 10),
    activity("b-call", "command", 10),
  ]);
  assert.deepEqual(entries.map((entry) => entry.activity.id), ["z-call", "a-message", "b-call"]);
});

test("a replayed message updates content without jumping past tools", () => {
  const activities = [
    activity("native-message", "message", 10, "Checking the source"),
    activity("read", "command", 20),
    activity("rollout-message", "message", 30, "Checking the source"),
  ];
  const original = structuredClone(activities);
  const entries = entriesFor(activities);
  assert.deepEqual(entries.map((entry) => [entry.activity.id, entry.activity.createdAt]), [
    ["native-message", 10], ["read", 20],
  ]);
  assert.deepEqual(activities, original);
});

test("separate native messages are retained even when their text is identical", () => {
  for (const ids of [
    ["codex:chat:turn:t:item:msg-1", "codex:chat:turn:t:item:msg-2"],
    ["codex-rollout:1", "codex-rollout:2"],
  ]) {
    const entries = entriesFor([
      activity(ids[0], "message", 10, "Running another check"),
      activity("check", "command", 20),
      activity(ids[1], "message", 30, "Running another check"),
    ]);
    assert.deepEqual(entries.map((entry) => entry.activity.id), [ids[0], "check", ids[1]]);
  }
});

test("legacy and turn-qualified IDs of the same native message still deduplicate", () => {
  const entries = entriesFor([
    activity("codex:chat:msg-1", "message", 10, "Reading the source"),
    activity("read", "command", 20),
    activity("codex:chat:turn:t:item:msg-1", "message", 30, "Reading the source"),
  ]);
  assert.deepEqual(entries.map((entry) => entry.activity.id), ["codex:chat:msg-1", "read"]);
});

test("completion metadata does not relocate a final message", () => {
  const entries = entriesFor([
    activity("prompt", "prompt", 10),
    activity("check", "command", 20),
    activity("final", "message", 30, "All checks passed"),
    activity("after-final", "tool", 40),
  ], {
    status: "completed", lastResponse: "All checks passed", updatedAt: 1_000,
    results: [{ id: "result", response: "All checks passed", files: [], createdAt: 35 }],
  });
  assert.deepEqual(entries.map((entry) => entry.activity.id), ["prompt", "check", "final", "after-final"]);
  const final = entries.find((entry) => entry.activity.id === "final");
  assert.equal(final.activity.createdAt, 30);
  assert.equal(final.durationMs, 25);
  assert.equal(final.isFinalResponse, true);
});

test("a later prompt does not move the previous final response into its turn", () => {
  const activities = [
    activity("old-prompt", "prompt", 10),
    activity("old-final", "message", 20, "Previous result"),
    activity("new-prompt", "prompt", 30),
    activity("new-command", "command", 40),
    activity("new-update", "message", 50, "Working on the next request"),
  ];
  for (const updatedAt of [60, 1_000]) {
    const entries = entriesFor(activities, { lastResponse: "Previous result", updatedAt });
    assert.deepEqual(entries.map((entry) => entry.activity.id), activities.map((item) => item.id));
    assert.equal(entries[1].activity.createdAt, 20);
  }
});

test("an active turn never fabricates an old lastResponse at the current time", () => {
  for (const status of ["running", "permission_required"]) {
    const entries = entriesFor([
      activity("new-prompt", "prompt", 30),
      activity("new-command", "command", 40),
    ], { status, lastResponse: "Previous result", updatedAt: 50 });
    assert.deepEqual(entries.map((entry) => entry.activity.id), ["new-prompt", "new-command"]);
  }
});

test("completed legacy sessions retain their last-response fallback", () => {
  const entries = entriesFor([], { status: "completed", lastResponse: "Legacy result", updatedAt: 50 });
  assert.equal(entries.length, 1);
  assert.equal(entries[0].activity.detail, "Legacy result");
  assert.equal(entries[0].activity.createdAt, 50);
  assert.equal(entries[0].isFinalResponse, true);
});

test("repeated replies in separate prompts keep their own timeline positions", () => {
  const entries = entriesFor([
    activity("prompt-1", "prompt", 10),
    activity("message-1", "message", 20, "Done"),
    activity("prompt-2", "prompt", 30),
    activity("message-2", "message", 40, "Done"),
  ], { lastResponse: "Done", status: "completed", updatedAt: 50 });
  assert.deepEqual(entries.map((entry) => entry.activity.id), ["prompt-1", "message-1", "prompt-2", "message-2"]);
  assert.equal(entries[1].isFinalResponse, undefined);
  assert.equal(entries[3].isFinalResponse, true);
});

test("changed files stay attached to the final response of their own prompt", () => {
  const activities = [
    activity("prompt-1", "prompt", 10),
    activity("edit-1", "file", 20),
    activity("final-1", "message", 30),
    activity("prompt-2", "prompt", 40),
    activity("edit-2", "file", 50),
    activity("final-2", "message", 60),
  ];
  const entries = entriesFor(activities, {
    results: [
      { id: "result-1", response: "final-1", files: [], createdAt: 35 },
      { id: "result-2", response: "final-2", files: [], createdAt: 65 },
    ],
  }, (item) => item.kind === "file" ? [{ path: item.id, added: 1, removed: 0 }] : []);
  const files = fileChangesForFinalResponses(buildConversationFeed(entries));
  assert.deepEqual([...files].map(([id, changes]) => [id, changes.map((file) => file.path)]), [
    ["activity:final-1", ["edit-1"]], ["activity:final-2", ["edit-2"]],
  ]);
});

test("a cancelled request is one notice in the chat, not a tool event and not twice", () => {
  const notice = (id, createdAt, detail) =>
    activity(id, "interrupt", createdAt, detail, { title: "Prompt interrupted", status: "interrupted" });
  const activities = [
    activity("prompt", "prompt", 10),
    activity("read", "command", 20),
    // Lume's own cancel, then the CLI's record of the same cancel a moment later.
    notice("lume-cancel", 30, undefined),
    notice("cli-record", 33, "user"),
    activity("again", "prompt", 60),
    notice("later", 90, "tool_use"),
  ];
  const feed = buildConversationFeed(entriesFor(activities));
  assert.deepEqual(
    feed.map((item) => (item.kind === "trace" ? "trace" : item.entry.activity.kind)),
    ["prompt", "trace", "interrupt", "prompt", "interrupt"],
    "the notice sits outside the tool events, and the two records of one cancel collapse",
  );
  assert.equal(
    feed.filter((item) => item.kind === "entry" && item.entry.activity.kind === "interrupt").length,
    2,
    "a later cancel is a new notice",
  );
});
