import assert from "node:assert/strict";
import { changedRanges, colorize, foldUnchanged, pairForSplit, presentLines, withChanges } from "../src/lib/diffPresentation.ts";

// Word-level changes keep the shared head and tail out of the highlight.
const ranges = changedRanges("const total = price * 2;", "const total = price * quantity;");
assert.deepEqual(ranges.before, [[22, 23]]);
assert.deepEqual(ranges.after, [[22, 30]]);
assert.deepEqual(changedRanges("same", "same"), { before: [], after: [] });
assert.deepEqual(changedRanges("alpha beta gamma", "x"), { before: [], after: [] }, "unrelated lines are not highlighted word by word");

// Unchanged runs far from a change fold; short runs and expanded ones do not.
const items = Array.from({ length: 30 }, (_, index) => ({ change: index === 15 }));
const folded = foldUnchanged(items, (item) => item.change, () => true, new Set());
assert.deepEqual(folded.filter((row) => row.type === "fold").map((row) => row.count), [12, 11]);
assert.equal(folded.filter((row) => row.type === "item").length, 7);
const open = foldUnchanged(items, (item) => item.change, () => true, new Set(["0-12"]));
assert.equal(open.filter((row) => row.type === "fold").length, 1);
assert.equal(foldUnchanged(items.slice(12, 20), (item) => item.change, () => true, new Set()).some((row) => row.type === "fold"), false);

// The colorizer separates strings, comments, numbers, keywords and calls.
const kinds = colorize('return call("x", 42) // done', "a.ts").filter((segment) => segment.kind).map((segment) => `${segment.kind}:${segment.text.trim()}`);
assert.deepEqual(kinds, ["kw:return", "fn:call", "str:\"x\"", "num:42", "com://done".replace("//done", "// done")]);
assert.equal(colorize("x = 1  # note", "a.py").at(-1).kind, "com");
assert.equal(colorize("// text", "a.json").every((segment) => segment.kind !== "com"), true);

// Changed ranges split colored segments without losing text.
const split = withChanges(colorize("let a = 1;", "a.ts"), [[4, 5]]);
assert.equal(split.map((segment) => segment.text).join(""), "let a = 1;");
assert.equal(split.filter((segment) => segment.changed).map((segment) => segment.text).join(""), "a");

// Removed/added blocks pair line by line and get word highlights; split view aligns them.
const lines = [
  { kind: "context", content: "x", oldLine: 1, newLine: 1 },
  { kind: "removed", content: "let n = 1;", oldLine: 2 },
  { kind: "added", content: "let n = 2;", newLine: 2 },
  { kind: "added", content: "let m = 3;", newLine: 3 },
];
const cells = presentLines(lines, "a.ts");
assert.equal(cells[1].segments.some((segment) => segment.changed && segment.text === "1"), true);
assert.equal(cells[2].segments.some((segment) => segment.changed && segment.text === "2"), true);
const pairs = pairForSplit(cells);
assert.equal(pairs.length, 3);
assert.equal(pairs[2].left, undefined);
console.log("diff presentation tests passed");
