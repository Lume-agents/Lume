import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { setTimeout as delay } from "node:timers/promises";
import { test } from "node:test";
import { TerminalOpeningTimeoutError, waitForTerminalWindow } from "../src/lib/terminalOpening.ts";

test("returns immediately for an already visible terminal", async () => {
  const windows = [{ label: "terminal-a", sessionId: "a" }];
  assert.equal(await waitForTerminalWindow("terminal-a", async () => windows), windows);
});

test("waits for the requested terminal, not merely another open window", async () => {
  let reads = 0;
  const windows = [{ label: "terminal-a" }];
  const result = await waitForTerminalWindow("terminal-a", async () => {
    reads += 1;
    return reads < 3 ? [{ label: "terminal-b" }] : windows;
  }, { pollIntervalMs: 1 });
  assert.equal(reads, 3);
  assert.equal(result, windows);
});

test("stops polling after a terminal fails to load", async () => {
  let reads = 0;
  await assert.rejects(waitForTerminalWindow("missing", async () => {
    reads += 1;
    return [];
  }, { timeoutMs: 15, pollIntervalMs: 1 }), TerminalOpeningTimeoutError);
  const stoppedAt = reads;
  await delay(10);
  assert.equal(reads, stoppedAt);
});

test("preserves native errors and cancels the retry timer", async () => {
  const failure = new Error("Native window unavailable");
  let reads = 0;
  await assert.rejects(waitForTerminalWindow("terminal-a", async () => {
    reads += 1;
    throw failure;
  }), (error) => error === failure);
  await delay(5);
  assert.equal(reads, 1);
});

test("a stalled native read cannot keep Opening active forever", async () => {
  await assert.rejects(waitForTerminalWindow("terminal-a", () => new Promise(() => {}), {
    timeoutMs: 10,
  }), TerminalOpeningTimeoutError);
});

test("ignores a read that resolves after the opening deadline", async () => {
  let finishRead;
  let reads = 0;
  const opening = waitForTerminalWindow("terminal-a", () => {
    reads += 1;
    return new Promise((resolve) => { finishRead = resolve; });
  }, { timeoutMs: 10, pollIntervalMs: 1 });
  await assert.rejects(opening, TerminalOpeningTimeoutError);
  finishRead([{ label: "terminal-a" }]);
  await delay(5);
  assert.equal(reads, 1);
});

test("frontend readiness does not depend on painting an invisible window", () => {
  const source = readFileSync(new URL("../src/lib/TerminalWindow.svelte", import.meta.url), "utf8");
  const initialization = source.indexOf("await initializeTerminal();");
  const ready = source.indexOf("await markTerminalFrontendReady(label);", initialization);
  assert.ok(initialization > 0 && ready > initialization);
  assert.match(source.slice(initialization, ready), /await tick\(\)/);
  assert.doesNotMatch(source.slice(initialization, ready), /requestAnimationFrame/);
});

test("expanded session actions no longer contain the duplicate terminal button", () => {
  const source = readFileSync(new URL("../src/routes/+page.svelte", import.meta.url), "utf8");
  assert.doesNotMatch(source, /session-terminal-action/);
  assert.match(source, /waitForTerminalWindow\(label, loadTerminalWindows\)/);
  assert.match(source, /id: "terminal-open-error"/);
});
