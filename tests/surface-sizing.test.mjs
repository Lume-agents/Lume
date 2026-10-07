import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { createSurfaceSizeQueue, settleSurfaceSize, surfaceMatches } from "../src/lib/surfaceSizing.ts";

test("coalesces obsolete sizes while keeping the latest geometry", async () => {
  const applied = [];
  let finishFirst;
  const resize = createSurfaceSizeQueue(async (size) => {
    applied.push(size);
    if (applied.length === 1) await new Promise((resolve) => { finishFirst = resolve; });
  });
  const first = resize({ width: 78, height: 44 });
  const second = resize({ width: 200, height: 300, syncLinuxSurface: true });
  const third = resize({ width: 392, height: 600 });
  finishFirst();
  await Promise.all([first, second, third]);
  assert.deepEqual(applied, [
    { width: 78, height: 44, syncLinuxSurface: false },
    { width: 392, height: 600, syncLinuxSurface: true },
  ]);
});

test("recovers from one rejected resize without dropping a later size", async () => {
  let calls = 0;
  let finishFirst;
  const resize = createSurfaceSizeQueue(async () => {
    calls++;
    if (calls === 1) await new Promise((_, reject) => { finishFirst = reject; });
  });
  const first = resize({ width: 78, height: 44 });
  const second = resize({ width: 392, height: 600 });
  finishFirst(new Error("GTK surface unavailable"));
  await assert.rejects(first, /GTK surface unavailable/);
  await second;
  assert.equal(calls, 2);
});

test("remeasures the expanded Orb after its opening morph completes", () => {
  const orb = readFileSync(new URL("../src/routes/+page.svelte", import.meta.url), "utf8");
  const toggleStart = orb.indexOf("async function toggleExpanded()");
  const animateStart = orb.indexOf("async function animateCapsule(", toggleStart);
  const toggle = orb.slice(toggleStart, animateStart);
  assert.match(toggle, /morphing = null;[\s\S]*?if \(opening\) \{\s*(?:\/\/[^\n]*\n\s*)*await tick\(\);\s*const panel = document\.querySelector<HTMLElement>\("\.panel"\);\s*if \(panel\) applyExpandedHeight\(panel\.offsetHeight, true\);/);
});

test("Orb rename and continue controls request window focus and focus their mounted field", () => {
  const orb = readFileSync(new URL("../src/routes/+page.svelte", import.meta.url), "utf8");
  assert.match(orb, /function beginSessionRename\(session: AgentSession\) \{\s*void bringOverlayToFront\(true\);[\s\S]*?void focusOrbField\("\.session-name-editor input", true\);/);
  assert.match(orb, /function toggleSessionComposer\(session: AgentSession\) \{[\s\S]*?void bringOverlayToFront\(true\);\s*void focusOrbField\("\.inline-composer textarea"\);/);
  // The text boxes always ask for the keyboard; an ordinary click only when the Orb lacks it, and
  // the click listener must not hand its event over as the `force` argument.
  assert.match(orb, /async function bringOverlayToFront\(force = false\) \{\s*if \(!isTauri\) return;(?:\s*\/\/[^\n]*)*\s*if \(!force && document\.hasFocus\(\)\) return;/);
  assert.match(orb, /function focusOverlayOnPointerDown\(\) \{\s*void bringOverlayToFront\(\);\s*\}/);
  assert.match(orb, /addEventListener\("pointerdown", focusOverlayOnPointerDown, true\)/);
  assert.match(orb, /activateOverlayWindow\(\)\.catch\(\(\) => false\)/, "XWayland is asked the way a window manager accepts");
  assert.match(orb, /async function focusOrbField\(selector: string, selectText = false\) \{\s*await tick\(\);[\s\S]*?field\.focus\(\{ preventScroll: true \}\);/);
});


const settleTarget = { width: 392, height: 310 };
const instantly = async () => undefined;

test("sizes match within the rounding a fractional scale causes", () => {
  assert.equal(surfaceMatches(settleTarget, { width: 392, height: 310 }), true);
  assert.equal(surfaceMatches(settleTarget, { width: 393, height: 309 }), true);
  assert.equal(surfaceMatches(settleTarget, { width: 392, height: 280 }), false, "a cut panel");
});

test("a window stuck at an intermediate size is asked again until it is right", async () => {
  // The window ends up 280 high after the animation, then follows the new request.
  const sizes = [280, 280, 310];
  let reads = 0;
  let applied = 0;
  const corrections = await settleSurfaceSize({
    target: settleTarget,
    wait: instantly,
    measure: async () => {
      const height = sizes[Math.min(reads, sizes.length - 1)];
      reads += 1;
      return [{ width: 392, height }, { width: 392, height }];
    },
    apply: async () => { applied += 1; },
  });
  assert.equal(corrections, 2);
  assert.equal(applied, 2);
});

test("either the viewport or the window being wrong counts, not only the window", async () => {
  let applied = 0;
  let reads = 0;
  await settleSurfaceSize({
    target: settleTarget,
    wait: instantly,
    measure: async () => {
      reads += 1;
      // The window is right but the WebView viewport is short: the content is clipped.
      return [{ width: 392, height: reads > 1 ? 310 : 270 }, { width: 392, height: 310 }];
    },
    apply: async () => { applied += 1; },
  });
  assert.equal(applied, 1);
});

test("nothing is asked when the sizes already match, and it gives up after a few tries", async () => {
  let applied = 0;
  const fine = await settleSurfaceSize({
    target: settleTarget, wait: instantly, apply: async () => { applied += 1; },
    measure: async () => [{ ...settleTarget }],
  });
  assert.equal(fine, 0);
  assert.equal(applied, 0);

  const stubborn = await settleSurfaceSize({
    target: settleTarget, wait: instantly, attempts: 3, apply: async () => { applied += 1; },
    measure: async () => [{ width: 392, height: 100 }],
  });
  assert.equal(stubborn, 3, "bounded: a window that never obeys does not loop forever");
});

test("a newer resize cancels the check", async () => {
  let measured = 0;
  await settleSurfaceSize({
    target: settleTarget, wait: instantly, shouldStop: () => true, apply: instantly,
    measure: async () => { measured += 1; return [{ width: 1, height: 1 }]; },
  });
  assert.equal(measured, 0);
});
