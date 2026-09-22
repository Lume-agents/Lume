import type { AgentSession, SessionActivity } from "$lib/domain";
import type { FileChangeSummary } from "$lib/fileChanges";
import { mergeFileChanges, summarizeFileChanges } from "$lib/fileChanges";
import { cleanPromptTransport, promptTextKey } from "$lib/chatAttachments";
import { isHiddenAgentActivity, isPresentableTraceActivity } from "$lib/activityPresentation";
import { latestResponseText, sameResponseText } from "$lib/responseDedup.js";

export type ConversationEntry = {
  id: string;
  activity: SessionActivity;
  files: FileChangeSummary[];
  sequence: number;
  durationMs?: number;
  isFinalResponse?: boolean;
};

export type ConversationFeedItem =
  | { kind: "entry"; id: string; entry: ConversationEntry }
  | { kind: "trace"; id: string; entries: ConversationEntry[]; files: FileChangeSummary[] };

function textKey(value?: string): string {
  return (value ?? "")
    .replace(/\r\n?/g, "\n")
    .replace(/[ \t]+$/gm, "")
    .trim();
}

function fileChangesKey(files: FileChangeSummary[]): string {
  return files
    .map((file) => `${file.path}\u0000${file.added}\u0000${file.removed}`)
    .sort()
    .join("\u0001");
}

function mergeAttachments(target: SessionActivity, source: SessionActivity) {
  const attachments = [...(target.attachments ?? [])];
  for (const attachment of source.attachments ?? []) {
    const duplicate = attachments.some((existing) =>
      existing.path && attachment.path
        ? existing.path.replace(/\\/g, "/").toLowerCase() === attachment.path.replace(/\\/g, "/").toLowerCase()
        : existing.name === attachment.name
    );
    if (!duplicate) attachments.push(attachment);
  }
  if (attachments.length) target.attachments = attachments;
}

export function buildConversationEntries(
  session: AgentSession | null,
  activities: SessionActivity[],
  activityChanges: (activity: SessionActivity) => FileChangeSummary[],
): ConversationEntry[] {
  let sequence = 0;
  const sortedActivities = [...activities].sort((left, right) => left.createdAt - right.createdAt);
  const uniquePrompts: SessionActivity[] = [];
  let messageSinceLastPrompt = false;
  for (const activity of sortedActivities) {
    if (activity.kind === "message") messageSinceLastPrompt = true;
    if (activity.kind !== "prompt") continue;
    const existing = uniquePrompts.at(-1);
    const duplicate = existing
      && !messageSinceLastPrompt
      && !(existing.id.startsWith("local:") && activity.id.startsWith("local:"))
      && promptTextKey(existing.detail) === promptTextKey(activity.detail)
      && Math.abs(existing.createdAt - activity.createdAt) < 60_000;
    if (!duplicate) uniquePrompts.push(activity);
    messageSinceLastPrompt = false;
  }
  const promptTimes = uniquePrompts.map((activity) => activity.createdAt);
  const promptSegment = (createdAt: number) => {
    let low = 0;
    let high = promptTimes.length - 1;
    let segment = Number.NEGATIVE_INFINITY;
    while (low <= high) {
      const middle = (low + high) >> 1;
      const promptTime = promptTimes[middle];
      if (promptTime <= createdAt) {
        segment = promptTime;
        low = middle + 1;
      } else {
        high = middle - 1;
      }
    }
    return segment;
  };
  const durationFromPrompt = (completedAt: number) => {
    const startedAt = promptSegment(completedAt);
    return Number.isFinite(startedAt) && completedAt >= startedAt
      ? completedAt - startedAt
      : undefined;
  };

  const entries: ConversationEntry[] = [];
  for (const activity of activities) {
    if (["queued_prompt", "plan", "plan_document"].includes(activity.kind)) continue;
    if (activity.kind === "prompt") {
      let duplicateIndex = -1;
      for (let index = entries.length - 1; index >= 0; index -= 1) {
        const existing = entries[index].activity;
        if (
          existing.kind === "prompt"
          && !(existing.id.startsWith("local:") && activity.id.startsWith("local:"))
          && promptTextKey(existing.detail) === promptTextKey(activity.detail)
          && Math.abs(existing.createdAt - activity.createdAt) < 60_000
        ) {
          duplicateIndex = index;
          break;
        }
      }
      const duplicatePrompt = duplicateIndex >= 0
        && !entries.slice(duplicateIndex + 1).some((entry) =>
          entry.activity.kind === "prompt" || entry.activity.kind === "message"
        )
        ? entries[duplicateIndex]
        : undefined;
      if (duplicatePrompt) {
        mergeAttachments(duplicatePrompt.activity, activity);
        continue;
      }
    }

    const files = activityChanges(activity);
    const matchingMessage = activity.kind === "message"
      ? entries.findLast((entry) =>
          entry.activity.kind === "message"
          && sameResponseText(entry.activity.detail, activity.detail)
          && promptSegment(entry.activity.createdAt) === promptSegment(activity.createdAt)
        )
      : undefined;
    if (matchingMessage) {
      const previousCreatedAt = matchingMessage.activity.createdAt;
      matchingMessage.activity.detail = latestResponseText(
        matchingMessage.activity.detail,
        activity.detail,
        previousCreatedAt,
        activity.createdAt,
      );
      if (activity.createdAt >= previousCreatedAt) {
        matchingMessage.activity = {
          ...matchingMessage.activity,
          ...activity,
          detail: matchingMessage.activity.detail,
        };
      }
      mergeFileChanges(matchingMessage.files, files);
      continue;
    }

    if (activity.kind === "file" && files.length) {
      const signature = fileChangesKey(files);
      const duplicateFileEntry = entries.findLast((entry) =>
        entry.activity.kind === "file"
        && promptSegment(entry.activity.createdAt) === promptSegment(activity.createdAt)
        && fileChangesKey(entry.files) === signature
      );
      if (duplicateFileEntry) {
        mergeFileChanges(duplicateFileEntry.files, files);
        continue;
      }
    }

    entries.push({
      id: `activity:${activity.id}`,
      activity: {
        ...activity,
        detail: activity.kind === "prompt"
          ? cleanPromptTransport(activity.detail) || undefined
          : activity.detail,
      },
      files,
      sequence: sequence++,
    });
  }

  for (const result of session?.results ?? []) {
    const resultFiles = summarizeFileChanges(result.response, result.files, session?.workingDirectory);
    const responseKey = textKey(result.response);
    const matchingMessage = entries.findLast((entry) =>
      entry.activity.kind === "message"
      && sameResponseText(entry.activity.detail, responseKey)
      && promptSegment(entry.activity.createdAt) === promptSegment(result.createdAt)
    );
    if (matchingMessage) {
      matchingMessage.activity.detail = latestResponseText(
        matchingMessage.activity.detail,
        result.response,
        matchingMessage.activity.createdAt,
        result.createdAt,
      );
      if (result.createdAt >= matchingMessage.activity.createdAt) {
        matchingMessage.activity.createdAt = result.createdAt;
        matchingMessage.activity.status = "completed";
      }
      mergeFileChanges(matchingMessage.files, resultFiles);
      matchingMessage.durationMs = durationFromPrompt(result.createdAt);
      matchingMessage.isFinalResponse = true;
      matchingMessage.activity.files = Array.from(new Set([
        ...matchingMessage.activity.files,
        ...result.files,
      ]));
    } else if (result.response || resultFiles.length) {
      entries.push({
        id: `result:${result.id}`,
        activity: {
          id: `response:${result.id}`,
          kind: result.response ? "message" : "file",
          title: result.response ? "Resposta do agente" : "Arquivos alterados",
          detail: result.response || undefined,
          status: "completed",
          createdAt: result.createdAt,
          files: result.files,
        },
        files: resultFiles,
        sequence: sequence++,
        durationMs: durationFromPrompt(result.createdAt),
        isFinalResponse: true,
      });
    }
  }

  if (session?.lastResponse) {
    const responseKey = textKey(session.lastResponse);
    const matchingMessage = entries.find((entry) =>
      entry.activity.kind === "message" && sameResponseText(entry.activity.detail, responseKey)
    );
    if (matchingMessage) {
      matchingMessage.activity.detail = latestResponseText(
        matchingMessage.activity.detail,
        session.lastResponse,
        matchingMessage.activity.createdAt,
        session.updatedAt,
      );
      if (session.updatedAt >= matchingMessage.activity.createdAt) {
        matchingMessage.activity.createdAt = session.updatedAt;
        matchingMessage.activity.status = "completed";
      }
      if (matchingMessage.durationMs === undefined && ["completed", "failed"].includes(session.status)) {
        matchingMessage.durationMs = durationFromPrompt(session.updatedAt);
      }
      matchingMessage.isFinalResponse = true;
    } else {
      entries.push({
        id: `last-response:${session.id}:${session.updatedAt}`,
        activity: {
          id: `response:${session.id}:${session.updatedAt}`,
          kind: "message",
          title: "Resposta do agente",
          detail: session.lastResponse,
          status: "completed",
          createdAt: session.updatedAt,
          files: [],
        },
        files: [],
        sequence: sequence++,
        durationMs: ["completed", "failed"].includes(session.status)
          ? durationFromPrompt(session.updatedAt)
          : undefined,
        isFinalResponse: true,
      });
    }
  }

  const sortedEntries = entries.sort((left, right) =>
    left.activity.createdAt - right.activity.createdAt || left.sequence - right.sequence
  );
  let observedTurnStart: number | undefined;
  for (const entry of sortedEntries) {
    if (entry.activity.kind === "prompt") {
      observedTurnStart = entry.activity.createdAt;
    } else if (observedTurnStart === undefined && !entry.isFinalResponse) {
      observedTurnStart = entry.activity.createdAt;
    }
    if (!entry.isFinalResponse) continue;
    if (entry.durationMs === undefined && observedTurnStart !== undefined && entry.activity.createdAt >= observedTurnStart) {
      entry.durationMs = entry.activity.createdAt - observedTurnStart;
    }
    observedTurnStart = undefined;
  }
  return sortedEntries;
}

export function buildConversationFeed(
  entries: ConversationEntry[],
  options: { includeAnalysisInTrace?: boolean } = {},
): ConversationFeedItem[] {
  const feed: ConversationFeedItem[] = [];
  let trace: Extract<ConversationFeedItem, { kind: "trace" }> | null = null;
  for (const entry of entries) {
    if (isHiddenAgentActivity(entry.activity)) continue;
    if (
      isPresentableTraceActivity(entry.activity)
      || (options.includeAnalysisInTrace && entry.activity.kind === "analysis")
    ) {
      const previous = trace?.entries[trace.entries.length - 1];
      if (!trace || (previous && entry.activity.createdAt - previous.activity.createdAt > 180_000)) {
        trace = { kind: "trace", id: `trace:${entry.id}`, entries: [], files: [] };
        feed.push(trace);
      }
      trace.entries.push(entry);
      mergeFileChanges(trace.files, entry.files);
      continue;
    }
    trace = null;
    feed.push({ kind: "entry", id: entry.id, entry });
  }
  return feed;
}

export function fileChangesForFinalResponses(feed: ConversationFeedItem[]): Map<string, FileChangeSummary[]> {
  const byResponse = new Map<string, FileChangeSummary[]>();
  let turnFiles: FileChangeSummary[] = [];
  for (const item of feed) {
    if (item.kind === "trace") {
      mergeFileChanges(turnFiles, item.files);
      continue;
    }
    if (item.entry.activity.kind === "prompt") {
      turnFiles = [];
      continue;
    }
    mergeFileChanges(turnFiles, item.entry.files);
    if (item.entry.isFinalResponse) {
      byResponse.set(item.entry.id, turnFiles);
      turnFiles = [];
    }
  }
  return byResponse;
}
