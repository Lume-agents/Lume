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
const { compactTokens, contextPercent, tokenGraphGeometry, tokenUsageSummary } = await import("../src/lib/tokenUsage.ts");

const sample = (totalTokens, createdAt, extra = {}) => ({ turnId: "t" + createdAt, totalTokens, inputTokens: totalTokens * 0.8, outputTokens: totalTokens * 0.2, createdAt, ...extra });

test("the chart spreads prompts over time and scales to the largest", () => {
  assert.deepEqual(tokenGraphGeometry([]), { points: "", area: "", dots: [] });
  const graph = tokenGraphGeometry([sample(100, 0), sample(400, 30), sample(200, 40)]);
  assert.deepEqual(graph.dots.map((dot) => Math.round(dot.x)), [4, 168, 222], "x follows the prompt time");
  assert.equal(graph.dots[1].y, 12, "the largest prompt reaches the top inset");
  assert.equal(graph.dots[1].sample.totalTokens, 400, "each dot carries its prompt");
  const single = tokenGraphGeometry([sample(50, 5)]);
  assert.equal(single.dots[0].x, 113, "a single prompt sits in the middle");
  assert.match(single.points, /^4,[\d.]+ 222,[\d.]+$/, "and draws a flat line across");
});

test("totals and context use", () => {
  const summary = tokenUsageSummary([sample(1000, 1), sample(3000, 2, { contextTokens: 50_000, contextWindow: 200_000 })]);
  assert.equal(summary.prompts, 2);
  assert.equal(summary.totalTokens, 4000);
  assert.equal(summary.inputTokens, 3200);
  assert.equal(summary.outputTokens, 800);
  assert.equal(summary.context, 25);
  assert.equal(contextPercent(sample(1, 1)), null, "unknown without the window");
  assert.equal(tokenUsageSummary([]).last, null);
  assert.equal(compactTokens(1234), "1.2K");
  assert.equal(compactTokens(45_678), "46K");
});

const metrics = await import("../src/lib/tokenUsage.ts");

test("stacked bars put output on input and scale to the largest prompt", () => {
  const bars = metrics.tokenBarsGeometry([sample(100, 0), sample(400, 10)], 226, 54);
  assert.equal(bars.length, 2);
  assert.ok(bars[0].x < bars[1].x, "spaced by order");
  assert.equal(Math.round(bars[1].outputY), 12, "the largest prompt reaches the top inset");
  assert.equal(bars[1].inputY, bars[1].outputY + bars[1].outputHeight, "output sits on input");
  assert.deepEqual(metrics.tokenBarsGeometry([]), []);
});

test("the context fill keeps only prompts that report it", () => {
  const fill = metrics.contextFillGeometry([sample(1, 1), sample(1, 2, { contextTokens: 50, contextWindow: 100 }), sample(1, 3, { contextTokens: 100, contextWindow: 100 })], 226, 36);
  assert.deepEqual(fill.dots.map((dot) => dot.percent), [50, 100]);
  assert.equal(fill.dots[1].y, 4, "full context reaches the top");
  assert.deepEqual(metrics.contextFillGeometry([sample(1, 1)]).dots, []);
});

test("metric tiles hide what no prompt reports", () => {
  const plain = metrics.tokenMetricStats([sample(1000, 1), sample(3000, 2)]);
  assert.equal(plain.averageTokens, 2000);
  assert.equal(plain.largest.index, 1);
  assert.equal(plain.cacheHitPercent, null);
  assert.equal(plain.averageDurationMs, null);
  assert.equal(plain.model, null);
  const rich = metrics.tokenMetricStats([
    sample(1000, 10_000, { cacheReadTokens: 400, promptAt: 4_000, model: "a" }),
    sample(1000, 30_000, { cacheReadTokens: 800, promptAt: 20_000, model: "b" }),
    sample(1000, 40_000),
  ]);
  assert.equal(rich.cacheHitPercent, 75, "cache over the input of the prompts that report it");
  assert.equal(rich.averageDurationMs, 8_000);
  assert.equal(rich.model, "b");
  assert.equal(metrics.compactDuration(72_000), "1m 12s");
  assert.equal(metrics.compactDuration(4_000), "4s");
});

test("a sample jumps to its prompt", () => {
  const prompts = [{ id: "p1", createdAt: 100 }, { id: "p2", createdAt: 1_000 }, { id: "p3", createdAt: 2_000 }, { id: "p4", createdAt: 2_500 }];
  const samples = [sample(1, 900), sample(1, 1_800), sample(1, 3_000)];
  assert.equal(metrics.resolvePromptJump(metrics.promptJumpForSample(samples, 1), prompts).id, "p2", "the last prompt between the previous turn and this one");
  assert.equal(metrics.resolvePromptJump(metrics.promptJumpForSample(samples, 2), prompts).id, "p4");
  assert.equal(metrics.resolvePromptJump({ sampleAt: 950, after: 900 }, prompts), null, "nothing fits");
  assert.equal(metrics.resolvePromptJump({ promptAt: 2_010, sampleAt: 9_000 }, prompts).id, "p3", "the prompt's own time wins");
  assert.equal(metrics.resolvePromptJump({ activityId: "p1" }, prompts).id, "p1");
  assert.equal(metrics.promptJumpNeedsOlder({ sampleAt: 3_000, after: 1_800 }, 2_000), true);
  assert.equal(metrics.promptJumpNeedsOlder({ sampleAt: 3_000, after: 1_800 }, 1_500), false);
});
