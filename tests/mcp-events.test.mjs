import assert from "node:assert/strict";
import { mcpActivityInfo, mcpEventTitle, mcpRunChips, mcpRunTitle, parseMcpToolName, sameMcpTool } from "../src/lib/mcpEvents.ts";
import { activityCategory, activityDisplayTitle, activityRunTitle, groupConsecutiveTraceActivities } from "../src/lib/activityPresentation.ts";

let counter = 0;
function call(tool, input, result, status = "completed", server = "atlassian") {
  counter += 1;
  return {
    id: `claude:s:tool:${counter}`, kind: "tool", title: `mcp__${server}__${tool}`, status, createdAt: counter, files: [], attachments: [],
    detail: JSON.stringify({ input, ...(result === undefined ? {} : { result }) }),
  };
}
const block = (data) => [{ type: "text", text: JSON.stringify({ data }) }];
const transition = (key, statusName) => call("transitionJiraIssue", { cloudId: "c", issueIdOrKey: key, transition: { id: "2" } },
  block({ message: `Issue ${key} transitioned successfully`, transitionId: 2, statusName }));

assert.deepEqual(parseMcpToolName("mcp__atlassian__transitionJiraIssue"), { server: "atlassian", tool: "transitionJiraIssue" });
assert.deepEqual(parseMcpToolName("mcp__claude_ai_Atlassian__getJiraIssue"), { server: "claude_ai_Atlassian", tool: "getJiraIssue" });
assert.equal(parseMcpToolName("Bash"), null);

// A ticket moved: named by the ticket and its new status, with a status chip.
const moved = transition("IPG-253", "REVISÃO");
const info = mcpActivityInfo(moved, "pt-BR");
assert.equal(info.serverLabel, "Jira");
assert.equal(mcpEventTitle(info, "pt-BR"), "IPG-253 → REVISÃO");
assert.deepEqual(info.chips.map((chip) => [chip.label, chip.tone]), [["REVISÃO", "warn"]]);
assert.equal(activityCategory(moved), "mcp");
assert.equal(activityDisplayTitle(moved, "pt-BR"), "IPG-253 → REVISÃO");
assert.equal(mcpEventTitle(mcpActivityInfo(transition("IPG-268", "IMPEDIDO")), "en"), "IPG-268 → IMPEDIDO");
assert.equal(mcpActivityInfo(transition("IPG-268", "IMPEDIDO")).chips[0].tone, "bad");

// An edit is a ticket edit, not "files edited", and it lists what it changed.
const edit = call("editJiraIssue", { issueIdOrKey: "IPG-258", fields: { description: "ipg-mobile#7", assignee: { accountId: "712020:x" } } }, block({ ok: true }));
assert.equal(activityCategory(edit), "mcp");
assert.equal(activityDisplayTitle(edit, "pt-BR"), "Editou IPG-258");
assert.deepEqual(mcpActivityInfo(edit).chips.map((chip) => chip.label), ["Description", "Assignee"]);
assert.deepEqual(mcpActivityInfo(edit).rows.slice(1), [["Description", "ipg-mobile#7"], ["Assignee", "712020:x"]], "an object argument reads as its name, not as JSON");

// A created ticket is named by the key the answer returns.
const created = call("createJiraIssue", { projectKey: "IPG", issueTypeName: "Task", summary: "Teste do PR #7 em aparelho" }, block({ id: "18257", key: "IPG-501" }));
assert.equal(activityDisplayTitle(created, "pt-BR"), "Criou IPG-501 · Teste do PR #7 em aparelho");
assert.equal(activityDisplayTitle(created, "en"), "Created IPG-501 · Teste do PR #7 em aparelho");

// A search counts what it found, and a cut-short answer still names the call.
const search = call("searchJiraIssuesUsingJql", { jql: "project = IPG", maxResults: 100 }, [{ type: "text", text: JSON.stringify({ issues: [{}, {}, {}] }) }]);
assert.match(activityDisplayTitle(search, "pt-BR"), /Buscou tickets no Jira · 3/);
const cut = { ...search, detail: '{"input":{"jql":"x"},"result":"[{\\"type\\":\\"text\\",\\"text\\":\\"{\\\\\\"issues\\\\\\":[{' };
assert.equal(mcpActivityInfo(cut).presenter, "jira:search");

// Calls of one tool in a row are one run: "23 tickets movidos", with a chip per status.
const runs = groupConsecutiveTraceActivities([...Array.from({ length: 22 }, (_, index) => transition(`IPG-${300 + index}`, "REVISÃO")), transition("IPG-268", "IMPEDIDO"), edit]);
assert.equal(runs.length, 2, "moves are one run and the edit another");
assert.equal(runs[0].activities.length, 23);
assert.equal(activityRunTitle(runs[0], "pt-BR"), "23 tickets movidos");
assert.equal(activityRunTitle(runs[0], "en"), "Moved 23 issues");
assert.deepEqual(mcpRunChips(runs[0].activities).map((chip) => chip.label), ["REVISÃO ×22", "IMPEDIDO ×1"]);
assert.equal(mcpRunTitle([created, { ...created, id: "x" }], "pt-BR"), "2 tickets criados");
assert.equal(sameMcpTool(moved, edit), false);

// Other servers get a readable title and a table of their arguments instead of raw JSON.
const other = call("list_pull_requests", { repo: "Vibetex/ipg-mobile", state: "open" }, [{ type: "text", text: "[]" }], "completed", "github");
const otherInfo = mcpActivityInfo(other, "en");
assert.equal(otherInfo.serverLabel, "Github");
assert.equal(activityDisplayTitle(other, "en"), "List pull requests · Vibetex/ipg-mobile");
assert.deepEqual(otherInfo.rows, [["Repo", "Vibetex/ipg-mobile"], ["State", "open"]]);

// A failed call says so, and one without a name that points at an MCP server is left alone.
const failed = { ...transition("IPG-1", "X"), status: "failed", detail: JSON.stringify({ input: { issueIdOrKey: "IPG-1" }, error: "Transition not allowed" }) };
assert.equal(mcpActivityInfo(failed).failed, true);
assert.equal(mcpActivityInfo(failed).error, "Transition not allowed");
const legacy = { id: "l", kind: "tool", title: '{"cloudId":"vibetex.atlassian.net","issueIdOrKey":"IPG-268"}', status: "completed", createdAt: 1, files: [], attachments: [], detail: "[]" };
assert.equal(mcpActivityInfo(legacy), null);
assert.equal(activityCategory(legacy), "tool");

// The archive keeps a call as small JSON whose answer is already data (not content blocks); it reads the same.
const archived = { id: "a", kind: "tool", title: "mcp__atlassian__transitionJiraIssue", status: "completed", createdAt: 1, files: [], attachments: [],
  detail: JSON.stringify({ input: { issueIdOrKey: "IPG-911", statusName: "REVISÃO" }, result: { data: { message: "Issue IPG-911 transitioned successfully", transitionId: 2, statusName: "REVISÃO" } } }) };
assert.equal(activityDisplayTitle(archived, "pt-BR"), "IPG-911 → REVISÃO");
assert.equal(mcpActivityInfo(archived).chips[0].label, "REVISÃO");

console.log("mcp events test passed");
