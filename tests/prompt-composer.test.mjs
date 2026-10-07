import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import { test } from "node:test";

// Match SvelteKit's runtime alias without bundling or changing app imports.
registerHooks({
  resolve(specifier, context, nextResolve) {
    if (specifier.startsWith("$lib/")) {
      return nextResolve(new URL(`../src/lib/${specifier.slice(5)}.ts`, import.meta.url).href, context);
    }
    return nextResolve(specifier, context);
  },
});

const { caretOnEdgeLine, emptyPromptHistory, historyEntries, stepPromptHistory } = await import("../src/lib/promptHistory.ts");
const { applyMention, mentionAtCaret } = await import("../src/lib/promptMentions.ts");

test("history keeps the draft and walks from newest to oldest", () => {
  let history = { ...emptyPromptHistory(), entries: ["terceiro", "segundo", "primeiro"] };
  let step = stepPromptHistory(history, 1, "rascunho");
  assert.deepEqual([step.text, step.history.index], ["terceiro", 0]);
  history = step.history;
  step = stepPromptHistory(history, 1, history.entries[0]);
  assert.equal(step.text, "segundo");
  history = step.history;
  assert.equal(stepPromptHistory({ ...history, index: 2 }, 1, "primeiro"), null, "stops at the oldest");
  step = stepPromptHistory(history, -1, "segundo");
  step = stepPromptHistory(step.history, -1, "terceiro");
  assert.deepEqual([step.text, step.history.index], ["rascunho", -1], "returns to the draft");
});

test("history drops repeated hook and transcript copies", () => {
  assert.deepEqual(historyEntries(["oi", " oi ", "", undefined, "tchau", "oi"]), ["oi", "tchau", "oi"]);
});

test("arrows only browse history from the edge line", () => {
  assert.equal(caretOnEdgeLine("uma\nduas", 2, 1), true);
  assert.equal(caretOnEdgeLine("uma\nduas", 6, 1), false);
  assert.equal(caretOnEdgeLine("uma\nduas", 6, -1), true);
  assert.equal(caretOnEdgeLine("uma\nduas", 2, -1), false);
});

test("@ completes paths at the caret like the CLI", () => {
  assert.deepEqual(mentionAtCaret("veja @src/li", 12), { start: 5, query: "src/li" });
  assert.deepEqual(mentionAtCaret("@", 1), { start: 0, query: "" });
  assert.equal(mentionAtCaret("email@host", 10), null, "an @ inside a word is not a mention");
  assert.equal(mentionAtCaret("veja @src agora", 15), null);

  const text = "veja @src/li agora";
  const file = applyMention(text, 12, mentionAtCaret(text, 12), "src/lib/lume.ts", false);
  assert.deepEqual(file, { text: "veja @src/lib/lume.ts agora", caret: 21 });
  const folder = applyMention("@sr", 3, mentionAtCaret("@sr", 3), "src/", true);
  assert.deepEqual(folder, { text: "@src/", caret: 5 }, "folders keep completing");
});
