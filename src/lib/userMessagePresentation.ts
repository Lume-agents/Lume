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

export type WorkflowPromptSummary = {
  /** `start` opens a step, `handoff` passes a result on, `skipped` continues without one. */
  kind: "start" | "handoff" | "skipped";
  role?: string;
  from?: string;
  to?: string;
  objective: string;
};

function section(text: string, heading: string) {
  const marker = `## ${heading}\n\n`;
  const start = text.indexOf(marker);
  if (start < 0) return "";
  const rest = text.slice(start + marker.length);
  const end = rest.indexOf("\n## ");
  return (end < 0 ? rest : rest.slice(0, end)).trim();
}

/** The prompts Lume's workflow sends to an agent read as a workflow step, not as something a person typed. */
export function parseWorkflowPrompt(text: string): WorkflowPromptSummary | null {
  const value = text.trimStart();
  if (!value.startsWith("# Lume workflow")) return null;
  const objective = section(value, "Objective");
  if (value.startsWith("# Lume workflow context")) {
    const transition = /Transition: \*\*(.+?)\*\* → \*\*(.+?)\*\*/.exec(value);
    return { kind: "handoff", from: transition?.[1], to: transition?.[2], objective };
  }
  const skipped = /The previous \*\*(.+?)\*\* step was skipped/.exec(value);
  if (skipped) return { kind: "skipped", role: skipped[1], objective };
  const role = /- Role: \*\*(.+?)\*\*/.exec(value);
  return { kind: "start", role: role?.[1], objective };
}
