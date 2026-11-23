import assert from "node:assert/strict";
import { parsePromptIntent, promptIntentOf, promptWithIntent } from "../src/lib/promptIntent.ts";
import { cleanPromptTransport } from "../src/lib/chatAttachments.ts";

assert.deepEqual(parsePromptIntent("/todo corrigir o login\ne os testes"), { intent: "todo", body: "corrigir o login\ne os testes" });
assert.deepEqual(parsePromptIntent("/GOAL  ship it"), { intent: "goal", body: "ship it" });
assert.deepEqual(parsePromptIntent("/planning"), { intent: "planning", body: "" });
assert.equal(parsePromptIntent("/todos fix"), null);
assert.equal(parsePromptIntent("fix /todo"), null);

const sent = promptWithIntent("corrigir o login", "goal", "codex");
assert.equal(promptIntentOf(sent), "goal");
assert.match(sent, /create_goal/);
assert.match(promptWithIntent("x", "todo", "claude_code"), /TodoWrite/);
assert.match(promptWithIntent("x", "planning", "antigravity"), /implementation_plan\.md/);
assert.equal(promptIntentOf("plain prompt"), null);
// The chat shows the person's words, not Lume's request.
assert.equal(cleanPromptTransport(sent), "corrigir o login");
