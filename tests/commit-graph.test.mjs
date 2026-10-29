import assert from "node:assert/strict";
import { layoutGraph, graphWidth } from "../src/lib/commitGraph.ts";

// A straight line stays in lane 0.
const line = layoutGraph([{ oid: "c", parents: ["b"] }, { oid: "b", parents: ["a"] }, { oid: "a", parents: [] }]);
assert.deepEqual(line.map((row) => row.lane), [0, 0, 0]);
assert.deepEqual(line.map((row) => row.continues), [false, true, true]);
assert.deepEqual(line.map((row) => row.to), [[0], [0], []]);
assert.equal(graphWidth(line), 1);

// A feature branch merged back: m(main, feature) -> main1 and f; both reach the root.
const merge = layoutGraph([
  { oid: "m", parents: ["main1", "f"] },
  { oid: "f", parents: ["root"] },
  { oid: "main1", parents: ["root"] },
  { oid: "root", parents: [] },
]);
assert.deepEqual(merge[0].to, [0, 1], "a merge sends one line to each parent");
assert.equal(merge[1].lane, 1, "the feature commit is in its own lane");
assert.deepEqual(merge[1].through, [0], "the main line passes beside it");
assert.equal(merge[2].lane, 0, "the main line stays in lane 0");
assert.deepEqual(merge[3].from, [0, 1], "both lines meet at the root commit");
assert.equal(graphWidth(merge), 2);

// Two branches that start from the same commit leave it in separate lanes.
const fork = layoutGraph([
  { oid: "x", parents: ["base"] },
  { oid: "y", parents: ["base"] },
  { oid: "base", parents: [] },
]);
assert.deepEqual(fork.map((row) => row.lane), [0, 1, 0]);
assert.deepEqual(fork[2].from, [0, 1]);

// A very wide history is clamped, never producing a lane past the limit.
const wide = layoutGraph(Array.from({ length: 30 }, (_, index) => ({ oid: `c${index}`, parents: [`p${index}`] })), 6);
assert.ok(wide.every((row) => row.lane <= 6));
assert.equal(layoutGraph([]).length, 0);
console.log("commit graph test passed");
