import assert from "node:assert/strict";
import { parseQuestionAnswers, isQuestionAnswerId } from "../src/lib/questionAnswers.ts";

assert.deepEqual(parseQuestionAnswers("[Gatilho] Como marcar?\n→ Os dois\n\n[Goal] O que fazer?\n→ Lume registra o goal"), [
  { header: "Gatilho", question: "Como marcar?", answer: "Os dois" },
  { header: "Goal", question: "O que fazer?", answer: "Lume registra o goal" },
]);
// Answers recorded before the questions were kept.
assert.deepEqual(parseQuestionAnswers("Gatilho: Os dois\nGoal: Lume"), [
  { header: "Gatilho", question: "", answer: "Os dois" },
  { header: "Goal", question: "", answer: "Lume" },
]);
assert.deepEqual(parseQuestionAnswers("Sim"), [{ header: "", question: "", answer: "Sim" }]);
assert.equal(isQuestionAnswerId("question-answer:q1:123"), true);
assert.equal(isQuestionAnswerId("prompt:1"), false);
