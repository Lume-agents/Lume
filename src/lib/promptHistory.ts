import { cleanPromptTransport, promptTextKey } from "$lib/chatAttachments";

/** Sent prompts, newest first, browsed with the arrow keys like a CLI. */
export type PromptHistory = {
  entries: string[];
  /** -1 while editing the draft, otherwise the entry shown (0 = newest). */
  index: number;
  draft: string;
};

export function emptyPromptHistory(): PromptHistory {
  return { entries: [], index: -1, draft: "" };
}

/** Hooks and transcripts can record the same prompt twice in a row. */
export function historyEntries(prompts: Array<string | undefined>): string[] {
  const entries: string[] = [];
  for (const prompt of prompts) {
    const text = cleanPromptTransport(prompt);
    if (!text) continue;
    if (entries.length && promptTextKey(entries[entries.length - 1]) === promptTextKey(text)) continue;
    entries.push(text);
  }
  return entries;
}

/** Moves one entry older (+1) or newer (-1); null when there is nowhere to go. */
export function stepPromptHistory(
  history: PromptHistory,
  direction: 1 | -1,
  currentText: string,
): { history: PromptHistory; text: string } | null {
  const index = history.index + direction;
  if (index < -1 || index >= history.entries.length) return null;
  const draft = history.index === -1 ? currentText : history.draft;
  return {
    history: { ...history, index, draft },
    text: index === -1 ? draft : history.entries[index],
  };
}

/** The keys only browse history when the caret is on the edge line, as in a terminal. */
export function caretOnEdgeLine(text: string, caret: number, direction: 1 | -1): boolean {
  return direction === 1 ? !text.slice(0, caret).includes("\n") : !text.slice(caret).includes("\n");
}
