import type { PromptAttachment, SessionActivity, SessionResult } from "$lib/domain";
// Node executes this module directly in the focused parser test; Vite resolves the same source in the app.
// @ts-expect-error TypeScript's bundler mode disallows the explicit source extension used by Node.
import { mergeFileChanges, summarizeFileChanges } from "./fileChanges.ts";

export interface ReviewFileChange {
  path: string;
  added: number;
  removed: number;
  diff?: string;
}

export interface ReviewTurn {
  id: string;
  prompt?: SessionActivity;
  result?: SessionResult;
  responseAttachments: PromptAttachment[];
  files: ReviewFileChange[];
  checks: string[];
  createdAt: number;
  activityAvailable: boolean;
}

export type ReviewDiffLineKind = "context" | "added" | "removed" | "hunk" | "meta";

export interface ReviewDiffLine {
  kind: ReviewDiffLineKind;
  content: string;
  oldLine?: number;
  newLine?: number;
}

function unifiedDiffChunks(detail: string) {
  const gitStarts = [...detail.matchAll(/^diff --git /gm)].map((match) => match.index ?? 0);
  const starts = gitStarts.length
    ? gitStarts
    : [...detail.matchAll(/^--- (?:a\/|\/dev\/null)/gm)].map((match) => match.index ?? 0);
  return starts.map((start, index) => detail.slice(start, starts[index + 1] ?? detail.length).trim());
}

function patchDiffChunks(detail: string) {
  const starts = [...detail.matchAll(/^\*\*\* (?:Update|Add|Delete) File:/gm)].map((match) => match.index ?? 0);
  return starts.map((start, index) => detail
    .slice(start, starts[index + 1] ?? detail.length)
    .replace(/^\*\*\* Begin Patch\s*/m, "")
    .replace(/\s*\*\*\* End Patch\s*$/m, "")
    .trim());
}

function recordedDiffs(detail: string) {
  const unified = unifiedDiffChunks(detail);
  if (unified.length) return unified;
  return patchDiffChunks(detail);
}

function structuredDiffChunks(detail: string) {
  let value: unknown;
  try {
    value = JSON.parse(detail);
  } catch {
    return [];
  }
  const chunks: string[] = [];
  const visit = (candidate: unknown): void => {
    if (Array.isArray(candidate)) {
      candidate.forEach(visit);
      return;
    }
    if (!candidate || typeof candidate !== "object") return;
    const record = candidate as Record<string, unknown>;
    const path = [record.path, record.filePath, record.filename]
      .find((item): item is string => typeof item === "string" && item.trim().length > 0);
    const diff = [record.unifiedDiff, record.diff, record.patch]
      .find((item): item is string => typeof item === "string" && item.trim().length > 0);
    if (path && diff) {
      const normalized = diff.trim();
      chunks.push(normalized.startsWith("diff --git ") || normalized.startsWith("*** ")
        ? normalized
        : `*** Update File: ${path}\n${normalized}`);
    }
    for (const [key, nested] of Object.entries(record)) {
      if (key === "diff" || key === "patch" || key === "unifiedDiff") continue;
      visit(nested);
    }
  };
  visit(value);
  return chunks;
}

export function collectReviewFiles(
  activities: SessionActivity[],
  workingDirectory?: string,
  since = 0,
): ReviewFileChange[] {
  const files: ReviewFileChange[] = [];
  for (const activity of activities) {
    if (activity.createdAt < since) continue;
    const reported = [...activity.files];
    if (activity.kind === "file" && !reported.includes(activity.title)) reported.push(activity.title);
    mergeFileChanges(files, summarizeFileChanges(activity.detail ?? "", reported, workingDirectory), workingDirectory);
    const detail = activity.detail ?? "";
    for (const diff of [...recordedDiffs(detail), ...structuredDiffChunks(detail)]) {
      const [summary] = summarizeFileChanges(diff, [], workingDirectory);
      if (!summary) continue;
      const current = files.find((file) => file.path === summary.path);
      if (current) current.diff = diff;
      else files.push({ ...summary, diff });
    }
  }
  return files.sort((left, right) => left.path.localeCompare(right.path));
}

export function buildReviewTurns(
  activities: SessionActivity[],
  results: SessionResult[],
  workingDirectory?: string,
): ReviewTurn[] {
  const prompts = activities
    .filter((activity) => activity.kind === "prompt")
    .sort((left, right) => left.createdAt - right.createdAt)
    .filter((prompt, index, sorted) => {
      const previous = sorted[index - 1];
      return !previous || prompt.detail?.trim() !== previous.detail?.trim()
        || prompt.createdAt - previous.createdAt >= 2_000;
    });
  const orderedResults = [...results].sort((left, right) => left.createdAt - right.createdAt);
  const matchedResults = new Set<string>();
  const turns: ReviewTurn[] = [];

  for (const [index, prompt] of prompts.entries()) {
    const nextPromptAt = prompts[index + 1]?.createdAt ?? Number.POSITIVE_INFINITY;
    const segment = activities.filter((activity) =>
      activity.createdAt >= prompt.createdAt && activity.createdAt < nextPromptAt
    );
    const segmentResults = orderedResults.filter((result) =>
      result.createdAt >= prompt.createdAt && result.createdAt < nextPromptAt
    );
    for (const result of segmentResults) matchedResults.add(result.id);
    const result = segmentResults.at(-1);
    const responseActivity = result
      ? [...segment].reverse().find((activity) =>
        activity.kind === "message" && activity.detail?.trim() === result.response.trim()
      )
      : undefined;
    const files = collectReviewFiles(segment, workingDirectory);
    const checks = [...new Set([
      ...(result?.tests ?? []),
      ...segment.filter((activity) => activity.kind === "test")
        .map((activity) => activity.detail?.trim() || activity.title.trim()),
    ].filter(Boolean))];
    if (!result && index < prompts.length - 1 && !files.length && !checks.length) continue;
    turns.push({
      id: result?.id ?? `prompt:${prompt.id}`,
      prompt,
      result,
      responseAttachments: responseActivity?.attachments ?? [],
      files,
      checks,
      createdAt: result?.createdAt ?? prompt.createdAt,
      activityAvailable: true,
    });
  }

  for (const result of orderedResults) {
    if (matchedResults.has(result.id)) continue;
    turns.push({
      id: result.id,
      result,
      responseAttachments: [],
      files: [],
      checks: [...new Set(result.tests.filter(Boolean))],
      createdAt: result.createdAt,
      activityAvailable: false,
    });
  }

  return turns.sort((left, right) => right.createdAt - left.createdAt);
}

/** Builds only the newest turn without partitioning the full conversation history. */
export function buildLatestReviewTurn(
  activities: SessionActivity[],
  results: SessionResult[],
  workingDirectory?: string,
): ReviewTurn | null {
  const prompts = activities
    .filter((activity) => activity.kind === "prompt")
    .sort((left, right) => left.createdAt - right.createdAt)
    .filter((prompt, index, sorted) => {
      const previous = sorted[index - 1];
      return !previous || prompt.detail?.trim() !== previous.detail?.trim()
        || prompt.createdAt - previous.createdAt >= 2_000;
    });
  const prompt = prompts.at(-1);

  if (!prompt) {
    const latestResultAt = results.reduce((latest, item) => Math.max(latest, item.createdAt), Number.NEGATIVE_INFINITY);
    const result = results.find((item) => item.createdAt === latestResultAt);
    return result ? {
      id: result.id,
      result,
      responseAttachments: [],
      files: [],
      checks: [...new Set(result.tests.filter(Boolean))],
      createdAt: result.createdAt,
      activityAvailable: false,
    } : null;
  }

  const segment = activities.filter((activity) => activity.createdAt >= prompt.createdAt);
  const result = results
    .filter((item) => item.createdAt >= prompt.createdAt)
    .sort((left, right) => left.createdAt - right.createdAt)
    .at(-1);
  const responseActivity = result
    ? [...segment].reverse().find((activity) =>
      activity.kind === "message" && activity.detail?.trim() === result.response.trim()
    )
    : undefined;
  const files = collectReviewFiles(segment, workingDirectory);
  const checks = [...new Set([
    ...(result?.tests ?? []),
    ...segment.filter((activity) => activity.kind === "test")
      .map((activity) => activity.detail?.trim() || activity.title.trim()),
  ].filter(Boolean))];

  return {
    id: result?.id ?? `prompt:${prompt.id}`,
    prompt,
    result,
    responseAttachments: responseActivity?.attachments ?? [],
    files,
    checks,
    createdAt: result?.createdAt ?? prompt.createdAt,
    activityAvailable: true,
  };
}

export function parseReviewDiff(diff: string): ReviewDiffLine[] {
  const result: ReviewDiffLine[] = [];
  let oldLine: number | undefined;
  let newLine: number | undefined;
  for (const line of diff.split(/\r?\n/)) {
    const hunk = line.match(/^@@\s+-(\d+)(?:,\d+)?\s+\+(\d+)(?:,\d+)?\s+@@/);
    if (hunk) {
      oldLine = Number(hunk[1]);
      newLine = Number(hunk[2]);
      result.push({ kind: "hunk", content: line });
      continue;
    }
    if (line.startsWith("diff --git ") || line.startsWith("index ") || line.startsWith("--- ") || line.startsWith("+++ ") || line.startsWith("*** ")) {
      result.push({ kind: "meta", content: line });
      continue;
    }
    if (line.startsWith("+") && !line.startsWith("+++")) {
      result.push({ kind: "added", content: line.slice(1), newLine });
      if (newLine !== undefined) newLine += 1;
      continue;
    }
    if (line.startsWith("-") && !line.startsWith("---")) {
      result.push({ kind: "removed", content: line.slice(1), oldLine });
      if (oldLine !== undefined) oldLine += 1;
      continue;
    }
    const content = line.startsWith(" ") ? line.slice(1) : line;
    result.push({ kind: "context", content, oldLine, newLine });
    if (oldLine !== undefined) oldLine += 1;
    if (newLine !== undefined) newLine += 1;
  }
  return result;
}
