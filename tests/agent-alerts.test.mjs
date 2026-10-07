import assert from "node:assert/strict";
import { collectAgentAlerts } from "../src/lib/agentAlerts.ts";

const now = 1_800_000_000_000;
const session = {
  id: "session-1",
  agent: "codex",
  agentLabel: "Codex",
  sessionName: "Lume principal",
  project: "Lume",
  source: "cli",
  controlOrigin: "lume",
  status: "running",
  statusLabel: "Executando",
  startedAt: "",
  updatedAt: now,
  permissionProfile: { mode: "workspace_write", label: "", approvalPolicy: "", canRespondFromLume: true, availableActions: [] },
  results: [],
  activities: [],
  rateLimits: [],
};

const usage = collectAgentAlerts([{ ...session, rateLimits: [{ id: "primary", label: "5h", usedPercent: 91 }] }], "pt-BR", now);
assert.equal(usage[0].tone, "warning");
assert.match(usage[0].message, /Restam apenas 9%/);

assert.equal(usage[0].usage, true);
assert.equal(usage[0].pinned, true);
const lowUsage = collectAgentAlerts([{ ...session, rateLimits: [{ id: "primary", label: "5h", usedPercent: 85 }] }], "pt-BR", now);
assert.equal(lowUsage[0].pinned, false);
assert.match(lowUsage[0].message, /Restam apenas 15%/);
assert.deepEqual(collectAgentAlerts([{ ...session, rateLimits: [{ id: "primary", label: "5h", usedPercent: 70 }] }], "pt-BR", now), []);

const exhausted = collectAgentAlerts([{ ...session, rateLimits: [{ id: "primary", label: "5h", usedPercent: 100 }] }], "pt-BR", now);
assert.equal(exhausted[0].tone, "error");
assert.equal(exhausted[0].duration, 0);
assert.equal(exhausted[0].pinned, true);

const mcp = collectAgentAlerts([{
  ...session,
  activities: [{ id: "mcp-1", kind: "tool", title: "MCP · github · create_issue", detail: "connection refused", status: "failed", createdAt: now - 100, files: [] }],
}], "pt-BR", now);
assert.equal(mcp[0].priority, 95);
assert.match(mcp[0].message, /falha no MCP/);

const usageFailure = collectAgentAlerts([{
  ...session,
  activities: [{ id: "limit-1", kind: "message", title: "Usage limit reached", status: "failed", createdAt: now - 50, files: [] }],
}], "pt-BR", now);
assert.equal(usageFailure[0].priority, 100);
assert.match(usageFailure[0].message, /atingiu o limite/);

const cliWarning = collectAgentAlerts([{
  ...session,
  activities: [{ id: "warning-1", kind: "warning", title: "Aviso de configuração", detail: "O servidor MCP está indisponível", status: "warning", createdAt: now - 25, files: [] }],
}], "pt-BR", now);
assert.equal(cliWarning[0].tone, "warning");
assert.equal(cliWarning[0].duration, 7_000);
assert.match(cliWarning[0].message, /MCP está indisponível/);

const permission = collectAgentAlerts([{
  ...session,
  pendingPermission: { id: "permission-1", kind: "command", summary: "executar testes", resource: "npm test", risk: "medium", requestedAt: new Date(now).toISOString() },
}], "pt-BR", now);
assert.match(permission[0].message, /aguardando permissão/);

const oldFailure = collectAgentAlerts([{
  ...session,
  activities: [{ id: "old", kind: "command", title: "old command", status: "failed", createdAt: now - 11 * 60 * 1_000, files: [] }],
}], "pt-BR", now);
assert.equal(oldFailure.length, 0);

console.log("agent alert tests passed");
