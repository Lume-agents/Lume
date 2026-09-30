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
for (const agent of ["codex", "opencode", "antigravity"]) {
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
assert.equal(sessionCapabilities(session({ agent: "claude_code" })).canTerminate, false);
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
for (const agent of ["claude_code", "antigravity"]) {
  const capabilities = sessionCapabilities(session({ agent, status: "running" }));
  assert.equal(capabilities.canPrompt, false, `${agent} cannot accept a new turn while busy`);
  assert.equal(capabilities.promptUnavailableReason, "agent_busy");
  assert.deepEqual(capabilities.promptDeliveries, ["new_turn"]);
}
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
