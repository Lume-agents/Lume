import assert from "node:assert/strict";
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
