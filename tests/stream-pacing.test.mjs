import assert from "node:assert/strict";
import { test } from "node:test";
import { MAX_HOLD_MS, STREAM_STEP_MS, stepsForBacklog, wordsForStep } from "../src/lib/streamPacing.ts";

const seconds = (steps) => (steps * STREAM_STEP_MS) / 1000;

test("a longer text gets more time, but never more than a few seconds", () => {
  assert.ok(seconds(stepsForBacklog(40)) < 0.7, "a short sentence is quick");
  assert.ok(seconds(stepsForBacklog(500)) > seconds(stepsForBacklog(100)));
  assert.equal(stepsForBacklog(6_000), stepsForBacklog(60_000), "the plan is capped");
  assert.ok(seconds(stepsForBacklog(6_000)) <= 2.8);
  assert.ok(MAX_HOLD_MS > seconds(stepsForBacklog(6_000)) * 1000, "the hold outlasts the longest plan");
});

test("the words per step finish the backlog in the planned steps", () => {
  for (const [words, steps] of [[500, 30], [80, 12], [3, 6], [1, 6]]) {
    let left = words;
    let taken = 0;
    for (let step = steps; left > 0; step = Math.max(1, step - 1)) {
      left -= wordsForStep(left, step);
      taken += 1;
      assert.ok(taken <= steps + 1, `${words} words overran ${steps} steps`);
    }
  }
  assert.equal(wordsForStep(0, 5), 1, "always moves forward");
  assert.equal(wordsForStep(10, 0), 10, "no steps left reveals the rest");
});
