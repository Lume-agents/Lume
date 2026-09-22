export type UserMessagePresentation = {
  kind: "full" | "clamped" | "pasted";
  preview: string;
  characterCount: number;
};

const LONG_MESSAGE_THRESHOLD = 900;
const LONG_MESSAGE_LINE_THRESHOLD = 10;
const PREVIEW_LIMIT = 760;
const LARGE_PASTE_THRESHOLD = 6_000;
const MULTILINE_PASTE_THRESHOLD = 2_800;
const MULTILINE_PASTE_LINES = 8;

function previewAtWordBoundary(text: string) {
  const candidate = text.slice(0, PREVIEW_LIMIT);
  const boundary = Math.max(candidate.lastIndexOf("\n"), candidate.lastIndexOf(" "));
  return `${candidate.slice(0, boundary >= PREVIEW_LIMIT * .72 ? boundary : PREVIEW_LIMIT).trimEnd()}…`;
}

export function userMessagePresentation(text: string): UserMessagePresentation {
  const characterCount = text.length;
  const lineCount = text.split("\n").length;
  const looksLikeLargePaste = characterCount >= LARGE_PASTE_THRESHOLD
    || (characterCount >= MULTILINE_PASTE_THRESHOLD && lineCount >= MULTILINE_PASTE_LINES);

  if (looksLikeLargePaste) {
    return { kind: "pasted", preview: "", characterCount };
  }
  if (characterCount > LONG_MESSAGE_THRESHOLD || lineCount > LONG_MESSAGE_LINE_THRESHOLD) {
    return { kind: "clamped", preview: previewAtWordBoundary(text), characterCount };
  }
  return { kind: "full", preview: text, characterCount };
}
