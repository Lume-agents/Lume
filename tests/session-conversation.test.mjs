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

test("Claude limit notices never become tools or synthetic replies, even without a prompt", () => {
  for (const notice of [
    activity("claude-error", "error", 10, "You've hit your session limit", { status: "failed", title: "Erro do Claude" }),
    activity("legacy-claude-limit", "tool", 10, "", { title: "You've hit your session limit" }),
    activity("claude-api-notice", "message", 10, "You’ve hit your session limit"),
  ]) {
    assert.deepEqual(buildConversationFeed(entriesFor([notice], { agent: "claude_code" })), []);
  }
  const explained = activity("explanation", "message", 10, "The CLI says ‘You've hit your session limit’. Wait for the reset.");
  assert.equal(buildConversationFeed(entriesFor([explained])).length, 1, "ordinary discussion of a usage error remains visible");
  const failedCommand = activity("failed-test", "command", 10, "Assertion failed: expected 'You've hit your session limit'", { title: "npm run test", status: "failed" });
  assert.equal(buildConversationFeed(entriesFor([failedCommand]))[0].kind, "trace", "real tool output mentioning a limit remains in the timeline");
});

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

test("Claude's own notices are not shown as something the user sent", () => {
  const entries = entriesFor([
    activity("typed", "prompt", 10, "Rode os testes"),
    activity("notice", "prompt", 20, "<task-notification>\n<task-id>abc</task-id>\n<status>completed</status>\n</task-notification>"),
    activity("reminder", "prompt", 25, "<system-reminder>\nContext\n</system-reminder>"),
    activity("answer", "message", 30, "Os testes passaram"),
  ]);
  assert.deepEqual(entries.map((entry) => entry.activity.id), ["typed", "answer"]);
});

test("a new Lume Codex chat hides injected context and reconciles native completion echoes", () => {
  const context = "# AGENTS.md instructions\n\n<INSTRUCTIONS>\nProject rules\n</INSTRUCTIONS>\n<environment_context>\n<cwd>/work/lume</cwd>\n</environment_context>";
  const activities = [
    activity("local:chat:1000", "prompt", 1_000, "oi"),
    activity("codex-rollout:chat:context", "prompt", 3_113, context),
    activity("codex:chat:prompt:3244", "prompt", 3_244, "oi"),
    activity("codex:chat:turn:t:item:msg-1", "message", 5_160, "Oi! O que vamos fazer hoje?"),
    activity("codex:chat:response:5478", "message", 5_478, "Oi! O que vamos fazer hoje?"),
  ];
  const original = structuredClone(activities);
  const entries = entriesFor(activities);
  assert.deepEqual(entries.map((entry) => [entry.activity.kind, entry.activity.detail]), [
    ["prompt", "oi"],
    ["message", "Oi! O que vamos fazer hoje?"],
  ]);
  assert.deepEqual(activities, original, "rendering saved history must not mutate it");

  const nextTurn = entriesFor([
    ...activities,
    activity("local:chat:10000", "prompt", 10_000, "oi"),
    activity("codex:chat:turn:t2:item:msg-2", "message", 11_000, "Oi! O que vamos fazer hoje?"),
    activity("codex:chat:response:11400", "message", 11_400, "Oi! O que vamos fazer hoje?"),
  ]);
  assert.deepEqual(nextTurn.map((entry) => entry.activity.kind), ["prompt", "message", "prompt", "message"]);
});

test("injected environment records do not hide real requests about project instructions", () => {
  const environment = "<environment_context>\n<cwd>/work/lume</cwd>\n</environment_context>";
  const rules = "# AGENTS.md instructions for /work/lume\n\n<INSTRUCTIONS>\nRules\n</INSTRUCTIONS>";
  const entries = entriesFor([
    activity("environment", "prompt", 10, environment),
    activity("rules", "prompt", 20, rules),
    activity("question", "prompt", 30, "Explique o AGENTS.md e <environment_context>."),
    activity("pasted", "prompt", 40, `${rules}\n\nExplique essas instruções.`),
  ]);
  assert.deepEqual(entries.map((entry) => entry.activity.id), ["question", "pasted"]);
});

test("the resume filler the CLI writes does not split a prompt from its own copy", () => {
  // Sent once from Lume, then read back from the transcript after the CLI wrote its own answer in between.
  const entries = entriesFor([
    activity("local:chat:1", "prompt", 1_000, "commit everything"),
    activity("claude:chat:transcript:filler:0", "message", 2_000, "No response requested."),
    activity("claude:chat:transcript:user:0", "prompt", 3_000, "commit everything"),
    activity("claude:chat:transcript:error:0", "message", 4_000, "Failed to refresh OAuth token: retry in a minute"),
  ]);
  assert.deepEqual(entries.map((entry) => `${entry.activity.kind}:${entry.activity.detail}`), [
    "prompt:commit everything",
    "message:Failed to refresh OAuth token: retry in a minute",
  ]);
  // Sending the same text again after a real answer is a new prompt.
  const resent = entriesFor([
    activity("local:chat:1", "prompt", 1_000, "commit everything"),
    activity("claude:chat:transcript:error:0", "message", 4_000, "Failed to refresh OAuth token: retry in a minute"),
    activity("local:chat:2", "prompt", 9_000, "commit everything"),
  ]);
  assert.equal(resent.filter((entry) => entry.activity.kind === "prompt").length, 2);
});

test("an image shown at send time and its cache copy from the transcript are one attachment", () => {
  const shown = { id: "attachment:1:0", name: "image.jpg", mimeType: "image/jpeg", previewDataUrl: "data:image/jpeg;base64,AAAA" };
  const copy = { id: "response-file:abc", name: "1791581411129-0-image.jpg", mimeType: "image/jpeg", previewDataUrl: "", path: "/cache/prompt-attachments/1791581411129-0-image.jpg" };
  const entries = entriesFor([
    activity("local:chat:1", "prompt", 1_000, "what is this?", { attachments: [shown] }),
    activity("claude:chat:transcript:user:0", "prompt", 2_000, "what is this?", { attachments: [copy] }),
  ]);
  assert.equal(entries.length, 1);
  const [attachment] = entries[0].activity.attachments;
  assert.equal(entries[0].activity.attachments.length, 1, "no second chip without a photo");
  assert.equal(attachment.name, "image.jpg");
  assert.ok(attachment.previewDataUrl, "the photo is kept");
  assert.equal(attachment.path, copy.path, "and it knows where the file is");
  const other = { ...copy, id: "response-file:def", name: "1791581411130-1-other.png", path: "/cache/prompt-attachments/1791581411130-1-other.png" };
  const two = entriesFor([
    activity("local:chat:1", "prompt", 1_000, "two files", { attachments: [shown] }),
    activity("claude:chat:transcript:user:0", "prompt", 2_000, "two files", { attachments: [copy, other] }),
  ]);
  assert.equal(two[0].activity.attachments.length, 2);
});

test("the login-refresh notice disappears when the prompt went through on the retry, and stays when nothing followed it", () => {
  const notice = "Failed to refresh OAuth token: another Claude Code process is refreshing it or exited mid-refresh. Retry in a minute.";
  const retried = entriesFor([
    activity("local:chat:1", "prompt", 1_000, "commit everything"),
    activity("claude:chat:transcript:notice:0", "message", 2_000, notice),
    activity("claude:chat:transcript:user:0", "prompt", 5_000, "commit everything"),
    activity("claude:chat:transcript:answer:0", "message", 9_000, "Done, committed."),
  ]);
  assert.deepEqual(retried.map((entry) => `${entry.activity.kind}:${entry.activity.detail}`), ["prompt:commit everything", "message:Done, committed."]);
  const failed = entriesFor([
    activity("local:chat:1", "prompt", 1_000, "commit everything"),
    activity("claude:chat:transcript:notice:0", "message", 2_000, notice),
  ]);
  assert.equal(failed.length, 2, "an unanswered failure is still shown");
});

test("separate Claude edits to one file add up in the files changed after the final message", () => {
  const edit = (id, createdAt) => activity(`claude:chat:tool:${id}`, "file", createdAt, `edit ${id}`, { files: ["/work/src/a.ts"] });
  const counts = { "edit one": [3, 1], "edit two": [2, 2], "edit three": [4, 0] };
  const changes = (item) => (counts[item.detail] ? [{ path: "src/a.ts", added: counts[item.detail][0], removed: counts[item.detail][1] }] : []);
  const activities = [
    activity("prompt", "prompt", 10),
    edit("one", 20), edit("two", 30), edit("three", 40),
    activity("final", "message", 50, "Done."),
  ];
  const entries = entriesFor(activities, { results: [{ id: "r", response: "Done.", createdAt: 50, files: [], tests: [] }] }, changes);
  const feed = buildConversationFeed(entries, { workingDirectory: "/work" });
  const files = fileChangesForFinalResponses(feed, "/work").get("activity:final");
  assert.deepEqual(files, [{ path: "src/a.ts", added: 9, removed: 3 }]);
});

test("two Claude edits with the same size are both kept", () => {
  const activities = [
    activity("prompt", "prompt", 10),
    activity("claude:chat:tool:one", "file", 20, "edit one", { files: ["/work/src/a.ts"] }),
    activity("claude:chat:tool:two", "file", 30, "edit two", { files: ["/work/src/a.ts"] }),
  ];
  const entries = entriesFor(activities, {}, () => [{ path: "src/a.ts", added: 1, removed: 1 }]);
  assert.equal(entries.filter((entry) => entry.activity.kind === "file").length, 2);
});
