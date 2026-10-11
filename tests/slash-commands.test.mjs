import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import {
  agentSlashCommands,
  filterSlashCommands,
  findSlashCommand,
  loadAgentSlashCommands,
  slashCommandQuery,
  slashCommandText,
} from "../src/lib/slashCommands.ts";

const claudeReported = [
  { name: "model", description: "Set the AI model", argumentHint: "[model]", prefix: "/", kind: "command" },
  { name: "compact", description: "Clear history but keep a summary", argumentHint: null, prefix: "/", kind: "command" },
  { name: "code-review", description: "Review the current diff", argumentHint: "[low|high]", prefix: "/", kind: "command" },
];

test("a leading slash or dollar without spaces opens the menu", () => {
  assert.deepEqual(slashCommandQuery("/"), { prefix: "/", query: "" });
  assert.deepEqual(slashCommandQuery("  $Lin"), { prefix: "$", query: "lin" });
  assert.equal(slashCommandQuery("/compact agora"), null);
  assert.equal(slashCommandQuery("compact"), null);
});

test("the menu lists what the agent reported, with /model owned by Lume", () => {
  const commands = agentSlashCommands(claudeReported, "claude_code");
  assert.deepEqual(commands.map((command) => [command.source, command.name]), [
    ["lume", "model"],
    ["lume", "mcp"],
    ["lume", "clear"],
    ["agent", "compact"],
    ["agent", "code-review"],
  ]);
  assert.equal(findSlashCommand(commands, "/mcp").action, "mcp");
  assert.equal(findSlashCommand(commands, "/model").action, "model");
  assert.equal(findSlashCommand(commands, "/code-review").argumentHint, "[low|high]");
  const antigravity = agentSlashCommands([], "antigravity");
  assert.deepEqual(antigravity.map((command) => [command.source, command.name]), [["lume", "model"], ["lume", "clear"]]);
  assert.equal(antigravity[0].description, "Choose the model and permissions");
  const reportedByAntigravity = agentSlashCommands(claudeReported, "antigravity");
  assert.deepEqual(reportedByAntigravity.map((command) => [command.source, command.name]), [
    ["lume", "model"],
    ["lume", "clear"],
    ["agent", "compact"],
    ["agent", "code-review"],
  ]);
});

test("/clear starts a new conversation instead of being sent, whatever the agent reports", () => {
  const reported = [
    ...claudeReported,
    { name: "clear", description: "Clear conversation history", argumentHint: null, prefix: "/", kind: "command" },
    { name: "reset", description: "Alias of clear", argumentHint: null, prefix: "/", kind: "command" },
  ];
  for (const agent of ["claude_code", "codex", "opencode", "antigravity", "omp"]) {
    const commands = agentSlashCommands(reported, agent);
    assert.equal(commands.filter((command) => command.name === "clear").length, 1);
    assert.equal(findSlashCommand(commands, "/clear").action, "clear");
    assert.equal(findSlashCommand(commands, "/new").action, "clear");
    assert.equal(findSlashCommand(commands, "/reset").action, "clear");
    assert.equal(commands.some((command) => command.name === "reset"), false);
  }
});

test("slash browses everything and dollar narrows to Codex skills", () => {
  const commands = agentSlashCommands(
    [{ name: "lint", description: "Run the linter", prefix: "$", kind: "skill" }],
    "codex",
  );
  assert.deepEqual(filterSlashCommands(commands, null), []);
  assert.equal(filterSlashCommands(commands, slashCommandQuery("/")).length, 5, "model, mcp, clear and plan are Lume's, plus the skill");
  assert.deepEqual(
    filterSlashCommands(commands, slashCommandQuery("$")).map((command) => command.name),
    ["lint"],
  );
  assert.deepEqual(
    filterSlashCommands(commands, slashCommandQuery("/linter")).map((command) => command.name),
    ["lint"],
  );
  assert.equal(slashCommandText(findSlashCommand(commands, "$lint")), "$lint ");
  assert.equal(slashCommandText(findSlashCommand(commands, "/model")), "/model");
});

test("Plan mode is Lume's /plan for Codex, once, and the agent's own /plan is not repeated", () => {
  const reported = [
    { name: "plan", description: "Switch to Plan mode", prefix: "/", kind: "command" },
    { name: "review", description: "Review the changes", prefix: "/", kind: "command" },
  ];
  const codex = agentSlashCommands(reported, "codex");
  assert.deepEqual(codex.filter((command) => command.name === "plan").map((command) => command.source), ["lume"]);
  assert.equal(findSlashCommand(codex, "/plan").action, "plan");
  assert.ok(findSlashCommand(codex, "/review"), "the other commands the agent reports stay");
  // Claude has no Plan toggle of Lume's: its own /plan is simply forwarded.
  const claude = agentSlashCommands(reported, "claude_code");
  assert.deepEqual(claude.filter((command) => command.name === "plan").map((command) => command.source), ["agent"]);
});

test("each session asks the agent once and retries after a failure", async () => {
  let calls = 0;
  const list = async () => {
    calls += 1;
    if (calls === 1) throw new Error("agent offline");
    return claudeReported;
  };
  await assert.rejects(loadAgentSlashCommands("session-test", list));
  assert.deepEqual(await loadAgentSlashCommands("session-test", list), claudeReported);
  assert.deepEqual(await loadAgentSlashCommands("session-test", list), claudeReported);
  assert.equal(calls, 2);
});

test("the orb terminal and the workspace load the agent's own commands", async () => {
  for (const file of ["TerminalWindow.svelte", "WorkspaceSessionPane.svelte"]) {
    const source = await readFile(new URL(`../src/lib/${file}`, import.meta.url), "utf8");
    assert.match(source, /loadAgentSlashCommands\(sessionId, listSessionSlashCommands\)/, file);
    assert.match(source, /class="slash-command-menu"/, file);
    assert.doesNotMatch(source, /claudeSlashCommands|codexSlashCommands/, file);
  }
});
