import assert from "node:assert/strict";
import { expectedUsageLimitCount, rateWindowLabel, usageGaugeGroups } from "../src/lib/usageGauges.ts";

const reset = Date.UTC(2026, 9, 9, 17, 30);
const antigravity = [
  { id: "antigravity:g5", label: "Gemini · 5h", usedPercent: 28, windowMinutes: 300, resetsAt: reset },
  { id: "antigravity:gw", label: "Gemini · Semanal", usedPercent: 61, windowMinutes: 10080 },
  { id: "antigravity:p5", label: "3P Models · 5h", usedPercent: 0, windowMinutes: 300 },
  { id: "antigravity:pw", label: "3P Models · Semanal", usedPercent: 104, windowMinutes: 10080 },
];

const groups = usageGaugeGroups(antigravity, "en");
assert.deepEqual(groups.map((group) => group.label), ["Gemini", "3P Models"]);
assert.deepEqual(groups.map((group) => group.gauges.map((gauge) => gauge.window)), [["5h", "7d"], ["5h", "7d"]]);
assert.deepEqual(groups.flatMap((group) => group.gauges.map((gauge) => gauge.remaining)), [72, 39, 100, 0]);

// The tooltip names the group, the window and, when known, the reset time.
const [geminiFiveHours, geminiWeekly] = groups[0].gauges;
assert.match(geminiFiveHours.title, /^Gemini · 5h: 72% remaining · resets /);
assert.ok(geminiFiveHours.title.endsWith(new Intl.DateTimeFormat("en", { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" }).format(new Date(reset))));
assert.equal(geminiWeekly.title, "Gemini · Semanal: 39% remaining");
assert.match(usageGaugeGroups(antigravity, "pt-BR")[0].gauges[0].title, /^Gemini · 5h: 72% restante · reseta /);

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

assert.equal(rateWindowLabel(45), "45m");
assert.equal(rateWindowLabel(undefined, "Weekly 7 d"), "7d");
assert.equal(rateWindowLabel(undefined, "3P Models · Semanal"), "Semanal");

assert.equal(expectedUsageLimitCount("antigravity"), 4);
assert.equal(expectedUsageLimitCount("codex"), 2);
assert.equal(expectedUsageLimitCount("claude_code"), 2);
assert.equal(expectedUsageLimitCount("gemini"), 0);

console.log("usage gauges ok");
