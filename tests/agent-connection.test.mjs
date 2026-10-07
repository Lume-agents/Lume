import assert from "node:assert/strict";
import { agentConnectionMessage, agentLoginCommand } from "../src/lib/agentConnection.ts";
import { claudeEffortForModel, claudeEffortValues, claudeModelOptions } from "../src/lib/claudeModels.ts";

assert.equal(
  agentConnectionMessage("Error: AGENT_CONNECTION_REQUIRED:Claude Code não está conectado."),
  "Claude Code não está conectado.",
);
assert.equal(agentConnectionMessage("OpenCode: Authentication required"), "OpenCode: Authentication required");
assert.equal(agentConnectionMessage("Please sign in to continue"), "Please sign in to continue");
assert.equal(agentConnectionMessage("Agente não conectado"), "Agente não conectado");
assert.equal(agentConnectionMessage("Connection reset by peer"), null);
assert.equal(agentLoginCommand("claude"), "claude auth login");
assert.equal(agentLoginCommand("opencode"), "opencode auth login");

const efforts = (...values) => values.map((value) => ({ value, description: "" }));
const models = [
  { model: "claude-opus-5-5", displayName: "Opus 5.5", description: "Complex work", isDefault: false,
    defaultReasoningEffort: "high", supportedReasoningEfforts: efforts("low", "medium", "high", "xhigh", "max", "ultra") },
  { model: "claude-sonnet-5-5", displayName: "Sonnet 5.5", description: "Routine tasks", isDefault: true,
    defaultReasoningEffort: "high", supportedReasoningEfforts: efforts("low", "medium", "high", "xhigh", "max") },
  { model: "claude-sonnet-4-6", displayName: "Sonnet 4.6", description: "Older", isDefault: false,
    defaultReasoningEffort: "high", supportedReasoningEfforts: efforts("low", "medium", "high", "max") },
  { model: "claude-haiku-4-5", displayName: "Haiku 4.5", description: "Fast", isDefault: false,
    defaultReasoningEffort: "", supportedReasoningEfforts: [] },
];
const options = claudeModelOptions(models, (_, portuguese) => portuguese);
assert.deepEqual(options.map((option) => option.value), models.map((model) => model.model), "no session-default entry");
assert.equal(options[1].description, "Padrão do Claude Code");
assert.ok(claudeEffortValues(models, "claude-opus-5-5").includes("ultra"));
assert.deepEqual(claudeEffortValues(models, "claude-haiku-4-5"), []);
assert.equal(claudeEffortForModel(models, "claude-sonnet-4-6", "max"), "max");
assert.equal(claudeEffortForModel(models, "claude-sonnet-4-6", "xhigh"), "high", "unsupported effort falls back");
assert.equal(claudeEffortForModel(models, "claude-haiku-4-5", "max"), "");
console.log("agent connection test suite passed");
