import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url), "utf8");
const mark = read("src/lib/ActivityStatusMark.svelte");
const trace = read("src/lib/ActivityTraceGroup.svelte");

// One drawing for every state, so a change morphs instead of swapping icons.
assert.match(mark, /class="ring"/);
assert.match(mark, /class="check"/);
assert.match(mark, /class="cross"/);
assert.ok(!/\{#if/.test(mark.slice(mark.indexOf("<svg"))), "no state swaps the drawing's elements");
// The arc keeps turning while it closes, and only then stops, so it never jumps.
assert.match(mark, /const CLOSING_MS = 460/);
assert.match(mark, /setTimeout\(\(\) => \(spinning = false\), CLOSING_MS\)/);
assert.match(mark, /\.running \.ring \{ stroke-dasharray: 26 74/);
assert.match(mark, /\.completed \.check, \.failed \.cross \{ stroke-dashoffset: 0; transition-delay: 170ms/);
assert.match(mark, /prefers-reduced-motion/);

// Every event uses it, and the old icons and the pulse are gone.
assert.equal((trace.match(/<ActivityStatusMark status=/g) ?? []).length, 6);
assert.ok(!/activity-pulse/.test(trace));

// The rail waits at the last mark and descends to the next event, which arrives after it.
assert.match(trace, /\.activity-row:last-child::before \{ bottom: calc\(100% - var\(--activity-row-height, 34px\) \/ 2\); \}/);
assert.match(trace, /transition: bottom 340ms/);
assert.match(trace, /animation-delay: calc\(var\(--step-delay\) \+ 90ms\)/);
assert.match(trace, /const hasTimeline = \$derived\(visibleRuns\.length > 0\)/);
// A row drawn again (its run grew, the group reopened) does not arrive a second time.
assert.match(trace, /class:settled=\{settled\(run\.id\)\}/);

console.log("activity status mark test passed");
