import assert from "node:assert/strict";
import { collectAgentAlerts } from "../src/lib/agentAlerts.ts";
import { createUsageAlertDismissals } from "../src/lib/usageAlertDismissals.ts";
import { get } from "svelte/store";

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
assert.deepEqual({ ...usage[0].usageInfo }, { agent: session.agent, agentLabel: session.agentLabel, windowLabel: "5h", remaining: 9, resetsAt: undefined });
const lowUsage = collectAgentAlerts([{ ...session, rateLimits: [{ id: "primary", label: "5h", usedPercent: 85 }] }], "pt-BR", now);
assert.equal(lowUsage[0].pinned, false);
assert.match(lowUsage[0].message, /Restam apenas 15%/);
assert.deepEqual(collectAgentAlerts([{ ...session, rateLimits: [{ id: "primary", label: "5h", usedPercent: 70 }] }], "pt-BR", now), []);

const exhausted = collectAgentAlerts([{ ...session, rateLimits: [{ id: "primary", label: "5h", usedPercent: 100 }] }], "pt-BR", now);
assert.equal(exhausted[0].tone, "error");
assert.equal(exhausted[0].duration, 0);
assert.equal(exhausted[0].pinned, true);

const claudeIdle = { ...session, agent: "claude_code", agentLabel: "Claude Code", status: "waiting_for_input", rateLimits: [{ id: "claude:session", label: "5h", usedPercent: 100, resetsAt: now + 60_000 }] };
assert.deepEqual(collectAgentAlerts([claudeIdle], "pt-BR", now, { usageScope: "active" }), [], "idle accounts must not raise global usage banners");
assert.equal(collectAgentAlerts([claudeIdle], "pt-BR", now).length, 1, "a Claude chat can still show its own account limit");
assert.equal(collectAgentAlerts([{ ...claudeIdle, status: "running" }], "pt-BR", now, { usageScope: "active" }).length, 1);
assert.deepEqual(collectAgentAlerts([{ ...claudeIdle, status: "running", rateLimits: [{ ...claudeIdle.rateLimits[0], resetsAt: now - 1 }] }], "pt-BR", now), [], "expired cached limits must not raise new banners");
assert.equal(collectAgentAlerts([claudeIdle, { ...claudeIdle, id: "claude-other" }], "pt-BR", now).length, 1, "sessions sharing an account must share one usage notice");

const saved = new Map();
const storageListeners = [];
const host = {
  localStorage: {
    getItem: key => saved.get(key) ?? null,
    setItem(key, value) {
      saved.set(key, value);
      storageListeners.forEach(listener => listener({ key }));
    },
  },
  addEventListener(type, listener) { if (type === "storage") storageListeners.push(listener); },
};
const orbDismissals = createUsageAlertDismissals(host);
const workspaceDismissals = createUsageAlertDismissals(host);
orbDismissals.dismiss(exhausted[0].id);
assert.ok(get(workspaceDismissals).includes(exhausted[0].id), "closing usage in Orb must update Workspace");
assert.ok(get(createUsageAlertDismissals(host)).includes(exhausted[0].id), "closing usage must survive a remounted screen");
assert.ok(!get(orbDismissals).includes(lowUsage[0].id), "low and exhausted notices have separate dismissal levels");
orbDismissals.dismiss(collectAgentAlerts([claudeIdle], "pt-BR", now)[0].id);
const nextCycle = collectAgentAlerts([{ ...claudeIdle, rateLimits: [{ ...claudeIdle.rateLimits[0], resetsAt: now + 120_000 }] }], "pt-BR", now)[0];
assert.ok(!get(orbDismissals).includes(nextCycle.id), "a new usage cycle must be allowed to notify");
workspaceDismissals.dismiss("permission:session:request");
assert.ok(!get(workspaceDismissals).includes("permission:session:request"), "permission requests must remain outside account usage dismissals");
const unavailableStorage = createUsageAlertDismissals({ localStorage: { getItem() { throw new Error("storage unavailable"); }, setItem() { throw new Error("storage unavailable"); } }, addEventListener() {} });
unavailableStorage.dismiss(exhausted[0].id);
assert.ok(get(unavailableStorage).includes(exhausted[0].id));

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
workspaceDismissals.dismiss(usageFailure[0].id);
assert.ok(get(orbDismissals).includes(usageFailure[0].id), "actual quota failures must also stay dismissed across views");

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
