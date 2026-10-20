import assert from "node:assert/strict";
import { sessionCapabilities } from "../src/lib/sessionCapabilities.ts";

function session(overrides = {}) {
  return {
    agent: "codex",
    source: "cli",
    status: "running",
    controlOrigin: "lume",
    nativeSessionId: "native-session",
    workingDirectory: "/tmp/project",
    permissionProfile: { canRespondFromLume: false },
    results: [],
    ...overrides,
  };
}

assert.equal(sessionCapabilities(session()).canInterrupt, true);
assert.equal(sessionCapabilities(session({ agent: "omp", controlOrigin: "external" })).canPrompt, false);
assert.equal(sessionCapabilities(session({ agent: "omp", controlOrigin: "external" })).promptUnavailableReason, "monitoring_only");
assert.deepEqual(sessionCapabilities(session({ agent: "omp", controlOrigin: "external" })).promptDeliveries, []);
const controlledOmp = sessionCapabilities(session({
  agent: "omp",
  source: "desktop",
  controlOrigin: "lume",
  status: "running",
  capabilities: undefined,
}));
assert.equal(controlledOmp.canPrompt, true);
assert.equal(controlledOmp.canInterrupt, true);
assert.deepEqual(controlledOmp.promptDeliveries, ["new_turn", "steer", "queue"]);
assert.equal(sessionCapabilities(session({
  agent: "omp",
  pendingPermission: { id: "approval" },
  pendingQuestion: { id: "question" },
  permissionProfile: { canRespondFromLume: true },
})).canApprove, true);
assert.equal(sessionCapabilities(session({
  agent: "omp",
  pendingQuestion: { id: "question" },
  permissionProfile: { canRespondFromLume: true },
})).canAnswerQuestion, true);
assert.equal(sessionCapabilities(session({ agent: "omp", controlOrigin: "external", nativeSessionId: "omp-1", processId: 42 })).canTakeControl, true);
assert.equal(sessionCapabilities(session({ agent: "omp", controlOrigin: "lume", source: "desktop" })).canTakeControl, false);
const monitoredOmp = sessionCapabilities(session({
  agent: "omp",
  controlOrigin: "external",
  pendingPermission: { id: "approval", actions: ["allow_once"] },
  pendingQuestion: { id: "question" },
  permissionProfile: { canRespondFromLume: true },
}));
assert.equal(monitoredOmp.canApprove, false);
assert.equal(monitoredOmp.canAnswerQuestion, false);
assert.equal(monitoredOmp.canAttachImages, false);
for (const agent of ["codex", "opencode", "antigravity", "omp"]) {
  for (const source of ["desktop", "cli"]) {
    assert.equal(
      sessionCapabilities(session({ agent, source })).canTerminate,
      true,
      `${agent} sessions managed without a CLI must still be terminable`,
    );
    assert.equal(
      sessionCapabilities(session({ agent, source, controlOrigin: "external" })).canTerminate,
      false,
      "a native session id alone must not authorize closing an external application",
    );
  }
  for (const source of ["web", "vscode"]) {
    assert.equal(sessionCapabilities(session({ agent, source })).canTerminate, false);
  }
}
assert.equal(sessionCapabilities(session({ processId: 4242 })).canTerminate, true);
assert.equal(sessionCapabilities(session({ nativeSessionId: " " })).canTerminate, false);
assert.equal(
  sessionCapabilities(session({ agent: "claude_code" })).canTerminate,
  true,
  "a Claude conversation Lume runs without a terminal can be ended from Lume",
);
assert.equal(
  sessionCapabilities(session({ agent: "claude_code", controlOrigin: "external" })).canTerminate,
  false,
  "an external Claude conversation is never closed from its session id alone",
);
assert.equal(
  sessionCapabilities(session({ agent: "claude_code" })).canInterrupt,
  true,
  "Claude Code interruption is supported by the backend protocol",
);
assert.equal(
  sessionCapabilities(session({ agent: "claude_code", controlOrigin: "external" })).canInterrupt,
  false,
  "externally controlled sessions must not expose Lume interruption",
);
assert.equal(
  sessionCapabilities(session({ agent: "claude_code", source: "desktop" })).canInterrupt,
  true,
  "a Claude conversation Lume opened without a terminal can be interrupted",
);
assert.equal(
  sessionCapabilities(session({ agent: "claude_code", source: "vscode" })).canInterrupt,
  false,
  "Claude interruption is limited to the CLI process Lume can target safely",
);
assert.equal(
  sessionCapabilities(session({ agent: "antigravity" })).canInterrupt,
  false,
  "Antigravity has no safe interruption protocol yet",
);
const legacyGeminiCli = sessionCapabilities(session({
  agent: "gemini",
  source: "cli",
  status: "idle",
}));
assert.equal(legacyGeminiCli.canPrompt, false);
assert.equal(legacyGeminiCli.canTerminate, false);
assert.equal(legacyGeminiCli.promptUnavailableReason, "monitoring_only");
assert.deepEqual(legacyGeminiCli.promptDeliveries, []);
assert.equal(
  sessionCapabilities(session({ agent: "gemini", source: "web", status: "waiting_for_input" })).canPrompt,
  true,
  "Gemini Web remains available through the separate Companion flow",
);
for (const agent of ["antigravity"]) {
  const capabilities = sessionCapabilities(session({ agent, status: "running" }));
  assert.equal(capabilities.canPrompt, false, `${agent} cannot accept a new turn while busy`);
  assert.equal(capabilities.promptUnavailableReason, "agent_busy");
  assert.deepEqual(capabilities.promptDeliveries, ["new_turn"]);
}
// Each Claude message Lume sends is its own run, so a busy conversation can queue and cancel.
for (const source of ["cli", "desktop"]) {
  const claude = sessionCapabilities(session({ agent: "claude_code", source, status: "running" }));
  assert.equal(claude.canPrompt, true, `Claude owned by Lume accepts a message while busy (${source})`);
  assert.deepEqual(claude.promptDeliveries, ["new_turn", "queue", "steer"]);
  assert.equal(claude.canInterrupt, true);
}
// An external CLI keeps its terminal to itself: no queue and no cancel from Lume.
const externalClaude = sessionCapabilities(
  session({ agent: "claude_code", status: "running", controlOrigin: "external" }),
);
assert.equal(externalClaude.canPrompt, false);
assert.equal(externalClaude.promptUnavailableReason, "external_session");
assert.equal(externalClaude.canInterrupt, false);
assert.deepEqual(externalClaude.promptDeliveries, ["new_turn"]);
assert.equal(
  sessionCapabilities(session({ agent: "claude_code", status: "running", source: "web" })).canPrompt,
  false,
  "the web Companion flow is unchanged",
);
assert.equal(
  sessionCapabilities(session({ status: "running" })).canPrompt,
  true,
  "Codex controlled by Lume supports queue/steer during a prompt",
);
assert.equal(
  sessionCapabilities(session({ agent: "gemini", source: "web", status: "running" })).canPrompt,
  false,
  "web Companion must not claim queue/steer while the page is running",
);
