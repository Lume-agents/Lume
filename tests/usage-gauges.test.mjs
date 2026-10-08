import assert from "node:assert/strict";
import { expectedUsageLimitCount, rateWindowLabel, stacksUsageGauges, usageGaugeGroups } from "../src/lib/usageGauges.ts";

const reset = Date.UTC(2026, 9, 9, 17, 30);
// Same order and labels `agy -p /usage` produces: weekly before 5h in each group.
const antigravity = [
  { id: "antigravity:gemini-weekly", label: "Gemini · Semanal", usedPercent: 61, windowMinutes: 10080 },
  { id: "antigravity:gemini-5h", label: "Gemini · 5h", usedPercent: 28, windowMinutes: 300, resetsAt: reset },
  { id: "antigravity:3p-weekly", label: "3P Models · Semanal", usedPercent: 104, windowMinutes: 10080 },
  { id: "antigravity:3p-5h", label: "3P Models · 5h", usedPercent: 0, windowMinutes: 300 },
];

const groups = usageGaugeGroups(antigravity, "en");
assert.deepEqual(groups.map((group) => group.label), ["Gemini", "3P Models"]);
assert.deepEqual(groups.map((group) => group.gauges.map((gauge) => gauge.window)), [["5h", "7d"], ["5h", "7d"]]);
assert.deepEqual(groups.map((group) => group.gauges.map((gauge) => gauge.id)), [
  ["antigravity:gemini-5h", "antigravity:gemini-weekly"],
  ["antigravity:3p-5h", "antigravity:3p-weekly"],
]);
assert.deepEqual(groups.flatMap((group) => group.gauges.map((gauge) => gauge.remaining)), [72, 39, 100, 0]);

// The tooltip names the group, the window and, when known, the reset time, in the viewer's language.
const [geminiFiveHours, geminiWeekly] = groups[0].gauges;
const resetText = new Intl.DateTimeFormat("en", { dateStyle: "short", timeStyle: "short" }).format(new Date(reset));
assert.equal(geminiFiveHours.title, `Gemini · 5h: 72% remaining · resets ${resetText}`);
assert.equal(geminiWeekly.title, "Gemini · 7d: 39% remaining");
assert.match(usageGaugeGroups(antigravity, "pt-BR")[0].gauges[0].title, /^Gemini · 5h: 72% restante · reinicia /);

// Codex and Claude publish ungrouped labels and keep a single unlabelled row.
const claude = usageGaugeGroups([
  { id: "claude:five_hour", label: "5h", usedPercent: 10, windowMinutes: 300 },
  { id: "claude:seven_day", label: "7d", usedPercent: 40, windowMinutes: 10080 },
], "en");
assert.equal(claude.length, 1);
assert.equal(claude[0].label, "");
assert.deepEqual(claude[0].gauges.map((gauge) => gauge.window), ["5h", "7d"]);
assert.equal(claude[0].gauges[0].title, "5h: 90% remaining");
assert.deepEqual(usageGaugeGroups([], "en"), []);

// Windows without a duration stay after the known ones and fall back to the label.
const unknown = usageGaugeGroups([
  { id: "antigravity:monthly", label: "Gemini · monthly", usedPercent: 0 },
  { id: "antigravity:gemini-5h", label: "Gemini · 5h", usedPercent: 0, windowMinutes: 300 },
], "en");
assert.deepEqual(unknown[0].gauges.map((gauge) => gauge.window), ["5h", "monthly"]);

assert.equal(rateWindowLabel(45), "45m");
assert.equal(rateWindowLabel(undefined, "Weekly 7 d"), "7d");

assert.equal(expectedUsageLimitCount("antigravity"), 4);
assert.equal(expectedUsageLimitCount("codex"), 2);
assert.equal(expectedUsageLimitCount("claude_code"), 2);
assert.equal(expectedUsageLimitCount("gemini"), 0);

assert.equal(stacksUsageGauges("antigravity", 0), true);
assert.equal(stacksUsageGauges("codex", 0), false);
assert.equal(stacksUsageGauges("claude_code", 2), false);
// Should Claude ever publish a third window, the chart moves below instead of being squeezed.
assert.equal(stacksUsageGauges("claude_code", 3), true);

console.log("usage gauges ok");
