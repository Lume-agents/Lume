import assert from "node:assert/strict";
import {
  BOARD_CARD_HEIGHT,
  BOARD_CARD_WIDTH,
  addConnection,
  addStep,
  analyzeChain,
  cardAt,
  cardRect,
  checkConnection,
  clampZoom,
  connectionRefusalMessage,
  connectionWaitsForApproval,
  edgeGeometry,
  fitView,
  insertStep,
  newGroup,
  newStep,
  normalizeLayout,
  placeCard,
  preserveRoleOverrides,
  problemMessage,
  removeConnection,
  removeStep,
  stepVisualState,
  toBoardPoint,
  updateConnection,
  updateStep,
  zoomAround,
} from "../src/lib/workflowBoard.ts";

const contract = { instruction: "do it", expectedInput: "a goal", producedOutput: "a result", completionCondition: "done" };
const replacement = { instruction: "review", expectedInput: "diff", producedOutput: "findings", completionCondition: "verified" };
assert.deepEqual(preserveRoleOverrides(newStep("default", "implementer", contract), contract, replacement), replacement);
assert.deepEqual(preserveRoleOverrides({ ...newStep("custom", "implementer", contract), instruction: "Only inspect security" }, contract, replacement), { ...replacement, instruction: "Only inspect security" }, "role changes preserve customized contract fields");
const step = (key, role = "implementer") => newStep(key, role, contract);
const build = (...keys) => keys.reduce((group, key) => addStep(group, step(key)), newGroup());
const chain = (group, ...pairs) => pairs.reduce((next, [from, to]) => addConnection(next, `step-${from}`, `step-${to}`), group);
const ab0 = () => chain(build("a", "b"), ["a", "b"]).connections[0];
const codes = (group) => analyzeChain(group).problems.map((problem) => problem.code);

// ── a group the runtime can run ──
const line = chain(build("a", "b", "c"), ["a", "b"], ["b", "c"]);
assert.deepEqual(analyzeChain(line).order, ["step-a", "step-b", "step-c"]);
assert.equal(analyzeChain(line).start, "step-a");
assert.deepEqual(codes(line), []);
assert.deepEqual(analyzeChain(chain(build("a"))).order, ["step-a"], "one step is a valid, if short, route");

// ── why a group cannot run yet ──
assert.deepEqual(codes(newGroup()), ["empty"]);
assert.deepEqual(codes(build("a", "b")), ["multiple-starts"], "two unconnected steps are two starts");
const halfConnected = chain(build("a", "b", "c"), ["a", "b"]);
assert.deepEqual(codes(halfConnected), ["multiple-starts"], "the runtime asks for a single first step before anything else");
assert.deepEqual(analyzeChain(halfConnected).problems[0].stepIds, ["step-a", "step-c"], "the board points at every candidate start");
// A start exists, but part of the group runs in a ring of its own.
const ring = build("a", "b", "c", "d");
const ringed = { ...ring, connections: ["a>b", "c>d", "d>c"].map((pair, index) => ({ ...ab0(), id: `r${index}`, fromStepId: `step-${pair[0]}`, toStepId: `step-${pair[2]}` })) };
assert.ok(codes(ringed).includes("disconnected"));
assert.deepEqual(analyzeChain(ringed).loose, ["step-c", "step-d"]);
assert.ok(codes(updateStep(build("a"), "step-a", { role: "custom" })).includes("custom-label"));
assert.ok(codes(updateStep(build("a"), "step-a", { expectedInput: " " })).includes("contract"));
const forged = { ...line, connections: [...line.connections, { ...line.connections[0], id: "extra", toStepId: "step-c" }] };
assert.ok(codes(forged).includes("branch-out"), "data that did not come from the board is still checked");
assert.ok(codes(forged).includes("branch-in"));
const loop = { ...line, connections: [...line.connections, { ...line.connections[0], id: "back", fromStepId: "step-c", toStepId: "step-a" }] };
assert.ok(codes(loop).includes("no-start"));

// ── what can be drawn ──
const three = build("a", "b", "c");
assert.deepEqual(checkConnection(three, "step-a", "step-a"), { ok: false, reason: "same-step" });
const ab = chain(three, ["a", "b"]);
assert.deepEqual(checkConnection(ab, "step-a", "step-b"), { ok: false, reason: "exists" });
assert.deepEqual(checkConnection(ab, "step-a", "step-c"), { ok: false, reason: "has-outgoing" });
assert.deepEqual(checkConnection(ab, "step-c", "step-b"), { ok: false, reason: "has-incoming" });
assert.deepEqual(checkConnection(ab, "step-b", "step-c"), { ok: true });
const abc = chain(three, ["a", "b"], ["b", "c"]);
assert.deepEqual(checkConnection(abc, "step-c", "step-a"), { ok: false, reason: "cycle" });
assert.equal(addConnection(abc, "step-c", "step-a"), abc, "a refused arrow changes nothing");
assert.match(connectionRefusalMessage("cycle", true), /laço/);
assert.match(connectionRefusalMessage("has-outgoing", false), /outgoing arrow/);
assert.match(problemMessage({ code: "multiple-starts", stepIds: [] }, true), /uma etapa/i);
// ── defaults of a new arrow ──
// ── the same defaults the orb gives a new arrow ──
// A new arrow asks for approval and then goes on by itself: manual and approval were the same pause twice.
const [connection] = ab.connections;
assert.equal(connection.requiresApproval, true);
assert.equal(connection.advanceMode, "automatic");


assert.equal(connection.contextPolicy, "standard");
assert.deepEqual(connection.contextSelection, { response: true, files: true, checks: true, plan: false, activity: false, diffs: false });
assert.equal(updateConnection(ab, connection.id, { advanceMode: "automatic" }).connections[0].advanceMode, "automatic");

// ── a session appears once ──
assert.equal(addStep(three, step("a")), three);

// ── removing keeps the route whole ──
const healed = removeStep(abc, "step-b");
assert.deepEqual(healed.steps.map((item) => item.id), ["step-a", "step-c"]);
assert.equal(healed.connections.length, 1);
assert.equal(healed.connections[0].fromStepId, "step-a");
assert.equal(healed.connections[0].toStepId, "step-c");
assert.equal(healed.connections[0].id, abc.connections[0].id, "A keeps passing on what it was passing on");
assert.deepEqual(analyzeChain(healed).order, ["step-a", "step-c"]);
assert.equal(removeStep(abc, "step-a").connections.length, 1, "removing the first step leaves the rest connected");
assert.equal(removeStep(abc, "step-c").connections.length, 1);
assert.equal(removeConnection(abc, abc.connections[0].id).connections.length, 1);

// ── inserting a step in the middle of an arrow ──
const two = chain(build("a", "b"), ["a", "b"]);
const arrow = two.connections[0];
const withMiddle = insertStep(two, arrow.id, step("n"));
assert.deepEqual(analyzeChain(withMiddle).order, ["step-a", "step-n", "step-b"]);
assert.deepEqual(codes(withMiddle), []);
assert.equal(withMiddle.connections.length, 2);
assert.equal(withMiddle.connections[0].id, arrow.id);
assert.equal(withMiddle.connections[0].toStepId, "step-n");
assert.equal(withMiddle.connections[1].fromStepId, "step-n");
assert.equal(withMiddle.connections[1].toStepId, "step-b");
assert.notEqual(withMiddle.connections[1].id, arrow.id);
const customized = updateConnection(two, arrow.id, { requiresApproval: false, additionalInstruction: "be brief" });
const customizedMiddle = insertStep(customized, arrow.id, step("n"));
assert.ok(customizedMiddle.connections.every((item) => !item.requiresApproval && item.additionalInstruction === "be brief"), "both halves carry what the arrow carried");
assert.equal(insertStep(two, arrow.id, step("a")), two, "an existing session cannot be inserted twice");
assert.equal(insertStep(two, "missing", step("n")), two);

// ── geometry ──
const left = cardRect({ x: 0, y: 0 });
const right = cardRect({ x: 500, y: 20 });
const forward = edgeGeometry(left, right);
assert.deepEqual(forward.start, { x: BOARD_CARD_WIDTH, y: BOARD_CARD_HEIGHT / 2 });
assert.deepEqual(forward.end, { x: 500, y: 20 + BOARD_CARD_HEIGHT / 2 });
assert.match(forward.path, /^M .* C /);
assert.ok(forward.mid.x > forward.start.x && forward.mid.x < forward.end.x);
const backward = edgeGeometry(right, left);
assert.equal(backward.start.x, 500, "an arrow going left leaves from the left side");
assert.equal(backward.end.x, BOARD_CARD_WIDTH);
const below = edgeGeometry(left, cardRect({ x: 10, y: 400 }));
assert.equal(below.start.y, BOARD_CARD_HEIGHT, "a card straight below is reached from the bottom");
assert.equal(below.end.y, 400);

// ── picking and placing ──
const positions = { "step-a": { x: 0, y: 0 }, "step-b": { x: 100, y: 50 } };
assert.equal(cardAt({ x: 150, y: 80 }, positions, ["step-a", "step-b"]), "step-b", "the topmost card wins where they overlap");
assert.equal(cardAt({ x: 150, y: 80 }, positions, ["step-b", "step-a"]), "step-a");
assert.equal(cardAt({ x: 900, y: 900 }, positions, ["step-a"]), null);
assert.deepEqual(placeCard([]), { x: 96, y: 120 });
assert.deepEqual(placeCard([{ x: 96, y: 120 }, { x: 500, y: 140 }]), { x: 500 + BOARD_CARD_WIDTH + 84, y: 140 });
assert.deepEqual(placeCard([], { x: 400, y: 300 }), { x: 400 - BOARD_CARD_WIDTH / 2, y: 300 - BOARD_CARD_HEIGHT / 2 });

// ── pan and zoom ──
const view = { x: 40, y: 30, zoom: 2 };
assert.deepEqual(toBoardPoint({ x: 140, y: 130 }, { x: 0, y: 0 }, view), { x: 50, y: 50 });
assert.deepEqual(toBoardPoint({ x: 340, y: 230 }, { x: 200, y: 100 }, view), { x: 50, y: 50 });
const zoomed = zoomAround({ x: 0, y: 0, zoom: 1 }, { x: 300, y: 200 }, 2);
assert.deepEqual(toBoardPoint({ x: 300, y: 200 }, { x: 0, y: 0 }, zoomed), { x: 300, y: 200 }, "the point under the cursor stays put");
assert.equal(clampZoom(9), 1.6);
assert.equal(clampZoom(0.01), 0.35);
const fitted = fitView([{ x: 0, y: 0 }, { x: 3000, y: 100 }], 1000, 600);
assert.ok(fitted.zoom < 1 && fitted.zoom >= 0.35);
assert.deepEqual(fitView([], 1000, 600), { x: 0, y: 0, zoom: 1 });

// ── saved layout ──
assert.deepEqual(normalizeLayout(null), { name: undefined, positions: {}, view: { x: 0, y: 0, zoom: 1 } });
const restored = normalizeLayout({ name: "  Review loop  ", positions: { a: { x: 1, y: 2 }, b: { x: "no", y: 1 }, c: null }, view: { x: 5, y: 6, zoom: 99 } });
assert.equal(restored.name, "Review loop");
assert.deepEqual(restored.positions, { a: { x: 1, y: 2 } });
assert.deepEqual(restored.view, { x: 5, y: 6, zoom: 1.6 });

// ── run state ──
const run = {
  status: "waiting_for_approval",
  currentStepId: "step-a",
  pendingConnectionId: "arrow-1",
  steps: [{ stepId: "step-a", status: "completed" }, { stepId: "step-b", status: "pending" }],
};
assert.equal(stepVisualState(run, "step-a"), "completed");
assert.equal(stepVisualState(run, "step-b"), "pending");
assert.equal(stepVisualState(run, "step-z"), "idle");
assert.equal(stepVisualState(null, "step-a"), "idle");
assert.equal(connectionWaitsForApproval(run, "arrow-1"), true);
assert.equal(connectionWaitsForApproval(run, "arrow-2"), false);
assert.equal(connectionWaitsForApproval({ ...run, status: "running" }, "arrow-1"), false);
