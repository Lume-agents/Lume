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
