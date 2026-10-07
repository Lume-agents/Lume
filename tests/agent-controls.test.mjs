import assert from "node:assert/strict";
import { test } from "node:test";
import { controlsDiffer } from "../src/lib/agentControls.ts";

const original = { model: "gpt-5", effort: "high", fast: false };

test("nothing to restore until the first read and while nothing changed", () => {
  assert.equal(controlsDiffer(null, { model: "other", effort: "low", fast: true }, true), false);
  assert.equal(controlsDiffer(original, { ...original }, true), false);
});

test("a different model or effort enables the restore", () => {
  assert.equal(controlsDiffer(original, { ...original, model: "gpt-5-mini" }, true), true);
  assert.equal(controlsDiffer(original, { ...original, effort: "xhigh" }, true), true);
});

test("Fast counts only for agents that have it", () => {
  assert.equal(controlsDiffer(original, { ...original, fast: true }, true), true);
  assert.equal(controlsDiffer(original, { ...original, fast: true }, false), false, "Claude has no Fast");
});

test("changing back to the original settles the button again", () => {
  const changed = { model: "gpt-5-mini", effort: "low", fast: true };
  assert.equal(controlsDiffer(original, changed, true), true);
  assert.equal(controlsDiffer(original, { ...original }, true), false);
});
