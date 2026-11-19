import assert from "node:assert/strict";
import { effortName } from "../src/lib/interruptNotice.ts";
import { effortAfterModelChange, effortsFor } from "../src/lib/modelChoices.ts";

const effort = (...values) => values.map((value) => ({ value }));
const models = [
  { model: "big", displayName: "Big", description: "", isDefault: true, defaultReasoningEffort: "medium", supportedReasoningEfforts: effort("low", "medium", "high", "xhigh") },
  { model: "small", displayName: "Small", description: "", isDefault: false, defaultReasoningEffort: "low", supportedReasoningEfforts: effort("low", "medium") },
  { model: "plain", displayName: "Plain", description: "", isDefault: false, defaultReasoningEffort: "", supportedReasoningEfforts: [] },
];

assert.deepEqual(effortsFor(models, "big"), ["low", "medium", "high", "xhigh"]);
assert.deepEqual(effortsFor(models, "plain"), []);
assert.deepEqual(effortsFor(models, "unknown"), []);

// The chosen effort survives a model that has it; otherwise the new model's own default is used.
assert.equal(effortAfterModelChange(models, "small", "medium"), "medium");
assert.equal(effortAfterModelChange(models, "small", "xhigh"), "low");
assert.equal(effortAfterModelChange(models, "plain", "high"), "");
assert.equal(effortAfterModelChange(models, "unknown", "high"), "high");

const pt = (_en, br) => br;
assert.equal(effortName("extra-low", pt), "Muito baixo");
assert.equal(effortName("xhigh", pt), "Muito alto");
assert.equal(effortName("max", pt), "Máximo");
console.log("model choices test passed");
