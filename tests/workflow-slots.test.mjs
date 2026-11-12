import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import test from "node:test";

registerHooks({
  resolve(specifier, context, nextResolve) {
    if (specifier.startsWith("$lib/")) {
      const path = specifier.slice(5);
      return nextResolve(new URL(`../src/lib/${path}${/\.[jt]s$/.test(path) ? "" : ".ts"}`, import.meta.url).href, context);
    }
    return nextResolve(specifier, context);
  },
});
const { arrangeSlots, basicSlotsFor, clearSlot, missingSlot, placeInSlot, slotRole } = await import("../src/lib/workflowSlots.ts");
const { analyzeChain, newGroup, newStep } = await import("../src/lib/workflowBoard.ts");

const contract = (role) => ({ instruction: "Do " + role, expectedInput: "Input", producedOutput: "Output", completionCondition: "Done" });
const contracts = Object.fromEntries(["planner", "implementer", "reviewer", "tester", "researcher", "custom"].map((role) => [role, contract(role)]));
const step = (key, role) => newStep(key, role, contracts[role]);
const roles = (group) => analyzeChain(group).order.map((id) => group.steps.find((item) => item.id === id).sessionNativeId + ":" + group.steps.find((item) => item.id === id).role);

test("a new workflow fits the slots, all empty", () => {
  assert.deepEqual(basicSlotsFor(newGroup()), { ok: true, slots: [null, null, null], third: "reviewer" });
  assert.equal(slotRole(0, "reviewer"), "planner");
  assert.equal(slotRole(2, "tester"), "tester");
});

test("filling slots builds the chain left to right, skipping the empty optional one", () => {
  const base = newGroup();
  const a = step("a", "researcher");
  const b = step("b", "planner");
  let group = arrangeSlots(base, ["step-a", "step-b", null], "reviewer", contracts, [a, b]);
  assert.deepEqual(roles(group), ["a:planner", "b:implementer"], "each card takes its slot's role");
  assert.equal(group.connections.length, 1);
  assert.deepEqual(analyzeChain(group).problems, []);
  assert.equal(group.steps[0].instruction, "Do planner", "default role fields follow the new role");
  group = arrangeSlots(group, ["step-a", "step-b", "step-c"], "reviewer", contracts, [step("c", "planner")]);
  assert.deepEqual(roles(group), ["a:planner", "b:implementer", "c:reviewer"]);
  assert.equal(group.connections.length, 2);
  // Taking the implementer out connects the orchestrator straight to the reviewer, but the slot is still required.
  const gap = arrangeSlots(group, clearSlot(basicSlotsFor(group).slots, 1), "reviewer", contracts);
  assert.deepEqual(roles(gap), ["a:planner", "c:reviewer"]);
  assert.equal(missingSlot(basicSlotsFor(gap).slots), 1);
  assert.equal(missingSlot([null, null, null]), null, "an empty workflow has nothing missing yet");
});

test("moving a card between slots swaps them and keeps what each arrow passed on", () => {
  let group = arrangeSlots(newGroup(), ["step-a", "step-b", null], "reviewer", contracts, [step("a", "planner"), step("b", "implementer")]);
  group = { ...group, connections: group.connections.map((item) => ({ ...item, additionalInstruction: "Keep it short" })), steps: group.steps.map((item) => item.id === "step-a" ? { ...item, instruction: "Tailored" } : item) };
  const fit = basicSlotsFor(group);
  assert.equal(fit.ok, true);
  const swapped = arrangeSlots(group, placeInSlot(fit.slots, "step-a", 1), "reviewer", contracts);
  assert.deepEqual(roles(swapped), ["b:planner", "a:implementer"]);
  assert.equal(swapped.steps.find((item) => item.id === "step-a").instruction, "Tailored", "a tailored field survives the role change");
  assert.equal(swapped.connections.length, 1);
  // Dropping a card from outside onto a filled slot replaces the card there.
  assert.deepEqual(placeInSlot(["x", "y", null], "z", 1), ["x", "z", null]);
  assert.deepEqual(placeInSlot(["x", null, null], "x", 2), [null, null, "x"]);
});

test("workflows the slots cannot show stay on the canvas", () => {
  const four = arrangeSlots(newGroup(), ["step-a", "step-b", "step-c"], "tester", contracts, [step("a", "planner"), step("b", "implementer"), step("c", "tester")]);
  const fit = basicSlotsFor(four);
  assert.deepEqual(fit, { ok: true, slots: ["step-a", "step-b", "step-c"], third: "tester" });
  assert.deepEqual(basicSlotsFor({ ...four, steps: [...four.steps, step("d", "researcher")] }), { ok: false, reason: "too-many" });
  const backwards = { ...four, connections: [] };
  assert.deepEqual(basicSlotsFor(backwards), { ok: false, reason: "route" }, "loose cards have no single route");
  const reversed = arrangeSlots(newGroup(), ["step-a", "step-b", null], "reviewer", {}, [step("a", "planner"), step("b", "implementer")]);
  const flipped = { ...reversed, connections: [{ ...reversed.connections[0], fromStepId: "step-b", toStepId: "step-a" }] };
  assert.deepEqual(basicSlotsFor(flipped), { ok: false, reason: "roles" }, "the implementer cannot come before the orchestrator");
  assert.deepEqual(basicSlotsFor({ ...reversed, steps: reversed.steps.map((item) => ({ ...item, role: "researcher" })) }), { ok: false, reason: "roles" });
});
