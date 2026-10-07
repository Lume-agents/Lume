import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { createSurfaceSizeQueue } from "../src/lib/surfaceSizing.ts";

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
  assert.match(orb, /function beginSessionRename\(session: AgentSession\) \{\s*void bringOverlayToFront\(\);[\s\S]*?void focusOrbField\("\.session-name-editor input", true\);/);
  assert.match(orb, /function toggleSessionComposer\(session: AgentSession\) \{[\s\S]*?void bringOverlayToFront\(\);\s*void focusOrbField\("\.inline-composer textarea"\);/);
  assert.match(orb, /async function focusOrbField\(selector: string, selectText = false\) \{\s*await tick\(\);[\s\S]*?field\.focus\(\{ preventScroll: true \}\);/);
});
