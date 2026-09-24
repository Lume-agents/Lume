import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const root = new URL("../", import.meta.url);
const sharedSource = await readFile(new URL("extensions/chromium/shared.js", root), "utf8");
const manifest = JSON.parse(
  await readFile(new URL("extensions/chromium/manifest.json", root), "utf8"),
);
const context = {};
vm.runInNewContext(sharedSource, context);

const {
  providerForHost,
  eventForTab,
  promptAckRetryState,
  promptWasAccepted,
  resolveWebSessionState,
} = context.LumeWebShared;
assert.equal(providerForHost("chatgpt.com"), "chatgpt");
assert.equal(providerForHost("chat.openai.com"), "chatgpt");
assert.equal(providerForHost("claude.ai"), "claude");
assert.equal(providerForHost("chat.deepseek.com"), "deepseek");
assert.equal(providerForHost("gemini.google.com"), "gemini");
assert.equal(providerForHost("example.com"), null);

const sourceEvent = { provider: "chatgpt", sessionId: "thread" };
assert.equal(eventForTab(sourceEvent, 41).sessionId, "thread.41");
assert.equal(eventForTab(sourceEvent, 42).sessionId, "thread.42");
assert.equal(sourceEvent.sessionId, "thread");

const confirmedPromptAck = promptAckRetryState(true, true);
assert.equal(confirmedPromptAck.pending, false);
assert.equal(confirmedPromptAck.submitted, false);
const lostPromptAck = promptAckRetryState(true, false);
assert.equal(lostPromptAck.pending, true);
assert.equal(lostPromptAck.submitted, true);
const rejectedPromptSubmit = promptAckRetryState(false, false);
assert.equal(rejectedPromptSubmit.pending, true);
assert.equal(rejectedPromptSubmit.submitted, false);

assert.equal(promptWasAccepted([], ["Please inspect the project"], "Please inspect the project"), true);
assert.equal(promptWasAccepted(["Please inspect the project"], ["Please inspect the project"], "Please inspect the project"), false);
assert.equal(promptWasAccepted(["Earlier prompt"], ["Earlier prompt", "Please inspect the project"], "Please inspect the project"), true);
assert.equal(promptWasAccepted([], [], "Please inspect the project"), false);
assert.equal(promptWasAccepted([], ["Please inspect the project"], "  Please   inspect\nthe project  "), true);

const activeWebSession = resolveWebSessionState({
  previousState: "running",
  hasStopControl: true,
  now: 1_000,
});
assert.equal(activeWebSession.state, "running");
assert.equal(activeWebSession.stopMissingSince, 0);

const transientMissingStop = resolveWebSessionState({
  previousState: "running",
  now: 1_000,
  graceMs: 3_000,
});
assert.equal(transientMissingStop.state, "running");
assert.equal(transientMissingStop.recheckAfterMs, 3_000);
assert.equal(resolveWebSessionState({
  previousState: "running",
  stopMissingSince: transientMissingStop.stopMissingSince,
  now: 2_500,
  graceMs: 3_000,
}).state, "running", "a brief missing stop control must not free a busy chat");
assert.equal(resolveWebSessionState({
  previousState: "running",
  stopMissingSince: transientMissingStop.stopMissingSince,
  now: 4_000,
  graceMs: 3_000,
}).state, "completed", "a stable missing stop control eventually completes the turn");
assert.equal(resolveWebSessionState({
  previousState: "running",
  permissionRequired: true,
  now: 1_000,
}).state, "permission_required");
assert.equal(resolveWebSessionState({
  previousState: "running",
  failed: true,
  now: 1_000,
}).state, "failed");

const contentScripts = manifest.content_scripts?.[0]?.js ?? [];
assert.deepEqual(contentScripts, ["shared.js", "content.js"]);
assert.ok(manifest.content_scripts[0].matches.includes("https://chatgpt.com/*"));
assert.ok(manifest.content_scripts[0].matches.includes("https://claude.ai/*"));
assert.ok(manifest.content_scripts[0].matches.includes("https://chat.deepseek.com/*"));
assert.ok(manifest.content_scripts[0].matches.includes("https://gemini.google.com/*"));

console.log("web companion test suite passed");
