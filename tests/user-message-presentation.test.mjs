import assert from "node:assert/strict";
import { userMessagePresentation } from "../src/lib/userMessagePresentation.ts";

assert.deepEqual(userMessagePresentation("Short prompt"), {
  kind: "full",
  preview: "Short prompt",
  characterCount: 12,
});

const longMessage = "A long readable message ".repeat(50);
const longPresentation = userMessagePresentation(longMessage);
assert.equal(longPresentation.kind, "clamped");
assert.ok(longPresentation.preview.length < longMessage.length);
assert.ok(longPresentation.preview.endsWith("…"));

const multilinePaste = Array.from({ length: 80 }, (_, index) => `const line${index} = ${index};`).join("\n");
assert.equal(userMessagePresentation(multilinePaste.repeat(2)).kind, "pasted");
assert.equal(userMessagePresentation("x".repeat(6_000)).kind, "pasted");

console.log("user message presentation test suite passed");
