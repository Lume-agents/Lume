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

export type AgentReportSummary = {
  /** Id of the subagent that sent the report. */
  from: string;
  /** First line of the report, shown while it is collapsed. */
  headline: string;
  /** The report without the harness framing and its indentation. */
  body: string;
};

const AGENT_REPORT = /^<agent-message from="([^"\n]{1,80})">\n([\s\S]*)\n<\/agent-message>$/;
const AGENT_REPORT_LABEL = /^\[Subagent hand-back\]\s*/;

/**
 * A subagent's hand-back is recorded with the user role, but it is model output and carries no user
 * authority. Only a message that is wholly that frame reads as a report, so a person who types the
 * tag inside a longer message still sees their own text.
 */
export function parseAgentReport(text: string): AgentReportSummary | null {
  const match = AGENT_REPORT.exec(text.trim());
  if (!match) return null;
  const body = match[2].split("\n").map((line) => line.replace(/^ {2}/, "")).join("\n").replace(AGENT_REPORT_LABEL, "").trim();
  if (!body) return null;
  const headline = body.split("\n").find((line) => line.trim())!.trim();
  return { from: match[1], headline, body };
}
