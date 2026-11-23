/** One answer the person gave to an agent's question, as the chat shows it. */
export type AnsweredQuestion = { header: string; question: string; answer: string };

/** The id Lume gives the prompt that records the answers (state.rs, `question-answer:`). */
export const isQuestionAnswerId = (id: string) => id.startsWith("question-answer:");

/**
 * Reads `[header] question` / `→ answer` blocks back. Answers recorded before the questions were kept
 * (`Header: answer`, or the answer alone) still read, without the question.
 */
export function parseQuestionAnswers(detail: string): AnsweredQuestion[] {
  const text = detail.replace(/\r\n?/g, "\n").trim();
  if (!text) return [];
  if (/^→ /m.test(text)) {
    return text.split(/\n{2,}(?=\[)/).flatMap((block) => {
      const [first, ...rest] = block.split("\n");
      const answerLine = rest.findIndex((line) => line.startsWith("→ "));
      if (answerLine < 0) return [];
      const head = /^\[([^\]]*)\]\s*(.*)$/.exec(first);
      const answer = [rest[answerLine].slice(2), ...rest.slice(answerLine + 1)].join("\n").trim();
      return [{ header: head?.[1] ?? "", question: (head ? head[2] : first).trim(), answer }];
    });
  }
  return text.split("\n").filter(Boolean).map((line) => {
    const pair = /^([^:\n]{1,40}):\s+(.+)$/.exec(line);
    return pair ? { header: pair[1], question: "", answer: pair[2] } : { header: "", question: "", answer: line };
  });
}
